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

/// Finite ordinary addition rounded toward zero, without the scalar EE alignment
/// mask. Qualified against configured PCSX2 VU SSE arithmetic and a controlled
/// live-bone GetWorldMat probe. General/exceptional domains remain unsupported.
pub fn add_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    if (a != 0.0 && !a.is_normal()) || (b != 0.0 && !b.is_normal()) {
        return Err(UnsupportedNumericDomain);
    }
    if a != 0.0 && b != 0.0 {
        let exponent = |v: f32| ((v.to_bits() >> 23) & 255) as i32;
        if (exponent(a) - exponent(b)).unsigned_abs() >= 25 {
            let (large, small) = if a.abs() > b.abs() { (a, b) } else { (b, a) };
            // Small is less than one ulp. Keep its sign as a sticky contribution;
            // converting through f64 alone would lose it at very large gaps.
            return Ok(if large.is_sign_negative() == small.is_sign_negative() {
                large
            } else {
                f32::from_bits(large.to_bits() - 1)
            });
        }
    }
    round_rz(f64::from(a) + f64::from(b))
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

/// Square root rounded toward zero, as configured PCSX2 microVU SQRT executes
/// SSE sqrtss under the VU0 MXCSR (Roundmode 3). Finite nonnegative normal
/// operands and zero only; the negative-operand flag path is not modelled.
pub fn sqrt_rz(a: f32) -> Result<f32, UnsupportedNumericDomain> {
    if a != 0.0 && (a.is_sign_negative() || !a.is_normal()) {
        return Err(UnsupportedNumericDomain);
    }
    let exact = f64::from(a);
    // f64 sqrt is correctly rounded; truncation can overshoot only when the f64
    // result lands on an f32 value above the true root, detected exactly below.
    let mut root = round_rz(exact.sqrt())?;
    if f64::from(root) * f64::from(root) > exact {
        root = f32::from_bits(root.to_bits() - 1);
    }
    Ok(root)
}

/// Ordinary VU subtraction rounded toward zero (`add_rz` of the negation).
pub fn sub_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    add_rz(a, -b)
}

/// VU0 macro-mode DIV: PCSX2 v2.6.3 COP2 macro ops run under the EE MXCSR
/// (configured Roundmode 3), so the quotient truncates toward zero.
pub fn div_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    if (a != 0.0 && !a.is_normal()) || !b.is_normal() {
        return Err(UnsupportedNumericDomain);
    }
    let exact = f64::from(a) / f64::from(b);
    // The f64 quotient can round onto an f32 value just above the true
    // quotient; compare the product exactly to step back in that case.
    let mut q = round_rz(exact)?;
    if q != 0.0 && (f64::from(q) * f64::from(b)).abs() > f64::from(a).abs() {
        q = f32::from_bits(q.to_bits() - 1);
    }
    Ok(q)
}

/// EE SQRT.S: pinned PCSX2 forces nearest rounding for this instruction.
/// Negative operands (sqrt of |x| plus flags on the EE) are not modelled.
pub fn sqrt_rn(a: f32) -> Result<f32, UnsupportedNumericDomain> {
    if a != 0.0 && (a.is_sign_negative() || !a.is_normal()) {
        return Err(UnsupportedNumericDomain);
    }
    // f64 sqrt of an f32 is correctly rounded; rounding again to f32 is
    // innocuous for square roots (53 >= 2*24 + 2).
    Ok(f64::from(a).sqrt() as f32)
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
    fn ordinary_add_keeps_far_opposite_sign_sticky_bits_without_ee_mask() {
        let tiny = f32::MIN_POSITIVE;
        assert_eq!(add_rz(1.0, tiny).unwrap(), 1.0);
        assert_eq!(add_rz(1.0, -tiny).unwrap().to_bits(), 0x3f7fffff);
        assert_eq!(add_rz(-1.0, tiny).unwrap().to_bits(), 0xbf7fffff);
        assert_eq!(add_rz(-0.0, -0.0).unwrap().to_bits(), 0x80000000);
        assert_eq!(
            add_rz(1.0, -f32::from_bits(0x3e000003)).unwrap().to_bits(),
            0x3f5fffff
        );
        assert_ne!(
            add_rz(1.0, -tiny).unwrap(),
            add_guarded_rz(1.0, -tiny).unwrap()
        );
        assert!(add_rz(f32::MAX, f32::MAX).is_err());
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
    fn square_root_truncates_and_never_exceeds_the_true_root() {
        assert_eq!(sqrt_rz(4.0).unwrap(), 2.0);
        assert_eq!(sqrt_rz(2.0).unwrap().to_bits(), 0x3fb504f3);
        assert_eq!(sqrt_rz(0.0).unwrap(), 0.0);
        for bits in [0x3f800001u32, 0x40000000, 0x3e99999a, 0x7f7fffff] {
            let a = f32::from_bits(bits);
            let r = sqrt_rz(a).unwrap();
            let next = f32::from_bits(r.to_bits() + 1);
            assert!(f64::from(r).powi(2) <= f64::from(a) && f64::from(next).powi(2) > f64::from(a));
        }
        assert!(sqrt_rz(-1.0).is_err());
        assert!(sqrt_rz(f32::from_bits(1)).is_err());
    }
    #[test]
    fn vu_division_truncates_and_ee_sqrt_rounds_to_nearest() {
        assert_eq!(div_rz(1.0, 3.0).unwrap().to_bits(), 0x3eaa_aaaa);
        assert_eq!(div_rz(-1.0, 3.0).unwrap().to_bits(), 0xbeaa_aaaa);
        assert_eq!(div_rz(6.0, 3.0).unwrap(), 2.0);
        assert_eq!(sqrt_rn(2.0).unwrap().to_bits(), 0x3fb504f3);
        assert_eq!(sqrt_rn(3.0).unwrap().to_bits(), 0x3fddb3d7);
        assert_eq!(sqrt_rz(3.0).unwrap().to_bits(), 0x3fddb3d7);
        assert_eq!(sqrt_rn(5.0).unwrap().to_bits(), 0x400f1bbd);
        assert_eq!(sqrt_rz(5.0).unwrap().to_bits(), 0x400f1bbc);
        assert!(div_rz(1.0, 0.0).is_err() && sqrt_rn(-1.0).is_err());
        assert_eq!(sub_rz(1.0, 1.0).unwrap().to_bits(), 0);
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
