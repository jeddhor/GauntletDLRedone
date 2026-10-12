//! Static original-data inspection, with explicit diagnostic material choices.
use bevy::{
    asset::RenderAssetUsages,
    core_pipeline::tonemapping::Tonemapping,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    mesh::Indices,
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
    sprite::Anchor,
};
use gdl_formats::{animation::Animations, objects::Objects, texdef::TextureTable, world::World};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, error::Error, fs, path::Path};

#[derive(Resource)]
struct SceneSource {
    objects: Objects,
    placements: Vec<Placement>,
    min: Vec3,
    max: Vec3,
    label: String,
    detail: Vec3,
    detail_distance: f32,
    table: TextureTable,
    textures: Vec<u8>,
    hashes: Vec<String>,
}
struct Placement {
    index: u32,
    label: String,
    position: Vec3,
}
#[derive(Resource)]
struct InspectionCamera {
    target: Vec3,
    overview: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
    overview_distance: f32,
    detail: Vec3,
    detail_distance: f32,
}
#[derive(Component)]
struct OrbitCamera;

fn mesh_for(part: &gdl_formats::objects::Part) -> Result<Mesh, Box<dyn Error>> {
    let geometry = part.decode_integer_geometry()?;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    for strip in geometry.strips {
        let start = positions.len() as u32;
        indices.extend(strip.indices.iter().map(|i| i + start));
        if strip.colors.is_empty() {
            // White is a diagnostic multiplier for an absent source channel.
            colors.extend(vec![[1.0; 4]; strip.positions.len()]);
        } else {
            colors.extend(strip.colors);
        }
        positions.extend(strip.positions);
        normals.extend(strip.normals);
        uvs.extend(strip.uvs);
    }
    Ok(Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices)))
}

