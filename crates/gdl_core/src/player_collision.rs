//! PlayerMotion's static collision stages: PlayerCollideWalls 0x144f28 with
//! PlayerWallCollide 0x11ee58 and SlideAlongWall 0x11f028, PlayerCollideFloor
//! 0x145218, then PlayerMotion's floor-height adjustment (EE 0x148d48..0x148dd4),
//! slope rescale (EE 0x148e30..0x148e94) and final position add (EE 0x14c14c).
//!
//! Not included, and therefore not a complete PlayerMotion: item/player/enemy
//! contact stages, CameraLimitPlayerDpos (camera system), PlayerNewFloor's
//! moving-floor handling and slope factor, transporters/exits, damage callbacks,
//! dynamic objects. The comparison against reference traces measures where this
//! composition is sufficient.
use crate::collision::{create_mat3_norm, normal_vector, CollisionWorld, Hit, UnsupportedCollision};
use crate::numeric::{add_guarded_rz as add, div_rn, mul_rz as mul, sub_guarded_rz as sub};
use crate::vu0::fqdist;

type R<T> = Result<T, UnsupportedCollision>;
type V3 = [f32; 3];

fn w(bits: u32) -> f32 {
    f32::from_bits(bits)
}

/// Player fields read or written by the stages here (offsets from the record).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerFloorState {
    pub floor_y_88c: f32,
    pub floor_object_89c: Option<usize>,
    pub flags_8ac: u32,
    pub word_100: i32,
    pub y_54: f32,
    pub wall_object_8a0: Option<usize>,
    /// Diagnostic output: object of PlayerCollideFloor's first contact. The
    /// original writer of +0x89c is not recovered.
    pub floor_contact: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
