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
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EffectHeader {
    /// 0 = WEAPONS/ANIM.PS2 (weapon_anim), 1 = POWERUPS/ANIM.PS2 (powerup_anim),
    /// 2 + n = the model of critter file n (InitCustomEffect), 0x40 + p =
    /// player p's SFX package (PlayerModelData +0x48), 0x80 + n = the
    /// level's n-th item package (item_anim, item_anim2), 0xc0 + t = enemy
    /// type t's package (enemy_anim[t]).
    pub source: u8,
    pub tree: usize,
    pub frames: i16,
    pub timing: i16,
    /// The first sequence's repeat flag: InitAnim's AnimInfo loop word
    /// (effect +0x50) that StartFXTree keeps on its frame-count path.
    pub repeat: u16,
}

/// Native render node + animation tree of a slot (presentation reads it).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EffectNode {
    /// Rows of the node's rotation; position is the translation row.
    pub matrix: [V3; 3],
    pub position: V3,
    pub scale: f32,
    /// ScaleFX's per-axis factors beyond `scale` (StartShieldFX: x/z only).
    pub stretch: V3,
    pub visual: Visual,
    /// ATime when the current tree started (presentation clock).
    pub start: f32,
    /// Current blast radius of an area effect (presentation reads it).
    pub radius: f32,
    /// SfxSetParent to a player's node (psfx.rs): the effect follows it.
    pub parent: Option<EffectParent>,
    /// SfxSetHitTarget's node (+0xe4): (critter, node index).
    pub target: Option<(usize, usize)>,
    /// MBTreeSetColor's 0xRRGGBB (node +0x64, flag 0x100) and
    /// MBTreeSetAlpha's value (0 opaque … 0xff transparent; node +0x53 =
    /// ~value): presentation.
    pub tint: Option<u32>,
    pub alpha: Option<i32>,
    /// MBTreeSetAmbientAdd's value (0..0x1ff; presentation).
    pub ambient: Option<i32>,
    /// MBNodeSetParent to another effect's node with an identity local
    /// matrix (PlayerStartMissile's weapon-powerup throw effect on its
    /// missile): drawn at that node, scaled with it.
    pub follow: Option<usize>,
    /// SfxSetStreak's poly instance (+0xd4): presentation.
    pub streak: Option<Streak>,
}

/// SfxSetStreak 0x17e0f0's MBNewPoly(0, 4, WeaponStreakTex — WEAPONS'
/// WEP_STREAK) with MBPolyInstSetColorAlpha(colour, alpha) — GS alpha
/// (0xff − alpha) >> 1 — and UpdateFXStreak 0x17ae48's four vertices (UV
/// (0,0) (1,0) (1,1) (0,1), a triangle fan: textured, Gouraud, blended).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Streak {
    pub colour: u32,
    pub alpha: u8,
    pub verts: [V3; 4],
}

/// An effect node parented to a player node, or (CritterDoSfxSub's
/// SfxSetParent) to a critter's node — `critter` = (instance, tree node; None
/// for the instance's own +0x70 node): local position and rotation.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EffectParent {
    pub player: usize,
    pub local: V3,
    pub local_matrix: [V3; 3],
    pub critter: Option<(usize, Option<usize>)>,
    /// An enemy's node (+0x70; DoEnemyAction's GRM attack effects): its
    /// record matrix (rows +0x10..+0x30, position +0x40) stands for the
    /// node (HYPOTHESIS as for players).
    pub enemy: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum Visual {
    /// PlayerMissileTreeInfo[player] (the class missile tree).
    Missile { player: usize },
    /// EffectHeaders[index].
    Header(usize),
    /// EnemyMissileTree[type][kind] ("<PREFIX>_ARROW/BOMB/FBALL").
    EnemyMissile { t: i32, kind: u8 },
    /// FamiliarSpit[player] (the class SFX package's FAMILIAR_SPIT) or, for
    /// the phoenix, weapon_anim's PHOENIX_FBALL.
    Familiar { player: usize, phoenix: bool },
    /// A tree outside EffectHeaders (AtreeMatch at InitPlayerMissiles:
    /// BallistaTree SUPERARROW, BossElecTree BOSSG_ELEC, BossAcidTree
    /// BOSSG_ACID of weapon_anim) or EffectHeaders[0]'s tree as a missile.
    Tree(EffectHeader),
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
    /// MagicEffects 0x2407a0: EffectHeaders index by potion colour (0..4).
    pub magic_effects: [i32; 5],
    /// ShieldEffects 0x2407d0 and ThrowMagicEffects 0x2407b8 by colour.
    pub shield_effects: [i32; 5],
    pub throw_magic_effects: [i32; 5],
    /// mclr_idx 0x23fa60 (colour → light_color row) and light_color
    /// 0x23f9f8 (7 rgb rows).
    pub mclr_idx: [usize; 5],
    pub light_color: [V3; 7],
    /// pclr_idx 0x23fa50: player colour → light_color row.
    pub pclr_idx: [usize; 4],
    /// EnemyMissileInfo 0x24de80: 28 types × 3 kinds × 12 words.
    pub enemy_missile_info: Vec<[u32; 12]>,
    /// EnemyMissileDesc 0x24de68: "ARROW", "BOMB", "FBALL".
    pub enemy_missile_desc: [String; 3],
    /// Tree labels of weapon_anim/powerup_anim for InitCustomEffect (exact match).
    pub shared: Vec<EffectHeaderLabel>,
    /// Source ids of item_anim / item_anim2 (set_item_role).
    pub item_sources: [Option<u8>; 2],
    /// BallistaMissileInfo 0x24ee40, BossElecMissileInfo 0x24ee70,
    /// BossAcidMissileInfo 0x24eea0 (12 words each).
    pub ballista_info: [u32; 12],
    pub boss_elec_info: [u32; 12],
    pub boss_acid_info: [u32; 12],
    /// pojo_offset 0x24ef00: the +0x138 0x400 missile origin offset.
    pub pojo_offset: V3,
    /// DmgTypeDesc 0x24de40: "", "RED", "BLU", "YEL", "GRE" (weapon
    /// powerup types 0..4).
    pub dmg_type_desc: [String; 5],
}

