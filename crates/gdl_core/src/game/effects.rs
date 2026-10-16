//! Effects 0x240840 (64 × 0xf0, NumEffects 0x244440) for player missiles:
//! FindEffectIdx 0x17ad10, DeleteEffect 0x17a740, ZeroEffect 0x17a680,
//! StartFXTree 0x17d7b0, ChangeEffect 0x17aaf0, SfxSetPhysics 0x17e000,
//! SfxSetHit 0x17df50, SfxSetDamage 0x17dfb0, StartMissile 0x18f680,
//! PlayerStartMissile 0x18f9a8, CalcTargetDir 0x18f5a8, GetAttackDir
//! 0x147d30, PlayerGetTarget 0x146990, closest_enemy 0x144270,
//! closest_generator 0x15ba48, smallsqrt 0x1d6bd0 and the missile subset of
//! ProcessEffects 0x17b248 with MissileCollideEnemy 0x190960,
//! WeaponWallCollide 0x11f270, WeaponItemCollide 0x158710,
//! WeaponCheckItemCollision 0x1585c0, SfxSkipItem 0x17b100, DamageItem
//! 0x15a620, PlayerDamagedEnemy 0x190f30 and PlayerDamagedItem 0x190dc0.
//!
//! Slots keep the original 0xf0 layout. The render node (+0x14) and
//! animation tree (+0x18) become [`EffectNode`]; lights, streaks, alpha,
//! z-mod, audio and particle calls are presentation and become events or are
//! omitted. Paths outside the player-missile subset return [`Unsupported`].
use super::enemies::e;
use super::record::Record;
use super::{unsupported, Event, Game, Unsupported, R};
use crate::collision::{normal_vector, Query};
use crate::item_collision::{item_coll_sub, normal_vector_2d};
use crate::libm::{atan2f, cosf, sinf};
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sub_guarded_rz as sub};
use crate::vu0::fqdist;
use gdl_formats::animation::Animations;
use gdl_formats::elf::Elf;
use serde::Serialize;

type V3 = [f32; 3];
/// WeaponWallCollide result: hit position and (object, water hit).
type WallHit = (Option<V3>, Option<(usize, bool)>);
pub const EFFECT: usize = 0xf0;
pub const MAX_EFFECTS: usize = 64;
/// EffectHeaders has 0xda entries (StartFXSub's bound).
pub const MAX_HEADERS: usize = 0xda;

/// Effect slot offsets.
pub mod fx {
    pub const KIND: usize = 0x60;
    pub const FLAGS: usize = 0x64;
    pub const END: usize = 0x68;
    pub const DURATION: usize = 0x6c;
    pub const LINGER: usize = 0x70;
    pub const NEXT_TIME: usize = 0x74;
    pub const HOLD: usize = 0x78;
    pub const FADE: usize = 0x7c;
    pub const VELOCITY: usize = 0x80;
    pub const SPIN: usize = 0x8c;
    pub const RADIUS: usize = 0x98;
    pub const CONE: usize = 0x9c;
    pub const GRAVITY: usize = 0xa0;
    pub const DRAG_X: usize = 0xa4;
    pub const DRAG_Z: usize = 0xa8;
    pub const DAMAGE: usize = 0xac;
    pub const HIT_SCALE: usize = 0xb0;
    pub const DAMAGE_FLAGS: usize = 0xb4;
    pub const BLAST: usize = 0xb8;
    pub const OWNER: usize = 0xbc;
    pub const HIT_FX: usize = 0xbe;
    pub const NEXT_FX: usize = 0xc0;
    pub const NEXT_FX2: usize = 0xc2;
    pub const HITS: usize = 0xc4;
    pub const FADE_IN: usize = 0xc8;
    pub const HIT_SOUND: usize = 0xcc;
    pub const WALL_SOUND: usize = 0xd0;
    pub const LINK: usize = 0xe8;
    pub const SFX_ID: usize = 0xea;
}

/// One EffectHeaders entry (InitEffect: AtreeMatch of EffectInfo's name in
/// weapon_anim, else powerup_anim) with its first sequence's timing.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EffectHeader {
    /// 0 = WEAPONS/ANIM.PS2 (weapon_anim), 1 = POWERUPS/ANIM.PS2 (powerup_anim).
    pub source: u8,
    pub tree: usize,
    pub frames: i16,
    pub timing: i16,
}

/// Native render node + animation tree of a slot (presentation reads it).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EffectNode {
    /// Rows of the node's rotation; position is the translation row.
    pub matrix: [V3; 3],
    pub position: V3,
    pub scale: f32,
    pub visual: Visual,
    /// ATime when the current tree started (presentation clock).
    pub start: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Visual {
    /// PlayerMissileTreeInfo[player] (the class missile tree).
    Missile { player: usize },
    /// EffectHeaders[index].
    Header(usize),
}

/// Static ELF tables read by the missile code.
#[derive(Debug, Clone)]
pub struct EffectTables {
    /// PlayerMissileInfo 0x24dcc0: 8 classes × 12 words.
    pub missile_info: Vec<[u32; 12]>,
    /// WallHitEffects 0x2406f8.
    pub wall_hit: Vec<i32>,
    /// The three-missile spread at 0x2ad9d0 (cos then sin, 5 each + pad).
    pub spread: [f32; 12],
    /// exp_damaged_by_weapon / killed_by_weapon / damaged_by_attack / killed_by_attack.
    pub exp: [Vec<i32>; 4],
    pub headers: Vec<Option<EffectHeader>>,
}

impl EffectTables {
    pub fn from_elf(elf: &Elf, weapon_anim: &Animations, powerup_anim: &Animations) -> Result<Self, Box<dyn std::error::Error>> {
        let words = |b: &[u8]| -> Vec<u32> { b.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect() };
        let info = words(elf.object("PlayerMissileInfo")?);
        if info.len() != 96 {
            return Err("PlayerMissileInfo size".into());
        }
        let missile_info = info.chunks_exact(12).map(|c| c.try_into().unwrap()).collect();
        let wall_hit = words(elf.object("WallHitEffects")?).into_iter().map(|v| v as i32).collect();
        let s = words(elf.read(0x2ad9d0, 0x30)?);
        let spread = std::array::from_fn(|k| f32::from_bits(s[k]));
        let int = |n: &str| -> Result<Vec<i32>, Box<dyn std::error::Error>> { Ok(words(elf.object(n)?).into_iter().map(|v| v as i32).collect()) };
        let exp = [
            int("exp_damaged_by_weapon")?,
            int("exp_killed_by_weapon")?,
            int("exp_damaged_by_attack")?,
            int("exp_killed_by_attack")?,
        ];
        // EffectInfo 0x23fa78: 0x50 × {name[0x20], zmod, alpha}.
        let table = elf.object("EffectInfo")?;
        let mut headers = vec![None; MAX_HEADERS];
        for (k, h) in headers.iter_mut().enumerate().take(0x50) {
            let raw = &table[0x28 * k..0x28 * k + 0x20];
            let name = &raw[..raw.iter().position(|&b| b == 0).unwrap_or(raw.len())];
            if name.is_empty() {
                continue;
            }
            let find = |a: &Animations, source: u8| {
                a.trees.iter().position(|t| t.label.as_bytes().eq_ignore_ascii_case(name)).map(|i| {
                    let s = a.trees[i].sequences.first();
                    EffectHeader { source, tree: i, frames: s.map_or(0, |s| s.frames), timing: s.map_or(0, |s| s.timing_factor) }
                })
            };
            *h = find(weapon_anim, 0).or_else(|| find(powerup_anim, 1));
        }
        Ok(Self { missile_info, wall_hit, spread, exp, headers })
    }
}

/// Native effect state carried by [`Game`].
#[derive(Debug, Clone)]
pub struct Effects {
    pub slots: Vec<Record>,
    pub nodes: Vec<Option<EffectNode>>,
    pub num: usize,
    /// SfxId 0x2b... (InitEffects sets 0x100).
    pub sfx_id: i32,
    pub tables: EffectTables,
    /// pmissile_sfxidx: the slots of the last PlayerStartMissile.
    pub pmissile: [i32; 5],
}

impl Effects {
    pub fn new(tables: EffectTables) -> Self {
        // InitEffects: nodes/trees cleared; slot words otherwise as loaded (zero).
        Self {
            slots: (0..MAX_EFFECTS).map(|_| Record::zeroed(EFFECT)).collect(),
            nodes: vec![None; MAX_EFFECTS],
            num: 0,
            sfx_id: 0x100,
            tables,
            pmissile: [-1; 5],
        }
    }
}

