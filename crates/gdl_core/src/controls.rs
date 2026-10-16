//! PlayerControls 0x105718 digital-direction subset: the game's level word
//! selects JoyAng/JoyMag entries built by InitJoyAng 0x105448. `magval`
//! (EE 0x2113e8) is read from the user's ELF; no table values are embedded.
use crate::libm::atan2f;
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sqrt_rn, sub_guarded_rz as sub, UnsupportedNumericDomain};

/// angidx (EE 0x211400, ELF data): nibble → table row/column.
pub type AngleIndex = [i32; 16];

#[derive(Debug, Clone)]
pub struct JoyTables {
    angle: [f32; 49],
    magnitude: [f32; 49],
    index: AngleIndex,
}

impl JoyTables {
    /// InitJoyAng: rows x = -3..3, columns y = 3..-3; axis cases take the
    /// original literal constants, others atan2f(y, x).
    pub fn build(magval: &[f32], index: AngleIndex) -> Result<Self, UnsupportedNumericDomain> {
        let mut angle = [0.0; 49];
        let mut magnitude = [0.0; 49];
        let mut x = -3.0f32;
        for row in 0..7 {
            let mut y = 3.0f32;
            for col in 0..7 {
                let a = if x == 0.0 && y == 0.0 {
                    0.0
                } else if x == 0.0 {
                    f32::from_bits(if 0.0 < y { 0x3fc9_0fdb } else { 0xbfc9_0fdb })
                } else if y == 0.0 {
                    if 0.0 < x { 0.0 } else { f32::from_bits(0x4049_0fdb) }
                } else {
                    atan2f(y, x)?
                };
                angle[row * 7 + col] = a;
                let radius = sqrt_rn(add(mul(x, x)?, mul(y, y)?)?)? as i32;
                magnitude[row * 7 + col] =
                    *magval.get(radius as usize).ok_or(UnsupportedNumericDomain)?;
                y = sub(y, 1.0)?;
            }
            x = add(x, 1.0)?;
        }
        Ok(Self { angle, magnitude, index })
    }

    pub fn tables(&self) -> (&[f32; 49], &[f32; 49]) {
        (&self.angle, &self.magnitude)
    }

    /// Control angle +0x1c and magnitude +0x20 for a digital level word.
    pub fn digital(&self, levels: u32) -> (f32, f32) {
        let col = self.index[(levels & 0xf) as usize] as usize;
        let row = self.index[((levels & 0xf0) >> 4) as usize] as usize;
        let i = (row * 7 + col).min(48);
        (self.angle[i], self.magnitude[i])
    }
}

/// Builds the tables from the user's ELF data objects `magval` and `angidx`.
pub fn tables_from_elf(elf: &gdl_formats::elf::Elf) -> Result<JoyTables, Box<dyn std::error::Error>> {
    let magval: Vec<f32> = elf
        .object("magval")?
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let raw = elf.object("angidx")?;
    if raw.len() != 64 {
        return Err("angidx size differs from the supported executable".into());
    }
    let index: AngleIndex = std::array::from_fn(|i| i32::from_le_bytes(raw[4 * i..4 * i + 4].try_into().unwrap()));
    if index.iter().any(|v| !(0..7).contains(v)) {
        return Err("angidx entry outside the 7×7 table".into());
    }
    Ok(JoyTables::build(&magval, index)?)
}

/// Level word for held digital directions, as observed in reference control
/// records: Left 0x10000003, Right 0x2000000c, Up 0x40000030, Down 0x800000c0.
pub fn direction_levels(up: bool, down: bool, left: bool, right: bool) -> u32 {
    let mut levels = 0;
    if left {
        levels |= 0x1000_0003;
    }
    if right {
        levels |= 0x2000_000c;
    }
    if up {
        levels |= 0x4000_0030;
    }
    if down {
        levels |= 0x8000_00c0;
    }
    levels
}
