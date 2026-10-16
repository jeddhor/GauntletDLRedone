//! Enemy creation and shared queries: generate_enemy 0x139478,
//! find_enemy_slot 0x138e68, init_enemy 0x138c30, init_enemy_vars 0x138980,
//! format_brain 0x1387c8, get_enemy_health/level/fight 0x137ff0/0x137f68/
//! 0x137eb0, enemy_fix_ai 0x139350, gendir 0x138fb8, check_enemy_pos 0x139158,
//! EnemyWallCollide 0x11ef40, PointLineColl 0x11d328, LineCylinderCollide
//! 0x1906a8, EnemyCollidePlayer 0x142738, EnemyCollideEnemy 0x142948,
//! ItemCollide 0x158488, SkipItem 0x158400, EnemyItemCollision 0x159f80,
//! kill_enemy 0x139c58, uncouple_enemy 0x1399a8, enemy_dies 0x139ac8,
//! PlaceEnemyInGrid/StartEnemyGrid/NextGridEnemy 0x1802a0/0x180470/0x180558.
//!
//! Enemy record pointers become indices: +0x298 target player, +0x2a4
//! generator item slot + 1, +0x398 carried item slot + 1. The enemy render
//! node exists whenever generate_enemy accepts a type (enemy_model >= 0), so
//! init_enemy always takes its node branch here; the node/object matrix
//! round trip of GetObjWorldMat is the identity under EnemyRootNode
//! (hypothesis recorded in docs/research/enemies.md).
use super::record::{Record, ENEMY};
use super::{unsupported, Game, Unsupported, R};
use crate::collision::Query;
use crate::item_collision::{item_coll_sub, normal_vector_2d, CollideEnv};
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sqrt_rn, sub_guarded_rz as sub};
use crate::player_collision::collision_position;
use crate::vu0::fqdist;
use gdl_formats::elf::Elf;

type V3 = [f32; 3];

/// Per-type tables (34 entries) and the cathedral/hell random substitutions.
#[derive(Clone, Debug)]
pub struct EnemyTables {
    pub height: Vec<f32>,
    pub width: Vec<f32>,
    pub attn: Vec<f32>,
    pub coll: Vec<f32>,
    pub speed: Vec<f32>,
    pub fight: Vec<f32>,
    pub armor: Vec<f32>,
    pub health: Vec<f32>,
    pub logic_num: Vec<i32>,
    pub damage_type: Vec<u32>,
    pub armor_type: Vec<u32>,
    pub turn: Vec<f32>,
    pub cathedral_enemy: [i32; 4],
    pub cathedral_ai: [i32; 4],
    pub hell_enemy: [i32; 4],
    pub hell_ai: [i32; 4],
}

fn floats(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()
}
fn ints(b: &[u8]) -> Vec<i32> {
    b.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()
}

impl EnemyTables {
    pub fn from_elf(elf: &Elf) -> Result<Self, Box<dyn std::error::Error>> {
        let f = |n: &str| -> Result<Vec<f32>, Box<dyn std::error::Error>> { Ok(floats(elf.object(n)?)) };
        let i = |n: &str| -> Result<Vec<i32>, Box<dyn std::error::Error>> { Ok(ints(elf.object(n)?)) };
        let four = |n: &str| -> Result<[i32; 4], Box<dyn std::error::Error>> {
            Ok(i(n)?.try_into().map_err(|_| format!("{n} size"))?)
        };
        let t = Self {
            height: f("ene_height")?,
            width: f("ene_width")?,
            attn: f("ene_attn")?,
            coll: f("ene_coll")?,
            speed: f("enemy_speed")?,
            fight: f("enemy_fight")?,
            armor: f("enemy_armor")?,
            health: f("enemy_health")?,
            logic_num: i("enemy_logic_num")?,
            damage_type: i("enemy_damagetype")?.into_iter().map(|v| v as u32).collect(),
            armor_type: i("enemy_armortype")?.into_iter().map(|v| v as u32).collect(),
            turn: f("enemy_turnval")?,
            cathedral_enemy: four("cathedral_rand_enemy")?,
            cathedral_ai: four("cathedral_rand_ai")?,
            hell_enemy: four("hell_rand_enemy")?,
            hell_ai: four("hell_rand_ai")?,
        };
        if t.height.len() != 34 || t.logic_num.len() != 34 {
            return Err("enemy table size".into());
        }
        Ok(t)
    }
}

/// Load-time per-type availability (enemy_model >= 0, enemy_loaded,
/// enemy_genfx), 45 entries each; captured from the level load.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EnemyLoad {
    pub model: Vec<i32>,
    pub loaded: Vec<i32>,
    pub genfx: Vec<i32>,
}

pub mod e {
    pub const TYPE: usize = 0x000;
    pub const POSITION: usize = 0x40;
    pub const ATTENTION: usize = 0x50;
    pub const COLLISION: usize = 0x60;
    pub const OFFSET_FLAGS: usize = 0x74;
    pub const STATE: usize = 0xc8;
    pub const HEALTH: usize = 0x214;
    pub const GRID_NEXT: usize = 0x210;
    pub const ATTN_OFFSET: usize = 0x234;
    pub const COL_OFFSET: usize = 0x240;
    pub const WIDTH: usize = 0x24c;
    pub const HALF_HEIGHT: usize = 0x250;
    pub const TARGET: usize = 0x298;
    pub const GENERATOR: usize = 0x2a4;
    pub const LOGIC: usize = 0x324;
    pub const PREV: usize = 0x348;
    pub const NEXT: usize = 0x34c;
    pub const CARRIED: usize = 0x398;
}

/// gendir: one of eight spawn directions; returns its angle.
pub fn gendir(d: V3, i: u32) -> R<(V3, f32)> {
    let k = f32::from_bits(0x3f34_fdf4);
    let (x, z) = (d[0], d[2]);
    let w = f32::from_bits;
    Ok(match i {
        1 => ([-x, d[1], -z], w(0x4049_0fdb)),
        2 => ([-z, d[1], x], w(0xbfc9_0fdb)),
        3 => ([z, d[1], -x], w(0x3fc9_0fdb)),
        4 => {
            let zk = mul(z, k)?;
            ([add(mul(x, k)?, zk)?, d[1], add(mul(-x, k)?, zk)?], w(0x3f49_0fdb))
        }
        5 => {
            let xk = mul(x, k)?;
            ([add(xk, mul(-z, k)?)?, d[1], add(xk, mul(z, k)?)?], w(0xbf49_0fdb))
        }
        6 => {
            let nx = mul(-x, k)?;
            ([add(nx, mul(z, k)?)?, d[1], add(nx, mul(-z, k)?)?], w(0x4016_cbe5))
        }
        7 => {
            let nz = mul(-z, k)?;
            ([add(mul(-x, k)?, nz)?, d[1], add(mul(x, k)?, nz)?], w(0xc016_cbe5))
        }
        _ => ([x, d[1], z], 0.0),
    })
}

