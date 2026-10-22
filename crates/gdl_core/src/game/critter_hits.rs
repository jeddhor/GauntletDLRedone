//! Critter attacks and damage: CritterDoDamage 0x194ed0, CritterDoSfx
//! 0x194270, CritterDoDamageFX 0x1946c8, CritterNodePlayerCollide 0x19bae8,
//! CritterDamagePlayer 0x19b910, CritterDamage 0x199100, ModifyDamage
//! 0x190b88 and the effect hit tests (CritterLineCollide 0x19a810,
//! CritterExpCollide 0x19a260). Effects parented to a critter node are
//! spawned at the node's world position and do not follow it afterwards
//! (labelled approximation); morph, colour, scale and texture work is
//! presentation.
use super::critter_ai::{pitch_vec3, yaw_vec3};
use super::critters::{c, mul_vec_mat3, CritterFile};
use super::damage::pd;
use super::effects::{calc_target_dir, fx, Visual};
use super::{unsupported, Game, Unsupported, R};
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sub_guarded_rz as sub};

type V3 = [f32; 3];

fn v3(r: &[u8], o: usize) -> V3 {
    [CritterFile::f32_pub(r, o), CritterFile::f32_pub(r, o + 4), CritterFile::f32_pub(r, o + 8)]
}

impl Game {
    fn node_rows(&self, i: usize, at: usize) -> [V3; 3] {
        let r = &self.crit(i).rec;
        std::array::from_fn(|k| r.v3(at + 16 * k))
    }

    /// CritterDoDamage 0x194ed0.
    pub(crate) fn critter_do_damage(&mut self, i: usize, d: usize, which: i32, first: bool) -> R<()> {
        let drec = self.critter_damage_rec(i, d);
        match CritterFile::i16_pub(&drec, 0) {
            0 => {
                self.critter_node_player_collide(i, &drec, true)?;
                self.critter_node_enemy_collide(i, &drec)?;
                if first {
                    self.critter_do_damage_fx(i, d, true, None)?;
                }
            }
            1 => {
                if first {
                    let base = self.crit(i).rec.v3(c::NODE_POS);
                    self.critter_do_damage_fx(i, d, false, Some(base))?;
                }
            }
            2 | 3 => {
                if first {
                    self.critter_do_damage_fx(i, d, true, None)?;
                }
            }
            4 => {
                self.critter_fire_player_collide(i, &drec)?;
                if first {
                    self.critter_do_damage_fx(i, d, true, None)?;
                }
            }
            5 => {
                // One effect per safe rock, parented to the rock's node.
                if first && 0 < self.safe_rock_num {
                    for k in 0..self.safe_rock_items.len() {
                        let fxk = self.critter_do_damage_fx(i, d, false, Some([0.0; 3]))?;
                        if 0 <= fxk {
                            self.parent_fx_to_item(fxk as usize, self.safe_rock_items[k])?;
                        }
                    }
                }
            }
            6 => {
                if first && self.safe_rock_num != 0 {
                    let target = self.crit(i).rec.i16_(c::TARGET);
                    self.safe_rock_idx = self.safe_rock_nearest_target(target)?;
                    if let Ok(idx) = usize::try_from(self.safe_rock_idx) {
                        let fxk = self.critter_do_damage_fx(i, d, false, Some([0.0; 3]))?;
                        if 0 <= fxk {
                            self.parent_fx_to_item(fxk as usize, self.safe_rock_items[idx])?;
                            // CriterSfxNumFrames: the effect tree's frame count.
                            let kind = self.fx(fxk as usize).u32_(fx::KIND) as usize;
                            let frames = self.effects.tables.headers.get(kind).copied().flatten().map_or(-1, |h| h.frames);
                            self.safe_rock_counter[idx] = mul((frames - 1) as f32, f32::from_bits(0x3d08_8880))?;
                        }
                    }
                }
            }
            7 => {
                if which == 1 {
                    self.critter_grab_player(i, &drec)?;
                    if first {
                        let base = self.crit(i).rec.v3(c::NODE_POS);
                        self.critter_do_damage_fx(i, d, false, Some(base))?;
                    }
                } else if which == 2 {
                    self.critter_release_player(i, &drec)?;
                }
            }
            8 => {
                if first {
                    let base = self.crit(i).rec.v3(c::TARGET_POS);
                    self.critter_do_damage_fx(i, d, false, Some(base))?;
                }
            }
            9 => {
                if first {
                    // acosf(DAMG +0x18) spread; HOME forward yawed/pitched × +0x30.
                    let spread = crate::libm::acosf(CritterFile::f32_pub(&drec, 0x18)).map_err(|_| Unsupported("acosf".into()))?;
                    let f = self.crit(i).rec.v3(c::HOME_MATRIX + 0x20);
                    let f = super::critter_ai::yaw_vec3(CritterFile::f32_pub(&drec, 0x14), f)?;
                    let f = pitch_vec3(CritterFile::f32_pub(&drec, 0x1c), f)?;
                    let s = CritterFile::f32_pub(&drec, 0x30);
                    let v = [mul(f[0], s)?, mul(f[1], s)?, mul(f[2], s)?];
                    let pos = self.crit(i).rec.v3(c::NODE_POS);
                    self.boss_spew_coins(spread, pos, v)?;
                }
            }
            _ => {
                if first {
                    self.critter_do_damage_fx(i, d, true, None)?;
                }
            }
        }
        Ok(())
    }

    /// CritterNodePlayerCollide 0x19bae8: point test of the transition node
    /// (+ DAMG +0x20 offset) against each player's cylinder; returns the
    /// nearest player hit or -1, damaging every hit player when `damage`.
    fn critter_node_player_collide(&mut self, i: usize, drec: &[u8], damage: bool) -> R<i32> {
        let dmg = if damage { mul(CritterFile::f32_pub(drec, 0x2c), self.level_f32(0xbc))? } else { 0.0 };
        let r = CritterFile::f32_pub(drec, 0xc);
        let off = mul_vec_mat3(v3(drec, 0x20), &self.node_rows(i, c::NODE_MATRIX))?;
        let prev = self.crit(i).rec.v3(c::PREV_NODE_POS);
        let cur = self.crit(i).rec.v3(c::NODE_POS);
        let a0 = [add(prev[0], off[0])?, add(prev[1], off[1])?, add(prev[2], off[2])?];
        let at = [add(cur[0], off[0])?, add(cur[1], off[1])?, add(cur[2], off[2])?];
        let col = self.crit(i).rec.v3(c::COL);
        let time = self.globals.time;
        let mut best = 2e21f32;
        let mut pick = -1;
        for p in 0..4 {
            if self.players[p].i32_(pd::STATE) != 1 || !self.player_can_be_damaged(p) {
                continue;
            }
            if 0.0 < dmg && time < self.players[p].f32_(0x8c0) {
                continue;
            }
            let pc = self.players[p].v3(0x70);
            let (w, h) = (add(r, self.players[p].f32_(0x828))?, add(r, self.players[p].f32_(0x82c))?);
            if super::enemies::line_cylinder_collide(pc, at, at, w, h, false)?.is_none() {
                continue;
            }
            let mut a = [sub(at[0], a0[0])?, sub(at[1], a0[1])?, sub(at[2], a0[2])?];
            let mut b = [sub(pc[0], col[0])?, 1.0, sub(pc[2], col[2])?];
            crate::collision::normal_vector_pub(&mut a)?;
            let dd = crate::collision::normal_vector_pub(&mut b)?;
            if dd < best {
                best = dd;
                pick = p as i32;
            }
            if 0.0 < dmg {
                let dir = [
                    add(add(add(a[0], b[0])?, a[0])?, b[0])?,
                    add(add(add(a[1], b[1])?, a[1])?, b[1])?,
                    add(add(add(a[2], b[2])?, a[2])?, b[2])?,
                ];
                self.critter_damage_player(i, p, drec, 0, dir, true)?;
            }
        }
        Ok(pick)
    }

