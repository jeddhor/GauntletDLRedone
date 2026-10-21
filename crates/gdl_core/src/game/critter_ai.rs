//! Critter per-frame logic: ProcessCritterList 0x199058, ProcessCritter
//! 0x198c28, CritterBossAI 0x197c50 and the move/target/action functions
//! they call (research/critters.md). Child critters (chimera heads) and the
//! golem/gargoyle AI are not ported yet and stop with Unsupported.
use super::critters::{c, mul_vec_mat3, CritterFile};
use super::damage::pd;
use super::{unsupported, Game, Unsupported, R};
use crate::animation_clock::{select, SelectionInput};
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sqrt_rn, sub_guarded_rz as sub};
use crate::tree_pose::local_pose_where;

type V3 = [f32; 3];

const PI: f32 = f32::from_bits(0x4049_0fdb);
const TWO_PI: f32 = f32::from_bits(0x40c9_0fdb);
const HALF_PI: f32 = f32::from_bits(0x3fc9_0fdb);
/// crit_player_damage_mul 0x251738 / crit_player_roar_damage_mul 0x251720
/// (index: active players), maxinvanger.279 0x25174c.
const DAMAGE_MUL: [f32; 5] = [1.0, 1.0, 0.5, 0.3, 0.2];
const ROAR_MUL: [f32; 5] = [1.0, 1.0, 1.5, 2.0, 2.0];
const MAX_INV_ANGER: f32 = 10.0;

fn wrap(v: f32) -> R<f32> {
    Ok(if PI < v {
        sub(v, TWO_PI)?
    } else if v <= -PI {
        add(v, TWO_PI)?
    } else {
        v
    })
}

/// SlowNormalVector 0x1d5de8 (FPU sqrt/div).
pub fn slow_normal_vector(v: &mut V3) -> R<f32> {
    let s = add(add(mul(v[0], v[0])?, mul(v[1], v[1])?)?, mul(v[2], v[2])?)?;
    let len = sqrt_rn(s)?;
    let k = if len <= 0.0 { 1.0 } else { div(1.0, len)? };
    for x in v.iter_mut() {
        *x = mul(k, *x)?;
    }
    Ok(len)
}

/// YawVec3 0x1d5a58.
pub fn yaw_vec3(a: f32, v: V3) -> R<V3> {
    let co = crate::libm::cosf(a).map_err(|_| Unsupported("cosf".into()))?;
    let si = crate::libm::sinf(a).map_err(|_| Unsupported("sinf".into()))?;
    Ok([sub(mul(v[0], co)?, mul(v[2], si)?)?, v[1], add(mul(v[2], co)?, mul(v[0], si)?)?])
}

/// PitchVec3 0x1d5ae8.
pub fn pitch_vec3(a: f32, v: V3) -> R<V3> {
    let co = crate::libm::cosf(a).map_err(|_| Unsupported("cosf".into()))?;
    let si = crate::libm::sinf(a).map_err(|_| Unsupported("sinf".into()))?;
    let x = add(mul(mul(v[0], v[1])?, si)?, mul(v[0], co)?)?;
    let y = add(mul(-add(mul(v[0], v[0])?, mul(v[2], v[2])?)?, si)?, mul(v[1], co)?)?;
    let z = add(mul(mul(v[1], v[2])?, si)?, mul(v[2], co)?)?;
    Ok([x, y, z])
}

/// Target query (8 floats at MOVE +0x60 / TYPE +0x80 / PTRN +0x30).
#[derive(Clone, Copy)]
struct Query([f32; 8]);

impl Query {
    fn at(rec: &[u8], o: usize) -> Self {
        Query(std::array::from_fn(|k| CritterFile::f32_pub(rec, o + 4 * k)))
    }
}

impl Game {
    fn cf(&self, i: usize, o: usize) -> f32 {
        self.crit(i).rec.f32_(o)
    }
    fn ci16(&self, i: usize, o: usize) -> i16 {
        self.crit(i).rec.i16_(o)
    }
    fn set_ci16(&mut self, i: usize, o: usize, v: i16) {
        self.crit_mut(i).rec.set_i16(o, v);
    }

    /// ProcessCritterList 0x199058 (called from do_enemies).
    pub fn process_critter_list(&mut self) -> R<i32> {
        self.critter_counter = self.critter_counter.wrapping_add(1);
        // CritterInitPlayerData 0x19a9d8.
        self.critter_nactive = (0..4).filter(|&k| self.players[k].i32_(pd::STATE) == 1).count();
        self.critter_ntargets = [0.0; 4];
        let mut n = 0;
        for i in 0..self.critters.len() {
            if self.critters[i].is_some() && self.process_critter(i)? {
                n += 1;
            }
        }
        Ok(n)
    }

    /// ProcessCritter 0x198c28.
    fn process_critter(&mut self, i: usize) -> R<bool> {
        if self.crit(i).parent.is_some() {
            return Ok(false);
        }
        let kids = self.critter_children(i);
        let live = kids.iter().filter(|&&k| 0.0 < self.cf(k, c::HEALTH)).count();
        self.crm_pub(i).set_u8(c::NLIVE_CHILDREN, live as u8);
        self.critter_attention_pub(i)?;
        self.critter_update_colnodes_pub(i)?;
        self.critter_do_knockback(i)?;
        self.critter_update_counters(i)?;
        for &k in &kids {
            self.critter_update_child(i, k)?;
            self.critter_update_colnodes_pub(k)?;
            self.critter_update_counters(k)?;
        }
        if self.crit(i).rec.i32_(c::STATE) == 3 {
            // A root with children dies once every child's health is gone.
            let mut n = 1;
            for &k in &kids {
                if n < 1 {
                    n = 0;
                } else {
                    n += 1;
                    if 0.0 < self.cf(k, c::HEALTH) {
                        n = 0;
                    }
                }
            }
            if 2 <= n {
                self.crm_pub(i).set_f32(c::HEALTH, -1.0);
            }
            if self.cf(i, c::HEALTH) <= 0.0 {
                self.critter_kill(i)?;
            }
        }
        match self.critter_kind(i) {
            4 => {
                if !self.critter_boss_ai(i)? {
                    return Ok(false);
                }
            }
            3 | 7 | 8 => {
                if !self.critter_golem_ai(i)? {
                    return Ok(false);
                }
            }
            _ => {
                self.critter_animate(i)?;
                self.critter_floor(i)?;
            }
        }
        // CritterUpdateSkinfx for the root and its children (DoTexMods and
        // the MBTree flags are presentation).
        self.critter_update_skinfx(i)?;
        for &k in &self.critter_children(i) {
            self.critter_update_skinfx(k)?;
        }
        self.critter_attention_pub(i)?;
        Ok(true)
    }

    /// CritterUpdateChild 0x198b78: the root's matrix, attention point and
    /// collision centre; the position follows the +0xdc transition node.
    fn critter_update_child(&mut self, root: usize, k: usize) -> R<()> {
        let m: Vec<f32> = (0..16).map(|n| self.cf(root, c::MATRIX + 4 * n)).collect();
        let (att, col) = (self.crit(root).rec.v3(c::ATTN), self.crit(root).rec.v3(c::COL));
        {
            let r = self.crm_pub(k);
            for (n, v) in m.iter().enumerate() {
                r.set_f32(c::MATRIX + 4 * n, *v);
            }
            r.set_v3(c::ATTN, att);
            r.set_v3(c::COL, col);
        }
        match self.crit(k).node_dc {
            Some(n) => {
                let w = self.critter_node_matrix_pub(k, Some(n), c::NODE_MATRIX, None)?;
                self.crm_pub(k).set_v3(c::POS, [w[12], w[13], w[14]]);
            }
            None => {
                let r = self.crm_pub(k);
                for (n, v) in m.iter().enumerate() {
                    r.set_f32(c::NODE_MATRIX + 4 * n, *v);
                }
            }
        }
        Ok(())
    }

    /// FloorCollide(5, 4, -1000, position, 0, 0, 2) and the floor offset.
    fn critter_floor(&mut self, i: usize) -> R<Option<crate::collision::Hit>> {
        let p = self.crit(i).rec.v3(c::POS);
        let (col, _) = self.collision.floor_collide(5.0, 4.0, -1000.0, p, false, 2)?;
        if let Some(h) = col.solid {
            let off = self.critter_type_f32(i, 0xb0);
            let y = add(h.position[1], off)?;
            self.crm_pub(i).set_f32(c::POS + 4, y);
        }
        Ok(col.solid)
    }