/// enemy_fix_ai(type, level, ai): consumes one rand() for the A group.
pub fn enemy_fix_ai(g: &mut Game, t: i32, level: i32, ai: i32) -> i32 {
    const A: [i32; 9] = [0, 3, 6, 9, 12, 15, 18, 21, 22];
    const B: [i32; 5] = [2, 7, 14, 17, 24];
    const C: [i32; 13] = [1, 4, 5, 8, 10, 11, 13, 16, 19, 20, 25, 32, 33];
    if A.contains(&t) {
        if ai == 2 || ai == 4 {
            return ai;
        }
        // RandInt(1) is always 0 but still advances the generator.
        let r = g.rng.rand_int(1).unwrap_or(0);
        return if r != 0 { 4 } else { 2 };
    }
    if B.contains(&t) {
        if ai != 0 {
            return ai;
        }
        return match level {
            4 => 0x17,
            3 => 0x1e,
            5 => 0x11,
            6 => 0x12,
            _ => 7,
        };
    }
    if C.contains(&t) {
        if ai != 0 {
            return ai;
        }
        return match level {
            5 => 0x11,
            4 => 0x17,
            6 => 0x12,
            _ => 7,
        };
    }
    match t {
        27 => 0x1f,
        29 => 0x13,
        30 => 3,
        31 => 0x1b,
        _ => ai,
    }
}

/// PointLineColl(point, a, b): distance to segment a..b and the closest point.
pub fn point_line_coll(p: V3, a: V3, b: V3) -> R<(f32, V3)> {
    let mut d = [sub(b[0], a[0])?, sub(b[1], a[1])?, sub(b[2], a[2])?];
    let len = crate::collision::normal_vector_pub(&mut d)?;
    let e = [sub(p[0], a[0])?, sub(p[1], a[1])?, sub(p[2], a[2])?];
    let t = add(add(mul(e[0], d[0])?, mul(e[1], d[1])?)?, mul(e[2], d[2])?)?;
    let dist3 = |v: V3| -> R<f32> { Ok(fqdist(fqdist(v[0], v[2])?.0, v[1])?.0) };
    if t <= 0.0 {
        return Ok((dist3(e)?, a));
    }
    if len <= t {
        let e2 = [sub(p[0], b[0])?, sub(p[1], b[1])?, sub(p[2], b[2])?];
        return Ok((dist3(e2)?, b));
    }
    let c = [add(mul(t, d[0])?, a[0])?, add(mul(t, d[1])?, a[1])?, add(mul(t, d[2])?, a[2])?];
    let e3 = [sub(p[0], c[0])?, sub(p[1], c[1])?, sub(p[2], c[2])?];
    Ok((dist3(e3)?, c))
}

/// LineCylinderCollide(center, from, to, radius, half height, directional).
pub fn line_cylinder_collide(center: V3, from: V3, to: V3, r: f32, h: f32, directional: bool) -> R<Option<V3>> {
    let (d, c) = point_line_coll(center, from, to)?;
    if add(r, h)? < d {
        return Ok(None);
    }
    let e = [sub(c[0], center[0])?, sub(c[1], center[1])?, sub(c[2], center[2])?];
    if r < fqdist(e[0], e[2])?.0 || h < e[1].abs() {
        return Ok(None);
    }
    if directional {
        let mut a = [sub(center[0], from[0])?, 0.0, sub(center[2], from[2])?];
        let la = normal_vector_2d(&mut a)?;
        if la <= r {
            let mut b = [sub(to[0], from[0])?, 0.0, sub(to[2], from[2])?];
            let lb = normal_vector_2d(&mut b)?;
            let tiny = f32::from_bits(0x3a83_126f);
            if la < tiny {
                if lb >= tiny {
                    return Ok(None);
                }
            } else if add(mul(b[0], a[0])?, mul(b[2], a[2])?)? < f32::from_bits(0xbc23_d70a) {
                return Ok(None);
            }
        }
    }
    Ok(Some(c))
}

impl Game {
    pub fn enemy(&self, i: usize) -> &Record {
        &self.enemies[i]
    }
    fn table_f(&self, v: &[f32], t: i32) -> R<f32> {
        usize::try_from(t).ok().and_then(|i| v.get(i)).copied().ok_or_else(|| Unsupported(format!("enemy type {t} table")))
    }

    pub fn get_enemy_health(&self, t: i32, level: i32) -> R<f32> {
        let mut h = self.table_f(&self.enemy_tables.health, t)?;
        if t != 0x1e {
            h = mul(h, self.level_f32(0xac))?;
        }
        if (t as u32) < 0x1c {
            h = mul(mul(h, f32::from_bits(0x3eaa_7efa))?, level as f32)?;
        }
        Ok(h)
    }
    pub fn get_enemy_level(&self, t: i32, health: f32) -> R<i16> {
        let h = mul(self.table_f(&self.enemy_tables.health, t)?, self.level_f32(0xac))?;
        if mul(h, f32::from_bits(0x3f2a_c083))? < health {
            return Ok(3);
        }
        if mul(h, f32::from_bits(0x3eaa_7efa))? < health {
            return Ok(2);
        }
        Ok(i16::from(0.0 < health))
    }
    pub fn get_enemy_fight(&self, health: f32, t: i32) -> R<f32> {
        let f = mul(self.table_f(&self.enemy_tables.fight, t)?, self.level_f32(0xbc))?;
        let h = mul(self.table_f(&self.enemy_tables.health, t)?, self.level_f32(0xac))?;
        if mul(h, f32::from_bits(0x3f2a_c083))? < health || t == 0x1e {
            return Ok(f);
        }
        if mul(h, f32::from_bits(0x3eaa_7efa))? < health {
            return mul(f, f32::from_bits(0x3f2a_c083)).map_err(Into::into);
        }
        mul(f, f32::from_bits(0x3eaa_7efa)).map_err(Into::into)
    }