    /// CritterNodeEnemyCollide 0x19c8d8: the transition node's contact
    /// damage against enemies (StopTime frozen enemies are not modelled).
    fn critter_node_enemy_collide(&mut self, i: usize, drec: &[u8]) -> R<i32> {
        use super::enemies::e;
        let dmg = mul(CritterFile::f32_pub(drec, 0x2c), self.level_f32(0xbc))?;
        let r = CritterFile::f32_pub(drec, 0xc);
        let off = mul_vec_mat3(v3(drec, 0x20), &self.node_rows(i, c::NODE_MATRIX))?;
        let prev = self.crit(i).rec.v3(c::PREV_NODE_POS);
        let cur = self.crit(i).rec.v3(c::NODE_POS);
        let a0 = [add(prev[0], off[0])?, add(prev[1], off[1])?, add(prev[2], off[2])?];
        let at = [add(cur[0], off[0])?, add(cur[1], off[1])?, add(cur[2], off[2])?];
        let time = self.globals.time;
        let mut n = 0;
        for k in self.enemies_near(r, at)? {
            let st = self.enemies[k].i32_(e::STATE);
            if !(st == 1 || st == 6) || self.enemies[k].i32_(e::TYPE) == 0x1f {
                continue;
            }
            if 0.0 < dmg && time < self.enemies[k].f32_(0x2c8) {
                continue;
            }
            let (w, h) = (add(r, self.enemies[k].f32_(e::WIDTH))?, add(r, self.enemies[k].f32_(e::HALF_HEIGHT))?);
            let Some(p) = super::enemies::line_cylinder_collide(self.enemies[k].v3(e::COLLISION), at, at, w, h, false)? else { continue };
            let d = [add(sub(at[0], a0[0])?, sub(at[0], a0[0])?)?, add(sub(at[1], a0[1])?, sub(at[1], a0[1])?)?, add(sub(at[2], a0[2])?, sub(at[2], a0[2])?)?];
            self.damage_enemy(dmg, k, -1, 0, Some(p), Some(d), 1)?;
            n += 1;
        }
        Ok(n)
    }

    /// CritterFirePlayerCollide 0x19bd98: a cone/beam from the transition
    /// node along its forward (DAMG yaw +0x14, pitch +0x1c, range +0xc,
    /// radius +8, minimum +0x10), blocked by items (ItemLineCollide 0.5).
    fn critter_fire_player_collide(&mut self, i: usize, drec: &[u8]) -> R<()> {
        let range = CritterFile::f32_pub(drec, 0xc);
        let r = CritterFile::f32_pub(drec, 8);
        let minr = CritterFile::f32_pub(drec, 0x10);
        let mut f = self.crit(i).rec.v3(c::NODE_MATRIX + 0x20);
        crate::collision::normal_vector_pub(&mut f)?;
        let f = yaw_vec3(CritterFile::f32_pub(drec, 0x14), f)?;
        let f = pitch_vec3(CritterFile::f32_pub(drec, 0x1c), f)?;
        let off = mul_vec_mat3(v3(drec, 0x20), &self.node_rows(i, c::NODE_MATRIX))?;
        let np = self.crit(i).rec.v3(c::NODE_POS);
        let start = [add(np[0], off[0])?, add(np[1], off[1])?, add(np[2], off[2])?];
        let end = [add(mul(range, f[0])?, start[0])?, add(mul(range, f[1])?, start[1])?, add(mul(range, f[2])?, start[2])?];
        let time = self.globals.time;
        for p in 0..4 {
            if self.players[p].i32_(pd::STATE) != 1 || time < self.players[p].f32_(0x8c0) {
                continue;
            }
            let pc = self.players[p].v3(0x70);
            let dist = crate::vu0::fqdist(sub(pc[0], start[0])?, sub(pc[2], start[2])?)?.0;
            if dist < minr || range < dist {
                continue;
            }
            let (w, h) = (add(r, self.players[p].f32_(0x828))?, add(r, self.players[p].f32_(0x82c))?);
            if super::enemies::line_cylinder_collide(pc, start, end, w, h, false)?.is_none() {
                continue;
            }
            if 0 <= self.item_line_collide(0.5, start, pc, Some(pc), true)? {
                continue;
            }
            let mut dir = [sub(end[0], start[0])?, sub(end[1], start[1])?, sub(end[2], start[2])?];
            crate::item_collision::normal_vector_2d(&mut dir)?;
            self.critter_damage_player(i, p, drec, 0, dir, true)?;
        }
        Ok(())
    }

    /// CritterGrabPlayer 0x19c038 with PlayerSetGrabbed 0x136cf8 /
    /// PlayerSetParent 0x136c58: the player is parented to the transition
    /// node at DAMG +0x20 − player +0x810 (status +0x93c |= 0x20, +0x8ac |= 0x4000).
    fn critter_grab_player(&mut self, i: usize, drec: &[u8]) -> R<()> {
        if 0 <= self.crit(i).rec.i16_(c::GRABBED) {
            return Ok(());
        }
        let p = self.critter_node_player_collide(i, drec, false)?;
        let Ok(p) = usize::try_from(p) else { return Ok(()) };
        let o = v3(drec, 0x20);
        let co = self.players[p].v3(0x810);
        let local = [sub(o[0], co[0])?, sub(o[1], co[1])?, sub(o[2], co[2])?];
        let pos = self.players[p].v3(0x50);
        self.players[p].set_v3(0xf0, pos);
        self.players[p].or_u16(0x93c, 0x20);
        self.players[p].or_u32(0x8ac, 0x4000);
        let node = self.crit(i).node_dc;
        self.player_grab[p] = Some((i, node, local));
        self.crit_mut(i).rec.set_i16(c::GRABBED, p as i16);
        let s_ = CritterFile::i16_pub(drec, 0x42);
        if let Ok(s_) = usize::try_from(s_) {
            // The effect is parented to the player's node (presentation).
            self.critter_do_sfx(i, s_, None, false)?;
        }
        Ok(())
    }

    /// CritterReleasePlayer 0x19c0e8 with PlayerUnsetGrabbed 0x136df0:
    /// thrown along the critter's forward (−0.1 y) × DAMG +0x30.
    fn critter_release_player(&mut self, i: usize, drec: &[u8]) -> R<()> {
        let Ok(p) = usize::try_from(self.crit(i).rec.i16_(c::GRABBED)) else { return Ok(()) };
        self.player_grab[p] = None;
        self.players[p].and_u16(0x93c, 0xffdf);
        self.players[p].and_u32(0x8ac, 0xffff_bfff);
        let f = self.crit(i).rec.v3(c::MATRIX + 0x20);
        let mut d = [f[0], -0.1, f[2]];
        crate::collision::normal_vector_pub(&mut d)?;
        let k = CritterFile::f32_pub(drec, 0x30);
        let dir = [mul(d[0], k)?, mul(d[1], k)?, mul(d[2], k)?];
        self.critter_damage_player(i, p, drec, 0x8050, dir, false)?;
        self.crit_mut(i).rec.set_i16(c::GRABBED, -1);
        Ok(())
    }

    /// The grabbed player's world position this update (GetObjWorldMat of
    /// the parented node), or None when not grabbed.
    pub fn grabbed_position(&self, p: usize) -> R<Option<V3>> {
        let Some((i, node, local)) = self.player_grab[p] else { return Ok(None) };
        let Some(Some(_)) = self.critters.get(i) else { return Ok(None) };
        let m = match node {
            Some(n) => self.critter_node_world(i, n, Some(local))?,
            None => self.critter_root_pub(i),
        };
        Ok(Some([m[12], m[13], m[14]]))
    }

    /// CritterDamagePlayer 0x19b910.
    fn critter_damage_player(&mut self, i: usize, p: usize, drec: &[u8], extra: u32, dir: V3, sfx: bool) -> R<i32> {
        let mut flags = extra | CritterFile::i32_pub(drec, 4) as u32;
        let mut dmg = mul(CritterFile::f32_pub(drec, 0x2c), self.level_f32(0xbc))?;
        let hit = CritterFile::i16_pub(drec, 0x42);
        if sfx && 0 <= hit {
            let at = self.players[p].v3(0x50);
            self.critter_do_sfx(i, hit as usize, Some(at), false)?;
            flags |= 0x100_0000;
        }
        if self.critter_kind(i) != 4 && self.globals.enemy_shrink < 1.0 {
            dmg = mul(dmg, 0.5)?;
        }
        let r = self.damage_player(dmg, p, 1, flags, Some(dir))?;
        self.players[p].set_u32(0x8ec, 0);
        self.players[p].set_f32(0x8c0, add(self.globals.time, 0.25)?);
        self.critter_damaged_player_sub(i, p, dmg)?;
        Ok(r)
    }

    /// CritterDamagedPlayerSub 0x19b850.
    fn critter_damaged_player_sub(&mut self, i: usize, p: usize, dmg: f32) -> R<()> {
        let at = c::ANGER + 0x10 * p;
        let v = add(dmg, self.crit(i).rec.f32_(at))?;
        let time = self.globals.time;
        let r = &mut self.crit_mut(i).rec;
        r.set_f32(at, v);
        r.set_f32(at + 4, time);
        Ok(())
    }

