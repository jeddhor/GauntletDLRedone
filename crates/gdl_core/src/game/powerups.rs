//! PlayerProcessPowerups 0x12d040 (simulation part) and the Tower shop's
//! purchase logic (do_shopping 0x18a898, make_skiplist 0x18a648,
//! recalc_afford_idx 0x18a7f8, ShopLoadData 0x189d68: SHPDATA/SHOP.WAD ITEM,
//! 0x50-byte records, kind +0x44, price +0x48, amount +0x4c).
//!
//! PlayerProcessPowerups runs every do_players update for players in
//! states 1, 4 and 8: it re-derives the combat words from the attributes,
//! rebuilds the weapon (+0x130), shield (+0x134) and powerup (+0x138) flag
//! words from the eight powerup slots (+0x144 + 16k: time, kind, amount,
//! flags), counts their timers down outside the Tower, applies speed/magic
//! boosts and the turbo refill, and clamps the derived words to the class
//! ranges. Not ported (presentation or separate systems): see-through, skin
//! effects, attached models and trees, PlayerProcessFamiliar,
//! PlayerProcessScale, death effects and sounds.
use super::{Game, R};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub};

/// One shop item (SHPDATA/SHOP.WAD ITEM record).
#[derive(Clone, Debug, PartialEq)]
pub struct ShopItem {
    pub name: String,
    pub word_40: f32,
    pub kind: i32,
    pub price: i32,
    pub amount: i32,
}

/// ShopLoadData: the ITEM chunk records (ITEMTAB_NUM = their count).
pub fn shop_items(wad: &[u8]) -> Result<Vec<ShopItem>, String> {
    let u32_ = |o: usize| wad.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or("SHOP.WAD truncated");
    let (dir, n) = (u32_(0)? as usize, u32_(4)?);
    if 16 < n {
        return Err("SHOP.WAD chunk count policy".into());
    }
    for i in 0..n as usize {
        let t = u32_(dir + 16 * i)?;
        if t.to_be_bytes() != *b"ITEM" {
            continue;
        }
        let (off, count) = (u32_(dir + 16 * i + 4)? as usize, u32_(dir + 16 * i + 8)? as usize);
        if 256 < count || wad.len() < off + 0x50 * count {
            return Err("SHOP.WAD ITEM range".into());
        }
        return Ok((0..count)
            .map(|k| {
                let r = &wad[off + 0x50 * k..off + 0x50 * (k + 1)];
                let w = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
                ShopItem {
                    name: String::from_utf8_lossy(&r[..r[..0x40].iter().position(|&c| c == 0).unwrap_or(0x40)]).into_owned(),
                    word_40: f32::from_bits(w(0x40)),
                    kind: w(0x44) as i32,
                    price: w(0x48) as i32,
                    amount: w(0x4c) as i32,
                }
            })
            .collect());
    }
    Err("SHOP.WAD without ITEM".into())
}

/// Result of one purchase attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purchase {
    Bought,
    /// Entry 0 (the empty record): leave the shop.
    Leave,
    /// Not affordable or the inventory is full (AudioBuzzer).
    Refused,
}

