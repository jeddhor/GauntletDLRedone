//! Bounded scalar arithmetic qualified against the configured PS2 reference.
//! This is a finite addition subset, not a complete Emotion Engine FPU.
//!
//! Denormals: the reference's `FPU/FPUDiv/VU0/VU1.DenormalsAreZero = true`
//! sets both DAZ and FTZ in PCSX2 (upstream claim: pcsx2 checkout 0f71e668,
//! Pcsx2Config.cpp read_fpcr and DEFAULT_FPU_FP_CONTROL_REGISTER), so
//! subnormal operands read as signed zero and results below the smallest
//! normal flush to signed zero. Infinities and NaNs remain unsupported.
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
#[inline]
pub fn add_guarded_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    let (mut wa, mut wb) = (daz(a)?.to_bits(), daz(b)?.to_bits());
    let (ea, eb) = ((wa >> 23) & 255, (wb >> 23) & 255);
    // The smaller operand keeps its bits down to one guard bit below the
    // larger's last place (sign only beyond 24 places).
    let mask = |shift: u32| if shift >= 25 { 0x8000_0000 } else { u32::MAX << (shift - 1) };
    if eb < ea {
        wb &= mask(ea - eb);
    } else if ea < eb {
        wa &= mask(eb - ea);
    }
    // After alignment masking, both operands together need at most 49 binary
    // significant digits. This sum is exact in f64; no host rounding-mode change
    // or fused operation is involved.
    round_rz(f64::from(f32::from_bits(wa)) + f64::from(f32::from_bits(wb)))
}

/// DAZ: a subnormal operand reads as zero of its sign; infinities and NaNs
/// are refused. (Bit form of the reference implementation kept in the
/// tests, `reference::daz`; compared over random and boundary operands.)
#[inline]
fn daz(v: f32) -> Result<f32, UnsupportedNumericDomain> {
    let bits = v.to_bits();
    match bits & 0x7f80_0000 {
        0x7f80_0000 => Err(UnsupportedNumericDomain),
        0 => Ok(f32::from_bits(bits & 0x8000_0000)),
        _ => Ok(v),
    }
}

/// Rounding toward zero of an f64 to f32 with FTZ: the significand
/// truncated to 23 bits (round toward zero for a normal result); results
/// below the smallest normal flush to signed zero; beyond f32::MAX, infinite
/// or NaN refused. Equal to `reference::round_rz` (nearest conversion
/// stepped back one ulp when it overshoots), which the tests compare.
#[inline]
fn round_rz(exact: f64) -> Result<f32, UnsupportedNumericDomain> {
    let bits = exact.to_bits();
    let sign = ((bits >> 32) as u32) & 0x8000_0000;
    let e64 = ((bits >> 52) & 0x7ff) as i32;
    let e32 = e64 - (1023 - 127);
    if e32 <= 0 {
        // FTZ (round toward zero first, then flush): signed zero.
        return Ok(f32::from_bits(sign));
    }
    let mant = bits & 0x000f_ffff_ffff_ffff;
    let top = (mant >> 29) as u32;
    if 255 <= e32 || (e32 == 254 && top == 0x7f_ffff && mant & 0x1fff_ffff != 0) {
        return Err(UnsupportedNumericDomain);
    }
    Ok(f32::from_bits(sign | (e32 as u32) << 23 | top))
}

#[inline]
pub fn sub_guarded_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    add_guarded_rz(a, -b)
}

/// Finite ordinary addition rounded toward zero, without the scalar EE alignment
/// mask. Qualified against configured PCSX2 VU SSE arithmetic and a controlled
/// live-bone GetWorldMat probe. General/exceptional domains remain unsupported.
#[inline]
pub fn add_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    let (a, b) = (daz(a)?, daz(b)?);
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
#[inline]
pub fn mul_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    let (a, b) = (daz(a)?, daz(b)?);
    // Two 24-bit significands have an exact product in f64.
    round_rz(f64::from(a) * f64::from(b))
}

/// Finite scalar division with nearest rounding, qualified for the reference's
/// separately configured FPU division mode=0. Exceptional domains are rejected.
pub fn div_rn(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    let (a, b) = (daz(a)?, daz(b)?);
    if b == 0.0 {
        return Err(UnsupportedNumericDomain);
    }
    let exact = f64::from(a) / f64::from(b);
    let result = exact as f32;
    if !result.is_finite() {
        return Err(UnsupportedNumericDomain);
    }
    // FTZ for quotients below the smallest normal (FPUDiv.DenormalsAreZero).
    if result != 0.0 && !result.is_normal() || result == 0.0 && exact != 0.0 {
        return Ok(if exact.is_sign_negative() { -0.0 } else { 0.0 });
    }
    Ok(result)
}