    /// CritterDamagedPlayer 0x19b880 for an effect owned by critter `id`
    /// (ProcessEffects passes owner & 0xfff). The original passes whatever
    /// f12 holds after damage_player; the effect damage is used here
    /// (research/unknowns.md).
    pub(crate) fn critter_damaged_player(&mut self, p: usize, id: u16, dmg: f32) -> R<()> {
        let found = (0..self.critters.len()).find(|&k| self.critters[k].as_ref().is_some_and(|cr| cr.rec.u16_(c::ID) == id));
        if let Some(k) = found {
            self.critter_damaged_player_sub(k, p, dmg)?;
        }
        Ok(())
    }

    /// CritterDoSfx 0x194270; returns the effect slot or -1.
    pub(crate) fn critter_do_sfx(&mut self, i: usize, s: usize, base: Option<V3>, attach: bool) -> R<i32> {
        self.critter_do_sfx_depth(i, s, base, attach, 0)
    }

    fn critter_do_sfx_depth(&mut self, i: usize, s: usize, base: Option<V3>, attach: bool, depth: usize) -> R<i32> {
        if 64 < depth {
            return unsupported("critter SFXX chain cycle");
        }
        let srec = self.csfx_pub(i, s);
        let flags = CritterFile::i32_pub(&srec, 0) as u32;
        if flags & 0x400 != 0 && self.globals.aux_levels & 0x80 != 0 {
            return Ok(-1);
        }
        let off = v3(&srec, 0x30);
        let mut out = -1;
        let mut attach = attach;
        if flags & 0x0f00_0000 != 0 || flags & 0x100 != 0 {
            // CritterDoParticle / CritterDoTexmodNode: presentation.
        } else if flags & 0x200 != 0 {
            let first = self.critter_type_i16(i, 0x11a) as usize;
            for k in 0..self.crit(i).colnode_nodes.len() {
                let t = self.crit(i).ty;
                let nf = CritterFile::i16_pub(&self.critter_data.files[t.0].wad.nodes[first + k], 0x10) as u16;
                if nf & 1 == 0 {
                    let cp = self.crit(i).rec.v3(c::COLNODES + 0x60 * k + 0x40);
                    let pos = [add(cp[0], off[0])?, add(cp[1], off[1])?, add(cp[2], off[2])?];
                    out = self.critter_do_sfx_sub(i, &srec, pos, false, flags)?;
                }
            }
        } else if CritterFile::i32_pub(&srec, 8) < 0 {
            out = -1;
        } else {
            let pos;
            if flags & 0x801 != 0 {
                attach = true;
                pos = off;
            } else if flags & 0x80 != 0 {
                let h = self.crit(i).rec.v3(c::HOME_POS);
                pos = [add(h[0], off[0])?, add(h[1], off[1])?, add(h[2], off[2])?];
                attach = false;
            } else if flags & 0x40 != 0 {
                let mut p = match self.crit(i).node_dc {
                    None => self.crit(i).rec.v3(c::POS),
                    Some(n) => {
                        let m = self.critter_node_world(i, n, Some(off))?;
                        [m[12], m[13], m[14]]
                    }
                };
                if let Some(b) = base {
                    p = [add(b[0], p[0])?, add(b[1], p[1])?, add(b[2], p[2])?];
                }
                pos = p;
                attach = false;
            } else {
                pos = match base {
                    Some(b) => [add(b[0], off[0])?, add(b[1], off[1])?, add(b[2], off[2])?],
                    None => off,
                };
            }
            out = self.critter_do_sfx_sub(i, &srec, pos, attach, flags)?;
        }
        let sound = CritterFile::i32_pub(&srec, 0xc);
        if 0 <= sound {
            let pos = self.crit(i).rec.v3(c::POS);
            // AudioPlaySound(id, 0xe0, position, 3D flag): positional sound.
            self.sfx(sound, Some(pos), 0xe0, 0x50);
        }
        if flags & 2 != 0 {
            // ShakeCamera(0.1, 0, 0, 0x5a, 0x64).
            self.shake_camera(f32::from_bits(0x3dcc_cccd), 0, 0, 0x5a, 0x64);
        }
        if flags & 0x20 != 0 {
            self.do_world_effects();
        }
        if flags & 0x4_0000 != 0 && self.crit(i).rec.i16_(c::MOVE_FX) < 0 {
            self.crit_mut(i).rec.set_i16(c::MOVE_FX, out as i16);
        }
        let next = CritterFile::i32_pub(&srec, 4);
        if let Ok(n) = usize::try_from(next) {
            self.critter_do_sfx_depth(i, n, base, attach, depth + 1)?;
        }
        Ok(out)
    }

    /// CritterDoSfxSub 0x193ff0: StartFXSub with the SFXX flags translated,
    /// owner = critter ID | 0x1000.
    fn critter_do_sfx_sub(&mut self, i: usize, srec: &[u8], pos: V3, attach: bool, flags: u32) -> R<i32> {
        let header = CritterFile::i32_pub(srec, 8);
        if header < 0 {
            return Ok(-1);
        }
        let mut fl = 0u32;
        if flags & 0x10 != 0 {
            fl = 0x20_0000;
        }
        if flags & 0x8000 != 0 {
            fl |= if self.globals.cur_world == 8 { 0x800_0000 } else { 0x1_0000 };
        }
        for (a, b) in [(0x1_0000, 0x40_0000), (0x2_0000, 0x200_0000), (0x40_0000, 0x400_0000), (0x80_0000, 0x2000_0000)] {
            if flags & a != 0 {
                fl |= b;
            }
        }
        let Some(h) = self.effects.tables.headers.get(header as usize).copied().flatten() else { return Ok(-1) };
        let world = if attach {
            // SfxSetParent: 0x800 the MB parent of +0xcc (a root's own matrix,
            // a child's tree parent), 1 the +0xcc animation root, otherwise
            // the +0xdc transition node; spawned at the parent's world
            // transform of the local offset.
            let ar = self.crit(i).anim_root;
            let node = if flags & 0x800 != 0 {
                match usize::try_from(self.crit(i).parents[ar]) {
                    Ok(p) => Some(Some(p)),
                    Err(_) => Some(None),
                }
            } else if flags & 1 != 0 {
                Some(Some(ar))
            } else {
                self.crit(i).node_dc.map(Some)
            };
            match node {
                Some(Some(n)) => {
                    let m = self.critter_node_world(i, n, Some(pos))?;
                    [m[12], m[13], m[14]]
                }
                Some(None) => {
                    let o = self.critter_pose_owner(i);
                    let m = crate::tree_pose::node_world(&[-1], &[crate::tree_pose::IDENTITY], self.critter_root_pub(o), 0, Some(pos)).map_err(|e| Unsupported(e.0))?;
                    [m[12], m[13], m[14]]
                }
                None => pos,
            }
        } else {
            pos
        };
        let time = CritterFile::f32_pub(srec, 0x3c);
        let k = self.start_fx_tree(time, Visual::Header(header as usize), h.frames, h.timing, Some(world), fl)?;
        self.fx_mut(k).set_u32(fx::KIND, header as u32);
        let id = self.crit(i).rec.u16_(c::ID) | 0x1000;
        self.fx_mut(k).set_i16(fx::OWNER, id as i16);
        Ok(k as i32)
    }

    /// MBNodeSetParent(SfxGetNode(effect), ItemGetNode(item)): the effect's
    /// position becomes relative to the item's node (rows +0x10..+0x30,
    /// translation +0x40); static rocks, so it is placed once.
    fn parent_fx_to_item(&mut self, k: usize, item: usize) -> R<()> {
        let r = &self.items.records[item];
        let rows = [r.v3(0x10), r.v3(0x20), r.v3(0x30)];
        let t = r.v3(0x40);
        if let Some(n) = self.effects.nodes[k] {
            let w = mul_vec_mat3(n.position, &rows)?;
            let pos = [add(w[0], t[0])?, add(w[1], t[1])?, add(w[2], t[2])?];
            if let Some(n) = self.effects.nodes[k].as_mut() {
                n.position = pos;
            }
        }
        Ok(())
    }

