//! Single-precision libm subset matching the SLUS-20047 ELF's atan2f/atanf/
//! sinf/cosf/tanf instruction sequences (EE 0x1e7b50/0x1e9db0, 0x1e6eb8, 0x1e7350,
//! 0x1e7160, 0x1e7440, reduction 0x1eae30, kernels 0x1ebdf0/0x1eb348/0x1ebef8). Their structure is
//! fdlibm/newlib-derived; this code was written independently from the reviewed
//! instructions. Constants are the ELF's literal words and table bytes.
//!
//! Every operation uses the reference-qualified scalar FPU subset in `numeric`.
//! Non-finite inputs, atan2f(0, 0), large-argument reduction (|x| > 0x43490f80,
//! the unimplemented 0x1eb4a0 path) and subnormal intermediates are rejected.
//! These routines are distinct from animation ffSin/ffCos.
use crate::numeric::{
    add_guarded_rz as add, div_rn as div, mul_rz as mul, sub_guarded_rz as sub,
    UnsupportedNumericDomain,
};

type Result<T> = std::result::Result<T, UnsupportedNumericDomain>;

fn w(bits: u32) -> f32 {
    f32::from_bits(bits)
}

fn fabs(x: f32) -> f32 {
    w(x.to_bits() & 0x7fff_ffff)
}

fn finite(x: f32) -> Result<f32> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(UnsupportedNumericDomain)
    }
}

/// EE 0x2b8c98, 0x2b8ca8 and 0x2b8cb8: identical in the ELF and frozen RAM.
const ATANHI: [u32; 4] = [0x3eed6338, 0x3f490fda, 0x3f7b985e, 0x3fc90fda];
const ATANLO: [u32; 4] = [0x31ac3769, 0x33222168, 0x33140fb4, 0x33a22168];
const AT: [u32; 11] = [
    0x3eaaaaab, 0xbe4ccccd, 0x3e124925, 0xbde38e38, 0x3dba2e6e, 0xbd9d8795, 0x3d886b35,
    0xbd6ef16b, 0x3d4bda59, 0xbd15a221, 0x3c8569d7,
];
/// EE 0x2b94c8, consulted by the medium reduction path for n < 32.
const NPIO2_HW: [u32; 32] = [
    0x3fc90f00, 0x40490f00, 0x4096cb00, 0x40c90f00, 0x40fb5300, 0x4116cb00, 0x412fed00,
    0x41490f00, 0x41623100, 0x417b5300, 0x418a3a00, 0x4196cb00, 0x41a35c00, 0x41afed00,
    0x41bc7e00, 0x41c90f00, 0x41d5a000, 0x41e23100, 0x41eec200, 0x41fb5300, 0x4203f200,
    0x420a3a00, 0x42108300, 0x4216cb00, 0x421d1400, 0x42235c00, 0x4229a500, 0x422fed00,
    0x42363600, 0x423c7e00, 0x4242c700, 0x42490f00,
];

/// atanf at EE 0x1e6eb8.
pub fn atanf(x: f32) -> Result<f32> {
    let hx = finite(x)?.to_bits() as i32;
    let ix = hx & 0x7fff_ffff;
    if ix > 0x507f_ffff {
        return if hx > 0 {
            add(w(ATANHI[3]), w(ATANLO[3]))
        } else {
            sub(-w(ATANHI[3]), w(ATANLO[3]))
        };
    }
    let mut x = x;
    let id: i32;
    if ix <= 0x3edf_ffff {
        if ix <= 0x30ff_ffff && add(x, w(0x7149_f2ca))? > 1.0 {
            return Ok(x);
        }
        id = -1;
    } else {
        x = fabs(x);
        if ix <= 0x3f97_ffff {
            if ix <= 0x3f2f_ffff {
                id = 0;
                x = div(sub(add(x, x)?, 1.0)?, add(x, 2.0)?)?;
            } else {
                id = 1;
                x = div(sub(x, 1.0)?, add(x, 1.0)?)?;
            }
        } else if ix <= 0x401b_ffff {
            id = 2;
            x = div(sub(x, 1.5)?, add(mul(x, 1.5)?, 1.0)?)?;
        } else {
            id = 3;
            x = div(-1.0, x)?;
        }
    }
    let z = mul(x, x)?;
    let ww = mul(z, z)?;
    let mut s1 = add(w(AT[8]), mul(ww, w(AT[10]))?)?;
    let mut s2 = add(w(AT[7]), mul(ww, w(AT[9]))?)?;
    for (odd, even) in [(6, 5), (4, 3), (2, 1)] {
        s1 = add(w(AT[odd]), mul(ww, s1)?)?;
        s2 = add(w(AT[even]), mul(ww, s2)?)?;
    }
    let s2 = mul(ww, s2)?;
    let s1 = mul(z, add(w(AT[0]), mul(ww, s1)?)?)?;
    let product = mul(x, add(s1, s2)?)?;
    if id < 0 {
        return sub(x, product);
    }
    let id = id as usize;
    let z = sub(w(ATANHI[id]), sub(sub(product, w(ATANLO[id]))?, x)?)?;
    Ok(if hx < 0 { -z } else { z })
}

