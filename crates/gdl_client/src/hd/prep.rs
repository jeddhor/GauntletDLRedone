//! NATIVE ENHANCEMENT (HD textures): the processing around the network,
//! the same for every texture as the Checkpoint B packs the user chose from
//! (tools/python/upscale_compare.py `upscale`, float32 in its operation
//! order, numpy's round-half-to-even):
//!   - each axis is padded by PAD texels before scaling and cropped after,
//!     wrapped where the record repeats and replicated where it clamps
//!     (record flags 0x4 / 0x8), so tiling surfaces stay seamless;
//!   - fully transparent texels (a cut-out's rejected ones, a blended
//!     texture's alpha 0) first take the mean colour of kept 8-neighbours,
//!     repeated outward, so no dark fringes grow into the upscaled edges;
//!   - opaque textures stay opaque, a cut-out's alpha is thresholded at
//!     half (the material's alpha class does not change), blended alpha
//!     goes through the network as grey (BT.601 luma of the result).
//!
//! Texels here are "dump" RGBA8: GS alpha doubled to 0..255 (texture_pack's
//! files), so 255 is opaque and ≤ 2 rejected by the alpha test.

pub const PAD: usize = 12;
pub const SCALE: usize = 4;

/// One texture's network inputs.
pub struct Prepared {
    pub width: usize,
    pub height: usize,
    pub opaque: bool,
    pub binary: bool,
    /// Padded RGB, channel-major (3 × (h + 2 PAD) × (w + 2 PAD)), 0..1.
    pub rgb: Vec<f32>,
    /// Padded alpha as grey (3 planes) when not opaque.
    pub alpha: Option<Vec<f32>>,
}

/// GS RGBA (alpha 0x80 = opaque) → dump RGBA8 (alpha doubled, at most 255).
pub fn dump_rgba(gs: &[u8]) -> Vec<u8> {
    gs.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2], (u16::from(p[3]) * 2).min(255) as u8]).collect()
}

/// numpy.pad index: wrapped (mode 'wrap') or replicated ('edge').
fn src_index(i: isize, size: usize, wrap: bool) -> usize {
    if wrap { i.rem_euclid(size as isize) as usize } else { i.clamp(0, size as isize - 1) as usize }
}

/// upscale_compare.py fill_clear: colours of texels outside `kept` from the
/// mean of kept 8-neighbours, repeated outward until every texel has one.
fn fill_clear(rgb: &mut [[f32; 3]], kept: &[bool], w: usize, h: usize, wrap_u: bool, wrap_v: bool) {
    let mut have = kept.to_vec();
    if !have.iter().any(|&k| k) {
        return;
    }
    while !have.iter().all(|&k| k) {
        let mut acc = vec![[0f32; 3]; w * h];
        let mut cnt = vec![0f32; w * h];
        for dy in [-1isize, 0, 1] {
            for dx in [-1isize, 0, 1] {
                if dy == 0 && dx == 0 {
                    continue;
                }
                for y in 0..h {
                    let sy = src_index(y as isize - dy, h, wrap_v);
                    for x in 0..w {
                        let sx = src_index(x as isize - dx, w, wrap_u);
                        let s = sy * w + sx;
                        let k = if have[s] { 1f32 } else { 0f32 };
                        let i = y * w + x;
                        for c in 0..3 {
                            acc[i][c] += rgb[s][c] * k * 1.0;
                        }
                        cnt[i] += k;
                    }
                }
            }
        }
        let mut next = have.clone();
        for i in 0..w * h {
            if !have[i] && 0.0 < cnt[i] {
                for c in 0..3 {
                    rgb[i][c] = acc[i][c] / cnt[i];
                }
                next[i] = true;
            }
        }
        have = next;
    }
}

