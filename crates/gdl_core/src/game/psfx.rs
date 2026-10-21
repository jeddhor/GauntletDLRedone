//! Player class effects (research/player.md): PlyrSfxDoDamage 0x14edd8,
//! PlyrSfxDoDamageSub 0x14e7c0, DoPlyrSfx 0x14e4d0, DoPlyrSfxSub 0x14e1f8,
//! CalcPlayerSfxVel 0x14e728, YawVec3 0x1d5a70, PitchMat3 0x1d5440,
//! PlaceEffectOnFloor 0x17ac30, SfxSetHit 0x17df50, SfxSetMorph 0x17e1b8.
//!
//! PlayerData[k] +4 is the class WAD's SFXX table (0x50 bytes per entry) and
//! +8 its DAMG table (0x58 per entry); both come from PDATA/<CLS>.WAD
//! ([`super::pickups::ClassData`]). DAMG fields (bytes): +0 type, +2 flags,
//! +4 effect damage flags, +8 radius, +0xc blast, +0x14 fade / missile
//! count, +0x18 linger, +0x1c morph time, +0x20 yaw, +0x24 cone, +0x28
//! pitch, +0x2c..+0x34 offset (missile scale), +0x38 damage (< 0: × player
//! +0x118), +0x3c/+0x40 launch speeds, +0x44 gravity, +0x48 SFXX index,
//! +0x4a hit SFXX, +0x4c morph SFXX, +0x4e next DAMG, +0x50/+0x52 first and
//! last frame, +0x54 help id. SFXX: +0 flags, +4 next, +8 effect header,
//! +0xc sound, +0x34 offset, +0x40 time, +0x4c colour.
//!
//! HYPOTHESIS: an effect parented to the player's node (SfxSetParent of
//! node +0x80, its child +0x78 or its parent +0x74) follows the player's
//! record matrix and position; the animated child node's own pose is not
//! applied. Lights, streaks, colours, ambient flashes, texture-mod nodes and
//! particles are presentation (events or omitted).
use super::effects::{fx, EffectParent, Visual};
use super::{Event, Game, Unsupported, R};
use crate::libm::{cosf, sinf};
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sub_guarded_rz as sub};
/// plight_color 0x2341f8: per class base the player effects' light colour.
const PLIGHT_COLOR: [[f32; 3]; 8] = [[2.0, 0.0, 0.0], [0.0, 0.0, 2.0], [0.0, 2.0, 2.0], [0.0, 2.0, 0.0], [2.0, 0.0, 0.0], [0.0, 0.0, 2.0], [0.0, 2.0, 2.0], [0.0, 2.0, 0.0]];


type V3 = [f32; 3];

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}
fn i16_at(b: &[u8], o: usize) -> i16 {
    i16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}

/// YawVec3 0x1d5a70: (x cos − z sin, y, z cos + x sin).
pub(crate) fn yaw_vec3(a: f32, v: V3) -> R<V3> {
    let (c, s) = (cosf(a)?, sinf(a)?);
    Ok([sub(mul(v[0], c)?, mul(v[2], s)?)?, v[1], add(mul(v[2], c)?, mul(v[0], s)?)?])
}

/// PitchVec3 0x1d5ae8.
pub(crate) fn pitch_vec3(a: f32, v: V3) -> R<V3> {
    let (c, s) = (cosf(a)?, sinf(a)?);
    let [x, y, z] = v;
    Ok([
        add(mul(mul(x, y)?, s)?, mul(x, c)?)?,
        add(mul(-add(mul(x, x)?, mul(z, z)?)?, s)?, mul(y, c)?)?,
        add(mul(mul(y, z)?, s)?, mul(z, c)?)?,
    ])
}

/// PitchMat3 0x1d5440 on rows 1 and 2.
pub(crate) fn pitch_mat3(m: &mut [V3; 3], a: f32) -> R<()> {
    if a.abs() < f32::from_bits(0x38d1_b717) {
        return Ok(());
    }
    let (s, c) = (sinf(a)?, cosf(a)?);
    let [_, r1, r2] = m;
    for (y, z) in r1.iter_mut().zip(r2.iter_mut()) {
        let f3 = *z;
        *z = add(mul(c, f3)?, mul(s, *y)?)?;
        *y = sub(mul(c, *y)?, mul(s, f3)?)?;
    }
    Ok(())
}

