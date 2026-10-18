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
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub};

/// Tower state: TowerInit 0x175778 (TowerNewPlayer, CrystalActiveTime),
/// do_players' welcome timer (welcome_timer_282), EnterTower's message
/// timers (tower_haveall_mtime, tower_door_mtime[9], tower_garg_mtime[3])
/// and Sumner (SumnerInit 0x174730 / SumnerAnimate 0x174848).
#[derive(Clone, Debug, Default)]
pub struct TowerState {
    pub new_player: bool,
    /// CrystalActiveTime 0x23f544: 0 = new player waiting, −1 = not new,
    /// otherwise the welcome time the crystals fade in from.
    pub crystal_active_time: f32,
    pub welcome_timer: i32,
    pub haveall_mtime: f32,
    pub door_mtime: [f32; 9],
    pub garg_mtime: [f32; 3],
    pub sumner: Option<SumnerState>,
    /// sumner_pidx (SumnerInit: 0) and sumner_speech_time.
    pub sumner_pidx: i32,
    pub sumner_speech_time: f32,
    /// sumner_plyr_newlevel[4]: the level each player reached a new tier at.
    pub plyr_newlevel: [i32; 4],
}

/// Sumner (the GWIZ tree of the level's item animations at lookout param 0):
/// SumnerDanim / SumnerAnimNow and his tree clock.
#[derive(Clone, Debug)]
pub struct SumnerState {
    pub matrix: [[f32; 4]; 4],
    pub danim: u16,
    pub anim_now: bool,
    pub clock: crate::animation_clock::ClockState,
    pub clips: Vec<crate::animation_clock::ClipTiming>,
    pub current: u16,
}

/// TowerInit: a new player when this is the session's first Tower entry and
/// no present player has experience in any class (+0x9ac + 0x18c).
pub fn tower_new_player(tower_counter: i32, players: &[super::record::Record]) -> bool {
    tower_counter == 0
        && !players.iter().any(|r| r.i32_(0xfc) != 0 && (0..16).any(|c| 0 < r.i32_(0x9ac + 0x18 * c)))
}

impl Game {
    fn class_at(&self, k: usize, o: usize) -> usize {
        o + 0xb4 * self.players[k].u32_(p::CHARACTER) as usize
    }