    /// CritterBossAI 0x197c50; false when the instance was deleted.
    fn critter_boss_ai(&mut self, i: usize) -> R<bool> {
        self.critter_get_target_players(i)?;
        self.critter_set_difficulty(i)?;
        let kids = self.critter_children(i);
        for &k in &kids {
            self.critter_get_target_players(k)?;
            self.critter_set_difficulty(k)?;
        }
        self.critter_resolve_multiple_targets(i);
        self.critter_process_safe_rocks()?;
        let time = self.globals.time;
        if self.crit(i).rec.i32_(c::STATE) == 0 {
            if self.boss_activate_time == 0.0 {
                if self.critter_type_u32(i, 0x5c) & 0x80 == 0 {
                    self.boss_activate_time = add(time, 2.0)?;
                }
            } else if self.boss_activate_time <= time {
                let need = self.critter_type_f32(i, 0xec);
                let far = self.critter_max_player_dist(i);
                if need <= 0.0 || far < need {
                    self.critter_activate(i);
                }
            }
        }
        if 0 <= self.ci16(i, c::GRABBED) {
            // LockCamera(0x1e, 2): camera presentation.
        }
        let aux = self.globals.aux_levels;
        if aux & 0x80 == 0 {
            self.critter_move_setup(i);
            self.critter_look_for_critical_move(i)?;
            if self.ci16(i, c::NEXT) < 0 {
                self.critter_look_for_block(i)?;
                if self.ci16(i, c::NEXT) < 0 {
                    self.critter_look_for_attack(i)?;
                    let mut busy = 0;
                    for &k in &kids {
                        self.critter_move_setup(k);
                        if 0 <= self.ci16(i, c::PATTERN) {
                            self.critter_child_get_pattern(i, k);
                            continue;
                        }
                        if 0 <= self.ci16(i, c::NEXT) {
                            continue;
                        }
                        self.critter_child_critical_move(k)?;
                        if self.ci16(k, c::NEXT) < 0 {
                            self.critter_look_for_attack(k)?;
                            if self.ci16(k, c::NEXT) < 0 {
                                self.critter_look_for_ready(k)?;
                                continue;
                            }
                        }
                        if 0 <= self.ci16(k, c::PATTERN) || 0x7e < self.critter_move_i32(k, self.ci16(k, c::NEXT) as usize, 0) as u32 {
                            busy += 1;
                        }
                    }
                    if busy != 0 {
                        let v = self.critter_find_move_type(i, 1, 1);
                        self.set_ci16(i, c::NEXT, v);
                    }
                    if self.ci16(i, c::NEXT) < 0 {
                        let cur = self.ci16(i, c::MOVE);
                        let cur_type = if 0 <= cur { self.critter_move_i32(i, cur as usize, 0) as u32 } else { self.critter_move_i32(i, 0, 0) as u32 };
                        if cur_type < 0x7f {
                            self.critter_look_for_translate(i)?;
                        }
                        if self.ci16(i, c::NEXT) < 0 {
                            self.critter_look_for_ready(i)?;
                        }
                    }
                }
            }
        } else {
            self.critter_get_next_move(i)?;
        }
        self.critter_animate(i)?;
        if self.ci16(i, c::MOVE) < 0 {
            self.set_ci16(i, c::MOVE, 0);
        }
        let mv = self.ci16(i, c::MOVE) as usize;
        let root_type = self.critter_move_i32(i, mv, 0);
        for &k in &kids {
            if self.crit(k).rec.i32_(c::STATE) == 1 {
                let v = self.critter_find_move_type(k, 0x11, 1);
                self.set_ci16(k, c::NEXT, v);
                self.critter_animate(k)?;
                continue;
            }
            if (root_type == 1 || 0 <= self.ci16(i, c::PATTERN))
                && 1 < self.globals.quest_item_state.wrapping_sub(2)
                && (0 <= self.ci16(k, c::MOVE) || 0 <= self.ci16(k, c::NEXT))
            {
                self.critter_animate(k)?;
                continue;
            }
            self.critter_copy_anim(i, k)?;
            if root_type == 1 {
                self.set_ci16(k, c::NEXT, 0);
            }
        }
        let frame = self.cf(i, c::ANIM_FRAME);
        let hold = self.critter_move_f32(i, mv, 0x8c);
        let done = self.crit(i).clock.mode_bits & 0xff != 0;
        let mtype = self.critter_move_i32(i, mv, 0);
        if 0.0 < hold {
            let until = self.cf(i, c::HOLD_UNTIL);
            if mtype == 0x11 {
                if done {
                    if until <= time {
                        if aux & 0x80 == 0 {
                            self.critter_del_inst(i)?;
                            return Ok(false);
                        }
                    } else if aux & 0x80 == 0 {
                        self.globals.boss_is_dead = 1;
                        // Fade-out alpha during the last 0.5 s: presentation.
                    }
                } else {
                    self.crm_pub(i).set_f32(c::HOLD_UNTIL, add(hold, time)?);
                }
            } else if !(done && until != 0.0) {
                self.crm_pub(i).set_f32(c::HOLD_UNTIL, add(hold, time)?);
            }
        } else {
            self.crm_pub(i).set_f32(c::HOLD_UNTIL, 0.0);
        }
        let q = self.globals.quest_item_state;
        if mtype == 0x10 {
            if self.critter_move_i16(i, mv, 0x54) < 0 && done && q == 1 {
                self.globals.quest_item_state = 2;
            }
        } else if mtype == 0x22 && done && q == 3 {
            self.globals.quest_item_state = 4;
        }
        let q = self.globals.quest_item_state;
        if q < 4 && 1 < q {
            self.crm_pub(i).set_i16(0xb46, 0xff);
        }
        self.critter_do_transition(i, mv)?;
        self.critter_do_actions(i, mv, frame as i32 as i16)?;
        self.critter_translate(i, mv)?;
        self.critter_rotate(i, mv)?;
        self.critter_look_at_player(i, Some(mv))?;
        for &k in &kids {
            let m = usize::try_from(self.ci16(k, c::MOVE)).ok();
            if let Some(m) = m {
                let frame = self.cf(k, c::ANIM_FRAME);
                self.critter_do_transition(k, m)?;
                self.critter_do_actions(k, m, frame as i32 as i16)?;
                self.critter_translate(k, m)?;
                self.critter_rotate(k, m)?;
            }
            self.critter_look_at_player(k, m)?;
        }
        if let Some(h) = self.critter_floor(i)?
            && self.crit(i).rec.i32_(c::STATE) == 0
            && self.boss_activate_time == 0.0
            && self.critter_type_u32(i, 0x5c) & 0x80 != 0
        {
            let mut b = self.items.objects.get(h.object).map_or(0, |o| o.byte_16);
            if let Some(p) = self.collision.objects.get(h.object).and_then(|o| o.parent) {
                b |= self.items.objects.get(p).map_or(0, |o| o.byte_16);
            }
            if b & 0x10 != 0 {
                self.boss_activate_time = add(time, 2.0)?;
                self.boss_activate(i, false);
            }
        }
        Ok(true)
    }

    /// CritterSetDifficulty 0x19c1d8.
    fn critter_set_difficulty(&mut self, i: usize) -> R<()> {
        let max = add(mul(self.critter_type_f32(i, 0xe4), self.level_f32(0xac))?, 1.0)?;
        let f = add(mul(sub(1.0, div(self.cf(i, c::HEALTH), max)?)?, 4.5)?, 0.5)?;
        let r = self.crm_pub(i);
        r.set_f32(c::INV_DIFFICULTY, div(1.0, f)?);
        r.set_f32(c::DIFFICULTY, f);
        Ok(())
    }

    /// CritterProcessSafeRocks 0x1979c8 with SafeRockSetup 0x153330.
    fn critter_process_safe_rocks(&mut self) -> R<()> {
        if self.safe_rock_num == 0 {
            let mut found = Vec::new();
            for (k, r) in self.items.records.iter().enumerate() {
                if r.active() && self.items.definitions[r.definition()].kind() == 10 && r.i16_(0xf0) == 0x29 {
                    found.push(k);
                    if 0x10 <= found.len() {
                        break;
                    }
                }
            }
            if self.critter_data.safe_rock_hide {
                for &k in &found {
                    self.items.records[k].set_i16(0xf2, -1);
                }
            }
            self.safe_rock_counter = vec![0.0; found.len()];
            if found.is_empty() {
                self.safe_rock_num = -1;
            } else {
                self.safe_rock_num = found.len() as i32;
                self.safe_rock_idx = self.rng.rand_int(found.len() as u32).ok_or_else(|| Unsupported("RandInt".into()))? as i32;
            }
            self.safe_rock_items = found;
        } else if 0 < self.safe_rock_num {
            for k in 0..self.safe_rock_items.len() {
                let v = self.safe_rock_counter[k];
                if 0.0 < v {
                    let v = sub(v, self.globals.spf)?;
                    self.safe_rock_counter[k] = v;
                    if v <= 0.0 {
                        self.safe_rock_activate(self.safe_rock_items[k])?;
                    }
                }
            }
        }
        Ok(())
    }

    /// SafeRockActivate 0x1533c0: the rock grows (health = definition +0x44
    /// × 3, stage 0, +0xe3 = definition +0x42) and SetSafeRock restages it;
    /// MBTreeClearFlags(tree, 1) is presentation.
    fn safe_rock_activate(&mut self, item: usize) -> R<()> {
        let d = self.items.records[item].definition();
        let (h, e3) = {
            let def = &self.items.definitions[d];
            (def.i16_(0x44).wrapping_mul(3), def.0[0x42])
        };
        let r = &mut self.items.records[item];
        r.set_i16(0xe4, h);
        r.set_i16(0xf2, 0);
        r.set_u8(0xe3, e3);
        self.set_safe_rock_pub(item)
    }

    /// CritterActivate 0x197658 / BossActivate 0x19dfb8.
    fn critter_activate(&mut self, i: usize) {
        self.crm_pub(i).set_i32(c::STATE, 3);
        for k in self.critter_children(i) {
            self.crm_pub(k).set_i32(c::STATE, 3);
        }
        if self.critter_kind(i) == 4 {
            self.boss_activate(i, true);
        }
    }
    fn boss_activate(&mut self, _i: usize, music: bool) {
        self.globals.boss_active = 1;
        if self.boss_cam_mode == 0 {
            self.boss_cam_mode = 1;
        }
        if music {
            // attn_area = 1, attn_change_now = 1: attention-area music change.
            self.events.push(super::Event::Audio { cue: "BossActivateMusic", player: None, arg: 1 });
        }
    }

    // ---- targets --------------------------------------------------------

    fn target_at(i: usize) -> usize {
        c::TARGETS + 0x30 * i
    }

    /// CritterGetTargetPlayers 0x19ae70.
    fn critter_get_target_players(&mut self, i: usize) -> R<()> {
        self.set_ci16(i, c::NTARGETS, 0);
        if self.cf(i, c::HEALTH) <= 0.0 {
            return Ok(());
        }
        let q = Query::at(self.critter_type_rec(i), 0x80);
        for p in 0..4 {
            let pl = &self.players[p];
            if pl.i32_(pd::STATE) != 1 {
                continue;
            }
            if pl.u32_(0x138) & 4 != 0 && self.crit(i).rec.i32_(c::STATE) != 0 && self.critter_kind(i) != 4 {
                continue;
            }
            let pos = pl.v3(0x70);
            let (score, rec) = self.critter_calc_target(i, Some(q), pos)?;
            let wp_gate = self.crit(i).rec.u32_(c::WAYPOINT) == 0 || self.cf(i, 0xb58) <= 0.0 || score <= self.cf(i, 0xb58);
            if !wp_gate {
                continue;
            }
            let mut rec = rec;
            if self.globals.time < self.players[p].f32_(0x8c0) {
                rec[3] = mul(rec[3], 1000.0)?;
            }
            if score < 1e21 {
                rec[0] = f32::from_bits(p as u32);
                rec[4] = self.critter_player_anger(i, p)?;
                rec[3] = mul(rec[3], rec[4])?;
                self.critter_insert_target(i, rec);
            }
        }
        for k in 0..self.ci16(i, c::NTARGETS).max(0) as usize {
            let p = self.crit(i).rec.i32_(Self::target_at(k));
            if let Ok(p) = usize::try_from(p) {
                self.critter_ntargets[p] = add(self.critter_ntargets[p], 1.0)?;
            }
        }
        Ok(())
    }