    /// SafeRockActive 0x1532f0: health +0xe4 > 0 and stage +0xf2 > 0.
    pub(crate) fn safe_rock_active(&self, item: usize) -> bool {
        let r = &self.items.records[item];
        0 < r.i16_(0xe4) && 0 < r.i16_(0xf2)
    }

    /// SafeRockNearestTarget 0x19c730(player): the nearest inactive rock to
    /// the player (xz), or for no target the first inactive rock in the
    /// rotation after SafeRockIdx.
    fn safe_rock_nearest_target(&self, target: i16) -> R<i32> {
        let n = self.safe_rock_num;
        if n <= 0 {
            return Ok(-1);
        }
        if target < 0 {
            let mut j = 1;
            for k in 0..n {
                j = (j + self.safe_rock_idx) % n;
                if !self.safe_rock_active(self.safe_rock_items[k as usize]) {
                    return Ok(j);
                }
                j = k + 2;
            }
            return Ok(-1);
        }
        let pp = self.players[target as usize].v3(0x50);
        let mut best = (f32::from_bits(0x6258_d727), -1);
        for k in 0..n as usize {
            let it = self.safe_rock_items[k];
            if self.safe_rock_active(it) {
                continue;
            }
            let t = self.items.records[it].v3(0x40);
            let d = crate::vu0::fqdist(sub(t[0], pp[0])?, sub(t[2], pp[2])?)?.0;
            if d < best.0 {
                best = (d, k as i32);
            }
        }
        Ok(best.1)
    }

    /// CritterDoDamageFX 0x1946c8 (`local`: offsets in the critter frame).
    fn critter_do_damage_fx(&mut self, i: usize, d: usize, local: bool, base: Option<V3>) -> R<i32> {
        let drec = self.critter_damage_rec(i, d);
        let f16 = CritterFile::i16_pub(&drec, 2) as u16;
        let dtype = CritterFile::i16_pub(&drec, 0);
        if f16 & 0x4000 != 0 && 0.0 < self.crit(i).rec.f32_(c::FX_HOLD) && dtype != 1 {
            return Ok(-1);
        }
        let off = v3(&drec, 0x20);
        let m = self.node_rows(i, c::MATRIX);
        let pos = self.crit(i).rec.v3(c::POS);
        let e = if !local {
            let r = mul_vec_mat3(off, &m)?;
            match base {
                Some(b) => [add(b[0], r[0])?, add(b[1], r[1])?, add(b[2], r[2])?],
                None => [add(r[0], pos[0])?, add(r[1], pos[1])?, add(r[2], pos[2])?],
            }
        } else {
            match base {
                // MulBodyVecMat4(base, base, critter matrix): base − the
                // translation dotted with each row (no caller passes both).
                Some(b) => {
                    let v = [sub(b[0], pos[0])?, sub(b[1], pos[1])?, sub(b[2], pos[2])?];
                    let dot = |r: V3| -> R<f32> { Ok(add(add(mul(v[0], r[0])?, mul(v[1], r[1])?)?, mul(v[2], r[2])?)?) };
                    let bb = [dot(m[0])?, dot(m[1])?, dot(m[2])?];
                    [add(bb[0], off[0])?, add(bb[1], off[1])?, add(bb[2], off[2])?]
                }
                None => off,
            }
        };
        let sfx = CritterFile::i16_pub(&drec, 0x40);
        let Ok(sfx) = usize::try_from(sfx) else { return Ok(-1) };
        let k = self.critter_do_sfx(i, sfx, Some(e), local)?;
        if k < 0 {
            return Ok(k);
        }
        let k = k as usize;
        let srec = self.csfx_pub(i, sfx);
        let mut extra = CritterFile::f32_pub(&drec, 0x2c);
        let mut fl: u32 = 0x801;
        if self.critter_kind(i) != 4 {
            fl = 0x809;
            let sid = self.fx(k).i16_(fx::SFX_ID);
            self.critter_set_fx_hit_time(i, f32::from_bits(0x4479_c000), sid)?;
        }
        match dtype - 1 {
            0 => {
                fl |= 6;
                if f16 & 0x4000 != 0 && 0.0 < self.crit(i).rec.f32_(c::FX_HOLD) {
                    let end = add(self.globals.atime, self.crit(i).rec.f32_(c::FX_HOLD))?;
                    self.fx_mut(k).set_f32(fx::END, end);
                }
            }
            1 => fl |= 0x30,
            2 | 4 | 5 | 7 => fl |= 0x20,
            3 => {
                extra = -1.0;
                fl = 0;
            }
            _ => {}
        }
        let dflags = CritterFile::i32_pub(&drec, 4) as u32;
        if dflags & 0x2_0000 != 0 {
            fl |= 0x2_0000;
        }
        if f16 & 0x40 != 0 {
            fl &= 0xffff_fff9;
        }
        if f16 & 0x1000 != 0 {
            fl &= 0xffff_fffe;
        }
        if f16 & 0x2000 != 0 {
            fl |= 0x400;
        }
        self.fx_mut(k).or_u32(fx::FLAGS, fl);
        let hit = CritterFile::i16_pub(&drec, 0x42);
        if CritterFile::i32_pub(&srec, 0) as u32 & 0x10 != 0 && hit < 0 {
            // PlaceEffectOnFloor.
            if let Some(n) = self.effects.nodes[k] {
                let r = self.fx(k).f32_(fx::RADIUS);
                let (col, _) = self.collision.floor_collide(add(r, 1.0)?, add(r, 5.0)?, -10.0, n.position, true, 0)?;
                if let Some(h) = col.solid
                    && let Some(nm) = self.effects.nodes[k].as_mut()
                {
                    nm.position = [h.position[0], add(h.position[1], 0.1)?, h.position[2]];
                }
            }
        }
        if dtype != 1 {
            let yaw = CritterFile::f32_pub(&drec, 0x14);
            if yaw != 0.0
                && let Some(nm) = self.effects.nodes[k].as_mut()
            {
                let mut mm = nm.matrix;
                crate::collision::yaw_mat3(&mut mm, yaw)?;
                nm.matrix = mm;
            }
            let pitch = CritterFile::f32_pub(&drec, 0x1c);
            if pitch != 0.0
                && let Some(nm) = self.effects.nodes[k].as_mut()
            {
                let mut mm = nm.matrix;
                crate::items::pitch_mat3(&mut mm, pitch)?;
                nm.matrix = mm;
            }
        }
        if extra < 0.0 {
            return Ok(k as i32);
        }
        let scale = 1.0;
        let r = self.fx_mut(k);
        r.set_f32(fx::DAMAGE, 0.0);
        let dmg = mul(extra, self.level_f32(0xbc))?;
        let r = self.fx_mut(k);
        r.set_f32(fx::DAMAGE, dmg);
        r.set_f32(fx::CONE, CritterFile::f32_pub(&drec, 0x18));
        r.set_f32(fx::BLAST, mul(scale, CritterFile::f32_pub(&drec, 0xc))?);
        r.set_u32(fx::DAMAGE_FLAGS, dflags);
        if 0 <= hit {
            let hrec = self.csfx_pub(i, hit as usize);
            let r = self.fx_mut(k);
            r.set_i16(fx::HIT_FX, CritterFile::i32_pub(&hrec, 8) as i16);
            r.set_u32(fx::HIT_SOUND, CritterFile::i32_pub(&hrec, 0xc) as u32);
            r.set_u32(fx::WALL_SOUND, CritterFile::i32_pub(&hrec, 0xc) as u32);
            if CritterFile::i32_pub(&hrec, 0) as u32 & 0x10 != 0 {
                r.or_u32(fx::FLAGS, 0x20_0000);
            }
        }
        if 0 <= CritterFile::i16_pub(&drec, 0x44) {
            // SfxSetMorph: presentation; its flag side effects are kept.
            let mut t = CritterFile::f32_pub(&drec, 0x3c);
            if t <= 0.0 {
                t = 15.0;
            }
            if f16 & 0x800 != 0 {
                self.fx_mut(k).or_u32(fx::FLAGS, 0x8000);
            }
            if dflags & 0x400_0000 != 0 {
                let r = self.fx_mut(k);
                r.set_f32(fx::HOLD, t);
                r.and_u32(fx::FLAGS, 0xffff_ffdf);
            }
        }
        let gravity = CritterFile::f32_pub(&drec, 0x38);
        let radius = mul(scale, CritterFile::f32_pub(&drec, 8))?;
        let smin = CritterFile::f32_pub(&drec, 0x30);
        if smin <= 0.0 {
            self.sfx_set_physics(k, gravity, radius, None, None);
            return Ok(k as i32);
        }
        let mut dif = self.crit(i).rec.f32_(c::DIFFICULTY);
        dif = dif.clamp(0.5, 1.5);
        let smax = CritterFile::f32_pub(&drec, 0x34);
        let speed = add(mul(mul(sub(smax, smin)?, sub(dif, 0.5)?)?, 0.75)?, smin)?;
        let row2 = self.crit(i).rec.v3(c::MATRIX + 0x20);
        let mut dir = if f16 & 4 != 0 {
            row2
        } else if f16 & 1 != 0 && 0 <= self.crit(i).rec.i16_(c::TARGET) {
            let tp = self.players[self.crit(i).rec.i16_(c::TARGET) as usize].v3(0x70);
            let ep = self.effects.nodes[k].map_or(e, |n| n.position);
            [sub(tp[0], ep[0])?, sub(tp[1], ep[1])?, sub(tp[2], ep[2])?]
        } else {
            [row2[0], -0.5, row2[2]]
        };
        if f16 & 8 == 0 {
            calc_target_dir(speed, div(1.0, speed)?, gravity, 0.0, &mut dir)?;
        } else {
            crate::collision::normal_vector_pub(&mut dir)?;
        }
        if dtype == 1 {
            let yaw = CritterFile::f32_pub(&drec, 0x14);
            let spread = CritterFile::f32_pub(&drec, 0x48);
            if yaw != 0.0 || spread != 0.0 {
                let mut a = yaw;
                if 0.0 < spread {
                    let rnd = self.random_pub(spread)?;
                    a = add(add(a, mul(-spread, 0.5)?)?, rnd)?;
                }
                dir = yaw_vec3(a, dir)?;
            }
            let pitch = CritterFile::f32_pub(&drec, 0x1c);
            if f16 & 8 != 0 && pitch != 0.0 {
                dir = pitch_vec3(pitch, dir)?;
            }
        }
        let v = [mul(speed, dir[0])?, mul(speed, dir[1])?, mul(speed, dir[2])?];
        let spin = if CritterFile::i32_pub(&srec, 0) as u32 & 8 != 0 {
            let a = self.random_pub(f32::from_bits(0x3fc9_0fdb))?;
            let b = self.random_pub(f32::from_bits(0x3fc9_0fdb))?;
            Some([a, 0.0, b])
        } else {
            None
        };
        self.sfx_set_physics(k, gravity, radius, Some(v), spin);
        Ok(k as i32)
    }

