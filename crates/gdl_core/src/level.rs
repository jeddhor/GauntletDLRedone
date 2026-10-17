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

/// ResolveWorldData 0x161738, per LEVL record: clears flag bit 0 and, once
/// (+6 = 0 → 1), fills zero multipliers +0xac..+0xdc with +0xa8 (itself and
/// +0xc4 default 1.0), multiplies +0xa8/+0xb0/+0xb4/+0xd0/+0xd4/+0xdc by the
/// difficulty factor (diff_mul_tab.15 0x23e168 [0x21edcc]) and stores
/// +0xb8/+0xc8/+0xc0/+0xd8 as 1 / (factor × value). Pointer fields (+0x60..)
/// and the level-name sound are not part of the native record.
pub fn resolve_level_record(raw: &mut [u8], factor: f32) -> Result<(), crate::numeric::UnsupportedNumericDomain> {
    use crate::numeric::{div_rn, mul_rz};
    let f = |r: &[u8], o: usize| f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
    let set = |r: &mut [u8], o: usize, v: f32| r[o..o + 4].copy_from_slice(&v.to_le_bytes());
    let flags = u32::from_le_bytes(raw[0..4].try_into().unwrap()) & !1;
    raw[0..4].copy_from_slice(&flags.to_le_bytes());
    if u16::from_le_bytes([raw[6], raw[7]]) != 0 {
        return Ok(());
    }
    raw[6..8].copy_from_slice(&1u16.to_le_bytes());
    if f(raw, 0xa8) == 0.0 {
        set(raw, 0xa8, 1.0);
    }
    let base = f(raw, 0xa8);
    for o in (0xac..=0xdc).step_by(4) {
        if f(raw, o) == 0.0 {
            set(raw, o, if o == 0xc4 { 1.0 } else { base });
        }
    }
    for o in [0xa8, 0xb0, 0xb4, 0xd0, 0xd4, 0xdc] {
        let v = mul_rz(factor, f(raw, o))?;
        set(raw, o, v);
    }
    for o in [0xb8, 0xc8, 0xc0, 0xd8] {
        let v = div_rn(1.0, mul_rz(factor, f(raw, o))?)?;
        set(raw, o, v);
    }
    Ok(())
}

/// The runtime copy of `level` after ResolveWorldData at `difficulty`
/// (the 0x21edcc word indexing diff_mul_tab.15, read from the ELF).
pub fn resolved_level(level: &LevelRecord, elf: &gdl_formats::elf::Elf, difficulty: i32) -> Result<Vec<u8>, String> {
    let tab = elf.symbol("diff_mul_tab.15").ok_or("diff_mul_tab.15 symbol")?;
    let k = u32::try_from(difficulty).map_err(|_| "difficulty index")?;
    let w = elf.read(tab.address + 4 * k, 4).map_err(|e| e.to_string())?;
    let mut raw = level.raw.clone();
    resolve_level_record(&mut raw, f32::from_le_bytes(w.try_into().unwrap())).map_err(|e| e.to_string())?;
    Ok(raw)
}

/// LEVL +0xcc..+0xdc of a (resolved) record: SetItem's multipliers.
pub fn multipliers_cc(raw: &[u8]) -> [f32; 5] {
    std::array::from_fn(|k| f32::from_le_bytes(raw[0xcc + 4 * k..0xd0 + 4 * k].try_into().unwrap()))
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

#[cfg(test)]
mod resolve_tests {
    #[test]
    fn resolve_level_record_defaults_and_reciprocals() {
        // Synthetic record: +0xa8 = 1, +0xb8 = 0.5, other multipliers 0;
        // factor 1.0 → zeros take +0xa8, +0xb8 becomes 2.0, applied once.
        let mut r = vec![0u8; 0x10c];
        r[0] = 1;
        r[0xa8..0xac].copy_from_slice(&1.0f32.to_le_bytes());
        r[0xb8..0xbc].copy_from_slice(&0.5f32.to_le_bytes());
        super::resolve_level_record(&mut r, 1.0).unwrap();
        let f = |r: &[u8], o: usize| f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
        assert_eq!(r[0], 0);
        assert_eq!((f(&r, 0xac), f(&r, 0xb8), f(&r, 0xc4), f(&r, 0xd8)), (1.0, 2.0, 1.0, 1.0));
        super::resolve_level_record(&mut r, 1.0).unwrap();
        assert_eq!(f(&r, 0xb8), 2.0);
    }
}
