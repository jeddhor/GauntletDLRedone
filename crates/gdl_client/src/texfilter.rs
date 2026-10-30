//! NATIVE ENHANCEMENT (Settings::texture_filter "sharp"; not part of the
//! original game). The GS samples a record's level 0 bilinearly, or, for
//! records the disc ships with mips, the level log2(1/Q) + K (TEX1 LCM 0;
//! research/textures.md "Mip selection") — a choice made for 448 lines, so
//! at window resolutions distant surfaces take a blurrier level than the
//! screen needs, and textures without mips alias. Sharp samples every base
//! texture with the GPU's own level of detail and 16× anisotropic
//! filtering over a full mip chain: the disc's levels where the record has
//! them, continued by levels generated here (2×2 box in linear light,
//! weighted by alpha). Cut-out textures (alpha either rejected by the GS
//! test, GREATER AREF 1, or full) stay cut-outs in every generated level,
//! binarised at the threshold that keeps level 0's coverage, so distant
//! fences and foliage neither thin out nor fill in.
//!
//! "original" keeps the client's images byte for byte: levels are only
//! added while sharp is on, and switching back truncates them again.
use bevy::{
    asset::{AssetId, Assets},
    image::{Image, ImageAddressMode, ImageSampler, ImageSamplerDescriptor},
};
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

static SHARP: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU32 = AtomicU32::new(0);

/// The sharp filter's level-of-detail bias (NATIVE choice, frozen-frame
/// comparison 2026-10-30: at 1920×1080 the GPU's own level already shows
/// C2's distant cliff sharper than the GS formula; −0.5 sharper still,
/// short of the shimmer a whole level invites).
pub const LOD_BIAS: f32 = -0.5;
static REGISTRY: Mutex<Option<HashMap<AssetId<Image>, TexKey>>> = Mutex::new(None);

/// Whether new base textures are made sharp (set from the settings before
/// packages load, and by `apply`).
pub fn sharp() -> bool {
    SHARP.load(Ordering::Relaxed)
}

/// What a base texture needs to change mode: its record's flags (address
/// modes) and the number of levels it was decoded with (the disc's, or 1
/// for a texture pack's replacement).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TexKey {
    pub flags: u16,
    pub levels: u8,
    /// A texture modifier's frame (`set_animated`): it keeps its own levels.
    pub animated: bool,
    /// A texture modifier's target: its texels are a frame's (copied in by
    /// gdl_play present_texmods, levels included), so only its sampler
    /// follows the mode.
    pub borrowed: bool,
}

/// Remember a base texture so `apply` can switch it.
pub fn register(id: AssetId<Image>, key: TexKey) {
    if let Ok(mut r) = REGISTRY.lock() {
        r.get_or_insert_with(HashMap::new).insert(id, key);
    }
}

/// The sampler of a base texture. Original: GS CLAMP_1 of the bitmap
/// record — flag 0x8 selected WMT CLAMP in every G1 draw matched to a
/// record (observed); flag 0x4 → WMS CLAMP is a HYPOTHESIS by symmetry
/// (draws with both clamps were not matched to records) — and
/// setTexInfo's TEX1 with the render settings the fixture holds (+0x370
/// MMAG 1 linear, +0x36c MMIN 5 LINEAR_MIPMAP_LINEAR, +0x368 L 0, +0x364 K
/// bias 0; research/textures.md), the levels clamped at MXL. Sharp: the
/// same address modes, 16× anisotropy, no level clamp.
pub fn sampler(key: TexKey, sharp: bool) -> ImageSampler {
    let mut s = ImageSamplerDescriptor::linear();
    s.address_mode_u = if key.flags & 0x4 != 0 { ImageAddressMode::ClampToEdge } else { ImageAddressMode::Repeat };
    s.address_mode_v = if key.flags & 0x8 != 0 { ImageAddressMode::ClampToEdge } else { ImageAddressMode::Repeat };
    if sharp {
        s.set_anisotropic_filter(anisotropy());
    } else if 1 < key.levels {
        s.lod_min_clamp = 0.0;
        s.lod_max_clamp = f32::from(key.levels - 1);
    }
    ImageSampler::Descriptor(s)
}

/// The sharp sampler's anisotropy (TEST switch GDL_SHARP_ANISO, 1..16;
/// default 16).
fn anisotropy() -> u16 {
    static A: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
    *A.get_or_init(|| std::env::var("GDL_SHARP_ANISO").ok().and_then(|v| v.parse().ok()).unwrap_or(16u16).clamp(1, 16))
}

/// `levels` (decoded RGBA8, level 0 first) continued to 1×1 when sharp.
pub fn with_chain(width: u32, height: u32, mut levels: Vec<Vec<u8>>, sharp: bool) -> Vec<Vec<u8>> {
    if sharp && !levels.is_empty() {
        let extra = generate(width, height, &levels);
        levels.extend(extra);
    }
    levels
}