/// atan2f wrapper EE 0x1e7b50 with core EE 0x1e9db0. `_LIB_VERSION` is 1 in
/// the fixture, so finite non-(0,0) arguments return the core result.
pub fn atan2f(y: f32, x: f32) -> Result<f32> {
    let (hy, hx) = (finite(y)?.to_bits() as i32, finite(x)?.to_bits() as i32);
    let (iy, ix) = (hy & 0x7fff_ffff, hx & 0x7fff_ffff);
    if iy == 0 && ix == 0 {
        // The wrapper's matherr path; not part of this subset.
        return Err(UnsupportedNumericDomain);
    }
    if hx == 0x3f80_0000 {
        return atanf(y);
    }
    let m = ((hy as u32 >> 31) | ((hx >> 30) as u32 & 2)) as u8;
    if iy == 0 {
        return Ok(match m {
            0 | 1 => y,
            2 => w(0x4049_0fda),
            _ => w(0xc049_0fda),
        });
    }
    if ix == 0 {
        return Ok(w(if hy >= 0 { 0x3fc9_0fdb } else { 0xbfc9_0fdb }));
    }
    let k = (iy - ix) >> 23;
    let z = if k > 60 {
        w(0x3fc9_0fdc)
    } else if hx < 0 && k < -60 {
        0.0
    } else {
        atanf(fabs(div(y, x)?))?
    };
    let (pi, pi_lo) = (w(0x4049_0fda), w(0x3422_2168));
    match m {
        0 => Ok(z),
        1 => Ok(w(z.to_bits() ^ 0x8000_0000)),
        2 => sub(pi, sub(z, pi_lo)?),
        _ => sub(sub(z, pi_lo)?, pi),
    }
}

/// Kernel EE 0x1ebdf0; `with_tail` corresponds to a nonzero a0 argument.
fn kernel_sin(x: f32, y: f32, with_tail: bool) -> Result<f32> {
    if x.to_bits() & 0x7fff_ffff <= 0x31ff_ffff && x as i32 == 0 {
        return Ok(x);
    }
    let z = mul(x, x)?;
    let v = mul(z, x)?;
    let mut r = mul(z, w(0x2f2e_c9d3))?;
    for c in [0xb2d7_2f34, 0x3638_ef1b, 0xb950_0d01] {
        r = mul(z, add(r, w(c))?)?;
    }
    let r = add(r, w(0x3c08_8889))?;
    let s1 = w(0xbe2a_aaab);
    if !with_tail {
        return add(x, mul(v, add(mul(z, r)?, s1)?)?);
    }
    let inner = sub(sub(mul(z, sub(mul(y, 0.5)?, mul(v, r)?)?)?, y)?, mul(v, s1)?)?;
    sub(x, inner)
}

/// Kernel EE 0x1eb348.
fn kernel_cos(x: f32, y: f32) -> Result<f32> {
    let ix = x.to_bits() & 0x7fff_ffff;
    if ix <= 0x31ff_ffff && x as i32 == 0 {
        return Ok(1.0);
    }
    let z = mul(x, x)?;
    let mut r = mul(z, w(0xad47_d74e))?;
    for c in [0x310f_74f6, 0xb493_f27c, 0x37d0_0d01, 0xbab6_0b61] {
        r = mul(z, add(r, w(c))?)?;
    }
    let r = mul(z, add(r, w(0x3d2a_aaab))?)?;
    let tail = sub(mul(z, r)?, mul(x, y)?)?;
    if ix <= 0x3e99_9999 {
        return sub(1.0, sub(mul(z, 0.5)?, tail)?);
    }
    let qx = w(if ix > 0x3f48_0000 { 0x3e90_0000 } else { ix - 0x0100_0000 });
    let hz = sub(mul(z, 0.5)?, qx)?;
    sub(sub(1.0, qx)?, sub(hz, tail)?)
}

