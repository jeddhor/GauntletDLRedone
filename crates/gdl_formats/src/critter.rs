//! Critter (boss/statue) description WADs (CRITTER/*.WAD): MB container read
//! by CritterInitHeader 0x191978 through MBGetFromWad with the tags SFXX,
//! DAMG, MOVE, PTRN, NODE, DESC, TYPE and ADDA. Record sizes come from the
//! consumers' strides: TYPE 0x140 (CritterLoadAllTypes), DESC 0x30 and ADDA
//! 0x30 (CritterInitHeader), MOVE 0x90 (CritterInitMoves), PTRN/NODE/DAMG/SFXX
//! 0x50 (CritterInitMoves, CritterInitDamage, CritterInitSfx). Records are kept
//! as raw bytes; unused bytes carry authoring-tool text residue that is data,
//! not semantics. Field meanings live with their consumers in gdl_core.
use crate::{binary::Reader, wad::ParseError};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Chunk {
    pub tag: String,
    pub offset: u32,
    pub count: u32,
    pub word_0c: u32,
}

/// One critter description file.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CritterWad {
    pub chunks: Vec<Chunk>,
    #[serde(skip)]
    pub types: Vec<Vec<u8>>,
    #[serde(skip)]
    pub descs: Vec<Vec<u8>>,
    #[serde(skip)]
    pub addas: Vec<Vec<u8>>,
    #[serde(skip)]
    pub nodes: Vec<Vec<u8>>,
    #[serde(skip)]
    pub moves: Vec<Vec<u8>>,
    #[serde(skip)]
    pub patterns: Vec<Vec<u8>>,
    #[serde(skip)]
    pub damages: Vec<Vec<u8>>,
    #[serde(skip)]
    pub sfx: Vec<Vec<u8>>,
}

pub const TYPE_SIZE: usize = 0x140;
pub const DESC_SIZE: usize = 0x30;
pub const ADDA_SIZE: usize = 0x30;
pub const NODE_SIZE: usize = 0x50;
pub const MOVE_SIZE: usize = 0x90;
pub const PTRN_SIZE: usize = 0x50;
pub const DAMG_SIZE: usize = 0x50;
pub const SFXX_SIZE: usize = 0x50;

impl CritterWad {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let r = Reader { bytes, family: "CRITTER" };
        r.range(0, 8)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(r.error(0, "critter data size policy exceeded"));
        }
        let (directory, count) = (r.u32(0) as usize, r.u32(4));
        if count > 64 {
            return Err(r.error(4, "chunk count policy exceeded"));
        }
        let mut out = Self::default();
        for i in 0..count as usize {
            let p = directory + 16 * i;
            r.range(p, 16)?;
            let tag = String::from_utf8_lossy(&r.u32(p).to_be_bytes()).into_owned();
            let chunk = Chunk { tag, offset: r.u32(p + 4), count: r.u32(p + 8), word_0c: r.u32(p + 12) };
            let size = match chunk.tag.as_str() {
                "TYPE" => TYPE_SIZE,
                "DESC" => DESC_SIZE,
                "ADDA" => ADDA_SIZE,
                "NODE" => NODE_SIZE,
                "MOVE" => MOVE_SIZE,
                "PTRN" => PTRN_SIZE,
                "DAMG" => DAMG_SIZE,
                "SFXX" => SFXX_SIZE,
                _ => {
                    out.chunks.push(chunk);
                    continue;
                }
            };
            if chunk.count > 4096 {
                return Err(r.error(p + 8, "record count policy exceeded"));
            }
            let base = chunk.offset as usize;
            r.range(base, size * chunk.count as usize)?;
            let records: Vec<Vec<u8>> = (0..chunk.count as usize).map(|k| bytes[base + k * size..base + (k + 1) * size].to_vec()).collect();
            match chunk.tag.as_str() {
                "TYPE" => out.types = records,
                "DESC" => out.descs = records,
                "ADDA" => out.addas = records,
                "NODE" => out.nodes = records,
                "MOVE" => out.moves = records,
                "PTRN" => out.patterns = records,
                "DAMG" => out.damages = records,
                _ => out.sfx = records,
            }
            out.chunks.push(chunk);
        }
        if out.types.is_empty() {
            // CritterInitHeader: FatalErrorC when TYPE is missing.
            return Err(r.error(0, "critter WAD without TYPE"));
        }
        Ok(out)
    }
}

/// NUL-terminated label of at most `size` bytes.
pub fn label(record: &[u8], offset: usize, size: usize) -> String {
    let field = &record[offset..offset + size];
    let end = field.iter().position(|&b| b == 0).unwrap_or(size);
    String::from_utf8_lossy(&field[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_synthetic_container() {
        // Synthetic: one TYPE record and one MOVE record, directory at 0x200.
        let mut b = vec![0u8; 0x200 + 32];
        b[0..4].copy_from_slice(&0x200u32.to_le_bytes());
        b[4..8].copy_from_slice(&2u32.to_le_bytes());
        let dir = |b: &mut Vec<u8>, i: usize, tag: &[u8; 4], off: u32, n: u32| {
            let p = 0x200 + 16 * i;
            b[p..p + 4].copy_from_slice(&u32::from_be_bytes(*tag).to_le_bytes());
            b[p + 4..p + 8].copy_from_slice(&off.to_le_bytes());
            b[p + 8..p + 12].copy_from_slice(&n.to_le_bytes());
            b[p + 12..p + 16].copy_from_slice(&n.to_le_bytes());
        };
        dir(&mut b, 0, b"TYPE", 0x10, 1);
        dir(&mut b, 1, b"MOVE", 0x150, 1);
        b[0x10 + 0x20..0x10 + 0x24].copy_from_slice(b"HEAD");
        let w = CritterWad::parse(&b).unwrap();
        assert_eq!(w.types.len(), 1);
        assert_eq!(w.moves.len(), 1);
        assert_eq!(label(&w.types[0], 0x20, 16), "HEAD");
        assert!(CritterWad::parse(&b[..0x100]).is_err());
    }
}
