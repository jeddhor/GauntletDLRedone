//! do_players 0x133190 on the player records (research/player.md), with
//! DoPlayerStart 0x14caa0, do_weakening 0x132a08, the node round trip
//! GetObjWorldMat 0x103450 / UpdatePlayerWorldMat 0x12aee0 (UpdateObjWorldMat,
//! UnparentMatrix 0x1d0620, UpdateObjColOffset / UpdateObjAttnOffset) and
//! the player clock initialisation.
//!
//! The player's node keeps its own matrix: do_players' GetObjWorldMat resets
//! the record from it at the start of each update, and UpdatePlayerWorldMat
//! refreshes it only when status +0x93c bit 1 is set (observed in
//! trace-player-g1-v1: a sub-0.01 wall-blocked step snaps back every
//! update). HYPOTHESIS: under PlayerRootNode GetWorldMat returns the local
//! matrix unchanged; under a moving floor object the translation-only
//! UnparentMatrix form (parent flag 4) is used; rotated parents are
//! Unsupported.
//! Presentation calls (WritePlayerInfo, WriteName, skins, weapon trails,
//! tree flags, the player light, AudioExitAnim, get_screen_pos, add_target
//! of the old camera) are noted where they occur.
use super::pcontact::ptr;
use super::{unsupported, Game, Unsupported, R};
use crate::animation_clock::{initialize, ClockInit};
use crate::numeric::{add_guarded_rz as add, add_rz, mul_rz as mul, sub_guarded_rz as sub};
use crate::player_collision::collision_position;

type V3 = [f32; 3];

/// MulMat4(left, right) on row matrices (world_transform::multiply): the
/// result is right × left in the row-vector convention.
fn mat_mul(left: &[[f32; 4]; 4], right: &[[f32; 4]; 4]) -> R<[[f32; 4]; 4]> {
    let mut a: [f32; 16] = std::array::from_fn(|k| left[k / 4][k % 4]);
    let mut b: [f32; 16] = std::array::from_fn(|k| right[k / 4][k % 4]);
    let w = crate::world_transform::multiply(&mut a, &mut b)?;
    Ok(std::array::from_fn(|i| std::array::from_fn(|j| w[i * 4 + j])))
}

/// InvertMat4 0x1d5b20: the transposed rotation and the translation
/// −(t · rowᵢ) summed with VU operations (x + y, then + z); w = 1.
fn invert_mat4(m: &[[f32; 4]; 4]) -> R<[[f32; 4]; 4]> {
    let mut out = [[0.0f32; 4]; 4];
    for (i, row) in out.iter_mut().take(3).enumerate() {
        for (j, v) in row.iter_mut().take(3).enumerate() {
            *v = m[j][i];
        }
    }
    let t = [-m[3][0], -m[3][1], -m[3][2]];
    for i in 0..3 {
        let r = m[i];
        out[3][i] = add_rz(add_rz(mul(t[0], r[0])?, mul(t[1], r[1])?)?, mul(t[2], r[2])?)?;
    }
    out[3][3] = 1.0;
    Ok(out)
}

impl Game {
    /// The player's AnimInfo clock on its idle sequence (the tree creation
    /// InitAnim; only when the record holds no valid sequence).
    pub fn init_player_anim(&mut self, p: usize) -> R<()> {
        let seq = self.players[p].u16_(0xa2);
        let clips = &self.player_clips[p];
        if clips.is_empty() || (seq as usize) < clips.len() {
            return Ok(());
        }
        let first = self.players[p].i32_(0x1e8).max(0) as u16;
        let Some(c) = clips.get(first as usize).copied() else {
            return unsupported("player idle sequence outside the class tree");
        };
        let clock = initialize(
            super::setup::fresh_clock(),
            ClockInit {
                sequence: first,
                frames: c.frames,
                timing_factor: c.timing_factor,
                repeat: c.repeat,
                start_frame: 0,
                enabled_word: 1,
                playback_scale: 1.0,
                blend_duration: 0.0,
                now: self.globals.atime,
                anim_time_scale: self.globals.anim_time_scale,
            },
        )?;
        super::paction::set_clock(&mut self.players[p], &clock);
        self.players[p].set_f32(0xbc, 1.0);
        Ok(())
    }

