//! Shared original-data presentation helpers for the native inspectors and the
//! interim playtest. World materials follow the GS state observed in the G1
//! dump (GsMaterial, docs/research/rendering.md); the folded single pass,
//! level-0 filtering and the blending colour space are presentation choices.
pub mod audio;
pub mod catalog;
pub mod menu;

use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
    mesh::{Indices, MeshVertexBufferLayoutRef},
    pbr::{MaterialPipeline, MaterialPipelineKey},
    prelude::*,
    render::render_resource::{
        AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, CompareFunction, Extent3d,
        PrimitiveTopology, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension,
        TextureFormat,
    },
    shader::ShaderRef,
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

/// GS vertex colour of a packed 16-bit colour, on the GS 0x80 = 1.0 scale.
/// Observed in the G1 dump: 5-bit channel c reaches the GS as (c << 2) | (c >> 3)
/// (every sampled value 66..127 fits), alpha bit 15 as 0x80.
fn gs_vertex_color(packed: u16) -> [f32; 4] {
    let channel = |c: u16| f32::from((c << 2) | (c >> 3)) / 128.0;
    [channel(packed & 31), channel((packed >> 5) & 31), channel((packed >> 10) & 31), if packed & 0x8000 != 0 { 1.0 } else { 0.0 }]
}

pub fn mesh_for(part: &gdl_formats::objects::Part) -> Result<Mesh, Box<dyn Error>> {
    Ok(mesh_and_colours(part)?.0)
}

/// The part's mesh and whether it carries a colour stream.
pub fn mesh_and_colours(part: &gdl_formats::objects::Part) -> Result<(Mesh, bool), Box<dyn Error>> {
    let geometry = part.decode_integer_geometry()?;
    let coloured = geometry.strips.iter().any(|s| !s.packed_colors.is_empty());
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut lightmap_uvs = Vec::new();
    let mut colors = Vec::new();
    let mut indices = Vec::new();
    let with_lightmap = geometry.strips.iter().any(|s| !s.lightmap_uvs.is_empty());
    for strip in geometry.strips {
        let start = positions.len() as u32;
        indices.extend(strip.indices.iter().map(|i| i + start));
        if strip.packed_colors.is_empty() {
            // PLACEHOLDER: an absent colour stream modulates by 0x80 (1.0).
            colors.extend(vec![[1.0; 4]; strip.positions.len()]);
        } else {
            colors.extend(strip.packed_colors.iter().map(|&p| gs_vertex_color(p)));
        }
        if with_lightmap {
            if strip.lightmap_uvs.len() == strip.positions.len() {
                lightmap_uvs.extend(strip.lightmap_uvs);
            } else {
                return Err("part mixes strips with and without lightmap UVs".into());
            }
        }
        positions.extend(strip.positions);
        normals.extend(strip.normals);
        uvs.extend(strip.uvs);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices));
    if with_lightmap {
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, lightmap_uvs);
    }
    Ok((mesh, coloured))
}

/// Registers the GS-equivalent material and its embedded shader.
pub struct GsMaterialPlugin;

impl Plugin for GsMaterialPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "gs.wgsl");
        bevy::asset::embedded_asset!(app, "blit.wgsl");
        bevy::asset::embedded_asset!(app, "display_to_linear.wgsl");
        app.add_plugins(bevy::core_pipeline::fullscreen_material::FullscreenMaterialPlugin::<DisplayToLinear>::default());
        app.add_plugins(MaterialPlugin::<GsMaterial>::default());
        app.add_plugins(bevy::sprite_render::Material2dPlugin::<BlitMaterial>::default());
    }
}

/// The final pass of the last camera: the frame holds GS display values
/// (GsMaterial, BlitMaterial and every blend work on them, as the GS does);
/// this decodes them once for the sRGB swapchain.
#[derive(Component, bevy::render::extract_component::ExtractComponent, Clone, Copy, Default, ShaderType)]
pub struct DisplayToLinear {
    pub enabled: f32,
}

