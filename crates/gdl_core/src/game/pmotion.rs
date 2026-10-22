//! PlayerMotion 0x1480a8 on the player record (research/player.md). The
//! stages follow the original order: control → displacement request (the
//! verified request/facing subset is the same arithmetic), item / player /
//! wall / floor / camera / new-floor / transporter / enemy contacts,
//! knockback and targeting → motion code → requested action, DoPlayerAction,
//! the animation event word (+0x8d8: melee, missiles, magic, powerups), the
//! position add and the facing turn, then team-move parenting and the
//! class effect damage (PlyrSfxDoDamage).
//!
//! Paths that need systems not ported yet return Unsupported when reached
//! (named in each message); presentation-only calls (trees, skins, shadow,
//! sounds without simulation effect) are noted where they occur.
use super::pcontact::{ptr, vadd, wrap, PI};
use super::{unsupported, Game, R};
use crate::libm::{atan2f, cosf, sinf};
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sub_guarded_rz as sub};
use crate::player_collision::{collide_floor, collide_walls, PlayerFloorState};
use crate::vu0::fqdist;

type V3 = [f32; 3];

fn vsub(a: V3, b: V3) -> R<V3> {
    Ok([sub(a[0], b[0])?, sub(a[1], b[1])?, sub(a[2], b[2])?])
}
fn vscale(k: f32, v: V3) -> R<V3> {
    Ok([mul(k, v[0])?, mul(k, v[1])?, mul(k, v[2])?])
}

