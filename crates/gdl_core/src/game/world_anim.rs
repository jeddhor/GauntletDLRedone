//! World animations: DoWorldAnimation 0x164808 / DoWorldAnimSub 0x11ce80
//! over the WORLDS.PS2 animation table (InitWorldInfo 0x11be78), plus
//! CreateRYPMatrix 0x1d6530 and CreatePYRMatrix 0x1d63f8.
//!
//! Each entry drives one world object: its object flags choose play
//! (0x200000), reverse (0x100000), stop (both) or loop (neither); the key
//! channels at the entry frame give rotation (RYP, or PYR with transform
//! flag 0x8000), a translation added to the object's +0x1c position and a
//! scale; the frame advances by SPF × 30 and the object flags record
//! animating (0x8000000), at end (0x800000) and at start (0x400000).
//! Channel sampling is the existing GetAnimAngXYZVal port
//! (crate::animation::sample_channels); InitAnimData's incremental cache is
//! not needed for a fresh evaluation.
//!
//! Presentation (gdl_play): DoTexMods (texture animation) and the
//! MBTreeClearFlags show call.
use super::enemy_ai::{ffcos, ffsin};
use super::{Game, Unsupported, R};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub};
use gdl_formats::animation_keys::KeyTrack;
use gdl_formats::world::WorldAnims;

type M4 = [[f32; 4]; 4];

/// Run-time state of one world animation entry (+6 flags, +8 frame).
#[derive(Clone, Debug)]
pub struct WorldAnimState {
    pub object: usize,
    pub frames: i16,
    pub flags: u16,
    pub frame: f32,
    pub transform_flags: u16,
    pub channels: u16,
    pub key_start: usize,
    /// None for entries whose data offset is not positive (Errorf, skipped).
    pub active: bool,
}

/// The level's world animations with the WORLDS bytes they decode from.
#[derive(Clone, Debug, Default)]
pub struct WorldAnimSet {
    pub bytes: std::sync::Arc<Vec<u8>>,
    pub tables: [usize; 3],
    pub entries: Vec<WorldAnimState>,
}

impl WorldAnimSet {
    /// InitWorldInfo: entries with data get flags 0x101 (their objects get
    /// flag 0x2000000, applied by the caller).
    pub fn new(bytes: std::sync::Arc<Vec<u8>>, anims: &WorldAnims) -> Self {
        let entries = anims
            .entries
            .iter()
            .map(|e| WorldAnimState {
                object: e.object.max(0) as usize,
                frames: e.frames,
                flags: 0x101,
                frame: 0.0,
                transform_flags: e.transform_flags,
                channels: e.channels,
                key_start: anims.base as usize + e.key_offset as usize,
                active: 0 < e.data as i32 && 0 <= e.object,
            })
            .collect();
        Self { bytes, tables: [anims.angles as usize, anims.positions as usize, anims.scales as usize], entries }
    }
}

impl WorldAnimSet {
    /// The objects of the entries with data (+0xc set), in entry order.
    pub fn anim_objects(&self) -> Vec<usize> {
        self.entries.iter().filter(|e| e.active).map(|e| e.object).collect()
    }
}

/// CreateRYPMatrix: rows 0..2 (translation and w untouched by the caller).
pub fn create_ryp_matrix(a: [f32; 3]) -> R<[[f32; 3]; 3]> {
    let c0 = ffcos(a[0])?;
    let s0 = -ffsin(a[0])?;
    let c1 = ffcos(a[1])?;
    let s1 = -ffsin(a[1])?;
    let c2 = ffcos(a[2])?;
    let s2 = -ffsin(a[2])?;
    let f4 = mul(-c2, s1)?;
    let f3 = -s2;
    let m10 = add(mul(f3, c0)?, mul(f4, s0)?)?;
    let m20 = add(mul(s2, s0)?, mul(f4, c0)?)?;
    let g4 = mul(f3, s1)?;
    let m11 = add(mul(c2, c0)?, mul(g4, s0)?)?;
    let m21 = add(mul(-c2, s0)?, mul(g4, c0)?)?;
    Ok([[mul(c2, c1)?, mul(s2, c1)?, s1], [m10, m11, mul(c1, s0)?], [m20, m21, mul(c1, c0)?]])
}

