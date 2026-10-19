//! PlayerMotion's input and contact helpers (research/player.md):
//! GetPlayerDMotion 0x1448e0, combo_player 0x144688, PlayerCollideItems
//! 0x146348, VerifyItemCol 0x146558, ModifyPlayerDpos 0x147b68,
//! PlayerCollidePlayers 0x146030, PushPlayer 0x147cc0, PlayerCollideEnemies
//! 0x145bf8, PlayerCollideCritters 0x145b40, PlayerWorldDamage 0x147910,
//! PlayerNewFloor 0x145988 with PlayerCheckMovingFloor 0x1458a0 and
//! OtherPlayerOnOtherMovingObject, DoTransporter 0x146758 with
//! player_collis_trans, PlayerKnockback 0x147580, PlayerCheckFloor 0x1451a0,
//! DoAutoExit 0x1465c8.
//!
//! Player record pointers are native indices + 1 (0 = null): +0x690/+0x694
//! partner player, +0x880 target item, +0x884 exit/transporter item, +0x888
//! grabbed item, +0x89c floor object, +0x8a0 wall object.
use super::enemies::{e, line_cylinder_collide};
use super::{unsupported, Game, R};
use crate::item_collision::{item_coll_sub, normal_vector_2d, player_skip_item};
use crate::libm::atan2f;
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sub_guarded_rz as sub};
use crate::vu0::fqdist;

type V3 = [f32; 3];
pub(crate) const PI: f32 = f32::from_bits(0x4049_0fdb);
pub(crate) const TWO_PI: f32 = f32::from_bits(0x40c9_0fdb);
const DEG135: f32 = f32::from_bits(0x4016_cbe5);
const DEG45: f32 = f32::from_bits(0x3f49_0fdb);

/// The PlayerMotion angle wrap: one 2π step into (−π, π].
pub(crate) fn wrap(v: f32) -> R<f32> {
    Ok(if PI < v {
        sub(v, TWO_PI)?
    } else if v <= -PI {
        add(v, TWO_PI)?
    } else {
        v
    })
}

pub(crate) fn vadd(a: V3, b: V3) -> R<V3> {
    Ok([add(a[0], b[0])?, add(a[1], b[1])?, add(a[2], b[2])?])
}

pub(crate) fn ptr(v: u32) -> Option<usize> {
    v.checked_sub(1).map(|i| i as usize)
}

impl Game {
    fn ctl(&self, p: usize) -> &super::record::Record {
        &self.controls.control[p]
    }

    /// WorldObjGetAllFlags 0x11be48: runtime +0x10 flags up the parent chain.
    pub(crate) fn wobj_all_flags(&self, o: Option<usize>) -> u32 {
        let mut f = 0;
        let mut k = o;
        while let Some(i) = k {
            f |= self.collision.runtime_flags[i];
            k = self.collision.objects[i].parent;
        }
        f
    }