/// MulVecMat3: v × rows 0..2.
fn mul_vec_mat3(v: V3, m: &[V3; 3]) -> R<V3> {
    let col = |c: usize| -> R<f32> { Ok(add(add(mul(v[0], m[0][c])?, mul(v[1], m[1][c])?)?, mul(v[2], m[2][c])?)?) };
    Ok([col(0)?, col(1)?, col(2)?])
}

impl Game {
    fn player_rows(&self, p: usize) -> [V3; 3] {
        let r = &self.players[p];
        [r.v3(0x20), r.v3(0x30), r.v3(0x40)]
    }

    fn psfx(&self, p: usize, i: usize) -> R<Vec<u8>> {
        let t = &self.classes[self.players[p].u32_(0) as usize].sfxx;
        t.get(i * 0x50..(i + 1) * 0x50).map(<[u8]>::to_vec).ok_or_else(|| Unsupported(format!("class SFXX entry {i} not loaded")))
    }

    fn pdamg(&self, p: usize, i: usize) -> R<Vec<u8>> {
        let t = &self.classes[self.players[p].u32_(0) as usize].damg;
        t.get(i * 0x58..(i + 1) * 0x58).map(<[u8]>::to_vec).ok_or_else(|| Unsupported(format!("class DAMG entry {i} not loaded")))
    }

    /// PlyrSfxDoDamage(previous frame, frame, player, DAMG index, partner, extra).
    pub(crate) fn plyr_sfx_do_damage(&mut self, prev: f32, frame: f32, p: usize, d: i16, partner: Option<usize>, extra: Option<V3>) -> R<()> {
        let mut d = d;
        let mut depth = 0;
        while 0 <= d {
            depth += 1;
            if 64 < depth {
                return super::unsupported("player DAMG chain cycle");
            }
            let rec = self.pdamg(p, d as usize)?;
            let first = i16_at(&rec, 0x50) as f32;
            let last = i16_at(&rec, 0x52) as f32;
            let flags = i16_at(&rec, 2) as u16;
            if first <= frame && (last < 0.0 || prev < last) {
                let amb = if flags & 0x2000 != 0 {
                    Some(0xbf4c_cccd)
                } else if flags & 0x20 != 0 {
                    Some(0xbf19_999a)
                } else if flags & 0x10 != 0 {
                    Some(0xbecc_cccd)
                } else {
                    None
                };
                if let Some(v) = amb {
                    // SetAmbientSpecial(1/30, value): the screen flash.
                    self.set_ambient_special(f32::from_bits(0x3d08_8889), f32::from_bits(v))?;
                    self.players[p].set_f32(0x7d4, 2.0);
                    if let Some(k) = partner {
                        self.players[k].set_f32(0x7d4, 2.0);
                    }
                }
            }
            let mut until = if 0.0 <= last { add(last, 1.0)? } else { first };
            if prev < 1.0 && 1.0 <= frame {
                let help = i16_at(&rec, 0x54);
                if 0 <= help {
                    self.help(i32::from(help), p);
                }
            }
            // +0x400: the +0x6b8 tree flags (presentation; no native tree).
            if flags & 0x80 != 0 {
                until = first;
            }
            if first <= frame && prev < until {
                if f32_at(&rec, 0x38) != 0.0 {
                    let t = sub(self.players[p].f32_(0x800), self.players[p].f32_(0x8e8))?;
                    self.players[p].set_f32(0x800, t);
                    self.players[p].set_f32(0x8e8, 0.0);
                }
                self.partner_parent = partner;
                match u16::from_le_bytes([rec[0], rec[1]]) {
                    0 | 5 => {}
                    2 => {
                        self.plyr_sfx_do_damage_sub(p, &rec, false, extra)?;
                    }
                    10 => {
                        if frame != prev {
                            self.plyr_sfx_missiles(p, &rec, frame, first, until)?;
                        }
                    }
                    _ => {
                        self.plyr_sfx_do_damage_sub(p, &rec, true, extra)?;
                    }
                }
            }
            d = i16_at(&rec, 0x4e);
        }
        Ok(())
    }