impl bevy::core_pipeline::fullscreen_material::FullscreenMaterial for DisplayToLinear {
    fn fragment_shader() -> ShaderRef {
        "embedded://gdl_client/display_to_linear.wgsl".into()
    }
    fn node_edges() -> Vec<bevy::render::render_graph::InternedRenderLabel> {
        use bevy::core_pipeline::core_2d::graph::Node2d;
        use bevy::render::render_graph::RenderLabel;
        vec![Node2d::Tonemapping.intern(), Self::node_label().intern(), Node2d::EndMainPassPostProcessing.intern()]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct BlitParams {
    /// GS vertex colour / 128 (0x80 = 1.0).
    pub rgba: Vec4,
    /// u0, u1, v0, v1 texture fractions (MBBlitSetCrop).
    pub crop: Vec4,
    /// The z-only mask blit's screen rectangle (w = 0: none).
    pub mask_rect: Vec4,
}

/// An MB blit sprite (blit.wgsl): MODULATE, alpha test GREATER 1, standard
/// alpha blending, bilinear — the Tower dump's HUD sprite state.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct BlitMaterial {
    #[uniform(0)]
    pub params: BlitParams,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub mask: Option<Handle<Image>>,
}

impl bevy::sprite_render::Material2d for BlitMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://gdl_client/blit.wgsl".into()
    }
    fn alpha_mode(&self) -> bevy::sprite_render::AlphaMode2d {
        bevy::sprite_render::AlphaMode2d::Blend
    }
}

/// The HUD view of one package's bitmap records (OBJECTS order, the
/// MBOX_FindTexture handle space): the OBJECTS bitmap name table (labels →
/// records, definition sizes), MBRomTexPtr size/flags.
pub fn hud_texture_set(name: &str, objects: &Objects) -> gdl_core::game::hud::TexSet {
    let records = objects
        .bitmap_records
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let def = objects.bitmap_definitions.iter().find(|d| d.bitmap_index as usize == i);
            gdl_core::game::hud::TexInfo {
                label: def.map(|d| d.label.clone()).unwrap_or_default(),
                width: b.width,
                height: b.height,
                flags: b.flags,
                def_width: def.map(|d| d.width).unwrap_or(b.width),
                def_height: def.map(|d| d.height).unwrap_or(b.height),
            }
        })
        .collect();
    let names = objects.bitmap_definitions.iter().map(|d| (d.label.clone(), d.bitmap_index as u16)).collect();
    gdl_core::game::hud::TexSet { name: name.to_string(), records, names }
}

