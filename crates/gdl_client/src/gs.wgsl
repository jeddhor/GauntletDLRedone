// GS-equivalent world material (docs/research/rendering.md).
//
// Observed G1 state: base pass TFX MODULATE (Cv = Ct*Cf >> 7, Av = At*Af >> 7),
// TEST_1 alpha GREATER 1, ALPHA_1 (Cs - Cd)*As + Cd; lightmap pass on the same
// geometry with ALPHA_1 Cd*As >> 7 where As is the PSMT4 lightmap's CLUT alpha.
// The two passes are folded into one fragment: framebuffer colour values are
// computed in GS (display) space and converted for the sRGB target.
#import bevy_pbr::forward_io::VertexOutput

struct GsParams {
    // Constant colour in GS units (rgb 1.0 = 255, alpha 1.0 = 0x80).
    tint: vec4<f32>,
    // x: base texture bound, y: lightmap bound, z/w: UV shift add.
    options: vec4<f32>,
    // VU1 lighting: xyz towards the light (w = enabled); rgb dir colour, a ambient.
    light_dir: vec4<f32>,
    light: vec4<f32>,
    // x: unlit colour add (× (1 + x)), y: lit ambient add.
    scale: vec4<f32>,
    // Point lights: position + radius, colour + attenuation.
    plight_pos: array<vec4<f32>, 8>,
    plight_col: array<vec4<f32>, 8>,
    // x: keep-A overlay bound (MBTreeSetAltTex −4), y: chrome (−3), z:
    // uv_xform applies.
    extra: vec4<f32>,
    // MBTreeSetUVScaleAdd: U scale, U add, V scale, V add.
    uv_xform: vec4<f32>,
    // setChrome's axes (xyz) and offsets (w). HYPOTHESIS (VU1 unread):
    // uv = (n·u + u.w, n·v + v.w).
    chrome_u: vec4<f32>,
    chrome_v: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: GsParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var lightmap_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var lightmap_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(5) var overlay_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(6) var overlay_sampler: sampler;

// Point lights: VU1 microcode (Tower state vu1MicroMem.bin, pairs 652..716
// and 1026..1090; docs/research/rendering.md). Per light, skipped when the
// vertex faces away or lies outside the radius: colour × min(7.5 ×
// attenuation × (1 − d²/r²) × cosθ / d, 1) — the 7.5 is the VU normal scale
// (5-bit unpack, ITOF4 − 7.5) against positions and lights × 8. Evaluated
// per fragment here (the VU lights vertices).
fn point_lights(n: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    var pl = vec3(0.0);
    for (var i = 0u; i < 8u; i = i + 1u) {
        let lp = params.plight_pos[i];
        if lp.w > 0.0 {
            let v = lp.xyz - p;
            let d = max(length(v), 0.001);
            let fall = 1.0 - (d * d) / (lp.w * lp.w);
            let cos_t = dot(n, v / d);
            if fall > 0.0 && cos_t > 0.0 {
                let k = min(7.5 * params.plight_col[i].a * fall * cos_t / d, 1.0);
                pl = pl + params.plight_col[i].rgb * k;
            }
        }
    }
    return pl;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
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
        rgb = rgb * min(base + point_lights(n, in.world_position.xyz), vec3(255.0 / 128.0));
    } else {
        // VU1 program B (vertex colours, lightmap pass): min(colour × scale,
        // 128) + point lights, ≤ 255. HYPOTHESIS: the scale is
        // MBLightmapMod / MBNoShadeMod's 1 + add.
        let base = min(rgb * max(1.0 + params.scale.x, 0.0), vec3(1.0));
        rgb = min(base + point_lights(normalize(in.world_normal), in.world_position.xyz), vec3(255.0 / 128.0));
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
        uv = vec2(dot(n, params.chrome_u.xyz) + params.chrome_u.w, dot(n, params.chrome_v.xyz) + params.chrome_v.w);
    }
    let t = textureSample(base_texture, base_sampler, uv);
    if params.options.x > 0.5 {
        rgb = rgb * t.rgb;
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
    let o = textureSample(overlay_texture, overlay_sampler, in.uv + params.options.zw);
    if params.extra.x > 0.5 && alpha >= 0.999 {
        let os = min(cf * o.rgb, vec3(1.0));
        let oa = af * o.a * (255.0 / 128.0);
        rgb = min(max((os - rgb) * oa + rgb, vec3(0.0)), vec3(1.0));
    }
#endif
#ifdef VERTEX_UVS_B
    let l = textureSample(lightmap_texture, lightmap_sampler, in.uv_b).r * (255.0 / 128.0);
    if params.options.y > 0.5 {
        rgb = rgb * l;
    }
#endif
    // Av (0..255) must exceed AREF 1.
    if floor(alpha * 128.0) <= 1.0 {
        discard;
    }
    // Display values: the frame is converted once by DisplayToLinear.
    return vec4(min(rgb, vec3(1.0)), min(alpha, 1.0));
}