    /// The type-10 missile volley of PlyrSfxDoDamage.
    fn plyr_sfx_missiles(&mut self, p: usize, rec: &[u8], frame: f32, first: f32, until: f32) -> R<()> {
        let flags = self.players[p].u32_(0x130) & 0xffb7_ffff;
        let count = f32_at(rec, 0x14);
        let yaw = f32_at(rec, 0x20);
        let fwd = self.players[p].v3(0x40);
        let mut n = 0;
        if first == until {
            if count == 0.0 {
                let mut dir = yaw_vec3(yaw, fwd)?;
                n = self.player_start_missile(-1.0, 2.0, p, &mut dir, flags, 0)?;
            } else {
                let mut t = 0.0f32;
                while t < 1.0 {
                    let d = yaw_vec3(mul(t, yaw)?, fwd)?;
                    let mut dir = [mul(d[0], f32::from_bits(0x3e99_999a))?, mul(d[1], 1.5)?, mul(d[2], f32::from_bits(0x3e99_999a))?];
                    n += self.player_start_missile(5.0, 2.0, p, &mut dir, flags, 0)?;
                    t = add(t, div(1.0, count)?)?;
                }
            }
        } else {
            if 0.0 < count {
                // fmodf is exact (the IEEE remainder of finite operands).
                let m = sub(frame, first)? % count;
                if !m.is_finite() || m as i32 != 0 {
                    return Ok(());
                }
            }
            let f16 = u16::from_le_bytes([rec[2], rec[3]]);
            let mut a = yaw;
            if f16 & 0x300 != 0 {
                let k = if first < until { div(sub(frame, first)?, sub(until, first)?)? } else { 1.0 };
                a = mul(a, if f16 & 0x200 == 0 { k } else { sub(1.0, k)? })?;
            }
            let d = yaw_vec3(a, fwd)?;
            let mut dir = [mul(d[0], f32_at(rec, 0x2c))?, f32_at(rec, 0x30), mul(d[2], f32_at(rec, 0x34))?];
            n = self.player_start_missile(-1.0, 2.0, p, &mut dir, flags, 0)?;
            for k in 0..n.max(0) as usize {
                let slot = self.effects.pmissile[k];
                if 0 <= slot {
                    let g = f32_at(rec, 0x44);
                    if 0.0 < g {
                        let v = mul(self.fx(slot as usize).f32_(fx::GRAVITY), g)?;
                        self.fx_mut(slot as usize).set_f32(fx::GRAVITY, v);
                    }
                    // +0x1000: MBRemovePolyInst of the missile trail (presentation).
                }
            }
            if add(first, 1.0)? < frame {
                n = 0;
            }
        }
        if 0 < n && self.players[p].u32_(0x130) & 0x10_0000 != 0 {
            self.player_use_powerup(1.0, p, 5, 0x10_0000)?;
        }
        Ok(())
    }

