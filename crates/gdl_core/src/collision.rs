//! Static world collision transcribed from SLUS-20047 EE code: WorldCollide
//! 0x11e160, NextGrid 0x11de48, WorldObjCollide 0x11da08 (static objects),
//! CTriListCollide 0x11d600, BTriLineCol 0x11af80 and its leaf routines.
//!
//! Scalar EE arithmetic uses the guarded toward-zero add/sub, toward-zero
//! multiply, nearest DIV.S and nearest SQRT.S models. VU0 macro arithmetic uses
//! ordinary toward-zero add/sub/mul/div/sqrt. Dynamic objects (WorldDynCollide),
//! matrix/offset objects (flags 0x1000000/0x1000), TriLineCol (CheckFlags 0x20)
//! and exceptional numeric domains are rejected, never approximated.
use crate::numeric::{
    add_guarded_rz as add, add_rz, div_rn, div_rz, mul_rz as mul, sqrt_rn, sqrt_rz,
    sub_guarded_rz as sub, sub_rz, UnsupportedNumericDomain,
};
use crate::vu0::fqdist;
use gdl_formats::world::{CollisionGrid, CollisionRecord, World};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedCollision(pub &'static str);
impl std::fmt::Display for UnsupportedCollision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported collision path: {}", self.0)
    }
}
impl std::error::Error for UnsupportedCollision {}
impl From<UnsupportedNumericDomain> for UnsupportedCollision {
    fn from(_: UnsupportedNumericDomain) -> Self {
        Self("numeric domain")
    }
}
type R<T> = Result<T, UnsupportedCollision>;
type V3 = [f32; 3];

