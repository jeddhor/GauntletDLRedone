//! FONTS/*.FNT glyph tables (font_init_desc 0x100de8 loads "fonts/%s.fnt";
//! MBNewFont 0x1c47b8 consumes it).
//!
//! Layout: +0 texture-name pointer and +8 glyph-table pointer (both zero in
//! the file; patched at load), +4 glyph height, then 16-byte glyph entries
//! from +0xc {character code, width, texel x, texel y} (i32 each) ended by a
//! code-0 entry. MBNewFont stops at the terminator or its per-font limit.
use crate::{binary::Reader, wad::ParseError};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Glyph {
    pub code: i32,
    pub width: i32,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Font {
    pub height: i32,
    /// Glyphs in file order (the order selects the texture page).
    pub glyphs: Vec<Glyph>,
}

impl Font {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let r = Reader { bytes, family: "FNT" };
        r.range(0, 12)?;
        if 64 * 1024 < bytes.len() {
            return Err(r.error(0, "font size policy exceeded"));
        }
        let height = r.i32(4);
        if !(1..=256).contains(&height) {
            return Err(r.error(4, "glyph height outside 1..=256"));
        }
        let mut glyphs = Vec::new();
        let mut p = 12;
        loop {
            r.range(p, 16)?;
            let g = Glyph { code: r.i32(p), width: r.i32(p + 4), x: r.i32(p + 8), y: r.i32(p + 12) };
            if g.code == 0 {
                break;
            }
            if !(1..256).contains(&g.code) || !(0..=256).contains(&g.width) || !(0..4096).contains(&g.x) || !(0..4096).contains(&g.y) {
                return Err(r.error(p, "glyph entry outside policy"));
            }
            glyphs.push(g);
            p += 16;
        }
        Ok(Self { height, glyphs })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_until_terminator() {
        let mut b = vec![0u8; 12];
        b[4..8].copy_from_slice(&16i32.to_le_bytes());
        for g in [[48i32, 8, 0, 0], [49, 8, 8, 0], [0, 0, 72, 0]] {
            for v in g {
                b.extend_from_slice(&v.to_le_bytes());
            }
        }
        let f = Font::parse(&b).unwrap();
        assert_eq!(f.height, 16);
        assert_eq!(f.glyphs, vec![Glyph { code: 48, width: 8, x: 0, y: 0 }, Glyph { code: 49, width: 8, x: 8, y: 0 }]);
        assert!(Font::parse(&b[..40]).is_err());
    }
}