fn v_sub(a: V3, b: V3) -> R<V3> {
    Ok([sub(a[0], b[0])?, sub(a[1], b[1])?, sub(a[2], b[2])?])
}
fn dot3(a: V3, b: V3) -> R<f32> {
    Ok(add(add(mul(a[0], b[0])?, mul(a[1], b[1])?)?, mul(a[2], b[2])?)?)
}
fn dot_xz(a: V3, b: V3) -> R<f32> {
    Ok(add(mul(a[0], b[0])?, mul(a[2], b[2])?)?)
}

/// smallsqrt 0x1d6bd0: piecewise-linear square root.
pub fn smallsqrt(x: f32) -> R<f32> {
    if x <= f32::from_bits(0x38d1_b717) {
        return Ok(0.0);
    }
    if 0.5 < x {
        return Ok(add(mul(sub(x, 0.5)?, f32::from_bits(0x3f15_f6fd))?, f32::from_bits(0x3f35_0481))?);
    }
    if 0.25 < x {
        return Ok(add(mul(x, f32::from_bits(0x3f54_1206))?, f32::from_bits(0x3e95_f6fd))?);
    }
    if 0.125 < x {
        return Ok(add(mul(x, f32::from_bits(0x3f95_f6fd))?, f32::from_bits(0x3e54_1206))?);
    }
    Ok(mul(x, f32::from_bits(0x4035_0481))?)
}

/// CalcTargetDir(speed, 1/speed, gravity, lift, dir) in place.
pub fn calc_target_dir(speed: f32, inv: f32, gravity: f32, lift: f32, d: &mut V3) -> R<()> {
    let (dist, _) = fqdist(d[0], d[2])?;
    let k = if f32::from_bits(0x3a83_126f) < dist { div(1.0, dist)? } else { 1.0 };
    d[0] = mul(k, d[0])?;
    d[2] = mul(k, d[2])?;
    let f2 = mul(dist, inv)?;
    let f0 = mul(speed, k)?;
    let f1 = mul(add(lift, d[1])?, f0)?;
    let g = mul(mul(gravity, 0.5)?, f2)?;
    d[1] = mul(add(f1, g)?, inv)?;
    Ok(())
}