/// Counts `apply` calls: texture modifiers' targets hold a copy of a
/// frame, so after a switch they copy their current frame in again.
pub fn generation() -> u32 {
    GENERATION.load(Ordering::Relaxed)
}

/// Switch every registered texture to `sharp` (levels and sampler); the
/// textures created afterwards follow it too. Images that no longer exist
/// are forgotten.
pub fn apply(images: &mut Assets<Image>, sharp: bool) {
    SHARP.store(sharp, Ordering::Relaxed);
    GENERATION.fetch_add(1, Ordering::Relaxed);
    let Ok(mut r) = REGISTRY.lock() else { return };
    let Some(map) = r.as_mut() else { return };
    map.retain(|id, key| {
        // Only images that change are taken mutably (each one taken is
        // re-uploaded).
        let Some(image) = images.get(*id) else { return false };
        if !needs_change(image, *key, sharp) {
            return true;
        }
        if let Some(image) = images.get_mut(*id) {
            set_mode(image, *key, sharp);
        }
        true
    });
}

fn needs_change(image: &Image, key: TexKey, sharp: bool) -> bool {
    if image.sampler != sampler(key, sharp) {
        return true;
    }
    if key.borrowed {
        return false;
    }
    let want = if sharp && !key.animated { full_levels(image.width(), image.height()) } else { u32::from(key.levels.max(1)) };
    image.texture_descriptor.mip_level_count != want
}

/// Texture modifiers' frames keep their own levels: a frame step uploads
/// the target again (gdl_play present_texmods), and with generated chains
/// those uploads cost the RX 580 about 3 ms on the frames that stepped
/// (perf-bench gfx-p1-sharp against gfx-p1-sharp-nochain, 2026-10-30).
/// Their targets hold a frame's texels and levels, so only their sampler
/// is managed from now on. With sharp on, the frames' generated levels are
/// dropped now; otherwise nothing changes.
pub fn set_animated(images: &mut Assets<Image>, targets: &[AssetId<Image>], frames: &[AssetId<Image>]) {
    let Ok(mut r) = REGISTRY.lock() else { return };
    let Some(map) = r.as_mut() else { return };
    let sharp = sharp();
    for (ids, target) in [(targets, true), (frames, false)] {
        for id in ids {
            let Some(key) = map.get_mut(id) else { continue };
            if target {
                key.borrowed = true;
            } else {
                key.animated = true;
            }
            let key = *key;
            if sharp
                && !key.borrowed
                && images.get(*id).is_some_and(|im| needs_change(im, key, sharp))
                && let Some(image) = images.get_mut(*id)
            {
                set_mode(image, key, sharp);
            }
        }
    }
}

/// One image's levels and sampler for `sharp` (unchanged images are not
/// touched, so nothing is re-uploaded).
pub fn set_mode(image: &mut Image, key: TexKey, sharp: bool) {
    let (w, h) = (image.width(), image.height());
    let count = image.texture_descriptor.mip_level_count;
    let want_sampler = sampler(key, sharp);
    let own = u32::from(key.levels.max(1));
    let full = full_levels(w, h);
    if !needs_change(image, key, sharp) {
        return;
    }
    if key.borrowed {
        image.sampler = want_sampler;
        return;
    }
    if let Some(data) = image.data.as_mut() {
        let own_bytes = level_bytes(w, h, own);
        if data.len() < own_bytes {
            return;
        }
        if sharp && !key.animated && count != full {
            let levels: Vec<Vec<u8>> = (0..own).map(|k| level_slice(data, w, h, k).to_vec()).collect();
            data.truncate(own_bytes);
            let extra = generate(w, h, &levels);
            for l in &extra {
                data.extend_from_slice(l);
            }
            image.texture_descriptor.mip_level_count = own + extra.len() as u32;
        } else if (!sharp || key.animated) && count != own {
            data.truncate(own_bytes);
            image.texture_descriptor.mip_level_count = own;
        }
    }
    image.sampler = want_sampler;
}

/// Levels down to 1×1.
pub fn full_levels(w: u32, h: u32) -> u32 {
    32 - w.max(h).max(1).leading_zeros()
}

fn level_size(w: u32, h: u32, k: u32) -> (u32, u32) {
    ((w >> k).max(1), (h >> k).max(1))
}

fn level_bytes(w: u32, h: u32, count: u32) -> usize {
    (0..count).map(|k| level_size(w, h, k)).map(|(a, b)| (a * b * 4) as usize).sum()
}

fn level_slice(data: &[u8], w: u32, h: u32, k: u32) -> &[u8] {
    let start = level_bytes(w, h, k);
    let (a, b) = level_size(w, h, k);
    &data[start..start + (a * b * 4) as usize]
}