    /// PlayersAverageLevel: mean level (+0x2ac0) of active players, else 1.
    pub fn players_average_level(&self) -> i32 {
        let (n, sum) = self.players.iter().filter(|p| p.i32_(0xfc) == 1).fold((0, 0), |(n, s), p| (n + 1, s + p.i32_(0x2ac0)));
        if n == 0 { 1 } else { sum / n }
    }

    /// format_brain.
    pub(crate) fn format_brain(&mut self, i: usize) {
        let logic = self.enemies[i].i16_(e::LOGIC);
        let mut r = |n: u32| self.rng.rand_int(n).unwrap_or(0) as i32;
        let mut w: Vec<(usize, u32, i32)> = vec![(0x310, 4, 0x404a_2dd0), (0x344, 4, 0)];
        let mut or344 = 0;
        match logic {
            0 | 0x1b => w.extend([(0x368, 4, 1), (0x378, 2, 0)]),
            2 | 4 => {
                w.push((0x388, 4, 0));
                or344 = 1;
            }
            5 | 6 => or344 = 1,
            3 => w.extend([(0x368, 4, 1), (0x378, 2, 0), (0x338, 4, 0), (0x33c, 4, -1), (0x334, 4, 0)]),
            7 | 10 | 0xd | 0x14 => w.extend([(0x368, 4, 0), (0x378, 2, 0), (0x37a, 2, 0)]),
            8 => w.extend([(0x368, 4, 0), (0x378, 2, 0), (0x37a, 2, 0), (0x350, 4, 0), (0x354, 4, -1), (0x358, 4, 0x47c3_5000)]),
            9 => {
                w.push((0xe4, 4, 0));
                or344 = 5;
            }
            0xb => or344 = 7,
            0xc => w.extend([(0x2f4, 4, 0), (0x2f8, 4, 0), (0x2fc, 4, 0), (0x368, 4, 0), (0x378, 2, 0), (0x37a, 2, 0)]),
            0xe => w.extend([(0x368, 4, 0), (0x378, 2, 0), (0x37a, 2, 0), (0x338, 4, 0)]),
            0x15 | 0x18 => w.push((0x338, 4, 0)),
            0x10 | 0x11 | 0x17 | 0x1a => {
                let v = r(10);
                w.extend([(0x334, 4, v), (0x338, 4, 0)]);
            }
            0x1c | 0x1d | 0x1f => {
                let v = r(0x1e);
                w.extend([(0x334, 4, v), (0x338, 4, 0), (0x33c, 4, 0)]);
            }
            0x1e => {
                w.extend([(0x368, 4, 1), (0x378, 2, 0)]);
                let v = r(0x3c);
                w.push((0x338, 4, v + 0x3c));
            }
            _ => {}
        }
        let avg = self.players_average_level();
        let rec = &mut self.enemies[i];
        for (o, size, v) in w {
            if size == 4 {
                rec.set_i32(o, v);
            } else {
                rec.set_i16(o, v as i16);
            }
        }
        rec.or_u32(0x344, or344);
        for (o, v) in [(0x370, 0), (0x36c, 0), (0x35c, -1), (0x360, 0), (0x364, 4)] {
            rec.set_i32(o, v);
        }
        rec.set_i16(0x2ea, 0);
        rec.set_i32(0x384, avg);
        rec.set_i32(0x37c, 8);
        let v = self.rng.rand_int(8).unwrap_or(0) as i32;
        self.enemies[i].set_i32(0x380, v);
    }

    /// init_enemy_vars(slot, ai) with the initial health in f12.
    fn init_enemy_vars(&mut self, i: usize, health: f32, ai: i32) -> R<()> {
        let t = self.enemies[i].i32_(e::TYPE);
        let height = self.table_f(&self.enemy_tables.height, t)?;
        let width = self.table_f(&self.enemy_tables.width, t)?;
        let aggr = mul(self.level_f32(0xb4), 30.0)?;
        let level = self.get_enemy_level(t, health)?;
        let rec = &mut self.enemies[i];
        for o in [0x1f8, 0xe0, 0x21c, 0x220, 0x2a0, 0x2ac, 0x370, 0x2e4, 0x2b4, 0x2b8, 0x2bc, 0x2c0, 0x2c4, 0x270, 0x274, 0x278, 0x27c, 0x284, 0x2a4, 0x390, 0x394, 0x334, 0x330, 0x33c, 0x338, 0x340, 0x31c, 0x318, 0x398, 0x39c] {
            rec.set_u32(o, 0);
        }
        for o in (0x2c8..0x2dc).step_by(4) {
            rec.set_u32(o, 0);
        }
        for o in [0x34c, 0x348, 0x298, 0x29c, 0x3a4, 0x1f4] {
            rec.set_i32(o, -1);
        }
        for o in [0x28a, 0x288, 0x2e0] {
            rec.set_i16(o, -1);
        }
        for o in [0x212, 0x294, 0x296, 0x2e2, 0x218, 0x32c, 0x32a, 0x2f2] {
            rec.set_i16(o, 0);
        }
        rec.set_f32(0x314, aggr);
        rec.set_f32(0x28c, 1.0);
        rec.set_f32(0x290, 1.0);
        rec.set_f32(e::HALF_HEIGHT, mul(height, 0.5)?);
        rec.set_f32(e::WIDTH, width);
        rec.set_f32(0x38c, 1.0);
        rec.set_i16(0x21a, level);
        rec.set_u32(0x320, 0xc479_f99a);
        rec.set_i16(0x2ee, 1);
        rec.set_i16(0x2f0, 1);
        let ai = match ai {
            1 => 0,
            10 => 7,
            a => a,
        };
        let logic = if (ai as u32) < 0x20 { ai as i16 } else { self.enemy_tables.logic_num[t as usize] as i16 };
        let rec = &mut self.enemies[i];
        rec.set_i16(e::LOGIC, logic);
        rec.set_i16(0x326, logic);
        rec.set_i16(0x328, logic);
        self.format_brain(i);
        let speed = self.table_f(&self.enemy_tables.speed, t)?;
        let fight = self.get_enemy_fight(self.enemies[i].f32_(e::HEALTH), t)?;
        let tt = t as usize;
        let (armor, dt, at) = (self.enemy_tables.armor[tt], self.enemy_tables.damage_type[tt], self.enemy_tables.armor_type[tt]);
        let rec = &mut self.enemies[i];
        rec.set_f32(0xcc, div(1.0, speed)?);
        rec.set_f32(0xd0, fight);
        rec.set_f32(0xd4, armor);
        rec.set_u32(0xd8, dt);
        rec.set_u32(0xdc, at);
        Ok(())
    }

