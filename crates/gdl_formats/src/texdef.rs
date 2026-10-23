//! Observed TEXDEF.PS2 v23 metadata and candidate base-level RGBA decoding.
//!
//! Eight-bit source alpha remains unmodified; one-bit selectors are displayed
//! as binary alpha, explicitly separate from original TEXA/material semantics.
//! Material/GS blending and mipmap parity are separate fidelity gates.
use crate::wad::ParseError;
use serde::Serialize;

pub const BASE_RGBA_DECODER_VERSION: &str = "ps2-base-rgba-v2";

fn fail(offset: usize, index: Option<u32>, reason: &str) -> ParseError {
    ParseError {
        entry_index: index,
        byte_offset: offset,
        reason: format!("TEXDEF: {reason}"),
    }
}
fn u16le(bytes: &[u8], p: usize) -> u16 {
    u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap())
}
fn u32le(bytes: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(bytes[p..p + 4].try_into().unwrap())
}

#[derive(Debug, Serialize)]
pub struct Bitmap {
    pub index: u32,
    pub label: String,
    pub definition_in_objects: i16,
    pub definition_width: u16,
    pub definition_height: u16,
    pub version: u32,
    pub flags: u16,
    pub format: u8,
    pub mipmap_count: u8,
    pub lod_k: i8,
    pub width: u16,
    pub height: u16,
    pub texture_offset: u32,
    pub texture_size: u32,
    pub raw_definition: Vec<u8>,
    pub raw_record: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct TextureTable {
    pub definition_offset: u32,
    pub bitmap_offset: u32,
    pub bitmaps: Vec<Bitmap>,
}
impl TextureTable {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        if bytes.len() < 12 {
            return Err(fail(0, None, "truncated header"));
        }
        let count = u32le(bytes, 0) as usize;
        if count > 100_000 {
            return Err(fail(0, None, "record count limit exceeded"));
        }
        let definitions = u32le(bytes, 4) as usize;
        let records = u32le(bytes, 8) as usize;
        let d_end = definitions
            .checked_add(count * 36)
            .ok_or_else(|| fail(4, None, "definition range overflow"))?;
        let r_end = records
            .checked_add(count * 64)
            .ok_or_else(|| fail(8, None, "bitmap range overflow"))?;
        if definitions < 12 || records < 12 || d_end > bytes.len() || r_end > bytes.len() {
            return Err(fail(4, None, "tables intersect header or exceed file"));
        }
        if count != 0 && definitions < r_end && records < d_end {
            return Err(fail(4, None, "metadata tables overlap"));
        }
        let mut bitmaps = Vec::with_capacity(count);
        let mut pixel_total = 0u64;
        for i in 0..count {
            let d = definitions + i * 36;
            let b = records + i * 64;
            let version = u32le(bytes, b);
            if version != 0xf00b0017 {
                return Err(fail(b, Some(i as u32), "unsupported record version"));
            }
            let width = u16le(bytes, b + 12);
            let height = u16le(bytes, b + 14);
            if !width.is_power_of_two()
                || !height.is_power_of_two()
                || width > 4096
                || height > 4096
            {
                return Err(fail(
                    b + 12,
                    Some(i as u32),
                    "invalid/over-limit texture dimensions",
                ));
            }
            pixel_total += width as u64 * height as u64;
            if pixel_total > 512 * 1024 * 1024 {
                return Err(fail(b, Some(i as u32), "total pixel limit exceeded"));
            }
            let label = bytes[d..d + 30]
                .iter()
                .take_while(|v| **v != 0)
                .map(|v| char::from(*v))
                .collect();
            bitmaps.push(Bitmap {
                index: i as u32,
                label,
                definition_in_objects: u16le(bytes, d + 30) as i16,
                definition_width: u16le(bytes, d + 32),
                definition_height: u16le(bytes, d + 34),
                version,
                flags: u16le(bytes, b + 4),
                format: bytes[b + 8],
                mipmap_count: bytes[b + 10],
                lod_k: bytes[b + 11] as i8,
                width,
                height,
                texture_offset: u32le(bytes, b + 16),
                texture_size: u32le(bytes, b + 20),
                raw_definition: bytes[d..d + 36].to_vec(),
                raw_record: bytes[b..b + 64].to_vec(),
            });
        }
        Ok(Self {
            definition_offset: definitions as u32,
            bitmap_offset: records as u32,
            bitmaps,
        })
    }
}
impl Bitmap {
    /// Candidate base pixels. CT16 RGB uses the GS five-bit channel shift.
    /// Its high bit is a TEXA selector, displayed as 0/255 diagnostically.
    /// IDs 130/146 and actual TEXA/material alpha await consumer evidence.
    pub fn decode_base_rgba(&self, textures: &[u8]) -> Result<Vec<u8>, ParseError> {
        self.decode_level_rgba(textures, 0)
    }

