//! Non-boss critter movement collision (CritterTranslate 0x197110 for
//! golems, gargoyles and generals): CritterCollideWorld 0x19cc30,
//! CritterCollideItems 0x19d3b0, CritterCollidePlayers 0x19d118,
//! CritterCollideEnemies 0x19d670, CritterCollideCritters 0x19d930, with
//! CritterMoveNodeCol 0x19a610, CritterColItem 0x159e20 and
//! CritterWorldDamage 0x19cad0. TYPE +0x7c radius, +0x78 height; TYPE +0x5c
//! flag 0x100 tests the collision nodes (+0x540 + 0x60k, position +0x40)
//! instead of the +0x60 centre. The last floor hit re-parents the +0x70
//! node (a flag-0x1000 object, else EnemyRootNode); the shadow matrix is
//! presentation.
use super::critters::{c, CritterFile};
use super::enemies::e;
use super::{Game, R};
use crate::collision::Query;
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub};
use crate::player_collision::{hit_normal, slide_along_wall};

type V3 = [f32; 3];

fn v_add(a: V3, b: V3) -> R<V3> {
    Ok([add(a[0], b[0])?, add(a[1], b[1])?, add(a[2], b[2])?])
}

fn v_sub(a: V3, b: V3) -> R<V3> {
    Ok([sub(a[0], b[0])?, sub(a[1], b[1])?, sub(a[2], b[2])?])
}

impl Game {
    /// Live collision nodes for movement: tree node present, health left
    /// (+0x5c < +0x58) and NODE +0x10 flag 8; (index, position, NODE +0x2c).
    fn critter_move_nodes(&self, i: usize) -> Vec<(usize, V3, f32)> {
        let t = self.crit(i).ty;
        let first = self.critter_type_i16(i, 0x11a) as usize;
        let mut out = Vec::new();
        for k in 0..self.crit(i).colnode_nodes.len() {
            if self.crit(i).colnode_nodes[k].0.is_none() {
                continue;
            }
            let at = c::COLNODES + 0x60 * k;
            if self.crit(i).rec.f32_(at + 0x58) <= self.crit(i).rec.f32_(at + 0x5c) {
                continue;
            }
            let nrec = &self.critter_data.files[t.0].wad.nodes[first + k];
            if CritterFile::i16_pub(nrec, 0x10) as u16 & 8 == 0 {
                continue;
            }
            out.push((k, self.crit(i).rec.v3(at + 0x40), CritterFile::f32_pub(nrec, 0x2c)));
        }
        out
    }

    /// CritterTranslate's collision for kinds other than 4; true when a
    /// player was pushed.
    pub(crate) fn critter_collide_move(&mut self, i: usize, mv: &mut V3) -> R<bool> {
        let w = self.critter_collide_world(i, mv)?;
        let it = self.critter_collide_items(i, mv)?;
        let pl = self.critter_collide_players(i, mv)?;
        // The original sums the results as the next passes' (unused) count.
        let _ = (w, it);
        self.critter_collide_enemies(i, mv)?;
        self.critter_collide_critters(i, mv)?;
        Ok(pl != 0)
    }

    /// CritterWorldDamage: world objects with damage kinds hurt the critter
    /// (5 / 5 flag 0x10 / 15 flag 0x20), knocked away from the contact.
    fn critter_world_damage(&mut self, i: usize, object: usize, pos: V3, contact: V3) -> R<()> {
        let mut flags = 0u32;
        let mut o = Some(object);
        while let Some(k) = o {
            flags |= self.items.objects[k].flags_10;
            o = self.collision.objects[k].parent;
        }
        let kind = flags & 0xf_0000;
        if kind == 0 || flags & 0xa00_0000 == 0x200_0000 {
            return Ok(());
        }
        let mut v = [sub(pos[0], contact[0])?, 0.0, sub(pos[2], contact[2])?];
        crate::item_collision::normal_vector_2d(&mut v)?;
        let (d, fl) = match kind {
            0x10000 => (5.0, 0),
            0x20000 => (5.0, 0x10),
            0x30000 | 0x40000 | 0x50000 => (15.0, 0x20),
            _ => (0.0, 0),
        };
        if 0.0 < d {
            self.critter_damage(d, i, -1, fl, Some(contact), Some(v), 1)?;
        }
        Ok(())
    }