    /// UpdateObjOffsets on the enemy object block (+0x10).
    pub fn enemy_update_offsets(&mut self, i: usize) -> R<()> {
        let rec = &mut self.enemies[i];
        let m: [V3; 3] = std::array::from_fn(|r| rec.v3(0x10 + 16 * r));
        let p = rec.v3(e::POSITION);
        let flags = rec.u32_(e::OFFSET_FLAGS);
        rec.set_v3(e::COLLISION, collision_position(p, rec.v3(e::COL_OFFSET), flags, &m)?);
        rec.set_v3(e::ATTENTION, collision_position(p, rec.v3(e::ATTN_OFFSET), flags, &m)?);
        Ok(())
    }

    /// init_enemy(slot, position, type, level, ai).
    fn init_enemy(&mut self, i: usize, pos: V3, t: i32, mut level: i32, ai: i32) -> R<()> {
        let attn = self.table_f(&self.enemy_tables.attn, t)?;
        let coll = self.table_f(&self.enemy_tables.coll, t)?;
        {
            let rec = &mut self.enemies[i];
            rec.set_i32(e::TYPE, t);
            rec.set_v3(e::ATTN_OFFSET, [0.0, attn, 0.0]);
            rec.set_v3(e::COL_OFFSET, [0.0, coll, 0.0]);
            for o in [0x254, 0x258, 0x25c] {
                rec.set_u32(o, 0);
            }
            rec.set_i32(e::STATE, 1);
            rec.set_i16(0x2e8, 0);
            // SetEnemyObj: presentation nodes; type 0x1f sets +0x230 = 3.0.
            rec.set_u32(0x230, if t == 0x1f { 0x4040_0000 } else { 0 });
        }
        self.attach_enemy_anim(i, t, level)?;
        if 4 <= level {
            level = 2;
        }
        if ai == 0x12 {
            level = 1;
        }
        let health = self.get_enemy_health(t, level)?;
        self.enemy_update_offsets(i)?;
        let (y, _) = self.collision.floor_pos(self.globals.world_bottom, f32::from_bits(0x3dcc_cccd), pos, 2)?;
        {
            let rec = &mut self.enemies[i];
            rec.set_v3(e::POSITION, [pos[0], y, pos[2]]);
            rec.set_f32(e::HEALTH, health);
        }
        self.init_enemy_vars(i, health, ai)?;
        let rec = &mut self.enemies[i];
        if t == 0x1e {
            rec.set_i16(0x21a, level as i16);
        }
        rec.set_f32(0x44, add(rec.f32_(0x44), rec.f32_(0x230))?);
        self.enemy_update_offsets(i)?;
        let y = self.enemies[i].f32_(0x44);
        self.enemies[i].set_f32(0x2a8, y);
        Ok(())
    }

    /// find_enemy_slot(type, visibility threshold).
    fn find_enemy_slot(&mut self, t: i32, threshold: i16) -> R<i32> {
        let mut best = 0f32;
        let mut best_slot = 0usize;
        let mut best_visible = 1i16;
        for (k, rec) in self.enemies.iter().enumerate() {
            if rec.i32_(e::STATE) == 0 {
                return Ok(k as i32);
            }
            if rec.i32_(e::TYPE) == 0x1f {
                continue;
            }
            let mut v = rec.f32_(0x290);
            let near = rec.i16_(0x2f0);
            let st = rec.i32_(e::STATE);
            if st == 8 || st == 6 || rec.i16_(0x2ec) != 0 {
                v = mul(v, f32::from_bits(0x3c23_d70a))?;
            } else if near == 0 {
                v = add(v, 10000.0)?;
            }
            if best < v {
                best = v;
                best_slot = k;
                best_visible = near;
            }
        }
        self.no_enemy_slot = true;
        if (t as u32) < 0x1c && threshold < best_visible {
            return Ok(-1);
        }
        self.kill_enemy(best_slot)?;
        self.enemies[best_slot].set_f32(0x28c, 1.0);
        self.enemies[best_slot].set_f32(0x290, 1.0);
        Ok(best_slot as i32)
    }

