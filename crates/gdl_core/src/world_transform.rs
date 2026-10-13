//! Source-derived finite GetWorldMat/MulMat4 experiment. Not accepted renderer
//! composition until actual guest world outputs and scale consumers are compared.
use crate::numeric::{UnsupportedNumericDomain, add_guarded_rz, add_rz, mul_rz};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixBox {
    pub matrix: [f32; 16],
    pub flags: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorldMatrix {
    pub matrix: [f32; 16],
    pub translation_only: bool,
}

/// Original MulMat4 (EE0x1d55a0): force both input W words to1, multiply/add
/// separate VU operations in column order. Source input-write side effects are
/// exposed through mutable arguments. Output aliasing does not affect the formula.
pub fn multiply(
    left: &mut [f32; 16],
    right: &mut [f32; 16],
) -> Result<[f32; 16], UnsupportedNumericDomain> {
    left[15] = 1.0;
    right[15] = 1.0;
    let mut out = [0.0; 16];
    for column in 0..4 {
        for row in 0..4 {
            let mut value = mul_rz(left[row], right[column * 4])?;
            for term in 1..4 {
                let product = mul_rz(left[term * 4 + row], right[column * 4 + term])?;
                value = add_rz(value, product)?;
            }
            out[column * 4 + row] = value;
        }
    }
    Ok(out)
}

/// Source parent order is immediate parent through outermost ancestor. Each
/// ancestor multiplies the current result, rather than a cached parent-world
/// matrix; reassociating those products changes rounding. No scale-vector
/// inheritance is inferred from this function. Chain validation is caller-owned.
pub fn compose(
    leaf: MatrixBox,
    parents: &mut [MatrixBox],
    offset: Option<[f32; 3]>,
) -> Result<WorldMatrix, UnsupportedNumericDomain> {
    if parents.len() > 128 || leaf.matrix.iter().any(|v| *v != 0.0 && !v.is_normal()) {
        return Err(UnsupportedNumericDomain);
    }
    let mut matrix = leaf.matrix;
    if let Some(offset) = offset {
        for axis in 0..3 {
            matrix[12 + axis] = add_guarded_rz(matrix[12 + axis], offset[axis])?;
        }
    }
    let mut translation_only = leaf.flags & 4 != 0;
    for parent in parents {
        if parent.flags & 4 != 0 {
            for axis in 0..3 {
                matrix[12 + axis] = add_guarded_rz(parent.matrix[12 + axis], matrix[12 + axis])?;
            }
        } else {
            matrix = multiply(&mut parent.matrix, &mut matrix)?;
            translation_only = false;
        }
    }
    Ok(WorldMatrix {
        matrix,
        translation_only,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity() -> [f32; 16] {
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    }
    #[test]
    fn original_w_writes_and_translation_only_return_are_preserved() {
        let mut left = identity();
        left[15] = 7.0;
        let mut right = identity();
        right[15] = 9.0;
        multiply(&mut left, &mut right).unwrap();
        assert_eq!(left[15], 1.0);
        assert_eq!(right[15], 1.0);
        let mut parent = MatrixBox {
            matrix: identity(),
            flags: 4,
        };
        parent.matrix[12] = 3.0;
        let leaf = MatrixBox {
            matrix: identity(),
            flags: 4,
        };
        let world = compose(leaf, &mut [parent], Some([2.0, 0.0, 0.0])).unwrap();
        assert_eq!(world.matrix[12], 5.0);
        assert!(world.translation_only);
        parent.flags = 0;
        assert!(!compose(leaf, &mut [parent], None).unwrap().translation_only);
    }
}