    /// CritterCalcTarget 0x19aaa8: (score, 12-word target record) where
    /// word 1 = forward dot, 2 = distance, 3 = score, 8..10 = direction.
    fn critter_calc_target(&self, i: usize, q: Option<Query>, pos: V3) -> R<(f32, [f32; 12])> {
        let mut rec = [0.0f32; 12];
        let diff = self.cf(i, c::DIFFICULTY);
        if let Some(q) = q {
            if diff < q.0[4] {
                return Ok((1.2e21, rec));
            }
            if 0.0 < q.0[5] && q.0[5] <= diff {
                return Ok((1.2e21, rec));
            }
        }
        let col = self.crit(i).rec.v3(c::COL);
        let mut d = [sub(pos[0], col[0])?, 0.0, sub(pos[2], col[2])?];
        let mut dy = sub(pos[1], col[1])?;
        let dist = slow_normal_vector(&mut d)?;
        let fwd = self.crit(i).rec.v3(c::MATRIX + 0x20);
        let (dot, score);
        match q {
            None => {
                let mut f = [fwd[0], 0.0, fwd[2]];
                slow_normal_vector(&mut f)?;
                dot = add(mul(d[0], f[0])?, mul(d[2], f[2])?)?;
                score = if dot <= 0.5 { add(dist, dist)? } else { div(dist, dot.abs())? };
            }
            Some(q) => {
                if dist < q.0[0] {
                    return Ok((1.01e21, rec));
                }
                if 0.0 < q.0[1] && q.0[1] < dist {
                    return Ok((1.02e21, rec));
                }
                if dy < 0.0 {
                    dy = -dy;
                }
                if 0.0 < q.0[7] && q.0[7] < dy {
                    return Ok((1.03e21, rec));
                }
                let mut f = yaw_vec3(-q.0[2], fwd)?;
                f[1] = 0.0;
                slow_normal_vector(&mut f)?;
                dot = add(mul(d[0], f[0])?, mul(d[2], f[2])?)?;
                if dot < q.0[3] {
                    return Ok((1.1e21, rec));
                }
                score = if dot <= 0.5 { add(dist, dist)? } else { div(dist, dot.abs())? };
            }
        }
        rec[1] = dot;
        rec[2] = dist;
        rec[3] = score;
        rec[8] = d[0];
        rec[9] = 0.0;
        rec[10] = d[2];
        Ok((score, rec))
    }

    /// CritterInsertTarget 0x19ad38 (sorted by score, at most four).
    fn critter_insert_target(&mut self, i: usize, rec: [f32; 12]) {
        let n = self.ci16(i, c::NTARGETS).max(0) as usize;
        let mut at = n;
        for k in 0..n {
            if rec[3] < self.cf(i, Self::target_at(k) + 0xc) {
                at = k;
                break;
            }
        }
        if at < n {
            let mut k = n.min(3);
            while at < k {
                for w in 0..12 {
                    let v = self.crit(i).rec.u32_(Self::target_at(k - 1) + 4 * w);
                    self.crit_mut(i).rec.set_u32(Self::target_at(k) + 4 * w, v);
                }
                k -= 1;
            }
        }
        if n < 4 {
            self.set_ci16(i, c::NTARGETS, n as i16 + 1);
        }
        if at < 4 {
            for (w, v) in rec.iter().enumerate() {
                self.crit_mut(i).rec.set_u32(Self::target_at(at) + 4 * w, v.to_bits());
            }
        }
    }

    /// CritterPlayerAnger 0x19aa28 (a NaN ratio yields 0.01, as the original).
    #[allow(clippy::manual_clamp)]
    fn critter_player_anger(&self, i: usize, p: usize) -> R<f32> {
        let a = self.cf(i, c::ANGER + 0x10 * p + 8);
        if a < 1.0 {
            return add(MAX_INV_ANGER, 1.0).map_err(Into::into);
        }
        let v = div(self.cf(i, c::ANGER + 0x10 * p), a)?;
        Ok(if v < 0.01 {
            0.01
        } else if MAX_INV_ANGER < v {
            MAX_INV_ANGER
        } else {
            v
        })
    }

    /// MaxPlayerDist 0x19b7e0.
    fn critter_max_player_dist(&self, i: usize) -> f32 {
        let mut m = 0.0f32;
        for k in 0..self.ci16(i, c::NTARGETS).max(0) as usize {
            let d = self.cf(i, Self::target_at(k) + 8);
            if m < d {
                m = d;
            }
        }
        if m <= 0.0 { 1e21 } else { m }
    }

    /// CritterReCalcTarget 0x19b420.
    fn critter_recalc_target(&self, i: usize, q: Option<Query>, k: usize) -> R<f32> {
        let at = Self::target_at(k);
        let diff = self.cf(i, c::DIFFICULTY);
        if let Some(q) = q
            && (diff < q.0[4] || (q.0[4] < q.0[5] && q.0[5] <= diff))
        {
            return Ok(1.2e21);
        }
        let dist = self.cf(i, at + 8);
        if let Some(q) = q {
            if dist < q.0[0] {
                return Ok(1.01e21);
            }
            if 0.0 < q.0[1] && q.0[1] < dist {
                return Ok(1.02e21);
            }
            let fwd = self.crit(i).rec.v3(c::MATRIX + 0x20);
            let mut f = yaw_vec3(-q.0[2], fwd)?;
            f[1] = 0.0;
            slow_normal_vector(&mut f)?;
            let dot = add(mul(self.cf(i, at + 0x20), f[0])?, mul(self.cf(i, at + 0x28), f[2])?)?;
            if dot < q.0[3] {
                return Ok(1.1e21);
            }
        }
        Ok(mul(dist, self.cf(i, at + 0x10))?)
    }

    /// CritterMoveGetTarget 0x19b578: the chosen player or -1.
    fn critter_move_get_target(&self, i: usize, q: Option<Query>, any: bool) -> R<i32> {
        if let Some(q) = q
            && 0.0 < q.0[6]
            && q.0[6] < self.cf(i, c::LEASH_DIST)
        {
            return Ok(-1);
        }
        let mut best = 2e21f32;
        let mut pick: i32 = -1;
        for k in 0..self.ci16(i, c::NTARGETS).max(0) as usize {
            let v = self.critter_recalc_target(i, q, k)?;
            if pick < 0 || v < best {
                pick = k as i32;
                best = v;
            }
        }
        if !any && 1e21 <= best {
            pick = -1;
        }
        if pick < 0 {
            if any && let Some(p) = self.crit(i).parent {
                return self.critter_move_get_target(p, q, any);
            }
            return Ok(-1);
        }
        Ok(self.crit(i).rec.i32_(Self::target_at(pick as usize)))
    }

    /// CritterMoveTargetPos 0x19b6c0 (waypoints unsupported).
    fn critter_move_target_pos(&mut self, i: usize) -> R<bool> {
        if self.ci16(i, c::NTARGETS) < 1 {
            // Waypoints: the current lookout until within 1 unit, then the
            // linked one (NextWaypoint) until the chain ends.
            while let Some(w) = self.crit(i).rec.u32_(c::WAYPOINT).checked_sub(1).map(|v| v as usize) {
                let lp = self.lookouts.get(w).ok_or_else(|| Unsupported("waypoint index".into()))?.position;
                let pos = self.crit(i).rec.v3(c::POS);
                let d = [sub(lp[0], pos[0])?, sub(lp[1], pos[1])?, sub(lp[2], pos[2])?];
                if 1.0 <= add(add(mul(d[0], d[0])?, mul(d[1], d[1])?)?, mul(d[2], d[2])?)? {
                    self.crm_pub(i).set_v3(c::TARGET_POS, lp);
                    return Ok(true);
                }
                let next = self.next_waypoint(w).map_or(0, |n| n as u32 + 1);
                self.crm_pub(i).set_u32(c::WAYPOINT, next);
            }
            self.crm_pub(i).set_v3(c::TARGET_POS, [0.0; 3]);
            return Ok(false);
        }
        let p = self.crit(i).rec.i32_(c::TARGETS) as usize;
        let pos = self.players[p].v3(0x70);
        self.crm_pub(i).set_v3(c::TARGET_POS, pos);
        Ok(true)
    }

    // ---- move selection --------------------------------------------------

    /// CritterMoveSetup 0x195ce0.
    fn critter_move_setup(&mut self, i: usize) {
        self.set_ci16(i, c::NEXT, -1);
        self.set_ci16(i, c::PATTERN_CAND, -1);
        self.set_ci16(i, c::TARGET_CAND, -1);
    }

    /// CritterFindMoveType 0x195350.
    fn critter_find_move_type(&self, i: usize, kind: i32, mode: i32) -> i16 {
        let mut pick: i32 = -1;
        let mut best = 0.0f32;
        let time = self.globals.time;
        for m in 0..self.move_count_pub(i).max(0) as usize {
            if self.critter_move_i32(i, m, 4) & 4 != 0 || self.critter_move_i32(i, m, 0) != kind {
                continue;
            }
            let delay = self.critter_move_f32(i, m, 0x80);
            let wait = if 0.0 < delay {
                let t = add(self.cf(i, c::MOVE_TIMES + 4 * m), delay).unwrap_or(f32::MAX);
                sub(t, time).unwrap_or(0.0)
            } else {
                0.0
            };
            if (wait <= 0.0 || mode != 0) && (pick < 0 || wait < best) {
                pick = m as i32;
                best = wait;
                if mode == 2 {
                    break;
                }
            }
        }
        if pick < 0 && mode != 0 {
            // Errorf, then any READY move.
            if kind != 0x20 {
                return self.critter_find_move_type(i, 0x20, 1);
            }
        }
        pick as i16
    }

    /// CritterChildCriticalMove 0x195fd0. The current-move record is read
    /// at +0x124 even when it is -1 (the preceding MOVE record).
    fn critter_child_critical_move(&mut self, i: usize) -> R<()> {
        let cur = self.ci16(i, c::MOVE);
        let q = self.globals.quest_item_state;
        if self.crit(i).rec.i32_(c::STATE) == 1 {
            let v = self.critter_find_move_type(i, 0x11, 1);
            self.set_ci16(i, c::NEXT, v);
        } else if q.wrapping_sub(3) < 3 && self.globals.boss_type == 0x23 {
            let v = self.critter_find_move_type(i, 0x20, 0);
            self.set_ci16(i, c::NEXT, v);
        } else {
            let n = self.critter_move_i16_signed(i, cur, 0x54)?;
            if 0 <= n {
                self.set_ci16(i, c::NEXT, n);
            }
        }
        if self.ci16(i, c::NEXT) < 0 {
            let flags = self.crit(i).rec.u32_(c::DAMAGE_FLAGS);
            let mut found = false;
            if flags & 0x120 != 0 {
                if flags & 0x100 != 0 {
                    let v = self.critter_find_move_type(i, 0x42, 0);
                    self.set_ci16(i, c::NEXT, v);
                }
                found = 0 <= self.ci16(i, c::NEXT);
                if !found {
                    let v = self.critter_find_move_type(i, 0x41, 0);
                    self.set_ci16(i, c::NEXT, v);
                }
            }
            if !found && self.ci16(i, c::NEXT) < 0 {
                let lim = mul(ROAR_MUL[self.critter_nactive.min(4)], 50.0)? as i32 as f32;
                if lim <= self.cf(i, c::DAMAGE_TAKEN) {
                    let v = self.critter_find_move_type(i, 0x22, 0);
                    self.set_ci16(i, c::NEXT, v);
                }
                if self.ci16(i, c::NEXT) < 0 {
                    if flags & 0x10 != 0 {
                        let v = self.critter_find_move_type(i, 0x40, 0);
                        self.set_ci16(i, c::NEXT, v);
                    }
                    if self.ci16(i, c::NEXT) < 0 {
                        return Ok(());
                    }
                }
            }
        }
        self.set_ci16(i, c::PATTERN_CAND, -2);
        Ok(())
    }

