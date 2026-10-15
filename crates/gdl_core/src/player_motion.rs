//! Ordinary PlayerMotion displacement request and facing turn subset.
//! EE 0x148178..0x148880 computes the pre-collision request stored at player
//! +0x860..+0x868; EE 0x14c288..0x14c340 turns facing +0x86c. Collision, pushes,
//! camera limits and the final displacement added at EE 0x14c14c are excluded:
//! the request is not a position result.
use crate::libm::{atan2f, cosf, sinf};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub};
use serde::{Deserialize, Serialize};

const PI: u32 = 0x4049_0fdb;
const NEG_PI: u32 = 0xc049_0fdb;
const TWO_PI: u32 = 0x40c9_0fdb;

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MotionRequestInput {
    /// MboxWinPtr +0x90/+0x98, read before game_main's camera update.
    pub camera_90: f32,
    pub camera_98: f32,
    pub control_angle_1c: f32,
    pub control_magnitude_20: f32,
    pub control_secondary_28: f32,
    /// GetPlayerDMotion return value, before the run-delay downgrade.
    pub base_code: u32,
    pub current_action_1e0: u32,
    pub status_93c: u16,
    pub facing_86c: f32,
    pub speed_124: f32,
    pub scale_964: f32,
    pub scale_96c: f32,
    pub velocity_830: f32,
    pub velocity_838: f32,
    pub knockback_848: [f32; 3],
    pub flags_8b0: u32,
    pub attack_state_8c8: i32,
    pub position_y_54: f32,
    pub matrix_44: f32,
    pub quest_item_state: u32,
    pub boss_type: i32,
    pub spf: f32,
    pub world_bottom: f32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct MotionRequestOutput {
    pub camera_yaw: f32,
    /// f21 for sinf/cosf, and f23, the facing target, on this accepted path.
    pub direction: f32,
    pub request: [f32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedMotionRequest;
impl std::fmt::Display for UnsupportedMotionRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unsupported PlayerMotion request/facing branch")
    }
}
impl std::error::Error for UnsupportedMotionRequest {}
impl From<crate::numeric::UnsupportedNumericDomain> for UnsupportedMotionRequest {
    fn from(_: crate::numeric::UnsupportedNumericDomain) -> Self {
        Self
    }
}

fn wrap(v: f32) -> Result<f32, UnsupportedMotionRequest> {
    Ok(if f32::from_bits(PI) < v {
        sub(v, f32::from_bits(TWO_PI))?
    } else if v <= f32::from_bits(NEG_PI) {
        add(v, f32::from_bits(TWO_PI))?
    } else {
        v
    })
}

fn clamp(v: f32, limit: f32) -> f32 {
    if v < -limit {
        -limit
    } else if limit < v {
        limit
    } else {
        v
    }
}

pub fn motion_request(i: MotionRequestInput) -> Result<MotionRequestOutput, UnsupportedMotionRequest> {
    // Rejected: special actions 0x8f/8/0x89/0x23/0x25 and 0x3e..0x4e, the boss
    // quest-item path, secondary control angles, angle-preserving codes and code
    // 0x1d, nonzero velocity/knockback, clamp flags 0x18160, attack states >= 11,
    // status bit 0x8 (a later f23 redefinition at EE 0x14a794), world-bottom
    // respawn and NaN matrices.
    if matches!(i.current_action_1e0, 0x8f | 8 | 0x89 | 0x23 | 0x25 | 0x3e..=0x4e)
        || i.quest_item_state == 2
        || i.boss_type >= 0
        || i.control_secondary_28 != 0.0
        || !matches!(i.base_code, 1 | 8 | 13 | 15 | 16)
        || i.velocity_830 != 0.0
        || i.velocity_838 != 0.0
        || i.knockback_848.iter().any(|v| *v != 0.0)
        || i.flags_8b0 & 0x1_8160 != 0
        || i.attack_state_8c8 >= 11
        || i.status_93c & 0x8 != 0
        || i.position_y_54 <= i.world_bottom
        || i.matrix_44.is_nan()
    {
        return Err(UnsupportedMotionRequest);
    }
    let camera_yaw = add(atan2f(i.camera_90, i.camera_98)?, f32::from_bits(PI))?;
    let scale = mul(i.control_magnitude_20, i.scale_96c)?;
    let direction = if 0.0 < scale {
        wrap(add(i.control_angle_1c, camera_yaw)?)?
    } else {
        i.facing_86c
    };
    let base = i.knockback_848.map(|v| mul(v, i.spf));
    let (base_x, base_y, base_z) = (base[0]?, base[1]?, base[2]?);
    let base_x = add(mul(i.velocity_830, i.spf)?, base_x)?;
    let base_z = add(mul(i.velocity_838, i.spf)?, base_z)?;
    let limit = mul(mul(i.speed_124, i.spf)?, 1.5)?;
    let y = if base_y < 0.0 {
        0.0
    } else if limit < base_y {
        limit
    } else {
        base_y
    };
    let speed = mul(mul(mul(scale, i.speed_124)?, i.spf)?, i.scale_964)?;
    let x = add(mul(speed, sinf(direction)?)?, base_x)?;
    let z = add(mul(speed, cosf(direction)?)?, base_z)?;
    Ok(MotionRequestOutput {
        camera_yaw,
        direction,
        request: [clamp(x, limit), y, clamp(z, limit)],
    })
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FacingInput {
    /// Player matrix +0x40/+0x48 before the update (f28 = atan2f of these).
    pub forward_40: f32,
    pub forward_48: f32,
    pub target: f32,
    pub turn_968: f32,
    pub spf: f32,
}

/// Facing turn at EE 0x14c288..0x14c33c for the ordinary path where the
/// collision-response facing branch at EE 0x14c1bc is not taken.
pub fn facing_turn(i: FacingInput) -> Result<f32, UnsupportedMotionRequest> {
    let current = atan2f(i.forward_40, i.forward_48)?;
    let rate = mul(mul(i.spf, f32::from_bits(0x417b_53d1))?, i.turn_968)?;
    let difference = wrap(sub(i.target, current)?)?;
    Ok(if rate < difference {
        add(current, rate)?
    } else if difference < -rate {
        sub(current, rate)?
    } else {
        i.target
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> MotionRequestInput {
        MotionRequestInput {
            camera_90: -0.5,
            camera_98: -0.5,
            control_angle_1c: 0.0,
            control_magnitude_20: 1.0,
            control_secondary_28: 0.0,
            base_code: 13,
            current_action_1e0: 17,
            status_93c: 0,
            facing_86c: 1.0,
            speed_124: 8.0,
            scale_964: 1.0,
            scale_96c: 1.0,
            velocity_830: 0.0,
            velocity_838: 0.0,
            knockback_848: [0.0; 3],
            flags_8b0: 0,
            attack_state_8c8: 0,
            position_y_54: 0.0,
            matrix_44: 0.0,
            quest_item_state: 0,
            boss_type: -1,
            spf: f32::from_bits(0x3d08_8889),
            world_bottom: -4.6,
        }
    }
    #[test]
    fn request_uses_magnitude_or_keeps_facing_and_rejects_named_branches() {
        let moving = motion_request(input()).unwrap();
        assert!((moving.direction - std::f32::consts::FRAC_PI_4).abs() < 1e-6);
        assert!(moving.request[0] > 0.18 && moving.request[1] == 0.0);
        let mut idle = input();
        idle.control_magnitude_20 = 0.0;
        let idle = motion_request(idle).unwrap();
        assert_eq!((idle.direction, idle.request), (1.0, [0.0; 3]));
        for change in [
            |x: &mut MotionRequestInput| x.current_action_1e0 = 8,
            |x: &mut MotionRequestInput| x.base_code = 0x1d,
            |x: &mut MotionRequestInput| x.knockback_848[1] = 1.0,
            |x: &mut MotionRequestInput| x.status_93c = 8,
            |x: &mut MotionRequestInput| x.position_y_54 = -5.0,
        ] {
            let mut x = input();
            change(&mut x);
            assert!(motion_request(x).is_err());
        }
    }
    #[test]
    fn request_axes_are_clamped_to_one_and_a_half_speed_steps() {
        let mut x = input();
        x.scale_964 = 4.0;
        let limit = mul(mul(8.0, x.spf).unwrap(), 1.5).unwrap();
        let out = motion_request(x).unwrap();
        assert_eq!((out.request[0], out.request[2]), (limit, limit));
    }
    #[test]
    fn facing_turn_is_rate_limited_in_wrapped_direction() {
        let spf = f32::from_bits(0x3d08_8889);
        let turn = |target| facing_turn(FacingInput { forward_40: 0.0, forward_48: 1.0, target, turn_968: 1.0, spf }).unwrap();
        let rate = mul(mul(spf, f32::from_bits(0x417b_53d1)).unwrap(), 1.0).unwrap();
        assert_eq!(turn(0.25), 0.25);
        assert_eq!(turn(2.0), rate);
        assert_eq!(turn(-2.0), -rate);
        assert_eq!(turn(3.0), rate);
    }
}