    /// Single-player level-start placement on the record (get_player_pos'
    /// PlayerStartPos path: FloorPos(y, 0.5, position, 1), the start yaw).
    pub fn place_player(&mut self, p: usize, start: V3, yaw: f32) -> R<()> {
        // HYPOTHESIS: SetPlayerStartPos leaves PlayerStartPos/Yaw at the
        // chosen start locator (the placement inputs).
        self.player_start = (start, yaw);
        let (y, hit) = self.collision.floor_pos(start[1], 0.5, start, 1).map_err(|e| Unsupported(e.0.to_string()))?;
        let r = &mut self.players[p];
        r.set_v3(0x50, [start[0], y, start[2]]);
        r.set_v3(0xd8, [0.0, yaw, 0.0]);
        r.set_f32(0x86c, yaw);
        r.set_f32(0x88c, y);
        r.set_f32(0x8a4, y);
        r.set_u32(0x89c, hit.map_or(0, |h| h.object as u32 + 1));
        r.set_i32(0xfc, 1);
        r.set_i16(0x928, 0);
        r.set_i16(0x1ca, 0);
        self.player_floor_parent[p] = None;
        let m = super::enemy_ai::create_ypr_matrix([0.0, yaw, 0.0])?;
        let r = &mut self.players[p];
        for (i, row) in m.iter().take(3).enumerate() {
            r.set_v3(0x20 + 0x10 * i, [row[0], row[1], row[2]]);
            r.set_f32(0x2c + 0x10 * i, 0.0);
        }
        r.set_f32(0x5c, 1.0);
        self.update_obj_offsets(p)?;
        self.player_node[p] = Some(self.record_matrix(p));
        Ok(())
    }

    /// The parent object's world matrix (rows) and whether it is a pure
    /// translation (treated as the parent node's flag 4; HYPOTHESIS).
    fn parent_world(&self, o: usize) -> R<([[f32; 4]; 4], bool)> {
        let m = self.collision.object_matrix(o).map_err(|e| Unsupported(e.0.to_string()))?;
        let pure = (0..3).all(|i| (0..3).all(|j| m[i][j] == if i == j { 1.0 } else { 0.0 }));
        Ok((m, pure))
    }

    /// The record matrix +0x20..+0x5c (4 × 4).
    fn record_matrix(&self, p: usize) -> [[f32; 4]; 4] {
        let r = &self.players[p];
        std::array::from_fn(|i| std::array::from_fn(|j| r.f32_(0x20 + 0x10 * i + 4 * j)))
    }

    /// GetObjWorldMat(+0x20, +0x81c, +0x810): the record matrix becomes the
    /// node's world matrix (GetWorldMat), then the collision (+0x70) and
    /// attention (+0x60) points. Under PlayerRootNode the world matrix is the
    /// node's local matrix; under a moving floor object its translation is
    /// added (translation-only parents).
    pub(crate) fn get_obj_world_mat(&mut self, p: usize) -> R<()> {
        let local = match self.player_node[p] {
            Some(m) => m,
            None => {
                let m = self.record_matrix(p);
                self.player_node[p] = Some(m);
                m
            }
        };
        let mut world = local;
        if let Some(o) = self.player_floor_parent[p] {
            // GetWorldMat: a translation-only parent adds its offset (add.s),
            // otherwise MulMat4(parent, node).
            let (pm, pure) = self.parent_world(o)?;
            if pure {
                for j in 0..3 {
                    world[3][j] = add(pm[3][j], local[3][j])?;
                }
            } else {
                world = mat_mul(&pm, &local)?;
            }
        }
        let r = &mut self.players[p];
        for (i, row) in world.iter().enumerate() {
            for (j, v) in row.iter().enumerate() {
                r.set_f32(0x20 + 0x10 * i + 4 * j, *v);
            }
        }
        self.update_obj_offsets(p)
    }

    /// UpdateObjOffsets: +0x70 from +0x81c and +0x60 from +0x810 (flags +0x84).
    pub(crate) fn update_obj_offsets(&mut self, p: usize) -> R<()> {
        let r = &self.players[p];
        let m = [r.v3(0x20), r.v3(0x30), r.v3(0x40)];
        let (pos, flags) = (r.v3(0x50), r.u32_(0x84));
        let col = collision_position(pos, r.v3(0x81c), flags, &m).map_err(|e| Unsupported(e.0.to_string()))?;
        let attn = collision_position(pos, r.v3(0x810), flags, &m).map_err(|e| Unsupported(e.0.to_string()))?;
        self.players[p].set_v3(0x70, col);
        self.players[p].set_v3(0x60, attn);
        Ok(())
    }

