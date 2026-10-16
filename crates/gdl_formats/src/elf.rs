//! Bounded little-endian ELF32 reader for the user's executable: section and
//! symbol tables plus reads from file-backed PT_LOAD ranges. Used to take small
//! original data tables from the supplied ELF at run time instead of embedding
//! game data in source.
use crate::{binary::Reader, wad::ParseError};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct Symbol {
    pub address: u32,
    pub size: u32,
    pub kind: u8,
}

#[derive(Debug, Clone, Copy)]
struct Segment {
    offset: u32,
    address: u32,
    file_size: u32,
}

pub struct Elf<'a> {
    bytes: &'a [u8],
    segments: Vec<Segment>,
    symbols: HashMap<String, Symbol>,
}

impl<'a> Elf<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ParseError> {
        let r = Reader { bytes, family: "ELF" };
        r.range(0, 0x34)?;
        if bytes[..4] != *b"\x7fELF" || bytes[4] != 1 || bytes[5] != 1 || bytes.len() > 64 * 1024 * 1024 {
            return Err(r.error(0, "not a supported little-endian ELF32 file"));
        }
        let (phoff, phentsize, phnum) = (r.u32(0x1c) as usize, r.u16(0x2a) as usize, r.u16(0x2c) as usize);
        let (shoff, shentsize, shnum) = (r.u32(0x20) as usize, r.u16(0x2e) as usize, r.u16(0x30) as usize);
        if phentsize < 32 || shentsize < 40 || phnum > 64 || shnum > 1024 {
            return Err(r.error(0x2a, "ELF header table sizes outside policy"));
        }
        let mut segments = Vec::new();
        for i in 0..phnum {
            let p = phoff + i * phentsize;
            r.range(p, 32)?;
            if r.u32(p) == 1 {
                let segment = Segment { offset: r.u32(p + 4), address: r.u32(p + 8), file_size: r.u32(p + 16) };
                r.range(segment.offset as usize, segment.file_size as usize)?;
                segments.push(segment);
            }
        }
        let mut sections = Vec::new();
        for i in 0..shnum {
            let p = shoff + i * shentsize;
            r.range(p, 40)?;
            sections.push((r.u32(p + 4), r.u32(p + 16) as usize, r.u32(p + 20) as usize, r.u32(p + 24) as usize));
        }
        let mut symbols = HashMap::new();
        for &(kind, offset, size, link) in &sections {
            if kind != 2 {
                continue;
            }
            let &(_, str_offset, str_size, _) =
                sections.get(link).ok_or_else(|| r.error(offset, "symbol string table missing"))?;
            r.range(offset, size)?;
            r.range(str_offset, str_size)?;
            if size / 16 > 200_000 {
                return Err(r.error(offset, "symbol count policy exceeded"));
            }
            for p in (offset..offset + size / 16 * 16).step_by(16) {
                let name_offset = r.u32(p) as usize;
                if name_offset >= str_size {
                    return Err(r.error(p, "symbol name outside string table"));
                }
                let start = str_offset + name_offset;
                let end = bytes[start..str_offset + str_size]
                    .iter()
                    .position(|b| *b == 0)
                    .map(|n| start + n)
                    .ok_or_else(|| r.error(p, "unterminated symbol name"))?;
                let name = String::from_utf8_lossy(&bytes[start..end]).into_owned();
                symbols
                    .entry(name)
                    .or_insert(Symbol { address: r.u32(p + 4), size: r.u32(p + 8), kind: bytes[p + 12] & 15 });
            }
        }
        Ok(Self { bytes, segments, symbols })
    }

    pub fn symbol(&self, name: &str) -> Option<Symbol> {
        self.symbols.get(name).copied()
    }

    /// Bytes at a guest address inside one file-backed load segment.
    pub fn read(&self, address: u32, size: u32) -> Result<&'a [u8], ParseError> {
        let r = Reader { bytes: self.bytes, family: "ELF" };
        for s in &self.segments {
            if s.address <= address && u64::from(address) + u64::from(size) <= u64::from(s.address) + u64::from(s.file_size) {
                let start = (s.offset + (address - s.address)) as usize;
                return Ok(&self.bytes[r.range(start, size as usize)?]);
            }
        }
        Err(r.error(0, format!("address {address:#x} is not file-backed")))
    }

    /// A data object symbol's bytes, requiring its recorded size.
    pub fn object(&self, name: &str) -> Result<&'a [u8], ParseError> {
        let r = Reader { bytes: self.bytes, family: "ELF" };
        let s = self.symbol(name).ok_or_else(|| r.error(0, format!("missing symbol {name}")))?;
        if s.kind != 1 || s.size == 0 {
            return Err(r.error(0, format!("{name} is not a sized data object")));
        }
        self.read(s.address, s.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn elf() -> Vec<u8> {
        // Header, one PT_LOAD at file 0x100 → address 0x1000 (16 bytes), a
        // symtab with one sized object symbol "tab" at 0x1004 (8 bytes).
        let mut b = vec![0u8; 0x400];
        b[..4].copy_from_slice(b"\x7fELF");
        b[4] = 1;
        b[5] = 1;
        b[0x1c..0x20].copy_from_slice(&0x34u32.to_le_bytes());
        b[0x2a..0x2c].copy_from_slice(&32u16.to_le_bytes());
        b[0x2c..0x2e].copy_from_slice(&1u16.to_le_bytes());
        b[0x20..0x24].copy_from_slice(&0x200u32.to_le_bytes());
        b[0x2e..0x30].copy_from_slice(&40u16.to_le_bytes());
        b[0x30..0x32].copy_from_slice(&3u16.to_le_bytes());
        let ph = 0x34;
        b[ph..ph + 4].copy_from_slice(&1u32.to_le_bytes());
        b[ph + 4..ph + 8].copy_from_slice(&0x100u32.to_le_bytes());
        b[ph + 8..ph + 12].copy_from_slice(&0x1000u32.to_le_bytes());
        b[ph + 16..ph + 20].copy_from_slice(&16u32.to_le_bytes());
        b[0x104..0x10c].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        // section 1: symtab at 0x300 (2 entries), link 2; section 2: strtab at 0x380.
        let s1 = 0x200 + 40;
        b[s1 + 4..s1 + 8].copy_from_slice(&2u32.to_le_bytes());
        b[s1 + 16..s1 + 20].copy_from_slice(&0x300u32.to_le_bytes());
        b[s1 + 20..s1 + 24].copy_from_slice(&32u32.to_le_bytes());
        b[s1 + 24..s1 + 28].copy_from_slice(&2u32.to_le_bytes());
        let s2 = 0x200 + 80;
        b[s2 + 4..s2 + 8].copy_from_slice(&3u32.to_le_bytes());
        b[s2 + 16..s2 + 20].copy_from_slice(&0x380u32.to_le_bytes());
        b[s2 + 20..s2 + 24].copy_from_slice(&8u32.to_le_bytes());
        b[0x380..0x385].copy_from_slice(b"\0tab\0");
        let sym = 0x310;
        b[sym..sym + 4].copy_from_slice(&1u32.to_le_bytes());
        b[sym + 4..sym + 8].copy_from_slice(&0x1004u32.to_le_bytes());
        b[sym + 8..sym + 12].copy_from_slice(&8u32.to_le_bytes());
        b[sym + 12] = 1;
        b
    }
    #[test]
    fn symbols_resolve_to_file_backed_bytes_and_bounds_fail() {
        let b = elf();
        let e = Elf::parse(&b).unwrap();
        assert_eq!(e.object("tab").unwrap(), &[1, 2, 3, 4, 5, 6, 7, 8]);
        assert!(e.read(0x100c, 8).is_err());
        assert!(e.object("missing").is_err());
        let mut bad = b.clone();
        bad[0x384] = b'x';
        bad[0x385..0x388].copy_from_slice(b"yyy");
        assert!(Elf::parse(&bad[..0x388]).is_err());
    }
}
