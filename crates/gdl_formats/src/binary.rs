use crate::wad::ParseError;
use std::ops::Range;

pub(crate) struct Reader<'a> {
    pub bytes: &'a [u8],
    pub family: &'static str,
}
impl Reader<'_> {
    pub fn error(&self, offset: usize, reason: impl Into<String>) -> ParseError {
        ParseError {
            entry_index: None,
            byte_offset: offset,
            reason: format!("{}: {}", self.family, reason.into()),
        }
    }
    pub fn range(&self, offset: usize, size: usize) -> Result<Range<usize>, ParseError> {
        let end = offset
            .checked_add(size)
            .ok_or_else(|| self.error(offset, "range overflow"))?;
        if end > self.bytes.len() {
            return Err(self.error(offset, "range exceeds input"));
        }
        Ok(offset..end)
    }
    pub fn table(
        &self,
        pointer: u32,
        count: u32,
        stride: usize,
        header: usize,
    ) -> Result<Range<usize>, ParseError> {
        if count > 50_000 {
            return Err(self.error(pointer as usize, "table count policy exceeded"));
        }
        if count != 0 && ((pointer as usize) < header || !pointer.is_multiple_of(4)) {
            return Err(self.error(pointer as usize, "table intersects header or is unaligned"));
        }
        self.range(pointer as usize, count as usize * stride)
    }
    // Callers establish a containing range before these field reads.
    pub fn u16(&self, p: usize) -> u16 {
        u16::from_le_bytes(self.bytes[p..p + 2].try_into().unwrap())
    }
    pub fn i16(&self, p: usize) -> i16 {
        self.u16(p) as i16
    }
    pub fn u32(&self, p: usize) -> u32 {
        u32::from_le_bytes(self.bytes[p..p + 4].try_into().unwrap())
    }
    pub fn i32(&self, p: usize) -> i32 {
        self.u32(p) as i32
    }
    pub fn float(&self, p: usize) -> Result<f32, ParseError> {
        let value = f32::from_bits(self.u32(p));
        if !value.is_finite() {
            return Err(self.error(p, "non-finite float"));
        }
        Ok(value)
    }
    pub fn xyz(&self, p: usize) -> Result<[f32; 3], ParseError> {
        Ok([self.float(p)?, self.float(p + 4)?, self.float(p + 8)?])
    }
    pub fn label(&self, p: usize, size: usize) -> String {
        self.bytes[p..p + size]
            .iter()
            .take_while(|b| **b != 0)
            .map(|b| char::from(*b))
            .collect()
    }
}
