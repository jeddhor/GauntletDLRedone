//! Per-frame items: ProcessItems 0x154ee8 (with ItemVisible 0x153ad0,
//! generate_now 0x153d50, did_generate 0x153df8, generate_single 0x153e70,
//! GetItemDir 0x1539a8, DeleteItem 0x1536f8, DistanceToClosestPlayer
//! 0x153b88, OnWobj 0x1539c8) and ProcessItemWobjs 0x154790.
//!
//! The camera is supplied by [`Camera`] (rest-pose hypothesis until do_camera
//! is ported); item animation clocks use the verified AnimateTree selection.
use super::enemies::e;
use super::{unsupported, Event, Game, R};

const HALF_PI: f32 = f32::from_bits(0x3fc9_0fdb);
use crate::animation_clock::{select, ClipTiming, ClockState, SelectionInput};
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sqrt_rn, sub_guarded_rz as sub};

type V3 = [f32; 3];

/// Native camera for the culling queries: basis rows (right, up, forward),
/// eye and the projection plane constants of the view struct (+0x60..+0x74).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Camera {
    pub basis: [V3; 3],
    pub eye: V3,
    /// StdCam +0xa4: the camera target.
    pub target: V3,
    /// near, far, horizontal (z, x) and vertical (z, y) plane coefficients.
    pub planes: [f32; 6],
    pub newcam_mode: i32,
}

impl Camera {
    /// MBWorldSphereClip 0x1c3eb0 == 0 (MBWorldSphereVisible).
    pub fn sphere_visible(&self, r: f32, p: V3) -> R<bool> {
        let d = [sub(p[0], self.eye[0])?, sub(p[1], self.eye[1])?, sub(p[2], self.eye[2])?];
        let dot = |b: V3| -> R<f32> { Ok(add(add(mul(d[0], b[0])?, mul(d[1], b[1])?)?, mul(d[2], b[2])?)?) };
        let (vx, vy, vz) = (dot(self.basis[0])?, dot(self.basis[1])?, dot(self.basis[2])?);
        let [near, far, kz, kx, kz2, ky] = self.planes;
        if vz < sub(near, r)? || add(r, far)? < vz {
            return Ok(false);
        }
        let h = add(mul(vz, kz)?, r)?;
        let x = mul(vx, kx)?;
        if h < x || x < -h {
            return Ok(false);
        }
        let v = add(mul(vz, kz2)?, r)?;
        let y = mul(vy, ky)?;
        Ok(!(v < y || y < -v))
    }
}

/// Native item animation clock (atree at record +0x80).
#[derive(Clone, Debug)]
pub struct ItemAnim {
    pub clock: ClockState,
    pub clips: Vec<ClipTiming>,
    pub current: u16,
}

impl Game {
    fn dist3(a: V3, b: V3) -> R<f32> {
        let d = [sub(a[0], b[0])?, sub(a[1], b[1])?, sub(a[2], b[2])?];
        Ok(sqrt_rn(add(add(mul(d[0], d[0])?, mul(d[1], d[1])?)?, mul(d[2], d[2])?)?)?)
    }

    /// DistanceToClosestPlayer (game_mode != 0x8008): limited to 1000.
    pub fn distance_to_closest_player(&self, p: V3) -> R<f32> {
        let mut best = 1000.0;
        for pl in &self.players {
            if pl.i32_(0xfc) == 1 {
                let d = Self::dist3(pl.v3(0x50), p)?;
                if d < best {
                    best = d;
                }
            }
        }
        Ok(best)
    }

    /// ItemVisible.
    fn item_visible(&self, slot: usize) -> R<bool> {
        let Some(cam) = &self.camera else { return Ok(true) };
        let r = &self.items.records[slot];
        let p = r.v3(0x50);
        let mut v = cam.sphere_visible(r.f32_(0xe8), p)?;
        if v && (cam.newcam_mode as u32).wrapping_sub(1) < 2 {
            let d = [sub(cam.target[0], p[0])?, sub(cam.target[1], p[1])?, sub(cam.target[2], p[2])?];
            if self.item_active_dist < add(add(mul(d[0], d[0])?, mul(d[1], d[1])?)?, mul(d[2], d[2])?)? {
                v = false;
            }
        }
        Ok(v)
    }

    /// DeleteItem(slot, with contents).
    pub fn delete_item(&mut self, slot: usize, contents: bool) {
        if contents {
            let r = &self.items.records[slot];
            let k = self.items.definitions[r.definition()].kind();
            if let (true, Some(c)) = (k == 1 || k == 2, r.u32_(0xfc).checked_sub(1)) {
                self.delete_item(c as usize, false);
            }
        }
        self.items.records[slot].set_i16(0xd8, -1);
        self.item_trees.remove(&slot);
        if slot < self.items.first_empty {
            self.items.first_empty = slot;
        }
        if let Some(a) = self.item_anims.get_mut(slot) {
            *a = None;
        }
    }

    /// AnimateATree on an item; returns the original mask.
    fn animate_item(&mut self, slot: usize, clip: u16, mode: i32) -> R<u32> {
        let Some(anim) = self.item_anims.get(slot).cloned().flatten() else { return Ok(0) };
        let (Some(&current), Some(&requested)) = (anim.clips.get(anim.current as usize), anim.clips.get(clip as usize)) else {
            return Ok(0);
        };
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
        let a = self.item_anims[slot].as_mut().unwrap();
        a.clock = out.state;
        if out.return_flags & 1 != 0 {
            a.current = clip;
        }
        // AnimInfo mirrors read by ProcessItems: +0x90 count, +0x94 frames, +0x92 sequence.
        let frames = a.clips[a.current as usize].frames;
        let count = a.clips.len() as i16;
        let r = &mut self.items.records[slot];
        r.set_i16(0x90, count);
        r.set_i16(0x92, a.current as i16);
        r.set_i16(0x94, frames);
        Ok(out.return_flags)
    }

    fn has_item_anim(&self, slot: usize) -> bool {
        self.item_anims.get(slot).is_some_and(|a| a.is_some())
    }

    /// did_generate(generator, max alive).
    fn did_generate(&mut self, slot: usize, max: i32) -> R<()> {
        let r = &mut self.items.records[slot];
        let t = mul(mul(f32::from(r.u8_(0xfb)), 6.0)?, add(r.f32_(0xfc), 1.0)?)?;
        r.set_i16(0xf8, t as i32 as i16);
        let m = max as f32;
        let f = add(div(1.0, add(m, m)?)?, r.f32_(0xfc))?;
        r.set_f32(0xfc, if 1.0 < f { 0.0 } else { f });
        Ok(())
    }

    /// generate_now(generator, position, max alive, visible).
    fn generate_now(&mut self, slot: usize, p: V3, max: i32, visible: bool) -> R<bool> {
        if self.globals.paused != 0 || self.globals.cam_wait != 0 {
            return Ok(false);
        }
        let t = self.items.records[slot].i16_(0xf8);
        if 0 < t {
            self.items.records[slot].set_i16(0xf8, t.wrapping_sub(self.globals.vb_elapsed as i16));
            return Ok(false);
        }
        if max < 1 || (self.no_enemy_slot && !visible) || self.globals.game_mode == 0x400c {
            return Ok(false);
        }
        Ok(self.distance_to_closest_player(p)? <= 1000.0)
    }

    /// Facing and spawn bookkeeping shared by generator spawns.
    fn orient_spawn(&mut self, k: usize, facing: f32) -> R<()> {
        let h = super::enemy_ai::wrap(add(facing, self.enemies[k].f32_(0x26c))?)?;
        let rec = &mut self.enemies[k];
        rec.set_f32(0x260, h);
        rec.set_f32(0x264, h);
        let p = rec.v3(e::POSITION);
        rec.set_v3(0x300, p);
        rec.set_u32(0x254, 0);
        rec.set_f32(0x258, h);
        rec.set_u32(0x25c, 0);
        Ok(())
    }