/// Reduction EE 0x1eae30 for |x| <= 0x43490f80; returns n and y[0], y[1].
fn rem_pio2f(x: f32) -> Result<(i32, f32, f32)> {
    let hx = x.to_bits() as i32;
    let ix = hx & 0x7fff_ffff;
    let (pio2_1, pio2_1t) = (w(0x3fc9_0f80), w(0x3735_4443));
    let (pio2_2, pio2_2t) = (w(0x3735_4400), w(0x2e85_a308));
    let (pio2_3, pio2_3t) = (w(0x2e85_a300), w(0x248d_3132));
    if ix <= 0x3f49_0fd8 {
        return Ok((0, x, 0.0));
    }
    if ix <= 0x4016_cbe3 {
        let near = (ix as u32 & 0xffff_fff0) == 0x3fc9_0fd0;
        return if hx > 0 {
            let mut z = sub(x, pio2_1)?;
            let t = if near {
                z = sub(z, pio2_2)?;
                pio2_2t
            } else {
                pio2_1t
            };
            let y0 = sub(z, t)?;
            Ok((1, y0, sub(sub(z, y0)?, t)?))
        } else {
            let mut z = add(x, pio2_1)?;
            let t = if near {
                z = add(z, pio2_2)?;
                pio2_2t
            } else {
                pio2_1t
            };
            let y0 = add(z, t)?;
            Ok((-1, y0, add(sub(z, y0)?, t)?))
        };
    }
    if ix > 0x4349_0f80 {
        return Err(UnsupportedNumericDomain);
    }
    let t = fabs(x);
    // cvt.w.S truncates toward zero; the operand is small and positive here.
    let n = add(mul(t, w(0x3f22_f984))?, 0.5)? as i32;
    let fn_ = n as f32;
    let mut r = sub(t, mul(fn_, pio2_1)?)?;
    let mut w_ = mul(fn_, pio2_1t)?;
    let mut y0 = sub(r, w_)?;
    if !(n < 32 && ix as u32 & 0xffff_ff00 != NPIO2_HW[(n - 1) as usize]) {
        let j = ix >> 23;
        let exponent = |v: f32| ((v.to_bits() >> 23) & 0xff) as i32;
        for (limit, high, low) in [(8, pio2_2, pio2_2t), (25, pio2_3, pio2_3t)] {
            if j - exponent(y0) <= limit {
                break;
            }
            let previous = r;
            w_ = mul(fn_, high)?;
            r = sub(previous, w_)?;
            w_ = sub(mul(fn_, low)?, sub(sub(previous, r)?, w_)?)?;
            y0 = sub(r, w_)?;
        }
    }
    let y1 = sub(sub(r, y0)?, w_)?;
    Ok(if hx < 0 { (-n, -y0, -y1) } else { (n, y0, y1) })
}

/// __kernel_tanf EE 0x1ebef8 coefficients T0..T12 (EE 0x2b9610).
const TAN_T: [u32; 13] = [
    0x3eaaaaab, 0x3e088889, 0x3d5d0dd1, 0x3cb327a4, 0x3c11371f, 0x3b6b6916, 0x3abede48, 0x3a1a26c8,
    0x398137b9, 0x38a3f445, 0x3895c07a, 0xb79bae5f, 0x37d95384,
];

