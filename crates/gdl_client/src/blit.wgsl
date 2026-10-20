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
    // A coincident z-only mask blit's screen rectangle (x, y, w, h; w = 0:
    // none): hidden where the mask texture's alpha is 0 (ATST EQUAL AREF 0).
    mask_rect: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> params: BlitParams;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var base_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var base_sampler: sampler;
@group(#{MATERIAL_BIND_GROUP}) @binding(3) var mask_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(4) var mask_sampler: sampler;

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
    if params.mask_rect.z > 0.0 {
        // HUD world → blit screen (512×384, y down).
        let s = vec2(in.world_position.x + 256.0, 192.0 - in.world_position.y);
        let m = textureSample(mask_texture, mask_sampler, (s - params.mask_rect.xy) / params.mask_rect.zw);
        if m.a * 255.0 < 0.5 {
            discard;
        }
    }
    // Texel alpha is raw GS alpha (0x80 = 1.0).
    let alpha = t.a * (255.0 / 128.0) * params.rgba.a;
    if floor(alpha * 128.0) <= 1.0 {
        discard;
    }
    // Display values: the frame is converted once by DisplayToLinear.
    return vec4(rgb, min(alpha, 1.0));
}