    /// SetPlayerVars 0x125ef8 (game_main calls it every frame; ProcessItems
    /// when no player is in game): see the body for the ported writes.
    pub fn set_player_vars(&mut self) {
        // SetPlayerVars 0x125ef8: StopTime (a time-stop powerup, +0x138 bit
        // 8), EnemyShrink (0.667 per shrink powerup, bit 0x200, outside boss
        // levels), ActivePlayerMask, +0x93e cleared, then SetNumPlayers.
        // NumVirtualPlayers = 0x20f954 when set (not captured: kept as is).
        self.stop_time = 0;
        self.old_enemy_shrink = self.globals.enemy_shrink;
        self.globals.enemy_shrink = 1.0;
        self.active_player_mask = 0;
        let (mut total, mut in_game) = (0, 0);
        for k in 0..4 {
            let st = self.players[k].i32_(0xfc);
            if st != 0 {
                total += 1;
                if 1 < (st as u32).wrapping_sub(2) {
                    in_game += 1;
                }
            }
            if st == 1 || st == 5 {
                self.active_player_mask |= 1 << k;
            }
            if st == 1 {
                let f = self.players[k].u32_(0x138);
                if f & 8 != 0 {
                    self.stop_time = 1;
                }
                if self.globals.boss_type < 0 && f & 0x200 != 0 {
                    self.globals.enemy_shrink = crate::numeric::mul_rz(self.globals.enemy_shrink, f32::from_bits(0x3f2a_c083)).unwrap_or(self.globals.enemy_shrink);
                }
            }
            self.players[k].set_u16(0x93e, 0);
        }
        self.globals.num_active_players = total;
        self.globals.num_in_game_players = in_game.clamp(0, 4);
    }

    /// Rotator (type 12) case of ProcessItems 0x1566c0: the bound object
    /// (+0xf0) turns by vb_elapsed × +0xf4 per update; subtype 0 always
    /// (unless LockCamReq and object flag 4), subtype 2 once triggered
    /// ((+0xd8 & 0x1001) == 1) until the turned angle +0xfc reaches ±+0xf8,
    /// then +0xd8 |= 0x1000. Subtype 1 only adds/removes a co-op camera
    /// target (presentation). The node turn clears object flag 1.
    fn process_rotator(&mut self, slot: usize) -> R<()> {
        let r = self.items.records[slot].clone();
        let sub_ = self.items.definitions[r.definition()].subtype();
        let Some(o) = r.u32_(0xf0).checked_sub(1).map(|o| o as usize) else { return Ok(()) };
        let vb = self.globals.vb_elapsed as f32;
        let speed = r.f32_(0xf4);
        let angle = match sub_ {
            0 => {
                if self.lock_cam_req != 0 && self.items.objects[o].flags_10 & 4 != 0 {
                    return Ok(());
                }
                mul(vb, speed)?
            }
            2 => {
                if r.u16_(0xd8) & 0x1001 != 1 {
                    return Ok(());
                }
                let (limit, turned) = (r.f32_(0xf8), r.f32_(0xfc));
                let more = if 0.0 <= speed { turned < limit } else { -limit < turned };
                let at = self.collision.object_matrix(o)?[3];
                let at = [at[0], at[1], at[2]];
                if !more {
                    self.audio_rotator(2, at)?;
                    let rr = &mut self.items.records[slot];
                    rr.or_u16(0xd8, 0x1000);
                    rr.set_u8(0xde, 2);
                    return Ok(());
                }
                self.audio_rotator(0, at)?;
                let a = mul(vb, speed)?;
                self.items.records[slot].set_f32(0xfc, add(a, turned)?);
                a
            }
            _ => return Ok(()),
        };
        self.items.records[slot].set_u8(0xde, 2);
        self.items.objects[o].flags_10 &= 0xffff_fffe;
        self.collision.yaw_object(o, angle).map_err(|e| super::Unsupported(format!("rotator: {e:?}")))?;
        Ok(())
    }

    /// trans_enemy 0x139888: check_enemy_pos at `to` (+0x244 lift). A free
    /// position returns -1; otherwise the enemy is removed (+0xc8 = 0; the
    /// MBTreeSetFlags(…, 2, 0) calls on +0x70/+0x1f0 are presentation) and 0
    /// is returned.
    fn trans_enemy(&mut self, k: usize, to: [f32; 3]) -> R<i32> {
        let p = [to[0], add(to[1], self.enemies[k].f32_(0x244))?, to[2]];
        if 0 < self.check_enemy_pos_pub(p, None, k)? {
            return Ok(-1);
        }
        self.enemies[k].set_i32(e::STATE, 0);
        Ok(0)
    }

    /// place_logic12 0x153ff8 (generator field block at item +0xf0): place
    /// the first two spawns 3.5 to either side of the generator facing, the
    /// rest at the spawn point; +0x330 records which (1, 2, 4).
    fn place_logic12(&mut self, slot: usize, k: usize) -> R<()> {
        use super::enemy_ai::{create_ypr_matrix, wrap};
        self.enemies[k].set_i32(0x330, 0);
        let phase = self.items.records[slot].i8_(0xf4);
        let pos = self.enemies[k].v3(e::POSITION);
        if phase == 0 || phase == 1 {
            let f = self.items.records[slot].f32_(0x100);
            let yaw = wrap(if phase == 0 { add(f, HALF_PI)? } else { sub(f, HALF_PI)? })?;
            let m = create_ypr_matrix([0.0, yaw, 0.0])?;
            let v = [0.0f32, 0.0, 3.5];
            let mut w = [0.0f32; 3];
            for (c, o) in w.iter_mut().enumerate() {
                *o = add(add(mul(v[0], m[0][c])?, mul(v[1], m[1][c])?)?, mul(v[2], m[2][c])?)?;
            }
            let to = [add(pos[0], w[0])?, add(pos[1], w[1])?, add(pos[2], w[2])?];
            self.enemies[k].set_v3(0x2f4, to);
            if self.trans_enemy(k, to)? != 0 {
                self.enemies[k].set_i32(0x330, if phase == 0 { 1 } else { 2 });
            }
        } else {
            self.enemies[k].set_v3(0x2f4, pos);
            self.enemies[k].set_i32(0x330, 4);
        }
        if self.enemies[k].i32_(0x330) != 0 {
            let p = self.enemies[k].v3(e::POSITION);
            self.enemies[k].set_v3(0x300, p);
            let r = &mut self.items.records[slot];
            r.set_i8(0xf2, r.i8_(0xf2).wrapping_add(1));
            r.set_i8(0xf4, r.i8_(0xf4).wrapping_add(1));
            if r.i8_(0xf3) <= r.i8_(0xf2) {
                r.set_u8(0xfa, 7);
                r.set_i8(0xf4, 3);
            }
        }
        Ok(())
    }

    /// place_logic13 0x154318: chain the spawns through +0x348/+0x34c with
    /// the generator's +0xf5 holding the newest.
    fn place_logic13(&mut self, slot: usize, k: usize) {
        let last = self.items.records[slot].i8_(0xf5);
        let prev = if last < 0 {
            -1
        } else {
            self.enemies[last as usize].set_i32(e::NEXT, k as i32);
            i32::from(last)
        };
        self.enemies[k].set_i32(e::PREV, prev);
        self.enemies[k].set_i32(e::NEXT, -1);
        let r = &mut self.items.records[slot];
        r.set_i8(0xf5, k as i8);
        r.set_i8(0xf2, r.i8_(0xf2).wrapping_add(1));
        r.set_i8(0xf4, r.i8_(0xf4).wrapping_add(1));
    }