#[derive(Debug, Clone)]
pub struct EffectHeaderLabel {
    pub label: String,
    pub header: EffectHeader,
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
                    EffectHeader { source, tree: i, frames: s.map_or(0, |s| s.frames), timing: s.map_or(0, |s| s.timing_factor), repeat: s.map_or(0, |s| s.repeat) }
                })
            };
            *h = find(weapon_anim, 0).or_else(|| find(powerup_anim, 1));
        }
        let m = words(elf.object("MagicEffects")?);
        let magic_effects: [i32; 5] = std::array::from_fn(|k| m.get(k).copied().unwrap_or(0) as i32);
        let five = |n: &str| -> Result<[i32; 5], Box<dyn std::error::Error>> {
            let w = words(elf.object(n)?);
            Ok(std::array::from_fn(|k| w.get(k).copied().unwrap_or(0) as i32))
        };
        let shield_effects = five("ShieldEffects")?;
        let throw_magic_effects = five("ThrowMagicEffects")?;
        let mclr_idx = five("mclr_idx")?.map(|v| v.clamp(0, 6) as usize);
        let pw = words(elf.object("pclr_idx")?);
        let pclr_idx: [usize; 4] = std::array::from_fn(|k| (pw.get(k).copied().unwrap_or(0) as i32).clamp(0, 6) as usize);
        let lc = words(elf.object("light_color")?);
        let light_color: [V3; 7] = std::array::from_fn(|k| std::array::from_fn(|c| f32::from_bits(lc.get(3 * k + c).copied().unwrap_or(0))));
        let em = words(elf.object("EnemyMissileInfo")?);
        let enemy_missile_info = em.chunks_exact(12).map(|c| c.try_into().unwrap()).collect();
        let d = elf.object("EnemyMissileDesc")?;
        let enemy_missile_desc = std::array::from_fn(|k| {
            let raw = &d[8 * k..8 * k + 8];
            String::from_utf8_lossy(&raw[..raw.iter().position(|&b| b == 0).unwrap_or(8)]).into_owned()
        });
        let twelve = |n: &str| -> Result<[u32; 12], Box<dyn std::error::Error>> {
            let w = words(elf.object(n)?);
            w.get(..12).and_then(|w| w.try_into().ok()).ok_or_else(|| format!("{n} size").into())
        };
        let (ballista_info, boss_elec_info, boss_acid_info) = (twelve("BallistaMissileInfo")?, twelve("BossElecMissileInfo")?, twelve("BossAcidMissileInfo")?);
        let po = words(elf.object("pojo_offset")?);
        let pojo_offset: V3 = std::array::from_fn(|k| f32::from_bits(po.get(k).copied().unwrap_or(0)));
        let dt = elf.object("DmgTypeDesc")?;
        let dmg_type_desc = std::array::from_fn(|k| {
            let raw = dt.get(8 * k..8 * k + 8).unwrap_or(&[]);
            String::from_utf8_lossy(&raw[..raw.iter().position(|&b| b == 0).unwrap_or(raw.len())]).into_owned()
        });
        let mut shared = Vec::new();
        for (source, a) in [(0u8, weapon_anim), (1, powerup_anim)] {
            for (k, t) in a.trees.iter().enumerate() {
                let s = t.sequences.first();
                shared.push(EffectHeaderLabel {
                    label: t.label.clone(),
                    header: EffectHeader { source, tree: k, frames: s.map_or(0, |s| s.frames), timing: s.map_or(0, |s| s.timing_factor), repeat: s.map_or(0, |s| s.repeat) },
                });
            }
        }
        Ok(Self {
            missile_info,
            wall_hit,
            spread,
            exp,
            headers,
            magic_effects,
            shield_effects,
            throw_magic_effects,
            mclr_idx,
            light_color,
            pclr_idx,
            enemy_missile_info,
            enemy_missile_desc,
            shared,
            item_sources: [None; 2],
            ballista_info,
            boss_elec_info,
            boss_acid_info,
            pojo_offset,
            dmg_type_desc,
        })
    }

    /// The level's item packages (item_anim, item_anim2), searched after
    /// weapon_anim and powerup_anim (InitCustomEffectSub 0x179aa8).
    /// The item package source ids (0x80 + n) of item_anim (the realm's,
    /// loaded while BossType < 0) and item_anim2 (the level's), for
    /// InitEffects' AtreeMatch in one of them.
    pub fn set_item_role(&mut self, role: usize, n: u8) {
        if let Some(r) = self.item_sources.get_mut(role) {
            *r = Some(0x80 + n);
        }
    }

    /// AtreeMatch(item_anim (role 0) or item_anim2 (1), name).
    pub fn item_header(&self, role: usize, name: &str) -> Option<EffectHeader> {
        let src = self.item_sources.get(role).copied().flatten()?;
        self.shared.iter().find(|h| h.header.source == src && h.label == name).map(|h| h.header)
    }

    pub fn add_item_package(&mut self, n: u8, anim: &Animations) {
        for (k, t) in anim.trees.iter().enumerate() {
            let s = t.sequences.first();
            self.shared.push(EffectHeaderLabel {
                label: t.label.clone(),
                header: EffectHeader { source: 0x80 + n, tree: k, frames: s.map_or(0, |s| s.frames), timing: s.map_or(0, |s| s.timing_factor), repeat: s.map_or(0, |s| s.repeat) },
            });
        }
    }

    /// AtreeMatch over weapon_anim, powerup_anim, then the item packages
    /// (strcmp).
    pub fn shared_header(&self, name: &str) -> Option<EffectHeader> {
        self.shared.iter().find(|h| h.label == name).map(|h| h.header)
    }

    /// AtreeMatch(weapon_anim, name).
    pub fn weapon_tree(&self, name: &str) -> Option<EffectHeader> {
        self.shared.iter().find(|h| h.header.source == 0 && h.label == name).map(|h| h.header)
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
    /// NumCustomEffects (InitCustomEffectSub).
    pub custom_effects: usize,
    /// WeapThrowFx 0x24f148: per player, the custom effect of each weapon
    /// powerup type ("WEAP_TW_<c>" of DmgTypeDesc; −1 for type 0 or when
    /// missing), set by InitPlayerMissiles.
    pub weap_throw_fx: [[i32; 5]; 4],
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
            custom_effects: 0,
            weap_throw_fx: [[-1; 5]; 4],
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

/// CreateDirMatrix 0x1d60a0(m, v, up): |v|² < 0.001 → identity rows;
/// without `up` CreateYPRMatrix(atan2f(vy, fqdist(vx, vz)) × 0.5,
/// atan2f(vx, vz), 0); otherwise row 2 = SlowNormalVector(v), row 1 =
/// SlowNormalVector(row 2 × up), row 0 = row 1 × row 2. ProcessEffects
/// passes Cam[0]'s row 2 (0x23e520: the camera's view direction, equal to the
/// window camera rows in the fixture), which rolls a missile's rows toward
/// the camera (trace-fight-g1-v4).
fn create_dir_matrix(v: V3, up: Option<V3>) -> R<[V3; 3]> {
    if add(add(mul(v[0], v[0])?, mul(v[1], v[1])?)?, mul(v[2], v[2])?)? < 0.001 {
        return Ok([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    }
    let Some(u) = up else {
        let h = fqdist(v[0], v[2])?.0;
        let pitch = mul(atan2f(v[1], h)?, 0.5)?;
        let m = super::enemy_ai::create_ypr_matrix([pitch, atan2f(v[0], v[2])?, 0.0])?;
        return Ok([[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]]);
    };
    let mut f = v;
    super::critter_ai::slow_normal_vector(&mut f)?;
    let mut r1 = [sub(mul(f[1], u[2])?, mul(f[2], u[1])?)?, sub(mul(f[2], u[0])?, mul(f[0], u[2])?)?, sub(mul(f[0], u[1])?, mul(f[1], u[0])?)?];
    super::critter_ai::slow_normal_vector(&mut r1)?;
    let r0 = [sub(mul(r1[1], f[2])?, mul(r1[2], f[1])?)?, sub(mul(r1[2], f[0])?, mul(r1[0], f[2])?)?, sub(mul(r1[0], f[1])?, mul(r1[1], f[0])?)?];
    Ok([r0, r1, f])
}

impl Game {
    pub(crate) fn fx(&self, i: usize) -> &Record {
        &self.effects.slots[i]
    }
    pub(crate) fn fx_mut(&mut self, i: usize) -> &mut Record {
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

    /// SfxDeleteParented 0x17aa38(enemy node, 0, −1) for enemy `e`: every
    /// effect whose node's parent is the enemy's (non-recursive).
    pub(crate) fn delete_enemy_parented(&mut self, e: usize) {
        for k in 0..self.effects.num {
            if self.effects.nodes[k].and_then(|n| n.parent).is_some_and(|p| p.enemy == Some(e)) && 0.0 < self.fx(k).f32_(0x68) {
                self.delete_effect(k, true);
            }
        }
    }

    /// StartEnemyAtkFX 0x17f5e8(position 0, k): StartFXSub(0, 0x56 + k,
    /// none, 0, 0x800) when the header exists (only the GRM's ATK1FX /
    /// ATK2FX are loaded), then SfxSetParent to enemy `e`'s node.
    pub(crate) fn start_enemy_atk_fx(&mut self, e: usize, k: usize) -> R<()> {
        let slot = self.start_fx_sub(0.0, 0x56 + k, None, 0)?;
        if let Ok(s) = usize::try_from(slot)
            && let Some(n) = self.effects.nodes[s].as_mut()
        {
            n.parent = Some(EffectParent { player: 0, local: [0.0; 3], local_matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], critter: None, enemy: Some(e) });
        }
        Ok(())
    }

    /// DeleteEffect(slot, force).
    pub(crate) fn delete_effect_pub(&mut self, i: usize) {
        self.delete_effect(i, true);
    }
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
        // +0x10: SfxSetLight's radius.
        r.set_f32(0x10, 0.0);
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
    pub(crate) fn start_fx_tree(&mut self, time: f32, visual: Visual, frames: i16, timing: i16, position: Option<V3>, flags: u32) -> R<usize> {
        let i = self.find_effect_idx();
        let atime = self.globals.atime;
        // +0x50 is the AnimInfo loop word: AtreeInitSub's InitAnim leaves
        // the first sequence's repeat flag there, and StartFXTree overwrites
        // it only on its time and zero-frame paths (0x17d7b0;
        // trace-b6-boss-v3 step 408, the dragon's header 102).
        let mut looping = match visual {
            Visual::Header(h) => self.effects.tables.headers.get(h).copied().flatten().map_or(0, |x| x.repeat),
            _ => 0,
        };
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
            stretch: [1.0; 3],
            visual,
            start: atime,
            radius: 0.0,
            parent: None,
            target: None,
            tint: None,
            ambient: None,
            alpha: None,
            follow: None,
            streak: None,
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
    fn start_missile(&mut self, speed: f32, damage: f32, owner: i32, position: V3, dir: V3, flags: u32, info: &[u32; 12], index: i32, sfx_flags: u32, visual: Visual) -> R<usize> {
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
        let i = self.start_fx_tree(time, visual, 0, 0, Some(position), f as u32 | 0x100_0000)?;
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
            // SfxSetPhysics: YawMat3(atan2f(vx, vz)) on StartFXTree's identity.
            let mut m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
            crate::collision::yaw_mat3(&mut m, yaw).map_err(|e| Unsupported(format!("YawMat3: {e:?}")))?;
            n.matrix = m;
            if flags & 0x200_0000 != 0 {
                n.scale = f32::from_bits(0x3fe6_6666);
            }
        }
        // SfxSetStreak 0x17e0f0 (owner > 0): flag 0x40000, +0xd8 the class
        // record's +0x17c, +0xdc the radius (its time argument is 0); the
        // colour streak_color 0x24eed0[player colour +4] and alpha
        // streak_alpha 0x24eee0[player +8] (+0x40 for flag 0x2000000, at
        // most 0xff), white at 0x40 for flags & 0x2100000 == 0x100000.
        if 0 < owner {
            let raw = &self.classes[(owner - 1) as usize].raw;
            let width = raw.get(0x17c..0x180).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
            let r = self.fx_mut(i);
            r.or_u32(fx::FLAGS, 0x4_0000);
            r.set_f32(0xd8, width);
            let rad = r.f32_(fx::RADIUS);
            r.set_f32(0xdc, rad);
            const STREAK_COLOR: [u32; 4] = [0xff_ff00, 0xff, 0xff_0000, 0xff00];
            const STREAK_ALPHA: [u32; 8] = [64, 64, 192, 64, 64, 64, 192, 64];
            let pl = &self.players[(owner - 1) as usize];
            let (colour, alpha) = if flags & 0x210_0000 == 0x10_0000 {
                (0xff_ffff, 0x40)
            } else {
                let mut a = STREAK_ALPHA.get(pl.u32_(8) as usize).copied().unwrap_or(64);
                if flags & 0x200_0000 != 0 {
                    a = (a + 0x40).min(0xff);
                }
                (STREAK_COLOR.get(pl.u32_(4) as usize).copied().unwrap_or(0xff_ffff), a)
            };
            if let Some(n) = self.effects.nodes[i].as_mut() {
                n.streak = Some(Streak { colour, alpha: ((0xff - alpha) >> 1) as u8, verts: [[0.0; 3]; 4] });
            }
        }
        Ok(i)
    }

    /// InitPlayerMissiles 0x18efe0's WeapThrowFx: InitCustomEffect(the
    /// player's SFX package, "WEAP_TW_<c>") for DmgTypeDesc types 1..4 (the
    /// throw trees, WEAP_HOLD_<type>, the familiar and breath trees are
    /// presentation looked up by the host).
    pub fn init_player_missiles(&mut self, p: usize) {
        for t in 0..5 {
            let desc = self.effects.tables.dmg_type_desc[t].clone();
            let Some(c) = desc.chars().next() else {
                self.effects.weap_throw_fx[p][t] = -1;
                continue;
            };
            let name = format!("WEAP_TW_{c}");
            let own = self.classes[p].sfx_trees.iter().position(|(l, _, _, _)| *l == name).map(|k| {
                let (_, frames, timing, repeat) = self.classes[p].sfx_trees[k];
                EffectHeader { source: 0x40 + p as u8, tree: k, frames, timing, repeat }
            });
            self.effects.weap_throw_fx[p][t] = self.register_custom_effect(own, &name);
        }
    }

    /// PlayerStartMissile(distance, power, player, direction, flags, origin mode).
    pub fn player_start_missile(&mut self, distance: f32, power: f32, player: usize, dir: &mut V3, flags: u32, origin_mode: u32) -> R<i32> {
        let pl = self.players[player].clone();
        let r138 = pl.u32_(0x138);
        let class = pl.u32_(8) as usize;
        let me = pl.u32_(0) as usize;
        let ballista = flags & 0x210_0000 == 0x10_0000;
        // The missile info: BossElec (+0x138 0x8000), BossAcid (0x4000), the
        // ballista (flag 0x100000 without 0x2000000), else the class's.
        let t = &self.effects.tables;
        let (info, special) = if r138 & 0x8000 != 0 {
            (t.boss_elec_info, true)
        } else if r138 & 0x4000 != 0 {
            (t.boss_acid_info, true)
        } else if ballista {
            (t.ballista_info, true)
        } else {
            (*t.missile_info.get(class).ok_or_else(|| Unsupported(format!("missile class {class}")))?, false)
        };
        let damage = mul(power, pl.f32_(0x128))?;
        let offset = if r138 & 0x400 != 0 {
            Some(self.effects.tables.pojo_offset)
        } else {
            match origin_mode {
                1 => Some(self.classes[player].missile_origin_5c),
                2 => Some(self.classes[player].missile_origin_158),
                _ => None,
            }
        };
        // MulVecMat3(offset, out, player matrix +0x20).
        let m = [pl.v3(0x20), pl.v3(0x30), pl.v3(0x40)];
        let mut o = [0.0f32; 3];
        if let Some(offset) = offset {
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
                    self.add_wall_hit_col(h, flags)?;
                    return Ok(-1);
                }
                start = back;
            }
        }
        if let Some((it, h)) = self.weapon_item_collide(radius, back, start, false, player as i32)? {
            let d = self.damage_item(damage, it, flags, player as i32)?;
            if 0.0 <= d {
                self.player_damaged_item(player, it, d == 0.0)?;
                self.add_item_hit_col(h, flags, d == 0.0)?;
            } else {
                self.add_wall_hit_col(h, flags)?;
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
        // The missile tree; the special ones add effect flag 0x10000.
        let tree_of = |g: &Game, label: &str| -> R<Visual> {
            g.effects.tables.weapon_tree(label).map(Visual::Tree).ok_or_else(|| Unsupported(format!("weapon_anim tree {label}")))
        };
        let (mut visual, sfx_flags) = if r138 & 0x8000 != 0 {
            (tree_of(self, "BOSSG_ELEC")?, 0x1_0000)
        } else if r138 & 0x4000 != 0 {
            (tree_of(self, "BOSSG_ACID")?, 0x1_0000)
        } else if r138 & 0x400 != 0 {
            // FamiliarSpit[4]: weapon_anim's PHOENIX_FBALL.
            (Visual::Familiar { player, phoenix: true }, 0x1_0000)
        } else if ballista {
            (tree_of(self, "SUPERARROW")?, 0)
        } else {
            (Visual::Missile { player }, 0)
        };
        let wtype = (flags & 0xf) as usize;
        let throw_fx = self.effects.weap_throw_fx.get(me).and_then(|w| w.get(wtype)).copied().unwrap_or(-1);
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
            // Classes 2/6 with a throw effect and the class info fly
            // EffectHeaders[0]'s tree under it.
            if (class == 2 || class == 6) && 0 <= throw_fx && !special {
                if let Some(h) = self.effects.tables.headers.first().copied().flatten() {
                    visual = Visual::Tree(h);
                } else {
                    return unsupported("effect header 0 as a missile tree");
                }
            }
            let mut slot = self.start_missile(speed, damage, me as i32 + 1, start, d, flags, &info, k, sfx_flags, visual)? as i32;
            if !special && 0 <= throw_fx {
                // StartFXSub(0, WeapThrowFx, 0, 0, 0x881880 / 0x81880) on the
                // missile: linked (+0xe8) and parented while the missile lives.
                let fx = self.start_fx_sub(0.0, throw_fx as usize, None, 0)?;
                let mut keep = false;
                if fx != slot {
                    if 0.0 < self.fx(slot as usize).f32_(fx::END) {
                        if 0 <= fx {
                            self.fx_mut(slot as usize).set_i16(fx::LINK, fx as i16);
                            if let Some(n) = self.effects.nodes[fx as usize].as_mut() {
                                n.follow = Some(slot as usize);
                            }
                        }
                        keep = true;
                    } else {
                        self.delete_effect(slot as usize, true);
                        slot = fx;
                    }
                }
                if !keep {
                    if 0 <= slot {
                        self.delete_effect(slot as usize, true);
                    }
                    slot = -1;
                }
            }
            if 0 <= slot {
                // PlayerMissileTreeInfo +4's MBTreeSetFlags: presentation.
                let mut scale = 1.0;
                if 1.0 < power {
                    scale = mul(power, 1.0)?;
                }
                if 0x62 < pl.i32_(0x2ac0) {
                    scale = mul(scale, f32::from_bits(0x3f99_999a))?;
                }
                if scale != 1.0
                    && let Some(n) = self.effects.nodes[slot as usize].as_mut()
                {
                    n.scale = mul(n.scale, scale)?;
                }
            }
            self.effects.pmissile[k as usize] = slot;
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
        let (hit, water) = self.weapon_wall_hit(r, from, to)?;
        Ok((hit.map(|h| h.position), hit.map(|h| (h.object, water))))
    }

    /// WeaponWallCollide keeping the hit (wall_col: normal for ReflectVector).
    fn weapon_wall_hit(&mut self, r: f32, from: V3, to: V3) -> R<(Option<crate::collision::Hit>, bool)> {
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
        Ok((c.solid, c.water.is_some()))
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
                        let at = self.items.records[slot].v3(0x40);
                        self.audio_rock_fall(at)?;
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
    /// Kinds 1 and 2 end in the shared tail 0x17b1dc: an indestructible
    /// item (+0xe3 = −1) is skipped unless the damage flags have 0x600
    /// (trace-b1-newgame-v2 step 278: a blast and the KEY).
    fn sfx_skip_item(&self, slot: usize, flags: u32, dflags: u32) -> u8 {
        let r = &self.items.records[slot];
        let d = &self.items.definitions[r.definition()];
        let st = d.subtype();
        let indestructible = r.i8_(0xe3) == -1;
        let tail = |v: u8| if indestructible && dflags & 0x600 == 0 { 1 } else { v };
        match d.kind() {
            1 => tail(if flags & 0x100 == 0 {
                0
            } else if dflags & 0x80_0000 == 0 || !(st == 3 || st == 1) {
                1
            } else {
                2
            }),
            2 if st == 0x2b => tail(u8::from(flags & 0x1000 != 0)),
            2 => tail(0),
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
    pub fn player_get_target(&mut self, p: usize, origin: V3, facing: V3, out: &mut V3, target: &mut i32, item: &mut Option<usize>) -> R<f32> {
        let range = if self.boss_crit_inst != 0 { 200.0 } else { 30.0 };
        let mut d = range;
        let pl = self.players[p].clone();
        // fStack_f0: the given target's position (stack residue otherwise;
        // see the kind-7 check below).
        let mut target_pos: Option<V3> = None;
        if (0..0x10000).contains(target) {
            let en = &self.enemies[*target as usize];
            if en.i32_(e::TYPE) != 0x1f {
                target_pos = Some(en.v3(e::COLLISION));
                *out = v_sub(en.v3(e::COLLISION), origin)?;
                d = sub(normal_vector(out)?, en.f32_(e::WIDTH))?;
                if (0.0 < d || en.i32_(e::TYPE) == 0x1e) && dot3(*out, facing)? < 0.5 {
                    d = range;
                }
            }
        } else if 0x10000 <= *target {
            let k = (*target & 0xffff) as usize;
            if self.critters.get(k).is_some_and(Option::is_some) {
                target_pos = Some(self.crit(k).rec.v3(super::critters::c::COL));
                d = self.critter_get_target_sub(f32::from_bits(0x3f34_fdf4), 0.0, k, origin, facing, out)?;
            }
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
            if let Some((k, dc, v)) = self.critter_get_target(cone, range, origin, facing)?
                && dc < d
            {
                *out = v;
                *target = k as i32 | 0x10000;
                d = dc;
            }
            let (dg, generator, gv) = self.closest_generator(cone, range, origin, facing)?;
            *item = generator;
            if dg < d {
                *out = gv.unwrap_or(*out);
                *target = -1;
                d = dg;
            }
        }
        // Versus (GamePrefs mode 2): another player in state 1 within the
        // 0.707 cone shortens the distance (its +0x828 radius off); `out`
        // keeps the last examined player's direction, the target stays.
        if self.globals.pvp_mode == 2 && range <= d {
            let mut best = d;
            for k in 0..4 {
                if k == p || self.players[k].i32_(0xfc) != 1 {
                    continue;
                }
                *out = v_sub(self.players[k].v3(0x50), origin)?;
                let dist = normal_vector(out)?;
                if f32::from_bits(0x3f34_fdf4) <= dot3(*out, facing)? {
                    let dk = sub(dist, self.players[k].f32_(0x828))?;
                    if dk < best {
                        best = dk;
                    }
                }
            }
            d = best;
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
        // A target behind the player's kind-7 item (+0x880): ItemCollSub
        // from the origin to 2 × radius towards the target's position.
        if 0 <= *target
            && let Some(it) = pl.u32_(0x880).checked_sub(1).map(|v| v as usize)
            && self.items.definitions[self.items.records[it].definition()].kind() == 7
        {
            // Without a given target fStack_f0 (sp+0x20, PlayerMotion's
            // sp − 0xf0) holds stack residue. PlayerMotion's last callee to
            // reach it is usually PlayerCollideFloor's FloorCollide, whose
            // saved ra (a code address: a denormal, 0 on the EE FPU) and its
            // zero upper half give x and y; z is older residue. HYPOTHESIS:
            // the world origin (trace-A1-death-v1 steps 105..113 match).
            let tp = target_pos.unwrap_or([0.0; 3]);
            let mut v = v_sub(tp, origin)?;
            normal_vector(&mut v)?;
            let r = pl.f32_(0x828);
            let k = add(r, r)?;
            let probe = [add(mul(k, v[0])?, origin[0])?, add(mul(k, v[1])?, origin[1])?, add(mul(k, v[2])?, origin[2])?];
            let env = self.collide_env();
            let c = crate::item_collision::item_coll_sub(&self.items, &mut self.collision, &env, it, origin, probe, None, r, pl.f32_(0x82c))?;
            if 0.0 <= c {
                *target = -1;
                d = range;
            }
        }
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
        self.audio_throw_weapon(p)?;
        Ok(n)
    }

    pub fn player_damaged_enemy_pub(&mut self, p: usize, i: usize, prior: i32, result: i32, attack: bool) -> R<()> {
        self.player_damaged_enemy(p, i, prior, result, attack)
    }
    pub fn player_damaged_item_pub(&mut self, p: usize, slot: usize, destroyed: bool) -> R<()> {
        self.player_damaged_item(p, slot, destroyed)
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
        self.add_exp(p, amount, i32::from(attack))?;
        Ok(())
    }

    /// PlayerDamagedItem(player, item, destroyed).
    fn player_damaged_item(&mut self, p: usize, slot: usize, destroyed: bool) -> R<()> {
        let r = &self.items.records[slot];
        let d = &self.items.definitions[r.definition()];
        match d.kind() {
            1 if r.i16_(0xda) < 1 && d.subtype() == 4 => {
                // start_magic(0.8, player, item +0x50, definition +0x3c, 0),
                // NewHelp(0xe), DeleteItem(item, 1).
                let (at, fl) = (r.v3(0x50), d.i32_(0x3c) as u32);
                self.start_magic(f32::from_bits(0x3f4c_cccd), p as i32, at, fl, 0)?;
                self.help(0xe, p);
                self.delete_item(slot, true);
                Ok(())
            }
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
                self.add_exp(p, amount, 0)?;
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
        let (mut result, mut shown);
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
            1 => {
                // Magic (flags 0x400) of at least 5 damage.
                let magic = flags & 0x400 != 0 && 5.0 <= dmg;
                let item_pos = self.items.records[slot].v3(0x40);
                match st {
                    1 => {
                        if magic {
                            // StartFXMat 0x20 / 0x21; treasure becomes junk.
                            self.start_fx_no_loop(0x20, item_pos)?;
                            self.start_fx_no_loop(0x21, item_pos)?;
                            self.replace_item_atree(slot, "TREAS_JUNK");
                            self.items.records[slot].set_i32(0xf4, 10);
                        }
                    }
                    2 | 10..=0x10 => {}
                    4 => {
                        if result == 0 {
                            // start_magic(0.8, -1, item +0x50, definition +0x3c, 0).
                            let r = &self.items.records[slot];
                            let (at, fl) = (r.v3(0x50), self.items.definitions[r.definition()].i32_(0x3c) as u32);
                            self.start_magic(f32::from_bits(0x3f4c_cccd), -1, at, fl, 0)?;
                            self.delete_item(slot, true);
                        }
                    }
                    _ => {
                        if st == 3 && result == -2 {
                            // Shot food spoils: BADMEAT (meat, +0xe4 == 2) or GAPPLE.
                            let (tree, value) = if self.items.records[slot].i16_(0xe4) == 2 { ("BADMEAT", -100) } else { ("GAPPLE", -0x32) };
                            self.replace_item_atree(slot, tree);
                            self.items.records[slot].set_i32(0xf4, value);
                            self.help(0x88, usize::MAX);
                        }
                        if magic {
                            self.start_fx_no_loop(0x20, item_pos)?;
                            self.start_fx_no_loop(0x21, item_pos)?;
                            // MBOX_NewObject("ITEMEXP0"): presentation.
                            self.delete_item(slot, true);
                            self.help(0x87, usize::MAX);
                        }
                    }
                }
            }
            2 => {
                let content = self.items.records[slot].i16_(0xf0);
                let holds_death = 0 <= content && self.items.definitions.get(content as usize).is_some_and(|d| self.items.enemy_desc_type(d.name()) == 0x1e);
                if holds_death && flags & 0x200 != 0 && st != 0x2b {
                    // Magic kills the Death inside: the chest DEFINITION's
                    // subtype (+4, shared data) becomes 1, the content APPLE
                    // (kind 1, subtype 3), AudioDeathDies, +0xf2 = damage × 3;
                    // the hit spark shows.
                    self.items.definitions[def].set_i32(4, 1);
                    let apple = self.items.find_info_index(1, 3, b"APPLE").ok_or_else(|| Unsupported("APPLE item".into()))?;
                    self.items.records[slot].set_i16(0xf0, apple as i16);
                    self.sfx3(0x65, at, 0x7f, 0x16)?;
                    self.items.records[slot].set_i16(0xf2, mul(dmg, 3.0)? as i32 as i16);
                    shown = true;
                } else if killed && self.items.records[slot].u16_(0xd8) & 0x200 != 0 {
                    if self.items.records[slot].u16_(0xd8) & 1 == 0 {
                        self.items.records[slot].or_u16(0xd8, 1);
                        self.open_chest(slot, None)?;
                        if st == 0x2b {
                            self.audio_wood_barrel(at)?;
                            self.events.push(Event::Help { id: 0x1b, player: usize::MAX });
                        }
                    }
                } else if flags & 0x400 != 0 && 5.0 <= dmg {
                    // A blast on a chest: the time-bomb chest (0x2c) opens
                    // (+0xde = +0xdc = 2); otherwise a Death chest releases
                    // it, then the chest bursts (FX 0x1f, 0x21; debris
                    // objects are presentation) and is deleted.
                    if st == 0x2c {
                        let r = &mut self.items.records[slot];
                        r.set_u8(0xde, 2);
                        r.set_u8(0xdc, 2);
                        r.or_u16(0xd8, 1);
                        self.open_chest(slot, None)?;
                    } else {
                        if holds_death {
                            self.open_chest(slot, None)?;
                        }
                        // StartFXMat(…, item matrix): the translation +0x40.
                        let item_pos = self.items.records[slot].v3(0x40);
                        self.start_fx_no_loop(0x1f, item_pos)?;
                        self.start_fx_no_loop(0x21, item_pos)?;
                        self.delete_item(slot, true);
                    }
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
                        // ItemGenDesc 0x151308 then AtreeMatchAnyHeader /
                        // ItemFindMBObjectL1 (0x15ac7c..): the new level's
                        // tree replaces the item's, else its model
                        // (presentation); with neither DeleteItem(item, 0)
                        // (trace-A1-statue-v2 step 239: no GEN_LOW0 model, so
                        // the masked +0xd8 below reads 0xfffe).
                        let name = self.item_gen_desc(slot);
                        if self.item_anim_lib.contains_key(&name) {
                            self.replace_item_atree(slot, &name);
                            self.events.push(Event::ItemModel { slot, level });
                        } else if self.mb_object_exists(&name)? || self.mb_object_exists(&format!("{name}L1"))? || self.mb_object_exists(&format!("{name}L1ROOT"))? {
                            self.events.push(Event::ItemModel { slot, level });
                        } else {
                            self.delete_item(slot, false);
                        }
                        if level == 0 {
                            self.items.records[slot].and_u16(0xd8, 0xfffe);
                            self.items.records[slot].set_i8(0xe3, -1);
                        }
                        let ty = i32::from(self.items.records[slot].i16_(0xf0));
                        self.start_gen_hit_fx(slot, ty, level == 0)?;
                        self.events.push(Event::GeneratorHit { slot, destroyed: level == 0 });
                    }
                    if level == 0 {
                        let ty = i32::from(self.items.records[slot].i16_(0xf0));
                        self.audio_generator_dies(at, ty)?;
                        for k in 0..self.num_enemies() {
                            if self.enemies[k].u32_(e::GENERATOR) == slot as u32 + 1 {
                                self.enemies[k].set_u32(e::GENERATOR, 0);
                            }
                        }
                    } else {
                        let ty = i32::from(self.items.records[slot].i16_(0xf0));
                        self.audio_generator_damaged(at, ty)?;
                    }
                }
            }
            4 => {
                let v = self.items.records[slot].u32_(0xf8) | 1;
                self.items.records[slot].set_u32(0xf8, v);
            }
            5 => {
                if flags & 0x800 == 0 && st == 0x1f {
                    // Shot switches: +0xe2 |= 0xf along the +0xf8 chain
                    // (native slot + 1).
                    result = 1;
                    let mut k = Some(slot);
                    let mut guard = 0;
                    while let Some(s) = k {
                        let v = self.items.records[s].u8_(0xe2) | 0xf;
                        self.items.records[s].set_u8(0xe2, v);
                        k = self.items.records[s].u32_(0xf8).checked_sub(1).map(|v| v as usize);
                        guard += 1;
                        if self.items.records.len() < guard {
                            return unsupported("DamageItem kind 5 chain cycle");
                        }
                    }
                }
            }
            10 if 0 <= result => {
                if shown && !killed && st != 0x29 {
                    self.items.records[slot].set_u16(0xf4, 1);
                }
                match st {
                    0x29 => {
                        if 0 <= self.items.records[slot].i16_(0xf2) {
                            self.set_safe_rock(slot)?;
                            killed = false;
                        }
                    }
                    0x2a => {
                        let h = i32::from(self.items.records[slot].i16_(0xe4));
                        self.audio_explode_wall(at, h)?;
                    }
                    0x2c | 0x2d if killed => {
                        self.items.records[slot].or_u16(0xd8, 1);
                        let centre = self.items.records[slot].v3(0x40);
                        let (k, kind) = if st == 0x2c { (30.0, 0x18) } else { (10.0, 0x19) };
                        self.start_explosion(mul(self.level_f32(0xdc), k)?, centre, kind)?;
                        if st == 0x2c {
                            self.audio_barrel_explode(at)?;
                        } else {
                            self.audio_poison_barrel(at)?;
                        }
                        // MBOX_NewObject debris (0x2a88f8 / 0x2a8900): presentation.
                        shown = false;
                        result = -2;
                        killed = false;
                    }
                    0x2c | 0x2d => self.sfx3(0x3c, at, 0x7f, 0x78)?,
                    _ => {
                        if killed {
                            self.items.records[slot].or_u16(0xd8, 1);
                            self.audio_wood_barrel(at)?;
                            killed = false;
                        } else {
                            self.sfx3(0x3c, at, 0x7f, 0x78)?;
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
            self.add_item_hit_col(at, flags, killed)?;
        }
        Ok(result as f32)
    }

    /// AddItemHitCol 0x17dcf8(position, flags, killed): StartFXSub(0,
    /// table[flags & 0xf], position, 0, 0x880) from 0x240728 when killed,
    /// else 0x240710 (an Effects slot; trace-G1-potion-v2). NATIVE: damage
    /// colours past the six entries are clamped.
    fn add_item_hit_col(&mut self, at: V3, flags: u32, killed: bool) -> R<i32> {
        const DAMAGE_HIT: [usize; 6] = [2, 11, 14, 17, 20, 0];
        const KILL_HIT: [usize; 6] = [3, 12, 15, 18, 21, 0];
        let k = ((flags & 0xf) as usize).min(5);
        self.start_hit_fx(if killed { KILL_HIT[k] } else { DAMAGE_HIT[k] }, at)
    }

    /// AddWallHitCol 0x17de10(position, flags): StartFXSub(0,
    /// WallHitEffects[flags & 0xf], position, 0, 0x880), then
    /// AudioEffect(5, position).
    fn add_wall_hit_col(&mut self, at: V3, flags: u32) -> R<i32> {
        let r = self.start_hit_fx(self.wall_hit_effect(flags), at)?;
        self.audio_effect(5, at)?;
        Ok(r)
    }

    /// start_magic 0x134408(power, player, position, potion flags, mode).
    /// Player -1 (a destroyed potion item): power × 20, size factor 0.8,
    /// mode 0, no potion audio. Mode 0: StartMagicFX 0x17eb50 (the potion
    /// blast); 1: StartShieldFX 0x17eec8 (turbo defence, parented to the
    /// player); 2+: StartThrowMagicFX 0x17ec80 (a thrown flask). Players
    /// past level 0x4a also get StartMagicHealFX 0x17ee48 (not for mode 1).
    pub fn start_magic(&mut self, power: f32, player: i32, position: V3, flags: u32, mode: i32) -> R<()> {
        let mut flags = flags;
        let mut mode = mode;
        let (kf, power) = match usize::try_from(player) {
            Err(_) => {
                mode = 0;
                (f32::from_bits(0x3f4c_cccd), mul(power, 20.0)?)
            }
            Ok(p) => {
                let mut kf = 1.0;
                let mut power = mul(power, self.players[p].f32_(0x120))?;
                if damage_color(flags) == player {
                    kf = f32::from_bits(0x3f8c_cccd);
                    power = mul(power, f32::from_bits(0x3f8c_cccd))?;
                }
                if 0x18 < self.players[p].i32_(0x2ac0) {
                    flags |= 0x80_0000;
                }
                (kf, power)
            }
        };
        let mut colour = flags & 0xf;
        if colour == 0 {
            let r = self.randpottype;
            colour = (r - (if r < 0 { r + 3 } else { r } >> 2) * 4 + 1) as u32;
            flags |= colour;
            self.randpottype += 1;
        }
        flags |= 0x200;
        let c = colour as usize % 5;
        let light = self.effects.tables.light_color[self.effects.tables.mclr_idx[c]];
        let size = |v: f32| -> R<f32> {
            let f = mul(v, 0.03125)?;
            Ok(if 1.0 < f { 1.0 } else { f })
        };
        let owner = player as i16 + 1;
        if mode == 1 {
            // StartShieldFX(25 kf, power / 4, no position, flags, player):
            // StartFXMissile(3, ShieldEffects[c], …, 0x13a), physics radius
            // = the power, ScaleFX(f, 1, f) with f = power / 12 in
            // [0.33, 1]; MBNodeSetParent to the player's node (+0x80).
            let p = player as usize;
            let power = mul(power, 0.25)?;
            let header = self.effects.tables.shield_effects[c] as usize;
            let at = self.players[p].v3(0x50);
            if let Some(k) = self.start_fx_missile(3.0, header, at, [0.0; 3], 0x13a, 0, 0, 0)? {
                self.sfx_set_physics_pub(k, 0.0, power, None, None);
                self.sfx_set_damage(k, mul(kf, 25.0)?, power, 0.0, flags, owner);
                let f4 = mul(power, f32::from_bits(0x3daa_aaab))?;
                let f3 = f4.clamp(0.33, 1.0);
                if let Some(n) = self.effects.nodes[k].as_mut() {
                    n.stretch = [f3, 1.0, f3];
                }
                self.sfx_set_light(k, mul(power, 1.5)?, light);
                self.parent_fx_to_player(k, p, [0.0; 3]);
            }
            self.audio_potion(c, position, 1)?;
            return Ok(());
        }
        if 2 <= mode {
            // StartThrowMagicFX(100, 40 kf, power × 0.75, start, velocity,
            // flags, player, colour + 7): from 2 × forward (+4 up) at speed
            // (short)+0x930 × 1.5 + 5 along (forward·0.707, 0.707);
            // StartFXSub(0.667, ThrowMagicEffects[c], start, 0x20010e),
            // SfxSetPhysics(100, 0.5, velocity, spin (10π, 0, 0) at
            // 0x240830), SfxSetHit(MagicEffects[c], colour + 7), +0xb0 the
            // hit scale min(power / 32, 1).
            let p = player as usize;
            let r = &self.players[p];
            let fwd = r.v3(0x40);
            let speed = add(mul(f32::from(r.i16_(0x930)), 1.5)?, 5.0)?;
            let start = [add(add(fwd[0], fwd[0])?, position[0])?, add(add(add(fwd[1], fwd[1])?, position[1])?, 4.0)?, add(add(fwd[2], fwd[2])?, position[2])?];
            let vel = [mul(mul(speed, fwd[0])?, 0.707)?, mul(speed, 0.707)?, mul(mul(speed, fwd[2])?, 0.707)?];
            let thrown = mul(power, 0.75)?;
            let header = self.effects.tables.throw_magic_effects[c] as usize;
            let k = self.start_fx_sub(f32::from_bits(0x3f2a_c083), header, Some(start), 0x20_010e)?;
            if 0 <= k {
                let k = k as usize;
                let spin = [f32::from_le_bytes(0x41fb_53d1u32.to_le_bytes()), 0.0, 0.0];
                self.sfx_set_physics_pub(k, 100.0, 0.5, Some(vel), Some(spin));
                let hit = self.effects.tables.magic_effects[c] as i16;
                self.sfx_set_hit(k, hit, colour + 7, colour + 7);
                self.sfx_set_damage(k, mul(kf, 40.0)?, thrown, 0.0, flags, owner);
                let f = size(thrown)?;
                self.fx_mut(k).set_f32(fx::HIT_SCALE, f);
                self.sfx_set_light(k, mul(thrown, 1.5)?, light);
            }
        } else {
            // StartMagicFX(40 kf, power, position, flags, player).
            let idx = *self.effects.tables.magic_effects.get(c).ok_or_else(|| Unsupported("MagicEffects index".into()))?;
            let h = self.effects.tables.headers.get(idx as usize).copied().flatten().ok_or_else(|| Unsupported(format!("magic effect header {idx}")))?;
            let i = self.start_fx_tree(0.0, Visual::Header(idx as usize), h.frames, h.timing, Some(position), 0x12a)?;
            let r = self.fx_mut(i);
            r.set_u32(fx::KIND, idx as u32);
            r.set_f32(fx::GRAVITY, 0.0);
            r.set_f32(fx::RADIUS, 0.0);
            r.set_i16(fx::HIT_FX, 0);
            r.set_u32(fx::HIT_SOUND, 0);
            r.set_u32(fx::WALL_SOUND, 0);
            self.sfx_set_damage(i, mul(kf, 40.0)?, power, 0.0, flags, owner);
            // ScaleFX(min(power / 32, 1)), SfxSetLight(1.5 power), then
            // start_magic's PlaceEffectOnFloor(effect, 0) (0x13468c): the
            // node lands on the floor + 0.1 (trace-G1-potion-v2).
            let f = size(power)?;
            if let Some(n) = self.effects.nodes[i].as_mut() {
                n.scale = f;
            }
            self.sfx_set_light(i, mul(power, 1.5)?, light);
            self.place_effect_on_floor(i)?;
            if 0 <= player {
                self.audio_potion(c, position, 0)?;
            }
        }
        // StartMagicHealFX(power, 0): StartFXSub(0, 0x33, …) scaled by
        // min(power / 32, 1), SfxSetParent to the player's node.
        if let Ok(p) = usize::try_from(player)
            && 0x4a < self.players[p].i32_(0x2ac0)
        {
            let at = self.players[p].v3(0x50);
            let k = self.start_fx_sub(0.0, 0x33, Some(at), 0)?;
            if 0 <= k {
                let f = size(power)?;
                if let Some(n) = self.effects.nodes[k as usize].as_mut() {
                    n.scale = f;
                }
                self.parent_fx_to_player(k as usize, p, [0.0; 3]);
            }
        }
        Ok(())
    }

    /// Random 0x1d6ad8: (rand() & 0x7fff) × x / 32767.
    pub(crate) fn random_pub(&mut self, x: f32) -> R<f32> {
        self.random(x)
    }
    fn random(&mut self, x: f32) -> R<f32> {
        let r = (self.rng.rand() & 0x7fff) as f32;
        Ok(div(mul(r, x)?, 32767.0)?)
    }

    /// shoot_missile 0x13a8b0(enemy, target, from): kind by logic.
    pub fn shoot_missile(&mut self, i: usize, target: V3, from: V3) -> R<()> {
        let logic = self.enemies[i].u16_(e::LOGIC);
        let kind = match logic {
            0x1c | 0x1d => 2,
            0x10 | 0x17 => 0,
            0x11 | 0x1a => 1,
            _ => 2,
        };
        if self.enemy_start_missile(i, target, from, kind)? {
            match kind {
                0 => self.sfx3(0x30, from, 0x7f, 0x7e)?,
                2 if self.enemies[i].i32_(e::TYPE) == 2 => {
                    // AudioDemonFireBall 0x15fcb8.
                    let id = match self.globals.cur_world {
                        3 => 0x27_0032,
                        4 => 0x28_0000,
                        6 => 0x38_001f,
                        _ => 0x26_0027,
                    };
                    self.sfx3(id, from, 0x7f, 0x32)?
                }
                2 => self.sfx3(0x32, from, 0x7f, 0x7e)?,
                _ => {}
            }
        }
        Ok(())
    }

    /// EnemyStartMissile 0x1901f0(enemy, target, from, kind).
    fn enemy_start_missile(&mut self, i: usize, target: V3, from: V3, kind: usize) -> R<bool> {
        let t = self.enemies[i].i32_(e::TYPE);
        let info = *self
            .effects
            .tables
            .enemy_missile_info
            .get(t as usize * 3 + kind)
            .ok_or_else(|| Unsupported(format!("EnemyMissileInfo type {t}")))?;
        let prefix = self.items.tables.enemy_table.iter().find(|(tt, _)| *tt == t).map(|(_, n)| String::from_utf8_lossy(n).to_uppercase());
        let label = prefix.map(|p| format!("{p}_{}", self.effects.tables.enemy_missile_desc[kind]));
        if !label.is_some_and(|l| self.enemy_anim_data.other_trees.contains_key(&(t, l))) {
            return Ok(false); // Errorf: no missile tree for this type/kind.
        }
        let fl = |k: usize| f32::from_bits(info[k]);
        let speed = mul(fl(2), self.level_f32(0xc4))?;
        let inv = div(1.0, speed)?;
        let mut d = v_sub(target, from)?;
        if kind == 2 {
            normal_vector(&mut d)?;
        } else {
            let lift = if kind == 1 { -5.5 } else { -3.5 };
            let r = self.random(5.0)?;
            let lift = add(lift, mul(sub(r, 2.5)?, self.level_f32(0xc8))?)?;
            calc_target_dir(speed, inv, fl(8), lift, &mut d)?;
        }
        if d[1] < 0.0 {
            d[1] = 0.0;
        }
        let mut flat = d;
        normal_vector_2d(&mut flat)?;
        let fwd = self.enemies[i].v3(0x30);
        if dot_xz(flat, fwd)? < f32::from_bits(0x3f34_fdf4) {
            return Ok(false);
        }
        let mut flags = self.enemies[i].u32_(0xd8);
        let mut sfx = 0u32;
        let mut origin = from;
        let mut cc: f32 = if kind == 2 { 0.0 } else { 2.5 };
        let mut absolute = false;
        match t {
            7 | 0x18 if kind == 2 => {
                origin[1] = add(from[1], 1.5)?;
                absolute = true;
            }
            0xd if kind == 0 => cc = 1.0,
            0xe if kind == 2 => cc = 1.0,
            0xe | 4 if kind == 0 => {
                origin[1] = add(from[1], 1.5)?;
                absolute = true;
            }
            0x11 => {
                cc = 2.0;
                origin[0] = add(add(d[0], d[0])?, origin[0])?;
                origin[2] = add(add(d[2], d[2])?, origin[2])?;
            }
            0x17 if kind == 0 => cc = 0.0,
            0x17 if kind == 1 => {
                cc = 0.0;
                origin[0] = sub(origin[0], mul(d[0], 2.5)?)?;
                origin[2] = sub(origin[2], mul(d[2], 2.5)?)?;
            }
            0x1b => {
                sfx = 0x800_0000;
                flags |= 0x10_0000;
                cc = 0.0;
            }
            _ => {}
        }
        if !absolute {
            origin[1] = add(cc, from[1])?;
        }
        let start = [add(mul(d[0], 3.0)?, origin[0])?, add(mul(d[1], 3.0)?, origin[1])?, add(mul(d[2], 3.0)?, origin[2])?];
        if self.weapon_wall_collide(fl(3), origin, start)?.0.is_some() {
            return Ok(false);
        }
        if self.weapon_item_collide(fl(3), origin, start, false, -1)?.is_some() {
            return Ok(false);
        }
        let mut damage = fl(1);
        if self.globals.enemy_shrink < 1.0 {
            damage = mul(damage, 0.5)?;
        }
        if kind == 0 {
            flags |= 0x2_0000;
        }
        if kind == 2 {
            flags |= 0x4_0000;
        }
        let k = self.start_missile(speed, damage, 0, start, d, flags, &info, 0, sfx, Visual::EnemyMissile { t, kind: kind as u8 })?;
        let shrink = self.globals.enemy_shrink;
        if shrink < 1.0
            && let Some(n) = self.effects.nodes[k].as_mut()
        {
            // ScaleFX(EnemyShrink, EnemyShrink, EnemyShrink).
            n.scale = shrink;
        }
        if kind == 1 {
            // SfxSetLight(5, missile, color.27 0x24ef10: red).
            self.sfx_set_light(k, 5.0, [2.0, 0.0, 0.0]);
        }
        Ok(true)
    }

    /// SuicideExplosion 0x17e938(damage, position): CurWorld 7/0xb a
    /// POISONEXP1 (0x19) blast (radius 7.5, damage flags 0x800), otherwise
    /// effect 0x16 (radius 6, flags 0x421) plus flame 0x1c; then 0x50.
    pub fn suicide_explosion(&mut self, damage: f32, position: V3) -> R<()> {
        // SuicideExplosion 0x17e938. ScaleFX/SfxSetMorph/SfxSetLight are
        // presentation; every effect still takes a slot.
        if self.globals.cur_world == 7 || self.globals.cur_world == 0xb {
            let Some(i) = self.start_fx_missile(0.0, 0x19, position, [0.0; 3], 0x2b, 0, 4, 0)? else {
                return unsupported("SuicideExplosion without POISONEXP1 (the original writes through effect -1)");
            };
            self.sfx_set_damage(i, damage, 7.5, 0.0, 0x800, 0);
            if let Some(n) = self.effects.nodes[i].as_mut() {
                n.position[1] = sub(n.position[1], 1.0)?;
            }
        } else {
            let Some(h) = self.effects.tables.headers.get(0x16).copied().flatten() else { return Ok(()) };
            let i = self.start_fx_tree(0.0, Visual::Header(0x16), h.frames, h.timing, Some(position), 0x29)?;
            self.fx_mut(i).set_u32(fx::KIND, 0x16);
            self.fx_mut(i).set_u32(fx::HIT_SOUND, 4);
            self.sfx_set_damage(i, damage, 6.0, 0.0, 0x421, 0);
            self.start_fx_missile(0.0, 0x1c, position, [0.0; 3], 0, 0, 0, 0)?;
            // SfxSetLight(2 × 10, light_color[0] red).
            self.sfx_set_light(i, 20.0, [2.0, 0.0, 0.0]);
        }
        if let Some(k) = self.start_fx_missile(0.0, 0x50, position, [0.0; 3], 0, 0, 0, 0)? {
            self.fx_mut(k).set_f32(fx::FADE_IN, 0.5);
        }
        Ok(())
    }

    /// StartEnterFX 0x17f2c8: StartFXSub(0, 0x43, position, 0, 0x80880) —
    /// the level-entry effect (the last argument is MB node flags).
    pub(crate) fn start_enter_fx(&mut self, position: V3) -> R<()> {
        let Some(h) = self.effects.tables.headers.get(0x43).copied().flatten() else { return Ok(()) };
        let i = self.start_fx_tree(0.0, Visual::Header(0x43), h.frames, h.timing, Some(position), 0)?;
        self.fx_mut(i).set_u32(fx::KIND, 0x43);
        Ok(())
    }

    /// StartGenFX 0x17f580(position, level): for levels 1..3 with
    /// EffectHeaders[0x50 + level] initialised, StartFXSub(0, 0x50 +
    /// level, position, 0, 0x800) — a generator's spawn flash.
    pub(crate) fn start_gen_fx(&mut self, at: V3, level: i32) -> R<i32> {
        if !(1..=3).contains(&level) {
            return Ok(-1);
        }
        self.start_fx_sub(0.0, (0x50 + level) as usize, Some(at), 0)
    }

    /// StartBlockFX 0x17f200(player) with the caller's f12 (damage_player's
    /// block factor k): StartFXSub(k, 0x42, none, 0, 0x80980), the tree
    /// coloured streak_color 0x24eed0[player colour] and alpha 0x40,
    /// SfxSetParent to the player's node.
    pub(crate) fn start_block_fx(&mut self, p: usize, k: f32) -> R<i32> {
        const STREAK_COLOR: [u32; 4] = [0xff_ff00, 0xff, 0xff_0000, 0xff00];
        let i = self.start_fx_sub(k, 0x42, None, 0)?;
        if let Ok(slot) = usize::try_from(i) {
            let colour = self.players[p].u32_(4) as usize;
            if let Some(n) = self.effects.nodes[slot].as_mut() {
                n.tint = STREAK_COLOR.get(colour).copied();
                n.alpha = Some(0x40);
            }
            self.parent_fx_to_player(slot, p, [0.0; 3]);
        }
        Ok(i)
    }

    /// StartFXNoLoop 0x17e348: StartFXSub(0, header, position, 0,
    /// 0x800); StartFXMat's SfxSetMat only orients the tree (presentation).
    pub(crate) fn start_fx_no_loop(&mut self, header: usize, position: V3) -> R<()> {
        let Some(h) = self.effects.tables.headers.get(header).copied().flatten() else { return Ok(()) };
        let i = self.start_fx_tree(0.0, Visual::Header(header), h.frames, h.timing, Some(position), 0)?;
        self.fx_mut(i).set_u32(fx::KIND, header as u32);
        Ok(())
    }

    /// StartFXMissile 0x17e278(time, header, position, velocity, flags, hit
    /// fx, hit sound, wall sound): StartFXSub then SfxSetPhysics(0, 0, …);
    /// None (-1) when the header was never initialised.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn start_fx_missile(&mut self, time: f32, header: usize, position: V3, vel: V3, flags: u32, hit_fx: i16, hit_sound: u32, wall_sound: u32) -> R<Option<usize>> {
        let Some(h) = self.effects.tables.headers.get(header).copied().flatten() else { return Ok(None) };
        let i = self.start_fx_tree(time, Visual::Header(header), h.frames, h.timing, Some(position), flags)?;
        let r = self.fx_mut(i);
        r.set_u32(fx::KIND, header as u32);
        r.set_f32(fx::GRAVITY, 0.0);
        r.set_f32(fx::RADIUS, 0.0);
        r.set_v3(fx::VELOCITY, vel);
        r.set_i16(fx::HIT_FX, hit_fx);
        r.set_u32(fx::HIT_SOUND, hit_sound);
        r.set_u32(fx::WALL_SOUND, wall_sound);
        Ok(Some(i))
    }

    /// StartFXSub(0, header, position, 0, 0x880): the hit/wall effects the
    /// damage code starts (node flags are presentation).
    pub(crate) fn start_hit_fx(&mut self, header: usize, at: V3) -> R<i32> {
        self.start_fx_sub(0.0, header, Some(at), 0)
    }

    /// AddEnemyHitCol 0x17db28(scale, position, matrix, flags, killed, type):
    /// enemy types 0xb/0x15 the Damage/KillAltEffects at half scale; 0x1d,
    /// 5 (or gore_level ≠ 0) the Hit tables at scale 1; otherwise the Blood
    /// tables at half scale (headers 4 and 5 alternate 6/7 and 8/9 by
    /// sfx_ene_bloodidx); ScaleFX and SfxSetMat (MBTreeSetZMod is
    /// presentation). gore_level is 0 in the ELF.
    pub(crate) fn add_enemy_hit_col(&mut self, scale: f32, at: V3, rows: Option<[V3; 3]>, flags: u32, killed: bool, ty: i32) -> R<i32> {
        const DAMAGE_ALT: [usize; 5] = [84, 11, 14, 17, 20];
        const KILL_ALT: [usize; 5] = [85, 12, 15, 18, 21];
        const DAMAGE_HIT: [usize; 5] = [2, 11, 14, 17, 20];
        const KILL_HIT: [usize; 5] = [3, 12, 15, 18, 21];
        const DAMAGE_BLOOD: [usize; 5] = [4, 11, 14, 17, 20];
        const KILL_BLOOD: [usize; 5] = [5, 12, 15, 18, 21];
        let k = (flags & 0xf) as usize;
        let (table, scale) = if ty == 0xb || ty == 0x15 {
            (if killed { KILL_ALT } else { DAMAGE_ALT }, mul(scale, 0.5)?)
        } else if ty == 0x1d || ty == 5 {
            (if killed { KILL_HIT } else { DAMAGE_HIT }, 1.0)
        } else {
            (if killed { KILL_BLOOD } else { DAMAGE_BLOOD }, mul(scale, 0.5)?)
        };
        // Damage types past the tables read the following words (NATIVE:
        // clamped to the last entry).
        let mut h = table[k.min(4)];
        if h == 4 {
            h = (self.sfx_ene_bloodidx & 1) as usize + 6;
            self.sfx_ene_bloodidx += 1;
        } else if h == 5 {
            h = (self.sfx_ene_bloodidx & 1) as usize + 8;
            self.sfx_ene_bloodidx += 1;
        }
        let i = self.start_hit_fx(h, at)?;
        if 0 <= i
            && let Some(n) = self.effects.nodes[i as usize].as_mut()
        {
            if let Some(r) = rows {
                n.matrix = r;
            }
            n.scale = scale;
        }
        Ok(i)
    }

    /// The tree `name` in enemy type `t`'s package (AtreeMatch(enemy_anim[t],
    /// name, 0)).
    fn enemy_effect_tree(&self, t: usize, name: &str) -> Option<EffectHeader> {
        let key = (t as i32, name.to_string());
        let &k = self.enemy_anim_data.tree_index.get(&key)?;
        let (frames, timing, repeat) = *self.enemy_anim_data.other_trees.get(&key)?;
        Some(EffectHeader { source: 0xc0 + t as u8, tree: k, frames, timing, repeat })
    }

    /// InitEffect 0x179d38(index, package, name, …): the header is always
    /// written — the AtreeMatch result, or none without a package or match.
    fn init_effect(&mut self, idx: usize, h: Option<EffectHeader>) -> bool {
        if let Some(slot) = self.effects.tables.headers.get_mut(idx) {
            *slot = h;
        }
        h.is_some()
    }

    /// InitEffects 0x179e20 after the EffectInfo headers: per loaded enemy
    /// type (0..0x2c in order) SUICIDEEXP into 0x50 (types not loaded as
    /// aux, while 0x50 is unset), GENFX1..3 into 0x51..0x53 while unset
    /// (enemy_genfx = 1 for each type that tries), TREEHIT/TREEDIE (0xb,
    /// 0x15), ATK1FX/ATK2FX/DEATHFX1/DEATHFX2 (0x1b), DEATH_ARC/DEATH_EXP
    /// (0x1e), then GenHitEffects[t] = InitCustomEffectSub(enemy_anim[t],
    /// "GENHIT") and GenDieEffects[t] ("GENDIE"); CurWorld 5/6 re-register
    /// index 0 from item_anim; boss levels LEGENDFX/LEGENDFX2/LEGENDPRJ/
    /// LEGENDHLD (0x5a, 0x5b, 0x5d, 0x5c) from item_anim2; 0x52/0x53 fall back
    /// to 0x51; InLevel("B6"/"I5") SAFEREXP into 0x1e from item_anim2
    /// (trace-B6-bosskill-v1: the dragon's spew blast bursts); WORLD_EXP into
    /// 0x5e from item_anim, else item_anim2. Textures are presentation.
    pub(crate) fn init_gen_effects(&mut self) {
        for k in 0x50..super::effects::MAX_HEADERS.min(self.effects.tables.headers.len()) {
            if k < 0x61 {
                self.effects.tables.headers[k] = None;
            }
        }
        for t in 0..45usize {
            let ti = t as i32;
            if let Some(g) = self.enemy_load.genfx.get_mut(t) {
                *g = 0;
            }
            let loaded = self.enemy_anim_data.trees.keys().any(|(tt, _)| *tt == ti);
            if !loaded {
                self.gen_hit_effects[t] = -1;
                self.gen_die_effects[t] = -1;
                continue;
            }
            let unset = |g: &Game, k: usize| g.effects.tables.headers.get(k).is_none_or(Option::is_none);
            if self.enemy_load.loaded.get(t) != Some(&4) && unset(self, 0x50) {
                self.init_effect(0x50, self.enemy_effect_tree(t, "SUICIDEEXP"));
            }
            for (k, name) in [(0x51, "GENFX1"), (0x52, "GENFX2"), (0x53, "GENFX3")] {
                if unset(self, k) {
                    self.init_effect(k, self.enemy_effect_tree(t, name));
                    if let Some(g) = self.enemy_load.genfx.get_mut(t) {
                        *g = 1;
                    }
                }
            }
            if t == 0xb || t == 0x15 {
                self.init_effect(0x54, self.enemy_effect_tree(t, "TREEHIT"));
                self.init_effect(0x55, self.enemy_effect_tree(t, "TREEDIE"));
            }
            if t == 0x1b {
                for (k, name) in [(0x56, "ATK1FX"), (0x57, "ATK2FX"), (0x58, "DEATHFX1"), (0x59, "DEATHFX2")] {
                    self.init_effect(k, self.enemy_effect_tree(0x1b, name));
                }
            }
            if t == 0x1e {
                self.init_effect(0x5f, self.enemy_effect_tree(0x1e, "DEATH_ARC"));
                self.init_effect(0x60, self.enemy_effect_tree(0x1e, "DEATH_EXP"));
            }
            for (name, die) in [("GENHIT", false), ("GENDIE", true)] {
                let own = self.enemy_effect_tree(t, name);
                let h = self.register_custom_effect(own, name);
                if die {
                    self.gen_die_effects[t] = h;
                } else {
                    self.gen_hit_effects[t] = h;
                }
            }
        }
        if (self.globals.cur_world as u32).wrapping_sub(5) < 2 {
            let own = self.effects.tables.item_header(0, "GENHIT");
            self.gen_hit_effects[0] = self.register_custom_effect(own, "GENHIT");
            let own = self.effects.tables.item_header(0, "GENDIE");
            self.gen_die_effects[0] = self.register_custom_effect(own, "GENDIE");
        }
        if 0 <= self.globals.boss_type {
            for (k, name) in [(0x5a, "LEGENDFX"), (0x5b, "LEGENDFX2"), (0x5d, "LEGENDPRJ"), (0x5c, "LEGENDHLD")] {
                self.init_effect(k, self.effects.tables.item_header(1, name));
            }
        }
        for k in [0x52, 0x53] {
            if self.effects.tables.headers.get(k).is_some_and(Option::is_none) {
                self.effects.tables.headers[k] = self.effects.tables.headers[0x51];
            }
        }
        if self.in_level(b"B6\0\0") || self.in_level(b"I5\0\0") {
            self.init_effect(0x1e, self.effects.tables.item_header(1, "SAFEREXP"));
        }
        if !self.init_effect(0x5e, self.effects.tables.item_header(0, "WORLD_EXP")) {
            self.init_effect(0x5e, self.effects.tables.item_header(1, "WORLD_EXP"));
        }
    }

    /// StartGenHitFx 0x17f720(item matrix, type, destroyed): GenDieEffects
    /// (types 1/4 fall back to effect 0x1e) or GenHitEffects of the type,
    /// StartFXNoLoop at the item's translation and SfxSetMat with its rows.
    pub(crate) fn start_gen_hit_fx(&mut self, slot: usize, ty: i32, destroyed: bool) -> R<()> {
        let t = ty.max(0) as usize;
        let mut h = if destroyed { self.gen_die_effects.get(t).copied().unwrap_or(-1) } else { self.gen_hit_effects.get(t).copied().unwrap_or(-1) };
        if destroyed && h < 0 && (t == 1 || t == 4) {
            h = 0x1e;
        }
        let Ok(h) = usize::try_from(h) else { return Ok(()) };
        let Some(head) = self.effects.tables.headers.get(h).copied().flatten() else { return Ok(()) };
        let r = self.items.records[slot].clone();
        let i = self.start_fx_tree(0.0, Visual::Header(h), head.frames, head.timing, Some(r.v3(0x40)), 0)?;
        self.fx_mut(i).set_u32(fx::KIND, h as u32);
        if let Some(n) = self.effects.nodes[i].as_mut() {
            n.matrix = [r.v3(0x10), r.v3(0x20), r.v3(0x30)];
        }
        Ok(())
    }

    /// AddPlayerHitCol 0x17dd70(position, flags, killed): DamageBlood /
    /// KillBloodEffects (gore_level 0).
    pub(crate) fn add_player_hit_col(&mut self, at: V3, flags: u32, killed: bool) -> R<i32> {
        const DAMAGE_BLOOD: [usize; 5] = [4, 11, 14, 17, 20];
        const KILL_BLOOD: [usize; 5] = [5, 12, 15, 18, 21];
        let k = ((flags & 0xf) as usize).min(4);
        self.start_hit_fx(if killed { KILL_BLOOD[k] } else { DAMAGE_BLOOD[k] }, at)
    }

    /// SfxSetLight 0x17f8b8(radius, effect, colour): +0x10 radius, +0..+8
    /// colour (ProcessEffects lights lit objects with it: presentation).
    pub(crate) fn sfx_set_light(&mut self, i: usize, radius: f32, colour: [f32; 3]) {
        let r = self.fx_mut(i);
        r.set_f32(0x10, radius);
        r.set_v3(0, colour);
    }

    /// SfxSetDamage 0x17dfb0(damage, blast, fade, effect, flags, owner).
    pub(crate) fn sfx_set_damage(&mut self, i: usize, damage: f32, blast: f32, fade: f32, flags: u32, owner: i16) {
        let flags = if 4 < flags & 0xf { flags & 0xffff_fff3 } else { flags };
        let r = self.fx_mut(i);
        r.set_f32(fx::DAMAGE, damage);
        r.set_u32(fx::DAMAGE_FLAGS, flags);
        r.set_f32(fx::BLAST, blast);
        r.set_f32(fx::FADE, fade);
        r.set_i16(fx::OWNER, owner);
    }

    /// StartExplosion 0x17e590(damage, matrix translation, kind):
    /// StartFXMissile(0, kind (0x1d: 0x16), position, 0, 0x2b, 0, 4, 0)
    /// then SfxSetDamage(damage, blast, 0, flags, 0): 0x17 (4, 0x11), 0x18
    /// barrels (12, 0x421, +2 lift, ScaleFX 1.75/1/1.75), 0x19 poison barrels
    /// (6.5, 0x800, morph 0x1a/0x1b after 4 s, ScaleFX 3.5), 0x1d the
    /// exploding chest (12, 0x421, +3, ScaleFX 2.5, plus effect 0x1d), others
    /// (6, 0x421, ScaleFX 1.5); a red light of 2 × blast with flag 1.
    pub fn start_explosion(&mut self, damage: f32, position: V3, kind: u32) -> R<()> {
        // (blast, damage flags, lift, ScaleFX x/z with y 1).
        let (blast, flags, lift, stretch) = match kind {
            0x17 => (4.0, 0x11u32, 0.0, None),
            0x18 => (12.0, 0x421, 2.0, Some(1.75)),
            0x19 => (6.5, 0x800, 0.0, Some(3.5)),
            0x1d => (12.0, 0x421, 3.0, Some(2.5)),
            _ => (6.0, 0x421, 0.0, Some(1.5)),
        };
        // Kind 0x1d (the exploding chest) draws header 0x16 and adds 0x1d.
        let header = if kind == 0x1d { 0x16 } else { kind };
        let h = self.effects.tables.headers.get(header as usize).copied().flatten().ok_or_else(|| Unsupported(format!("effect header {header:#x}")))?;
        let i = self.start_fx_tree(0.0, Visual::Header(header as usize), h.frames, h.timing, Some(position), 0x2b)?;
        let flags = if 4 < flags & 0xf { flags & 0xffff_fff3 } else { flags };
        let r = self.fx_mut(i);
        r.set_u32(fx::KIND, header);
        r.set_f32(fx::GRAVITY, 0.0);
        r.set_f32(fx::RADIUS, 0.0);
        r.set_i16(fx::HIT_FX, 0);
        r.set_u32(fx::HIT_SOUND, 4);
        r.set_u32(fx::WALL_SOUND, 0);
        r.set_f32(fx::DAMAGE, damage);
        r.set_u32(fx::DAMAGE_FLAGS, flags);
        r.set_f32(fx::BLAST, blast);
        r.set_f32(fx::FADE, 0.0);
        r.set_i16(fx::OWNER, 0);
        if let Some(n) = self.effects.nodes[i].as_mut() {
            n.position[1] = add(n.position[1], lift)?;
            if let Some(k) = stretch {
                n.stretch = [k, 1.0, k];
            }
        }
        if kind == 0x19 {
            self.sfx_set_morph(i, 4.0, 0x1a, 0x1b);
        }
        if kind == 0x1d {
            // StartFXSub(0, 0x1d, position) with the item's matrix
            // (SfxSetMat: presentation).
            self.start_fx_sub(0.0, 0x1d, Some(position), 0)?;
        }
        if kind == 0x1d || !matches!(kind, 0x17..=0x19) || flags & 1 != 0 {
            // SfxSetLight(2 × blast, light_color[0] red).
            self.sfx_set_light(i, blast + blast, [2.0, 0.0, 0.0]);
        }
        if self.globals.cur_world == 10 && kind == 0x18 {
            // Three fireworks (0x17e810): size 0.6 + Random(0.5), speed 6 +
            // Random(3), yaw Random(2π) − π and pitch −(π/3 + Random(0.524))
            // applied to (0, 0, 1).
            for _ in 0..3 {
                let size = add(self.random(0.5)?, f32::from_bits(0x3f19_999a))?;
                let speed = add(self.random(3.0)?, 6.0)?;
                let yaw = add(self.random(f32::from_bits(0x40c9_0fdb))?, f32::from_bits(0xc049_0fdb))?;
                let pitch = add(self.random(f32::from_bits(0x3f06_0a92))?, f32::from_bits(0x3f86_0a92))?;
                let v = super::critter_ai::yaw_vec3(yaw, [0.0, 0.0, 1.0])?;
                let v = super::critter_ai::pitch_vec3(-pitch, v)?;
                let vel = [mul(speed, v[0])?, mul(speed, v[1])?, mul(speed, v[2])?];
                self.start_firework(size, 6.0, position, vel, 0xb4)?;
            }
        }
        Ok(())
    }

    /// StartBagFX 0x17f2f8(power, position, item): effect 0x44 (3 s, flags
    /// 0x200004, hit fx 0x45) thrown with gravity 50 and radius 1 at 20 ×
    /// up, or towards the nearest active player within 30 (power/2 × the
    /// horizontal direction, power/2 × 0.707 up); spin x 0.2618 +
    /// Random(1.0472). The item rides at +0xec and is placed where the
    /// effect ends (ProcessEffects).
    pub(crate) fn start_bag_fx(&mut self, power: f32, position: V3, item: usize) -> R<i32> {
        let p = if power <= 0.0 { 20.0 } else { power };
        let mut vel = [mul(p, 0.0)?, mul(p, 1.0)?, mul(p, 0.0)?];
        let mut best = 30.0f32;
        for k in 0..4 {
            if self.players[k].i32_(0xfc) != 1 {
                continue;
            }
            let pp = self.players[k].v3(0x50);
            let mut d = [sub(pp[0], position[0])?, sub(pp[1], position[1])?, sub(pp[2], position[2])?];
            let len = crate::item_collision::normal_vector_2d(&mut d)?;
            if len < best {
                best = len;
                let h = mul(p, 0.5)?;
                vel = [mul(h, d[0])?, mul(h, f32::from_bits(0x3f34_fdf4))?, mul(h, d[2])?];
            }
        }
        let spin = [add(self.random(f32::from_bits(0x3f86_0a92))?, f32::from_bits(0x3e86_0a92))?, 0.0, 0.0];
        let Some(h) = self.effects.tables.headers.get(0x44).copied().flatten() else { return unsupported("effect header 0x44 (bag)") };
        let i = self.start_fx_tree(3.0, Visual::Header(0x44), h.frames, h.timing, Some(position), 0x20_0004)?;
        let r = self.fx_mut(i);
        r.set_u32(fx::KIND, 0x44);
        r.set_i16(fx::HIT_FX, 0x45);
        r.set_u32(fx::HIT_SOUND, 0);
        r.set_u32(fx::WALL_SOUND, 0);
        r.set_v3(fx::VELOCITY, vel);
        r.set_v3(fx::SPIN, spin);
        r.set_f32(fx::GRAVITY, 50.0);
        r.set_f32(fx::RADIUS, 1.0);
        // +0xec: the carried item (native slot + 1).
        r.set_u32(0xec, item as u32 + 1);
        Ok(i as i32)
    }

    /// WorldObjectExplode 0x1647a8: WorldExplosion at the hit, then the
    /// object and its parents vanish (+0x10 |= 0x10000000: no collision;
    /// the MB tree hide flag is presentation).
    pub(crate) fn world_object_explode(&mut self, object: usize, at: V3) -> R<()> {
        self.world_explosion(at)?;
        let mut o = Some(object);
        let mut guard = 0;
        while let Some(k) = o {
            self.items.objects[k].flags_10 |= 0x1000_0000;
            if let Some(f) = self.collision.runtime_flags.get_mut(k) {
                *f |= 0x1000_0000;
            }
            self.events.push(Event::ObjectHidden { object: k });
            o = self.collision.objects[k].parent;
            guard += 1;
            if self.collision.objects.len() < guard {
                return unsupported("object parent cycle");
            }
        }
        Ok(())
    }

    /// WorldExplosion 0x164688 (CurWorld 9/0xb: effect 0x5e radius 6 flags
    /// 0x800, else or without that header 0x16 flags 0x21, radius 5 outside
    /// 9/0xb): damage 50, StartFXMissile(0, kind, pos, 0, 0x2b, 0, 4, 0).
    fn world_explosion(&mut self, at: V3) -> R<()> {
        let special = self.globals.cur_world == 9 || self.globals.cur_world == 0xb;
        let (mut kind, blast, mut fl) = if special { (0x5e, 6.0, 0x800) } else { (0x16, 5.0, 0x21) };
        if self.effects.tables.headers.get(kind).copied().flatten().is_none() {
            kind = 0x16;
            fl = 0x21;
        }
        if let Some(i) = self.start_fx_missile(0.0, kind, at, [0.0; 3], 0x2b, 0, 4, 0)? {
            self.sfx_set_damage(i, 50.0, blast, 0.0, fl, 0);
        }
        self.audio_world_cue(1, at)?;
        Ok(())
    }

    /// StartDeathFX 0x17f7d0(parent, level, flags): effect 0x60 (level 2)
    /// or 0x5f for 10 s, parented to Death's node
    /// (presentation), linger until ATime + 0.5; -1 without the header.
    pub(crate) fn start_death_fx(&mut self, level: i32) -> R<i32> {
        let header = if level == 2 { 0x60 } else { 0x5f };
        let Some(h) = self.effects.tables.headers.get(header).copied().flatten() else { return Ok(-1) };
        let i = self.start_fx_tree(10.0, Visual::Header(header), h.frames, h.timing, None, 0)?;
        let at = add(self.globals.atime, 0.5)?;
        let r = self.fx_mut(i);
        r.set_u32(fx::KIND, header as u32);
        r.set_f32(fx::LINGER, at);
        Ok(i as i32)
    }

    /// StartFirework 0x17e4d8: StartFXMissile(duration `size`, kind 0,
    /// position, velocity, flags 0x10020000, hit fx/sounds -1), +0xb8 and
    /// +0xc4 from the arguments; MBPsysFlame is presentation. No damage.
    fn start_firework(&mut self, size: f32, b8: f32, position: V3, vel: V3, c4: i16) -> R<()> {
        let Some(h) = self.effects.tables.headers.first().copied().flatten() else { return unsupported("effect header 0 for a firework") };
        let i = self.start_fx_tree(size, Visual::Header(0), h.frames, h.timing, Some(position), 0x1002_0000)?;
        let r = self.fx_mut(i);
        r.set_u32(fx::KIND, 0);
        r.set_f32(fx::GRAVITY, 0.0);
        r.set_f32(fx::RADIUS, 0.0);
        r.set_v3(fx::VELOCITY, vel);
        r.set_i16(fx::HIT_FX, -1);
        r.set_i32(fx::HIT_SOUND, -1);
        r.set_i32(fx::WALL_SOUND, -1);
        r.set_f32(fx::BLAST, b8);
        r.set_i16(fx::HITS, c4);
        Ok(())
    }

    /// SetSafeRock 0x153438: damage stage from health (+0xe4) against the
    /// definition's +0x44; the model "<name><stage>" (then +"L1", +"ROOT")
    /// must exist (MBOX_FindObject_Sub, strncmp 15) or the rock stops
    /// staging (+0xf2 = -1).
    pub(crate) fn set_safe_rock_pub(&mut self, slot: usize) -> R<()> {
        self.set_safe_rock(slot)
    }

    fn set_safe_rock(&mut self, slot: usize) -> R<()> {
        let r = &self.items.records[slot];
        let def = &self.items.definitions[r.definition()];
        let h = i32::from(r.i16_(0xe4));
        let base = i32::from(def.i16_(0x44));
        let stage: i16 = if h == 0 {
            0
        } else if base < h {
            if h <= base << 1 { 2 } else { 3 }
        } else {
            1
        };
        if stage == r.i16_(0xf2) {
            return Ok(());
        }
        let name = format!("{}{}", String::from_utf8_lossy(def.name()), stage);
        self.items.records[slot].set_i16(0xf2, stage);
        let found = self.mb_object_exists(&name)? || self.mb_object_exists(&format!("{name}L1"))? || self.mb_object_exists(&format!("{name}L1ROOT"))?;
        let pos = self.items.records[slot].v3(0x40);
        if !found {
            // MBTreeSetFlags(tree, 1, 1): hidden (presentation).
            self.items.records[slot].set_i16(0xf2, -1);
            self.events.push(Event::ItemModel { slot, level: 0xff });
        } else {
            // MBSetObject: model swap (presentation).
            self.events.push(Event::ItemModel { slot, level: stage as u8 });
            if stage != 0 {
                return Ok(());
            }
            self.items.records[slot].and_u16(0xd8, 0xfffe);
            self.items.records[slot].set_u8(0xe3, 0xff);
        }
        if stage == 0 {
            // StartFXNoLoop(0x1e): non-damaging effect (presentation).
            self.start_fx_no_loop(0x1e, pos)?;
        }
        Ok(())
    }

    /// ItemGenDesc 0x151308: "GEN_SPECIAL<+0xf6>" for +0xf0 < −1, else
    /// "GEN_<EnemyTypePrefix(+0xf0)><+0xf6>". Its "BOSSGEN" test reads the
    /// caller's uninitialised name buffer; taken as not matching
    /// (HYPOTHESIS).
    pub(crate) fn item_gen_desc(&self, slot: usize) -> String {
        let r = &self.items.records[slot];
        let level = r.u8_(0xf6);
        let t = r.i16_(0xf0);
        if t < -1 {
            return format!("GEN_SPECIAL{level}");
        }
        // EnemyTypePrefix returns 0 for an unknown type: sprintf's "(null)".
        let prefix = self.enemy_tables.prefix.iter().find(|(k, _)| *k == i32::from(t)).map_or("(null)", |(_, p)| p.as_str());
        format!("GEN_{prefix}{level}")
    }

    /// MBOX_FindObject_Sub over the loaded object tables (objcmp = strncmp 15).
    fn mb_object_exists(&self, name: &str) -> R<bool> {
        let Some(set) = self.mb_objects.as_ref() else { return unsupported("MBOX object lookup without loaded object names") };
        let key: String = name.chars().take(15).collect();
        Ok(set.contains(&key))
    }

    /// MissileCollidePlayer 0x190880(radius, from, to): first active player hit.
    fn missile_collide_player(&self, r: f32, from: V3, to: V3) -> R<Option<(usize, V3)>> {
        for k in 0..4 {
            let pl = &self.players[k];
            if pl.i32_(0xfc) != 1 {
                continue;
            }
            let w = add(r, pl.f32_(0x828))?;
            let h = add(r, pl.f32_(0x82c))?;
            if let Some(c) = super::enemies::line_cylinder_collide(pl.v3(0x70), from, to, w, h, false)? {
                return Ok(Some((k, c)));
            }
        }
        Ok(None)
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

    /// The live critter node an effect node hangs under, if any.
    fn effect_critter_parent(&self, i: usize) -> Option<(usize, Option<usize>)> {
        let (c, n) = self.effects.nodes[i]?.parent?.critter?;
        self.critters.get(c).is_some_and(Option::is_some).then_some((c, n))
    }

    /// UnparentMatrix(node, parent) 0x1d0620 under critter `c`'s node `n`:
    /// recursive, so the outermost ancestor goes first; a translation-only
    /// level subtracts its offset, the others MulMat4(InvertMat4(local), m)
    /// (collision::unparent_chain's order).
    fn effect_unparent_from_critter(&self, c: usize, n: Option<usize>, mut m: [[f32; 4]; 4]) -> R<[[f32; 4]; 4]> {
        let chain = self.critter_parent_chain(c, n)?;
        let rows = |b: &[f32; 16]| -> [[f32; 4]; 4] { std::array::from_fn(|r| std::array::from_fn(|k| b[4 * r + k])) };
        let fail = |e: crate::collision::UnsupportedCollision| Unsupported(format!("UnparentMatrix: {e:?}"));
        for b in chain.iter().rev() {
            if b.flags & 4 != 0 {
                for (j, v) in m[3].iter_mut().take(3).enumerate() {
                    *v = sub(*v, b.matrix[12 + j])?;
                }
            } else {
                let inv = crate::collision::invert_mat4(&rows(&b.matrix)).map_err(fail)?;
                m = crate::collision::mul_mat4(&inv, &m).map_err(fail)?;
            }
        }
        Ok(m)
    }

    fn process_effect(&mut self, i: usize) -> R<()> {
        if let Some((c, node)) = self.effect_critter_parent(i) {
            // GetWorldMat of a node under a critter node (SfxSetParent): its
            // local (rows and translation) times each ancestor in turn.
            let par = self.effects.nodes[i].and_then(|n| n.parent).ok_or_else(|| Unsupported("effect parent".into()))?;
            let mut leaf = crate::tree_pose::IDENTITY;
            for r in 0..3 {
                leaf[4 * r..4 * r + 3].copy_from_slice(&par.local_matrix[r]);
            }
            leaf[12..15].copy_from_slice(&par.local);
            let mut chain = self.critter_parent_chain(c, node)?;
            let w = crate::world_transform::compose(crate::world_transform::MatrixBox { matrix: leaf, flags: 0 }, &mut chain, None)
                .map_err(|_| Unsupported("effect GetWorldMat".into()))?
                .matrix;
            if let Some(n) = self.effects.nodes[i].as_mut() {
                n.position = [w[12], w[13], w[14]];
                n.matrix = [[w[0], w[1], w[2]], [w[4], w[5], w[6]], [w[8], w[9], w[10]]];
            }
        } else if let Some(Some(par)) = self.effects.nodes[i].map(|n| n.parent) {
            // The parent node's world transform this update (psfx.rs HYPOTHESIS
            // for players).
            let parent = match (par.critter, par.enemy) {
                // The critter instance is gone: the node keeps its last transform.
                (Some(_), _) => None,
                (None, Some(e)) => {
                    let r = &self.enemies[e];
                    Some(([r.v3(0x10), r.v3(0x20), r.v3(0x30)], r.v3(0x40)))
                }
                (None, None) => {
                    let r = &self.players[par.player];
                    Some(([r.v3(0x20), r.v3(0x30), r.v3(0x40)], r.v3(0x50)))
                }
            };
            if let Some((rows, base)) = parent {
                let col = |v: V3, c: usize| -> R<f32> { Ok(add(add(mul(v[0], rows[0][c])?, mul(v[1], rows[1][c])?)?, mul(v[2], rows[2][c])?)?) };
                let w = [add(col(par.local, 0)?, base[0])?, add(col(par.local, 1)?, base[1])?, add(col(par.local, 2)?, base[2])?];
                let lm = par.local_matrix;
                let m = [[col(lm[0], 0)?, col(lm[0], 1)?, col(lm[0], 2)?], [col(lm[1], 0)?, col(lm[1], 1)?, col(lm[1], 2)?], [col(lm[2], 0)?, col(lm[2], 1)?, col(lm[2], 2)?]];
                if let Some(n) = self.effects.nodes[i].as_mut() {
                    n.position = w;
                    n.matrix = m;
                }
            }
        }
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
        let homing = match (flags & 0x4000_0000 != 0, node.target) {
            (true, Some((c, n))) if self.critters.get(c).is_some_and(Option::is_some) => {
                // SfxSetHitTarget: steer at the target node at speed +0xa0.
                let m = self.critter_node_world(c, n, None)?;
                let mut d = [sub(m[12], from[0])?, sub(m[13], from[1])?, sub(m[14], from[2])?];
                normal_vector(&mut d)?;
                let speed = self.fx(i).f32_(fx::GRAVITY);
                let v = [mul(d[0], speed)?, mul(d[1], speed)?, mul(d[2], speed)?];
                self.fx_mut(i).set_v3(fx::VELOCITY, v);
                to = [add(mul(v[0], spf)?, from[0])?, add(mul(v[1], spf)?, from[1])?, add(mul(v[2], spf)?, from[2])?];
                moved = true;
                true
            }
            _ => false,
        };
        if !homing && flags & 0xc000 != 0xc000 {
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
            // The spin (+0x8c..+0x94, radians per second): PitchMat3(−x ·
            // SPF), YawMat3(y · SPF), RollMat3(z · SPF) for each nonzero
            // component (trace-fight-g1-v4: the missile's rows roll ~36° an
            // update about its forward row).
            let s = self.fx(i).v3(fx::SPIN);
            let rot = |e: crate::collision::UnsupportedCollision| Unsupported(format!("spin: {e:?}"));
            if s[0] != 0.0 {
                crate::collision::pitch_mat3(&mut m, mul(-s[0], spf)?).map_err(rot)?;
                moved = true;
            }
            if s[1] != 0.0 {
                crate::collision::yaw_mat3(&mut m, mul(s[1], spf)?).map_err(rot)?;
                moved = true;
            }
            if s[2] != 0.0 {
                crate::collision::roll_mat3(&mut m, mul(s[2], spf)?).map_err(rot)?;
                moved = true;
            }
        } else {
            // CreateDirMatrix(…, 0x23e520): row 2 of Cam[0]'s matrix (Cam
            // 0x23e4f0 + 0x30), which CamUpdate copies from the StdCam; the
            // boss camera leaves it (w2l6-boss-v1: (0, −sin 1, −cos 1)).
            // Before a level's first CamUpdate the window row stands in.
            let up = self.std_camera.as_ref().map(|c| c.cam0_mat.map_or(c.pb.cam_rows[2], |m| m[2]));
            m = create_dir_matrix(self.fx(i).v3(fx::VELOCITY), up)?;
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
        if let Some(n) = self.effects.nodes[i].as_mut() {
            n.radius = radius;
        }
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
            if mode == 0 {
                let r = if flags & 0x200 != 0 { 0.01 } else { radius };
                if let Some((k, p)) = self.missile_collide_player(r, from, to)? {
                    let left = sub(self.fx(i).f32_(fx::DURATION), age)?;
                    if k as i32 == owner_raw as i32 - 1 {
                        if 0.2 < left {
                            result = -1;
                        }
                    } else if flags & 0x200 != 0 {
                        if 0.1 < left {
                            result = -1;
                        }
                    } else if self.players[k].u32_(0x134) & 0x102_0000 != 0 {
                        // A reflecting shield (+0x134 0x1020000):
                        // AudioWeaponReflect, flags −0xc00 +8, the velocity
                        // reversed and this step undone, the lifetime capped
                        // at ATime + 10 (else 1 s shorter), damage ≤ 15. The
                        // CreateDirMatrix turn (flag 0x20000) is presentation.
                        self.audio_weapon_reflect(p)?;
                        let r = self.fx_mut(i);
                        r.set_u32(fx::FLAGS, flags & 0xffff_f3ff | 8);
                        let v = r.v3(fx::VELOCITY);
                        let nv = [-v[0], -v[1], -v[2]];
                        r.set_v3(fx::VELOCITY, nv);
                        to = [add(mul(nv[0], spf)?, to[0])?, add(mul(nv[1], spf)?, to[1])?, add(mul(nv[2], spf)?, to[2])?];
                        moved = true;
                        let cap = add(atime, 10.0)?;
                        let end = self.fx(i).f32_(fx::END);
                        let e2 = if cap < end { cap } else { sub(end, 1.0)? };
                        self.fx_mut(i).set_f32(fx::END, e2);
                        if 15.0 < self.fx(i).f32_(fx::DAMAGE) {
                            self.fx_mut(i).set_f32(fx::DAMAGE, 15.0);
                        }
                    } else {
                        let mut dir = self.fx(i).v3(fx::VELOCITY);
                        normal_vector(&mut dir)?;
                        let mut res = 0;
                        if self.players[k].f32_(0x8c0) <= time {
                            let pain = flags & 0x2000 == 0;
                            let df = self.fx(i).u32_(fx::DAMAGE_FLAGS) | if pain { 0 } else { 0x80 };
                            res = self.damage_player(if pain { damage } else { 0.0 }, k, i32::from(pain), df, Some(dir))?;
                            if 0xfff < owner_raw {
                                self.critter_damaged_player(k, owner_raw as u16 & 0xfff, damage)?;
                            }
                            if 2.0 < damage {
                                self.players[k].set_f32(0x8c0, add(cool, time)?);
                            }
                        }
                        if pierce == 0 {
                            to = p;
                            moved = true;
                        } else if self.fx(i).u32_(fx::DAMAGE_FLAGS) & 0x20_0000 != 0 && 0 <= self.fx(i).i16_(fx::HIT_FX) && 2.0 < damage {
                            // A piercing shot's hit effect at the player
                            // (+0x60), on the floor for flags 0x300000; damage
                            // flag 0x100000 cleared.
                            let at = self.players[k].v3(0x60);
                            let h = self.fx(i).i16_(fx::HIT_FX) as usize;
                            let k2 = self.start_fx_sub(0.0, h, Some(at), 0)?;
                            if 0 <= k2 && self.fx(i).u32_(fx::FLAGS) & 0x30_0000 != 0 {
                                self.place_effect_on_floor(k2 as usize)?;
                            }
                            let df = self.fx(i).u32_(fx::DAMAGE_FLAGS) & 0xffef_ffff;
                            self.fx_mut(i).set_u32(fx::DAMAGE_FLAGS, df);
                        }
                        result = if res != 0 { 3 } else { 2 };
                    }
                }
                if pierce != 0 {
                    result = 0;
                }
            } else if 0.0 < radius && flags & 0x200 == 0 {
                let mut df = self.fx(i).u32_(fx::DAMAGE_FLAGS);
                if damage < 5.0 {
                    df = df & 0xffff_fe8f | 0x100_0000;
                }
                for k in 0..4 {
                    let pl = &self.players[k];
                    if pl.i32_(0xfc) != 1 || time < pl.f32_(0x8c0) || k as i32 == owner_raw as i32 - 1 {
                        continue;
                    }
                    let mut v = v_sub(pl.v3(0x70), to)?;
                    if add(radius, pl.f32_(0x82c))? < v[1].abs() {
                        continue;
                    }
                    let dist = normal_vector_2d(&mut v)?;
                    let w = pl.f32_(0x828);
                    if add(radius, w)? < dist {
                        continue;
                    }
                    if 10.0 < dist {
                        let pc = self.players[k].v3(0x70);
                        if 0 <= self.item_line_collide(f32::from_bits(0x3dcc_cccd), to, pc, Some(pc), true)? {
                            continue;
                        }
                    }
                    let mut cone = self.fx(i).f32_(fx::CONE);
                    if -1.0 < cone {
                        if dist < mul(add(radius, w)?, f32::from_bits(0x3e99_999a))? {
                            cone = mul(cone, f32::from_bits(0x3f59_999a))?;
                        }
                        if dot_xz(v, fwd)? < cone {
                            continue;
                        }
                    }
                    let push = [mul(v[0], 0.25)?, 0.0, mul(v[2], 0.25)?];
                    self.damage_player(damage, k, 1, df, Some(push))?;
                    if 0xfff < owner_raw {
                        self.critter_damaged_player(k, owner_raw as u16 & 0xfff, damage)?;
                    }
                    if self.fx(i).u32_(fx::DAMAGE_FLAGS) & 0x800 == 0 {
                        if 2.0 < damage {
                            self.players[k].set_f32(0x8c0, add(cool, time)?);
                        }
                    } else {
                        self.players[k].set_f32(0x8c0, add(time, 0.5)?);
                    }
                }
            }
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
                            self.start_hit_fx(hf as usize, self.enemies[k].v3(e::ATTENTION))?;
                        }
                    }
                }
            }
            if result == 0 && !self.critters_empty() {
                let (r, clip) = self.effect_vs_critters(i, mode != 0, radius, from, to, fwd, damage, owner as i32 - 1, hold, age, pierce != 0)?;
                result = r;
                if let Some(p) = clip {
                    to = p;
                    moved = true;
                }
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
                let mut rr = radius;
                if self.fx(i).u32_(fx::DAMAGE_FLAGS) & 0x400 != 0 {
                    rr = sub(radius, 1.5)?;
                    if rr < 0.0 {
                        rr = 0.0;
                    }
                }
                if 0.0 < rr {
                    let env = self.collide_env();
                    for it in self.grid.items_near(&self.items, rr, to)? {
                        let rec = &self.items.records[it];
                        let d = &self.items.definitions[rec.definition()];
                        let content = rec.i16_(0xf0);
                        let bomb = d.kind() == 2
                            && 0 <= content
                            && self.items.definitions.get(content as usize).is_some_and(|c| c.kind() == 4);
                        let skip = if bomb { 0 } else { self.sfx_skip_item(it, self.fx(i).u32_(fx::FLAGS), self.fx(i).u32_(fx::DAMAGE_FLAGS)) };
                        if skip == 1 {
                            continue;
                        }
                        if self.fx(i).u32_(fx::FLAGS) & 0x100_0000 == 0 && time < self.items.records[it].f32_(0xec) {
                            continue;
                        }
                        let rec = &self.items.records[it];
                        let mut dv = v_sub(rec.v3(0x60), to)?;
                        let dist = normal_vector_2d(&mut dv)?;
                        let lim = add(rr, self.items.definitions[rec.definition()].f32_(0xc))?;
                        if lim < dist {
                            continue;
                        }
                        let mut cone = self.fx(i).f32_(fx::CONE);
                        if -1.0 < cone {
                            if dist < mul(lim, 0.2)? {
                                cone = mul(cone, f32::from_bits(0x3f59_999a))?;
                            }
                            if dot_xz(dv, fwd)? < cone {
                                continue;
                            }
                        }
                        let mut out2 = [0.0; 3];
                        let hit = item_coll_sub(&self.items, &mut self.collision, &env, it, from, to, Some(&mut out2), rr, rr)?;
                        if hit < 0.0 {
                            continue;
                        }
                        if self.fx(i).u32_(fx::DAMAGE_FLAGS) & 0x80_0000 != 0 && 0 < owner {
                            self.heal_item(it, owner - 1)?;
                        }
                        if skip == 0 {
                            let h = self.fx(i).i16_(fx::HITS) + 1;
                            self.fx_mut(i).set_i16(fx::HITS, h);
                            let df = self.fx(i).u32_(fx::DAMAGE_FLAGS);
                            let dmg = self.damage_item(damage, it, df, owner as i32 - 1)?;
                            if 0.0 <= dmg && 0 < owner {
                                self.player_damaged_item(owner - 1, it, dmg == 0.0)?;
                            }
                            if 2.0 < damage && self.fx(i).u32_(fx::FLAGS) & 0x80_0000 == 0 {
                                self.items.records[it].set_f32(0xec, add(cool, time)?);
                            }
                        }
                    }
                }
            }
        }
        if result == 0 {
            if self.fx(i).u32_(fx::FLAGS) & 4 != 0 {
                let (wall, water) = self.weapon_wall_hit(mul(radius, 0.5)?, from, to)?;
                if let Some(wh) = wall {
                    let (h, o) = (wh.position, wh.object);
                    splash = water;
                    to = h;
                    moved = true;
                    if self.collision.objects[o].flags & 4 != 0 {
                        to[1] = add(h[1], 2.0)?;
                    }
                    let all = self.collision.runtime_flags.get(o).copied().unwrap_or(self.collision.objects[o].flags);
                    if matches!(all & 0xf_0000, 0x4_0000 | 0x5_0000) {
                        self.world_object_explode(o, h)?;
                    }
                    if self.fx(i).u32_(fx::DAMAGE_FLAGS) & 0x20_0000 == 0 {
                        result = 1;
                    } else {
                        // Bouncing shot: ReflectVector(velocity, wall normal),
                        // upward speed × 0.4, lifetime capped at ATime + 10 or
                        // shortened by 1 s; flight continues. AudioWeaponReflect
                        // and the CreateDirMatrix orientation (flag 0x20000) are
                        // presentation.
                        if self.fx(i).u32_(fx::WALL_SOUND) != 0 {
                            self.audio_weapon_reflect(h)?;
                        }
                        let n = crate::player_collision::hit_normal(&self.collision, &wh)?;
                        let v = self.fx(i).v3(fx::VELOCITY);
                        let dot = add(add(mul(v[0], n[0])?, mul(v[1], n[1])?)?, mul(v[2], n[2])?)?;
                        let k = add(-dot, -dot)?;
                        let mut r = [add(mul(k, n[0])?, v[0])?, add(mul(k, n[1])?, v[1])?, add(mul(k, n[2])?, v[2])?];
                        if 0.0 < r[1] {
                            r[1] = mul(r[1], f32::from_bits(0x3ecc_cccd))?;
                        }
                        self.fx_mut(i).set_v3(fx::VELOCITY, r);
                        let cap = add(atime, 10.0)?;
                        let end = self.fx(i).f32_(fx::END);
                        let e2 = if cap < end { cap } else { sub(end, 1.0)? };
                        self.fx_mut(i).set_f32(fx::END, e2);
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
                // Firework burst: MBPsysFirework is presentation, but its
                // RandInt(6) ×2, RandInt(+0xc4 >> 1) and Random(0.5) advance
                // the generator; the effect then ends (result -2).
                let n = u32::try_from(self.fx(i).i16_(fx::HITS) >> 1).map_err(|_| Unsupported("firework RandInt range".into()))?;
                for k in [6, 6, n] {
                    self.rng.rand_int(k).ok_or_else(|| Unsupported("RandInt(0)".into()))?;
                }
                self.random(0.5)?;
                result = -2;
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
                self.sfx3(0x24, hit_at, 0xb4, 0x78)?;
            } else {
                let s = self.fx(i).u32_(if result == 1 { fx::WALL_SOUND } else { fx::HIT_SOUND });
                if s != 0 {
                    self.audio_effect(s as i32, hit_at)?;
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
                if f & 0x30_0000 != 0 {
                    // PlaceEffectOnFloor 0x17ac30: onto the floor + 0.1 (the
                    // floor-aligned orientation is presentation).
                    let r = self.fx(i).f32_(fx::RADIUS);
                    let (col, _) = self.collision.floor_collide(add(r, 1.0)?, add(r, 5.0)?, -10.0, to, true, 0)?;
                    if let Some(h) = col.solid {
                        to = [h.position[0], add(h.position[1], 0.1)?, h.position[2]];
                    }
                    moved = true;
                }
                if f & 0x400_0000 != 0 {
                    self.boss_generate_enemy(m, to)?;
                    self.fx_mut(i).set_u32(fx::FLAGS, f & 0xfbff_ffff | 0x400);
                }
                let _ = hit_at;
            }
        }
        if sub(self.fx(i).f32_(fx::END), f32::from_bits(0x3d08_7a8d))? <= atime {
            let f = self.fx(i).u32_(fx::FLAGS);
            if f & 0x8_0000 == 0 {
                let next = self.fx(i).i16_(fx::NEXT_FX) as i32;
                if f & 0x4000 == 0 || 0xd8 < (next - 1) as u32 {
                    if f & 0x200_0000 != 0 {
                        // AddItem(3, 0, "BOSSGEN", effect matrix): a boss generator.
                        if let Some(d) = self.items.find_info_index(3, 0, b"BOSSGEN") {
                            let n = self.effects.nodes[i].ok_or_else(|| Unsupported("effect node".into()))?;
                            let at = if moved { to } else { n.position };
                            self.add_item_sub(d, Some((n.matrix, at)))?;
                        }
                    }
                    if f & 0x400_0000 != 0 {
                        self.boss_generate_enemy(m, if moved { to } else { from })?;
                    }
                    if let Some(it) = self.fx(i).u32_(0xec).checked_sub(1).map(|v| v as usize) {
                        // The carried item lands: visible (+0xe1 = 0), kind-1
                        // pickup delay +0x100 = 10, at the effect position.
                        let n = self.effects.nodes[i].ok_or_else(|| Unsupported("effect node".into()))?;
                        let at = if moved { to } else { n.position };
                        let def = self.items.records[it].definition();
                        let r = &mut self.items.records[it];
                        r.set_u8(0xe1, 0);
                        if self.items.definitions[def].kind() == 1 {
                            r.set_i16(0x100, 10);
                        }
                        r.set_f32(0x40, at[0]);
                        r.set_f32(0x44, at[1]);
                        r.set_f32(0x48, at[2]);
                        self.items.place_item(it, &mut self.collision)?;
                    }
                    self.delete_effect(i, true);
                    return Ok(());
                }
                // SfxSetMorph's chain: ChangeEffect to +0xc0; the end is
                // +0x74 after now (or the new tree's length); then +0xc2
                // follows, or the chain ends (flag 0x4000 and the blast
                // cleared); the tree loops.
                self.change_effect(i, next as usize);
                let t = self.fx(i).f32_(fx::NEXT_TIME);
                let end = if t <= 0.0 { add(self.header_duration(next as usize)?, atime)? } else { add(t, atime)? };
                let r = self.fx_mut(i);
                r.set_f32(fx::END, end);
                r.set_f32(fx::DURATION, sub(end, atime)?);
                if r.i16_(fx::NEXT_FX2) < 1 {
                    r.and_u32(fx::FLAGS, 0xffff_bfff);
                    r.set_f32(fx::BLAST, 0.0);
                } else {
                    let second = r.i16_(fx::NEXT_FX2);
                    r.set_i16(fx::NEXT_FX, second);
                    r.set_i16(fx::NEXT_FX2, 0);
                    r.set_f32(fx::NEXT_TIME, 0.0);
                }
                r.set_u16(0x50, 1);
            }
        }
        if moved {
            // CopyMat4(world, node) + UnparentMatrix(node, node +0x74): a
            // parented node keeps the moved transform as its local
            // (trace-H4-boss-v1 step 484, Garm's header 102 under its tree).
            let world = [[m[0][0], m[0][1], m[0][2], 0.0], [m[1][0], m[1][1], m[1][2], 0.0], [m[2][0], m[2][1], m[2][2], 0.0], [to[0], to[1], to[2], 1.0]];
            let local = match (self.effect_critter_parent(i), self.effects.nodes[i].and_then(|n| n.parent)) {
                (Some((c, n)), _) => Some(self.effect_unparent_from_critter(c, n, world)?),
                (None, Some(EffectParent { critter: None, enemy: Some(e), .. })) => {
                    let r = &self.enemies[e];
                    let pm: [[f32; 4]; 4] = std::array::from_fn(|i| std::array::from_fn(|j| r.f32_(0x10 + 0x10 * i + 4 * j)));
                    Some(super::doplayers::unparent_matrix(world, Some(&pm))?)
                }
                // HYPOTHESIS (psfx.rs): the player's record matrix stands for its node chain.
                (None, Some(EffectParent { critter: None, player, .. })) => {
                    let pm = super::doplayers::record_matrix_of(&self.players[player]);
                    Some(super::doplayers::unparent_matrix(world, Some(&pm))?)
                }
                _ => None,
            };
            if let Some(n) = self.effects.nodes[i].as_mut() {
                n.matrix = m;
                n.position = to;
                if let (Some(l), Some(par)) = (local, n.parent.as_mut()) {
                    par.local = [l[3][0], l[3][1], l[3][2]];
                    par.local_matrix = [[l[0][0], l[0][1], l[0][2]], [l[1][0], l[1][1], l[1][2]], [l[2][0], l[2][1], l[2][2]]];
                }
            }
        }
        self.update_fx_streak(i);
        Ok(())
    }

    /// UpdateFXStreak 0x17ae48 (ProcessEffects, a slot with a streak poly)
    /// at the effect's position: the velocity's direction × Cam[0]'s row 2,
    /// normalised and scaled by its length (at least 0.25 either way);
    /// width w = +0xdc; the front pos + velocity × +0xd8/30 at ±0.8w, the
    /// back pos − velocity × min(w/2, the effect's age) at ±0.1w.
    /// Presentation arithmetic (not compared).
    fn update_fx_streak(&mut self, i: usize) {
        let Some(n) = self.effects.nodes[i] else { return };
        let Some(mut st) = n.streak else { return };
        let r = self.fx(i);
        let w = r.f32_(0xdc);
        let (a, b) = (w * 0.8, w * 0.1);
        let age = r.f32_(fx::DURATION) - (r.f32_(fx::END) - self.globals.atime);
        let back = (w * 0.5).min(age);
        let v = r.v3(fx::VELOCITY);
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let d = if len == 0.0 { v } else { [v[0] / len, v[1] / len, v[2] / len] };
        let c = self.std_camera.as_ref().map_or([0.0, 0.0, 1.0], |c| c.cam0_mat.map_or(c.pb.cam_rows[2], |m| m[2]));
        let mut side = [d[1] * c[2] - d[2] * c[1], d[2] * c[0] - d[0] * c[2], d[0] * c[1] - d[1] * c[0]];
        let sl = (side[0] * side[0] + side[1] * side[1] + side[2] * side[2]).sqrt();
        if sl != 0.0 {
            side = side.map(|x| x / sl);
        }
        let k = if (0.0..0.25).contains(&sl) { 0.25 } else if (-0.25..0.0).contains(&sl) { -0.25 } else { sl };
        side = side.map(|x| x * k);
        let f = r.f32_(0xd8) * f32::from_bits(0x3d08_8889);
        let p = n.position;
        let front = [0, 1, 2].map(|j| f * v[j] + p[j]);
        let tail = [0, 1, 2].map(|j| -back * v[j] + p[j]);
        st.verts = [
            [0, 1, 2].map(|j| b * side[j] + tail[j]),
            [0, 1, 2].map(|j| a * side[j] + front[j]),
            [0, 1, 2].map(|j| -a * side[j] + front[j]),
            [0, 1, 2].map(|j| -b * side[j] + tail[j]),
        ];
        if let Some(n) = self.effects.nodes[i].as_mut() {
            n.streak = Some(st);
        }
    }
}

impl Game {
    /// ItemLineCollide 0x157708(radius, from, to, max position, first): the
    /// blocking item along a segment (barrels 0x2b, type-10 breakables and
    /// standing rocks) or -1.
    pub(crate) fn item_line_collide(&mut self, r: f32, from: V3, to: V3, max: Option<V3>, first: bool) -> R<i32> {
        let maxd = match max {
            Some(m) => fqdist(sub(m[0], from[0])?, sub(m[2], from[2])?)?.0,
            None => 1e21,
        };
        let mut best: i32 = -1;
        for k in 0..self.items.records.len() {
            let rec = &self.items.records[k];
            if !rec.active() {
                continue;
            }
            let d = &self.items.definitions[rec.definition()];
            match (d.kind(), d.subtype()) {
                (2, 0x2b) if rec.u8_(0xdc) != 2 => {}
                (10, 0x28 | 0x31 | 0x33 | 0x34 | 0x35) => continue,
                (10, 0x29) if rec.i16_(0xe4) < 1 || rec.i16_(0xf2) < 1 => continue,
                (10, _) => {}
                _ => continue,
            }
            let hit = if d.i16_(8) == 4 {
                let rr = rec.clone();
                crate::item_collision::item_object_col(&mut self.collision, &rr, from, to, r)?.is_some()
            } else {
                let (w, h) = (add(r, d.f32_(0xc))?, add(r, d.f32_(0x10))?);
                super::enemies::line_cylinder_collide(rec.v3(0x60), from, to, w, h, false)?.is_some()
            };
            if hit {
                let p = self.items.records[k].v3(0x60);
                let dd = fqdist(sub(p[0], from[0])?, sub(p[2], from[2])?)?.0;
                if !((0 <= best && 0.0 <= dd) || maxd < dd) {
                    best = k as i32;
                }
            }
            if 0 <= best && first {
                return Ok(best);
            }
        }
        Ok(best)
    }

    /// BossGenerateEnemy 0x19dbe0(effect matrix): Garm (BossType 0x2c)
    /// raises two GRM enemies (type 0x1b, level 3, generate_enemy(1, …,
    /// threshold 1)) 3 units out along the matrix forward yawed by
    /// genang.15 = ±π/4, facing that way, +0x300 = the matrix position.
    fn boss_generate_enemy(&mut self, rows: [V3; 3], at: V3) -> R<()> {
        if self.globals.boss_type != 0x2c {
            return Ok(());
        }
        for a in [f32::from_bits(0x3f49_0fdb), f32::from_bits(0xbf49_0fdb)] {
            let d = super::critter_ai::yaw_vec3(a, rows[2])?;
            let pos = [add(mul(d[0], 3.0)?, at[0])?, add(mul(d[1], 3.0)?, at[1])?, add(mul(d[2], 3.0)?, at[2])?];
            let k = self.generate_enemy(1.0, pos, 0x1b, 3, d, -1, 0, 1)?;
            // NATIVE guard: the original writes through a failed (−1) index.
            let Ok(k) = usize::try_from(k) else { continue };
            let h = crate::libm::atan2f(d[0], d[2])?;
            let r = &mut self.enemies[k];
            r.set_f32(0x260, h);
            r.set_f32(0x264, h);
            r.set_u32(0x254, 0);
            r.set_f32(0x258, h);
            r.set_u32(0x25c, 0);
            r.set_v3(0x300, at);
        }
        Ok(())
    }
}

/// DamageColor 0x190b20: the player colour a potion colour favours.
pub fn damage_color(flags: u32) -> i32 {
    match flags & 0xf {
        1 => 2,
        2 => 1,
        3 => 0,
        4 => 3,
        _ => -1,
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