fn w(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// Runtime fields of one world object that collision reads.
#[derive(Debug, Clone)]
pub struct CollisionObject {
    pub flags: u32,
    pub parent: Option<usize>,
    /// Position after the loader's parent-chain sum (runtime +0x1c).
    pub position: V3,
    pub radius: f32,
    pub exclusion: i8,
    pub first_triangle: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct Triangle {
    pub min_y64: i16,
    pub max_y64: i16,
    pub scale: f32,
    pub normal: V3,
    pub origin: V3,
    pub uv: [f32; 4],
}

pub struct CollisionWorld {
    pub objects: Vec<CollisionObject>,
    pub triangles: Vec<Triangle>,
    grid: CollisionGrid,
    min_x: f32,
    min_z: f32,
    inv_cell: f32,
    check: Vec<i8>,
    check_counter: u32,
}

/// Inputs/outputs of one WorldCollide call; `hit` corresponds to the
/// col_obj/col_dist/col_pos/col_tri globals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub object: usize,
    pub triangle: usize,
    pub distance: f32,
    pub position: V3,
}

#[derive(Debug, Clone, Copy)]
pub struct Query {
    pub radius: f32,
    pub first: V3,
    pub last: V3,
    pub check_flags: u32,
    pub min_normal_y: f32,
    pub max_normal_y: f32,
    pub object_mask: u32,
    pub exclusion_mask: i32,
}

/// Result of one WorldCollide call: the nearest solid hit (col_* globals) and
/// the nearest water-object hit (water_col +0x30..+0x44).
#[derive(Debug, Clone, Copy)]
pub struct Collision {
    pub solid: Option<Hit>,
    pub water: Option<Hit>,
}

/// Values WorldCollide leaves in its globals, for comparison with captures.
#[derive(Debug, Clone, Copy)]
pub struct QueryState {
    pub direction: V3,
    pub length: f32,
    pub min_y: f32,
    pub max_y: f32,
    pub check_num: u8,
}

impl CollisionWorld {
    /// The loader stores each static object's parent-chain position sum,
    /// formed top-down with scalar EE adds (parent sum + own position); all
    /// 1,319 static runtime positions in the fixture match bit for bit.
    pub fn from_world(world: &World) -> R<Self> {
        let n = world.nodes.len();
        let mut parent = vec![None; n];
        for (i, node) in world.nodes.iter().enumerate() {
            let mut child = node.child;
            let mut guard = 0;
            while child >= 0 {
                parent[child as usize] = Some(i);
                child = world.nodes[child as usize].next;
                guard += 1;
                if guard > n {
                    return Err(UnsupportedCollision("world sibling cycle"));
                }
            }
        }
        let mut position = vec![None; n];
        fn sum(i: usize, w: &World, p: &[Option<usize>], out: &mut [Option<V3>], depth: usize) -> R<V3> {
            if let Some(v) = out[i] {
                return Ok(v);
            }
            if depth > 256 {
                return Err(UnsupportedCollision("world parent depth"));
            }
            let own = w.nodes[i].position;
            // Matrix objects (flag 0x1000000) keep their local file position;
            // verified for all 41 such objects in the Poison Fields fixture.
            if w.nodes[i].flags & 0x100_0000 != 0 {
                out[i] = Some(own);
                return Ok(own);
            }
            let v = match p[i] {
                None => own,
                Some(q) if w.nodes[q].flags & 0x100_0000 != 0 => {
                    return Err(UnsupportedCollision("static object under a matrix object"));
                }
                Some(q) => {
                    let base = sum(q, w, p, out, depth + 1)?;
                    [add(base[0], own[0])?, add(base[1], own[1])?, add(base[2], own[2])?]
                }
            };
            out[i] = Some(v);
            Ok(v)
        }
        let mut objects = Vec::with_capacity(n);
        for (i, node) in world.nodes.iter().enumerate() {
            let raw = &node.raw;
            objects.push(CollisionObject {
                flags: node.flags,
                parent: parent[i],
                position: sum(i, world, &parent, &mut position, 0)?,
                radius: node.radius,
                exclusion: raw[0x35] as i8,
                first_triangle: i32::from_le_bytes(raw[0x38..0x3c].try_into().unwrap()),
            });
        }
        let triangles = world.collisions.iter().map(Triangle::from_record).collect();
        let header = &world.raw_header;
        let cell = f32::from_le_bytes(header[60..64].try_into().unwrap());
        Ok(Self {
            objects,
            triangles,
            min_x: world.min[0],
            min_z: world.min[2],
            // Load-time reciprocal; the fixture's runtime value is 1/3 rounded
            // to nearest (0x3eaaaaab), matching the EE DIV.S model.
            inv_cell: div_rn(1.0, cell)?,
            check: vec![0; world.collisions.len()],
            check_counter: 0,
            grid: world.grid.clone(),
        })
    }

    /// PlayerCollideWalls increments the hit triangle's CheckNum stamp so the
    /// following query (whose counter is one higher) skips that triangle.
    pub fn bump_check(&mut self, triangle: usize) {
        if let Some(stamp) = self.check.get_mut(triangle) {
            *stamp = stamp.wrapping_add(1);
        }
    }

    /// FloorPos 0x11ed38: FloorCollide(radius, +4, -10, CheckFlags 0x17); the
    /// hit's y, or `default_y` without a hit.
    pub fn floor_pos(&mut self, default_y: f32, radius: f32, position: V3, exclusion_mask: i32) -> R<(f32, Option<Hit>)> {
        let (c, _) = self.floor_collide(radius, 4.0, -10.0, position, true, exclusion_mask)?;
        Ok((c.solid.map_or(default_y, |h| h.position[1]), c.solid))
    }

    /// FloorCollide 0x11ec58: vertical segment at `position` from y+top to
    /// y+bottom, CheckFlags 7 or 0x17, normal-y 0.5..2.0, object mask 0x23c.
    pub fn floor_collide(
        &mut self,
        radius: f32,
        top: f32,
        bottom: f32,
        position: V3,
        nearest_by_parameter: bool,
        exclusion_mask: i32,
    ) -> R<(Collision, QueryState)> {
        let first = [position[0], add(top, position[1])?, position[2]];
        let last = [position[0], add(bottom, position[1])?, position[2]];
        self.world_collide(Query {
            radius,
            first,
            last,
            check_flags: if nearest_by_parameter { 0x17 } else { 7 },
            min_normal_y: 0.5,
            max_normal_y: 2.0,
            object_mask: 0x23c,
            exclusion_mask,
        })
    }

    pub fn world_collide(&mut self, q: Query) -> R<(Collision, QueryState)> {
        if q.check_flags & 0x20 != 0 {
            return Err(UnsupportedCollision("TriLineCol query (CheckFlags 0x20)"));
        }
        let r = q.radius;
        let mut dir = [sub(q.last[0], q.first[0])?, sub(q.last[1], q.first[1])?, sub(q.last[2], q.first[2])?];
        let length = normal_vector(&mut dir)?;
        let (mut start_x, mut start_z, mut end_x, mut end_z) = if q.first[0] <= q.last[0] {
            (q.first[0], q.first[2], q.last[0], q.last[2])
        } else {
            (q.last[0], q.last[2], q.first[0], q.first[2])
        };
        let (min_y, max_y) = if q.first[1] < q.last[1] {
            (sub(q.first[1], r)?, add(r, q.last[1])?)
        } else {
            (sub(q.last[1], r)?, add(r, q.first[1])?)
        };
        let dx = sub(end_x, start_x)?;
        let dz = sub(end_z, start_z)?;
        let slope = if dx == 0.0 { 0.0 } else { div_rn(dz, dx)? };
        start_x = sub(start_x, r)?;
        end_x = add(r, end_x)?;
        if 0.0 <= dz {
            start_z = sub(start_z, r)?;
            end_z = add(r, end_z)?;
        } else {
            start_z = add(r, start_z)?;
            end_z = sub(end_z, r)?;
        }
        let (min_x, min_z) = (self.min_x, self.min_z);
        let mut outside = end_x < min_x;
        if start_x < min_x {
            if dx != 0.0 {
                let mut t = div_rn(sub(min_x, start_x)?, dx)?;
                if t < 0.0 {
                    t = -t;
                }
                start_z = add(mul(t, dz)?, start_z)?;
            }
            start_x = min_x;
        }
        if end_z < min_z {
            if dz == 0.0 {
                outside = true;
            } else {
                let mut t = div_rn(sub(min_z, end_z)?, dz)?;
                if t < 0.0 {
                    t = -t;
                }
                end_x = sub(end_x, mul(t, dx)?)?;
                end_z = add(min_z, w(0x3c23d70a))?;
            }
        }
        if start_z < min_z {
            if dz != 0.0 {
                let mut t = div_rn(sub(min_z, start_z)?, dz)?;
                if t < 0.0 {
                    t = -t;
                }
                start_x = add(mul(t, dx)?, start_x)?;
            }
            start_z = min_z;
        }
        self.check_counter += 1;
        if self.check_counter > 0xff {
            self.check_counter = 1;
        }
        let state = QueryState {
            direction: dir,
            length,
            min_y,
            max_y,
            check_num: self.check_counter as u8,
        };
        let ctx = Ctx {
            q,
            dir,
            length,
            min_y,
            max_y,
            start_x,
            start_z,
            end_z,
            end_x,
            check_num: self.check_counter as u8 as i8,
        };
        let mut found = Collision { solid: None, water: None };
        if !outside {
            let gx0 = trunc(mul(sub(start_x, min_x)?, self.inv_cell)?);
            let gz0 = trunc(mul(sub(start_z, min_z)?, self.inv_cell)?);
            let mut gx = if gx0 < 0 { 0 } else { gx0.min(self.grid.dim_x as i32 - 1) };
            let mut gz = if gz0 < 0 { 0 } else { gz0.min(self.grid.dim_z as i32 - 1) };
            let mut steps = 0;
            loop {
                steps += 1;
                if steps > 1 << 16 {
                    return Err(UnsupportedCollision("grid walk policy exceeded"));
                }
                let row = self.grid.rows[gz as usize];
                if i32::from(row.min_x) <= gx && gx <= i32::from(row.max_x) {
                    let cell = self.grid.cells[(gx as u32 - u32::from(row.min_x) + row.cell_offset) as usize];
                    let mut offset = (cell & 0x3f_ffff) as usize;
                    for _ in 0..cell >> 22 {
                        let read = |o: usize| i16::from_le_bytes(self.grid.contents[o..o + 2].try_into().unwrap());
                        let count = read(offset + 2);
                        if count != 0 {
                            let index = read(offset) as usize;
                            let list: Vec<i16> = (0..count as usize).map(|k| read(offset + 4 + 2 * k)).collect();
                            self.object_collide(&ctx, index, &list, &mut found)?;
                            offset += 6 + (count as usize - 1) * 2;
                        }
                    }
                }
                if !next_grid(&ctx, self, dx, dz, slope, r, &mut gx, &mut gz)? {
                    break;
                }
            }
        }
        // WorldDynCollide would run here; the native subset has no dynamic grid.
        Ok((found, state))
    }

    fn object_collide(
        &mut self,
        c: &Ctx,
        index: usize,
        list: &[i16],
        found: &mut Collision,
    ) -> R<()> {
        let object = self.objects.get(index).ok_or(UnsupportedCollision("grid object index"))?.clone();
        let mut exclusion = object.exclusion;
        if let Some(p) = object.parent {
            exclusion |= self.objects[p].exclusion;
        }
        if c.q.object_mask & object.flags == 0
            || i32::from(exclusion) & c.q.exclusion_mask != 0
            || object.first_triangle < 0
        {
            return Ok(());
        }
        if object.flags & (0x100_0000 | 0x1000) != 0 {
            return Err(UnsupportedCollision("matrix or offset world object"));
        }
        let p = object.position;
        let first = c.q.first;
        let d = [sub(p[0], first[0])?, sub(p[1], first[1])?, sub(p[2], first[2])?];
        let t = add(add(mul(d[0], c.dir[0])?, mul(d[1], c.dir[1])?)?, mul(d[2], c.dir[2])?)?;
        let rr = add(c.q.radius, object.radius)?;
        if add(rr, c.length)? < t || t < -rr {
            return Ok(());
        }
        let near = [add(mul(t, c.dir[0])?, first[0])?, add(mul(t, c.dir[1])?, first[1])?, add(mul(t, c.dir[2])?, first[2])?];
        let e = [sub(p[0], near[0])?, sub(p[1], near[1])?, sub(p[2], near[2])?];
        let dist2 = add(add(mul(e[0], e[0])?, mul(e[1], e[1])?)?, mul(e[2], e[2])?)?;
        if dist2 >= mul(rr, rr)? {
            return Ok(());
        }
        let ymin = trunc(mul(c.min_y, 64.0)?);
        let ymax = trunc(mul(c.max_y, 64.0)?);
        let (distance, hit) =
            self.tri_list_collide(c, object.first_triangle, list, ymin, ymax, object.flags & 0x40 != 0)?;
        if 0.0 <= distance {
            let (triangle, position) = hit.ok_or(UnsupportedCollision("hit without triangle"))?;
            let slot = if object.flags & 0x200 != 0 { &mut found.water } else { &mut found.solid };
            // Both start at 1e21 (col_dist and water_col +0x40 initialisation).
            if distance < slot.map_or(w(0x6258d727), |h| h.distance) {
                *slot = Some(Hit { object: index, triangle, distance, position });
            }
        }
        Ok(())
    }

    #[allow(clippy::type_complexity)]
    fn tri_list_collide(
        &mut self,
        c: &Ctx,
        first: i32,
        list: &[i16],
        ymin: i32,
        ymax: i32,
        ignore_filters: bool,
    ) -> R<(f32, Option<(usize, V3)>)> {
        let mut result = -1.0f32;
        let mut best = w(0x6258d727);
        let mut found = None;
        for &offset in list {
            let index = (first + i32::from(offset)) as usize;
            let stamp = self.check.get_mut(index).ok_or(UnsupportedCollision("triangle index"))?;
            if *stamp == c.check_num {
                continue;
            }
            *stamp = c.check_num;
            let tri = self.triangles[index];
            if !ignore_filters
                && (tri.normal[1] < c.q.min_normal_y
                    || c.q.max_normal_y < tri.normal[1]
                    || i32::from(tri.max_y64) < ymin
                    || ymax < i32::from(tri.min_y64))
            {
                continue;
            }
            let Some((d, hit)) = b_tri_line_col(c.q.radius, &tri, c.q.first, c.q.last)? else {
                continue;
            };
            let mut metric = if c.q.check_flags & 0x10 != 0 && d != 0.0 {
                mul(d, w(0x461c4000))?
            } else {
                let e = [sub(hit[0], c.q.first[0])?, sub(hit[1], c.q.first[1])?, sub(hit[2], c.q.first[2])?];
                add(add(mul(e[0], e[0])?, mul(e[1], e[1])?)?, mul(e[2], e[2])?)?
            };
            let mut facing =
                add(add(mul(c.dir[0], tri.normal[0])?, mul(c.dir[1], tri.normal[1])?)?, mul(c.dir[2], tri.normal[2])?)?;
            if facing < 0.0 {
                facing = -facing;
            }
            if facing < 0.25 {
                metric = mul(metric, w(0x3f733333))?;
            }
            if metric < best {
                best = metric;
                result = metric;
                found = Some((index, hit));
            }
        }
        Ok((result, found))
    }
}

/// Values derived once per WorldCollide call.
struct Ctx {
    q: Query,
    dir: V3,
    length: f32,
    min_y: f32,
    max_y: f32,
    start_x: f32,
    start_z: f32,
    end_x: f32,
    end_z: f32,
    check_num: i8,
}

/// CVT.W.S truncation for the bounded grid coordinates used here.
fn trunc(v: f32) -> i32 {
    v as i32
}

#[allow(clippy::too_many_arguments)]
fn next_grid(c: &Ctx, world: &CollisionWorld, dx: f32, dz: f32, slope: f32, r: f32, gx: &mut i32, gz: &mut i32) -> R<bool> {
    let cell = world.grid.cell_size;
    let (start_x, start_z, end_x, end_z) = (c.start_x, c.start_z, c.end_x, c.end_z);
    let positive = if dx == 0.0 { 0.0 <= dz } else { 0.0 <= slope };
    let offset = if positive { r } else { -r };
    let z = *gz + if positive { 1 } else { -1 };
    let edge = add(mul((*gx + 1) as f32, cell)?, world.min_x)?;
    let along = if dx == 0.0 { dz } else { mul(sub(edge, start_x)?, slope)? };
    if 0 <= z && z < world.grid.dim_z as i32 {
        let mut reach = add(add(along, start_z)?, add(offset, offset)?)?;
        let row = if positive {
            if end_z < reach {
                reach = end_z;
            }
            let v = trunc(mul(sub(reach, world.min_z)?, world.inv_cell)?);
            v < z
        } else {
            if reach < end_z {
                reach = end_z;
            }
            let v = trunc(mul(sub(reach, world.min_z)?, world.inv_cell)?);
            z < v
        };
        if !row {
            *gz = z;
            return Ok(true);
        }
    }
    if end_x < edge {
        return Ok(false);
    }
    let x = *gx + 1;
    if world.grid.dim_x as i32 <= x {
        return Ok(false);
    }
    let mut zz = start_z;
    if dx != 0.0 && sub(edge, start_x)? >= add(r, r)? {
        let past = sub(add(edge, cell)?, add(add(r, r)?, start_x)?)?;
        if 0.0 < past {
            zz = add(mul(past, slope)?, start_z)?;
        }
    }
    if positive {
        if end_z < zz {
            zz = end_z;
        }
    } else if zz < end_z {
        zz = end_z;
    }
    let v = trunc(mul(sub(zz, world.min_z)?, world.inv_cell)?);
    if world.grid.dim_z as i32 <= v || v < 0 {
        return Ok(false);
    }
    *gx = x;
    *gz = v;
    Ok(true)
}

impl Triangle {
    fn from_record(r: &CollisionRecord) -> Self {
        let s = |v: i16| f32::from(v) * 0.015625;
        Self {
            min_y64: r.min_y64,
            max_y64: r.max_y64,
            scale: f32::from_bits(r.scale_bits),
            normal: r.normal,
            origin: r.origin,
            uv: r.vertices64.map(s),
        }
    }
}

// ---------- vector helpers ----------

fn vsub(a: V3, b: V3) -> R<V3> {
    Ok([sub_rz(a[0], b[0])?, sub_rz(a[1], b[1])?, sub_rz(a[2], b[2])?])
}
fn vadd(a: V3, b: V3) -> R<V3> {
    Ok([add_rz(a[0], b[0])?, add_rz(a[1], b[1])?, add_rz(a[2], b[2])?])
}
fn vscale(a: V3, s: f32) -> R<V3> {
    Ok([mul(a[0], s)?, mul(a[1], s)?, mul(a[2], s)?])
}
/// VMUL.xyz then VADDy.x and VADDz.x: (x + y) + z.
fn vdot(a: V3, b: V3) -> R<f32> {
    Ok(add_rz(add_rz(mul(a[0], b[0])?, mul(a[1], b[1])?)?, mul(a[2], b[2])?)?)
}
/// VMUL.xz then VADDz.x.
fn vdot_xz(a: V3, b: V3) -> R<f32> {
    Ok(add_rz(mul(a[0], b[0])?, mul(a[2], b[2])?)?)
}
/// VOPMULA/VOPMSUB: ACC = a.yzx*b.zxy, result = ACC - b.yzx*a.zxy.
fn vcross(a: V3, b: V3) -> R<V3> {
    Ok([
        sub_rz(mul(a[1], b[2])?, mul(b[1], a[2])?)?,
        sub_rz(mul(a[2], b[0])?, mul(b[2], a[0])?)?,
        sub_rz(mul(a[0], b[1])?, mul(b[0], a[1])?)?,
    ])
}
/// Det3Vectors 0x11a428: c · (a × b).
fn det3(a: V3, b: V3, c: V3) -> R<f32> {
    vdot(c, vcross(a, b)?)
}
/// VU macro normalisation in LineLineDist3D2D.
fn vnormalize(v: V3) -> R<(V3, f32)> {
    let len = add_rz(0.0, sqrt_rz(vdot(v, v)?)?)?;
    let inv = div_rz(1.0, len)?;
    Ok((vscale(v, inv)?, len))
}

/// NormalVector 0x1d5d50 (in place); returns the length.
/// NormalVector 0x1d5d50 for other modules.
pub fn normal_vector_pub(v: &mut V3) -> R<f32> {
    normal_vector(v)
}

pub(crate) fn normal_vector(v: &mut V3) -> R<f32> {
    let xz = fqdist(v[0], v[2])?.0;
    let len = fqdist(xz, v[1])?.0;
    let scale = if len <= 0.0 { 1.0 } else { div_rn(1.0, len)? };
    for c in v.iter_mut() {
        *c = mul(scale, *c)?;
    }
    Ok(len)
}

/// BodyVectorNorm 0x11ad00: world vector into the triangle's local frame.
fn body_vector_norm(k: f32, v: V3, n: V3) -> R<V3> {
    let ny = n[1];
    if w(0x3f7fffef) < ny {
        return Ok(v);
    }
    if ny < w(0xbf7fffef) {
        return Ok([v[0], -v[1], -v[2]]);
    }
    let f8 = mul(n[0], k)?;
    let f9 = mul(n[2], k)?;
    let nx = -v[0];
    let x = add(mul(nx, f9)?, mul(v[2], f8)?)?;
    let y = add(add(mul(v[0], n[0])?, mul(v[1], ny)?)?, mul(v[2], n[2])?)?;
    let a = mul(mul(nx, f8)?, ny)?;
    let b = mul(mul(v[1], sub(1.0, mul(ny, ny)?)?)?, k)?;
    let c = mul(mul(-v[2], ny)?, f9)?;
    Ok([x, y, add(add(a, b)?, c)?])
}

/// WorldVectorNorm 0x11ac28: local vector back to world orientation.
fn world_vector_norm(v: V3, k: f32, n: V3) -> R<V3> {
    let ny = n[1];
    if w(0x3f7fffef) < ny {
        return Ok(v);
    }
    if ny < w(0xbf7fffef) {
        return Ok([v[0], -v[1], -v[2]]);
    }
    let f2 = mul(n[0], k)?;
    let f6 = mul(n[2], k)?;
    let x = add(add(mul(-v[0], f6)?, mul(v[1], n[0])?)?, mul(mul(-v[2], f2)?, ny)?)?;
    let y = add(mul(v[1], ny)?, mul(mul(v[2], sub(1.0, mul(ny, ny)?)?)?, k)?)?;
    let z = add(add(mul(v[0], f2)?, mul(v[1], n[2])?)?, mul(mul(-v[2], ny)?, f6)?)?;
    Ok([x, y, z])
}

/// CreateMat3Norm 0x11d538: rows 0..2 (w lanes zero) of the contact basis.
pub fn create_mat3_norm(k: f32, n: V3) -> R<[[f32; 4]; 3]> {
    let ny = n[1];
    if w(0x3f7fbe77) < ny {
        return Ok([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]);
    }
    if ny < w(0xbf7fbe77) {
        return Ok([[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0]]);
    }
    let f1 = mul(n[0], k)?;
    let f4 = mul(n[2], k)?;
    Ok([
        [-f4, 0.0, f1, 0.0],
        [n[0], ny, n[2], 0.0],
        [mul(-f1, ny)?, mul(sub(1.0, mul(ny, ny)?)?, k)?, mul(-ny, f4)?, 0.0],
    ])
}

/// PointLineDist 0x11a458: squared distance from `x` to segment a + d*[0, len].
fn point_line_dist(x: V3, a: V3, d: V3, len: f32) -> R<(f32, V3)> {
    let t = vdot(vsub(x, a)?, d)?;
    let closest = if t < 0.0 {
        a
    } else if len <= t {
        vadd(vscale(d, len)?, a)?
    } else {
        vadd(vscale(d, t)?, a)?
    };
    let e = vsub(closest, x)?;
    Ok((vdot(e, e)?, closest))
}

/// PointLineDist2D 0x11a530 (x/z only; `d` unnormalised).
fn point_line_dist_2d(x: V3, a: V3, d: V3) -> R<(f32, V3)> {
    let len = sqrt_rn(add(mul(d[0], d[0])?, mul(d[2], d[2])?)?)?;
    if len == 0.0 {
        let e = vsub(a, x)?;
        return Ok((vdot_xz(e, e)?, a));
    }
    let inv = div_rn(1.0, len)?;
    let e = vsub(x, a)?;
    let dn = vscale(d, inv)?;
    let t = vdot_xz(e, dn)?;
    let out = if t < 0.0 {
        a
    } else if len <= t {
        vadd(d, a)?
    } else {
        vadd(vscale(dn, t)?, a)?
    };
    let e = vsub(x, out)?;
    Ok((vdot_xz(e, e)?, out))
}

/// LineLineDist 0x11a6d8: segments p + u*[0,lu] and q + v*[0,lv].
fn line_line_dist(p: V3, u: V3, lu: f32, q: V3, v: V3, lv: f32) -> R<(f32, V3)> {
    let wv = vsub(q, p)?;
    let n = vcross(u, v)?;
    let nn = vdot(n, n)?;
    if nn == 0.0 {
        let t = vdot(wv, u)?;
        if t < 0.0 {
            return Ok((point_line_dist(p, q, v, lv)?.0, p));
        }
        if lu <= t {
            let out = vadd(vscale(u, lu)?, p)?;
            return Ok((point_line_dist(out, q, v, lv)?.0, out));
        }
        let out = vadd(vscale(u, t)?, p)?;
        let e = vsub(out, q)?;
        return Ok((vdot(e, e)?, out));
    }
    let inv = div_rn(1.0, nn)?;
    let s = mul(det3(wv, v, n)?, inv)?;
    let end_p = if s <= 0.0 {
        Some(p)
    } else if lu <= s {
        Some(vadd(vscale(u, lu)?, p)?)
    } else {
        None
    };
    let t = mul(det3(wv, u, n)?, inv)?;
    let end_q = if t <= 0.0 {
        Some(q)
    } else if lv <= t {
        Some(vadd(vscale(v, lv)?, q)?)
    } else {
        None
    };
    if end_p.is_none() && end_q.is_none() {
        let out = vadd(vscale(u, s)?, p)?;
        let other = vadd(vscale(v, t)?, q)?;
        let e = vsub(other, out)?;
        return Ok((vdot(e, e)?, out));
    }
    let a = match end_p {
        Some(ep) => point_line_dist(ep, q, v, lv)?.0,
        None => w(0x6258d727),
    };
    let (b, out_b) = match end_q {
        Some(eq) => {
            let (d, c) = point_line_dist(eq, p, u, lu)?;
            (d, Some(c))
        }
        None => (w(0x6258d727), None),
    };
    if a < b {
        return Ok((a, end_p.unwrap()));
    }
    Ok((b, out_b.ok_or(UnsupportedCollision("LineLineDist output unset"))?))
}

/// LineLineDist3D2D 0x11aa70: local segment a..b against triangle edge p..q.
fn line_line_dist_3d2d(a: V3, b: V3, p: V3, q: V3, crossing: bool) -> R<(f32, V3)> {
    let d1 = vsub(b, a)?;
    let mut d2 = vsub(q, p)?;
    let xz = sqrt_rn(add(mul(d1[0], d1[0])?, mul(d1[2], d1[2])?)?)?;
    if xz < w(0x3a83126f) {
        let (r, mut out) = point_line_dist_2d(a, p, d2)?;
        let y = if crossing {
            0.0
        } else if a[1].abs() <= b[1].abs() {
            a[1]
        } else {
            b[1]
        };
        out[1] = 0.0;
        return Ok((add(r, mul(y, y)?)?, out));
    }
    let p2 = [p[0], 0.0, p[2]];
    d2[1] = 0.0;
    let (u, lu) = vnormalize(d2)?;
    let (v, lv) = vnormalize(d1)?;
    line_line_dist(p2, u, lu, a, v, lv)
}

/// BTriLineCol 0x11af80 for the segment first..last with radius `r`.
/// Returns the distance metric and the world hit position, or None (-1.0).
fn b_tri_line_col(r: f32, tri: &Triangle, first: V3, last: V3) -> R<Option<(f32, V3)>> {
    let o = tri.origin;
    let k = tri.scale;
    let n = tri.normal;
    let a = body_vector_norm(k, vsub(first, o)?, n)?;
    if a[1] < 0.0 {
        return Ok(None);
    }
    let b = body_vector_norm(k, vsub(last, o)?, n)?;
    if a[1] < b[1] {
        return Ok(None);
    }
    let r2 = mul(r, r)?;
    let crossing = if 0.0 < a[1] { b[1] <= 0.0 } else { a[1] >= 0.0 || b[1] >= 0.0 };
    let mut inside_crossing = false;
    let (mut cx, mut cz) = (0.0f32, 0.0f32);
    if !crossing {
        if r < a[1] && r < b[1] {
            return Ok(None);
        }
        if a[1] < -r && b[1] < -r {
            return Ok(None);
        }
    } else {
        inside_crossing = true;
        let dx = sub(b[0], a[0])?;
        let dz = sub(b[2], a[2])?;
        if w(0x3a83126f) < fqdist(dx, dz)?.0 {
            let sum = add(a[1].abs(), b[1].abs())?;
            if sum == 0.0 {
                inside_crossing = false;
            } else {
                let t = div_rn(a[1].abs(), sum)?;
                cx = add(mul(dx, t)?, a[0])?;
                cz = add(mul(dz, t)?, a[2])?;
            }
        } else {
            cx = a[0];
            cz = a[2];
        }
    }
    let [u1, v1, u2, v2] = tri.uv;
    let p0 = [0.0, 0.0, 0.0];
    let p1 = [u1, 0.0, v1];
    let p2 = [u2, 0.0, v2];
    let mut distance = 0.0f32;
    let mut local: Option<V3> = None;
    if inside_crossing {
        let mut none_yet = true;
        fn consider(d: f32, out: V3, none_yet: &mut bool, distance: &mut f32, local: &mut Option<V3>) {
            if *none_yet || d < *distance {
                *distance = d;
                *local = Some(out);
            }
            *none_yet = false;
        }
        if 0.0 < sub(mul(cz, u1)?, mul(cx, v1)?)? {
            let (d, out) = line_line_dist_3d2d(a, b, p0, p1, true)?;
            consider(d, out, &mut none_yet, &mut distance, &mut local);
        }
        let (e0, e1) = (sub(u2, u1)?, sub(v2, v1)?);
        if 0.0 < sub(mul(e0, sub(cz, v1)?)?, mul(e1, sub(cx, u1)?)?)? {
            let (d, out) = line_line_dist_3d2d(a, b, p1, p2, true)?;
            consider(d, out, &mut none_yet, &mut distance, &mut local);
        }
        if 0.0 < sub(mul(-u2, sub(cz, v2)?)?, mul(-v2, sub(cx, u2)?)?)? {
            let (d, out) = line_line_dist_3d2d(a, b, p0, p2, true)?;
            consider(d, out, &mut none_yet, &mut distance, &mut local);
        }
        if none_yet {
            return finish(tri, [cx, 0.0, cz], 0.0);
        }
    } else {
        let order = if a[1].abs() < b[1].abs() {
            1
        } else if a[1] == b[1] {
            3
        } else {
            2
        };
        let (mut ia, mut ib) = (true, true);
        let outside_a = |x: f32, z: f32, edge: u8| -> R<bool> {
            Ok(0.0 < match edge {
                0 => sub(mul(z, u1)?, mul(x, v1)?)?,
                1 => sub(mul(sub(u2, u1)?, sub(z, v1)?)?, mul(sub(v2, v1)?, sub(x, u1)?)?)?,
                _ => sub(mul(-u2, sub(z, v2)?)?, mul(-v2, sub(x, u2)?)?)?,
            })
        };
        'tests: for edge in 0..3u8 {
            if outside_a(a[0], a[2], edge)? {
                ia = false;
                if order & 2 == 0 {
                    break 'tests;
                }
            }
            if outside_a(b[0], b[2], edge)? {
                ib = false;
                if order & 1 == 0 && edge < 2 {
                    break 'tests;
                }
            }
        }
        if ia && order & 1 != 0 {
            return finish(tri, [a[0], 0.0, a[2]], 0.0);
        }
        if ib && order & 2 != 0 {
            return finish(tri, [b[0], 0.0, b[2]], 0.0);
        }
        let (d, out) = line_line_dist_3d2d(a, b, p0, p1, false)?;
        distance = d;
        local = Some(out);
        let (d, out) = line_line_dist_3d2d(a, b, p1, p2, false)?;
        if d < distance {
            distance = d;
            local = Some(out);
        }
        let (d, out) = line_line_dist_3d2d(a, b, p0, p2, false)?;
        if d < distance {
            distance = d;
            local = Some(out);
        }
    }
    if r2 < distance {
        return Ok(None);
    }
    finish(tri, local.ok_or(UnsupportedCollision("edge distance without point"))?, distance)
}