    /// PlyrSfxDoDamageSub(player, DAMG, local, extra): the class effect and
    /// its damage parameters; returns the effect slot or −1.
    fn plyr_sfx_do_damage_sub(&mut self, p: usize, rec: &[u8], local: bool, extra: Option<V3>) -> R<i32> {
        let rows = self.player_rows(p);
        let off = [f32_at(rec, 0x2c), f32_at(rec, 0x30), f32_at(rec, 0x34)];
        let pos = if !local {
            match extra {
                None => {
                    let w = mul_vec_mat3(off, &rows)?;
                    let base = self.players[p].v3(0x50);
                    [add(w[0], base[0])?, add(w[1], base[1])?, add(w[2], base[2])?]
                }
                Some(e) => {
                    let w = mul_vec_mat3(off, &rows)?;
                    [add(e[0], w[0])?, add(e[1], w[1])?, add(e[2], w[2])?]
                }
            }
        } else {
            match extra {
                None => off,
                Some(_) => return super::unsupported("PlyrSfxDoDamageSub with a body-space extra offset"),
            }
        };
        let sfx = i16_at(rec, 0x48);
        let k = self.do_plyr_sfx(p, sfx, Some(pos), local, -1)?;
        let Ok(k) = usize::try_from(k) else { return Ok(-1) };
        let srec = self.psfx(p, sfx as usize)?;
        let sflags = u32_at(&srec, 0);
        let mut f = match i16_at(rec, 0).wrapping_sub(2) {
            0 => 0xe,
            1 => 0x3a,
            2 | 4 | 5 | 7 => 0x2a,
            _ => 0,
        };
        f |= if self.globals.pvp_mode == 2 { 0x101 } else { 0x300 };
        let f16 = u16::from_le_bytes([rec[2], rec[3]]);
        if f16 & 0x40 != 0 {
            f &= 0xffff_fffb;
        }
        if u32_at(rec, 4) & 0x2_0000 != 0 {
            f |= 0x2_0000;
        }
        self.fx_mut(k).or_u32(fx::FLAGS, f);
        if sflags & 0x10 != 0 && i16_at(rec, 0x4a) < 0 {
            self.place_effect_on_floor(k)?;
        }
        let yaw = f32_at(rec, 0x20);
        let pitch = f32_at(rec, 0x28);
        if let Some(n) = self.effects.nodes[k].as_mut() {
            let m = match &mut n.parent {
                Some(par) => &mut par.local_matrix,
                None => &mut n.matrix,
            };
            if yaw != 0.0 {
                crate::collision::yaw_mat3(m, yaw)?;
            }
            if pitch != 0.0 {
                pitch_mat3(m, pitch)?;
            }
        }
        let mut dmg = f32_at(rec, 0x38);
        if dmg < 0.0 {
            dmg = mul(-dmg, self.players[p].f32_(0x118))?;
        }
        if 0.0 < dmg {
            let atime = self.globals.atime;
            let owner = self.players[p].u32_(0) as i16 + 1;
            let r = self.fx_mut(k);
            r.set_f32(fx::DAMAGE, dmg);
            r.set_f32(fx::CONE, f32_at(rec, 0x24));
            r.set_f32(fx::BLAST, f32_at(rec, 0xc));
            r.set_u32(fx::DAMAGE_FLAGS, u32_at(rec, 4));
            r.set_f32(fx::FADE, f32_at(rec, 0x14));
            if 0.0 < f32_at(rec, 0x18) {
                r.set_f32(fx::LINGER, add(atime, f32_at(rec, 0x18))?);
            }
            r.set_i16(fx::OWNER, owner);
            let hit = i16_at(rec, 0x4a);
            if 0 <= hit {
                let h = self.psfx(p, hit as usize)?;
                self.sfx_set_hit(k, i32::from_le_bytes(h[8..12].try_into().unwrap()) as i16, u32_at(&h, 0xc), u32_at(&h, 0xc));
                if u32_at(&h, 0) & 0x10 != 0 {
                    self.fx_mut(k).or_u32(fx::FLAGS, 0x20_0000);
                }
            }
            let morph = i16_at(rec, 0x4c);
            if 0 <= morph {
                let h = self.psfx(p, morph as usize)?;
                self.sfx_set_morph(k, f32_at(rec, 0x1c), i32::from_le_bytes(h[8..12].try_into().unwrap()) as i16, 0);
                if f16 & 0x800 != 0 {
                    self.fx_mut(k).or_u32(fx::FLAGS, 0x8000);
                }
            }
            // SfxSetLight(2 × (+0xc, else +8), plight_color 0x2341f8[class]).
            let lr = if f32_at(rec, 0xc) != 0.0 { f32_at(rec, 0xc) } else { f32_at(rec, 8) };
            if lr != 0.0 {
                let c = PLIGHT_COLOR.get(self.players[p].u32_(8) as usize).copied().unwrap_or([0.0; 3]);
                self.sfx_set_light(k, lr + lr, c);
            }
            let speed = f32_at(rec, 0x3c);
            let (gravity, radius) = (f32_at(rec, 0x44), f32_at(rec, 8));
            if 0.0 < speed {
                let mut vel = if f16 & 4 == 0 {
                    let v = add(mul(sub(f32_at(rec, 0x40), speed)?, 0.5)?, speed)?;
                    self.calc_player_sfx_vel(v, p)?
                } else {
                    self.players[p].v3(0x40)
                };
                if i16_at(rec, 0) == 2 {
                    if yaw != 0.0 {
                        vel = yaw_vec3(yaw, vel)?;
                    }
                    if f16 & 8 != 0 && pitch != 0.0 {
                        vel = pitch_vec3(pitch, vel)?;
                    }
                }
                let spin = if u32_at(&srec, 0) & 8 != 0 {
                    let a = self.random_pub(f32::from_bits(0x3fc9_0fdb))?;
                    let b = self.random_pub(f32::from_bits(0x3fc9_0fdb))?;
                    Some([a, 0.0, b])
                } else {
                    None
                };
                self.sfx_set_physics_pub(k, gravity, radius, Some(vel), spin);
            } else {
                self.sfx_set_physics_pub(k, gravity, radius, None, None);
            }
        } else if 0 <= i16_at(rec, 0x4a) {
            let h = self.psfx(p, i16_at(rec, 0x4a) as usize)?;
            self.sfx_set_hit(k, i32::from_le_bytes(h[8..12].try_into().unwrap()) as i16, u32_at(&h, 0xc), u32_at(&h, 0xc));
        }
        Ok(k as i32)
    }

