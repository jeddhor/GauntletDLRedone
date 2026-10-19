//! Per-world gameplay WAD (wdata/*.wad): MB container (MBSetupWad 0x1d3f38 /
//! MBGetFromWad 0x1d4008) with tagged chunks consumed by LoadWorldData
//! 0x161b98: LEVL (0x10c-byte level records), ENMY (24-byte enemy entries) and
//! SNDS (world sounds), AUDS (stage audio), CAMS (camera parameters, 0x6c
//! bytes; LEVL +0x58 selects one, ResolveWorldData 0x161738 points +0x60 at
//! it); MAPS/BCAM/WRLD retained raw until their consumers are recovered.
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
    /// +0x58: CAMS index (ResolveWorldData clamps a negative one to 0).
    pub camera_index: i16,
    /// +0x8c: BCAM index (negative: no boss camera; ResolveWorldData +0x6c).
    pub boss_camera_index: i16,
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

/// SNDS world sound (0x18 bytes): name, id (−1 until AudioFindSound),
/// volume and priority words (PlayWorldSound 0x15c4f0).
#[derive(Debug, Clone, Serialize)]
pub struct WorldSound {
    pub name: String,
    pub id: i32,
    pub volume: i16,
    pub priority: i16,
    pub raw: Vec<u8>,
}

/// AUDS stage record (0x3c bytes): bank name, enter sound, music stream,
/// areas and parts per area (AudioLoadStage 0x15caf8, BGPlayStream 0x15c7a0).
#[derive(Debug, Clone, Serialize)]
pub struct StageAudio {
    pub bank: String,
    pub word_10: i16,
    pub enter_sound: i16,
    pub word_14: i32,
    pub stream: String,
    pub areas: i16,
    pub word_2a: i16,
    pub parts: Vec<i16>,
    pub raw: Vec<u8>,
}

/// CAMS record (0x6c bytes) fields with known consumers (StdCam module
/// 0x113358.., init_game_cam 0x16fec8, InitCamera 0x167748); all bytes in `raw`.
#[derive(Debug, Clone, Serialize)]
pub struct CameraParams {
    /// +0x08: TransGetYawPitch's multi-player pitch limit (negated).
    pub pitch_08: f32,
    /// +0x0c / +0x18: CamGetPlayerAvgPos clamp box (init_game_cam derives it
    /// from the world bounds when the +0x24 byte is 0).
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub bounds_set: u8,
    /// +0x2c: single-player distance (CalcDist); +0x30: radius (CamInit,
    /// UpdateCam × ADJUST_MAX_RAD).
    pub dist_2c: f32,
    pub radius_30: f32,
    /// +0x34: NumEnemies (init_game_cam).
    pub num_enemies_34: i16,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldData {
    pub chunks: Vec<Chunk>,
    pub levels: Vec<LevelRecord>,
    pub enemies: Vec<EnemyEntry>,
    pub sounds: Vec<WorldSound>,
    pub stages: Vec<StageAudio>,
    pub cameras: Vec<CameraParams>,
    /// BCAM boss-camera records (0x54 bytes; BossCameraStart 0x19fe68 and the
    /// BossCam calculators read and rewrite them), kept raw.
    pub boss_cameras: Vec<Vec<u8>>,
    /// world_data +4 (WRLD chunk +4, 16 bytes): the realm's model directory
    /// name, e.g. "levelG" (GetCritterDesc, LevelEnemyDesc).
    pub world_dir: Option<String>,
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
                    camera_index: r.i16(p + 0x58).max(0),
                    boss_camera_index: r.i16(p + 0x8c),
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
        let mut sounds = Vec::new();
        if let Some(c) = find("SNDS") {
            let range = r.table(c.offset, c.count, 0x18, 8)?;
            for p in range.step_by(0x18) {
                sounds.push(WorldSound {
                    name: r.label(p, 16),
                    id: r.i32(p + 0x10),
                    volume: r.i16(p + 0x14),
                    priority: r.i16(p + 0x16),
                    raw: bytes[p..p + 0x18].to_vec(),
                });
            }
        }
        let mut stages = Vec::new();
        if let Some(c) = find("AUDS") {
            let range = r.table(c.offset, c.count, 0x3c, 8)?;
            for p in range.step_by(0x3c) {
                stages.push(StageAudio {
                    bank: r.label(p, 16),
                    word_10: r.i16(p + 0x10),
                    enter_sound: r.i16(p + 0x12),
                    word_14: r.i32(p + 0x14),
                    stream: r.label(p + 0x18, 16),
                    areas: r.i16(p + 0x28),
                    word_2a: r.i16(p + 0x2a),
                    parts: (0..8).map(|k| r.i16(p + 0x2c + 2 * k)).collect(),
                    raw: bytes[p..p + 0x3c].to_vec(),
                });
            }
        }
        let mut cameras = Vec::new();
        if let Some(c) = find("CAMS") {
            let range = r.table(c.offset, c.count, 0x6c, 8)?;
            for p in range.step_by(0x6c) {
                let v3 = |o: usize| -> Result<[f32; 3], ParseError> { Ok([r.float(o)?, r.float(o + 4)?, r.float(o + 8)?]) };
                cameras.push(CameraParams {
                    pitch_08: r.float(p + 8)?,
                    min: v3(p + 0xc)?,
                    max: v3(p + 0x18)?,
                    bounds_set: bytes[p + 0x24],
                    dist_2c: r.float(p + 0x2c)?,
                    radius_30: r.float(p + 0x30)?,
                    num_enemies_34: r.i16(p + 0x34),
                    raw: bytes[p..p + 0x6c].to_vec(),
                });
            }
        }
        let mut boss_cameras = Vec::new();
        if let Some(c) = find("BCAM") {
            let range = r.table(c.offset, c.count, 0x54, 8)?;
            for p in range.step_by(0x54) {
                boss_cameras.push(bytes[p..p + 0x54].to_vec());
            }
        }
        for level in &levels {
            if 0 <= level.boss_camera_index && boss_cameras.len() <= level.boss_camera_index as usize {
                return Err(r.error(0, "level boss camera index outside BCAM"));
            }
            let k = i16::from_le_bytes([level.raw[0x58], level.raw[0x59]]);
            if (k.max(0) as usize) >= cameras.len().max(1) && !cameras.is_empty() {
                return Err(r.error(0, "level camera index outside CAMS"));
            }
        }
        let world_dir = match find("WRLD") {
            Some(c) => {
                r.range(c.offset as usize + 4, 16)?;
                Some(r.label(c.offset as usize + 4, 16))
            }
            None => None,
        };
        Ok(Self { chunks, levels, enemies, sounds, stages, cameras, boss_cameras, world_dir })
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
