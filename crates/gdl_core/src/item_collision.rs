//! Item proximity collision: dynamic grid (InitDynGrid 0x180100, DynGridX/Z
//! 0x180188/0x1801c8, SetupDynGrid 0x180320, PlaceItemInGrid 0x180208,
//! StartItemGrid 0x180630, NextGridItem 0x180718), PlayerSkipItem 0x1462d8,
//! ItemCollSub 0x157988, NormalVector2D 0x1d5eb0 and PlayerCollideItems
//! 0x146348. Transcribed from the instructions; operation order follows them.
use crate::items::{ItemRecord, ItemWorld};
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sub_guarded_rz as sub};
use crate::vu0::fqdist;

type V3 = [f32; 3];
type R<T> = Result<T, crate::items::UnsupportedItem>;

/// DynGrid EE 0x245038: 64 × 64 cells of (enemy head, item head).
#[derive(Clone)]
pub struct DynGrid {
    pub min_x: f32,
    pub min_z: f32,
    pub size: f32,
    pub inv_size: f32,
    pub max_enemy_radius: f32,
    pub max_item_radius: f32,
    pub enemy_head: Vec<i16>,
    pub item_head: Vec<i16>,
}

impl DynGrid {
    /// InitDynGrid(max enemy radius, max item radius) over the world bounds
    /// (WorldInfo +0x18 min, +0x24 max: the WORLDS header bounds).
    pub fn new(min: V3, max: V3, max_enemy_radius: f32, max_item_radius: f32) -> R<Self> {
        let x = sub(max[0], min[0])?;
        let mut extent = sub(max[2], min[2])?;
        if extent < x {
            extent = x;
        }
        let size = mul(extent, 0.015625)?;
        Ok(Self {
            min_x: min[0],
            min_z: min[2],
            size,
            inv_size: div(1.0, size)?,
            max_enemy_radius,
            max_item_radius,
            enemy_head: vec![-1; 64 * 64],
            item_head: vec![-1; 64 * 64],
        })
    }
    fn cell(&self, v: f32, min: f32) -> R<usize> {
        let i = mul(sub(v, min)?, self.inv_size)? as i32;
        Ok(i.clamp(0, 63) as usize)
    }
    pub fn grid_x(&self, x: f32) -> R<usize> {
        self.cell(x, self.min_x)
    }
    pub fn grid_z(&self, z: f32) -> R<usize> {
        self.cell(z, self.min_z)
    }
    /// SetupDynGrid's item half: clear, then PlaceItemInGrid for every active
    /// item with a definition (links through record +0xe6, newest first).
    pub fn place_items(&mut self, items: &mut ItemWorld) -> R<()> {
        self.item_head.iter_mut().for_each(|h| *h = -1);
        self.enemy_head.iter_mut().for_each(|h| *h = -1);
        for i in 0..items.records.len() {
            let r = &items.records[i];
            if !r.active() || items.definitions[r.definition()].kind() == -1 {
                continue;
            }
            let p = r.position();
            let c = self.grid_x(p[0])? + 64 * self.grid_z(p[2])?;
            let previous = self.item_head[c];
            self.item_head[c] = i as i16;
            items.records[i].set_i16(0xe6, previous);
        }
        Ok(())
    }
    /// StartItemGrid + NextGridItem: item slots in the cells covering
    /// `position` ± radius (+ MaxItemRad when positive, |radius| otherwise).
    pub fn items_near(&self, items: &ItemWorld, radius: f32, position: V3) -> R<Vec<usize>> {
        let r = if 0.0 < radius { add(radius, self.max_item_radius)? } else { -radius };
        let (x0, z0) = (self.grid_x(sub(position[0], r)?)?, self.grid_z(sub(position[2], r)?)?);
        let (x1, z1) = (self.grid_x(add(r, position[0])?)?, self.grid_z(add(r, position[2])?)?);
        let mut out = Vec::new();
        for z in z0..=z1 {
            for x in x0..=x1 {
                let mut i = self.item_head[x + 64 * z];
                let mut guard = 0;
                while i >= 0 {
                    out.push(i as usize);
                    i = items.records[i as usize].i16_(0xe6);
                    guard += 1;
                    if guard > items.records.len() {
                        return Err(crate::items::UnsupportedItem("item grid cycle".into()));
                    }
                }
            }
        }
        Ok(out)
    }
}

