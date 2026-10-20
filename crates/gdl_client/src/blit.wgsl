// MB blit sprite (DrawBlit 0x1c1e68; docs/research/hud.md).
//
// Observed Tower HUD sprites: TFX MODULATE (Cv = Ct*Cf >> 7, Av = At*Af >> 7),
// TEST alpha GREATER 1, ALPHA (Cs - Cd)*As + Cd, bilinear (TEX1 0x60). Colours
// are GS units (rgba 0x80 = 1.0); framebuffer values are display values,
// converted for the sRGB target.
#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct BlitParams {
    // GS vertex colour / 128.
    rgba: vec4<f32>,
    // u0, u1, v0, v1 (texture fractions).
    crop: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: BlitParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = vec2(mix(params.crop.x, params.crop.y, in.uv.x), mix(params.crop.z, params.crop.w, in.uv.y));
    // At the original 640×448 frame the bilinear taps of a cropped sprite
    // (font glyph cells) stay within half a texel of the crop; at higher
    // window resolutions edge samples would blend the neighbouring cell.
    let half = 0.5 / vec2<f32>(textureDimensions(base_texture));
    let lo = min(params.crop.xz, params.crop.yw) + half;
    let hi = max(params.crop.xz, params.crop.yw) - half;
    let t = textureSample(base_texture, base_sampler, clamp(uv, min(lo, hi), max(lo, hi)));
    let rgb = min(t.rgb * params.rgba.rgb, vec3(1.0));
    // Texel alpha is raw GS alpha (0x80 = 1.0).
    let alpha = t.a * (255.0 / 128.0) * params.rgba.a;
    if floor(alpha * 128.0) <= 1.0 {
        discard;
    }
    return vec4(srgb_to_linear(rgb), min(alpha, 1.0));
}
