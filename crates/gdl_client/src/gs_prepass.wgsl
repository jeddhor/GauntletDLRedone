// NATIVE ENHANCEMENT: the GS material in Bevy's prepass and shadow passes
// (character shadows, ambient occlusion; research/enhancements.md
// "Lighting"). Depth only, with gs.wgsl's alpha test (Av must exceed AREF
// 1) for materials that may discard; the normal prepass writes the
// interpolated world normal.
#import bevy_pbr::prepass_io::VertexOutput
#ifdef PREPASS_FRAGMENT
#import bevy_pbr::prepass_io::FragmentOutput
#endif
#ifdef BINDLESS
#import bevy_pbr::mesh_bindings::mesh
#import bevy_render::bindless::{bindless_samplers_filtering, bindless_textures_2d}
#endif

// gs.wgsl's GsParams (the same layout).
struct GsParams {
    tint: vec4<f32>,
    options: vec4<f32>,
    light_dir: vec4<f32>,
    light: vec4<f32>,
    scale: vec4<f32>,
    extra: vec4<f32>,
    uv_xform: vec4<f32>,
    chrome_u: vec4<f32>,
    chrome_v: vec4<f32>,
};

#ifdef BINDLESS
struct GsBindings {
    material: u32,
    base_texture: u32,
    base_sampler: u32,
    lightmap_texture: u32,
    lightmap_sampler: u32,
    overlay_texture: u32,
    overlay_sampler: u32,
    lights: u32,
};
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<storage> material_indices: array<GsBindings>;
@group(#{MATERIAL_BIND_GROUP}) @binding(10) var<storage> material_array: array<GsParams>;
#else
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: GsParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
#endif

// Av as gs.wgsl computes it (constant × vertex × texel alpha; the chrome
// mapping is left out).
fn gs_alpha(in: VertexOutput) -> f32 {
#ifdef BINDLESS
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let params = material_array[material_indices[slot].material];
#else
    let params = material;
#endif
    var alpha = params.tint.a;
#ifdef VERTEX_COLORS
    alpha = alpha * in.color.a;
#endif
#ifdef VERTEX_UVS_A
    var uv = in.uv;
    if params.extra.z > 0.5 {
        uv = uv * params.uv_xform.xz + params.uv_xform.yw;
    }
    uv = uv + params.options.zw;
#ifdef BINDLESS
    let t = textureSample(bindless_textures_2d[material_indices[slot].base_texture], bindless_samplers_filtering[material_indices[slot].base_sampler], uv);
#else
    let t = textureSample(base_texture, base_sampler, uv);
#endif
    if params.options.x > 0.5 {
        alpha = alpha * t.a * (255.0 / 128.0);
    }
#endif
    return alpha;
}

#ifdef PREPASS_FRAGMENT
@fragment
fn fragment(in: VertexOutput) -> FragmentOutput {
#ifdef MAY_DISCARD
    if floor(gs_alpha(in) * 128.0) <= 1.0 {
        discard;
    }
#endif
    var out: FragmentOutput;
#ifdef NORMAL_PREPASS
    // Parts without normals (zero) write the up axis.
    let n = select(vec3(0.0, 1.0, 0.0), normalize(in.world_normal), 1e-12 < dot(in.world_normal, in.world_normal));
    out.normal = vec4(n * 0.5 + vec3(0.5), 1.0);
#endif
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) {
    if floor(gs_alpha(in) * 128.0) <= 1.0 {
        discard;
    }
}
#endif
