//! Bounded v13 OBJECTS tables and diagnostic integer VIF geometry.
//!
//! Semantic scales/winding follow a locally surveyed encoding, qualified in
//! docs/research/geometry.md. Raw packets and attributes remain available.
//! Unsupported float geometry never silently becomes replacement geometry.
use crate::{binary::Reader, wad::ParseError};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ObjectDefinition {
    pub index: u32,
    pub label: String,
    pub object_index: i16,
    pub frames: i16,
    pub raw: Vec<u8>,
}
#[derive(Debug, Serialize)]
pub struct ObjectBitmap {
    pub index: u32,
    pub format: u8,
    pub flags: u16,
    pub width: u16,
    pub height: u16,
    pub texture_offset: u32,
    pub tex0: u64,
    pub raw: Vec<u8>,
}
#[derive(Debug, Serialize)]
pub struct Object {
    pub index: u32,
    pub radius: f32,
    pub flags: u32,
    pub declared_unique_vertices: i32,
    pub declared_triangles: i32,
    pub raw: Vec<u8>,
    pub parts: Vec<Part>,
}
#[derive(Debug, Serialize)]
pub struct Part {
    pub index: u32,
    pub texture_index: u16,
    pub lightmap_index: u16,
    pub lod_k: i16,
    pub file_offset: u32,
    pub raw_descriptor: Vec<u8>,
    pub raw_container: Vec<u8>,
}
#[derive(Debug, Serialize)]
pub struct Objects {
    pub raw_header: Vec<u8>,
    pub definitions: Vec<ObjectDefinition>,
    pub bitmap_records: Vec<ObjectBitmap>,
    pub objects: Vec<Object>,
}
impl Objects {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let r = Reader {
            bytes,
            family: "OBJECTS",
        };
        r.range(0, 160)?;
        if bytes.len() > 64 * 1024 * 1024 || r.u32(64) != 0xf00b000d {
            return Err(r.error(64, "unsupported version or file size policy"));
        }
        let objects = r.table(r.u32(84), r.u32(68), 64, 160)?;
        let bitmaps = r.table(r.u32(88), r.u32(72), 64, 160)?;
        let defs = r.table(r.u32(92), r.u32(76), 24, 160)?;
        let bitmap_defs = r.table(r.u32(96), r.u32(80), 36, 160)?;
        let extras = r.range(
            r.u32(100) as usize,
            (r.u32(104) as usize)
                .checked_sub(r.u32(100) as usize)
                .ok_or_else(|| r.error(100, "reversed descriptor range"))?,
        )?;
        let ranges = [&objects, &bitmaps, &defs, &bitmap_defs, &extras];
        for (i, a) in ranges.iter().enumerate() {
            for b in &ranges[i + 1..] {
                if !a.is_empty() && !b.is_empty() && a.start < b.end && b.start < a.end {
                    return Err(r.error(a.start, "metadata tables overlap"));
                }
            }
        }
        if r.u32(108) as usize != bytes.len() {
            return Err(r.error(108, "declared object file end differs"));
        }
        let mut definitions = Vec::new();
        for (index, p) in (defs.start..defs.end).step_by(24).enumerate() {
            let object_index = r.i16(p + 20);
            if object_index < 0 || object_index as u32 >= r.u32(68) {
                return Err(r.error(p + 20, "definition object index out of range"));
            }
            definitions.push(ObjectDefinition {
                index: index as u32,
                label: r.label(p, 16),
                object_index,
                frames: r.i16(p + 22),
                raw: bytes[p..p + 24].to_vec(),
            });
        }
        let mut bitmap_records = Vec::new();
        for (index, p) in (bitmaps.start..bitmaps.end).step_by(64).enumerate() {
            bitmap_records.push(ObjectBitmap {
                index: index as u32,
                format: bytes[p],
                flags: r.u16(p + 8),
                texture_offset: r.u32(p + 12),
                width: r.u16(p + 22),
                height: r.u16(p + 24),
                tex0: u64::from_le_bytes(bytes[p + 32..p + 40].try_into().unwrap()),
                raw: bytes[p..p + 64].to_vec(),
            });
        }
        let mut results = Vec::new();
        let mut total_parts = 0usize;
        let mut packet_bytes = 0usize;
        let mut packet_ranges = Vec::new();
        for (index, p) in (objects.start..objects.end).step_by(64).enumerate() {
            let count = r.i32(p + 12);
            if !(0..=4096).contains(&count) {
                return Err(r.error(p + 12, "invalid part count"));
            }
            total_parts += count as usize;
            if total_parts > 100_000 {
                return Err(r.error(p + 12, "total part count policy"));
            }
            let mut descriptors = Vec::new();
            if count > 0 {
                descriptors.push(p + 16);
            }
            if count > 1 {
                let extra = r.range(r.u32(p + 24) as usize, (count as usize - 1) * 8)?;
                if extra.start < extras.start || extra.end > extras.end {
                    return Err(r.error(p + 24, "additional descriptors exceed declared table"));
                }
                descriptors.extend((extra.start..extra.end).step_by(8));
            }
            let mut cursor = r.u32(p + 28) as usize;
            let mut parts = Vec::new();
            for (part_index, d) in descriptors.into_iter().enumerate() {
                r.range(cursor, 8)?;
                if !cursor.is_multiple_of(16) {
                    return Err(r.error(cursor, "unaligned packet container"));
                }
                let length = (r.u16(cursor) as usize + 1) * 16;
                packet_bytes += length;
                if packet_bytes > 128 * 1024 * 1024 {
                    return Err(r.error(cursor, "total retained packet byte policy exceeded"));
                }
                if r.u16(d) as usize * 16 != length {
                    return Err(r.error(d, "descriptor/container qword counts disagree"));
                }
                let range = r.range(cursor, length)?;
                if range.start < 160
                    || ranges
                        .iter()
                        .any(|a| !a.is_empty() && range.start < a.end && a.start < range.end)
                {
                    return Err(r.error(cursor, "packet intersects header/metadata"));
                }
                for at in [d + 2, d + 4] {
                    let value = r.u16(at);
                    if value != u16::MAX && value as usize >= bitmap_records.len() {
                        return Err(r.error(at, "part bitmap index out of range"));
                    }
                }
                parts.push(Part {
                    index: part_index as u32,
                    texture_index: r.u16(d + 2),
                    lightmap_index: r.u16(d + 4),
                    lod_k: r.i16(d + 6),
                    file_offset: cursor as u32,
                    raw_descriptor: bytes[d..d + 8].to_vec(),
                    raw_container: bytes[range.clone()].to_vec(),
                });
                packet_ranges.push(range);
                cursor += length;
            }
            results.push(Object {
                index: index as u32,
                radius: r.float(p + 4)?,
                flags: r.u32(p + 8),
                declared_unique_vertices: r.i32(p + 32),
                declared_triangles: r.i32(p + 36),
                raw: bytes[p..p + 64].to_vec(),
                parts,
            });
        }
        packet_ranges.sort_by_key(|a| (a.start, a.end));
        for pair in packet_ranges.windows(2) {
            // Exact aliases may be shared geometry. Partial overlaps are corrupt.
            if pair[0].end > pair[1].start && pair[0] != pair[1] {
                return Err(r.error(pair[1].start, "packet containers partially overlap"));
            }
        }
        Ok(Self {
            raw_header: bytes[..160].to_vec(),
            definitions,
            bitmap_records,
            objects: results,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct Strip {
    pub raw_geometry: Vec<u8>,
    pub raw_position_words: Vec<[i32; 3]>,
    pub raw_position_sentinel: [i32; 3],
    pub packed_normals: Vec<u16>,
    pub packed_colors: Vec<u16>,
    pub raw_uvs: Vec<Vec<u32>>,
    pub direction: f32,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub uvs: Vec<[f32; 2]>,
    pub lightmap_uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}
#[derive(Debug, Serialize)]
pub struct Geometry {
    pub strips: Vec<Strip>,
}
impl ObjectBitmap {
    /// Format 146 (lightmap) texels: 4-bit indices, two per byte, low nibble
    /// first, no inline palette. The G1 GS dump uploads these bytes unchanged
    /// as PSMT4 (all 12 G1 lightmaps byte-identical) with a shared CLUT; the
    /// CLUT is the consumer's, not part of this record.
    pub fn decode_lightmap_indices(&self, textures: &[u8]) -> Result<Vec<u8>, ParseError> {
        let r = Reader {
            bytes: textures,
            family: "TEXTURES",
        };
        if self.format != 146 {
            return Err(r.error(self.texture_offset as usize, "not a format 146 bitmap"));
        }
        let count = self.width as usize * self.height as usize;
        let range = r.range(self.texture_offset as usize, count.div_ceil(2))?;
        let bytes = &textures[range];
        Ok((0..count).map(|i| (bytes[i / 2] >> ((i % 2) * 4)) & 15).collect())
    }
}

impl Part {
    pub fn decode_integer_geometry(&self) -> Result<Geometry, ParseError> {
        let r = Reader {
            bytes: &self.raw_container,
            family: "VIF",
        };
        r.range(0, 16)?;
        let mut cursor = 8usize;
        let mut strips = Vec::new();
        let mut current: Option<Strip> = None;
        while cursor < r.bytes.len() {
            cursor = (cursor + 3) & !3;
            if cursor == r.bytes.len() {
                break;
            }
            r.range(cursor, 4)?;
            let header = cursor;
            let kind = r.bytes[cursor];
            let count = r.bytes[cursor + 2] as usize;
            let storage = r.bytes[cursor + 3];
            cursor += 4;
            let stride = match (kind, storage) {
                (0, 0 | 0x14 | 0x17) | (4, 1) => 0,
                (0, 0x6c) => 16,
                (1, 0x68) => 12,
                (1, 0x69) => 6,
                (1, 0x6a) => 3,
                (2 | 3, 0x6f) => 2,
                (4, 0x64 | 0x6d) => 8,
                (4, 0x65) => 4,
                (4, 0x66) => 2,
                _ => return Err(r.error(header, format!("unsupported stream {kind}/{storage:#x}"))),
            };
            r.range(cursor, count * stride)?;
            if kind == 0 && storage == 0x6c {
                if count != 1 {
                    return Err(r.error(header, "geometry header count differs from one"));
                }
                if let Some(strip) = current.take() {
                    strips.push(finish_strip(strip, &r, header)?);
                }
                current = Some(Strip {
                    raw_geometry: r.bytes[header..cursor + 16].to_vec(),
                    direction: r.float(cursor + 8)?,
                    raw_position_words: Vec::new(),
                    raw_position_sentinel: [0; 3],
                    packed_normals: Vec::new(),
                    packed_colors: Vec::new(),
                    raw_uvs: Vec::new(),
                    positions: Vec::new(),
                    normals: Vec::new(),
                    colors: Vec::new(),
                    uvs: Vec::new(),
                    lightmap_uvs: Vec::new(),
                    indices: Vec::new(),
                });
            } else if stride != 0 {
                let strip = current
                    .as_mut()
                    .ok_or_else(|| r.error(header, "attribute without geometry header"))?;
                match kind {
                    1 => {
                        if !strip.positions.is_empty() || count < 4 {
                            return Err(r.error(header, "duplicate or short positions"));
                        }
                        for i in 0..count {
                            let p = cursor + i * stride;
                            let mut word = [0; 3];
                            for (axis, w) in word.iter_mut().enumerate() {
                                *w = match storage {
                                    0x68 => r.i32(p + axis * 4),
                                    0x69 => r.i16(p + axis * 2) as i32,
                                    _ => r.bytes[p + axis] as i8 as i32,
                                };
                            }
                            if i + 1 == count {
                                strip.raw_position_sentinel = word;
                            } else {
                                strip.raw_position_words.push(word);
                                strip.positions.push(word.map(|v| v as f32 / 128.0));
                            }
                        }
                    }
                    2 | 3 => {
                        let values = if kind == 2 {
                            &mut strip.packed_normals
                        } else {
                            &mut strip.packed_colors
                        };
                        if !values.is_empty() {
                            return Err(r.error(header, "duplicate packed attribute"));
                        }
                        values.extend((0..count).map(|i| r.u16(cursor + i * 2)));
                    }
                    4 => {
                        if !strip.uvs.is_empty() {
                            return Err(r.error(header, "duplicate UV stream"));
                        }
                        for i in 0..count {
                            let p = cursor + i * stride;
                            let components = if storage == 0x6d { 4 } else { 2 };
                            let mut raw = Vec::new();
                            for axis in 0..components {
                                raw.push(match storage {
                                    0x64 => r.u32(p + axis * 4),
                                    0x66 => r.bytes[p + axis] as u32,
                                    _ => r.u16(p + axis * 2) as u32,
                                });
                            }
                            strip
                                .uvs
                                .push([raw[0] as f32 / 128.0, raw[1] as f32 / 128.0]);
                            if components == 4 {
                                strip
                                    .lightmap_uvs
                                    .push([raw[2] as f32 / 32768.0, raw[3] as f32 / 32768.0]);
                            }
                            strip.raw_uvs.push(raw);
                        }
                    }
                    _ => unreachable!(),
                }
            }
            cursor += count * stride;
        }
        if let Some(strip) = current {
            strips.push(finish_strip(strip, &r, cursor)?);
        }
        Ok(Geometry { strips })
    }
}
fn finish_strip(mut s: Strip, r: &Reader<'_>, at: usize) -> Result<Strip, ParseError> {
    let n = s.positions.len();
    if n < 3
        || s.packed_normals.len() != n
        || s.uvs.len() != n
        || (!s.packed_colors.is_empty() && s.packed_colors.len() != n)
    {
        return Err(r.error(
            at,
            format!(
                "attribute counts differ: pos={n}, normal={}, color={}, uv={}",
                s.packed_normals.len(),
                s.packed_colors.len(),
                s.uvs.len()
            ),
        ));
    }
    if s.direction != 1.0 && s.direction != -1.0 {
        return Err(r.error(at, "unknown strip direction"));
    }
    for &packed in &s.packed_normals {
        let v = [
            (packed & 31) as f32 / 15.0 - 1.0,
            ((packed >> 5) & 31) as f32 / 15.0 - 1.0,
            ((packed >> 10) & 31) as f32 / 15.0 - 1.0,
        ];
        let length = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        s.normals.push(v.map(|x| x / length));
    }
    for &p in &s.packed_colors {
        s.colors.push([
            (p & 31) as f32 / 31.0,
            ((p >> 5) & 31) as f32 / 31.0,
            ((p >> 10) & 31) as f32 / 31.0,
            ((p >> 15) & 1) as f32,
        ]);
    }
    for i in 2..n {
        if s.packed_normals[i] & 0x8000 != 0 {
            continue;
        }
        let a = (i - 2) as u32;
        let b = (i - 1) as u32;
        let c = i as u32;
        let flip = s.direction == -1.0;
        let triangle = if ((i - 2 + usize::from(flip)) & 1) == 1 {
            [a, b, c]
        } else {
            [b, a, c]
        };
        s.indices.extend_from_slice(&triangle);
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn packet() -> Part {
        let mut b = vec![0; 8];
        b.extend([0, 0, 1, 0x6c]);
        b.extend(4u32.to_le_bytes());
        b.extend([0, 0, 0, 0x2d]);
        b.extend((-1.0f32).to_le_bytes());
        b.extend((-1.0f32).to_le_bytes());
        b.extend([1, 0, 5, 0x69]);
        for v in [
            [0i16, 0, 0],
            [128, 0, 0],
            [0, 128, 0],
            [128, 128, 0],
            [999, 999, 999],
        ] {
            for x in v {
                b.extend(x.to_le_bytes());
            }
        }
        while !b.len().is_multiple_of(4) {
            b.push(0);
        }
        b.extend([2, 0, 4, 0x6f]);
        for n in [0xc1efu16, 0xc1ef, 0x41ef, 0xc1ef] {
            b.extend(n.to_le_bytes());
        }
        b.extend([4, 0, 4, 0x65]);
        for uv in [[0u16, 0], [128, 0], [0, 128], [128, 128]] {
            for x in uv {
                b.extend(x.to_le_bytes());
            }
        }
        b.extend([0, 0, 0, 0x14]);
        while !b.len().is_multiple_of(16) {
            b.extend([0; 4]);
        }
        Part {
            index: 0,
            texture_index: 0,
            lightmap_index: 0,
            lod_k: 0,
            file_offset: 0,
            raw_descriptor: vec![],
            raw_container: b,
        }
    }
    #[test]
    fn lightmap_indices_low_nibble_first() {
        let bitmap = ObjectBitmap {
            index: 0,
            format: 146,
            flags: 0,
            width: 2,
            height: 2,
            texture_offset: 1,
            tex0: 0,
            raw: Vec::new(),
        };
        assert_eq!(bitmap.decode_lightmap_indices(&[0xff, 0x21, 0x43]).unwrap(), vec![1, 2, 3, 4]);
        assert!(bitmap.decode_lightmap_indices(&[0xff, 0x21]).is_err());
        let base = ObjectBitmap { format: 66, ..bitmap };
        assert!(base.decode_lightmap_indices(&[0xff, 0x21, 0x43]).is_err());
    }
    #[test]
    fn strip_sentinel_skip_bit_scale_and_winding_are_explicit() {
        let g = packet().decode_integer_geometry().unwrap();
        let s = &g.strips[0];
        assert_eq!(
            s.positions,
            vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [1.0, 1.0, 0.0]
            ]
        );
        assert_eq!(s.raw_position_sentinel, [999; 3]);
        assert_eq!(s.indices, [0, 1, 2]);
        assert_eq!(s.uvs[3], [1.0, 1.0]);
        assert!(s.colors.is_empty());
        let mut p = packet();
        p.raw_container[20..24].copy_from_slice(&1.0f32.to_le_bytes());
        assert_eq!(
            p.decode_integer_geometry().unwrap().strips[0].indices,
            [1, 0, 2]
        );
    }
    #[test]
    fn malformed_packets_never_yield_partial_semantic_geometry() {
        let valid = packet();
        // Every truncation through the UV data must fail, including misalignment.
        for end in 0..92 {
            let mut p = packet();
            p.raw_container.truncate(end);
            assert!(p.decode_integer_geometry().is_err(), "prefix {end}");
        }
        let mut p = packet();
        p.raw_container[31] = 0xff;
        assert!(p.decode_integer_geometry().is_err());
        let mut p = packet();
        p.raw_container[20..24].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(p.decode_integer_geometry().is_err());
        assert!(valid.decode_integer_geometry().is_ok());
    }
    #[test]
    fn table_ranges_and_count_budgets_precede_field_reads() {
        let mut b = vec![0; 160];
        b[64..68].copy_from_slice(&0xf00b000du32.to_le_bytes());
        b[108..112].copy_from_slice(&160u32.to_le_bytes());
        assert!(Objects::parse(&b).is_ok());
        for end in 0..160 {
            assert!(Objects::parse(&b[..end]).is_err());
        }
        b[68..72].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Objects::parse(&b).is_err());
        b[68..72].copy_from_slice(&1u32.to_le_bytes());
        b[84..88].copy_from_slice(&160u32.to_le_bytes());
        assert!(Objects::parse(&b).is_err());
    }
}