    /// SfxSetPhysics 0x17e000 (velocity orientation is presentation).
    pub(crate) fn sfx_set_physics_pub(&mut self, k: usize, gravity: f32, radius: f32, vel: Option<V3>, spin: Option<V3>) {
        self.sfx_set_physics(k, gravity, radius, vel, spin)
    }

    fn sfx_set_physics(&mut self, k: usize, gravity: f32, radius: f32, vel: Option<V3>, spin: Option<V3>) {
        if let Some(v) = vel
            && let Ok(a) = crate::libm::atan2f(v[0], v[2])
            && let Some(n) = self.effects.nodes[k].as_mut()
        {
            // YawMat3(atan2f(vx, vz), node): the node faces its velocity.
            let _ = crate::collision::yaw_mat3(&mut n.matrix, a);
        }
        let r = self.fx_mut(k);
        if let Some(v) = vel {
            r.set_v3(fx::VELOCITY, v);
        }
        if let Some(s) = spin {
            r.set_v3(fx::SPIN, s);
        }
        if 0.0 <= gravity {
            r.set_f32(fx::GRAVITY, gravity);
        }
        if 0.0 <= radius {
            r.set_f32(fx::RADIUS, radius);
        }
    }

    /// CritterSetFxHitTime 0x199e08.
    fn critter_set_fx_hit_time(&mut self, i: usize, hold: f32, sfx_id: i16) -> R<()> {
        if hold <= 0.0 {
            return Ok(());
        }
        let time = self.globals.time;
        let until = add(hold, time)?;
        let mut oldest = -1.0f32;
        let mut pick = None;
        for k in 0..4 {
            let t = self.crit(i).rec.f32_(0x52c + 4 * k);
            if t < time {
                let r = &mut self.crit_mut(i).rec;
                r.set_i16(0x524 + 2 * k, sfx_id);
                r.set_f32(0x52c + 4 * k, until);
                return Ok(());
            }
            if oldest < 0.0 || t < oldest {
                oldest = t;
                pick = Some(k);
            }
        }
        if let Some(k) = pick {
            let r = &mut self.crit_mut(i).rec;
            r.set_i16(0x524 + 2 * k, sfx_id);
            r.set_f32(0x52c + 4 * k, until);
        }
        Ok(())
    }

    /// CritterNoHit(Sub) 0x199f50: the effect `sfx_id` may not hit yet.
    /// CritterNoHit 0x199f50: CritterNoHitSub on the instance, then on a
    /// root's children or a child's root.
    fn critter_no_hit(&self, i: usize, sfx_id: i16) -> bool {
        if self.critter_no_hit_sub(i, sfx_id) {
            return true;
        }
        match self.crit(i).parent {
            None => self.critter_children(i).into_iter().any(|k| self.critter_no_hit_sub(k, sfx_id)),
            Some(p) => self.critter_no_hit_sub(p, sfx_id),
        }
    }

    fn critter_no_hit_sub(&self, i: usize, sfx_id: i16) -> bool {
        let time = self.globals.time;
        (0..4).any(|k| self.crit(i).rec.i16_(0x524 + 2 * k) == sfx_id && time < self.crit(i).rec.f32_(0x52c + 4 * k))
    }

    /// CritterLineCollide for EnemyCollideEnemy 0x142a40 (kind 2: only
    /// collision nodes with NODE flag 8, no SfxId test).
    pub(crate) fn critter_line_collide_enemy(&mut self, r: f32, from: V3, to: V3) -> R<Option<usize>> {
        Ok(self.critter_line_collide_kind(r, from, to, None, 2, None)?.map(|(k, _)| k))
    }

    /// CritterLineCollide 0x19a810 (from CritterCollideStart: all critters).
    fn critter_line_collide(&mut self, r: f32, from: V3, to: V3, sfx_id: Option<i16>) -> R<Option<(usize, V3)>> {
        self.critter_line_collide_kind(r, from, to, sfx_id, 0, None)
    }

    /// CritterLineCollide's body: `mode` (t0) is passed to LineCylinderCollide
    /// as its directional flag; mode 2 also limits node tests to NODE flag 8.
    /// `skip` is CritterCollideStart's crit_skipcol.
    pub(crate) fn critter_line_collide_kind(&mut self, r: f32, from: V3, to: V3, sfx_id: Option<i16>, mode: i32, skip: Option<usize>) -> R<Option<(usize, V3)>> {
        let dxz = add(mul(sub(to[0], from[0])?, sub(to[0], from[0])?)?, mul(sub(to[2], from[2])?, sub(to[2], from[2])?)?)?;
        let dy = mul(sub(to[1], from[1])?, sub(to[1], from[1])?)?;
        for k in 0..self.critters.len() {
            if self.critters[k].is_none() || skip == Some(k) {
                continue;
            }
            if let Some(id) = sfx_id
                && self.critter_no_hit(k, id)
            {
                continue;
            }
            if self.critter_type_u32(k, 0x5c) & 2 != 0
                && let Some(p) = self.critter_line_node_col(k, r, dxz, dy, from, to, mode)?
            {
                return Ok(Some((k, p)));
            }
            if let Some(p) = self.critter_line_root_col(k, r, dxz, dy, from, to, mode)? {
                return Ok(Some((k, p)));
            }
        }
        Ok(None)
    }

    /// CritterLineRootColSub 0x19a3d8.
    #[allow(clippy::too_many_arguments)]
    fn critter_line_root_col(&self, k: usize, r: f32, dxz: f32, dy: f32, from: V3, to: V3, mode: i32) -> R<Option<V3>> {
        let h = add(r, self.critter_type_f32(k, 0x78))?;
        let w = add(r, self.critter_type_f32(k, 0x7c))?;
        let col = self.crit(k).rec.v3(c::COL);
        let a = sub(col[0], to[0])?;
        let b = sub(col[2], to[2])?;
        if !(add(mul(a, a)?, mul(b, b)?)? <= add(mul(w, w)?, dxz)? && sub(col[1], to[1])? <= add(h, dy)?) {
            return Ok(None);
        }
        super::enemies::line_cylinder_collide(col, from, to, w, h, mode != 0)
    }