fn setup(
    mut commands: Commands,
    source: Res<SceneSource>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut material_cache = HashMap::new();
    let mut unresolved_materials = Vec::new();
    for bitmap in &source.objects.bitmap_records {
        let candidates: Vec<_> = source
            .table
            .bitmaps
            .iter()
            .filter(|b| {
                b.texture_offset == bitmap.texture_offset
                    && b.format == bitmap.format
                    && b.width == bitmap.width
                    && b.height == bitmap.height
            })
            .collect();
        let decoded = if candidates.len() == 1 && bitmap.flags & 0x20 == 0 {
            candidates[0].decode_base_rgba(&source.textures).ok()
        } else {
            None
        };
        let texture = decoded.map(|rgba| {
            let mut image = Image::new(
                Extent3d {
                    width: bitmap.width as u32,
                    height: bitmap.height as u32,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                rgba,
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            );
            let mut sampler = ImageSamplerDescriptor::nearest();
            sampler.set_address_mode(ImageAddressMode::Repeat);
            image.sampler = ImageSampler::Descriptor(sampler);
            images.add(image)
        });
        if texture.is_none() {
            unresolved_materials.push(bitmap.index);
        }
        material_cache.insert(
            bitmap.index as u16,
            materials.add(StandardMaterial {
                base_color: if texture.is_some() {
                    Color::WHITE
                } else {
                    Color::srgb(1.0, 0.0, 0.8)
                },
                base_color_texture: texture,
                // Deliberately diagnostic: original alpha, lightmaps and culling
                // are retained in the format report but not accepted here.
                unlit: true,
                cull_mode: None,
                alpha_mode: AlphaMode::Opaque,
                ..default()
            }),
        );
    }
    let unknown_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.0, 0.8),
        unlit: true,
        cull_mode: None,
        ..default()
    });
    let mut cache = HashMap::new();
    let mut unsupported = Vec::new();
    let mut triangles = 0usize;
    for object in &source.objects.objects {
        let mut parts = Vec::new();
        for part in &object.parts {
            match mesh_for(part) {
                Ok(mesh) => {
                    triangles += mesh.indices().map_or(0, |i| i.len() / 3);
                    parts.push((
                        meshes.add(mesh),
                        material_cache
                            .get(&part.texture_index)
                            .cloned()
                            .unwrap_or_else(|| unknown_material.clone()),
                    ));
                }
                Err(error) => unsupported.push(format!("{}/{}: {error}", object.index, part.index)),
            }
        }
        cache.insert(object.index, parts);
    }
    let names: HashMap<_, _> = source
        .objects
        .definitions
        .iter()
        .map(|d| (d.label.as_str(), d.object_index as u32))
        .collect();
    let mut placed = 0;
    let mut unmatched = Vec::new();
    for node in &source.placements {
        if let Some(parts) = names
            .get(node.label.as_str())
            .and_then(|index| cache.get(index))
        {
            for (mesh, material) in parts {
                commands.spawn((
                    Name::new(format!("world:{}:{}", node.index, node.label)),
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(node.position),
                ));
                placed += 1;
            }
        } else {
            unmatched.push((node.index, node.label.clone()));
        }
    }
    println!(
        "{} candidate triangles in {} object packets; {placed} placed mesh parts",
        triangles,
        source
            .objects
            .objects
            .iter()
            .map(|o| o.parts.len())
            .sum::<usize>()
    );
    println!("Unsupported geometry: {unsupported:?}");
    println!(
        "Undecoded/external/ambiguous bitmap indices (magenta diagnostic): {unresolved_materials:?}"
    );
    println!("Nodes without a model definition (groups/unknown semantics): {unmatched:?}");
    let center = (source.min + source.max) * 0.5;
    let extent = source.max - source.min;
    commands.insert_resource(InspectionCamera {
        target: center,
        overview: center,
        yaw: 0.0,
        pitch: 0.8,
        // Fit the source bounding sphere inside the default 45-degree view.
        distance: extent.length() * 1.4,
        overview_distance: extent.length() * 1.4,
        detail: source.detail,
        detail_distance: source.detail_distance,
    });
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        Transform::from_translation(center + Vec3::new(0.0, 300.0, 300.0))
            .looking_at(center, Vec3::Y),
        OrbitCamera,
    ));
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
    ));
    commands.spawn((Text2d::new(format!(
        "{} | STATIC DATA INSPECTOR | gameplay not implemented\nArrow keys: orbit   WASD: pan   Q/E: distance   P: detail   R: overview   Esc: close\nOpaque base textures + source vertex colors; double sided; lightmaps/GS blending unverified\n{placed} placed parts | {} unresolved bitmap records shown magenta\nOBJECTS {}  placement source {}",
        source.label,unresolved_materials.len(),&source.hashes[0][..12],&source.hashes[1][..12])),
        TextFont {font_size:15.0,..default()},TextColor(Color::WHITE),Anchor::TOP_LEFT,
        Transform::from_xyz(-580.0,380.0,0.0)));
}

fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut view: ResMut<InspectionCamera>,
    mut camera: Query<&mut Transform, With<OrbitCamera>>,
    mut exit: MessageWriter<AppExit>,
) {
    let dt = time.delta_secs().min(0.1);
    if keys.pressed(KeyCode::ArrowLeft) {
        view.yaw += dt;
    }
    if keys.pressed(KeyCode::ArrowRight) {
        view.yaw -= dt;
    }
    if keys.pressed(KeyCode::ArrowUp) {
        view.pitch = (view.pitch + dt).min(1.5);
    }
    if keys.pressed(KeyCode::ArrowDown) {
        view.pitch = (view.pitch - dt).max(0.05);
    }
    if keys.pressed(KeyCode::KeyQ) {
        view.distance = (view.distance * (1.0 - dt)).max(5.0);
    }
    if keys.pressed(KeyCode::KeyE) {
        view.distance = (view.distance * (1.0 + dt)).min(2000.0);
    }
    let speed = view.distance * 0.5 * dt;
    if keys.pressed(KeyCode::KeyW) {
        view.target.z -= speed;
    }
    if keys.pressed(KeyCode::KeyS) {
        view.target.z += speed;
    }
    if keys.pressed(KeyCode::KeyA) {
        view.target.x -= speed;
    }
    if keys.pressed(KeyCode::KeyD) {
        view.target.x += speed;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        // Recorded fixture, explicitly an inspection view, not a spawn rule.
        view.target = view.detail;
        view.distance = view.detail_distance;
        view.yaw = 0.0;
        view.pitch = 0.9;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        view.target = view.overview;
        view.distance = view.overview_distance;
        view.yaw = 0.0;
        view.pitch = 0.8;
    }
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
    let offset = Vec3::new(
        view.yaw.sin() * view.pitch.cos(),
        view.pitch.sin(),
        view.yaw.cos() * view.pitch.cos(),
    ) * view.distance;
    if let Ok(mut transform) = camera.single_mut() {
        *transform =
            Transform::from_translation(view.target + offset).looking_at(view.target, Vec3::Y);
    }
}

