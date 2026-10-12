//! Observed TEXDEF.PS2 v23 metadata and candidate base-level RGBA decoding.
//!
//! Color layout is locally plausible, but source alpha remains unmodified.
//! Material/GS blending and mipmap parity are separate fidelity gates.
use crate::wad::ParseError;
use serde::Serialize;

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
    /// Decode only observed 32-bit color families. Preserve source alpha bytes.
    /// IDs 0/16/48/130/146 deliberately await further format evidence.
    pub fn decode_base_rgba(&self, textures: &[u8]) -> Result<Vec<u8>, ParseError> {
        let count = self.width as usize * self.height as usize;
        let (palette_bytes, pixels_bytes) = match self.format {
            2 => (0usize, count * 4),
            34 => (64, count.div_ceil(2)),
            66 => (1024, count),
            _ => {
                return Err(fail(
                    self.texture_offset as usize,
                    Some(self.index),
                    "unsupported pixel format",
                ));
            }
        };
        let start = self.texture_offset as usize;
        let end = start
            .checked_add(self.texture_size as usize)
            .ok_or_else(|| fail(start, Some(self.index), "texture range overflow"))?;
        let bytes = textures
            .get(start..end)
            .ok_or_else(|| fail(start, Some(self.index), "texture exceeds payload"))?;
        if palette_bytes + pixels_bytes > bytes.len() {
            return Err(fail(
                start,
                Some(self.index),
                "truncated base palette/pixels",
            ));
        }
        if self.format == 2 {
            return Ok(bytes[..pixels_bytes].to_vec());
        }
        let indices = &bytes[palette_bytes..palette_bytes + pixels_bytes];
        let mut rgba = Vec::with_capacity(count * 4);
        for i in 0..count {
            let index = if self.format == 34 {
                (indices[i / 2] >> ((i % 2) * 4)) & 15
            } else {
                indices[i]
            } as usize;
            // The 256-entry GS CSM1 CLUT swaps address bits 3 and 4.
            // A 16-entry palette retains its own sequential storage.
            let palette_index = if self.format == 66 {
                (index & !24) | ((index & 8) << 1) | ((index & 16) >> 1)
            } else {
                index
            };
            rgba.extend_from_slice(&bytes[palette_index * 4..palette_index * 4 + 4]);
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
}