    /// place_logic14 0x154390: alternate ±π/4 turns (+0x330 = 1 / -1).
    fn place_logic14(&mut self, slot: usize, k: usize) -> R<()> {
        let q = f32::from_bits(0x3f49_0fdb);
        let even = self.items.records[slot].u8_(0xf4) & 1 == 0;
        let h = self.enemies[k].f32_(0x260);
        let h = if even { add(h, q)? } else { sub(h, q)? };
        self.enemies[k].set_i32(0x330, if even { 1 } else { -1 });
        let h = super::enemy_ai::wrap(h)?;
        self.enemies[k].set_f32(0x260, h);
        self.enemies[k].set_f32(0x264, h);
        let r = &mut self.items.records[slot];
        r.set_i8(0xf2, r.i8_(0xf2).wrapping_add(1));
        r.set_i8(0xf4, r.i8_(0xf4).wrapping_add(1));
        Ok(())
    }

    /// The generator (type 3) case of ProcessItems.
    fn process_generator(&mut self, slot: usize) -> R<()> {
        let r = self.items.records[slot].clone();
        let visible = r.u16_(0xd8) & 0x4000 != 0;
        if !(1 < self.mode_950 && self.mode_950 != 2 && 0 < r.i8_(0xf6) && r.i16_(0xf0) != -1) {
            return Ok(());
        }
        let max = i32::from(r.i8_(0xf3));
        if r.i8_(0xf7) == 0xf {
            // generate_single(item, 0xf, 0): one enemy per generator (+0xf2
            // counts it) with +0x2ec = 2 and +0x324 = 0xf.
            if r.i8_(0xf2) == 0 {
                let reach = self.items.definitions[r.definition()].f32_(0xc);
                let k = self.generate_enemy(reach, r.v3(0x60), i32::from(r.i16_(0xf0)), i32::from(r.i8_(0xf6)), r.v3(0x30), 0xf, slot as u32 + 1, 0)?;
                if let Ok(k) = usize::try_from(k) {
                    self.enemies[k].set_i16(0x2ec, 2);
                    self.enemies[k].set_i16(0x324, 0xf);
                    self.orient_spawn(k, r.f32_(0x100))?;
                    let rr = &mut self.items.records[slot];
                    rr.set_i8(0xf2, rr.i8_(0xf2) + 1);
                }
            }
            return Ok(());
        }
        if max <= i32::from(r.i8_(0xf2)) {
            return Ok(());
        }
        let p = r.v3(0x60);
        if !self.generate_now(slot, p, max, visible)? {
            return Ok(());
        }
        let def = &self.items.definitions[r.definition()];
        let reach = def.f32_(0xc);
        let dir = r.v3(0x30);
        if r.i8_(0xf7) == 0xc && 2 < r.i8_(0xf4) {
            let rr = &mut self.items.records[slot];
            rr.set_i8(0xf4, rr.i8_(0xf4) - 3);
            rr.set_u8(0xfa, 0);
        }
        let threshold = if visible { 0 } else { -1 };
        let k = self.generate_enemy(reach, p, i32::from(r.i16_(0xf0)), i32::from(r.i8_(0xf6)), dir, i32::from(r.i8_(0xf7)), slot as u32 + 1, threshold)?;
        if k < 0 {
            return Ok(());
        }
        let k = k as usize;
        self.did_generate(slot, max)?;
        self.enemies[k].set_i16(0x2ec, 0);
        self.orient_spawn(k, r.f32_(0x100))?;
        match r.i8_(0xf7) {
            0xc => self.place_logic12(slot, k)?,
            0xd => self.place_logic13(slot, k),
            0xe => self.place_logic14(slot, k)?,
            _ => {
                let rr = &mut self.items.records[slot];
                rr.set_i8(0xf2, rr.i8_(0xf2) + 1);
                rr.set_i8(0xf4, rr.i8_(0xf4).wrapping_add(1));
            }
        }
        Ok(())
    }

    /// The placed-enemy (type 4) case of ProcessItems.
    fn process_placed_enemy(&mut self, slot: usize) -> R<()> {
        let r = self.items.records[slot].clone();
        let t = r.i16_(0xf0);
        if t < 0 {
            return Ok(());
        }
        let gm = self.globals.game_mode;
        if !(gm & 0x8000 == 0 || gm == 0x8006 || gm == 0x8003) {
            return Ok(());
        }
        if !self.start_enemyinfo || self.mode_950 == 0 {
            return Ok(());
        }
        let Some(cam) = self.camera.clone() else { return Ok(()) };
        if !cam.sphere_visible(mul(r.f32_(0xe8), 4.0)?, r.v3(0x40))? {
            return Ok(());
        }
        if t == 0x1f && self.globals.num_virtual_players <= 1 {
            return Ok(());
        }
        if 50.0 < Self::dist3(cam.target, r.v3(0x40))? {
            return Ok(());
        }
        if (t as u32).wrapping_sub(0x1d) < 2 || t == 0x20 {
            // Statues (0x155eb0): EnemyShrink model scale is presentation. They
            // wait for +0xf8 bit 0 (DamageItem), then mark +0xd8 bit 0 and
            // +0xde = 1 for one update, then spawn once +0xda < 1.
            if r.u32_(0xf8) & 1 == 0 {
                return Ok(());
            }
            if r.u16_(0xd8) & 1 == 0 {
                let rr = &mut self.items.records[slot];
                rr.or_u16(0xd8, 1);
                rr.set_u8(0xde, 1);
                return Ok(());
            }
            if 0 < r.i16_(0xda) {
                return Ok(());
            }
        }
        // 0x155f40: gargoyle 0x20 → CritterNewInst(7), golem 0x1d → (3),
        // general 0x21 → (8) at the item matrix (+0x10, translation +0x40);
        // other types, or no instance, generate an enemy.
        let ckind = match t {
            0x20 => Some(7),
            0x1d => Some(3),
            0x21 => Some(8),
            _ => None,
        };
        if let Some(kind) = ckind {
            let m: crate::tree_pose::Mat = std::array::from_fn(|k| r.f32_(0x10 + 4 * k));
            if let Some(k) = self.critter_new_inst(kind, 0, m)? {
                self.delete_item(slot, false);
                if 0.0 < r.f32_(0xfc) {
                    let v = mul(r.f32_(0xfc), self.level_f32(0xb4))?;
                    self.crm_pub(k).set_f32(0xb58, v);
                }
                if let Ok(link) = u32::try_from(r.i16_(0x102)) {
                    // +0xb54: the item dropped at death (native slot + 1).
                    self.crm_pub(k).set_u32(0xb54, link + 1);
                }
                return Ok(());
            }
        }
        let pos = r.v3(0x60);
        let dir = r.v3(0x30);
        let k = self.generate_enemy(1.0, pos, i32::from(t), i32::from(r.i8_(0xf2)), dir, i32::from(r.i8_(0xf3)), 0, 1)?;
        if k < 0 {
            // Errorf reports a missing model or slot; the item stays for a later try.
            return Ok(());
        }
        let k = k as usize;
        self.enemies[k].set_i16(0x2ec, 1);
        self.enemies[k].set_i16(0x2e8, 1);
        let h = super::enemy_ai::wrap(crate::libm::atan2f(dir[0], dir[2])?)?;
        {
            let rec = &mut self.enemies[k];
            rec.set_f32(0x260, h);
            rec.set_f32(0x264, h);
            rec.set_f32(0x258, h);
        }
        let m = super::enemy_ai::create_ypr_matrix(self.enemies[k].v3(0x254))?;
        let rec = &mut self.enemies[k];
        for (row, values) in m.iter().take(3).enumerate() {
            for (col, v) in values.iter().enumerate() {
                rec.set_f32(0x10 + 16 * row + 4 * col, *v);
            }
        }
        let p = rec.v3(e::POSITION);
        rec.set_v3(0x300, p);
        let et = rec.i32_(e::TYPE);
        if (et as u32).wrapping_sub(0x1e) < 2 || r.i8_(0xf2) != 0 {
            if r.i8_(0xf2) < 4 {
                rec.set_i32(0x220, 0x1e);
            }
        } else {
            rec.set_i32(e::STATE, 6);
        }
        if et == 0x1e {
            rec.set_f32(0x314, 1000.0);
        } else if 0.0 < r.f32_(0xfc) {
            let v = mul(r.f32_(0xfc), self.level_f32(0xb4))?;
            self.enemies[k].set_f32(0x314, v);
        }
        let n = r.i16_(0x100);
        if 0 < n {
            let v = if n == 1 { 0.0 } else { mul(f32::from(n), f32::from_bits(0x3dcc_cccd))? };
            self.enemies[k].set_f32(0x38c, v);
        }
        let carried = r.i16_(0x102);
        if 0 <= carried {
            self.enemies[k].set_u32(e::CARRIED, carried as u32 + 1);
        }
        self.delete_item(slot, false);
        Ok(())
    }

