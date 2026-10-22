//! INTERIM do_players for the local player (labelled; not fidelity evidence).
//!
//! Native and reference-compared pieces: PlayerSim (controls, motion code,
//! displacement request, facing, static/dynamic collision), choose() and
//! consume_events() for the ordinary throw subset, and the ported missile
//! chain (PlayerGetTarget → GetAttackDir → PlayerStartMissile).
//! Interim pieces: the presentation clock that advances clips (no
//! CalcAnimInfo/blending), library return flags taken as "clip ended or
//! action changed", the record sync below (matrix rows, offsets added
//! without rotation, facing words), PlayerGetTarget's origin = +0x70 and
//! the update's facing used for both PlayerMotion facing vectors.
//!
//! Exit flow (transcribed): DoExit 0x146610 (PlayerMotion), player_collis_exit
//! 0x15b5a8, all_players_go_to_same_level 0x132930, anybody_playing 0x1328d8
//! and do_exit 0x132b58 for player state 4. FireScrollActive, skins, audio
//! and the help text are presentation. Level start placement follows
//! get_player_pos 0x14cef8's single-player path (PlayerStartPos, FloorPos);
//! test_location/new_offset spreading is not ported.
use super::{Game, Unsupported, R};
use crate::animation_clock::ClipTiming;
use crate::controls::JoyTables;
use crate::libm::{atan2f, cosf, sinf};
use crate::numeric::add_guarded_rz as add;
use crate::player_animation::{choose, consume_events, ActionContext, ActionEventInput};
use crate::player_sim::{PlayerSim, StepReport};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct PlayerDriver {
    pub sim: PlayerSim,
    pub tables: JoyTables,
    pub previous_levels: u32,
    /// action → [clip, unused] (start-state presentation map).
    pub action_map: Vec<[i64; 2]>,
    pub clips: Vec<ClipTiming>,
    pub anim: PlayerAnim,
    pub event_word: u32,
    pub throw_start_time: f32,
    pub last: Option<StepReport>,
    pub missiles_started: u32,
    /// First consume_events refusal (the throw is not fired after it).
    pub event_error: Option<String>,
    /// INTERIM: magic button edge this update (the original raises the
    /// +0x8d8 0x20000 event from the magic action's animation).
    pub magic_pressed: bool,
    /// (previous, new) action of this update's presentation transition.
    pub transition: Option<(u32, u32)>,
}

/// DoPlayerAction 0x177c28 exit switch (previous action) → +0x8d8 bits, for
/// the melee/area attack actions.
pub fn exit_bits(action: u32) -> u32 {
    match action {
        0x21 | 0x3e | 0x3f | 0x40 | 0x43 | 0x45 => 4,
        0x27..=0x29 | 0x2c | 0x2d | 0x30 | 0x31 | 0x34 | 0x35 | 0x38 | 0x39 | 0x3c | 0x4f | 0x50 => 2,
        0x52 => 8,
        _ => 0,
    }
}