impl Game {
    /// PlayerProcessPowerups (simulation part).
    pub fn player_process_powerups(&mut self, p: usize) -> R<()> {
        let r = &self.players[p];
        let atts = [r.f32_(0x108), r.f32_(0x10c), r.f32_(0x110), r.f32_(0x114)];
        let words = self.class_tables.derived_words(atts, r.u32_(8))?;
        let tick = self.globals.cur_world != 0xd && self.globals.trig_cam_active == 0 && self.globals.start_of_stage == 0;
        let (spf, boss_type, boss_active) = (self.globals.spf, self.globals.boss_type, self.globals.boss_active);
        let r = &mut self.players[p];
        for (k, w) in words.iter().enumerate() {
            r.set_f32(0x118 + 4 * k, *w);
        }
        r.set_u32(0x130, 0);
        r.set_u32(0x134, 0);
        r.set_u32(0x138, 0);
        // Longest-lasting element weapon (fVar22) and the timers the
        // presentation code reads (fVar21 gold skin, fVar20 invisibility,
        // fVar18 fade) — only the first matters to the simulation.
        let mut weapon_time = -1.0f32;
        for k in 0..8 {
            let o = 0x144 + 16 * k;
            let mut time = r.f32_(o);
            if time == 0.0 {
                continue;
            }
            if tick && 0.0 < time {
                if boss_type < 0 {
                    r.set_f32(o, sub(time, spf)?);
                } else if boss_active != 0 {
                    r.set_f32(o, sub(time, mul(spf, 3.0)?)?);
                }
                if r.f32_(o) < 0.0 {
                    r.set_f32(o, 0.0);
                }
            }
            time = r.f32_(o);
            let (kind, amount, flags) = (r.i32_(o + 4), r.f32_(o + 8), r.u32_(o + 12));
            match kind {
                5 => {
                    let (a, b) = if flags & 0xf == 0 {
                        (r.u32_(0x130), flags)
                    } else {
                        if weapon_time < 0.0 || (0.0 < time && weapon_time < time) {
                            r.set_u32(0x130, (r.u32_(0x130) & 0xffff_fff0) | flags);
                            weapon_time = time;
                        }
                        (flags & 0xffff_fff0, r.u32_(0x130))
                    };
                    r.set_u32(0x130, a | b);
                }
                6 => {
                    let v = r.u32_(0x134) | flags;
                    r.set_u32(0x134, v);
                }
                7 => {
                    r.set_f32(0x124, add(r.f32_(0x124), amount)?);
                    let v = r.u32_(0x138) | 0x1_0000;
                    r.set_u32(0x138, v);
                }
                8 => r.set_f32(0x120, add(r.f32_(0x120), amount)?),
                9 => {
                    let v = r.u32_(0x138) | flags;
                    r.set_u32(0x138, v);
                    if flags & 0x8_0000 != 0 {
                        let t = add(r.f32_(0x800), 100.0)?;
                        r.set_f32(0x800, if 100.0 < t { 100.0 } else { t });
                        if 0.0 <= r.f32_(o) {
                            r.set_f32(o, 0.0);
                        }
                    }
                }
                _ => {}
            }
        }
        // The model/tree branch is presentation; in every path it ends with
        // +0x140 = +0x13c.
        let v = r.u32_(0x13c);
        r.set_u32(0x140, v);
        r.set_u32(0x13c, 0);
        if r.u32_(0x134) & 0x8_0000 == 0 {
            r.set_u16(0x936, 0);
        }
        let stop = u16::from(r.u32_(0x138) & 8 != 0);
        r.set_u16(0x938, stop);
        for k in 0..6 {
            let (lo, hi) = self.class_tables.ranges[k];
            let r = &mut self.players[p];
            let v = r.f32_(0x118 + 4 * k);
            r.set_f32(0x118 + 4 * k, if v < lo { lo } else if v <= hi { v } else { hi });
        }
        Ok(())
    }

    /// make_skiplist: items the player cannot use now (unaffordable, full
    /// key ring or potion bag, attribute at 999, health at its maximum).
    /// The original loops 1..=ITEMTAB_NUM (one record past the table);
    /// the native table stops at its last record.
    pub fn shop_skiplist(&self, p: usize, items: &[ShopItem]) -> R<Vec<bool>> {
        let r = &self.players[p];
        let gold = r.i32_(0x1a20);
        let mut skip = vec![false; items.len()];
        for (k, it) in items.iter().enumerate().skip(1) {
            skip[k] = gold < it.price
                || match it.kind {
                    1 => self.globals.max_keys_per_player <= r.i32_(0x1a14),
                    3 => self.globals.max_potions_per_player <= r.i32_(0x1a18),
                    5 => 999.0 <= r.f32_(0x108),
                    6 => 999.0 <= r.f32_(0x114),
                    7 => 999.0 <= r.f32_(0x10c),
                    8 => 999.0 <= r.f32_(0x110),
                    0x11 => crate::player_class::max_health(r.i32_(0x2ac0)) <= r.f32_(0x1a10),
                    _ => false,
                };
        }
        Ok(skip)
    }

    /// PlayerInc<Att>(player, n): the class's bonus attribute += n, then
    /// PlayerUpdateAtts.
    fn player_inc_att(&mut self, p: usize, k: usize, n: f32) -> R<()> {
        let o = 0x9b4 + 0x18 * self.players[p].u32_(0xc) as usize + 4 * k;
        let v = add(n, self.players[p].f32_(o))?;
        self.players[p].set_f32(o, v);
        self.player_update_atts(p)
    }