    /// ProcessItems.
    pub fn process_items(&mut self) -> R<()> {
        let quiet = if self.globals.game_mode == 0x8008 {
            false
        } else if self.globals.game_mode == 0x4010 {
            self.globals.start_of_stage != 0
        } else {
            true
        };
        self.items.session.random_item_num = self.items.session.random_item_num.wrapping_add(1);
        self.no_enemy_slot = false;
        self.find_enemy_slot_probe()?;
        if self.globals.end_of_stage == 0 {
            self.exit_state = 0;
        }
        if self.globals.num_in_game_players == 0 {
            self.set_player_vars();
        }
        let vb = self.globals.vb_elapsed as i16;
        let n = self.items.records.len();
        for slot in 0..n {
            let r = &self.items.records[slot];
            if !r.active() {
                continue;
            }
            let kind = self.items.definitions[r.definition()].kind();
            if kind == -1 {
                continue;
            }
            let visible = self.item_visible(slot)?;
            if !quiet || kind == 10 {
                let rr = &mut self.items.records[slot];
                if visible {
                    rr.or_u16(0xd8, 0x4000);
                } else {
                    rr.and_u16(0xd8, 0xbfff);
                }
            }
            let is_key = kind == 1 && self.items.definitions[self.items.records[slot].definition()].subtype() == 2;
            let pinned = self.globals.global_958 != 0 || self.globals_954 != 0;
            let hidden = self.items.records[slot].u8_(0xe1);
            let mut animate = false;
            if hidden == 1 || (hidden == 2 && (is_key || pinned)) {
                if !visible || is_key || pinned {
                    if self.show_item_now(slot) {
                        self.items.records[slot].set_u8(0xe1, 0);
                        animate = true;
                    }
                } else if !quiet {
                    self.items.records[slot].set_u8(0xe1, 2);
                }
            } else if hidden == 0 {
                if !self.show_item_now(slot) {
                    if !visible || pinned {
                        self.items.records[slot].set_u8(0xe1, 1);
                    }
                } else {
                    animate = true;
                }
            }
            if animate {
                self.animate_item_state(slot, visible, vb)?;
            }
        }
        // The music attention area the nearest kind-0xd item asks for.
        let mut attn = (-1i32, 0i32);
        for slot in 0..self.items.records.len() {
            let r = &self.items.records[slot];
            if !r.active() || r.u16_(0xd8) & 0x8100 != 0 {
                continue;
            }
            let mut r = r.clone();
            crate::items::ItemWorld::update_offsets(&mut r)?;
            self.items.records[slot] = r.clone();
            let d = &self.items.definitions[r.definition()];
            let (kind, subtype) = (d.kind(), d.subtype());
            if kind == -1 || r.u8_(0xe1) != 0 {
                continue;
            }
            let always = (kind == 3 && r.i8_(0xf7) == 0xf) || (kind == 0xc && subtype == 2);
            if !always && r.u16_(0xd8) & 0x4040 == 0 {
                continue;
            }
            match kind {
                1 => {
                    let t = r.i16_(0x100);
                    if 0 < t {
                        self.items.records[slot].set_i16(0x100, t.wrapping_sub(vb));
                    }
                    if subtype == 0xf && r.u16_(0xd8) & 0x800 != 0 {
                        self.crystal_fade(slot)?;
                    }
                }
                2 => self.process_chest(slot)?,
                3 => self.process_generator(slot)?,
                4 => self.process_placed_enemy(slot)?,
                5 => self.process_trigger(slot)?,
                8 => {
                    if self.stop_time != 0 {
                        let rr = &mut self.items.records[slot];
                        rr.set_u8(0xdc, 0);
                        rr.set_u8(0xde, 0);
                        rr.set_u16(0x92, 0);
                        rr.set_i16(0xda, 0x1e);
                    } else if r.u8_(0xdc) == 1 && r.u8_(0xdd) == 0 && subtype == 4 {
                        let near = match &self.camera {
                            Some(cam) => Self::dist3(cam.target, r.v3(0x40))? < 40.0,
                            None => false,
                        };
                        if near {
                            self.sfx3(0x25_0001, r.v3(0x40), 0xe0, 0x32)?;
                        }
                    }
                    let rr = &mut self.items.records[slot];
                    rr.set_u8(0xdd, rr.u8_(0xdc));
                }
                9 => self.process_exit(slot)?,
                10 => self.process_type10(slot)?,
                0xc => self.process_rotator(slot)?,
                0xd => self.process_sound_item(slot, &mut attn)?,
                _ => {}
            }
        }
        if 0 <= attn.0 && attn.0 != self.bg_music.attn_area {
            self.set_attention_area(attn.0, attn.1);
        }
        self.process_item_wobjs()?;
        self.did_rock_fly = self.do_rock_fly;
        Ok(())
    }

    /// ProcessItems case 0xd: a world sound (+0xfc < 1: the +0xf4 loop at a
    /// level by the nearest player's distance against the +0xf0 radius,
    /// DoItemSound 0x15d2b0) or a music attention area (+0xfc − 1, chosen
    /// when a player is inside the radius; +0x100 the change mode). The
    /// +0xf8 node (a moving world node) is presentation.
    fn process_sound_item(&mut self, slot: usize, attn: &mut (i32, i32)) -> R<()> {
        let r = self.items.records[slot].clone();
        let radius = r.f32_(0xf0);
        let pos = r.v3(0x40);
        let d = self.distance_to_closest_player(pos)?;
        let area = r.i16_(0xfc);
        if 0 < area {
            let a = i32::from(area) - 1;
            if d < radius && attn.0 < a {
                *attn = (a, i32::from(r.i16_(0x100)));
            }
            return Ok(());
        }
        let k = if d < radius || radius <= 2.0 {
            1.0
        } else {
            let t = sub(mul(radius, 1.5)?, d)?;
            crate::numeric::div_rn(add(t, t)?, radius)?
        };
        let state = r.i16_(0xfe);
        if 0.0 < k || state < 0 {
            let id = r.i32_(0xf4);
            let playing = self.do_item_sound(slot, k, id, pos, r.u16_(0x100))?;
            self.items.records[slot].set_i16(0xfe, i16::from(playing));
        } else if 0 < state {
            self.events.push(Event::ItemSoundStop { slot });
            self.items.records[slot].set_i16(0xfe, 0);
        }
        Ok(())
    }

