//! Player ↔ item interaction: PlayerItemCollision 0x159708, GrabPowerup
//! 0x158c58, use_key 0x15b4b8, PlayerGiveGold 0x134710, heal_player 0x132588,
//! ExpireItem 0x1537b0, FlatGenerator 0x1585b0, PlayerAddPowerup 0x12c0e8,
//! AddItemSub 0x153898, OpenChest 0x1588a8. Player record offsets as in the
//! original (index ×4 of the decompiler's int view).
use super::{unsupported, Event, Game, Unsupported, R};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub};
use crate::vu0::fqdist;

/// Player record fields used here.
pub mod p {
    pub const INDEX: usize = 0x00;
    pub const CHARACTER: usize = 0x0c;
    pub const POSITION: usize = 0x50;
    pub const ATTENTION: usize = 0x60;
    pub const POWERUP_FLAGS: usize = 0x138;
    /// 8 × {time, kind, amount, flags} at +0x144.
    pub const POWERUPS: usize = 0x144;
    pub const STATUS_934: usize = 0x934;
    pub const PREVIOUS_POSITION: usize = 0x854;
    /// Item slot + 1 the player will grab (GrabPowerup in do_players).
    pub const GRAB: usize = 0x888;
    /// Floor object index + 1.
    pub const FLOOR_OBJECT: usize = 0x89c;
    pub const HAZARD_TIME: usize = 0x8c0;
    /// Per-character statistics, 7 words each; word 0 counts gold.
    pub const STATS: usize = 0xb40;
    pub const HEALTH: usize = 0x1a10;
    pub const KEYS: usize = 0x1a14;
    pub const POTIONS: usize = 0x1a18;
    pub const GOLD: usize = 0x1a20;
    pub const POTION_SLOTS: usize = 0x2a9c;
    pub const LEVEL: usize = 0x2ac0;
}

/// Class data (PlayerData[player] → class record) values the port reads.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ClassData {
    /// Raw class record (first 0x200 bytes) captured by the export.
    #[serde(rename = "raw_hex", with = "super::record::hex_bytes")]
    pub raw: Vec<u8>,
    /// +0x58: powerup duration multiplier.
    pub powerup_time_58: f32,
    /// +0x5c and +0x158: PlayerStartMissile origin offsets (modes 1 and 2).
    pub missile_origin_5c: [f32; 3],
    pub missile_origin_158: [f32; 3],
}

impl Game {

    /// ExpireItem: +0xda = mode (or 0x10 when negative), +0xd8 |= 0x100.
    pub fn expire_item(&mut self, slot: usize, mode: i16) {
        let r = &mut self.items.records[slot];
        r.set_i16(0xda, if mode >= 0 { mode } else { 0x10 });
        r.set_u16(0xd8, r.u16_(0xd8) | 0x100);
        self.events.push(Event::ItemExpired { slot, mode: i32::from(mode) });
    }

    /// use_key: spend a key; Options+4 bit 0 grants unlimited keys.
    pub fn use_key(&mut self, player: usize) -> bool {
        let pl = &mut self.players[player];
        let keys = pl.i32_(p::KEYS);
        if 0 < keys {
            pl.set_i32(p::KEYS, keys - 1);
            return true;
        }
        self.globals.options_4 & 1 != 0
    }

    /// PlayerGiveGold: gold += n, capped at 99,999.
    pub fn give_gold(&mut self, player: usize, n: i32) {
        let pl = &mut self.players[player];
        let gold = n.wrapping_add(pl.i32_(p::GOLD));
        pl.set_i32(p::GOLD, if 99_999 < gold { 99_999 } else { gold });
    }

    /// heal_player: 0 = already full, 1 = capped, 2 = healed.
    pub fn heal_player(&mut self, amount: f32, player: usize) -> R<i32> {
        let pl = &mut self.players[player];
        let mut max = add(mul((pl.i32_(p::LEVEL) - 1) as f32, 100.0)?, 500.0)?;
        if 9999.0 < max {
            max = 9999.0;
        }
        let health = pl.f32_(p::HEALTH);
        if 0.0 < amount && max <= health {
            return Ok(0);
        }
        let new = add(amount, health)?;
        pl.set_f32(p::HEALTH, new);
        if max < new {
            pl.set_f32(p::HEALTH, max);
            return Ok(1);
        }
        Ok(2)
    }