fn bounded_read(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    use std::io::Read;
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > 64 * 1024 * 1024 {
        return Err("scene input exceeds policy".into());
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("scene input grew beyond policy".into());
    }
    Ok(bytes)
}
fn run() -> Result<(), Box<dyn Error>> {
    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let rig = args.first().is_some_and(|a| a == "--rig");
    if rig {
        args.remove(0);
    }
    if args.len() != 4 {
        return Err("Usage: gdl_scene OBJECTS.PS2 WORLDS.PS2 TEXDEF.PS2 TEXTURES.PS2\n       gdl_scene --rig OBJECTS.PS2 ANIM.PS2 TEXDEF.PS2 TEXTURES.PS2".into());
    }
    let files: Vec<_> = args
        .iter()
        .map(|p| bounded_read(Path::new(p)))
        .collect::<Result<_, _>>()?;
    let objects = Objects::parse(&files[0])?;
    let (placements, min, max, label, detail, detail_distance) = if rig {
        let animations = Animations::parse(&files[1])?;
        if animations.trees.len() != 1 {
            return Err("rig inspector requires one explicit tree".into());
        }
        let tree = &animations.trees[0];
        let translations = tree.initial_translations()?;
        let placements: Vec<_> = tree
            .nodes
            .iter()
            .zip(translations)
            .map(|(node, position)| Placement {
                index: node.index,
                label: (tree.prefix.clone() + &node.label)
                    .chars()
                    .take(16)
                    .collect(),
                position: Vec3::from_array(position),
            })
            .collect();
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for placement in &placements {
            for def in objects
                .definitions
                .iter()
                .filter(|d| d.label == placement.label)
            {
                let object = &objects.objects[def.object_index as usize];
                for part in &object.parts {
                    for strip in part.decode_integer_geometry()?.strips {
                        for vertex in strip.positions {
                            let point = Vec3::from_array(vertex) + placement.position;
                            min = min.min(point);
                            max = max.max(point);
                        }
                    }
                }
            }
        }
        if !min.is_finite() || !max.is_finite() {
            return Err("no matching rig geometry".into());
        }
        let center = (min + max) * 0.5;
        let distance = (max - min).length() * 1.4;
        (
            placements,
            min,
            max,
            format!(
                "{} | translation-only rest hierarchy; animated pose unverified",
                tree.label
            ),
            center,
            distance,
        )
    } else {
        let world = World::parse(&files[1])?;
        let placements = world
            .placements()?
            .iter()
            .map(|p| {
                let node = &world.nodes[p.node_index as usize];
                Placement {
                    index: node.index,
                    label: node.label.clone(),
                    position: Vec3::from_array(p.summed_position),
                }
            })
            .collect();
        (
            placements,
            Vec3::from_array(world.min),
            Vec3::from_array(world.max),
            "The Poison Fields".into(),
            Vec3::new(24.056019, 0.0, 5.14752),
            70.0,
        )
    };
    let source = SceneSource {
        objects,
        placements,
        min,
        max,
        label,
        detail,
        detail_distance,
        table: TextureTable::parse(&files[2])?,
        textures: files[3].clone(),
        hashes: files
            .iter()
            .map(|b| format!("{:x}", Sha256::digest(b)))
            .collect(),
    };
    App::new()
        .insert_resource(source)
        .insert_resource(ClearColor(Color::srgb(0.035, 0.04, 0.05)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gauntlet PS2 native static scene inspector".into(),
                resolution: (1200, 800).into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(Update, controls)
        .run();
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
