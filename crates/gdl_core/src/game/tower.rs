//! The Tower hub (CurWorld 0xd) and realm progress: PlayersSetWaveFinished
//! 0x1774c8, EnterTower 0x175838 (door triggers and HideExits 0x15b690),
//! ActivateSpecialTrigger 0x157300, WorldOpen 0x177778 and the player
//! progress queries (PlayerHasShard 0x176b60, PlayerHasAllCrystals
//! 0x176ff0, PlayerHasAllGargItems 0x1772a0, AnyPlayerHasAllCrystals
//! 0x177070, PlayerWaves 0x177658).
//!
//! Player progress lives in the class record (+0xcf0 + class × 0xb4) and the
//! wave bytes (+0x182c + class × 0xe + world). A player whose descriptor
//! (+0x104) is player_sumner_desc (the secret Sumner character, set by
//! change_player type 0x10) has every wave, crystal and gargoyle item, all
//! shards and runes at EnterTower, and is left out of TowerCheckMessages'
//! deliveries and SumnerLevelSpeechStart. Not modelled: DemoMode and exit
//! model swaps.
//! TowerCheckMessages 0x175e88 drives Sumner's level/shard/rune speeches
//! (SumnerDoSpeech, SumnerSpeechEnd, the sumner_event_flag steps).
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
    /// SumnerDoSpeech: sumner_speech_frame, sumner_speech_idx (−1 none,
    /// 0..=8 shards, ≥ 100 runes), sumner_delay, sumner_cam_active,
    /// sumner_plyr_sidx (the speech sound started), the caption state
    /// (CaptionText's msgidx / startframe / delay) and the ghost's matrix
    /// (SumnerGhostStart at the nearest waypoint; presentation).
    pub speech_frame: i32,
    pub speech_idx: i32,
    pub delay: i32,
    pub cam_active: bool,
    pub plyr_sidx: i32,
    pub caption: (i32, i32, i32),
    /// sumner_plyr_fxidx / fxidx2: the level-up and gem effects started.
    pub plyr_fxidx: i32,
    pub plyr_fxidx2: i32,
    /// SumnerGhostNode (the WIZARD tree; SumnerGhostDanim stays 0) and the
    /// tree's sequences (None: item_anim has no WIZARD tree).
    pub ghost: Option<GhostState>,
    pub ghost_clips: Option<Vec<crate::animation_clock::ClipTiming>>,
    /// sumner_event_flag / sumner_event_timer / sumner_event_shownode (a
    /// world node) / sumner_event_showitem (an item slot): TowerCheckMessages'
    /// steps after a shard or rune speech.
    pub event_flag: i32,
    pub event_timer: i32,
    pub event_shownode: Option<usize>,
    pub event_showitem: Option<usize>,
}

/// Sumner's ghost (SumnerGhostStart): its matrix and tree clock.
#[derive(Clone, Debug)]
pub struct GhostState {
    pub matrix: [[f32; 4]; 4],
    pub clock: crate::animation_clock::ClockState,
    pub current: u16,
}