    /// generate_enemy(distance, position, type, level, direction, ai,
    /// generator slot + 1, visibility threshold). Negative results mirror the
    /// original codes (-1 attract, -2 no slot, -3 blocked, -4 time stop/boss, -5 not loaded).
    #[allow(clippy::too_many_arguments)]
    pub fn generate_enemy(&mut self, dist: f32, pos: V3, mut t: i32, mut level: i32, dir: V3, ai: i32, generator: u32, threshold: i16) -> R<i32> {
        if self.globals.game_mode == 0x8007 {
            return Ok(-1);
        }
        if (self.stop_time != 0 && generator != 0) || (0 <= self.globals.boss_type && self.boss_is_dying != 0) {
            return Ok(-4);
        }
        let mut ai = enemy_fix_ai(self, t, level, ai);
        if t == -2 || t == -3 {
            let k = self.rng.rand_int(4).unwrap_or(0) as usize;
            let (en, a) = if t == -2 { (self.enemy_tables.cathedral_enemy, self.enemy_tables.cathedral_ai) } else { (self.enemy_tables.hell_enemy, self.enemy_tables.hell_ai) };
            level = if t == -2 { 2 } else { 3 };
            t = en[k];
            ai = a[k];
        }
        let ti = t as usize;
        let model_ok = self.enemy_load.model.get(ti).is_some_and(|m| 0 <= *m)
            && (self.enemy_load.loaded.get(ti) != Some(&4) || 3 < level);
        if !((t as u32).wrapping_sub(0x1e) < 2 || model_ok) {
            return Ok(-5);
        }
        let slot = self.find_enemy_slot(t, threshold)?;
        if slot < 0 {
            return Ok(-2);
        }
        let s = slot as usize;
        self.init_enemy(s, pos, t, level, ai)?;
        self.enemies[s].set_u32(e::GENERATOR, generator);
        let finish = |g: &mut Game| -> R<i32> {
            g.enemy_update_offsets(s)?;
            g.enemies[s].set_i32(0xe0, 1);
            let genfx = g.enemy_load.genfx.get(ti).copied().unwrap_or(0);
            if 2.0 < g.enemies[s].f32_(e::HALF_HEIGHT) && level < 4 && genfx != 0 {
                g.audio("StartGenFX", None, level);
            }
            Ok(slot)
        };
        if generator == 0 || t == 0x1e {
            self.enemies[s].set_f32(0x26c, 0.0);
            return finish(self);
        }
        let (mut n, mut blocked) = match t {
            1 | 4 | 5 | 7 | 8 | 10 | 0xb | 0xe | 0xf | 0x13 | 0x18 | 0x19 => (8, 0xffceu32),
            0x12 => (2, 0),
            _ => (8, 0),
        };
        if ai == 0xc {
            n = 1;
        }
        let rec = &self.enemies[s];
        let start = [pos[0], add(pos[1], rec.f32_(0x244))?, pos[2]];
        let reach = if t == 0x11 { 0.0 } else { add(dist, rec.f32_(e::WIDTH))? };
        let base = [mul(reach, dir[0])?, 0.0, mul(reach, dir[2])?];
        let first = self.rng.rand_int(n).unwrap_or(0);
        let mut k = first;
        loop {
            if (blocked as i32 >> (k & 31)) & 1 == 0 {
                let (offset, angle) = gendir(base, k)?;
                self.enemies[s].set_f32(0x26c, angle);
                let r = self.check_enemy_pos(start, Some(offset), s)?;
                if 0 < r {
                    return finish(self);
                }
                if r < 0 {
                    blocked |= (1 << (k & 31)) & 0xffff;
                }
            }
            k = (k + 1) % n;
            if k == first {
                break;
            }
        }
        self.enemies[s].set_i32(e::STATE, 0);
        Ok(-3)
    }

    /// check_enemy_pos(position, offset, slot): 1 free, 0 occupied, -1 blocked.
    fn check_enemy_pos(&mut self, pos: V3, offset: Option<V3>, i: usize) -> R<i32> {
        let r = self.enemies[i].f32_(e::WIDTH);
        let h = self.enemies[i].f32_(e::HALF_HEIGHT);
        let p = match offset {
            None => pos,
            Some(o) => {
                let q = [add(o[0], pos[0])?, add(o[1], pos[1])?, add(o[2], pos[2])?];
                if self.enemy_wall_collide(r, pos, q)?.is_some() {
                    return Ok(-1);
                }
                q
            }
        };
        self.enemies[i].set_v3(e::POSITION, p);
        let (c, _) = self.collision.floor_collide(f32::from_bits(0x3dcc_cccd), 4.0, -10.0, p, false, 2)?;
        let Some(hit) = c.solid else { return Ok(-1) };
        if 6.0 < sub(hit.position[1], pos[1])?.abs() {
            return Ok(-1);
        }
        let mut p = p;
        p[1] = hit.position[1];
        self.enemies[i].set_f32(0x44, hit.position[1]);
        // UpdateObjColOffset only (+0x60).
        {
            let rec = &mut self.enemies[i];
            let m: [V3; 3] = std::array::from_fn(|r| rec.v3(0x10 + 16 * r));
            let cp = collision_position(rec.v3(e::POSITION), rec.v3(e::COL_OFFSET), rec.u32_(e::OFFSET_FLAGS), &m)?;
            rec.set_v3(e::COLLISION, cp);
        }
        if 0 <= self.enemy_collide_player(r, h, i, true, pos, p)? {
            return Ok(0);
        }
        if 0 <= self.enemy_collide_enemy(mul(r, 0.5)?, h, i, pos, p, false)? {
            return Ok(0);
        }
        let Some(item) = self.item_collide(mul(r, 0.5)?, pos, p, None, false)? else { return Ok(1) };
        if self.enemies[i].u32_(e::GENERATOR) == item as u32 + 1 {
            return Ok(1);
        }
        if !self.enemy_item_collision(None, item)? {
            return Ok(1);
        }
        Ok(0)
    }

    /// EnemyWallCollide(radius, from, to): the wall hit position.
    pub fn enemy_wall_collide(&mut self, r: f32, from: V3, to: V3) -> R<Option<V3>> {
        let (c, _) = self.collision.world_collide(Query {
            radius: r,
            first: from,
            last: to,
            check_flags: 7,
            min_normal_y: f32::from_bits(0xbf5d_b22d),
            max_normal_y: f32::from_bits(0x3f5d_b22d),
            object_mask: 0x13a,
            exclusion_mask: 2,
        })?;
        self.enemy_wall_hit = c.solid.map(|h| h.object);
        Ok(c.solid.map(|h| h.position))
    }

    /// get_actual_closest_player(enemy): nearest active player by 3D distance.
    pub fn get_actual_closest_player(&self, i: usize) -> R<i32> {
        let c = self.enemies[i].v3(e::COLLISION);
        let mut best = 100_000.0;
        let mut who = -1;
        for (k, pl) in self.players.iter().enumerate() {
            if pl.i32_(0xfc) != 1 {
                continue;
            }
            let p = pl.v3(0x70);
            let d = [sub(c[0], p[0])?, sub(c[1], p[1])?, sub(c[2], p[2])?];
            let s = sqrt_rn(add(add(mul(d[0], d[0])?, mul(d[1], d[1])?)?, mul(d[2], d[2])?)?)?;
            if s < best {
                best = s;
                who = k as i32;
            }
        }
        Ok(who)
    }

    /// EnemyCollidePlayer(r, h, enemy, all players, from, to): nearest hit player.
    pub fn enemy_collide_player(&mut self, r: f32, h: f32, i: usize, all: bool, from: V3, to: V3) -> R<i32> {
        let (first, last) = if all {
            (0, 3)
        } else {
            if self.enemies[i].i16_(0x288) < 0 {
                return Ok(-1);
            }
            let c = self.get_actual_closest_player(i)?;
            (c, c)
        };
        let mut best = 100_000.0;
        let mut who = -1;
        if first < 0 {
            // No active player: the original would read players[-1].
            return Ok(-1);
        }
        for k in first..=last {
            let pl = &self.players[k as usize];
            if pl.i32_(0xfc) != 1 {
                continue;
            }
            let center = pl.v3(0x70);
            if let Some(c) = line_cylinder_collide(center, from, to, add(r, pl.f32_(0x828))?, add(h, pl.f32_(0x82c))?, true)? {
                let d = fqdist(sub(c[0], to[0])?, sub(c[2], to[2])?)?.0;
                if d < best {
                    best = d;
                    who = k;
                }
            }
        }
        Ok(who)
    }

