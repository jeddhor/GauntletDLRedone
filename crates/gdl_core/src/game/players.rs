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
        if self.players[p].f32_(0x1a10) <= 0.0 {
            return Ok(());
        }
        let edges = levels & !d.previous_levels;
        d.previous_levels = levels;
        let from = self.players[p].v3(0x50);
        let report = d.sim.step(&mut self.collision, &d.tables, levels, edges).map_err(|e| Unsupported(e.to_string()))?;
        d.last = Some(report);
        self.sync_player_record(p, &d.sim)?;
        self.player_pickups(p, from)?;
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
