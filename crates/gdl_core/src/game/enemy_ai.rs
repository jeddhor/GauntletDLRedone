//! Per-frame enemy behaviour: do_enemies 0x13b510, find_closest_player
//! 0x137340, enemy_attack_control 0x13a998, enemy_knockback_control 0x13ad98,
//! enemy_ai_control 0x13b208, do_ai 0x1424f0, move_logic00 0x142100, move_logic02/04 0x141d30/0x141928, move_logic05/06/07, move_logic14/17/18/23/30
//! (0x141578/0x1411c8/0x140d70), set_enemy_trans 0x13c4b8, turn_enemy(_ang)
//! 0x13c7b8/0x13c5e0, do_enemy_move 0x143990, do_enemy_collide 0x143568,
//! EnemyCollideWalls 0x142ff8, EnemyCollideFloor 0x143120, DoEnemyItemCollide
//! 0x15a148, find_closest_item 0x154568, do_enemy_attack 0x142bf8,
//! RequestEnemyAction 0x177a08, DoEnemyAction 0x1793f8, InitActions 0x177a88,
//! CreateYPRMatrix 0x1d6660 with ffsin/ffcos 0x1d6d18/0x1d6ea0, get_yaw
//! 0x168b68, FixAngle 0x1d6328, check_vacancy 0x13c9e0, FastWallCollide
//! 0x11ed90, DeadEnd 0x13be90, blocked00 0x142438, blocked07 0x141120, EnemyWorldDamage 0x142e90,
//! move_logic01/15/19/24/27/28/31 (0x141f48/0x13efe0/0x13e4e8/0x13d8b0/0x13d4e8/
//! 0x13d310/0x13cbc0), xlate31 0x13cb10, get_target_ang 0x13c2d8,
//! set_suicide_bomber_index 0x139920.
//!
//! Visibility (MBWorldSphereVisible) is a camera query; until the camera is
//! simulated natively it is supplied per enemy by the caller (`visible`).
use super::enemies::e;
use super::{unsupported, Game, R};
use crate::animation_clock::{select, ClipTiming, ClockState, SelectionInput};
use crate::collision::Query;
use crate::item_collision::{item_coll_sub, normal_vector_2d};
use crate::libm::{atan2f, cosf, sinf};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sqrt_rn, sub_guarded_rz as sub};
use crate::player_collision::{hit_normal, slide_along_wall};
use crate::vu0::fqdist;

type V3 = [f32; 3];
const PI: f32 = f32::from_bits(0x4049_0fdb);
const TWO_PI: f32 = f32::from_bits(0x40c9_0fdb);
const HALF_PI: f32 = f32::from_bits(0x3fc9_0fdb);

/// One-step wrap used inline throughout the enemy code.
pub fn wrap(v: f32) -> R<f32> {
    Ok(if PI < v {
        sub(v, TWO_PI)?
    } else if v <= -PI {
        add(v, TWO_PI)?
    } else {
        v
    })
}

/// FixAngle: repeated wrap into (-π, π].
pub fn fix_angle(mut v: f32) -> R<f32> {
    let mut guard = 0;
    while PI < v {
        v = sub(v, TWO_PI)?;
        guard += 1;
        if guard > 64 {
            return unsupported("FixAngle range");
        }
    }
    while v <= -PI {
        v = add(v, TWO_PI)?;
        guard += 1;
        if guard > 128 {
            return unsupported("FixAngle range");
        }
    }
    Ok(v)
}

/// get_yaw(a, b): heading of a seen from b.
pub fn get_yaw(a: V3, b: V3) -> R<f32> {
    let dx = sub(a[0], b[0])?;
    let dz = sub(a[2], b[2])?;
    let mut az = dz.abs();
    if az <= 0.001 {
        az = 0.001;
    }
    let mut t = atan2f(dx.abs(), az)?;
    if 0.0 <= dz {
        if dx < 0.0 {
            t = -t;
        }
    } else if 0.0 <= dx {
        t = sub(PI, t)?;
    } else {
        t = add(t, PI)?;
    }
    fix_angle(t)
}

/// SIN_POLY 0x1d6cb8.
fn sin_poly(x: f32) -> R<f32> {
    let x2 = mul(x, x)?;
    let mut p = add(mul(x2, f32::from_bits(0x3638_ef1d))?, f32::from_bits(0xb950_0d01))?;
    p = add(mul(x2, p)?, f32::from_bits(0x3c08_8889))?;
    p = add(mul(x2, p)?, f32::from_bits(0xbe2a_aaab))?;
    let q = mul(x2, p)?;
    Ok(add(x, mul(x, q)?)?)
}

/// ffsin: range-reduced polynomial sine (non-finite input reports and returns 0).
pub fn ffsin(x: f32) -> R<f32> {
    if (x.to_bits() >> 23) & 0xff == 0xff {
        return Ok(0.0);
    }
    let mut v = x;
    let mut guard = 0;
    while v < 0.0 {
        v = add(v, TWO_PI)?;
        guard += 1;
        if guard > 64 {
            return unsupported("ffsin range");
        }
    }
    while TWO_PI <= v {
        v = sub(v, TWO_PI)?;
        guard += 1;
        if guard > 128 {
            return unsupported("ffsin range");
        }
    }
    if v < HALF_PI {
        sin_poly(v)
    } else if v < PI {
        sin_poly(sub(PI, v)?)
    } else if v < f32::from_bits(0x4096_cbe4) {
        Ok(-sin_poly(sub(v, PI)?)?)
    } else {
        Ok(-sin_poly(sub(TWO_PI, v)?)?)
    }
}

pub fn ffcos(x: f32) -> R<f32> {
    ffsin(add(x, HALF_PI)?)
}