impl Game {
    /// PlayerDamageEnemy over an enemy index or critter | 0x10000.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn player_damage_target(&mut self, damage: f32, cooldown: f32, p: usize, target: i32, flags: u32, hit: Option<V3>, attack: bool) -> R<i32> {
        if target < 0 {
            return Ok(-1);
        }
        if target < 0x10000 {
            self.player_damage_enemy(damage, cooldown, p, target as usize, flags, hit, attack)
        } else {
            self.player_damage_critter(damage, cooldown, p, (target & 0xffff) as usize, flags, hit)
        }
    }

    /// The 0x89 / 0x8f team-move contact damage (PlayerDamageItem with
    /// PlyrSfxDoDamage of the partner's class DAMG +0x20 at the contact).
    fn team_move_item_hit(&mut self, p: usize, cur: u32, item: usize, at: V3) -> R<()> {
        let dmg = if cur == 0x89 { 50.0 } else { 10.0 };
        self.player_damage_item(dmg, p, item, 0x20)?;
        self.team_move_sfx(p, at)
    }

    /// PlyrSfxDoDamage(−1, 1, partner, PlayerData[partner] +0x20, player, at).
    fn team_move_sfx(&mut self, p: usize, at: V3) -> R<()> {
        if let Some(k) = ptr(self.players[p].u32_(0x690)) {
            let d = self.class_sfx(self.players[k].u32_(0) as usize, 0x20);
            if 0 <= d {
                self.plyr_sfx_do_damage(-1.0, 1.0, k, d, Some(p), Some(at))?;
            }
        }
        Ok(())
    }

    /// PlayerData[k] +off (i16 class sfx index).
    fn class_sfx(&self, k: usize, off: usize) -> i16 {
        let raw = &self.classes[k].raw;
        raw.get(off..off + 2).map_or(-1, |b| i16::from_le_bytes([b[0], b[1]]))
    }

    /// PlayerMotion 0x1480a8.
    pub fn player_motion(&mut self, p: usize) -> R<()> {
        let radius = self.players[p].f32_(0x828);
        let height = self.players[p].f32_(0x82c);
        let me = self.players[p].u32_(0) as usize;
        let cur = self.players[p].u32_(0x1e0);
        let spf = self.globals.spf;
        let mut small_target = 0u32; // uStack_10c
        let mut target = -1i32; // uStack_118
        let mut item: Option<usize> = None; // piStack_114
        let mut bounce = 0; // iStack_fc
        if self.players[p].v3(0x20).iter().chain(self.players[p].v3(0x30).iter()).chain(self.players[p].v3(0x40).iter()).any(|v| v.is_nan()) {
            // FatalErrorC (non-fatal on retail) then get_player_pos.
            self.get_player_pos(p, 0)?;
        }
        self.players[p].and_u16(0x93c, 0xfffe);
        let cam = add(atan2f(self.globals.camera_90, self.globals.camera_98)?, PI)?;
        let mut code = self.get_player_dmotion(p, cam)?;
        let pc = self.controls.control[p].clone();
        let attack_button = match code {
            0xf => 1,
            0x10 => 2,
            _ => 0,
        }; // iStack_f8
        let facing = atan2f(self.players[p].f32_(0x40), self.players[p].f32_(0x48))?; // __x
        // Control magnitude (fVar24) and angle (fVar33).
        let s96c = self.players[p].f32_(0x96c);
        let mut partner_drive = None;
        if cur == 0x8f
            && let Some(k) = ptr(self.players[p].u32_(0x690))
        {
            let c = &self.controls.control[self.players[k].u32_(0) as usize];
            if 0.0 < c.f32_(0x20) {
                partner_drive = Some((c.f32_(0x20), c.f32_(0x1c)));
            }
        }
        let (mag, ang) = if let Some(v) = partner_drive {
            v
        } else if cur != 8 {
            let m = pc.f32_(0x20);
            if m == 0.0 && self.players[p].i32_(8) == 0 && (cur == 0x23 || cur == 0x25) {
                (0.5, sub(self.players[p].f32_(0x86c), cam)?)
            } else {
                (m, pc.f32_(0x1c))
            }
        } else if pc.f32_(0x20) < 0.25 {
            (1.0, sub(self.players[p].f32_(0x86c), cam)?)
        } else {
            (1.0, pc.f32_(0x1c))
        };
        let mut scale = mul(mag, s96c)?; // fVar26
        // Target facing (__x_00) and movement direction (fVar27).
        let facing_86c = self.players[p].f32_(0x86c);
        let (mut want, dir) = if self.globals.quest_item_state == 2 {
            let Some(b) = usize::try_from(self.boss_crit_inst - 1).ok() else {
                return unsupported("QuestItemState 2 without BossCritInst");
            };
            let bp = self.crit(b).rec.v3(0x40);
            let pp = self.players[p].v3(0x50);
            let a = atan2f(sub(bp[0], pp[0])?, sub(bp[2], pp[2])?)?;
            scale = 0.0;
            (a, a)
        } else if 0.0 < pc.f32_(0x28) {
            (add(cam, pc.f32_(0x24))?, wrap(add(ang, cam)?)?)
        } else if code.wrapping_sub(2) < 5 || code.wrapping_sub(9) < 4 || code.wrapping_sub(0x11) < 4 {
            (facing_86c, wrap(add(ang, cam)?)?)
        } else if 0.0 < scale {
            let d = wrap(add(ang, cam)?)?;
            (d, d)
        } else {
            if cur.wrapping_sub(0x3e) < 0x11 {
                scale = 0.5;
            }
            (facing_86c, facing_86c)
        };
        // Knockback / push velocity.
        let mut d: V3;
        if self.players[p].i32_(0x8c8) < 0xb || self.players[p].u32_(0x8b0) & 0x8000 != 0 {
            let k = vscale(spf, self.players[p].v3(0x848))?;
            d = k;
            if f32::from_bits(0x3b23_d70b) <= add(add(mul(k[0], k[0])?, mul(k[1], k[1])?)?, mul(k[2], k[2])?)? {
                let a = wrap(sub(atan2f(k[0], k[2])?, dir)?)?;
                let lim = f32::from_bits(0x4006_0a92);
                if a < -lim || lim < a {
                    scale = 0.0;
                }
            }
            d[0] = add(mul(self.players[p].f32_(0x830), spf)?, d[0])?;
            d[2] = add(mul(self.players[p].f32_(0x838), spf)?, d[2])?;
        } else {
            d = [0.0; 3];
        }
        let mut lim = mul(mul(self.players[p].f32_(0x124), spf)?, 1.5)?;
        if self.players[p].u32_(0x8b0) & 0x1_8160 != 0 {
            lim = mul(spf, 40.0)?;
        }
        if self.globals.quest_item_state.wrapping_sub(1) < 4 && self.globals.boss_type == 0x23 && self.globals.boss_active != 0 {
            lim = add(spf, spf)?;
        }
        d[1] = if d[1] < 0.0 {
            0.0
        } else if lim < d[1] {
            lim
        } else {
            d[1]
        };
        let speed124 = self.players[p].f32_(0x124);
        let speed; // fVar33
        if cur == 0x89 {
            speed = mul(spf, 30.0)?;
            d[0] = mul(speed, sinf(dir)?)?;
            d[2] = mul(speed, cosf(dir)?)?;
        } else if code == 0x1d {
            speed = mul(mul(mul(scale, speed124)?, spf)?, 4.0)?;
            d[0] = mul(speed, sinf(dir)?)?;
            d[1] = if scale == 0.0 { mul(spf, 15.0)? } else { 0.0 };
            d[2] = mul(speed, cosf(dir)?)?;
        } else {
            speed = mul(mul(mul(scale, speed124)?, spf)?, self.players[p].f32_(0x964))?;
            let x = add(mul(speed, sinf(dir)?)?, d[0])?;
            let z = add(mul(speed, cosf(dir)?)?, d[2])?;
            let c = |v: f32| if v < -lim { -lim } else if lim < v { lim } else { v };
            d[0] = c(x);
            d[2] = c(z);
        }
        if self.players[p].f32_(0x54) <= self.globals.world_bottom {
            self.get_player_pos(p, 0)?;
        }
        let request = d;
        self.players[p].set_v3(0x860, d);
        let mut pos = self.players[p].v3(0x70);
        // Contacts.
        let mut new_floor = 0; // iStack_100
        let grabbed = self.players[p].u16_(0x93c) & 0x20 != 0;
        let mut bounce_normal: Option<V3> = None; // uStack_190 (wall_normal x, 0, z)
        if !grabbed {
            if code == 0x1d {
                self.players[p].set_u32(0x89c, 0);
                self.player_floor_parent[p] = None;
                self.players[p].or_u16(0x93c, 1);
                let b = self.globals.world_bottom;
                self.players[p].set_f32(0x88c, b);
                self.players[p].set_f32(0x8a4, b);
            } else {
                let mut to = vadd(pos, d)?;
                self.players[p].set_u32(0x884, 0);
                let mut hit_item = None;
                let mut out = to;
                let blocking = self.player_collide_items(radius, height, p, pos, to, Some(&mut out), false, Some(&mut hit_item))?;
                if blocking != 0
                    && let Some(it) = hit_item {
                        let r = &self.items.records[it];
                        if r.i8_(0xe3) < 0 || r.i16_(0xe4) < 1 || (attack_button == 0 && self.players[p].i32_(0x8c8) == 0) {
                            d[0] = sub(out[0], pos[0])?;
                            d[2] = sub(out[2], pos[2])?;
                        } else {
                            d[0] = 0.0;
                            d[2] = 0.0;
                        }
                        if cur == 0x89 || cur == 0x8f {
                            self.team_move_item_hit(p, cur, it, out)?;
                            bounce = 1;
                        }
                    }
                to = vadd(pos, d)?;
                let mut out = to;
                let k = self.player_collide_players(radius, height, p, pos, to, &mut out, false)?;
                let mut k = usize::try_from(k).ok();
                if let Some(kk) = k
                    && cur == 0x89
                    && ptr(self.players[p].u32_(0x690)) == Some(kk)
                    && self.globals.time < add(self.players[p].f32_(0x8d4), 1.5)?
                {
                    k = None;
                }
                let mut it_swap = false;
                if let Some(kk) = k {
                    if self.players[kk].u16_(0x928) == 0 {
                        self.push_player(kk, d)?;
                    }
                    d[0] = sub(out[0], pos[0])?;
                    d[2] = sub(out[2], pos[2])?;
                    if cur == 0x89 {
                        bounce = 1;
                    }
                    if 0x3c < self.players[p].u16_(0x92c) {
                        self.globals.it = kk as i32;
                        self.players[kk].set_u16(0x92c, 1);
                        self.players[p].set_u16(0x92c, 0);
                        self.help(0x32, self.players[kk].u32_(0) as usize);
                        let at = self.players[kk].v3(0x60);
                        self.audio_it_swap(at);
                        it_swap = true;
                    }
                }
                if !it_swap || k.is_none() {
                    let t = self.players[p].u16_(0x92c);
                    if t != 0 {
                        self.players[p].set_u16(0x92c, t.wrapping_add(self.globals.vb_elapsed as u16));
                    }
                }
                // Walls (one unit up), then floor.
                pos[1] = add(pos[1], 1.0)?;
                let action = cur;
                let (w, wall) = collide_walls(&mut self.collision, radius, action, pos, &mut d)?;
                if w == 0 {
                    self.players[p].set_u32(0x8a0, 0);
                } else if let Some(c) = wall {
                    self.players[p].set_u32(0x8a0, c.object as u32 + 1);
                    self.player_world_damage(p, c.object, pos, c.hit)?;
                    if cur == 0x89 {
                        bounce = 2;
                        bounce_normal = Some([c.normal[0], 0.0, c.normal[2]]);
                    }
                }
                pos[1] = sub(pos[1], 1.0)?;
                let r = &self.players[p];
                let mut fs = PlayerFloorState {
                    floor_y_88c: r.f32_(0x88c),
                    floor_object_89c: ptr(r.u32_(0x89c)),
                    flags_8ac: r.u32_(0x8ac),
                    word_100: r.i32_(0x100),
                    y_54: r.f32_(0x54),
                    wall_object_8a0: wall.map(|c| c.object),
                    floor_contact: None,
                    floor_hit: None,
                    water: None,
                };
                let wb = self.globals.world_bottom;
                let f = collide_floor(&mut self.collision, radius, height, &mut fs, pos, &mut d, w, wall.as_ref(), wb)?;
                {
                    let r = &mut self.players[p];
                    r.set_f32(0x88c, fs.floor_y_88c);
                    r.set_u32(0x89c, fs.floor_object_89c.map_or(0, |o| o as u32 + 1));
                    r.set_u32(0x8ac, fs.flags_8ac);
                }
                let floor_hit = fs.floor_hit;
                match fs.water {
                    Some((wo, wy)) => {
                        let at = [pos[0], self.players[p].f32_(0x88c), pos[2]];
                        self.player_world_damage(p, wo, pos, at)?;
                        self.players[p].set_f32(0x8a4, wy);
                    }
                    None => {
                        let y = self.players[p].f32_(0x88c);
                        self.players[p].set_f32(0x8a4, y);
                    }
                }
                self.player_water = fs.water;
                if let Some(fo) = ptr(self.players[p].u32_(0x89c)) {
                    let at = floor_hit.map_or([0.0; 3], |(h, _)| h.position);
                    self.player_world_damage(p, fo, pos, at)?;
                }
                let floor_obj = ptr(self.players[p].u32_(0x89c));
                let adjust = if 0 < f {
                    true
                } else if f == 0 {
                    floor_obj.is_none_or(|o| self.collision.runtime_flags[o] & 0x1000 == 0)
                } else if f == -2 {
                    floor_obj.is_none() || self.players[p].u32_(0x8ac) & 0x8000 != 0
                } else {
                    false
                };
                if adjust {
                    let mut dy = sub(self.players[p].f32_(0x88c), self.players[p].f32_(0x54))?;
                    let floor = mul(spf, -16.0)?;
                    if dy < floor {
                        dy = floor;
                    }
                    d[1] = add(dy, d[1])?;
                }
                // lVar9 after the floor stage.
                let wall_flag = if f != 1 { 1 } else { i32::from(w != 0) };
                if self.players[p].u32_(0x8b0) & 0x8000 == 0 {
                    let res = self.camera_limit_player_dpos(p, &mut d, wall_flag == 0)?;
                    let v = if wall_flag != 0 { -res } else { res };
                    self.players[p].set_i32(0x978, v);
                    if res == 5 && cur == 0x89 {
                        bounce = 1;
                    }
                } else {
                    self.players[p].set_i32(0x978, 0);
                }
                if mul(speed, f32::from_bits(0x3dcc_cccd))? < d[1] {
                    let l = fqdist(speed, d[1])?.0;
                    if f32::from_bits(0x3c23_d70a) < l {
                        let k = div(speed, l)?;
                        d[0] = mul(k, d[0])?;
                        d[2] = mul(k, d[2])?;
                    }
                }
                if 0 < f {
                    new_floor = self.player_new_floor(p, floor_hit, &mut d)?;
                }
                if new_floor == 0 {
                    let saved = self.players[p].u32_(0x888);
                    let to = vadd(pos, d)?;
                    let mut out2 = to;
                    let mut it2 = None;
                    let n = self.player_collide_items(mul(radius, f32::from_bits(0x3f66_6666))?, height, p, pos, to, Some(&mut out2), true, Some(&mut it2))?;
                    if n != 0
                        && let Some(it) = it2
                        && self.verify_item_col(it, pos, request)?
                    {
                        if attack_button == 0 && self.players[p].i32_(0x8c8) == 0 && self.players[p].i32_(0x978) == 0 {
                            self.modify_player_dpos(p, &mut d, to, out2)?;
                        } else {
                            d[0] = 0.0;
                            d[2] = 0.0;
                        }
                        if cur == 0x89 || cur == 0x8f {
                            self.team_move_item_hit(p, cur, it, to)?;
                            bounce = 1;
                        }
                    }
                    if blocking != 0 {
                        self.players[p].set_u32(0x888, saved);
                    }
                }
                let mut trans = self.do_transporter(radius, p, pos, &mut d)?;
                self.do_exit_motion(p);
                if new_floor == 0 && trans != 1 {
                    let to = if trans == 0 {
                        vadd(pos, d)?
                    } else {
                        pos = vadd(self.players[p].v3(0x70), d)?;
                        pos
                    };
                    let mut out2 = to;
                    let k = self.player_collide_players(mul(radius, f32::from_bits(0x3f66_6666))?, height, p, pos, to, &mut out2, true)?;
                    if 0 <= k {
                        if trans == 0 {
                            if cur == 0x89 {
                                if add(self.players[p].f32_(0x8d4), 1.5)? < self.globals.time {
                                    bounce = 1;
                                    self.modify_player_dpos(p, &mut d, to, out2)?;
                                }
                            } else {
                                self.modify_player_dpos(p, &mut d, to, out2)?;
                            }
                        } else {
                            d = [0.0; 3];
                        }
                        trans = 0;
                    }
                }
                if new_floor == 0 && trans != 1 {
                    let rr = match cur {
                        0x89 => 4.0,
                        0x8f => 3.0,
                        _ => radius,
                    };
                    let to = vadd(pos, d)?;
                    let mut out2 = to;
                    let e = self.player_collide_enemies(rr, height, p, pos, to, &mut out2)?;
                    target = e;
                    if 0 <= e {
                        let (critter, crit4) = if e < 0x10000 {
                            (false, false)
                        } else {
                            (true, self.critter_kind((e & 0xffff) as usize) == 4)
                        };
                        if trans == 0 {
                            if cur != 8 || crit4 || critter {
                                if cur == 0x89 || cur == 0x8f {
                                    let dmg = if cur == 0x89 { 50.0 } else { 20.0 };
                                    let r = self.player_damage_target(dmg, 1.0, p, e, 0x20, Some(out2), false)?;
                                    if 0 <= r {
                                        self.team_move_sfx(p, out2)?;
                                    }
                                    if crit4 || critter {
                                        bounce = 1;
                                        d[0] = 0.0;
                                        d[2] = 0.0;
                                    }
                                } else if pc.i32_(0x38) == 0 && wall_flag == 0 {
                                    self.modify_player_dpos(p, &mut d, to, out2)?;
                                } else {
                                    d[0] = 0.0;
                                    d[2] = 0.0;
                                }
                            } else {
                                d[0] = mul(d[0], 0.75)?;
                                d[2] = mul(d[2], 0.75)?;
                            }
                        } else {
                            self.player_damage_target(10000.0, 0.0, p, e, 0, None, false)?;
                        }
                    }
                } else {
                    target = -1;
                }
                if trans == 2 {
                    let y = self.transporter_floor_y.take().unwrap_or(self.players[p].f32_(0x88c));
                    self.players[p].set_f32(0x88c, y);
                    let wy = self.player_water.map_or(y, |(_, wy)| wy);
                    self.players[p].set_f32(0x8a4, wy);
                }
            }
            self.players[p].set_u32(0x8e4, 0);
        } else {
            // GetObjWorldMat of the grabbing node (critter_hits keeps it).
            if let Some(at) = self.grabbed_position(p)? {
                self.players[p].set_v3(0x50, at);
            }
            self.do_auto_exit_player(p);
            self.players[p].set_u32(0x8e4, 0);
        }
        // Knockback and targeting.
        let mut aim = self.players[p].v3(0x40); // fStack_1e0
        let mut dist = 999.0f32; // fVar27
        let mut no_throw = false; // iStack_104
        let mut extra: f32; // fVar33
        let mut reach = radius; // fVar25
        let knock;
        let mut knock_angle = want; // fStack_11c
        if !grabbed || self.players[p].u32_(0x8ac) & 0x4000 != 0 {
            let k = self.players[p].v3(0x848);
            let s = f32::from_bits(0x3f2a_c083);
            self.players[p].set_v3(0x848, [mul(k[0], s)?, mul(k[1], s)?, mul(k[2], s)?]);
            let mut kk = self.player_knockback(facing, p, &mut knock_angle)?;
            let ka = knock_angle;
            if kk < 300 {
                if cur == 0x83 {
                    kk = 0x14;
                } else if cur == 0x85 {
                    kk = 0xb;
                } else if kk < 10 && cur == 0x82 {
                    kk = 10;
                } else if self.players[p].f32_(0x870) <= self.globals.time || code != 1 {
                    if kk < 1 {
                        if cur == 0x81 {
                            kk = 3;
                        } else if cur == 0x7f {
                            kk = 2;
                        }
                    }
                } else {
                    kk = 100;
                }
            }
            extra = if attack_button != 0 { 1.0 } else { 0.0 };
            let to = vadd(pos, d)?;
            match kk {
                300 => code = 0x29,
                0x12d => code = 0x23,
                200 => {
                    if code != 7 {
                        code = 0x21;
                    }
                }
                100 => {
                    d = [0.0; 3];
                    code = 0x20;
                }
                0x1e | 0x1f => {
                    code = 0x25;
                    want = ka;
                }
                0x15 => {
                    code = 0x23;
                    want = ka;
                }
                0x14 => {
                    code = 0x24;
                    want = ka;
                }
                10 | 11 => {
                    code = 0x22;
                    want = ka;
                }
                2 | 3 => {
                    d = [0.0; 3];
                    if cur == 0x7f || cur == 0x81 {
                        kk = 0;
                        code = 1;
                    } else {
                        code = 0x1f;
                    }
                }
                _ => {
                    let f1f0 = [sinf(want)?, 0.0, cosf(want)?];
                    item = ptr(self.players[p].u32_(0x880));
                    let mut tgt = -1;
                    dist = self.player_get_target(p, to, f1f0, &mut aim, &mut tgt, &mut item)?;
                    target = tgt;
                    let (crit_i, enemy) = if target < 0x10000 {
                        (-1i32, usize::try_from(target).ok())
                    } else {
                        (target & 0xffff, None)
                    };
                    let st = self.players[p].u16_(0x93c) as i16 as i32;
                    if st & 0xc000 != 0 || pc.u32_(4) & 0x5000 != 0 || pc.i32_(0x34) == 0 || 0.0 < pc.f32_(0x28) {
                        aim[0] = f1f0[0];
                        aim[2] = f1f0[2];
                    }
                    let a = atan2f(aim[0], aim[2])?;
                    let rel = wrap(sub(a, facing)?)?;
                    self.players[p].set_f32(0x8dc, rel);
                    let mut crit4 = false;
                    if crit_i < 0 {
                        match enemy {
                            None => {
                                if let Some(it) = item {
                                    self.players[p].or_u32(0x8e4, 0x20);
                                    let h = self.items.definitions[self.items.records[it].definition()].f32_(0x10);
                                    if h <= 3.5 && dist < add(radius, 2.0)? {
                                        self.players[p].or_u32(0x8e4, 0x22);
                                    }
                                }
                            }
                            Some(en) => {
                                self.players[p].or_u32(0x8e4, 0x10);
                                let small = self.enemies[en].f32_(super::enemies::e::HALF_HEIGHT) <= 2.0 || self.globals.enemy_shrink < 1.0;
                                if small && dist < add(radius, 2.0)? {
                                    self.players[p].or_u32(0x8e4, 2);
                                }
                            }
                        }
                    } else {
                        self.players[p].or_u32(0x8e4, 0x10);
                        crit4 = self.critter_kind(crit_i as usize) == 4;
                    }
                    if self.globals.aux_levels & 0x80 != 0 {
                        self.players[p].or_u32(0x8e4, 2);
                    }
                    small_target = self.players[p].u32_(0x8e4) & 2;
                    let r134 = self.players[p].u32_(0x134);
                    'target: {
                        if r134 & 0x8_0000 != 0
                            && let Some(en) = enemy
                            && self.enemies[en].i32_(0) == 0x1e
                            && rel.abs() < f32::from_bits(0x3fc9_0fdb)
                        {
                            // The halo against Death.
                            self.damage_enemy(1.0, en, me as i32, 0, None, None, 1)?;
                            want = a;
                            if self.players[p].i16_(0x936) == 0 {
                                self.audio_player_sfx(p, 0x52, 0x3c);
                                self.players[p].set_i16(0x936, 1);
                            }
                            let at = self.enemies[en].v3(0x60);
                            self.audio_death_hurt(at);
                            self.players[p].or_u32(0x13c, 1);
                            if self.enemies[en].i16_(0x21a) == 2 {
                                self.players[p].or_u32(0x13c, 3);
                            }
                            self.players[p].set_i16(0x934, 2);
                            code = 1;
                            break 'target;
                        }
                        if add(reach, 1.0)? <= dist {
                            break 'target;
                        }
                        if r134 & 0x20_0000 == 0 || target < 0 {
                            if r134 & 0x40_0000 != 0 && 0 <= target {
                                let r = self.player_damage_target(20.0, 1.0, p, target, 0x22, Some(to), false)?;
                                if 0 <= r {
                                    // StartFXNoLoop(0x37) oriented at the target (presentation).
                                    let at = self.players[p].v3(0x50);
                                    self.start_fx_no_loop(0x37, at)?;
                                }
                                break 'target;
                            }
                            if cur != 8 || target < 0 || crit4 {
                                if (code == 8 || code == 0xd) && self.players[p].u32_(0x8d0) == 0 && cur.wrapping_sub(0x27) > 0x4b && pc.i32_(0x38) != 0 {
                                    let gen_ok = item.is_some_and(|it| {
                                        let r = &self.items.records[it];
                                        self.items.definitions[r.definition()].kind() == 3 && 0 <= r.i8_(0xe3)
                                    });
                                    let boss_crit = crit4 && self.globals.boss_type != 0x25 && self.globals.boss_type != 0x29;
                                    if (target < 0 || boss_crit) && !gen_ok {
                                        break 'target;
                                    }
                                    no_throw = true;
                                    extra = 0.0;
                                    code = 0xf;
                                }
                                break 'target;
                            }
                            self.player_damage_target(3.0, 1.0, p, target, 0x20, Some(to), false)?;
                        } else {
                            if code == 1 {
                                code = 8;
                            }
                            if code == 8 || code == 0xd {
                                self.player_damage_target(3.0, 0.0, p, target, 1, Some(to), false)?;
                            }
                        }
                    }
                }
            }
            if code < 0x1f {
                match self.players[p].i16_(0x934) {
                    2 => {
                        code = 0x1b;
                        self.players[p].set_i16(0x934, 0);
                    }
                    3 => {
                        code = 0x20;
                        self.players[p].set_i16(0x934, 0);
                    }
                    1 if self.globals.secret_world == 0 => {
                        code = 0xe;
                        self.players[p].set_i16(0x934, 0);
                    }
                    _ => {}
                }
            }
            // Turbo +0x800.
            if self.players[p].i32_(0x8c8) < 0xb {
                let t = self.players[p].f32_(0x800);
                if cur == 8 {
                    let v = sub(t, mul(spf, 20.0)?)?;
                    self.players[p].set_f32(0x800, if v < 0.0 { 0.0 } else { v });
                } else {
                    let mut t = t;
                    if t < 100.0 {
                        t = if self.globals.aux_levels & 0x10 == 0 { add(add(spf, spf)?, t)? } else { add(mul(spf, 15.0)?, t)? };
                        self.players[p].set_f32(0x800, t);
                    }
                    if 100.0 <= t {
                        if 0xf <= self.globals.num_visible_enemies {
                            self.help(0x6e, p);
                        }
                        self.players[p].set_f32(0x800, 100.0);
                    } else if 40.0 <= t && 1 < self.globals.num_active_players && 0xe < self.globals.num_visible_enemies {
                        self.help(0x6f, p);
                    }
                }
            }
            if code == 8 {
                let v = add(self.players[p].f32_(0x874), spf)?;
                self.players[p].set_f32(0x874, v);
            } else if code == 0xd {
                if self.players[p].f32_(0x874) < 0.1 && self.globals.boss_type < 0 {
                    code = 8;
                }
                let v = add(self.players[p].f32_(0x874), spf)?;
                self.players[p].set_f32(0x874, v);
            } else {
                self.players[p].set_f32(0x874, 0.0);
            }
            let g = &self.globals;
            if 0 <= g.boss_type && 0 <= g.boss_index && g.good_wizard < 2 && g.boss_is_dead != 0 && self.players[p].u16_(0x93c) & 0x800 == 0 {
                code = 0x1c;
            }
            knock = kk;
        } else {
            dist = 999999.0;
            reach = 0.0;
            extra = 0.0;
            knock = 0;
            no_throw = false;
        }
        self.players[p].set_u32(0x1dc, code);
        if 1 < self.players[p].i32_(0x80c) {
            code = 1;
        }
        if code == 0x15 {
            extra = 0.0;
        }
        if self.globals.aux_levels & 0x40 != 0 {
            dist = if mag == 0.0 { 0.0 } else { add(add(reach, 1.0)?, extra)? };
        }
        let bit = if dist < add(add(reach, 1.0)?, extra)? {
            1
        } else if dist < add(add(reach, 2.0)?, extra)? {
            4
        } else {
            8
        };
        self.players[p].or_u32(0x8e4, bit);
        let r138 = self.players[p].u32_(0x138);
        let r130 = self.players[p].u32_(0x130);
        let power = if no_throw {
            0
        } else if r138 & 0x1000 != 0 || r138 & 0x2000 != 0 {
            0x6e
        } else if r138 & 0x8000 != 0 {
            0x67
        } else if r138 & 0x4000 != 0 {
            0x68
        } else if r130 & 0x10_0000 != 0 {
            0x6b
        } else if r130 & 0x1000_0000 != 0 {
            0x70
        } else if r138 & 0x70 != 0 {
            0x6e
        } else {
            0
        };
        self.choose_request(p, &mut code, knock, knock_angle, power, no_throw, small_target, mag, &mut want, facing, d, &pc)?;
        self.legend_item_actions(p, cur)?;
        self.do_player_action(p)?;
        if self.players[p].i32_(0x80c) < 4 {
            if self.players[p].u16_(0x93c) & 0x20 == 0 {
                // param_1[0x232] - 1U < 10: an unsigned test (0 fails).
                let at = self.players[p].u32_(0x8c8);
                if at.wrapping_sub(1) < 10 && at != 7 && pc.u32_(4) & 0x5000 == 0 && pc.i32_(0x34) != 0 && scale == 0.0 && pc.f32_(0x28) == 0.0 {
                    want = atan2f(aim[0], aim[2])?;
                }
                let to = vadd(pos, d)?;
                self.consume_action_events(p, radius, height, pos, to, facing, &mut aim, dist, &mut target, &mut item)?;
                // Position add.
                let p50 = self.players[p].v3(0x50);
                self.players[p].set_v3(0x854, p50);
                self.players[p].set_v3(0x50, vadd(p50, d)?);
                if 0.0001 < add(add(mul(d[0], d[0])?, mul(d[1], d[1])?)?, mul(d[2], d[2])?)? {
                    self.players[p].or_u16(0x93c, 1);
                }
                let f = if bounce == 0 || 0 < self.players[p].i16_(0x1d4) {
                    let rate = mul(mul(spf, f32::from_bits(0x417b_53d1))?, self.players[p].f32_(0x968))?;
                    let dd = wrap(sub(want, facing)?)?;
                    let f = if rate < dd {
                        add(facing, rate)?
                    } else if dd < -rate {
                        sub(facing, rate)?
                    } else {
                        want
                    };
                    self.players[p].set_f32(0x86c, if code.wrapping_sub(0x22) < 2 { want } else { f });
                    f
                } else {
                    let f = if bounce == 2 {
                        let Some(mut n) = bounce_normal else {
                            return unsupported("team-move wall bounce without a normal");
                        };
                        crate::item_collision::normal_vector_2d(&mut n)?;
                        let fwd = self.players[p].v3(0x40);
                        let r = reflect_vector_2d(fwd, n)?;
                        atan2f(r[0], r[2])?
                    } else {
                        add(self.players[p].f32_(0x86c), f32::from_bits(0x4016_cbe5))?
                    };
                    let f = wrap(f)?;
                    self.players[p].set_f32(0x86c, f);
                    self.players[p].set_i16(0x1d4, 10);
                    f
                };
                if 0.0001 < sub(self.players[p].f32_(0xdc), f)?.abs() {
                    self.players[p].or_u16(0x93c, 1);
                }
                self.players[p].set_f32(0xdc, f);
                self.player_matrix_from_ypr(p)?;
                // UpdatePlayerShadow and the instance root y (+1.5 with
                // +0x138 & 1) are presentation.
                if self.players[p].u16_(0x93a) & 3 != 0 {
                    if r138 & 1 == 0 {
                        // AudioWalk(player, side, surface): 4 in water, 3
                        // with the 0x10000 shield, else (+0x898 >> 2) & 2.
                        let side = self.players[p].u16_(0x93a) >> 1 & 1;
                        let r = &self.players[p];
                        let surface = if r.f32_(0x8a4) > r.f32_(0x88c) {
                            4
                        } else if r.u32_(0x134) & 0x1_0000 != 0 {
                            3
                        } else {
                            (r.u32_(0x898) >> 2 & 2) as i32
                        };
                        self.audio_walk(p, i32::from(side), surface)?;
                    }
                    self.players[p].and_u16(0x93a, 0xfffc);
                }
            }
        } else {
            self.legend_item_finale(p)?;
            self.players[p].set_f32(0x86c, want);
            self.players[p].set_f32(0xdc, want);
            self.player_matrix_from_ypr(p)?;
            self.players[p].or_u16(0x93c, 1);
        }
        self.team_move_parenting(p)?;
        self.action_sfx_damage(p)?;
        let frame = self.players[p].f32_(0xac);
        self.players[p].set_f32(0x974, frame);
        Ok(())
    }

    /// CreateYPRMatrix(+0x20, +0xd8 angles): rotation rows, w = 0 and
    /// +0x5c = 1; the position row +0x50 is not written. The tree scale
    /// (PlayerProcessScale) lives in the node, not in these rows.
    fn player_matrix_from_ypr(&mut self, p: usize) -> R<()> {
        let a = self.players[p].v3(0xd8);
        let m = super::enemy_ai::create_ypr_matrix(a)?;
        let r = &mut self.players[p];
        for (i, row) in m.iter().take(3).enumerate() {
            r.set_v3(0x20 + 0x10 * i, [row[0], row[1], row[2]]);
            r.set_f32(0x2c + 0x10 * i, 0.0);
        }
        r.set_f32(0x5c, 1.0);
        Ok(())
    }

    /// CameraLimitPlayerDpos 0x1a2428 with the StdCam (new camera switch):
    /// the slide's collision test is PlayerWallCollide then PlayerCollideFloor.
    fn camera_limit_player_dpos(&mut self, p: usize, d: &mut V3, slide: bool) -> R<i32> {
        let Some(cam) = self.std_camera.take() else {
            return Ok(1);
        };
        let rec = self.players[p].clone();
        let radius = rec.f32_(0x828);
        let height = rec.f32_(0x82c);
        let action = rec.u32_(0x1e0);
        let wb = self.globals.world_bottom;
        let world = &mut self.collision;
        let mut state = PlayerFloorState {
            floor_y_88c: rec.f32_(0x88c),
            floor_object_89c: ptr(rec.u32_(0x89c)),
            flags_8ac: rec.u32_(0x8ac),
            word_100: rec.i32_(0x100),
            y_54: rec.f32_(0x54),
            wall_object_8a0: None,
            floor_contact: None,
            floor_hit: None,
            water: None,
        };
        let mut collide = |from: V3, to: V3| -> R<bool> {
            let mut dd = vsub(to, from)?;
            let (w, wall) = collide_walls(world, radius, action, from, &mut dd)?;
            if w != 0 {
                return Ok(true);
            }
            let f = collide_floor(world, radius, height, &mut state, from, &mut dd, w, wall.as_ref(), wb)?;
            Ok(f < 0)
        };
        let r = cam.limit_player_dpos(&rec, d, slide, &mut collide);
        self.std_camera = Some(cam);
        let r = r?;
        if r.floor_reset {
            let y = self.player_water.map_or(self.players[p].f32_(0x88c), |(_, wy)| wy);
            self.players[p].set_f32(0x8a4, y);
        }
        Ok(r.code)
    }

    /// DoExit 0x146610 from PlayerMotion (stick magnitude PlayerControl +0x20).
    fn do_exit_motion(&mut self, p: usize) {
        let m = self.controls.control[p].f32_(0x20);
        self.do_exit_check(p, m);
    }

    /// The requested action (+0x1e4) from the motion code (0x14a23c switch).
    #[allow(clippy::too_many_arguments)]
    fn choose_request(&mut self, p: usize, code: &mut u32, knock: i32, knock_angle: f32, power: u32, no_throw: bool, small: u32, mag: f32, want: &mut f32, facing: f32, d: V3, pc: &super::record::Record) -> R<()> {
        let set = |g: &mut Self, v: u32| g.players[p].set_u32(0x1e4, v);
        let e4 = self.players[p].u32_(0x8e4);
        let melee_dash = |g: &mut Self, want: f32| -> R<bool> {
            if e4 & 4 != 0 && small == 0 && mag != 0.0 {
                let rel = wrap(sub(want, facing)?)?;
                g.players[p].set_f32(0x8dc, rel);
                g.players[p].set_u32(0x1e4, 0x3e);
                return Ok(true);
            }
            Ok(false)
        };
        loop {
            match *code {
                1 | 8 | 0xd => {
                    let l = fqdist(d[0], d[2])?.0;
                    if l <= 0.01 || self.players[p].u16_(0x93c) & 8 == 0 {
                        if knock == 3 {
                            set(self, 0x81);
                            *want = knock_angle;
                        } else if knock != 0 {
                            set(self, 0x1b);
                            *want = knock_angle;
                        } else {
                            set(self, match *code {
                                0xd => 0x13,
                                8 => 0x11,
                                _ => 0,
                            });
                        }
                    } else {
                        set(self, 0x1a);
                        *want = atan2f(d[0], d[2])?;
                    }
                }
                2 => set(self, 0x77),
                3..=7 => set(self, *code + 1),
                9 => set(self, 9),
                10 => set(self, 0xb),
                0xb => set(self, 0xd),
                0xc => set(self, 0xf),
                0xe => set(self, 0x1c),
                0xf => {
                    if power != 0 {
                        set(self, power);
                    } else if !melee_dash(self, *want)? {
                        let v = if e4 & 1 == 0 && !no_throw {
                            0x5c
                        } else if small == 0 {
                            0x27
                        } else {
                            0x52
                        };
                        set(self, v);
                    }
                }
                0x10 => {
                    if power != 0 {
                        set(self, power);
                    } else if !no_throw && self.players[p].u32_(0x130) & 0x48_0000 != 0 {
                        set(self, 0x5c);
                    } else {
                        let combo = self.players[p].i32_(0x8e0);
                        if combo != 0 && mag != 0.0 && e4 & 5 != 0 {
                            set(self, 0x20);
                        } else if !melee_dash(self, *want)? {
                            if e4 & 1 == 0 && !no_throw {
                                set(self, 99);
                            } else if small == 0 {
                                set(self, 0x20);
                            } else {
                                set(self, 0x4f);
                            }
                        }
                    }
                }
                0x11 => set(self, if power == 0 { 0x47 } else { power }),
                0x12 => set(self, if power == 0 { 0x49 } else { power }),
                0x13 => set(self, if power == 0 { 0x4b } else { power }),
                0x14 => set(self, if power == 0 { 0x4d } else { power }),
                0x15 => {
                    let t = self.players[p].f32_(0x800);
                    if self.players[p].u32_(0x138) & 0x400 == 0 {
                        if 100.0 <= t {
                            set(self, 0x57);
                            self.players[p].set_f32(0x8e8, 100.0);
                        } else if 40.0 <= t {
                            set(self, 0x56);
                            self.players[p].set_f32(0x8e8, 40.0);
                        }
                    } else if 40.0 <= t {
                        self.players[p].set_f32(0x8e8, 40.0);
                        set(self, 0x6e);
                    }
                }
                0x16 => {
                    set(self, 0x58);
                    self.players[p].set_f32(0x8e8, 50.0);
                }
                0x17 => set(self, 0x59),
                0x18..=0x1a => {
                    if self.players[p].i32_(0x1a18) == 0 && self.globals.options_4 & 2 == 0 {
                        self.help(6, p);
                        let m = pc.f32_(0x20);
                        *code = if m <= 0.75 {
                            if 0.0 < m { 8 } else { 1 }
                        } else {
                            0xd
                        };
                        continue;
                    }
                    match *code {
                        0x19 => set(self, 0x75),
                        0x1a => {
                            set(self, 0x73);
                            self.players[p].or_u16(0x92e, 2);
                        }
                        _ => set(self, 0x73),
                    }
                }
                0x1b => set(self, 0x1d),
                0x1c => set(self, 0x7b),
                0x1f => set(self, if knock == 3 { 0x81 } else { 0x7f }),
                0x20 => set(self, 0x7a),
                0x21 => set(self, 0x80),
                0x22..=0x25 => {
                    set(self, [0x82, 0x83, 0x85, 0x87][(*code - 0x22) as usize]);
                    *want = knock_angle;
                }
                0x26 => {
                    let v = match ptr(self.players[p].u32_(0x690)) {
                        None => 0,
                        Some(k) => match self.players[k].i32_(8) {
                            0 => 0x88,
                            1 => 0x8b,
                            2 => 0x8c,
                            3 => 0x8d,
                            4 => 0x8e,
                            5 => 0x91,
                            6 => 0x92,
                            7 => 0x93,
                            _ => 0,
                        },
                    };
                    set(self, v);
                }
                0x27 => {
                    let v = match ptr(self.players[p].u32_(0x690)) {
                        None => 0,
                        Some(k) => match self.players[k].i32_(8) {
                            0 => 0x89,
                            4 => 0x8f,
                            _ => 0,
                        },
                    };
                    set(self, v);
                }
                0x29 => set(self, 0x94),
                _ => set(self, 0),
            }
            return Ok(());
        }
    }

    /// StartFXSub(time, header, position, flags, node flags) on the native
    /// effect slots; −1 when the header was never initialised.
    pub(crate) fn start_fx_sub(&mut self, time: f32, header: usize, at: Option<V3>, flags: u32) -> R<i32> {
        let Some(h) = self.effects.tables.headers.get(header).copied().flatten() else { return Ok(-1) };
        let k = self.start_fx_tree(time, super::effects::Visual::Header(header), h.frames, h.timing, at, flags)?;
        self.fx_mut(k).set_u32(super::effects::fx::KIND, header as u32);
        Ok(k as i32)
    }

    /// StartComboFX 0x17f0b0(position, colour a, colour b): ComboEffects[4]
    /// coloured streak_color[b] with MBTreeSetAmbientAdd(0x1ff) and
    /// SfxSetLight(10, light_color[pclr_idx[b]]), then ComboEffects[a] with
    /// MBTreeSetAmbientAdd(0x1ff).
    fn start_combo_fx(&mut self, at: V3, a: i32, b: i32) -> R<i32> {
        const STREAK_COLOR: [u32; 4] = [0xff_ff00, 0xff, 0xff_0000, 0xff00];
        let table = self.combo_effects.clone();
        let mut k = -1;
        if 0 <= b
            && let Some(&h) = table.get(4)
        {
            k = self.start_fx_sub(0.0, h as usize, Some(at), 0)?;
            if let Ok(slot) = usize::try_from(k) {
                if let Some(n) = self.effects.nodes[slot].as_mut() {
                    n.tint = STREAK_COLOR.get(b as usize).copied();
                    n.ambient = Some(0x1ff);
                }
                let t = &self.effects.tables;
                let light = t.light_color[t.pclr_idx.get(b as usize).copied().unwrap_or(0)];
                self.sfx_set_light(slot, 10.0, light);
            }
        }
        if 0 <= a
            && let Some(&h) = table.get(a as usize)
        {
            k = self.start_fx_sub(0.0, h as usize, Some(at), 0)?;
            if let Ok(slot) = usize::try_from(k)
                && let Some(n) = self.effects.nodes[slot].as_mut()
            {
                n.ambient = Some(0x1ff);
            }
        }
        Ok(k)
    }

    /// The legend-item (+0x80c) stages before DoPlayerAction (0x14a8f0..).
    fn legend_item_actions(&mut self, p: usize, _cur: u32) -> R<()> {
        let bt = self.globals.boss_type;
        if self.players[p].i32_(0x80c) == 0 || bt < 0 {
            return Ok(());
        }
        if self.quest_weapon_sfx < 0 && self.players[p].i32_(0x80c) < 4 {
            // The quest weapon glow (header 0x5c, 999999 s) parented to the
            // player's +0x6a8 hand node (0x21 < BossType < 0x28) or tree at
            // (0, 8, 0); HYPOTHESIS: both follow the player's record.
            let local = if (0x22..0x28).contains(&bt) { [0.0; 3] } else { [0.0, 8.0, 0.0] };
            let k = self.start_fx_sub(f32::from_bits(0x4974_23f0), 0x5c, None, 0)?;
            if 0 <= k
                && let Some(n) = self.effects.nodes[k as usize].as_mut() {
                    n.parent = Some(super::effects::EffectParent { player: p, local, local_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] });
                }
            self.quest_weapon_sfx = k;
        }
        if self.players[p].i32_(0x80c) == 2 {
            let c = self.players[p].i32_(4);
            let at = self.players[p].v3(0x60);
            let k = self.start_combo_fx(at, c, c)?;
            if 0 <= k {
                // Effects +0x44 (the combo effect's scale) = 1/3.
                self.fx_mut(k as usize).set_f32(0x44, f32::from_bits(0x3eaa_7efa));
            }
            self.players[p].set_i32(0x80c, 3);
            self.players[p].set_i16(0x1d2, 0x3c);
            let at = self.players[p].v3(0x60);
            self.audio_legend_item(0, Some(at));
        }
        if self.players[p].i32_(0x80c) == 3 && self.players[p].i16_(0x1d2) < 1 {
            self.players[p].set_i32(0x80c, 4);
            let at = self.players[p].v3(0x60);
            self.audio_legend_item(1, Some(at));
        }
        if 3 < self.players[p].i32_(0x80c) && self.players[p].u32_(0x8d8) & 0xffff_fffe == 0 {
            let cur = self.players[p].u32_(0x1e0);
            let (want, keep) = match bt {
                0x22 | 0x23 | 0x26 | 0x27 => (99, cur.wrapping_sub(99) <= 1),
                0x24 | 0x25 => (0x6b, cur == 0x6b),
                _ => (0x73, cur.wrapping_sub(0x73) <= 1),
            };
            if !keep {
                self.players[p].set_u32(0x1e0, 0);
                self.players[p].set_u32(0x1e4, want);
            }
        }
        Ok(())
    }

    /// The +0x80c ≥ 4 branch after DoPlayerAction (0x14ab00..0x14b3f4): once
    /// the forced action raises an event, the legend item fires at the boss.
    fn legend_item_finale(&mut self, p: usize) -> R<()> {
        if self.players[p].u32_(0x8d8) & 0xffff_fffe == 0 {
            return Ok(());
        }
        let me = self.players[p].u32_(0) as i32;
        let bt = self.globals.boss_type;
        if ((0x22..0x28).contains(&bt) || bt == 0x29)
            && 0 <= self.quest_weapon_sfx {
                self.delete_effect(self.quest_weapon_sfx as usize, true);
                self.quest_weapon_sfx = -1;
                self.players[p].set_u32(0x708, 0);
            }
        self.players[p].set_i32(0x80c, 0);
        self.players[p].set_u32(0x8d8, 0);
        let at = self.players[p].v3(0x60);
        self.audio_legend_item(2, Some(at));
        let Ok(b) = usize::try_from(self.boss_crit_inst - 1) else {
            return super::unsupported("legend item finale without BossCritInst");
        };
        let health = self.crit(b).rec.f32_(0x4f4);
        let ten = f32::from_bits(0x3dcc_cccd);
        let spin = [f32::from_bits(0x4149_0fdb), 0.0, f32::from_bits(0x3fc9_0fdb)];
        // (damage, speed, gravity, radius, sfx flags, target node, aim point)
        let mut damage = 0.0;
        let mut speed = 0.0;
        let (mut gravity, mut radius, mut sflags) = (0.0f32, 2.0f32, 0u32);
        let mut target: Option<(usize, usize)> = None;
        let mut aim = [0.0f32; 3];
        let crit_pos = |g: &Self, o: usize| g.crit(b).rec.v3(o);
        match bt {
            0x22 => {
                target = Some((b, self.crit(b).anim_root));
                damage = mul(health, ten)?;
                speed = 20.0;
            }
            0x23 => {
                // BossCritInst +0xb60 → +0xb60 → +0xd4, then the node's first
                // children (+0x78) to the last. HYPOTHESIS: the first child is
                // the lowest tree index under the node.
                let c2 = self.crit(b).child.and_then(|c1| self.crit(c1).child);
                let Some(c2) = c2 else { return super::unsupported("BossType 0x23 without its chained critter") };
                let Some(mut n) = self.crit(c2).node_d4 else { return super::unsupported("BossType 0x23 without +0xd4 node") };
                let parents = self.crit(c2).parents.clone();
                while let Some(k) = parents.iter().position(|&q| q == n as i32) {
                    n = k;
                }
                target = Some((c2, n));
                damage = mul(self.crit(c2).rec.f32_(0x4f4), 1.5)?;
                speed = 20.0;
            }
            0x24 => {
                let a = crit_pos(self, 0x50);
                aim = [a[0], add(a[1], 0.0)?, a[2]];
                speed = 20.0;
                radius = 5.0;
                gravity = 5.0;
                damage = mul(health, ten)?;
            }
            0x25 => {
                let k = self.start_fx_sub(0.0, 0x5d, Some([0.0, 0.0, 5.0]), 0)?;
                if 0 <= k {
                    if let Some(n) = self.effects.nodes[k as usize].as_mut() {
                        n.parent = Some(super::effects::EffectParent { player: p, local: [0.0, 0.0, 5.0], local_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] });
                    }
                    self.sfx_set_morph(k as usize, 3.0, 0x5a, 0);
                }
                // MBTreeSetColor/SetFlags and the boss tree's 0.8 scale: presentation.
                damage = mul(health, ten)?;
                self.critter_damage(damage, b, me, 0, None, None, 0)?;
                self.crm_pub(b).set_f32(0xb50, 0.5);
            }
            0x26 => {
                let Some(n) = self.crit(b).node_d8 else { return super::unsupported("BossType 0x26 without +0xd8 node") };
                target = Some((b, n));
                sflags = 0x2_0000;
                damage = mul(health, ten)?;
                speed = 20.0;
            }
            0x27 => {
                let c = crit_pos(self, 0x460);
                let at = [sub(c[0], 7.0)?, sub(c[1], 5.0)?, add(c[2], 85.0)?];
                let k = self.start_fx_sub(0.0, 0x5a, Some(at), 0)?;
                if 0 <= k {
                    self.sfx_set_morph(k as usize, 30.0, 0x5b, 0);
                    self.quest_wall_sfx = k;
                }
                damage = mul(health, ten)?;
                self.critter_damage(damage, b, me, 0, None, None, 0)?;
                self.crm_pub(b).set_f32(0xb50, 0.5);
                let c = crit_pos(self, 0x460);
                aim = [c[0], add(c[1], 10.0)?, c[2]];
                gravity = 10.0;
                speed = 20.0;
            }
            0x28 => {
                let c = crit_pos(self, 0x460);
                let at = [c[0], sub(c[1], 5.0)?, add(c[2], 10.0)?];
                let k = self.start_fx_sub(0.0, 0x5d, Some(at), 0)?;
                if 0 <= k {
                    self.sfx_set_morph(k as usize, 30.0, 0x5a, 0);
                    self.quest_wall_sfx = k;
                }
                damage = mul(health, ten)?;
                self.critter_damage(500.0, b, me, 0, None, None, 0)?;
                self.crm_pub(b).set_f32(0xb50, 0.25);
            }
            0x29 => {
                let at = crit_pos(self, 0x460);
                let k = self.start_fx_sub(0.0, 0x5d, Some(at), 0x100_0000)?;
                if 0 <= k {
                    self.sfx_set_morph(k as usize, 5.0, 0x5a, 0);
                }
                damage = mul(health, 0.25)?;
                self.critter_damage(damage, b, me, 0, None, None, 0)?;
            }
            0x2a => {
                let c = crit_pos(self, 0x460);
                let at = [c[0], add(c[1], 17.0)?, add(c[2], 3.0)?];
                let k = self.start_fx_sub(0.0, 0x5d, Some(at), 0)?;
                if 0 <= k {
                    self.sfx_set_morph(k as usize, 30.0, 0x5a, 0);
                    self.quest_wall_sfx = k;
                }
                damage = mul(health, ten)?;
                self.critter_damage(damage, b, me, 0, None, None, 0)?;
                self.crm_pub(b).set_f32(0xb50, ten);
            }
            _ => {}
        }
        if 0.0 < speed {
            let p60 = self.players[p].v3(0x60);
            let k = if target.is_none() {
                let mut d = [sub(aim[0], p60[0])?, sub(aim[1], p60[1])?, sub(aim[2], p60[2])?];
                crate::collision::normal_vector_pub(&mut d)?;
                let start = [add(d[0], p60[0])?, add(add(d[1], p60[1])?, 2.0)?, add(d[2], p60[2])?];
                let k = self.start_fx_sub(6.0, 0x5d, Some(start), sflags | 8)?;
                if 0 <= k {
                    let v = [mul(speed, d[0])?, mul(speed, d[1])?, mul(speed, d[2])?];
                    self.sfx_set_physics_pub(k as usize, gravity, radius, Some(v), Some(spin));
                }
                k
            } else {
                let f = self.players[p].v3(0x40);
                let start = [add(f[0], p60[0])?, add(add(f[1], p60[1])?, 2.0)?, add(f[2], p60[2])?];
                let k = self.start_fx_sub(6.0, 0x5d, Some(start), sflags | 8)?;
                if 0 <= k {
                    self.sfx_set_physics_pub(k as usize, 0.0, radius, None, Some(spin));
                    // SfxSetHitTarget(speed, effect, node).
                    let r = self.fx_mut(k as usize);
                    r.set_f32(super::effects::fx::GRAVITY, speed);
                    r.or_u32(super::effects::fx::FLAGS, 0x4000_0000);
                    if let Some(n) = self.effects.nodes[k as usize].as_mut() {
                        n.target = target;
                    }
                }
                k
            };
            if 0 <= k {
                self.sfx_set_damage(k as usize, damage, 0.0, 0.0, 0, me as i16);
            }
            // The PARTICLE1_A / CHIMKEY_PART particle system is presentation.
        }
        Ok(())
    }

    /// The +0x8d8 event word consumers (0x14b5c0..0x14c0f0).
    #[allow(clippy::too_many_arguments)]
    fn consume_action_events(&mut self, p: usize, radius: f32, height: f32, pos: V3, to: V3, facing: f32, aim: &mut V3, mut dist: f32, target: &mut i32, item: &mut Option<usize>) -> R<()> {
        let me = self.players[p].u32_(0) as i16;
        let ev = self.players[p].u32_(0x8d8);
        let r138 = self.players[p].u32_(0x138);
        if ev & 0x200_0000 != 0 {
            // The thunder hammer: StartFXSub(0, 0x1c, 0, 0x2a, 0x880) on the
            // player's tree, SfxSetDamage(100, 35, 0.1, …, 0x20).
            let k = self.start_fx_sub(0.0, 0x1c, None, 0x2a)?;
            if 0 <= k {
                self.parent_fx_to_player(k as usize, p, [0.0; 3]);
                self.sfx_set_damage(k as usize, 100.0, 35.0, f32::from_bits(0x3dcc_cccd), 0x20, me + 1);
            }
            self.players[p].and_u32(0x8d8, 0xfdff_ffff);
            self.player_use_powerup(1.0, p, 5, 0x1000_0000)?;
            self.shake_camera(f32::from_bits(0x3e99_999a), 0, 0, 0x1e, 200);
            self.audio_player_sfx(p, 0x50, 0x28);
        }
        if self.players[p].u32_(0x8d8) & 0x100_0000 != 0 {
            // Breath weapons (+0x138): horn/mask 0x38, fire 0x34, electric
            // 0x35, acid 0x36.
            let (header, damage, flags) = if r138 & 0x3000 != 0 {
                (0x38, 50.0, 0x21)
            } else if r138 & 0x410 != 0 {
                (0x34, 40.0, 0x21)
            } else if r138 & 0x20 != 0 {
                (0x35, 40.0, 0x24)
            } else if r138 & 0x40 != 0 {
                (0x36, 40.0, 0x22)
            } else {
                (0, 0.0, 0)
            };
            if header != 0 {
                let k = self.start_fx_sub(0.0, header, None, 0x2a)?;
                if 0 <= k {
                    self.sfx_set_damage(k as usize, damage, 20.0, 0.0, flags, me + 1);
                    if r138 & 0x3000 == 0 {
                        self.audio_player_breath(p);
                    } else if r138 & 0x1000 != 0 {
                        self.audio_player_sfx(p, 0x5a, 0x28);
                    } else {
                        self.audio_player_sfx(p, 0x5b, 0x28);
                    }
                    if r138 & 0x400 != 0 && self.players[p].u32_(0x768) != 0 {
                        // The turbo dragon's own head node (powerup model).
                        let t = sub(self.players[p].f32_(0x800), self.players[p].f32_(0x8e8))?;
                        self.players[p].set_f32(0x800, t);
                        self.players[p].set_f32(0x8e8, 0.0);
                        self.audio_player_turbo(p, 0, 0);
                    }
                    // SfxSetParent(+0x6ac head node). HYPOTHESIS: the head sits
                    // at the attention offset +0x810.
                    let head = self.players[p].v3(0x810);
                    self.parent_fx_to_player(k as usize, p, head);
                    self.fx_mut(k as usize).set_f32(super::effects::fx::CONE, f32::from_bits(0x3f5d_b22d));
                    self.player_use_powerup(1.0, p, 9, 0x70)?;
                }
            }
            self.players[p].and_u32(0x8d8, 0xfeff_ffff);
        }
        let f1f0 = [sinf(facing)?, 0.0, cosf(facing)?];
        let mut ev = self.players[p].u32_(0x8d8);
        if ev & 0x1000_0000 != 0 {
            if r138 & 0x80 != 0 || self.players[p].u32_(0x720) != 0 {
                self.familiar_fireball(p, dist, f1f0, *aim, *target, *item)?;
            }
            self.players[p].and_u32(0x8d8, 0xefff_ffff);
            ev = self.players[p].u32_(0x8d8);
        }
        let mut melee_reach = radius;
        if ev & 0xff != 0 {
            if ev & 0xfe != 0 {
                let mut dmg = self.players[p].f32_(0x118);
                let mut flags = self.players[p].u32_(0x130);
                if self.players[p].u32_(0x880) == 0 && self.controls.control[p].f32_(0x20) == 0.0 {
                    let k = add(radius, radius)?;
                    let probe = [add(mul(k, f1f0[0])?, pos[0])?, add(mul(k, f1f0[1])?, pos[1])?, add(mul(k, f1f0[2])?, pos[2])?];
                    self.player_collide_items(radius, height, p, pos, probe, None, false, None)?;
                }
                // piStack_114 carries over from the targeting stage (the
                // probe's +0x880 update is not re-read).
                let mut tgt = -1;
                dist = self.player_get_target(p, to, f1f0, aim, &mut tgt, item)?;
                *target = tgt;
                let enemy = usize::try_from(tgt).ok().filter(|&t| t < 0x10000);
                let crit = if 0x10000 <= tgt { Some((tgt & 0xffff) as usize) } else { None };
                let ev = self.players[p].u32_(0x8d8);
                if ev & 0xf0 == 0 {
                    if ev & 4 != 0 {
                        dmg = add(dmg, dmg)?;
                        flags |= 0x10;
                    } else if ev & 8 != 0 && let Some(en) = enemy && self.enemies[en].f32_(super::enemies::e::HALF_HEIGHT) <= 2.0 {
                        flags |= 0x20;
                    }
                } else {
                    dmg = mul(dmg, 3.0)?;
                    flags |= 0x20;
                    let t = sub(self.players[p].f32_(0x800), self.players[p].f32_(0x8e8))?;
                    self.players[p].set_f32(0x800, t);
                    self.players[p].set_f32(0x8e8, 0.0);
                }
                let reach = if ev & 2 == 0 { add(radius, 2.0)? } else { add(add(radius, 1.0)?, 1.0)? };
                melee_reach = add(radius, 2.0)?;
                let hit = [add(mul(melee_reach, f1f0[0])?, pos[0])?, add(mul(melee_reach, f1f0[1])?, pos[1])?, add(mul(melee_reach, f1f0[2])?, pos[2])?];
                if dist < reach {
                    if tgt < 0 {
                        match *item {
                            None => {
                                if self.globals.pvp_mode == 2 {
                                    self.player_damage_player(dmg, p, f1f0, flags)?;
                                }
                            }
                            Some(it) => self.player_damage_item(dmg, p, it, flags)?,
                        }
                    } else {
                        let tp = match (enemy, crit) {
                            (Some(en), _) => self.enemies[en].v3(super::enemies::e::COLLISION),
                            (_, Some(k)) => self.crit(k).rec.v3(super::critters::c::COL),
                            _ => unreachable!(),
                        };
                        if self.fast_wall_collide(pos, tp, 0)?.is_none() {
                            let r = self.player_damage_target(dmg, 0.0, p, tgt, flags, Some(hit), true)?;
                            if enemy.is_none_or(|en| 2.0 < self.enemies[en].f32_(super::enemies::e::HALF_HEIGHT)) {
                                let v = self.players[p].i32_(0x90c) + if r != 0 { 3 } else { 1 };
                                self.players[p].set_i32(0x90c, v);
                                self.players[p].set_f32(0x910, self.globals.time);
                            }
                        }
                    }
                }
            }
            let ev = self.players[p].u32_(0x8d8);
            if ev & 1 != 0 {
                self.players[p].set_f32(0x8d4, self.globals.time);
                if r138 & 0x400 != 0 {
                    self.players[p].or_u32(0x8d8, 0x2000_0000);
                }
            }
            self.players[p].and_u32(0x8d8, 0xffff_ff00);
        }
        let _ = melee_reach;
        let ev = self.players[p].u32_(0x8d8);
        if ev & 0xff00 != 0 {
            let mut flags = self.players[p].u32_(0x130);
            let (charge, power, mode) = if ev & 0x6000 != 0 {
                (0.06, 1.0, 0)
            } else if ev & 0x1000 != 0 {
                flags |= 0x200_0010;
                (0.06, 2.0, 2)
            } else if ev & 0x800 != 0 {
                if self.player_use_powerup(1.0, p, 5, 0x10_0000)? == 0 {
                    (0.0, 1.0, 0)
                } else {
                    flags |= 0x10_0000;
                    (0.0, if 0 <= self.globals.boss_type { 1.5 } else { 2.0 }, 0)
                }
            } else {
                let mut c = sub(sub(self.globals.time, self.players[p].f32_(0x8d4))?, f32::from_bits(0x3e8a_3d71))?;
                c = c.clamp(0.0, 0.1);
                (c, 1.0, 1)
            };
            // GetAttackDir(dist, charge, …): the launch distance depends on the charge.
            let _ = dist;
            let k = self.get_attack_dir(charge, p, f1f0, aim, flags, *target, *item)?;
            self.player_start_missile(k, power, p, aim, flags, mode)?;
            let cue = if r138 & 0x8000 != 0 {
                "AudioGauntletL"
            } else if r138 & 0x4000 != 0 {
                "AudioGauntletR"
            } else {
                "AudioThrowWeapon"
            };
            match cue {
                "AudioThrowWeapon" => self.audio_throw_weapon(p)?,
                "AudioGauntletL" => self.audio_player_sfx(p, 0x5c, 0x28),
                _ => self.audio_player_sfx(p, 0x5d, 0x28),
            }
            let v = (self.players[p].u32_(0x8d8) & 0xffff_00ff) | 0x1000_0000;
            self.players[p].set_u32(0x8d8, v);
        }
        if self.players[p].u32_(0x8d8) & 0x10000 != 0 {
            // StartMagicPlayerFX (the potion glow, a presentation effect).
            self.players[p].and_u32(0x8d8, 0xfffe_ffff);
        }
        let ev = self.players[p].u32_(0x8d8);
        if ev & 0x60000 != 0 {
            if self.players[p].i32_(0x80c) == 0 && self.globals.quest_item_state.wrapping_sub(1) >= 4 {
                let mode = if ev & 0x40000 == 0 {
                    if self.players[p].u16_(0x92e) & 2 == 0 {
                        0
                    } else {
                        self.audio_turbo_defense(p);
                        1
                    }
                } else if self.controls.control[p].u32_(4) & 0x10000 != 0 {
                    3
                } else {
                    2
                };
                let n = self.players[p].i32_(0x1a18);
                let at = self.players[p].v3(0x50);
                if n < 1 {
                    self.start_magic(1.0, me as i32, at, 0, mode)?;
                } else {
                    self.players[p].set_i32(0x1a18, n - 1);
                    let colour = self.players[p].u32_(0x2a98 + 4 * n as usize);
                    self.start_magic(1.0, me as i32, at, colour, mode)?;
                }
                self.players[p].set_u16(0x92e, 0x80);
            }
            self.players[p].and_u32(0x8d8, 0xfff9_ffff);
        }
        Ok(())
    }

    /// SfxSetParent(effect, player node) with a local offset.
    pub(crate) fn parent_fx_to_player(&mut self, k: usize, p: usize, local: V3) {
        if let Some(n) = self.effects.nodes[k].as_mut() {
            n.parent = Some(super::effects::EffectParent { player: p, local, local_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] });
        }
    }

    /// The familiar's fireball (0x14b940..0x14ba40, StartFamiliarFireBall
    /// 0x17e370): from the class offset PDAT +0x170 in the player's frame,
    /// GetAttackDir with charge 0.03, CalcTargetDir(50, 0.02, 10 / 0, −0.5 / 0).
    fn familiar_fireball(&mut self, p: usize, dist: f32, facing: V3, aim: V3, target: i32, item: Option<usize>) -> R<()> {
        let _ = dist;
        let me = self.players[p].u32_(0) as usize;
        let raw = self.classes[me].raw.clone();
        let f = |o: usize| f32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
        // The tree's node scale (+0x40..+0x48): PlayerProcessScale's value.
        let sc = self.player_scale(p);
        let off = [mul(f(0x170), sc)?, mul(f(0x174), sc)?, mul(f(0x178), sc)?];
        let r = &self.players[p];
        let rows = [r.v3(0x20), r.v3(0x30), r.v3(0x40)];
        let base = r.v3(0x50);
        let col = |c: usize| -> R<f32> { Ok(add(add(mul(off[0], rows[0][c])?, mul(off[1], rows[1][c])?)?, mul(off[2], rows[2][c])?)?) };
        let start = [add(col(0)?, base[0])?, add(col(1)?, base[1])?, add(col(2)?, base[2])?];
        let mut dir = aim;
        let flags = self.players[p].u32_(0x130);
        let k = self.get_attack_dir(f32::from_bits(0x3cf5_c28f), p, facing, &mut dir, flags, target, item)?;
        let mut vel = [mul(k, dir[0])?, mul(k, dir[1])?, mul(k, dir[2])?];
        let gravity = if self.globals.boss_type < 0 {
            super::effects::calc_target_dir(50.0, f32::from_bits(0x3ca3_d70a), 10.0, -0.5, &mut vel)?;
            10.0
        } else {
            super::effects::calc_target_dir(50.0, f32::from_bits(0x3ca3_d70a), 0.0, 0.0, &mut vel)?;
            0.0
        };
        let (kind, damage) = if self.players[p].u32_(0x138) & 0x80 == 0 {
            let lvl = self.players[p].i32_(0x2ac0);
            (me, add(mul((lvl - 0x19) as f32, f32::from_bits(0x3dcc_cccd))?, 2.5)?)
        } else {
            (4, 10.0)
        };
        // StartFXTree(3, FamiliarSpit[kind], start, 0x101000e, 0x880).
        let visual = super::effects::Visual::Familiar { player: me, phoenix: kind == 4 };
        let fx = self.start_fx_tree(3.0, visual, 0, 0, Some(start), 0x101_000e)?;
        let v = [mul(35.0, vel[0])?, mul(35.0, vel[1])?, mul(35.0, vel[2])?];
        self.sfx_set_physics_pub(fx, gravity, 1.0, Some(v), None);
        // fam_dmg_type 0x240818: 0 for the class familiars, 0x11 for the phoenix.
        let dtype = if kind == 4 { 0x11 } else { 0 };
        self.sfx_set_damage(fx, damage, 0.0, 0.0, dtype, me as i16 + 1);
        // SfxSetLight(3, light_color[pclr_idx[colour]]) — on effect slot
        // `kind` (a0 = the FamiliarSpit index, 0x17e494), not the new
        // fireball: as the original.
        let t = &self.effects.tables;
        let light = t.light_color[t.pclr_idx.get(self.players[p].u32_(4) as usize).copied().unwrap_or(0)];
        if kind < self.effects.slots.len() {
            self.sfx_set_light(kind, 3.0, light);
        }
        Ok(())
    }

    /// PlayerProcessScale's node scale: class 0xc 1.6, the giant powerup
    /// (+0x138 0x100) 1.3, level > 98 1.2, otherwise 1.
    pub(crate) fn player_scale(&self, p: usize) -> f32 {
        let r = &self.players[p];
        if r.u32_(0xc) == 0xc {
            f32::from_bits(0x3fcc_cccd)
        } else if r.u32_(0x138) & 0x100 != 0 {
            f32::from_bits(0x3fa6_6666)
        } else if 0x62 < r.i32_(0x2ac0) {
            f32::from_bits(0x3f99_999a)
        } else {
            1.0
        }
    }

    /// PlayerDamagePlayer 0x1473e8(damage, player, facing, flags): the
    /// nearest other player in state 1 inside the 0.707 cone whose distance
    /// less its +0x828 radius is below the attacker's radius + 2 takes
    /// damage_player(damage, k, 2, flags, dir) — dir is the last examined
    /// player's direction (the loop's vector), as in the original.
    fn player_damage_player(&mut self, damage: f32, p: usize, facing: V3, flags: u32) -> R<()> {
        let mut best = add(self.players[p].f32_(0x828), 2.0)?;
        let mut found = None;
        let mut dir = [0.0f32; 3];
        let at = self.players[p].v3(0x50);
        for k in 0..4 {
            if k == p || self.players[k].i32_(0xfc) != 1 {
                continue;
            }
            let o = self.players[k].v3(0x50);
            dir = [sub(o[0], at[0])?, sub(o[1], at[1])?, sub(o[2], at[2])?];
            let dist = crate::collision::normal_vector(&mut dir)?;
            let dot = add(add(mul(dir[0], facing[0])?, mul(dir[1], facing[1])?)?, mul(dir[2], facing[2])?)?;
            if f32::from_bits(0x3f34_fdf4) <= dot {
                let dk = sub(dist, self.players[k].f32_(0x828))?;
                if dk < best {
                    best = dk;
                    found = Some(k);
                }
            }
        }
        if let Some(k) = found {
            self.damage_player(damage, k, 2, flags, Some(dir))?;
        }
        Ok(())
    }

    /// PlyrSfxDoDamage for the class special actions (0x14c818).
    fn action_sfx_damage(&mut self, p: usize) -> R<()> {
        if 2 <= self.players[p].i32_(0x80c) {
            return Ok(());
        }
        let off = match self.players[p].u32_(0x1e0) {
            0x23 => [0xc, 0],
            0x25 => [0x10, 0],
            0x3c => [0x12, 0],
            0x54 => [0xe, 0],
            0x56 => [0x16, 0],
            0x57 => [0x18, 0x1a],
            0x58 => [0x1c, 0],
            0x5a => [0x1e, 0],
            99 => [0x14, 0],
            0x7b => [0x22, 0],
            _ => return Ok(()),
        };
        let k = self.players[p].u32_(0) as usize;
        let prev = self.players[p].f32_(0x974);
        let frame = self.players[p].f32_(0xac);
        let partner = ptr(self.players[p].u32_(0x690));
        // 0x58: StartComboFX at the partner (presentation) when the action
        // starts (+0x974 < 0, frame ≥ 0).
        for o in off.into_iter().filter(|&o| o != 0) {
            let d = self.class_sfx(k, o);
            if 0 <= d {
                self.plyr_sfx_do_damage(prev, frame, p, d, partner, None)?;
            }
        }
        Ok(())
    }
}

/// ReflectVector2D 0x1d6058 (y is not written; callers read x/z only).
fn reflect_vector_2d(v: V3, n: V3) -> R<V3> {
    let k = -add(mul(v[0], n[0])?, mul(v[2], n[2])?)?;
    let k2 = add(k, k)?;
    Ok([add(mul(k2, n[0])?, v[0])?, 0.0, add(mul(k2, n[2])?, v[2])?])
}