/// A HUD texture (bilinear, clamped): raw GS RGBA. Format 146 (4-bit, no
/// inline palette) takes the shared CLUT CBP 8968 the Tower dump binds for
/// the 8Hifonts/INITIALS glyph sprites (white, alpha 8 × index).
pub fn hud_image(objects: &Objects, table: &TextureTable, textures: &[u8], index: usize) -> Option<Image> {
    let b = objects.bitmap_records.get(index)?;
    let rgba = if b.format == 146 {
        b.decode_lightmap_indices(textures).ok()?.into_iter().flat_map(|i| [255, 255, 255, i * 8]).collect()
    } else {
        let def = table.bitmaps.iter().find(|t| t.texture_offset == b.texture_offset && t.format == b.format && t.width == b.width && t.height == b.height)?;
        let mut rgba = def.decode_base_rgba(textures).ok()?;
        // 16-bit texel alpha → TEXA TA1 0x80 / TA0 0 (as materials_for).
        if matches!(b.format, 0 | 16 | 48) {
            for p in rgba.chunks_exact_mut(4) {
                p[3] = if p[3] != 0 { 0x80 } else { 0 };
            }
        }
        rgba
    };
    let mut sampler = ImageSamplerDescriptor::linear();
    sampler.set_address_mode(ImageAddressMode::ClampToEdge);
    Some(raw_image(b.width, b.height, rgba, TextureFormat::Rgba8Unorm, ImageSampler::Descriptor(sampler)))
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct GsParams {
    /// Constant colour in GS units (rgb 1.0 = 255, alpha 1.0 = 0x80).
    pub tint: Vec4,
    /// x: base texture bound, y: lightmap bound, z/w: the texture shift's
    /// U/V add (MB_tex_shift_tex via setTexShift; texture modifiers).
    pub options: Vec4,
    /// VU1 vertex lighting of unlit-by-lightmap, colourless parts: xyz the
    /// direction towards the light (render space), w = 1 when enabled.
    pub light_dir: Vec4,
    /// rgb the directional light colour × intensity, a the ambient.
    pub light: Vec4,
    /// x: colour add of unlit draws (MBLightmapMod / MBNoShadeMod's add:
    /// colour × (1 + x)), y: ambient add of lit draws (MBTreeSetAmbientAdd
    /// / 255). HYPOTHESIS: setPrimColor's +0x48 term scales the colour.
    pub scale: Vec4,
    /// Point lights (MBPosLightNew from ProcessEffects): xyz render-space
    /// position, w radius; colour rgb, a attenuation (w = 0: unused).
    pub plight_pos: [Vec4; 8],
    pub plight_col: [Vec4; 8],
}

/// The observed G1 world material state folded into one pass (gs.wgsl):
/// MODULATE texture × vertex colour, alpha test GREATER 1, standard alpha
/// blending, then × lightmap CLUT alpha / 0x80. Double sided. Per-node draw
/// state (`GsDraw`) follows setPrimInfo: GS draws write depth unless the node
/// masks it, including translucent ones.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(GsDraw)]
pub struct GsMaterial {
    #[uniform(0)]
    pub params: GsParams,
    /// Lit by the scene light (characters, items: no colours, no lightmap).
    pub lit: bool,
    #[texture(1)]
    #[sampler(2)]
    pub base: Option<Handle<Image>>,
    #[texture(3)]
    #[sampler(4)]
    pub lightmap: Option<Handle<Image>>,
    pub alpha_mode: AlphaMode,
    pub draw: GsDraw,
}

/// GS draw state chosen from MB node flags (setPrimInfo 0x1beea0,
/// pbSetDORegs 0x1bfac8): ALPHA_1 0x48 (Cs·As + Cd) for flag 0x800000 on
/// parts without a lightmap, ZBUF_1 ZMSK for flag 0x80, TEST_1 0x3001d
/// (ZTST ALWAYS) for flag 0x40.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct GsDraw {
    pub additive: bool,
    pub no_depth_write: bool,
    pub depth_always: bool,
    /// ALPHA_1 0x89 (Cd·As): psys flag 0x10 sprites.
    pub multiply: bool,
    /// ALPHA C = FIX 0x80 (As = 1.0): psys flag 0x20 sprites.
    pub fixed_alpha: bool,
}

impl From<&GsMaterial> for GsDraw {
    fn from(material: &GsMaterial) -> Self {
        material.draw
    }
}

impl GsDraw {
    /// The draw state of a part under MB node flags `flags`. A lightmapped
    /// part keeps ALPHA 0x44 (setLmapInfo's pb_lm_tex_alpha_reg); flag
    /// 0x4000 disables the lightmap pass (pbSetDORegs).
    pub fn from_node_flags(flags: u32, lightmapped: bool) -> Self {
        let lightmapped = lightmapped && flags & 0x4000 == 0;
        GsDraw {
            additive: flags & 0x800000 != 0 && !lightmapped,
            no_depth_write: flags & 0x80 != 0,
            depth_always: flags & 0x40 != 0,
            multiply: false,
            fixed_alpha: false,
        }
    }
}