/// Texel alpha the GS alpha test passes (vertex alpha 0x80: Av = At; TEST_1
/// ATE GREATER AREF 1).
const PASS: u8 = 2;

fn srgb_to_linear(v: u8) -> f32 {
    let c = f32::from(v) / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5).floor().clamp(0.0, 255.0) as u8
}

/// Levels after the last of `levels` down to 1×1 (empty when complete).
/// Level 0 decides the texture's class: cut-out (some texels rejected, the
/// rest at least 0x80), or otherwise alpha averaged as it is.
pub fn generate(width: u32, height: u32, levels: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let total = full_levels(width, height);
    let Some(mut prev) = levels.last().cloned() else { return Vec::new() };
    let mut k = levels.len() as u32;
    let base = &levels[0];
    let rejected = base.chunks_exact(4).filter(|p| p[3] < PASS).count();
    let cutout = 0 < rejected && base.chunks_exact(4).all(|p| p[3] < PASS || p[3] >= 0x80);
    let coverage = 1.0 - rejected as f64 / (base.len() / 4).max(1) as f64;
    let lut: Vec<f32> = (0..=255u8).map(srgb_to_linear).collect();
    let mut out = Vec::new();
    while k < total {
        let (pw, ph) = level_size(width, height, k - 1);
        let (w, h) = level_size(width, height, k);
        let mut next = vec![0u8; (w * h * 4) as usize];
        let mut alphas = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                let (mut rgb, mut weight, mut alpha, mut n) = ([0.0f32; 3], 0.0f32, 0.0f32, 0.0f32);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let (sx, sy) = ((2 * x + dx).min(pw - 1), (2 * y + dy).min(ph - 1));
                    let p = &prev[((sy * pw + sx) * 4) as usize..][..4];
                    let a = f32::from(p[3]);
                    // Cut-outs average only texels the test passes; other
                    // textures weight colour by alpha (no dark fringes from
                    // transparent texels), or plainly where all are clear.
                    let wgt = if cutout { if p[3] >= PASS { 1.0 } else { 0.0 } } else { a };
                    for c in 0..3 {
                        rgb[c] += lut[p[c] as usize] * wgt;
                    }
                    weight += wgt;
                    alpha += a;
                    n += 1.0;
                }
                let o = ((y * w + x) * 4) as usize;
                if weight <= 0.0 {
                    // All clear: the plain average keeps the texels' colour.
                    rgb = [0.0; 3];
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((2 * x + dx).min(pw - 1), (2 * y + dy).min(ph - 1));
                        let p = &prev[((sy * pw + sx) * 4) as usize..][..4];
                        for c in 0..3 {
                            rgb[c] += lut[p[c] as usize];
                        }
                    }
                    weight = n;
                }
                for c in 0..3 {
                    next[o + c] = linear_to_srgb(rgb[c] / weight);
                }
                let a = alpha / n;
                next[o + 3] = (a + 0.5).floor().min(255.0) as u8;
                alphas.push(a);
            }
        }
        if cutout {
            binarise(&mut next, w, &alphas, coverage);
        }
        out.push(next.clone());
        prev = next;
        k += 1;
    }
    out
}

