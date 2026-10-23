//! Static world collision transcribed from SLUS-20047 EE code: WorldCollide
//! 0x11e160, NextGrid 0x11de48, WorldObjCollide 0x11da08 (static objects),
//! CTriListCollide 0x11d600, BTriLineCol 0x11af80 and its leaf routines.
//!
//! Scalar EE arithmetic uses the guarded toward-zero add/sub, toward-zero
//! multiply, nearest DIV.S and nearest SQRT.S models. VU0 macro arithmetic uses
//! ordinary toward-zero add/sub/mul/div/sqrt. Dynamic objects (WorldDynCollide),
//! matrix/offset objects (flags 0x1000000/0x1000); TriLineCol (CheckFlags 0x20) is transcribed, not yet compared
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
    /// The file position: CreateWorldNode's node translation (+0x30), copied
    /// before the sum.
    pub local: V3,
    pub radius: f32,
    pub exclusion: i8,
    pub first_triangle: i32,
    /// +0x36: triangle count of a dynamic (offset/matrix) object.
    pub triangle_count: i16,
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

#[derive(Clone)]
pub struct CollisionWorld {
    pub objects: Vec<CollisionObject>,
    pub triangles: Vec<Triangle>,
    /// Objects with flags 0x1000/0x1000000 (WorldDynCollide candidates). The
    /// original's list (InitDynobjList) is the first grid cell word's
    /// entries (WORLDS header +20, cells[0]: count << 22 | contents offset,
    /// plain i16 object indices); it is this set, in this order, in G1, L1,
    /// A1 and K4, and no grid row uses cell 0.
    pub dynamic: Vec<usize>,
    /// CreateDynobjGrid 0x11f5f8's GetWorldMat per 0x1000000 dynamic object
    /// (the dynobj entry's +0x00 matrix). WorldDynCollide passes the entry to
    /// WorldObjCollide (a3 at 0x11fd70), so the dynamic pass tests those
    /// objects at the pose of the last grid build (world_update after
    /// SetupDynGrid; GauntWorldInit after AddItemInstList).
    pub dyn_matrix: Vec<Option<[[f32; 4]; 4]>>,
    /// Runtime +0x10 flags and +0x35 exclusion bytes, kept by the game.
    pub runtime_flags: Vec<u32>,
    pub runtime_exclusion: Vec<i8>,
    /// Node translation y of root offset objects driven by item wobjs.
    pub node_y: Vec<Option<f32>>,
    /// Node rotation rows 0..2 changed at run time (rotators' YawMat3);
    /// None = the identity rest rotation.
    pub node_rot: Vec<Option<[V3; 3]>>,
    /// Node local matrix written by a world animation (DoWorldAnimSub):
    /// rows 0..2 rotation, row 3 translation (the object's +0x1c position
    /// plus the animated offset); the scale channel is kept separately.
    pub node_local: Vec<Option<[[f32; 4]; 4]>>,
    pub node_scale: Vec<Option<V3>>,
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
    /// World matrix of a dynamic object hit (rows, translation); None for static.
    pub matrix: Option<[[f32; 4]; 4]>,
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

/// An object's +0x10 after NewWorldObject 0x11bd08 (the WORLDS records are
/// used in place): objects with a model (+0x28 == 1) or bit 31 get bit 31
/// (outside 0x800 particle objects), and objects with 0x100 lose
/// 0x2/0x8/0x10/0x20 (`& 0xffffffc5`; A1, K2, the Tower and the boss levels:
/// every 0x102 object holds 0x100 in the captures). WorldPsysActivate's
/// 0x800000 on 0x800 objects is not applied (it depends on the level's psys
/// table; no collision test reads it).
pub fn new_world_object_flags(raw: &[u8]) -> u32 {
    let word = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
    let mut f = word(0x10);
    if f & 0x800 == 0 && (word(0x28) == 1 || f & 0x8000_0000 != 0) {
        f |= 0x8000_0000;
    }
    if f & 0x100 != 0 {
        f &= 0xffff_ffc5;
    }
    f
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
            // CreateWorldNode 0x11b9a0: objects with flag 0x1000 or 0x1000000
            // keep their local file position; the others add their parent's
            // (already final) +0x1c, even under such an object (Errorf only).
            // Verified for all 41 matrix objects in the Poison Fields fixture
            // and the Tower's world-animated 0x1000 objects.
            if w.nodes[i].flags & 0x100_1000 != 0 {
                out[i] = Some(own);
                return Ok(own);
            }
            let v = match p[i] {
                None => own,
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
                flags: new_world_object_flags(&node.raw),
                parent: parent[i],
                position: sum(i, world, &parent, &mut position, 0)?,
                local: node.position,
                radius: node.radius,
                exclusion: raw[0x35] as i8,
                first_triangle: i32::from_le_bytes(raw[0x38..0x3c].try_into().unwrap()),
                triangle_count: i16::from_le_bytes([raw[0x36], raw[0x37]]),
            });
        }
        let triangles = world.collisions.iter().map(Triangle::from_record).collect();
        let header = &world.raw_header;
        let cell = f32::from_le_bytes(header[60..64].try_into().unwrap());
        let dynamic = objects
            .iter()
            .enumerate()
            .filter(|(_, o)| o.flags & (0x100_0000 | 0x1000) != 0 && 0 <= o.first_triangle && 0 < o.triangle_count)
            .map(|(i, _)| i)
            .collect();
        let runtime_flags = objects.iter().map(|o| o.flags).collect();
        let runtime_exclusion = objects.iter().map(|o| o.exclusion).collect();
        let node_y = vec![None; objects.len()];
        let node_rot = vec![None; objects.len()];
        let node_local = vec![None; objects.len()];
        let node_scale = vec![None; objects.len()];
        let dyn_matrix = vec![None; objects.len()];
        Ok(Self {
            dyn_matrix,
            node_rot,
            node_local,
            node_scale,
            dynamic,
            runtime_flags,
            runtime_exclusion,
            node_y,
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
        self.dyn_collide(&ctx, &mut found)?;
        Ok((found, state))
    }

    /// ItemObjectCol's collide: transform from/to into the frame (rows `m`,
    /// translation `t`), test `count` triangles from `first` (CheckFlags 0,
    /// normal-y -0.5..2.0) and return the world contact and normal.
    #[allow(clippy::too_many_arguments)]
    pub fn local_list_collide(&mut self, r: f32, first: i32, count: i16, from: V3, to: V3, m: [V3; 3], t: V3) -> R<Option<(V3, V3)>> {
        let (lo, hi) = if from[1] < to[1] { (sub(from[1], r)?, add(r, to[1])?) } else { (sub(to[1], r)?, add(r, from[1])?) };
        self.check_counter += 1;
        if self.check_counter > 0xff {
            self.check_counter = 1;
        }
        let body = |v: V3| -> R<V3> {
            let q = [sub(v[0], t[0])?, sub(v[1], t[1])?, sub(v[2], t[2])?];
            let row = |k: usize| -> R<f32> { Ok(add(add(mul(q[0], m[k][0])?, mul(q[1], m[k][1])?)?, mul(q[2], m[k][2])?)?) };
            Ok([row(0)?, row(1)?, row(2)?])
        };
        let (lf, ll) = (body(from)?, body(to)?);
        let scale = |y: f32| -> R<i32> { Ok(trunc(mul(mul(sub(y, t[1])?, m[1][1])?, 64.0)?) as i16 as i32) };
        let (ymin, ymax) = (scale(lo)?, scale(hi)?);
        let mut dir = [sub(ll[0], lf[0])?, sub(ll[1], lf[1])?, sub(ll[2], lf[2])?];
        let length = normal_vector(&mut dir)?;
        let q = Query {
            radius: r,
            first: lf,
            last: ll,
            check_flags: 0,
            min_normal_y: -0.5,
            max_normal_y: 2.0,
            object_mask: 0,
            exclusion_mask: 0,
        };
        // CTriListCollide's metric uses ColDirVec, the world direction.
        let mut wdir = [sub(to[0], from[0])?, sub(to[1], from[1])?, sub(to[2], from[2])?];
        normal_vector(&mut wdir)?;
        let c = Ctx { q, dir: wdir, length, min_y: lo, max_y: hi, start_x: 0.0, start_z: 0.0, end_x: 0.0, end_z: 0.0, check_num: self.check_counter as u8 as i8 };
        let _ = dir;
        let list: Vec<i16> = (0..count.max(0)).collect();
        let (d, hit) = self.tri_list_collide(&c, first, &list, ymin, ymax, false)?;
        if d < 0.0 {
            return Ok(None);
        }
        let (tri, local) = hit.ok_or(UnsupportedCollision("hit without triangle"))?;
        let world = |v: V3, with_t: bool| -> R<V3> {
            let col = |k: usize| -> R<f32> {
                let mut acc = mul(m[0][k], v[0])?;
                acc = add_rz(acc, mul(m[1][k], v[1])?)?;
                acc = add_rz(acc, mul(m[2][k], v[2])?)?;
                if with_t {
                    acc = add_rz(acc, t[k])?;
                }
                Ok(acc)
            };
            Ok([col(0)?, col(1)?, col(2)?])
        };
        let n = crate::player_collision::world_vector(self.triangles[tri].normal, &m).map_err(|_| UnsupportedCollision("WorldVector domain"))?;
        Ok(Some((world(local, true)?, n)))
    }

    /// GetWorldMat 0x1d06b8 of an object's node: its local matrix
    /// (node_matrix), times each object ancestor's local with MulMat4
    /// 0x1d55a0 in order (immediate parent first), then the
    /// world root nodes every object node hangs from: an identity under
    /// MulMat4 (+0x60 0x1000; it turns −0.0 into +0.0) and a translation-only
    /// identity (+0x60 4: add.s of a zero translation). Read from the G1 and
    /// Tower savestates (nodes 0x1693be0/0x16935e0, 0x1dee0a0/0x1dedaa0).
    pub fn object_matrix(&self, o: usize) -> R<[[f32; 4]; 4]> {
        self.chain_world(self.node_matrix(o), self.node_parent(o))
    }

    /// The object's node parent: CreateWorldNode 0x11b9a0 makes every
    /// object's node under its parent object's node (roots under
    /// CurWorldRoot), whatever the flags (w1l5-boss-v1: A5ELEVATOR, flag
    /// 0x1000, under the static A5UNNAMED#0 and A5GEOMETRY, whose node holds
    /// y −0.0703).
    pub fn node_parent(&self, o: usize) -> Option<usize> {
        self.objects[o].parent
    }

    /// The object node's local matrix: a world animation's, else the run-time
    /// rotation rows (identity) with the node translation — the file
    /// position (CreateWorldNode copies it before the static sum), its y the
    /// item-driven one for wobjs (ProcessItemWobjs writes node +0x34).
    pub fn node_matrix(&self, o: usize) -> [[f32; 4]; 4] {
        if let Some(m) = self.node_local[o] {
            return m;
        }
        let ob = &self.objects[o];
        let t = match self.node_y[o] {
            Some(y) => [ob.local[0], y, ob.local[2]],
            None => ob.local,
        };
        let r = self.node_rot[o].unwrap_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        [[r[0][0], r[0][1], r[0][2], 0.0], [r[1][0], r[1][1], r[1][2], 0.0], [r[2][0], r[2][1], r[2][2], 0.0], [t[0], t[1], t[2], 1.0]]
    }

    /// UpdateObjWorldMat's UnparentMatrix 0x1d0620 for a node under the
    /// object `parent`: recursive, so the outermost ancestor goes first —
    /// the translation-only root (sub.s), the identity root
    /// (MulMat4(InvertMat4(identity), m)), then each object ancestor's local
    /// inverse down to the parent (MulMat4(InvertMat4(local), m)). Each level
    /// inverts its own local: a transposed animated rotation with scale is not
    /// an exact inverse, so this differs from inverting the parent's world.
    pub fn unparent_chain(&self, local: [[f32; 4]; 4], parent: Option<usize>) -> R<[[f32; 4]; 4]> {
        let mut chain = Vec::new();
        let mut k = parent;
        while let Some(p) = k {
            chain.push(p);
            if chain.len() > 128 {
                return Err(UnsupportedCollision("object parent chain policy exceeded"));
            }
            k = self.node_parent(p);
        }
        let mut m = local;
        for v in m[3].iter_mut().take(3) {
            *v = sub(*v, 0.0)?;
        }
        let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        m = mul_mat4(&invert_mat4(&identity)?, &m)?;
        for &p in chain.iter().rev() {
            m = mul_mat4(&invert_mat4(&self.node_matrix(p))?, &m)?;
        }
        Ok(m)
    }

    /// GetWorldMat from a node's local matrix under the object `parent`
    /// (None: directly under the world root).
    pub fn chain_world(&self, local: [[f32; 4]; 4], parent: Option<usize>) -> R<[[f32; 4]; 4]> {
        let mut m = local;
        let mut k = parent;
        let mut guard = 0;
        while let Some(p) = k {
            m = mul_mat4(&self.node_matrix(p), &m)?;
            k = self.node_parent(p);
            guard += 1;
            if guard > 128 {
                return Err(UnsupportedCollision("object parent chain policy exceeded"));
            }
        }
        let identity = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        m = mul_mat4(&identity, &m)?;
        for v in m[3].iter_mut().take(3) {
            *v = add(0.0, *v)?;
        }
        Ok(m)
    }

    /// object_matrix at the file rest pose (no item-driven y, no run-time
    /// rotation); presentation uses it to displace moving nodes.
    pub fn object_rest_matrix(&self, o: usize) -> R<[[f32; 4]; 4]> {
        let ob = &self.objects[o];
        let t = match ob.parent {
            Some(p) => {
                let pm = self.object_rest_matrix(p)?;
                [add_rz(ob.local[0], pm[3][0])?, add_rz(ob.local[1], pm[3][1])?, add_rz(ob.local[2], pm[3][2])?]
            }
            None => ob.local,
        };
        Ok([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [t[0], t[1], t[2], 1.0]])
    }

    /// Whether item logic has moved this object (elevator y or rotation).
    pub fn object_moved(&self, o: usize) -> bool {
        self.node_y[o].is_some() || self.node_rot[o].is_some() || self.node_local[o].is_some()
    }

    /// YawMat3 0x1d5388 on an object's node rotation (rows 0 and 2).
    pub fn yaw_object(&mut self, o: usize, a: f32) -> R<()> {
        let m = self.node_rot[o].get_or_insert([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
        yaw_mat3(m, a)
    }

    /// CreateDynobjGrid: cache each 0x1000000 dynamic object's world matrix
    /// (the cells themselves are not modelled; see dyn_collide).
    pub fn create_dynobj_grid(&mut self) -> R<()> {
        for k in 0..self.dynamic.len() {
            let o = self.dynamic[k];
            self.dyn_matrix[o] = if self.runtime_flags[o] & 0x100_0000 != 0 { Some(self.object_matrix(o)?) } else { None };
        }
        Ok(())
    }

    /// WorldDynCollide/WorldObjCollide for dynamic objects. Candidates are all
    /// dynamic objects instead of the dyngrid cells; WorldObjCollide's own
    /// segment-sphere rejection is applied as in the original.
    fn dyn_collide(&mut self, c: &Ctx, found: &mut Collision) -> R<()> {
        for k in 0..self.dynamic.len() {
            let o = self.dynamic[k];
            let flags = self.runtime_flags[o];
            let mut excl = self.runtime_exclusion[o];
            if let Some(p) = self.objects[o].parent {
                excl |= self.runtime_exclusion[p];
            }
            if c.q.object_mask & flags == 0 || i32::from(excl) & c.q.exclusion_mask != 0 || flags & 0x1000_0000 != 0 {
                continue;
            }
            // 0x1000000: the grid build's matrix (fresh GetWorldMat only
            // before any build); 0x1000 roots: the live node.
            let cached = if flags & 0x100_0000 != 0 { self.dyn_matrix[o] } else { None };
            let m = match cached {
                Some(m) => m,
                None => self.object_matrix(o)?,
            };
            let p = [m[3][0], m[3][1], m[3][2]];
            let first = c.q.first;
            let d = [sub(p[0], first[0])?, sub(p[1], first[1])?, sub(p[2], first[2])?];
            let t = add(add(mul(d[0], c.dir[0])?, mul(d[1], c.dir[1])?)?, mul(d[2], c.dir[2])?)?;
            let rr = add(c.q.radius, self.objects[o].radius)?;
            if add(rr, c.length)? < t || t < -rr {
                continue;
            }
            let e = [
                sub(p[0], add(mul(t, c.dir[0])?, first[0])?)?,
                sub(p[1], add(mul(t, c.dir[1])?, first[1])?)?,
                sub(p[2], add(mul(t, c.dir[2])?, first[2])?)?,
            ];
            if mul(rr, rr)? <= add(add(mul(e[0], e[0])?, mul(e[1], e[1])?)?, mul(e[2], e[2])?)? {
                continue;
            }
            let body = |v: V3| -> R<V3> {
                let q = [sub(v[0], m[3][0])?, sub(v[1], m[3][1])?, sub(v[2], m[3][2])?];
                let row = |r: usize| -> R<f32> { Ok(add(add(mul(q[0], m[r][0])?, mul(q[1], m[r][1])?)?, mul(q[2], m[r][2])?)?) };
                Ok([row(0)?, row(1)?, row(2)?])
            };
            let offset_only = flags & 0x100_0000 == 0 && flags & 1 != 0;
            let (lf, ll, ymin, ymax) = if offset_only {
                (
                    [sub(first[0], m[3][0])?, sub(first[1], m[3][1])?, sub(first[2], m[3][2])?],
                    [sub(c.q.last[0], m[3][0])?, sub(c.q.last[1], m[3][1])?, sub(c.q.last[2], m[3][2])?],
                    sub(c.min_y, m[3][1])?,
                    sub(c.max_y, m[3][1])?,
                )
            } else {
                (body(first)?, body(c.q.last)?, mul(sub(c.min_y, m[3][1])?, m[1][1])?, mul(sub(c.max_y, m[3][1])?, m[1][1])?)
            };
            let local_q = Query { first: lf, last: ll, ..c.q };
            let lc = Ctx { q: local_q, ..*c };
            let list: Vec<i16> = (0..self.objects[o].triangle_count).collect();
            let (distance, hit) = self.tri_list_collide(
                &lc,
                self.objects[o].first_triangle,
                &list,
                trunc(mul(ymin, 64.0)?),
                trunc(mul(ymax, 64.0)?),
                flags & 0x40 != 0,
            )?;
            if distance < 0.0 {
                continue;
            }
            let (triangle, local) = hit.ok_or(UnsupportedCollision("hit without triangle"))?;
            // WorldCollide's tail turns the local hit into the output struct
            // with a fresh GetWorldMat for 0x1000000 objects.
            let out = if cached.is_some() { self.object_matrix(o)? } else { m };
            let position = if offset_only {
                [add(local[0], out[3][0])?, add(local[1], out[3][1])?, add(local[2], out[3][2])?]
            } else {
                // MulVec4Mat4 (VU0 under EE rounding).
                let col = |c_: usize| -> R<f32> {
                    let mut acc = mul(out[0][c_], local[0])?;
                    acc = add_rz(acc, mul(out[1][c_], local[1])?)?;
                    acc = add_rz(acc, mul(out[2][c_], local[2])?)?;
                    Ok(add_rz(acc, out[3][c_])?)
                };
                [col(0)?, col(1)?, col(2)?]
            };
            let slot = if flags & 0x200 != 0 { &mut found.water } else { &mut found.solid };
            if distance < slot.map_or(w(0x6258d727), |h| h.distance) {
                *slot = Some(Hit { object: o, triangle, distance, position, matrix: Some(out) });
            }
        }
        Ok(())
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
        // The live +0x10 word (the original has one; items and
        // NewWorldObject rewrite it).
        let live = self.runtime_flags.get(index).copied().unwrap_or(object.flags);
        if c.q.object_mask & live == 0
            || i32::from(exclusion) & c.q.exclusion_mask != 0
            || object.first_triangle < 0
        {
            return Ok(());
        }
        if object.flags & (0x100_0000 | 0x1000) != 0 {
            // Dynamic objects are not in the static grid; WorldDynCollide handles them.
            return Ok(());
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
            self.tri_list_collide(c, object.first_triangle, list, ymin, ymax, live & 0x40 != 0)?;
        if 0.0 <= distance {
            let (triangle, position) = hit.ok_or(UnsupportedCollision("hit without triangle"))?;
            let slot = if live & 0x200 != 0 { &mut found.water } else { &mut found.solid };
            // Both start at 1e21 (col_dist and water_col +0x40 initialisation).
            if distance < slot.map_or(w(0x6258d727), |h| h.distance) {
                *slot = Some(Hit { object: index, triangle, distance, position, matrix: None });
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
            let tested = if c.q.check_flags & 0x20 != 0 {
                tri_line_col(&tri, c.q.first, c.q.last)?
            } else {
                b_tri_line_col(c.q.radius, &tri, c.q.first, c.q.last)?
            };
            let Some((d, hit)) = tested else {
                continue;
            };
            if c.q.check_flags & 0x20 != 0 {
                // TriLineCol queries stop at the first hit.
                return Ok((0.0, Some((index, hit))));
            }
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

/// InvertMat4 0x1d4e80: the transposed rotation (w 0) and the translation
/// −t · rowᵢ with VU operations — vsub from vf0 (0 − t, so +0 stays +0),
/// vmul, then (x + y) + z; w keeps the input's.
pub fn invert_mat4(m: &[[f32; 4]; 4]) -> R<[[f32; 4]; 4]> {
    let mut out = [[0.0f32; 4]; 4];
    for (i, row) in out.iter_mut().take(3).enumerate() {
        for (j, v) in row.iter_mut().take(3).enumerate() {
            *v = m[j][i];
        }
    }
    let t = [sub_rz(0.0, m[3][0])?, sub_rz(0.0, m[3][1])?, sub_rz(0.0, m[3][2])?];
    for i in 0..3 {
        let r = m[i];
        out[3][i] = add_rz(add_rz(mul(t[0], r[0])?, mul(t[1], r[1])?)?, mul(t[2], r[2])?)?;
    }
    out[3][3] = m[3][3];
    Ok(out)
}

/// MulMat4 0x1d55a0(parent, child) on row matrices: child × parent with the
/// VU0 multiply/accumulate order (world_transform::multiply).
pub fn mul_mat4(parent: &[[f32; 4]; 4], child: &[[f32; 4]; 4]) -> R<[[f32; 4]; 4]> {
    let mut a: [f32; 16] = std::array::from_fn(|k| parent[k / 4][k % 4]);
    let mut b: [f32; 16] = std::array::from_fn(|k| child[k / 4][k % 4]);
    let w = crate::world_transform::multiply(&mut a, &mut b)?;
    Ok(std::array::from_fn(|i| std::array::from_fn(|j| w[i * 4 + j])))
}

/// Row 1 of create_mat3_norm (the normal, or ±y past the 0x3f7fbe77 limits).
pub fn create_mat3_norm_row1(n: V3) -> V3 {
    if w(0x3f7fbe77) < n[1] {
        [0.0, 1.0, 0.0]
    } else if n[1] < w(0xbf7fbe77) {
        [0.0, -1.0, 0.0]
    } else {
        n
    }
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

/// TriLineCol 0x11b6c0 (CheckFlags 0x20): the radius-less segment test;
/// a hit reports distance 0.0 to CTriListCollide.
fn tri_line_col(tri: &Triangle, first: V3, last: V3) -> R<Option<(f32, V3)>> {
    let o = tri.origin;
    let k = tri.scale;
    let n = tri.normal;
    let a = body_vector_norm(k, vsub(first, o)?, n)?;
    if a[1] < 0.0 {
        return Ok(None);
    }
    let b = body_vector_norm(k, vsub(last, o)?, n)?;
    if a[1] < b[1] || (0.0 < a[1] && 0.0 < b[1]) || (a[1] < 0.0 && b[1] < 0.0) {
        return Ok(None);
    }
    let dx = sub(b[0], a[0])?;
    let dz = sub(b[2], a[2])?;
    let (mut px, mut pz) = (a[0], a[2]);
    if w(0x3c23_d70a) < fqdist(dx, dz)?.0 {
        let sum = add(a[1].abs(), b[1].abs())?;
        let t = if sum == 0.0 { mul(a[1].abs(), 1000.0)? } else { div_rn(a[1].abs(), sum)? };
        px = add(mul(dx, t)?, a[0])?;
        pz = add(mul(dz, t)?, a[2])?;
    }
    let [u1, v1, u2, v2] = tri.uv;
    if 0.0 < sub(mul(u1, pz)?, mul(v1, px)?)? {
        return Ok(None);
    }
    if 0.0 < sub(mul(sub(u2, u1)?, sub(pz, v1)?)?, mul(sub(v2, v1)?, sub(px, u1)?)?)? {
        return Ok(None);
    }
    if 0.0 < sub(mul(-u2, sub(pz, v2)?)?, mul(-v2, sub(px, u2)?)?)? {
        return Ok(None);
    }
    let local = world_vector_norm([px, 0.0, pz], k, n)?;
    Ok(Some((0.0, vadd(local, o)?)))
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

/// YawMat3 0x1d5388: no-op below |a| < 0.0001 (0x38d1b717); otherwise
/// row0' = row0·cos − row2·sin and row2' = row2·cos + row0·sin per column.
pub fn yaw_mat3(m: &mut [V3; 3], a: f32) -> R<()> {
    if a.abs() < f32::from_bits(0x38d1_b717) {
        return Ok(());
    }
    let s = crate::libm::sinf(a).map_err(|_| UnsupportedCollision("sinf domain"))?;
    let c = crate::libm::cosf(a).map_err(|_| UnsupportedCollision("cosf domain"))?;
    let [r0, _, r2] = m;
    for (x, z) in r0.iter_mut().zip(r2.iter_mut()) {
        let f5 = *z;
        *z = add(mul(f5, c)?, mul(*x, s)?)?;
        *x = sub(mul(*x, c)?, mul(f5, s)?)?;
    }
    Ok(())
}

/// The shared PitchMat3 0x1d5440 / RollMat3 0x1d54f0 form: no-op below
/// |a| < 0.0001; per column a' = cos·a − sin·b, b' = cos·b + sin·a over rows
/// (`ra`, `rb`) — (1, 2) for pitch, (0, 1) for roll.
fn rot_rows(m: &mut [V3; 3], a: f32, ra: usize, rb: usize) -> R<()> {
    if a.abs() < f32::from_bits(0x38d1_b717) {
        return Ok(());
    }
    let s = crate::libm::sinf(a).map_err(|_| UnsupportedCollision("sinf domain"))?;
    let c = crate::libm::cosf(a).map_err(|_| UnsupportedCollision("cosf domain"))?;
    let (a_row, b_row) = (m[ra], m[rb]);
    for (j, (x, y)) in a_row.into_iter().zip(b_row).enumerate() {
        m[ra][j] = sub(mul(c, x)?, mul(s, y)?)?;
        m[rb][j] = add(mul(c, y)?, mul(s, x)?)?;
    }
    Ok(())
}

/// PitchMat3 0x1d5440: rows 1 and 2.
pub fn pitch_mat3(m: &mut [V3; 3], a: f32) -> R<()> {
    rot_rows(m, a, 1, 2)
}

/// RollMat3 0x1d54f0: rows 0 and 1.
pub fn roll_mat3(m: &mut [V3; 3], a: f32) -> R<()> {
    rot_rows(m, a, 0, 1)
}

#[cfg(test)]
mod tests {

    #[test]
    fn yaw_mat3_rotates_rows_zero_and_two() {
        // Synthetic: a quarter turn maps row 0 (1,0,0) to (cos, 0, sin).
        let mut m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        super::yaw_mat3(&mut m, std::f32::consts::FRAC_PI_2).unwrap();
        assert!(m[0][0].abs() < 1e-6 && (m[0][2] + 1.0).abs() < 1e-6);
        assert!((m[2][0] - 1.0).abs() < 1e-6 && m[2][2].abs() < 1e-6);
        assert_eq!(m[1], [0.0, 1.0, 0.0]);
        let before = m;
        super::yaw_mat3(&mut m, 0.00005).unwrap();
        assert_eq!(m, before);
    }
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
