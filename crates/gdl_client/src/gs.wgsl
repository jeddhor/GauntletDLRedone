// GS-equivalent world material (docs/research/rendering.md).
//
// Observed G1 state: base pass TFX MODULATE (Cv = Ct*Cf >> 7, Av = At*Af >> 7),
// TEST_1 alpha GREATER 1, ALPHA_1 (Cs - Cd)*As + Cd; lightmap pass on the same
// geometry with ALPHA_1 Cd*As >> 7 where As is the PSMT4 lightmap's CLUT alpha.
// The two passes are folded into one fragment: framebuffer colour values are
// computed in GS (display) space and converted for the sRGB target.
#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view
#ifdef BINDLESS
#import bevy_pbr::mesh_bindings::mesh
#import bevy_render::bindless::{bindless_samplers_filtering, bindless_textures_2d}
#endif

struct GsParams {
    // Constant colour in GS units (rgb 1.0 = 255, alpha 1.0 = 0x80).
    tint: vec4<f32>,
    // x: base texture bound, y: lightmap bound, z/w: UV shift add.
    options: vec4<f32>,
    // VU1 lighting: xyz towards the light (w = enabled); rgb dir colour, a ambient.
    light_dir: vec4<f32>,
    light: vec4<f32>,
    // x: unlit colour add (× (1 + x)), y: lit ambient add, z: TEX1 K,
    // w: mipmapped base sampled at the GS LOD.
    scale: vec4<f32>,
    // x: keep-A overlay bound (MBTreeSetAltTex −4), y: chrome (−3), z:
    // uv_xform applies, w: the GS frame's 16-bit writes are modelled.
    extra: vec4<f32>,
    // MBTreeSetUVScaleAdd: U scale, U add, V scale, V add.
    uv_xform: vec4<f32>,
    // setChrome's axes (xyz) and offsets (w). HYPOTHESIS (VU1 unread):
    // uv = (n·u + u.w, n·v + v.w).
    chrome_u: vec4<f32>,
    chrome_v: vec4<f32>,
};

// Point lights, shared by every material: position + radius, colour +
// attenuation (lib.rs POINT_LIGHTS), then the NATIVE ENHANCEMENT switches
// (lib.rs write_enhance; all 0 is the original presentation): x sharp
// texture filtering (texfilter.rs), y full colour precision, z smooth
// lightmaps, w the sharp filter's level-of-detail bias.
struct PointLights {
    pos: array<vec4<f32>, 8>,
    col: array<vec4<f32>, 8>,
    enhance: vec4<f32>,
};

