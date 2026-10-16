//! Level setup transcribed from LoadWorldData 0x161b98 / GetEnemyTypes 0x1628a0.
use gdl_formats::wdata::{LevelRecord, WorldData};
use serde::Serialize;

/// GetEnemySubtype 0x162830: jump table at EE 0x2a92d0; each case returns a
/// literal (`b end; li a1, K`); types ≥ 45 return 0.
pub fn enemy_subtype(kind: i32) -> i32 {
    const TARGET: [u8; 45] = [
        0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 2, 2, 0, 1, 2, 0, 0, 1, 2, 2, 8, 2, 8, 3, 4, 5, 6, 3, 7, 7,
        7, 7, 7, 7, 7, 7, 7, 7, 7,
    ];
    const VALUE: [i32; 9] = [1, 3, 4, 5, 6, 7, 8, 9, 0];
    usize::try_from(kind).ok().and_then(|k| TARGET.get(k)).map_or(0, |&t| VALUE[t as usize])
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct EnemyTypes {
    pub kind: [i32; 8],
    pub subtype: [i32; 8],
    /// ENMY entry index per slot (EnemyData pointer in the original).
    pub data: [Option<usize>; 8],
    pub gen_kind: [i32; 8],
}

/// GetEnemyTypes for `level`; `world_kind` is world_data[0] (0xd = tower);
/// `mode_950` is the global at EE 0x20f950 (the trailing pass runs below 2).
pub fn enemy_types(world: &WorldData, level: &LevelRecord, world_kind: i32, mode_950: i32) -> Result<EnemyTypes, String> {
    let mut t = EnemyTypes { kind: [-1; 8], subtype: [0; 8], data: [None; 8], gen_kind: [-1; 8] };
    let mut death_seen = false;
    for i in 0..8 {
        let index = if i < 6 { level.enemy_indices[i] } else { -1 };
        if index < 0 {
            t.kind[i] = if level.boss_type < 0 && world_kind != 0xd && !death_seen { 0x1e } else { -1 };
        } else {
            let e = world.enemies.get(index as usize).ok_or("enemy index outside ENMY")?;
            t.kind[i] = e.kind;
            t.subtype[i] = e.subtype;
            t.data[i] = Some(index as usize);
        }
        if t.kind[i] == 0x1e {
            death_seen = true;
        }
        if t.kind[i] >= 0 {
            if t.subtype[i] == 0 {
                t.subtype[i] = enemy_subtype(t.kind[i]);
            }
            if t.subtype[i] < 1 {
                return Err(format!("enemy type {} has no subtype", t.kind[i]));
            }
        }
    }
    for i in 0..8 {
        if t.subtype[i] < 6 {
            t.gen_kind[t.subtype[i] as usize] = t.kind[i];
        }
    }
    if mode_950 < 2 {
        for i in 0..8 {
            if t.subtype[i] == 2 {
                t.subtype[i] = 4;
            } else {
                t.kind[i] = -1;
            }
        }
    }
    Ok(t)
}