/// NormalVector2D: normalise x/z in place (y = 0), returning the length.
pub fn normal_vector_2d(v: &mut V3) -> R<f32> {
    let (d, _) = fqdist(v[0], v[2])?;
    let k = if d <= 0.0 { 1.0 } else { div(1.0, d)? };
    v[0] = mul(k, v[0])?;
    v[1] = 0.0;
    v[2] = mul(k, v[2])?;
    Ok(d)
}

/// PlayerSkipItem: collected treasure (type 1 with +0xfc set) and inactive
/// hazards are ignored by players.
pub fn player_skip_item(items: &ItemWorld, r: &ItemRecord) -> bool {
    match items.definitions[r.definition()].kind() {
        1 => r.u32_(0xfc) != 0,
        8 => {
            let state = r.i8_(0xdc);
            if state == 2 || state == 4 {
                r.u16_(0xd8) & 1 == 0
            } else {
                true
            }
        }
        _ => false,
    }
}

/// Environment ItemCollSub consults.
pub struct CollideEnv<'a> {
    pub num_in_game_players: i32,
    /// AnyPlayerHasAllCrystals(f6) for portal triggers.
    pub any_player_has_all_crystals: &'a dyn Fn(i8) -> bool,
}

fn dot_xz(a: V3, m: V3) -> R<f32> {
    Ok(add(mul(a[0], m[0])?, mul(a[2], m[2])?)?)
}
fn row(r: &ItemRecord, o: usize) -> V3 {
    [r.f32_(o), r.f32_(o + 4), r.f32_(o + 8)]
}