/// The network inputs of one dump RGBA8 texture with record `flags`.
pub fn prepare(rgba8: &[u8], width: usize, height: usize, flags: u16) -> Prepared {
    let (wrap_u, wrap_v) = (flags & 0x4 == 0, flags & 0x8 == 0);
    let a: Vec<u8> = rgba8.chunks_exact(4).map(|p| p[3]).collect();
    let opaque = a.iter().all(|&v| v == 255);
    let binary = a.iter().all(|&v| v <= 2 || v == 255);
    let mut rgb: Vec<[f32; 3]> = rgba8.chunks_exact(4).map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0]).collect();
    if !opaque {
        let kept: Vec<bool> = a.iter().map(|&v| if binary { 2 < v } else { 0 < v }).collect();
        fill_clear(&mut rgb, &kept, width, height, wrap_u, wrap_v);
    }
    let (pw, ph) = (width + 2 * PAD, height + 2 * PAD);
    let mut prgb = vec![0f32; 3 * pw * ph];
    let mut palpha = (!opaque).then(|| vec![0f32; 3 * pw * ph]);
    for y in 0..ph {
        let sy = src_index(y as isize - PAD as isize, height, wrap_v);
        for x in 0..pw {
            let sx = src_index(x as isize - PAD as isize, width, wrap_u);
            let s = sy * width + sx;
            for c in 0..3 {
                prgb[c * pw * ph + y * pw + x] = rgb[s][c];
            }
            if let Some(pa) = palpha.as_mut() {
                let v = a[s] as f32 / 255.0;
                for c in 0..3 {
                    pa[c * pw * ph + y * pw + x] = v;
                }
            }
        }
    }
    Prepared { width, height, opaque, binary, rgb: prgb, alpha: palpha }
}

/// The 4× dump RGBA8 from the network's cropped outputs (each 3 × 4h × 4w,
/// clamped to 0..1): `rgb` and, when not opaque, the alpha pass's grey.
pub fn finish(p: &Prepared, rgb: &[f32], alpha: Option<&[f32]>) -> Vec<u8> {
    let (ow, oh) = (p.width * SCALE, p.height * SCALE);
    let plane = ow * oh;
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round_ties_even() as u8;
    let mut out = Vec::with_capacity(plane * 4);
    for i in 0..plane {
        let a = if p.opaque {
            1.0
        } else {
            let g = alpha.expect("alpha pass");
            let v = 0.299f32 * g[i] + 0.587f32 * g[plane + i] + 0.114f32 * g[2 * plane + i];
            if p.binary { if 0.5 <= v { 1.0 } else { 0.0 } } else { v }
        };
        out.extend([q(rgb[i]), q(rgb[plane + i]), q(rgb[2 * plane + i]), q(a)]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_wraps_or_clamps_per_axis() {
        // 2×1 texture, U clamped (flag 0x4), V wrapped.
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255];
        let p = prepare(&rgba, 2, 1, 0x4);
        let pw = 2 + 2 * PAD;
        // Left of the texture: the first texel replicated; right: the last.
        assert_eq!(p.rgb[PAD * pw], 1.0);
        assert_eq!(p.rgb[PAD * pw + pw - 1], 0.0);
        assert_eq!(p.rgb[pw * (2 + 2 * PAD) / 2 + pw - 1], 0.0);
        assert!(p.opaque && p.alpha.is_none());
    }

    #[test]
    fn clear_texels_take_kept_neighbours_colour() {
        // A cut-out: one kept red texel, three rejected black ones.
        let rgba = [255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let p = prepare(&rgba, 2, 2, 0);
        assert!(p.binary && !p.opaque);
        let (pw, ph) = (2 + 2 * PAD, 2 + 2 * PAD);
        for y in PAD..PAD + 2 {
            for x in PAD..PAD + 2 {
                assert_eq!(p.rgb[y * pw + x], 1.0, "red everywhere");
                assert_eq!(p.rgb[pw * ph + y * pw + x], 0.0);
            }
        }
    }

    #[test]
    fn finish_keeps_alpha_classes() {
        let p = Prepared { width: 1, height: 1, opaque: false, binary: true, rgb: Vec::new(), alpha: None };
        let rgb = vec![0.5f32; 3 * 16];
        let low = vec![0.4f32; 3 * 16];
        let out = finish(&p, &rgb, Some(&low));
        assert!(out.chunks_exact(4).all(|t| t == [128, 128, 128, 0]));
        let p = Prepared { binary: false, ..p };
        let out = finish(&p, &rgb, Some(&low));
        assert!(out.chunks_exact(4).all(|t| t[3] == 102));
    }
}