    /// CritterLineNodeColSub 0x19a468 (sets +0xb40 to the colnode hit).
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn critter_line_node_col(&mut self, k: usize, r: f32, dxz: f32, dy: f32, from: V3, to: V3, mode: i32) -> R<Option<V3>> {
        let t = self.crit(k).ty;
        let first = self.critter_type_i16(k, 0x11a) as usize;
        for n in 0..self.crit(k).colnode_nodes.len() {
            if self.crit(k).colnode_nodes[n].0.is_none() {
                continue;
            }
            let at = c::COLNODES + 0x60 * n;
            if self.crit(k).rec.f32_(at + 0x58) <= self.crit(k).rec.f32_(at + 0x5c) {
                continue;
            }
            let nrec = &self.critter_data.files[t.0].wad.nodes[first + n];
            if mode == 2 && CritterFile::i16_pub(nrec, 0x10) as u16 & 8 == 0 {
                continue;
            }
            let rr = add(r, CritterFile::f32_pub(nrec, 0x2c))?;
            let cp = self.crit(k).rec.v3(at + 0x40);
            let a = sub(cp[0], to[0])?;
            let b = sub(cp[2], to[2])?;
            if add(mul(a, a)?, mul(b, b)?)? <= add(mul(rr, rr)?, dxz)?
                && sub(cp[1], to[1])? <= add(rr, dy)?
                && let Some(p) = super::enemies::line_cylinder_collide(cp, from, to, rr, rr, mode != 0)?
            {
                self.crit_mut(k).rec.set_i16(c::COLNODE_HIT, n as i16);
                return Ok(Some(p));
            }
        }
        self.crit_mut(k).rec.set_i16(c::COLNODE_HIT, -1);
        Ok(None)
    }

    /// CritterExpCollide 0x19a260 from critter index `start`: the next
    /// critter inside a blast (radius, cone, centre, forward).
    fn critter_exp_collide(&mut self, start: usize, r: f32, cone: f32, centre: V3, fwd: V3, sfx_id: i16) -> R<Option<(usize, V3)>> {
        for k in start..self.critters.len() {
            if self.critters[k].is_none() || self.critter_no_hit(k, sfx_id) {
                continue;
            }
            if self.critter_type_u32(k, 0x5c) & 2 != 0 {
                let t = self.crit(k).ty;
                let first = self.critter_type_i16(k, 0x11a) as usize;
                let mut hit = None;
                for n in 0..self.crit(k).colnode_nodes.len() {
                    if self.crit(k).colnode_nodes[n].0.is_none() {
                        continue;
                    }
                    let at = c::COLNODES + 0x60 * n;
                    if self.crit(k).rec.f32_(at + 0x58) <= self.crit(k).rec.f32_(at + 0x5c) {
                        continue;
                    }
                    let cp = self.crit(k).rec.v3(at + 0x40);
                    let mut d = [sub(cp[0], centre[0])?, sub(cp[1], centre[1])?, sub(cp[2], centre[2])?];
                    let dist = crate::item_collision::normal_vector_2d(&mut d)?;
                    let lim = add(r, CritterFile::f32_pub(&self.critter_data.files[t.0].wad.nodes[first + n], 0x2c))?;
                    if dist <= lim && (cone <= -1.0 || cone <= add(mul(d[0], fwd[0])?, mul(d[2], fwd[2])?)?) {
                        hit = Some((n, d));
                        break;
                    }
                }
                match hit {
                    Some((n, d)) => {
                        self.crit_mut(k).rec.set_i16(c::COLNODE_HIT, n as i16);
                        return Ok(Some((k, d)));
                    }
                    None => self.crit_mut(k).rec.set_i16(c::COLNODE_HIT, -1),
                }
            }
            let col = self.crit(k).rec.v3(c::COL);
            let mut d = [sub(col[0], centre[0])?, sub(col[1], centre[1])?, sub(col[2], centre[2])?];
            let dist = crate::item_collision::normal_vector_2d(&mut d)?;
            if dist <= add(r, self.critter_type_f32(k, 0x7c))? && (cone <= -1.0 || cone <= add(mul(d[0], fwd[0])?, mul(d[2], fwd[2])?)?) {
                return Ok(Some((k, d)));
            }
        }
        Ok(None)
    }

