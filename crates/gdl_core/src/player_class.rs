//! Player classes from the disc: the class record (PDATA/<CLS>.WAD PDAT,
//! LoadPlyrData 0x14d9f0), the derived combat words of check_player_atts
//! 0x12fee0 (GetFight/GetArmor/GetMagic/GetSpeed/GetMissileDmg/
//! GetMissileSpd with the ELF ranges fight_range 0x220f70 …), the player
//! action map (InitActions 0x177a88 over PActionDesc 0x23f670 against the
//! class ANIM.PS2 tree) and the experience tables (LevelToExp 0x134778,
//! ExpToLevel 0x1347b8, player_max_health 0x132538).
//!
//! Actual reference checks: the SOR PDAT equals the captured class record
//! except its pointer words; the derived words for the captured attributes
//! (200/150/600/400) equal the captured +0x118..+0x12c; the derived action
//! map equals the captured 149 entries.
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub, UnsupportedNumericDomain};
use gdl_formats::animation::Animations;
use gdl_formats::elf::Elf;

/// player_desc 0x220d70: the class tags (class index order).
pub const CLASS_TAGS: [&str; 17] = ["war", "val", "wiz", "arc", "dwf", "kni", "sor", "jes", "min", "fal", "jac", "tig", "ogr", "uni", "med", "hye", "sum"];

/// The ELF tables class code reads.
#[derive(Clone, Debug)]
pub struct ClassTables {
    /// (low, high) pairs: fight, armor, magic, speed, missile damage, speed.
    pub ranges: [(f32, f32); 6],
    /// PActionDesc action names (149; the 150th word ends the list).
    pub action_names: Vec<String>,
}

impl ClassTables {
    pub fn from_elf(elf: &Elf) -> Result<Self, String> {
        let mut ranges = [(0.0, 0.0); 6];
        for (k, name) in ["fight_range", "armor_range", "magic_range", "speed_range", "missiledmg_range", "missilespd_range"].iter().enumerate() {
            let b = elf.object(name).map_err(|e| e.to_string())?;
            if b.len() != 8 {
                return Err(format!("{name} size"));
            }
            ranges[k] = (f32::from_le_bytes(b[0..4].try_into().unwrap()), f32::from_le_bytes(b[4..8].try_into().unwrap()));
        }
        let d = elf.object("PActionDesc").map_err(|e| e.to_string())?;
        let mut action_names = Vec::new();
        for w in d.chunks_exact(4) {
            let p = u32::from_le_bytes(w.try_into().unwrap());
            if p == 0 {
                break;
            }
            let s = elf.read(p, 32).map_err(|e| e.to_string())?;
            action_names.push(String::from_utf8_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(32)]).into_owned());
        }
        Ok(Self { ranges, action_names })
    }

    /// Get<Att>(att) = att × 0.001 × (high − low) + low.
    pub fn derive(&self, k: usize, att: f32) -> Result<f32, UnsupportedNumericDomain> {
        let (lo, hi) = self.ranges[k];
        add(mul(mul(att, 0.001)?, sub(hi, lo)?)?, lo)
    }

    /// check_player_atts' derived words +0x118..+0x12c from the attribute
    /// words +0x108..+0x114 (fight, armor, magic, speed); classes 2 and 6
    /// (wizard, sorceress) base their missiles on magic, the others on fight.
    pub fn derived_words(&self, atts: [f32; 4], class: u32) -> Result<[f32; 6], UnsupportedNumericDomain> {
        let base = if class == 2 || class == 6 { atts[2] } else { atts[0] };
        Ok([
            self.derive(0, atts[0])?,
            self.derive(1, atts[1])?,
            self.derive(2, atts[2])?,
            self.derive(3, atts[3])?,
            self.derive(4, base)?,
            self.derive(5, base)?,
        ])
    }

    /// InitActions(class tree, player +0x1e8, PActionDesc): per action the
    /// first sequence with that label and its frame count, else (-1, 0).
    pub fn action_map(&self, anim: &Animations) -> Vec<[i64; 2]> {
        let Some(t) = anim.trees.first() else { return Vec::new() };
        self.action_names
            .iter()
            .map(|n| match t.sequences.iter().position(|s| &s.label == n) {
                Some(i) => [i as i64, i64::from(t.sequences[i].frames)],
                None => [-1, 0],
            })
            .collect()
    }
}

/// LevelToExp 0x134778.
pub fn level_to_exp(level: i32) -> i32 {
    if level < 0x3d {
        (level * 0x1e + 1000) * (level - 1)
    } else {
        (level - 0x3c) * 0x11f8 + 0x28550
    }
}

/// ExpToLevel 0x1347b8: the highest level 1..=99 whose threshold is reached.
pub fn exp_to_level(exp: i32) -> i32 {
    (1..=99).rev().find(|&l| level_to_exp(l) <= exp).unwrap_or(1)
}

/// player_max_health 0x132538: (level − 1) × 100 + 500, at most 9999.
pub fn max_health(level: i32) -> f32 {
    (((level - 1) as f32) * 100.0 + 500.0).min(9999.0)
}

/// def_char_type 0x131300: the default class of player slot `k`.
pub fn default_class(k: usize) -> u32 {
    match k {
        1 => 5,
        2 => 4,
        3 => 7,
        _ => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_words_follow_the_ranges() {
        // Synthetic ranges equal to the ELF values; attributes as in the
        // user save (fight 200, armor 150, magic 600, speed 400), class 6.
        let t = ClassTables { ranges: [(5.0, 20.0), (0.0, 5.0), (8.0, 32.0), (5.0, 12.5), (5.0, 20.0), (20.0, 60.0)], action_names: Vec::new() };
        let w = t.derived_words([200.0, 150.0, 600.0, 400.0], 6).unwrap();
        assert_eq!(w[3], 8.0);
        assert!((w[0] - 8.0).abs() < 1e-5 && (w[1] - 0.75).abs() < 1e-5);
        assert!((w[2] - 22.4).abs() < 1e-4 && (w[4] - 14.0).abs() < 1e-4 && (w[5] - 44.0).abs() < 1e-4);
    }

    #[test]
    fn experience_tables() {
        assert_eq!(level_to_exp(1), 0);
        assert_eq!(level_to_exp(2), 1060);
        assert_eq!(exp_to_level(1059), 1);
        assert_eq!(exp_to_level(1060), 2);
        assert_eq!(max_health(1), 500.0);
    }
}
