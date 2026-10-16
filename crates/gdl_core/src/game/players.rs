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
        }
    }

    /// Presentation clock: (chosen selection context, library flags).
    fn advance(&mut self) -> Option<(ActionContext, crate::player_animation::ActionSelection, u32)> {
        let sim = &mut self.sim;
        let anim = &mut self.anim;
        let frames = self.clips.get(anim.clip).map_or(1.0, |c| f32::from(c.frames.max(1)));
        let mut choose_now = sim.requested_action != anim.requested;
        let mut ended = false;
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
        let state = self.players[p].i32_(0xfc);
        if self.players[p].f32_(0x1a10) <= 0.0 || state == 5 {
            return Ok(());
        }
        if state == 4 {
            let go = self.all_players_go_to_same_level();
            if (go != 0 || self.globals.end_of_stage != 0) && !self.anybody_playing() {
                self.do_exit(p, go)?;
                d.sim.position = self.players[p].v3(0x50);
                return Ok(());
            }
        }
        let edges = levels & !d.previous_levels;
        d.previous_levels = levels;
        let from = self.players[p].v3(0x50);
        let report = d.sim.step(&mut self.collision, &d.tables, levels, edges).map_err(|e| Unsupported(e.to_string()))?;
        d.last = Some(report);
        self.sync_player_record(p, &d.sim)?;
        // PlayerMotion EE 0x14a0..: +0x884 cleared before PlayerCollideItems.
        self.players[p].set_u32(0x884, 0);
        self.player_pickups(p, from)?;
        let (_, magnitude) = d.tables.digital(levels);
        self.do_exit_check(p, magnitude);
        if std::mem::take(&mut d.magic_pressed) {
            self.use_potion(p)?;
        }
        let Some((context, chosen, library)) = d.advance() else { return Ok(()) };
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
        self.start_magic(1.0, p, pos, colour, 0)?;
        self.players[p].set_u16(0x92e, 0x80);
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
    fn do_exit_check(&mut self, p: usize, magnitude: f32) {
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

    fn all_players_go_to_same_level(&self) -> i32 {
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

    fn anybody_playing(&self) -> bool {
        self.globals.end_of_stage == 0 && self.players.iter().any(|pl| matches!(pl.i32_(0xfc), 1 | 8))
    }

    /// do_exit(player, destination wave); sets `level_done` on state 5.
    fn do_exit(&mut self, p: usize, dest: i32) -> R<()> {
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
}