    /// PlayerAddPowerup(amount, time, player, kind, flags).
    pub fn add_powerup(&mut self, amount: f32, time: f32, player: usize, kind: i32, flags: u32, class: &ClassData) -> R<()> {
        let time = mul(time, class.powerup_time_58)?;
        let pl = &mut self.players[player];
        let slot = |i: usize| p::POWERUPS + 16 * i;
        for i in 0..8 {
            if pl.i32_(slot(i) + 4) == kind && pl.u32_(slot(i) + 12) == flags {
                if 0.0 < amount {
                    pl.set_f32(slot(i) + 8, add(amount, pl.f32_(slot(i) + 8))?);
                }
                let left = pl.f32_(slot(i));
                if !(left < 0.0 || time <= 0.0) {
                    pl.set_f32(slot(i), add(mul(time, 0.5)?, left)?);
                } else if time < 0.0 {
                    pl.set_f32(slot(i), time);
                }
                if flags & 8 != 0 {
                    self.max_stop_time = self.players[player].f32_(slot(i));
                }
                return Ok(());
            }
        }
        let mut best = 0;
        let mut best_time = -2.0f32;
        for i in 0..8 {
            let mut t = pl.f32_(slot(i));
            if t < 0.0 {
                t = if pl.i32_(slot(i) + 4) == kind { -1.0 } else { -2.0 };
            }
            if best_time == -2.0 || t == 0.0 || (0.0 <= t && t < best_time) {
                best_time = t;
                best = i;
            }
            if best_time == 0.0 {
                break;
            }
        }
        pl.set_f32(slot(best), time);
        pl.set_i32(slot(best) + 4, kind);
        pl.set_f32(slot(best) + 8, amount);
        pl.set_u32(slot(best) + 12, flags);
        if flags & 8 != 0 {
            self.max_stop_time = time;
        }
        Ok(())
    }