    /// CritterCollideWorld: wall slide (centre or first hitting node), then
    /// the floor ahead; returns the +0x480 result (2 stopped at a wall, 0x10
    /// no floor).
    fn critter_collide_world(&mut self, i: usize, mv: &mut V3) -> R<u32> {
        let r = self.critter_type_f32(i, 0x7c);
        let h = self.critter_type_f32(i, 0x78);
        let col = self.crit(i).rec.v3(c::COL);
        let floor_min = mul(self.globals.spf, -16.0)?;
        let wall = |g: &mut Game, radius: f32, from: V3, to: V3| -> R<Option<crate::collision::Hit>> {
            let (cl, _) = g.collision.world_collide(Query {
                radius,
                first: from,
                last: to,
                check_flags: 7,
                min_normal_y: f32::from_bits(0xbf5d_b22d),
                max_normal_y: f32::from_bits(0x3f5d_b22d),
                object_mask: 0x13a,
                exclusion_mask: 2,
            })?;
            g.enemy_wall_hit = cl.solid.map(|h| h.object);
            Ok(cl.solid)
        };
        let mut hit = None;
        let mut from_used = col;
        if self.critter_type_u32(i, 0x5c) & 0x100 == 0 {
            let to = v_add(col, *mv)?;
            hit = wall(self, r, col, to)?;
        } else {
            for (_, p, nr) in self.critter_move_nodes(i) {
                let to = v_add(p, *mv)?;
                if let Some(hh) = wall(self, nr, p, to)? {
                    hit = Some(hh);
                    from_used = p;
                    break;
                }
            }
        }
        let mut result = 0u32;
        if let Some(hh) = hit {
            self.critter_world_damage(i, hh.object, col, hh.position)?;
            if self.items.objects[hh.object].flags_10 & 0x38 == 0 {
                let n = hit_normal(&self.collision, &hh)?;
                if slide_along_wall(r, from_used, mv, hh.position, n)? < 0 {
                    mv[0] = 0.0;
                    mv[2] = 0.0;
                    result = 2;
                }
            }
        }
        let mut d = *mv;
        let len = crate::collision::normal_vector_pub(&mut d)?;
        let reach = add(r, len)?;
        let probe = [add(mul(reach, d[0])?, col[0])?, add(mul(reach, d[1])?, col[1])?, add(mul(reach, d[2])?, col[2])?];
        let down = sub(-h, 3.0)?;
        let (f1, _) = self.collision.floor_collide(1.0, h, down, probe, false, 2)?;
        let mut ok = true;
        // The last FloorCollide's hit (the original's lVar9).
        let mut last = f1.solid;
        match f1.solid {
            None => ok = false,
            Some(fh) => {
                self.critter_world_damage(i, fh.object, col, fh.position)?;
                let fy = fh.position[1];
                let base = sub(self.crit(i).rec.f32_(c::POS + 4), self.critter_type_f32(i, 0xb0))?;
                let step = sub(fy, base)?.abs();
                let mut y = fy;
                if step <= add(reach, reach)? {
                    if 0.0 < len && mul(len, f32::from_bits(0x3dcc_cccd))? < step {
                        let p2 = v_add(col, *mv)?;
                        let (f2, _) = self.collision.floor_collide(1.0, h, down, p2, false, 2)?;
                        last = f2.solid;
                        match f2.solid {
                            Some(h2) => y = h2.position[1],
                            None => {
                                ok = false;
                                y = fy;
                            }
                        }
                    }
                } else {
                    ok = false;
                }
                if !ok {
                    let (f3, _) = self.collision.floor_collide(1.0, h, down, col, false, 2)?;
                    last = f3.solid;
                    y = f3.solid.map_or(base, |h3| h3.position[1]);
                }
                let mut dy = sub(y, base)?;
                if dy < floor_min {
                    dy = floor_min;
                }
                mv[1] = add(dy, mv[1])?;
            }
        }
        if !ok {
            mv[0] = 0.0;
            mv[2] = 0.0;
            result |= 0x10;
        }
        self.crm_pub(i).set_u32(0x480, result);
        // MBNodeSetParent(+0x70, the floor object's node when its +0x28 node
        // exists and flag 0x1000 is set, else EnemyRootNode).
        if let Some(fh) = last {
            let moving = self.collision.runtime_flags.get(fh.object).is_some_and(|f| f & 0x1000 != 0);
            self.crit_mut(i).floor = moving.then_some(fh.object);
        }
        Ok(result)
    }