/// CreatePYRMatrix: rows 0..2.
pub fn create_pyr_matrix(a: [f32; 3]) -> R<[[f32; 3]; 3]> {
    let c0 = ffcos(a[0])?;
    let s0 = -ffsin(a[0])?;
    let c1 = ffcos(a[1])?;
    let s1 = -ffsin(a[1])?;
    let c2 = ffcos(a[2])?;
    let s2 = -ffsin(a[2])?;
    let f3 = mul(s0, s1)?;
    let m01 = add(mul(-f3, c2)?, mul(c0, s2)?)?;
    let m11 = add(mul(f3, s2)?, mul(c0, c2)?)?;
    let g3 = mul(c0, s1)?;
    let m02 = add(mul(g3, c2)?, mul(s0, s2)?)?;
    let m12 = add(mul(g3, -s2)?, mul(s0, c2)?)?;
    Ok([[mul(c1, c2)?, m01, m02], [mul(-c1, s2)?, m11, m12], [-s1, mul(-s0, c1)?, mul(c0, c1)?]])
}

/// ExtractPYR 0x1d6790: the [pitch, yaw, roll] CreatePYRMatrix rebuilds,
/// from a matrix's rows (16 floats; m[k] = float +4k).
pub fn extract_pyr(m: &[f32; 16]) -> R<[f32; 3]> {
    let atan2 = |y: f32, x: f32| crate::libm::atan2f(y, x).map_err(|_| Unsupported("atan2f".into()));
    if sub(1.0, m[2].abs())?.abs() < f32::from_bits(0x38d1_b717) {
        let a = atan2(m[9], m[5])?;
        let y = if 0.0 < m[2] { f32::from_bits(0xbfc9_0fdb) } else { f32::from_bits(0x3fc9_0fdb) };
        return Ok([a, y, 0.0]);
    }
    let p = atan2(-m[6], m[10])?;
    let c = crate::libm::cosf(p).map_err(|_| Unsupported("cosf".into()))?;
    if c == 0.0 {
        return Ok(if 0.0 < p { [p, atan2(-m[2], -m[6])?, atan2(-m[8], -m[9])?] } else { [p, atan2(-m[2], m[6])?, atan2(m[8], m[9])?] });
    }
    // div.S (EE FPU, 0x1d68c8/0x1d68f0/0x1d68fc): a zero divisor (m[10] = 0
    // makes k zero) gives ±FLT_MAX (div_rn_ee; D5's look-at nodes reach it).
    let div = |a: f32, b: f32| crate::numeric::div_rn_ee(a, b).map_err(|_| Unsupported("ExtractPYR div".into()));
    let k = div(m[10], c)?;
    let y = atan2(-m[2], k)?;
    let r = atan2(div(-m[1], k)?, div(m[0], k)?)?;
    Ok([p, y, r])
}