/// Native matrix helpers for the node orientation (presentation only; the
/// original YawMat3/CreateDirMatrix arithmetic is not reproduced bit for bit).
fn dir_matrix(v: V3) -> [V3; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if n * n < 0.001 {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    let f = [v[0] / n, v[1] / n, v[2] / n];
    let up = [0.0, 1.0, 0.0];
    let mut r = [up[1] * f[2] - up[2] * f[1], up[2] * f[0] - up[0] * f[2], up[0] * f[1] - up[1] * f[0]];
    let rl = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt().max(1e-6);
    r = [r[0] / rl, r[1] / rl, r[2] / rl];
    let u = [f[1] * r[2] - f[2] * r[1], f[2] * r[0] - f[0] * r[2], f[0] * r[1] - f[1] * r[0]];
    [r, u, f]
}

impl Game {
    fn fx(&self, i: usize) -> &Record {
        &self.effects.slots[i]
    }
    fn fx_mut(&mut self, i: usize) -> &mut Record {
        &mut self.effects.slots[i]
    }

    /// ZeroEffect(slot).
    fn zero_effect(&mut self, i: usize) {
        let dir = self.fx(i).u32_(fx::FLAGS) & 0x2_0000 != 0;
        if let Some(n) = self.effects.nodes[i].as_mut()
            && !dir
        {
            n.matrix = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        }
        let r = self.fx_mut(i);
        for o in [0x80, 0x84, 0x88, 0x8c, 0x90, 0x94] {
            r.set_f32(o, 0.0);
        }
        r.set_f32(fx::RADIUS, 1.0);
        for o in [0xa0, 0xa4, 0xa8] {
            r.set_f32(o, 0.0);
        }
        r.set_i16(fx::HIT_FX, 0);
        r.set_i16(fx::NEXT_FX, 0);
        r.set_i16(fx::NEXT_FX2, 0);
        r.set_u32(fx::HIT_SOUND, 0);
        r.set_u32(fx::WALL_SOUND, 0);
        r.set_f32(fx::FADE, 0.0);
        r.set_u32(0xd4, 0); // streak poly instance (presentation)
        r.set_f32(fx::LINGER, 0.0);
        r.set_i16(fx::LINK, -1);
    }

    /// DeleteEffect(slot, force).
    pub fn delete_effect(&mut self, i: usize, force: bool) {
        if MAX_EFFECTS <= i {
            return;
        }
        let r = self.fx(i);
        if !force && 0.0 < r.f32_(fx::LINGER) && self.globals.atime < r.f32_(fx::LINGER) {
            return;
        }
        let (hits, kind, owner) = (r.i16_(fx::HITS), r.u32_(fx::KIND), r.u16_(fx::OWNER));
        if hits == 0 && (0x23..0x27).contains(&kind) && owner.wrapping_sub(1) < 4 {
            self.help(0x12, usize::from(owner - 1));
        }
        if 0.0 < self.fx(i).f32_(fx::END) {
            let link = self.fx(i).i16_(fx::LINK);
            if 0 <= link {
                self.delete_effect(link as usize, true);
                self.fx_mut(i).set_i16(fx::LINK, -1);
            }
        }
        self.effects.nodes[i] = None;
        let sfx = self.effects.sfx_id as i16;
        let r = self.fx_mut(i);
        r.set_u32(fx::KIND, 0);
        r.set_i16(fx::HITS, 0);
        r.set_u32(0x14, 0);
        r.set_i16(fx::LINK, -1);
        for o in [fx::END, fx::FADE_IN, fx::NEXT_TIME, fx::HOLD, fx::DURATION, fx::DAMAGE, fx::BLAST] {
            r.set_f32(o, 0.0);
        }
        r.set_u32(fx::FLAGS, 0);
        r.set_u32(fx::DAMAGE_FLAGS, 0);
        r.set_f32(fx::HIT_SCALE, 1.0);
        r.set_f32(fx::CONE, -1.0);
        r.set_i16(fx::OWNER, 0);
        for o in [0xe0, 0xe4, 0xec] {
            r.set_u32(o, 0);
        }
        r.set_i16(fx::SFX_ID, sfx);
        self.effects.sfx_id += 1;
        if 0x7f00 <= self.effects.sfx_id {
            self.effects.sfx_id = 0x100;
        }
        self.fx_mut(i).set_f32(0x10, 0.0);
        self.zero_effect(i);
    }

    /// FindEffectIdx: first free slot below 64, else the oldest reusable one.
    fn find_effect_idx(&mut self) -> usize {
        let mut oldest = 0usize;
        let mut best = 0.0f32;
        let mut k = 0usize;
        let free = if self.effects.num == 0 || self.fx(0).f32_(fx::END) <= 0.0 {
            true
        } else {
            loop {
                let r = self.fx(k);
                if r.u32_(fx::FLAGS) & 0x8008_0000 == 0 && (best == 0.0 || r.f32_(fx::END) < best) {
                    best = r.f32_(fx::END);
                    oldest = k;
                }
                k += 1;
                if !(k < self.effects.num && 0.0 < self.fx(k).f32_(fx::END)) {
                    break;
                }
            }
            k < MAX_EFFECTS
        };
        if free {
            oldest = k;
            if k == self.effects.num {
                self.effects.num = k + 1;
            }
        }
        self.fx_mut(oldest).set_u32(fx::KIND, 0);
        self.delete_effect(oldest, true);
        oldest
    }

    /// StartFXTree(time, tree, position, flags, node flags).
    fn start_fx_tree(&mut self, time: f32, visual: Visual, frames: i16, timing: i16, position: Option<V3>, flags: u32) -> R<usize> {
        let i = self.find_effect_idx();
        let atime = self.globals.atime;
        let mut looping = 0u16;
        let end = if 0.0 < time {
            looping = 1;
            add(time, atime)?
        } else {
            let mut f = frames;
            if f == 0 {
                f = 0x1e;
                looping = 1;
            }
            add(mul(mul(f as f32, timing as f32)?, f32::from_bits(0x3a91_a2aa))?, atime)?
        };
        if flags & 0x2000_0000 != 0 {
            looping = 0;
        }
        self.effects.nodes[i] = Some(EffectNode {
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            position: position.unwrap_or([0.0; 3]),
            scale: 1.0,
            visual,
            start: atime,
        });
        let r = self.fx_mut(i);
        r.set_u32(0x14, 1);
        r.set_f32(fx::END, end);
        r.set_u16(0x50, looping);
        r.set_f32(fx::DURATION, sub(end, atime)?);
        r.set_u32(fx::FLAGS, flags);
        Ok(i)
    }

    /// ChangeEffect(slot, header, node flags): swap the tree (presentation).
    fn change_effect(&mut self, i: usize, header: usize) {
        if self.effects.tables.headers.get(header).copied().flatten().is_none() {
            return;
        }
        let link = self.fx(i).i16_(fx::LINK);
        if 0 <= link {
            self.delete_effect(link as usize, true);
            self.fx_mut(i).set_i16(fx::LINK, -1);
        }
        let atime = self.globals.atime;
        if let Some(n) = self.effects.nodes[i].as_mut() {
            n.visual = Visual::Header(header);
            n.start = atime;
        }
    }

    fn header_duration(&self, header: usize) -> R<f32> {
        let h = self.effects.tables.headers.get(header).copied().flatten().ok_or_else(|| Unsupported(format!("effect header {header} not loaded")))?;
        Ok(mul(mul(h.frames as f32, h.timing as f32)?, f32::from_bits(0x3a91_a2aa))?)
    }

    /// StartMissile(speed, damage, owner, position, direction, flags, info,
    /// tree, index, node flags).
    #[allow(clippy::too_many_arguments)]
    fn start_missile(&mut self, speed: f32, damage: f32, owner: i32, position: V3, dir: V3, flags: u32, info: &[u32; 12], index: i32, sfx_flags: u32) -> R<usize> {
        let fl = |k: usize| f32::from_bits(info[k]);
        let mut time = 3.0;
        let mut sound = info[11];
        if flags & 0x48_0000 != 0 {
            time = 2.0;
            if sound == 5 {
                sound = if index != 0 {
                    0
                } else if flags & 0x40_0000 != 0 {
                    7
                } else {
                    6
                };
            }
        }
        let v = [mul(speed, dir[0])?, mul(speed, dir[1])?, mul(speed, dir[2])?];
        if add(add(mul(v[0], v[0])?, mul(v[1], v[1])?)?, mul(v[2], v[2])?)? < 0.001 {
            return unsupported("StartMissile zero velocity (FatalErrorC)");
        }
        let mut f = u64::from(sfx_flags);
        if owner < 1 {
            f |= 0x1107;
        } else {
            f |= match self.globals.pvp_mode {
                1 => 0x200f,
                2 => 0xf,
                _ => 0x20e,
            };
            if flags & 0x10_0000 != 0 {
                f &= !4;
            }
        }
        if fl(5) == 0.0 && fl(6) == 0.0 && fl(7) == 0.0 {
            f |= 0x2_0000;
        }
        let player = usize::try_from(owner - 1).map_err(|_| Unsupported("enemy missile tree".into()))?;
        let i = self.start_fx_tree(time, Visual::Missile { player }, 0, 0, Some(position), f as u32 | 0x100_0000)?;
        let mut radius = fl(3);
        if flags & 0x200_0000 != 0 {
            radius = mul(radius, f32::from_bits(0x3fe6_6666))?;
        }
        // SfxSetPhysics(gravity, radius, slot, velocity, spin).
        let yaw = atan2f(v[0], v[2])?;
        {
            let r = self.fx_mut(i);
            r.set_v3(fx::VELOCITY, v);
            r.set_v3(fx::SPIN, [fl(5), fl(6), fl(7)]);
            if 0.0 <= fl(8) {
                r.set_f32(fx::GRAVITY, fl(8));
            }
            if 0.0 <= radius {
                r.set_f32(fx::RADIUS, radius);
            }
            // SfxSetHit(slot, hit fx, hit sound, wall sound).
            r.set_i16(fx::HIT_FX, info[9] as i16);
            r.set_u32(fx::HIT_SOUND, info[10]);
            r.set_u32(fx::WALL_SOUND, sound);
            // SfxSetDamage(damage, blast, fade, slot, flags, owner).
            r.set_f32(fx::DAMAGE, damage);
            let mut df = flags | info[0];
            if 4 < (df & 0xf) {
                df &= 0xffff_fff3;
            }
            r.set_u32(fx::DAMAGE_FLAGS, df);
            r.set_f32(fx::BLAST, fl(4));
            r.set_f32(fx::FADE, 0.0);
            r.set_i16(fx::OWNER, owner as i16);
        }
        if let Some(n) = self.effects.nodes[i].as_mut() {
            let (s, c) = (yaw.sin(), yaw.cos());
            n.matrix = [[c, 0.0, -s], [0.0, 1.0, 0.0], [s, 0.0, c]];
            if flags & 0x200_0000 != 0 {
                n.scale = f32::from_bits(0x3fe6_6666);
            }
        }
        // SfxSetStreak (owner > 0): streak colour/alpha are presentation.
        Ok(i)
    }

    /// PlayerStartMissile(distance, power, player, direction, flags, origin mode).
    pub fn player_start_missile(&mut self, distance: f32, power: f32, player: usize, dir: &mut V3, flags: u32, origin_mode: u32) -> R<i32> {
        let pl = self.players[player].clone();
        if pl.u32_(0x138) & 0xc400 != 0 || flags & 0x210_0000 == 0x10_0000 {
            return unsupported("boss/ballista/familiar missile info");
        }
        let class = pl.u32_(8) as usize;
        let info = *self.effects.tables.missile_info.get(class).ok_or_else(|| Unsupported(format!("missile class {class}")))?;
        let damage = mul(power, pl.f32_(0x128))?;
        let offset = match origin_mode {
            1 => self.class.missile_origin_5c,
            2 => self.class.missile_origin_158,
            _ => [0.0; 3],
        };
        // MulVecMat3(offset, out, player matrix +0x20).
        let m = [pl.v3(0x20), pl.v3(0x30), pl.v3(0x40)];
        let mut o = [0.0f32; 3];
        if origin_mode == 1 || origin_mode == 2 {
            for (c, oc) in o.iter_mut().enumerate() {
                *oc = add(add(mul(offset[0], m[0][c])?, mul(offset[1], m[1][c])?)?, mul(offset[2], m[2][c])?)?;
            }
        }
        let base = pl.v3(0x70);
        let origin = [add(base[0], o[0])?, add(base[1], o[1])?, add(base[2], o[2])?];
        let back = [add(mul(dir[0], -3.0)?, origin[0])?, add(mul(dir[1], -3.0)?, origin[1])?, add(mul(dir[2], -3.0)?, origin[2])?];
        let mut start = [add(dir[0], dir[0])?, add(dir[1], dir[1])?, add(dir[2], dir[2])?];
        start = [add(start[0], origin[0])?, add(start[1], origin[1])?, add(start[2], origin[2])?];
        let radius = f32::from_bits(info[3]);
        if flags & 0x10_0000 == 0 {
            let (hit, _) = self.weapon_wall_collide(radius, back, start)?;
            if let Some(h) = hit {
                if flags & 0x20_0000 == 0 {
                    self.events.push(Event::Effect { header: self.wall_hit_effect(flags), position: h });
                    return Ok(-1);
                }
                start = back;
            }
        }
        if let Some((it, h)) = self.weapon_item_collide(radius, back, start, false, player as i32)? {
            let d = self.damage_item(damage, it, flags, player as i32)?;
            if 0.0 <= d {
                self.player_damaged_item(player, it, d == 0.0)?;
                self.events.push(Event::Effect { header: 2, position: h });
            } else {
                self.events.push(Event::Effect { header: self.wall_hit_effect(flags), position: h });
            }
            if flags & 0x10_0000 == 0 {
                return Ok(-1);
            }
        }
        let speed_word = pl.f32_(0x12c);
        let speed = if 1.0 <= speed_word { speed_word.min(100.0) } else { 1.0 };
        if flags & 0x10_0000 == 0 && 0.0 < distance {
            *dir = [mul(distance, dir[0])?, mul(distance, dir[1])?, mul(distance, dir[2])?];
            calc_target_dir(speed, div(1.0, speed)?, f32::from_bits(info[8]), -0.5, dir)?;
        }
        let multi = flags & 0x48_0000;
        let wide = flags & 0x40_0000;
        let spread = self.effects.tables.spread;
        let mut k = 0i32;
        loop {
            if 0 < k && multi == 0 {
                return Ok(k);
            }
            if 3 <= k && wide == 0 {
                return Ok(k);
            }
            let (c, s) = (spread[k as usize], spread[5 + k as usize]);
            let d = [add(mul(dir[0], c)?, mul(dir[2], s)?)?, dir[1], add(mul(-dir[0], s)?, mul(dir[2], c)?)?];
            // WeapThrowFx[player][flags & 0xf] is runtime data; the fixture's
            // row is −1 only for the ordinary (no weapon powerup) throw.
            if (class == 2 || class == 6) && flags & 0xf != 0 {
                return unsupported("WeapThrowFx weapon-powerup throw effect");
            }
            let slot = self.start_missile(speed, damage, player as i32 + 1, start, d, flags, &info, k, 0)?;
            let mut scale = 1.0;
            if 1.0 < power {
                scale = mul(power, 1.0)?;
            }
            if 0x62 < pl.i32_(0x2ac0) {
                scale = mul(scale, f32::from_bits(0x3f99_999a))?;
            }
            if scale != 1.0
                && let Some(n) = self.effects.nodes[slot].as_mut()
            {
                n.scale = mul(n.scale, scale)?;
            }
            self.effects.pmissile[k as usize] = slot as i32;
            k += 1;
            if 4 < k {
                return Ok(k);
            }
        }
    }

    fn wall_hit_effect(&self, flags: u32) -> usize {
        self.effects.tables.wall_hit.get((flags & 0xf) as usize).copied().unwrap_or(1).max(0) as usize
    }

    /// WeaponWallCollide(radius, from, to): hit position and object.
    fn weapon_wall_collide(&mut self, r: f32, from: V3, to: V3) -> R<WallHit> {
        let (c, _) = self.collision.world_collide(Query {
            radius: r,
            first: from,
            last: to,
            check_flags: 7,
            min_normal_y: -2.0,
            max_normal_y: 2.0,
            object_mask: 0x23e,
            exclusion_mask: 4,
        })?;
        Ok((c.solid.map(|h| h.position), c.solid.map(|h| (h.object, c.water.is_some()))))
    }

    /// WeaponCheckItemCollision(item, player).
    fn weapon_check_item(&mut self, slot: usize, player: i32) -> R<bool> {
        let r = &self.items.records[slot];
        let d = &self.items.definitions[r.definition()];
        Ok(match d.kind() {
            1 => 0 >= r.i16_(0xda) && r.u32_(0xfc) == 0 && d.subtype() == 4,
            2 | 4 | 7 => true,
            3 => r.u8_(0xf6) != 0,
            5 => d.subtype() == 0x1f,
            8 => {
                if d.subtype() != 5 {
                    false
                } else if r.u8_(0xdc).wrapping_sub(1) < 2 {
                    self.items.records[slot].set_u8(0xde, 3);
                    true
                } else {
                    false
                }
            }
            10 => match d.subtype() {
                0x1e | 0x28 | 0x31 | 0x33 | 0x35 => false,
                0x29 => 1 <= r.i16_(0xf2),
                0x34 => {
                    if player < 0 || r.u16_(0xd8) & 1 != 0 {
                        false
                    } else {
                        self.audio("AudioRockFall", None, slot as i32);
                        self.items.records[slot].or_u16(0xd8, 1);
                        false
                    }
                }
                _ => true,
            },
            _ => false,
        })
    }

    /// WeaponItemCollide(radius, from, to, nearest, player): item and contact.
    fn weapon_item_collide(&mut self, r: f32, from: V3, to: V3, nearest: bool, player: i32) -> R<Option<(usize, V3)>> {
        let near = self.grid.items_near(&self.items, r, to)?;
        let env = self.collide_env();
        let mut best = 100_000.0f32;
        let mut hit = None;
        for slot in near {
            if !self.weapon_check_item(slot, player)? {
                continue;
            }
            let mut out = [0.0; 3];
            let d = item_coll_sub(&self.items, &mut self.collision, &env, slot, from, to, Some(&mut out), r, r)?;
            if 0.0 <= d && d < best {
                best = d;
                hit = Some((slot, out));
                if !nearest {
                    break;
                }
            }
        }
        Ok(hit)
    }

    /// SfxSkipItem(item, effect flags, damage flags): 0 hit, 1 skip, 2 pass.
    fn sfx_skip_item(&self, slot: usize, flags: u32, dflags: u32) -> u8 {
        let r = &self.items.records[slot];
        let d = &self.items.definitions[r.definition()];
        let st = d.subtype();
        let indestructible = r.i8_(0xe3) == -1;
        match d.kind() {
            1 => {
                if flags & 0x100 == 0 {
                    0
                } else if dflags & 0x80_0000 == 0 || !(st == 3 || st == 1) {
                    1
                } else {
                    2
                }
            }
            2 if st == 0x2b => u8::from(flags & 0x1000 != 0),
            2 => u8::from(indestructible && dflags & 0x600 == 0),
            3 => u8::from(flags & 0x1000 != 0),
            4 if dflags & 0x200 != 0 => 0,
            8 => {
                if dflags & 0x80_0000 == 0 {
                    1
                } else {
                    2
                }
            }
            10 => {
                let skip = (st != 0x29 && flags & 0x1000 != 0) || dflags & 0x200 != 0 || (indestructible && dflags & 0x400 == 0);
                if !skip {
                    0
                } else if dflags & 0x80_0000 == 0 {
                    1
                } else {
                    2
                }
            }
            _ => {
                let b = u8::from(flags & 0x100 != 0);
                if !indestructible || dflags & 0x600 != 0 { b } else { 1 }
            }
        }
    }

    /// MissileCollideEnemy(radius, from, to, owner, check cooldown, start).
    fn missile_collide_enemy(&self, r: f32, from: V3, to: V3, owner: usize, check: bool, start: usize) -> R<Option<(usize, V3)>> {
        let d = v_sub(to, from)?;
        let seg2 = add(mul(d[0], d[0])?, mul(d[2], d[2])?)?;
        let segy = mul(d[1], d[1])?;
        for k in start..25 {
            let en = &self.enemies[k];
            let st = en.i32_(e::STATE);
            if st != 1 && st != 6 {
                continue;
            }
            if check && self.globals.time < en.f32_(0x2c8 + 4 * owner) {
                continue;
            }
            let h = add(r, en.f32_(e::HALF_HEIGHT))?;
            let w = add(r, en.f32_(e::WIDTH))?;
            let c = en.v3(e::COLLISION);
            let dx = sub(c[0], to[0])?;
            let dz = sub(c[2], to[2])?;
            if add(mul(w, w)?, seg2)? < add(mul(dx, dx)?, mul(dz, dz)?)? {
                continue;
            }
            if add(h, segy)? < sub(c[1], to[1])? {
                continue;
            }
            if let Some(p) = super::enemies::line_cylinder_collide(c, from, to, w, h, false)? {
                return Ok(Some((k, p)));
            }
        }
        Ok(None)
    }

    /// closest_enemy(cone, range, origin, facing, flags): (distance, enemy, vector).
    fn closest_enemy(&self, cone: f32, range: f32, origin: V3, facing: V3, flags: u32) -> R<(f32, i32, Option<V3>)> {
        let k = div(sub(1.0, cone)?, range)?;
        let mut best = range;
        let mut who = -1;
        let mut vec = None;
        for i in self.enemies_near(range, origin)? {
            let en = &self.enemies[i];
            let st = en.i32_(e::STATE);
            let t = en.i32_(e::TYPE);
            if (st != 1 && st != 6) || t == 0x1f || (t == 0x1e && flags & 0x8_0000 == 0) {
                continue;
            }
            let mut v = v_sub(en.v3(e::COLLISION), origin)?;
            if 10.0 < v[1].abs() {
                continue;
            }
            let d = sub(normal_vector(&mut v)?, en.f32_(e::WIDTH))?;
            if range < d {
                continue;
            }
            let (f0, _) = fqdist(v[0], v[2])?;
            let f1 = dot_xz(v, facing)?;
            let f2 = mul(add(cone, mul(d, k)?)?, f0)?;
            if f1 < f2 || d >= best || d.is_nan() {
                continue;
            }
            who = i as i32;
            best = d;
            vec = Some(v);
        }
        Ok((best, who, vec))
    }

    /// closest_generator(cone, range, origin, facing): (distance, item, vector).
    fn closest_generator(&self, cone: f32, range: f32, origin: V3, facing: V3) -> R<(f32, Option<usize>, Option<V3>)> {
        let k = div(sub(1.0, cone)?, range)?;
        let mut best = range;
        let mut found: Option<(usize, V3)> = None;
        for slot in self.grid.items_near(&self.items, range, origin)? {
            let r = &self.items.records[slot];
            let fl = r.i16_(0xd8);
            if fl == -1 || (fl as u16) & 0x8100 != 0 || r.i8_(0xe3) == -1 {
                continue;
            }
            let d = &self.items.definitions[r.definition()];
            let kind = d.kind();
            if kind == -1 || r.u8_(0xe1) != 0 || (fl as u16) & 0x4000 == 0 {
                continue;
            }
            let st = d.subtype();
            let ok = match kind {
                3 => true,
                5 => st == 0x1f,
                2 | 10 => {
                    let barrel = (0x2b..=0x2d).contains(&st);
                    !((kind == 2 && !barrel) || (kind == 10 && st == 0x29) || (barrel && 0 < r.i8_(0xdc)))
                }
                _ => false,
            };
            if !ok {
                continue;
            }
            let mut v = v_sub(r.v3(0x60), origin)?;
            if add(d.f32_(0x10), d.f32_(0x10))? < v[1].abs() {
                continue;
            }
            let mut dist = normal_vector(&mut v)?;
            if kind != 3 {
                dist = mul(dist, if st == 0x2c || st == 0x2d { f32::from_bits(0x3f66_6666) } else { f32::from_bits(0x3f99_999a) })?;
            }
            let rad = if 5.0 <= d.f32_(0xc) { 5.0 } else { d.f32_(0xc) };
            let dist = sub(dist, rad)?;
            if range < dist {
                continue;
            }
            let (f0, _) = fqdist(v[0], v[2])?;
            if dot_xz(v, facing)? < mul(add(cone, mul(dist, k)?)?, f0)? || dist >= best || dist.is_nan() {
                continue;
            }
            best = dist;
            found = Some((slot, v));
        }
        Ok((best, found.map(|f| f.0), found.map(|f| f.1)))
    }

    /// PlayerGetTarget(player, origin, facing, out direction, target, item).
    pub fn player_get_target(&self, p: usize, origin: V3, facing: V3, out: &mut V3, target: &mut i32, item: &mut Option<usize>) -> R<f32> {
        if self.boss_crit_inst != 0 || !self.critters_empty() {
            return unsupported("PlayerGetTarget with critters");
        }
        if self.globals.pvp_mode == 2 {
            return unsupported("PlayerGetTarget player-vs-player");
        }
        let range = 30.0;
        let mut d = range;
        let pl = &self.players[p];
        let mut target_pos = [0.0; 3];
        if (0..0x10000).contains(target) {
            let en = &self.enemies[*target as usize];
            if en.i32_(e::TYPE) != 0x1f {
                target_pos = en.v3(e::COLLISION);
                *out = v_sub(target_pos, origin)?;
                d = sub(normal_vector(out)?, en.f32_(e::WIDTH))?;
                if (0.0 < d || en.i32_(e::TYPE) == 0x1e) && dot3(*out, facing)? < 0.5 {
                    d = range;
                }
            }
        } else if 0x10000 <= *target {
            return unsupported("critter target");
        }
        let mut far = range <= d;
        if let Some(it) = item.filter(|_| far)
            && 0 <= self.items.records[it].i8_(0xe3)
        {
            let r = &self.items.records[it];
            *out = v_sub(r.v3(0x60), origin)?;
            d = sub(normal_vector_2d(out)?, self.items.definitions[r.definition()].f32_(0xc))?;
            if dot3(*out, facing)? < f32::from_bits(0x3f34_fdf4) {
                d = range;
            }
            far = range <= d;
        }
        if far {
            let cone = if 0 < self.target_cone_mode { f32::from_bits(0x3f34_fdf4) } else { 0.5 };
            let flags = pl.f32_(0x11c) as i32 as u32;
            let (de, who, v) = self.closest_enemy(cone, range, origin, facing, flags)?;
            d = de;
            *target = who;
            if let Some(v) = v {
                *out = v;
            }
            let (dg, generator, gv) = self.closest_generator(cone, range, origin, facing)?;
            *item = generator;
            if dg < d {
                *out = gv.unwrap_or(*out);
                *target = -1;
                d = dg;
            }
        }
        if range <= d {
            *target = -1;
            let pc = pl.v3(0x70);
            let dx = sub(origin[0], pc[0])?;
            let dy = sub(origin[1], pc[1])?;
            let dz = sub(origin[2], pc[2])?;
            let (h, _) = fqdist(dx, dz)?;
            out[1] = if h < f32::from_bits(0x3d4c_cccd) { 0.0 } else { div(dy, h)? };
            out[0] = sinf(pl.f32_(0xdc))?;
            out[2] = cosf(pl.f32_(0xdc))?;
            normal_vector(out)?;
        }
        if 0 <= *target
            && let Some(it) = pl.u32_(0x880).checked_sub(1).map(|v| v as usize)
            && self.items.definitions[self.items.records[it].definition()].kind() == 7
        {
            return unsupported("PlayerGetTarget blocking item (kind 7) check");
        }
        let _ = target_pos;
        Ok(d)
    }

    /// GetAttackDir(distance, charge, player, facing, direction, flags, enemy, item).
    #[allow(clippy::too_many_arguments)]
    pub fn get_attack_dir(&self, charge: f32, p: usize, facing: V3, dir: &mut V3, flags: u32, enemy: i32, item: Option<usize>) -> R<f32> {
        let pl = &self.players[p];
        let align = dot_xz(facing, *dir)?;
        let (len, _) = fqdist(dir[0], dir[2])?;
        let mut pitch = pl.f32_(0x894);
        if f32::from_bits(0x3f34_fdf4) < pitch {
            pitch = f32::from_bits(0x3f34_fdf4);
        }
        if pl.u32_(0x898) & 8 != 0 && 0.0 < pitch && pitch < 0.5 {
            pitch = 0.5;
        }
        if pl.i32_(8) == 4 {
            pitch = add(pitch, f32::from_bits(0x3e4c_cccd))?;
        }
        let toward = |d: &mut V3| {
            d[0] = facing[0];
            d[2] = facing[2];
            d[1] = pitch;
        };
        if align < mul(len, f32::from_bits(0x3f5d_b22d))? || flags & 0x10_0000 != 0 {
            toward(dir);
        } else if enemy < 0 && item.is_none() {
            let k = smallsqrt(sub(1.0, mul(pitch, pitch)?)?)?;
            dir[0] = mul(k, dir[0])?;
            dir[2] = mul(k, dir[2])?;
            dir[1] = pitch;
        } else {
            if 0.0 < dir[1] {
                dir[1] = mul(dir[1], f32::from_bits(0x3f99_999a))?;
            }
            if dir[1].abs() < pitch.abs() {
                dir[1] = mul(add(pitch, dir[1])?, 0.5)?;
            }
        }
        if dir[1] < 0.0 {
            dir[1] = mul(dir[1], 0.5)?;
        }
        normal_vector(dir)?;
        if f32::from_bits(0x3f5d_b22d) < dir[1] {
            *dir = facing;
        }
        Ok(add(mul(charge, 200.0)?, 15.0)?)
    }

    /// The ordinary-throw launch of PlayerMotion (EE 0x14be30..0x14be48):
    /// GetAttackDir then PlayerStartMissile with the update's target.
    #[allow(clippy::too_many_arguments)]
    pub fn player_throw(&mut self, p: usize, facing: V3, target_dir: V3, enemy: i32, item: Option<usize>, charge: f32, power: f32, origin_mode: u32) -> R<i32> {
        let flags = self.players[p].u32_(0x130);
        let mut dir = target_dir;
        let distance = self.get_attack_dir(charge, p, facing, &mut dir, flags, enemy, item)?;
        let n = self.player_start_missile(distance, power, p, &mut dir, flags, origin_mode)?;
        self.audio("AudioThrowWeapon", Some(p), 0);
        Ok(n)
    }

    /// PlayerDamagedEnemy(player, enemy, prior state, result, attack).
    fn player_damaged_enemy(&mut self, p: usize, i: usize, prior: i32, result: i32, attack: bool) -> R<()> {
        let t = self.enemies[i].i32_(e::TYPE);
        if (t == self.globals.boss_type && self.globals.boss_active == 0) || !(prior == 1 || prior == 6) {
            return Ok(());
        }
        if 0 < result && self.enemies[i].i16_(0x2ec) == 0 {
            let c = self.players[p].i32_(0x8f0) + 1;
            self.players[p].set_i32(0x8f0, c);
            if 10 <= c && self.globals.boss_type < 0 {
                self.help(0x16, p);
            }
        }
        let k = match t {
            -2 => 1,
            -3 => 2,
            t if t < 0 => 0,
            t => t,
        } as usize;
        let table = match (attack, result != 0) {
            (false, false) => 0,
            (false, true) => 1,
            (true, false) => 2,
            (true, true) => 3,
        };
        let amount = *self.effects.tables.exp[table].get(k).ok_or_else(|| Unsupported("exp table index".into()))?;
        self.events.push(Event::Exp { player: p, amount, attack });
        Ok(())
    }

    /// PlayerDamagedItem(player, item, destroyed).
    fn player_damaged_item(&mut self, p: usize, slot: usize, destroyed: bool) -> R<()> {
        let r = &self.items.records[slot];
        let d = &self.items.definitions[r.definition()];
        match d.kind() {
            1 if r.i16_(0xda) < 1 && d.subtype() == 4 => unsupported("start_magic from a shot potion"),
            3 => {
                if destroyed {
                    self.players[p].set_i32(0x8f0, 0);
                    let o = 0xb3c + 0x1c * self.players[p].u32_(0xc) as usize;
                    let v = self.players[p].i32_(o);
                    self.players[p].set_i32(o, v + 1);
                }
                let k = match self.items.records[slot].i16_(0xf0) {
                    -2 => 1,
                    -3 => 2,
                    v if v < 0 => 0,
                    v => v,
                } as usize;
                let table = usize::from(destroyed);
                let amount = 5 * *self.effects.tables.exp[table].get(k).ok_or_else(|| Unsupported("exp table index".into()))?;
                self.events.push(Event::Exp { player: p, amount, attack: false });
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// DamageItem(damage, item, flags, player): new health, -1 indestructible,
    /// -2 explosive-flag pass.
    pub fn damage_item(&mut self, damage: f32, slot: usize, flags: u32, player: i32) -> R<f32> {
        let mut dmg = damage;
        let def = self.items.records[slot].definition();
        let kind = self.items.definitions[def].kind();
        let st = self.items.definitions[def].subtype();
        let target = self.level_f32(0x9c);
        if kind == 3 && 0 <= player && 0.0 < target {
            let lvl = self.players[player as usize].i32_(0x2ac0) as f32;
            let k = if lvl < target {
                sub(1.0, mul(sub(target, lvl)?, f32::from_bits(0x3c23_d70a))?)?
            } else if target < lvl {
                add(mul(sub(lvl, target)?, f32::from_bits(0x3dcc_cccd))?, 1.0)?
            } else {
                1.0
            };
            dmg = mul(dmg, k)?;
            if dmg < 1.0 {
                dmg = 1.0;
            }
        }
        let mut killed = false;
        let (result, shown);
        if flags & 0x800 == 0 {
            let armor = self.items.records[slot].i8_(0xe3);
            if 0 <= armor {
                dmg = sub(dmg, armor as f32)?;
                if dmg <= 0.0 {
                    dmg = 1.0;
                }
                let n = round(dmg)?;
                let h = self.items.records[slot].u16_(0xe4);
                let nh = (u32::from(h)).wrapping_sub(n as u32);
                self.items.records[slot].set_u16(0xe4, nh as u16);
                if ((nh << 16) as i32) < 0 {
                    self.items.records[slot].set_i16(0xe4, 0);
                }
                killed = self.items.records[slot].i16_(0xe4) == 0;
            }
            if armor == -1 {
                result = -1;
                shown = false;
            } else {
                result = self.items.records[slot].i16_(0xe4) as i32;
                shown = true;
            }
        } else {
            result = if dmg <= 2.0 { -1 } else { -2 };
            dmg = 0.0;
            shown = false;
        }
        let pos = self.items.records[slot].v3(0x60);
        let at = [pos[0], add(pos[1], 2.0)?, pos[2]];
        match kind {
            1 => match st {
                3 if result == -2 => return unsupported("DamageItem explosive potion swap"),
                4 if result == 0 => return unsupported("start_magic from a destroyed potion"),
                _ if flags & 0x400 != 0 && 5.0 <= dmg => return unsupported("DamageItem magic on powerup"),
                _ => {}
            },
            2 => {
                let content = self.items.records[slot].i16_(0xf0);
                if 0 <= content && flags & 0x200 != 0 {
                    return unsupported("DamageItem death-chest potion path");
                }
                if killed && self.items.records[slot].u16_(0xd8) & 0x200 != 0 {
                    if self.items.records[slot].u16_(0xd8) & 1 == 0 {
                        self.items.records[slot].or_u16(0xd8, 1);
                        self.open_chest(slot, None)?;
                        if st == 0x2b {
                            self.audio("AudioWoodBarrel", None, 0);
                            self.events.push(Event::Help { id: 0x1b, player: usize::MAX });
                        }
                    }
                } else if flags & 0x400 != 0 && 5.0 <= dmg {
                    return unsupported("DamageItem magic on chest");
                }
            }
            3 => {
                let base = mul(self.items.definitions[def].i16_(0x44) as f32, self.level_f32(0xcc))? as i32;
                if 0 <= result {
                    let h = self.items.records[slot].i16_(0xe4) as i32;
                    let level: u8 = if killed {
                        0
                    } else if base < h {
                        if h <= base << 1 { 2 } else { 3 }
                    } else {
                        1
                    };
                    if level != self.items.records[slot].u8_(0xf6) {
                        let r = &mut self.items.records[slot];
                        r.set_u8(0xf6, level);
                        if level == 0 {
                            r.set_i8(0xe3, -1);
                        } else {
                            r.set_u8(0xf3, r.u8_(0xf3) << 1);
                        }
                        if r.u8_(0xf7).wrapping_sub(0x1c) < 2 || r.u8_(0xf7) == 0x1e {
                            r.set_u8(0xf7, 0);
                        }
                        // ItemGenDesc + model/tree swap: presentation (GEN_<prefix><level>).
                        self.events.push(Event::ItemModel { slot, level });
                        if level == 0 {
                            self.items.records[slot].and_u16(0xd8, 0xfffe);
                            self.items.records[slot].set_i8(0xe3, -1);
                        }
                        self.events.push(Event::GeneratorHit { slot, destroyed: level == 0 });
                    }
                    if level == 0 {
                        self.audio("AudioGeneratorDies", None, slot as i32);
                        for k in 0..self.enemies.len() {
                            if self.enemies[k].u32_(e::GENERATOR) == slot as u32 + 1 {
                                self.enemies[k].set_u32(e::GENERATOR, 0);
                            }
                        }
                    } else {
                        self.audio("AudioGeneratorDamaged", None, slot as i32);
                    }
                }
            }
            4 => {
                let v = self.items.records[slot].u32_(0xf8) | 1;
                self.items.records[slot].set_u32(0xf8, v);
            }
            5 => {
                if flags & 0x800 == 0 && st == 0x1f {
                    return unsupported("DamageItem kind 5 chain");
                }
            }
            10 if 0 <= result => {
                if shown && !killed && st != 0x29 {
                    self.items.records[slot].set_u16(0xf4, 1);
                }
                match st {
                    0x29 | 0x2a | 0x2c | 0x2d => return unsupported("DamageItem rock/wall/explosive barrel"),
                    _ => {
                        if killed {
                            self.items.records[slot].or_u16(0xd8, 1);
                            self.audio("AudioWoodBarrel", None, 0);
                            killed = false;
                        } else {
                            self.audio("AudioWeaponHitBarrel", None, 0);
                        }
                    }
                }
                if killed {
                    self.delete_item(slot, false);
                }
            }
            _ => {}
        }
        if shown {
            // AddItemHitCol: hit spark (presentation).
            self.events.push(Event::Effect { header: if killed { 3 } else { 2 }, position: at });
        }
        Ok(result as f32)
    }

    /// The missile subset of ProcessEffects.
    pub fn process_effects(&mut self) -> R<()> {
        if self.globals.paused != 0 || self.globals.help != 0 {
            return Ok(());
        }
        let mut i = 0;
        while i < self.effects.num {
            if 0.0 < self.fx(i).f32_(fx::END) {
                self.process_effect(i)?;
            }
            i += 1;
        }
        Ok(())
    }

    fn process_effect(&mut self, i: usize) -> R<()> {
        let Some(node) = self.effects.nodes[i] else { return Ok(()) };
        let spf = self.globals.spf;
        let atime = self.globals.atime;
        let time = self.globals.time;
        let mut m = node.matrix;
        let from = node.position;
        let mut to = from;
        let mut moved = false;
        let mut splash = false;
        let flags = self.fx(i).u32_(fx::FLAGS);
        if flags & 0x4000_0000 != 0 {
            return unsupported("homing effect (0x40000000)");
        }
        if flags & 0xc000 != 0xc000 {
            let v = self.fx(i).v3(fx::VELOCITY);
            if v[0] != 0.0 || v[1] != 0.0 || v[2] != 0.0 {
                to = [add(mul(v[0], spf)?, from[0])?, add(mul(v[1], spf)?, from[1])?, add(mul(v[2], spf)?, from[2])?];
                moved = true;
            }
            let r = self.fx(i);
            let nv = [
                sub(v[0], mul(r.f32_(fx::DRAG_X), spf)?)?,
                sub(v[1], mul(r.f32_(fx::GRAVITY), spf)?)?,
                sub(v[2], mul(r.f32_(fx::DRAG_Z), spf)?)?,
            ];
            self.fx_mut(i).set_v3(fx::VELOCITY, nv);
        }
        if moved {
            let r = self.fx(i).f32_(fx::RADIUS);
            if add(add(r, r)?, to[1])? < sub(self.globals.world_bottom, 25.0)? {
                self.fx_mut(i).set_u32(fx::KIND, 0);
                self.delete_effect(i, true);
                return Ok(());
            }
        }
        if flags & 0x2_0000 == 0 {
            let s = self.fx(i).v3(fx::SPIN);
            if s != [0.0; 3] {
                // Pitch/Yaw/RollMat3: orientation only (presentation).
                moved = true;
            }
        } else {
            m = dir_matrix(self.fx(i).v3(fx::VELOCITY));
            moved = true;
        }
        let age = sub(self.fx(i).f32_(fx::END), atime)?;
        let flags = self.fx(i).u32_(fx::FLAGS);
        let (mode, mut cool) = if flags & 0x20 == 0 {
            if 0.0 < self.fx(i).f32_(fx::HOLD) { (4, 0.0) } else { (0, 0.25) }
        } else if flags & 0x10 == 0 {
            (1, add(age, f32::from_bits(0x3d88_88b5))?)
        } else {
            (2, if 1.0 < age { 1.0 } else { age })
        };
        let pierce = self.fx(i).u32_(fx::DAMAGE_FLAGS) & 0x10_0000;
        let hit_fx = self.fx(i).i16_(fx::HIT_FX);
        if 0 < hit_fx
            && flags & 0x20 == 0
            && pierce == 0
            && let Some(h) = self.effects.tables.headers.get(hit_fx as usize).copied().flatten()
            && 0.0 < h.frames as f32
        {
            cool = mul(h.frames as f32, f32::from_bits(0x3d08_8889))?;
        }
        let hold = if flags & 0x80_0000 != 0 {
            0.0
        } else if pierce != 0 {
            add(cool, 3.0)?
        } else if flags & 0x20 != 0 && cool < 1.0 {
            1.0
        } else if cool < 0.2 {
            0.2
        } else {
            cool
        };
        let r = self.fx(i);
        let (radius, power) = match mode {
            2 | 4 => (r.f32_(fx::BLAST), 1.0),
            0 => (r.f32_(fx::RADIUS), 1.0),
            _ => {
                let dur = sub(r.f32_(fx::DURATION), r.f32_(fx::FADE))?;
                let mut t = if dur < age {
                    0.0
                } else if f32::from_bits(0x3d08_8889) < dur {
                    div(age, dur)?
                } else {
                    1.0
                };
                let rad = if f32::from_bits(0x3ea8_f5c3) < t {
                    let k = add(sub(1.0, t)?, f32::from_bits(0x3ea8_f5c3))?;
                    t = mul(sub(t, f32::from_bits(0x3ea8_f5c3))?, 1.5)?;
                    mul(k, r.f32_(fx::BLAST))?
                } else {
                    t = 0.0;
                    0.0
                };
                (rad, t)
            }
        };
        let damage = mul(power, self.fx(i).f32_(fx::DAMAGE))?;
        let fwd = {
            let mut f = m[2];
            normal_vector_2d(&mut f)?;
            f
        };
        let owner_raw = self.fx(i).i16_(fx::OWNER);
        let owner = if (owner_raw as i64 as u64) < 5 { owner_raw as usize } else { 0 };
        let hits_before = self.fx(i).i16_(fx::HITS);
        let mut result: i32 = 0;
        let mut out;
        let flags = self.fx(i).u32_(fx::FLAGS);
        if flags & 1 != 0 {
            return unsupported("effect vs player collision (MissileCollidePlayer)");
        }
        if flags & 8 != 0 {
            if mode == 0 {
                let check = (flags >> 24) & 1 == 0;
                let mut start = 0;
                while let Some((k, p)) = self.missile_collide_enemy(radius, from, to, owner, check, start)? {
                    out = p;
                    if self.fx(i).u32_(fx::FLAGS) & 0x400 == 0 {
                        let mut dir = self.fx(i).v3(fx::VELOCITY);
                        let h = self.fx(i).i16_(fx::HITS) + 1;
                        self.fx_mut(i).set_i16(fx::HITS, h);
                        normal_vector(&mut dir)?;
                        let prior = if self.enemies[k].f32_(e::HEALTH) <= 0.0 { 0 } else { self.enemies[k].i32_(e::STATE) };
                        let df = self.fx(i).u32_(fx::DAMAGE_FLAGS);
                        let res = self.damage_enemy(damage, k, owner as i32 - 1, df, Some(out), Some(dir), 2)?;
                        if res < 0 {
                            result = 2;
                        } else {
                            if 2.0 < damage && self.fx(i).u32_(fx::FLAGS) & 0x80_0000 == 0 {
                                self.enemies[k].set_f32(0x2c8 + 4 * owner, add(hold, time)?);
                                self.fx_mut(i).and_u32(fx::FLAGS, 0xfeff_ffff);
                            }
                            if 0 < owner {
                                self.player_damaged_enemy(owner - 1, k, prior, res, false)?;
                            }
                            result = if res != 0 { 3 } else { 2 };
                        }
                        start = k + 1;
                        if pierce != 0 {
                            continue;
                        }
                        to = out;
                        moved = true;
                    } else if f32::from_bits(0x3d88_9a02) < sub(self.fx(i).f32_(fx::DURATION), age)? {
                        result = -1;
                    }
                    start = k + 1;
                    if pierce == 0 {
                        break;
                    }
                }
                if pierce != 0 {
                    result = 0;
                }
            } else if 0.0 < radius && self.fx(i).u32_(fx::FLAGS) & 0x400 == 0 {
                for k in self.enemies_near(radius, to)? {
                    let st = self.enemies[k].i32_(e::STATE);
                    let f = self.fx(i).u32_(fx::FLAGS);
                    if !(st == 1 || st == 6) || (st == 6 && f & 0x100 != 0) {
                        continue;
                    }
                    if f & 0x100_0000 == 0 && time < self.enemies[k].f32_(0x2c8 + 4 * owner) {
                        continue;
                    }
                    let mut d = v_sub(self.enemies[k].v3(e::COLLISION), to)?;
                    let dist = normal_vector_2d(&mut d)?;
                    let lim = add(radius, self.enemies[k].f32_(e::WIDTH))?;
                    if lim < dist {
                        continue;
                    }
                    let mut cone = self.fx(i).f32_(fx::CONE);
                    if -1.0 < cone {
                        if dist < mul(lim, 0.2)? {
                            cone = mul(cone, f32::from_bits(0x3f59_999a))?;
                        }
                        if dot_xz(d, fwd)? < cone {
                            continue;
                        }
                    }
                    let h = self.fx(i).i16_(fx::HITS) + 1;
                    self.fx_mut(i).set_i16(fx::HITS, h);
                    let prior = if self.enemies[k].f32_(e::HEALTH) <= 0.0 { 0 } else { self.enemies[k].i32_(e::STATE) };
                    let df = self.fx(i).u32_(fx::DAMAGE_FLAGS);
                    let res = self.damage_enemy(damage, k, owner as i32 - 1, df, None, Some(d), 2)?;
                    if 0 <= res {
                        if 2.0 < damage && self.fx(i).u32_(fx::FLAGS) & 0x80_0000 == 0 {
                            self.enemies[k].set_f32(0x2c8 + 4 * owner, add(hold, time)?);
                        }
                        self.enemies[k].set_i32(0x2dc, i as i32);
                        if 0 < owner {
                            self.player_damaged_enemy(owner - 1, k, prior, res, false)?;
                        }
                        let hf = self.fx(i).i16_(fx::HIT_FX);
                        if 0 <= hf {
                            self.events.push(Event::Effect { header: hf as usize, position: self.enemies[k].v3(e::ATTENTION) });
                        }
                    }
                }
            }
            if result == 0 && !self.critters_empty() {
                return unsupported("effect vs critters");
            }
        }
        if result == 0 && self.fx(i).u32_(fx::FLAGS) & 2 != 0 {
            if mode == 0 {
                if let Some((it, p)) = self.weapon_item_collide(radius, from, to, true, owner as i32 - 1)? {
                    out = p;
                    let mut skip = self.sfx_skip_item(it, self.fx(i).u32_(fx::FLAGS), self.fx(i).u32_(fx::DAMAGE_FLAGS));
                    if self.fx(i).u32_(fx::DAMAGE_FLAGS) & 0x800 != 0 {
                        skip = 1;
                    }
                    if skip == 0 {
                        if pierce == 0 {
                            to = out;
                            moved = true;
                        }
                        let h = self.fx(i).i16_(fx::HITS) + 1;
                        self.fx_mut(i).set_i16(fx::HITS, h);
                        let df = self.fx(i).u32_(fx::DAMAGE_FLAGS);
                        let d = self.damage_item(damage, it, df, owner as i32 - 1)?;
                        if 0.0 <= d && 0 < owner {
                            self.player_damaged_item(owner - 1, it, d == 0.0)?;
                        }
                        if 2.0 < damage && self.fx(i).u32_(fx::FLAGS) & 0x80_0000 == 0 {
                            self.items.records[it].set_f32(0xec, add(cool, time)?);
                        }
                        result = if d < -1.0 {
                            -1
                        } else if d < 0.0 {
                            1
                        } else if d == 0.0 {
                            3
                        } else {
                            2
                        };
                    } else {
                        result = 1;
                    }
                    if pierce != 0 {
                        let r = &self.items.records[it];
                        if self.items.definitions[r.definition()].kind() == 10 && r.i16_(0xf0) == 0x29 && 1 <= r.i16_(0xe4) {
                            self.fx_mut(i).set_i16(fx::HIT_FX, -1);
                        } else {
                            result = 0;
                        }
                    }
                }
            } else {
                return unsupported("area effect vs items");
            }
        }
        if result == 0 {
            if self.fx(i).u32_(fx::FLAGS) & 4 != 0 {
                let (hit, obj) = self.weapon_wall_collide(mul(radius, 0.5)?, from, to)?;
                if let (Some(h), Some((o, water))) = (hit, obj) {
                    splash = water;
                    to = h;
                    moved = true;
                    if self.collision.objects[o].flags & 4 != 0 {
                        to[1] = add(h[1], 2.0)?;
                    }
                    let all = self.collision.runtime_flags.get(o).copied().unwrap_or(self.collision.objects[o].flags);
                    if matches!(all & 0xf_0000, 0x4_0000 | 0x5_0000) {
                        return unsupported("WorldObjectExplode (breakable world object)");
                    }
                    if self.fx(i).u32_(fx::DAMAGE_FLAGS) & 0x20_0000 == 0 {
                        result = 1;
                    } else {
                        return unsupported("bouncing missile (ReflectVector)");
                    }
                }
            }
            if result == 0 && self.fx(i).u32_(fx::FLAGS) & 0x40 != 0 && self.fx(i).f32_(0x84) < 0.1 {
                let (fy, _) = self.collision.floor_pos(self.globals.world_bottom, radius, to, 0)?;
                if sub(to[1], add(fy, 0.1)?)? < 0.1 {
                    self.fx_mut(i).set_v3(fx::VELOCITY, [0.0; 3]);
                    self.fx_mut(i).set_f32(fx::GRAVITY, 0.0);
                    moved = true;
                    to[1] = add(fy, 0.1)?;
                }
            }
        }
        let f = self.fx(i).u32_(fx::FLAGS);
        if 0.0 < self.fx(i).f32_(fx::BLAST) && f & 0x4020 == 0 && sub(self.fx(i).f32_(fx::END), f32::from_bits(0x3d88_9a02))? <= atime {
            result = 1;
            if f & 0x1000_0000 != 0 {
                return unsupported("firework effect");
            }
        }
        if pierce != 0 {
            let df = self.fx(i).u32_(fx::DAMAGE_FLAGS);
            if df & 0x200_0000 != 0 && hits_before < self.fx(i).i16_(fx::HITS) {
                let half = mul(self.fx(i).f32_(fx::DAMAGE), 0.5)?;
                self.fx_mut(i).set_f32(fx::DAMAGE, half);
                if half < 1.0 {
                    self.fx_mut(i).and_u32(fx::DAMAGE_FLAGS, 0xffef_ffff);
                }
            }
        }
        if result == -1 && 0 < self.fx(i).i16_(fx::HIT_FX) && 0.0 < self.fx(i).f32_(fx::BLAST) {
            result = 1;
        }
        let linger = self.fx(i).f32_(fx::LINGER);
        if 0.0 < linger && atime < linger {
            result = 0;
        }
        if result < 0 {
            self.fx_mut(i).set_f32(fx::END, atime);
            self.fx_mut(i).set_u16(0x50, 0);
        } else if result != 0 {
            let hit_at = if moved { to } else { from };
            if splash {
                self.audio("AudioSplash", None, 0);
            } else {
                let s = self.fx(i).u32_(if result == 1 { fx::WALL_SOUND } else { fx::HIT_SOUND });
                if s != 0 {
                    self.audio("AudioEffect", None, s as i32);
                }
            }
            let mut h = self.fx(i).i16_(fx::HIT_FX) as i32;
            if h < 1 && result == 1 {
                h = self.wall_hit_effect(self.fx(i).u32_(fx::DAMAGE_FLAGS)) as i32;
                self.fx_mut(i).and_u32(fx::FLAGS, 0xffff_fff0);
            }
            if 0xd8 < (h - 1) as u32 {
                self.fx_mut(i).set_f32(fx::END, atime);
                self.fx_mut(i).set_u16(0x50, 0);
            } else {
                let h = h as usize;
                self.change_effect(i, h);
                self.zero_effect(i);
                if let Some(n) = self.effects.nodes[i].as_ref() {
                    m = n.matrix;
                }
                let hs = self.fx(i).f32_(fx::HIT_SCALE);
                if hs != 1.0
                    && let Some(n) = self.effects.nodes[i].as_mut()
                {
                    n.scale = hs;
                }
                if self.fx(i).u32_(fx::DAMAGE_FLAGS) & 0x400_0000 != 0 && self.fx(i).f32_(fx::HOLD) == 0.0 {
                    self.fx_mut(i).set_f32(fx::HOLD, 20.0);
                }
                let hold_t = self.fx(i).f32_(fx::HOLD);
                if 0.0 < hold_t {
                    let end = add(atime, hold_t)?;
                    let r = self.fx_mut(i);
                    r.set_f32(fx::END, end);
                    r.set_f32(fx::DURATION, sub(end, atime)?);
                    r.set_u16(0x50, 0);
                    r.set_u32(fx::FLAGS, r.u32_(fx::FLAGS) & 0xfe7d_fbf9 | 0x10_0000);
                } else {
                    let end = add(self.header_duration(h)?, atime)?;
                    let blast = self.fx(i).f32_(fx::BLAST);
                    let no_potion = self.fx(i).u32_(fx::DAMAGE_FLAGS) & 0x200 == 0;
                    let r = self.fx_mut(i);
                    r.set_f32(fx::END, end);
                    r.set_f32(fx::DURATION, sub(end, atime)?);
                    r.set_u16(0x50, 0);
                    if blast <= 0.0 {
                        r.and_u32(fx::FLAGS, 0xffff_fff0);
                    } else {
                        let mut v = r.u32_(fx::FLAGS) & 0xf67c_fbfb;
                        if no_potion {
                            v &= 0xf67c_fbf9;
                        }
                        r.set_u32(fx::FLAGS, v | 0x28 | u32::from(owner != 0));
                    }
                }
                let f = self.fx(i).u32_(fx::FLAGS);
                if f & 0x30_0000 != 0 && self.fx(i).f32_(fx::HOLD) <= 0.0 {
                    return unsupported("PlaceEffectOnFloor");
                }
                if f & 0x400_0000 != 0 {
                    return unsupported("BossGenerateEnemy effect");
                }
                let _ = hit_at;
            }
        }
        if sub(self.fx(i).f32_(fx::END), f32::from_bits(0x3d08_7a8d))? <= atime {
            let f = self.fx(i).u32_(fx::FLAGS);
            if f & 0x8_0000 == 0 {
                let next = self.fx(i).i16_(fx::NEXT_FX) as i32;
                if f & 0x4000 == 0 || 0xd8 < (next - 1) as u32 {
                    if f & 0x600_0000 != 0 || self.fx(i).u32_(0xec) != 0 {
                        return unsupported("effect end item/boss generation");
                    }
                    self.delete_effect(i, true);
                    return Ok(());
                }
                return unsupported("chained effect (0x4000)");
            }
        }
        if moved && let Some(n) = self.effects.nodes[i].as_mut() {
            n.matrix = m;
            n.position = to;
        }
        Ok(())
    }
}

/// Round 0x1d4cd8: x ± 0.5 then truncate.
pub fn round(x: f32) -> R<i32> {
    let y = if 0.0 <= x { add(x, 0.5)? } else { add(x, -0.5)? };
    Ok(y as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smallsqrt_pieces_follow_the_source_constants() {
        // Synthetic: each branch boundary of smallsqrt 0x1d6bd0.
        assert_eq!(smallsqrt(0.0001).unwrap(), 0.0);
        let one = smallsqrt(1.0).unwrap();
        assert!(0.9999 < one && one < 1.0);
        let quarter = smallsqrt(0.25).unwrap();
        assert!((quarter - 0.5).abs() < 0.001);
        assert!((smallsqrt(0.1).unwrap() - 0.282_84).abs() < 1e-5);
    }

    #[test]
    fn round_adds_half_away_from_zero_then_truncates() {
        assert_eq!(round(2.5).unwrap(), 3);
        assert_eq!(round(-2.5).unwrap(), -3);
        assert_eq!(round(13.4).unwrap(), 13);
    }

    #[test]
    fn calc_target_dir_normalises_xz_and_lifts_y() {
        let mut d = [0.0, 0.0, 20.0];
        calc_target_dir(44.0, 1.0 / 44.0, 8.0, -0.5, &mut d).unwrap();
        assert_eq!(d[0], 0.0);
        assert_eq!(d[2], 1.0);
        assert!(0.0 < d[1]);
    }
}