    /// CritterChildGetPattern 0x196a78: a child joins its root's pattern.
    fn critter_child_get_pattern(&mut self, root: usize, k: usize) {
        let p = self.ci16(root, c::PATTERN);
        if p < self.critter_type_i16(k, 0x114) {
            let step = self.ci16(root, c::PATTERN_STEP);
            self.set_ci16(k, c::PATTERN, p);
            self.set_ci16(k, c::PATTERN_STEP, step);
            let m = self.critter_pattern_i16(k, p as usize, (0x20 + 2 * i32::from(step)) as usize);
            self.set_ci16(k, c::NEXT, m);
        }
    }

    /// CritterResolveMultipleTargets 0x19b060: when a player is targeted by
    /// more instances than its anger allows (2 above 1.0, 3 above 0.75,
    /// else 4), the children's entries with the highest +0xc score for
    /// that player are dropped.
    fn critter_resolve_multiple_targets(&mut self, i: usize) {
        if self.crit(i).rec.u8_(c::NLIVE_CHILDREN) as i8 <= 0 || 0 <= self.ci16(i, c::PATTERN) {
            return;
        }
        let mut t = 0;
        while t < self.ci16(i, c::NTARGETS) as usize {
            let at = Self::target_at(t);
            let w = self.cf(i, at + 0x10);
            let p = self.crit(i).rec.i32_(at) as usize;
            let lim = if 1.0 < w { 2.0 } else if 0.75 < w { 3.0 } else { 4.0 };
            if p < 4 && lim < self.critter_ntargets[p] {
                loop {
                    let mut best: Option<(usize, usize)> = None;
                    let mut score = 0.0f32;
                    for k in self.critter_children(i) {
                        if 0 <= self.ci16(k, c::PATTERN) {
                            continue;
                        }
                        for e in 0..self.ci16(k, c::NTARGETS).max(0) as usize {
                            let ea = Self::target_at(e);
                            if self.crit(k).rec.i32_(ea) != p as i32 {
                                continue;
                            }
                            let sc = self.cf(k, ea + 0xc);
                            if best.is_none() || score < sc {
                                score = sc;
                                best = Some((k, e));
                                break;
                            }
                        }
                    }
                    let Some((k, e)) = best else { break };
                    let n = self.ci16(k, c::NTARGETS);
                    for m in e..(n as usize).saturating_sub(1) {
                        let bytes: Vec<u8> = (0..0x30).map(|b| self.crit(k).rec.u8_(Self::target_at(m + 1) + b)).collect();
                        for (b, v) in bytes.iter().enumerate() {
                            self.crm_pub(k).set_u8(Self::target_at(m) + b, *v);
                        }
                    }
                    self.set_ci16(k, c::NTARGETS, n - 1);
                    self.critter_ntargets[p] -= 1.0;
                    if self.critter_ntargets[p] <= lim {
                        break;
                    }
                }
            }
            t += 1;
        }
    }

    /// CritterLookForCriticalMove 0x195cf8.
    fn critter_look_for_critical_move(&mut self, i: usize) -> R<()> {
        let cur = self.ci16(i, c::MOVE);
        let st = self.crit(i).rec.i32_(c::STATE);
        let q = self.globals.quest_item_state;
        let pick: Option<(i32, i32)>;
        if cur < 0 || st == 0 || st == 2 {
            pick = Some((0, 1));
        } else {
            let mtype = self.critter_move_i32(i, cur as usize, 0);
            if mtype == 0 {
                pick = Some((0x10, 0));
            } else if st == 1 {
                pick = Some((0x11, 1));
            } else if 0 <= self.critter_move_i16(i, cur as usize, 0x54) {
                let n = self.critter_move_i16(i, cur as usize, 0x54);
                self.set_ci16(i, c::NEXT, n);
                pick = None;
            } else if q.wrapping_sub(1) < 2 {
                pick = Some((0x20, 1));
            } else if q == 3 && mtype != 0x22 {
                pick = Some((0x22, 1));
            } else if (q.wrapping_sub(3) < 3 && self.globals.boss_type == 0x23) || (mtype == 0x10 && self.critter_kind(i) == 4) {
                pick = Some((0x20, 0));
            } else {
                pick = None;
            }
        }
        if let Some((k, m)) = pick {
            let v = self.critter_find_move_type(i, k, m);
            self.set_ci16(i, c::NEXT, v);
        }
        let flags = self.crit(i).rec.u32_(c::DAMAGE_FLAGS);
        if self.ci16(i, c::NEXT) < 0 {
            if flags & 0x120 != 0 {
                if flags & 0x100 != 0 {
                    let v = self.critter_find_move_type(i, 0x42, 0);
                    self.set_ci16(i, c::NEXT, v);
                }
                if self.ci16(i, c::NEXT) < 0 {
                    let v = self.critter_find_move_type(i, 0x41, 0);
                    self.set_ci16(i, c::NEXT, v);
                }
            }
            if self.ci16(i, c::NEXT) < 0 {
                let lim = mul(ROAR_MUL[self.critter_nactive.min(4)], 50.0)? as i32 as f32;
                if lim <= self.cf(i, c::DAMAGE_TAKEN) {
                    let v = self.critter_find_move_type(i, 0x22, 0);
                    self.set_ci16(i, c::NEXT, v);
                }
                if self.ci16(i, c::NEXT) < 0 && flags & 0x10 != 0 {
                    let v = self.critter_find_move_type(i, 0x40, 0);
                    self.set_ci16(i, c::NEXT, v);
                }
            }
        }
        // crit_debug counting and the pattern reset on a critical move.
        if 0 <= self.ci16(i, c::NEXT) && 0 <= self.ci16(i, c::PATTERN) {
            self.set_ci16(i, c::PATTERN, -1);
            self.set_ci16(i, c::PATTERN_STEP, -1);
            for k in self.critter_children(i) {
                self.set_ci16(k, c::PATTERN, -1);
                self.set_ci16(k, c::PATTERN_STEP, -1);
            }
        }
        self.crm_pub(i).and_u32(c::DAMAGE_FLAGS, 0xffff_fecf);
        Ok(())
    }

    /// The +4 flag gates shared by the block/attack scans; true = skip.
    fn critter_move_gated(&self, i: usize, m: usize) -> bool {
        let fl = self.critter_move_i32(i, m, 4) as u32;
        if fl & 4 != 0 {
            return true;
        }
        if fl & 2 != 0 && self.crit(i).rec.u8_(c::NCHILDREN) != self.crit(i).rec.u8_(c::NLIVE_CHILDREN) {
            return true;
        }
        if fl & 0x10 != 0 {
            if self.critter_move_i16(i, m, 0xe) < 0 {
                return true;
            }
            let next = self.critter_move_i16(i, m, 0x54);
            if 0 <= next && self.critter_move_i16(i, next as usize, 0xe) < 0 {
                return true;
            }
        }
        false
    }

    /// CritterLookForBlock 0x196170.
    fn critter_look_for_block(&mut self, i: usize) -> R<()> {
        let time = self.globals.time;
        for m in 0..self.move_count_pub(i).max(0) as usize {
            if self.critter_move_i32(i, m, 0) != 0x23 || self.critter_move_gated(i, m) {
                continue;
            }
            let delay = self.critter_move_f32(i, m, 0x80);
            if !(delay <= 0.0 || add(self.cf(i, c::MOVE_TIMES + 4 * m), delay)? <= time) {
                continue;
            }
            let q = Query::at(self.critter_move_rec(i, m), 0x60);
            let p = self.critter_move_get_target(i, Some(q), false)?;
            if 0 <= p && self.player_attacking(p as usize, 1) {
                self.set_ci16(i, c::NEXT, m as i16);
            }
        }
        Ok(())
    }

    /// PlayerAttacking 0x136e28 (player +0x8c8).
    fn player_attacking(&self, p: usize, strict: i32) -> bool {
        let a = self.players[p].i32_(0x8c8);
        if 0xb <= a || (strict < 2 && (a == 2 || a == 5 || a == 10)) {
            return true;
        }
        strict < 1 && 1 < a
    }

    /// CritterLookForAttack 0x1962c8.
    fn critter_look_for_attack(&mut self, i: usize) -> R<()> {
        let time = self.globals.time;
        let mut tgt: i32 = -1;
        let mut ptrn: i32 = -1;
        let mut single: i32 = -1;
        let mut best = 999999.0f32;
        let pat = self.ci16(i, c::PATTERN);
        if 0 <= pat {
            let step = self.ci16(i, c::PATTERN_STEP) as i32 + 1;
            if step < 8 {
                let m = self.critter_pattern_i16(i, pat as usize, 0x20 + 2 * step as usize);
                self.set_ci16(i, c::NEXT, m);
                if 0 <= m {
                    self.set_ci16(i, c::PATTERN_CAND, pat);
                    return Ok(());
                }
            }
        }
        let npat = self.critter_type_i16(i, 0x114).max(0) as usize;
        for p in 0..npat {
            if p as i16 == self.ci16(i, c::PATTERN) {
                continue;
            }
            let fl = self.critter_pattern_i16(i, p, 0x10) as u16;
            if fl & 2 != 0 && self.crit(i).rec.u8_(c::NCHILDREN) != self.crit(i).rec.u8_(c::NLIVE_CHILDREN) {
                continue;
            }
            if fl & 0x1000 != 0 {
                continue;
            }
            let last = self.cf(i, c::PATTERN_TIMES + 4 * p);
            if add(last, self.critter_pattern_f32(i, p, 0x14))? <= time {
                let q = Query::at(self.critter_pattern_rec(i, p), 0x30);
                let t = self.critter_move_get_target(i, Some(q), false)?;
                if 0 <= t && last < best {
                    tgt = t;
                    best = last;
                    ptrn = p as i32;
                }
            }
        }
        let cur = self.ci16(i, c::MOVE);
        for m in 0..self.move_count_pub(i).max(0) as usize {
            if m as i16 == cur {
                continue;
            }
            let mt = self.critter_move_i32(i, m, 0) as u32;
            if !(0x7f..0xf0).contains(&mt) || self.critter_move_gated(i, m) {
                continue;
            }
            if 0 <= self.ci16(i, c::GRABBED) && mt == 0x81 {
                ptrn = -1;
                single = m as i32;
                break;
            }
            let delay = self.critter_move_f32(i, m, 0x80);
            let last = self.cf(i, c::MOVE_TIMES + 4 * m);
            if !(delay <= 0.0 || add(last, delay)? <= time) {
                continue;
            }
            let q = Query::at(self.critter_move_rec(i, m), 0x60);
            let t = self.critter_move_get_target(i, Some(q), false)?;
            if t < 0 {
                continue;
            }
            // A longer-idle move wins; otherwise one that may interrupt the
            // current single candidate (only while no pattern is chosen).
            if last < best || (ptrn < 0 && (single < 0 || 1 < self.critter_anim_interrupt(i, single as usize, m))) {
                ptrn = -1;
                single = m as i32;
                tgt = t;
                best = last;
            }
        }
        if 0 <= ptrn {
            self.set_ci16(i, c::PATTERN_CAND, ptrn as i16);
            self.set_ci16(i, c::TARGET_CAND, tgt as i16);
            let m = self.critter_pattern_i16(i, ptrn as usize, 0x20);
            self.set_ci16(i, c::NEXT, m);
        } else if 0 <= single {
            self.set_ci16(i, c::NEXT, single as i16);
            self.set_ci16(i, c::TARGET_CAND, tgt as i16);
        }
        Ok(())
    }