const IDENTITY: [[f32; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
/// LevelUpEffects 0x2407e8 (effect headers per player colour).
pub(super) const LEVEL_UP_EFFECTS: [i32; 4] = [0x39, 0x3a, 0x3b, 0x3c];
/// gem_color.39 0x23f588 (StartGemFX argument per player colour).
const GEM_COLOR: [i32; 4] = [6, 4, 2, 5];

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

    /// PlayerHasShard(p, n) for one present player (class +0xcf0 | +0x1a24;
    /// n < 0xf is one bit).
    pub(crate) fn player_has_shard(&self, k: usize, n: u32) -> bool {
        if self.players[k].i32_(0xfc) == 0 {
            return false;
        }
        let bits = u32::from(self.players[k].u16_(self.class_at(k, 0xcf0)) | self.players[k].u16_(0x1a24));
        let want = if n < 0xf { 1 << n } else { n };
        bits & want & 0xffff == want & 0xffff
    }

    /// Player k's descriptor is player_sumner_desc (the Tower's test; the
    /// profile's also checks class 2).
    fn is_sumner_desc(&self, k: usize) -> bool {
        self.players[k].u32_(0x104) == super::pselect::SUMNER_DESC
    }

    /// PlayerWaves(player, world): the completed-level bits of that world
    /// (Sumner: every level, 0xffffffff).
    fn player_waves(&self, k: usize, world: usize) -> u32 {
        if self.is_sumner_desc(k) {
            return 0xffff_ffff;
        }
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
        // Sumner has delivered everything.
        if self.is_sumner_desc(k) {
            return 2;
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
    pub fn world_open(&self, world: i32) -> bool {
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
                    self.sync_collision_object(o);
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
        // The delivered shards (class backup +0x1d7c, bits 1..=8) stand in
        // the window (SHARD<n> at L1WINDOWFRAME); without all eight the
        // light ray is hidden: MBTreeSetFlags(L1XPLIGHTRAY01, 2, 0). A
        // Sumner player replaces the bits so far with 0x7fe.
        let mut shards = 0u16;
        for k in 0..4 {
            if self.players[k].i32_(0xfc) != 0 {
                if self.is_sumner_desc(k) {
                    shards = 0x7fe;
                } else {
                    shards |= self.players[k].u16_(self.class_at(k, 0x1d7c));
                }
            }
        }
        if shards != 0
            && let Some(o) = self.find_world_object("L1WINDOWFRAME")
        {
            for n in 1..9 {
                if (shards >> n) & 1 != 0 {
                    self.tower_display_fx(&format!("SHARD{n}"), o)?;
                }
            }
        }
        if shards & 0x1fe != 0x1fe {
            self.world_node_set_flags("L1XPLIGHTRAY01", 2);
        }
        // Runes (class backup +0x1d7e): the all-runes door (0xff) with rune
        // 13, runes 1..=12 at L1RUNEPLACE, rune 13 at L1RUNE13 (Sumner:
        // 0x1fff, as for the shards).
        let mut runes = 0u16;
        for k in 0..4 {
            if self.players[k].i32_(0xfc) != 0 {
                if self.is_sumner_desc(k) {
                    runes = 0x1fff;
                } else {
                    runes |= self.players[k].u16_(self.class_at(k, 0x1d7e));
                }
            }
        }
        if runes != 0 {
            if runes & 0x1000 != 0 {
                self.activate_special_trigger(0xff, true)?;
            }
            if let Some(o) = self.find_world_object("L1RUNEPLACE") {
                for n in 0..0xc {
                    if (runes >> n) & 1 != 0 {
                        self.tower_display_fx(&format!("RUNE{}", n + 1), o)?;
                    }
                }
            }
            if runes & 0x1000 != 0
                && let Some(o) = self.find_world_object("L1RUNE13")
            {
                self.tower_display_fx("RUNE13", o)?;
            }
        }
        self.hide_exits();
        self.tower_check_messages(true)
    }

    /// TowerSetStartPos / ClosestStartPos 0x150958: among the start
    /// locators `starts` (id, position) of open realms (WorldOrder[id]),
    /// the closest to the players' average position (GetPlayerAvgPos: the
    /// players in state 1).
    pub fn closest_start_pos(&self, starts: &[(i16, [f32; 3])], order: &[i32]) -> i16 {
        let ps: Vec<[f32; 3]> = (0..4).filter(|&k| self.players[k].i32_(0xfc) == 1).map(|k| self.players[k].v3(0x50)).collect();
        let n = ps.len().max(1) as f32;
        let s = ps.iter().fold([0.0f32; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
        let avg = [s[0] / n, s[1] / n, s[2] / n];
        let mut best = (-1.0f32, 0i16);
        for id in 0..0xe {
            let Some(&(_, p)) = starts.iter().find(|(i, _)| *i == id) else { continue };
            if p[1] <= -100000.0 || !order.get(id as usize).is_some_and(|&w| self.world_open(w)) {
                continue;
            }
            let d = (p[0] - avg[0]).powi(2) + (p[1] - avg[1]).powi(2) + (p[2] - avg[2]).powi(2);
            if best.0 < 0.0 || d < best.0 {
                best = (d, id);
            }
        }
        best.1
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

    /// FindWorldObject 0x11c318 by exact label.
    pub fn find_world_object(&self, name: &str) -> Option<usize> {
        self.world_labels.iter().position(|l| l == name)
    }

    /// DoWorldEffects 0x1535c0 (SFXX flag 0x20 of player and critter effects):
    /// boss 0x2a counts DoRockFly up; bosses 0x29 and 0x2c hide the world
    /// object G5BIGDIRT / H4NSFFXL_PURPLE (MBTreeSetFlags 1).
    pub(crate) fn do_world_effects(&mut self) {
        match self.globals.boss_type {
            0x2a => self.do_rock_fly += 1,
            0x29 => self.world_node_set_flags("G5BIGDIRT", 1),
            0x2c => self.world_node_set_flags("H4NSFFXL_PURPLE", 1),
            _ => {}
        }
    }

    /// MBTreeSetFlags(node, bits, 0) on a named world node (presentation).
    pub fn world_node_set_flags(&mut self, name: &str, bits: u32) {
        if let Some(i) = self.find_world_object(name) {
            self.node_set_flags_at(i, bits);
        }
    }

    /// MBTreeClearFlags(node, bits, 0) on a named world node.
    pub fn world_node_clear_flags(&mut self, name: &str, bits: u32) {
        if let Some(i) = self.find_world_object(name) {
            self.node_clear_flags_at(i, bits);
        }
    }

    /// MBTreeSetFlags(node, bits, 0) on world node `i`.
    pub(crate) fn node_set_flags_at(&mut self, i: usize, bits: u32) {
        let (Some(s), Some(c)) = (self.node_mb_set.get(i), self.node_mb_clear.get(i)) else { return };
        if s & bits != bits || c & bits != 0 {
            self.node_mb_set[i] |= bits;
            self.node_mb_clear[i] &= !bits;
            self.node_state_version = self.node_state_version.wrapping_add(1);
        }
    }

    /// MBTreeClearFlags(node, bits, 0) on world node `i`.
    pub(crate) fn node_clear_flags_at(&mut self, i: usize, bits: u32) {
        let (Some(s), Some(c)) = (self.node_mb_set.get(i), self.node_mb_clear.get(i)) else { return };
        if c & bits != bits || s & bits != 0 {
            self.node_mb_clear[i] |= bits;
            self.node_mb_set[i] &= !bits;
            self.node_state_version = self.node_state_version.wrapping_add(1);
        }
    }

    /// EndTower 0x126730's state part: LastFinishedWorld/Level = −1.
    pub fn end_tower(&mut self) {
        self.globals.last_finished_world = -1;
        self.globals.last_finished_level = -1;
    }

    /// SumnerInit: the GWIZ tree at lookout param 0's matrix (supplied by the
    /// caller from the level's item animations).
    pub fn sumner_init(&mut self, clips: Vec<crate::animation_clock::ClipTiming>, ghost_clips: Option<Vec<crate::animation_clock::ClipTiming>>) -> R<()> {
        // Without lookout param 0 Sumner's node stays at identity.
        let matrix = match self.lookouts.iter().find(|l| l.param == 0) {
            Some(l) => super::lookouts::lookout_matrix(l)?,
            None => IDENTITY,
        };
        self.tower.sumner = Some(SumnerState { matrix, danim: 0, anim_now: false, clock: super::setup::fresh_clock(), clips, current: 0 });
        self.tower.ghost_clips = ghost_clips;
        // SumnerInit's globals (CaptionText's state is CaptionTextReset's).
        self.tower.ghost = None;
        self.tower.cam_active = false;
        self.tower.sumner_pidx = 0;
        self.tower.plyr_fxidx = -1;
        self.tower.plyr_fxidx2 = -1;
        self.tower.plyr_sidx = -1;
        self.tower.delay = 0x3c;
        self.tower.speech_idx = -1;
        self.tower.event_flag = 0;
        self.tower.caption = (0, 0, 0x3c);
        Ok(())
    }

    fn ghost_animate(&mut self) -> R<()> {
        let (atime, scale) = (self.globals.atime, self.globals.anim_time_scale);
        // AnimateATree(SumnerGhostAtree, SumnerGhostDanim 0, 0).
        if let (Some(g), Some(clips)) = (self.tower.ghost.as_mut(), self.tower.ghost_clips.as_ref())
            && let Some(&clip) = clips.first()
        {
            let out = crate::animation_clock::select(
                g.clock,
                clips.get(g.current as usize).copied().unwrap_or(clip),
                clip,
                crate::animation_clock::SelectionInput { sequence: 0, mode: 0, start_frame: 0, blend_duration: 0.0, playback_scale: 1.0, now: atime, anim_time_scale: scale },
            )?;
            g.clock = out.state;
            if out.return_flags & 1 != 0 {
                g.current = 0;
            }
        }
        Ok(())
    }

    /// SumnerAnimate: AnimateATree(SumnerAtree, SumnerDanim, SumnerAnimNow ?
    /// 2 : 0); a finished sequence advances SumnerDanim through 0..=2.
    fn sumner_animate(&mut self) -> R<()> {
        self.ghost_animate()?;
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
                if let Err(e) = self.crystal_cam_activate() {
                    self.log.note("welcome", &e);
                }
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

    /// SumnerDoSpeech 0x1750a0: after sumner_delay and 120 vblanks, either
    /// the caption of each player with a new level tier (GameText 0x18
    /// formatted with the colour 0x16 and class 0x17 names, the level and
    /// the rank list text; StartLevelUpFX after 0xf0 frames, StartGemFX
    /// after 0x10e) or the shard/rune caption (SCROLL_E NewShards line n,
    /// MoreShards, AllShards, NewRunes line 0, Rune13No, All12RunesNo,
    /// All12RunesYes, Rune13Yes) at y 0x138; once the caption is shown the
    /// speech time runs out and SumnerSpeechEnd follows. Sumner's camera
    /// at the nearest waypoint while he speaks. A player's level speech
    /// ends once the caption is revealed, 0x168 frames passed and its line
    /// (AudioExp's sound) no longer plays (AudioSoundPlaying; the track
    /// timing of audio_queue).
    fn sumner_do_speech(&mut self) -> R<()> {
        let t = &mut self.tower;
        if t.sumner_speech_time <= 0.0 || self.globals.cam_wait != 0 || self.globals.start_of_stage != 0 {
            return Ok(());
        }
        if 0 < t.delay {
            t.delay -= self.globals.vb_elapsed;
            return Ok(());
        }
        let level_up = 0 <= t.sumner_pidx && t.plyr_newlevel.iter().any(|&n| 0 < n);
        if !t.cam_active {
            self.sumner_ghost_start();
            let avg = {
                let ps: Vec<[f32; 3]> = (0..4).filter(|&k| self.players[k].i32_(0xfc) == 1).map(|k| self.players[k].v3(0x50)).collect();
                let n = ps.len().max(1) as f32;
                let s = ps.iter().fold([0.0f32; 3], |a, p| [a[0] + p[0], a[1] + p[1], a[2] + p[2]]);
                [s[0] / n, s[1] / n, s[2] / n]
            };
            if let Some(k) = self.find_closest_waypoint(100_000.0, avg, true)? {
                let l = self.lookouts[k];
                let m = super::lookouts::lookout_matrix(&l)?;
                if let Some(g) = self.tower.ghost.as_mut() {
                    g.matrix = m;
                }
                let idx = self.tower.speech_idx;
                let table = if level_up {
                    0
                } else if idx < 100 {
                    usize::from(0 <= idx)
                } else {
                    2
                };
                self.sumner_cam_activate(table, usize::try_from(l.param).unwrap_or(0))?;
            }
            self.tower.cam_active = true;
        }
        for k in 0..4 {
            self.players[k].set_u16(0x1d8, 0);
            self.players[k].set_u16(0x1da, 0);
        }
        self.tower.speech_frame += self.globals.vb_elapsed;
        let frame = self.tower.speech_frame - 0x78;
        if frame < 0 {
            return Ok(());
        }
        if 0 <= self.tower.sumner_pidx {
            while self.tower.sumner_pidx < 4 {
                let p = self.tower.sumner_pidx as usize;
                if 0 < self.tower.plyr_newlevel[p] {
                    let r = &self.players[p];
                    let (colour, class, lv) = (r.i32_(4), r.i32_(0xc), r.i32_(0x2ac0));
                    let h = &self.hud;
                    let fmt = h.string_text(0x18, 0).unwrap_or_default();
                    let rank = if lv == 99 {
                        h.string_text(0x15, 0)
                    } else {
                        let list = h.text.as_ref().and_then(|r| r.lists.first()).and_then(|(_, v)| v.get(class as usize).copied());
                        list.and_then(|e| h.string_text(e as i32, (lv / 10) >> 1))
                    }
                    .unwrap_or_default();
                    use super::hud::Arg;
                    let text = super::hud::format_c(&fmt, &[Arg::S(h.string_text(0x16, colour).unwrap_or_default()), Arg::S(h.string_text(0x17, class).unwrap_or_default()), Arg::D(lv), Arg::S(rank)]);
                    // CaptionTextSub(text, 6, frame / 2, y, 0.667) at
                    // 0x17536c: font_desc[6], font32 — not the entry's font
                    // (GameText 0x18's FONT 0 '8Hi_fonts5' matches nothing,
                    // so it would be font8x8; until 2026-11-13 the port drew
                    // that, the reference shows font32).
                    let font = h.tables.as_ref().and_then(|t| t.font_desc.iter().position(|n| n == "font32")).unwrap_or(6);
                    let done = self.hud.caption(f32::from_bits(0x3f2a_c083), &text, font, (frame >> 1) - self.tower.caption.1, 0x138);
                    if self.tower.plyr_sidx < 0 {
                        // sumner_plyr_sidx = AudioExp(...): the level line.
                        let ch = self.players[p].i32_(0xc);
                        self.tower.plyr_sidx = self.audio_exp(p, ch, lv);
                    }
                    if self.tower.plyr_fxidx < 0 && 0xef < frame {
                        // StartLevelUpFX(0, colour): LevelUpEffects[colour]
                        // (0x39 + colour), parented to the player's node.
                        self.tower.plyr_fxidx = self.sumner_player_fx(p, LEVEL_UP_EFFECTS.get(colour as usize).copied())?;
                    }
                    if self.tower.plyr_fxidx2 < 0 && 0x10d < frame {
                        // StartGemFX(0, gem_color[colour]): 0x45 + n.
                        let h = GEM_COLOR.get(colour as usize).map(|&n| match n {
                            0x400 => 0x4f,
                            0x100 => 0x4e,
                            n => 0x45 + n,
                        });
                        self.tower.plyr_fxidx2 = self.sumner_player_fx(p, h)?;
                    }
                    if !done || frame < 0x168 || self.audio_sound_playing(self.tower.plyr_sidx) {
                        break;
                    }
                    self.tower.caption = (0, 0, 0x3c);
                    self.tower.speech_frame = 0x78;
                    self.tower.plyr_fxidx = -1;
                    self.tower.plyr_fxidx2 = -1;
                    self.tower.plyr_sidx = -1;
                }
                self.tower.sumner_pidx += 1;
            }
            if self.tower.sumner_pidx == 4 {
                self.tower.sumner_pidx = -1;
            }
            return Ok(());
        }
        let idx = self.tower.speech_idx;
        // (message, line: −1 pages through the message's lines).
        let mut msg: Option<(&str, i32)> = None;
        if idx < 100 {
            if (0..9).contains(&idx) {
                msg = Some(("NewShards", idx));
                if self.tower.plyr_sidx < 1 {
                    self.audio_shard_speech(idx);
                    self.tower.plyr_sidx = 1;
                }
            } else if idx == 0xf || idx == 0x10 {
                msg = Some((if idx == 0xf { "MoreShards" } else { "AllShards" }, -1));
                if self.tower.plyr_sidx < 2 {
                    self.audio_shard_speech(idx);
                    self.tower.plyr_sidx = 2;
                }
            }
        } else {
            let r = idx - 100;
            if r < 0xd {
                msg = Some(("NewRunes", 0));
                if self.tower.plyr_sidx < 1 {
                    self.audio_rune_speech(0);
                    self.tower.plyr_sidx = 1;
                }
            } else {
                let name = match r {
                    0xd => Some("Rune13No"),
                    0x19 => Some("All12RunesNo"),
                    0x1a => Some("All12RunesYes"),
                    0x24 => Some("Rune13Yes"),
                    _ => None,
                };
                if let Some(name) = name {
                    msg = Some((name, -1));
                    if self.tower.plyr_sidx < 2 {
                        self.audio_rune_speech(r);
                        self.tower.plyr_sidx = 2;
                    }
                }
            }
        }
        let shown = match msg {
            Some((name, line)) => self.caption_text(name, line, frame >> 1, 0x138).unwrap_or(true),
            None => true,
        };
        if shown {
            self.tower.sumner_speech_time = sub(self.tower.sumner_speech_time, self.globals.spf)?;
        }
        if self.tower.sumner_speech_time <= 0.0 {
            self.sumner_speech_end()?;
        }
        Ok(())
    }

    /// CaptionText 0x165568(0, message, line, frame, y) over the scroll
    /// text (SCROLL_E): line ≥ 0 shows that line from its start; −1 shows
    /// caption_msgidx and, once it is revealed, waits caption_delay (0x3c
    /// vblanks) before the next line. None: no such message
    /// (FindScrollMessage −1).
    fn caption_text(&mut self, name: &str, line: i32, frame: i32, y: i32) -> Option<bool> {
        let rom = self.hud.scrolls.as_ref()?;
        let e = rom.entry(&name.to_uppercase())?;
        if 0 <= line {
            self.tower.caption.0 = line;
            self.tower.caption.1 = 0;
        }
        let (idx, start, _) = self.tower.caption;
        let text = e.lines.get(usize::try_from(idx).ok()?).cloned();
        let count = e.lines.len() as i32;
        let scale = mul(e.scale, f32::from_bits(0x3f2a_c083)).ok()?;
        let font = rom.fonts.get(e.font as usize).and_then(|n| self.hud.tables.as_ref()?.font_desc.iter().position(|f| f == n)).unwrap_or(0);
        // CaptionTextSub of a missing line reports −1 (true).
        let Some(text) = text else { return Some(true) };
        let mut done = self.hud.caption(scale, &text, font, frame - start, y);
        if done && line < 0 && idx + 1 < count {
            // vb_elapsed_ctl: the unclamped vblank delta (ClockOncePerFrame),
            // vb_elapsed outside pauses and menus.
            self.tower.caption.2 -= self.globals.vb_elapsed;
            if self.tower.caption.2 < 1 {
                self.tower.caption = (idx + 1, frame, 0x3c);
            }
            done = false;
        }
        Some(done)
    }

    /// StartFXSub(0, header, 0, 0, …) then SfxSetParent to player `p`'s
    /// node: the effect slot or −1.
    pub(super) fn sumner_player_fx(&mut self, p: usize, header: Option<i32>) -> R<i32> {
        let Some(h) = header.and_then(|h| usize::try_from(h).ok()) else { return Ok(-1) };
        let k = self.start_fx_sub(0.0, h, None, 0)?;
        if 0 <= k
            && let Some(n) = self.effects.nodes[k as usize].as_mut()
        {
            n.parent = Some(super::effects::EffectParent { player: p, local: [0.0; 3], local_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], critter: None, enemy: None });
        }
        Ok(k)
    }

    /// AudioShardSpeech 0x161220: shard_sidx[n] (n < 9), 0xe00ac (0xf),
    /// 0x3000b (0x10); AudioAnyQueAdd4(−1, 10, 1, id, 0xe0, 0x7f, 2).
    fn audio_shard_speech(&mut self, n: i32) {
        let id = match n {
            n if n < 9 => usize::try_from(n).ok().map_or(-1, |n| self.speech_sidx("shard_sidx", n)),
            0xf => 0xe00ac,
            0x10 => 0x3000b,
            _ => -1,
        };
        self.speech_line(id);
    }

    /// AudioRuneSpeech 0x161358.
    fn audio_rune_speech(&mut self, n: i32) {
        let id = match n {
            0 => 0xe00a4,
            0xd => 0xe00a8,
            0x19 => 0xe00a5,
            0x1a => 0xe00a6,
            0x24 => 0xe00a7,
            _ => -1,
        };
        self.speech_line(id);
    }

    /// SumnerGhostStart 0x174998: the WIZARD tree of item_anim (AtreeInit
    /// flags 0xc00880) under the world root, at identity until
    /// SumnerDoSpeech copies the waypoint's matrix.
    fn sumner_ghost_start(&mut self) {
        if self.tower.ghost.is_none() && self.tower.ghost_clips.is_some() {
            self.tower.ghost = Some(GhostState { matrix: IDENTITY, clock: super::setup::fresh_clock(), current: 0 });
        }
    }

    /// SumnerShardSpeechStart 0x174a38(n).
    fn sumner_shard_speech_start(&mut self, n: i32) {
        self.sumner_ghost_start();
        let t = &mut self.tower;
        t.sumner_speech_time = 3.0;
        t.speech_frame = 0;
        t.speech_idx = n;
        t.caption = (0, 0, 0x3c);
        self.disable_controls();
    }

    /// SumnerRuneSpeechStart 0x174a88(n): runes 0..=0xd speak (index 100 +
    /// n); 0xe (all twelve runes and shard 9) goes straight to
    /// SumnerSpeechEnd.
    fn sumner_rune_speech_start(&mut self, n: i32) -> R<()> {
        self.sumner_ghost_start();
        self.tower.speech_idx = n + 100;
        if n < 0xe {
            self.tower.sumner_speech_time = 3.0;
            self.tower.speech_frame = 0;
        } else {
            self.tower.sumner_speech_time = 0.0;
            self.sumner_speech_end()?;
        }
        self.disable_controls();
        self.tower.caption = (0, 0, 0x3c);
        Ok(())
    }

    /// FindExit 0x15b968(id): the first active kind-9 item whose +0xf0 is
    /// `id` (world << 8 | level).
    pub(crate) fn find_exit(&self, id: i16) -> Option<usize> {
        (0..self.items.records.len()).find(|&s| {
            let r = &self.items.records[s];
            r.i16_(0xd8) != -1 && self.items.definitions[r.definition()].kind() == 9 && r.i16_(0xf0) == id
        })
    }

    /// MBTreeSetAlpha(item tree, a, 1): 0 opaque … 0xff transparent.
    fn item_set_alpha(&mut self, slot: Option<usize>, a: i32) {
        if let Some(s) = slot {
            self.item_alpha.insert(s, a);
        }
    }

    /// MBTreeSetAlpha(world node, a, 1).
    pub(crate) fn world_node_set_alpha(&mut self, node: Option<usize>, a: i32) {
        if let Some(n) = node
            && let Some(v) = self.node_alpha.get_mut(n)
        {
            let a = Some(a.clamp(0, 0xff) as u8);
            if *v != a {
                *v = a;
                self.node_state_version = self.node_state_version.wrapping_add(1);
            }
        }
    }

    /// InitCustomEffect(0, name, 0, 0) and StartFXSub(0, header, the world
    /// object's translation, 0x80000, 0x800); SfxSetParent to the world root
    /// is the default.
    fn sumner_world_fx(&mut self, name: &str, object: usize) -> R<Option<usize>> {
        let m = self.collision.object_matrix(object)?;
        let h = self.init_custom_effect(None, name, 0, 0);
        if h < 0 {
            return Ok(None);
        }
        let k = self.start_fx_sub(0.0, h as usize, Some([m[3][0], m[3][1], m[3][2]]), 0x80000)?;
        Ok(usize::try_from(k).ok())
    }

    /// EnterTower's display of a delivered shard or rune: the effect as
    /// SumnerSpeechEnd starts it, its tree clock set to its last frame
    /// (start ATime − frames / 30). AtreeKillPsys (shards) is presentation.
    fn tower_display_fx(&mut self, name: &str, object: usize) -> R<()> {
        let Some(k) = self.sumner_world_fx(name, object)? else { return Ok(()) };
        let h = self.fx(k).u32_(super::effects::fx::KIND) as usize;
        let frames = self.effects.tables.headers.get(h).copied().flatten().map_or(0, |h| h.frames);
        let start = sub(self.globals.atime, mul(f32::from(frames), f32::from_bits(0x3d08_8889))?)?;
        if let Some(n) = self.effects.nodes[k].as_mut() {
            n.start = start;
        }
        Ok(())
    }

    /// SumnerSpeechEnd 0x174cc8: the ghost goes, the trigger camera ends,
    /// controls return; then the speech's event: RUNE13 at L1RUNE13 (rune
    /// camera 1, the Battle 3 exit 0x803 hidden for its fade, event 0x1e
    /// with all 13 runes else 0x20); SHARD<n> at L1WINDOWFRAME (window
    /// camera 0, the light ray hidden; event 10 with all eight shards, the
    /// Temple exit 0x500 hidden for its fade, else 0xe); RUNE<n+1> at
    /// L1RUNEPLACE (rune camera 0; the Hell exit 0x600 hidden; events
    /// 0x14/0x15 with nine shards and twelve runes, else 0x18 with twelve
    /// runes, else 0x16). A missing object only logs (Errorf).
    fn sumner_speech_end(&mut self) -> R<()> {
        self.tower.ghost = None;
        self.trigger_camera_end();
        self.tower.cam_active = false;
        self.enable_controls();
        let idx = self.tower.speech_idx;
        if idx == 0x70 {
            let Some(o) = self.find_world_object("L1RUNE13") else { return Ok(()) };
            self.sumner_world_fx("RUNE13", o)?;
            self.rune_cam_activate(1)?;
            let e = self.find_exit(0x803);
            self.item_set_alpha(e, 0xff);
            self.tower.event_flag = if self.player_has_rune(-1, 0x1fff) { 0x1e } else { 0x20 };
        } else if idx < 100 {
            if (0..9).contains(&idx) {
                let Some(o) = self.find_world_object("L1WINDOWFRAME") else { return Ok(()) };
                self.sumner_world_fx(&format!("SHARD{idx}"), o)?;
                self.window_cam_activate(0)?;
                self.world_node_set_flags("L1XPLIGHTRAY01", 2);
                if !self.players_have_shards(0x1fe) {
                    self.tower.event_flag = 0xe;
                    self.audio_tower_fx(0xe);
                } else {
                    let e = self.find_exit(0x500);
                    self.item_set_alpha(e, 0xff);
                    self.tower.event_flag = 10;
                    self.audio_tower_fx(10);
                }
            }
        } else {
            let r = idx - 100;
            if r < 0xf
                && let Some(o) = self.find_world_object("L1RUNEPLACE")
            {
                if r < 0xd {
                    self.sumner_world_fx(&format!("RUNE{}", idx - 99), o)?;
                    self.rune_cam_activate(0)?;
                    self.audio_tower_fx(0x15);
                }
                let e = self.find_exit(0x600);
                self.item_set_alpha(e, 0xff);
                let runes = self.player_has_rune(-1, 0xfff);
                self.tower.event_flag = if self.players_have_shards(0x3fe) && runes {
                    if r < 0xd { 0x14 } else { 0x15 }
                } else if runes {
                    0x18
                } else {
                    0x16
                };
            }
        }
        Ok(())
    }

    /// TowerCheckMessages' sumner_event_flag switch: the steps after a
    /// shard/rune speech (sounds once the camera has held 5 vblanks, the
    /// next camera, an exit or the light ray fading in over 0xb4 vblanks as
    /// flag + 100 counts down, then the follow-up speech of index flag or
    /// flag + 100).
    fn sumner_event(&mut self) -> R<()> {
        let flag = self.tower.event_flag;
        let (delay, hold) = (self.globals.trig_cam_delay, self.trig_cam.hold);
        let done = delay < 5 && hold < 5;
        let speech = |g: &mut Self, idx: i32| {
            g.tower.sumner_speech_time = 2.0;
            g.tower.speech_frame = 0;
            g.tower.event_flag = 0;
            g.tower.speech_idx = idx;
            g.tower.caption = (0, 0, 0x3c);
        };
        let step = |g: &mut Self, gate: bool, fx: i32| {
            if gate {
                g.audio_tower_fx(fx);
                g.tower.event_flag += 1;
            }
        };
        match flag {
            0 => {}
            10 => step(self, hold <= 0xf9, 2),
            0xb => {
                if done {
                    self.window_cam_activate(1)?;
                    let e = self.find_exit(0x500);
                    self.tower.event_showitem = e;
                    self.item_set_alpha(e, 0xff);
                    let ray = self.find_world_object("L1XPLIGHTRAY01");
                    if ray.is_some() {
                        self.world_node_clear_flags("L1XPLIGHTRAY01", 2);
                    }
                    self.tower.event_shownode = ray;
                    self.world_node_set_alpha(ray, 0xff);
                    self.tower.event_timer = 0xb4;
                    self.tower.event_flag = 0x74;
                    self.audio_tower_fx(0xb);
                }
            }
            0xe => {
                if hold < 0xfa {
                    step(self, true, 2);
                } else if done {
                    speech(self, flag);
                }
            }
            0xf | 0x10 => {
                if done {
                    speech(self, flag);
                }
            }
            0x14 | 0x16 | 0x18 => step(self, hold < 0xfa, 1),
            0x15 => {
                let e = self.find_exit(0x600);
                self.tower.event_showitem = e;
                self.item_set_alpha(e, 0xff);
                if done {
                    self.rune_cam_activate(0)?;
                    self.tower.event_shownode = None;
                    self.tower.event_timer = 0xb4;
                    self.tower.event_flag = 0x7e;
                }
            }
            0x17 | 0x21 => self.tower.event_flag = 0,
            0x19 | 0x1a | 0x24 => {
                if done {
                    speech(self, flag + 100);
                }
            }
            0x1e | 0x20 => step(self, hold < 0xdc, 1),
            0x1f => {
                if done {
                    self.rune_cam_activate(2)?;
                    self.activate_special_trigger(0xff, false)?;
                    let e = self.find_exit(0x803);
                    self.tower.event_showitem = e;
                    self.item_set_alpha(e, 0xff);
                    self.tower.event_shownode = None;
                    self.tower.event_timer = 0xb4;
                    self.tower.event_flag = 0x88;
                }
            }
            f if f < 0 => self.tower.event_flag = 0,
            _ => {
                self.tower.event_timer -= self.globals.vb_elapsed;
                if self.tower.event_timer < 1 {
                    self.tower.event_flag -= 100;
                    self.tower.event_timer = 0;
                }
                let a = self.tower.event_timer * 0xff / 0xb4;
                let (node, item) = (self.tower.event_shownode, self.tower.event_showitem);
                self.world_node_set_alpha(node, a);
                self.item_set_alpha(item, a);
            }
        }
        Ok(())
    }

    /// TowerCheckMessages(param) (Tower only): SumnerDoSpeech, SumnerAnimate
    /// and the sumner_event_flag steps; outside trigger cameras, pauses and
    /// menus, SumnerLevelSpeechStart, and with `param` the newly collected
    /// shards and runes (+0x1a24 / +0x1a26 against the class backup) are
    /// merged into the class record and its backup and the lowest new one
    /// starts Sumner's speech (also Rune13No after Battle 2 without rune 13,
    /// and index 0xe with all twelve runes and shard 9); after 3 s, complete
    /// gargoyle-item and crystal sets are announced (UnlockSection /
    /// UnlockLevel with Sumner's speech) and marked delivered (−1, also in
    /// the backup).
    pub fn tower_check_messages(&mut self, param: bool) -> R<()> {
        if self.globals.cur_world != 0xd {
            return Ok(());
        }
        if self.globals.game_mode == 0x4010 {
            self.sumner_do_speech()?;
            self.sumner_animate()?;
        }
        if self.globals.cam_wait != 0 || self.globals.start_of_stage != 0 {
            return Ok(());
        }
        self.sumner_event()?;
        if self.globals.trig_cam_active != 0 || self.globals.paused != 0 || self.hud.menu_on != 0 {
            return Ok(());
        }
        self.sumner_level_speech_start();
        if param {
            let mut news = [0u32; 2];
            let mut olds = [0u32; 2];
            for (j, (live, rec, backup)) in [(0x1a24, 0xcf0, 0x1d7c), (0x1a26, 0xcf2, 0x1d7e)].into_iter().enumerate() {
                for k in 0..4 {
                    if self.players[k].i32_(0xfc) == 0 || self.is_sumner_desc(k) {
                        continue;
                    }
                    let c = self.class_at(k, 0);
                    let v = self.players[k].u16_(live);
                    olds[j] |= u32::from(self.players[k].u16_(c + backup));
                    news[j] |= u32::from(v);
                    let a = self.players[k].u16_(c + rec) | v;
                    self.players[k].set_u16(c + rec, a);
                    let b = self.players[k].u16_(c + backup) | v;
                    self.players[k].set_u16(c + backup, b);
                }
                if j == 0 {
                    let fresh = news[0] & (news[0] ^ olds[0]);
                    if let Some(n) = (1..9).find(|n| (fresh >> n) & 1 != 0) {
                        self.sumner_shard_speech_start(n);
                    }
                }
            }
            let fresh = news[1] & (news[1] ^ olds[1]);
            match (0..0xd).find(|n| (fresh >> n) & 1 != 0) {
                Some(n) => self.sumner_rune_speech_start(n)?,
                None => {
                    let g = &self.globals;
                    if g.last_world_played == 8 && g.last_level_played == 2 && olds[1] & 0x1000 == 0 {
                        self.sumner_rune_speech_start(0xd)?;
                    }
                    // Shard bit 0x200 of the newly collected shards.
                    let fresh_shards = news[0] & (news[0] ^ olds[0]);
                    if olds[1] & 0xfff == 0xfff && fresh_shards & 0x200 != 0 {
                        self.sumner_rune_speech_start(0xe)?;
                    }
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
                    if self.players[k].i32_(0xfc) == 0 || self.is_sumner_desc(k) {
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
                // AudioCrystalSpeech 0x161298 / AudioGargSpeech 0x1612f8.
                self.speech_line(audio);
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
            if self.players[k].i32_(0xfc) == 0 || self.is_sumner_desc(k) {
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
            // SumnerGhostStart (0x174c60; the port skipped it until
            // 2026-11-13, so level speeches showed no ghost), 0.5 s of
            // speech, DisablePlayerControls, CaptionTextReset.
            self.sumner_ghost_start();
            self.tower.sumner_speech_time = 0.5;
            self.tower.speech_frame = 0;
            self.tower.caption = (0, 0, 0x3c);
            self.disable_controls();
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

/// The FONT table names of a text ROM, resolved against font_desc
/// (StringInitSub: no match leaves font 0).
pub(crate) fn r_fonts(h: &super::hud::Hud) -> Vec<String> {
    h.text.as_ref().map(|r| r.fonts.clone()).unwrap_or_default()
}
pub(crate) fn r_font(h: &super::hud::Hud, fonts: Vec<String>, k: u32) -> Option<usize> {
    let name = fonts.get(k as usize)?;
    h.tables.as_ref()?.font_desc.iter().position(|n| n == name)
}

