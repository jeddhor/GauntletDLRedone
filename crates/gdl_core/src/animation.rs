//! Native exact-key reconstruction. Interpolation, matrices and events are separate.
use crate::numeric::{UnsupportedNumericDomain, add_guarded_rz, mul_rz, sub_guarded_rz};
use gdl_formats::{
    animation_keys::{AXIS_FLAGS, KeyTrack},
    wad::ParseError,
};
use std::{error::Error, fmt};

/// InitAnim's period conversion, not an assumption that a metadata field is FPS.
/// `anim_time_scale` is a source clock input (1/30 in the measured fixture).
pub fn frame_period(
    timing_factor: i16,
    playback_scale: f32,
    anim_time_scale: f32,
) -> Result<f32, UnsupportedNumericDomain> {
    if playback_scale <= 0.0 || anim_time_scale <= 0.0 {
        return Err(UnsupportedNumericDomain);
    }
    let scale = if timing_factor > 0 {
        mul_rz(
            mul_rz(f32::from(timing_factor), f32::from_bits(0x3d08_8889))?,
            playback_scale,
        )?
    } else {
        playback_scale
    };
    mul_rz(scale, anim_time_scale)
}

/// CalcAnimation's transition channel blend, using InterpPYR/InterpXYZ and the
/// original post-blend mirror axes. Angle differences wrap; results are not
/// normalized afterward. This differs from the sparse-key interpolation path.
pub fn blend_channels(
    from: [f32; 9],
    target: [f32; 9],
    fraction: f32,
    mirror: bool,
) -> Result<[f32; 9], UnsupportedNumericDomain> {
    if from
        .iter()
        .chain(&target)
        .chain(std::iter::once(&fraction))
        .any(|&v| v != 0.0 && !v.is_normal())
    {
        return Err(UnsupportedNumericDomain);
    }
    let mut result = from;
    for axis in 0..9 {
        if from[axis] != target[axis] {
            let mut delta = sub_guarded_rz(target[axis], from[axis])?;
            if axis < 3 {
                if delta > f32::from_bits(0x4049_0fdb) {
                    delta = sub_guarded_rz(delta, f32::from_bits(0x40c9_0fdb))?;
                }
                if delta <= f32::from_bits(0xc049_0fdb) {
                    delta = add_guarded_rz(delta, f32::from_bits(0x40c9_0fdb))?;
                }
            }
            result[axis] = add_guarded_rz(from[axis], mul_rz(fraction, delta)?)?;
        }
    }
    if mirror {
        for axis in [1, 2, 3, 6] {
            result[axis] = -result[axis];
        }
    }
    Ok(result)
}

#[derive(Debug)]
pub enum KeyError {
    Format(ParseError),
    Numeric {
        key: u16,
        axis: usize,
        source: UnsupportedNumericDomain,
    },
    MissingKey(u16),
    UnsupportedSample,
}
impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Format(e) => e.fmt(f),
            Self::Numeric { key, axis, source } => write!(f, "key {key}, axis {axis}: {source}"),
            Self::MissingKey(key) => write!(f, "frame {key} is not an encoded key"),
            Self::UnsupportedSample => {
                f.write_str("unsupported animation frame or interpolation gap")
            }
        }
    }
}
impl Error for KeyError {}
impl From<ParseError> for KeyError {
    fn from(value: ParseError) -> Self {
        Self::Format(value)
    }
}

/// Visits each decoded key without retaining an unbounded decoded animation.
/// Work is bounded by the track's signed 16-bit source frame count and nine axes.
pub fn visit_keys(
    track: &KeyTrack<'_>,
    mut visit: impl FnMut(u16, [f32; 9]),
) -> Result<(), KeyError> {
    visit_keys_through(track, u16::MAX, &mut visit)
}

fn visit_keys_through(
    track: &KeyTrack<'_>,
    end: u16,
    mut visit: impl FnMut(u16, [f32; 9]),
) -> Result<(), KeyError> {
    let mut values = track.initial;
    visit(0, values);
    for (ordinal, &key) in track.key_frames[1..].iter().enumerate() {
        if key > end {
            break;
        }
        let row = track.row(ordinal)?;
        if track.compressed {
            for (axis, bit) in AXIS_FLAGS.iter().enumerate() {
                if track.transform_flags & bit != 0 {
                    values[axis] = add_guarded_rz(values[axis], row[axis])
                        .map_err(|source| KeyError::Numeric { key, axis, source })?;
                }
            }
        } else {
            values = row;
        }
        visit(key, values);
    }
    Ok(())
}