impl GsMaterial {
    pub fn solid(color: Color, alpha_mode: AlphaMode) -> Self {
        let c = color.to_srgba();
        GsMaterial {
            params: GsParams { tint: Vec4::new(c.red, c.green, c.blue, c.alpha), options: Vec4::ZERO, ..default() },
            lit: false,
            base: None,
            lightmap: None,
            alpha_mode,
            draw: GsDraw::default(),
        }
    }

    /// This material under `draw` (lightmap dropped when flag 0x4000 asked).
    pub fn with_draw(&self, flags: u32) -> Self {
        let mut m = self.clone();
        if flags & 0x4000 != 0 {
            m.lightmap = None;
            m.params.options.y = 0.0;
        }
        m.draw = GsDraw::from_node_flags(flags, m.lightmap.is_some());
        if m.draw.additive {
            m.alpha_mode = AlphaMode::Add;
        } else if m.draw.no_depth_write && m.alpha_mode == AlphaMode::Opaque {
            m.alpha_mode = AlphaMode::Blend;
        }
        m
    }
}

impl Material for GsMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://gdl_client/gs.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        let draw = key.bind_group_data;
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = !draw.no_depth_write;
            if draw.depth_always {
                depth.depth_compare = CompareFunction::Always;
            }
        }
        // GS blend equations (Cs/Cd colours, As source alpha or FIX 0x80):
        // 0x48 Cs·As + Cd, 0x89 Cd·As, 0x44 (Cs − Cd)·As + Cd.
        let color = match (draw.additive, draw.multiply, draw.fixed_alpha) {
            (true, _, false) => Some((BlendFactor::SrcAlpha, BlendFactor::One)),
            (true, _, true) => Some((BlendFactor::One, BlendFactor::One)),
            (false, true, false) => Some((BlendFactor::Zero, BlendFactor::SrcAlpha)),
            (false, true, true) => Some((BlendFactor::Zero, BlendFactor::One)),
            (false, false, true) => Some((BlendFactor::One, BlendFactor::Zero)),
            (false, false, false) => None,
        };
        if let Some((src, dst)) = color
            && let Some(target) = descriptor.fragment.as_mut().and_then(|f| f.targets.get_mut(0)).and_then(|t| t.as_mut())
        {
            target.blend = Some(BlendState {
                color: BlendComponent { src_factor: src, dst_factor: dst, operation: BlendOperation::Add },
                alpha: BlendComponent { src_factor: BlendFactor::Zero, dst_factor: BlendFactor::One, operation: BlendOperation::Add },
            });
        }
        Ok(())
    }
}

/// Images of an OBJECTS package: base textures (with their alpha class) and
/// format-146 lightmaps, keyed by bitmap record index.
pub struct MaterialSet {
    pub base: HashMap<u16, Handle<GsMaterial>>,
    /// Decoded base images by TEXDEF label (upper case; MBOX_FindTexture).
    pub labels: HashMap<String, Handle<Image>>,
    /// Bitmap record by upper-case label, and every record's image
    /// (placeholders included) for texture modifiers.
    pub label_index: HashMap<String, u16>,
    pub record_images: HashMap<u16, Handle<Image>>,
    pub lightmaps: HashMap<u16, Handle<Image>>,
    pub unknown: Handle<GsMaterial>,
    pub unresolved: Vec<u32>,
}

/// GS CLAMP_1 of a bitmap record: flag 0x8 selected WMT CLAMP in every G1
/// draw matched to a record (observed); flag 0x4 → WMS CLAMP is a HYPOTHESIS
/// by symmetry (draws with both clamps were not matched to records).
fn record_sampler(flags: u16) -> ImageSampler {
    let mut sampler = ImageSamplerDescriptor::linear();
    sampler.address_mode_u = if flags & 0x4 != 0 { ImageAddressMode::ClampToEdge } else { ImageAddressMode::Repeat };
    sampler.address_mode_v = if flags & 0x8 != 0 { ImageAddressMode::ClampToEdge } else { ImageAddressMode::Repeat };
    ImageSampler::Descriptor(sampler)
}