    /// UpdatePlayerWorldMat(player, offsets): UpdateObjWorldMat copies the
    /// record matrix into the node and UnparentMatrix makes it relative to
    /// the node's parent (translation-only form); not while grabbed.
    pub(crate) fn update_player_world_mat(&mut self, p: usize, offsets: bool) -> R<()> {
        if self.players[p].u16_(0x93c) & 0x20 != 0 {
            return Ok(());
        }
        let mut m = self.record_matrix(p);
        // UnparentMatrix's product with the root's identity leaves −0.0 as
        // +0.0 (observed: node rows hold +0.0 where the record has −0.0).
        for row in m.iter_mut() {
            for v in row.iter_mut() {
                *v = add_rz(*v, 0.0)?;
            }
        }
        if let Some(o) = self.player_floor_parent[p] {
            let (pm, pure) = self.parent_world(o)?;
            if pure {
                for j in 0..3 {
                    m[3][j] = sub(m[3][j], pm[3][j])?;
                }
            } else {
                // InvertMat4 (rigid inverse), then MulMat4(inverse, node, node).
                m = mat_mul(&invert_mat4(&pm)?, &m)?;
            }
        }
        self.player_node[p] = Some(m);
        if offsets {
            self.update_obj_offsets(p)?;
        }
        Ok(())
    }

    /// DoPlayerStart 0x14caa0 (start_of_stage: hidden until the zoom-in,
    /// then the entry action 0x7c once).
    fn do_player_start(&mut self, p: usize) -> R<()> {
        if self.globals.start_of_stage < 2 {
            self.players[p].set_u32(0x1e4, 0);
        } else {
            if self.players[p].u32_(0x1e0) == 0x7c {
                self.players[p].or_u16(0x93c, 0x1000);
            }
            let v = if self.players[p].u16_(0x93c) & 0x1000 == 0 { 0x7c } else { 0 };
            self.players[p].set_u32(0x1e4, v);
        }
        self.do_player_action(p)
    }

    /// do_weakening 0x132a08(player, weakest): health drain and heartbeat.
    fn do_weakening(&mut self, p: usize, weakest: bool) -> i32 {
        let rate = self.players[p].i32_(0x94c);
        if rate != 0 && self.players[p].i32_(0xfc) == 1 && self.globals.aux_levels & 4 == 0 && self.globals.cur_world != 0xc {
            let t = self.players[p].i32_(0x948) + self.globals.vb_elapsed;
            self.players[p].set_i32(0x948, if rate <= t { t - rate } else { t });
        }
        let h = self.players[p].f32_(0x1a10);
        if h < 1.0 {
            return -1;
        }
        if !weakest || 200.0 < h {
            return 0;
        }
        let t = self.players[p].u16_(0x1ce).wrapping_sub(self.globals.vb_elapsed as u16) as i16;
        self.players[p].set_i16(0x1ce, t);
        if 0 < t {
            return 0;
        }
        if self.globals.cur_world != 0xd && self.players[p].u32_(0x134) & 0x11_0000 == 0 {
            self.audio("AudioHeartBeat", Some(p), 0);
        }
        let v = if h < 100.0 {
            if h < 25.0 { 0x1e } else { 0x3c }
        } else {
            0x78
        };
        self.players[p].set_i16(0x1ce, v);
        0
    }

    /// test_location 0x14cc40(radius, position): FloorPos(WldBtm, r/2, pos,
    /// 1) must hit within 3 of the position's y; then y takes the floor.
    fn test_location(&mut self, r: f32, pos: &mut V3) -> R<bool> {
        let wb = self.globals.world_bottom;
        let (y, hit) = self.collision.floor_pos(wb, mul(r, 0.5)?, *pos, 1).map_err(|e| Unsupported(e.0.to_string()))?;
        if hit.is_none() || 3.0 < sub(y, pos[1])?.abs() {
            return Ok(false);
        }
        pos[1] = y;
        Ok(true)
    }

