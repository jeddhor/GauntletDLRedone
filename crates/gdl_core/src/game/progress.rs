//! Experience and levels: AddExp 0x134930, ModifyExp 0x134808, LevelToExp
//! 0x134778 and the attribute part of PlayerUpdateAtts 0x12bab0.
//!
//! Player record: +0x1a1c experience, +0x2ac0 level, +0x108/+0x10c/+0x110/
//! +0x114 attribute words, +0x9ac + 0x18 × character per-character bonuses,
//! +0x800 turbo meter, +0x8c8 attack state. PlayerUpdateAtts reads the class
//! record (PlayerData[player], loaded by LoadPlyrData): +0x28..+0x44 base
//! and cap pairs, the player's ClassData (the host loads each player's
//! class record). The derived combat words +0x118..+0x12c follow the new
//! attributes through PlayerProcessPowerups' check_player_atts each update.
use super::{Event, Game, R};
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sub_guarded_rz as sub};

/// LevelToExp: experience needed for `level`.
pub fn level_to_exp(level: i32) -> i32 {
    if level < 0x3d {
        (level * 0x1e + 1000) * (level - 1)
    } else {
        (level - 0x3c) * 0x11f8 + 0x28550
    }
}

impl Game {
    /// AddExp(player, amount, kind): kind 1 = melee attack (charges turbo),
    /// 0 = weapon. Returns ModifyExp's level change (1 up, −1 down, 0).
    pub fn add_exp(&mut self, p: usize, amount: i32, kind: i32) -> R<i32> {
        let level = self.players[p].i32_(0x2ac0);
        let n = if kind == -2 {
            // Level-delta share (Death's drain): amount × (int)(LevelDeltaExp × 0.01).
            if amount < 0 && level == 99 {
                return Ok(0);
            }
            let d = if level < 0x3d { (level - 1) * 0x3c + 1000 } else { 0x11f8 };
            amount.wrapping_mul(mul(d as f32, f32::from_bits(0x3c23_d70a))? as i32)
        } else {
            self.scaled_exp(p, amount)?
        };
        self.add_exp_sub(p, n, kind)
    }

    /// AddExp's level_data +0x9c/+0xa0 scaling (and the AuxLevels ×5).
    fn scaled_exp(&self, p: usize, amount: i32) -> R<i32> {
        let target = self.level_f32(0x9c);
        let mut mult = self.level_f32(0xa0);
        let level = self.players[p].i32_(0x2ac0) as f32;
        if 0.0 < target && target < level {
            let k = add(mul(sub(level, target)?, f32::from_bits(0x3dcc_cccd))?, 1.0)?;
            mult = mul(mult, div(1.0, k)?)?;
        }
        let mut n = mul(amount as f32, mult)? as i32;
        if self.globals.aux_levels & 0x10 != 0 {
            n = mul(n as f32, 5.0)? as i32;
        }
        Ok(n)
    }

    fn add_exp_sub(&mut self, p: usize, n: i32, kind: i32) -> R<i32> {
        let changed = self.modify_exp(p, n)?;
        if kind == 1 {
            let r = &mut self.players[p];
            if r.i32_(0x8c8) < 0xb {
                let t = add(mul(n as f32, f32::from_bits(0x3ccc_cccd))?, r.f32_(0x800))?;
                r.set_f32(0x800, t);
            }
            if 100.0 < r.f32_(0x800) {
                r.set_f32(0x800, 100.0);
            }
        }
        if changed != 0 {
            if n < 0 {
                self.audio_gained_level(p, -1);
            } else if 0 < n {
                self.help(0x22, p);
                let h = add(self.players[p].f32_(0x1a10), 100.0)?;
                self.players[p].set_f32(0x1a10, h);
                self.events.push(Event::LevelUp { player: p, level: self.players[p].i32_(0x2ac0) });
            }
        }
        Ok(changed)
    }

    /// ModifyExp(player, delta).
    pub fn modify_exp(&mut self, p: usize, delta: i32) -> R<i32> {
        let e = self.players[p].i32_(0x1a1c).wrapping_add(delta);
        self.players[p].set_i32(0x1a1c, e.max(0));
        let mut changed = 0;
        if 0 < delta {
            if level_to_exp(self.players[p].i32_(0x2ac0) + 1) <= self.players[p].i32_(0x1a1c) {
                while self.players[p].i32_(0x2ac0) < 99 {
                    let l = self.players[p].i32_(0x2ac0) + 1;
                    self.players[p].set_i32(0x2ac0, l);
                    self.player_update_atts(p)?;
                    changed = 1;
                    if self.players[p].i32_(0x1a1c) < level_to_exp(l + 1) {
                        break;
                    }
                }
            }
        } else if delta < 0 {
            while 1 < self.players[p].i32_(0x2ac0) && self.players[p].i32_(0x1a1c) < level_to_exp(self.players[p].i32_(0x2ac0)) {
                let l = self.players[p].i32_(0x2ac0) - 1;
                self.players[p].set_i32(0x2ac0, l);
                changed = -1;
                self.player_update_atts(p)?;
            }
        }
        Ok(changed)
    }

    /// PlayerUpdateAtts(player, character, NULL): attribute words from the
    /// class base + 5 per level (capped) + the character's bonus, below 999.
    pub fn player_update_atts(&mut self, p: usize) -> R<()> {
        let class = self.classes[p].raw.clone();
        if class.len() < 0x48 {
            return super::unsupported("class record not captured");
        }
        let cf = |o: usize| f32::from_le_bytes(class[o..o + 4].try_into().unwrap());
        let r = &self.players[p];
        let character = r.u32_(0xc) as usize;
        let bonus = 0x9ac + 0x18 * character;
        let step = ((r.i32_(0x2ac0) - 1) * 5) as f32;
        let mut out = [0f32; 4];
        for (k, (base, cap, b)) in [(0x28, 0x2c, 8), (0x38, 0x3c, 0xc), (0x40, 0x44, 0x10), (0x30, 0x34, 0x14)].into_iter().enumerate() {
            let mut v = add(step, cf(base))?;
            if v >= cf(cap) || v.is_nan() {
                v = cf(cap);
            }
            let v = add(v, r.f32_(bonus + b))?;
            out[k] = if v < 999.0 { v } else { 999.0 };
        }
        for (k, v) in out.iter().enumerate() {
            self.players[p].set_f32(0x108 + 4 * k, *v);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::level_to_exp;

    #[test]
    fn level_to_exp_follows_both_source_branches() {
        // Synthetic: LevelToExp 0x134778 formula at its branch boundary.
        assert_eq!(level_to_exp(1), 0);
        assert_eq!(level_to_exp(2), 1060);
        assert_eq!(level_to_exp(60), (60 * 30 + 1000) * 59);
        assert_eq!(level_to_exp(61), 0x11f8 + 0x28550);
    }
}