    /// CritterColItem: 0 ignore, 1 block, 2 pass (golems/gargoyles through
    /// chests), 3 block by damaging the item (golems/gargoyles).
    fn critter_col_item(&self, i: usize, slot: usize) -> u8 {
        let r = &self.items.records[slot];
        let d = &self.items.definitions[r.definition()];
        let k = self.critter_kind(i);
        let big = k == 3 || k == 7;
        match d.kind() {
            1 | 5 | 8 | 9 | 11 | 12 => 0,
            2 => match (d.subtype() == 0x2b, big) {
                (false, true) => 2,
                (true, true) => 3,
                _ => 1,
            },
            3 => {
                if 3.0 < d.f32_(0x10) {
                    u8::from(r.u8_(0xf6) != 0)
                } else if big {
                    3
                } else {
                    0
                }
            }
            10 => match d.subtype() {
                0x28 | 0x31 | 0x33..=0x35 => 0,
                0x2b..=0x2d if big => 3,
                _ => 1,
            },
            _ => 1,
        }
    }

    /// CritterCollideItems: blocking items stop the x/z move; damaged ones
    /// (CritterColItem 3) block only while DamageItem reports damage. The
    /// node path keeps the previous item's distance when no node is tested.
    fn critter_collide_items(&mut self, i: usize, mv: &mut V3) -> R<u32> {
        let r = self.critter_type_f32(i, 0x7c);
        let h = self.critter_type_f32(i, 0x78);
        let col = self.crit(i).rec.v3(c::COL);
        let probe = v_add(col, *mv)?;
        let near = self.grid.items_near(&self.items, r, probe)?;
        let env = self.collide_env();
        let mut d = -1.0f32;
        for slot in near {
            let ci = self.critter_col_item(i, slot);
            if ci == 0 {
                continue;
            }
            let mut out = [0.0; 3];
            if self.critter_type_u32(i, 0x5c) & 0x100 == 0 {
                let to = v_add(col, *mv)?;
                d = crate::item_collision::item_coll_sub(&self.items, &mut self.collision, &env, slot, col, to, Some(&mut out), r, h)?;
            } else {
                for (_, p, nr) in self.critter_move_nodes(i) {
                    let to = v_add(p, *mv)?;
                    d = crate::item_collision::item_coll_sub(&self.items, &mut self.collision, &env, slot, p, to, Some(&mut out), nr, nr)?;
                    if 0.0 <= d {
                        break;
                    }
                }
            }
            let mut blocked = false;
            if 0.0 <= d && ci != 2 {
                if ci == 3 {
                    let dmg = mul(self.critter_type_f32(i, 0xb8), self.level_f32(0xbc))?;
                    blocked = self.damage_item(dmg, slot, 0, -1)? != 0.0;
                } else {
                    blocked = true;
                }
            }
            if blocked {
                mv[0] = 0.0;
                mv[2] = 0.0;
            }
        }
        Ok(0)
    }

    /// CritterMoveNodeCol: the first (or nearest in x/z to `target`) node
    /// whose moved cylinder (r/h + NODE +0x2c) hits `target`; node + 1.
    #[allow(clippy::too_many_arguments)]
    fn critter_move_node_col(&mut self, i: usize, r: f32, h: f32, mv: V3, target: V3, out: &mut V3, first: bool) -> R<usize> {
        let mut best = -1.0f32;
        let mut pick = None;
        for (k, p, nr) in self.critter_move_nodes(i) {
            let to = v_add(p, mv)?;
            let Some(hp) = super::enemies::line_cylinder_collide(target, p, to, add(r, nr)?, add(h, nr)?, true)? else { continue };
            if first {
                *out = hp;
                return Ok(k + 1);
            }
            let dd = crate::vu0::fqdist(sub(target[0], to[0])?, sub(target[2], to[2])?)?.0;
            if pick.is_none() || dd < best {
                best = dd;
                pick = Some(k);
                *out = hp;
            }
        }
        Ok(pick.map_or(0, |k| k + 1))
    }