pub struct DisplacementInput {
    /// Player matrix rows 0..2 (+0x20/+0x30/+0x40, xyz) as of the update start.
    pub matrix: [V3; 3],
    /// Collision offset +0x81c and object offset flags +0x84.
    pub collision_offset_81c: V3,
    pub offset_flags_84: u32,
    /// World position +0x50..+0x58 that receives the final displacement.
    pub position: V3,
    pub request: V3,
    /// f22: the speed scalar from the request computation (slope rescale).
    pub speed: f32,
    pub radius_828: f32,
    pub height_82c: f32,
    pub flags_8b0: u32,
    pub current_action: u32,
    pub spf: f32,
    pub world_bottom: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct DisplacementOutput {
    pub displacement: V3,
    pub position: V3,
    pub wall_result: i32,
    pub floor_result: i32,
}

struct WallContact {
    object: usize,
    triangle: usize,
    hit: V3,
    normal: V3,
}

/// Collision output-struct row 1 (CreateMat3Norm normal row) for a static hit.
pub(crate) fn hit_normal(world: &CollisionWorld, hit: &Hit) -> R<V3> {
    let t = world.triangles[hit.triangle];
    let m = create_mat3_norm(t.scale, t.normal)?;
    Ok([m[1][0], m[1][1], m[1][2]])
}

fn object_flags_with_parents(world: &CollisionWorld, mut object: Option<usize>) -> u32 {
    let mut flags = 0;
    while let Some(o) = object {
        flags |= world.objects[o].flags;
        object = world.objects[o].parent;
    }
    flags
}

/// PlayerWallCollide: CheckFlags 7, normal-y ±0x3f5db22d, mask 0x13a, exclusion 1.
fn wall_collide(world: &mut CollisionWorld, r: f32, first: V3, last: V3) -> R<Option<WallContact>> {
    let (c, _) = world.world_collide(crate::collision::Query {
        radius: r,
        first,
        last,
        check_flags: 7,
        min_normal_y: w(0xbf5db22d),
        max_normal_y: w(0x3f5db22d),
        object_mask: 0x13a,
        exclusion_mask: 1,
    })?;
    Ok(match c.solid {
        Some(h) => Some(WallContact {
            object: h.object,
            triangle: h.triangle,
            hit: h.position,
            normal: hit_normal(world, &h)?,
        }),
        None => None,
    })
}

/// SlideAlongWall 0x11f028; returns 0, 1 or -1 as the original.
pub(crate) fn slide_along_wall(r: f32, p: V3, d: &mut V3, h: V3, n: V3) -> R<i32> {
    let ex = sub(add(p[0], d[0])?, h[0])?;
    let ez = sub(add(p[2], d[2])?, h[2])?;
    let depth = sub(add(mul(ex, n[0])?, mul(ez, n[2])?)?, r)?;
    let mostly_z = d[0].abs() < d[2].abs();
    if depth >= 0.0 {
        return Ok(0);
    }
    let mut result = 1;
    let fx = mul(depth, n[0])?;
    if mostly_z || (0.0 < d[0] && 0.0 < fx) || (d[0] < 0.0 && fx < 0.0) {
        let limit = add(d[0].abs(), mul(r, 0.5)?)?;
        if if fx < 0.0 { -fx < limit } else { fx < limit } {
            d[0] = sub(d[0], fx)?;
        } else {
            d[0] = 0.0;
            result = -1;
        }
    }
    let fz = mul(depth, n[2])?;
    if !mostly_z || (0.0 < d[2] && 0.0 < fz) || (d[2] < 0.0 && fz < 0.0) {
        let limit = add(d[2].abs(), mul(r, 0.5)?)?;
        if if fz < 0.0 { -fz < limit } else { fz < limit } {
            d[2] = sub(d[2], fz)?;
        } else {
            d[2] = 0.0;
            result = -1;
        }
    }
    Ok(result)
}

/// PlayerCollideWalls; returns (result, wall contact used by the floor stage).
fn collide_walls(
    world: &mut CollisionWorld,
    r: f32,
    action: u32,
    pos: V3,
    d: &mut V3,
) -> R<(i32, Option<WallContact>)> {
    let target = [add(pos[0], d[0])?, add(pos[1], d[1])?, add(pos[2], d[2])?];
    let Some(first) = wall_collide(world, r, pos, target)? else {
        return Ok((0, None));
    };
    let flags = world.objects[first.object].flags;
    if flags & 0x38 != 0 {
        return Ok((if action == 0x8f { 1 } else { 2 }, Some(first)));
    }
    if flags & 0x1000 != 0 {
        // Offset (moving) object: remove the displacement along the normal.
        let n = first.normal;
        let k = -add(add(mul(d[0], n[0])?, mul(d[1], n[1])?)?, mul(d[2], n[2])?)?;
        d[0] = add(mul(k, n[0])?, d[0])?;
        d[1] = add(mul(k, n[1])?, d[1])?;
        d[2] = add(mul(k, n[2])?, d[2])?;
    } else {
        slide_along_wall(r, pos, d, first.hit, first.normal)?;
    }
    let target = [add(pos[0], d[0])?, add(pos[1], d[1])?, add(pos[2], d[2])?];
    world.bump_check(first.triangle);
    let mut contact = first;
    if let Some(second) = wall_collide(world, mul(r, w(0x3f733333))?, pos, target)? {
        let n0 = contact.normal;
        let n1 = second.normal;
        let facing = add(add(mul(n1[0], n0[0])?, mul(n1[1], n0[1])?)?, mul(n1[2], n0[2])?)?;
        if facing < 0.5 {
            *d = [0.0; 3];
        }
        contact = second;
    }
    Ok((1, Some(contact)))
}

/// PlayerCollideFloor 0x145218.
#[allow(clippy::too_many_arguments)]
fn collide_floor(
    world: &mut CollisionWorld,
    r: f32,
    h: f32,
    s: &mut PlayerFloorState,
    pos: V3,
    d: &mut V3,
    wall_result: i32,
    wall: Option<&WallContact>,
    world_bottom: f32,
) -> R<i32> {
    let bottom = -add(h, 3.0)?;
    let target = |d: &V3| -> R<V3> { Ok([add(pos[0], d[0])?, add(pos[1], d[1])?, add(pos[2], d[2])?]) };
    // Water hits only feed player_water_* (water height/damage), not displacement.
    let floor = |world: &mut CollisionWorld, at: V3, by_parameter: bool| -> R<Option<Hit>> {
        Ok(world.floor_collide(r, 0.0, bottom, at, by_parameter, 1)?.0.solid)
    };
    let mut at = target(d)?;
    let first = floor(world, at, true)?;
    let speed = fqdist(d[0], d[2])?.0;
    let all = object_flags_with_parents(world, s.floor_object_89c);
    let gate = all & 0x0c00_0000 != 0 && all & 0x2000_0000 != 0;
    let first_object = first.map(|h| h.object);
    s.floor_contact = first_object;
    if gate && s.floor_object_89c != first_object {
        *d = [0.0; 3];
        return Ok(if first.is_some() { -1 } else { -2 });
    }
    let Some(contact) = first else {
        if speed < w(0x3a83126f) {
            s.floor_object_89c = None;
        }
        if s.flags_8ac & 0x8000 == 0 {
            d[0] = 0.0;
            d[2] = 0.0;
        }
        s.floor_y_88c = world_bottom;
        return Ok(-2);
    };
    let contact_y = contact.position[1];
    let change = sub(contact_y, s.floor_y_88c)?.abs();
    if s.flags_8ac & 0x8000 != 0 {
        if sub(s.y_54, contact_y)? >= w(0x3e4ccccd) {
            return Ok(0);
        }
        s.flags_8ac &= 0xffff_7fff;
    }
    if speed < w(0x3a83126f) && s.word_100 == 1 {
        if 3.0 < change || world.objects[contact.object].flags & 0x1000 != 0 {
            s.floor_y_88c = contact_y;
        }
        return Ok(if w(0x3a83126f) < change { 1 } else { 0 });
    }
    let step_limit = if Some(contact.object) == s.floor_object_89c { 4.0 } else { 3.0 };
    if step_limit < change {
        s.floor_y_88c = s.y_54;
        d[0] = 0.0;
        d[2] = 0.0;
        return Ok(0);
    }
    let requery = |world: &mut CollisionWorld, d: &mut V3, s: &mut PlayerFloorState| -> R<i32> {
        let found = floor(world, target(d)?, false)?;
        if gate && s.floor_object_89c != found.map(|h| h.object) {
            *d = [0.0; 3];
            return Ok(-1);
        }
        match found {
            Some(hit) => {
                s.floor_y_88c = hit.position[1];
                Ok(2)
            }
            None => {
                d[0] = 0.0;
                d[2] = 0.0;
                Ok(0)
            }
        }
    };
    if wall_result == 2 {
        let wall = wall.ok_or(UnsupportedCollision("wall result without contact"))?;
        if Some(contact.object) != Some(wall.object) {
            slide_along_wall(r, pos, d, wall.hit, wall.normal)?;
            return requery(world, d, s);
        }
    }
    let mut e = [sub(at[0], contact.position[0])?, sub(at[1], contact.position[1])?, sub(at[2], contact.position[2])?];
    let mut along = add(add(mul(d[0], e[0])?, mul(d[1], e[1])?)?, mul(d[2], e[2])?)?;
    let mut off = fqdist(e[0], e[2])?.0;
    if off < w(0x3a83126f) && wall_result == 0 {
        let mut step = *d;
        normal_vector(&mut step)?;
        let step = [mul(r, step[0])?, mul(r, step[1])?, mul(r, step[2])?];
        at = [add(at[0], step[0])?, add(at[1], step[1])?, add(at[2], step[2])?];
        let (c, _) = world.floor_collide(r, 0.0, bottom, at, true, 1)?;
        if gate && s.floor_object_89c != c.solid.map(|h| h.object) {
            *d = [0.0; 3];
            return Ok(-1);
        }
        match c.solid {
            Some(ahead) => {
                let flat = world.objects[ahead.object].flags & 8 != 0
                    && sub(ahead.position[1], contact_y)?.abs() < 4.0;
                if flat {
                    off = 0.0;
                } else {
                    e = [sub(at[0], ahead.position[0])?, sub(at[1], ahead.position[1])?, sub(at[2], ahead.position[2])?];
                    along = add(add(mul(d[0], e[0])?, mul(d[1], e[1])?)?, mul(d[2], e[2])?)?;
                    off = fqdist(e[0], e[2])?.0;
                }
            }
            None => {
                e = step;
                along = 0.0;
                off = r;
            }
        }
    }
    if off < w(0x3a83126f) || along < 0.0 {
        s.floor_y_88c = contact_y;
        return Ok(if off >= w(0x3a83126f) && along >= w(0xbe800000) { 2 } else { 1 });
    }
    d[0] = sub(d[0], e[0])?;
    d[2] = sub(d[2], e[2])?;
    requery(world, d, s)
}

/// WorldVector 0x1d5948: row vector × matrix rows 0..2, scalar EE arithmetic.
pub fn world_vector(v: V3, m: &[V3; 3]) -> R<V3> {
    let col = |c: usize| -> R<f32> { Ok(add(add(mul(v[0], m[0][c])?, mul(v[1], m[1][c])?)?, mul(v[2], m[2][c])?)?) };
    Ok([col(0)?, col(1)?, col(2)?])
}

/// UpdateObjColOffset 0x103208, run by do_players before PlayerMotion:
/// the collision position +0x70 from position, offset and flags +0x84.
pub fn collision_position(position: V3, offset: V3, flags_84: u32, m: &[V3; 3]) -> R<V3> {
    if flags_84 & 1 != 0 {
        return Ok(position);
    }
    let o = if flags_84 & 2 != 0 { offset } else { world_vector(offset, m)? };
    Ok([add(position[0], o[0])?, add(position[1], o[1])?, add(position[2], o[2])?])
}

/// The static displacement composition described in the module header.
pub fn player_displacement(
    world: &mut CollisionWorld,
    input: DisplacementInput,
    state: &mut PlayerFloorState,
) -> R<DisplacementOutput> {
    let mut d = input.request;
    let r = input.radius_828;
    let mut pos = collision_position(input.position, input.collision_offset_81c, input.offset_flags_84, &input.matrix)?;
    pos[1] = add(pos[1], 1.0)?;
    let (wall_result, wall) = collide_walls(world, r, input.current_action, pos, &mut d)?;
    state.wall_object_8a0 = wall.as_ref().map(|c| c.object);
    pos[1] = sub(pos[1], 1.0)?;
    let floor_result = collide_floor(
        world,
        r,
        input.height_82c,
        state,
        pos,
        &mut d,
        wall_result,
        wall.as_ref(),
        input.world_bottom,
    )?;
    let adjust = if floor_result > 0 {
        true
    } else if floor_result == 0 {
        match state.floor_object_89c {
            None => true,
            Some(o) => world.objects[o].flags & 0x1000 == 0,
        }
    } else if floor_result == -2 {
        state.floor_object_89c.is_none() || state.flags_8ac & 0x8000 != 0
    } else {
        false
    };
    if adjust {
        let mut dy = sub(state.floor_y_88c, state.y_54)?;
        let floor = mul(input.spf, -16.0)?;
        if dy < floor {
            dy = floor;
        }
        d[1] = add(dy, d[1])?;
    }
    if input.flags_8b0 & 0x8000 == 0 {
        // CameraLimitPlayerDpos is part of the unrecovered camera system.
    }
    if mul(input.speed, w(0x3dcccccd))? < d[1] {
        let length = fqdist(input.speed, d[1])?.0;
        if w(0x3c23d70a) < length {
            let scale = div_rn(input.speed, length)?;
            d[0] = mul(scale, d[0])?;
            d[2] = mul(scale, d[2])?;
        }
    }
    let position = [add(input.position[0], d[0])?, add(input.position[1], d[1])?, add(input.position[2], d[2])?];
    Ok(DisplacementOutput { displacement: d, position, wall_result, floor_result })
}
