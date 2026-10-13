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
    rig: Option<RigSource>,
}
struct Placement {
    index: u32,
    label: String,
    transform: Mat4,
}
struct RigSource {
    animations: Animations,
    bytes: Vec<u8>,
}
#[derive(Resource, Default)]
struct RigPlayback {
    sequence: usize,
    frame: f32,
    playing: bool,
}
#[derive(Component)]
struct RigNode(u32);
#[derive(Component)]
struct AnimationHud;

fn rig_pose(source: &RigSource, sequence: usize, frame: f32) -> Result<Vec<Mat4>, Box<dyn Error>> {
    let tree = &source.animations.trees[0];
    let mut locals = Vec::new();
    for node in &tree.nodes {
        if node.node_type != 1 {
            return Err("pose inspector supports skeletal nodes only".into());
        }
        let track = tree.key_track(&source.bytes, node.index, sequence as u32)?;
        let channels = gdl_core::animation::sample_channels(&track, frame)?;
        let (matrix, scale) = gdl_core::transform::local_transform(
            channels,
            node.initial_position,
            track.transform_flags,
        )?;
        locals.push(Mat4::from_cols_array(&matrix) * Mat4::from_scale(Vec3::from_array(scale)));
    }
    // Presentation composition only: source local matrices are qualified;
    // these host products are not accepted VU/world-transform arithmetic.
    let mut worlds = vec![None; locals.len()];
    for index in 0..locals.len() {
        let mut chain = Vec::new();
        let mut current = Some(index);
        while let Some(i) = current {
            if worlds[i].is_some() {
                break;
            }
            chain.push(i);
            current = usize::try_from(tree.nodes[i].parent).ok();
        }
        while let Some(i) = chain.pop() {
            let parent = usize::try_from(tree.nodes[i].parent)
                .ok()
                .map_or(Mat4::IDENTITY, |p| worlds[p].unwrap());
            let world = parent * locals[i];
            if !world.is_finite() {
                return Err("non-finite presentation transform".into());
            }
            worlds[i] = Some(world);
        }
    }
    Ok(worlds.into_iter().map(Option::unwrap).collect())
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
                    Transform::from_matrix(node.transform),
                    RigNode(node.index),
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
    let fit_distance = extent.length() * if source.rig.is_some() { 1.8 } else { 1.4 };
    commands.insert_resource(InspectionCamera {
        target: center,
        overview: center,
        yaw: 0.0,
        pitch: 0.8,
        // Fit the source bounding sphere inside the default 45-degree view.
        distance: fit_distance,
        overview_distance: fit_distance,
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
        "{} | {} DATA INSPECTOR | gameplay not implemented\nArrow keys: orbit   WASD: pan   Q/E: distance   P: detail   R: overview   Esc: close\nOpaque base textures + source vertex colors; double sided; lightmaps/GS blending unverified\n{placed} placed parts | {} unresolved bitmap records shown magenta\nOBJECTS {}  placement source {}",
        source.label,if source.rig.is_some() {"ANIMATION"} else {"STATIC"},unresolved_materials.len(),&source.hashes[0][..12],&source.hashes[1][..12])),
        TextFont {font_size:15.0,..default()},TextColor(Color::WHITE),Anchor::TOP_LEFT,
        Transform::from_xyz(-580.0,380.0,0.0)));
    if source.rig.is_some() {
        commands.spawn((
            Text2d::new("Loading original animation channels..."),
            TextFont {
                font_size: 15.0,
                ..default()
            },
            TextColor(Color::WHITE),
            Anchor::TOP_LEFT,
            Transform::from_xyz(-580.0, 280.0, 0.0),
            AnimationHud,
        ));
    }
}

