// Final pass (DisplayToLinear): the frame holds GS display values (every
// draw blends them as the GS does); the sRGB swapchain encodes on write, so
// they are decoded once here.
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
struct Settings {
    enabled: f32,
    offset_x: f32,
    offset_y: f32,
    dither: f32,
};
@group(0) @binding(2) var<uniform> settings: Settings;

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((max(c, vec3(0.0)) + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // Screen Adjust: the DISPLAY offset moves the picture; uncovered rows
    // and columns are black.
    let uv = in.uv - vec2(settings.offset_x, settings.offset_y);
    let c = textureSample(screen_texture, screen_sampler, uv);
    if any(uv < vec2(0.0)) || any(uv > vec2(1.0)) {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }
    var rgb = clamp(c.rgb, vec3(0.0), vec3(1.0));
    if settings.dither > 0.5 {
        // PSMCT16S frame with DTHE 1: each write stores
        // clamp(c + DM[y & 3][x & 3], 0, 255) >> 3 (DIMX 0x6071243571603524)
        // and the PCRTC shows v << 3. Only the final value is quantised.
        var dm = array<f32, 16>(-4.0, 2.0, -3.0, 3.0, 0.0, -2.0, 1.0, -1.0, -3.0, 3.0, -4.0, 2.0, 1.0, -1.0, 0.0, -2.0);
        let p = vec2<u32>(clamp(floor(uv * vec2(640.0, 448.0)), vec2(0.0), vec2(639.0, 447.0)));
        let d = dm[(p.y & 3u) * 4u + (p.x & 3u)];
        let v = clamp(floor(rgb * 255.0 + 0.001) + d, vec3(0.0), vec3(255.0));
        rgb = floor(v / 8.0) * 8.0 / 255.0;
    }
    return vec4(srgb_to_linear(rgb), c.a);
}
