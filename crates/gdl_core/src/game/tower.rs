//! The Tower hub (CurWorld 0xd) and realm progress: PlayersSetWaveFinished
//! 0x1774c8, EnterTower 0x175838 (door triggers and HideExits 0x15b690),
//! ActivateSpecialTrigger 0x157300, WorldOpen 0x177778 and the player
//! progress queries (PlayerHasShard 0x176b60, PlayerHasAllCrystals
//! 0x176ff0, PlayerHasAllGargItems 0x1772a0, AnyPlayerHasAllCrystals
//! 0x177070, PlayerWaves 0x177658).
//!
//! Player progress lives in the class record (+0xcf0 + class × 0xb4) and the
//! wave bytes (+0x182c + class × 0xe + world). Not modelled: the secret
//! Sumner character (player_sumner_desc 0x220ed0 is a runtime pointer; every
//! player is treated as not Sumner), DemoMode, the shard/rune display
//! effects, exit model swaps and TowerCheckMessages (Sumner's speeches and
//! window cameras).
use super::pickups::p;
use super::{Event, Game, R};

impl Game {
    fn class_at(&self, k: usize, o: usize) -> usize {
        o + 0xb4 * self.players[k].u32_(p::CHARACTER) as usize
    }

    /// PlayersSetWaveFinished(world, level) for a completed level with LEVL
    /// +0x90/+0x92 (its realm-progress bit indices): every player in state
    /// 1, 4 or 5 gets the class-record bits (+0xcf8/+0xcfa, +0xcfc/+0xcfe:
    /// the first and repeated completions) and the wave bit.
    pub fn players_set_wave_finished(&mut self, world: i32, level: i32, lv90: i16, lv92: i16) {
        for k in 0..4 {
            let st = self.players[k].i32_(0xfc);
            if !matches!(st, 1 | 4 | 5) {
                continue;
            }
            if 0 < lv90 {
                let bit = 1u16 << ((lv90 - 1) & 0x1f);
                let a = self.class_at(k, 0xcf8);
                if self.players[k].u16_(a) & bit == 0 {
                    let v = self.players[k].u16_(a) | bit;
                    self.players[k].set_u16(a, v);
                } else {
                    let b = self.class_at(k, 0xcfa);
                    let v = self.players[k].u16_(b) | bit;
                    self.players[k].set_u16(b, v);
                }
            }
            if 0 < lv92 {
                let bit = 1u16 << (lv92 & 0x1f);
                let a = self.class_at(k, 0xcfc);
                if self.players[k].u16_(a) & bit == 0 {
                    let v = self.players[k].u16_(a) | bit;
                    self.players[k].set_u16(a, v);
                } else {
                    let b = self.class_at(k, 0xcfe);
                    let v = self.players[k].u16_(b) | bit;
                    self.players[k].set_u16(b, v);
                }
            }
            if let Ok(w) = usize::try_from(world) {
                let at = 0x182c + 0xe * self.players[k].u32_(p::CHARACTER) as usize + w;
                let v = self.players[k].u8_(at) | (1u32 << (level & 0x1f)) as u8;
                self.players[k].set_u8(at, v);
            }
        }
    }

    /// PlayerWaves(player, world): the completed-level bits of that world.
    fn player_waves(&self, k: usize, world: usize) -> u32 {
        u32::from(self.players[k].u8_(0x182c + 0xe * self.players[k].u32_(p::CHARACTER) as usize + world))
    }

    /// PlayerHasShard(-1, mask): the shard bits of every present player
    /// (class record +0xcf0 | +0x1a24) contain `mask` (< 0xf: one bit).
    fn players_have_shards(&self, mask: u32) -> bool {
        let mut bits = 0u32;
        for k in 0..4 {
            if self.players[k].i32_(0xfc) != 0 {
                bits |= u32::from(self.players[k].u16_(self.class_at(k, 0xcf0)) | self.players[k].u16_(0x1a24));
            }
        }
        let want = if mask < 0xf { 1 << mask } else { mask };
        bits & want & 0xffff == want & 0xffff
    }

    /// PlayerHasAllCrystals / PlayerHasAllGargItems: 0 absent or short, 1 the
    /// count equals the maximum, 2 delivered (count < 0).
    fn player_has_all(&self, k: usize, n: usize, garg: bool) -> i32 {
        if self.players[k].i32_(0xfc) == 0 {
            return 0;
        }
        let (o, table) = if garg { (0xd04, &self.items.tables.max_garg_items) } else { (0xd0a, &self.items.tables.max_crystals) };
        let c = i32::from(self.players[k].i16_(self.class_at(k, o) + 2 * n));
        if c < 0 {
            return 2;
        }
        i32::from(table.get(n).is_some_and(|&m| c == m))
    }

    /// AnyPlayerHasAllCrystals(n).
    fn any_player_has_all_crystals(&self, n: usize) -> bool {
        let mut most = 0;
        for k in 0..4 {
            if self.players[k].i32_(0xfc) != 0 {
                if self.player_has_all(k, n, false) != 0 {
                    return true;
                }
                most = most.max(i32::from(self.players[k].i16_(self.class_at(k, 0xd0a) + 2 * n)));
            }
        }
        self.items.tables.max_crystals.get(n).is_some_and(|&m| m <= most)
    }