/// DoWorldAnimSub's matrix part for one entry at `frame`, starting from the
/// object's rest local matrix (identity rows, `position` = its +0x1c).
pub fn eval_local(set: &WorldAnimSet, e: &WorldAnimState, frame: f32, position: [f32; 3]) -> R<M4> {
    let tf = e.transform_flags;
    if tf & 0xfff == 0 {
        return Ok([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
    }
    let track = KeyTrack::from_parts(&set.bytes, e.key_start, tf, e.channels, e.frames.max(0) as u16, set.tables)
        .map_err(|err| Unsupported(format!("world animation keys: {err}")))?;
    let ch = crate::animation::sample_channels_long_gaps(&track, frame.max(0.0)).map_err(|err| Unsupported(format!("world animation sample: {err:?}")))?;
    let mut m = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [position[0], position[1], position[2], 1.0]];
    if tf & 7 != 0 {
        let rows = if tf & 0x8000 == 0 { create_ryp_matrix([ch[0], ch[1], ch[2]])? } else { create_pyr_matrix([ch[0], ch[1], ch[2]])? };
        for r in 0..3 {
            m[r] = [rows[r][0], rows[r][1], rows[r][2], 0.0];
        }
    }
    if tf & 0x70 != 0 {
        m[3] = [add(position[0], ch[3])?, add(position[1], ch[4])?, add(position[2], ch[5])?, 1.0];
    }
    Ok(m)
}

/// DoWorldAnimSub 0x11ce80 for entry `k` on the animation set, the object
/// words and the collision world's node matrices (shared by world_update and
/// GauntWorldInit's world_update before AddItemInstList). A looping object
/// with flags & 0x100f0000 == 0x50000 that wraps is returned in `explode`.
pub fn world_anim_sub(
    anims: &mut WorldAnimSet,
    k: usize,
    objects: &mut [crate::items::ObjectWords],
    collision: &mut crate::collision::CollisionWorld,
    spf: f32,
    stop_time: bool,
    explode: &mut Option<usize>,
) -> R<()> {
    let e = anims.entries[k].clone();
    let o = e.object;
    if objects.len() <= o {
        return Ok(());
    }
    let oflags = objects[o].flags_10;
    let mut flags = if oflags & 0x30_0000 == 0x30_0000 {
        e.flags & 0xfffe
    } else if oflags & 0x10_0000 == 0 {
        if oflags & 0x20_0000 == 0 {
            (e.flags & 0xfffd) | 0x101
        } else {
            (e.flags & 0xfcfd) | 1
        }
    } else {
        (e.flags & 0xfcff) | 3
    };
    anims.entries[k].flags = flags;
    if flags & 1 == 0 {
        objects[o].flags_10 &= 0xf7ff_ffff;
        return Ok(());
    }
    let tf = e.transform_flags;
    let local: Option<M4>;
    if tf & 0xfff == 0 {
        // CopyMat4(identity): the node matrix loses its translation too.
        local = Some([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]);
    } else {
        let bytes = anims.bytes.clone();
        let track = KeyTrack::from_parts(&bytes, e.key_start, tf, e.channels, e.frames.max(0) as u16, anims.tables)
            .map_err(|err| Unsupported(format!("world animation keys: {err}")))?;
        let ch = crate::animation::sample_channels_long_gaps(&track, e.frame.max(0.0)).map_err(|err| Unsupported(format!("world animation sample: {err:?}")))?;
        let p0 = collision.objects[o].position;
        let mut m = collision.node_local[o].unwrap_or([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [p0[0], p0[1], p0[2], 1.0]]);
        if tf & 7 != 0 {
            let rows = if tf & 0x8000 == 0 { create_ryp_matrix([ch[0], ch[1], ch[2]])? } else { create_pyr_matrix([ch[0], ch[1], ch[2]])? };
            for r in 0..3 {
                m[r] = [rows[r][0], rows[r][1], rows[r][2], 0.0];
            }
        }
        if tf & 0x70 != 0 {
            let base = collision.objects[o].position;
            m[3] = [add(base[0], ch[3])?, add(base[1], ch[4])?, add(base[2], ch[5])?, 1.0];
        }
        if tf & 0x700 != 0 {
            collision.node_scale[o] = Some([ch[6], ch[7], ch[8]]);
        }
        local = Some(m);
    }
    if let Some(m) = local {
        collision.node_local[o] = Some(m);
    }
    if stop_time && flags & 0x100 != 0 {
        return Ok(());
    }
    if spf <= 0.0 {
        return Ok(());
    }
    let ob = &mut objects[o];
    ob.flags_10 = (ob.flags_10 & 0xff3f_ffff) | 0x800_0000;
    let step = mul(spf, 30.0)?;
    let last = i32::from(e.frames) - 1;
    let mut frame = e.frame;
    let mark;
    if flags & 2 == 0 {
        frame = add(step, frame)?;
        if (frame as i32 as i16 as i32) < last {
            if (frame as i32 as i16) <= 1 && objects[o].flags_10 & 0x1000_0000 != 0 {
                objects[o].flags_10 &= 0xefff_ffff;
            }
            anims.entries[k].frame = frame;
            return Ok(());
        }
        if flags & 0x200 == 0 {
            if flags & 0x100 == 0 {
                frame = last as f32;
                objects[o].flags_10 &= 0xf7ff_ffff;
            } else {
                frame = 0.0;
                if objects[o].flags_10 & 0x100f_0000 == 0x5_0000 {
                    *explode = Some(o);
                }
            }
        } else {
            frame = last as f32;
            flags ^= 2;
        }
        mark = 0x80_0000;
    } else {
        frame = sub(frame, step)?;
        if 0.0 <= frame {
            if last - 1 <= (frame as i32 as i16 as i32) && objects[o].flags_10 & 0x1000_0000 != 0 {
                objects[o].flags_10 &= 0xefff_ffff;
            }
            anims.entries[k].frame = frame;
            return Ok(());
        }
        if flags & 0x200 == 0 {
            if flags & 0x100 == 0 {
                frame = 0.0;
                objects[o].flags_10 &= 0xf7ff_ffff;
            } else {
                frame = last as f32;
                if objects[o].flags_10 & 0x100f_0000 == 0x5_0000 {
                    *explode = Some(o);
                }
            }
        } else {
            frame = 0.0;
            flags ^= 2;
        }
        mark = 0x40_0000;
    }
    objects[o].flags_10 |= mark;
    let st = &mut anims.entries[k];
    st.frame = frame;
    st.flags = flags;
    Ok(())
}


impl Game {
    /// DoWorldAnimation (world_update's first step): every active entry
    /// when players are in game and no trigger camera holds the world.
    pub fn do_world_animation(&mut self) -> R<()> {
        if self.world_anims.entries.is_empty() {
            return Ok(());
        }
        let in_game = 0 < self.globals.num_in_game_players || self.globals.game_mode & 0x8000 != 0;
        // trig_cam_delay − 0xb above 0x18694 (unsigned): delays 0..=10 and
        // ≥ 0x1869f let the world animate.
        if !in_game || (self.globals.trig_cam_delay.wrapping_sub(0xb) as u32) <= 0x18694 {
            return Ok(());
        }
        for k in 0..self.world_anims.entries.len() {
            if self.world_anims.entries[k].active {
                self.do_world_anim_sub(k)?;
            }
        }
        Ok(())
    }

    fn do_world_anim_sub(&mut self, k: usize) -> R<()> {
        let mut explode = None;
        world_anim_sub(&mut self.world_anims, k, &mut self.items.objects, &mut self.collision, self.globals.spf, self.stop_time != 0, &mut explode)?;
        if let Some(o) = explode {
            // GetWorldMat(node) → WorldObjectExplode at its translation.
            let at = self.collision.object_matrix(o).map_err(|e| Unsupported(format!("{e:?}")))?[3];
            self.world_object_explode(o, [at[0], at[1], at[2]])?;
        }
        Ok(())
    }

    /// FindWobjWanim(object): the world animation entry driving `object`.
    pub fn find_wobj_wanim(&self, object: usize) -> Option<usize> {
        self.world_anims.entries.iter().position(|e| e.active && e.object == object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_angles_give_the_polynomial_identity() {
        // ffcos(0) is 1.0000074 (the captured player matrix scale has the
        // same value); off-diagonal terms are signed zeros.
        let c = ffcos(0.0).unwrap();
        for m in [create_ryp_matrix([0.0; 3]).unwrap(), create_pyr_matrix([0.0; 3]).unwrap()] {
            for r in 0..3 {
                for k in 0..3 {
                    let want = if r == k { if r == 0 || (r == 1 && m[1][1] == c * c) { c * c } else { m[r][k] } } else { 0.0 };
                    assert!((m[r][k] - want).abs() < 1e-5, "{m:?}");
                }
            }
        }
    }
}
