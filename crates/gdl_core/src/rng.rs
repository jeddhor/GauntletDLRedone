//! The executable's random source: newlib rand 0x1fb780 (state × 0x41c64e6d +
//! 0x3039, result & 0x7fffffff), pbRand 0x1aedc8 = rand, RandInt 0x1d6b50 =
//! rand() % n (unsigned). The original reseeds from CPU time every main loop
//! (srand), so native play matches the algorithm, not captured values; the
//! seed policy is a documented port decision.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rand {
    pub state: u32,
}

impl Rand {
    pub fn new(seed: u32) -> Self {
        Self { state: seed }
    }
    pub fn rand(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(0x41c6_4e6d).wrapping_add(0x3039);
        self.state & 0x7fff_ffff
    }
    /// RandInt; `n == 0` traps (break 7) in the original.
    pub fn rand_int(&mut self, n: u32) -> Option<u32> {
        let r = self.rand();
        (n != 0).then(|| r % n)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn newlib_sequence() {
        let mut r = super::Rand::new(1);
        // 1 × 0x41c64e6d + 0x3039 = 0x41c67ea6.
        assert_eq!(r.rand(), 0x41c6_7ea6);
        assert_eq!(r.state, 0x41c6_7ea6);
    }
}