#ifdef BINDLESS
// The slab's index table (GsMaterial's bindless indices 0..8): the data
// array entry, then each texture / sampler's slot in Bevy's global binding
// arrays, then the point-light buffer's slot.
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
@group(#{MATERIAL_BIND_GROUP}) @binding(11) var<storage, read> plights_array: binding_array<PointLights>;
#else
@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: GsParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var lightmap_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var lightmap_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var overlay_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var overlay_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(7) var<storage, read> plights: PointLights;
#endif

// Point lights: VU1 microcode (Tower state vu1MicroMem.bin, pairs 652..716
// and 1026..1090; docs/research/rendering.md). Per light, skipped when the
// vertex faces away or lies outside the radius: colour × min(7.5 ×
// attenuation × (1 − d²/r²) × cosθ / d, 1) — the 7.5 is the VU normal scale
// (5-bit unpack, ITOF4 − 7.5) against positions and lights × 8. Evaluated
// per fragment here (the VU lights vertices).
fn point_lights(plights: PointLights, n: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    var pl = vec3(0.0);
    for (var i = 0u; i < 8u; i = i + 1u) {
        let lp = plights.pos[i];
        if lp.w > 0.0 {
            let v = lp.xyz - p;
            let d = max(length(v), 0.001);
            let fall = 1.0 - (d * d) / (lp.w * lp.w);
            let cos_t = dot(n, v / d);
            if fall > 0.0 && cos_t > 0.0 {
                let k = min(7.5 * plights.col[i].a * fall * cos_t / d, 1.0);
                pl = pl + plights.col[i].rgb * k;
            }
        }
    }
    return pl;
}

// One write to the PSMCT16S frame with DTHE (rendering.md "GS output
// stage"): clamp(c + DM[y & 3][x & 3], 0, 255) >> 3, read back as v << 3,
// at the fragment's PS2 pixel (the viewport is the 640×384 3-D area).
fn gs_write(c: vec3<f32>, frag: vec2<f32>) -> vec3<f32> {
    let d = gs_dither(frag);
    let v = clamp(floor(min(c, vec3(1.0)) * 255.0 + 0.001) + d, vec3(0.0), vec3(255.0));
    return floor(v / 8.0) * 8.0 / 255.0;
}

// DIMX at the fragment's PS2 pixel.
fn gs_dither(frag: vec2<f32>) -> f32 {
    var dm = array<f32, 16>(-4.0, 2.0, -3.0, 3.0, 0.0, -2.0, 1.0, -1.0, -3.0, 3.0, -4.0, 2.0, 1.0, -1.0, 0.0, -2.0);
    let uv = (frag - view.viewport.xy) / view.viewport.zw;
    let p = vec2<u32>(clamp(floor(uv * vec2(640.0, 384.0)), vec2(0.0), vec2(639.0, 383.0)));
    return dm[(p.y & 3u) * 4u + (p.x & 3u)];
}

// TFX MODULATE on the GS: (Cv × Ct) >> 7 per channel in integers — the
// vertex colour as sent (0..255, 128 = 1.0; `cv` here in that ratio) and
// the texel 0..255; the result as a display value. `full` (NATIVE
// ENHANCEMENT, full colour precision): the same product without its steps,
// at the truncation's mean level (−0.5).
fn gs_modulate(cv: vec3<f32>, tex: vec3<f32>, full: bool) -> vec3<f32> {
    if full {
        return max(clamp(cv, vec3(0.0), vec3(255.0 / 128.0)) * tex - 0.5 / 255.0, vec3(0.0));
    }
    let v = floor(clamp(cv * 128.0, vec3(0.0), vec3(255.0)) + 0.001);
    let t = floor(tex * 255.0 + 0.5);
    return floor(v * t / 128.0) / 255.0;
}

// NATIVE ENHANCEMENT (smooth lightmaps): the lightmap's cubic B-spline
// through four bilinear taps (Sigg & Hadwiger, GPU Gems 2 ch. 20). The
// 4-bit lightmaps' texel steps and 16 levels become smooth gradients; the
// level 0 taps need no derivatives.
fn bspline_r(t: texture_2d<f32>, s: sampler, uv: vec2<f32>) -> f32 {
    let size = vec2<f32>(textureDimensions(t, 0));
    let p = uv * size - 0.5;
    let i = floor(p);
    let f = p - i;
    let f2 = f * f;
    let f3 = f2 * f;
    let w0 = (1.0 - 3.0 * f + 3.0 * f2 - f3) / 6.0;
    let w1 = (4.0 - 6.0 * f2 + 3.0 * f3) / 6.0;
    let w2 = (1.0 + 3.0 * f + 3.0 * f2 - 3.0 * f3) / 6.0;
    let w3 = f3 / 6.0;
    let g0 = w0 + w1;
    let g1 = w2 + w3;
    let h0 = (i - 1.0 + w1 / g0 + 0.5) / size;
    let h1 = (i + 1.0 + w3 / g1 + 0.5) / size;
    let t00 = textureSampleLevel(t, s, vec2(h0.x, h0.y), 0.0).r;
    let t10 = textureSampleLevel(t, s, vec2(h1.x, h0.y), 0.0).r;
    let t01 = textureSampleLevel(t, s, vec2(h0.x, h1.y), 0.0).r;
    let t11 = textureSampleLevel(t, s, vec2(h1.x, h1.y), 0.0).r;
    return g0.y * (g0.x * t00 + g1.x * t10) + g1.y * (g0.x * t01 + g1.x * t11);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef BINDLESS
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let params = material_array[material_indices[slot].material];
    let pl_data = plights_array[material_indices[slot].lights];
#else
    let params = material;
    let pl_data = plights;
#endif
    let sharp = pl_data.enhance.x > 0.5;
    let full = pl_data.enhance.y > 0.5;
    var rgb = params.tint.rgb;
    var alpha = params.tint.a;
#ifdef VERTEX_COLORS
    rgb = rgb * in.color.rgb;
    alpha = alpha * in.color.a;
#endif
    if params.light_dir.w > 0.5 {
        let n = normalize(in.world_normal);
        let d = max(dot(n, params.light_dir.xyz), 0.0);
        // VU1 program A: min(ambient + light, 128) + point lights, ≤ 255.
        let base = min(vec3(1.0), vec3(params.light.a + params.scale.y) + d * params.light.rgb);
        rgb = rgb * min(base + point_lights(pl_data, n, in.world_position.xyz), vec3(255.0 / 128.0));
    } else {
        // VU1 program B (vertex colours, lightmap pass): min(colour × scale,
        // 128) + point lights, ≤ 255. HYPOTHESIS: the scale is
        // MBLightmapMod / MBNoShadeMod's 1 + add.
        let base = min(rgb * max(1.0 + params.scale.x, 0.0), vec3(1.0));
        rgb = min(base + point_lights(pl_data, normalize(in.world_normal), in.world_position.xyz), vec3(255.0 / 128.0));
    }
    let cf = rgb;
    let af = alpha;
#ifdef VERTEX_UVS_A
    var uv = in.uv;
    if params.extra.z > 0.5 {
        uv = uv * params.uv_xform.xz + params.uv_xform.yw;
    }
    uv = uv + params.options.zw;
    if params.extra.y > 0.5 {
        let n = normalize(in.world_normal);
        // VU1 program A: t = N·axis (the ×7.5 normal encoding cancels the
        // axes' 1/7.5), then setTexShift's chrome scale −0.5 and offset
        // −0.5 + the camera angle (pb_debug_chrome +0x34/+0x38 = −1.0,
        // texsh_def [1, 0, 1, 0]).
        let t = vec2(dot(n, params.chrome_u.xyz), dot(n, params.chrome_v.xyz));
        uv = t * -0.5 + vec2(params.chrome_u.w - 0.5, params.chrome_v.w - 0.5);
    }
    // GS LOD (TEX1 LCM 0, L 0): log2(1/Q) + K, 1/Q the view depth in game
    // units (the G1 dump's Q at the player matches it to ~5%). LOD < 0 is
    // magnification at level 0; the sampler clamps at MXL.
    // Both samples are taken and one chosen: in bindless mode `params` is
    // per instance, and implicit-derivative sampling must stay in uniform
    // control flow.
    let depth = max(-(view.view_from_world * vec4(in.world_position.xyz, 1.0)).z, 1e-3);
    let lod = max(log2(depth) + params.scale.z, 0.0);
#ifdef BINDLESS
    let t_lod = textureSampleLevel(bindless_textures_2d[material_indices[slot].base_texture], bindless_samplers_filtering[material_indices[slot].base_sampler], uv, lod);
    let t_auto = textureSampleBias(bindless_textures_2d[material_indices[slot].base_texture], bindless_samplers_filtering[material_indices[slot].base_sampler], uv, pl_data.enhance.w);
#else
    let t_lod = textureSampleLevel(base_texture, base_sampler, uv, lod);
    let t_auto = textureSampleBias(base_texture, base_sampler, uv, pl_data.enhance.w);
#endif
    // Sharp (NATIVE ENHANCEMENT, texfilter.rs): the GPU's own level of
    // detail at the window's resolution, anisotropic, for every texture.
    let t = select(t_auto, t_lod, params.scale.w > 0.5 && !sharp);
    if params.options.x > 0.5 {
        rgb = gs_modulate(rgb, t.rgb, full);
        alpha = alpha * t.a * (255.0 / 128.0);
    }
#endif
    rgb = min(rgb, vec3(1.0));
#ifdef VERTEX_UVS_A
    // Keep-A pass (setupKeepA): the same geometry with the alternate
    // texture, TFX MODULATE, ALPHA_1 0x44 and TEST_1 0x5c000 (DATE/DATM:
    // only where the base pass wrote alpha bit 7, Av >= 0x80).
    // VU1 program A re-kicks the base pass's vertex block for it, so it
    // uses the base UVs (rendering.md "VU1 microcode").
#ifdef BINDLESS
    let o = textureSample(bindless_textures_2d[material_indices[slot].overlay_texture], bindless_samplers_filtering[material_indices[slot].overlay_sampler], in.uv + params.options.zw);
#else
    let o = textureSample(overlay_texture, overlay_sampler, in.uv + params.options.zw);
#endif
    if params.extra.x > 0.5 && alpha >= 0.999 {
        let os = min(cf * o.rgb, vec3(1.0));
        let oa = af * o.a * (255.0 / 128.0);
        rgb = min(max((os - rgb) * oa + rgb, vec3(0.0)), vec3(1.0));
    }
#endif
#ifdef VERTEX_UVS_B
#ifdef BINDLESS
    let lm_bilinear = textureSample(bindless_textures_2d[material_indices[slot].lightmap_texture], bindless_samplers_filtering[material_indices[slot].lightmap_sampler], in.uv_b).r;
    let lm_smooth = bspline_r(bindless_textures_2d[material_indices[slot].lightmap_texture], bindless_samplers_filtering[material_indices[slot].lightmap_sampler], in.uv_b);
#else
    let lm_bilinear = textureSample(lightmap_texture, lightmap_sampler, in.uv_b).r;
    let lm_smooth = bspline_r(lightmap_texture, lightmap_sampler, in.uv_b);
#endif
    let lm = select(lm_bilinear, lm_smooth, pl_data.enhance.z > 0.5);
    if params.options.y > 0.5 {
        // The base pass's own frame write truncates before the lightmap
        // pass reads it back (opaque texels; translucent ones blend first).
        // Full colour: that write's mean level without its steps — the
        // DIMX matrix holds each offset −4..3 twice, so the dithered 16-bit
        // write of x averages max(x − 4, 0) for integer x, x − 4.5 here.
        if params.extra.w > 0.5 && alpha >= 0.999 {
            if full {
                rgb = clamp(rgb - 4.5 / 255.0, vec3(0.0), vec3(248.0 / 255.0));
            } else {
                rgb = gs_write(rgb, in.position.xy);
            }
        }
        // ALPHA_1 Cd × As >> 7 in integers (As: the lightmap's CLUT alpha).
        if full {
            rgb = max(min(rgb, vec3(1.0)) * (lm * 255.0 / 128.0) - 0.5 / 255.0, vec3(0.0));
        } else {
            rgb = floor(floor(min(rgb, vec3(1.0)) * 255.0 + 0.5) * floor(lm * 255.0 + 0.5) / 128.0) / 255.0;
        }
    }
#endif
    // Av (0..255) must exceed AREF 1.
    if floor(alpha * 128.0) <= 1.0 {
        discard;
    }
#ifdef LIGHTMAP_PASS
    // setLmapInfo's pass on its own (GsDraw::lightmap_pass): ALPHA_1 Cd × As,
    // As the lightmap's CLUT alpha, TEST 0x5c000 (DATE/DATM: where the
    // destination alpha bit is set — the pixels this part's base pass wrote
    // with FBA 1; the floor below already ran its own lightmap pass, whose
    // alpha leaves the bit clear). So: the base pass's alpha test, then the
    // frame (the base blended over what was there) times the lightmap.
#ifdef VERTEX_UVS_B
    if full {
        return vec4(vec3(min(lm * 255.0 / 128.0, 1.0)), 1.0);
    }
    return vec4(vec3(min(floor(lm * 255.0 + 0.5) / 128.0, 1.0)), 1.0);
#else
    return vec4(1.0);
#endif
#else
#ifdef ADDITIVE
    // ALPHA 0x48 Cs·As + Cd: (Cs × As) >> 7 in integers with As up to 0xff
    // (2.0), the sum saturating (COLCLAMP) — premultiplied, blended One/One.
    if full {
        // The truncated product and the write's step at their mean level
        // (−0.5, −4), not below 0: the GS's darkening by faint texels is
        // left out.
        return vec4(max(min(rgb, vec3(1.0)) * min(alpha, 255.0 / 128.0) - 4.5 / 255.0, vec3(0.0)), 1.0);
    }
    let cs = floor(min(rgb, vec3(1.0)) * 255.0 + 0.5);
    let as_ = min(floor(alpha * 128.0), 255.0);
    let v = min(floor(cs * as_ / 128.0), vec3(255.0));
    if params.extra.w > 0.5 {
        // Each GS write stores clamp(Cd + v + DM) >> 3 in the PSMCT16S
        // frame, and Cd is already a multiple of 8, so the draw adds
        // floor((v + DM) / 8) × 8 (−8 where v + DM < 0). The final pass
        // quantises the frame below once (Cd's own write), so emitting that
        // step here gives the GS value for any stack of additive layers.
        return vec4(floor((v + gs_dither(in.position.xy)) / 8.0) * 8.0 / 255.0, 1.0);
    }
    return vec4(v / 255.0, 1.0);
#else
    // Display values: the frame is converted once by DisplayToLinear.
    return vec4(min(rgb, vec3(1.0)), min(alpha, 1.0));
#endif
#endif
}