    /// do_shopping's accept case for item `idx`: the purchase, gold, and
    /// (for a real item) PlayerProcessPowerups. RandInt(4) picks a potion's
    /// colour.
    pub fn shop_buy(&mut self, p: usize, items: &[ShopItem], idx: usize) -> R<Purchase> {
        let Some(it) = items.get(idx) else { return Ok(Purchase::Refused) };
        // do_shopping's cursor never rests on an unaffordable or skipped
        // entry; the native menu lists them, so they are refused here.
        if self.players[p].i32_(0x1a20) < it.price || self.shop_skiplist(p, items)?[idx] {
            return Ok(Purchase::Refused);
        }
        let class = self.classes[p].clone();
        let mut ok = true;
        let mut leave = false;
        // PlayerAddPowerup(amount, time, player, kind, flags).
        let pw = |g: &mut Game, amount: f32, time: f32, kind: i32, flags: u32| g.add_powerup(amount, time, p, kind, flags, &class);
        match it.kind {
            1 => {
                let k = self.players[p].i32_(0x1a14);
                if k < self.globals.max_keys_per_player {
                    self.players[p].set_i32(0x1a14, k + 1);
                } else {
                    ok = false;
                }
            }
            2 => pw(self, 0.0, 90.0, 5, 0x20_0000)?,
            3 => {
                let n = self.players[p].i32_(0x1a18);
                if n < self.globals.max_potions_per_player {
                    let colour = self.rng.rand_int(4).unwrap_or(0) as i32 + 1;
                    self.players[p].set_i32(0x2a9c + 4 * n as usize, colour);
                    self.players[p].set_i32(0x1a18, n + 1);
                } else {
                    ok = false;
                }
            }
            4 | 0x13 => pw(self, 0.0, 30.0, 9, 0x100)?,
            5 => {
                self.player_inc_att(p, 0, 10.0)?;
                let v = add(self.players[p].f32_(0x98c), 10.0)?;
                self.players[p].set_f32(0x98c, v);
            }
            6 => {
                self.player_inc_att(p, 3, 10.0)?;
                let v = add(self.players[p].f32_(0x998), 10.0)?;
                self.players[p].set_f32(0x998, v);
            }
            7 => {
                self.player_inc_att(p, 1, 10.0)?;
                let v = add(self.players[p].f32_(0x990), 10.0)?;
                self.players[p].set_f32(0x990, v);
            }
            8 => {
                self.player_inc_att(p, 2, 10.0)?;
                let v = add(self.players[p].f32_(0x994), 10.0)?;
                self.players[p].set_f32(0x994, v);
            }
            9 => pw(self, 0.0, 30.0, 6, 0x2_0000)?,
            10 => pw(self, 0.0, 45.0, 9, 0x80)?,
            0xb => pw(self, 0.0, 30.0, 5, 0x2000_0000)?,
            0xc => pw(self, 0.0, 45.0, 5, 0x8_0000)?,
            0xd => pw(self, 3.0, -1.0, 5, 0x1000_0000)?,
            0xe => pw(self, 0.0, 15.0, 6, 0x40_0000)?,
            0xf => pw(self, 0.0, 15.0, 6, 0x20_0000)?,
            0x10 => pw(self, 0.0, 20.0, 6, 0x11_0000)?,
            0x11 => {
                self.heal_player(it.amount as f32, p)?;
            }
            0x12 => pw(self, 0.0, 60.0, 9, 1)?,
            0x14 => pw(self, 0.0, 90.0, 5, 1)?,
            0x15 => pw(self, 0.0, 90.0, 5, 2)?,
            0x16 => pw(self, 0.0, 90.0, 5, 3)?,
            0x17 => pw(self, 0.0, 90.0, 5, 4)?,
            0x18 => pw(self, 5.0, -1.0, 5, 0x10_0000)?,
            0x19 => pw(self, 5.0, -1.0, 9, 0x10)?,
            0x1a => pw(self, 5.0, -1.0, 9, 0x40)?,
            0x1b => pw(self, 5.0, -1.0, 9, 0x20)?,
            0x1c => pw(self, 4.0, 40.0, 9, 0x1_0000)?,
            0x1d => pw(self, 0.0, 15.0, 9, 0x200)?,
            0x1e => pw(self, 0.0, 15.0, 9, 4)?,
            0x1f => pw(self, 0.0, 30.0, 6, 0x1_0000)?,
            0x20 => pw(self, 0.0, 120.0, 9, 2)?,
            0x21 => pw(self, 0.0, 15.0, 6, 0x2008)?,
            0x22 => pw(self, 0.0, 45.0, 5, 0x40_0000)?,
            0x23 => pw(self, 0.0, 120.0, 6, 0x8_0000)?,
            0x24 => pw(self, 0.0, 25.0, 9, 8)?,
            _ => leave = true,
        }
        if !ok {
            return Ok(Purchase::Refused);
        }
        let g = self.players[p].i32_(0x1a20) - it.price;
        self.players[p].set_i32(0x1a20, g);
        if idx != 0 {
            self.audio_pickup(p);
            self.player_process_powerups(p)?;
        }
        Ok(if leave { Purchase::Leave } else { Purchase::Bought })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_a_synthetic_shop_table() {
        // Synthetic: ITEM chunk at 0x10 with two records, directory at 0xb0.
        let mut b = vec![0u8; 0xc0];
        b[0..4].copy_from_slice(&0xb0u32.to_le_bytes());
        b[4..8].copy_from_slice(&1u32.to_le_bytes());
        b[0xb0..0xb4].copy_from_slice(&u32::from_be_bytes(*b"ITEM").to_le_bytes());
        b[0xb4..0xb8].copy_from_slice(&0x10u32.to_le_bytes());
        b[0xb8..0xbc].copy_from_slice(&2u32.to_le_bytes());
        b[0x60..0x67].copy_from_slice(b"SHP_KEY");
        b[0x60 + 0x44..0x60 + 0x48].copy_from_slice(&1i32.to_le_bytes());
        b[0x60 + 0x48..0x60 + 0x4c].copy_from_slice(&100i32.to_le_bytes());
        let items = super::shop_items(&b).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!((items[1].name.as_str(), items[1].kind, items[1].price), ("SHP_KEY", 1, 100));
        assert!(super::shop_items(&b[..0x40]).is_err());
    }
}