    /// WorldOpen(world): Temple/Hell/Battle need shards (Hell also all
    /// runes), the Tower is open, the rest need their crystals.
    fn world_open(&self, world: i32) -> bool {
        match world {
            6 => self.players_have_shards(0x3fe) && self.player_has_rune(-1, 0xfff),
            5 => self.players_have_shards(0x1fe),
            8 => self.players_have_shards(0x7fe),
            0xd => true,
            w => usize::try_from(w)
                .ok()
                .and_then(|w| self.world_crystal.get(w))
                .and_then(|&n| usize::try_from(n).ok())
                .is_some_and(|n| self.any_player_has_all_crystals(n)),
        }
    }

    /// ActivateSpecialTrigger(id, instant): every active type-5 trigger item
    /// with +0xf6 == id (and its +0xf8 chain) fires: +0xd8 |= 0x400, +0xdc/
    /// +0xde = 2, the bound object's +0x16 = 0x2f; instant elevators jump to
    /// their end height, world-animated objects get flags 0x200000 (and
    /// 0xa00000 when instant).
    pub fn activate_special_trigger(&mut self, id: u8, instant: bool) -> R<()> {
        for s in 0..self.items.records.len() {
            let r = &self.items.records[s];
            if r.i16_(0xd8) == -1 || r.u16_(0xd8) & 0x8100 != 0 || self.items.definitions[r.definition()].kind() != 5 || r.u8_(0xf6) != id {
                continue;
            }
            let mut k = Some(s);
            let mut guard = 0;
            while let Some(i) = k {
                let rr = &mut self.items.records[i];
                rr.or_u16(0xd8, 0x400);
                rr.set_u8(0xdc, 2);
                rr.set_u8(0xde, 2);
                if let Some(o) = rr.u32_(0xf0).checked_sub(1).map(|v| v as usize) {
                    let old = self.items.objects[o].flags_10;
                    self.items.objects[o].byte_16 = 0x2f;
                    if old & 0x200_0000 == 0 {
                        if instant && let Some(w) = self.items.wobjs.iter_mut().find(|w| w.object == o) {
                            w.y = w.endy;
                            let y = crate::numeric::add_guarded_rz(w.inity, w.endy)?;
                            self.items.object_node_y[o] = y;
                        }
                    } else {
                        self.items.objects[o].byte_17 = 0x2f;
                        self.items.objects[o].flags_10 = if instant { old | 0xa0_0000 } else { old | 0x20_0000 };
                    }
                }
                k = self.items.records[i].u32_(0xf8).checked_sub(1).map(|v| v as usize);
                guard += 1;
                if self.items.records.len() < guard {
                    return super::unsupported("special trigger chain cycle");
                }
            }
        }
        Ok(())
    }

    /// EnterTower's simulation part: doors for delivered gargoyle items and
    /// crystals, the all-runes door (0xff), then HideExits.
    pub fn enter_tower(&mut self) -> R<()> {
        self.sumner_hint_time = 0.0;
        for g in 0..3 {
            if (0..4).any(|k| self.player_has_all(k, g, true) == 2) {
                self.activate_special_trigger(g as u8 + 0x65, true)?;
                if g == 2 {
                    self.activate_special_trigger(0x68, true)?;
                    self.activate_special_trigger(199, true)?;
                }
            }
        }
        for n in 0..9 {
            if self.items.tables.max_crystals.get(n).is_some_and(|&m| m != 0) && (0..4).any(|k| self.player_has_all(k, n, false) == 2) {
                self.activate_special_trigger(n as u8, true)?;
            }
        }
        let mut runes = 0u16;
        for k in 0..4 {
            if self.players[k].i32_(0xfc) != 0 {
                runes |= self.players[k].u16_(self.class_at(k, 0x1d7e));
            }
        }
        if runes & 0x1000 != 0 {
            self.activate_special_trigger(0xff, true)?;
        }
        self.hide_exits();
        Ok(())
    }

    /// HideExits: exits to levels no present player has reached (level n
    /// needs bit n−1 of that world's wave byte) or to closed realms (Temple,
    /// Hell, Battle; Battle 3 also needs all runes) are disabled (+0xd8 =
    /// 0x8000; the "inactive" exit model is presentation).
    fn hide_exits(&mut self) {
        let mut waves = [0u32; 14];
        for k in 0..4 {
            if self.players[k].i32_(0xfc) != 0 {
                for (w, v) in waves.iter_mut().enumerate() {
                    *v |= self.player_waves(k, w);
                }
            }
        }
        for s in 0..self.items.records.len() {
            let r = &self.items.records[s];
            if r.i16_(0xd8) == -1 || self.items.definitions[r.definition()].kind() != 9 {
                continue;
            }
            let wave = i32::from(r.i16_(0xf0));
            let world = wave >> 8;
            let level = wave & 0xff;
            let bit_rule = |w: i32| -> bool { 1 <= level && usize::try_from(w).ok().and_then(|w| waves.get(w)).is_some_and(|v| (v >> (level - 1)) & 1 == 0) };
            let hidden = if world < 5 {
                bit_rule(world)
            } else if world < 7 {
                !self.world_open(world)
            } else if world != 8 {
                bit_rule(world)
            } else if !self.world_open(8) {
                true
            } else if level != 3 {
                bit_rule(8)
            } else {
                !self.player_has_rune(-1, 0x1fff)
            };
            if hidden {
                self.items.records[s].set_u16(0xd8, 0x8000);
                self.events.push(Event::ExitHidden { slot: s });
            }
        }
    }
}