    /// Candidate pixels of mip level `level` (0 = the base level). The levels
    /// follow the base indices consecutively, each max(1, width >> level) ×
    /// max(1, height >> level), and share the palette — the slices the
    /// original uploads for each level (research/textures.md "Reference upload
    /// and memory checks"). Levels beyond `mipmap_count` are refused.
    pub fn decode_level_rgba(&self, textures: &[u8], level: u8) -> Result<Vec<u8>, ParseError> {
        if self.mipmap_count < level {
            return Err(fail(self.texture_offset as usize, Some(self.index), "mip level beyond the record's count"));
        }
        let (palette_bytes, bits) = match self.format {
            0 => (0usize, 16usize),
            2 => (0, 32),
            16 => (32, 4),
            34 => (64, 4),
            48 => (512, 8),
            66 => (1024, 8),
            _ => {
                return Err(fail(
                    self.texture_offset as usize,
                    Some(self.index),
                    "unsupported pixel format",
                ));
            }
        };
        let dims = |k: u8| ((self.width as usize >> k).max(1), (self.height as usize >> k).max(1));
        let level_bytes = |k: u8| {
            let (w, h) = dims(k);
            (w * h * bits).div_ceil(8)
        };
        let (width, height) = dims(level);
        let count = width * height;
        let pixels_bytes = level_bytes(level);
        let skip: usize = (0..level).map(level_bytes).sum();
        let start = self.texture_offset as usize;
        let end = start
            .checked_add(self.texture_size as usize)
            .ok_or_else(|| fail(start, Some(self.index), "texture range overflow"))?;
        let bytes = textures
            .get(start..end)
            .ok_or_else(|| fail(start, Some(self.index), "texture exceeds payload"))?;
        if palette_bytes + skip + pixels_bytes > bytes.len() {
            return Err(fail(
                start,
                Some(self.index),
                "truncated palette/pixels",
            ));
        }
        if self.format == 2 {
            return Ok(bytes[skip..skip + pixels_bytes].to_vec());
        }
        let color_bytes = if matches!(self.format, 0 | 16 | 48) {
            2
        } else {
            4
        };
        let indices = &bytes[palette_bytes + skip..palette_bytes + skip + pixels_bytes];
        let mut rgba = Vec::with_capacity(count * 4);
        for i in 0..count {
            let index = if self.format == 0 {
                i
            } else if matches!(self.format, 16 | 34) {
                ((indices[i / 2] >> ((i % 2) * 4)) & 15) as usize
            } else {
                indices[i] as usize
            };
            // The 256-entry GS CSM1 CLUT swaps address bits 3 and 4.
            // A 16-entry palette retains its own sequential storage.
            let palette_index = if matches!(self.format, 48 | 66) {
                (index & !24) | ((index & 8) << 1) | ((index & 16) >> 1)
            } else {
                index
            };
            if color_bytes == 2 {
                // Format 0 is direct CT16: the texel itself, in this level.
                let packed = if self.format == 0 { u16le(bytes, skip + i * 2) } else { u16le(bytes, palette_index * 2) };
                rgba.extend_from_slice(&[
                    ((packed & 31) << 3) as u8,
                    (((packed >> 5) & 31) << 3) as u8,
                    (((packed >> 10) & 31) << 3) as u8,
                    if packed & 0x8000 != 0 { 255 } else { 0 },
                ]);
            } else {
                rgba.extend_from_slice(&bytes[palette_index * 4..palette_index * 4 + 4]);
            }
        }
        Ok(rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table(format: u8, texture_size: u32) -> Vec<u8> {
        let mut bytes = vec![0; 112];
        for (p, n) in [
            (0, 1u32),
            (4, 12),
            (8, 48),
            (48, 0xf00b0017),
            (68, texture_size),
        ] {
            bytes[p..p + 4].copy_from_slice(&n.to_le_bytes());
        }
        bytes[56] = format;
        for p in [44, 46, 60, 62] {
            bytes[p..p + 2].copy_from_slice(&8u16.to_le_bytes());
        }
        bytes
    }
    #[test]
    fn mip_levels_follow_the_base_and_share_the_palette() {
        // 8x8 format 66 with two mips: palette 1024, then 64 + 16 + 4 indices.
        let mut bytes = table(66, 1108);
        bytes[58] = 2;
        let t = TextureTable::parse(&bytes).unwrap();
        let mut tex = vec![0u8; 1108];
        for p in 0..256 {
            tex[p * 4..p * 4 + 4].copy_from_slice(&[p as u8, p as u8, p as u8, 0x80]);
        }
        tex[1024 + 64..1024 + 80].fill(5);
        tex[1024 + 80..].fill(9);
        let b = &t.bitmaps[0];
        assert_eq!(b.decode_level_rgba(&tex, 0).unwrap().len(), 64 * 4);
        assert_eq!(b.decode_level_rgba(&tex, 1).unwrap(), [5, 5, 5, 0x80].repeat(16));
        // Index 9 → CLUT entry 17 (bits 3 and 4 swapped).
        assert_eq!(b.decode_level_rgba(&tex, 2).unwrap(), [17, 17, 17, 0x80].repeat(4));
        assert!(b.decode_level_rgba(&tex, 3).is_err());
        assert!(b.decode_level_rgba(&tex[..1100], 2).is_err());
        // Direct CT16 (format 0) levels read their own texels.
        let mut bytes = table(0, 160);
        bytes[58] = 1;
        let t = TextureTable::parse(&bytes).unwrap();
        let mut tex = vec![0u8; 160];
        for p in (128..160).step_by(2) {
            tex[p..p + 2].copy_from_slice(&0x801fu16.to_le_bytes());
        }
        assert_eq!(t.bitmaps[0].decode_level_rgba(&tex, 1).unwrap(), [248, 0, 0, 255].repeat(16));
    }

    #[test]
    fn exact_palette_addressing_and_nibble_order_preserve_alpha() {
        let mut texture = vec![0; 1088];
        for i in 0..256 {
            texture[i * 4..i * 4 + 4].copy_from_slice(&[i as u8, 7, 9, 128]);
        }
        for i in 0..64 {
            texture[1024 + i] = i as u8;
        }
        let bytes = table(66, 1088);
        let bitmap = &TextureTable::parse(&bytes).unwrap().bitmaps[0];
        let decoded = bitmap.decode_base_rgba(&texture).unwrap();
        assert_eq!(&decoded[8 * 4..8 * 4 + 4], &[16, 7, 9, 128]);
        assert_eq!(&decoded[16 * 4..16 * 4 + 4], &[8, 7, 9, 128]);
        let bytes = table(34, 96);
        let bitmap = &TextureTable::parse(&bytes).unwrap().bitmaps[0];
        let mut texture = vec![0; 96];
        texture[4..8].copy_from_slice(&[1, 2, 3, 4]);
        texture[8..12].copy_from_slice(&[5, 6, 7, 8]);
        texture[64] = 0x21;
        assert_eq!(
            &bitmap.decode_base_rgba(&texture).unwrap()[..8],
            &[1, 2, 3, 4, 5, 6, 7, 8]
        );
    }
    #[test]
    fn truncated_overlapping_and_unsupported_inputs_are_explicit() {
        let bytes = table(66, 1088);
        for n in 0..112 {
            assert!(TextureTable::parse(&bytes[..n]).is_err());
        }
        let table = TextureTable::parse(&bytes).unwrap();
        assert!(table.bitmaps[0].decode_base_rgba(&[0; 1087]).is_err());
        let mut overlap = bytes.clone();
        overlap[8..12].copy_from_slice(&12u32.to_le_bytes());
        assert!(TextureTable::parse(&overlap).is_err());
        let mut bad_version = bytes.clone();
        bad_version[48] = 0;
        assert!(TextureTable::parse(&bad_version).is_err());
        let mut bad_dimension = bytes.clone();
        bad_dimension[60] = 7;
        assert!(TextureTable::parse(&bad_dimension).is_err());
        let mut unknown = bytes.clone();
        unknown[56] = 255;
        let table = TextureTable::parse(&unknown).unwrap();
        assert_eq!(table.bitmaps[0].raw_record[8], 255);
        assert!(table.bitmaps[0].decode_base_rgba(&[0; 1088]).is_err());
    }
    #[test]
    fn ct16_channel_shift_binary_selector_and_palette_addressing() {
        let bytes = table(48, 576);
        let bitmap = &TextureTable::parse(&bytes).unwrap().bitmaps[0];
        let mut pixels = vec![0; 576];
        pixels[16 * 2..16 * 2 + 2].copy_from_slice(&0x83ffu16.to_le_bytes());
        pixels[512] = 8;
        assert_eq!(
            &bitmap.decode_base_rgba(&pixels).unwrap()[..4],
            &[248, 248, 0, 255]
        );
        assert!(bitmap.decode_base_rgba(&pixels[..575]).is_err());
        let bytes = table(0, 128);
        let bitmap = &TextureTable::parse(&bytes).unwrap().bitmaps[0];
        let mut pixels = vec![0; 128];
        pixels[..2].copy_from_slice(&0x7c00u16.to_le_bytes());
        assert_eq!(
            &bitmap.decode_base_rgba(&pixels).unwrap()[..4],
            &[0, 0, 248, 0]
        );
        let bytes = table(16, 64);
        let bitmap = &TextureTable::parse(&bytes).unwrap().bitmaps[0];
        let mut pixels = vec![0; 64];
        pixels[2..4].copy_from_slice(&0x801fu16.to_le_bytes());
        pixels[4..6].copy_from_slice(&0x03e0u16.to_le_bytes());
        pixels[32] = 0x21;
        assert_eq!(
            &bitmap.decode_base_rgba(&pixels).unwrap()[..8],
            &[248, 0, 0, 255, 0, 248, 0, 0]
        );
    }
}