/// CreateYPRMatrix(angles at +0, +4, +8): 4×4 with zero w column and w = 1.
pub fn create_ypr_matrix(a: V3) -> R<[[f32; 4]; 4]> {
    let c0 = ffcos(a[0])?;
    let s0 = -ffsin(a[0])?;
    let c1 = ffcos(a[1])?;
    let s1 = -ffsin(a[1])?;
    let c2 = ffcos(a[2])?;
    let s2 = -ffsin(a[2])?;
    let s1s0 = mul(s1, s0)?;
    let m00 = sub(mul(c1, c2)?, mul(s1s0, s2)?)?;
    let m10 = sub(mul(-c1, s2)?, mul(s1s0, c2)?)?;
    let m20 = mul(-s1, c0)?;
    let c1s0 = mul(c1, s0)?;
    let m02 = add(mul(s1, c2)?, mul(c1s0, s2)?)?;
    let m12 = add(mul(-s1, s2)?, mul(c1s0, c2)?)?;
    let m22 = mul(c1, c0)?;
    Ok([
        [m00, mul(c0, s2)?, m02, 0.0],
        [m10, mul(c0, c2)?, m12, 0.0],
        [m20, -s0, m22, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

/// Native enemy animation state (atree clock and the selected tree's clips).
#[derive(Clone, Debug)]
pub struct EnemyAnim {
    pub clock: ClockState,
    pub clips: Vec<ClipTiming>,
    pub current: u16,
    /// (type, level) of the attached tree, for presentation lookup.
    pub tree: (i32, i32),
}

/// Clip timings by sequence index and the InitActions table.
pub type EnemyTree = (Vec<ClipTiming>, Vec<(i32, i32)>);

/// Per-type, per-level clip data and InitActions tables supplied by the loader.
#[derive(Clone, Debug, Default)]
pub struct EnemyAnimData {
    /// (type, level) → (clip timings by sequence index, action table 33 × (index, +0x20)).
    pub trees: std::collections::HashMap<(i32, i32), EnemyTree>,
    /// (type, tree label) of other trees in the type's package, e.g.
    /// "ZOM_ARROW" (InitEnemyMissiles matches "<PREFIX>_<desc>").
    pub other_trees: std::collections::HashMap<(i32, String), (i16, i16, u16)>,
    /// (type, tree label) → the tree's index in the type's package.
    pub tree_index: std::collections::HashMap<(i32, String), usize>,
    /// (type, level) → the tree's node count (AtreeInitSub: atree +0x3c,
    /// the record's +0xbc).
    pub tree_nodes: std::collections::HashMap<(i32, i32), i32>,
    /// (type, level) trees with a type-1 node: AnimateATree's DoAnimation
    /// turns a nonzero reset byte into 2 (OANIM-only trees never do).
    pub tree_animated: std::collections::HashSet<(i32, i32)>,
    /// (type, label) → clips, a type-1 node, node count of the package's
    /// statue trees (SetItem's AtreeMatch: GOL_STATUE in enemy_anim[0x1d],
    /// DEATHSTATUE1/2 in enemy_anim[0x1e]).
    pub statue_trees: std::collections::HashMap<(i32, String), (Vec<crate::animation_clock::ClipTiming>, bool, i32)>,
}

/// InitActions: action names (EE 0x23f8c8 list) → sequence index and +0x20 word.
pub const ACTION_NAMES: [&str; 33] = [
    "READY", "START", "TAUNT", "WALK", "RUN", "FLY", "HOVER", "LANDING", "WALKTOREADY", "READYTOWALK", "WALKTOREADY", "READYTOWALK",
    "ATTACK1", "ATTACK1R", "ATTACK2", "ATTACK2R", "ATTACK3", "ATTACK3R", "ATTACK4", "ATTACK4R", "ATTACK5", "ATTACK5R", "RUNATTACK1",
    "RUNATTACK2", "THROW1", "THROW2", "THROWF", "ATTTOREADY", "HIT1", "HIT2", "HIT3", "GETUP", "DEATH",
];

pub fn init_actions(labels: &[(String, i16)]) -> Vec<(i32, i32)> {
    ACTION_NAMES
        .iter()
        .map(|name| match labels.iter().position(|(l, _)| l == name) {
            Some(i) => (i as i32, i32::from(labels[i].1)),
            None => (-1, 0),
        })
        .collect()
}

/// e_actpri EE 0x23f950 (33 priorities), read from the ELF by the loader.
pub type ActionPriority = Vec<i32>;

impl Game {
    fn ef(&self, i: usize, o: usize) -> f32 {
        self.enemies[i].f32_(o)
    }

    /// RequestEnemyAction(enemy, action).
    pub fn request_enemy_action(&mut self, i: usize, a: u32) {
        let rec = &self.enemies[i];
        let busy = 0.0 < rec.f32_(0x390);
        if busy && (a.wrapping_sub(0xc) <= 8 || a.wrapping_sub(0x18) <= 2) {
            return;
        }
        let pri = |k: u32| self.enemy_action_priority.get(k as usize).copied().unwrap_or(0);
        if pri(rec.u32_(0xe4)) < pri(a) {
            self.enemies[i].set_u32(0xe4, a);
        }
    }

    /// find_closest_player(enemy).
    pub fn find_closest_player(&mut self, i: usize) -> R<()> {
        let it = self.globals.it;
        let any_active = self.players.iter().any(|p| p.i32_(0xfc) == 1);
        if !any_active {
            self.enemies[i].set_i16(0x2f2, 0);
        }
        let t = self.enemies[i].i32_(e::TYPE);
        let col = self.enemies[i].v3(e::COLLISION);
        let dist3 = |p: V3| -> R<f32> {
            let d = [sub(col[0], p[0])?, sub(col[1], p[1])?, sub(col[2], p[2])?];
            Ok(sqrt_rn(add(add(mul(d[0], d[0])?, mul(d[1], d[1])?)?, mul(d[2], d[2])?)?)?)
        };
        let it_ok = 0 <= it
            && self.players[it as usize].i32_(0xfc) == 1
            && self.players[it as usize].u32_(0x138) & 4 == 0
            && !(t == 0x1e && self.players[it as usize].u32_(0x134) & 0x8_0000 != 0);
        if it_ok {
            let target = self.enemies[i].i16_(0x288);
            self.enemies[i].set_i16(0x28a, target);
            self.enemies[i].set_i16(0x288, it as i16);
            let d = dist3(self.players[it as usize].v3(0x70))?;
            let crowd = self.players[it as usize].f32_(0x944);
            self.enemies[i].set_f32(0x290, d);
            self.enemies[i].set_f32(0x28c, add(d, crowd)?);
        } else {
            let target = self.enemies[i].i16_(0x288);
            let stagger = (self.frame_main & 7) != (i as i32 & 7);
            let mut rescan = if stagger && 0 <= target { 0 } else { 1 };
            if 0 <= target && self.players[target as usize].i32_(0xfc) != 1 {
                rescan = -1;
            }
            if rescan != 0 {
                let rec = &mut self.enemies[i];
                rec.set_i16(0x28a, target);
                rec.set_i16(0x288, -1);
                rec.set_u32(0x28c, 0x47c3_5000);
                rec.set_u32(0x290, 0x47c3_5000);
                if t == 0x1e {
                    rec.set_i32(0x33c, -1);
                }
                let mut best_halo = 100_000.0;
                for k in 0..4 {
                    let pl = &self.players[k];
                    if pl.i32_(0xfc) != 1 || pl.u32_(0x138) & 4 != 0 {
                        continue;
                    }
                    let d = dist3(pl.v3(0x70))?;
                    if self.ef(i, 0x314) < d {
                        continue;
                    }
                    if t == 0x1e && pl.u32_(0x134) & 0x8_0000 != 0 {
                        if d < best_halo {
                            best_halo = d;
                            self.enemies[i].set_i32(0x33c, k as i32);
                        }
                        continue;
                    }
                    let weighted = if mul(self.ef(i, e::WIDTH), 5.0)? < d { add(d, pl.f32_(0x944))? } else { d };
                    if weighted < self.ef(i, 0x28c) {
                        let cone = self.ef(i, 0x310);
                        let in_view = PI <= cone || {
                            let y = get_yaw(pl.v3(0x70), col)?;
                            sub(y, self.ef(i, 0x258))?.abs() <= cone
                        };
                        if in_view {
                            let rec = &mut self.enemies[i];
                            rec.set_f32(0x28c, weighted);
                            rec.set_f32(0x290, d);
                            rec.set_i16(0x288, k as i16);
                        }
                    }
                }
            }
        }
        let target = self.enemies[i].i16_(0x288);
        if target < 0 {
            self.enemies[i].set_u32(0x290, 0x47c3_5000);
            self.enemies[i].set_u32(0x28c, 0x47c3_5000);
        } else if self.ef(i, 0x290) <= self.ef(i, 0x314) {
            self.enemies[i].set_i16(0x2f2, 1);
            let pl = &mut self.players[target as usize];
            pl.set_i32(0x940, pl.i32_(0x940) + 1);
            pl.set_f32(0x944, add(pl.f32_(0x944), 2.0)?);
        }
        Ok(())
    }

    /// FastWallCollide(from, to, exclusion): CheckFlags 0x20 line test.
    pub fn fast_wall_collide(&mut self, from: V3, to: V3, exclusion: i32) -> R<Option<V3>> {
        let (c, _) = self.collision.world_collide(Query {
            radius: f32::from_bits(0x3dcc_cccd),
            first: from,
            last: to,
            check_flags: 0x20,
            min_normal_y: f32::from_bits(0xbf5d_b22d),
            max_normal_y: f32::from_bits(0x3f5d_b22d),
            object_mask: 0x13a,
            exclusion_mask: exclusion,
        })?;
        Ok(c.solid.map(|h| h.position))
    }

    /// check_vacancy(position, enemy): -1 free, 0 occupied.
    fn check_vacancy(&mut self, p: V3, i: usize) -> R<i32> {
        let r = add(self.ef(i, e::WIDTH), f32::from_bits(0x3dcc_cccd))?;
        let h = add(self.ef(i, e::HALF_HEIGHT), f32::from_bits(0x3dcc_cccd))?;
        let c = self.enemies[i].v3(e::COLLISION);
        let from = [c[0], p[1], c[2]];
        if 0 <= self.enemy_collide_enemy(r, h, i, from, p, false)? {
            return Ok(0);
        }
        // ItemCollide returns the first hit in grid order, so a non-blocking
        // item ahead of a blocking one leaves the step vacant.
        let item = self.item_collide(r, from, p, None, false)?;
        let blocks = match item {
            Some(item) => self.enemy_item_collision(None, item)?,
            None => false,
        };
        if blocks {
            return Ok(0);
        }
        if self.fast_wall_collide(from, p, 2)?.is_some() {
            return Ok(0);
        }
        Ok(-1)
    }

    /// DeadEnd(enemy, frames).
    fn dead_end(&mut self, i: usize, t: i32) {
        let rec = &mut self.enemies[i];
        if rec.i32_(0x36c) < 1 {
            rec.set_i32(0x36c, t);
            if 0x3b < t && rec.i32_(0xe4).wrapping_sub(3) as u32 <= 1 {
                rec.set_i32(0xe4, 0);
            }
        }
    }

    fn blocked07(&mut self, i: usize, short: i32, long: i32) {
        let dir = self.enemies[i].i32_(0x368).abs();
        if dir < 3 {
            let c = self.enemies[i].i16_(0x378) + 1;
            self.enemies[i].set_i16(0x378, c);
            self.dead_end(i, short);
        } else {
            self.dead_end(i, long);
            let f = self.faceang;
            let rec = &mut self.enemies[i];
            rec.set_f32(0x260, f);
            rec.set_f32(0x258, f);
            rec.set_i16(0x378, 0);
            rec.set_i32(0x368, 0);
        }
        if 6 < self.enemies[i].i16_(0x378) {
            let rec = &mut self.enemies[i];
            rec.set_i32(0x368, rec.i32_(0x368).wrapping_mul(-2));
            rec.set_i16(0x378, 0);
        }
    }

    /// blocked00 0x142438: count dead ends; after nine, reverse and double
    /// the turn direction, facing the target when it exceeds 2.
    fn blocked00(&mut self, i: usize) {
        let c = self.enemies[i].i16_(0x378) + 1;
        self.enemies[i].set_i16(0x378, c);
        self.dead_end(i, if self.enemies[i].i32_(0x368).abs() < 3 { 5 } else { 0x3c });
        if 8 < self.enemies[i].i16_(0x378) {
            let old = self.enemies[i].i32_(0x368);
            let new = old.wrapping_mul(-2);
            self.enemies[i].set_i32(0x368, new);
            self.enemies[i].set_i16(0x378, 0);
            if 2 < new.abs() {
                let f = self.faceang;
                self.enemies[i].set_f32(0x260, f);
                self.enemies[i].set_f32(0x258, f);
            }
        }
    }

    fn blocked(&mut self, i: usize, logic: i16, short: i32, long: i32) -> R<()> {
        match logic {
            7 => self.blocked07(i, short, long),
            0 => self.blocked00(i),
            8 | 10 | 0x14 => return unsupported(format!("blocked handler for logic {logic}")),
            _ => self.dead_end(i, 0x14),
        }
        Ok(())
    }

    /// set_enemy_trans(speed scale, heading, enemy).
    fn set_enemy_trans(&mut self, scale: f32, heading: f32, i: usize) -> R<()> {
        let t = self.enemies[i].i32_(e::TYPE);
        if t == self.globals.boss_type || self.enemies[i].i32_(0xe0) == 1 {
            return Ok(());
        }
        self.request_enemy_action(i, if 1.25 <= scale { 4 } else { 3 });
        let a = self.enemies[i].i32_(0xe0);
        if !((a as u32).wrapping_sub(3) < 2 || a == 0x16 || a == 0x17 || 0 <= self.enemies[i].i32_(0x298)) {
            return Ok(());
        }
        if self.ef(i, 0x320) != heading {
            let (s, c) = (sinf(heading)?, cosf(heading)?);
            let rec = &mut self.enemies[i];
            rec.set_f32(0x318, s);
            rec.set_f32(0x31c, c);
            rec.set_f32(0x320, heading);
        }
        let fw = self.fwrdval[t as usize];
        let rec = &mut self.enemies[i];
        rec.set_f32(0x224, add(mul(mul(rec.f32_(0x318), fw)?, scale)?, rec.f32_(0x224))?);
        rec.set_f32(0x22c, add(mul(mul(rec.f32_(0x31c), fw)?, scale)?, rec.f32_(0x22c))?);
        Ok(())
    }

    /// turn_enemy_ang(target heading, enemy).
    fn turn_enemy_ang(&self, target: f32, i: usize) -> R<f32> {
        let rec = &self.enemies[i];
        let a = rec.i32_(0xe0);
        let yaw = rec.f32_(0x258);
        if 0x1b < a || a == 1 {
            return Ok(yaw);
        }
        let mut step = self.enemy_tables.turn[rec.i32_(e::TYPE) as usize];
        if a == 4 || a == 0x1c || a == 0x1d {
            step = mul(step, 3.0)?;
        }
        let d = wrap(sub(target, yaw)?)?;
        step = mul(step, self.globals.vb_elapsed as f32)?;
        let (beyond, positive) = if 0.0 <= d { (step < d, true) } else { (d < -step, false) };
        let out = if !beyond {
            target
        } else if positive {
            add(yaw, step)?
        } else {
            sub(yaw, step)?
        };
        wrap(out)
    }

    /// get_face_ang(enemy, toward target).
    fn get_face_ang(&self, i: usize, toward: bool) -> R<f32> {
        let rec = &self.enemies[i];
        let t = rec.i16_(0x288);
        if t < 0 || !toward {
            return Ok(rec.f32_(0x260));
        }
        get_yaw(self.players[t as usize].v3(0x50), rec.v3(e::POSITION))
    }

    /// get_face_ang2 0x13c3a0: toward the +0x33c player (flee logic), else
    /// the stored heading.
    fn get_face_ang2(&self, i: usize, toward: bool) -> R<f32> {
        let rec = &self.enemies[i];
        let t = rec.i32_(0x33c);
        if t < 0 || !toward {
            return Ok(rec.f32_(0x260));
        }
        get_yaw(self.players[t as usize].v3(0x50), rec.v3(e::POSITION))
    }

    /// set_turn_dir: 1 or -1, whichever side of ±30° is nearer the target.
    fn set_turn_dir(&self, i: usize) -> R<i32> {
        let rec = &self.enemies[i];
        let p = self.players[rec.i16_(0x288).max(0) as usize].v3(0x50);
        let dx = sub(rec.f32_(0x40), p[0])?;
        let dz = sub(rec.f32_(0x48), p[2])?;
        let k = f32::from_bits(0x3f06_0a92);
        let a = wrap(add(rec.f32_(0x258), k)?)?;
        let ax = add(sinf(a)?, dx)?;
        let az = add(cosf(a)?, dz)?;
        let b = wrap(sub(rec.f32_(0x258), k)?)?;
        let bx = add(sinf(b)?, dx)?;
        let bz = add(cosf(b)?, dz)?;
        let db = add(mul(bx, bx)?, mul(bz, bz)?)?;
        let da = add(mul(ax, ax)?, mul(az, az)?)?;
        Ok(if db <= da { -1 } else { 1 })
    }

    /// set_suicide_bomber_index 0x139920: the first live logic-0x12 enemy
    /// near the camera (+0x2f0) running or about to run (action 4 at +0xe0
    /// or +0xe4); unchanged when there is none.
    fn set_suicide_bomber_index(&mut self) {
        if let Some(k) = self.enemies.iter().position(|r| {
            r.i32_(e::STATE) == 1 && r.i16_(e::LOGIC) == 0x12 && r.i16_(0x2f0) != 0 && (r.i32_(0xe0) == 4 || r.i32_(0xe4) == 4)
        }) {
            self.suicide_bomber_index = k as i32;
        }
    }

    fn found_suicide_bomber(&self, i: usize) -> R<bool> {
        let s = self.suicide_bomber_index;
        if s < 0 {
            return Ok(false);
        }
        let b = &self.enemies[s as usize];
        if b.i32_(e::STATE) != 1 || self.ef(i, 0x314) < b.f32_(0x290) {
            return Ok(false);
        }
        let rec = &self.enemies[i];
        if i as i32 == s || rec.i16_(0x2ec) != 0 || 0 < rec.i32_(0x36c) {
            return Ok(false);
        }
        let d = [sub(b.f32_(0x40), rec.f32_(0x40))?, sub(b.f32_(0x44), rec.f32_(0x44))?, sub(b.f32_(0x48), rec.f32_(0x48))?];
        let q = add(add(mul(d[0], d[0])?, mul(d[1], d[1])?)?, mul(d[2], d[2])?)?;
        Ok(q < 100.0)
    }

    /// move_logic07: chase the target, sidestepping blocked headings.
    fn move_logic07(&mut self, i: usize) -> R<()> {
        let t = self.enemies[i].i32_(e::TYPE);
        let fw = self.fwrdval[t as usize];
        if self.found_suicide_bomber(i)? {
            self.enemies[i].set_i16(e::LOGIC, 0x18);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(0x2f2) == 0 || self.enemies[i].i16_(0x288) < 0 {
            self.enemies[i].set_i16(e::LOGIC, ((i & 1) + 5) as i16);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        self.faceang = self.get_face_ang(i, true)?;
        let mut heading;
        let mut stuck = false;
        let dead = self.enemies[i].i32_(0x36c);
        let skip_choice = 0 < dead && {
            let v = dead - self.globals.vb_elapsed;
            self.enemies[i].set_i32(0x36c, v);
            0 < v
        };
        if skip_choice {
            heading = self.ef(i, 0x260);
        } else {
            heading = self.faceang;
            if self.enemies[i].i32_(0x298) < 0 {
                let (dir, k) = if self.enemies[i].i16_(0x212) == 1 {
                    let mut d = self.enemies[i].i32_(0x368);
                    if d == 0 {
                        d = self.set_turn_dir(i)?;
                        self.enemies[i].set_i32(0x368, d);
                    }
                    (Some(d), self.enemies[i].i16_(0x378))
                } else if self.enemies[i].u32_(0x2a0) != 0 || 0 <= self.enemies[i].i32_(0x29c) {
                    heading = self.ef(i, 0x260);
                    (Some(self.enemies[i].i32_(0x368)), self.enemies[i].i16_(0x378))
                } else {
                    (None, 0)
                };
                if let Some(d) = dir {
                    let off = *self.angoff07.get(k.max(0) as usize).ok_or_else(|| super::Unsupported("angoff07 index".into()))?;
                    heading = if d < 1 { sub(heading, off)? } else { add(heading, off)? };
                }
            }
            heading = wrap(heading)?;
            let rec = &self.enemies[i];
            let mut probe = [rec.f32_(0x40), add(add(rec.f32_(e::WIDTH), f32::from_bits(0x3dcc_cccd))?, rec.f32_(0x44))?, rec.f32_(0x48)];
            probe[0] = add(mul(sinf(heading)?, fw)?, probe[0])?;
            probe[2] = add(mul(cosf(heading)?, fw)?, probe[2])?;
            let k = f32::from_bits(0x3d0e_fa35);
            let prev = self.ef(i, 0x264);
            let cur = self.ef(i, 0x260);
            if (k < sub(cur, prev)?.abs() && sub(heading, prev)?.abs() <= k) || self.check_vacancy(probe, i)? == 0 {
                stuck = true;
                let c = self.enemies[i].i16_(0x37a) + 1;
                self.enemies[i].set_i16(0x37a, c);
            } else {
                self.enemies[i].set_i16(0x37a, 0);
            }
            if 10 < self.enemies[i].i16_(0x37a) {
                self.enemies[i].set_f32(0x260, self.faceang);
                heading = self.faceang;
            }
            if !stuck {
                let cur = self.enemies[i].u32_(0x260);
                self.enemies[i].set_u32(0x264, cur);
                self.enemies[i].set_f32(0x260, heading);
            }
        }
        self.set_enemy_trans(1.0, heading, i)?;
        if !stuck || heading == self.faceang {
            let y = self.turn_enemy_ang(heading, i)?;
            self.enemies[i].set_f32(0x258, y);
        }
        self.do_enemy_move(i)?;
        if t == 3 && self.enemies[i].i32_(0xe4) == 0 {
            self.request_enemy_action(i, 3);
        }
        Ok(())
    }

    /// move_logic05 (turn −90°) and move_logic06 (+90°): wander along walls.
    fn move_logic_wander(&mut self, i: usize, quarter: f32) -> R<()> {
        let r = self.ef(i, e::WIDTH);
        let t = self.enemies[i].i32_(e::TYPE);
        let fw = self.fwrdval[t as usize];
        if self.found_suicide_bomber(i)? {
            self.enemies[i].set_i16(e::LOGIC, 0x18);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let turn = |g: &mut Game| -> R<()> {
            let h = wrap(add(g.ef(i, 0x260), quarter)?)?;
            g.enemies[i].set_f32(0x260, h);
            Ok(())
        };
        let dead = self.enemies[i].i32_(0x36c);
        if 0 < dead {
            let v = dead - self.globals.vb_elapsed;
            self.enemies[i].set_i32(0x36c, v);
            if v < 1 {
                turn(self)?;
                let c = self.enemies[i].i32_(0x370) + 1;
                self.enemies[i].set_i32(0x370, if 3 < c { 0 } else { c });
            }
        }
        let rec = &self.enemies[i];
        let base = [rec.f32_(0x40), add(add(r, f32::from_bits(0x3dcc_cccd))?, rec.f32_(0x44))?, rec.f32_(0x48)];
        let h = rec.f32_(0x260);
        let reach = add(r, 0.5)?;
        let ahead = [add(mul(sinf(h)?, reach)?, base[0])?, base[1], add(mul(cosf(h)?, reach)?, base[2])?];
        if self.fast_wall_collide(base, ahead, 2)?.is_none() {
            let step = [add(mul(sinf(h)?, fw)?, base[0])?, base[1], add(mul(cosf(h)?, fw)?, base[2])?];
            if self.check_vacancy(step, i)? == 0 {
                turn(self)?;
                self.dead_end(i, 0x14);
            }
        } else {
            turn(self)?;
            self.dead_end(i, 0x14);
        }
        let h = self.ef(i, 0x260);
        self.set_enemy_trans(1.0, h, i)?;
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)
    }

    /// move_logic00: chase the target, probing up to nine angoff00 offsets
    /// (turn direction +0x368) for a vacant step each brain period.
    fn move_logic00(&mut self, i: usize) -> R<()> {
        let t = self.enemies[i].i32_(e::TYPE);
        let fw = self.fwrdval[t as usize];
        if self.found_suicide_bomber(i)? {
            self.enemies[i].set_i16(e::LOGIC, 0x18);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(0x2f2) == 0 || self.enemies[i].i16_(0x288) < 0 {
            self.enemies[i].set_i16(e::LOGIC, ((i & 1) + 5) as i16);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let timer = self.enemies[i].i32_(0x36c);
        let choose = timer < 1 || {
            let left = timer - self.globals.vb_elapsed;
            self.enemies[i].set_i32(0x36c, left);
            left < 1
        };
        if choose {
            let mut x = self.get_face_ang(i, true)?;
            self.faceang = x;
            let k = f32::from_bits(0x3d0e_fa36);
            let mut n = 0usize;
            loop {
                let off = *self.angoff00.get(n).ok_or_else(|| super::Unsupported("angoff00 index".into()))?;
                x = if self.enemies[i].i32_(0x368) < 1 { sub(x, off)? } else { add(x, off)? };
                x = wrap(x)?;
                let rec = &self.enemies[i];
                let mut probe = [rec.f32_(0x40), add(add(rec.f32_(e::WIDTH), f32::from_bits(0x3dcc_cccd))?, rec.f32_(0x44))?, rec.f32_(0x48)];
                probe[0] = add(mul(sinf(x)?, fw)?, probe[0])?;
                probe[2] = add(mul(cosf(x)?, fw)?, probe[2])?;
                let cur = self.ef(i, 0x260);
                let prev = self.ef(i, 0x264);
                let d = wrap(sub(x, cur)?)?;
                let settled = sub(cur, prev)?.abs() <= k || k < sub(x, prev)?.abs();
                if settled && d.abs() < f32::from_bits(0x4046_d3f2) && self.check_vacancy(probe, i)? != 0 {
                    break;
                }
                n += 1;
                if 8 < n {
                    break;
                }
            }
            let chosen = if n < 9 {
                self.enemies[i].set_i16(0x378, n as i16);
                x
            } else {
                self.faceang
            };
            let cur = self.enemies[i].u32_(0x260);
            self.enemies[i].set_u32(0x264, cur);
            self.enemies[i].set_f32(0x260, chosen);
        }
        let h = self.ef(i, 0x260);
        self.set_enemy_trans(1.0, h, i)?;
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)
    }

    /// move_logic18: suicide bomber — wait until close (stage 0), wind up
    /// (stage 1, action 9 → 4), then run weaving at 1.5× and explode on
    /// reaching a player or after 240 vblanks.
    fn move_logic18(&mut self, i: usize) -> R<()> {
        let t0 = self.enemies[i].i32_(0x36c);
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let f = self.get_face_ang(i, true)?;
        self.enemies[i].set_f32(0x260, f);
        let vb = self.globals.vb_elapsed;
        let stage = self.enemies[i].i16_(0x32a);
        let mut run = stage == 2;
        if stage == 1 {
            if 0 <= self.enemies[i].i16_(0x288) && self.enemies[i].i32_(0xe0) != 4 {
                let c = self.enemies[i].i32_(0x330) - vb;
                self.enemies[i].set_i32(0x330, c);
                if c < 1 {
                    self.request_enemy_action(i, 9);
                }
            }
            if self.enemies[i].i32_(0xe0) == 4 {
                self.enemies[i].set_i16(0x32a, stage + 1);
                let pos = self.enemies[i].v3(e::ATTENTION);
                self.sfx3(0x37, pos, 0xe0, 0x34)?;
                run = true;
            }
        } else if stage != 2 && 0 <= self.enemies[i].i16_(0x288) && self.ef(i, 0x290) <= self.ef(i, 0x314) {
            self.enemies[i].set_i16(0x32a, stage + 1);
            self.enemies[i].set_i32(0x330, 0x3c);
            self.enemies[i].set_i32(0x334, 0);
        }
        if run {
            let c = self.enemies[i].i32_(0x334) + vb;
            self.enemies[i].set_i32(0x334, c);
            if self.enemies[i].i16_(0x2f2) == 0 || self.enemies[i].i16_(0x288) < 0 {
                self.enemies[i].set_i16(e::LOGIC, ((i & 1) + 5) as i16);
                return self.do_ai(i);
            }
            let mut timer = self.enemies[i].i32_(0x36c);
            let mut skip_trans = false;
            if self.enemies[i].i16_(0x212) == 2 {
                if 0 < timer {
                    timer -= vb;
                    self.enemies[i].set_i32(0x36c, timer);
                } else {
                    timer = 0;
                    self.enemies[i].set_i32(0x36c, 0);
                }
            }
            if timer < 1 {
                self.enemies[i].set_i32(0x36c, 0);
            } else {
                let k = self.enemies[i].i32_(0x338);
                let mut off = 0.0;
                if k < 0x10 {
                    off = *self.offset_step5.get(k as usize).ok_or_else(|| super::Unsupported("offset_step5 index".into()))?;
                    self.enemies[i].set_i32(0x338, k + 1);
                    self.enemies[i].set_i32(0x36c, 0);
                }
                let h = wrap(add(off, self.ef(i, 0x260))?)?;
                self.enemies[i].set_f32(0x260, h);
                if 0 < self.enemies[i].i32_(0x36c) && sub(h, self.ef(i, 0x268))?.abs() < f32::from_bits(0x3dd6_7750) {
                    skip_trans = true;
                } else {
                    self.enemies[i].set_i32(0x36c, 0);
                }
            }
            if !skip_trans {
                let h = self.ef(i, 0x260);
                self.set_enemy_trans(1.5, h, i)?;
            }
        }
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)?;
        if self.enemies[i].i16_(0x294) != 0 {
            self.enemies[i].set_i32(0x36c, 0);
        }
        if 240.0 <= self.enemies[i].i32_(0x334) as f32 || 0 <= self.enemies[i].i32_(0x298) {
            self.suicide_bomber_index = -1;
            self.damage_enemy(f32::from_bits(0x4479_c000), i, -2, 1, None, None, 0)?;
        } else if t0 < 1 && 0 < self.enemies[i].i32_(0x36c) {
            let h = self.enemies[i].u32_(0x260);
            self.enemies[i].set_u32(0x268, h);
            self.enemies[i].set_i32(0x338, 0);
        }
        Ok(())
    }

    /// move_logic14 0x13f320: strafe around the target, re-aiming at it
    /// with a widening offset (π/4 + n·π/12) every few turns.
    fn move_logic14(&mut self, i: usize) -> R<()> {
        if self.found_suicide_bomber(i)? {
            self.enemies[i].set_i16(e::LOGIC, 0x18);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(0x2f2) == 0 || self.enemies[i].i16_(0x288) < 0 {
            self.enemies[i].set_i16(e::LOGIC, ((i & 1) + 5) as i16);
            return self.do_ai(i);
        }
        if self.ef(i, 0x28c) <= 8.0 {
            self.enemies[i].set_i16(e::LOGIC, 0);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        self.faceang = self.get_face_ang(i, true)?;
        let vb = self.globals.vb_elapsed;
        let t = self.enemies[i].i32_(0x370) - vb;
        self.enemies[i].set_i32(0x370, t);
        if t < 1 {
            let dir = -self.enemies[i].i32_(0x330);
            self.enemies[i].set_i32(0x330, dir);
            let h = self.ef(i, 0x260);
            let h = if dir < 1 { sub(h, HALF_PI)? } else { add(h, HALF_PI)? };
            self.enemies[i].set_f32(0x260, wrap(h)?);
            let t = self.enemies[i].i32_(0x370) + 0x2d;
            self.enemies[i].set_i32(0x370, t);
            let c = self.enemies[i].i16_(0x32a) + 1;
            self.enemies[i].set_i16(0x32a, c);
        }
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        let h = self.ef(i, 0x260);
        self.set_enemy_trans(1.0, h, i)?;
        self.do_enemy_move(i)?;
        let diff = wrap(sub(self.faceang, self.ef(i, 0x260))?)?;
        let reaim = self.enemies[i].i32_(0x338) < 1
            && (0 < self.enemies[i].i32_(0x36c) || (4 <= self.enemies[i].i16_(0x32a) && HALF_PI < diff.abs()));
        if reaim {
            let n = self.enemies[i].i32_(0x334) as f32;
            let off = add(mul(n, f32::from_bits(0x3e86_0a92))?, f32::from_bits(0x3f49_0fdb))?;
            if 3 < self.enemies[i].i16_(0x32a) {
                let d = -self.enemies[i].i32_(0x330);
                self.enemies[i].set_i32(0x330, d);
            }
            self.enemies[i].set_i32(0x36c, 0);
            let p = self.enemies[i].i16_(0x288) as usize;
            let yaw = get_yaw(self.players[p].v3(0x50), self.enemies[i].v3(0x40))?;
            self.enemies[i].set_f32(0x260, yaw);
            let h = if self.enemies[i].i32_(0x330) < 1 { sub(yaw, off)? } else { add(off, yaw)? };
            self.enemies[i].set_f32(0x260, wrap(h)?);
            self.enemies[i].set_i32(0x370, 0);
            self.enemies[i].set_i16(0x32a, 0);
            let c = self.enemies[i].i32_(0x334) + 1;
            self.enemies[i].set_i32(0x334, c);
            self.enemies[i].set_i32(0x338, 0x1e);
        } else {
            let c = self.enemies[i].i32_(0x334);
            if 0 < c {
                self.enemies[i].set_i32(0x334, c - 1);
            }
            let t = self.enemies[i].i32_(0x338) - vb;
            self.enemies[i].set_i32(0x338, t);
        }
        Ok(())
    }

    /// move_logic23 0x13da78 (logics 0x11 and 0x17): stationary, face the
    /// target and, within range and ±10 height, count the +0x334 cooldown
    /// down by vb_elapsed, then request the ranged attack (action 0x18).
    /// (move_logic17 0x13eba8, the same without the cooldown, and
    /// move_logic26 0x13d598 have no caller in this build.)
    fn move_logic23(&mut self, i: usize) -> R<()> {
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let f = self.get_face_ang(i, true)?;
        self.enemies[i].set_f32(0x260, f);
        if let Ok(p) = usize::try_from(self.enemies[i].i16_(0x288)) {
            let dy = sub(self.ef(i, 0x44), self.players[p].f32_(0x54))?;
            if self.enemies[i].i16_(0x2f0) != 0 && self.ef(i, 0x290) <= self.ef(i, 0x314) && (-10.0..=10.0).contains(&dy) {
                let c = self.enemies[i].i32_(0x334);
                if 0 < c {
                    self.enemies[i].set_i32(0x334, c - self.globals.vb_elapsed);
                } else {
                    self.request_enemy_action(i, 0x18);
                }
            }
        }
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)
    }

    /// move_logic30 0x13cd40: close in with move_logic00, attacking (action
    /// 0xc, or 0x10 at level 2+) on a randomised period.
    fn move_logic30(&mut self, i: usize) -> R<()> {
        if self.found_suicide_bomber(i)? {
            self.enemies[i].set_i16(e::LOGIC, 0x18);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(0x2f2) == 0 || self.enemies[i].i16_(0x288) < 0 {
            self.enemies[i].set_i16(e::LOGIC, ((i & 1) + 5) as i16);
            return self.do_ai(i);
        }
        if self.ef(i, 0x290) <= 6.0 {
            self.enemies[i].set_i16(e::LOGIC, 7);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let mut attack = true;
        if self.enemies[i].i32_(0x36c) < 1 {
            let t = self.enemies[i].i32_(0x338) - self.globals.vb_elapsed;
            self.enemies[i].set_i32(0x338, t);
            if t < 1 {
                let period = mul(self.level_f32(0xc0), 90.0)? as i32;
                let r = self.rng.rand_int(10).ok_or_else(|| super::Unsupported("RandInt(10)".into()))? as i32;
                self.dead_end(i, r + 0x14);
                let half = u32::try_from(period >> 1).map_err(|_| super::Unsupported("RandInt(negative)".into()))?;
                let r2 = self.rng.rand_int(half).ok_or_else(|| super::Unsupported("RandInt(0) trap".into()))? as i32;
                let v = r2 + self.enemies[i].i32_(0x36c) + period;
                self.enemies[i].set_i32(0x338, v);
            }
            if self.enemies[i].i32_(0x36c) < 1 {
                self.enemies[i].set_i16(e::LOGIC, 0);
                attack = false;
            }
        }
        if attack {
            if self.enemies[i].i16_(0x2f0) != 0 {
                let a = if self.enemies[i].i16_(0x21a) < 2 { 0xc } else { 0x10 };
                self.request_enemy_action(i, a);
            }
            self.enemies[i].set_i16(e::LOGIC, 0);
        }
        self.move_logic00(i)?;
        self.enemies[i].set_i16(e::LOGIC, 0x1e);
        Ok(())
    }

    /// move_logic16 0x13ecd8: ranged attacker (action 0x18 on the +0x334
    /// timer); inside 0.6 × range it backs off (action 0x16 at speed 0.8,
    /// heading away plus an offset_step5 step per +0x36c contact) until
    /// beyond 0.8 × range.
    fn move_logic16(&mut self, i: usize) -> R<()> {
        let mut off = 0.0f32;
        let contact_before = self.enemies[i].i32_(0x36c);
        if self.found_suicide_bomber(i)? {
            self.enemies[i].set_i16(e::LOGIC, 0x18);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let f = self.get_face_ang(i, true)?;
        self.enemies[i].set_f32(0x260, f);
        let in_reach = match usize::try_from(self.enemies[i].i16_(0x288)) {
            Ok(p) if self.enemies[i].i16_(0x2f0) != 0 => {
                let dy = sub(self.ef(i, 0x44), self.players[p].f32_(0x54))?;
                (-10.0..=10.0).contains(&dy)
            }
            _ => false,
        };
        if in_reach {
            let range = self.ef(i, 0x314);
            if self.enemies[i].i32_(0x330) == 0 {
                if self.ef(i, 0x290) <= mul(range, f32::from_bits(0x3f19_999a))? {
                    self.enemies[i].set_i32(0x330, 1);
                }
            } else if mul(range, f32::from_bits(0x3f4c_cccd))? < self.ef(i, 0x290) {
                self.enemies[i].set_i32(0x330, 0);
            } else if 0 < self.enemies[i].i32_(0x36c) {
                let k = self.enemies[i].i32_(0x338);
                if k < 8 {
                    off = *usize::try_from(k).ok().and_then(|k| self.offset_step5.get(k)).ok_or_else(|| super::Unsupported("offset_step5 index".into()))?;
                    self.enemies[i].set_i32(0x338, k + 1);
                    self.enemies[i].set_i32(0x36c, 0);
                } else {
                    self.enemies[i].set_i32(0x330, 0);
                }
            }
            let t = self.enemies[i].i32_(0x334);
            if t < 1 {
                if self.enemies[i].i32_(0x330) == 0 {
                    self.request_enemy_action(i, 0x18);
                } else {
                    let a = wrap(add(add(self.ef(i, 0x260), PI)?, off)?)?;
                    self.request_enemy_action(i, 0x16);
                    if (self.enemies[i].i32_(0xe0) as u32).wrapping_sub(0x16) < 2 {
                        self.set_enemy_trans(f32::from_bits(0x3f4c_cccd), a, i)?;
                    }
                    self.enemies[i].set_i32(0x36c, 0);
                }
            } else {
                self.enemies[i].set_i32(0x334, t - self.globals.vb_elapsed);
            }
        }
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)?;
        if self.enemies[i].i16_(0x294) != 0 {
            self.enemies[i].set_i32(0x36c, 0);
        }
        if contact_before < 1 && 0 < self.enemies[i].i32_(0x36c) {
            let h = self.ef(i, 0x260);
            self.enemies[i].set_f32(0x268, h);
            self.enemies[i].set_i32(0x338, 0);
        }
        Ok(())
    }

    /// The enemy AnimInfo (+0x84, the player's +0x94 layout) frame +0x9c,
    /// kept on the native clock.
    fn enemy_anim_frame(&self, i: usize) -> f32 {
        self.enemy_anims.get(i).and_then(|a| a.as_ref()).map_or(self.ef(i, 0x9c), |a| a.clock.frame)
    }

    /// AnimInfo +0x1e (+0xa2): 0 freezes the clock (CalcAnimInfo).
    fn set_enemy_anim_enabled(&mut self, i: usize, on: u16) {
        self.enemies[i].set_u16(0xa2, on);
        if let Some(Some(a)) = self.enemy_anims.get_mut(i) {
            a.clock.enabled_word = on;
        }
    }

    /// move_logic19 0x13e4e8 (golems): frozen (AnimInfo disabled) until a
    /// target within range comes near the camera (+0x2f0) — +0x330 born,
    /// AudioGolemBorn. Then +0x32a stages: 0 → 1 request WALK; once walking
    /// 2 with a 30..119-vblank walk (+0x338), stepping toward the target
    /// (paused while +0x36c counts down) until it runs out (WALK request
    /// dropped), it is blocked on +0x212 or the action falls back to READY.
    /// Stomps at WALK frames 10 and 25, AudioGolemSwing at frame 2 of
    /// ATTACK1/2/3; +0x3a0 keeps the previous frame.
    fn move_logic19(&mut self, i: usize) -> R<()> {
        if self.enemies[i].i32_(0x330) == 0 {
            if 0 <= self.enemies[i].i16_(0x288) && self.ef(i, 0x290) < self.ef(i, 0x314) && self.enemies[i].i16_(0x2f0) != 0 {
                self.enemies[i].set_i32(0x330, 1);
                let pos = self.enemies[i].v3(e::POSITION);
                self.audio_golem_born(pos);
            }
            if self.enemies[i].i32_(0x330) == 0 {
                self.set_enemy_anim_enabled(i, 0);
                return Ok(());
            }
        }
        self.set_enemy_anim_enabled(i, 1);
        if self.enemies[i].i16_(0x2f2) == 0 || self.enemies[i].i16_(0x288) < 0 {
            self.enemies[i].set_i16(e::LOGIC, ((i & 1) + 5) as i16);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let f = self.get_face_ang(i, true)?;
        self.enemies[i].set_f32(0x260, f);
        let stage = self.enemies[i].i16_(0x32a);
        let mut walk = stage == 2;
        if stage != 2 {
            if stage != 1 {
                self.enemies[i].set_i32(0x338, 1);
                self.enemies[i].set_i16(0x32a, stage.wrapping_add(1));
            }
            self.request_enemy_action(i, 3);
            if self.enemies[i].i32_(0xe0) == 3 {
                let r = self.rng.rand_int(0x5a).ok_or_else(|| super::Unsupported("RandInt".into()))? as i32;
                self.enemies[i].set_i32(0x338, r + 0x1e);
                let s = self.enemies[i].i16_(0x32a).wrapping_add(1);
                self.enemies[i].set_i16(0x32a, s);
            }
            walk = self.enemies[i].i16_(0x32a) == 2;
        }
        if walk {
            let vb = self.globals.vb_elapsed;
            let t = self.enemies[i].i32_(0x36c);
            let step = t < 1 || {
                self.enemies[i].set_i32(0x36c, t - vb);
                t - vb < 1
            };
            if step {
                let h = self.ef(i, 0x260);
                self.set_enemy_trans(1.0, h, i)?;
            }
            let left = self.enemies[i].i32_(0x338) - vb;
            self.enemies[i].set_i32(0x338, left);
            if left < 1 && self.enemies[i].i32_(0xe4) == 3 {
                self.enemies[i].set_i32(0xe4, 0);
            }
            let blocked = 0 < self.enemies[i].i32_(0x36c) && self.enemies[i].i16_(0x212) == 1;
            if blocked || self.enemies[i].i32_(0xe0) == 0 {
                self.enemies[i].set_i16(0x32a, 0);
            }
        }
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)?;
        let prev = self.ef(i, 0x3a0);
        let cur = self.enemy_anim_frame(i);
        if self.enemies[i].i32_(0xe0) == 3 {
            if 0 < self.enemies[i].i32_(0x36c) && 1 < self.enemies[i].i32_(0x338) {
                self.enemies[i].set_i32(0x338, 1);
            }
            if (prev < 10.0 && 10.0 <= cur) || (prev < 25.0 && 25.0 <= cur) {
                self.audio_enemy_walk(i)?;
            }
        }
        if matches!(self.enemies[i].i32_(0xe0), 0xc | 0xe | 0x10) && prev < 2.0 && 2.0 <= cur {
            let pos = self.enemies[i].v3(e::POSITION);
            self.audio_golem_swing(pos)?;
        }
        self.enemies[i].set_f32(0x3a0, cur);
        Ok(())
    }

    /// move_logic24 0x13d8b0 (entered from found_suicide_bomber): run at 2×
    /// directly away from the SuicideBomberIndex enemy, adding the next
    /// offset_step5 step (up to eight, +0x338) on each +0x36c contact.
    fn move_logic24(&mut self, i: usize) -> R<()> {
        let mut off = 0.0f32;
        let contact_before = self.enemies[i].i32_(0x36c);
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        if 0 < self.enemies[i].i32_(0x36c) {
            let k = self.enemies[i].i32_(0x338);
            if k < 8 {
                off = *usize::try_from(k).ok().and_then(|k| self.offset_step5.get(k)).ok_or_else(|| super::Unsupported("offset_step5 index".into()))?;
                self.enemies[i].set_i32(0x338, k + 1);
                self.enemies[i].set_i32(0x36c, 0);
            }
        }
        let s = usize::try_from(self.suicide_bomber_index).map_err(|_| super::Unsupported("move_logic24 without a bomber".into()))?;
        let y = get_yaw(self.enemies[s].v3(0x40), self.enemies[i].v3(0x40))?;
        let h = wrap(add(add(y, PI)?, off)?)?;
        self.enemies[i].set_f32(0x260, h);
        self.set_enemy_trans(2.0, h, i)?;
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)?;
        if self.enemies[i].i16_(0x294) != 0 {
            self.enemies[i].set_i32(0x36c, 0);
        }
        if contact_before < 1 && 0 < self.enemies[i].i32_(0x36c) {
            let h = self.ef(i, 0x260);
            self.enemies[i].set_f32(0x268, h);
            self.enemies[i].set_i32(0x338, 0);
        }
        Ok(())
    }

    /// xlate31 0x13cb10(scale, enemy): set_enemy_trans's step along +0x260
    /// without its action request or checks.
    fn xlate31(&mut self, scale: f32, i: usize) -> R<()> {
        let heading = self.ef(i, 0x260);
        if self.ef(i, 0x320) != heading {
            let (s, c) = (sinf(heading)?, cosf(heading)?);
            let rec = &mut self.enemies[i];
            rec.set_f32(0x318, s);
            rec.set_f32(0x31c, c);
            rec.set_f32(0x320, heading);
        }
        let fw = self.fwrdval[self.enemies[i].i32_(e::TYPE) as usize];
        let rec = &mut self.enemies[i];
        rec.set_f32(0x224, add(mul(mul(rec.f32_(0x318), fw)?, scale)?, rec.f32_(0x224))?);
        rec.set_f32(0x22c, add(mul(mul(rec.f32_(0x31c), fw)?, scale)?, rec.f32_(0x22c))?);
        Ok(())
    }

    /// get_target_ang 0x13c2d8: toward the target each brain period
    /// (+0x380 ≥ +0x37c), stored in +0x260.
    fn get_target_ang(&mut self, i: usize) -> R<f32> {
        let rec = &self.enemies[i];
        let Ok(p) = usize::try_from(rec.i16_(0x288)) else { return Ok(rec.f32_(0x260)) };
        if rec.i32_(0x380) < rec.i32_(0x37c) {
            return Ok(rec.f32_(0x260));
        }
        let y = get_yaw(self.players[p].v3(0x50), rec.v3(e::POSITION))?;
        self.enemies[i].set_f32(0x260, y);
        Ok(y)
    }

    /// move_logic15 0x13efe0: patrol the lookout chain — stage 0 picks the
    /// nearest lookout with a next link, stage 1 walks to it and follows
    /// +0x70 on arrival (|dy| < 4, planar < 1); a target within 0.8 ×
    /// range (+0x28c and planar distance) switches to logic 0.
    fn move_logic15(&mut self, i: usize) -> R<()> {
        if self.found_suicide_bomber(i)? {
            self.enemies[i].set_i16(e::LOGIC, 0x18);
            return self.do_ai(i);
        }
        if let Ok(p) = usize::try_from(self.enemies[i].i16_(0x288)) {
            let reach = mul(self.ef(i, 0x314), f32::from_bits(0x3f4c_cccd))?;
            if self.ef(i, 0x28c) <= reach {
                let pp = self.players[p].v3(0x50);
                let (d, _) = fqdist(sub(pp[0], self.ef(i, 0x40))?, sub(pp[2], self.ef(i, 0x48))?)?;
                if d <= mul(self.ef(i, 0x314), f32::from_bits(0x3f4c_cccd))? {
                    self.enemies[i].set_i16(e::LOGIC, 0);
                    self.do_ai(i)?;
                    self.enemies[i].set_i16(0x32a, 0);
                    return Ok(());
                }
            }
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let stage = self.enemies[i].i16_(0x32a);
        if stage == 0 {
            let me = self.enemies[i].v3(e::POSITION);
            let mut best = (-1i32, f32::from_bits(0x47c3_5000));
            for (k, l) in self.lookouts.iter().enumerate() {
                if l.next < 0 {
                    continue;
                }
                let d = [sub(l.position[0], me[0])?, sub(l.position[1], me[1])?, sub(l.position[2], me[2])?];
                let q = sqrt_rn(add(add(mul(d[0], d[0])?, mul(d[1], d[1])?)?, mul(d[2], d[2])?)?)?;
                if q < best.1 {
                    best = (k as i32, q);
                }
            }
            self.enemies[i].set_i32(0x330, best.0);
            self.enemies[i].set_i16(0x32a, 1);
        }
        if stage == 0 || stage == 1 {
            let l = usize::try_from(self.enemies[i].i32_(0x330))
                .ok()
                .and_then(|k| self.lookouts.get(k))
                .ok_or_else(|| super::Unsupported("move_logic15 without a linked lookout (Lookouts[-1])".into()))?;
            let (lp, next) = (l.position, l.next);
            let me = self.enemies[i].v3(e::POSITION);
            let y = get_yaw(lp, me)?;
            self.enemies[i].set_f32(0x260, y);
            if sub(lp[1], me[1])?.abs() < 4.0 && fqdist(sub(lp[0], me[0])?, sub(lp[2], me[2])?)?.0 < 1.0 {
                self.enemies[i].set_i32(0x330, i32::from(next));
            }
        }
        let h = self.ef(i, 0x260);
        self.set_enemy_trans(1.0, h, i)?;
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)
    }

    /// move_logic01 0x141f48: walk straight at the target (get_target_ang
    /// each brain period). After a dead end (+0x36c, counting down only
    /// while +0x212 is 2) it waits until the heading moves 6° (0x3dd67750)
    /// from the blocked one at +0x268.
    fn move_logic01(&mut self, i: usize) -> R<()> {
        let contact_before = self.enemies[i].i32_(0x36c);
        if self.found_suicide_bomber(i)? {
            self.enemies[i].set_i16(e::LOGIC, 0x18);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(0x2f2) == 0 || self.enemies[i].i16_(0x288) < 0 {
            self.enemies[i].set_i16(e::LOGIC, ((i & 1) + 5) as i16);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let h = self.get_target_ang(i)?;
        self.enemies[i].set_f32(0x260, h);
        let mut t = self.enemies[i].i32_(0x36c);
        if self.enemies[i].i16_(0x212) == 2 && 0 < t {
            t -= self.globals.vb_elapsed;
            self.enemies[i].set_i32(0x36c, t);
        }
        let step = t < 1 || f32::from_bits(0x3dd6_7750) <= sub(self.ef(i, 0x260), self.ef(i, 0x268))?.abs();
        if step {
            self.enemies[i].set_i32(0x36c, 0);
            let h = self.ef(i, 0x260);
            self.set_enemy_trans(1.0, h, i)?;
        }
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)?;
        if contact_before < 1 && 0 < self.enemies[i].i32_(0x36c) {
            let h = self.ef(i, 0x260);
            self.enemies[i].set_f32(0x268, h);
        }
        Ok(())
    }

    /// move_logic31 0x13cbc0: face the target near the camera; lunge on
    /// ATTACK1 (marking +0x2e0 within 7.5), rest 30..59 vblanks after an
    /// attack, otherwise creep at half speed until the +0x334 timer is out,
    /// then ATTACK1 within 10 or ATTACK3.
    fn move_logic31(&mut self, i: usize) -> R<()> {
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let f = self.get_face_ang(i, true)?;
        self.enemies[i].set_f32(0x260, f);
        if 0 <= self.enemies[i].i16_(0x288) {
            self.enemies[i].set_i32(0xe4, 0);
            if self.enemies[i].i16_(0x2f0) != 0 {
                let a = self.enemies[i].i32_(0xe0);
                let rand = |g: &mut Game| -> R<i32> { Ok(g.rng.rand_int(0x1e).ok_or_else(|| super::Unsupported("RandInt".into()))? as i32 + 0x1e) };
                if (a as u32).wrapping_sub(0xc) < 2 {
                    self.xlate31(1.0, i)?;
                    let r = rand(self)?;
                    self.enemies[i].set_i32(0x334, r);
                    if self.ef(i, 0x290) <= 7.5 {
                        let t = self.enemies[i].i16_(0x288);
                        self.enemies[i].set_i16(0x2e0, t);
                    }
                } else if (a as u32).wrapping_sub(0x10) < 2 {
                    let r = rand(self)?;
                    self.enemies[i].set_i32(0x334, r);
                } else if self.enemies[i].i32_(0x334) < 1 {
                    self.request_enemy_action(i, if self.ef(i, 0x290) <= 10.0 { 0xc } else { 0x10 });
                } else if a != 1 {
                    let t = self.enemies[i].i32_(0x334) - self.globals.vb_elapsed;
                    self.enemies[i].set_i32(0x334, t);
                    self.xlate31(0.5, i)?;
                }
            }
        }
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)
    }

    /// move_logic28 0x13d310: hold position facing the target and attack
    /// from range (0xc/0xe alternating on +0x33c, 0x10 at level +0x21a ≥ 3,
    /// when the +0x334 timer is out) while it is near the camera, within
    /// +0x314 and ±10 in height; within 6 it switches to logic 7.
    fn move_logic28(&mut self, i: usize) -> R<()> {
        if 0 <= self.enemies[i].i16_(0x288) && self.ef(i, 0x290) <= 6.0 {
            self.enemies[i].set_i16(e::LOGIC, 7);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let f = self.get_face_ang(i, true)?;
        self.enemies[i].set_f32(0x260, f);
        if let Ok(p) = usize::try_from(self.enemies[i].i16_(0x288)) {
            let dy = sub(self.ef(i, 0x44), self.players[p].f32_(0x54))?;
            if self.enemies[i].i16_(0x2f0) != 0 && self.ef(i, 0x290) <= self.ef(i, 0x314) && (-10.0..=10.0).contains(&dy) {
                let t = self.enemies[i].i32_(0x334);
                if 0 < t {
                    self.enemies[i].set_i32(0x334, t - self.globals.vb_elapsed);
                } else if 3 <= self.enemies[i].i16_(0x21a) {
                    self.request_enemy_action(i, 0x10);
                } else {
                    let n = self.enemies[i].i32_(0x33c).wrapping_add(1);
                    self.enemies[i].set_i32(0x33c, n);
                    self.request_enemy_action(i, if n & 1 != 0 { 0xc } else { 0xe });
                }
            }
        }
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)
    }

    /// move_logic27 0x13d4e8: stand still until a target is found, then
    /// chase it for good (+0x314 range 1e6, logic 0, move_logic00).
    fn move_logic27(&mut self, i: usize) -> R<()> {
        if self.enemies[i].i16_(0x288) < 0 {
            for o in [0x224, 0x228, 0x22c] {
                self.enemies[i].set_u32(o, 0);
            }
            return self.do_enemy_move(i);
        }
        self.enemies[i].set_u32(0x314, 0x4974_23f0);
        self.enemies[i].set_i16(e::LOGIC, 0);
        self.move_logic00(i)
    }

    /// move_logic29 0x13cf40: keep a 8–18 band from the target (+0x330: 0
    /// hold, 1 back off, 2 close in), stepping through offset_step5 on each
    /// +0x36c contact, attacking (0xc/0xe alternating, 0x10 at level 3+)
    /// when its +0x334 timer runs out in the hold band.
    fn move_logic29(&mut self, i: usize) -> R<()> {
        let mut off = 0.0f32;
        let contact_before = self.enemies[i].i32_(0x36c);
        if self.found_suicide_bomber(i)? {
            self.enemies[i].set_i16(e::LOGIC, 0x18);
            return self.do_ai(i);
        }
        if 0 <= self.enemies[i].i16_(0x288) && self.ef(i, 0x290) <= 6.0 {
            self.enemies[i].set_i16(e::LOGIC, 7);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let f = self.get_face_ang(i, true)?;
        self.enemies[i].set_f32(0x260, f);
        let in_reach = match usize::try_from(self.enemies[i].i16_(0x288)) {
            Ok(p) if self.enemies[i].i16_(0x2f0) != 0 => {
                let dy = sub(self.ef(i, 0x44), self.players[p].f32_(0x54))?;
                (-10.0..=10.0).contains(&dy)
            }
            _ => false,
        };
        if in_reach {
            let d = self.ef(i, 0x290);
            if self.enemies[i].i32_(0x330) == 0 {
                if d <= 8.0 {
                    self.enemies[i].set_i32(0x330, 1);
                }
                if 18.0 < self.ef(i, 0x290) {
                    self.enemies[i].set_i32(0x330, 2);
                }
            } else {
                let back = if self.enemies[i].i32_(0x330) == 1 { 10.0 < d } else { d <= 16.0 };
                if back {
                    self.enemies[i].set_i32(0x330, 0);
                }
                if self.enemies[i].i32_(0x330) != 0 && 0 < self.enemies[i].i32_(0x36c) {
                    let k = self.enemies[i].i32_(0x338);
                    if k < 8 {
                        off = *usize::try_from(k).ok().and_then(|k| self.offset_step5.get(k)).ok_or_else(|| super::Unsupported("offset_step5 index".into()))?;
                        self.enemies[i].set_i32(0x338, k + 1);
                        self.enemies[i].set_i32(0x36c, 0);
                    } else {
                        self.enemies[i].set_i32(0x330, 0);
                    }
                }
            }
            let t = self.enemies[i].i32_(0x334);
            if t < 1 {
                let mode = self.enemies[i].i32_(0x330);
                if mode == 0 || self.enemies[i].i16_(0x212) == 1 {
                    if self.enemies[i].i16_(0x21a) < 3 {
                        let n = self.enemies[i].u32_(0x33c).wrapping_add(1);
                        self.enemies[i].set_u32(0x33c, n);
                        self.request_enemy_action(i, if n & 1 == 0 { 0xe } else { 0xc });
                    } else {
                        self.request_enemy_action(i, 0x10);
                    }
                } else {
                    let h = self.ef(i, 0x260);
                    let a = if mode == 1 { add(add(h, PI)?, off)? } else { add(off, h)? };
                    let a = wrap(a)?;
                    self.set_enemy_trans(f32::from_bits(0x3f4c_cccd), a, i)?;
                    self.request_enemy_action(i, 3);
                    self.enemies[i].set_i32(0x36c, 0);
                }
            } else {
                self.enemies[i].set_i32(0x334, t - self.globals.vb_elapsed);
            }
        }
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)?;
        if self.enemies[i].i16_(0x294) != 0 {
            self.enemies[i].set_i32(0x36c, 0);
        }
        if contact_before < 1 && 0 < self.enemies[i].i32_(0x36c) {
            let h = self.ef(i, 0x260);
            self.enemies[i].set_f32(0x268, h);
            self.enemies[i].set_i32(0x338, 0);
        }
        Ok(())
    }

    /// move_logic03 0x141b40 (flee): away from the +0x33c player (+ offset
    /// steps after contacts), the move vector damped by 0.9; a visible
    /// target returns to logic 0, no flee player to wander 5/6.
    fn move_logic03(&mut self, i: usize) -> R<()> {
        if self.enemies[i].i16_(0x2f2) != 0 && 0 <= self.enemies[i].i16_(0x288) {
            self.enemies[i].set_i16(e::LOGIC, 0);
            return self.do_ai(i);
        }
        if self.enemies[i].i32_(0x33c) < 0 {
            self.enemies[i].set_i16(e::LOGIC, (i as i16 & 1) + 5);
            return self.do_ai(i);
        }
        let contact_before = self.enemies[i].i32_(0x36c);
        let mut off = 0.0f32;
        if 0 < contact_before {
            let k = self.enemies[i].i32_(0x338);
            if k < 8 {
                off = *usize::try_from(k).ok().and_then(|k| self.offset_step5.get(k)).ok_or_else(|| super::Unsupported("offset_step5 index".into()))?;
                self.enemies[i].set_i32(0x338, k + 1);
                self.enemies[i].set_i32(0x36c, 0);
            }
        }
        let a = wrap(add(add(self.get_face_ang2(i, true)?, PI)?, off)?)?;
        self.enemies[i].set_f32(0x260, a);
        self.set_enemy_trans(1.0, a, i)?;
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        for o in [0x224, 0x228, 0x22c] {
            let v = mul(self.ef(i, o), f32::from_bits(0x3f66_6666))?;
            self.enemies[i].set_f32(o, v);
        }
        self.do_enemy_move(i)?;
        if self.enemies[i].i16_(0x294) != 0 {
            self.enemies[i].set_i32(0x36c, 0);
        }
        if contact_before < 1 && 0 < self.enemies[i].i32_(0x36c) {
            let h = self.ef(i, 0x260);
            self.enemies[i].set_f32(0x268, h);
            self.enemies[i].set_i32(0x338, 0);
        }
        Ok(())
    }

    /// move_logic02 (+45°, then logic 4) and move_logic04 (−45°, then
    /// logic 2): face the target player, else step the heading every
    /// brain period; after four steps switch to the sibling logic.
    fn move_logic_diag(&mut self, i: usize, clockwise: bool, sibling: i16) -> R<()> {
        let eighth = f32::from_bits(0x3f49_0fdb);
        if self.found_suicide_bomber(i)? {
            self.enemies[i].set_i16(e::LOGIC, 0x18);
            return self.do_ai(i);
        }
        if 0 <= self.enemies[i].i16_(0x288) && self.ef(i, 0x28c) <= 8.0 {
            self.enemies[i].set_i16(e::LOGIC, 0);
            return self.do_ai(i);
        }
        if self.enemies[i].i16_(e::LOGIC) != self.enemies[i].i16_(0x328) {
            self.format_brain(i);
        }
        let timer = self.enemies[i].i32_(0x36c);
        if 0 < timer {
            let left = timer - self.globals.vb_elapsed;
            self.enemies[i].set_i32(0x36c, left);
            if left < 1 {
                let h0 = self.ef(i, 0x260);
                let h = wrap(if clockwise { add(h0, eighth)? } else { sub(h0, eighth)? })?;
                self.enemies[i].set_f32(0x260, h);
                let c = self.enemies[i].i32_(0x370) + 1;
                self.enemies[i].set_i32(0x370, c);
                if 4 <= c {
                    self.enemies[i].set_i32(0x370, 0);
                    self.enemies[i].set_i16(e::LOGIC, sibling);
                    self.enemies[i].set_i16(0x326, sibling);
                    self.enemies[i].set_i32(0x388, 4);
                }
            }
        }
        if let Ok(p) = usize::try_from(self.enemies[i].i32_(0x298)) {
            let y = get_yaw(self.players[p].v3(0x50), self.enemies[i].v3(0x40))?;
            self.enemies[i].set_f32(0x260, y);
        }
        let h = self.ef(i, 0x260);
        self.set_enemy_trans(1.0, h, i)?;
        let y = self.turn_enemy_ang(self.ef(i, 0x260), i)?;
        self.enemies[i].set_f32(0x258, y);
        self.do_enemy_move(i)
    }

    /// do_ai: clear the move vector and run the enemy's logic.
    pub fn do_ai(&mut self, i: usize) -> R<()> {
        for o in [0x224, 0x228, 0x22c] {
            self.enemies[i].set_u32(o, 0);
        }
        match self.enemies[i].i16_(e::LOGIC) {
            0 => self.move_logic00(i)?,
            1 => self.move_logic01(i)?,
            2 => self.move_logic_diag(i, true, 4)?,
            3 => self.move_logic03(i)?,
            4 => self.move_logic_diag(i, false, 2)?,
            5 => self.move_logic_wander(i, -HALF_PI)?,
            6 => self.move_logic_wander(i, HALF_PI)?,
            7 => self.move_logic07(i)?,
            0x12 => self.move_logic18(i)?,
            0x13 => self.move_logic19(i)?,
            0xe => self.move_logic14(i)?,
            0xf => self.move_logic15(i)?,
            // do_ai's jump table 0x2a7a30 (checked entry by entry): 0x10 and
            // 0x1a run move_logic16, 0x11 and 0x17 move_logic23 (the +0x334
            // cooldown), 0x19 kill_enemy; move_logic17/26 are not in it
            // (trace-A1-statue-v2 step 87: a relocated logic-0x11 grunt).
            0x10 | 0x1a => self.move_logic16(i)?,
            0x11 | 0x17 => self.move_logic23(i)?,
            0x18 => self.move_logic24(i)?,
            0x19 => self.kill_enemy(i)?,
            0x1b => self.move_logic27(i)?,
            0x1c => self.move_logic28(i)?,
            0x1d => self.move_logic29(i)?,
            0x1e => self.move_logic30(i)?,
            0x1f => self.move_logic31(i)?,
            l if (0..0x20).contains(&l) => return unsupported(format!("move_logic{l:02}")),
            _ => self.kill_enemy(i)?,
        }
        Ok(())
    }

    /// enemy_mod_alpha 0x13b110: tree alpha ± delta·vb_elapsed in 0..=255.
    /// Only enemy_mod_alpha reads enemy tree alpha, so this is presentation
    /// state (`enemy_alpha`; the initial tree alpha 255 is a HYPOTHESIS).
    fn enemy_mod_alpha(&mut self, i: usize, delta: i32) {
        let a = self.enemy_alpha[i];
        if (delta < 1 || a < 0xff) && (delta < 0 || a != 0) {
            self.enemy_alpha[i] = (delta * self.globals.vb_elapsed + a).clamp(0, 0xff);
        }
    }

    /// enemy_ai_control: returns true when the enemy was removed.
    fn enemy_ai_control(&mut self, i: usize) -> R<bool> {
        let rec = &mut self.enemies[i];
        rec.set_i16(0x2ea, rec.i16_(0x2ea).wrapping_add(self.globals.vb_elapsed as i16));
        if rec.f32_(0x314) < rec.f32_(0x290) && rec.i16_(0x2f0) == 0 {
            return Ok(true);
        }
        let t = rec.i16_(0x28a);
        if 0 <= t && rec.i16_(0x288) != t {
            rec.set_u32(0x360, 0);
            rec.set_u32(0x364, 4);
        }
        if 0 <= self.globals.boss_type && self.boss_is_dying != 0 {
            // AddEnemyHitCol(+0x250, +0x50, +0x10, 0, killed, type).
            let r = &self.enemies[i];
            let (scale, at, rows, t) = (r.f32_(e::HALF_HEIGHT), r.v3(0x50), [r.v3(0x10), r.v3(0x20), r.v3(0x30)], r.i32_(e::TYPE));
            self.add_enemy_hit_col(scale, at, Some(rows), 0, true, t)?;
            self.kill_enemy(i)?;
            return Ok(true);
        }
        self.do_ai(i)?;
        if self.enemies[i].i32_(0xe0) == 1 {
            self.enemies[i].set_i32(0xe4, 3);
        }
        if self.enemies[i].i32_(e::TYPE) == 0x18 {
            // Phasing (type 0x18): +0x2e8 alternates fade-out (positive) and
            // fade-in (negative) periods of RandInt(60) + 60 updates.
            let st = self.enemies[i].i32_(0xe0);
            if (st as u32).wrapping_sub(3) < 2 || st == 0 {
                let t = self.enemies[i].i16_(0x2e8);
                let vb = self.globals.vb_elapsed as i16;
                let rand60 = |g: &mut Self| g.rng.rand_int(0x3c).map(|r| r as i16 + 0x3c).ok_or_else(|| super::Unsupported("RandInt(60)".into()));
                if t == 0 {
                    self.enemy_mod_alpha(i, -16);
                    let r = rand60(self)?;
                    self.enemies[i].set_i16(0x2e8, r);
                } else if t < 1 {
                    self.enemy_mod_alpha(i, 16);
                    let n = t.wrapping_add(vb);
                    self.enemies[i].set_i16(0x2e8, if 0 < n { 0 } else { n });
                } else {
                    self.enemy_mod_alpha(i, -16);
                    let n = t.wrapping_sub(vb);
                    self.enemies[i].set_i16(0x2e8, n);
                    if n < 0 {
                        let r = rand60(self)?;
                        self.enemies[i].set_i16(0x2e8, -r);
                    }
                }
            } else {
                self.enemy_mod_alpha(i, -16);
                self.enemies[i].set_i16(0x2e8, 0);
            }
        }
        Ok(self.enemies[i].i32_(e::STATE) == 0)
    }

    /// enemy_attack_control: apply landed attacks (+0x2e4 low nibble) and missiles.
    fn enemy_attack_control(&mut self, i: usize) -> R<()> {
        let logic = self.enemies[i].u16_(e::LOGIC);
        let ev = self.enemies[i].u32_(0x2e4);
        if ((logic.wrapping_sub(0x1c) < 3 && ev & 0xf != 0) || (logic == 0x1f && ev & 2 != 0)) && self.enemies[i].i32_(0x298) < 0 {
            let target = self.missile_target(i)?;
            let from = self.enemies[i].v3(e::COLLISION);
            self.shoot_missile(i, target, from)?;
            self.enemies[i].and_u32(0x2e4, 0xffff_fff0);
        }
        let hit = self.enemies[i].i16_(0x2e0);
        if 0 <= hit && self.enemies[i].u32_(0x2e4) & 0xf != 0 {
            let p = hit as usize;
            if self.players[p].i32_(0xfc) == 1 {
                let rec = &self.enemies[i];
                let mut dmg = rec.f32_(0xd0);
                let mut flags = 0u32;
                let half = rec.f32_(e::HALF_HEIGHT);
                let t = rec.i32_(e::TYPE);
                let small;
                if self.globals.enemy_shrink < 1.0 {
                    dmg = mul(dmg, 0.5)?;
                    flags |= 0x4000_0000;
                    small = half <= 2.0;
                } else {
                    if rec.u32_(0x2e4) & 2 != 0 {
                        dmg = mul(dmg, 1.5)?;
                        if 2.0 < half {
                            flags = 0x10;
                        }
                    }
                    if t == 0x1d {
                        flags |= 0x20;
                    }
                    if half <= 2.0 {
                        flags |= 0x4000_0000;
                        small = true;
                    } else {
                        small = false;
                    }
                }
                let mut pain = 1;
                if (small && t != 0x1f) || t == 2 || t == 8 || t == 0xb {
                    self.audio_enemy_bite(i)?;
                    pain = 0;
                } else if (t as u32).wrapping_sub(4) < 2 || t == 10 {
                    let level = self.enemies[i].i16_(0x21a);
                    self.audio_player_hit(p, if level < 2 { 4 } else { 3 });
                    pain = 0;
                }
                if flags & 0x130 == 0 {
                    self.damage_player(dmg, p, pain, flags, None)?;
                } else {
                    let pl = self.players[p].v3(0x70);
                    let c = self.enemies[i].v3(e::COLLISION);
                    let mut v = [sub(pl[0], c[0])?, -sub(pl[1], c[1])?, sub(pl[2], c[2])?];
                    crate::collision::normal_vector_pub(&mut v)?;
                    self.damage_player(dmg, p, pain, flags, Some(v))?;
                }
                self.did_action = true;
                let rec = &mut self.enemies[i];
                rec.set_i16(0x2e2, rec.i16_(0x2e2) + 1);
                rec.and_u32(0x2e4, 0xffff_fff0);
            }
            self.enemies[i].set_i16(0x2e0, -1);
        }
        if self.enemies[i].u32_(0x2e4) & 0x10 != 0 {
            let target = self.missile_target(i)?;
            let from = self.enemies[i].v3(e::COLLISION);
            self.shoot_missile(i, target, from)?;
            self.enemies[i].set_i32(0x330, 1);
            self.enemies[i].and_u32(0x2e4, 0xffff_ffef);
        }
        Ok(())
    }

    /// enemy_attack_control's aim point: the +0x288 target player's +0x70,
    /// else 20 units ahead along the +0x258 heading.
    fn missile_target(&self, i: usize) -> R<V3> {
        let rec = &self.enemies[i];
        match usize::try_from(rec.i16_(0x288)) {
            Ok(p) => Ok(self.players[p].v3(0x70)),
            Err(_) => {
                let c = rec.v3(e::COLLISION);
                let h = rec.f32_(0x258);
                Ok([add(mul(sinf(h)?, 20.0)?, c[0])?, c[1], add(mul(cosf(h)?, 20.0)?, c[2])?])
            }
        }
    }

    /// enemy_knockback_control.
    fn enemy_knockback_control(&mut self, i: usize) -> R<()> {
        let t = self.enemies[i].i32_(e::TYPE);
        if t == self.globals.boss_type {
            return Ok(());
        }
        let rec = &self.enemies[i];
        if 1.0 <= rec.f32_(0x2b4) {
            let flags = rec.u32_(0x2b8);
            let push = |g: &mut Game, k: f32| -> R<()> {
                let r = &mut g.enemies[i];
                for n in 0..3 {
                    let o = 0x270 + 4 * n;
                    r.set_f32(o, add(mul(k, r.f32_(0x2bc + 4 * n))?, r.f32_(o))?);
                }
                let yaw = wrap(add(atan2f(r.f32_(0x2bc), r.f32_(0x2c4))?, PI)?)?;
                r.set_f32(0x27c, yaw);
                r.set_u32(0x2bc, 0);
                r.set_u32(0x2c0, 0);
                r.set_u32(0x2c4, 0);
                Ok(())
            };
            if flags & 0x10160 != 0 || (10.0 < rec.f32_(0x2b4) && flags & 0x200 != 0) {
                self.request_enemy_action(i, 0x1d);
                if t == 0x1d {
                    self.enemies[i].set_i32(0x330, 1);
                }
                let k = if t == 0x15 {
                    0.0
                } else if self.ef(i, e::HALF_HEIGHT) <= 2.0 && t != 0x1f {
                    40.0
                } else if t == 0x1d {
                    2.0
                } else {
                    20.0
                };
                push(self, k)?;
            } else if flags & 0x10 != 0 {
                if t != 0x1d && t != 0x15 {
                    self.request_enemy_action(i, 0x1c);
                    push(self, 8.0)?;
                } else {
                    self.enemies[i].set_i32(0x330, 1);
                }
            } else if t == 0x1d {
                self.enemies[i].set_i32(0x330, 1);
            } else {
                self.request_enemy_action(i, 0x1c);
            }
            let r = &mut self.enemies[i];
            let k = [r.f32_(0x270), r.f32_(0x274), r.f32_(0x278)];
            if 1600.0 < add(add(mul(k[0], k[0])?, mul(k[1], k[1])?)?, mul(k[2], k[2])?)? {
                let mut v = k;
                crate::collision::normal_vector_pub(&mut v)?;
                for (n, x) in v.iter().enumerate() {
                    r.set_f32(0x270 + 4 * n, mul(*x, 40.0)?);
                }
            }
            r.set_u32(0x2b4, 0);
            r.set_u32(0x2b8, 0);
            if self.ef(i, e::HEALTH) <= self.ef(i, 0x2b4) {
                self.request_enemy_action(i, 0x20);
            } else {
                // SetSkinFX(1, TextureWhite, 1, 1): the knockback flash.
                super::skin::set_skin_fx(&mut self.enemies[i], 0x1f8, 1.0, super::skin::SkinTex::White, 1, 1.0);
            }
        }
        let r = &mut self.enemies[i];
        let k0 = r.f32_(0x270);
        let k2 = r.f32_(0x278);
        r.set_f32(0x280, add(mul(k0, k0)?, mul(k2, k2)?)?);
        Ok(())
    }

    /// EnemyWorldDamage(enemy, object, position, contact).
    fn enemy_world_damage(&mut self, i: usize, object: usize, pos: V3, contact: V3) -> R<()> {
        let mut flags = 0u32;
        let mut o = Some(object);
        while let Some(k) = o {
            flags |= self.items.objects[k].flags_10;
            o = self.collision.objects[k].parent;
        }
        if self.enemies[i].i32_(e::TYPE) == 0x1b {
            return Ok(());
        }
        let kind = flags & 0xf_0000;
        if kind == 0 || flags & 0xa00_0000 == 0x200_0000 {
            return Ok(());
        }
        let mut v = [sub(pos[0], contact[0])?, 0.0, sub(pos[2], contact[2])?];
        normal_vector_2d(&mut v)?;
        match kind {
            0x10000 => {
                self.damage_enemy(5.0, i, -1, 0, Some(contact), Some(v), 1)?;
            }
            0x20000 => {
                self.damage_enemy(5.0, i, -1, 0x10, Some(contact), Some(v), 1)?;
            }
            0x30000 | 0x40000 | 0x50000 => {
                self.damage_enemy(15.0, i, -1, 0x20, Some(contact), Some(v), 1)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// EnemyCollideWalls(r, enemy, position, move): 0 free, 1 slid, 2 stopped.
    fn enemy_collide_walls(&mut self, r: f32, i: usize, pos: V3, mv: &mut V3) -> R<i32> {
        let to = [add(pos[0], mv[0])?, add(pos[1], mv[1])?, add(pos[2], mv[2])?];
        let r15 = mul(r, 1.5)?;
        let (c, _) = self.collision.world_collide(Query {
            radius: r15,
            first: pos,
            last: to,
            check_flags: 7,
            min_normal_y: f32::from_bits(0xbf5d_b22d),
            max_normal_y: f32::from_bits(0x3f5d_b22d),
            object_mask: 0x13a,
            exclusion_mask: 2,
        })?;
        let Some(hit) = c.solid else {
            self.enemy_wall_hit = None;
            return Ok(0);
        };
        self.enemy_wall_hit = Some(hit.object);
        self.enemy_wall_point = hit.position;
        self.enemy_wall_contact = Some(hit);
        self.enemy_world_damage(i, hit.object, pos, hit.position)?;
        if self.items.objects[hit.object].flags_10 & 0x38 != 0 {
            return Ok(0);
        }
        if self.enemies[i].u32_(0x344) & 1 != 0 {
            return Ok(1);
        }
        let n = hit_normal(&self.collision, &hit)?;
        if slide_along_wall(r15, pos, mv, hit.position, n)? < 0 {
            mv[0] = 0.0;
            mv[2] = 0.0;
            return Ok(2);
        }
        Ok(1)
    }

    /// EnemyCollideFloor(r, speed, enemy, position, move, wall result): floor object + 1.
    fn enemy_collide_floor(&mut self, r: f32, speed: f32, i: usize, pos: V3, mv: &mut V3, wall: i32) -> R<u32> {
        let half = self.ef(i, e::HALF_HEIGHT);
        let mut limit = add(add(speed, r)?, f32::from_bits(0x3dcc_cccd))?;
        limit = add(limit, limit)?;
        let t = self.enemies[i].i32_(e::TYPE);
        if t == 0x1d || half <= 2.0 {
            let rec = &self.enemies[i];
            let a = [sub(rec.f32_(0x300), pos[0])?, sub(rec.f32_(0x304), pos[1])?, sub(rec.f32_(0x308), pos[2])?];
            let b = [add(a[0], mv[0])?, add(a[1], mv[1])?, add(a[2], mv[2])?];
            let q = |v: V3| -> R<f32> { Ok(add(add(mul(v[0], v[0])?, mul(v[1], v[1])?)?, mul(v[2], v[2])?)?) };
            if q(a)? < q(b)? {
                limit = mul(add(speed, r)?, 0.5)?;
            }
        }
        let mut d = *mv;
        let len = crate::collision::normal_vector_pub(&mut d)?;
        let k = add(r, len)?;
        let probe = [add(pos[0], mul(k, d[0])?)?, add(pos[1], mul(k, d[1])?)?, add(pos[2], mul(k, d[2])?)?];
        let (c, _) = self.collision.floor_collide(mul(r, 0.5)?, half, sub(-half, 5.0)?, probe, false, 2)?;
        let Some(mut hit) = c.solid else {
            if 0.01 <= self.ef(i, 0x280) {
                let y = sub(self.globals.world_bottom, 1000.0)?;
                self.enemies[i].set_f32(0x2a8, y);
            } else {
                mv[0] = 0.0;
                mv[2] = 0.0;
            }
            return Ok(0);
        };
        self.enemy_world_damage(i, hit.object, pos, hit.position)?;
        let mut floor_y = hit.position[1];
        let base = sub(self.ef(i, 0x2a8), self.ef(i, 0x230))?;
        let step = sub(floor_y, base)?.abs();
        if limit < step {
            if 0.01 <= self.ef(i, 0x280) {
                let y = add(floor_y, self.ef(i, 0x230))?;
                self.enemies[i].set_f32(0x2a8, y);
            } else {
                mv[0] = 0.0;
                mv[2] = 0.0;
            }
            return Ok(0);
        }
        if 0.0 < speed && mul(speed, f32::from_bits(0x3dcc_cccd))? < step {
            let p2 = [add(pos[0], mv[0])?, add(pos[1], mv[1])?, add(pos[2], mv[2])?];
            let (c2, _) = self.collision.floor_collide(mul(r, 0.5)?, half, sub(-half, 5.0)?, p2, false, 2)?;
            match c2.solid {
                None => {
                    mv[0] = 0.0;
                    mv[2] = 0.0;
                    return Ok(0);
                }
                // The second FloorCollide reassigns the object the rest
                // reads and returns (0x143120: iVar2; trace-b1-newgame-v2
                // step 276 at the B1RAMP#6 / B1FLOOR00#04 seam).
                Some(h2) => {
                    floor_y = h2.position[1];
                    hit = h2;
                }
            }
        }
        let slide = (wall as u32).wrapping_sub(1) < 2 || (wall == 3 && self.enemy_wall_hit != Some(hit.object));
        if slide {
            let ok = (wall == 1 || wall == 3) && self.enemies[i].u32_(0x344) & 1 == 0 && {
                // wall_col +0x10: the last EnemyWallCollide hit's normal row.
                let n = match self.enemy_wall_contact {
                    Some(wh) => hit_normal(&self.collision, &wh)?,
                    None => [0.0; 3],
                };
                0 <= slide_along_wall(r, pos, mv, self.enemy_wall_point, n)?
            };
            if !ok {
                mv[0] = 0.0;
                mv[2] = 0.0;
                return Ok(0);
            }
        }
        let y = add(floor_y, self.ef(i, 0x230))?;
        self.enemies[i].set_f32(0x2a8, y);
        Ok(hit.object as u32 + 1)
    }

    /// do_enemy_collide(speed, enemy): wall result.
    fn do_enemy_collide(&mut self, speed: f32, i: usize) -> R<i32> {
        let floor_min = mul(self.globals.spf, -16.0)?;
        let logic = self.enemies[i].i16_(e::LOGIC);
        let t = self.enemies[i].i32_(e::TYPE);
        if t == 0x1f || self.enemies[i].i32_(0x36c) < 1 {
            self.enemies[i].set_i16(0x212, 0);
        }
        let moving = self.enemies[i].i16_(0x294) != 0;
        let floor_obj = self.enemies[i].u32_(0x2ac).checked_sub(1).map(|o| o as usize);
        let on_moving = floor_obj.is_some_and(|o| self.items.objects[o].flags_10 & 0x1000 == 0);
        let r = self.ef(i, e::WIDTH);
        let mut wall = 0;
        let mut dy;
        if !moving && on_moving {
            dy = self.ef(i, 0x2a8);
        } else {
            let c = self.enemies[i].v3(e::COLLISION);
            let pos = [c[0], add(sub(2.0, self.ef(i, 0x230))?, c[1])?, c[2]];
            let mut mv = self.enemies[i].v3(0x224);
            if moving {
                wall = self.enemy_collide_walls(if t == 0x1d { mul(r, 0.5)? } else { r }, i, pos, &mut mv)?;
            }
            let floor = self.enemy_collide_floor(r, speed, i, pos, &mut mv, wall)?;
            self.enemies[i].set_v3(0x224, mv);
            if floor == 0 {
                if !(floor_obj.is_some() && on_moving) {
                    let rec = &mut self.enemies[i];
                    rec.set_u32(0x22c, 0);
                    rec.set_u32(0x224, 0);
                    let half = rec.f32_(e::HALF_HEIGHT);
                    let (c2, _) = self.collision.floor_collide(mul(r, 0.5)?, half, sub(-half, 5.0)?, pos, false, 2)?;
                    if let Some(h) = c2.solid {
                        let y = add(h.position[1], self.ef(i, 0x230))?;
                        self.enemies[i].set_f32(0x2a8, y);
                    }
                }
            } else {
                self.enemies[i].set_u32(0x2ac, floor);
                let o = floor as usize - 1;
                if self.ef(i, e::HALF_HEIGHT) <= 2.0 && self.items.objects[o].flags_10 & 0x38 != 0 {
                    let rec = &mut self.enemies[i];
                    rec.set_u32(0x22c, 0);
                    rec.set_u32(0x224, 0);
                    wall = 2;
                }
            }
            let flags = floor.checked_sub(1).map_or(0, |o| self.items.objects[o as usize].flags_10);
            self.enemies[i].set_u32(0x2b0, flags);
            let rec = &self.enemies[i];
            if fqdist(rec.f32_(0x224), rec.f32_(0x22c))?.0 < 0.01 {
                match logic {
                    7 => self.blocked07(i, 10, 0x3c),
                    0 => self.blocked00(i),
                    8 | 10 | 0x14 => return unsupported(format!("blocked handler for logic {logic}")),
                    _ => self.dead_end(i, 0x14),
                }
                self.enemies[i].set_i16(0x212, 1);
            }
            dy = self.ef(i, 0x2a8);
        }
        dy = sub(dy, self.ef(i, 0x44))?;
        if dy < -5.0 {
            self.damage_enemy(f32::from_bits(0x47c3_4f80), i, -1, 0, None, None, 0)?;
        }
        if dy < floor_min {
            dy = floor_min;
        }
        let rec = &mut self.enemies[i];
        rec.set_f32(0x228, add(dy, rec.f32_(0x228))?);
        rec.set_f32(0x2a8, add(dy, rec.f32_(0x44))?);
        Ok(wall)
    }

    /// find_closest_item 0x154568 (radius, position, kind filter; kind 4
    /// skips placed enemies already activated, +0xf8 bit 1) over
    /// StartItemGrid(position, radius): the nearest blocking item (+0xd8 &
    /// 0x4040) with its xz distance less the definition radius (third
    /// argument), and the nearest distance over every item that passed the
    /// filters, blocking or not (fourth argument).
    pub(crate) fn find_closest_item_kind(&self, r: f32, p: V3, kind: i32) -> R<(Option<usize>, f32, f32)> {
        let mut best = 100_000.0;
        let mut any = 100_000.0;
        let mut found = None;
        for slot in self.grid.items_near(&self.items, r, p)? {
            let rec = &self.items.records[slot];
            if !rec.active() || rec.u16_(0xd8) & 0x8100 != 0 {
                continue;
            }
            let d = &self.items.definitions[rec.definition()];
            if d.i16_(8) == 0 {
                continue;
            }
            if kind != 0 && (d.kind() != kind || (kind == 4 && rec.u32_(0xf8) & 1 != 0)) {
                continue;
            }
            if d.kind() == -1 || rec.u8_(0xe1) != 0 || self.skip_item(slot) {
                continue;
            }
            let ip = rec.v3(0x60);
            let dist = sub(fqdist(sub(ip[0], p[0])?, sub(ip[2], p[2])?)?.0, d.f32_(0xc))?;
            if dist < any {
                any = dist;
            }
            if rec.u16_(0xd8) & 0x4040 != 0 && dist < best {
                best = dist;
                found = Some(slot);
            }
        }
        Ok((found, best, any))
    }

    /// DoEnemyItemCollide(enemy, from, to, moving).
    fn do_enemy_item_collide(&mut self, i: usize, from: V3, to: V3, moving: bool) -> R<bool> {
        let r = mul(self.ef(i, e::WIDTH), 0.5)?;
        let h = mul(r, 1.5)?;
        let env = self.collide_env();
        let mut item = None;
        let mut check = !moving && self.enemies[i].u32_(0x2a0) == 0;
        if !moving && !check {
            let s = self.enemies[i].u32_(0x2a0) as usize - 1;
            if 0.0 <= item_coll_sub(&self.items, &mut self.collision, &env, s, from, to, None, r, h)? {
                item = Some(s);
            }
        }
        if moving {
            let v = self.enemies[i].i32_(0x340) - self.globals.vb_elapsed;
            self.enemies[i].set_i32(0x340, v);
            check = true;
        }
        if check && self.enemies[i].i32_(0x340) < 1 {
            // find_closest_item(r, to, 0, NULL, &any): the delay uses the
            // nearest item of any kind; the collision test the blocking one.
            let (found, _, dist) = self.find_closest_item_kind(r, to, 0)?;
            let d = sub(dist, r)?;
            if 0.0 < d {
                let v = mul(mul(self.ef(i, 0xcc), 0.5)?, d)? as i32;
                self.enemies[i].set_i32(0x340, v.min(0x1e));
            }
            item = match found {
                Some(s) if 0.0 <= item_coll_sub(&self.items, &mut self.collision, &env, s, from, to, None, r, h)? => Some(s),
                _ => None,
            };
        }
        let blocked = match item {
            Some(s) => self.enemy_item_collision(Some(i), s)?,
            None => false,
        };
        self.enemies[i].set_u32(0x2a0, if blocked { item.unwrap() as u32 + 1 } else { 0 });
        Ok(blocked)
    }

    /// do_enemy_attack's Death (0x1e) branch: every 3 updates drain the
    /// touched player (health -(+0xd0), or one level-delta of experience for
    /// level 2) from the +0x214 pool; Death leaves (enemy_dies) when it is
    /// spent. StartDeathFX keeps its effect in +0x1f4.
    fn death_touch(&mut self, i: usize) -> R<()> {
        if 0 < self.enemies[i].i32_(0x220) {
            return Ok(());
        }
        let target = self.enemies[i].i32_(0x298);
        let Ok(p) = usize::try_from(target) else { return Ok(()) };
        if self.players[p].u32_(0x134) & 0x8_0000 != 0 {
            return Ok(());
        }
        let wait = self.enemies[i].i16_(0x2e8);
        if 0 < wait {
            self.enemies[i].set_i16(0x2e8, wait - 1);
            return Ok(());
        }
        self.request_enemy_action(i, 0xc);
        let t = self.enemies[i].i32_(0x21c) - self.globals.vb_elapsed;
        self.enemies[i].set_i32(0x21c, t);
        if 0 < t {
            return Ok(());
        }
        self.enemies[i].set_i32(0x21c, t + 3);
        let level2 = self.enemies[i].i16_(0x21a) == 2;
        let hit = if level2 {
            self.add_exp(p, -1, -2)?;
            0
        } else {
            let d = -self.ef(i, 0xd0);
            self.damage_player(d, p, 1, 0x1000, None)?
        };
        self.help(if level2 { 0x80 } else { 0x82 }, p);
        self.did_action = true;
        let n = self.enemies[i].i16_(0x2e2).wrapping_add(1);
        self.enemies[i].set_i16(0x2e2, n);
        if hit == 0 {
            if self.enemies[i].i32_(0x1f4) < 0 {
                let fx = self.start_death_fx(i32::from(self.enemies[i].i16_(0x21a)))?;
                self.enemies[i].set_i32(0x1f4, fx);
            }
        } else {
            self.enemies[i].set_f32(0x214, 0.0);
        }
        let left = sub(self.ef(i, 0x214), self.ef(i, 0xd0))?;
        self.enemies[i].set_f32(0x214, left);
        let at = self.enemies[i].v3(0x40);
        if 0.0 <= left {
            // do_death_sound(Death +0x40): the 0x66 loop panned at Death.
            self.do_death_sound(at);
            self.death_touch = true;
        } else {
            self.enemies[i].set_i32(0x334, 1);
            self.sfx(0x68, Some(at), 0xe0, 0xc);
            self.enemy_dies(i, target as i16)?;
        }
        Ok(())
    }

    /// do_enemy_attack(enemy): queue an attack on the touched player.
    fn do_enemy_attack(&mut self, i: usize) -> R<()> {
        let t = self.enemies[i].i32_(e::TYPE);
        if t == 0x1e {
            return self.death_touch(i);
        }
        if t == 0x1f {
            // The "It" enemy tags its target: the old It's +0x92c cleared,
            // It = the target (+0x298) with +0x92c = 1, NewHelp 0x32 at the
            // player, and the tagger dies.
            if let Ok(old) = usize::try_from(self.globals.it) {
                self.players[old].set_u16(0x92c, 0);
            }
            let it = self.enemies[i].i32_(0x298);
            self.globals.it = it;
            let Ok(p) = usize::try_from(it) else { return Ok(()) };
            self.players[p].set_u16(0x92c, 1);
            let at = self.players[p].v3(0x60);
            let who = self.players[p].u32_(0) as usize;
            self.help_at(0x32, who, Some(at));
            return self.enemy_dies(i, it as i16);
        }
        if self.enemies[i].i16_(e::LOGIC) == 0x1f {
            return Ok(());
        }
        let target = self.enemies[i].i32_(0x298) as i16;
        self.enemies[i].set_i16(0x2e0, target);
        let a = if self.enemies[i].u16_(0x2e2) & 7 != 7 { 0xc } else { 0x10 };
        self.request_enemy_action(i, a);
        Ok(())
    }

    /// do_enemy_move(enemy).
    pub fn do_enemy_move(&mut self, i: usize) -> R<()> {
        let logic = self.enemies[i].i16_(e::LOGIC);
        let r = self.ef(i, e::WIDTH);
        let h = self.ef(i, e::HALF_HEIGHT);
        let spf = self.globals.spf;
        {
            let rec = &mut self.enemies[i];
            if 0 < rec.i32_(0x220) {
                rec.set_i32(0x220, rec.i32_(0x220) - self.globals.vb_elapsed);
                for o in [0x224, 0x228, 0x22c] {
                    rec.set_u32(o, 0);
                }
            }
            if 0x1b < rec.i32_(0xe0) {
                for o in [0x224, 0x228, 0x22c] {
                    rec.set_u32(o, 0);
                }
            }
            for n in 0..3 {
                let o = 0x224 + 4 * n;
                rec.set_f32(o, add(mul(rec.f32_(0x270 + 4 * n), spf)?, rec.f32_(o))?);
            }
        }
        let speed = fqdist(self.ef(i, 0x224), self.ef(i, 0x22c))?.0;
        self.enemies[i].set_i16(0x294, i16::from(f32::from_bits(0x3a83_126f) < speed));
        let wall = self.do_enemy_collide(speed, i)?;
        let start = self.enemies[i].v3(e::POSITION);
        let mv = self.enemies[i].v3(0x224);
        let moved = [add(start[0], mv[0])?, add(start[1], mv[1])?, add(start[2], mv[2])?];
        self.enemies[i].set_v3(e::POSITION, moved);
        let from = self.enemies[i].v3(e::COLLISION);
        let mut to = [add(from[0], mv[0])?, add(from[1], mv[1])?, add(from[2], mv[2])?];
        let reset = |g: &mut Game| -> R<()> {
            let rec = &mut g.enemies[i];
            rec.set_i16(0x294, 0);
            rec.set_v3(e::POSITION, start);
            for o in [0x224, 0x228, 0x22c] {
                rec.set_u32(o, 0);
            }
            g.enemy_col_offset(i)
        };
        let touched = if self.enemies[i].i16_(0x2f0) == 0 {
            -1
        } else {
            self.enemy_collide_player(add(r, 0.5)?, h, i, false, from, to)?
        };
        self.enemies[i].set_i32(0x298, touched);
        let mut stopped = false;
        if touched < 0 {
            if t_is(self, i, 0x1e) {
                self.enemies[i].set_i32(0x1f4, -1);
            }
            let left = self.enemies[i].i32_(0x21c);
            if 0 < left {
                let v = left - self.globals.vb_elapsed;
                self.enemies[i].set_i32(0x21c, v.max(0));
            }
            let other = self.enemy_collide_enemy(r, h, i, from, to, wall == 0)?;
            self.enemies[i].set_i32(0x29c, other);
            if 0 <= other {
                self.enemies[i].set_u32(0x2a0, 0);
                let pushed = if other < 0x10000 {
                    self.enemies[other as usize].set_i32(0x29c, i as i32);
                    Some(other as usize)
                } else {
                    None
                };
                // The original's slide-along-enemy branch reads a flag that
                // EnemyCollideEnemy never writes, so it is unreachable.
                let push_target = pushed.filter(|_| 1.0 < self.ef(i, 0x280) && 0x1c <= self.enemies[i].i32_(0xe0));
                if let Some(o) = push_target {
                    for n in 0..3 {
                        let k = mul(self.ef(i, 0x270 + 4 * n), 0.5)?;
                        let v = add(k, self.ef(o, 0x270 + 4 * n))?;
                        self.enemies[o].set_f32(0x270 + 4 * n, v);
                        let m = mul(self.ef(i, 0x224 + 4 * n), 0.5)?;
                        self.enemies[o].set_f32(0x224 + 4 * n, m);
                    }
                } else {
                    stopped = true;
                    reset(self)?;
                }
                self.enemy_col_offset(i)?;
                match pushed {
                    None => self.dead_end(i, 0x14),
                    Some(o) if logic == 0 => {
                        let d = get_turn_dir(self.enemies[i].v3(e::POSITION), self.enemies[o].v3(0x40));
                        self.enemies[i].set_i32(0x368, d);
                        self.dead_end(i, 0x3c);
                    }
                    Some(o) if (logic as u16).wrapping_sub(7) < 2 || logic == 10 || logic == 0x14 => {
                        let cur = self.enemies[i].i32_(0x368);
                        if cur == 0 || 2 < cur.abs() {
                            let d = get_turn_dir(self.enemies[i].v3(e::POSITION), self.enemies[o].v3(0x40));
                            self.enemies[i].set_i32(0x368, d);
                            self.enemies[i].set_i16(0x378, 0);
                        }
                        let (a, b) = match logic {
                            8 => (10, 0x3c),
                            0x14 => (10, 0x1e),
                            _ => (0xf, 0x32),
                        };
                        self.blocked(i, logic, a, b)?;
                    }
                    Some(_) => self.dead_end(i, 0x14),
                }
                self.enemies[i].set_i16(0x212, 2);
            }
            if !stopped {
                let moving = self.enemies[i].i16_(0x294) != 0;
                if self.do_enemy_item_collide(i, from, to, moving)? {
                    stopped = true;
                    reset(self)?;
                    let item = self.enemies[i].u32_(0x2a0) as usize - 1;
                    let has_node = true; // every placed item has a render node here
                    if logic == 0 {
                        if has_node {
                            let d = get_turn_dir(self.enemies[i].v3(e::POSITION), self.items.records[item].v3(0x40));
                            self.enemies[i].set_i32(0x368, d);
                        }
                        self.dead_end(i, 0x3c);
                    } else if (logic as u16).wrapping_sub(7) < 2 || logic == 10 || logic == 0x14 {
                        let cur = self.enemies[i].i32_(0x368);
                        if cur == 0 || 2 < cur.abs() {
                            let d = get_turn_dir(self.enemies[i].v3(e::POSITION), self.items.records[item].v3(0x40));
                            self.enemies[i].set_i32(0x368, d);
                            self.enemies[i].set_i16(0x378, 0);
                        }
                        self.blocked(i, logic, 0xf, 0xf)?;
                    } else {
                        self.dead_end(i, 0x14);
                    }
                    self.enemies[i].set_i16(0x212, 3);
                }
                if !stopped {
                    if logic == 0 && self.enemies[i].i32_(0x36c) < 1 {
                        self.enemies[i].set_i32(0x368, 1);
                        self.enemies[i].set_i16(0x378, 0);
                    } else if logic == 2 || logic == 4 {
                        let v = self.enemies[i].i32_(0x388) - 1;
                        self.enemies[i].set_i32(0x388, v);
                        if v < 1 {
                            self.enemies[i].set_u32(0x370, 0);
                        }
                    }
                }
            }
        } else {
            self.enemies[i].set_i32(0x29c, -1);
            self.enemies[i].set_u32(0x2a0, 0);
            reset(self)?;
            let d = get_turn_dir(self.enemies[i].v3(e::POSITION), self.players[touched as usize].v3(0x50));
            self.enemies[i].set_i32(0x368, d);
            self.do_enemy_attack(i)?;
        }
        let _ = &mut to;
        if self.enemies[i].i32_(e::STATE) != 0 {
            if 0.1 < self.ef(i, 0x280) {
                let y = self.enemies[i].u32_(0x27c);
                self.enemies[i].set_u32(0x258, y);
            }
            let m = create_ypr_matrix(self.enemies[i].v3(0x254))?;
            let rec = &mut self.enemies[i];
            for (row, values) in m.iter().take(3).enumerate() {
                for (col, v) in values.iter().take(4).enumerate() {
                    rec.set_f32(0x10 + 16 * row + 4 * col, *v);
                }
            }
        }
        let rec = &mut self.enemies[i];
        let a = rec.i32_(0xe0);
        if rec.i16_(0x294) == 0 {
            if (a as u32).wrapping_sub(3) < 2 || a == 0 {
                rec.set_i16(0x296, rec.i16_(0x296).wrapping_add(self.globals.vb_elapsed as i16));
            } else {
                rec.set_i16(0x296, 0);
            }
        } else {
            rec.set_i16(0x296, 0);
        }
        if 0xb4 < rec.i16_(0x296) {
            rec.set_i16(0x296, 0);
        }
        if 0x3c < rec.i16_(0x296) && (rec.i32_(0xe4) as u32).wrapping_sub(3) < 2 && logic != 0x12 && rec.i32_(e::TYPE) != 0x1d {
            rec.set_i32(0xe4, 0);
        }
        Ok(())
    }

    /// UpdateObjColOffset for the enemy (+0x60 only).
    fn enemy_col_offset(&mut self, i: usize) -> R<()> {
        let rec = &mut self.enemies[i];
        let m: [V3; 3] = std::array::from_fn(|r| rec.v3(0x10 + 16 * r));
        let cp = crate::player_collision::collision_position(rec.v3(e::POSITION), rec.v3(e::COL_OFFSET), rec.u32_(e::OFFSET_FLAGS), &m)?;
        rec.set_v3(e::COLLISION, cp);
        Ok(())
    }

    /// DoEnemyAction: the enemy action state machine over its animation clock.
    pub fn do_enemy_action(&mut self, i: usize) -> R<u32> {
        let rec = &self.enemies[i];
        let cur = rec.u32_(0xe0);
        let req = rec.u32_(0xe4);
        let t = rec.i32_(e::TYPE);
        let table = |k: u32| -> i32 { rec.i32_(0xe8 + 8 * k as usize) };
        let mut mode: i32 = 2;
        let mut flag: i16 = 1;
        let mut next = req;
        let mut cur_out = cur;
        let state = if req < 0x1c { cur } else { 0 };
        // LAB_001796ec: the replacement applies when the probed table entry exists.
        let pick = |probe: i32, replacement: u32, fallback: u32| if 0 <= probe { replacement } else { fallback };
        match state {
            0 => {
                if t == 0x1d {
                    mode = if req < 0x1c { 0 } else { 2 };
                    flag = 0;
                }
                if t == 0x1b {
                    mode = 2;
                } else if req == 3 && 0 <= table(9) {
                    next = 9;
                    mode = 0;
                } else if req == 4 && 0 <= table(0xb) {
                    next = 0xb;
                    mode = 0;
                } else if req == 0xc || req == 0xe || req == 0x10 {
                    mode = 0;
                }
            }
            1 => {
                mode = 0;
                flag = 0;
                if matches!(t, 1 | 4 | 10 | 7) {
                    next = pick(table(3), 3, 4);
                }
            }
            2 | 0x1c | 0x1e | 0x1f | 0x20 => {
                mode = 0;
                flag = 0;
            }
            3 | 5 | 6 | 7 => {
                if t == 0x1d && req == 0 {
                    flag = 0;
                }
                if req == 0 {
                    if 0 <= table(8) {
                        next = 8;
                    }
                    mode = 0;
                }
            }
            4 => {
                if req == 0 && 0 <= table(10) {
                    next = 10;
                    mode = 0;
                }
            }
            8 | 10 => {
                if req < 0x1c {
                    mode = 0;
                    next = 0;
                    flag = 0;
                }
            }
            9 | 0xb => {
                if req < 0x1c {
                    mode = 0;
                    flag = 0;
                    next = if state == 9 { pick(table(3), 3, 4) } else { pick(table(4), 4, 3) };
                }
            }
            0xc | 0xd | 0xe | 0xf | 0x10 | 0x12 | 0x14 | 0x16 | 0x17 | 0x1a => {
                flag = 0;
                if 0x1b < req {
                    mode = 2;
                } else {
                    mode = 0;
                    next = match state {
                        0xc => 0xd,
                        0xd if 0 <= table(14) => 0xe,
                        0xd | 0xf => {
                            if req == 0xc {
                                0xc
                            } else {
                                req
                            }
                        }
                        0xe => 0xf,
                        0x10 => 0x11,
                        0x12 => 0x13,
                        0x14 => 0x15,
                        0x16 => {
                            if req == 0x16 {
                                0x17
                            } else {
                                req
                            }
                        }
                        0x17 => {
                            if req == 0x16 {
                                0x16
                            } else {
                                req
                            }
                        }
                        _ => match req {
                            0x18 => 0x19,
                            0 => 0x1b,
                            r => r,
                        },
                    };
                }
            }
            0x11 | 0x1b => {
                if state == 0x1b {
                    next = 0;
                }
                mode = if req < 0x1c { 0 } else { 2 };
                flag = 0;
            }
            0x13 | 0x15 => {
                if req == 0 {
                    next = pick(rec.i32_(0x298), 0xc, 0);
                }
            }
            0x18 | 0x19 => {
                if state == 0x18 && table(24) < 0 {
                    next = 0x19;
                    cur_out = 0x19;
                }
                flag = 0;
                if req < 0x1c {
                    mode = 0;
                    next = 0x1a;
                } else {
                    mode = 2;
                }
            }
            0x1d => {
                mode = 0;
                flag = 0;
                next = pick(table(31), 0x1f, req);
            }
            _ => {}
        }
        // Second switch (fallbacks when the chosen clip is missing).
        let fallback = match next.wrapping_sub(3) {
            0 | 8 => Some(4),
            1 | 6 => Some(3),
            5 | 7 | 0x18 => {
                if table(next) < 0 {
                    next = 0;
                }
                None
            }
            0xb | 0xd | 0xf | 0x11 => Some(0xc),
            0x16 => Some(0x18),
            _ => None,
        };
        if let (Some(f), true) = (fallback, table(next) < 0) {
            next = f;
        }
        let mut clip = table(next);
        if clip < 0 {
            if next == 0x20 {
                clip = rec.i32_(0x1d0);
            }
            if clip < 0 {
                clip = if 0 <= table(0) { table(0) } else { 0 };
                if mode != 0 {
                    mode = 2;
                }
                flag = 1;
            }
        }
        // AnimInfo +0x34 (sh t0 at 0x179888) before AnimateATree: the loop
        // word of the native clock too (InitAnim replaces it with the clip's
        // repeat flag when the sequence changes).
        self.enemies[i].set_i16(0xb8, flag);
        if let Some(Some(a)) = self.enemy_anims.get_mut(i) {
            a.clock.loop_word = flag as u16;
        }
        let switched = self.animate_enemy(i, clip as u16, mode)?;
        if !switched {
            return Ok(cur_out);
        }
        let rec = &mut self.enemies[i];
        let landed = match cur_out {
            0xc | 0xe | 0x12 | 0x14 => matches!(next, 0xd | 0xf | 0x13 | 0x15).then_some(1),
            0x10 => (next == 0x11).then_some(2),
            0x16 => (next == 0x17).then_some(0x10),
            0x18 | 0x19 => (next == 0x1a).then_some(0x10),
            _ => None,
        };
        if let Some(bit) = landed {
            rec.or_u32(0x2e4, bit);
        }
        if (next.wrapping_sub(0x18)) < 3 {
            let lc = self.level_f32(0xc0);
            let rec = &self.enemies[i];
            let mut f = add(mul(rec.f32_(0x38c), lc)?, rec.f32_(0x394))?;
            if 0.0 < f {
                let mut whole = 0.0f32;
                while 1.0 < f {
                    whole = add(whole, 1.0)?;
                    f = sub(f, 1.0)?;
                }
                let rec = &mut self.enemies[i];
                rec.set_f32(0x394, f);
                rec.set_f32(0x390, whole);
                if 1.0 <= whole {
                    let v = add(mul(f32::from(rec.i16_(0x94)), f32::from_bits(0x3d08_8889))?, whole)?;
                    rec.set_f32(0x390, v);
                }
            }
        }
        Ok(next)
    }

    /// AnimateATree on the native clock; true when the requested clip took over.
    pub(crate) fn animate_enemy(&mut self, i: usize, clip: u16, mode: i32) -> R<bool> {
        let Some(anim) = self.enemy_anims.get(i).cloned().flatten() else {
            return unsupported("enemy animation clock not initialised");
        };
        let current = *anim.clips.get(anim.current as usize).ok_or_else(|| super::Unsupported("current clip".into()))?;
        let requested = *anim.clips.get(clip as usize).ok_or_else(|| super::Unsupported("requested clip".into()))?;
        let out = select(
            anim.clock,
            current,
            requested,
            SelectionInput {
                sequence: clip,
                mode,
                start_frame: 0,
                blend_duration: 0.0,
                playback_scale: 1.0,
                now: self.globals.atime,
                anim_time_scale: self.globals.anim_time_scale,
            },
        )?;
        let switched = out.return_flags & 1 != 0;
        let animated = self.enemy_anim_data.tree_animated.contains(&anim.tree);
        let a = self.enemy_anims[i].as_mut().unwrap();
        a.clock = out.state;
        // DoAnimation 0x10fd10: a nonzero reset byte becomes 2 for trees with
        // a type-1 node (trace-k1-newgame-v1's ACI/PLA; G1's OANIM trees not).
        if animated && a.clock.reset_byte != 0 {
            a.clock.reset_byte = 2;
        }
        if switched {
            a.current = clip;
            // InitAnim: AnimInfo +0x10 (+0x94) = the sequence's frame count
            // (trace-fight-g1-v3 steps 250/264: 0xc → 0xf on seq 6 → 1).
            self.enemies[i].set_i16(0x94, requested.frames);
        }
        Ok(out.return_flags != 0)
    }

    /// do_enemies (game_mode 0x4010 path with the camera visibility supplied).
    pub fn do_enemies(&mut self, visible: &dyn Fn(&Game, usize) -> (bool, bool)) -> R<()> {
        self.process_critter_list()?;
        if 0 <= self.globals.boss_index {
            let b = self.globals.boss_index as usize;
            self.enemies[b].set_i32(e::STATE, 1);
        }
        if self.globals.paused != 0 || self.globals.help != 0 {
            return Ok(());
        }
        if self.globals.cam_wait != 0 {
            if self.globals.start_of_stage != 0 {
                // Stage start: live non-boss enemies play their idle action
                // (type 0x1d → 1, type 0 → 3, else 0) without AI.
                for k in 0..self.enemies.len() {
                    let t = self.enemies[k].i32_(e::TYPE);
                    if self.enemies[k].i32_(e::STATE) != 1 || t == self.globals.boss_type {
                        continue;
                    }
                    self.enemies[k].set_u32(0xe4, match t {
                        0x1d => 1,
                        0 => 3,
                        _ => 0,
                    });
                    if self.enemies[k].u32_(0x80) != 0 {
                        let a = self.do_enemy_action(k)?;
                        self.enemies[k].set_u32(0xe0, a);
                    }
                }
            }
            return Ok(());
        }
        for pl in self.players.iter_mut() {
            if pl.i32_(0xfc) == 1 {
                pl.set_u32(0x940, 0);
                pl.set_u32(0x944, 0);
            }
        }
        let scale = self.level_f32(0xb0);
        let vb = self.globals.vb_elapsed as f32;
        // The original runs 45 entries over a 34-entry table; types >= 34 are bosses.
        self.fwrdval.resize(self.enemy_tables.speed.len(), 0.0);
        for k in 0..self.enemy_tables.speed.len() {
            self.fwrdval[k] = mul(mul(vb, scale)?, self.enemy_tables.speed[k])?;
        }
        self.set_suicide_bomber_index();
        self.globals.num_visible_enemies = 0;
        for k in 0..self.enemies.len() {
            if self.enemies[k].i32_(e::STATE) != 0 {
                let (v, near) = visible(self, k);
                self.enemies[k].set_i16(0x2ee, i16::from(v));
                self.enemies[k].set_i16(0x2f0, i16::from(near));
                if v {
                    self.globals.num_visible_enemies += 1;
                }
            }
        }
        for k in 0..self.enemies.len() {
            let logic = self.enemies[k].i16_(e::LOGIC);
            {
                let rec = &mut self.enemies[k];
                rec.set_i16(0x326, logic);
                rec.set_i32(0x380, rec.i32_(0x380) + self.globals.vb_elapsed);
                if 0.0 < rec.f32_(0x390) {
                    rec.set_f32(0x390, sub(rec.f32_(0x390), self.globals.spf)?);
                }
                let t = rec.i32_(e::TYPE);
                rec.set_u32(0xe4, if t == 0 { 3 } else { 0 });
            }
            match self.enemies[k].i32_(e::STATE) {
                1 | 7 => {
                    if self.enemies[k].i32_(e::TYPE) != self.globals.boss_type {
                        self.enemy_get_obj_world_mat(k)?;
                        if self.stop_time != 0 {
                            // StopTime: the animation start (+0xa4) moves with
                            // time, freezing the pose; knockback and collision run.
                            let st = add(self.ef(k, 0xa4), self.globals.spf)?;
                            self.enemies[k].set_f32(0xa4, st);
                            if let Some(Some(an)) = self.enemy_anims.get_mut(k) {
                                an.clock.start_time = add(an.clock.start_time, self.globals.spf)?;
                            }
                            self.enemy_knockback_control(k)?;
                            self.do_enemy_collide(0.0, k)?;
                        } else if self.globals.trig_cam_active != 0 {
                            // Trigger camera: action 0 played out, no AI.
                            self.enemies[k].set_u32(0xe4, 0);
                            let a = self.do_enemy_action(k)?;
                            self.enemies[k].set_u32(0xe0, a);
                            self.do_enemy_collide(0.0, k)?;
                            self.enemy_knockback_control(k)?;
                        } else {
                            self.find_closest_player(k)?;
                            self.enemy_attack_control(k)?;
                            self.enemy_knockback_control(k)?;
                            if self.enemy_ai_control(k)? {
                                // 0x13b8f8: straight to the common tail (no
                                // skin effects, no UpdateObjWorldMat).
                                self.do_enemies_tail(k)?;
                                continue;
                            }
                            let a = self.do_enemy_action(k)?;
                            self.enemies[k].set_u32(0xe0, a);
                            let v = if a != self.enemies[k].u32_(0xe4) { 0 } else { -1 };
                            self.enemies[k].set_i32(0x3a4, v);
                        }
                        self.enemy_process_skin_fx(k)?;
                        self.enemy_update_obj_world_mat(k)?;
                    }
                }
                // GetObjWorldMat (the attention/collision points) for states
                // 6 and 8 too, before the type checks (0x13b8c0, 0x13b8f0).
                6 => self.enemy_get_obj_world_mat(k)?,
                8 => {
                    self.enemy_get_obj_world_mat(k)?;
                    if self.enemies[k].i32_(e::TYPE) == self.globals.boss_type {
                        // boss shadow follows; presentation only
                    } else if self.enemies[k].i32_(e::TYPE) == 0x1e {
                        // Death leaving: its touch effect is deleted, it rises
                        // (y += 10 spf) while the +0x39c alpha counts to 0xff,
                        // then help 0x81/0x83 if it drained someone and kill.
                        let alpha = self.enemies[k].i32_(0x39c);
                        if let Ok(fx) = usize::try_from(self.enemies[k].i32_(0x1f4)) {
                            self.delete_effect_pub(fx);
                            self.enemies[k].set_i32(0x1f4, -1);
                        }
                        if alpha < 0xff {
                            if let Some(a) = self.enemy_alpha.get_mut(k) {
                                *a = alpha;
                            }
                            self.enemies[k].set_i32(0x39c, alpha + 4 * self.globals.vb_elapsed);
                            let y = add(mul(self.globals.spf, 10.0)?, self.ef(k, 0x44))?;
                            self.enemies[k].set_f32(0x44, y);
                            self.enemy_update_obj_world_mat(k)?;
                        } else {
                            if self.enemies[k].i32_(0x334) != 0 {
                                let h = if self.enemies[k].i16_(0x21a) == 2 { 0x81 } else { 0x83 };
                                let p = self.enemies[k].i32_(0x298).max(0) as usize;
                                self.help(h, p);
                            }
                            self.kill_enemy(k)?;
                        }
                    } else {
                        self.enemy_knockback_control(k)?;
                        self.enemies[k].set_u32(0xe4, 0x20);
                        let y = self.turn_enemy_ang(self.ef(k, 0x260), k)?;
                        self.enemies[k].set_f32(0x258, y);
                        self.do_enemy_move(k)?;
                        // 0x13bad4..0x13bb10: with a tree (+0x80), the action
                        // (not for BossType) and +0x3a4 = 0 when it differs
                        // from +0xe4, else −1 (trace-b1-newgame-v2 step 404).
                        if self.enemies[k].u32_(0x80) != 0 {
                            if self.enemies[k].i32_(e::TYPE) != self.globals.boss_type {
                                let a = self.do_enemy_action(k)?;
                                self.enemies[k].set_u32(0xe0, a);
                            }
                            let v = if self.enemies[k].u32_(0xe0) != self.enemies[k].u32_(0xe4) { 0 } else { -1 };
                            self.enemies[k].set_i32(0x3a4, v);
                        }
                        let a = self.enemies[k].u32_(0xe0);
                        // The dying enemy stays while its death skin runs
                        // (actions 0x1c/0x1d/0x20).
                        self.enemy_process_skin_fx(k)?;
                        let still = (a.wrapping_sub(0x1c) < 2 || a == 0x20) && 0.0 < self.enemies[k].f32_(0x1f8);
                        if !still {
                            if self.enemies[k].i32_(e::TYPE) == 0x1d {
                                let r = self.rng.rand_int(2).ok_or_else(|| super::Unsupported("RandInt(2)".into()))?;
                                // AudioSuchBravery / AudioHeroicEffort(+0x212).
                                let who = i32::from(self.enemies[k].i16_(0x212));
                                self.audio_praise(r != 0, who);
                            }
                            self.kill_enemy(k)?;
                        } else {
                            self.enemy_update_obj_world_mat(k)?;
                        }
                    }
                }
                _ => {}
            }
            self.do_enemies_tail(k)?;
        }
        Ok(())
    }

    /// do_enemies' per-enemy tail (0x13bbd4): the logic words, the brain
    /// period and the decaying push velocity.
    fn do_enemies_tail(&mut self, k: usize) -> R<()> {
        let rec = &mut self.enemies[k];
        let used = rec.i16_(e::LOGIC);
        rec.set_i16(0x328, used);
        rec.set_i16(e::LOGIC, rec.i16_(0x326));
        if rec.i32_(0x37c) <= rec.i32_(0x380) {
            rec.set_i32(0x380, rec.i32_(0x380) - rec.i32_(0x37c));
        }
        for n in 0..3 {
            let o = 0x270 + 4 * n;
            let v = mul(rec.f32_(o), 0.8)?;
            rec.set_f32(o, if v.abs() < 0.01 { 0.0 } else { v });
        }
        if 0.0 < rec.f32_(0x274) {
            let v = sub(rec.f32_(0x274), mul(self.globals.spf, 100.0)?)?;
            rec.set_f32(0x274, if v < 0.0 { 0.0 } else { v });
        }
        Ok(())
    }
}

fn t_is(g: &Game, i: usize, t: i32) -> bool {
    g.enemies[i].i32_(e::TYPE) == t
}

/// get_turn_dir(a, b).
pub fn get_turn_dir(a: V3, b: V3) -> i32 {
    if (a[2] - b[2]).abs() <= (a[0] - b[0]).abs() {
        if b[2] <= a[2] { -1 } else { 1 }
    } else if a[0] < b[0] {
        -1
    } else {
        1
    }
}