/// ItemCollSub(item, from, pos, out, radius, height): xz distance beyond the
/// item radius, or -1.0. `out` receives the pushed-out position.
#[allow(clippy::too_many_arguments)]
pub fn item_coll_sub(
    items: &ItemWorld,
    env: &CollideEnv,
    slot: usize,
    from: V3,
    pos: V3,
    out: Option<&mut V3>,
    radius: f32,
    height: f32,
) -> R<f32> {
    let r = &items.records[slot];
    let miss = Ok(-1.0);
    if !r.active() || r.u16_(0xd8) & 0x8100 != 0 {
        return miss;
    }
    let d = &items.definitions[r.definition()];
    if d.kind() == -1 || r.u8_(0xe1) != 0 {
        return miss;
    }
    let mut shape = d.i16_(8);
    let mut reach = d.f32_(0xc);
    if shape == 0 || r.u16_(0xd8) & 0x4040 == 0 {
        return miss;
    }
    let mut ok = true;
    match d.kind() {
        2 if d.subtype() == 0x2b => ok = r.i8_(0xdc) != 2,
        3 if r.i8_(0xe3) < 0 && d.f32_(0x10) <= 3.0 => ok = false,
        4 => {
            let f = r.f32_(0xfc);
            reach = if 0.0 <= f { f } else { 3.0 };
            shape = 1;
        }
        5 => {
            if r.u16_(0xd8) & 0x400 != 0 {
                ok = false;
            } else {
                let f = r.f32_(0xfc);
                if 0.0 < f {
                    reach = f;
                    shape = 1;
                } else if d.subtype() == 0x1b {
                    reach = add(reach, reach)?;
                    shape = 1;
                }
                let flags = r.u16_(0xf4);
                if flags & 0x200 != 0 {
                    ok = false;
                } else if flags & 0x40 != 0 && r.i8_(0xf6) < 100 && (env.any_player_has_all_crystals)(r.i8_(0xf6)) {
                    reach = add(reach, reach)?;
                }
            }
        }
        7 => {
            let state = r.i8_(0xdc);
            if state >= 2 || (state == 1 && r.i16_(0xda) >= 0x1f) {
                ok = false;
            }
        }
        9 if d.subtype() != 0x32 => {
            reach = add(reach, sub(env.num_in_game_players as f32, 1.0)?)?;
        }
        10 if (0x2b..0x2e).contains(&(d.subtype() as u32)) => ok = r.i8_(0xdc) < 1,
        13 => ok = false,
        _ => {}
    }
    if !ok {
        return miss;
    }
    let outer = add(radius, reach)?;
    let cx = sub(r.f32_(0x60), pos[0])?;
    let cz = sub(r.f32_(0x68), pos[2])?;
    if mul(outer, outer)? < add(mul(cx, cx)?, mul(cz, cz)?)? {
        return miss;
    }
    let ip = row(r, 0x60); // GetItemPos 0x153988
    let e = [sub(pos[0], ip[0])?, sub(pos[1], ip[1])?, sub(pos[2], ip[2])?];
    if shape != 2 && add(height, d.f32_(0x10))? < e[1].abs() {
        return miss;
    }
    let mut dist = fqdist(e[0], e[2])?.0;
    if outer < dist {
        return miss;
    }
    let (m0, m2) = (row(r, 0x10), row(r, 0x30));
    let (half0, half2) = (d.f32_(0x14), d.f32_(0x18));
    let hit = match shape {
        2 => fqdist(dist, e[1])?.0 <= outer,
        1 => true,
        3 => dot_xz(e, m0)?.abs() <= add(radius, half0)? && dot_xz(e, m2)?.abs() <= add(radius, half2)?,
        4 => return Err(crate::items::UnsupportedItem("ItemObjectCol (shape 4) not yet ported".into())),
        _ => false,
    };
    if !hit {
        return miss;
    }
    dist = sub(dist, reach)?;
    if dist < 0.0 {
        dist = 0.0;
    }
    let kind = d.kind();
    if kind == 5 || kind == 8 || kind == 9 || kind == 11 {
        if let Some(o) = out {
            *o = pos;
        }
        return Ok(dist);
    }
    // Approach test from the previous position.
    let ef = [sub(from[0], ip[0])?, 0.0, sub(from[2], ip[2])?];
    let mut approach = false;
    match shape {
        3 => {
            let f3 = dot_xz(ef, m0)?;
            let f4 = dot_xz(ef, m2)?;
            if f3.abs() <= add(radius, half0)? && f4.abs() <= add(radius, half2)? {
                let ep = [sub(pos[0], ip[0])?, 0.0, sub(pos[2], ip[2])?];
                let p3 = dot_xz(ep, m0)?;
                let p4 = dot_xz(ep, m2)?;
                approach = !((f3 < 0.0 && f3 < p3) || (0.0 < f3 && p3 < f3) || (f4 < 0.0 && f4 < p4))
                    && (f4 <= 0.0 || f4 <= p4);
            }
        }
        1 => approach = true,
        4 => {}
        _ => approach = fqdist(ef[0], ef[2])?.0 <= outer,
    }
    let mut a = [sub(pos[0], from[0])?, 0.0, sub(pos[2], from[2])?];
    let mut b = [sub(ip[0], from[0])?, 0.0, sub(ip[2], from[2])?];
    if approach {
        normal_vector_2d(&mut a)?;
        normal_vector_2d(&mut b)?;
        if add(mul(a[0], b[0])?, mul(a[2], b[2])?)? < 0.0 {
            return miss;
        }
    }
    let Some(o) = out else {
        return Ok(dist);
    };
    if shape == 3 {
        let ep = [sub(pos[0], ip[0])?, 0.0, sub(pos[2], ip[2])?];
        let f3 = dot_xz(ep, m0)?;
        let f4 = dot_xz(ep, m2)?;
        let p0 = sub(add(radius, half0)?, f3.abs())?;
        let p2 = sub(add(radius, half2)?, f4.abs())?;
        let push = |k: f32, m: V3| -> R<V3> {
            Ok([add(mul(k, m[0])?, pos[0])?, add(mul(k, m[1])?, pos[1])?, add(mul(k, m[2])?, pos[2])?])
        };
        *o = if 0.0 < p0 && p0 < p2 {
            push(if 0.0 < f3 { p0 } else { -p0 }, m0)?
        } else if 0.0 < p2 && p2 < p0 {
            push(if 0.0 < f4 { p2 } else { -p2 }, m2)?
        } else {
            from
        };
        return Ok(dist);
    }
    // Cylinder/sphere: slide along the tangent of the item seen from `from`.
    if !approach {
        a = [sub(pos[0], from[0])?, 0.0, sub(pos[2], from[2])?];
        b = [sub(ip[0], from[0])?, 0.0, sub(ip[2], from[2])?];
    }
    let cross = sub(mul(a[2], b[0])?, mul(a[0], b[2])?)?;
    let mut t = if 0.0 < cross { [-b[2], b[1], b[0]] } else { [b[2], b[1], -b[0]] };
    normal_vector_2d(&mut t)?;
    let k = add(mul(a[0], t[0])?, mul(a[2], t[2])?)?;
    *o = [add(mul(k, t[0])?, from[0])?, add(mul(k, t[1])?, from[1])?, add(mul(k, t[2])?, from[2])?];
    Ok(dist)
}