    /// CritterLookForTranslate 0x196688.
    fn critter_look_for_translate(&mut self, i: usize) -> R<()> {
        if self.critter_type_u32(i, 0x5c) & 0x10000 == 0 || !self.critter_move_target_pos(i)? {
            return Ok(());
        }
        let time = self.globals.time;
        let tpos = self.crit(i).rec.v3(c::TARGET_POS);
        let mut best = 1e21f32;
        let mut pick: i32 = -1;
        for m in 0..self.move_count_pub(i).max(0) as usize {
            let mt = self.critter_move_i32(i, m, 0) as u32;
            if !(0x30..0x3a).contains(&mt) {
                continue;
            }
            if mt == 0x38 && self.ci16(i, c::TARGET) < 0 {
                continue;
            }
            if self.critter_move_i32(i, m, 4) & 4 != 0 {
                continue;
            }
            if self.ci16(i, c::NTARGETS) == 0 && self.crit(i).rec.u32_(c::WAYPOINT) != 0 && 0.0 < self.critter_move_f32(i, m, 0x84) {
                pick = m as i32;
                break;
            }
            let delay = self.critter_move_f32(i, m, 0x80);
            if !(delay <= 0.0 || add(self.cf(i, c::MOVE_TIMES + 4 * m), delay)? <= time) {
                continue;
            }
            let q = Query::at(self.critter_move_rec(i, m), 0x60);
            let (v, _) = self.critter_calc_target(i, Some(q), tpos)?;
            if v < best {
                best = v;
                pick = m as i32;
            }
        }
        if 0 <= pick {
            self.set_ci16(i, c::NEXT, pick as i16);
        }
        Ok(())
    }

    /// CritterLookForReady 0x196838.
    fn critter_look_for_ready(&mut self, i: usize) -> R<()> {
        let mut m = -1;
        if self.cf(i, c::DIFFICULTY) < f32::from_bits(0x3f4c_cccd) {
            m = self.critter_find_move_type(i, 0x21, 0);
        }
        if m < 0 {
            m = self.critter_find_move_type(i, 0x20, 1);
        }
        self.set_ci16(i, c::NEXT, m);
        Ok(())
    }

    /// CritterGetNextMove 0x1968a0 (AuxLevels 0x80 debug stepping).
    fn critter_get_next_move(&mut self, _i: usize) -> R<()> {
        unsupported("CritterGetNextMove (AuxLevels 0x80)")
    }

    /// CritterAnimInterrupt 0x195470 between moves a (current) and b.
    fn critter_anim_interrupt(&self, i: usize, a: usize, b: usize) -> u32 {
        let pa = self.critter_move_i32(i, a, 8);
        let pb = self.critter_move_i32(i, b, 8);
        match self.critter_move_i16(i, a, 0x56) {
            0 => 0,
            0x14 => {
                if (pb as u32 & 0xffff_ff00) as i32 <= (pa as u32 & 0xffff_ff00) as i32 {
                    1
                } else {
                    2
                }
            }
            0x3c => {
                if pb < pa {
                    1
                } else {
                    2
                }
            }
            0x50 => {
                if pb < 1 {
                    1
                } else {
                    2
                }
            }
            0x5a => 2,
            _ => {
                if pa < pb {
                    2
                } else {
                    1
                }
            }
        }
    }

    /// CritterGetDmove 0x195540.
    fn critter_get_dmove(&self, i: usize) -> i16 {
        let cand = self.ci16(i, c::PATTERN_CAND);
        if cand < 0 {
            return self.ci16(i, c::NEXT);
        }
        let step = self.crit(i).rec.u16_(c::PATTERN_STEP);
        if 7 < step {
            return self.ci16(i, c::NEXT);
        }
        let m = self.critter_pattern_i16(i, cand as usize, 0x20 + 2 * (step as usize + 1));
        if m < 0 { self.ci16(i, c::NEXT) } else { m }
    }

    // ---- animation -------------------------------------------------------

    /// CritterAnimate 0x195778 (with the native pose update).
    fn critter_animate(&mut self, i: usize) -> R<()> {
        let cur = self.ci16(i, c::MOVE);
        let mut dmove = self.critter_get_dmove(i);
        let time = self.globals.time;
        let aux80 = self.globals.aux_levels & 0x80 != 0;
        let cur_m = usize::try_from(cur).ok();
        let new_m = usize::try_from(dmove).ok();
        let seq_of = |g: &Game, m: usize| g.critter_move_i16(i, m, 0xc);
        let (seq, mut mode): (i16, i32);
        match new_m {
            None => {
                let m = cur_m.ok_or_else(|| Unsupported("CritterAnimate without moves".into()))?;
                seq = seq_of(self, m);
                mode = 0;
            }
            Some(n) => {
                // The hold path (LAB_0019583c/40) versus the direct mode-3 start.
                let hold = aux80
                    || Some(n) == cur_m
                    || self.critter_move_i32(i, n, 8) < 0xf00
                    || cur_m.is_some_and(|m| self.critter_move_i16(i, m, 0x56) == 0);
                if !hold {
                    seq = seq_of(self, n);
                    mode = 3;
                } else if time < self.cf(i, c::HOLD_UNTIL) {
                    seq = cur_m.map_or(-1, |m| seq_of(self, m));
                    mode = 0;
                    dmove = cur;
                } else if aux80 {
                    return unsupported("CritterAnimate AuxLevels 0x80");
                } else {
                    seq = seq_of(self, n);
                    mode = match cur_m {
                        Some(m) => self.critter_anim_interrupt(i, m, n) as i32,
                        None => 3,
                    };
                }
            }
        }
        if seq < 0 {
            return unsupported("critter move without a sequence");
        }
        if cur_m != new_m && mode == 0 && self.cf(i, c::HOLD_UNTIL) < time {
            mode = i32::from(self.crit(i).clock.mode_bits & 0xff != 0);
        }
        let mut flags = 0u32;
        if self.ci16(i, c::PAUSE) < 1 {
            flags = self.critter_animate_tree(i, seq as u16, mode)?;
        } else {
            let v = add(self.cf(i, c::ANIM_START), self.globals.spf)?;
            self.crm_pub(i).set_f32(c::ANIM_START, v);
        }
        if cur_m.is_some() && cur_m == new_m && cur_m.is_some_and(|m| self.critter_move_i32(i, m, 0) == 0) {
            flags = 0;
        }
        self.crm_pub(i).set_u16(c::MOVE_DONE, (flags & 3) as u16);
        if flags & 3 == 0 {
            if dmove < 0 && self.crit(i).clock.mode_bits & 0xff != 0 {
                self.set_ci16(i, c::MOVE, -1);
            }
        } else {
            self.critter_move_done(i, dmove)?;
        }
        self.critter_update_pose(i)?;
        Ok(())
    }

    /// AnimateATree on the critter clock; returns the AnimateTree mask.
    fn critter_animate_tree(&mut self, i: usize, seq: u16, mode: i32) -> R<u32> {
        let cr = self.crit(i);
        let clips = &cr.clips;
        let current = *clips.get(cr.clip as usize).or_else(|| clips.get(seq as usize)).ok_or_else(|| Unsupported("critter clip".into()))?;
        let requested = *clips.get(seq as usize).ok_or_else(|| Unsupported("critter requested clip".into()))?;
        let out = select(
            cr.clock,
            current,
            requested,
            SelectionInput {
                sequence: seq,
                mode,
                start_frame: 0,
                blend_duration: 0.0,
                playback_scale: 1.0,
                now: self.globals.atime,
                anim_time_scale: self.globals.anim_time_scale,
            },
        )?;
        let switched = out.return_flags & 1 != 0;
        let cr = self.crit_mut(i);
        cr.clock = out.state;
        if switched {
            cr.clip = seq;
        }
        let (s, f, fr) = (cr.clock.sequence, cr.clock.frame, cr.clips.get(cr.clip as usize).map_or(0, |c| c.frames));
        cr.rec.set_u16(c::ANIM_SEQ, s);
        cr.rec.set_f32(c::ANIM_FRAME, f);
        cr.rec.set_i16(c::ANIM_FRAMES, fr);
        Ok(out.return_flags)
    }

    /// Native pose of the critter tree at its clock (blending not modelled).
    fn critter_update_pose(&mut self, i: usize) -> R<()> {
        let t = self.crit(i).ty;
        let Some(tree) = self.critter_data.files[t.0].trees[t.1] else { return Ok(()) };
        let Some(model) = self.critter_data.files[t.0].model.clone() else { return Ok(()) };
        let cr = self.crit(i);
        if cr.clock.sequence == u16::MAX {
            return Ok(());
        }
        let frame = cr.clock.frame.max(0.0);
        let seq = u32::from(cr.clock.sequence);
        // Children animate their detached subtrees; the root everything else.
        let owner = self.critter_pose_owner(i);
        let roots: Vec<(usize, usize)> = self.critter_children(owner).into_iter().map(|k| (self.crit(k).anim_root, k)).collect();
        let parents = self.crit(owner).parents.clone();
        let animator = |n: usize| -> usize {
            let mut m = n as i32;
            for _ in 0..=parents.len() {
                let Ok(u) = usize::try_from(m) else { break };
                if let Some(&(_, k)) = roots.iter().find(|r| r.0 == u) {
                    return k;
                }
                m = parents.get(u).copied().unwrap_or(-1);
            }
            owner
        };
        let locals = local_pose_where(&model.0, &model.1, tree, seq, frame, &|n| animator(n) == i).map_err(|e| Unsupported(format!("critter pose: {}", e.0)))?;
        let out = &mut self.crit_mut(owner).locals;
        for (n, m) in locals.into_iter().enumerate() {
            if let (Some(m), Some(slot)) = (m, out.get_mut(n)) {
                *slot = m;
            }
        }
        Ok(())
    }