pub fn exact_key(track: &KeyTrack<'_>, frame: u16) -> Result<[f32; 9], KeyError> {
    if !track.key_frames.contains(&frame) {
        return Err(KeyError::MissingKey(frame));
    }
    let mut found = None;
    visit_keys_through(track, frame, |key, values| {
        if key == frame {
            found = Some(values);
        }
    })?;
    Ok(found.unwrap())
}

/// Fresh channel evaluation from the inspected GetAnimAngXYZVal path. Clip
/// clocks, transition blending, mirror flags and node matrices are separate.
/// Reciprocal spacing table values for 1..255 were checked in frozen reference
/// RAM and equal nearest-rounded float32 reciprocals. Longer gaps are rejected.
pub fn sample_channels(track: &KeyTrack<'_>, frame: f32) -> Result<[f32; 9], KeyError> {
    sample_channels_with(track, frame, false)
}

/// [`sample_channels`] that also interpolates key gaps of 256 frames or more
/// with a nearest-rounded float32 reciprocal (HYPOTHESIS: the table beyond
/// 255 was not checked). Used by the critter skeleton (tree_pose).
pub fn sample_channels_long_gaps(track: &KeyTrack<'_>, frame: f32) -> Result<[f32; 9], KeyError> {
    sample_channels_with(track, frame, true)
}

