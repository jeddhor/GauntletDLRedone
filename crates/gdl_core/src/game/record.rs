//! Original-layout records (player 0x2b00, enemy 0x3b0). Guest pointers are
//! replaced field by field with documented native indices; offsets in the
//! game modules cite the original record directly.
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record(#[serde(with = "hex_bytes")] pub Vec<u8>);

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.iter().map(|b| format!("{b:02x}")).collect::<String>())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() % 2 != 0 {
            return Err(serde::de::Error::custom("odd hex length"));
        }
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(serde::de::Error::custom))
            .collect()
    }
}

macro_rules! accessors {
    ($($get:ident, $set:ident, $t:ty;)*) => {$(
        #[inline]
        pub fn $get(&self, o: usize) -> $t {
            <$t>::from_le_bytes(self.0[o..o + std::mem::size_of::<$t>()].try_into().unwrap())
        }
        #[inline]
        pub fn $set(&mut self, o: usize, v: $t) {
            self.0[o..o + std::mem::size_of::<$t>()].copy_from_slice(&v.to_le_bytes());
        }
    )*};
}

impl Record {
    pub fn zeroed(size: usize) -> Self {
        Self(vec![0; size])
    }
    accessors! {
        u8_, set_u8, u8; i8_, set_i8, i8; u16_, set_u16, u16; i16_, set_i16, i16;
        u32_, set_u32, u32; i32_, set_i32, i32; f32_, set_f32, f32;
    }
    pub fn v3(&self, o: usize) -> [f32; 3] {
        [self.f32_(o), self.f32_(o + 4), self.f32_(o + 8)]
    }
    pub fn set_v3(&mut self, o: usize, v: [f32; 3]) {
        for (i, x) in v.iter().enumerate() {
            self.set_f32(o + 4 * i, *x);
        }
    }
    pub fn or_u16(&mut self, o: usize, v: u16) {
        self.set_u16(o, self.u16_(o) | v);
    }
    pub fn and_u16(&mut self, o: usize, v: u16) {
        self.set_u16(o, self.u16_(o) & v);
    }
    pub fn or_u32(&mut self, o: usize, v: u32) {
        self.set_u32(o, self.u32_(o) | v);
    }
    pub fn and_u32(&mut self, o: usize, v: u32) {
        self.set_u32(o, self.u32_(o) & v);
    }
}

impl std::fmt::Debug for Record {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Record({} bytes)", self.0.len())
    }
}

pub const PLAYER: usize = 0x2b00;
pub const ENEMY: usize = 0x3b0;