    /// DoItemSound(level, id, position, flags): volume level × 224 × LEVL
    /// +0x98 (16 while Sumner speaks, a trigger camera runs or time stops;
    /// the first treasure room ×4 within 0x40..0xff); killed in game modes
    /// 0x4014/0x400c and at the end of the stage; priority 3 with flag 2,
    /// else 0x70; flag 1 lowers the music (LowerBGMusic(0.5, 1 − level/2)).
    /// True while it plays.
    fn do_item_sound(&mut self, slot: usize, k: f32, id: i32, pos: V3, flags: u16) -> R<bool> {
        if id < 0 {
            return Ok(false);
        }
        let mut vol = mul(mul(k, 224.0)?, self.level_f32(0x98))? as i32;
        let sumner = self.globals.cur_world == 0xd && 0.0 < self.tower.sumner_speech_time;
        if sumner || self.globals.trig_cam_active != 0 || self.stop_time != 0 {
            vol = 0x10;
        }
        if matches!(self.globals.game_mode, 0x4014 | 0x400c) || self.globals.end_of_stage != 0 {
            self.events.push(Event::ItemSoundStop { slot });
            return Ok(false);
        }
        vol = vol.max(0);
        if self.globals.cur_world == 0xc && self.globals.cur_level == 0 {
            let v = vol << 2;
            vol = if v < 0x40 { 0x40 } else { v.min(0xff) };
        }
        let priority = if flags & 2 != 0 { 3 } else { 0x70 };
        if self.sound_on() {
            self.events.push(Event::ItemSound { slot, id, volume: vol, position: pos, priority });
        }
        if flags & 1 != 0 {
            let amount = sub(1.0, mul(k, 0.5)?)?;
            self.lower_bg_music(0.5, amount);
        }
        Ok(true)
    }

    /// ProcessItems' find_enemy_slot(0, -1) probe: only sets no_enemy_slot.
    fn find_enemy_slot_probe(&mut self) -> R<()> {
        if self.enemies.iter().any(|r| r.i32_(e::STATE) == 0) {
            return Ok(());
        }
        self.no_enemy_slot = true;
        Ok(())
    }

    fn show_item_now(&self, slot: usize) -> bool {
        let r = &self.items.records[slot];
        let mut need = i32::from(r.i8_(0xe0));
        let s = &self.items.session;
        let mut players = if s.game_mode & 0x8000 != 0 { 2 } else { self.globals.num_virtual_players };
        let exact = 10 < need;
        if exact {
            need -= 10;
        }
        if 0 < self.globals.global_958 {
            players = self.globals.global_958;
        }
        if self.globals.global_958 <= 0 && !exact {
            return need <= players;
        }
        players == need
    }

    /// ProcessItems' first-loop animation/timer state machine (LAB_00155148..).
    fn animate_item_state(&mut self, slot: usize, visible: bool, vb: i16) -> R<()> {
        let r = self.items.records[slot].clone();
        let flags = r.u16_(0xd8);
        let kind = self.items.definitions[r.definition()].kind();
        if flags & 0x40 == 0 && !visible {
            let busy = (flags & 4 != 0 && 0 < r.i8_(0xdc)) || (kind == 2 && 0 < r.i8_(0xdc));
            if !busy {
                return Ok(());
            }
        }
        if 0 < r.i16_(0xda) {
            self.items.records[slot].set_i16(0xda, r.i16_(0xda).wrapping_sub(vb));
        }
        if self.items.records[slot].u16_(0xd8) & 0x100 != 0 && self.items.records[slot].i16_(0xda) < 1 {
            self.delete_item(slot, false);
            return Ok(());
        }
        let r = self.items.records[slot].clone();
        let flags = r.u16_(0xd8);
        let def = self.items.definitions[r.definition()].clone();
        if flags & 1 == 0 {
            if self.has_item_anim(slot) {
                let cur = r.i8_(0xdc);
                let mut target = r.i8_(0xde);
                let mut mode = if cur == target { 0 } else { 2 };
                if kind == 5 {
                    if cur == 0 && target == 2 {
                        target = 1;
                    } else if cur == 2 && target == 0 {
                        target = 3;
                    }
                    mode = 1;
                }
                if self.animate_item(slot, target.max(0) as u16, mode)? & 3 != 0 {
                    let de = self.items.records[slot].u8_(0xde);
                    self.items.records[slot].set_u8(0xdc, de);
                }
            }
        } else if !self.has_item_anim(slot) {
            if r.i16_(0xda) < 1 {
                let mut f = flags;
                if f & 4 != 0 && (r.i8_(0xdc) == 0 || f & 2 == 0) {
                    let open = r.i8_(0xdc) == 0;
                    self.items.records[slot].set_u8(0xdc, u8::from(open));
                    let mut t = if open {
                        i32::from(def.i16_(0x4a)) << 1
                    } else {
                        let mut t = i32::from(def.i16_(0x48)) << 1;
                        if kind == 8 {
                            t = mul(t as f32, self.level_f32(0xd8))? as i32;
                        }
                        t
                    };
                    if t < 0 {
                        let n = t.wrapping_neg();
                        t = (n >> 1) + self.rng.rand_int(n as u32).unwrap_or(0) as i32;
                    }
                    self.items.records[slot].set_i16(0xda, t as i16);
                    f = self.items.records[slot].u16_(0xd8);
                }
                if f & 0x80 != 0 {
                    self.items.records[slot].set_u16(0xd8, f & 0xfffe);
                }
            }
        } else {
            let mut mode = 2;
            if r.i16_(0xda) < 1 {
                let f = r.u16_(0xd8);
                if f & 4 != 0 {
                    let cur = r.i8_(0xde);
                    let next = cur + 1;
                    let count = r.i16_(0x90);
                    let v = if i16::from(next) < count {
                        next
                    } else if f & 2 == 0 {
                        0
                    } else {
                        cur
                    };
                    self.items.records[slot].set_i8(0xde, v);
                }
                let f = self.items.records[slot].u16_(0xd8);
                if f & 0x80 != 0 {
                    self.items.records[slot].set_u16(0xd8, f & 0xfffe);
                }
            }
            let rr = &self.items.records[slot];
            if rr.i8_(0xdc) == rr.i8_(0xde) {
                mode = 0;
            }
            let target = rr.i8_(0xde).max(0) as u16;
            if self.animate_item(slot, target, mode)? & 3 != 0 {
                let rr = self.items.records[slot].clone();
                if rr.u16_(0xd8) & 0x400 == 0 {
                    let mut t = if rr.i8_(0xde) == 0 {
                        let mut t = i32::from(def.i16_(0x48)) << 1;
                        if kind == 8 {
                            t = mul(t as f32, self.level_f32(0xd8))? as i32;
                        }
                        t
                    } else {
                        i32::from(def.i16_(0x4a)) << 1
                    };
                    let mut base = (add(mul(f32::from(rr.i16_(0x94)), 1.0)?, 0.5)? as i32) << 1;
                    if kind == 8 && 4 < rr.i16_(0x90) && t < 0 {
                        t = 0;
                        base = 0;
                    }
                    let v = self.items.get_active_time(t, base)?;
                    self.items.records[slot].set_i16(0xda, v as i16);
                }
                let de = self.items.records[slot].u8_(0xde);
                self.items.records[slot].set_u8(0xdc, de);
            }
        }
        Ok(())
    }

    /// Chest (type 2) case.
    fn process_chest(&mut self, slot: usize) -> R<()> {
        let r = self.items.records[slot].clone();
        if r.i8_(0xdc) == 0 && 0 < r.i16_(0xf2) {
            let t = r.u16_(0xf2).wrapping_sub(self.globals.vb_elapsed as u16) as i16;
            self.items.records[slot].set_i16(0xf2, if t < 1 { 0 } else { t });
            let mut a = [0.0f32, r.f32_(0xf4), 0.0];
            if 0 < t {
                a[0] = if (i32::from(t) + 4) & 8 != 0 { -f32::from_bits(0x3d56_6cf4) } else { f32::from_bits(0x3d56_6cf4) };
                a[0] = super::enemy_ai::wrap(add(a[0], 0.0)?)?;
                let k = f32::from_bits(0x3d8e_fa35);
                a[1] = super::enemy_ai::wrap(add(a[1], if t & 8 != 0 { k } else { -k })?)?;
            }
            let m = super::enemy_ai::create_ypr_matrix(a)?;
            let rr = &mut self.items.records[slot];
            for (row, values) in m.iter().take(3).enumerate() {
                for (col, v) in values.iter().enumerate() {
                    rr.set_f32(0x10 + 16 * row + 4 * col, *v);
                }
            }
        }
        let r = self.items.records[slot].clone();
        if r.i8_(0xdc) == 2 {
            let sub_ = self.items.definitions[r.definition()].subtype();
            if sub_ == 0x2c {
                // The time-bomb chest: StartExplosion(level +0xdc × 50, kind
                // 0x1d), AudioBarrelExplode, DeleteItem, NewHelp 0x89.
                let at = r.v3(0x60); // GetItemPos 0x153988
                let dmg = mul(self.level_f32(0xdc), 50.0)?;
                self.start_explosion(dmg, r.position(), 0x1d)?;
                self.audio_barrel_explode(at)?;
                self.delete_item(slot, false);
                self.help(0x89, usize::MAX);
            } else if r.u32_(0xfc) == 0 && sub_ != 0x2b && sub_ != 0x2f {
                self.delete_item(slot, false);
            }
            for k in 0..self.enemies.len() {
                if self.enemies[k].i32_(0x354) == slot as i32 + 1 {
                    let rec = &mut self.enemies[k];
                    rec.set_u32(0x350, 0);
                    rec.set_i32(0x354, -1);
                    rec.set_u32(0x358, 0x47c3_5000);
                }
            }
        }
        Ok(())
    }

