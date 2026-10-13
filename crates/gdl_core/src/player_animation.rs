//! Bounded DoPlayerAction animation-input branches for the first reference slice.
//! Pad/action production, combat events and post-selection gameplay writes remain
//! separate. Unsupported actions/conditions fail instead of choosing a substitute.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActionContext {
    pub current_action: u32,
    pub requested_action: u32,
    pub frame: f32,
    pub flags_130: u32,
    pub flags_134: u32,
    pub flags_138: u32,
    pub flags_8ac: u32,
    pub status_93c: u16,
    pub idle_counter_1d8: i16,
    pub idle_counter_1da: i16,
    pub word_80c: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ActionSelection {
    pub action: u32,
    pub mode: i32,
    pub start_frame: i16,
    pub blend_duration: f32,
    /// DoPlayerAction writes this before the first CalcAnimInfo call. It differs
    /// from InitAnim's sequence-header repeat word on an actual initialization.
    pub loop_word_before_selection: u16,
    pub playback_scale: f32,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActionEventInput {
    pub event_word: u32,
    pub throw_start_time: f32,
    pub now: f32,
    pub class_word: u32,
    pub word_720: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct MissileRequest {
    pub flags: u32,
    pub charge: f32,
    pub power_scale: f32,
    pub origin_mode: u32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ActionEventOutput {
    pub event_word: u32,
    pub throw_start_time: f32,
    /// Source call request only. Native projectile creation, GetAttackDir,
    /// collision, damage and PlayerStartMissile's success result are excluded.
    pub missile_request: Option<MissileRequest>,
}

/// Ordinary Sorceress event-word subset of DoPlayerAction -> PlayerMotion.
/// Input state is sampled before the update; output is after the inspected
/// consumption path. Special powers/weapon effects fail rather than disappearing.
pub fn consume_events(
    context: ActionContext,
    chosen: ActionSelection,
    library_return_flags: u32,
    input: ActionEventInput,
) -> Result<ActionEventOutput, UnsupportedActionBranch> {
    if input.class_word != 6
        || input.word_720 != 0
        || context.word_80c >= 4
        || context.flags_138 & 0x480 != 0
        || input.event_word & !0x1000_0000 != 0
        || !input.now.is_finite()
        || !input.throw_start_time.is_finite()
        || !matches!(context.current_action, 0 | 17..=20 | 91..=92 | 95..=98)
        || !matches!(chosen.action, 0 | 17..=20 | 91..=92 | 95..=98)
    {
        return Err(UnsupportedActionBranch);
    }
    let mut word = input.event_word;
    let mut throw_start_time = input.throw_start_time;
    if library_return_flags != 0 {
        if matches!(context.current_action, 95..=96) {
            word |= 0x100; // Original release-exit branch EE0x178d5c.
        }
        if matches!(chosen.action, 91..=92) {
            word |= 1; // Original start-entry branch EE0x178e6c.
        }
    }
    // Previous launch visual bit is consumed before this update's new request.
    // The ordinary no-weapon/no-power bypass reaches EE0x14ba40 directly.
    word &= !0x1000_0000;
    if word & 1 != 0 {
        throw_start_time = input.now; // PlayerMotion EE0x14bdc0/0x14bdc4.
    }
    word &= !0xff;
    let missile_request = if word & 0xff00 != 0 {
        let sub = crate::numeric::sub_guarded_rz;
        let charge = sub(input.now, throw_start_time)
            .and_then(|elapsed| sub(elapsed, f32::from_bits(0x3e8a_3d71)))
            .map_err(|_| UnsupportedActionBranch)?;
        let charge = charge.clamp(0.0, f32::from_bits(0x3dcc_cccd));
        // The call's result is ignored here by the source caller. This marker
        // proves a request, never that a projectile or hit was produced.
        word = (word & !0xff00) | 0x1000_0000;
        Some(MissileRequest {
            flags: context.flags_130,
            charge,
            power_scale: 1.0,
            origin_mode: 1,
        })
    } else {
        None
    };
    Ok(ActionEventOutput {
        event_word: word,
        throw_start_time,
        missile_request,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedActionBranch;
impl std::fmt::Display for UnsupportedActionBranch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unsupported DoPlayerAction branch or numeric domain")
    }
}
impl std::error::Error for UnsupportedActionBranch {}

/// PlayerAttackType's raw category, not an attack-event or damage predicate.
/// Original EE 0x177b50, using action IDs independently from sequence IDs.
pub fn attack_type(action: u32) -> u32 {
    match action {
        0x20..=0x26 => 2,
        0x27..=0x2b => 3,
        0x2c..=0x3b => 4,
        0x3c..=0x3d => 5,
        0x3e..=0x46 => 6,
        0x47..=0x4e => 7,
        0x4f..=0x55 => 8,
        0x56..=0x57 | 0x6b..=0x71 => 11,
        0x58..=0x5a | 0x88..=0x93 => 12,
        0x5b..=0x62 => 9,
        0x63..=0x6a => 10,
        4..=8 | 0x77..=0x79 => 1,
        _ => 0,
    }
}

/// Recovered ordinary READY, WALK1/2, RUN1/2 and throw start/release/recovery
/// branches. Jump-table targets are source-qualified in the research dossier.
pub fn choose(input: ActionContext) -> Result<ActionSelection, UnsupportedActionBranch> {
    if !matches!(input.current_action, 0 | 17..=20 | 91..=92 | 95..=98)
        || !matches!(input.requested_action, 0 | 17..=20 | 91..=92)
        || !input.frame.is_finite()
        || input.frame < 0.0
    {
        return Err(UnsupportedActionBranch);
    }
    let requested_type = attack_type(input.requested_action);
    let mut action = input.requested_action;
    let mut mode = 0;
    let mut loop_word = 0;
    match input.current_action {
        0 => {
            loop_word = 1;
            mode = 2;
            // AuxLevels changes idle thresholds (180/1800 and60/600). This
            // subset accepts counters below both thresholds, or the bypass.
            if action == 0
                && input.word_80c == 0
                && (input.idle_counter_1d8 > 180 || input.idle_counter_1da > 60)
            {
                return Err(UnsupportedActionBranch);
            }
        }
        17 | 18 => {
            if action >= 27 || requested_type != 0 {
                mode = 2;
            }
            if action == 17 {
                action = if input.current_action == 17 { 18 } else { 17 };
            }
        }
        19 | 20 => {
            if (input.current_action == 19 && action >= 27)
                || (input.current_action == 20 && action == 27)
                || requested_type != 0
            {
                mode = 2;
            }
            if action == 19 {
                action = if input.current_action == 19 { 20 } else { 19 };
            }
        }
        91 | 92 => {
            // Both captured start actions enter EE0x17849c with a0=0, then
            // increment to1. The sibling branch at0x178498 is outside this set.
            if input.frame >= 2.0 {
                mode = 2;
            }
            action = 95;
        }
        95 | 96 => {
            action = if input.current_action == 95 { 97 } else { 98 };
        }
        97 | 98 => {}
        _ => unreachable!(),
    }
    let mut blend_duration = 0.0;
    if action == 0 {
        // A powered ready variant takes action21; qualify it separately.
        if input.flags_134 & 0x0062_0000 != 0 {
            return Err(UnsupportedActionBranch);
        }
        if input.current_action != 0 && !(0x56..=0x93).contains(&input.current_action) {
            blend_duration = f32::from_bits(0x3d88_88b5);
        }
    }
    let playback_scale = if input.status_93c & 0xd0 != 0 {
        1.0
    } else if (requested_type == 9 && input.flags_130 & 0x2000_0000 != 0)
        || input.flags_138 & 0x10000 != 0
    {
        0.75
    } else {
        1.0
    };
    // In this accepted action set, the flags_8ac fast branch's selected>=130
    // condition is always false; raw flags are retained in input provenance.
    Ok(ActionSelection {
        action,
        mode,
        start_frame: 0,
        blend_duration,
        loop_word_before_selection: loop_word,
        playback_scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(current_action: u32, requested_action: u32, frame: f32) -> ActionContext {
        ActionContext {
            current_action,
            requested_action,
            frame,
            flags_130: 0,
            flags_134: 0,
            flags_138: 0,
            flags_8ac: 0,
            status_93c: 0,
            idle_counter_1d8: 0,
            idle_counter_1da: 0,
            word_80c: 0,
        }
    }
    #[test]
    fn walking_waits_but_throw_start_interrupts_at_frame_two() {
        let walk = choose(context(17, 19, 3.0)).unwrap();
        assert_eq!((walk.action, walk.mode), (19, 0));
        let run = choose(context(19, 0, 1.0)).unwrap();
        assert_eq!((run.action, run.mode), (0, 0));
        assert_eq!(run.blend_duration.to_bits(), 0x3d88_88b5);
        assert_eq!(choose(context(92, 92, 1.0)).unwrap().mode, 0);
        assert_eq!(choose(context(92, 92, 2.0)).unwrap().mode, 2);
        assert_eq!(choose(context(92, 92, 2.0)).unwrap().action, 95);
        assert_eq!(choose(context(95, 92, 2.0)).unwrap().action, 97);
    }
    #[test]
    fn unsupported_actions_and_ready_variants_are_explicit() {
        assert!(choose(context(21, 0, 0.0)).is_err());
        let mut input = context(0, 0, 0.0);
        input.idle_counter_1da = 61;
        assert!(choose(input).is_err());
        input.word_80c = 1;
        assert!(choose(input).is_ok());
        input.flags_134 = 0x20000;
        assert!(choose(input).is_err());
        assert!(choose(context(17, 19, f32::NAN)).is_err());
    }
    #[test]
    fn walk_and_run_alternation_only_rewrites_the_primary_requested_id() {
        for (old, requested, expected) in [
            (17, 17, 18),
            (18, 17, 17),
            (17, 18, 18),
            (18, 18, 18),
            (19, 19, 20),
            (20, 19, 19),
            (19, 20, 20),
            (20, 20, 20),
        ] {
            assert_eq!(
                choose(context(old, requested, 0.0)).unwrap().action,
                expected
            );
        }
    }
    #[test]
    fn raw_attack_categories_and_scale_precedence_remain_separate() {
        for (action, category) in [
            (0, 0),
            (4, 1),
            (8, 1),
            (9, 0),
            (31, 0),
            (32, 2),
            (38, 2),
            (39, 3),
            (91, 9),
            (98, 9),
            (99, 10),
            (106, 10),
            (107, 11),
            (113, 11),
            (114, 0),
            (119, 1),
            (121, 1),
            (122, 0),
            (136, 12),
            (147, 12),
            (148, 0),
            (u32::MAX, 0),
        ] {
            assert_eq!(attack_type(action), category);
        }
        let mut input = context(0, 92, 0.0);
        input.flags_130 = 0x20000000;
        assert_eq!(choose(input).unwrap().playback_scale, 0.75);
        input.status_93c = 0x10;
        assert_eq!(choose(input).unwrap().playback_scale, 1.0);
    }
    #[test]
    fn throw_event_request_is_on_release_exit_and_prior_marker_is_consumed() {
        let input = ActionEventInput {
            event_word: 0,
            throw_start_time: 10.0,
            now: 11.0,
            class_word: 6,
            word_720: 0,
        };
        let start_context = context(0, 92, 0.0);
        let start =
            consume_events(start_context, choose(start_context).unwrap(), 1, input).unwrap();
        assert_eq!(start.throw_start_time, 11.0);
        assert!(start.missile_request.is_none());
        let release_context = context(95, 92, 2.0);
        let request =
            consume_events(release_context, choose(release_context).unwrap(), 3, input).unwrap();
        assert_eq!(request.event_word, 0x10000000);
        assert_eq!(
            request.missile_request.unwrap().charge.to_bits(),
            0x3dcccccd
        );
        let recovery_context = context(97, 92, 0.0);
        let consumed = consume_events(
            recovery_context,
            choose(recovery_context).unwrap(),
            0,
            ActionEventInput {
                event_word: request.event_word,
                ..input
            },
        )
        .unwrap();
        assert_eq!(consumed.event_word, 0);
        assert!(consumed.missile_request.is_none());
        assert!(
            consume_events(
                start_context,
                choose(start_context).unwrap(),
                1,
                ActionEventInput {
                    word_720: 1,
                    ..input
                }
            )
            .is_err()
        );
    }
}
