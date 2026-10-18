//! Player class data (PDATA/<CLS>.WAD): MB container read by LoadPlyrData
//! 0x14d9f0 (LoadPdataFile 0x14d740 opens "<player_desc>.wad") through
//! MBGetFromWad with the tags PDAT (the class record), SFXX and DAMG (player
//! effect and damage tables for PlayerSfxInitData). Chunks are kept as raw
//! bytes; the PDAT record's field meanings live with their consumers in
//! gdl_core (pointer words +4/+8/+0x24 are runtime-filled).
use crate::{binary::Reader, wad::ParseError};
use serde::Serialize;

/// PDAT chunk length on the disc (all sixteen files: the chunk ends at the
/// MB directory 0x180 bytes later).
pub const PDAT_SIZE: usize = 0x180;

#[derive(Debug, Clone, Serialize)]
pub struct PdataChunk {
    pub tag: String,
    pub offset: u32,
    pub count: u32,
    pub word_0c: u32,
    #[serde(skip)]
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerData {
    pub chunks: Vec<PdataChunk>,
    #[serde(skip)]
    pub pdat: Vec<u8>,
}

impl PlayerData {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let r = Reader { bytes, family: "PDATA" };
        r.range(0, 8)?;
        if bytes.len() > 1024 * 1024 {
            return Err(r.error(0, "player data size policy exceeded"));
        }
        let (directory, count) = (r.u32(0) as usize, r.u32(4));
        if count > 16 {
            return Err(r.error(4, "chunk count policy exceeded"));
        }
        let mut heads = Vec::new();
        for i in 0..count as usize {
            let p = directory + 16 * i;
            r.range(p, 16)?;
            heads.push((String::from_utf8_lossy(&r.u32(p).to_be_bytes()).into_owned(), r.u32(p + 4), r.u32(p + 8), r.u32(p + 12)));
        }
        // A chunk ends at the next chunk (or the directory / file end).
        let mut starts: Vec<usize> = heads.iter().map(|h| h.1 as usize).collect();
        starts.push(directory);
        starts.push(bytes.len());
        let mut chunks = Vec::new();
        for (tag, offset, n, w) in heads {
            let o = offset as usize;
            let end = starts.iter().copied().filter(|&s| o < s).min().unwrap_or(bytes.len());
            r.range(o, end - o)?;
            chunks.push(PdataChunk { tag, offset, count: n, word_0c: w, bytes: bytes[o..end].to_vec() });
        }
        let pdat = chunks.iter().find(|c| c.tag == "PDAT").ok_or_else(|| r.error(0, "no PDAT chunk"))?;
        if pdat.bytes.len() < PDAT_SIZE {
            return Err(r.error(pdat.offset as usize, "PDAT shorter than 0x180"));
        }
        let pdat = pdat.bytes[..PDAT_SIZE].to_vec();
        Ok(Self { chunks, pdat })
    }

    pub fn chunk(&self, tag: &str) -> Option<&PdataChunk> {
        self.chunks.iter().find(|c| c.tag == tag)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_synthetic_pdata() {
        // Synthetic: directory at 0x200 with PDAT at 0x10 (0x180 bytes).
        let mut b = vec![0u8; 0x210];
        b[0..4].copy_from_slice(&0x200u32.to_le_bytes());
        b[4..8].copy_from_slice(&1u32.to_le_bytes());
        b[0x200..0x204].copy_from_slice(&u32::from_be_bytes(*b"PDAT").to_le_bytes());
        b[0x204..0x208].copy_from_slice(&0x10u32.to_le_bytes());
        b[0x208..0x20c].copy_from_slice(&1u32.to_le_bytes());
        b[0x10] = 0x42;
        let d = super::PlayerData::parse(&b).unwrap();
        assert_eq!(d.pdat.len(), super::PDAT_SIZE);
        assert_eq!(d.pdat[0], 0x42);
    }
}
