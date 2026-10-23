//! Player damage: damage_player 0x131c80, player_can_be_damaged 0x131c30,
//! ModifyDamage 0x190b88, player_dies 0x131838. Vibration, audio and block
//! FX become events.
use super::pickups::p;
use super::{Event, Game, R};
use crate::libm::atan2f;
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub};

pub mod pd {
    pub const STATE: usize = 0xfc;
    pub const ARMOR: usize = 0x11c;
    pub const RESIST: usize = 0x134;
    pub const CURRENT_ACTION: usize = 0x1e0;
    pub const HIT_SOUND_TIMER: usize = 0x1c8;
    pub const BLOCK_TIMER: usize = 0x1d6;
    pub const CARRIED_BY: usize = 0x690;
    pub const TURBO: usize = 0x800;
    pub const FACING: usize = 0x86c;
    pub const STUN_TIME: usize = 0x870;
    pub const DAMAGE_ACC: usize = 0x8a8;
    pub const DAMAGE_FLAGS: usize = 0x8ac;
    pub const KNOCK_ACC: usize = 0x8b4;
    pub const ATTACK_STATE: usize = 0x8c8;
    pub const DEATH_COUNTER: usize = 0x8f8;
    pub const PAIN_ACC: usize = 0x8fc;
    pub const STATUS: usize = 0x93c;
}

const PI: f32 = f32::from_bits(0x4049_0fdb);
const TWO_PI: f32 = f32::from_bits(0x40c9_0fdb);
const HALF_PI: f32 = f32::from_bits(0x3fc9_0fdb);

/// ModifyDamage(armor, damage, flags, resistances).
pub fn modify_damage(armor: f32, damage: &mut f32, flags: &mut u32, resist: u32, boss_type: i32) -> R<()> {
    let old = *flags;
    let mut d = *damage;
    if resist & 0x10_0000 != 0 {
        *damage = if 1.0 < d { mul(-d, f32::from_bits(0x3dcc_cccd))? } else { 0.0 };
        return Ok(());
    }
    if resist & 0x1_0000 != 0 || (resist & 0x1000 != 0 && old & 0x200 != 0) || (resist & 0x2000 != 0 && old & 0x800 != 0) {
        *damage = 0.0;
        return Ok(());
    }
    let (low, mid, high) = if boss_type < 0 { (0.5, 1.5, 2.0) } else { (0.75, 1.25, 1.5) };
    if resist & 0x4_0000 != 0 {
        *flags &= 0xfffe_fe8f;
    }
    if resist & 0x10 != 0 && old & 0x200 != 0 {
        d = mul(d, low)?;
    }
    if old & 0xa00 == 0 {
        d = if d < 0.0 {
            -d
        } else if d <= armor {
            0.0
        } else {
            sub(d, armor)?
        };
    }
    if 0.0 < d {
        let (weak, immune, strong) = match old & 0xf {
            1 => (1, 0x100, 0x202),
            2 => (2, 0x200, 0x101),
            3 => (4, 0x400, 0x808),
            4 => (8, 0x800, 0x404),
            _ => {
                *damage = d;
                return Ok(());
            }
        };
        d = if resist & weak != 0 {
            mul(d, low)?
        } else if resist & immune != 0 {
            0.0
        } else if resist & strong != 0 {
            mul(d, high)?
        } else {
            mul(d, mid)?
        };
    }
    *damage = d;
    Ok(())
}

impl Game {
    pub fn player_can_be_damaged(&self, player: usize) -> bool {
        let pl = &self.players[player];
        2 < pl.u32_(pd::CURRENT_ACTION).wrapping_sub(0x58)
            && pl.i32_(pd::ATTACK_STATE) < 0xb
            && pl.u16_(pd::STATUS) & 0x10 == 0
            && pl.u32_(pd::CARRIED_BY) == 0
    }

    /// player_dies.
    pub fn player_dies(&mut self, player: usize) -> R<()> {
        self.audio_player_dies(player)?;
        self.hud.kill_got_it(player);
        let pl = &mut self.players[player];
        pl.set_i32(pd::STATE, 8);
        pl.set_i32(pd::DEATH_COUNTER, 4);
        pl.set_f32(p::HEALTH, 0.0);
        pl.set_f32(pd::TURBO, 0.0);
        if self.globals.it == player as i32 {
            self.globals.it = -1;
        }
        self.events.push(Event::PlayerDied { player });
        Ok(())
    }