    /// CalcPlayerSfxVel(speed, player): forward with the slope pitch.
    fn calc_player_sfx_vel(&self, k: f32, p: usize) -> R<V3> {
        let r = &self.players[p];
        let mut pitch = r.f32_(0x894);
        if r.u32_(0x898) & 8 != 0 {
            if f32::from_bits(0x3f34_fdf4) < pitch {
                pitch = f32::from_bits(0x3f34_fdf4);
            }
            if 0.0 < pitch && pitch < 0.5 {
                pitch = 0.5;
            }
        }
        Ok([mul(k, r.f32_(0x40))?, mul(k, pitch)?, mul(k, r.f32_(0x48))?])
    }

    /// DoPlyrSfx(player, SFXX index, position, local, parent effect).
    pub(crate) fn do_plyr_sfx(&mut self, p: usize, s: i16, pos: Option<V3>, local: bool, parent_fx: i32) -> R<i32> {
        let mut s = s;
        let mut local = local;
        let mut first = None;
        let mut depth = 0;
        let mut pfx = parent_fx;
        while 0 <= s {
            let mut out = -1;
            depth += 1;
            if 64 < depth {
                return super::unsupported("player SFXX chain cycle");
            }
            let srec = self.psfx(p, s as usize)?;
            let flags = u32_at(&srec, 0);
            // +0x400: tree flags on the player's model (presentation).
            if flags & 0x100 != 0 || flags & 0xf00_0000 != 0 {
                // PsfxDoTexmodNode / PsfxDoParticle: presentation.
            } else if flags & 0x200 == 0 {
                let off = [f32_at(&srec, 0x34), f32_at(&srec, 0x38), f32_at(&srec, 0x3c)];
                let o = if local { off } else { mul_vec_mat3(off, &self.player_rows(p))? };
                let at = if flags & 1 != 0 {
                    local = true;
                    o
                } else {
                    match pos {
                        Some(b) => [add(b[0], o[0])?, add(b[1], o[1])?, add(b[2], o[2])?],
                        None => o,
                    }
                };
                out = self.do_plyr_sfx_sub(p, &srec, at, local, flags, pfx)?;
            }
            let sound = u32_at(&srec, 0xc) as i32;
            if 0 <= sound && flags & 0xf00_0000 == 0 {
                let at = self.players[p].v3(0x50);
                self.sfx(sound, Some(at), 0xe0, 1);
            }
            if flags & 2 != 0 {
                self.shake_camera(f32::from_bits(0x3dcc_cccd), 0, 0, 0x5a, 100);
            }
            if flags & 0x20 != 0 {
                self.events.push(Event::Audio { cue: "DoWorldEffects", player: Some(p), arg: 0 });
            }
            first.get_or_insert(out);
            // The next link is parented to this link's effect (uVar6).
            pfx = out;
            s = u32_at(&srec, 4) as i32 as i16;
        }
        Ok(first.unwrap_or(-1))
    }