    /// CritterCopyAnim 0x195a20: the child's subtree takes the root's
    /// sequence at the rounded frame (AnimateATreeFrame →
    /// AnimateTreeFrameInterp with no interpolation, then AnimateNode).
    fn critter_copy_anim(&mut self, root: usize, k: usize) -> R<()> {
        let seq = self.crit(root).rec.u16_(c::ANIM_SEQ);
        let frame = add(self.cf(root, c::ANIM_FRAME), 0.5)? as i32;
        let cr = self.crit_mut(k);
        if (seq as i16) < cr.clips.len() as i16 {
            if seq != cr.clock.sequence {
                cr.clock.start_time = cr.clock.last_time;
                cr.clock.sequence = seq;
            }
            let frames = cr.clips[seq as usize].frames;
            cr.clock.blend_fraction = 0.0;
            let mut f = frame as f32;
            let last = (frames as i32 - 1) as f32;
            if last < f {
                f = last;
            }
            if f < 0.0 {
                f = 0.0;
            }
            cr.clock.frame = f;
            cr.clock.blend_end_time = 0.0;
            cr.clock.reset_byte = 1;
            cr.clip = seq;
            cr.rec.set_u16(c::ANIM_SEQ, seq);
            cr.rec.set_i16(c::ANIM_FRAMES, frames);
            cr.rec.set_f32(c::ANIM_FRAME, f);
        }
        self.critter_update_pose(k)?;
        let done = self.crit(root).rec.u16_(c::MOVE_DONE);
        let cr = self.crit_mut(k);
        cr.rec.set_u16(c::MOVE_DONE, done);
        cr.rec.set_i16(c::MOVE, -1);
        cr.rec.set_i16(c::PATTERN, -1);
        cr.rec.set_i16(c::PATTERN_STEP, -1);
        Ok(())
    }

    /// CritterMoveDone 0x195598.
    fn critter_move_done(&mut self, i: usize, dmove: i16) -> R<()> {
        let cur = self.ci16(i, c::MOVE);
        if 0 <= cur {
            let mt = self.critter_move_i32(i, cur as usize, 0);
            if mt == 0x10 && self.critter_move_i16(i, cur as usize, 0x54) < 0 && self.globals.quest_item_state == 1 {
                self.globals.quest_item_state = 2;
            } else if mt == 0x22 && self.globals.quest_item_state == 3 {
                self.globals.quest_item_state = 4;
            }
        }
        let time = self.globals.time;
        if self.ci16(i, c::PATTERN) < 0 {
            let cand = self.ci16(i, c::PATTERN_CAND);
            if cand < 0 {
                if let Ok(m) = usize::try_from(dmove) {
                    let frames = self.ci16(i, c::ANIM_FRAMES) as i32 - 2;
                    let v = add(mul(frames as f32, f32::from_bits(0x3d08_8889))?, time)?;
                    self.crm_pub(i).set_f32(c::MOVE_TIMES + 4 * m, v);
                }
            } else {
                self.crm_pub(i).set_f32(c::PATTERN_TIMES + 4 * cand as usize, time);
                self.set_ci16(i, c::PATTERN, cand);
                self.set_ci16(i, c::PATTERN_STEP, 0);
            }
        } else if self.crit(i).parent.is_none_or(|p| self.ci16(p, c::PATTERN) < 0) {
            // A child follows its root's pattern without stepping it.
            let step = self.ci16(i, c::PATTERN_STEP) + 1;
            self.set_ci16(i, c::PATTERN_STEP, step);
            let mut next = -1;
            if step < 8 {
                next = self.critter_pattern_i16(i, self.ci16(i, c::PATTERN) as usize, 0x20 + 2 * step as usize);
            }
            if next < 0 {
                self.set_ci16(i, c::PATTERN, -1);
                self.set_ci16(i, c::PATTERN_STEP, -1);
                for k in self.critter_children(i) {
                    self.set_ci16(k, c::PATTERN, -1);
                    self.set_ci16(k, c::PATTERN_STEP, -1);
                }
            }
        }
        let fxi = self.ci16(i, c::MOVE_FX);
        if 0 <= fxi {
            self.delete_effect_pub(fxi as usize);
            self.set_ci16(i, c::MOVE_FX, -1);
        }
        self.set_ci16(i, c::MOVE, dmove);
        self.crm_pub(i).set_f32(c::HOLD_UNTIL, 0.0);
        Ok(())
    }

    // ---- per-move effects ------------------------------------------------

    /// CritterDoTransition 0x196cc0.
    fn critter_do_transition(&mut self, i: usize, mv: usize) -> R<()> {
        let tgt = self.ci16(i, c::TARGET);
        let done = self.crit(i).rec.u16_(c::MOVE_DONE) != 0;
        if tgt < 0 || done {
            if self.ci16(i, c::SLOW) < 1 {
                let cand = self.ci16(i, c::TARGET_CAND);
                if cand < 0 {
                    let q = Query::at(self.critter_move_rec(i, mv), 0x60);
                    let p = self.critter_move_get_target(i, Some(q), true)?;
                    self.set_ci16(i, c::TARGET, p as i16);
                } else {
                    self.set_ci16(i, c::TARGET, cand);
                }
            } else {
                self.set_ci16(i, c::TARGET, -1);
            }
            if done {
                self.crm_pub(i).set_u16(c::ACTION_FLAGS, 0);
                self.crm_pub(i).set_u16(c::SFX_FLAGS, 0);
                let node = self.critter_move_i16(i, mv, 0xe);
                let ar = self.crit(i).anim_root;
                let n = usize::try_from(node).ok().filter(|&k| k < self.crit(i).parents.len()).or(Some(ar));
                self.crit_mut(i).node_dc = n;
            }
        }
        let prev = self.crit(i).rec.v3(c::NODE_POS);
        self.crm_pub(i).set_v3(c::PREV_NODE_POS, prev);
        let node = self.crit(i).node_dc;
        self.critter_node_matrix_pub(i, node, c::NODE_MATRIX, None)?;
        Ok(())
    }

    /// CritterGetDoAction 0x195a90: bit 0 first window, bit 1 second.
    fn critter_get_do_action(&mut self, i: usize, mv: usize, frame: i32) -> u32 {
        let mt = self.critter_move_i32(i, mv, 0);
        let s0 = self.critter_move_i32(i, mv, 0x40);
        let s1 = self.critter_move_i32(i, mv, 0x44);
        let e0 = self.critter_move_i16(i, mv, 0x50) as i32;
        let e1 = self.critter_move_i16(i, mv, 0x52) as i32;
        let interval = self.critter_move_f32(i, mv, 0x4c);
        let af = self.crit(i).rec.u16_(c::ACTION_FLAGS);
        let fmodz = |a: i32, b: f32| -> bool {
            let r = (a as f32) % b;
            r as i32 == 0
        };
        match mt {
            0x80 | 0x83 | 0x86 => {
                let mut b = u32::from(s0 <= frame && frame <= e0);
                if 0 <= s1 && s1 <= frame && frame <= e1 {
                    b |= 2;
                }
                b
            }
            0x85 => {
                let mut b = 0;
                if s0 <= frame && frame <= e0 && (interval <= 0.0 || fmodz(frame - s0, interval)) {
                    b = 1;
                }
                if 0 <= s1 && s1 <= frame && frame <= e1 && (interval <= 0.0 || fmodz(frame - s1, interval)) {
                    b |= 2;
                }
                b
            }
            0x88 => {
                let mut b = 0;
                if af & 1 == 0 && s0 <= frame && 0 <= self.ci16(i, c::TARGET) {
                    // GetPlayerColPos(target, +0x240).
                    let p = self.players[self.ci16(i, c::TARGET) as usize].v3(0x70);
                    self.crm_pub(i).set_v3(c::TARGET_POS, p);
                    b = 1;
                }
                if 0 <= s1 && af & 1 != 0 && af & 2 == 0 && s1 <= frame {
                    b |= 2;
                }
                b
            }
            0x81 => {
                let mut b = u32::from(s0 <= frame && frame <= e0);
                if 0 <= s1 && af & 2 == 0 && s1 <= frame {
                    b |= 2;
                }
                b
            }
            _ => {
                let mut b = if af & 1 == 0 { u32::from(s0 <= frame) } else { 0 };
                if 0 <= s1 && af & 2 == 0 && s1 <= frame {
                    b |= 2;
                }
                b
            }
        }
    }

    /// CritterDoActions 0x196ac8.
    fn critter_do_actions(&mut self, i: usize, mv: usize, frame: i16) -> R<()> {
        if 0 <= self.critter_move_i32(i, mv, 0x40) {
            let b = self.critter_get_do_action(i, mv, frame as i32);
            let before = self.crit(i).rec.u16_(c::ACTION_FLAGS);
            let is85 = self.critter_move_i32(i, mv, 0) == 0x85;
            if b & 1 != 0 {
                if !is85 {
                    self.crm_pub(i).or_u16(c::ACTION_FLAGS, 1);
                }
                let d = self.critter_move_i16(i, mv, 0x48);
                if 0 <= d {
                    self.critter_do_damage(i, d as usize, 1, (before ^ 1) & 1 != 0)?;
                }
            }
            if b & 2 != 0 {
                if !is85 {
                    self.crm_pub(i).or_u16(c::ACTION_FLAGS, 2);
                }
                let d = self.critter_move_i16(i, mv, 0x4a);
                if 0 <= d {
                    self.critter_do_damage(i, d as usize, 2, ((before >> 1) ^ 1) & 1 != 0)?;
                }
            }
        }
        let d = self.critter_move_i16(i, mv, 0x48);
        if 0 <= d {
            let rec = self.critter_damage_rec(i, d as usize);
            if CritterFile::i16_pub(&rec, 2) as u16 & 0x4000 != 0 && 0.0 < self.cf(i, c::FX_HOLD) && CritterFile::i16_pub(&rec, 0) != 1 {
                return Ok(());
            }
        }
        let sf = self.crit(i).rec.u16_(c::SFX_FLAGS);
        let s0 = self.critter_move_i16(i, mv, 0x58);
        if sf & 1 == 0 && 0 <= s0 && self.critter_move_i16(i, mv, 0x5a) <= frame {
            self.crm_pub(i).or_u16(c::SFX_FLAGS, 1);
            self.critter_do_sfx(i, s0 as usize, None, true)?;
        }
        let sf = self.crit(i).rec.u16_(c::SFX_FLAGS);
        let s1 = self.critter_move_i16(i, mv, 0x5c);
        if sf & 2 == 0 && 0 <= s1 && self.critter_move_i16(i, mv, 0x5e) <= frame {
            self.crm_pub(i).or_u16(c::SFX_FLAGS, 2);
            self.critter_do_sfx(i, s1 as usize, None, true)?;
        }
        Ok(())
    }