    /// get_player_pos 0x14cef8(player, 0) outside the stage start: beside
    /// another living player (Unsupported: try_location's screen test), or
    /// at PlayerStartPos (then the 16 new_offset probes, then
    /// PlayerInitStartPos[0]) with the start yaw.
    pub(crate) fn get_player_pos(&mut self, p: usize) -> R<()> {
        self.players[p].set_u32(0x89c, 0);
        self.player_floor_parent[p] = None;
        if !matches!(self.players[p].i32_(0xfc), 1 | 4) {
            return Ok(());
        }
        self.get_obj_world_mat(p)?;
        self.rng.rand_int(4);
        if self.globals.game_mode == 0x400c || self.globals.start_of_stage != 0 {
            return super::unsupported("get_player_pos at the stage start (player spreading)");
        }
        if (0..4).any(|k| k != p && matches!(self.players[k].i32_(0xfc), 1 | 4 | 8) && !self.player_clips[k].is_empty()) {
            return super::unsupported("get_player_pos beside another player (try_location)");
        }
        // new_offset 0x234120 (16 × (x, z), ELF data).
        const NEW_OFFSET: [[f32; 2]; 16] = [
            [-2.0, -2.0], [-2.0, 0.0], [-2.0, 2.0], [0.0, -2.0], [0.0, 2.0], [2.0, -2.0], [2.0, 0.0], [2.0, 2.0],
            [-2.0, -1.0], [-2.0, 1.0], [-1.0, -2.0], [-1.0, 2.0], [1.0, -2.0], [1.0, 2.0], [2.0, -1.0], [2.0, 1.0],
        ];
        let (start, yaw) = self.player_start;
        let r = add(self.players[p].f32_(0x828), 0.5)?;
        let mut pos = start;
        if !self.test_location(r, &mut pos)? {
            let mut found = false;
            for o in NEW_OFFSET {
                pos = [add(mul(r, o[0])?, start[0])?, start[1], add(mul(r, o[1])?, start[2])?];
                if self.test_location(r, &mut pos)? {
                    found = true;
                    break;
                }
            }
            if !found {
                pos = self.player_init_start;
            }
        }
        let (y, _) = self.collision.floor_pos(pos[1], 0.5, pos, 1).map_err(|e| Unsupported(e.0.to_string()))?;
        self.players[p].set_f32(0x88c, y);
        let mut m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        crate::collision::yaw_mat3(&mut m, yaw)?;
        let r = &mut self.players[p];
        for (i, row) in m.iter().enumerate() {
            r.set_v3(0x20 + 0x10 * i, *row);
            r.set_f32(0x2c + 0x10 * i, 0.0);
        }
        r.set_v3(0x50, [pos[0], y, pos[2]]);
        r.set_f32(0x5c, 1.0);
        r.set_v3(0xd8, [0.0, yaw, 0.0]);
        r.set_u32(0x1dc, 0);
        self.update_player_world_mat(p, false)
    }

    /// update_player_milestone 0x150fb8: the milestones within 3.5 (y) and
    /// 4.5 (xz) push onto the five-entry history +0x950 (newest first). The
    /// ShowMilestones debug arrows are presentation.
    fn update_player_milestone(&mut self, p: usize) -> R<()> {
        let pos = self.players[p].v3(0x50);
        for i in 0..self.milestones.len() {
            let m = self.milestones[i];
            let d = [sub(pos[0], m[0])?, sub(pos[1], m[1])?, sub(pos[2], m[2])?];
            let near = d[1].abs() < 3.5 && crate::vu0::fqdist(d[0], d[2])?.0 < 4.5;
            if !near {
                continue;
            }
            let last = self.players[p].i32_(0x950);
            if 0 <= last && last == i as i32 {
                continue;
            }
            for k in (1..5).rev() {
                let v = self.players[p].i32_(0x950 + 4 * (k - 1));
                self.players[p].set_i32(0x950 + 4 * k, v);
            }
            self.players[p].set_i32(0x950, i as i32);
        }
        Ok(())
    }

    fn player_selecting(&self, p: usize) -> bool {
        (self.players[p].i32_(0xfc) as u32).wrapping_sub(2) < 2
    }