fn raw_image(width: u16, height: u16, data: Vec<u8>, format: TextureFormat, sampler: ImageSampler) -> Image {
    let mut image = Image::new(
        Extent3d { width: width as u32, height: height as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        format,
        RenderAssetUsages::default(),
    );
    image.sampler = sampler;
    image
}

/// GS-equivalent materials keyed by bitmap record index. Texels stay raw
/// (Rgba8Unorm); 16-bit texel alpha becomes TEXA's TA1 0x80 / TA0 0 (the G1
/// dump's TEXA). Base filtering is bilinear at level 0 (TEX1_1 MMAG/MMIN
/// linear; the mipmap levels are not decoded). Lightmap texels carry the
/// observed shared CLUT alpha 8 × index (DBP 0x2300 upload in the G1 dump;
/// HYPOTHESIS for other packages).
pub fn materials_for(
    objects: &Objects,
    table: &TextureTable,
    textures: &[u8],
    images: &mut Assets<Image>,
    materials: &mut Assets<GsMaterial>,
) -> MaterialSet {
    let mut base = HashMap::new();
    let mut labels = HashMap::new();
    let mut label_index = HashMap::new();
    let mut record_images = HashMap::new();
    let mut lightmaps = HashMap::new();
    let mut unresolved = Vec::new();
    for bitmap in &objects.bitmap_records {
        if bitmap.format == 146 {
            match bitmap.decode_lightmap_indices(textures) {
                Ok(indices) => {
                    let alpha = indices.iter().map(|i| i * 8).collect();
                    let mut sampler = ImageSamplerDescriptor::linear();
                    sampler.set_address_mode(ImageAddressMode::Repeat);
                    let image = raw_image(bitmap.width, bitmap.height, alpha, TextureFormat::R8Unorm, ImageSampler::Descriptor(sampler));
                    lightmaps.insert(bitmap.index as u16, images.add(image));
                }
                Err(_) => unresolved.push(bitmap.index),
            }
            continue;
        }
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
        if candidates.len() == 1 {
            label_index.entry(candidates[0].label.to_uppercase()).or_insert(bitmap.index as u16);
        }
        let Some(mut rgba) = decoded else {
            unresolved.push(bitmap.index);
            // A magenta placeholder image: texture modifiers (MBSetRomTexture /
            // MBCopyTexture) may later copy a decoded frame into this record.
            let image = images.add(raw_image(1, 1, vec![255, 0, 204, 128], TextureFormat::Rgba8Unorm, record_sampler(bitmap.flags)));
            record_images.insert(bitmap.index as u16, image.clone());
            base.insert(
                bitmap.index as u16,
                materials.add(GsMaterial {
                    params: GsParams { tint: Vec4::ONE, options: Vec4::new(1.0, 0.0, 0.0, 0.0), ..default() },
                    lit: false,
                    base: Some(image),
                    lightmap: None,
                    alpha_mode: AlphaMode::Blend,
                    draw: GsDraw::default(),
                }),
            );
            continue;
        };
        if matches!(bitmap.format, 0 | 16 | 48) {
            for p in rgba.chunks_exact_mut(4) {
                p[3] = if p[3] != 0 { 0x80 } else { 0 };
            }
        }
        // Opaque when every texel passes at full coverage; Mask when texels
        // are either rejected by the alpha test or full; otherwise blended.
        let opaque = rgba.chunks_exact(4).all(|p| p[3] >= 0x80);
        let binary = rgba.chunks_exact(4).all(|p| p[3] <= 1 || p[3] >= 0x80);
        let alpha_mode = if opaque {
            AlphaMode::Opaque
        } else if binary {
            AlphaMode::Mask(0.5)
        } else {
            AlphaMode::Blend
        };
        let image = images.add(raw_image(bitmap.width, bitmap.height, rgba, TextureFormat::Rgba8Unorm, record_sampler(bitmap.flags)));
        labels.entry(candidates[0].label.to_uppercase()).or_insert_with(|| image.clone());
        record_images.insert(bitmap.index as u16, image.clone());
        base.insert(
            bitmap.index as u16,
            materials.add(GsMaterial {
                params: GsParams { tint: Vec4::ONE, options: Vec4::new(1.0, 0.0, 0.0, 0.0), ..default() },
                lit: false,
                base: Some(image),
                lightmap: None,
                alpha_mode,
                draw: GsDraw::default(),
            }),
        );
    }
    let unknown = materials.add(GsMaterial::solid(Color::srgb(1.0, 0.0, 0.8), AlphaMode::Opaque));
    MaterialSet { base, labels, label_index, record_images, lightmaps, unknown, unresolved }
}

/// Mesh/material handles for every object index, with unsupported parts listed.
/// A part with a lightmap record and lightmap UVs gets its base material
/// with the lightmap bound (one material per base/lightmap pair).
#[allow(clippy::type_complexity)]
pub fn object_meshes(
    objects: &Objects,
    set: &MaterialSet,
    materials: &mut Assets<GsMaterial>,
    meshes: &mut Assets<Mesh>,
) -> (HashMap<u32, Vec<(Handle<Mesh>, Handle<GsMaterial>)>>, Vec<String>) {
    let mut cache = HashMap::new();
    let mut pairs: HashMap<(u16, u16), Handle<GsMaterial>> = HashMap::new();
    let mut lit_pairs: HashMap<u16, Handle<GsMaterial>> = HashMap::new();
    let mut unsupported = Vec::new();
    for object in &objects.objects {
        let mut parts = Vec::new();
        for part in &object.parts {
            match mesh_and_colours(part) {
                Ok((mesh, coloured)) => {
                    let base = set.base.get(&part.texture_index).cloned().unwrap_or_else(|| set.unknown.clone());
                    let lightmap = set.lightmaps.get(&part.lightmap_index).filter(|_| mesh.attribute(Mesh::ATTRIBUTE_UV_1).is_some());
                    // Colourless parts without a lightmap are lit by the VU
                    // (ambient + directional; Tower dump fit, research/rendering.md).
                    let base = if lightmap.is_none() && !coloured {
                        lit_pairs
                            .entry(part.texture_index)
                            .or_insert_with(|| {
                                let mut m = materials.get(&base).cloned().expect("base material");
                                m.lit = true;
                                materials.add(m)
                            })
                            .clone()
                    } else {
                        base
                    };
                    let material = match lightmap {
                        Some(image) => pairs
                            .entry((part.texture_index, part.lightmap_index))
                            .or_insert_with(|| {
                                let mut m = materials.get(&base).cloned().expect("base material");
                                m.lightmap = Some(image.clone());
                                m.params.options.y = 1.0;
                                materials.add(m)
                            })
                            .clone(),
                        None => base,
                    };
                    parts.push((meshes.add(mesh), material));
                }
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
    tree_pose(animations, 0, bytes, sequence, frame, false)
}

/// Node world matrices (relative to the tree root) of tree `tree_index` at
/// `frame` of `sequence`. With `rest_others`, non-skeletal nodes keep their
/// initial position instead of failing (item trees mix node types).
pub fn tree_pose(animations: &Animations, tree_index: usize, bytes: &[u8], sequence: usize, frame: f32, rest_others: bool) -> Result<Vec<Mat4>, Box<dyn Error>> {
    let tree = animations.trees.get(tree_index).ok_or("tree index")?;
    let mut locals = Vec::new();
    for node in &tree.nodes {
        if node.node_type != 1 {
            if rest_others {
                locals.push(Mat4::from_translation(Vec3::from_array(node.initial_position)));
                continue;
            }
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