    /// EnemyCollideEnemy(r, h, enemy, from, to, any): nearest blocking enemy.
    /// Critters (CritterLineCollide) are not present in the native world yet.
    pub fn enemy_collide_enemy(&mut self, r: f32, h: f32, i: usize, from: V3, to: V3, any: bool) -> R<i32> {
        if !self.critters_empty() {
            return unsupported("CritterLineCollide");
        }
        let favoured = self.enemies[i].i32_(0x29c);
        let mut forced = if !any && favoured < 0x10000 { favoured } else { -1 };
        let mut best = 100_000.0;
        let mut out = -1;
        let near = self.enemies_near(r, to)?;
        let mut it = near.into_iter();
        loop {
            let k = if 0 <= forced {
                let f = forced;
                forced = -1;
                f
            } else {
                match it.next() {
                    Some(k) => k as i32,
                    None => break,
                }
            };
            let ku = k as usize;
            if ku == i || ku >= self.enemies.len() {
                continue;
            }
            let o = &self.enemies[ku];
            let st = o.i32_(e::STATE);
            if st == 0 || st == 8 || self.is_tail(i, ku) || (o.f32_(e::HALF_HEIGHT) <= 2.0 && o.i32_(e::TYPE) == 0x1d) {
                continue;
            }
            let c = o.v3(e::COLLISION);
            let mut d = [sub(c[0], to[0])?, sub(c[1], to[1])?, sub(c[2], to[2])?];
            let dist = normal_vector_2d(&mut d)?;
            if best <= dist {
                continue;
            }
            let o = &self.enemies[ku];
            if line_cylinder_collide(c, from, to, add(r, o.f32_(e::WIDTH))?, add(h, o.f32_(e::HALF_HEIGHT))?, true)?.is_some() {
                best = dist;
                out = k;
                if !any && k == favoured {
                    break;
                }
            }
        }
        Ok(out)
    }

    fn is_tail(&self, i: usize, other: usize) -> bool {
        let mut k = self.enemies[i].i32_(e::NEXT);
        let mut guard = 0;
        while 0 <= k {
            if k as usize == other {
                return true;
            }
            k = self.enemies[k as usize].i32_(e::NEXT);
            guard += 1;
            if guard > self.enemies.len() {
                return false;
            }
        }
        false
    }

    /// SkipItem: items enemies ignore.
    pub fn skip_item(&self, slot: usize) -> bool {
        let r = &self.items.records[slot];
        match self.items.definitions[r.definition()].kind() {
            8 => {
                let s = r.i8_(0xdc);
                if s == 2 || s == 4 { r.u16_(0xd8) & 1 == 0 } else { true }
            }
            1 => r.u32_(0xfc) != 0,
            13 => true,
            _ => false,
        }
    }

    /// ItemCollide(r, from, to, out, nearest): the first colliding item, or
    /// the nearest one when `nearest` (the original's a3) is set.
    pub fn item_collide(&mut self, r: f32, from: V3, to: V3, mut out: Option<&mut V3>, nearest: bool) -> R<Option<usize>> {
        let h = mul(r, 1.5)?;
        let near = self.grid.items_near(&self.items, r, to)?;
        let env = self.collide_env();
        let mut best = 100_000.0;
        let mut found = None;
        for slot in near {
            if self.skip_item(slot) {
                continue;
            }
            let d = item_coll_sub(&self.items, &mut self.collision, &env, slot, from, to, out.as_deref_mut(), r, h)?;
            if 0.0 <= d && d < best {
                best = d;
                found = Some(slot);
                if !nearest {
                    break;
                }
            }
        }
        Ok(found)
    }

