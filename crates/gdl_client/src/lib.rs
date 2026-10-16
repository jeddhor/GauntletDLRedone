//! Shared original-data presentation helpers for the native inspectors and the
//! interim playtest. Material choices are explicitly diagnostic: unlit, opaque,
//! double sided, base textures with source vertex colours; lightmaps and GS
//! blending are not applied.
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    mesh::Indices,
    prelude::*,
    render::render_resource::{Extent3d, PrimitiveTopology, TextureDimension, TextureFormat},
};
use gdl_formats::{animation::Animations, objects::Objects, texdef::TextureTable};
use std::{collections::HashMap, error::Error, fs, path::Path};

pub fn bounded_read(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    use std::io::Read;
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > 64 * 1024 * 1024 {
        return Err("input exceeds policy".into());
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("input grew beyond policy".into());
    }
    Ok(bytes)
}

pub fn mesh_for(part: &gdl_formats::objects::Part) -> Result<Mesh, Box<dyn Error>> {
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
    Ok(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices)))
}

/// Diagnostic materials keyed by bitmap record index, plus unresolved indices.
/// With `alpha_cutout`, each texture is alpha-masked at half its own maximum
/// source alpha (textures whose alpha is all zero stay opaque). This is a
/// presentation choice for the playtest; original GS/TEXA alpha is unresolved.
pub fn materials_for(
    objects: &Objects,
    table: &TextureTable,
    textures: &[u8],
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    alpha_cutout: bool,
) -> (HashMap<u16, Handle<StandardMaterial>>, Handle<StandardMaterial>, Vec<u32>) {
    let mut cache = HashMap::new();
    let mut unresolved = Vec::new();
    for bitmap in &objects.bitmap_records {
        let candidates: Vec<_> = table
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
            candidates[0].decode_base_rgba(textures).ok()
        } else {
            None
        };
        let mut alpha_mode = AlphaMode::Opaque;
        if let (true, Some(rgba)) = (alpha_cutout, &decoded) {
            let (min, max) = rgba.chunks_exact(4).fold((255u8, 0u8), |(lo, hi), p| (lo.min(p[3]), hi.max(p[3])));
            if max > 0 && min < max {
                alpha_mode = AlphaMode::Mask(f32::from(max) / 510.0);
            }
        }
        let texture = decoded.map(|rgba| {
            let mut image = Image::new(
                Extent3d { width: bitmap.width as u32, height: bitmap.height as u32, depth_or_array_layers: 1 },
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
            unresolved.push(bitmap.index);
        }
        cache.insert(
            bitmap.index as u16,
            materials.add(StandardMaterial {
                base_color: if texture.is_some() { Color::WHITE } else { Color::srgb(1.0, 0.0, 0.8) },
                base_color_texture: texture,
                unlit: true,
                cull_mode: None,
                alpha_mode,
                ..default()
            }),
        );
    }
    let unknown = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.0, 0.8),
        unlit: true,
        cull_mode: None,
        ..default()
    });
    (cache, unknown, unresolved)
}

/// Mesh/material handles for every object index, with unsupported parts listed.
#[allow(clippy::type_complexity)]
pub fn object_meshes(
    objects: &Objects,
    material_cache: &HashMap<u16, Handle<StandardMaterial>>,
    unknown: &Handle<StandardMaterial>,
    meshes: &mut Assets<Mesh>,
) -> (HashMap<u32, Vec<(Handle<Mesh>, Handle<StandardMaterial>)>>, Vec<String>) {
    let mut cache = HashMap::new();
    let mut unsupported = Vec::new();
    for object in &objects.objects {
        let mut parts = Vec::new();
        for part in &object.parts {
            match mesh_for(part) {
                Ok(mesh) => parts.push((
                    meshes.add(mesh),
                    material_cache.get(&part.texture_index).cloned().unwrap_or_else(|| unknown.clone()),
                )),
                Err(error) => unsupported.push(format!("{}/{}: {error}", object.index, part.index)),
            }
        }
        cache.insert(object.index, parts);
    }
    (cache, unsupported)
}

/// Source local matrices for every node at a clip frame, composed with host
/// products for presentation (not accepted VU world-transform arithmetic).
pub fn rig_pose(animations: &Animations, bytes: &[u8], sequence: usize, frame: f32) -> Result<Vec<Mat4>, Box<dyn Error>> {
    let tree = &animations.trees[0];
    let mut locals = Vec::new();
    for node in &tree.nodes {
        if node.node_type != 1 {
            return Err("pose evaluation supports skeletal nodes only".into());
        }
        let track = tree.key_track(bytes, node.index, sequence as u32)?;
        let channels = gdl_core::animation::sample_channels(&track, frame)?;
        let (matrix, scale) =
            gdl_core::transform::local_transform(channels, node.initial_position, track.transform_flags)?;
        locals.push(Mat4::from_cols_array(&matrix) * Mat4::from_scale(Vec3::from_array(scale)));
    }
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
            let parent = usize::try_from(tree.nodes[i].parent).ok().map_or(Mat4::IDENTITY, |p| worlds[p].unwrap());
            let world = parent * locals[i];
            if !world.is_finite() {
                return Err("non-finite presentation transform".into());
            }
            worlds[i] = Some(world);
        }
    }
    Ok(worlds.into_iter().map(Option::unwrap).collect())
}
