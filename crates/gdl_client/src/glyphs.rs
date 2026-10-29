//! Button prompts for Xbox-style controllers (NATIVE ENHANCEMENT, M21: the
//! original shows only PlayStation buttons). The prompts are separate
//! 32×32 textures (BUTTON_X, BUTTON_O, BUTTON_SQ, BUTTON_TRI: a black disc
//! with the coloured symbol; BUTTON_U/D/L/R are d-pad arrows and stay).
//! The Xbox variant keeps the original texture's disc and rim, clears the
//! symbol and draws the letter of the controller button that presses the
//! same pad entry (gdl_play's player_input: South cross, East circle,
//! North triangle, West square) in strokes of the symbols' weight. The
//! simulation never sees the swap: the presentation picks the image.

/// The prompts' style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    PlayStation,
    Xbox,
}

/// A controller's style: Sony pads (USB vendor 0x054c, or a name naming
/// one when no vendor is reported) keep the original prompts; every other
/// controller gets the Xbox letters (XInput pads, Steam's virtual pad and
/// most PC controllers share the Xbox layout).
pub fn pad_style(vendor: Option<u16>, name: &str) -> Style {
    let n = name.to_ascii_lowercase();
    if vendor == Some(0x054c) || ["sony", "playstation", "dualshock", "dualsense", "ps3", "ps4", "ps5"].iter().any(|k| n.contains(k)) {
        Style::PlayStation
    } else {
        Style::Xbox
    }
}

/// The Xbox letter and colour (texture RGB) for a PlayStation prompt.
fn letter_for(label: &str) -> Option<(char, [u8; 3])> {
    Some(match label.to_ascii_uppercase().as_str() {
        "BUTTON_X" => ('A', [0x30, 0xc8, 0x30]),
        "BUTTON_O" => ('B', [0xe0, 0x30, 0x20]),
        "BUTTON_SQ" => ('X', [0x30, 0x70, 0xe8]),
        "BUTTON_TRI" => ('Y', [0xe8, 0xc0, 0x10]),
        _ => return None,
    })
}

#[derive(Clone, Copy)]
enum Stroke {
    /// A segment with round ends.
    Line([f32; 2], [f32; 2]),
    /// The arc of radius r around c from angle a0 to a1 (degrees, y down).
    Arc([f32; 2], f32, f32, f32),
}

/// The letter's strokes in the 32×32 texture's units (cap height 8..24,
/// centred on the disc's centre 16, 16).
fn strokes(letter: char) -> &'static [Stroke] {
    use Stroke::{Arc, Line};
    match letter {
        'A' => &[Line([16.0, 8.0], [9.75, 24.0]), Line([16.0, 8.0], [22.25, 24.0]), Line([11.6, 19.25], [20.4, 19.25])],
        'B' => &[
            Line([11.0, 8.0], [11.0, 24.0]),
            Line([11.0, 8.0], [16.5, 8.0]),
            Arc([16.5, 11.75], 3.75, -90.0, 90.0),
            Line([11.0, 15.5], [17.0, 15.5]),
            Arc([17.0, 19.75], 4.25, -90.0, 90.0),
            Line([11.0, 24.0], [17.0, 24.0]),
        ],
        'X' => &[Line([10.25, 8.0], [21.75, 24.0]), Line([21.75, 8.0], [10.25, 24.0])],
        'Y' => &[Line([10.25, 8.0], [16.0, 16.25]), Line([21.75, 8.0], [16.0, 16.25]), Line([16.0, 16.25], [16.0, 24.0])],
        _ => &[],
    }
}

fn distance(p: [f32; 2], s: Stroke) -> f32 {
    let len = |x: f32, y: f32| (x * x + y * y).sqrt();
    match s {
        Stroke::Line(a, b) => {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            len(p[0] - a[0] - t * dx, p[1] - a[1] - t * dy)
        }
        Stroke::Arc(c, r, a0, a1) => {
            let angle = (p[1] - c[1]).atan2(p[0] - c[0]).to_degrees();
            if (a0..=a1).contains(&angle) {
                (len(p[0] - c[0], p[1] - c[1]) - r).abs()
            } else {
                [a0, a1]
                    .map(|a| {
                        let (s, co) = a.to_radians().sin_cos();
                        len(p[0] - c[0] - r * co, p[1] - c[1] - r * s)
                    })
                    .into_iter()
                    .fold(f32::MAX, f32::min)
            }
        }
    }
}