    /// damage_player(damage, player, pain sound kind, flags, direction).
    /// Returns 1 when the player died.
    pub fn damage_player(&mut self, damage: f32, player: usize, pain: i32, flags: u32, dir: Option<[f32; 3]>) -> R<i32> {
        if self.players[player].i32_(pd::STATE) != 1 || self.globals.trig_cam_active != 0 || flags & 0x200 != 0 {
            return Ok(0);
        }
        let mut d = damage;
        let mut f = flags;
        if !self.player_can_be_damaged(player) {
            return Ok(0);
        }
        let mut level = self.globals.options;
        if level < 2 {
            level = i32::from(0 < level && d != -1.0);
        }
        if f & 0x8000 == 0 {
            let g = &self.globals;
            if 2 < level
                || self.players[player].i32_(pd::STATE) == 4
                || g.good_wizard != 0
                || (0 <= g.boss_type && 0 <= g.boss_index && self.boss_state_is_not_active())
            {
                return Ok(0);
            }
            if 1.0 < d {
                d = mul(d, self.level_f32(0xa4))?;
            }
        }
        let pl = &self.players[player];
        modify_damage(pl.f32_(pd::ARMOR), &mut d, &mut f, pl.u32_(pd::RESIST), self.globals.boss_type)?;
        if f & 0x4000_0000 != 0 && self.players[player].u32_(p::POWERUP_FLAGS) & 1 != 0 {
            d = 0.0;
        }
        let status = self.players[player].u16_(pd::STATUS);
        if 1.0 < d {
            if status & 0x600 == 0 {
                if 15.0 < d && f & 0x10160 != 0 && status & 0x2000 == 0 && 60.0 < self.globals.time {
                    self.help(0x7d, player);
                }
            } else {
                let mut blocked = d;
                let mut full = false;
                if status & 0x200 != 0 {
                    full = true;
                    if let Some(v) = dir {
                        let mut a = sub(atan2f(v[0], v[2])?, self.players[player].f32_(pd::FACING))?;
                        if PI < a {
                            a = sub(a, TWO_PI)?;
                        } else if a <= -PI {
                            a = add(a, TWO_PI)?;
                        }
                        // The original's test is always true (a <= π/2 || -π/2 <= a).
                        if a <= HALF_PI || -HALF_PI <= a {
                            full = false;
                        }
                    }
                }
                blocked = if full { 0.0 } else { mul(blocked, 0.5)? };
                if 2.0 < sub(d, blocked)? && self.players[player].i16_(pd::BLOCK_TIMER) < 1 {
                    let k = mul(blocked, 0.01)?;
                    let k = k.clamp(0.333, 1.0);
                    self.start_block_fx(player, k)?;
                    let pl = &mut self.players[player];
                    pl.set_i16(pd::BLOCK_TIMER, mul(k, 60.0)? as i32 as i16);
                    pl.or_u16(pd::STATUS, 0x2000);
                }
                d = blocked;
                if f & 0x10160 == 0 {
                    f &= 0xffff_ffef;
                } else {
                    f = (f & 0xfffe_fe9f) | 0x10;
                }
            }
        }
        let before = self.players[player].f32_(p::HEALTH);
        let knock = |pl: &mut super::record::Record, v: [f32; 3]| -> R<()> {
            for (i, x) in v.iter().enumerate() {
                let o = pd::KNOCK_ACC + 4 * i;
                pl.set_f32(o, add(*x, pl.f32_(o))?);
            }
            Ok(())
        };
        let after = if d < 0.0 {
            let pl = &mut self.players[player];
            pl.set_f32(p::HEALTH, sub(before, d)?);
            if f & 0x8000 != 0 {
                if f & 0xf != 0 {
                    pl.and_u32(pd::DAMAGE_FLAGS, 0xffff_fff0);
                }
                pl.or_u32(pd::DAMAGE_FLAGS, f);
                if let Some(v) = dir {
                    knock(pl, v)?;
                }
            }
            pl.f32_(p::HEALTH)
        } else {
            let pl = &mut self.players[player];
            if level == 0 && f & 0x8000 == 0 {
                pl.set_f32(p::HEALTH, sub(pl.f32_(p::HEALTH), d)?);
            }
            pl.set_f32(pd::DAMAGE_ACC, add(pl.f32_(pd::DAMAGE_ACC), d)?);
            if level <= 1 {
                if f & 0xf != 0 {
                    pl.and_u32(pd::DAMAGE_FLAGS, 0xffff_fff0);
                }
                if d <= 2.0 {
                    f &= 0xfffe_fe8f;
                }
                pl.or_u32(pd::DAMAGE_FLAGS, f);
                if let Some(v) = dir {
                    knock(pl, v)?;
                }
                if 0.0 < d {
                    if f & 0x800 != 0 {
                        pl.set_f32(pd::STUN_TIME, add(self.globals.time, 1.0)?);
                    }
                    if f & 0x1000 != 0 {
                        pl.set_f32(pd::STUN_TIME, add(self.globals.time, f32::from_bits(0x3d88_8889))?);
                    }
                    let (strength, time) = if f & 0x10040 != 0 {
                        (3, 0x1e)
                    } else if f & 0x120 != 0 {
                        (2, 0x14)
                    } else if f & 0x90 != 0 {
                        (1, 0xf)
                    } else {
                        (0, 10)
                    };
                    self.do_vibe(player, strength, time);
                }
            }
            self.players[player].f32_(p::HEALTH)
        };
        if after < 1.0 {
            self.player_dies(player)?;
            return Ok(1);
        }
        let was = add(before, 0.5)? as i32;
        let now = add(self.players[player].f32_(p::HEALTH), 0.5)? as i32;
        if 0.0 < d {
            let pl = &mut self.players[player];
            pl.set_f32(pd::PAIN_ACC, add(pl.f32_(pd::PAIN_ACC), d)?);
        }
        let mut pain = pain;
        let mut need_food = None;
        if 0x97 <= was && now <= 0x96 {
            self.help(0xd, player);
            // NewHelp's return value (nonzero skips the cue) is not modelled; the
            // cue is emitted. Presentation only.
            need_food = Some(1);
        } else if 0x33 <= was && now <= 0x32 {
            need_food = Some(if self.globals.itime & 1 == 0 { 2 } else { 3 });
        } else {
            match pain {
                2 => {
                    if 0.0 < d {
                        self.audio_player_pain(player)?;
                    }
                    self.players[player].set_f32(pd::PAIN_ACC, 0.0);
                }
                3 => {
                    self.audio_player_severe_pain(player)?;
                    self.players[player].set_f32(pd::PAIN_ACC, 0.0);
                }
                0 => {}
                _ => {
                    if was - now < 0x3d {
                        if 30.0 <= self.players[player].f32_(pd::PAIN_ACC) {
                            let pl = &mut self.players[player];
                            pl.set_f32(pd::PAIN_ACC, sub(pl.f32_(pd::PAIN_ACC), 30.0)?);
                            pain = 0;
                            if 0.0 < d {
                                self.audio_player_pain(player)?;
                            }
                        }
                    } else {
                        pain = 0;
                        if 0.0 < d {
                            self.audio_player_pain(player)?;
                        }
                        self.players[player].set_f32(pd::PAIN_ACC, 0.0);
                    }
                }
            }
        }
        if let Some(kind) = need_food {
            self.audio_need_food(kind, player);
        }
        if pain == 1 {
            if f & 0x800 == 0 {
                if self.players[player].i16_(pd::HIT_SOUND_TIMER) < 1 {
                    let kind = if f & 0x2_0000 != 0 {
                        1
                    } else if f & 0x4_0000 != 0 {
                        2
                    } else {
                        0
                    };
                    if 0.0 < d {
                        self.audio_player_hit(player, kind)?;
                    }
                    self.players[player].set_i16(pd::HIT_SOUND_TIMER, 0x1e);
                }
            } else if 0.0 < d {
                self.audio_player_poison(player);
            }
        }
        Ok(0)
    }

    /// The boss check in damage_player: enemies[BossIndex] +0xc8 != 1.
    fn boss_state_is_not_active(&self) -> bool {
        let i = self.globals.boss_index as usize;
        self.enemies.get(i).is_none_or(|e| e.i32_(0xc8) != 1)
    }
}