/// Cut-out levels: the `coverage` fraction of texels with the highest
/// averaged alpha pass (0x80), the rest are rejected (0). Texels of equal
/// alpha are taken in 8×8 Bayer order, so a partly kept band is spread
/// evenly rather than cut at one row.
fn binarise(level: &mut [u8], width: u32, alphas: &[f32], coverage: f64) {
    const BAYER: [u8; 64] = [
        0, 32, 8, 40, 2, 34, 10, 42, 48, 16, 56, 24, 50, 18, 58, 26, 12, 44, 4, 36, 14, 46, 6, 38, 60, 28, 52, 20, 62, 30, 54, 22, 3, 35, 11, 43, 1, 33, 9, 41, 51, 19, 59, 27, 49, 17, 57,
        25, 15, 47, 7, 39, 13, 45, 5, 37, 63, 31, 55, 23, 61, 29, 53, 21,
    ];
    let n = alphas.len();
    let keep = ((coverage * n as f64).round() as usize).min(n);
    let rank = |i: usize| {
        let (x, y) = (i as u32 % width, i as u32 / width);
        BAYER[((y & 7) * 8 + (x & 7)) as usize]
    };
    let mut order: Vec<usize> = (0..n).filter(|&i| 0.0 < alphas[i]).collect();
    order.sort_by(|&a, &b| alphas[b].total_cmp(&alphas[a]).then(rank(a).cmp(&rank(b))));
    let mut pass = vec![false; n];
    for &i in order.iter().take(keep) {
        pass[i] = true;
    }
    for (p, ok) in level.chunks_exact_mut(4).zip(pass) {
        p[3] = if ok { 0x80 } else { 0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };

    fn image(w: u32, h: u32, data: Vec<u8>) -> Image {
        Image::new(Extent3d { width: w, height: h, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8Unorm, RenderAssetUsages::default())
    }

    #[test]
    fn chain_sizes_reach_one_texel() {
        assert_eq!(full_levels(64, 32), 7);
        assert_eq!(full_levels(1, 1), 1);
        let levels = generate(64, 32, &[vec![0x40; 64 * 32 * 4]]);
        let sizes: Vec<usize> = levels.iter().map(|l| l.len() / 4).collect();
        assert_eq!(sizes, vec![32 * 16, 16 * 8, 8 * 4, 4 * 2, 2, 1]);
        // The disc's own levels are continued, not replaced.
        let own = vec![vec![0u8; 8 * 8 * 4], vec![0u8; 4 * 4 * 4]];
        assert_eq!(generate(8, 8, &own).iter().map(|l| l.len() / 4).collect::<Vec<_>>(), vec![4, 1]);
    }

    #[test]
    fn colour_is_averaged_in_linear_light() {
        // Black and white texels, opaque: the mean of linear 0 and 1 is
        // 0.5, sRGB 188 (not the 128 of averaging display values).
        let mut l0 = Vec::new();
        for v in [0u8, 255, 255, 0] {
            l0.extend_from_slice(&[v, v, v, 0x80]);
        }
        let next = generate(2, 2, &[l0]);
        assert_eq!(next[0], vec![188, 188, 188, 0x80]);
    }

    #[test]
    fn cutouts_stay_binary_and_keep_their_coverage() {
        // A 16×16 grate: every fourth column open (25 % rejected).
        let mut l0 = Vec::new();
        for _y in 0..16 {
            for x in 0..16 {
                l0.extend_from_slice(if x % 4 == 0 { &[0, 0, 0, 0] } else { &[200, 100, 50, 0x80] });
            }
        }
        for level in generate(16, 16, &[l0]) {
            assert!(level.chunks_exact(4).all(|p| p[3] == 0 || p[3] == 0x80));
            let n = level.len() / 4;
            let pass = level.chunks_exact(4).filter(|p| p[3] == 0x80).count();
            if 4 <= n {
                assert!((pass as f64 / n as f64 - 0.75).abs() <= 1.0 / n as f64, "{pass}/{n}");
            }
            // Colour from the passing texels only: no black fringe.
            assert!(level.chunks_exact(4).filter(|p| p[3] == 0x80).all(|p| p[0] == 200 && p[1] == 100 && p[2] == 50));
        }
    }

    #[test]
    fn switching_back_restores_the_original_bytes() {
        let key = TexKey { flags: 0x8, levels: 1, animated: false, borrowed: false };
        let data: Vec<u8> = (0..32 * 16 * 4).map(|i| (i * 7 % 251) as u8).collect();
        let mut im = image(32, 16, data.clone());
        im.sampler = sampler(key, false);
        set_mode(&mut im, key, true);
        assert_eq!(im.texture_descriptor.mip_level_count, 6);
        assert_eq!(im.data.as_ref().unwrap().len(), level_bytes(32, 16, 6));
        set_mode(&mut im, key, false);
        assert_eq!(im.texture_descriptor.mip_level_count, 1);
        assert_eq!(im.data.as_deref(), Some(&data[..]));
        assert_eq!(im.sampler, sampler(key, false));
        // An animated texture keeps its own levels with the sharp sampler.
        let anim = TexKey { animated: true, ..key };
        set_mode(&mut im, anim, true);
        assert_eq!((im.texture_descriptor.mip_level_count, im.data.as_deref()), (1, Some(&data[..])));
        assert_eq!(im.sampler, sampler(anim, true));
    }

    #[test]
    fn original_sampler_is_unchanged() {
        // As lib.rs mip_sampler before the enhancement.
        let ImageSampler::Descriptor(s) = sampler(TexKey { flags: 0x4, levels: 4, animated: false, borrowed: false }, false) else { panic!() };
        assert_eq!((s.address_mode_u, s.address_mode_v, s.lod_max_clamp, s.anisotropy_clamp), (ImageAddressMode::ClampToEdge, ImageAddressMode::Repeat, 3.0, 1));
        let ImageSampler::Descriptor(s) = sampler(TexKey { flags: 0, levels: 1, animated: false, borrowed: false }, false) else { panic!() };
        assert_eq!((s.lod_min_clamp, s.lod_max_clamp), (0.0, 32.0));
        let ImageSampler::Descriptor(s) = sampler(TexKey { flags: 0, levels: 4, animated: false, borrowed: false }, true) else { panic!() };
        assert_eq!((s.anisotropy_clamp, s.lod_max_clamp), (16, 32.0));
    }
}