/// Kernel EE 0x1ebef8: tan(x + y) for |x| <= π/4 (`iy` 1) or −1/tan (`iy` −1).
fn kernel_tan(mut x: f32, mut y: f32, iy: i32) -> Result<f32> {
    let hx = x.to_bits() as i32;
    let ix = hx & 0x7fff_ffff;
    if ix <= 0x317f_ffff && x as i32 == 0 {
        if (ix | (iy + 1)) == 0 {
            // 1/fabsf(0): the EE divide-by-zero result is not modelled.
            return div(1.0, fabs(x));
        }
        return if iy == 1 { Ok(x) } else { div(-1.0, x) };
    }
    let big = ix > 0x3f2c_a13f;
    if big {
        if hx < 0 {
            x = -x;
            y = -y;
        }
        let z = sub(w(0x3f49_0fda), x)?;
        let lo = sub(w(0x3322_2168), y)?;
        x = add(z, lo)?;
        y = 0.0;
    }
    let t = TAN_T.map(w);
    let z = mul(x, x)?;
    let ww = mul(z, z)?;
    let s = mul(z, x)?;
    let poly = |hi: [usize; 5], last: usize| -> Result<f32> {
        let mut acc = add(t[hi[4]], mul(ww, t[last])?)?;
        for &k in hi[..4].iter().rev() {
            acc = add(t[k], mul(ww, acc)?)?;
        }
        Ok(acc)
    };
    let r = poly([1, 3, 5, 7, 9], 11)?;
    let v = poly([2, 4, 6, 8, 10], 12)?;
    let inner = add(mul(s, add(r, mul(z, v)?)?)?, y)?;
    let r = add(add(y, mul(z, inner)?)?, mul(t[0], s)?)?;
    let wv = add(x, r)?;
    if big {
        let fv = iy as f32;
        let sign = (1 - ((hx >> 30) & 2)) as f32;
        let q = sub(div(mul(wv, wv)?, add(wv, fv)?)?, r)?;
        let d = sub(x, q)?;
        return mul(sign, sub(fv, add(d, d)?)?);
    }
    if iy == 1 {
        return Ok(wv);
    }
    let trunc = |v: f32| w(v.to_bits() & 0xffff_f000);
    let zz = trunc(wv);
    let vv = sub(r, sub(zz, x)?)?;
    let a = div(-1.0, wv)?;
    let tt = trunc(a);
    let ss = add(mul(tt, zz)?, 1.0)?;
    add(tt, mul(a, add(ss, mul(tt, vv)?)?)?)
}

/// tanf at EE 0x1e7440.
pub fn tanf(x: f32) -> Result<f32> {
    if finite(x)?.to_bits() & 0x7fff_ffff <= 0x3f49_0fda {
        return kernel_tan(x, 0.0, 1);
    }
    let (n, y0, y1) = rem_pio2f(x)?;
    kernel_tan(y0, y1, 1 - ((n & 1) << 1))
}

/// sinf at EE 0x1e7350.
pub fn sinf(x: f32) -> Result<f32> {
    if finite(x)?.to_bits() & 0x7fff_ffff <= 0x3f49_0fd8 {
        return kernel_sin(x, 0.0, false);
    }
    let (n, y0, y1) = rem_pio2f(x)?;
    match n & 3 {
        0 => kernel_sin(y0, y1, true),
        1 => kernel_cos(y0, y1),
        2 => Ok(-kernel_sin(y0, y1, true)?),
        _ => Ok(-kernel_cos(y0, y1)?),
    }
}