    fn excluded_mode(&self) -> bool {
        matches!(self.globals.game_mode, 0x4012 | 0x400d | 0x400f | 0x4016)
    }

    /// do_players 0x133190 after the welcome block (step_with runs
    /// welcome_check and TowerCheckMessages). Returns the original's result
    /// (false while any player is still active/dying).
    pub fn do_players(&mut self) -> R<bool> {
        let mut result = self.globals.active_players != 0;
        let mut weakest = -1i32;
        let mut same_level = 0;
        let vb = self.globals.vb_elapsed;
        if !self.excluded_mode() {
            // DoHelp: help timers, the help pause and the message box.
            self.do_help();
            for k in 0..4 {
                if self.players[k].i32_(0xfc) != 0 && !self.player_selecting(k) {
                    self.get_obj_world_mat(k)?;
                    // +0xe4: the tree root's world offset (WorldVector of the
                    // tree +0x30) — presentation (shadow while dying).
                    let pos = self.players[k].v3(0x50);
                    self.players[k].set_v3(0xe4, pos);
                }
            }
            if self.globals.paused != 0 || self.globals.help != 0 {
                self.write_player_info(-1)?;
                return Ok(false);
            }
            if self.globals.trig_cam_active == 1 {
                self.write_player_info(-1)?;
                // The animation clocks wait: AnimInfo start time += SPF.
                for k in 0..4 {
                    let v = add(self.players[k].f32_(0xb4), self.globals.spf)?;
                    self.players[k].set_f32(0xb4, v);
                }
                return Ok(false);
            }
            if self.globals.cam_wait != 0 {
                if self.globals.start_of_stage != 0 {
                    for k in 0..4 {
                        let t = self.players[k].u32_(0x8f4);
                        if t != 0 {
                            self.players[k].set_u32(0x8f4, t - 1);
                        }
                        if self.players[k].i32_(0xfc) == 1 && !self.player_clips[k].is_empty() {
                            if self.globals.game_mode == 0x4010 {
                                self.players[k].set_i32(0x2ac4, 1);
                            }
                            self.do_player_action(k)?;
                        }
                        self.players[k].set_u32(0x1e4, 0);
                        self.update_player_world_mat(k, false)?;
                    }
                }
                self.write_player_info(-1)?;
                if self.globals.skip_wave == 0 && self.globals.quit_level == 0 {
                    result = false;
                }
            }
            if self.globals.start_of_stage == 0 || self.globals.skip_wave != 0 || self.globals.quit_level != 0 {
                self.globals.someone_wants_to_exit = 0;
                let mut low = 9999.0;
                for k in 0..4 {
                    // Push velocity (+0x83c) → +0x830 this update, decayed.
                    let r = &self.players[k];
                    let cap = mul(r.f32_(0x124), 0.5)?;
                    let mut v = r.v3(0x83c);
                    let mut n = crate::item_collision::normal_vector_2d(&mut v)?;
                    if cap < n {
                        n = cap;
                    }
                    let push = [mul(n, v[0])?, mul(n, v[1])?, mul(n, v[2])?];
                    let mut n2 = mul(n, f32::from_bits(0x3f2a_c083))?;
                    if n2 < 0.01 {
                        n2 = 0.0;
                    }
                    let rest = [mul(n2, v[0])?, mul(n2, v[1])?, mul(n2, v[2])?];
                    let r = &mut self.players[k];
                    r.set_v3(0x830, push);
                    r.set_v3(0x83c, rest);
                    let s = r.u16_(0x93c);
                    r.set_u16(0x93c, if s & 4 != 0 { (s & 0xfffb) | 8 } else { s & 0xfff7 });
                    if r.i32_(0xfc) == 1 && r.f32_(0x1a10) < low {
                        low = r.f32_(0x1a10);
                        weakest = k as i32;
                    }
                    if (r.i32_(0xfc) == 4 && self.controls.control[k].u8_(4) == 0) || 2 < self.globals.skip_wave {
                        self.globals.someone_wants_to_exit = 1;
                    }
                }
                same_level = self.all_players_go_to_same_level();
            }
        }
        // get_screen_pos (+0x878/+0x87c, ±2000) feeds the HUD arrows.
        for k in 0..4 {
            let gm = self.globals.game_mode;
            if gm == 0x400b || gm == 0x400f {
                continue;
            }
            if !self.player_selecting(k) {
                if self.excluded_mode() {
                    continue;
                }
                let g = &self.globals;
                if g.start_of_stage != 0 && g.skip_wave == 0 && g.quit_level == 0 {
                    if self.players[k].i32_(0xfc) == 1 {
                        self.player_process_powerups(k)?;
                        self.do_player_start(k)?;
                        self.update_player_world_mat(k, false)?;
                    }
                    continue;
                }
            }
            let t = self.players[k].u32_(0x8f4);
            if t != 0 {
                self.players[k].set_u32(0x8f4, t - 1);
            }
            for o in [0x1c8, 0x1d2, 0x1d4, 0x1d6] {
                let v = self.players[k].i16_(o);
                if 0 < v {
                    self.players[k].set_i16(o, v.wrapping_sub(vb as i16));
                }
            }
            if self.players[k].i32_(0x1dc) == 1 {
                let o = if self.players[k].i16_(0x1da) == 0 { 0x1d8 } else { 0x1da };
                let v = self.players[k].i16_(o).wrapping_add(vb as i16);
                self.players[k].set_i16(o, v);
            } else {
                self.players[k].set_i16(0x1d8, 0);
                self.players[k].set_i16(0x1da, 0);
            }
            let state = self.players[k].i32_(0xfc);
            match state {
                0 => {
                    if 0 < self.players[k].i16_(0x1cc) {
                        let v = self.players[k].u16_(0x1cc).wrapping_sub(vb as u16) as i16;
                        self.players[k].set_i16(0x1cc, v);
                        if v < 1 {
                            return unsupported("setup_player_display (player slot timer)");
                        }
                    }
                }
                1 | 4 => {
                    if state == 4 {
                        if self.globals.auto_exit_flag == 0 {
                            result = false;
                        }
                        if (same_level != 0 || self.globals.end_of_stage != 0)
                            && !self.anybody_playing() {
                                self.do_exit(k, same_level)?;
                                self.players[k].set_i32(0x100, state);
                                continue;
                            }
                        if 599 < self.players[k].i16_(0x928) {
                            let v = self.players[k].i16_(0x928) - 0x21c;
                            self.players[k].set_i16(0x928, v);
                            self.audio("AudioWaitToExit", Some(k), 0);
                        }
                    }
                    if self.globals.game_mode == 0x4010 {
                        self.players[k].set_i32(0x2ac4, 1);
                        let o = 0xb44 + 0x1c * self.players[k].u32_(0xc) as usize;
                        if o + 4 <= self.players[k].0.len() {
                            let v = add(vb as f32, self.players[k].f32_(o))?;
                            self.players[k].set_f32(o, v);
                        }
                        if self.level.first().is_some_and(|b| b & 8 != 0) {
                            // MBPosLightNew(player_lightrad, attn, +0x60 + dy, colour).
                            self.events.push(super::Event::PlayerLight { player: k });
                        }
                    }
                    if self.globals.quest_item_state.wrapping_sub(2) < 2 {
                        if self.players[k].i32_(0x80c) == 0 {
                            self.players[k].set_f32(0x7d4, f32::from_bits(0x3f4c_cccd));
                        } else {
                            if self.players[k].i32_(0x80c) == 1 {
                                self.players[k].set_i32(0x80c, 2);
                                let w = self.globals.cur_world;
                                self.player_use_legend_item(k, w);
                            }
                            self.players[k].set_f32(0x7d4, 2.0);
                        }
                    }
                    // SumnerActive 0x175710: 0 < sumner_speech_time.
                    let sumner = self.globals.cur_world == 0xd && 0.0 < self.tower.sumner_speech_time;
                    if !sumner && self.globals.trig_cam_active == 0 && 0 < self.players[k].i16_(0x1d0)
                        && self.globals.paused == 0 && self.globals.help == 0 {
                            let v = self.players[k].u16_(0x1d0).wrapping_sub(vb as u16) as i16;
                            self.players[k].set_i16(0x1d0, v.max(0));
                            // WriteName (the HUD name banner) is presentation.
                        }
                    self.update_player_milestone(k)?;
                    self.player_process_powerups(k)?;
                    self.player_motion(k)?;
                    let t = self.players[k].f32_(0x910);
                    if 0.0 < t && add(t, 2.0)? < self.globals.time {
                        let n = self.players[k].i32_(0x90c);
                        if 0x2d <= n {
                            self.audio("AudioSuchBravery", Some(k), 0);
                        } else if 0x1e <= n {
                            self.audio("AudioHeroicEffort", Some(k), 0);
                        }
                        self.players[k].set_f32(0x910, 0.0);
                        self.players[k].set_i32(0x90c, 0);
                    }
                    self.do_weakening(k, k as i32 == weakest);
                    if self.globals.auto_exit_flag == 0 {
                        result = false;
                    }
                }
                5 => {
                    if matches!(self.globals.game_mode, 0x4013 | 0x4017) {
                        if !self.player_clips[k].is_empty() {
                            self.do_player_action(k)?;
                            // PlayerProcessFamiliar (the +0x720 tree marker)
                            // and PlayerProcessScale (node scale, derived by
                            // player_scale): presentation-side here.
                            let lvl = self.players[k].i32_(0x2ac0);
                            self.players[k].set_u32(0x720, u32::from(0x1e <= lvl));
                        }
                        result = false;
                    } else if self.globals.end_of_stage < 2 && self.globals.skip_wave == 0 && self.globals.quit_level == 0 {
                        result = false;
                    }
                }
                8 => {
                    self.player_process_powerups(k)?;
                    self.player_check_floor(k)?;
                    if 0 < self.players[k].i32_(0x8f8) {
                        self.players[k].set_u32(0x1e4, 0x7e);
                        if self.players[k].u32_(0x1e0) == 0x7e {
                            let v = self.players[k].i32_(0x8f8) - 1;
                            self.players[k].set_i32(0x8f8, v);
                        }
                    }
                    self.do_player_action(k)?;
                    if self.players[k].i32_(0x8f8) < 1 && self.players[k].u32_(0x1e0) == 0x7e {
                        self.players[k].set_u32(0x1e4, 0);
                    }
                    result = false;
                    if self.players[k].i32_(0x8f8) < 1 && self.players[k].u32_(0x1e0) != 0x7e {
                        self.kill_player(k)?;
                    }
                }
                2 | 3 => return unsupported(format!("player state {state} (selecting / joining)")),
                0xb => {
                    if self.globals.game_mode == 0x4010 && self.players[k].i32_(0x2ad4) == 1 {
                        let ed = self.controls.control[k].u32_(8);
                        if ed & 0x800_0000 != 0 {
                            return unsupported("inactivate_player (continue declined)");
                        }
                        if ed & 0x200_0000 != 0 {
                            self.players[k].set_i32(0x2ad4, 0);
                        }
                    }
                }
                _ => {}
            }
            self.players[k].set_i32(0x100, state);
        }
        if !matches!(self.globals.game_mode, 0x400b | 0x400d | 0x4012 | 0x400f | 0x4016) {
            for k in 0..4 {
                if self.players[k].i32_(0xfc) != 0 && self.players[k].u16_(0x93c) & 1 != 0 {
                    self.update_player_world_mat(k, false)?;
                }
            }
        }
        // GrabPowerup in rotating order (firstgetidx).
        let first = self.globals.firstgetidx % 4;
        self.globals.firstgetidx += 1;
        for j in 0..4 {
            let k = ((first + j) % 4) as usize;
            let Some(slot) = ptr(self.players[k].u32_(0x888)) else { continue };
            if self.players[k].i32_(0xfc) == 1 && self.globals.game_mode == 0x4010 {
                let class = self.classes[k].clone();
                self.grab_powerup(k, slot, &class)?;
                for i in 0..4 {
                    if i != k && ptr(self.players[i].u32_(0x888)) == Some(slot) {
                        self.players[i].set_u32(0x888, 0);
                    }
                }
            }
            self.players[k].set_u32(0x888, 0);
        }
        // AudioExitAnim, LockCamReq (old camera), do_got_it and
        // DoTimestopSound are presentation; WritePlayerInfo drives the panels.
        if !matches!(self.globals.game_mode, 0x400d | 0x4012 | 0x4016) && self.globals.start_of_stage == 0 {
            self.write_player_info(-1)?;
            self.do_got_it();
        }
        Ok(result)
    }
}
