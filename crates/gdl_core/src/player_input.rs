//! Ordinary logical motion-code subset of GetPlayerDMotion/PlayerMotion.
//! These codes are action intent, not displacement. Pad polling, angle-dependent
//! attacks, collision, forced modes and requested-action mapping are separate.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MotionCodeInput {
    pub control_word_04: u32,
    pub control_word_08: u32,
    pub magnitude_20: f32,
    pub secondary_magnitude_28: f32,
    pub status_93c: u16,
    pub state_934: i16,
    pub flags_138: u32,
    pub word_80c: u32,
    pub current_action: u32,
    pub boss_type: i32,
    pub walking_timer: f32,
    pub spf: f32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct MotionCodeOutput {
    pub base_code: u32,
    pub code: u32,
    pub walking_timer: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedMotionCode;
impl std::fmt::Display for UnsupportedMotionCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unsupported GetPlayerDMotion/PlayerMotion branch")
    }
}
impl std::error::Error for UnsupportedMotionCode {}

/// Source ordinary idle/magnitude/primary-button classification, then the
/// PlayerMotion+0x874 run-delay timer branch. Angles are unused in these accepted
/// branches; no camera mapping or displacement formula is inferred.
pub fn motion_code(input: MotionCodeInput) -> Result<MotionCodeOutput, UnsupportedMotionCode> {
    if input.control_word_04 & (0x10000 | 0x8000 | 0x100 | 0x20000 | 0x4000 | 0x08000000) != 0
        || input.control_word_08 & (0x800 | 0x1000 | 0x2000) != 0
        || input.secondary_magnitude_28 != 0.0
        || !(0.0..=1.0).contains(&input.magnitude_20)
        || (input.magnitude_20 != 0.0 && !input.magnitude_20.is_normal())
        || input.status_93c & (0x40 | 0x10 | 0x80) != 0
        || input.state_934 != 0
        || input.flags_138 & 0x400 != 0
        || input.word_80c != 0
        || !matches!(input.current_action,0|17..=20|91..=92|95..=98)
        || input.boss_type >= 0
        || !input.walking_timer.is_finite()
        || input.walking_timer < 0.0
        || !input.spf.is_normal()
        || input.spf <= 0.0
    {
        return Err(UnsupportedMotionCode);
    }
    let base_code = if input.control_word_04 & 0x200 != 0 {
        15
    } else if input.control_word_04 & 0x400 != 0 {
        16
    } else if input.magnitude_20 > 0.75 {
        13
    } else if input.magnitude_20 > 0.0 {
        8
    } else {
        1
    };
    let code = if base_code == 13 && input.walking_timer < f32::from_bits(0x3dcc_cccd) {
        8
    } else {
        base_code
    };
    let walking_timer = if matches!(base_code, 8 | 13) {
        crate::numeric::add_guarded_rz(input.walking_timer, input.spf)
            .map_err(|_| UnsupportedMotionCode)?
    } else {
        0.0
    };
    Ok(MotionCodeOutput {
        base_code,
        code,
        walking_timer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input() -> MotionCodeInput {
        MotionCodeInput {
            control_word_04: 0,
            control_word_08: 0,
            magnitude_20: 0.0,
            secondary_magnitude_28: 0.0,
            status_93c: 0,
            state_934: 0,
            flags_138: 0,
            word_80c: 0,
            current_action: 0,
            boss_type: -1,
            walking_timer: 0.0,
            spf: f32::from_bits(0x3d088889),
        }
    }
    #[test]
    fn intent_cutoff_timer_boundary_and_primary_precedence_are_explicit() {
        let mut x = input();
        assert_eq!(motion_code(x).unwrap().code, 1);
        x.magnitude_20 = 0.75;
        assert_eq!(motion_code(x).unwrap().base_code, 8);
        x.magnitude_20 = 1.0;
        assert_eq!(motion_code(x).unwrap().base_code, 13);
        assert_eq!(motion_code(x).unwrap().code, 8);
        x.walking_timer = f32::from_bits(0x3dcccccd);
        assert_eq!(motion_code(x).unwrap().code, 13);
        x.control_word_04 = 0x600;
        let attack = motion_code(x).unwrap();
        assert_eq!(attack.code, 15);
        assert_eq!(attack.walking_timer, 0.0);
        x.boss_type = 0;
        assert!(motion_code(x).is_err());
        x = input();
        x.control_word_08 = 0x800;
        assert!(motion_code(x).is_err());
        x = input();
        x.secondary_magnitude_28 = 1.0;
        assert!(motion_code(x).is_err());
        x = input();
        x.magnitude_20 = f32::from_bits(1);
        assert!(motion_code(x).is_err());
    }
}
