//! Original fast trigonometry and local animation matrices, independently
//! expressed from inspected retail instructions. Finite reference subset only.
use crate::numeric::{
    UnsupportedNumericDomain, add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub,
};

const HALF_PI: f32 = f32::from_bits(0x3fc9_0fdb);
const PI: f32 = f32::from_bits(0x4049_0fdb);
const THREE_HALF_PI: f32 = f32::from_bits(0x4096_cbe4);
const TAU: f32 = f32::from_bits(0x40c9_0fdb);

fn sine_polynomial(angle: f32) -> Result<f32, UnsupportedNumericDomain> {
    let square = mul(angle, angle)?;
    let mut term = mul(square, f32::from_bits(0x3638_ef1d))?;
    for coefficient in [0xb950_0d01, 0x3c08_8889, 0xbe2a_aaab] {
        term = mul(square, add(term, f32::from_bits(coefficient))?)?;
    }
    add(angle, mul(angle, term)?)
}

pub fn fast_sin(mut angle: f32) -> Result<f32, UnsupportedNumericDomain> {
    // Animation angles are small; reject the unbounded original reduction loop
    // outside a tested finite policy rather than hang on huge guest values.
    if !angle.is_finite() || angle.abs() > 16.0 * TAU {
        return Err(UnsupportedNumericDomain);
    }
    while angle < 0.0 {
        angle = add(angle, TAU)?;
    }
    while angle >= TAU {
        angle = sub(angle, TAU)?;
    }
    if angle < HALF_PI {
        sine_polynomial(angle)
    } else if angle < PI {
        sine_polynomial(sub(PI, angle)?)
    } else if angle < THREE_HALF_PI {
        Ok(-sine_polynomial(sub(angle, PI)?)?)
    } else {
        Ok(-sine_polynomial(sub(TAU, angle)?)?)
    }
}

pub fn fast_cos(angle: f32) -> Result<f32, UnsupportedNumericDomain> {
    fast_sin(add(angle, HALF_PI)?)
}

/// Flat four-column matrix in original memory order. Original RYP and PYR
/// construction paths have different operation order and are kept explicit.
pub fn rotation_matrix(angles: [f32; 3], pyr: bool) -> Result<[f32; 16], UnsupportedNumericDomain> {
    let cp = fast_cos(angles[0])?;
    let sp = -fast_sin(angles[0])?;
    let cy = fast_cos(angles[1])?;
    let sy = -fast_sin(angles[1])?;
    let cr = fast_cos(angles[2])?;
    let sr = -fast_sin(angles[2])?;
    let mut m = [0.0; 16];
    m[15] = 1.0;
    if pyr {
        m[0] = mul(cy, cr)?;
        m[4] = mul(-cy, sr)?;
        m[8] = -sy;
        let t = mul(sp, sy)?;
        m[1] = add(mul(-t, cr)?, mul(cp, sr)?)?;
        m[5] = add(mul(t, sr)?, mul(cp, cr)?)?;
        m[9] = mul(-sp, cy)?;
        let t = mul(cp, sy)?;
        m[2] = add(mul(t, cr)?, mul(sp, sr)?)?;
        m[6] = add(mul(t, -sr)?, mul(sp, cr)?)?;
        m[10] = mul(cp, cy)?;
    } else {
        let t = mul(-cr, sy)?;
        m[0] = mul(cr, cy)?;
        m[4] = add(mul(-sr, cp)?, mul(t, sp)?)?;
        m[8] = add(mul(sr, sp)?, mul(t, cp)?)?;
        let t = mul(-sr, sy)?;
        m[1] = mul(sr, cy)?;
        m[5] = add(mul(cr, cp)?, mul(t, sp)?)?;
        m[9] = add(mul(-cr, sp)?, mul(t, cp)?)?;
        m[2] = sy;
        m[6] = mul(cy, sp)?;
        m[10] = mul(cy, cp)?;
    }
    Ok(m)
}

/// DoAnimation's local output. Scale remains separate; world composition and
/// rendering are not performed here. No-channel records copy an identity basis.
pub fn local_transform(
    values: [f32; 9],
    initial_position: [f32; 3],
    flags: u16,
) -> Result<([f32; 16], [f32; 3]), UnsupportedNumericDomain> {
    let mut matrix = if flags & 0x0fff == 0 {
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    } else {
        rotation_matrix(values[..3].try_into().unwrap(), flags & 0x8000 != 0)?
    };
    for axis in 0..3 {
        if !initial_position[axis].is_finite() || !values[axis + 6].is_finite() {
            return Err(UnsupportedNumericDomain);
        }
        matrix[axis + 12] = if flags & 0x0fff == 0 {
            initial_position[axis]
        } else {
            add(values[axis + 3], initial_position[axis])?
        };
    }
    Ok((matrix, values[6..].try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quadrant_signs_and_bounded_reduction_are_explicit() {
        assert_eq!(fast_sin(0.0).unwrap().to_bits(), 0);
        assert_eq!(fast_sin(PI).unwrap().to_bits(), 0x8000_0000);
        assert!((fast_sin(HALF_PI).unwrap() - 1.0).abs() < 0.00001);
        assert!((fast_sin(THREE_HALF_PI).unwrap() + 1.0).abs() < 0.00001);
        for angle in [-7.0, -2.0, -0.5, 0.5, 2.0, 7.0] {
            assert!((fast_sin(angle).unwrap() - angle.sin()).abs() < 0.00001);
        }
        assert!(fast_sin(f32::NAN).is_err());
        assert!(fast_sin(1e30).is_err());
    }
    #[test]
    fn axis_order_differs_for_two_quarter_turns() {
        let ryp = rotation_matrix([HALF_PI, HALF_PI, 0.0], false).unwrap();
        let pyr = rotation_matrix([HALF_PI, HALF_PI, 0.0], true).unwrap();
        let expected_ryp = [
            0.0, 0.0, -1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let expected_pyr = [
            0.0, -1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        for (index, ((a, b), (x, y))) in ryp
            .iter()
            .zip(expected_ryp)
            .zip(pyr.iter().zip(expected_pyr))
            .enumerate()
        {
            // The original polynomial overshoots sin(pi/2) by about 3.7e-6;
            // three multiplied factors retain that error instead of normalizing.
            assert!((a - b).abs() < 0.00002, "RYP word {index}: {a} vs {b}");
            assert!((x - y).abs() < 0.00002, "PYR word {index}: {x} vs {y}");
        }
    }
    #[test]
    fn no_channel_transform_copies_identity_and_translation_bits() {
        let values = gdl_formats::animation_keys::DEFAULT_CHANNELS;
        let (matrix, scale) = local_transform(values, [-0.0, 2.0, 3.0], 0x2000).unwrap();
        assert_eq!(matrix[0], 1.0);
        assert_eq!(matrix[5], 1.0);
        assert_eq!(matrix[10], 1.0);
        assert_eq!(matrix[12].to_bits(), 0x8000_0000);
        assert_eq!(matrix[13..], [2.0, 3.0, 1.0]);
        assert_eq!(scale, [1.0; 3]);
    }
}