/// cosf at EE 0x1e7160.
pub fn cosf(x: f32) -> Result<f32> {
    if finite(x)?.to_bits() & 0x7fff_ffff <= 0x3f49_0fd8 {
        return kernel_cos(x, 0.0);
    }
    let (n, y0, y1) = rem_pio2f(x)?;
    match n & 3 {
        0 => kernel_cos(y0, y1),
        1 => Ok(-kernel_sin(y0, y1, true)?),
        2 => Ok(-kernel_cos(y0, y1)?),
        _ => kernel_sin(y0, y1, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Synthetic sanity only: closeness to host f64 does not establish reference
    // fidelity, which comes from the owned-data movement comparisons.
    #[test]
    fn subset_stays_within_a_few_ulps_of_host_math_over_movement_angles() {
        let ulps = |a: f32, b: f64| ((a as f64 - b).abs() / (b.abs().max(1e-30) * f64::from(f32::EPSILON))) as u32;
        for i in -400..=400 {
            let x = i as f32 * 0.0125;
            if x != 0.0 {
                assert!(ulps(sinf(x).unwrap(), (x as f64).sin()) <= 4 || (x as f64).sin().abs() < 1e-3, "sin {x}");
                assert!(ulps(cosf(x).unwrap(), (x as f64).cos()) <= 4 || (x as f64).cos().abs() < 1e-3, "cos {x}");
            }
            for y in [-0.9f32, -0.38, 0.0, 0.25, 1.0, 3.0] {
                if x == 0.0 && y == 0.0 {
                    assert!(atan2f(y, x).is_err());
                    continue;
                }
                let expected = (y as f64).atan2(x as f64);
                assert!(ulps(atan2f(y, x).unwrap(), expected) <= 4 || expected.abs() < 1e-3, "atan2 {y} {x}");
            }
        }
    }
    #[test]
    fn exact_branch_constants_and_rejections() {
        assert_eq!(atan2f(0.0, -1.0).unwrap().to_bits(), 0x4049_0fda);
        assert_eq!(atan2f(-0.0, -1.0).unwrap().to_bits(), 0xc049_0fda);
        assert_eq!(atan2f(1.0, 0.0).unwrap().to_bits(), 0x3fc9_0fdb);
        assert_eq!(atan2f(-1.0, -0.0).unwrap().to_bits(), 0xbfc9_0fdb);
        assert_eq!(atanf(1e20).unwrap(), add(w(ATANHI[3]), w(ATANLO[3])).unwrap());
        assert_eq!(sinf(1e-20).unwrap(), 1e-20);
        assert_eq!(cosf(1e-20).unwrap(), 1.0);
        assert!(sinf(f32::NAN).is_err());
        assert!(cosf(1000.0).is_err());
        assert!(atan2f(f32::INFINITY, 1.0).is_err());
    }
}

/// acosf: the wrapper at EE 0x1e7a50 rejects |x| > 1 (matherr path); the
/// kernel at 0x1e9980 (operation order and literal words from the
/// instructions) uses the integer sqrtf at 0x1eb210, which returns the
/// round-to-nearest root for the normal arguments reached here.
pub fn acosf(x: f32) -> Result<f32> {
    finite(x)?;
    const PS: [u32; 6] = [0x3e2aaaab, 0xbea6b090, 0x3e4e0aa8, 0xbd241146, 0x3a4f7f04, 0x3811ef08];
    const QS: [u32; 4] = [0xc019d139, 0x4001572d, 0xbf303361, 0x3d9dc62e];
    let pio2_hi = w(0x3fc90fda);
    let pio2_lo = w(0x33a22168);
    let pi = w(0x40490fda);
    let ix = x.to_bits() & 0x7fff_ffff;
    if ix == 0x3f80_0000 {
        return Ok(if 0 < x.to_bits() as i32 { 0.0 } else { w(0x40490fdb) });
    }
    if 0x3f80_0000 < ix {
        return Err(UnsupportedNumericDomain);
    }
    let pq = |z: f32| -> Result<(f32, f32)> {
        let mut p = add(mul(z, w(PS[5]))?, w(PS[4]))?;
        for k in (0..4).rev() {
            p = add(mul(z, p)?, w(PS[k]))?;
        }
        let p = mul(z, p)?;
        let mut q = add(mul(z, w(QS[3]))?, w(QS[2]))?;
        q = add(mul(z, q)?, w(QS[1]))?;
        q = add(mul(z, q)?, w(QS[0]))?;
        let q = add(mul(z, q)?, 1.0)?;
        Ok((p, q))
    };
    let sqrtf = |z: f32| crate::numeric::sqrt_rn(z);
    if ix < 0x3f00_0000 {
        if ix <= 0x2300_0000 {
            return Ok(w(0x3fc90fdb));
        }
        let z = mul(x, x)?;
        let (p, q) = pq(z)?;
        let r = div(p, q)?;
        return sub(pio2_hi, sub(x, sub(pio2_lo, mul(x, r)?)?)?);
    }
    if (x.to_bits() as i32) < 0 {
        let z = mul(add(x, 1.0)?, 0.5)?;
        let (p, q) = pq(z)?;
        let s = sqrtf(z)?;
        let r = div(p, q)?;
        let wv = sub(mul(r, s)?, pio2_lo)?;
        let t = add(s, wv)?;
        return sub(pi, add(t, t)?);
    }
    let z = mul(sub(1.0, x)?, 0.5)?;
    let s = sqrtf(z)?;
    let df = w(s.to_bits() & 0xffff_f000);
    let c = div(sub(z, mul(df, df)?)?, add(s, df)?)?;
    let (p, q) = pq(z)?;
    let r = div(p, q)?;
    let wv = add(mul(r, s)?, c)?;
    let t = add(df, wv)?;
    add(t, t)
}

#[cfg(test)]
mod acos_tests {
    #[test]
    fn acosf_tracks_the_host_closely() {
        // Synthetic: the port against f64 acos within a few ulps.
        for x in [-0.99f32, -0.7, -0.3, 0.0, 0.2, 0.49, 0.5, 0.8, 0.999] {
            let a = super::acosf(x).unwrap();
            assert!((f64::from(a) - f64::from(x).acos()).abs() < 1e-6, "{x}");
        }
        assert!(super::acosf(1.5).is_err());
        assert_eq!(super::acosf(1.0).unwrap(), 0.0);
    }
}
