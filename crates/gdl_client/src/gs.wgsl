// GS-equivalent world material (docs/research/rendering.md).
//
// Observed G1 state: base pass TFX MODULATE (Cv = Ct*Cf >> 7, Av = At*Af >> 7),
// TEST_1 alpha GREATER 1, ALPHA_1 (Cs - Cd)*As + Cd; lightmap pass on the same
// geometry with ALPHA_1 Cd*As >> 7 where As is the PSMT4 lightmap's CLUT alpha.
// The two passes are folded into one fragment: framebuffer colour values are
// computed in GS (display) space and converted for the sRGB target.
#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::view

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
    // Point lights: position + radius, colour + attenuation.
    plight_pos: array<vec4<f32>, 8>,
    plight_col: array<vec4<f32>, 8>,
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

// One write to the PSMCT16S frame with DTHE (rendering.md "GS output
// stage"): clamp(c + DM[y & 3][x & 3], 0, 255) >> 3, read back as v << 3,
// at the fragment's PS2 pixel (the viewport is the 640×384 3-D area).
fn gs_write(c: vec3<f32>, frag: vec2<f32>) -> vec3<f32> {
    var dm = array<f32, 16>(-4.0, 2.0, -3.0, 3.0, 0.0, -2.0, 1.0, -1.0, -3.0, 3.0, -4.0, 2.0, 1.0, -1.0, 0.0, -2.0);
    let uv = (frag - view.viewport.xy) / view.viewport.zw;
    let p = vec2<u32>(clamp(floor(uv * vec2(640.0, 384.0)), vec2(0.0), vec2(639.0, 383.0)));
    let d = dm[(p.y & 3u) * 4u + (p.x & 3u)];
    let v = clamp(floor(min(c, vec3(1.0)) * 255.0 + 0.001) + d, vec3(0.0), vec3(255.0));
    return floor(v / 8.0) * 8.0 / 255.0;
}

// TFX MODULATE on the GS: (Cv × Ct) >> 7 per channel in integers — the
// vertex colour as sent (0..255, 128 = 1.0; `cv` here in that ratio) and
// the texel 0..255; the result as a display value.
fn gs_modulate(cv: vec3<f32>, tex: vec3<f32>) -> vec3<f32> {
    let v = floor(clamp(cv * 128.0, vec3(0.0), vec3(255.0)) + 0.001);
    let t = floor(tex * 255.0 + 0.5);
    return floor(v * t / 128.0) / 255.0;
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
    // GS LOD (TEX1 LCM 0, L 0): log2(1/Q) + K, 1/Q the view depth in game
    // units (the G1 dump's Q at the player matches it to ~5%). LOD < 0 is
    // magnification at level 0; the sampler clamps at MXL.
    var t: vec4<f32>;
    if params.scale.w > 0.5 {
        let depth = max(-(view.view_from_world * vec4(in.world_position.xyz, 1.0)).z, 1e-3);
        t = textureSampleLevel(base_texture, base_sampler, uv, max(log2(depth) + params.scale.z, 0.0));
    } else {
        t = textureSample(base_texture, base_sampler, uv);
    }
    if params.options.x > 0.5 {
        rgb = gs_modulate(rgb, t.rgb);
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
    let lm = textureSample(lightmap_texture, lightmap_sampler, in.uv_b).r;
    if params.options.y > 0.5 {
        // The base pass's own frame write truncates before the lightmap
        // pass reads it back (opaque texels; translucent ones blend first).
        if params.extra.w > 0.5 && alpha >= 0.999 {
            rgb = gs_write(rgb, in.position.xy);
        }
        // ALPHA_1 Cd × As >> 7 in integers (As: the lightmap's CLUT alpha).
        rgb = floor(floor(min(rgb, vec3(1.0)) * 255.0 + 0.5) * floor(lm * 255.0 + 0.5) / 128.0) / 255.0;
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
    return vec4(vec3(min(floor(lm * 255.0 + 0.5) / 128.0, 1.0)), 1.0);
#else
    return vec4(1.0);
#endif
#else
#ifdef ADDITIVE
    // ALPHA 0x48 Cs·As + Cd: (Cs × As) >> 7 in integers with As up to 0xff
    // (2.0), the sum saturating (COLCLAMP) — premultiplied, blended One/One.
    let cs = floor(min(rgb, vec3(1.0)) * 255.0 + 0.5);
    let as_ = min(floor(alpha * 128.0), 255.0);
    return vec4(min(floor(cs * as_ / 128.0), vec3(255.0)) / 255.0, 1.0);
#else
    // Display values: the frame is converted once by DisplayToLinear.
    return vec4(min(rgb, vec3(1.0)), min(alpha, 1.0));
#endif
#endif
}