    pub fn collide_env(&self) -> CollideEnv<'static> {
        CollideEnv { num_in_game_players: self.globals.num_in_game_players, any_player_has_all_crystals: &|_| false }
    }

    /// EnemyItemCollision(enemy or none, item): whether the item blocks.
    pub fn enemy_item_collision(&mut self, enemy: Option<usize>, slot: usize) -> R<bool> {
        let r = self.items.records[slot].clone();
        let d = &self.items.definitions[r.definition()];
        let (kind, sub_) = (d.kind(), d.subtype());
        let et = enemy.map(|k| self.enemies[k].i32_(e::TYPE));
        Ok(match kind {
            1 | 5 | 9 | 0xb | 0xc => false,
            2 => match et {
                None => true,
                Some(0x1d) => false,
                Some(0x20) => false,
                Some(_) => true,
            },
            3 => match et {
                None => true,
                Some(t) => {
                    let mut block = r.u8_(0xf6) != 0;
                    if r.i16_(0xf0) == 0x11 {
                        block = false;
                    }
                    if (t == 0x1d || t == 0x20) && d.f32_(0x10) <= 3.0 {
                        false
                    } else {
                        block
                    }
                }
            },
            8 => match et {
                None => true,
                Some(t) if (t as u32).wrapping_sub(0x1d) < 2 => false,
                Some(0x20) => false,
                Some(t) if t != 3 && t != 0 => true,
                Some(_) => {
                    let s = r.i8_(0xdc);
                    if s != 2 && s != 4 {
                        false
                    } else {
                        let k = enemy.unwrap();
                        let pos = self.enemies[k].v3(e::ATTENTION);
                        self.damage_enemy(r.f32_(0xf0), k, -1, 0, Some(pos), None, 2)?;
                        false
                    }
                }
            },
            10 => !matches!(sub_, 0x28 | 0x31 | 0x33 | 0x34 | 0x35),
            _ => true,
        })
    }

    /// uncouple_enemy: unlink from a chain, release a suicide bomber and the generator count.
    pub fn uncouple_enemy(&mut self, i: usize) {
        let (prev, next) = (self.enemies[i].i32_(e::PREV), self.enemies[i].i32_(e::NEXT));
        if 0 <= prev {
            self.enemies[prev as usize].set_i32(e::NEXT, next);
            self.enemies[i].set_i32(e::PREV, -1);
        }
        let (prev, next) = (self.enemies[i].i32_(e::PREV), self.enemies[i].i32_(e::NEXT));
        if 0 <= next {
            self.enemies[next as usize].set_i32(e::PREV, prev);
            self.enemies[i].set_i32(e::NEXT, -1);
        }
        let logic = self.enemies[i].i16_(e::LOGIC);
        if logic == 0x12 {
            self.suicide_bomber_index = -1;
            self.enemies[i].set_i16(e::LOGIC, logic.wrapping_neg());
        }
        if let Some(g) = self.enemies[i].u32_(e::GENERATOR).checked_sub(1) {
            let g = g as usize;
            let r = &mut self.items.records[g];
            if r.i8_(0xf5) == i as i8 {
                r.set_u8(0xf5, self.enemies[i].u8_(e::PREV));
            }
            if self.enemies[i].i16_(e::LOGIC) == 0xf {
                r.set_u8(0xf7, 0);
                r.set_u8(0xf2, 0);
            }
            let kind = self.items.definitions[r.definition()].kind();
            if kind == 3 && 0 < r.i8_(0xf2) {
                r.set_i8(0xf2, r.i8_(0xf2) - 1);
            }
            self.enemies[i].set_u32(e::GENERATOR, 0);
        }
    }

    /// enemy_dies(enemy, killer).
    pub fn enemy_dies(&mut self, i: usize, killer: i16) -> R<()> {
        let rec = &mut self.enemies[i];
        rec.set_f32(e::HEALTH, 0.0);
        rec.set_i32(e::STATE, 8);
        rec.set_i16(0x212, killer);
        if rec.i16_(e::LOGIC) == 0x12 {
            let pos = self.enemies[i].v3(e::COLLISION);
            let dmg = mul(self.level_f32(0xbc), 50.0)?;
            self.suicide_explosion(dmg, pos)?;
            self.sfx3(0x38, pos, 0xe0, 0x32)?;
        }
        self.uncouple_enemy(i);
        Ok(())
    }

    /// kill_enemy: free the slot, dropping a carried item.
    pub fn kill_enemy(&mut self, i: usize) -> R<()> {
        if self.globals.trig_cam_active != 0 {
            return Ok(());
        }
        let carried = self.enemies[i].u32_(e::CARRIED);
        let t = self.enemies[i].i32_(e::TYPE);
        if carried != 0 {
            let slot = carried as usize - 1;
            self.enemies[i].set_u32(e::CARRIED, 0);
            self.items.records[slot].set_u8(0xe1, 10);
            self.audio("StartBagFX", None, slot as i32);
        } else if t == 0x20 {
            return unsupported("gargoyle drop (AddItem GARG…)");
        }
        let rec = &mut self.enemies[i];
        rec.set_f32(e::HEALTH, 0.0);
        rec.set_i32(e::STATE, 0);
        self.uncouple_enemy(i);
        Ok(())
    }

    /// StartEnemyGrid/NextGridEnemy over the grid built by SetupDynGrid.
    pub fn enemies_near(&self, radius: f32, p: V3) -> R<Vec<usize>> {
        let g = &self.grid;
        let r = if 0.0 < radius { add(radius, g.max_enemy_radius)? } else { -radius };
        let (x0, z0) = (g.grid_x(sub(p[0], r)?)?, g.grid_z(sub(p[2], r)?)?);
        let (x1, z1) = (g.grid_x(add(r, p[0])?)?, g.grid_z(add(r, p[2])?)?);
        let mut out = Vec::new();
        for z in z0..=z1 {
            for x in x0..=x1 {
                let mut k = g.enemy_head[x + 64 * z];
                let mut guard = 0;
                while 0 <= k {
                    out.push(k as usize);
                    k = self.enemies[k as usize].i16_(e::GRID_NEXT);
                    guard += 1;
                    if guard > self.enemies.len() {
                        return Err(Unsupported("enemy grid cycle".into()));
                    }
                }
            }
        }
        Ok(out)
    }

    /// SetupDynGrid: items then active enemies (PlaceEnemyInGrid).
    pub fn setup_dyn_grid(&mut self) -> R<()> {
        self.grid.place_items(&mut self.items)?;
        for k in 0..self.enemies.len() {
            if self.enemies[k].i32_(e::STATE) == 0 {
                continue;
            }
            let p = self.enemies[k].v3(e::POSITION);
            let c = self.grid.grid_x(p[0])? + 64 * self.grid.grid_z(p[2])?;
            let prev = self.grid.enemy_head[c];
            self.grid.enemy_head[c] = k as i16;
            self.enemies[k].set_i16(e::GRID_NEXT, prev);
        }
        Ok(())
    }

    pub fn critters_empty(&self) -> bool {
        true
    }
}

pub fn new_enemy_records() -> Vec<Record> {
    (0..25).map(|_| Record::zeroed(ENEMY)).collect()
}

