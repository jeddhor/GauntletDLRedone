//! Original animation clock and library selection, separate from player actions and rendering.
//! Original finite arithmetic and integer state words are preserved explicitly.
use crate::numeric::{
    UnsupportedNumericDomain, add_guarded_rz as add, div_rn as div, mul_rz as mul,
    sub_guarded_rz as sub,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClockState {
    pub sequence: u16,
    pub previous_sequence: u16,
    pub reset_byte: u8,
    pub option_bits: u8,
    pub enabled_word: u16,
    pub loop_word: u16,
    pub mode_bits: u16,
    pub frame: f32,
    pub blend_fraction: f32,
    pub start_time: f32,
    pub blend_end_time: f32,
    pub period_scale: f32,
    pub last_time: f32,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct ClockAdvance {
    pub state: ClockState,
    /// Original consumer return:0 no change,1 updated,15 loop boundary. These
    /// codes are not attack/damage events or a recovered action state machine.
    pub return_code: u32,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClockInit {
    pub sequence: u16,
    pub frames: i16,
    pub timing_factor: i16,
    pub repeat: u16,
    pub start_frame: i16,
    pub enabled_word: u16,
    pub playback_scale: f32,
    pub blend_duration: f32,
    pub now: f32,
    pub anim_time_scale: f32,
}

/// Fields read from an already validated original sequence header.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClipTiming {
    pub frames: i16,
    pub timing_factor: i16,
    pub repeat: u16,
    pub flags: u16,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionInput {
    pub sequence: u16,
    pub mode: i32,
    pub start_frame: i16,
    pub blend_duration: f32,
    pub playback_scale: f32,
    pub now: f32,
    pub anim_time_scale: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct SelectionOutput {
    pub state: ClockState,
    /// AnimateTree's raw return mask. Root transfer (bit 8) is requested here,
    /// not performed. DoAnimateTree/sampling and player consumers are separate.
    pub return_flags: u32,
    pub clock_return_codes: Vec<u32>,
}

/// AnimateTree's valid-sequence selection path (EE 0x110480). The caller supplies
/// both source headers and requested inputs; this does not choose player actions.
/// AnimCompressSetup's key-cache work and subsequent tree evaluation are excluded.
pub fn select(
    state: ClockState,
    current: ClipTiming,
    requested: ClipTiming,
    input: SelectionInput,
) -> Result<SelectionOutput, UnsupportedNumericDomain> {
    let first = advance(state, input.now, current.frames, input.anim_time_scale)?;
    let mut state = first.state;
    let mut codes = vec![first.return_code];
    let old_sequence = state.sequence;
    let old_loop = state.loop_word;
    let ended = state.mode_bits & 0xff == 0xff;
    let changed = input.sequence != old_sequence;
    let initialize_requested = match input.mode {
        0 => ended && changed,
        1 => ended,
        2 => ended || changed,
        _ => true,
    };
    let mut flags = 0;
    let mut frames = current.frames;
    if initialize_requested {
        // The original pre-adjustment is overwritten by valid InitAnim's global
        // ATime-based start calculation; preserve the operation order regardless.
        let earliest = sub(state.last_time, f32::from_bits(0x3d08_8889))?;
        if !ended || state.start_time < earliest {
            state.start_time = earliest;
        }
        state = initialize(
            state,
            ClockInit {
                sequence: input.sequence,
                frames: requested.frames,
                timing_factor: requested.timing_factor,
                repeat: requested.repeat,
                start_frame: input.start_frame,
                enabled_word: 1,
                playback_scale: input.playback_scale,
                blend_duration: input.blend_duration,
                now: input.now,
                anim_time_scale: input.anim_time_scale,
            },
        )?;
        frames = requested.frames;
        flags = 1 | if current.flags & 1 != 0 { 8 } else { 0 };
    }
    if initialize_requested || first.return_code == 15 {
        let next = advance(state, input.now, frames, input.anim_time_scale)?;
        state = next.state;
        codes.push(next.return_code);
        if next.return_code == 15 {
            let again = advance(state, input.now, frames, input.anim_time_scale)?;
            state = again.state;
            codes.push(again.return_code);
        }
    }
    if flags != 0 && (ended || state.mode_bits & 0xff == 0xff) {
        flags |= 2;
    }
    if ended && (old_loop != 0 || !changed) {
        flags |= 4;
    }
    Ok(SelectionOutput {
        state,
        return_flags: flags,
        clock_return_codes: codes,
    })
}

/// InitAnim's valid-sequence branch. Asset callers must validate the requested
/// sequence ID. Invalid-sequence logging/fallback is not silently invented here.
pub fn initialize(
    mut state: ClockState,
    input: ClockInit,
) -> Result<ClockState, UnsupportedNumericDomain> {
    if input.frames < 0
        || !input.now.is_finite()
        || !input.blend_duration.is_finite()
        || input.blend_duration < 0.0
    {
        return Err(UnsupportedNumericDomain);
    }
    let period_scale =
        crate::animation::frame_period(input.timing_factor, input.playback_scale, 1.0)?;
    let period = mul(period_scale, input.anim_time_scale)?;
    if period <= 0.0 {
        return Err(UnsupportedNumericDomain);
    }
    let frame = if input.start_frame > input.frames {
        0.0
    } else {
        f32::from(input.start_frame)
    };
    state.last_time = input.now;
    state.enabled_word = input.enabled_word;
    state.period_scale = period_scale;
    state.frame = frame;
    state.start_time = sub(input.now, mul(period, frame)?)?;
    if input.blend_duration != 0.0 {
        state.start_time = sub(state.start_time, f32::from_bits(0x3d08_8889))?;
        state.blend_end_time = sub(
            add(input.blend_duration, input.now)?,
            f32::from_bits(0x3d08_8889),
        )?;
    } else {
        state.blend_end_time = 0.0;
    }
    state.reset_byte = 1;
    state.blend_fraction = 0.0;
    state.previous_sequence = state.sequence;
    state.sequence = input.sequence;
    state.mode_bits &= 0xf000;
    state.loop_word = input.repeat;
    Ok(state)
}

pub fn advance(
    mut state: ClockState,
    now: f32,
    frames: i16,
    anim_time_scale: f32,
) -> Result<ClockAdvance, UnsupportedNumericDomain> {
    if frames < 0
        || !now.is_finite()
        || !anim_time_scale.is_finite()
        || [
            state.frame,
            state.blend_fraction,
            state.start_time,
            state.blend_end_time,
            state.period_scale,
            state.last_time,
        ]
        .iter()
        .any(|v| !v.is_finite())
    {
        return Err(UnsupportedNumericDomain);
    }
    state.last_time = now;
    if state.reset_byte == 2 {
        state.reset_byte = 0;
    }
    if state.mode_bits & 0xff == 0xff {
        if state.mode_bits & 0x100 == 0 {
            return Ok(ClockAdvance {
                state,
                return_code: 0,
            });
        }
        state.mode_bits &= 0xff00;
    }
    if state.enabled_word == 0 {
        return Ok(ClockAdvance {
            state,
            return_code: 0,
        });
    }
    if frames == 0 {
        state.enabled_word = 0;
        state.mode_bits |= 0xff;
        state.start_time = now;
        return Ok(ClockAdvance {
            state,
            return_code: 0,
        });
    }
    let period = mul(state.period_scale, anim_time_scale)?;
    if period <= 0.0 {
        return Err(UnsupportedNumericDomain);
    }
    let reciprocal = div(1.0, period)?;
    if now < state.blend_end_time {
        state.blend_fraction = div(
            sub(now, state.start_time)?,
            sub(state.blend_end_time, state.start_time)?,
        )?;
        return Ok(ClockAdvance {
            state,
            return_code: 1,
        });
    }
    if state.previous_sequence != state.sequence {
        state.start_time = sub(now, mul(period, state.frame)?)?;
        state.blend_end_time = 0.0;
        state.blend_fraction = 0.0;
        state.previous_sequence = state.sequence;
        state.reset_byte = 1;
    }
    let mut frame = mul(sub(now, state.start_time)?, reciprocal)?;
    let nearest = add(frame, 0.5)?.floor();
    if state.option_bits & 2 == 0 || sub(frame, nearest)?.abs() < 0.125 || period < anim_time_scale
    {
        frame = nearest;
    }
    let last = f32::from(frames - 1);
    if frame >= add(last, 0.5)? {
        state.mode_bits |= 0xff;
        state.frame = last;
        if state.loop_word != 0 {
            state.start_time = now;
            state.mode_bits |= 0x100;
            state.reset_byte = 1;
            return Ok(ClockAdvance {
                state,
                return_code: 15,
            });
        }
        state.mode_bits &= 0xfeff;
        state.blend_fraction = 0.0;
        return Ok(ClockAdvance {
            state,
            return_code: 0,
        });
    }
    state.frame = frame.max(0.0);
    Ok(ClockAdvance {
        state,
        return_code: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> ClockState {
        ClockState {
            sequence: 0,
            previous_sequence: 0,
            reset_byte: 0,
            option_bits: 2,
            enabled_word: 1,
            loop_word: 0,
            mode_bits: 0,
            frame: 0.0,
            blend_fraction: 0.0,
            start_time: 0.0,
            blend_end_time: 0.0,
            period_scale: 1.0,
            last_time: 0.0,
        }
    }
    fn clip() -> ClipTiming {
        ClipTiming {
            frames: 4,
            timing_factor: 30,
            repeat: 0,
            flags: 0,
        }
    }
    fn selection(sequence: u16, mode: i32, now: f32) -> SelectionInput {
        SelectionInput {
            sequence,
            mode,
            start_frame: 0,
            blend_duration: 0.0,
            playback_scale: 1.0,
            now,
            anim_time_scale: 1.0,
        }
    }
    #[test]
    fn selection_modes_distinguish_interrupt_restart_and_wait() {
        for (mode, ended, changed, expected) in [
            (0, false, true, false),
            (0, true, false, false),
            (0, true, true, true),
            (1, false, true, false),
            (1, true, false, true),
            (1, true, true, true),
            (2, false, false, false),
            (2, false, true, true),
            (2, true, false, true),
            (-1, false, false, true),
            (3, false, false, true),
        ] {
            let output = select(
                state(),
                clip(),
                clip(),
                selection(u16::from(changed), mode, if ended { 4.0 } else { 1.0 }),
            )
            .unwrap();
            assert_eq!(
                output.return_flags & 1 != 0,
                expected,
                "mode={mode}, ended={ended}, changed={changed}"
            );
            assert_eq!(
                output.state.frame,
                if expected {
                    0.0
                } else if ended {
                    3.0
                } else {
                    1.0
                }
            );
        }
    }
    #[test]
    fn library_loop_boundary_is_evaluated_again_without_reinitializing() {
        let mut old = state();
        old.loop_word = 1;
        let current = ClipTiming {
            repeat: 1,
            ..clip()
        };
        let output = select(old, current, clip(), selection(0, 0, 4.0)).unwrap();
        assert_eq!(output.clock_return_codes, [15, 1]);
        assert_eq!(output.return_flags, 4);
        assert_eq!(output.state.mode_bits, 0x100);
        assert_eq!(output.state.frame, 0.0);
        assert_eq!(output.state.start_time, 4.0);
    }
    #[test]
    fn selection_flags_use_old_header_and_zero_frame_result() {
        let old = ClipTiming { flags: 1, ..clip() };
        let output = select(state(), old, clip(), selection(1, 2, 1.0)).unwrap();
        assert_eq!(output.return_flags, 9);
        let output = select(state(), clip(), old, selection(1, 2, 1.0)).unwrap();
        assert_eq!(output.return_flags, 1);
        let zero = ClipTiming {
            frames: 0,
            ..clip()
        };
        let output = select(state(), clip(), zero, selection(1, 2, 1.0)).unwrap();
        assert_eq!(output.return_flags, 3);
        assert_eq!(output.state.enabled_word, 0);
        let restart = select(state(), clip(), clip(), selection(0, 1, 4.0)).unwrap();
        assert_eq!(restart.return_flags, 7);
    }
    #[test]
    fn clock_snap_option_and_fast_period_have_distinct_behavior() {
        assert_eq!(advance(state(), 2.05, 8, 1.0).unwrap().state.frame, 2.0);
        assert_eq!(advance(state(), 2.4, 8, 1.0).unwrap().state.frame, 2.4);
        let mut s = state();
        s.option_bits = 0;
        assert_eq!(advance(s, 2.4, 8, 1.0).unwrap().state.frame, 2.0);
        s = state();
        s.period_scale = 0.5;
        assert_eq!(advance(s, 2.2, 8, 1.0).unwrap().state.frame, 4.0);
    }
    #[test]
    fn stop_loop_and_freeze_preserve_source_words() {
        let stopped = advance(state(), 3.75, 4, 1.0).unwrap();
        assert_eq!(stopped.return_code, 0);
        assert_eq!(stopped.state.frame, 3.0);
        assert_eq!(stopped.state.mode_bits, 0xff);
        let mut s = state();
        s.loop_word = 1;
        let looped = advance(s, 3.75, 4, 1.0).unwrap();
        assert_eq!(looped.return_code, 15);
        assert_eq!(looped.state.mode_bits, 0x1ff);
        assert_eq!(
            advance(looped.state, 3.75, 4, 1.0).unwrap().state.frame,
            0.0
        );
        let frozen = advance(stopped.state, 20.0, 4, 1.0).unwrap();
        assert_eq!(frozen.state.frame, 3.0);
        assert_eq!(frozen.state.last_time, 20.0);
        assert_eq!(frozen.return_code, 0);
    }
    #[test]
    fn zero_frame_and_sequence_change_are_explicit() {
        let zero = advance(state(), 5.0, 0, 1.0).unwrap();
        assert_eq!(zero.state.enabled_word, 0);
        assert_eq!(zero.state.mode_bits, 0xff);
        let mut s = state();
        s.sequence = 1;
        s.frame = 2.0;
        let changed = advance(s, 10.0, 8, 1.0).unwrap();
        assert_eq!(changed.state.start_time, 8.0);
        assert_eq!(changed.state.previous_sequence, 1);
        assert_eq!(changed.state.reset_byte, 1);
        assert_eq!(changed.state.frame, 2.0);
        assert!(advance(state(), f32::NAN, 4, 1.0).is_err());
        assert!(advance(state(), 1.0, -1, 1.0).is_err());
    }
    #[test]
    fn blend_clock_holds_frame_until_end_then_rebases_sequence() {
        let mut s = state();
        s.sequence = 1;
        s.frame = 2.0;
        s.start_time = 10.0;
        s.blend_end_time = 12.0;
        let halfway = advance(s, 11.0, 8, 1.0).unwrap();
        assert_eq!(halfway.state.blend_fraction, 0.5);
        assert_eq!(halfway.state.frame, 2.0);
        assert_eq!(halfway.state.previous_sequence, 0);
        let finished = advance(halfway.state, 12.0, 8, 1.0).unwrap();
        assert_eq!(finished.state.blend_end_time, 0.0);
        assert_eq!(finished.state.blend_fraction, 0.0);
        assert_eq!(finished.state.previous_sequence, 1);
        assert_eq!(finished.state.frame, 2.0);
    }
    #[test]
    fn initialization_preserves_options_and_masks_only_source_mode_bits() {
        let mut old = state();
        old.sequence = 2;
        old.mode_bits = 0xa1ff;
        old.option_bits = 3;
        let input = ClockInit {
            sequence: 3,
            frames: 8,
            timing_factor: 30,
            repeat: 1,
            start_frame: 2,
            enabled_word: 1,
            playback_scale: 1.0,
            blend_duration: 0.0,
            now: 10.0,
            anim_time_scale: 1.0,
        };
        let new = initialize(old, input).unwrap();
        assert_eq!(new.previous_sequence, 2);
        assert_eq!(new.sequence, 3);
        assert_eq!(new.start_time, 8.0);
        assert_eq!(new.period_scale, 1.0);
        assert_eq!(new.mode_bits, 0xa000);
        assert_eq!(new.option_bits, 3);
        assert_eq!(new.loop_word, 1);
        assert_eq!(new.reset_byte, 1);
        assert_eq!(
            initialize(
                old,
                ClockInit {
                    start_frame: 9,
                    ..input
                }
            )
            .unwrap()
            .frame,
            0.0
        );
        // Equality is accepted by the original '<' check and handled on update.
        assert_eq!(
            initialize(
                old,
                ClockInit {
                    start_frame: 8,
                    ..input
                }
            )
            .unwrap()
            .frame,
            8.0
        );
        assert!(
            initialize(
                old,
                ClockInit {
                    blend_duration: -1.0,
                    ..input
                }
            )
            .is_err()
        );
    }
}