    /// Exit (type 9) case: exit animation states and ExitState.
    fn process_exit(&mut self, slot: usize) -> R<()> {
        let r = self.items.records[slot].clone();
        let sub_ = self.items.definitions[r.definition()].subtype();
        if sub_ == 0x32 {
            // ProcessItems case 9, subtype 0x32: idle until a player touched it.
            let touched = r.u32_(0xf4);
            if touched == 0 {
                return Ok(());
            }
            // A secret exit: once, while no other destination is pending,
            // skip_wave = +0xf0 + 3 (world 12 level +0xf0) and SaveStage
            // with the first touching player as leader; the notices go.
            if self.globals.skip_wave < 3 && r.u16_(0xd8) & 1 == 0 {
                let p = (0..4).find(|&k| touched >> k & 1 != 0).unwrap_or(0);
                self.items.records[slot].or_u16(0xd8, 1);
                self.globals.skip_wave = i32::from(r.u8_(0xf0)) + 3;
                let at = self.players[p].v3(0x50);
                self.save_stage(slot, p, at);
                self.hud.init_got_it();
            }
            return Ok(());
        }
        let mut alive = 0u32;
        let mut exiting = 0;
        for (k, pl) in self.players.iter().enumerate() {
            let st = pl.i32_(0xfc);
            if st == 1 || st == 4 || st == 5 {
                alive |= 1 << k;
            }
            if st == 5 {
                exiting += 1;
            }
        }
        {
            let rr = &mut self.items.records[slot];
            rr.set_u16(0xd8, (rr.u16_(0xd8) & 0xfbff) | 1);
        }
        let vb_done = |g: &Game| g.items.records[slot].i16_(0xda) < 1;
        if exiting == 0 {
            let touched = r.u32_(0xf4);
            if alive == 0 || touched != alive {
                if touched == 0 {
                    if r.i8_(0xdc) < 1 {
                        self.items.records[slot].set_u32(0xf4, 0);
                    } else {
                        if vb_done(self) {
                            let de = self.items.records[slot].i8_(0xde);
                            self.items.records[slot].set_i8(0xde, if de < 4 { de + 1 } else { 0 });
                        } else if r.i8_(0xdc) == 3 {
                            let rr = &mut self.items.records[slot];
                            rr.set_i16(0xda, 0);
                            rr.set_u8(0xde, 4);
                            rr.set_u8(0xdc, 4);
                        }
                        self.items.records[slot].set_u32(0xf4, 0);
                    }
                    let dc = i32::from(self.items.records[slot].i8_(0xdc));
                    self.exit_state = self.exit_state.max(dc);
                    self.items.records[slot].set_u8(0xdd, 0);
                } else {
                    if vb_done(self) {
                        let de = self.items.records[slot].i8_(0xde);
                        if de < 3 {
                            self.items.records[slot].set_i8(0xde, de + 1);
                        } else if 3 < de {
                            self.items.records[slot].set_i8(0xde, 0);
                        }
                    }
                    self.items.records[slot].set_u32(0xf4, 0);
                    let dc = i32::from(self.items.records[slot].i8_(0xdc));
                    self.exit_state = self.exit_state.max(dc);
                    self.items.records[slot].set_u8(0xdd, 2);
                }
            } else {
                if r.i8_(0xde) == 0 {
                    self.items.records[slot].set_i16(0xda, 0);
                }
                if vb_done(self) && self.items.records[slot].i8_(0xde) < 4 {
                    let de = self.items.records[slot].i8_(0xde);
                    self.items.records[slot].set_i8(0xde, de + 1);
                }
                let dc = self.items.records[slot].i8_(0xdc);
                if dc < 4 {
                    self.items.records[slot].set_u32(0xf4, 0);
                }
                if dc == 4 && vb_done(self) {
                    self.exit_state = if self.players.iter().any(|p| p.i32_(0xfc) == 4) { 0xe } else { 0xf };
                } else {
                    self.exit_state = i32::from(dc) + 10;
                }
            }
        } else {
            self.exit_state = 0xf;
        }
        let rr = &mut self.items.records[slot];
        if rr.i8_(0xdc) == 3 {
            if rr.u8_(0xdd) == 2 {
                rr.set_i16(0xda, 0x2d);
            }
            rr.or_u16(0xd8, 0x400);
        }
        let de = rr.i8_(0xde);
        rr.set_i16(0xb8, i16::from(de == 3 || de < 2));
        rr.set_u8(0xdd, rr.u8_(0xdc));
        Ok(())
    }