fn finish(tri: &Triangle, local: V3, distance: f32) -> R<Option<(f32, V3)>> {
    let world = world_vector_norm(local, tri.scale, tri.normal)?;
    Ok(Some((distance, vadd(world, tri.origin)?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn flat() -> Triangle {
        Triangle {
            min_y64: -64,
            max_y64: 64,
            scale: f32::INFINITY,
            normal: [0.0, 1.0, 0.0],
            origin: [0.0, 0.0, 0.0],
            uv: [0.0, 4.0, 4.0, 0.0],
        }
    }
    #[test]
    fn vertical_segment_through_flat_triangle_hits_at_zero_distance() {
        let hit = b_tri_line_col(1.5, &flat(), [1.0, 2.0, 1.0], [1.0, -3.0, 1.0]).unwrap().unwrap();
        assert_eq!(hit, (0.0, [1.0, 0.0, 1.0]));
    }
    #[test]
    fn segment_beside_triangle_uses_edge_distance_within_radius() {
        let tri = flat();
        // Outside the u-axis edge by 1.0: the closest edge point is reported.
        let (d, p) = b_tri_line_col(1.5, &tri, [-1.0, 2.0, 1.0], [-1.0, -3.0, 1.0]).unwrap().unwrap();
        assert_eq!((d, p), (1.0, [0.0, 0.0, 1.0]));
        assert!(b_tri_line_col(0.5, &tri, [-1.0, 2.0, 1.0], [-1.0, -3.0, 1.0]).unwrap().is_none());
        // Above the plane on one side only, or below the start, is rejected.
        assert!(b_tri_line_col(1.5, &tri, [1.0, -1.0, 1.0], [1.0, -3.0, 1.0]).unwrap().is_none());
    }
    #[test]
    fn flat_basis_and_vector_frames_take_the_original_shortcuts() {
        assert_eq!(create_mat3_norm(f32::INFINITY, [0.0, 1.0, 0.0]).unwrap()[1], [0.0, 1.0, 0.0, 0.0]);
        assert_eq!(body_vector_norm(f32::INFINITY, [1.0, 2.0, 3.0], [0.0, -1.0, 0.0]).unwrap(), [1.0, -2.0, -3.0]);
        let n = [0.0, 0.0, 1.0];
        let local = body_vector_norm(1.0, [1.0, 2.0, 3.0], n).unwrap();
        assert_eq!(world_vector_norm(local, 1.0, n).unwrap(), [1.0, 2.0, 3.0]);
    }
}