    /// The critter part of ProcessEffects (0x17c5f0–0x17c934) for effect
    /// `k`; returns the effect result code and the clipped end point.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn effect_vs_critters(&mut self, k: usize, blast: bool, radius: f32, from: V3, to: V3, fwd: V3, damage: f32, owner: i32, hold: f32, age: f32, pierce: bool) -> R<(i32, Option<V3>)> {
        let flags = self.fx(k).u32_(fx::FLAGS);
        let sid = self.fx(k).i16_(fx::SFX_ID);
        if blast {
            if !(0.0 < radius && flags & 0x400 == 0) {
                return Ok((0, None));
            }
            let cone = self.fx(k).f32_(fx::CONE);
            let mut start = 0;
            while let Some((c_, d)) = self.critter_exp_collide(start, radius, cone, to, fwd, sid)? {
                self.critter_set_fx_hit_time(c_, hold, sid)?;
                if !(self.critter_kind(c_) == 4 && flags & 0x800 != 0) {
                    let h = self.fx(k).i16_(fx::HITS) + 1;
                    self.fx_mut(k).set_i16(fx::HITS, h);
                    let df = self.fx(k).u32_(fx::DAMAGE_FLAGS);
                    if 0 <= self.critter_damage(damage, c_, owner, df, None, Some(d), 2)? {
                        let hf = self.fx(k).i16_(fx::HIT_FX);
                        if 0 <= hf && self.critters[c_].is_some() {
                            self.start_hit_fx(hf as usize, self.crit(c_).rec.v3(c::ATTN))?;
                        }
                    }
                }
                start = c_ + 1;
            }
            return Ok((0, None));
        }
        let Some((c_, p)) = self.critter_line_collide(radius, from, to, Some(sid))? else { return Ok((0, None)) };
        self.critter_set_fx_hit_time(c_, hold, sid)?;
        let boss = self.critter_kind(c_) == 4;
        let pierce = pierce && !boss;
        let mut result = 0;
        let mut out = None;
        if flags & 0x400 == 0 && !(boss && flags & 0x800 != 0) {
            let mut dir = self.fx(k).v3(fx::VELOCITY);
            let h = self.fx(k).i16_(fx::HITS) + 1;
            self.fx_mut(k).set_i16(fx::HITS, h);
            crate::collision::normal_vector_pub(&mut dir)?;
            let df = self.fx(k).u32_(fx::DAMAGE_FLAGS);
            let r = self.critter_damage(damage, c_, owner, df, Some(p), Some(dir), 2)?;
            result = if 0 <= r && r != 0 { 3 } else { 2 };
            if !pierce {
                out = Some(p);
            }
            let q = self.globals.quest_item_state;
            if q.wrapping_sub(2) < 2 {
                // ProcessEffects 0x17c7c4: the legend-item hit on the boss.
                match self.globals.boss_type {
                    0x24 => {
                        self.crm_pub(c_).set_i16(0xb4e, 0x708);
                        // StartFXSub(28, 0x5a, (0, 6, 0), 0, 0x400880) parented
                        // to the critter's +0xcc node: QuestWallSfx.
                        let at = {
                            let ar = self.crit(c_).anim_root;
                            let m = self.critter_node_world(c_, ar, Some([0.0, 6.0, 0.0]))?;
                            [m[12], m[13], m[14]]
                        };
                        self.quest_wall_sfx = match self.effects.tables.headers.get(0x5a).copied().flatten() {
                            Some(h) => {
                                let k2 = self.start_fx_tree(28.0, Visual::Header(0x5a), h.frames, h.timing, Some(at), 0)?;
                                self.fx_mut(k2).set_u32(fx::KIND, 0x5a);
                                k2 as i32
                            }
                            None => -1,
                        };
                        self.globals.quest_item_state = 4;
                    }
                    0x22 => {
                        // +0xb48: the swapped texture (presentation).
                        self.crm_pub(c_).set_i16(0xb4c, 0x4b0);
                        self.globals.quest_item_state = 4;
                    }
                    0x26 => {
                        // MBSetObject of the boss part (presentation).
                        self.crm_pub(c_).set_i16(0xb4e, 18000);
                    }
                    _ => {}
                }
                self.audio_legend_item(3, Some(p));
                if result < 2 {
                    result = 2;
                }
            }
            if self.globals.quest_item_state == 5 {
                if self.globals.boss_type != 0x23 {
                    if pierce {
                        result = 0;
                    }
                    return Ok((result, out));
                }
                self.globals.quest_item_state = 6;
            }
        } else if f32::from_bits(0x3d88_9a02) < sub(self.fx(k).f32_(fx::DURATION), age)? {
            result = -1;
        }
        if pierce {
            result = 0;
        }
        Ok((result, out))
    }

    /// CritterDamage 0x199100: -1 not damageable, 1 killed, 0 otherwise.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn critter_damage(&mut self, dmg: f32, i: usize, player: i32, flags: u32, pos: Option<V3>, dir: Option<V3>, kind: i32) -> R<i32> {
        if self.critters[i].is_none() || (self.crit(i).rec.u32_(c::STATE)) < 2 {
            return Ok(-1);
        }
        let mv = self.crit(i).rec.i16_(c::MOVE).max(0) as usize;
        let (mut d, mut fl) = (dmg, flags);
        if self.critter_move_i32(i, mv, 0) == 0x23 {
            fl = flags & 0xffff_fecf;
            d = mul(dmg, 0.25)?;
            let s0 = self.critter_move_i16(i, mv, 0x58);
            if 1000 <= self.critter_move_i16(i, mv, 0x5a) && self.crit(i).rec.u16_(c::SFX_FLAGS) & 1 == 0 && 0 <= s0 {
                self.crit_mut(i).rec.or_u16(c::SFX_FLAGS, 1);
                self.critter_do_sfx(i, s0 as usize, None, true)?;
            }
        }
        let armor = self.critter_type_f32(i, 0xbc);
        let resist = self.critter_type_u32(i, 0xe0);
        modify_damage(armor, &mut d, &mut fl, resist, self.globals.boss_type)?;
        let kind_c = self.critter_kind(i);
        if self.globals.options == 3 && 0 <= player {
            d = 10000.0;
        }
        if kind_c != 4 && self.globals.enemy_shrink < 1.0 {
            d = add(d, d)?;
        }
        let taken = add(self.crit(i).rec.f32_(c::DAMAGE_TAKEN), d)?;
        self.crit_mut(i).rec.set_f32(c::DAMAGE_TAKEN, taken);
        if kind_c == 4 && 3 < self.globals.quest_item_state.wrapping_sub(1) {
            d = mul(d, [1.0, 1.0, 0.5, 0.3, 0.2][self.critter_nactive.min(4)])?;
        }
        if let Ok(p) = usize::try_from(player) {
            let mut eff = 0.0;
            if 0.0 <= d {
                eff = d;
                if self.crit(i).rec.f32_(c::HEALTH) < d {
                    eff = self.crit(i).rec.f32_(c::HEALTH);
                }
            }
            let max = add(mul(self.critter_type_f32(i, 0xe4), self.level_f32(0xac))?, 1.0)?;
            let frac = div(eff, max)?.min(1.0);
            let mut exp = mul(frac, self.critter_type_f32(i, 0xe8))? as i32;
            if kind_c == 4 {
                exp *= self.critter_nactive as i32;
            }
            self.critter_award_exp_pub(exp as f32, Some(p))?;
            let at = c::ANGER + 0x10 * p + 8;
            let v = add(eff, self.crit(i).rec.f32_(at))?;
            let time = self.globals.time;
            self.crit_mut(i).rec.set_f32(at, v);
            self.crit_mut(i).rec.set_f32(at + 4, time);
            if fl & 0x80_0000 != 0 {
                // do_heal_players(effective damage, player, critter matrix).
                let at = self.crit(i).rec.v3(0x40);
                self.do_heal_players(eff, p, at)?;
            }
            let lv = self.level_f32(0x9c);
            if kind_c != 4 && 0.0 < lv {
                let pl = self.players[p].i32_(0x2ac0) as f32;
                let mut k = 1.0;
                if pl < lv {
                    k = sub(1.0, mul(sub(lv, pl)?, 0.02)?)?;
                }
                if k < 0.1 {
                    k = 0.1;
                }
                d = mul(k, d)?;
            }
        }
        if fl & 0x10_0320 == 0
            && let Ok(n) = usize::try_from(self.crit(i).rec.i16_(c::COLNODE_HIT))
        {
            let at = c::COLNODES + 0x60 * n;
            let (max, acc) = (self.crit(i).rec.f32_(at + 0x58), self.crit(i).rec.f32_(at + 0x5c));
            if max <= acc {
                d = 0.0;
            } else {
                let t = self.crit(i).ty;
                let first = self.critter_type_i16(i, 0x11a) as usize;
                let nrec = self.critter_data.files[t.0].wad.nodes[first + n].clone();
                d = mul(d, CritterFile::f32_pub(&nrec, 0x40))?;
                if max < add(acc, d)? {
                    d = sub(max, acc)?;
                    if CritterFile::i16_pub(&nrec, 0x10) as u16 & 2 != 0 {
                        return unsupported("breakable critter collision node");
                    }
                }
                self.crit_mut(i).rec.set_f32(at + 0x5c, add(acc, d)?);
            }
        }
        if d <= 0.0 {
            return Ok(0);
        }
        self.crit_mut(i).rec.or_u32(c::DAMAGE_FLAGS, fl);
        if let Some(v) = dir {
            let k0 = self.crit(i).rec.v3(c::KNOCK);
            self.crit_mut(i).rec.set_v3(c::KNOCK, [add(v[0], k0[0])?, add(v[1], k0[1])?, add(v[2], k0[2])?]);
        }
        let time = self.globals.time;
        self.crit_mut(i).rec.set_f32(c::HIT_TIME, time);
        let at = pos.unwrap_or(self.crit(i).rec.v3(c::ATTN));
        let h = sub(self.crit(i).rec.f32_(c::HEALTH), d)?;
        self.crit_mut(i).rec.set_f32(c::HEALTH, h);
        if h <= 0.0 {
            self.critter_kill(i)?;
            if let Ok(p) = usize::try_from(player) {
                let o = 0xb2c + 0x1c * self.players[p].u32_(0xc) as usize;
                let v = self.players[p].i32_(o) + 1;
                self.players[p].set_i32(o, v);
            }
            return Ok(1);
        }
        if let Some(root) = self.crit(i).parent {
            // A child's damage is also taken from its root.
            let rh = sub(self.crit(root).rec.f32_(c::HEALTH), d)?;
            self.crit_mut(root).rec.set_f32(c::HEALTH, rh);
            if rh <= 0.0 {
                self.critter_kill(root)?;
                return Ok(1);
            }
        }
        if 0 < self.crit(i).rec.u8_(c::NCHILDREN) as i8 {
            // A root's damage is split over its active children at half rate.
            let kids = self.critter_children(i);
            let mut n = 0.0f32;
            for &k in &kids {
                if 1 < self.crit(k).rec.u32_(c::STATE) {
                    n = add(n, 1.0)?;
                }
            }
            if 0.0 < n {
                let share = mul(div(d, n)?, 0.5)?;
                for &k in &kids {
                    if 1 < self.crit(k).rec.u32_(c::STATE) {
                        let kh = sub(self.crit(k).rec.f32_(c::HEALTH), share)?;
                        self.crit_mut(k).rec.set_f32(c::HEALTH, kh);
                        if kh <= 0.0 {
                            self.critter_kill(k)?;
                        }
                    }
                }
            }
        }
        if fl & 0x100_0000 == 0 {
            if fl & 0xf == 0 {
                let mut s = self.critter_type_i16(i, 0xf4);
                if kind == 2 && 0 <= self.critter_type_i16(i, 0xf6) {
                    s = self.critter_type_i16(i, 0xf6);
                }
                if let Ok(s) = usize::try_from(s) {
                    self.critter_do_sfx(i, s, Some(at), false)?;
                }
            } else {
                // AddEnemyHitCol(type +0x78, at, the critter's matrix, flags, 0).
                let rows = [self.crit(i).rec.v3(c::MATRIX), self.crit(i).rec.v3(c::MATRIX + 0x10), self.crit(i).rec.v3(c::MATRIX + 0x20)];
                let scale = self.critter_type_f32(i, 0x78);
                self.add_enemy_hit_col(scale, at, Some(rows), fl, false, -1)?;
            }
            if fl & 0x10_0320 == 0 {
                if let Ok(n) = usize::try_from(self.crit(i).rec.i16_(c::COLNODE_HIT)) {
                    self.crit_mut(i).rec.set_i32(c::COLNODES + 0x60 * n + 0x54, 2);
                }
            } else {
                self.crit_mut(i).rec.set_i16(0xb44, 2);
            }
        }
        self.crit_mut(i).rec.set_u32(c::WAYPOINT, 0);
        Ok(0)
    }
}