    /// CritterTranslate 0x197110: bosses (kind 4) move on a leash around
    /// +0x4e0 without world collision; other kinds collide (critter_move.rs).
    /// True when a player was pushed.
    fn critter_translate(&mut self, i: usize, mv: usize) -> R<bool> {
        let speed = mul(mul(self.critter_move_f32(i, mv, 0x84), self.globals.spf)?, self.level_f32(0xb0))?;
        let leash = self.critter_type_f32(i, 0xac);
        if leash <= 0.0 {
            return Ok(false);
        }
        let mt = self.critter_move_i32(i, mv, 0);
        let pos = self.crit(i).rec.v3(c::POS);
        let (dx, dz);
        if mt == 0x38 {
            let t = self.crit(i).rec.v3(c::TARGET_POS);
            let (nx, nz) = get_vector_normal_xz([sub(t[0], pos[0])?, 0.0, sub(t[2], pos[2])?])?;
            dx = mul(speed, nx)?;
            dz = mul(speed, nz)?;
        } else {
            let src = if self.critter_type_u32(i, 0x5c) & 0x40 == 0 { c::MATRIX + 0x20 } else { c::HOME_MATRIX + 0x20 };
            let (fx, fz) = get_vector_normal_xz(self.crit(i).rec.v3(src))?;
            match mt {
                0x32 => {
                    dx = mul(-speed, fz)?;
                    dz = mul(speed, fx)?;
                }
                0x33 => {
                    dx = mul(speed, fz)?;
                    dz = mul(-speed, fx)?;
                }
                0x35 => {
                    dx = mul(-speed, fx)?;
                    dz = mul(-speed, fz)?;
                }
                0x36 => {
                    dx = add(mul(-speed, fz)?, mul(speed, fx)?)?;
                    dz = add(mul(speed, fx)?, mul(speed, fz)?)?;
                }
                _ => {
                    dx = mul(speed, fx)?;
                    dz = mul(speed, fz)?;
                }
            }
        }
        let vel = self.crit(i).rec.v3(c::VELOCITY);
        let spf = self.globals.spf;
        let mut p = [
            add(add(pos[0], mul(vel[0], spf)?)?, dx)?,
            add(add(pos[1], mul(vel[1], spf)?)?, 0.0)?,
            add(add(pos[2], mul(vel[2], spf)?)?, dz)?,
        ];
        let k = f32::from_bits(0x3f4c_cccd);
        let mut v = [mul(vel[0], k)?, mul(vel[1], k)?, mul(vel[2], k)?];
        for x in v.iter_mut() {
            if x.abs() < 0.01 {
                *x = 0.0;
            }
        }
        if 0.0 < v[1] {
            v[1] = sub(v[1], mul(spf, 100.0)?)?;
            if v[1] < 0.0 {
                v[1] = 0.0;
            }
        }
        self.crm_pub(i).set_v3(c::VELOCITY, v);
        if self.critter_kind(i) != 4 {
            let pos0 = self.crit(i).rec.v3(c::POS);
            let mut d = [sub(p[0], pos0[0])?, sub(p[1], pos0[1])?, sub(p[2], pos0[2])?];
            let pushed = self.critter_collide_move(i, &mut d)?;
            let np = [add(pos0[0], d[0])?, add(pos0[1], d[1])?, add(pos0[2], d[2])?];
            self.crm_pub(i).set_v3(c::POS, np);
            return Ok(pushed);
        }
        let centre = self.crit(i).rec.v3(c::LEASH_CENTRE);
        p = [sub(p[0], centre[0])?, sub(p[1], centre[1])?, sub(p[2], centre[2])?];
        if self.crit(i).rec.i32_(c::STATE) != 1 {
            let d = crate::vu0::fqdist(p[0], p[2])?.0;
            self.crm_pub(i).set_f32(c::LEASH_DIST, d);
            if self.critter_type_u32(i, 0x5c) & 0x20 == 0 {
                if leash < d {
                    let s = div(leash, d)?;
                    p[0] = mul(s, p[0])?;
                    p[2] = mul(s, p[2])?;
                }
            } else {
                p[0] = p[0].clamp(-leash, leash);
                p[2] = p[2].clamp(-leash, leash);
            }
        }
        self.crm_pub(i).set_v3(c::POS, [add(centre[0], p[0])?, add(centre[1], p[1])?, add(centre[2], p[2])?]);
        Ok(false)
    }

    /// CritterRotate 0x196dc0.
    fn critter_rotate(&mut self, i: usize, mv: usize) -> R<()> {
        let max = self.critter_type_f32(i, 0xcc);
        let mut rate = self.critter_move_f32(i, mv, 0x88);
        let yaw = self.cf(i, c::YAW);
        let delta: f32;
        if self.critter_move_i32(i, mv, 4) & 0x20 != 0 {
            delta = sub(self.cf(i, c::YAW_TARGET), yaw)?;
        } else if 0 <= self.ci16(i, c::GRABBED) {
            delta = 0.0;
        } else if 0 <= self.ci16(i, c::TARGET) && 0.0 < rate {
            let tp = self.players[self.ci16(i, c::TARGET) as usize].v3(0x70);
            let pos = self.crit(i).rec.v3(c::POS);
            let d = [sub(tp[0], pos[0])?, sub(tp[1], pos[1])?, sub(tp[2], pos[2])?];
            let a = crate::libm::atan2f(d[0], d[2]).map_err(|_| Unsupported("atan2f".into()))?;
            if self.critter_type_u32(i, 0x5c) & 0x400 == 0 {
                let base = self.cf(i, c::YAW_TARGET);
                let mut r = wrap(sub(a, base)?)?;
                r = r.clamp(-max, max);
                delta = sub(add(r, base)?, yaw)?;
            } else {
                delta = sub(a, yaw)?;
            }
        } else if let (Some(w), 0) = (self.crit(i).rec.u32_(c::WAYPOINT).checked_sub(1), self.ci16(i, c::NTARGETS)) {
            let lp = self.lookouts.get(w as usize).ok_or_else(|| Unsupported("waypoint index".into()))?.position;
            let pos = self.crit(i).rec.v3(c::POS);
            let d = [sub(lp[0], pos[0])?, sub(lp[1], pos[1])?, sub(lp[2], pos[2])?];
            let a = crate::libm::atan2f(d[0], d[2]).map_err(|_| Unsupported("atan2f".into()))?;
            delta = sub(a, yaw)?;
        } else {
            delta = 0.0;
        }
        if delta != 0.0 {
            let d = wrap(delta)?;
            rate = mul(rate, self.globals.spf)?;
            if 0 < self.ci16(i, c::SLOW) {
                rate = mul(rate, f32::from_bits(0x3dcc_cccd))?;
            }
            let step = d.clamp(-rate, rate);
            let v = add(step, yaw)?;
            self.crm_pub(i).set_f32(c::YAW, v);
            self.critter_set_yaw_matrix_pub(i)?;
        }
        Ok(())
    }

    /// CritterLookAtPlayer 0x19c600: decrements +0xb4e. The head/neck turn
    /// (NodeLookAtPos on +0xd4/+0xd8) is not applied to the native pose
    /// (labelled approximation).
    fn critter_look_at_player(&mut self, i: usize, mv: Option<usize>) -> R<()> {
        if let Some(m) = mv {
            let mt = self.critter_move_i32(i, m, 0);
            if mt == 0x10 || mt == 0 {
                return Ok(());
            }
        }
        if 0 < self.ci16(i, c::SLOW) {
            let v = self.ci16(i, c::SLOW).wrapping_sub(self.globals.vb_elapsed as i16);
            self.set_ci16(i, c::SLOW, v);
        }
        Ok(())
    }

    /// CritterUpdateCounters 0x1988b0.
    fn critter_update_counters(&mut self, i: usize) -> R<()> {
        let time = self.globals.time;
        let mv = self.ci16(i, c::MOVE).max(0) as usize;
        let mt = self.critter_move_i32(i, mv, 0) as u32;
        let hit = self.cf(i, c::HIT_TIME);
        let stale = 0.0 < hit && 3.0 < sub(time, hit)?;
        if stale || mt == 0x22 || mt.wrapping_sub(0x40) <= 0x3e {
            let r = self.crm_pub(i);
            r.set_f32(c::DAMAGE_TAKEN, 0.0);
            r.set_u32(c::DAMAGE_FLAGS, 0);
            r.set_f32(c::HIT_TIME, 0.0);
        }
        for p in 0..4 {
            for (t, v) in [(c::ANGER + 0x10 * p + 4, c::ANGER + 0x10 * p), (c::ANGER + 0x10 * p + 0xc, c::ANGER + 0x10 * p + 8)] {
                let ts = self.cf(i, t);
                if 0.0 < ts && 15.0 < sub(time, ts)? {
                    let r = self.crm_pub(i);
                    r.set_f32(v, 0.0);
                    r.set_f32(t, 0.0);
                }
            }
        }
        Ok(())
    }

    /// CritterDoKnockback 0x1989e8 (bosses skip it): +0x500 knock impulse ×
    /// strength (20 dead, 10 for damage flags 0x10140, 7.5 for 0x20, 5 for
    /// 0x10; golems 5 less) into the +0x510 velocity, capped at length 40.
    fn critter_do_knockback(&mut self, i: usize) -> R<()> {
        let kind = self.critter_kind(i);
        if kind == 4 {
            return Ok(());
        }
        let fl = self.crit(i).rec.u32_(c::DAMAGE_FLAGS);
        let mut k = if self.cf(i, c::HEALTH) <= 0.0 {
            20.0
        } else if fl & 0x10140 != 0 {
            10.0
        } else if fl & 0x20 != 0 {
            7.5
        } else if fl & 0x10 != 0 {
            5.0
        } else {
            0.0
        };
        if kind == 3 {
            k = sub(k, 5.0)?;
        }
        if 0.0 < k {
            let kn = self.crit(i).rec.v3(c::KNOCK);
            let v0 = self.crit(i).rec.v3(c::VELOCITY);
            let mut v = [add(mul(k, kn[0])?, v0[0])?, add(mul(k, kn[1])?, v0[1])?, add(mul(k, kn[2])?, v0[2])?];
            let q = add(add(mul(v[0], v[0])?, mul(v[1], v[1])?)?, mul(v[2], v[2])?)?;
            if 1600.0 < q {
                crate::collision::normal_vector_pub(&mut v)?;
                v = [mul(v[0], 40.0)?, mul(v[1], 40.0)?, mul(v[2], 40.0)?];
            }
            let r = self.crm_pub(i);
            r.set_v3(c::VELOCITY, v);
            r.set_v3(c::KNOCK, [0.0; 3]);
        }
        Ok(())
    }

