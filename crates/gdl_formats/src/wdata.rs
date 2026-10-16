//! Per-world gameplay WAD (wdata/*.wad): MB container (MBSetupWad 0x1d3f38 /
//! MBGetFromWad 0x1d4008) with tagged chunks consumed by LoadWorldData
//! 0x161b98: LEVL (0x10c-byte level records), ENMY (24-byte enemy entries) and
//! CAMS/AUDS/SNDS/MAPS/BCAM/WRLD retained raw until their consumers are recovered.
use crate::{binary::Reader, wad::ParseError};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Chunk {
    pub tag: String,
    pub offset: u32,
    pub count: u32,
    pub word_0c: u32,
}

/// LEVL record fields with known consumers; everything is retained in `raw`.
#[derive(Debug, Clone, Serialize)]
pub struct LevelRecord {
    pub index: u32,
    pub flags_00: u32,
    pub word_04: u16,
    pub code: String,
    pub boss_type: i32,
    /// +0x4c: six i16 ENMY indices (GetEnemyTypes 0x1628a0).
    pub enemy_indices: [i16; 6],
    /// +0xcc..+0xdc: generator/item multipliers read by SetItem 0x151878.
    pub multipliers_cc: [f32; 5],
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnemyEntry {
    pub index: u32,
    pub kind: i32,
    pub subtype: i32,
    pub name: String,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldData {
    pub chunks: Vec<Chunk>,
    pub levels: Vec<LevelRecord>,
    pub enemies: Vec<EnemyEntry>,
}

impl WorldData {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let r = Reader { bytes, family: "WDATA" };
        r.range(0, 8)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(r.error(0, "world data size policy exceeded"));
        }
        let (directory, count) = (r.u32(0) as usize, r.u32(4));
        if count > 64 {
            return Err(r.error(4, "chunk count policy exceeded"));
        }
        let mut chunks = Vec::new();
        for i in 0..count as usize {
            let p = directory + 16 * i;
            r.range(p, 16)?;
            let tag = r.u32(p).to_be_bytes();
            chunks.push(Chunk {
                tag: String::from_utf8_lossy(&tag).into_owned(),
                offset: r.u32(p + 4),
                count: r.u32(p + 8),
                word_0c: r.u32(p + 12),
            });
        }
        let find = |tag: &str| chunks.iter().find(|c| c.tag == tag);
        let mut levels = Vec::new();
        if let Some(c) = find("LEVL") {
            let range = r.table(c.offset, c.count, 0x10c, 8)?;
            for (index, p) in range.step_by(0x10c).enumerate() {
                levels.push(LevelRecord {
                    index: index as u32,
                    flags_00: r.u32(p),
                    word_04: r.u16(p + 4),
                    code: r.label(p + 8, 4),
                    boss_type: r.i32(p + 0x44),
                    enemy_indices: std::array::from_fn(|i| r.i16(p + 0x4c + 2 * i)),
                    multipliers_cc: [
                        r.float(p + 0xcc)?,
                        r.float(p + 0xd0)?,
                        r.float(p + 0xd4)?,
                        r.float(p + 0xd8)?,
                        r.float(p + 0xdc)?,
                    ],
                    raw: bytes[p..p + 0x10c].to_vec(),
                });
            }
        }
        let mut enemies = Vec::new();
        if let Some(c) = find("ENMY") {
            let range = r.table(c.offset, c.count, 24, 8)?;
            for (index, p) in range.step_by(24).enumerate() {
                enemies.push(EnemyEntry {
                    index: index as u32,
                    kind: r.i32(p),
                    subtype: r.i32(p + 4),
                    name: r.label(p + 8, 8),
                    raw: bytes[p..p + 24].to_vec(),
                });
            }
        }
        for level in &levels {
            if level.enemy_indices.iter().any(|&i| i >= 0 && i as usize >= enemies.len()) {
                return Err(r.error(0, "level enemy index outside ENMY"));
            }
        }
        Ok(Self { chunks, levels, enemies })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn container_chunks_and_bounds() {
        // Directory at 0x40 with an ENMY chunk of one entry at 0x10.
        let mut b = vec![0u8; 0x50];
        b[0..4].copy_from_slice(&0x40u32.to_le_bytes());
        b[4..8].copy_from_slice(&1u32.to_le_bytes());
        b[0x40..0x44].copy_from_slice(&u32::from_be_bytes(*b"ENMY").to_le_bytes());
        b[0x44..0x48].copy_from_slice(&0x10u32.to_le_bytes());
        b[0x48..0x4c].copy_from_slice(&1u32.to_le_bytes());
        b[0x10..0x14].copy_from_slice(&13i32.to_le_bytes());
        b[0x18..0x1b].copy_from_slice(b"ZOM");
        let w = WorldData::parse(&b).unwrap();
        assert_eq!((w.enemies[0].kind, w.enemies[0].name.as_str()), (13, "ZOM"));
        assert!(WorldData::parse(&b[..0x48]).is_err());
    }
}