impl Game {
    /// CritterGetTargetSub 0x199888: distance to critter `k` inside the
    /// targeting cone (2e21 when outside) and the direction to it.
    pub(crate) fn critter_get_target_sub(&self, cos: f32, range: f32, k: usize, origin: V3, facing: V3, out: &mut V3) -> R<f32> {
        let widen = if 0.0 < range { div(sub(1.0, cos)?, range)? } else { 0.0 };
        if self.crit(k).rec.f32_(c::HEALTH) <= 0.0 || self.crit(k).rec.u32_(c::STATE) < 2 {
            return Ok(2e21);
        }
        let t = self.crit(k).ty;
        if self.critter_type_u32(k, 0x5c) & 2 != 0 {
            let first = self.critter_type_i16(k, 0x11a) as usize;
            let (mut best_d, mut best_s) = (2e21f32, 2e21f32);
            for n in 0..self.crit(k).colnode_nodes.len() {
                if self.crit(k).colnode_nodes[n].0.is_none() {
                    continue;
                }
                let at = c::COLNODES + 0x60 * n;
                if self.crit(k).rec.f32_(at + 0x58) <= self.crit(k).rec.f32_(at + 0x5c) {
                    continue;
                }
                let cp = self.crit(k).rec.v3(at + 0x40);
                let mut d = [sub(cp[0], origin[0])?, sub(cp[1], origin[1])?, sub(cp[2], origin[2])?];
                let mut len = crate::collision::normal_vector_pub(&mut d)?;
                if !(range <= 0.0 || len <= range) {
                    continue;
                }
                let nrec = &self.critter_data.files[t.0].wad.nodes[first + n];
                let lim = CritterFile::f32_pub(nrec, 0x18);
                if 0.0 < lim && lim < len {
                    continue;
                }
                len = sub(len, CritterFile::f32_pub(nrec, 0x2c))?;
                let h = crate::vu0::fqdist(d[0], d[2])?.0;
                let dot = add(mul(d[0], facing[0])?, mul(d[2], facing[2])?)?;
                let thr = if 0.0 < widen { mul(add(cos, mul(len, widen)?)?, h)? } else { cos };
                if thr < dot {
                    // NODE +0x1c can be 0: the EE quotient is ±FLT_MAX (0x199aa8).
                    let score = crate::numeric::div_rn_ee(len, mul(sub(dot, thr)?, CritterFile::f32_pub(nrec, 0x1c))?)?;
                    if score < best_s {
                        *out = d;
                        best_d = len;
                        best_s = score;
                    }
                }
            }
            if best_d < 2e21 {
                return Ok(best_d);
            }
        }
        let col = self.crit(k).rec.v3(c::COL);
        let mut d = [sub(col[0], origin[0])?, sub(col[1], origin[1])?, sub(col[2], origin[2])?];
        let mut len = crate::collision::normal_vector_pub(&mut d)?;
        if 0.0 < range && range < len {
            return Ok(2e21);
        }
        len = sub(len, self.critter_type_f32(k, 0x7c))?;
        let h = crate::vu0::fqdist(d[0], d[2])?.0;
        if add(mul(d[0], facing[0])?, mul(d[2], facing[2])?)? <= mul(add(cos, mul(len, widen)?)?, h)? {
            return Ok(2e21);
        }
        *out = d;
        Ok(len)
    }

    /// CritterGetTarget 0x199c28: the best active root critter (and its
    /// children) for a player attack.
    pub(crate) fn critter_get_target(&self, cos: f32, range: f32, origin: V3, facing: V3) -> R<Option<(usize, f32, V3)>> {
        let mut best: Option<(usize, f32, V3)> = None;
        for k in 0..self.critters.len() {
            let Some(cr) = self.critters[k].as_ref() else { continue };
            if cr.rec.u32_(c::STATE) < 2 || cr.parent.is_some() {
                continue;
            }
            let mut node = Some(k);
            while let Some(n) = node {
                let mut v = [0.0; 3];
                let d = self.critter_get_target_sub(cos, range, n, origin, facing, &mut v)?;
                if d < best.map_or(2e21, |b| b.1) {
                    best = Some((n, d, v));
                }
                node = self.crit(n).child;
            }
        }
        Ok(best)
    }

    /// PlayerDamageEnemy 0x147070 for a critter target (index | 0x10000).
    pub fn player_damage_critter(&mut self, damage: f32, cooldown: f32, p: usize, k: usize, flags: u32, hit: Option<V3>) -> R<i32> {
        if 0.0 < cooldown && self.critter_no_hit(k, p as i16 + 1) {
            return Ok(-1);
        }
        let mut dmg = if damage == 0.0 { self.players[p].f32_(0x118) } else { damage };
        if self.players[p].u32_(0x138) & 0x100 != 0 {
            dmg = add(dmg, dmg)?;
        }
        let f = self.players[p].v3(0x40);
        let mut kk = mul(dmg, f32::from_bits(0x3d4c_cccd))?;
        if 2.0 < kk {
            kk = 2.0;
        }
        let r = self.critter_damage(dmg, k, p as i32, flags, hit, Some([f[0], kk, f[2]]), 1)?;
        if 0.0 < cooldown && self.critters[k].is_some() {
            self.critter_set_fx_hit_time(k, cooldown, p as i16 + 1)?;
        }
        Ok(r)
    }
}

/// ModifyDamage 0x190b88(armor, damage, damage flags, resistance flags).
pub fn modify_damage(armor: f32, dmg: &mut f32, flags: &mut u32, resist: u32, boss_type: i32) -> R<()> {
    let fl = *flags;
    let mut d = *dmg;
    if resist & 0x10_0000 != 0 {
        *dmg = if 1.0 < d { mul(-d, 0.1)? } else { 0.0 };
        return Ok(());
    }
    if resist & 0x1_0000 != 0 || (resist & 0x1000 != 0 && fl & 0x200 != 0) || (resist & 0x2000 != 0 && fl & 0x800 != 0) {
        *dmg = 0.0;
        return Ok(());
    }
    let (lo, mid, hi) = if boss_type < 0 { (0.5, 1.5, 2.0) } else { (0.75, 1.25, 1.5) };
    if resist & 0x4_0000 != 0 {
        *flags &= 0xfffe_fe8f;
    }
    if resist & 0x10 != 0 && fl & 0x200 != 0 {
        d = mul(d, lo)?;
    }
    if fl & 0xa00 == 0 {
        if 0.0 <= d {
            d = if d <= armor { 0.0 } else { sub(d, armor)? };
        } else {
            d = -d;
        }
    }
    let kind = fl & 0xf;
    if 0.0 < d {
        let (weak, strong_a, strong_b) = match kind {
            1 => (1u32, 0x100u32, 0x202u32),
            2 => (2, 0x200, 0x101),
            3 => (4, 0x400, 0x808),
            4 => (8, 0x800, 0x404),
            _ => {
                *dmg = d;
                return Ok(());
            }
        };
        if resist & weak != 0 {
            d = mul(d, lo)?;
        } else if resist & strong_a != 0 {
            *dmg = 0.0;
            return Ok(());
        } else if resist & strong_b == 0 {
            d = mul(d, mid)?;
        } else {
            d = mul(d, hi)?;
        }
    }
    *dmg = d;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::modify_damage;

    #[test]
    fn modify_damage_armor_and_weakness() {
        // Synthetic: armor 1 subtracts; element 1 with resistance bit 1 is
        // weak (×0.75 with a boss), no bits ×1.25.
        let (mut d, mut f) = (10.0f32, 1u32);
        modify_damage(1.0, &mut d, &mut f, 1, 0x29).unwrap();
        assert_eq!(d, 9.0 * 0.75);
        let (mut d, mut f) = (10.0f32, 1u32);
        modify_damage(1.0, &mut d, &mut f, 0, 0x29).unwrap();
        assert_eq!(d, 9.0 * 1.25);
        let (mut d, mut f) = (10.0f32, 0x20u32);
        modify_damage(1.0, &mut d, &mut f, 8, 0x29).unwrap();
        assert_eq!(d, 9.0);
    }
}