    /// GetPlayerDMotion 0x1448e0 (camera yaw + π in `cam`).
    pub fn get_player_dmotion(&mut self, p: usize, cam: f32) -> R<u32> {
        let debug = self.globals.aux_levels & 0x10 != 0 || self.globals.options_18 != 0;
        let rel = wrap(sub(add(cam, self.ctl(p).f32_(0x1c))?, self.players[p].f32_(0x86c))?)?;
        let c = &mut self.controls.control[p];
        if self.players[p].i32_(0x80c) != 0 && 0 <= self.globals.boss_type {
            c.and_u32(8, 0xffff_f6ff);
            c.and_u32(4, 0xffff_f6ff);
        }
        if self.players[p].u32_(0x138) & 0x400 != 0 && c.u32_(4) & 0x400 != 0 {
            let v = (c.u32_(4) & 0xffff_fbff) | 0x200;
            c.set_u32(4, v);
        }
        if debug && c.u32_(8) & 0x2000 != 0 {
            let v = (c.u32_(8) & 0xffff_dfff) | 0x800;
            c.set_u32(8, v);
        }
        let status = self.players[p].u16_(0x93c);
        let mut code = if status & 0x40 != 0 {
            0x27
        } else if status & 0x10 != 0 {
            0x26
        } else if status & 0x80 != 0 {
            0x17
        } else if debug && self.ctl(p).u32_(4) & 0x0800_0000 != 0 {
            0x1d
        } else {
            0
        };
        'pick: {
            if code != 0 {
                break 'pick;
            }
            let lv = self.ctl(p).u32_(4);
            let ed = self.ctl(p).u32_(8);
            let w92e = self.players[p].u16_(0x92e);
            if lv & 0x10000 != 0 {
                if w92e & 0x80 == 0 {
                    code = 0x19;
                }
            } else if lv & 0x8000 != 0 {
                if w92e & 0x80 == 0 {
                    code = 0x1a;
                }
            } else if lv & 0x100 != 0 {
                if w92e & 0x80 == 0 {
                    code = 0x18;
                }
            } else if w92e == 0x80 {
                self.players[p].set_u16(0x92e, 0);
            }
            if code != 0 {
                break 'pick;
            }
            let turbo = self.players[p].f32_(0x800);
            if lv & 0x20000 != 0 && 50.0 <= turbo {
                let k = self.combo_player(p, 5.0, f32::from_bits(0x3f34_fdf4))?;
                if 0 <= k {
                    code = 0x16;
                    self.players[p].set_u32(0x694, k as u32 + 1);
                    break 'pick;
                }
            }
            if ed & 0x800 != 0 && 0.0 <= turbo {
                code = 0x15;
                break 'pick;
            }
            if ed & 0x1000 != 0 {
                code = 2;
                break 'pick;
            }
            let c = self.ctl(p);
            let (lmag, rang, rmag, lang) = (c.f32_(0x20), c.f32_(0x24), c.f32_(0x28), c.f32_(0x1c));
            // joined_r0x00144c38: the four 90° sectors of `a`.
            let sector = |a: f32| -> u32 {
                if !(-DEG135..=DEG135).contains(&a) {
                    0x12
                } else if DEG45 < a {
                    0x14
                } else if a < -DEG45 {
                    0x13
                } else {
                    0x11
                }
            };
            if 0.0 < rmag {
                if 0.0 < lmag {
                    code = sector(wrap(sub(rang, lang)?)?);
                } else {
                    code = if lv & 0x400 != 0 { 0x10 } else { 0xf };
                }
            } else if lv & 0x4000 != 0 && 0.0 < lmag {
                if lv & 0x600 != 0 {
                    code = sector(rel);
                } else {
                    code = if !(-DEG135..=DEG135).contains(&rel) {
                        10
                    } else if DEG45 < rel {
                        0xc
                    } else if rel < -DEG45 {
                        0xb
                    } else {
                        9
                    };
                }
            } else if ed & 0x2000 != 0 && 5.0 <= turbo {
                code = 7;
            } else if lv & 0x200 != 0 {
                code = 0xf;
            } else if lv & 0x400 != 0 {
                code = 0x10;
            }
            if code == 0 {
                code = if 0.75 < lmag {
                    0xd
                } else if 0.0 < lmag {
                    8
                } else {
                    1
                };
            }
        }
        let cur = self.players[p].u32_(0x1e0);
        if cur == 0x73 {
            if code == 0x18 {
                if self.players[p].u16_(0x92e) & 4 != 0 {
                    self.players[p].or_u16(0x92e, 2);
                }
            } else {
                self.players[p].or_u16(0x92e, 4);
            }
        } else if cur == 0x75 {
            if code.wrapping_sub(0x18) > 1 {
                self.players[p].or_u16(0x92e, 8);
            } else if self.players[p].u16_(0x92e) & 8 == 0 {
                let v = self.players[p].u16_(0x930).wrapping_add(self.globals.vb_elapsed as u16);
                self.players[p].set_u16(0x930, v);
            }
        }
        let lv = self.ctl(p).u32_(4);
        if lv & 0x600 == 0 {
            if self.players[p].u32_(0x8cc) != 0 {
                self.players[p].set_u32(0x8cc, 0);
            }
        } else {
            let held = self.players[p].u32_(0x8cc);
            let v = (lv ^ held) | self.players[p].u32_(0x8d0);
            self.players[p].set_u32(0x8d0, v);
            self.players[p].set_u32(0x8cc, held | lv);
        }
        Ok(code)
    }

    /// combo_player 0x144688(range, cosine, player): the nearest partner in
    /// front of the player for a team move, or −1.
    fn combo_player(&self, p: usize, mut range: f32, cosine: f32) -> R<i32> {
        let r = &self.players[p];
        if r.u32_(0x690) != 0 || r.u32_(0x694) != 0 || r.u32_(0x6b4) == 0 || r.u32_(0x138) & 0x400 != 0 || r.i32_(0xfc) != 1 || r.f32_(0x800) < 50.0 {
            return Ok(-1);
        }
        let Some(floor) = ptr(r.u32_(0x89c)) else { return Ok(-1) };
        if self.collision.runtime_flags[floor] & 0x1000 != 0 {
            return Ok(-1);
        }
        let mut f = r.v3(0x40);
        normal_vector_2d(&mut f)?;
        let mut best = -1;
        for k in 0..4 {
            let o = &self.players[k];
            let a = o.u32_(0x1e0);
            let partner_floor = ptr(o.u32_(0x89c));
            if k == r.u32_(0) as usize
                || o.i32_(0xfc) != 1
                || o.u32_(0x690) != 0
                || o.u16_(0x93c) & 0x50 != 0
                || a.wrapping_sub(0x54) <= 6
                || 0x6b <= a
                || o.u32_(0x138) & 0x400 != 0
                || (r.i32_(0x80c) != 0 && 0 <= self.globals.boss_type)
            {
                continue;
            }
            let Some(pf) = partner_floor else { continue };
            if self.collision.runtime_flags[pf] & 0x1000 != 0 {
                continue;
            }
            let (op, rp) = (o.v3(0x50), r.v3(0x50));
            let mut d = [sub(op[0], rp[0])?, sub(op[1], rp[1])?, sub(op[2], rp[2])?];
            if 3.0 < d[1].abs() {
                continue;
            }
            let len = normal_vector_2d(&mut d)?;
            if len <= range && cosine <= add(mul(d[0], f[0])?, mul(d[2], f[2])?)? {
                range = len;
                best = k as i32;
            }
        }
        Ok(best)
    }

    /// PlayerCollideItems 0x146348(radius, height, player, from, to, out,
    /// blocking, item): the number of blocking items touched; +0x884 takes
    /// exit/transporter items, +0x880 the nearest item unless `blocking`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn player_collide_items(&mut self, radius: f32, height: f32, p: usize, from: V3, to: V3, mut out: Option<&mut V3>, blocking: bool, item_out: Option<&mut Option<usize>>) -> R<i32> {
        let grid_r = if (self.globals.boss_type as u32).wrapping_sub(0x2a) < 2 { add(radius, 50.0)? } else { radius };
        let near = self.grid.items_near(&self.items, grid_r, to)?;
        let env = self.collide_env();
        let mut best: Option<(usize, f32)> = None;
        let mut count = 0;
        for slot in near {
            if player_skip_item(&self.items, &self.items.records[slot]) {
                continue;
            }
            let mut tmp = to;
            let d = item_coll_sub(&self.items, &mut self.collision, &env, slot, from, to, Some(&mut tmp), radius, height)?;
            if d < 0.0 {
                continue;
            }
            let res = self.player_item_collision(p, slot)?;
            let kind = self.items.definitions[self.items.records[slot].definition()].kind();
            if res != 0 {
                if res == 1 {
                    count += 1;
                }
                if kind == 9 || kind == 0xb {
                    self.players[p].set_u32(0x884, slot as u32 + 1);
                }
            }
            if best.is_some_and(|(_, bd)| bd <= d) && (res == 0 || kind != 10) {
                continue;
            }
            if let Some(o) = out.as_deref_mut() {
                *o = tmp;
            }
            best = Some((slot, d));
            if blocking && count != 0 {
                break;
            }
        }
        let b = best.map(|(s, _)| s);
        if !blocking {
            self.players[p].set_u32(0x880, b.map_or(0, |s| s as u32 + 1));
        }
        if let Some(i) = item_out {
            *i = b;
        }
        Ok(count)
    }

    /// VerifyItemCol 0x146558: false when the item lies behind the request.
    pub(crate) fn verify_item_col(&self, item: usize, pos: V3, req: V3) -> R<bool> {
        let r = &self.items.records[item];
        if self.items.definitions[r.definition()].kind() == 7 {
            return Ok(true);
        }
        let ip = r.v3(0x60);
        Ok(0.0 <= add(mul(sub(ip[0], pos[0])?, req[0])?, mul(sub(ip[2], pos[2])?, req[2])?)?)
    }

    /// ModifyPlayerDpos 0x147b68: removes the blocked part of each axis; the
    /// rest becomes +0x83c/+0x844 push velocity.
    pub(crate) fn modify_player_dpos(&mut self, p: usize, d: &mut V3, to: V3, out: V3) -> R<i32> {
        // FPS 0x20f90c = 1 / SPF (ClockOncePerFrame).
        let fps = div(1.0, self.globals.spf)?;
        let mut n = 0;
        for (axis, o) in [(0usize, 0x83c), (2usize, 0x844)] {
            let f = sub(out[axis], to[axis])?;
            if 0.0 < f && d[axis] < 0.0 {
                let v = add(f, d[axis])?;
                d[axis] = if 0.0 < v { 0.0 } else { v };
                n += 1;
            } else if f < 0.0 && 0.0 < d[axis] {
                let v = add(f, d[axis])?;
                d[axis] = if v < 0.0 { 0.0 } else { v };
                n += 1;
            } else {
                let v = add(mul(sub(out[axis], to[axis])?, fps)?, self.players[p].f32_(o))?;
                self.players[p].set_f32(o, v);
                d[axis] = 0.0;
            }
        }
        Ok(n)
    }

    /// PlayerCollidePlayers 0x146030(radius, height, player, from, to, out,
    /// first): the player hit (nearest, or the first when `first`), with the
    /// pushed-out position in `out`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn player_collide_players(&mut self, radius: f32, height: f32, p: usize, from: V3, to: V3, out: &mut V3, first: bool) -> R<i32> {
        let me = self.players[p].u32_(0) as usize;
        let mut best = -1i32;
        let mut bd = 0.0;
        for k in 0..4 {
            let o = &self.players[k];
            if k == me || !matches!(o.i32_(0xfc), 1 | 4) || o.u16_(0x93c) & 0x20 != 0 {
                continue;
            }
            let c = o.v3(0x70);
            let a = [sub(c[0], from[0])?, sub(c[2], from[2])?];
            let b = [sub(to[0], from[0])?, sub(to[2], from[2])?];
            if add(mul(a[0], b[0])?, mul(a[1], b[1])?)? < 0.0 {
                continue;
            }
            let Some(hit) = line_cylinder_collide(c, from, to, add(radius, o.f32_(0x828))?, height, true)? else { continue };
            let d = fqdist(sub(hit[0], to[0])?, sub(hit[2], to[2])?)?.0;
            if best < 0 || d < bd {
                best = k as i32;
                bd = d;
            }
            if first {
                break;
            }
        }
        if let Ok(k) = usize::try_from(best) {
            let o = &self.players[k];
            let c = o.v3(0x70);
            let (dx, dz) = (sub(to[0], c[0])?, sub(to[2], c[2])?);
            let d = fqdist(dx, dz)?.0;
            if 0.001 < d {
                let s = div(sub(add(radius, o.f32_(0x828))?, d)?, d)?;
                *out = [add(mul(s, dx)?, to[0])?, add(mul(s, 0.0)?, to[1])?, add(mul(s, dz)?, to[2])?];
            } else {
                *out = from;
            }
        }
        Ok(best)
    }

    /// PushPlayer 0x147cc0: add `d` to the player's push velocity +0x83c.
    pub(crate) fn push_player(&mut self, k: usize, d: V3) -> R<()> {
        let r = &mut self.players[k];
        if r.u16_(0x93c) & 4 == 0 {
            r.set_v3(0x83c, [0.0; 3]);
        }
        let v = r.v3(0x83c);
        r.set_v3(0x83c, vadd(d, v)?);
        r.or_u16(0x93c, 4);
        Ok(())
    }

    /// PlayerCollideEnemies 0x145bf8: enemy index, critter | 0x10000, or −1.
    pub(crate) fn player_collide_enemies(&mut self, radius: f32, height: f32, p: usize, from: V3, to: V3, out: &mut V3) -> R<i32> {
        let mut n = 0;
        let mut best = -1i32;
        let mut bd = 0.0;
        // StartEnemyGrid(to) with the radius still in f12.
        let near = self.enemies_near(radius, to)?;
        for i in near {
            let en = &self.enemies[i];
            let st = en.i32_(e::STATE);
            if !(st == 1 || st == 6 || (st == 8 && self.stop_time != 0)) || en.i32_(e::TYPE) == 0x1f {
                continue;
            }
            let rr = add(radius, en.f32_(e::WIDTH))?;
            let c = en.v3(e::COLLISION);
            let (dx, dz) = (sub(c[0], to[0])?, sub(c[2], to[2])?);
            if !(add(mul(dx, dx)?, mul(dz, dz)?)? < mul(rr, rr)? && sub(c[1], to[1])?.abs() < en.f32_(e::HALF_HEIGHT)) {
                continue;
            }
            let Some(hit) = line_cylinder_collide(c, from, to, rr, add(height, en.f32_(e::HALF_HEIGHT))?, true)? else { continue };
            let d = fqdist(sub(hit[0], to[0])?, sub(hit[2], to[2])?)?.0;
            if best < 0 || d < bd {
                *out = hit;
                best = i as i32;
                bd = d;
            }
            n += 1;
        }
        if let Ok(i) = usize::try_from(best) {
            let c = self.enemies[i].v3(e::COLLISION);
            if self.fast_wall_collide(from, c, 0)?.is_some() {
                best = -1;
            }
        }
        if let Some((k, hit)) = self.player_collide_critters(radius, p, from, to)? {
            let d = fqdist(sub(hit[0], to[0])?, sub(hit[2], to[2])?)?.0;
            if best < 0 || d < bd {
                best = self.crit(k).rec.i16_(0) as i32 | 0x10000;
                *out = hit;
            }
            n += 1;
        }
        if 0 <= best {
            if self.ctl(p).i32_(0x38) != 0 {
                return Ok(best);
            }
            if n == 1 {
                let (c, w) = if best < 0x10000 {
                    let en = &self.enemies[best as usize];
                    (en.v3(e::COLLISION), en.f32_(e::WIDTH))
                } else {
                    let k = (best & 0xffff) as usize;
                    (self.crit(k).rec.v3(super::critters::c::COL), self.critter_type_f32(k, 0x7c))
                };
                let (dx, dz) = (sub(to[0], c[0])?, sub(to[2], c[2])?);
                let d = fqdist(dx, dz)?.0;
                if 0.001 < d {
                    // The width passes through cvt.w.s / cvt.s.w (truncated).
                    let s = div(sub(add(radius, w as i32 as f32)?, d)?, d)?;
                    *out = [add(mul(s, dx)?, to[0])?, add(mul(s, 0.0)?, to[1])?, add(mul(s, dz)?, to[2])?];
                    return Ok(best);
                }
            }
        }
        *out = from;
        Ok(best)
    }

    /// PlayerCollideCritters 0x145b40 (CritterLineCollide kind 2); a kind-4
    /// critter only counts while +0x8ac has 0x8000.
    fn player_collide_critters(&mut self, radius: f32, p: usize, from: V3, to: V3) -> R<Option<(usize, V3)>> {
        let hit = self.critter_line_collide_kind(radius, from, to, None, 2, None)?;
        Ok(match hit {
            Some((k, _)) if self.critter_kind(k) == 4 && self.players[p].u32_(0x8ac) & 0x8000 == 0 => None,
            h => h,
        })
    }

    /// PlayerWorldDamage 0x147910(player, world object, position, contact).
    pub(crate) fn player_world_damage(&mut self, p: usize, o: usize, pos: V3, at: V3) -> R<()> {
        let f = self.wobj_all_flags(Some(o));
        let kind = f & 0xf_0000;
        if kind == 0 || f & 0xa00_0000 == 0x200_0000 || 0x1e < self.players[p].u32_(0x1dc) || self.globals.time < self.players[p].f32_(0x8c4) {
            return Ok(());
        }
        let mut d = [sub(pos[0], at[0])?, 0.0, sub(pos[2], at[2])?];
        normal_vector_2d(&mut d)?;
        let later = add(self.globals.time, 1.0)?;
        match kind {
            0x30000 | 0x40000 | 0x50000 => {
                self.players[p].set_f32(0x8c4, later);
                self.damage_player(15.0, p, 1, 0x20, Some(d))?;
                self.audio("AudioWorldHitPlyr", Some(p), 0);
            }
            0x20000 => {
                self.damage_player(10.0, p, 1, 0x10, Some(d))?;
                self.players[p].set_f32(0x8c4, later);
            }
            _ => {
                self.players[p].set_f32(0x8c4, later);
                self.damage_player(5.0, p, 1, 0, None)?;
            }
        }
        Ok(())
    }

    /// PlayerNewFloor 0x145988(player_floor_col, player, dpos).
    pub(crate) fn player_new_floor(&mut self, p: usize, floor: Option<(crate::collision::Hit, V3)>, d: &mut V3) -> R<i32> {
        let cur = ptr(self.players[p].u32_(0x89c));
        let obj = floor.map(|(h, _)| h.object);
        if let Some(c) = cur {
            let f = self.collision.runtime_flags[c];
            if f & 0xc00_0000 != 0 && f & 0x2000_0000 != 0 && Some(c) != obj {
                *d = [0.0; 3];
                return Ok(0);
            }
        }
        // CopyMat3(floor_col, +0x6a0): the shadow tree's orientation (presentation).
        let res = self.player_check_moving_floor(p, obj, d)?;
        let normal = floor.map_or([0.0; 3], |(_, n)| n);
        let slope = |r: &super::record::Record| -> R<f32> {
            let f = r.v3(0x40);
            let (nx, ny, nz) = (normal[0], normal[1], normal[2]);
            Ok(sub(mul(sub(mul(nx, f[1])?, mul(ny, f[0])?)?, nx)?, mul(sub(mul(ny, f[2])?, mul(nz, f[1])?)?, nz)?)?)
        };
        let moving = obj.is_some_and(|o| self.collision.runtime_flags[o] & 8 != 0);
        if !moving {
            let s = slope(&self.players[p])?;
            self.players[p].set_f32(0x894, s);
        } else {
            let a = fqdist(d[0], d[2])?.0;
            let b = fqdist(a, d[1])?.0;
            if 0.01 < a && 0.01 < b {
                let y = d[1];
                if !(self.players[p].u32_(0x898) & 8 != 0 && y.abs() <= 0.01) {
                    self.players[p].set_f32(0x894, div(y, b)?);
                }
            }
        }
        let f = obj.map_or(0, |o| self.collision.runtime_flags[o]);
        self.players[p].set_u32(0x898, f);
        Ok(res)
    }

    /// PlayerCheckMovingFloor 0x1458a0: parent the player node to a moving
    /// floor object's node (flag 0x1000), unless another player is already
    /// on a different moving object (then the step is refused).
    fn player_check_moving_floor(&mut self, p: usize, obj: Option<usize>, d: &mut V3) -> R<i32> {
        let mut refused = false;
        if let Some(o) = obj
            && self.collision.runtime_flags[o] & 0x1000 != 0
            && self.collision.objects[o].parent.is_some()
        {
            if self.other_player_on_other_moving_object(p, o) {
                refused = true;
            } else {
                self.player_floor_parent[p] = Some(o);
            }
        }
        let keep = if refused { ptr(self.players[p].u32_(0x89c)) } else { obj };
        if keep.is_none_or(|o| self.collision.runtime_flags[o] & 0x1000 == 0) {
            self.player_floor_parent[p] = None;
        }
        if obj != ptr(self.players[p].u32_(0x89c)) {
            self.players[p].or_u16(0x93c, 1);
        }
        self.players[p].set_u32(0x89c, obj.map_or(0, |o| o as u32 + 1));
        if refused {
            d[0] = 0.0;
            d[2] = 0.0;
        }
        Ok(i32::from(refused))
    }

    fn other_player_on_other_moving_object(&self, p: usize, o: usize) -> bool {
        (0..4).any(|k| {
            let r = &self.players[k];
            k != p
                && r.i32_(0xfc) == 1
                && ptr(r.u32_(0x89c)).is_some_and(|f| f != o && self.collision.objects[f].parent.is_some() && self.collision.runtime_flags[f] & 0x4000 != 0)
        })
    }

    /// DoTransporter 0x146758: 0 nothing, 1 fading out (no motion), 2
    /// arrived (`d` moves to the destination).
    pub(crate) fn do_transporter(&mut self, radius: f32, p: usize, pos: V3, d: &mut V3) -> R<i32> {
        let t = self.players[p].i32_(0x918);
        if t < 1 {
            let trans = self.player_collis_trans(p);
            let wait = self.players[p].i32_(0x914);
            let Some(dest_item) = trans else {
                if 0 < wait {
                    self.players[p].set_i32(0x914, wait - 1);
                }
                return Ok(0);
            };
            if 0 < wait {
                return Ok(0);
            }
            let dest = self.items.records[dest_item].v3(0x40);
            self.players[p].set_v3(0x91c, dest);
            if !self.point_visible(-radius, dest)? {
                return Ok(0);
            }
            let (c, _) = self.collision.floor_collide(radius, 4.0, -10.0, dest, false, 1)?;
            if c.solid.is_none() {
                return Ok(1);
            }
            self.audio("AudioTransport", Some(p), 0);
            self.players[p].set_i32(0x918, 0x3c);
            return Ok(1);
        }
        let n = (t - 2 * self.globals.vb_elapsed).max(0);
        self.players[p].set_i32(0x918, n);
        // MBTreeSetAlpha fade (presentation): n < 0x1e fades in, else out.
        if t < 0x1e {
            return Ok(1);
        }
        if n < 0x1e {
            let dest = self.players[p].v3(0x91c);
            let (c, _) = self.collision.floor_collide(radius, 4.0, -10.0, dest, false, 1)?;
            let y = c.solid.map_or(dest[1], |h| h.position[1]);
            self.transporter_floor_y = Some(y);
            d[0] = sub(dest[0], pos[0])?;
            d[2] = sub(dest[2], pos[2])?;
            d[1] = sub(y, self.players[p].f32_(0x54))?;
            self.players[p].set_i32(0x914, 1);
            self.help(9, p);
            return Ok(2);
        }
        Ok(1)
    }

    /// player_collis_trans: the transporter (+0x884, kind 0xb) destination
    /// item (+0xf8) when it is shown and active.
    fn player_collis_trans(&self, p: usize) -> Option<usize> {
        let it = ptr(self.players[p].u32_(0x884))?;
        let r = &self.items.records[it];
        if self.items.definitions[r.definition()].kind() != 0xb {
            return None;
        }
        let dest = ptr(r.u32_(0xf8))?;
        let dr = &self.items.records[dest];
        if dr.i16_(0xd8) != -1 && dr.u8_(0xe1) == 0 && dr.u16_(0xd8) & 0x4000 != 0 {
            Some(dest)
        } else {
            None
        }
    }

    /// PointVisible(radius, point): the camera frustum test of the culling view.
    fn point_visible(&self, radius: f32, at: V3) -> R<bool> {
        match &self.camera {
            Some(c) => c.sphere_visible(radius, at),
            None => unsupported("PointVisible without a camera view"),
        }
    }

    /// PlayerKnockback 0x147580(facing, player, &angle): the hit reaction
    /// code (0, 1..3, 10/11, 0x14/0x15, 0x1e/0x1f, 200, 300, 0x12d).
    pub(crate) fn player_knockback(&mut self, facing: f32, p: usize, angle: &mut f32) -> R<i32> {
        let r = &mut self.players[p];
        if 0.0 < r.f32_(0x8b8) {
            r.set_f32(0x8b8, 0.0);
        }
        let prev = r.u32_(0x8b0);
        let flags = r.u32_(0x8ac);
        r.set_u32(0x8b0, flags);
        if flags & 0x4000 != 0 {
            r.set_u32(0x8ac, 0x4000);
            return Ok(300);
        }
        if flags & 0x8000 != 0 {
            r.set_u32(0x8ac, flags & 0x8000);
            if prev & 0x8000 == 0 {
                let k = r.v3(0x8b4);
                r.set_v3(0x848, k);
            }
            return Ok(0x12d);
        }
        if prev & 0x8000 != 0 {
            let dmg = r.f32_(0x8a8);
            self.damage_player(dmg, p, 1, 0, None)?;
            self.players[p].set_f32(0x8a8, 0.0);
        }
        let r = &mut self.players[p];
        if r.u32_(0x134) & 0x10000 != 0 {
            r.set_f32(0x8a8, 0.0);
            r.set_u32(0x8ac, 0);
            return Ok(0);
        }
        let flags = r.u32_(0x8ac);
        let mut code = if flags & 0x400_0000 != 0 { 200 } else { 0 };
        let clear = |r: &mut super::record::Record| {
            r.set_f32(0x8b8, 0.0);
            r.set_f32(0x8bc, 0.0);
            r.set_f32(0x8a8, 0.0);
            r.set_u32(0x8ac, 0);
        };
        if r.f32_(0x8a8) <= 1.0 {
            if flags & 0x80 != 0 {
                code = 2;
            }
            r.set_f32(0x8b4, 0.0);
            clear(r);
            return Ok(code);
        }
        let push = |r: &mut super::record::Record, k: f32| -> R<()> {
            let (kn, v) = (r.v3(0x8b4), r.v3(0x848));
            r.set_v3(0x848, [add(mul(kn[0], k)?, v[0])?, add(mul(kn[1], k)?, v[1])?, add(mul(kn[2], k)?, v[2])?]);
            Ok(())
        };
        if flags & 0x10000 != 0 {
            code = 0x1e;
            push(r, 100.0)?;
        } else if flags & 0x40 != 0 {
            code = 0x14;
            push(r, 100.0)?;
        } else if flags & 0x120 != 0 {
            code = 0x14;
            push(r, if r.u32_(0x138) & 0x400 == 0 { 32.0 } else { 80.0 })?;
        } else if flags & 0x10 != 0 {
            code = 10;
            push(r, 16.0)?;
        } else {
            code = if flags & 0x2000 != 0 {
                3
            } else if flags & 0x80 != 0 {
                2
            } else {
                1
            };
        }
        if 10 <= code {
            let k = r.v3(0x8b4);
            let mut a = atan2f(k[0], k[2])?;
            let rel = wrap(sub(a, facing)?)?;
            if f32::from_bits(0x3fc9_0fdb) < rel.abs() {
                a = add(a, PI)?;
                code += 1;
            }
            *angle = wrap(a)?;
        }
        // AddPlayerHitCol (unless 0x1000000) and SetSkinFX(TextureWhite) are
        // presentation.
        r.set_f32(0x8b4, 0.0);
        clear(r);
        Ok(code)
    }

    /// PlayerCheckFloor 0x1451a0 (game_mode 0x4010: FloorCollide at +0x50).
    pub(crate) fn player_check_floor(&mut self, p: usize) -> R<bool> {
        if self.globals.game_mode == 0x4010 {
            let r = &self.players[p];
            let (rad, h, pos) = (r.f32_(0x828), r.f32_(0x82c), r.v3(0x50));
            let (c, _) = self.collision.floor_collide(rad, 0.0, -add(h, 3.0)?, pos, false, 1)?;
            self.players[p].set_u32(0x89c, c.solid.map_or(0, |h| h.object as u32 + 1));
            self.players[p].or_u16(0x93c, 1);
        }
        Ok(self.players[p].u32_(0x89c) != 0)
    }

    /// PlayerUsePowerup 0x12c2e0(amount, player, kind, mask): the first
    /// powerup slot of `kind` whose flags meet `mask` loses `amount` (all
    /// of it when negative); returns its truncated remaining value before
    /// the use (1 for an unlimited slot), 0 when none matches.
    pub(crate) fn player_use_powerup(&mut self, amount: f32, p: usize, kind: i32, mask: u32) -> R<i32> {
        let r = &self.players[p];
        let Some(i) = (0..8).find(|i| {
            let o = 0x144 + 0x10 * i;
            r.f32_(o) != 0.0 && r.i32_(o + 4) == kind && mask & r.u32_(o + 0xc) != 0
        }) else {
            return Ok(0);
        };
        let o = 0x144 + 0x10 * i;
        let v = r.f32_(o + 8) as i32;
        if v < 0 {
            return Ok(1);
        }
        if self.globals.cur_world == 0xd {
            return Ok(v);
        }
        let left = if amount < 0.0 { 0.0 } else { sub(r.f32_(o + 8), amount)? };
        let r = &mut self.players[p];
        r.set_f32(o + 8, left);
        // Also clears a NaN amount, as the original's `0.0 < x` test fails.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !(0.0 < left) {
            r.set_f32(o, 0.0);
        }
        Ok(v)
    }

    /// DoAutoExit 0x1465c8.
    pub(crate) fn do_auto_exit_player(&mut self, p: usize) -> bool {
        let g = &self.globals;
        if g.skip_wave != 0 && (g.someone_wants_to_exit != 0 || g.auto_exit_flag != 0) {
            self.players[p].set_i32(0xfc, 4);
            self.players[p].set_i16(0x1ca, 0);
            return true;
        }
        false
    }
}