    /// PlayersSetWaveFinished(world, level) for a completed level with LEVL
    /// +0x90/+0x92 (its realm-progress bit indices): every player in state
    /// 1, 4 or 5 gets the class-record bits (+0xcf8/+0xcfa, +0xcfc/+0xcfe:
    /// the first and repeated completions) and the wave bit.
    pub fn players_set_wave_finished(&mut self, world: i32, level: i32, lv90: i16, lv92: i16) {
        self.globals.last_finished_world = world;
        self.globals.last_finished_level = level;
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

    /// EnterTower's simulation part: TowerCounter, the message timers, doors
    /// for delivered gargoyle items and crystals, the all-runes door (0xff),
    /// HideExits, then TowerCheckMessages(1).
    pub fn enter_tower(&mut self) -> R<()> {
        self.globals.tower_counter += 1;
        self.sumner_hint_time = 0.0;
        self.tower.haveall_mtime = add(self.globals.time, 1.0)?;
        self.tower.door_mtime = [0.0; 9];
        self.tower.garg_mtime = [0.0; 3];
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
        self.tower_check_messages(true)
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

    /// EndTower 0x126730's state part: LastFinishedWorld/Level = −1.
    pub fn end_tower(&mut self) {
        self.globals.last_finished_world = -1;
        self.globals.last_finished_level = -1;
    }

    /// SumnerInit: the GWIZ tree at lookout param 0's matrix (supplied by the
    /// caller from the level's item animations).
    pub fn sumner_init(&mut self, clips: Vec<crate::animation_clock::ClipTiming>) -> R<()> {
        let Some(l) = self.lookouts.iter().find(|l| l.param == 0) else { return Ok(()) };
        let matrix = super::lookouts::lookout_matrix(l)?;
        self.tower.sumner = Some(SumnerState { matrix, danim: 0, anim_now: false, clock: super::setup::fresh_clock(), clips, current: 0 });
        self.tower.sumner_pidx = 0;
        Ok(())
    }

    /// SumnerAnimate: AnimateATree(SumnerAtree, SumnerDanim, SumnerAnimNow ?
    /// 2 : 0); a finished sequence advances SumnerDanim through 0..=2.
    fn sumner_animate(&mut self) -> R<()> {
        let (atime, scale) = (self.globals.atime, self.globals.anim_time_scale);
        let Some(s) = self.tower.sumner.as_mut() else { return Ok(()) };
        let (Some(&current), Some(&requested)) = (s.clips.get(s.current as usize), s.clips.get(s.danim as usize)) else { return Ok(()) };
        let out = crate::animation_clock::select(
            s.clock,
            current,
            requested,
            crate::animation_clock::SelectionInput { sequence: s.danim, mode: if s.anim_now { 2 } else { 0 }, start_frame: 0, blend_duration: 0.0, playback_scale: 1.0, now: atime, anim_time_scale: scale },
        )?;
        s.clock = out.state;
        if out.return_flags & 1 != 0 {
            s.current = s.danim;
        }
        if out.return_flags & 2 != 0 {
            s.danim += 1;
            if 2 < s.danim {
                s.danim = 0;
            }
            s.anim_now = false;
        }
        Ok(())
    }

    /// do_players' welcome block (game mode 0x4010, WelcomeFlag ≥ 0, AuxLevels
    /// bit 0x10 clear): after the stage start, the Tower shows WelcomeMessage
    /// to a new player (Sumner's sequence 6, CrystalActiveTime = Time, the
    /// crystal camera) and GarmMessage after the Underworld's boss.
    pub fn welcome_check(&mut self) {
        let g = &self.globals;
        if !(g.game_mode == 0x4010 && 0 <= g.welcome_flag && g.aux_levels & 0x10 == 0) {
            self.globals.welcome_flag = -1;
            return;
        }
        if g.cam_wait != 0 || g.start_of_stage != 0 {
            self.tower.welcome_timer = 0;
            return;
        }
        self.tower.welcome_timer += g.vb_elapsed;
        if self.tower.welcome_timer < 1 {
            return;
        }
        if g.demo_mode != 0 {
            // DemoWelcome / DemoLevel: the demo build is not supported.
            return;
        }
        if g.cur_world == 0xd {
            if self.tower.new_player {
                self.events.push(Event::ScrollNamed { player_mask: u32::MAX, name: "WelcomeMessage", line: None, audio: -1 });
                if let Some(s) = self.tower.sumner.as_mut() {
                    s.danim = 6;
                    s.anim_now = true;
                }
                self.tower.crystal_active_time = self.globals.time;
                self.events.push(Event::CameraRequest { kind: "crystal", arg: 0 });
            }
            if self.globals.last_finished_world == 6 && self.globals.last_finished_level == 1 {
                self.events.push(Event::ScrollNamed { player_mask: u32::MAX, name: "GarmMessage", line: None, audio: -1 });
            }
        }
        self.globals.welcome_flag = -1;
    }

    /// ProcessItems' kind-1 case for a hidden (0x800) Tower crystal once
    /// CrystalActiveTime is set: the crystals within (Time − CAT + 1.75) × 15
    /// of the Tower's centre (x/z) fade in (tree alpha − 8 per update) and
    /// lose 0x800.
    pub(crate) fn crystal_fade(&mut self, slot: usize) -> R<()> {
        let cat = self.tower.crystal_active_time;
        if cat == 0.0 {
            return Ok(());
        }
        // FindLookoutParam(0) only gates the test; the distance is the x/z
        // fqdist from zerovector (the Tower's centre), as in the instructions.
        if 0.0 < cat && self.lookouts.iter().any(|l| l.param == 0) {
            let elapsed = add(sub(self.globals.time, cat)?, 1.75)?;
            let p = self.items.records[slot].v3(0x40);
            let (dist, _) = crate::vu0::fqdist(sub(p[0], 0.0)?, sub(p[2], 0.0)?)?;
            if mul(elapsed, 15.0)? < dist {
                return Ok(());
            }
        }
        let a = *self.item_alpha.get(&slot).unwrap_or(&0);
        if a - 8 < 1 {
            self.items.records[slot].and_u16(0xd8, 0xf7ff);
            self.item_alpha.insert(slot, 0);
        } else {
            self.item_alpha.insert(slot, a - 8);
        }
        Ok(())
    }

    /// AnyPlayerHasAllGargItems 0x177320 (n clamped to 2).
    pub(crate) fn any_player_has_all_garg_items(&self, n: i32) -> bool {
        let n = n.clamp(0, 2) as usize;
        let mut most = 0;
        for k in 0..4 {
            if self.players[k].i32_(0xfc) != 0 {
                if self.player_has_all(k, n, true) != 0 {
                    return true;
                }
                most = most.max(i32::from(self.players[k].i16_(self.class_at(k, 0xd04) + 2 * n)));
            }
        }
        self.items.tables.max_garg_items.get(n).is_some_and(|&m| m <= most)
    }

    pub(crate) fn any_player_has_all_crystals_pub(&self, n: i32) -> bool {
        usize::try_from(n).is_ok_and(|n| self.any_player_has_all_crystals(n))
    }

    /// TowerNeedCrystalsMsg 0x176828 / TowerNeedGargItemsMsg 0x176900: the
    /// NeedCrystals (NeedGargItems) line of door `n`, at most every 10 s.
    pub(crate) fn tower_need_msg(&mut self, mask: u8, n: usize, garg: bool) -> R<()> {
        let t = self.globals.time;
        let slot = if garg { self.tower.garg_mtime.get_mut(n) } else { self.tower.door_mtime.get_mut(n) };
        let Some(m) = slot else { return Ok(()) };
        if *m < t {
            *m = add(t, 10.0)?;
            let name = if garg { "NeedGargItems" } else { "NeedCrystals" };
            self.events.push(Event::ScrollNamed { player_mask: u32::from(mask), name, line: Some(n), audio: -1 });
        }
        Ok(())
    }

    /// TowerCheckMessages(param) (Tower only): SumnerAnimate (the speech
    /// presentation of SumnerDoSpeech and the event cameras are not ported);
    /// with `param` the newly collected shards and runes are merged into the
    /// class record and its backup; after 3 s, complete gargoyle-item and
    /// crystal sets are announced (UnlockSection / UnlockLevel with Sumner's
    /// speech) and marked delivered (−1, also in the backup).
    pub fn tower_check_messages(&mut self, param: bool) -> R<()> {
        if self.globals.cur_world != 0xd {
            return Ok(());
        }
        if self.globals.game_mode == 0x4010 {
            // SumnerDoSpeech: INTERIM — the speech runs down by SPF per update
            // (its captions, ghost and camera are not ported).
            if 0.0 < self.tower.sumner_speech_time && self.globals.cam_wait == 0 && self.globals.start_of_stage == 0 {
                self.tower.sumner_speech_time = sub(self.tower.sumner_speech_time, self.globals.spf)?;
            }
            self.sumner_animate()?;
        }
        if self.globals.cam_wait != 0 || self.globals.start_of_stage != 0 || self.globals.trig_cam_active != 0 || self.globals.paused != 0 {
            return Ok(());
        }
        self.sumner_level_speech_start();
        if param {
            for k in 0..4 {
                if self.players[k].i32_(0xfc) == 0 {
                    continue;
                }
                let c = self.class_at(k, 0);
                for (live, rec, backup) in [(0x1a24, 0xcf0, 0x1d7c), (0x1a26, 0xcf2, 0x1d7e)] {
                    let v = self.players[k].u16_(live);
                    let a = self.players[k].u16_(c + rec) | v;
                    self.players[k].set_u16(c + rec, a);
                    let b = self.players[k].u16_(c + backup) | v;
                    self.players[k].set_u16(c + backup, b);
                }
            }
        }
        if sub(self.globals.time, self.tower.haveall_mtime)? < 3.0 {
            return Ok(());
        }
        // Gargoyle items (+0xd04, backup +0x1d90) then crystals (+0xd0a,
        // backup +0x1d96).
        for (garg, count, rec, backup, name) in [(true, 3usize, 0xd04, 0x1d90, "UnlockSection"), (false, 9, 0xd0a, 0x1d96, "UnlockLevel")] {
            for n in 0..count {
                let max = if garg { self.items.tables.max_garg_items.get(n).copied() } else { self.items.tables.max_crystals.get(n).copied() };
                let Some(max) = max else { continue };
                if !garg && max == 0 {
                    continue;
                }
                let mut best = 0i32;
                for k in 0..4 {
                    if self.players[k].i32_(0xfc) == 0 {
                        continue;
                    }
                    let v = i32::from(self.players[k].i16_(self.class_at(k, rec) + 2 * n));
                    if 0 <= best && (v < 0 || best < v) {
                        best = v;
                    }
                }
                if best != max {
                    continue;
                }
                let table = if garg { "garg_sidx" } else { "crystal_sidx" };
                let audio = self.speech_sidx(table, n);
                if 0 <= audio {
                    self.queue(audio, 0xe0, 2);
                }
                self.events.push(Event::ScrollNamed { player_mask: u32::MAX, name, line: Some(n), audio });
                for k in 0..4 {
                    let o = self.class_at(k, rec) + 2 * n;
                    if i32::from(self.players[k].i16_(o)) == max {
                        self.players[k].set_i16(o, -1);
                        let b = self.class_at(k, backup) + 2 * n;
                        self.players[k].set_i16(b, -1);
                    }
                }
                self.tower.haveall_mtime = self.globals.time;
            }
        }
        Ok(())
    }

    /// SumnerLevelSpeechStart 0x174b08: for each present player, a new level
    /// tier since the last Tower visit (the backup's class experience vs
    /// +0x1a1c; reaching 99 always counts) queues Sumner's level speech;
    /// both the backup and the class entry take the current experience.
    fn sumner_level_speech_start(&mut self) -> i32 {
        if self.tower.sumner_pidx < 0 || 0.0 < self.tower.sumner_speech_time {
            return 0;
        }
        let mut n = 0;
        for k in 0..4 {
            self.tower.plyr_newlevel[k] = 0;
            if self.players[k].i32_(0xfc) == 0 {
                continue;
            }
            let c = self.players[k].u32_(0xc) as usize;
            let exp = self.players[k].i32_(0x1a1c);
            let old = crate::player_class::exp_to_level(self.players[k].i32_(0x1a38 + 0x18 * c));
            let new = crate::player_class::exp_to_level(exp);
            if new < 99 || 98 < old {
                if old / 10 != new / 10 {
                    self.tower.plyr_newlevel[k] = new;
                    n += 1;
                }
            } else {
                self.tower.plyr_newlevel[k] = new;
                n += 1;
            }
            self.players[k].set_i32(0x1a38 + 0x18 * c, exp);
            self.players[k].set_i32(0x9ac + 0x18 * c, exp);
        }
        if n != 0 {
            // SumnerGhostStart, 0.5 s of speech, DisablePlayerControls: the
            // caption presentation is not ported yet.
            self.tower.sumner_speech_time = 0.5;
            self.events.push(Event::SumnerAnim { player: usize::MAX });
        }
        n
    }

    /// crystal_sidx 0x23dd68 / garg_sidx 0x23dd90 (AudioCrystalSpeech /
    /// AudioGargSpeech sound ids).
    fn speech_sidx(&self, table: &str, n: usize) -> i32 {
        self.speech_tables.get(table).and_then(|v| v.get(n)).copied().unwrap_or(-1)
    }
}