    /// PlayerItemCollision: 0 pass, 1 blocking/consumed, 2 exit/transporter.
    pub fn player_item_collision(&mut self, player: usize, slot: usize) -> R<i32> {
        let pos = self.players[player].v3(p::POSITION);
        let r = self.items.records[slot].clone();
        let ipos = r.v3(0x60);
        let def = self.items.definitions[r.definition()].clone();
        let (kind, subtype) = (def.kind(), def.subtype());
        let idx = self.players[player].u32_(p::INDEX) as usize;
        Ok(match kind {
            1 => {
                if 0 < r.i16_(0x100) || self.players[player].u32_(p::GRAB) != 0 {
                    return Ok(0);
                }
                self.players[player].set_u32(p::GRAB, slot as u32 + 1);
                0
            }
            2 => {
                if r.i8_(0xdc) == 2 {
                    if subtype == 0x2f {
                        let gold = r.i32_(0xf4);
                        if 0x19 <= gold {
                            self.help(0x11, idx);
                        }
                        self.give_gold(idx, gold);
                        self.audio_pickup(idx);
                        self.got_it(idx, 1, gold);
                        self.expire_item(slot, 8);
                        let o = p::STATS + 0x1c * self.players[player].u32_(p::CHARACTER) as usize;
                        let v = self.players[player].i32_(o);
                        self.players[player].set_i32(o, gold.wrapping_add(v));
                        1
                    } else {
                        let inner = r.u32_(0xfc);
                        if inner == 0 {
                            return Ok(1);
                        }
                        if self.players[player].u32_(p::GRAB) == 0 {
                            self.players[player].set_u32(p::GRAB, inner);
                        }
                        0
                    }
                } else {
                    if r.u16_(0xd8) & 0x11 != 0x10 || r.u8_(0xdc) != 0 {
                        return Ok(1);
                    }
                    if !self.use_key(player) {
                        self.help(2, idx);
                        if subtype == 0x30 {
                            self.help(0x17, idx);
                        }
                        return Ok(1);
                    }
                    self.sfx3(0x2c, r.v3(0x60), 0x7f, 0x40)?;
                    let rr = &mut self.items.records[slot];
                    rr.set_u16(0xd8, rr.u16_(0xd8) | 1);
                    rr.set_u8(0xdf, idx as u8);
                    if 0 <= rr.i16_(0xf0) {
                        self.open_chest(slot, Some(idx))?;
                    }
                    1
                }
            }
            3 => i32::from(r.u8_(0xf6) != 0),
            4 => {
                if 0.0 <= r.f32_(0xfc) {
                    let rr = &mut self.items.records[slot];
                    rr.set_u32(0xf8, rr.u32_(0xf8) | 1);
                }
                let d = fqdist(sub(ipos[0], pos[0])?, sub(ipos[2], pos[2])?)?.0;
                i32::from(d <= def.f32_(0xc))
            }
            5 => {
                let flags = r.u16_(0xf4);
                if let (true, Some(bound)) = (flags & 0x100 != 0, r.u32_(0xf0).checked_sub(1)) {
                    {
                        let floor = self.players[player].u32_(p::FLOOR_OBJECT);
                        if floor != bound + 1 {
                            // Walk the bound object's children (+0x2e child, +0x2c next).
                            let mut found = false;
                            let mut c = self.items.object_child[bound as usize];
                            let mut guard = 0;
                            while let Some(o) = c {
                                if floor == o as u32 + 1 {
                                    found = true;
                                    break;
                                }
                                c = self.items.object_next[o];
                                guard += 1;
                                if guard > self.items.object_next.len() {
                                    return unsupported("object sibling cycle");
                                }
                            }
                            if !found {
                                return Ok(0);
                            }
                        }
                    }
                }
                if r.u8_(0xe2) == 0 && 2 <= self.globals.num_active_players && flags & 0x400 != 0 {
                    self.help(if flags & 0x100 != 0 { 0x7e } else { 0x7f }, idx);
                }
                let bit = 1u8.wrapping_shl(idx as u32);
                if flags & 0x40 != 0 {
                    let rr = &mut self.items.records[slot];
                    rr.set_u8(0xe2, rr.u8_(0xe2) | bit);
                } else {
                    let mut s = Some(slot);
                    let mut guard = 0;
                    while let Some(i) = s {
                        let rr = &mut self.items.records[i];
                        rr.set_u8(0xe2, rr.u8_(0xe2) | bit);
                        s = rr.u32_(0xf8).checked_sub(1).map(|x| x as usize);
                        guard += 1;
                        if guard > self.items.records.len() {
                            return unsupported("trigger chain cycle");
                        }
                    }
                }
                if self.globals.cur_world == 0xd && r.u8_(0xf6) == 0xf0 {
                    return unsupported("tower Sumner hint trigger");
                }
                0
            }
            7 => {
                if r.u16_(0xd8) & 1 != 0 || r.u8_(0xdc) != 0 {
                    return Ok(1);
                }
                let prev = self.players[player].v3(p::PREVIOUS_POSITION);
                let a = [sub(pos[0], prev[0])?, sub(pos[2], prev[2])?];
                let b = [sub(ipos[0], pos[0])?, sub(ipos[2], pos[2])?];
                if add(mul(a[0], b[0])?, mul(a[1], b[1])?)? < 0.0 {
                    return Ok(1);
                }
                if self.use_key(player) {
                    let rr = &mut self.items.records[slot];
                    rr.set_u16(0xd8, rr.u16_(0xd8) | 1);
                    self.audio_door(r.v3(0x60), subtype)?;
                    0
                } else {
                    self.help(1, idx);
                    1
                }
            }
            8 => {
                if self.players[player].u32_(p::POWERUP_FLAGS) & 1 != 0 {
                    return Ok(0);
                }
                if self.players[player].f32_(p::HAZARD_TIME) <= self.globals.time {
                    let flags = def.0[0x3c..0x40].try_into().map(u32::from_le_bytes).unwrap() | 0x80;
                    let pain = if matches!(subtype, 0 | 3 | 4) { 3 } else { 2 };
                    let dir = if flags & 0x30 != 0 { Some(r.v3(0x30).map(|v| -v)) } else { None };
                    self.damage_player(r.f32_(0xf0), idx, pain, flags, dir)?;
                    let mut sound = subtype;
                    if self.globals.cur_world == 3 && subtype == 1 && def.name() == b"FLAMEH" {
                        sound = 6;
                    }
                    let at = self.players[player].v3(0x50);
                    self.audio_damage_tile(at, sound);
                    let t = add(mul((i32::from(r.i16_(0xda)) + 1) as f32, f32::from_bits(0x3d08_8889))?, self.globals.time)?;
                    self.players[player].set_f32(p::HAZARD_TIME, t);
                    self.help(0x15, idx);
                }
                0
            }
            9 => {
                let rr = &mut self.items.records[slot];
                rr.set_u32(0xf4, (1u32 << idx) | rr.u32_(0xf4));
                2
            }
            10 => match subtype {
                0x28 | 0x35 | 0x31 => {
                    if r.u16_(0xd8) & 1 != 0 {
                        return Ok(0);
                    }
                    if subtype == 0x31 {
                        self.audio_leaf_fall(r.v3(0x40))?;
                    } else {
                        self.audio_rock_fall(r.v3(0x40))?;
                    }
                    let rr = &mut self.items.records[slot];
                    rr.set_u16(0xd8, rr.u16_(0xd8) | 1);
                    0
                }
                0x29 => i32::from(1 <= r.i16_(0xf2)),
                0x33 | 0x34 => 0,
                _ => 1,
            },
            11 => 2,
            12 => {
                if subtype == 2 && r.u16_(0xd8) & 1 == 0 {
                    let rr = &mut self.items.records[slot];
                    rr.set_u16(0xd8, rr.u16_(0xd8) | 1);
                }
                0
            }
            _ => 0,
        })
    }