fn sample_channels_with(track: &KeyTrack<'_>, frame: f32, long_gaps: bool) -> Result<[f32; 9], KeyError> {
    if !frame.is_finite() || frame < 0.0 {
        return Err(KeyError::UnsupportedSample);
    }
    if track.initial_only || frame == 0.0 {
        return Ok(track.initial);
    }
    let upper_index = track
        .key_frames
        .partition_point(|&key| f32::from(key) < frame)
        .min(track.key_frames.len() - 1);
    let upper_frame = track.key_frames[upper_index];
    let lower_frame = track.key_frames[upper_index.saturating_sub(1)];
    let mut lower = track.initial;
    let mut upper = track.initial;
    visit_keys_through(track, upper_frame, |key, value| {
        if key == lower_frame {
            lower = value;
        }
        if key == upper_frame {
            upper = value;
        }
    })?;
    let numeric_error = |axis, source| KeyError::Numeric {
        key: upper_frame,
        axis,
        source,
    };
    let gap_to_upper =
        sub_guarded_rz(f32::from(upper_frame), frame).map_err(|e| numeric_error(0, e))?;
    let mut values = upper;
    if upper_frame > lower_frame && gap_to_upper > f32::from_bits(0x3e00_0000) {
        let spacing = upper_frame - lower_frame;
        if spacing >= 256 && !long_gaps {
            return Err(KeyError::UnsupportedSample);
        }
        let reciprocal = (1.0f64 / f64::from(spacing)) as f32;
        let fraction = mul_rz(
            sub_guarded_rz(frame, f32::from(lower_frame)).map_err(|e| numeric_error(0, e))?,
            reciprocal,
        )
        .map_err(|e| numeric_error(0, e))?;
        for axis in 0..9 {
            let difference =
                sub_guarded_rz(upper[axis], lower[axis]).map_err(|e| numeric_error(axis, e))?;
            if axis < 3
                && !(difference > f32::from_bits(0xbfc9_0fdb)
                    && difference < f32::from_bits(0x3fc9_0fdb))
            {
                values[axis] = lower[axis];
            } else {
                values[axis] = add_guarded_rz(
                    lower[axis],
                    mul_rz(difference, fraction).map_err(|e| numeric_error(axis, e))?,
                )
                .map_err(|e| numeric_error(axis, e))?;
            }
        }
    }
    for (axis, value) in values[..3].iter_mut().enumerate() {
        if *value > f32::from_bits(0x4049_0fdb) {
            *value = sub_guarded_rz(*value, f32::from_bits(0x40c9_0fdb))
                .map_err(|e| numeric_error(axis, e))?;
        } else if *value <= f32::from_bits(0xc049_0fdb) {
            *value = add_guarded_rz(*value, f32::from_bits(0x40c9_0fdb))
                .map_err(|e| numeric_error(axis, e))?;
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gdl_formats::animation::{Sequence, SequenceInfo, Tree};
    #[test]
    fn metadata_factor_scales_period_instead_of_frame_rate() {
        let base = f32::from_bits(0x3d08_8889);
        assert_eq!(frame_period(30, 1.0, base).unwrap(), base);
        assert_eq!(frame_period(15, 1.0, base).unwrap(), base * 0.5);
        assert_eq!(frame_period(30, 2.0, base).unwrap(), base * 2.0);
        assert_eq!(frame_period(0, 1.0, base).unwrap(), base);
        assert_eq!(frame_period(-1, 1.0, base).unwrap(), base);
        assert!(frame_period(30, 0.0, base).is_err());
        assert!(frame_period(30, f32::NAN, base).is_err());
    }
    #[test]
    fn transition_blend_wraps_difference_and_preserves_equal_channel_bits() {
        let from = [3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let mut target = from;
        target[0] = -3.0;
        target[3] = 4.0;
        target[4] = -0.0;
        let value = blend_channels(from, target, 0.5, false).unwrap();
        assert!(value[0] > 3.0); // does not wrap the resulting angle back below pi
        assert_eq!(value[3], 2.0);
        assert_eq!(value[4].to_bits(), 0); // equality branch copies the old +zero
        let mirrored = blend_channels(from, target, 0.5, true).unwrap();
        assert_eq!(mirrored[3], -2.0);
        assert_eq!(mirrored[6], -1.0);
        assert_eq!(mirrored[1].to_bits(), 0x8000_0000);
    }
    #[test]
    fn exact_keys_accumulate_deltas_and_stop_before_later_invalid_data() {
        let tree = Tree {
            index: 0,
            label: "SYNTHETIC".into(),
            file_offset: 0,
            prefix: String::new(),
            raw_info: vec![],
            raw_header: vec![],
            raw_animation_header: vec![],
            compressed_angles_offset: 16,
            compressed_positions_offset: 16,
            compressed_scales_offset: 16,
            blocks_offset: 0,
            sequences: vec![Sequence {
                index: 0,
                label: "TEST".into(),
                frames: 33,
                timing_factor: 30,
                repeat: 0,
                root_transfer_flags: 0,
                frame_order_flags: 0,
                raw: vec![],
            }],
            nodes: vec![],
            sequence_infos: vec![SequenceInfo {
                node_index: 0,
                sequence_index: 0,
                transform_flags: 0x2001,
                channels: 1,
                relative_data_offset: 0,
                raw: vec![],
            }],
        };
        let mut bytes = vec![0; 28];
        bytes[0] = 5;
        bytes[4] = 1;
        bytes[8..12].copy_from_slice(&0.5f32.to_le_bytes());
        bytes[12..14].copy_from_slice(&[1, 2]);
        bytes[20..24].copy_from_slice(&0.125f32.to_le_bytes());
        bytes[24..28].copy_from_slice(&(-0.25f32).to_le_bytes());
        let track = tree.key_track(&bytes, 0, 0).unwrap();
        assert_eq!(exact_key(&track, 2).unwrap()[0], 0.625);
        assert_eq!(sample_channels(&track, 1.0).unwrap()[0], 0.5625);
        assert_eq!(sample_channels(&track, 1.875).unwrap()[0], 0.625);
        assert!(sample_channels(&track, 1.874).unwrap()[0] < 0.625);
        let last = exact_key(&track, 32).unwrap();
        assert_eq!(last[0], 0.375);
        assert_eq!(last[6..], [1.0; 3]);
        assert_eq!(sample_channels(&track, 99.0).unwrap()[0], last[0]);
        assert!(sample_channels(&track, -1.0).is_err());
        assert!(sample_channels(&track, f32::NAN).is_err());
        assert!(matches!(exact_key(&track, 3), Err(KeyError::MissingKey(3))));
        bytes[13] = 255;
        let track = tree.key_track(&bytes, 0, 0).unwrap();
        assert_eq!(exact_key(&track, 2).unwrap()[0], 0.625);
        assert!(exact_key(&track, 32).is_err());
        bytes[13] = 2;
        bytes[20..24].copy_from_slice(&3.0f32.to_le_bytes());
        let track = tree.key_track(&bytes, 0, 0).unwrap();
        // The source sampler holds the lower angle when the difference reaches
        // its pi/2 cutoff; it does not select a generic shortest quaternion path.
        assert_eq!(sample_channels(&track, 1.0).unwrap()[0], 0.5);
        assert_eq!(
            sample_channels(&track, 2.0).unwrap()[0],
            sub_guarded_rz(3.5, f32::from_bits(0x40c9_0fdb)).unwrap()
        );
    }
}