    /// CritterGetSingleTargetPlayer 0x19b260 (golem AI): the lowest-score
    /// active, targetable player as the only target (score × 1000 while the
    /// player's +0x8c0 hold runs); a target clears the waypoint.
    fn critter_get_single_target_player(&mut self, i: usize) -> R<()> {
        self.set_ci16(i, c::NTARGETS, 0);
        if self.cf(i, c::HEALTH) <= 0.0 {
            return Ok(());
        }
        let q = Query::at(self.critter_type_rec(i), 0x80);
        for p in 0..4 {
            let pl = &self.players[p];
            if pl.i32_(pd::STATE) != 1 || pl.u32_(0x138) & 4 != 0 {
                continue;
            }
            let pos = pl.v3(0x70);
            let (mut score, mut rec) = self.critter_calc_target(i, Some(q), pos)?;
            let gate = self.crit(i).rec.u32_(c::WAYPOINT) == 0 || self.cf(i, 0xb58) <= 0.0 || score <= self.cf(i, 0xb58);
            if !gate {
                continue;
            }
            if self.globals.time < self.players[p].f32_(0x8c0) {
                score = mul(score, 1000.0)?;
            }
            if self.ci16(i, c::NTARGETS) != 0 && self.cf(i, Self::target_at(0) + 0xc) <= score {
                continue;
            }
            self.set_ci16(i, c::NTARGETS, 1);
            rec[0] = f32::from_bits(p as u32);
            rec[3] = score;
            rec[4] = 1.0;
            for (w, v) in rec.iter().enumerate() {
                self.crm_pub(i).set_f32(Self::target_at(0) + 4 * w, *v);
            }
        }
        if self.ci16(i, c::NTARGETS) != 0 {
            self.crm_pub(i).set_u32(c::WAYPOINT, 0);
            let p = self.crit(i).rec.i32_(Self::target_at(0)) as usize;
            self.critter_ntargets[p] = add(self.critter_ntargets[p], 1.0)?;
        }
        Ok(())
    }

    /// CritterDropItem 0x197778: the linked item (+0xb54), or for a gargoyle
    /// a new "GARG<EnemyData +0x10>" item, leaves in a bag effect.
    fn critter_drop_item(&mut self, i: usize) -> R<()> {
        let kind = self.critter_kind(i);
        let linked = self.crit(i).rec.u32_(0xb54).checked_sub(1).map(|v| v as usize);
        let item = match linked {
            Some(it) => {
                self.crm_pub(i).set_u32(0xb54, 0);
                Some(it)
            }
            None if kind == 7 => {
                let name = format!("GARG{}", self.level_enemy_desc_name(0x20)?).to_ascii_uppercase();
                match self.items.find_info_index(1, 0x10, name.as_bytes()) {
                    Some(d) => Some(self.add_item_sub(d, None)?),
                    None => None,
                }
            }
            None => None,
        };
        if let Some(it) = item {
            match kind {
                7 => self.help(0x8a, usize::MAX),
                8 => self.help(0x86, usize::MAX),
                _ => {}
            }
            self.items.records[it].set_u8(0xe1, 10);
            let at = self.crit(i).rec.v3(c::ATTN);
            self.start_bag_fx(0.0, at, it)?;
        }
        Ok(())
    }

    /// CritterGolemAI 0x198598 (golems 3, gargoyles 7, generals 8); false
    /// when the instance was deleted. EnemyShrink tree scaling and the death
    /// fade alpha are presentation.
    fn critter_golem_ai(&mut self, i: usize) -> R<bool> {
        self.critter_get_single_target_player(i)?;
        self.critter_set_difficulty(i)?;
        if self.crit(i).rec.i32_(c::STATE) == 0 {
            // MaxPlayerDist without a waypoint: the result is unused.
            self.critter_activate(i);
        }
        let prev = self.ci16(i, c::MOVE);
        let prev_type = self.critter_move_i32(i, prev.max(0) as usize, 0);
        self.critter_move_setup(i);
        self.critter_look_for_critical_move(i)?;
        let stop = self.stop_time != 0;
        if self.globals.trig_cam_active != 0 {
            if self.ci16(i, c::NEXT) < 0 {
                self.critter_look_for_ready(i)?;
                if self.ci16(i, c::NEXT) < 0 {
                    let m = self.ci16(i, c::MOVE);
                    self.set_ci16(i, c::NEXT, m);
                }
            }
        } else if stop {
            if self.ci16(i, c::NEXT) < 0 {
                let m = self.ci16(i, c::MOVE);
                self.set_ci16(i, c::NEXT, m);
            }
        } else if self.ci16(i, c::NEXT) < 0 {
            self.critter_look_for_block(i)?;
            if self.ci16(i, c::NEXT) < 0 {
                self.critter_look_for_attack(i)?;
                if self.ci16(i, c::NEXT) < 0 {
                    self.critter_look_for_translate(i)?;
                    if self.ci16(i, c::NEXT) < 0 {
                        self.critter_look_for_ready(i)?;
                        if self.ci16(i, c::NEXT) < 0 {
                            let m = self.ci16(i, c::MOVE);
                            self.set_ci16(i, c::NEXT, m);
                        }
                    }
                }
            }
        }
        let animate = if stop && 0 <= self.ci16(i, c::MOVE) {
            // The next-move record is read even when +0x126 is -1.
            let next_type = i32::from(self.critter_move_i16_signed(i, self.ci16(i, c::NEXT), 0)?);
            prev_type == 0x11 || next_type == 0x11 || prev_type == 0x10 || next_type == 0x10
        } else {
            true
        };
        if animate {
            self.critter_animate(i)?;
        }
        if self.ci16(i, c::MOVE) < 0 {
            self.set_ci16(i, c::MOVE, 0);
        }
        let mv = self.ci16(i, c::MOVE) as usize;
        let frame = self.cf(i, c::ANIM_FRAME) as i32;
        let mtype = self.critter_move_i32(i, mv, 0);
        if mtype == 0x11 {
            if self.crit(i).clock.mode_bits & 0xff != 0 {
                self.critter_drop_item(i)?;
                self.critter_del_inst(i)?;
                return Ok(false);
            }
            // MBTreeSetAlpha fade over the move's last frames: presentation.
        } else if stop {
            return Ok(true);
        }
        self.critter_do_transition(i, mv)?;
        self.critter_do_actions(i, mv, frame as i16)?;
        if !self.critter_translate(i, mv)? {
            self.critter_rotate(i, mv)?;
        }
        self.critter_look_at_player(i, Some(mv))?;
        Ok(true)
    }

    /// CritterKill 0x1976c0.
    pub(crate) fn critter_kill(&mut self, i: usize) -> R<()> {
        if self.crit(i).rec.i32_(c::STATE) == 1 {
            return Ok(());
        }
        self.crm_pub(i).set_i32(c::STATE, 1);
        let exp = mul(self.critter_type_f32(i, 0xe8), 0.2)?;
        self.critter_award_exp(exp, None)?;
        if self.crit(i).parent.is_none() {
            for k in self.critter_children(i) {
                self.crm_pub(k).set_f32(c::HEALTH, 1.0);
            }
        }
        if self.critter_kind(i) == 4 && self.crit(i).parent.is_none() {
            self.boss_is_dying = 1;
        }
        Ok(())
    }

    /// CritterAwardExp 0x19ba40.
    pub(crate) fn critter_award_exp_pub(&mut self, amount: f32, player: Option<usize>) -> R<()> {
        self.critter_award_exp(amount, player)
    }
    fn critter_award_exp(&mut self, amount: f32, player: Option<usize>) -> R<()> {
        let range = match player {
            Some(p) => p..p + 1,
            None => 0..4,
        };
        for p in range {
            if self.players[p].i32_(pd::STATE) == 1 {
                self.add_exp(p, amount as i32, 0)?;
            }
        }
        Ok(())
    }

    /// CritterDelInst 0x193180 (+ BossDeath 0x19dda8 for a boss root).
    fn critter_del_inst(&mut self, i: usize) -> R<()> {
        if self.critter_kind(i) == 4 && self.crit(i).parent.is_none() {
            self.boss_death(i)?;
        }
        if let Some(ch) = self.crit(i).child {
            self.critter_del_inst(ch)?;
        }
        self.critters[i] = None;
        if self.boss_crit_inst == i as i32 + 1 {
            // BossCritInst keeps pointing at the freed slot in the original;
            // its +4 type word reads 0 afterwards.
        }
        Ok(())
    }

    /// BossDeath 0x19dda8: StartGoodWizard, shards for active players, the
    /// boss key effect (BossType < 0x2a) and health meter hide.
    fn boss_death(&mut self, i: usize) -> R<()> {
        self.globals.boss_is_dead = 1;
        // StartGoodWizard 0x165cf0 (optmenu_disable is presentation).
        self.globals.good_wizard = 1;
        for p in 0..4 {
            let st = self.players[p].i32_(pd::STATE);
            if st == 1 || st == 8 {
                self.events.push(super::Event::Audio { cue: "PlayerGiveShard", player: Some(p), arg: self.globals.cur_world });
            }
        }
        let t = self.crit(i).ty;
        let ty = self.critter_type_rec_t(t).to_vec();
        let home = self.crit(i).rec.v3(c::HOME_POS);
        let at = [
            add(CritterFile::f32_pub(&ty, 0xd0), home[0])?,
            add(CritterFile::f32_pub(&ty, 0xd4), home[1])?,
            add(CritterFile::f32_pub(&ty, 0xd8), home[2])?,
        ];
        if self.globals.boss_type < 0x2a {
            self.events.push(super::Event::BossKey { position: at });
            self.boss_key_position = Some(at);
        }
        Ok(())
    }
}

/// GetVectorNormalXZ 0x197080.
fn get_vector_normal_xz(v: V3) -> R<(f32, f32)> {
    let d = crate::vu0::fqdist(v[0], v[2])?.0;
    if 0.0 < d {
        let k = div(1.0, d)?;
        Ok((mul(v[0], k)?, mul(v[2], k)?))
    } else {
        Ok((v[0], v[2]))
    }
}

#[allow(dead_code)]
fn _unused(m: &[[f32; 3]; 3]) -> R<V3> {
    let _ = HALF_PI;
    mul_vec_mat3([0.0; 3], m)
}

#[allow(dead_code)]
fn _damage_mul() -> f32 {
    DAMAGE_MUL[0]
}