/// Square root rounded toward zero, as configured PCSX2 microVU SQRT executes
/// SSE sqrtss under the VU0 MXCSR (Roundmode 3). Finite nonnegative normal
/// operands and zero only; the negative-operand flag path is not modelled.
/// [`div_rn`] with the EE's zero-divisor result: a divisor whose exponent is
/// zero gives ±FLT_MAX with the XOR of the operand signs (upstream claim:
/// PCSX2 FPU.cpp checkDivideByZero, not a hardware capture). Only call sites
/// whose instructions can divide by a data-driven zero use this.
pub fn div_rn_ee(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    if b.to_bits() & 0x7f80_0000 == 0 {
        return Ok(f32::from_bits(((a.to_bits() ^ b.to_bits()) & 0x8000_0000) | 0x7f7f_ffff));
    }
    div_rn(a, b)
}

pub fn sqrt_rz(a: f32) -> Result<f32, UnsupportedNumericDomain> {
    let a = daz(a)?;
    if a != 0.0 && a.is_sign_negative() {
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
#[inline]
pub fn sub_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    add_rz(a, -b)
}

/// VU0 macro-mode DIV: PCSX2 v2.6.3 COP2 macro ops run under the EE MXCSR
/// (configured Roundmode 3), so the quotient truncates toward zero.
pub fn div_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
    let (a, b) = (daz(a)?, daz(b)?);
    if b == 0.0 {
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
    let a = daz(a)?;
    if a != 0.0 && a.is_sign_negative() {
        return Err(UnsupportedNumericDomain);
    }
    // f64 sqrt of an f32 is correctly rounded; rounding again to f32 is
    // innocuous for square roots (53 >= 2*24 + 2).
    Ok(f64::from(a).sqrt() as f32)
}

#[cfg(test)]
mod reference {
    //! The original (pre-2026-10-24) forms of daz and round_rz, kept as the
    //! oracle for the bit forms above.
    use super::UnsupportedNumericDomain;

    pub fn daz(v: f32) -> Result<f32, UnsupportedNumericDomain> {
        if !v.is_finite() {
            return Err(UnsupportedNumericDomain);
        }
        Ok(if v != 0.0 && !v.is_normal() { 0.0f32.copysign(v) } else { v })
    }

    pub fn add_guarded_rz(a: f32, b: f32) -> Result<f32, UnsupportedNumericDomain> {
        let (a, b) = (daz(a)?, daz(b)?);
        let mut words = [a.to_bits(), b.to_bits()];
        let exponents = words.map(|w| ((w >> 23) & 255) as i32);
        let distance = exponents[0] - exponents[1];
        if distance != 0 {
            let smaller = usize::from(distance > 0);
            let shift = distance.unsigned_abs();
            let mask = if shift >= 25 { 0x8000_0000 } else { u32::MAX << (shift - 1) };
            words[smaller] &= mask;
        }
        let exact = f64::from(f32::from_bits(words[0])) + f64::from(f32::from_bits(words[1]));
        round_rz(exact)
    }

    pub fn round_rz(exact: f64) -> Result<f32, UnsupportedNumericDomain> {
        if !exact.is_finite() || exact.abs() > f64::from(f32::MAX) {
            return Err(UnsupportedNumericDomain);
        }
        if exact.abs() < f64::from(f32::MIN_POSITIVE) {
            return Ok(if exact.is_sign_negative() { -0.0 } else { 0.0 });
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
}

#[cfg(test)]
mod bit_forms {
    use super::reference;

    fn same(a: Result<f32, super::UnsupportedNumericDomain>, b: Result<f32, super::UnsupportedNumericDomain>) -> bool {
        match (a, b) {
            (Ok(x), Ok(y)) => x.to_bits() == y.to_bits(),
            (Err(_), Err(_)) => true,
            _ => false,
        }
    }

    /// Synthetic: the bit forms of daz and round_rz equal the reference
    /// forms on boundary values and four million random operands (random
    /// words, nearby exponents, f64 sums, products and quotients).
    #[test]
    fn bit_forms_equal_the_reference_forms() {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            x.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };
        let specials: Vec<f32> = vec![0.0, -0.0, 1.0, -1.0, f32::MAX, -f32::MAX, f32::MIN_POSITIVE, -f32::MIN_POSITIVE,
            f32::from_bits(1), f32::from_bits(0x807f_ffff), f32::INFINITY, f32::NEG_INFINITY, f32::NAN, 0.5, 3.0e-39, 1.0e38];
        for &v in &specials {
            assert!(same(super::daz(v), reference::daz(v)), "daz {v:?}");
        }
        let f64_specials: Vec<f64> = vec![0.0, -0.0, f64::from(f32::MAX), -f64::from(f32::MAX), f64::from(f32::MAX) * (1.0 + f64::EPSILON),
            f64::from(f32::MIN_POSITIVE), f64::from(f32::MIN_POSITIVE) * (1.0 - f64::EPSILON), 1.0e-50, 1.0e300, f64::INFINITY, f64::NAN,
            f64::from(f32::MAX) + 1.0e20, 2.0f64.powi(128), 2.0f64.powi(128) * (1.0 - f64::EPSILON)];
        for &e in &f64_specials {
            assert!(same(super::round_rz(e), reference::round_rz(e)), "round_rz {e:?}");
            assert!(same(super::round_rz(-e), reference::round_rz(-e)), "round_rz {:?}", -e);
        }
        for _ in 0..4_000_000 {
            let r = next();
            let a = f32::from_bits(r as u32);
            // b: random, or a's exponent ± a few (cancellation and alignment).
            let b = if r >> 62 == 0 {
                f32::from_bits((r >> 32) as u32)
            } else {
                let ea = (a.to_bits() >> 23) & 255;
                let eb = (ea as i32 + ((r >> 40) % 9) as i32 - 4).clamp(0, 255) as u32;
                f32::from_bits(((r >> 33) as u32 & 0x807f_ffff) | eb << 23)
            };
            assert!(same(super::daz(a), reference::daz(a)), "daz {a:?}");
            assert!(same(super::add_guarded_rz(a, b), reference::add_guarded_rz(a, b)), "add {a:?} {b:?}");
            assert!(same(super::add_guarded_rz(b, a), reference::add_guarded_rz(b, a)), "add {b:?} {a:?}");
            for e in [f64::from(a) + f64::from(b), f64::from(a) * f64::from(b), f64::from(a) / f64::from(b), f64::from(a).abs().sqrt(), f64::from_bits(next())] {
                assert!(same(super::round_rz(e), reference::round_rz(e)), "round_rz {e:?} ({a:?}, {b:?})");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn div_rn_ee_zero_divisor_gives_signed_max() {
        // Synthetic: x/±0 and 0/0 follow PCSX2's checkDivideByZero.
        assert_eq!(super::div_rn_ee(3.0, 0.0).unwrap(), f32::MAX);
        assert_eq!(super::div_rn_ee(-3.0, 0.0).unwrap(), -f32::MAX);
        assert_eq!(super::div_rn_ee(3.0, -0.0).unwrap(), -f32::MAX);
        assert_eq!(super::div_rn_ee(0.0, 0.0).unwrap(), f32::MAX);
        assert_eq!(super::div_rn_ee(6.0, 2.0).unwrap(), 3.0);
    }

    use super::*;
    #[test]
    fn denormals_read_and_flush_as_signed_zero() {
        // Synthetic: DAZ/FTZ as configured for the reference (upstream claim).
        let tiny = f32::MIN_POSITIVE;
        assert_eq!(mul_rz(tiny, 0.5).unwrap().to_bits(), 0);
        assert_eq!(mul_rz(-tiny, 0.5).unwrap().to_bits(), 0x8000_0000);
        assert_eq!(add_guarded_rz(f32::from_bits(1), 1.0).unwrap(), 1.0);
        assert!(mul_rz(f32::INFINITY, 1.0).is_err());
    }
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
        for value in [f32::NAN, f32::INFINITY] {
            assert!(add_guarded_rz(value, 1.0).is_err());
        }
        assert!(add_guarded_rz(f32::MAX, f32::MAX).is_err());
        // DAZ/FTZ: a subnormal operand is zero; a subnormal sum flushes.
        assert_eq!(add_guarded_rz(f32::from_bits(1), 1.0).unwrap(), 1.0);
        assert_eq!(add_guarded_rz(f32::MIN_POSITIVE, -f32::from_bits(0x0080_0001)).unwrap().to_bits(), 0x8000_0000);
    }
    #[test]
    fn multiplication_rounds_each_operation_toward_zero() {
        let a = f32::from_bits(0x3f80_0001);
        let b = f32::from_bits(0x3fc0_0001);
        assert_eq!(mul_rz(a, b).unwrap().to_bits(), 0x3fc0_0002);
        assert_eq!((a * b).to_bits(), 0x3fc0_0003);
        assert_eq!(mul_rz(-a, b).unwrap().to_bits(), 0xbfc0_0002);
        assert_eq!(mul_rz(-0.0, 1.0).unwrap().to_bits(), 0x8000_0000);
        assert_eq!(mul_rz(f32::MIN_POSITIVE, 0.5).unwrap().to_bits(), 0);
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
        assert_eq!(sqrt_rz(f32::from_bits(1)).unwrap(), 0.0);
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
        assert_eq!(div_rn(f32::MIN_POSITIVE, 2.0).unwrap().to_bits(), 0);
    }
}