fn animate_rig(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    source: Res<SceneSource>,
    mut playback: ResMut<RigPlayback>,
    mut nodes: Query<(&RigNode, &mut Transform)>,
    mut hud: Query<&mut Text2d, With<AnimationHud>>,
) {
    let Some(rig) = &source.rig else {
        return;
    };
    let clips = &rig.animations.trees[0].sequences;
    if keys.just_pressed(KeyCode::PageUp) {
        playback.sequence = (playback.sequence + 1) % clips.len();
        playback.frame = 0.0;
    }
    if keys.just_pressed(KeyCode::PageDown) {
        playback.sequence = (playback.sequence + clips.len() - 1) % clips.len();
        playback.frame = 0.0;
    }
    let clip = &clips[playback.sequence];
    if keys.just_pressed(KeyCode::Space) {
        playback.playing = !playback.playing;
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        playback.frame = (playback.frame + 1.0).min(f32::from((clip.frames - 1).max(0)));
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        playback.frame = (playback.frame - 1.0).max(0.0);
    }
    if playback.playing && clip.frames > 0 {
        playback.frame = (playback.frame
            + time.delta_secs().min(0.1)
                / gdl_core::animation::frame_period(
                    clip.timing_factor,
                    1.0,
                    f32::from_bits(0x3d08_8889),
                )
                .expect("validated positive preview period"))
            % f32::from(clip.frames);
    }
    if !playback.is_changed() {
        return;
    }
    match rig_pose(rig, playback.sequence, playback.frame) {
        Ok(pose) => {
            for (node, mut transform) in &mut nodes {
                *transform = Transform::from_matrix(pose[node.0 as usize]);
            }
            if let Ok(mut text) = hud.single_mut() {
                text.0 = format!(
                    "Clip {} {} | frame {:.3}/{} | source timing factor {} | {}\nPgUp/PgDn: clip   [ / ]: frame   Space: diagnostic preview loop\nLocal matrices verified in measured clips; world composition/materials/playback clock unaccepted",
                    clip.index,
                    clip.label,
                    playback.frame,
                    clip.frames,
                    clip.timing_factor,
                    if playback.playing {
                        "preview loop"
                    } else {
                        "paused pose"
                    }
                );
            }
        }
        Err(e) => {
            playback.playing = false;
            if let Ok(mut text) = hud.single_mut() {
                text.0 = format!("Unsupported pose: {e}");
            }
        }
    }
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
    let animated = args.first().is_some_and(|a| a == "--animate");
    let rig = animated || args.first().is_some_and(|a| a == "--rig");
    if rig {
        args.remove(0);
    }
    let mut playback = RigPlayback::default();
    while args
        .first()
        .is_some_and(|a| a == "--clip" || a == "--frame")
    {
        if !animated || args.len() < 2 {
            return Err("--clip/--frame require animation mode and a value".into());
        }
        let flag = args.remove(0);
        let value = args.remove(0);
        let text = value.to_str().ok_or("pose argument is not UTF-8")?;
        if flag == "--clip" {
            playback.sequence = text.parse()?;
        } else {
            playback.frame = text.parse()?;
        }
    }
    if args.len() != if animated { 5 } else { 4 } {
        return Err("Usage: gdl_scene OBJECTS.PS2 WORLDS.PS2 TEXDEF.PS2 TEXTURES.PS2\n       gdl_scene --rig OBJECTS.PS2 ANIM.PS2 TEXDEF.PS2 TEXTURES.PS2\n       gdl_scene --animate OBJECTS.PS2 RIG_ANIM.PS2 CLIP_ANIM.PS2 TEXDEF.PS2 TEXTURES.PS2".into());
    }
    let files: Vec<_> = args
        .iter()
        .map(|p| bounded_read(Path::new(p)))
        .collect::<Result<_, _>>()?;
    let objects = Objects::parse(&files[0])?;
    let rig_source = if animated {
        let animations = Animations::parse(&files[2])?;
        let geometry_rig = Animations::parse(&files[1])?;
        if animations.trees.len() != 1
            || geometry_rig.trees.len() != 1
            || animations.trees[0].sequences.is_empty()
            || animations.trees[0].nodes.len() > 1024
            || animations.trees[0].nodes.len() != geometry_rig.trees[0].nodes.len()
            || !animations.trees[0]
                .nodes
                .iter()
                .zip(&geometry_rig.trees[0].nodes)
                .all(|(a, b)| a.label == b.label && a.parent == b.parent)
        {
            return Err("explicit clip/geometry rig correspondence is required".into());
        }
        let clip = animations.trees[0]
            .sequences
            .get(playback.sequence)
            .ok_or("clip index out of range")?;
        if !playback.frame.is_finite()
            || playback.frame < 0.0
            || playback.frame > f32::from((clip.frames - 1).max(0))
        {
            return Err("frame outside selected clip".into());
        }
        Some(RigSource {
            animations,
            bytes: files[2].clone(),
        })
    } else {
        None
    };
    let (placements, min, max, label, detail, detail_distance) = if rig {
        let animations = Animations::parse(&files[1])?;
        if animations.trees.len() != 1 {
            return Err("rig inspector requires one explicit tree".into());
        }
        let tree = &animations.trees[0];
        let translations = tree.initial_translations()?;
        let pose = rig_source
            .as_ref()
            .map(|s| rig_pose(s, playback.sequence, playback.frame))
            .transpose()?;
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
                transform: pose
                    .as_ref()
                    .map_or(Mat4::from_translation(Vec3::from_array(position)), |p| {
                        p[node.index as usize]
                    }),
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
                            let point = placement
                                .transform
                                .transform_point3(Vec3::from_array(vertex));
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
            if animated {
                format!("{} | original channel/matrix pose inspector", tree.label)
            } else {
                format!(
                    "{} | translation-only rest hierarchy; animated pose unverified",
                    tree.label
                )
            },
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
                    transform: Mat4::from_translation(Vec3::from_array(p.summed_position)),
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
        table: TextureTable::parse(&files[if animated { 3 } else { 2 }])?,
        textures: files[if animated { 4 } else { 3 }].clone(),
        hashes: files
            .iter()
            .map(|b| format!("{:x}", Sha256::digest(b)))
            .collect(),
        rig: rig_source,
    };
    App::new()
        .insert_resource(source)
        .insert_resource(playback)
        .insert_resource(ClearColor(Color::srgb(0.035, 0.04, 0.05)))
        .add_plugins(
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: if animated {
                        "Gauntlet PS2 native animation inspector"
                    } else {
                        "Gauntlet PS2 native static scene inspector"
                    }
                    .into(),
                    resolution: (1200, 800).into(),
                    ..default()
                }),
                ..default()
            }),
        )
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, animate_rig))
        .run();
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
