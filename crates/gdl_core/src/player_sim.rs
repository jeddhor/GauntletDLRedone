//! Closed-loop single-player update for the labelled interim playtest.
//!
//! Verified components: control tables (InitJoyAng), motion code and run timer,
//! displacement request and facing turn, static wall/floor collision and the
//! position add (each byte-compared with reference updates). Interim pieces,
//! NOT fidelity evidence: requested-action choice uses observed code→action
//! pairs (1→0, 8→17, 13→19, 15→92) instead of the untranscribed PlayerMotion
//! branch; the player matrix is rebuilt as rotation(facing)×scale instead of
//! CreateYPRMatrix; floor-object tracking (+0x89c) follows the last floor hit.
use crate::collision::CollisionWorld;
use crate::controls::JoyTables;
use crate::libm::{cosf, sinf};
use crate::numeric::mul_rz as mul;
use crate::player_collision::{player_displacement, DisplacementInput, PlayerFloorState};
use crate::player_input::{motion_code, MotionCodeInput};
use crate::player_motion::{facing_turn, motion_request, FacingInput, MotionRequestInput};
use serde::{Deserialize, Serialize};
use std::error::Error;

type V3 = [f32; 3];

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerStats {
    pub speed_124: f32,
    pub scale_964: f32,
    pub scale_96c: f32,
    pub radius_828: f32,
    pub height_82c: f32,
    pub turn_968: f32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlayerSim {
    pub position: V3,
    pub matrix: [V3; 3],
    pub matrix_scale: f32,
    pub facing: f32,
    pub collision_offset_81c: V3,
    pub offset_flags_84: u32,
    pub floor_y_88c: f32,
    pub floor_object_89c: Option<usize>,
    pub flags_8ac: u32,
    pub word_100: i32,
    pub walking_timer: f32,
    pub motion_code: u32,
    pub current_action: u32,
    pub requested_action: u32,
    pub stats: PlayerStats,
    pub status_93c: u16,
    pub state_934: i16,
    pub flags_138: u32,
    pub word_80c: u32,
    pub flags_8b0: u32,
    pub attack_state_8c8: i32,
    pub camera_90: f32,
    pub camera_98: f32,
    pub spf: f32,
    pub world_bottom: f32,
    pub boss_type: i32,
    pub quest_item_state: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct StepReport {
    pub motion_code: u32,
    pub requested_action: u32,
    pub floor_result: i32,
    pub wall_result: i32,
}

impl PlayerSim {
    /// One game update (two vblanks at the fixture's SPF of 1/30).
    pub fn step(
        &mut self,
        world: &mut CollisionWorld,
        tables: &JoyTables,
        levels: u32,
        edges: u32,
    ) -> Result<StepReport, Box<dyn Error>> {
        let (angle, magnitude) = tables.digital(levels);
        let code = motion_code(MotionCodeInput {
            control_word_04: levels,
            control_word_08: edges,
            magnitude_20: magnitude,
            secondary_magnitude_28: 0.0,
            status_93c: self.status_93c,
            state_934: self.state_934,
            flags_138: self.flags_138,
            word_80c: self.word_80c,
            current_action: self.current_action,
            boss_type: self.boss_type,
            walking_timer: self.walking_timer,
            spf: self.spf,
        })?;
        let request = motion_request(MotionRequestInput {
            camera_90: self.camera_90,
            camera_98: self.camera_98,
            control_angle_1c: angle,
            control_magnitude_20: magnitude,
            control_secondary_28: 0.0,
            base_code: code.base_code,
            current_action_1e0: self.current_action,
            status_93c: self.status_93c,
            facing_86c: self.facing,
            speed_124: self.stats.speed_124,
            scale_964: self.stats.scale_964,
            scale_96c: self.stats.scale_96c,
            velocity_830: 0.0,
            velocity_838: 0.0,
            knockback_848: [0.0; 3],
            flags_8b0: self.flags_8b0,
            attack_state_8c8: self.attack_state_8c8,
            position_y_54: self.position[1],
            matrix_44: self.matrix[2][1],
            quest_item_state: self.quest_item_state,
            boss_type: self.boss_type,
            spf: self.spf,
            world_bottom: self.world_bottom,
        })?;
        let mut floor = PlayerFloorState {
            floor_y_88c: self.floor_y_88c,
            floor_object_89c: self.floor_object_89c,
            flags_8ac: self.flags_8ac,
            word_100: self.word_100,
            y_54: self.position[1],
            wall_object_8a0: None,
            floor_contact: None,
            floor_hit: None,
            water: None,
        };
        let moved = player_displacement(
            world,
            DisplacementInput {
                matrix: self.matrix,
                collision_offset_81c: self.collision_offset_81c,
                offset_flags_84: self.offset_flags_84,
                position: self.position,
                request: request.request,
                speed: request.speed,
                radius_828: self.stats.radius_828,
                height_82c: self.stats.height_82c,
                flags_8b0: self.flags_8b0,
                current_action: self.current_action,
                spf: self.spf,
                world_bottom: self.world_bottom,
            },
            &mut floor,
        )?;
        self.facing = facing_turn(FacingInput {
            forward_40: self.matrix[2][0],
            forward_48: self.matrix[2][2],
            target: request.direction,
            turn_968: self.stats.turn_968,
            spf: self.spf,
        })?;
        self.position = moved.position;
        self.floor_y_88c = floor.floor_y_88c;
        self.flags_8ac = floor.flags_8ac;
        if moved.floor_result > 0 {
            // Interim: the last floor hit is treated as the current floor object.
            self.floor_object_89c = floor.floor_contact.or(self.floor_object_89c);
        }
        self.walking_timer = code.walking_timer;
        self.motion_code = code.code;
        self.requested_action = match code.base_code {
            1 => 0,
            8 => 17,
            13 => 19,
            15 => 92,
            other => return Err(format!("interim action map has no observed pair for code {other}").into()),
        };
        let (s, c) = (sinf(self.facing)?, cosf(self.facing)?);
        let k = self.matrix_scale;
        self.matrix = [[mul(c, k)?, 0.0, mul(-s, k)?], [0.0, k, 0.0], [mul(s, k)?, 0.0, mul(c, k)?]];
        Ok(StepReport {
            motion_code: code.code,
            requested_action: self.requested_action,
            floor_result: moved.floor_result,
            wall_result: moved.wall_result,
        })
    }
}