    /// CritterCollidePlayers: players in the way are pushed (+0x848 += 2 ×
    /// the overlap-scaled direction, scale clamped to [1, 3]); the move stops
    /// when the last tested player was hit. Returns the hit count.
    fn critter_collide_players(&mut self, i: usize, mv: &mut V3) -> R<i32> {
        let r = self.critter_type_f32(i, 0x7c);
        let h = self.critter_type_f32(i, 0x78);
        let col = self.crit(i).rec.v3(c::COL);
        let probe = v_add(col, *mv)?;
        let nodes = self.critter_type_u32(i, 0x5c) & 0x100 != 0;
        let mut hits = 0;
        let mut last = false;
        for p in 0..4 {
            let st = self.players[p].i32_(0xfc);
            if !(st == 1 || st == 4) || self.players[p].u16_(0x93c) & 0x20 != 0 {
                continue;
            }
            let pr = self.players[p].f32_(0x828);
            let ph = self.players[p].f32_(0x82c);
            let rr = add(r, pr)?;
            let ppos = self.players[p].v3(0x70);
            let mut out = [0.0; 3];
            let push = if nodes {
                let k = self.critter_move_node_col(i, pr, ph, *mv, ppos, &mut out, false)?;
                last = k != 0;
                if k == 0 {
                    continue;
                }
                v_sub(ppos, self.crit(i).rec.v3(c::COLNODES + 0x60 * (k - 1) + 0x40))?
            } else {
                let hit = super::enemies::line_cylinder_collide(ppos, col, probe, rr, add(h, ph)?, true)?;
                last = hit.is_some();
                if hit.is_none() {
                    continue;
                }
                v_sub(ppos, probe)?
            };
            hits += 1;
            let mut v = push;
            let len = crate::collision::normal_vector_pub(&mut v)?;
            let s = sub(rr, len)?.clamp(1.0, 3.0);
            let v = [mul(s, v[0])?, mul(s, v[1])?, mul(s, v[2])?];
            let cur = self.players[p].v3(0x848);
            let nv = [add(add(v[0], v[0])?, cur[0])?, add(add(v[1], v[1])?, cur[1])?, add(add(v[2], v[2])?, cur[2])?];
            self.players[p].set_v3(0x848, nv);
        }
        if last {
            mv[0] = 0.0;
            mv[2] = 0.0;
        }
        Ok(hits)
    }

    /// CritterCollideEnemies: the nearest enemy in the way stops the move,
    /// except that golems damage small enemies (half height ≤ 2) instead.
    fn critter_collide_enemies(&mut self, i: usize, mv: &mut V3) -> R<u32> {
        let r = self.critter_type_f32(i, 0x7c);
        let h = self.critter_type_f32(i, 0x78);
        let col = self.crit(i).rec.v3(c::COL);
        let probe = v_add(col, *mv)?;
        let nodes = self.critter_type_u32(i, 0x5c) & 0x100 != 0;
        let mut best: Option<(usize, f32, V3)> = None;
        for k in self.enemies_near(r, probe)? {
            let en = &self.enemies[k];
            let st = en.i32_(e::STATE);
            if !(st == 1 || st == 6 || (st == 8 && self.stop_time != 0)) || en.i32_(e::TYPE) == 0x1f {
                continue;
            }
            let rr = add(r, en.f32_(e::WIDTH))?;
            let hh = add(h, en.f32_(e::HALF_HEIGHT))?;
            let centre = en.v3(e::COLLISION);
            let mut out = [0.0; 3];
            let hit = if nodes {
                self.critter_move_node_col(i, rr, hh, *mv, centre, &mut out, true)? != 0
            } else {
                let dd = v_sub(centre, probe)?;
                if mul(rr, rr)? < add(mul(dd[0], dd[0])?, mul(dd[2], dd[2])?)? {
                    continue;
                }
                match super::enemies::line_cylinder_collide(centre, col, probe, rr, hh, true)? {
                    Some(p) => {
                        out = p;
                        true
                    }
                    None => false,
                }
            };
            if !hit {
                continue;
            }
            let dist = crate::vu0::fqdist(sub(out[0], probe[0])?, sub(out[2], probe[2])?)?.0;
            if best.is_none_or(|b| dist < b.1) {
                best = Some((k, dist, out));
            }
        }
        let Some((k, _, at)) = best else { return Ok(0) };
        if self.critter_kind(i) == 3 && self.enemies[k].f32_(e::HALF_HEIGHT) <= 2.0 {
            let dmg = mul(self.critter_type_f32(i, 0xb8), self.level_f32(0xbc))?;
            self.damage_enemy(dmg, k, -1, 0, Some(at), None, 1)?;
            return Ok(0);
        }
        mv[0] = 0.0;
        mv[2] = 0.0;
        Ok(1)
    }

    /// CritterCollideCritters: CritterLineCollide (mode 1, skipping this
    /// instance) from the centre to the moved centre stops the x/z move.
    fn critter_collide_critters(&mut self, i: usize, mv: &mut V3) -> R<bool> {
        let r = self.critter_type_f32(i, 0x7c);
        let col = self.crit(i).rec.v3(c::COL);
        let probe = v_add(col, *mv)?;
        let hit = self.critter_line_collide_kind(r, col, probe, None, 1, Some(i))?.is_some();
        if hit {
            mv[0] = 0.0;
            mv[2] = 0.0;
        }
        Ok(hit)
    }
}