/// Half the symbols' stroke width (the originals' strokes are about 2.5
/// texels wide).
const HALF_WIDTH: f32 = 1.25;

/// The Xbox variant of a PlayStation face-button prompt (`rgba`: the
/// decoded texture, GS alpha), or None for any other texture. Opaque
/// texels (the disc and the symbol) are redrawn black with the letter
/// over them (4×4 samples a texel); the rim's translucent texels stay.
pub fn xbox_variant(label: &str, width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let (letter, colour) = letter_for(label)?;
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || rgba.len() != w * h * 4 {
        return None;
    }
    let opaque = rgba.chunks_exact(4).map(|p| p[3]).max()?;
    let (sx, sy) = (w as f32 / 32.0, h as f32 / 32.0);
    let shapes = strokes(letter);
    let mut out = rgba.to_vec();
    for y in 0..h {
        for x in 0..w {
            let p = &mut out[(y * w + x) * 4..(y * w + x) * 4 + 4];
            if p[3] != opaque {
                continue;
            }
            let mut hits = 0u32;
            for j in 0..4 {
                for i in 0..4 {
                    let q = [(x as f32 + (i as f32 + 0.5) / 4.0) / sx, (y as f32 + (j as f32 + 0.5) / 4.0) / sy];
                    if shapes.iter().any(|&s| distance(q, s) <= HALF_WIDTH) {
                        hits += 1;
                    }
                }
            }
            for (c, &v) in p[..3].iter_mut().zip(&colour) {
                *c = ((u32::from(v) * hits + 8) / 16) as u8;
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 32×32 disc of radius 15 (GS alpha 0x80) with a white mark.
    fn disc() -> Vec<u8> {
        let mut v = vec![0u8; 32 * 32 * 4];
        for y in 0..32 {
            for x in 0..32 {
                let (dx, dy) = (x as f32 + 0.5 - 16.0, y as f32 + 0.5 - 16.0);
                if (dx * dx + dy * dy).sqrt() < 15.0 {
                    let p = &mut v[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
                    p.copy_from_slice(&[0, 0, 0, 0x80]);
                    if dx.abs() < 2.0 {
                        p[..3].copy_from_slice(&[255, 255, 255]);
                    }
                }
            }
        }
        v
    }

    #[test]
    fn face_buttons_get_letters_and_others_stay() {
        let d = disc();
        assert!(xbox_variant("BUTTON_U", 32, 32, &d).is_none());
        let a = xbox_variant("button_x", 32, 32, &d).unwrap();
        // Alpha unchanged; the old mark gone; green strokes present.
        assert!(a.chunks_exact(4).zip(d.chunks_exact(4)).all(|(n, o)| n[3] == o[3]));
        assert!(a.chunks_exact(4).all(|p| p[0] != 255));
        assert!(a.chunks_exact(4).any(|p| p[1] == 0xc8 && p[0] == 0x30));
        // The A's apex is on the centre line near the top, its middle is empty.
        let at = |x: usize, y: usize| a[(y * 32 + x) * 4 + 1];
        assert!(0x60 < at(16, 9) && at(16, 14) < 0x20);
    }

    #[test]
    fn sony_pads_keep_the_original_prompts() {
        assert_eq!(pad_style(Some(0x054c), "Wireless Controller"), Style::PlayStation);
        assert_eq!(pad_style(None, "PS4 Controller"), Style::PlayStation);
        assert_eq!(pad_style(Some(0x045e), "Xbox Wireless Controller"), Style::Xbox);
        assert_eq!(pad_style(None, "Steam Virtual Gamepad"), Style::Xbox);
    }
}