    /// AddItemSub: a new item from a definition, optionally at a matrix
    /// (then placed on the floor); without one it uses the identity at the origin.
    pub fn add_item_sub(&mut self, def: usize, at: Option<([[f32; 3]; 3], [f32; 3])>) -> R<usize> {
        let max = self.items.records.len() + 500;
        let slot = self.items.new_item_ptr(max.max(self.items.max_items))?;
        match at {
            Some((m, t)) => {
                self.items.set_item(slot, None, def, m, t)?;
                self.items.place_item(slot, &mut self.collision)?;
            }
            None => self.items.set_item(slot, None, def, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0; 3])?,
        }
        Ok(slot)
    }

    /// OpenChest(chest, opener).
    pub fn open_chest(&mut self, chest: usize, opener: Option<usize>) -> R<()> {
        let c = self.items.records[chest].clone();
        let chest_subtype = self.items.definitions[c.definition()].subtype();
        let mut content = c.i16_(0xf0);
        if content < 0 {
            return Ok(());
        }
        let mut d = content as usize;
        let mut guard = 0;
        while self.items.definitions.get(d).ok_or_else(|| super::Unsupported("chest content".into()))?.kind() == -1 {
            // A random definition: RandItemIdx(chest slot, choice count +4)
            // picks one of its +8 choices (RandomItemNum += 0x1b7).
            let n = self.items.definitions[d].subtype() as u32;
            let k = self.items.rand_item_idx(chest, n) as usize;
            content = self.items.definitions[d].i16_(8 + 2 * k);
            self.items.records[chest].set_i16(0xf0, content);
            d = usize::try_from(content).map_err(|_| super::Unsupported("random chest choice".into()))?;
            guard += 1;
            if 64 < guard {
                return unsupported("random chest content chain");
            }
        }
        let dk = self.items.definitions[d].kind();
        let ds = self.items.definitions[d].subtype();
        if (chest_subtype == 0x30 && dk == 1 && ds == 1) || chest_subtype == 0x2f {
            if chest_subtype == 0x30 {
                return unsupported("gold chest content model swap (chest_gold_info)");
            }
            let v = i32::from(self.items.definitions[d].i16_(0x40));
            self.items.records[chest].set_i32(0xf4, v);
            return Ok(());
        }
        if chest_subtype == 0x2c {
            let rr = &mut self.items.records[chest];
            rr.set_u16(0xd8, rr.u16_(0xd8) | 0x40);
            if self.sounds.rom.is_some() {
                self.events.push(Event::Sound { id: 0x2d, position: None, volume: 0xe0, priority: 0x14, queue: false });
            }
            return Ok(());
        }
        if dk == 1 && ds == 2 && 2 <= c.i16_(0x100) {
            d = self.items.find_info_index(1, 2, b"KEYRING").ok_or_else(|| super::Unsupported("KEYRING".into()))?;
            content = d as i16;
        }
        let _ = content;
        let dk = self.items.definitions[d].kind();
        let new = if dk == 1 && c.u32_(0xf8) != 0 {
            // Inside an open chest model node: created unplaced and parented.
            let n = self.add_item_sub(d, None)?;
            self.items.records[chest].set_u32(0xfc, n as u32 + 1);
            self.items.records[n].set_u32(0xfc, chest as u32 + 1);
            self.items.records[n].set_u8(0xdf, opener.map_or(0xff, |o| o as u8));
            return unsupported("chest content parented to the chest model node (presentation-dependent position)");
        } else {
            let n = self.add_item_sub(d, Some((c.matrix(), c.position())))?;
            let df = if chest_subtype == 0x2b {
                opener.map_or(0xfe, |o| o as u8)
            } else {
                if dk == 1 {
                    self.items.records[n].set_u32(0xfc, chest as u32 + 1);
                }
                opener.map_or(0xff, |o| o as u8)
            };
            self.items.records[n].set_u8(0xdf, df);
            n
        };
        let nd = &self.items.definitions[self.items.records[new].definition()];
        let (nk, ns) = (nd.kind(), nd.subtype());
        if nk == 4 {
            let rr = &mut self.items.records[new];
            rr.set_u32(0xf8, rr.u32_(0xf8) | 1);
            if rr.i16_(0xf0) == 0x1e && self.items.session.death_statue_anim && c.i16_(0x100) != 0 {
                return unsupported("death statue from a chest");
            }
            rr.set_u16(0x102, 0xffff);
            return Ok(());
        }
        if nk != 1 {
            return Ok(());
        }
        if ns == 2 {
            let n = i32::from(c.i16_(0x100));
            self.items.records[new].set_i32(0xf4, if 0 < n { n } else { 1 });
        } else if ns == 0xe {
            self.items.records[new].set_i32(0xf4, i32::from(c.i16_(0x100)));
        }
        self.items.records[new].set_u16(0x100, 0x1e);
        Ok(())
    }

    /// GrabPowerup(player, item) for the pending grab.
    pub fn grab_powerup(&mut self, player: usize, slot: usize, class: &ClassData) -> R<()> {
        let r = self.items.records[slot].clone();
        let def = self.items.definitions[r.definition()].clone();
        let idx = self.players[player].u32_(p::INDEX) as usize;
        let amount = r.i32_(0xf4);
        let stolen = |g: &mut Game, df: i8| {
            if 0 <= df && df as usize != idx {
                g.audio_player_stolen(idx);
            }
        };
        let mut expire = false;
        let df = r.i8_(0xdf);
        match def.subtype() {
            1 => {
                self.give_gold(idx, amount);
                let o = p::STATS + 0x1c * self.players[player].u32_(p::CHARACTER) as usize;
                let v = self.players[player].i32_(o);
                self.players[player].set_i32(o, amount.wrapping_add(v));
                self.audio_pickup_gold(idx, amount)?;
                self.got_it(idx, def.subtype(), amount);
                self.players[player].set_u16(p::STATUS_934, 1);
                if self.globals.cur_world == 0xc {
                    return unsupported("secret-world coin (PlayersGiveSecretCoin)");
                }
                if 0x18 < amount {
                    self.help(0x11, idx);
                    if 0 <= df {
                        stolen(self, df);
                    }
                }
                expire = true;
            }
            2 => {
                let keys = self.players[player].i32_(p::KEYS);
                let max = self.globals.max_keys_per_player;
                let gates = self.items.world_has_gates;
                if max < keys + amount {
                    if max <= keys {
                        self.help(4, idx);
                    } else {
                        let take = max - keys;
                        self.help(if gates == 0 { 2 } else { 8 }, idx);
                        self.players[player].set_i32(p::KEYS, take + keys);
                        self.items.records[slot].set_i32(0xf4, amount - take);
                        self.audio_key(idx);
                        self.got_it(idx, def.subtype(), amount - take);
                        self.players[player].set_u16(p::STATUS_934, 1);
                    }
                } else {
                    self.help(if gates == 0 { 2 } else { 8 }, idx);
                    self.players[player].set_i32(p::KEYS, keys + amount);
                    stolen(self, df);
                    self.audio_key(idx);
                    self.got_it(idx, def.subtype(), amount);
                    self.players[player].set_u16(p::STATUS_934, 1);
                    expire = true;
                }
            }
            3 => {
                let mut food = amount as f32;
                if self.players[player].u32_(p::POWERUP_FLAGS) & 0x400 != 0 && def.name() == b"CHICKEN" {
                    food = -100.0;
                }
                if 0.0 <= food {
                    if self.heal_player(food, idx)? == 0 {
                        self.help(0x85, idx);
                        return self.finish_grab(slot, false, df);
                    }
                } else {
                    self.damage_player(-food, idx, 0, 0x800, None)?;
                }
                if 100 <= amount {
                    self.help(0xf, idx);
                } else if 0x31 < amount {
                    self.help(0x10, idx);
                } else if amount < 0 {
                    self.help(0x1c, idx);
                }
                stolen(self, df);
                self.got_it(idx, def.subtype(), food as i32);
                if food < 0.0 {
                    self.players[player].set_u16(p::STATUS_934, 3);
                    self.audio_player_poison(idx);
                } else {
                    self.players[player].set_u16(p::STATUS_934, 1);
                    let n = def.name();
                    let which = if n == b"PINEAPPLE" { 3 } else if n == b"APPLE" { 1 } else if n == b"BANANA" { 2 } else { 0 };
                    self.audio_player_eat_sfx(idx, which)?;
                }
                expire = true;
            }
            4 => {
                let max = self.globals.max_potions_per_player;
                let mut have = self.players[player].i32_(p::POTIONS);
                if max <= have {
                    self.help(3, idx);
                    return self.finish_grab(slot, false, df);
                }
                let potion = u32::from_le_bytes(def.0[0x3c..0x40].try_into().unwrap());
                let mut i = 0;
                if 0 < amount {
                    loop {
                        self.players[player].set_u32(p::POTION_SLOTS + 4 * have as usize, potion);
                        have += 1;
                        self.players[player].set_i32(p::POTIONS, have);
                        i += 1;
                        if amount <= i || max <= have {
                            break;
                        }
                    }
                }
                self.help(7, idx);
                stolen(self, df);
                self.audio_pickup(idx);
                self.got_it(idx, def.subtype(), 0);
                self.players[player].set_u16(p::STATUS_934, 1);
                expire = true;
            }
            5..=9 => {
                let flags = r.u32_(0xf0);
                if def.subtype() == 9 && flags & 0xf000 != 0 && self.players[player].u32_(p::POWERUP_FLAGS) & 0xf000 != 0 {
                    return self.finish_grab(slot, false, df);
                }
                self.add_powerup(amount as f32, r.f32_(0xf8), idx, def.subtype(), flags, class)?;
                if let Some(h) = powerup_help(def.subtype(), flags) {
                    self.help(h, idx);
                }
                self.audio_pickup_special(idx, def.subtype(), flags);
                self.got_it(idx, def.subtype(), 0);
                self.players[player].set_u16(p::STATUS_934, 1);
                expire = true;
            }
            10 => {
                let rune = def.i16_(0x40) as i32;
                if self.player_has_rune(idx as i32, rune) {
                    self.help(0x5a, idx);
                } else {
                    for k in 0..4 {
                        self.player_give_rune(k, rune);
                    }
                    self.audio("AudioPickupRune", Some(idx), 0);
                    self.got_it(idx, def.subtype(), amount);
                    // RuneDispTimer = 300: HUD presentation.
                    self.start_gem_fx(r.v3(0x40), 0x400)?;
                    self.items.rune_item = None;
                    let mut bits = 0u32;
                    for k in 0..4 {
                        if self.players[k].i32_(0xfc) != 0 {
                            bits |= u32::from(self.players[k].u16_(0x1a26));
                        }
                    }
                    let n = (0..0xd).filter(|b| bits & (1 << b) != 0).count() as i32;
                    self.audio("AudioNumRunesFound", None, n);
                    expire = true;
                }
            }
            0xd => {
                self.help(amount + 0x71, idx);
                self.player_give_class_bit(idx, 0xcf4, amount);
                self.audio_pickup(idx);
                self.got_it(idx, def.subtype(), amount);
                expire = true;
            }
            0xe => {
                self.events.push(Event::ScrollMessage { player_mask: 1 << (idx & 0x1f), text: amount - 1 });
                expire = true;
            }
            0xf => {
                self.start_gem_fx(r.v3(0x40), amount)?;
                self.player_give_counted(0xd0a, 0x1d96, amount, false)?;
                self.audio_pickup(idx);
                self.got_it(idx, def.subtype(), amount);
                expire = true;
            }
            0x10 => {
                self.player_give_counted(0xd04, 0x1d90, amount, true)?;
                self.audio_pickup(idx);
                self.got_it(idx, def.subtype(), amount);
                self.start_gem_fx(r.v3(0x40), 0x100)?;
                expire = true;
            }
            _ => {}
        }
        self.finish_grab(slot, expire, df)
    }

    /// PlayerHasRune 0x1769a0(player or -1, rune): the rune bit among the
    /// active players' +0x1a26 and class-record (+0xcf2 + class × 0xb4) words.
    pub fn player_has_rune(&self, player: i32, rune: i32) -> bool {
        let range = if player < 0 { 0..4 } else { player as usize..player as usize + 1 };
        let mut bits = 0u32;
        for k in range {
            if self.players[k].i32_(0xfc) != 0 {
                let cls = self.players[k].u32_(p::CHARACTER) as usize;
                bits |= u32::from(self.players[k].u16_(0xcf2 + cls * 0xb4) | self.players[k].u16_(0x1a26));
            }
        }
        let want = if (0..0xe).contains(&rune) { 1u32 << rune } else { rune as u32 };
        bits & want & 0xffff == want & 0xffff
    }

    /// PlayerGiveRune 0x176a48 for one player (state 1 or 4): +0x1a26 bit.
    fn player_give_rune(&mut self, k: usize, rune: i32) {
        let st = self.players[k].i32_(0xfc);
        if st == 1 || st == 4 {
            let v = self.players[k].u16_(0x1a26) | (1u32 << (rune & 0x1f)) as u16;
            self.players[k].set_u16(0x1a26, v);
        }
    }

    /// PlayerGiveLegendItem 0x176d88: a bit in the class record word at `o`.
    fn player_give_class_bit(&mut self, k: usize, o: usize, bit: i32) {
        let st = self.players[k].i32_(0xfc);
        if st == 1 || st == 4 {
            let at = o + 0xb4 * self.players[k].u32_(p::CHARACTER) as usize;
            let v = self.players[k].u16_(at) | (1u32 << (bit & 0x1f)) as u16;
            self.players[k].set_u16(at, v);
        }
    }

    /// PlayerGiveCrystal 0x176e90 / PlayerGiveGargItem 0x177158 for every
    /// active player: the class-record count at `o` + 2n up to MAX_CRYSTALS
    /// / max_garg_items (the tower copy at `tower` too in CurWorld 0xd), and
    /// the +0x900 display id (n, or n + 0x100 for gargoyle items) for 3 s.
    fn player_give_counted(&mut self, o: usize, tower: usize, n: i32, garg: bool) -> R<()> {
        let table = if garg { &self.items.tables.max_garg_items } else { &self.items.tables.max_crystals };
        let max = *usize::try_from(n).ok().and_then(|k| table.get(k)).ok_or_else(|| Unsupported("crystal/garg item index".into()))?;
        for k in 0..4 {
            let st = self.players[k].i32_(0xfc);
            if st != 1 && st != 4 {
                continue;
            }
            let cls = 0xb4 * self.players[k].u32_(p::CHARACTER) as usize;
            let at = o + cls + 2 * n as usize;
            let c = i32::from(self.players[k].i16_(at));
            if 0 <= c && c < max {
                self.players[k].set_i16(at, c as i16 + 1);
                if self.globals.cur_world == 0xd {
                    let t = tower + cls + 2 * n as usize;
                    let v = self.players[k].i16_(t) + 1;
                    self.players[k].set_i16(t, v);
                }
            }
            self.players[k].set_i32(0x900, if garg { n + 0x100 } else { n });
            self.players[k].set_f32(0x904, 3.0);
        }
        Ok(())
    }

    /// StartGemFX 0x17f518(position, n): effect 0x4f (runes, 0x400), 0x4e
    /// (gargoyle items, 0x100) or 0x45 + n (crystals).
    fn start_gem_fx(&mut self, position: [f32; 3], n: i32) -> R<()> {
        let header = match n {
            0x400 => 0x4f,
            0x100 => 0x4e,
            n => usize::try_from(n + 0x45).map_err(|_| Unsupported("gem effect".into()))?,
        };
        let Some(h) = self.effects.tables.headers.get(header).copied().flatten() else { return Ok(()) };
        let i = self.start_fx_tree(0.0, super::effects::Visual::Header(header), h.frames, h.timing, Some(position), 0)?;
        self.fx_mut(i).set_u32(0x60, header as u32);
        Ok(())
    }

    fn finish_grab(&mut self, slot: usize, expire: bool, df: i8) -> R<()> {
        let mode = if df == -1 { 8 } else { 0xf };
        if expire {
            self.expire_item(slot, mode);
            if let Some(chest) = self.items.records[slot].u32_(0xfc).checked_sub(1) {
                // The chest's content pointer is cleared by expiring it too.
                self.expire_item(chest as usize, mode);
            }
        }
        Ok(())
    }
}

