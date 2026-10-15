//! VU0 microprograms reached from gameplay code, reproduced with the
//! reference-qualified VU arithmetic subset. Microcode is fixture-observed
//! (vu0MicroMem.bin in the preserved state); no VU interpreter is involved.
use crate::numeric::{add_rz, mul_rz, sqrt_rz, UnsupportedNumericDomain};

/// fqdist EE 0x1d6b90 → VCALLMS 0x44 (micro pc 68..75): MUL.xy vf16 squares the
/// staged (x, z) lanes, ADDy.x sums them, SQRT/WAITQ, then ADDq.x returns Q in
/// the x lane. The y lane keeps z² and is returned for comparison.
pub fn fqdist(x: f32, z: f32) -> Result<(f32, f32), UnsupportedNumericDomain> {
    let xx = mul_rz(x, x)?;
    let zz = mul_rz(z, z)?;
    let q = sqrt_rz(add_rz(xx, zz)?)?;
    // ADDq.x vf16, vf00, Q: vf00.x is 0.0, so the ordinary VU add keeps Q.
    Ok((add_rz(0.0, q)?, zz))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn squared_terms_and_root_both_round_toward_zero() {
        assert_eq!(fqdist(3.0, 4.0).unwrap(), (5.0, 16.0));
        assert_eq!(fqdist(0.0, 0.0).unwrap(), (0.0, 0.0));
        // x*x truncates, so the root of a single axis can fall one ulp short.
        let x = f32::from_bits(0x3fcccccb);
        assert!(fqdist(x, 0.0).unwrap().0 <= x);
    }
}