/// DoPlayerAction entry switch (new action): attack start bit 1.
pub fn entry_bits(action: u32) -> u32 {
    match action {
        0x20 | 0x21 | 0x27..=0x29 | 0x2c | 0x2d | 0x30 | 0x31 | 0x34 | 0x35 | 0x38 | 0x39 | 0x3c | 0x3e | 0x3f | 0x40 | 0x43 | 0x45 | 0x4f | 0x50 | 0x52 => 1,
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PlayerAnim {
    pub action: u32,
    pub requested: u32,
    pub clip: usize,
    pub frame: f32,
}

impl PlayerDriver {
    pub fn new(sim: PlayerSim, tables: JoyTables, action_map: Vec<[i64; 2]>, clips: Vec<ClipTiming>) -> Self {
        let action = sim.current_action;
        let clip = action_map.get(action as usize).map_or(0, |[c, _]| (*c).max(0) as usize);
        Self {
            sim,
            tables,
            previous_levels: 0,
            action_map,
            clips,
            anim: PlayerAnim { action, requested: action, clip, frame: 0.0 },
            event_word: 0,
            throw_start_time: 0.0,
            last: None,
            missiles_started: 0,
            event_error: None,
            magic_pressed: false,
            transition: None,
        }
    }

    /// Presentation clock: (chosen selection context, library flags).
    fn advance(&mut self) -> Option<(ActionContext, crate::player_animation::ActionSelection, u32)> {
        let sim = &mut self.sim;
        let anim = &mut self.anim;
        let frames = self.clips.get(anim.clip).map_or(1.0, |c| f32::from(c.frames.max(1)));
        // INTERIM: melee actions run to the end of their clip before a new
        // request is taken (DoPlayerAction's attack branches are not ported).
        let locked = exit_bits(anim.action) != 0;
        let mut choose_now = sim.requested_action != anim.requested && !locked;
        let mut ended = false;
        self.transition = None;
        if !choose_now {
            let timing = self.clips.get(anim.clip).map_or(30, |c| c.timing_factor);
            let period = crate::animation::frame_period(timing, 1.0, sim.spf).unwrap_or(sim.spf);
            anim.frame += sim.spf / period;
            if anim.frame >= frames {
                anim.frame -= frames;
                choose_now = true;
                ended = true;
            }
        }
        let mut result = None;
        if choose_now {
            let context = ActionContext {
                current_action: anim.action,
                requested_action: sim.requested_action,
                frame: anim.frame,
                flags_130: 0,
                flags_134: 0,
                flags_138: sim.flags_138,
                flags_8ac: sim.flags_8ac,
                status_93c: sim.status_93c,
                idle_counter_1d8: 0,
                idle_counter_1da: 0,
                word_80c: sim.word_80c,
            };
            let chosen = choose(context).ok();
            let action = chosen.map_or(sim.requested_action, |s| s.action);
            let changed = action != anim.action;
            if changed || ended {
                self.transition = Some((anim.action, action));
            }
            if changed {
                anim.frame = 0.0;
            }
            anim.action = action;
            anim.requested = sim.requested_action;
            if let Some(&[clip, _]) = self.action_map.get(action as usize)
                && clip >= 0
                && (clip as usize) < self.clips.len()
            {
                anim.clip = clip as usize;
            }
            if let Some(c) = chosen {
                result = Some((context, c, u32::from(ended || changed)));
            }
        }
        sim.current_action = anim.action;
        result
    }
}

impl Game {
    /// One interim do_players update for player `p` with the pad level word.
    pub fn drive_player(&mut self, p: usize, d: &mut PlayerDriver, levels: u32) -> R<()> {
        // PlayerControls 0x105710: disabled controls read as zero.
        let levels = if self.disable_player_controls != 0 { 0 } else { levels };
        let state = self.players[p].i32_(0xfc);
        let g = &self.globals;
        if g.cam_wait != 0 && g.start_of_stage != 0 {
            // do_players during the stage start: +0x8f4 counts down, players in
            // state 1 only run DoPlayerAction (no PlayerMotion), +0x1e4 = 0.
            let t = self.players[p].u32_(0x8f4);
            if t != 0 {
                self.players[p].set_u32(0x8f4, t - 1);
            }
            if state == 1 {
                d.sim.requested_action = 0;
                d.advance();
            }
            self.players[p].set_u32(0x1e4, 0);
            d.previous_levels = levels;
            return Ok(());
        }
        if g.start_of_stage != 0 && g.skip_wave == 0 && g.quit_level == 0 {
            // The player loop runs only once start_of_stage is clear.
            d.previous_levels = levels;
            return Ok(());
        }
        if self.players[p].f32_(0x1a10) <= 0.0 || state == 5 {
            return Ok(());
        }
        if state == 4 {
            if self.auto_exit_done() {
                return Ok(());
            }
            let go = self.all_players_go_to_same_level();
            if (go != 0 || self.globals.end_of_stage != 0) && !self.anybody_playing() {
                self.do_exit(p, go)?;
                d.sim.position = self.players[p].v3(0x50);
                return Ok(());
            }
        }
        // do_players (states 1 and 4): update_player_milestone, then
        // PlayerProcessPowerups; the interim motion uses the re-derived speed.
        self.player_process_powerups(p)?;
        d.sim.stats.speed_124 = self.players[p].f32_(0x124);
        if let Some(pos) = self.grabbed_position(p)? {
            // Parented to a critter node (status +0x93c 0x20): the node moves
            // the player; movement input is not applied (INTERIM: the
            // original's parented PlayerMotion path is not ported).
            d.sim.position = pos;
            d.previous_levels = levels;
            self.sync_player_record(p, &d.sim)?;
            return Ok(());
        }
        let edges = levels & !d.previous_levels;
        d.previous_levels = levels;
        let from = self.players[p].v3(0x50);
        if exit_bits(d.anim.action) == 0 {
            let report = d.sim.step(&mut self.collision, &d.tables, levels, edges).map_err(|e| Unsupported(e.to_string()))?;
            d.last = Some(report);
            if d.sim.requested_action == 0x5c {
                d.sim.requested_action = self.attack_request(p, d.sim.facing)?;
            }
        } else {
            // INTERIM: the player stands still while a melee action plays
            // (the attack's own motion/requested-action logic is not ported).
            d.sim.requested_action = 0;
        }
        self.sync_player_record(p, &d.sim)?;
        // PlayerMotion EE 0x14a0..: +0x884 cleared before PlayerCollideItems.
        self.players[p].set_u32(0x884, 0);
        self.player_pickups(p, from)?;
        let (_, magnitude) = d.tables.digital(levels);
        self.do_exit_check(p, magnitude);
        if std::mem::take(&mut d.magic_pressed) {
            self.use_potion(p)?;
        }
        let selection = d.advance();
        if let Some((old, new)) = d.transition {
            let bits = exit_bits(old) | entry_bits(new);
            if bits & 1 != 0 {
                d.throw_start_time = self.globals.time;
            }
            if bits & 0xfe != 0 {
                let yaw = d.sim.facing;
                let facing = [sinf(yaw)?, 0.0, cosf(yaw)?];
                self.player_melee(p, bits, facing)?;
            }
        }
        let Some((context, chosen, library)) = selection else { return Ok(()) };
        let input = ActionEventInput {
            event_word: d.event_word,
            throw_start_time: d.throw_start_time,
            now: self.globals.time,
            class_word: self.players[p].u32_(8),
            word_720: self.players[p].u32_(0x720),
        };
        let out = match consume_events(context, chosen, library, input) {
            Ok(o) => o,
            Err(_) => {
                d.event_error.get_or_insert_with(|| format!("consume_events refused action {} → {}", context.current_action, chosen.action));
                return Ok(());
            }
        };
        d.event_word = out.event_word;
        d.throw_start_time = out.throw_start_time;
        if let Some(req) = out.missile_request {
            let yaw = d.sim.facing;
            let facing = [sinf(yaw)?, 0.0, cosf(yaw)?];
            let origin = self.players[p].v3(0x70);
            let mut target = -1;
            let mut item = self.players[p].u32_(0x880).checked_sub(1).map(|v| v as usize);
            let mut dir = [0.0; 3];
            self.player_get_target(p, origin, facing, &mut dir, &mut target, &mut item)?;
            let n = self.player_throw(p, facing, dir, target, item, req.charge, req.power_scale, req.origin_mode)?;
            if 0 < n {
                d.missiles_started += n as u32;
            }
        }
        Ok(())
    }

    /// PlayerMotion EE 0x14c0b8..0x14c0f0 (mode 0): spend a potion and cast
    /// its colour (+0x2a98 + 4 × count); +0x92e = 0x80. INTERIM: requires a
    /// potion and is triggered directly by the button.
    pub fn use_potion(&mut self, p: usize) -> R<()> {
        let count = self.players[p].i32_(0x1a18);
        if count < 1 || self.players[p].i32_(0xfc) != 1 {
            return Ok(());
        }
        let colour = self.players[p].u32_(0x2a98 + 4 * count as usize);
        self.players[p].set_i32(0x1a18, count - 1);
        let pos = self.players[p].v3(0x50);
        self.start_magic(1.0, p as i32, pos, colour, 0)?;
        self.players[p].set_u16(0x92e, 0x80);
        Ok(())
    }

    /// PlayerMotion's attack-code decision (EE 0x14a5a8..0x14a6c0, case 0xf)
    /// for the ordinary no-power player: target within reach + 1 → melee
    /// 0x27 (0x52 against a small enemy), otherwise the throw 0x5c. The
    /// moving mid-range dash (0x3e) is not taken (stick input not modelled).
    fn attack_request(&mut self, p: usize, yaw: f32) -> R<u32> {
        let pl = &self.players[p];
        if pl.u32_(0x138) & 0xf070 != 0 || pl.u32_(0x130) & 0x1010_0000 != 0 {
            return Ok(0x5c);
        }
        let facing = [sinf(yaw)?, 0.0, cosf(yaw)?];
        let origin = pl.v3(0x70);
        let reach = pl.f32_(0x828);
        let mut target = -1;
        let mut item = pl.u32_(0x880).checked_sub(1).map(|v| v as usize);
        let mut dir = [0.0; 3];
        let d = self.player_get_target(p, origin, facing, &mut dir, &mut target, &mut item)?;
        let close = d < add(reach, 1.0)?;
        let small = usize::try_from(target)
            .ok()
            .filter(|&t| t < 0x10000)
            .is_some_and(|t| (self.enemies[t].f32_(super::enemies::e::HALF_HEIGHT) <= 2.0 || self.globals.enemy_shrink < 1.0) && d < add(reach, 2.0).unwrap_or(0.0));
        Ok(if close {
            if small { 0x52 } else { 0x27 }
        } else {
            0x5c
        })
    }

    /// PlayerMotion's melee consumption (EE 0x14b9xx..0x14bdb0): damage the
    /// target in reach with the class melee damage (+0x118).
    fn player_melee(&mut self, p: usize, bits: u32, facing: [f32; 3]) -> R<()> {
        let mul = crate::numeric::mul_rz;
        let mut damage = self.players[p].f32_(0x118);
        let mut flags = self.players[p].u32_(0x130);
        let origin = self.players[p].v3(0x70);
        let mut target = -1;
        let mut item = self.players[p].u32_(0x880).checked_sub(1).map(|v| v as usize);
        let mut dir = [0.0; 3];
        let d = self.player_get_target(p, origin, facing, &mut dir, &mut target, &mut item)?;
        let enemy = usize::try_from(target).ok().filter(|&t| t < 0x10000);
        if bits & 0xf0 != 0 {
            // The turbo (power) melee: triple damage, flag 0x20, the turbo
            // cost +0x8e8 taken from +0x800 and cleared.
            damage = mul(damage, 3.0)?;
            flags |= 0x20;
            let r = &mut self.players[p];
            let t = crate::numeric::sub_guarded_rz(r.f32_(0x800), r.f32_(0x8e8))?;
            r.set_f32(0x800, t);
            r.set_u32(0x8e8, 0);
        } else if bits & 4 != 0 {
            damage = add(damage, damage)?;
            flags |= 0x10;
        } else if bits & 8 != 0 && enemy.is_some_and(|e| self.enemies[e].f32_(super::enemies::e::HALF_HEIGHT) <= 2.0) {
            flags |= 0x20;
        }
        let r = self.players[p].f32_(0x828);
        let reach = if bits & 2 == 0 { add(r, 2.0)? } else { add(add(r, 1.0)?, 1.0)? };
        let ahead = add(r, 2.0)?;
        let pos = origin;
        let hit = [add(mul(ahead, facing[0])?, pos[0])?, add(mul(ahead, facing[1])?, pos[1])?, add(mul(ahead, facing[2])?, pos[2])?];
        if reach <= d {
            return Ok(());
        }
        let critter = usize::try_from(target).ok().filter(|&t| 0x10000 <= t).map(|t| t & 0xffff);
        match (enemy, critter) {
            (Some(e), _) => {
                let to = self.enemies[e].v3(super::enemies::e::COLLISION);
                if self.fast_wall_collide(pos, to, 0)?.is_none() {
                    let res = self.player_damage_enemy(damage, 0.0, p, e, flags, Some(hit), true)?;
                    if 2.0 < self.enemies[e].f32_(super::enemies::e::HALF_HEIGHT) {
                        let v = self.players[p].i32_(0x90c) + if res != 0 { 3 } else { 1 };
                        self.players[p].set_i32(0x90c, v);
                        self.players[p].set_f32(0x910, self.globals.time);
                    }
                }
            }
            (None, Some(k)) => {
                let to = self.crit(k).rec.v3(super::critters::c::COL);
                if self.fast_wall_collide(pos, to, 0)?.is_none() {
                    let res = self.player_damage_critter(damage, 0.0, p, k, flags, Some(hit))?;
                    let v = self.players[p].i32_(0x90c) + if res != 0 { 3 } else { 1 };
                    self.players[p].set_i32(0x90c, v);
                    self.players[p].set_f32(0x910, self.globals.time);
                }
            }
            (None, None) => {
                if let Some(it) = item {
                    self.player_damage_item(damage, p, it, flags)?;
                }
            }
        }
        Ok(())
    }

    /// PlayerDamageEnemy 0x147070(damage, cooldown, player, enemy, flags, hit, attack).
    #[allow(clippy::too_many_arguments)]
    pub fn player_damage_enemy(&mut self, damage: f32, cooldown: f32, p: usize, e: usize, flags: u32, hit: Option<[f32; 3]>, attack: bool) -> R<i32> {
        let mul = crate::numeric::mul_rz;
        let slot = 0x2cc + 4 * p;
        if 0.0 < cooldown && self.globals.time < self.enemies[e].f32_(slot) {
            return Ok(-1);
        }
        let mut dmg = if damage == 0.0 { self.players[p].f32_(0x118) } else { damage };
        if self.players[p].u32_(0x138) & 0x100 != 0 {
            dmg = add(dmg, dmg)?;
        }
        let f = self.players[p].v3(0x40);
        let mut k = mul(dmg, f32::from_bits(0x3d4c_cccd))?;
        if 2.0 < k {
            k = 2.0;
        }
        let knock = [f[0], k, f[2]];
        let prior = self.enemies[e].i32_(super::enemies::e::STATE);
        let mut res = -1;
        if 0.0 < self.enemies[e].f32_(super::enemies::e::HEALTH) && (prior == 1 || prior == 6) {
            res = self.damage_enemy(dmg, e, p as i32, flags, hit, Some(knock), 1)?;
        }
        if 0 <= res {
            self.player_damaged_enemy_pub(p, e, prior, res, attack)?;
        }
        if 0.0 < cooldown {
            self.enemies[e].set_f32(slot, add(cooldown, self.globals.time)?);
        }
        Ok(res)
    }

    /// PlayerDamageItem 0x1472e8(damage, player, item, flags).
    pub fn player_damage_item(&mut self, damage: f32, p: usize, it: usize, flags: u32) -> R<()> {
        let dmg = if damage == 0.0 { self.players[p].f32_(0x118) } else { damage };
        let d = self.damage_item(dmg, it, flags, p as i32)?;
        if 0.0 <= d {
            self.player_damaged_item_pub(p, it, d == 0.0)?;
            let def = &self.items.definitions[self.items.records[it].definition()];
            if def.kind() == 10 && def.i32_(0x28) as u8 == 0 {
                self.help(0x14, p);
            }
            self.did_action = true;
        }
        Ok(())
    }

    /// player_collis_exit: the exit item's destination wave (+0xf0).
    fn player_collis_exit(&self, p: usize) -> i32 {
        let mut dest = if self.globals.skip_wave == 0 { 0 } else { -1 };
        if let Some(it) = self.players[p].u32_(0x884).checked_sub(1) {
            let r = &self.items.records[it as usize];
            let d = &self.items.definitions[r.definition()];
            if d.kind() == 9 && d.subtype() != 0x32 {
                dest = i32::from(r.i16_(0xf0));
            }
        }
        dest
    }

    /// DoExit(player) with the stick magnitude (PlayerControl +0x20).
    pub(crate) fn do_exit_check(&mut self, p: usize, magnitude: f32) {
        if self.globals.skip_wave != 0 && (self.globals.someone_wants_to_exit != 0 || self.globals.auto_exit_flag != 0) {
            self.players[p].set_i32(0xfc, 4);
            self.players[p].set_i16(0x1ca, 0);
            return;
        }
        if self.globals.trig_cam_active != 0 {
            return;
        }
        let vb = self.globals.vb_elapsed as i16;
        let mut count = true;
        if self.globals.auto_exit_flag == 0 {
            if self.player_collis_exit(p) == 0 {
                self.players[p].set_i16(0x928, 0);
                count = false;
            } else if self.globals.someone_wants_to_exit == 0 && magnitude != 0.0 {
                count = false;
            }
        }
        if count {
            let c = self.players[p].i16_(0x928).wrapping_add(vb);
            self.players[p].set_i16(0x928, c);
        }
        if self.players[p].i16_(0x928) < 6 {
            if self.players[p].i32_(0xfc) == 4 {
                self.players[p].set_i32(0xfc, 1);
            }
        } else {
            self.players[p].set_i32(0xfc, 4);
            self.players[p].set_i16(0x1ca, 0);
        }
    }

    pub(crate) fn all_players_go_to_same_level(&self) -> i32 {
        if self.exit_state < 0xe && self.globals.skip_wave == 0 {
            return 0;
        }
        let mut dest = 0;
        for k in 0..4 {
            match self.players[k].i32_(0xfc) {
                1 => return 0,
                4 => {
                    let d = self.player_collis_exit(k);
                    if dest == 0 {
                        dest = d;
                    } else if dest != d {
                        return 0;
                    }
                }
                _ => {}
            }
        }
        dest
    }

    pub(crate) fn anybody_playing(&self) -> bool {
        self.globals.end_of_stage == 0 && self.players.iter().any(|pl| matches!(pl.i32_(0xfc), 1 | 8))
    }

    /// do_exit(player, destination wave); sets `level_done` on state 5.
    pub(crate) fn do_exit(&mut self, p: usize, dest: i32) -> R<()> {
        let vb = self.globals.vb_elapsed;
        if self.players[p].i16_(0x1ca) == 0 && dest != 0 {
            self.players[p].set_i16(0x1ca, if self.globals.skip_wave == 0 { 0x32 } else { 0 });
            if self.globals.end_of_stage == 0 {
                let pos = self.players[p].v3(0x50);
                self.sfx(4, Some(pos), 0x7f, 9);
                self.globals.end_of_stage = 1;
            }
            let v = self.players[p].u32_(0x88c);
            self.players[p].set_u32(0x890, v);
            if 3 <= self.globals.skip_wave || self.globals.skip_wave < 0 {
                return super::unsupported("do_exit skip_wave destination");
            }
            if self.globals.skip_wave != 0 {
                return super::unsupported("do_exit Next/PrevWorldLevel");
            }
            self.players[p].set_i32(0x808, dest);
            // SetSkinFX(0.4, TextureExit, 10, 1): the exit glow.
            super::skin::set_skin_fx(&mut self.players[p], 0x7b4, f32::from_bits(0x3ecc_cccd), super::skin::SkinTex::Exit, 10, 1.0);
        }
        let left = i32::from(self.players[p].u16_(0x1ca)) - (vb & 0xffff);
        self.players[p].set_i16(0x1ca, left as i16);
        if (left as i16) < 1 {
            self.players[p].set_i16(0x1ca, 0);
            if self.players[p].i32_(0xfc) != 5 {
                self.players[p].set_i32(0xfc, 5);
                self.level_done = Some(self.players[p].i32_(0x808));
            }
        } else {
            let r = &self.players[p];
            let limit = add(add(add(r.f32_(0x82c), r.f32_(0x82c))?, r.f32_(0x54))?, 1.0)?;
            if r.f32_(0x890) < limit {
                let y = add(r.f32_(0x54), crate::numeric::mul_rz(vb as f32, f32::from_bits(0xbdf5_c28f))?)?;
                self.players[p].set_f32(0x54, y);
                let s = self.players[p].u16_(0x93c) | 1;
                self.players[p].set_u16(0x93c, s);
            }
        }
        Ok(())
    }

    /// PLACEHOLDER co-op join: player `p` enters as a copy of player
    /// `from`'s record (class, stats, inventory) beside it. The original's
    /// join flow (character select, LoadPlyrData, can_join timers) is not
    /// ported; not a fidelity result.
    pub fn join_player_copy(&mut self, p: usize, from: usize, sim: &mut PlayerSim, at: [f32; 3], yaw: f32) -> R<()> {
        let mut r = self.players[from].clone();
        r.set_u32(0x0, p as u32);
        self.players[p] = r;
        self.place_player_start(p, sim, at, yaw)?;
        self.set_player_vars();
        Ok(())
    }

    /// Single-player level start (get_player_pos): PlayerStartPos with its
    /// yaw, y from FloorPos(y, 0.5, position, 1).
    pub fn place_player_start(&mut self, p: usize, sim: &mut PlayerSim, start: [f32; 3], yaw: f32) -> R<()> {
        let (y, hit) = self.collision.floor_pos(start[1], 0.5, start, 1)?;
        sim.position = [start[0], y, start[2]];
        sim.world_bottom = self.globals.world_bottom;
        sim.facing = yaw;
        sim.floor_y_88c = y;
        sim.floor_object_89c = hit.map(|h| h.object);
        let (sn, cs) = (sinf(yaw)?, cosf(yaw)?);
        let k = sim.matrix_scale;
        let mul = crate::numeric::mul_rz;
        sim.matrix = [[mul(cs, k)?, 0.0, mul(-sn, k)?], [0.0, k, 0.0], [mul(sn, k)?, 0.0, mul(cs, k)?]];
        self.players[p].set_i32(0xfc, 1);
        self.players[p].set_i16(0x928, 0);
        self.players[p].set_i16(0x1ca, 0);
        self.sync_player_record(p, sim)
    }

    /// Copy the interim PlayerSim state into the original-layout record.
    pub fn sync_player_record(&mut self, p: usize, s: &PlayerSim) -> R<()> {
        let r = &mut self.players[p];
        r.set_v3(0x20, s.matrix[0]);
        r.set_v3(0x30, s.matrix[1]);
        r.set_v3(0x40, s.matrix[2]);
        r.set_v3(0x50, s.position);
        let a = r.v3(0x810);
        let c = r.v3(0x81c);
        let pos = s.position;
        r.set_v3(0x60, [add(pos[0], a[0])?, add(pos[1], a[1])?, add(pos[2], a[2])?]);
        r.set_v3(0x70, [add(pos[0], c[0])?, add(pos[1], c[1])?, add(pos[2], c[2])?]);
        let yaw = atan2f(s.matrix[2][0], s.matrix[2][2])?;
        r.set_f32(0xdc, yaw);
        r.set_f32(0x86c, s.facing);
        r.set_u32(0x89c, s.floor_object_89c.map_or(0, |o| o as u32 + 1));
        r.set_u32(0x1e0, s.current_action);
        Ok(())
    }

    /// kill_player 0x131a98, after the dying state. In the Tower the
    /// player plays on with PlayerRestoreState; elsewhere PlayersSetWaveDied,
    /// player_set_continue (+0xfc = 0xb, +0x2ad4 = 1), health 0, drop_keys
    /// and init_player_powerups. The level-start backup is restored when the
    /// player is next in the Tower (init_next_level).
    pub fn kill_player(&mut self, p: usize) -> R<()> {
        let class = self.players[p].u32_(0xc) as usize;
        if self.globals.cur_world == 0xd {
            self.players[p].set_i32(0xfc, 1);
            // The class record is the player's PDAT (player_get_from_save
            // reads the current class only).
            let mut v = vec![Vec::new(); 16];
            if let Some(slot) = v.get_mut(class) {
                *slot = self.classes[p].raw.clone();
            }
            super::profile::player_restore_state(&mut self.players[p], &super::profile::Pdats(v), &self.class_tables).map_err(|e| Unsupported(e.to_string()))?;
            return Ok(());
        }
        // PlayersSetWaveDied(p, CurWorld, CurLevel): the last level of the
        // world (the boss) marks the backup's class record +0x14 (else +0x16).
        if self.globals.cur_level == self.world_num_levels - 1 {
            let o = 0x1d8c + 0xb4 * class;
            let bit = 1u16 << (self.globals.cur_world & 0x1f);
            if self.players[p].u16_(o) & bit == 0 {
                let v = self.players[p].u16_(o) | bit;
                self.players[p].set_u16(o, v);
            } else {
                let v = self.players[p].u16_(o + 2) | bit;
                self.players[p].set_u16(o + 2, v);
            }
        }
        self.players[p].set_i32(0xfc, 0xb);
        self.players[p].set_i32(0x2ad4, 1);
        self.players[p].set_f32(0x1a10, 0.0);
        // drop_keys: a KEY (one) or KEYRING item with +0xf4 = count at the
        // player's position; boss levels just lose them.
        let keys = self.players[p].i32_(0x1a14);
        if 0 < keys && self.globals.boss_type < 0 {
            let name: &[u8] = if keys == 1 { b"KEY" } else { b"KEYRING" };
            if let Some(def) = self.items.definitions.iter().position(|d| d.name() == name) {
                let m = [self.players[p].v3(0x20), self.players[p].v3(0x30), self.players[p].v3(0x40)];
                let at = self.players[p].v3(0x50);
                let slot = self.add_item_sub(def, Some((m, at)))?;
                self.items.records[slot].set_i32(0xf4, keys);
            }
        }
        self.players[p].set_i32(0x1a14, 0);
        super::profile::init_player_powerups(&mut self.players[p]);
        Ok(())
    }
}