/// GrabPowerup's NewHelp ids for powerup subtypes 5..9 and their flags.
fn powerup_help(subtype: i32, f: u32) -> Option<i32> {
    match subtype {
        5 => Some(if f & 0x80000 != 0 {
            0x25
        } else if f & 0x400000 != 0 {
            0x2f
        } else if f & 0x200000 != 0 {
            0x26
        } else if f & 0x100000 != 0 {
            0x30
        } else if f & 0x1000_0000 != 0 {
            0x56
        } else if f & 0x2000_0000 != 0 {
            0x57
        } else {
            return match f & 0xf {
                1 => Some(0x28),
                2 => Some(0x29),
                3 => Some(0x2a),
                4 => Some(0x2b),
                _ => None,
            };
        }),
        6 => [(0x100000, 0x36), (0x10000, 0x23), (0x80000, 0x31), (0x20000, 0x34), (0x200000, 0x5b), (0x400000, 0x5c), (0x2000, 0x84)]
            .iter()
            .find(|(b, _)| f & b != 0)
            .map(|(_, h)| *h),
        7 => Some(0x20),
        8 => Some(0x21),
        9 => [
            (4, 0x24),
            (2, 0x27),
            (8, 0x33),
            (1, 0x35),
            (0x10, 0x51),
            (0x20, 0x52),
            (0x40, 0x53),
            (0x80, 0x54),
            (0x100, 0x58),
            (0x200, 0x59),
            (0x400, 0x5d),
            (0x2000, 0x62),
            (0x1000, 99),
            (0x8000, 100),
            (0x4000, 100),
            (0x80000, 0x71),
        ]
        .iter()
        .find(|(b, _)| f & b != 0)
        .map(|(_, h)| *h),
        _ => None,
    }
}