    /// DoPlyrSfxSub: StartFXSub with the SFXX flags translated, then the
    /// placement (world, at the player's matrix, or parented to its node).
    fn do_plyr_sfx_sub(&mut self, p: usize, srec: &[u8], pos: V3, local: bool, flags: u32, parent_fx: i32) -> R<i32> {
        let header = u32_at(srec, 8) as i32;
        if header < 0 {
            return Ok(-1);
        }
        let mut fx_flags = 0u32;
        if flags & 0x10 != 0 {
            fx_flags = 0x20_0000;
        }
        if flags & 0x8000 != 0 {
            fx_flags |= 0x1_0000;
        }
        if flags & 0x1_0000 != 0 {
            fx_flags |= 0x40_0000;
        }
        let Some(h) = self.effects.tables.headers.get(header as usize).copied().flatten() else { return Ok(-1) };
        let owner = match self.partner_parent {
            Some(k) if flags & 0x2000 != 0 => k,
            _ => p,
        };
        let rows = self.player_rows(owner);
        let base = self.players[owner].v3(0x50);
        let time = f32_at(srec, 0x40);
        let (world, matrix, parent) = if flags & 0x80 != 0 {
            (self.players[p].v3(0x50), self.player_rows(p), None)
        } else if flags & 0x40 != 0 {
            let w = if local {
                let v = mul_vec_mat3(pos, &rows)?;
                [add(v[0], base[0])?, add(v[1], base[1])?, add(v[2], base[2])?]
            } else {
                pos
            };
            (w, rows, None)
        } else if !local {
            if flags & 0x4_0000 != 0 && 0 <= parent_fx {
                return super::unsupported("player effect parented to another effect");
            }
            (pos, [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], None)
        } else {
            if flags & 0x4_0000 != 0 && 0 <= parent_fx {
                return super::unsupported("player effect parented to another effect");
            }
            let v = mul_vec_mat3(pos, &rows)?;
            let w = [add(v[0], base[0])?, add(v[1], base[1])?, add(v[2], base[2])?];
            (w, rows, Some(EffectParent { player: owner, local: pos, local_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] }))
        };
        let k = self.start_fx_tree(time, Visual::Header(header as usize), h.frames, h.timing, Some(world), fx_flags)?;
        self.fx_mut(k).set_u32(fx::KIND, header as u32);
        if let Some(n) = self.effects.nodes[k].as_mut() {
            n.matrix = matrix;
            n.parent = parent;
        }
        // +0x20000 SfxSetStreak, ambient add and colour (+0x4c): presentation.
        Ok(k as i32)
    }

    /// PlaceEffectOnFloor(effect, 0): FloorCollide(radius + 1, radius + 5,
    /// −10) at the effect; on a hit the node takes floor_col's matrix
    /// (+0.1 y), otherwise the identity rotation.
    fn place_effect_on_floor(&mut self, k: usize) -> R<()> {
        let r = self.fx(k).f32_(fx::RADIUS);
        let Some(n) = self.effects.nodes[k] else { return Ok(()) };
        let (c, _) = self.collision.floor_collide(add(r, 1.0)?, add(r, 5.0)?, -10.0, n.position, false, 1)?;
        if let Some(n) = self.effects.nodes[k].as_mut() {
            match c.solid {
                Some(h) => {
                    n.position = [h.position[0], add(h.position[1], 0.1)?, h.position[2]];
                }
                None => n.matrix = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            }
        }
        Ok(())
    }

    /// SfxSetHit 0x17df50(effect, hit header, hit sound, wall sound).
    pub(crate) fn sfx_set_hit(&mut self, k: usize, hit: i16, sound: u32, wall: u32) {
        let r = self.fx_mut(k);
        r.set_i16(fx::HIT_FX, hit);
        r.set_u32(fx::HIT_SOUND, sound);
        r.set_u32(fx::WALL_SOUND, wall);
    }

    /// SfxSetMorph 0x17e1b8(time, effect, next header, second header).
    pub(crate) fn sfx_set_morph(&mut self, k: usize, time: f32, next: i16, second: i16) {
        let r = self.fx_mut(k);
        r.set_i16(fx::NEXT_FX, next);
        r.set_i16(fx::NEXT_FX2, second);
        r.or_u32(fx::FLAGS, 0x4000);
        r.set_f32(fx::NEXT_TIME, time);
    }
}
