//! Bounded scalar arithmetic qualified against the configured PS2 reference.
//! This is a finite addition subset, not a complete Emotion Engine FPU.
use std::{error::Error, fmt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedNumericDomain;
impl fmt::Display for UnsupportedNumericDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("reference arithmetic: non-finite, subnormal or overflowing value")
    }
}
impl Error for UnsupportedNumericDomain {}

/// Addition with one alignment guard bit and rounding toward zero.
/// Verified for normal finite operands and zero in the SLUS-20047 animation
/// fixture, using PCSX2 2.6.3 with FPU.Roundmode=3. Exceptional domains are
/// explicitly rejected until their original consumers and behavior are tested.
pub fn add_guarded_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    let valid = |v: f32| v == 0.0 || v.is_normal();
    if !valid(a) || !valid(b) {
        return Err(UnsupportedNumericDomain);
    }
    let mut words = [a.to_bits(), b.to_bits()];
    let exponents = words.map(|w| ((w >> 23) & 255) as i32);
    let distance = exponents[0] - exponents[1];
    if distance != 0 {
        let smaller = usize::from(distance > 0);
        let shift = distance.unsigned_abs();
        let mask = if shift >= 25 {
            0x8000_0000
        } else {
            u32::MAX << (shift - 1)
        };
        words[smaller] &= mask;
    }
    // After alignment masking, both operands together need at most 49 binary
    // significant digits. This sum is exact in f64; no host rounding-mode change
    // or fused operation is involved.
    let exact = f64::from(f32::from_bits(words[0])) + f64::from(f32::from_bits(words[1]));
    round_rz(exact)
}

fn round_rz(exact: f64) -> Result<f32, UnsupportedNumericDomain> {
    if !exact.is_finite()
        || exact.abs() > f64::from(f32::MAX)
        || (exact != 0.0 && exact.abs() < f64::from(f32::MIN_POSITIVE))
    {
        return Err(UnsupportedNumericDomain);
    }
    let rounded = exact as f32;
    let result = if f64::from(rounded).abs() > exact.abs() {
        f32::from_bits(rounded.to_bits() - 1)
    } else {
        rounded
    };
    if result != 0.0 && !result.is_normal() {
        return Err(UnsupportedNumericDomain);
    }
    Ok(result)
}

pub fn sub_guarded_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    add_guarded_rz(a, -b)
}

/// The configured reference's finite scalar multiply path, rounded toward zero.
/// This is not certification of the real hardware's multiplier corner cases.
pub fn mul_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    if (a != 0.0 && !a.is_normal()) || (b != 0.0 && !b.is_normal()) {
        return Err(UnsupportedNumericDomain);
    }
    // Two 24-bit significands have an exact product in f64.
    round_rz(f64::from(a) * f64::from(b))
}

/// Finite scalar division with nearest rounding, qualified for the reference's
/// separately configured FPU division mode=0. Exceptional domains are rejected.
pub fn div_rn(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    if (a != 0.0 && !a.is_normal()) || !b.is_normal() {
        return Err(UnsupportedNumericDomain);
    }
    let exact = f64::from(a) / f64::from(b);
    let result = exact as f32;
    if !result.is_finite()
        || (result != 0.0 && !result.is_normal())
        || (result == 0.0 && exact != 0.0)
    {
        return Err(UnsupportedNumericDomain);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alignment_guard_changes_cancellation_and_is_symmetric() {
        let small = -f32::from_bits(0x3e00_0003);
        assert_ne!(1.0 + small, 0.875);
        assert_eq!(add_guarded_rz(1.0, small).unwrap(), 0.875);
        assert_eq!(add_guarded_rz(small, 1.0).unwrap(), 0.875);
        assert_eq!(add_guarded_rz(-1.0, -small).unwrap(), -0.875);
    }
    #[test]
    fn toward_zero_and_exponent_distance_boundaries() {
        assert_eq!(
            add_guarded_rz(1.0, f32::from_bits(0x3380_0000)).unwrap(),
            1.0
        );
        assert_eq!(
            add_guarded_rz(1.0, -f32::from_bits(0x3380_0000))
                .unwrap()
                .to_bits(),
            0x3f7f_ffff
        );
        assert_eq!(
            add_guarded_rz(1.0, -f32::from_bits(0x3300_0000)).unwrap(),
            1.0
        );
        assert_eq!(add_guarded_rz(-0.0, -0.0).unwrap().to_bits(), 0x8000_0000);
        assert_eq!(add_guarded_rz(1.0, -1.0).unwrap().to_bits(), 0);
    }
    #[test]
    fn unsupported_domains_fail_instead_of_saturating() {
        for value in [f32::NAN, f32::INFINITY, f32::from_bits(1)] {
            assert!(add_guarded_rz(value, 1.0).is_err());
        }
        assert!(add_guarded_rz(f32::MAX, f32::MAX).is_err());
        assert!(add_guarded_rz(f32::MIN_POSITIVE, -f32::from_bits(0x0080_0001)).is_err());
    }
    #[test]
    fn multiplication_rounds_each_operation_toward_zero() {
        let a = f32::from_bits(0x3f80_0001);
        let b = f32::from_bits(0x3fc0_0001);
        assert_eq!(mul_rz(a, b).unwrap().to_bits(), 0x3fc0_0002);
        assert_eq!((a * b).to_bits(), 0x3fc0_0003);
        assert_eq!(mul_rz(-a, b).unwrap().to_bits(), 0xbfc0_0002);
        assert_eq!(mul_rz(-0.0, 1.0).unwrap().to_bits(), 0x8000_0000);
        assert!(mul_rz(f32::MIN_POSITIVE, 0.5).is_err());
    }
    #[test]
    fn division_uses_the_separate_nearest_rounding_contract() {
        assert_eq!(div_rn(1.0, 3.0).unwrap().to_bits(), 0x3eaa_aaab);
        assert_eq!(div_rn(-1.0, 3.0).unwrap().to_bits(), 0xbeaa_aaab);
        assert!(div_rn(1.0, 0.0).is_err());
        assert!(div_rn(f32::INFINITY, 1.0).is_err());
        assert!(div_rn(f32::MIN_POSITIVE, 2.0).is_err());
    }
}