impl Game {
    /// damage_enemy(damage, enemy, player or -1, flags, hit position,
    /// knock vector, sound kind). 1 = killed, 0 = hurt, -1 = immune.
    #[allow(clippy::too_many_arguments)]
    pub fn damage_enemy(&mut self, damage: f32, i: usize, player: i32, flags: u32, hit: Option<V3>, knock: Option<V3>, sound: i32) -> R<i32> {
        let old = self.enemies[i].f32_(e::HEALTH);
        let st = self.enemies[i].i32_(e::STATE);
        let t = self.enemies[i].i32_(e::TYPE);
        if st == 7 || st == 8 || t == 0x1f {
            return Ok(-1);
        }
        let pl = usize::try_from(player).ok().filter(|&p| p < 4);
        if pl.is_some() {
            self.did_action = true;
        }
        let mut sound = sound;
        let kill_stat = |g: &mut Game| {
            if let Some(p) = pl {
                let o = 0xb2c + 0x1c * g.players[p].u32_(0xc) as usize;
                let v = g.players[p].i32_(o);
                g.players[p].set_i32(o, v + 1);
            }
        };
        if t == 0x1e {
            // Death: hits take one point, potions (0x200) dispel it.
            if flags & 0x200 != 0 {
                if let Some(p) = pl {
                    let lvl = self.players[p].i32_(0x2ac0);
                    if 0x4c <= lvl {
                        let k = add(mul((lvl - 0x4b) as f32, f32::from_bits(0x3d03_126f))?, f32::from_bits(0x3e4c_cccd))?;
                        let h = mul(k, self.enemies[i].f32_(e::HEALTH))?;
                        self.heal_player(h, p)?;
                    }
                }
                self.enemies[i].set_f32(e::HEALTH, 0.0);
            } else if st == 6 {
                if sound != 0 {
                    {
                        let pos = self.enemies[i].v3(e::POSITION);
                        self.sfx3(0x3c, pos, 0x7f, 0x78)?;
                    }
                }
                let c = self.enemies[i].i16_(0x2e8).wrapping_sub(1);
                self.enemies[i].set_i16(0x2e8, c);
                if 0 < c {
                    return Ok(0);
                }
                self.enemies[i].set_i32(e::STATE, 1);
                if sound != 0 {
                    {
                        let pos = self.enemies[i].v3(e::POSITION);
                        self.sfx(0x67, Some(pos), 0xe0, 0xd);
                    }
                }
                // SetEnemyObj(…, 1) swaps the statue model; matrix preserved.
                self.enemy_update_offsets(i)?;
                return Ok(0);
            } else {
                let h = sub(self.enemies[i].f32_(e::HEALTH), 1.0)?;
                self.enemies[i].set_f32(e::HEALTH, h);
                match pl {
                    Some(p) if self.players[p].u32_(0x134) & 0x8_0000 != 0 => {
                        if self.enemies[i].i16_(0x21a) == 2 {
                            return unsupported("AddExp from the death statue");
                        }
                        let v = add(self.players[p].f32_(0x1a10), 1.0)?;
                        self.players[p].set_f32(0x1a10, v);
                    }
                    Some(p) => self.help(0, p),
                    None => {}
                }
            }
            if 0.0 < self.enemies[i].f32_(e::HEALTH) {
                return Ok(0);
            }
            if sound != 0 {
                {
                    let pos = self.enemies[i].v3(e::POSITION);
                    self.sfx(0x65, Some(pos), 0x7f, 0x16);
                }
            }
            self.enemy_dies(i, player as i16)?;
            kill_stat(self);
            return Ok(1);
        }
        self.enemies[i].set_i16(0x2ea, 0);
        let mut d = damage;
        let mut f = flags;
        if f & 0x80_0000 != 0 && pl.is_some() {
            return unsupported("vampiric heal (do_heal_players)");
        }
        if let Some(p) = pl {
            let target = self.level_f32(0x9c);
            if 0.0 < target {
                let lvl = self.players[p].i32_(0x2ac0) as f32;
                let k = if lvl < target {
                    sub(1.0, mul(sub(target, lvl)?, f32::from_bits(0x3c23_d70a))?)?
                } else if target < lvl {
                    add(mul(sub(lvl, target)?, f32::from_bits(0x3dcc_cccd))?, 1.0)?
                } else {
                    1.0
                };
                d = mul(d, k)?;
            }
        }
        let rec = &self.enemies[i];
        super::damage::modify_damage(rec.f32_(0xd4), &mut d, &mut f, rec.u32_(0xdc), self.globals.boss_type)?;
        if self.globals.enemy_shrink < 1.0 {
            d = add(d, d)?;
        }
        if pl.is_some() && d < 1.0 {
            d = 1.0;
        }
        {
            let rec = &mut self.enemies[i];
            rec.set_f32(0x2b4, add(rec.f32_(0x2b4), d)?);
            if f & 0xf != 0 {
                rec.and_u32(0x2b8, 0xffff_fff0);
            }
            rec.or_u32(0x2b8, f);
            if let Some(k) = knock {
                for (n, v) in k.iter().enumerate() {
                    let o = 0x2bc + 4 * n;
                    rec.set_f32(o, add(*v, rec.f32_(o))?);
                }
            }
        }
        let mut at = self.enemies[i].v3(e::ATTENTION);
        if t == 0x1d {
            let n = self.globals.num_in_game_players;
            if 3 <= n {
                d = mul(d, 0.75)?;
            } else if 2 <= n {
                d = mul(d, 0.5)?;
            }
        }
        if d <= 0.0 {
            sound = 0;
        } else {
            let c = self.enemies[i].i16_(0x218).wrapping_add(1);
            self.enemies[i].set_i16(0x218, c);
        }
        let take = if self.globals.options == 3 { 10000.0 } else { d };
        let health = sub(self.enemies[i].f32_(e::HEALTH), take)?;
        self.enemies[i].set_f32(e::HEALTH, health);
        let fight = self.get_enemy_fight(health, t)?;
        self.enemies[i].set_f32(0xd0, fight);
        let logic = self.enemies[i].i16_(e::LOGIC);
        if let Some(g) = self.enemies[i].u32_(e::GENERATOR).checked_sub(1) {
            let r = &mut self.items.records[g as usize];
            if logic == 0xc && self.enemies[i].i16_(0x32a) < 2 {
                r.set_u8(0xfa, 7);
                r.set_u8(0xf4, 3);
            } else if logic == 0xf {
                r.set_u8(0xf7, 0);
            }
        }
        let health = self.enemies[i].f32_(e::HEALTH);
        if 0.0 < health {
            if sound != 0 {
                self.audio_enemy_hurt(i, sound)?;
            }
            if f & 0x100_0000 == 0 && t != self.globals.boss_type {
                if let Some(h) = hit.filter(|_| 4.0 <= self.enemies[i].f32_(e::HALF_HEIGHT)) {
                    at = h;
                }
                let _ = at;
                self.audio("AddEnemyHitCol", None, (f & 0xf) as i32);
            }
            return Ok(0);
        }
        if t == self.globals.boss_type {
            if 0.0 < old {
                kill_stat(self);
            }
            return Ok(1);
        }
        if sound != 0 {
            if logic == 0x12 {
                self.events.push(super::Event::StopSound { id: 0x37 });
            }
            self.audio_enemy_dies(i, sound)?;
        }
        self.enemy_dies(i, player as i16)?;
        kill_stat(self);
        if player < -1 {
            return Ok(1);
        }
        if f & 0x100_0000 == 0 && t != self.globals.boss_type {
            self.audio("AddEnemyHitCol", None, 0x100 | (f & 0xf) as i32);
        }
        Ok(1)
    }
}