    /// Type 10 case (G1: subtypes 0x2a..0x2d and 0).
    fn process_type10(&mut self, slot: usize) -> R<()> {
        let r = self.items.records[slot].clone();
        let sub_ = self.items.definitions[r.definition()].subtype();
        // +0xf4/+0xf6 flash timers drive an alternate texture only.
        if 2 <= r.i16_(0xf4) {
            let t = r.i16_(0xf6).wrapping_add(self.globals.vb_elapsed as i16);
            let rr = &mut self.items.records[slot];
            if 0x3d <= t {
                rr.set_i16(0xf6, 0);
                let c = rr.i16_(0xf4) - 1;
                rr.set_i16(0xf4, if c == 1 { 0 } else { c });
            } else {
                rr.set_i16(0xf6, t);
            }
        } else if r.i16_(0xf4) == 1 {
            let rr = &mut self.items.records[slot];
            rr.set_u16(0xf4, 0);
            rr.set_u16(0xf6, 0);
        }
        match sub_ {
            0x2c | 0x2d => {
                if r.i8_(0xdc) == 2 {
                    let mut best = 100_000.0;
                    for pl in &self.players {
                        if pl.i32_(0xfc) == 1 {
                            best = Self::dist3(r.v3(0x60), pl.v3(0x50))?.min(best);
                        }
                    }
                    if best <= 9.0 {
                        self.help(if sub_ == 0x2c { 0x2c } else { 0x2d }, usize::MAX);
                    }
                    self.delete_item(slot, false);
                }
            }
            0x28 | 0x31 | 0x33 | 0x34 | 0x35 => {
                // 0x33: when DoRockFly changed (DoWorldEffects, boss 0x2a)
                // the rock takes off: +0xfc = 20 + Random(10) (1), 30 +
                // Random(15) (2) or 50 + Random(100) with +0xf8/+0x100 = 10 ×
                // the x/z direction from RockFlyCenter (0, 0, 0 in the
                // data) (3); +0xd8 bit 0 set.
                if sub_ == 0x33 && self.do_rock_fly != self.did_rock_fly {
                    let up = match self.do_rock_fly {
                        1 => Some(add(self.random_pub(10.0)?, 20.0)?),
                        2 => Some(add(self.random_pub(15.0)?, 30.0)?),
                        3 => {
                            let p = r.v3(0x40);
                            let mut d = p;
                            crate::item_collision::normal_vector_2d(&mut d)?;
                            let rr = &mut self.items.records[slot];
                            rr.set_f32(0xf8, mul(d[0], 10.0)?);
                            rr.set_f32(0x100, mul(d[2], 10.0)?);
                            Some(add(self.random_pub(100.0)?, 50.0)?)
                        }
                        _ => None,
                    };
                    if let Some(v) = up {
                        self.items.records[slot].set_f32(0xfc, v);
                    }
                    self.items.records[slot].or_u16(0xd8, 1);
                }
                let r = self.items.records[slot].clone();
                if r.u16_(0xd8) & 1 == 0 {
                    return Ok(());
                }
                let rot = &self.items.tables.rand_rot_216;
                let (f29, f27) = (rot[!slot & 7], rot[slot & 7]);
                let m: [f32; 12] = std::array::from_fn(|k| r.f32_(0x10 + 4 * k));
                let mut ypr = extract_ypr(&m)?;
                let spf = self.globals.spf;
                let (k, dv) = match sub_ {
                    0x31 => (f32::from_bits(0x3e32_b8c3), 1.0),
                    0x35 => (f32::from_bits(0x3c8e_fa36), 2.0),
                    _ => (f32::from_bits(0x3eb2_b8c3), 2.0),
                };
                let vy = sub(r.f32_(0xfc), dv)?;
                ypr[0] = add(mul(mul(f27, k)?, spf)?, ypr[0])?;
                ypr[2] = add(mul(mul(f29, k)?, spf)?, ypr[2])?;
                let nm = super::enemy_ai::create_ypr_matrix(ypr)?;
                let rr = &mut self.items.records[slot];
                rr.set_f32(0xfc, vy);
                for (row, v) in nm.iter().take(3).enumerate() {
                    for (col, x) in v.iter().take(3).enumerate() {
                        rr.set_f32(0x10 + 16 * row + 4 * col, *x);
                    }
                }
                let p = [
                    add(mul(rr.f32_(0xf8), spf)?, rr.f32_(0x40))?,
                    add(mul(rr.f32_(0xfc), spf)?, rr.f32_(0x44))?,
                    add(mul(rr.f32_(0x100), spf)?, rr.f32_(0x48))?,
                ];
                for (c, v) in p.iter().enumerate() {
                    rr.set_f32(0x40 + 4 * c, *v);
                }
                crate::items::ItemWorld::update_offsets(rr)?;
                if p[1] < sub(self.globals.world_bottom, 200.0)? {
                    self.delete_item(slot, false);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Trigger (type 5) case: drives bound world objects through byte +0x16.
    fn process_trigger(&mut self, slot: usize) -> R<()> {
        let r = self.items.records[slot].clone();
        let sub_ = self.items.definitions[r.definition()].subtype();
        let flags = r.u16_(0xf4);
        if r.u16_(0xd8) & 0x400 != 0 {
            return Ok(());
        }
        if 0 < r.i16_(0x100) {
            self.items.records[slot].set_i16(0x100, r.i16_(0x100).wrapping_sub(self.globals.vb_elapsed as i16));
        }
        let bound = r.u32_(0xf0).checked_sub(1).map(|o| o as usize);
        let prev_state = r.i8_(0xde);
        if flags & 0x40 != 0 {
            // Crystal doors (+0xf6 < 100): a touch without the realm's crystals
            // shows NeedCrystals; gargoyle doors (+0xf6 − 0x65) NeedGargItems.
            let f6 = i32::from(r.i8_(0xf6));
            let e2 = r.u8_(0xe2);
            if f6 < 100 {
                if e2 != 0 && !self.any_player_has_all_crystals_pub(f6) {
                    self.tower_need_msg(e2, f6.max(0) as usize, false)?;
                    self.items.records[slot].set_u8(0xe2, 0);
                }
            } else {
                let n = f6 - 0x65;
                if e2 != 0 && !self.any_player_has_all_garg_items(n) {
                    if n < 3 {
                        self.tower_need_msg(e2, n.max(0) as usize, true)?;
                    }
                    self.items.records[slot].set_u8(0xe2, 0);
                }
            }
            let e2 = self.items.records[slot].u8_(0xe2);
            let mut s = Some(slot);
            let mut guard = 0;
            while let Some(i) = s {
                self.items.records[i].set_u8(0xe2, e2);
                s = self.items.records[i].u32_(0xf8).checked_sub(1).map(|x| x as usize);
                guard += 1;
                if guard > self.items.records.len() {
                    return unsupported("trigger chain cycle");
                }
            }
        }
        let mut touched = u32::from(self.items.records[slot].u8_(0xe2));
        if touched != 0 && flags & 0x100 != 0 {
            let mut mask = 0xffff_fff0u32;
            for (k, pl) in self.players.iter().enumerate() {
                if pl.i32_(0xfc) != 1 {
                    continue;
                }
                let floor = pl.u32_(0x89c).checked_sub(1).map(|o| o as usize);
                let on_bound = match (floor, bound) {
                    (Some(f), Some(b)) => f == b || self.collision.objects[f].parent == Some(b),
                    _ => false,
                };
                if on_bound {
                    mask |= 1 << k;
                }
            }
            let rr = &mut self.items.records[slot];
            rr.set_u8(0xe2, rr.u8_(0xe2) & mask as u8);
            touched = u32::from(rr.u8_(0xe2));
        }
        let flags = self.items.records[slot].u16_(0xf4);
        let mut state = self.items.records[slot].i8_(0xde);
        if flags & 0x400 != 0 {
            // All active players must stand on it (0x1563f4); the bound
            // object's +0x10 bit 0x04000000 mirrors the result.
            let all = touched == self.active_player_mask;
            if !all {
                self.items.records[slot].set_u8(0xe2, 0);
                touched = 0;
            }
            if let Some(o) = bound {
                let f = &mut self.items.objects[o].flags_10;
                *f = if all { *f | 0x0400_0000 } else { *f & 0xfbff_ffff };
            }
        }
        match bound {
            None => {
                if flags & 4 == 0 {
                    if touched != 0 {
                        state = 2;
                    }
                } else {
                    state = if touched == 0 { 0 } else { 2 };
                }
            }
            Some(o) => {
                let w = &mut self.items.objects[o];
                if flags & 1 != 0 {
                    if touched != 0 {
                        state = 2;
                        if w.byte_16 & 0xf0 == 0x20 {
                            w.byte_16 &= 0xf;
                        }
                    } else {
                        state = if w.byte_16 & 0x20 == 0 { 2 } else { 0 };
                    }
                } else if flags & 2 == 0 {
                    if flags & 4 == 0 {
                        if touched != 0 {
                            w.byte_16 = touched as u8 | 0x20;
                            state = 2;
                        } else {
                            state = if w.byte_16 & 0x20 != 0 { 2 } else { 0 };
                        }
                    } else if touched != 0 {
                        let b = w.byte_16;
                        let mut done = true;
                        if b & 0x10 == 0 {
                            if b & 0xf < (b & 0xf) | touched as u8 {
                                w.byte_16 = (b & 0xf0) | touched as u8;
                            }
                            let expired = self.items.records[slot].i16_(0x100) < 1;
                            if expired {
                                let w = &mut self.items.objects[o];
                                w.byte_16 = ((w.byte_16 ^ 0x20) & 0xf0) | touched as u8;
                            }
                            state = 2;
                            done = expired;
                        }
                        if done {
                            self.items.records[slot].set_i16(0x100, 0x78);
                            state = 2;
                        }
                    } else {
                        state = 0;
                        if flags & 0x800 != 0 {
                            let v = ((self.globals.num_in_game_players - 1) * 0x3c) as i16;
                            self.items.records[slot].set_i16(0x100, v);
                        }
                    }
                } else if touched == 0 {
                    state = if w.byte_16 & 0x20 != 0 { 2 } else { 0 };
                } else {
                    if w.byte_16 & 0xf0 == 0 {
                        w.byte_16 = (w.byte_16 & 0xf) | 0x20;
                    }
                    state = 2;
                }
            }
        }
        self.items.records[slot].set_i8(0xde, state);
        if sub_ == 0x17 && self.globals.game_mode & 0x8000 == 0 {
            self.help(5, usize::MAX);
        }
        if state != 0 && prev_state == 0 {
            if flags & 0x1000 != 0 {
                // ShakeCamera(0.1, 0, 0, 0xb4, 0x64).
                self.shake_camera(f32::from_bits(0x3dcc_cccd), 0, 0, 0xb4, 0x64);
            }
            if flags & 0x2000 != 0 {
                // ActivateNearestEnemy 0x1546f8(trigger +0x40).
                let at = self.items.records[slot].v3(0x40);
                if let (Some(k), d) = self.find_closest_item_kind(10.0, at, 4)?
                    && d < 10.0
                {
                    let v = self.items.records[k].u32_(0xf8) | 1;
                    self.items.records[k].set_u32(0xf8, v);
                }
            }
            if 0 <= r.i16_(0x102) && touched != 0 {
                // TriggerCameraActivate(bound object, transmitter position,
                // angles, its radius byte as the delay, wait 0x1e, hold 0).
                let k = r.i16_(0x102) as usize;
                if let Some(t) = self.std_camera.as_ref().map(|c| c.transmitters.get(k).cloned()) {
                    let t = t.ok_or_else(|| super::Unsupported("trigger camera transmitter index".into()))?;
                    self.trigger_camera_activate(bound, t.pos, [t.pitch, t.yaw], i32::from(t.radius), 0x1e, 0)?;
                }
            }
        }
        self.items.records[slot].set_i16(0xb8, 0);
        if self.globals.trig_cam_active != 0 || self.items.records[slot].u16_(0xf4) & 0xc0 != 0 {
            return Ok(());
        }
        let rr = &mut self.items.records[slot];
        let e2 = rr.u8_(0xe2);
        rr.set_u8(0xe2, if e2 & 0xf == 0 { 0 } else { e2 & 0xf0 });
        Ok(())
    }

    /// OnWobj(object, count enemies): 2 player on it, 1 enemy on it, 0 none.
    fn on_wobj(&self, o: usize, enemies: bool) -> i32 {
        for pl in &self.players {
            let st = pl.i32_(0xfc);
            if (st == 1 || st == 8) && pl.u32_(0x89c) == o as u32 + 1 {
                return 2;
            }
        }
        if enemies && self.enemies.iter().any(|e| e.i32_(e::STATE) == 1 && e.u32_(0x2ac) == o as u32 + 1) {
            return 1;
        }
        0
    }

    /// ProcessItemWobjs: drive trigger-bound objects (elevators by y offset,
    /// matrix objects through their world-animation direction bits).
    pub fn process_item_wobjs(&mut self) -> R<()> {
        for k in 0..self.items.wobjs.len() {
            let o = self.items.wobjs[k].object;
            let mut b16 = self.items.objects[o].byte_16;
            let on = self.on_wobj(o, true);
            let tf = self.items.objects[o].word_14 >> 8;
            // Bridge/trap/elevator audio cues are presentation; +0x17 mirrors +0x16.
            self.items.objects[o].byte_17 = b16;
            self.items.object_exclusion[o] = 0;
            let flags = self.items.objects[o].flags_10;
            let moving;
            if flags & 0x800 != 0 {
                moving = false; // world particle systems only
            } else if tf & 0x10 != 0 {
                // Fade objects: visibility byte +0x35 and alpha (presentation).
                let shown = if tf & 0x20 == 0 { b16 & 0xf == 0 } else { b16 & 0x2f != 0 };
                self.items.object_visible[o] = shown;
                self.items.object_exclusion[o] = if shown { -1 } else { 0 };
                moving = false;
            } else if flags & 0x200_0000 == 0 {
                if tf & 8 != 0 || on < 2 {
                    let w = &self.items.wobjs[k];
                    let target = if b16 & 0x20 == 0 { w.starty } else { w.endy };
                    let mut d = sub(target, w.y)?;
                    let step = mul(self.globals.spf, 4.0)?;
                    let mut m = true;
                    if 0.001 < d {
                        if step < d {
                            d = step;
                        }
                    } else if d < -0.001 {
                        if d < -step {
                            d = -step;
                        }
                    } else if tf & 0x20 == 0 {
                        m = false;
                    } else {
                        b16 ^= 0x20;
                    }
                    if m {
                        let w = &mut self.items.wobjs[k];
                        w.y = add(d, w.y)?;
                        self.items.objects[o].flags_10 |= 0x800_0000;
                    } else {
                        self.items.objects[o].flags_10 &= 0xf7ff_ffff;
                    }
                    let w = &self.items.wobjs[k];
                    self.items.object_node_y[o] = add(w.inity, w.y)?;
                    moving = m;
                } else {
                    let w = &self.items.wobjs[k];
                    self.items.object_node_y[o] = add(w.inity, w.y)?;
                    continue;
                }
            } else if tf & 8 != 0 || on < 2 {
                let f = &mut self.items.objects[o].flags_10;
                let mut m = true;
                if tf & 0x20 == 0 {
                    *f = if b16 & 0x20 != 0 { (*f | 0x20_0000) & 0xffef_ffff } else { (*f | 0x10_0000) & 0xffdf_ffff };
                    if *f & 0x50_0000 == 0x50_0000 || *f & 0xa0_0000 == 0xa0_0000 {
                        m = false;
                    }
                } else if b16 & 0xf == 0 {
                    if *f & 0x80_0000 != 0 {
                        *f |= 0x30_0000;
                        b16 &= 0xcf;
                        m = false;
                    }
                } else {
                    *f &= 0xffcf_ffff;
                    b16 |= 0x30;
                }
                moving = m;
            } else {
                let w = &mut self.items.objects[o];
                w.flags_10 |= 0x30_0000;
                w.byte_16 &= 0xef;
                continue;
            }
            if moving && tf & 8 == 0 {
                self.items.object_exclusion[o] = 1;
            }
            let f = &mut self.items.objects[o];
            if moving {
                if b16 & 0x10 == 0 {
                    b16 |= 0x10;
                }
                f.flags_10 |= 0x2000_0000;
            } else {
                b16 &= 0xef;
                f.flags_10 &= 0xdfff_ffff;
            }
            if tf & 0x47 == 0 && (on == 0 || tf & 0x10 == 0) {
                b16 = 0;
            }
            f.byte_16 = b16;
        }
        Ok(())
    }

}

/// ExtractYPR 0x1d6950: (pitch, yaw, roll) from matrix rows (12 floats).
pub fn extract_ypr(m: &[f32; 12]) -> R<[f32; 3]> {
    use crate::libm::{atan2f, cosf};
    if sub(1.0, m[9].abs())?.abs() < f32::from_bits(0x38d1_b717) {
        let yaw = atan2f(-m[2], m[0])?;
        let pitch = if 0.0 < m[9] { f32::from_bits(0x3fc9_0fdb) } else { f32::from_bits(0xbfc9_0fdb) };
        return Ok([pitch, yaw, 0.0]);
    }
    let roll = atan2f(-m[1], m[5])?;
    let c = cosf(roll)?;
    let (pitch, yaw) = if c == 0.0 {
        if 0.0 < roll {
            (atan2f(m[9], -m[1])?, atan2f(m[6], -m[4])?)
        } else {
            (atan2f(m[9], m[1])?, atan2f(-m[6], m[4])?)
        }
    } else {
        let k = crate::numeric::div_rn(m[5], c)?;
        (atan2f(m[9], k)?, atan2f(crate::numeric::div_rn(m[8], k)?, crate::numeric::div_rn(m[10], k)?)?)
    };
    Ok([pitch, yaw, roll])
}
