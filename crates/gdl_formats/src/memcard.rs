//! Read-only PS2 memory-card image reader (PCSX2 .ps2 files): the
//! superblock, the two-level indirect FAT and 512-byte directory entries.
//!
//! Layout source: the public PS2 memory-card file-system descriptions (an
//! upstream claim, not verified against the BIOS); the reader is checked
//! against the user's copied card (actual data: the superblock magic, the
//! /BASLUS-20047GDL directory and its files). Images with or without the
//! 16-byte per-page ECC spare area are accepted; ECC is not checked. The
//! reader never writes.
use crate::{binary::Reader, wad::ParseError};
use serde::Serialize;

pub const MAGIC: &[u8; 28] = b"Sony PS2 Memory Card Format ";
/// Directory-entry mode bits.
pub const MODE_EXISTS: u16 = 0x8000;
pub const MODE_DIR: u16 = 0x0020;
pub const MODE_FILE: u16 = 0x0010;

#[derive(Debug, Clone, Serialize)]
pub struct Superblock {
    pub page_len: u16,
    pub pages_per_cluster: u16,
    pub pages_per_block: u16,
    pub clusters_per_card: u32,
    pub alloc_offset: u32,
    pub alloc_end: u32,
    pub rootdir_cluster: u32,
    pub ifc_list: Vec<u32>,
    pub card_type: u8,
    pub card_flags: u8,
    /// Page stride in the image (page_len, or page_len + 16 with ECC).
    pub stride: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirEntry {
    pub mode: u16,
    pub length: u32,
    pub cluster: u32,
    pub name: String,
    /// Raw 512-byte entry (timestamps and attributes retained).
    #[serde(skip)]
    pub raw: Vec<u8>,
}

impl DirEntry {
    pub fn is_dir(&self) -> bool {
        self.mode & MODE_DIR != 0
    }
    pub fn exists(&self) -> bool {
        self.mode & MODE_EXISTS != 0
    }
}

pub struct MemoryCard<'a> {
    bytes: &'a [u8],
    pub superblock: Superblock,
}

const FAT_END: u32 = 0x7fff_ffff;

impl<'a> MemoryCard<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, ParseError> {
        let r = Reader { bytes, family: "MEMCARD" };
        r.range(0, 0x154)?;
        if &bytes[..28] != MAGIC {
            return Err(r.error(0, "superblock magic"));
        }
        let page_len = r.u16(0x28);
        let pages_per_cluster = r.u16(0x2a);
        let pages_per_block = r.u16(0x2c);
        let clusters_per_card = r.u32(0x30);
        if page_len != 512 || pages_per_cluster != 2 || clusters_per_card == 0 || clusters_per_card > 0x10_0000 {
            return Err(r.error(0x28, "unsupported card geometry"));
        }
        let pages = clusters_per_card as usize * usize::from(pages_per_cluster);
        let stride = if bytes.len() == pages * (usize::from(page_len) + 16) {
            usize::from(page_len) + 16
        } else if bytes.len() == pages * usize::from(page_len) {
            usize::from(page_len)
        } else {
            return Err(r.error(0, "image size matches neither ECC nor plain layout"));
        };
        let ifc_list = (0..32).map(|k| r.u32(0x50 + 4 * k)).collect();
        let superblock = Superblock {
            page_len,
            pages_per_cluster,
            pages_per_block,
            clusters_per_card,
            alloc_offset: r.u32(0x34),
            alloc_end: r.u32(0x38),
            rootdir_cluster: r.u32(0x3c),
            ifc_list,
            card_type: bytes[0x150],
            card_flags: bytes[0x151],
            stride,
        };
        Ok(Self { bytes, superblock })
    }

    fn reader(&self) -> Reader<'a> {
        Reader { bytes: self.bytes, family: "MEMCARD" }
    }

    /// One absolute cluster (1024 bytes, ECC spare removed).
    pub fn cluster(&self, c: u32) -> Result<Vec<u8>, ParseError> {
        let s = &self.superblock;
        if s.clusters_per_card <= c {
            return Err(self.reader().error(0, format!("cluster {c} beyond the card")));
        }
        let mut out = Vec::with_capacity(1024);
        for k in 0..2usize {
            let p = (c as usize * 2 + k) * s.stride;
            let range = self.reader().range(p, 512)?;
            out.extend_from_slice(&self.bytes[range]);
        }
        Ok(out)
    }

    /// FAT entry of allocatable cluster `n` (relative to alloc_offset).
    fn fat(&self, n: u32) -> Result<u32, ParseError> {
        let word = |b: &[u8], k: usize| u32::from_le_bytes(b[4 * k..4 * k + 4].try_into().unwrap());
        let indirect = (n / 256) as usize;
        let ifc = *self.superblock.ifc_list.get(indirect / 256).ok_or_else(|| self.reader().error(0, "FAT index beyond the indirect list"))?;
        let fat_cluster = word(&self.cluster(ifc)?, indirect % 256);
        Ok(word(&self.cluster(fat_cluster)?, (n % 256) as usize))
    }

    /// The cluster chain starting at relative cluster `first` (bounded).
    pub fn chain(&self, first: u32) -> Result<Vec<u32>, ParseError> {
        let mut out = vec![first];
        let mut n = first;
        loop {
            let e = self.fat(n)?;
            if e == 0xffff_ffff || e & 0x8000_0000 == 0 {
                return Err(self.reader().error(0, format!("cluster {n} is not allocated")));
            }
            let next = e & 0x7fff_ffff;
            if next == FAT_END {
                return Ok(out);
            }
            if self.superblock.clusters_per_card as usize <= out.len() {
                return Err(self.reader().error(0, "cluster chain cycle"));
            }
            out.push(next);
            n = next;
        }
    }

    /// The first `len` bytes of the chain at relative cluster `first`
    /// (walks only as many clusters as `len` needs).
    pub fn read_chain(&self, first: u32, len: usize) -> Result<Vec<u8>, ParseError> {
        let mut out = Vec::new();
        let mut n = first;
        while out.len() < len {
            out.extend(self.cluster(n + self.superblock.alloc_offset)?);
            if len <= out.len() {
                break;
            }
            let e = self.fat(n)?;
            if e == 0xffff_ffff || e & 0x8000_0000 == 0 || e & 0x7fff_ffff == FAT_END {
                return Err(self.reader().error(0, "file shorter than its directory length"));
            }
            n = e & 0x7fff_ffff;
        }
        out.truncate(len);
        Ok(out)
    }

    fn entries(&self, first: u32, count: u32) -> Result<Vec<DirEntry>, ParseError> {
        if 0x1_0000 < count {
            return Err(self.reader().error(0, "directory entry count policy exceeded"));
        }
        let raw = self.read_chain(first, count as usize * 512)?;
        Ok(raw
            .chunks_exact(512)
            .map(|e| {
                let name = &e[0x40..0x60];
                DirEntry {
                    mode: u16::from_le_bytes([e[0], e[1]]),
                    length: u32::from_le_bytes(e[4..8].try_into().unwrap()),
                    cluster: u32::from_le_bytes(e[0x10..0x14].try_into().unwrap()),
                    name: String::from_utf8_lossy(&name[..name.iter().position(|&c| c == 0).unwrap_or(32)]).into_owned(),
                    raw: e.to_vec(),
                }
            })
            .collect())
    }

    /// The live entries of the root directory (without "." and "..").
    pub fn root(&self) -> Result<Vec<DirEntry>, ParseError> {
        let first = self.entries(self.superblock.rootdir_cluster, 1)?;
        let all = self.entries(self.superblock.rootdir_cluster, first[0].length)?;
        Ok(all.into_iter().skip(2).filter(DirEntry::exists).collect())
    }

    /// The live entries of a directory entry.
    pub fn list(&self, dir: &DirEntry) -> Result<Vec<DirEntry>, ParseError> {
        if !dir.is_dir() {
            return Err(self.reader().error(0, format!("{} is not a directory", dir.name)));
        }
        Ok(self.entries(dir.cluster, dir.length)?.into_iter().skip(2).filter(DirEntry::exists).collect())
    }

    /// File contents of a file entry.
    pub fn read(&self, file: &DirEntry) -> Result<Vec<u8>, ParseError> {
        if file.is_dir() || file.mode & MODE_FILE == 0 {
            return Err(self.reader().error(0, format!("{} is not a file", file.name)));
        }
        if file.length == 0 {
            return Ok(Vec::new());
        }
        self.read_chain(file.cluster, file.length as usize)
    }

    /// The file at "/DIR/NAME" (one directory level, as game saves use).
    pub fn file(&self, dir: &str, name: &str) -> Result<Option<Vec<u8>>, ParseError> {
        let Some(d) = self.root()?.into_iter().find(|e| e.is_dir() && e.name == dir) else { return Ok(None) };
        match self.list(&d)?.into_iter().find(|e| !e.is_dir() && e.name == name) {
            Some(f) => self.read(&f).map(Some),
            None => Ok(None),
        }
    }
}

/// SYNTHETIC test image: a plain (no ECC) card holding one directory of
/// files, with only the superblock fields this reader uses. It is not a
/// formatted card a BIOS or PCSX2 would accept; tests use it so that card
/// readers can be checked without private data.
pub fn synthetic_image(dir: &str, files: &[(&str, &[u8])]) -> Vec<u8> {
    let entry = |mode: u16, length: u32, cluster: u32, name: &str| {
        let mut e = vec![0u8; 512];
        e[0..2].copy_from_slice(&mode.to_le_bytes());
        e[4..8].copy_from_slice(&length.to_le_bytes());
        e[0x10..0x14].copy_from_slice(&cluster.to_le_bytes());
        e[0x40..0x40 + name.len().min(31)].copy_from_slice(&name.as_bytes()[..name.len().min(31)]);
        e
    };
    // Objects in allocation order: root (3 entries: relative clusters 0-1),
    // the directory (from relative 2), then each file.
    let count = 2 + files.len() as u32;
    let mut objects: Vec<Vec<u8>> = vec![[entry(0x8427, 3, 0, "."), entry(0xa426, 0, 0, ".."), entry(0x8427, count, 2, dir)].concat()];
    let mut dir_entries = vec![entry(0x8427, count, 2, "."), entry(0xa426, 0, 0, "..")];
    let mut next = 2 + (512 * count as usize).div_ceil(1024) as u32;
    for (name, bytes) in files {
        dir_entries.push(entry(0x8497, bytes.len() as u32, next, name));
        next += bytes.len().div_ceil(1024).max(1) as u32;
    }
    objects.push(dir_entries.concat());
    objects.extend(files.iter().map(|(_, b)| b.to_vec()));
    let alloc = 8usize;
    let clusters = alloc + next as usize;
    assert!(next <= 256, "synthetic image: one FAT cluster");
    let mut b = vec![0u8; clusters * 1024];
    b[..28].copy_from_slice(MAGIC);
    b[0x28..0x2a].copy_from_slice(&512u16.to_le_bytes());
    b[0x2a..0x2c].copy_from_slice(&2u16.to_le_bytes());
    b[0x2c..0x2e].copy_from_slice(&16u16.to_le_bytes());
    b[0x30..0x34].copy_from_slice(&(clusters as u32).to_le_bytes());
    b[0x34..0x38].copy_from_slice(&(alloc as u32).to_le_bytes());
    b[0x50..0x54].copy_from_slice(&1u32.to_le_bytes());
    b[1024..1028].copy_from_slice(&2u32.to_le_bytes());
    let mut at = 0usize;
    for o in &objects {
        let n = o.len().div_ceil(1024).max(1);
        b[(alloc + at) * 1024..(alloc + at) * 1024 + o.len()].copy_from_slice(o);
        for k in 0..n {
            let v = if k + 1 == n { 0x8000_0000 | FAT_END } else { 0x8000_0000 | (at + k + 1) as u32 };
            b[2048 + 4 * (at + k)..2052 + 4 * (at + k)].copy_from_slice(&v.to_le_bytes());
        }
        at += n;
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(mode: u16, length: u32, cluster: u32, name: &str) -> Vec<u8> {
        let mut e = vec![0u8; 512];
        e[0..2].copy_from_slice(&mode.to_le_bytes());
        e[4..8].copy_from_slice(&length.to_le_bytes());
        e[0x10..0x14].copy_from_slice(&cluster.to_le_bytes());
        e[0x40..0x40 + name.len()].copy_from_slice(name.as_bytes());
        e
    }

    /// Synthetic plain (no ECC) 64-cluster card: ifc at 1, FAT at 2,
    /// allocation from cluster 8; root (relative 0) holds ".", "..", "GDL";
    /// GDL (relative 1) holds ".", "..", "save" (relative 2..3, 1500 bytes).
    fn synthetic() -> Vec<u8> {
        let mut b = vec![0u8; 64 * 1024];
        b[..28].copy_from_slice(MAGIC);
        b[0x28..0x2a].copy_from_slice(&512u16.to_le_bytes());
        b[0x2a..0x2c].copy_from_slice(&2u16.to_le_bytes());
        b[0x2c..0x2e].copy_from_slice(&16u16.to_le_bytes());
        b[0x30..0x34].copy_from_slice(&64u32.to_le_bytes());
        b[0x34..0x38].copy_from_slice(&8u32.to_le_bytes());
        b[0x50..0x54].copy_from_slice(&1u32.to_le_bytes());
        b[1024..1028].copy_from_slice(&2u32.to_le_bytes());
        let fat = [0xffff_ffffu32, 0xffff_ffff, 0x8000_0003, 0xffff_ffff];
        for (k, v) in fat.iter().enumerate() {
            b[2048 + 4 * k..2052 + 4 * k].copy_from_slice(&v.to_le_bytes());
        }
        let c = |n: usize| (n + 8) * 1024;
        b[c(0)..c(0) + 512].copy_from_slice(&entry(0x8427, 3, 0, "."));
        b[c(0) + 512..c(0) + 1024].copy_from_slice(&entry(0xa426, 0, 0, ".."));
        // The root's third entry is in its second cluster: chain 0 → 4.
        b[2048..2052].copy_from_slice(&0x8000_0004u32.to_le_bytes());
        b[2048 + 16..2048 + 20].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        b[c(4)..c(4) + 512].copy_from_slice(&entry(0x8427, 3, 1, "GDL"));
        b[c(1)..c(1) + 512].copy_from_slice(&entry(0x8427, 3, 1, "."));
        b[c(1) + 512..c(1) + 1024].copy_from_slice(&entry(0xa426, 0, 0, ".."));
        // GDL's third entry is in its second cluster: chain 1 → 5.
        b[2048 + 4..2052 + 4].copy_from_slice(&0x8000_0005u32.to_le_bytes());
        b[2048 + 20..2048 + 24].copy_from_slice(&0xffff_ffffu32.to_le_bytes());
        b[c(5)..c(5) + 512].copy_from_slice(&entry(0x8497, 1500, 2, "save"));
        for k in 0..1500 {
            let at = if k < 1024 { c(2) + k } else { c(3) + k - 1024 };
            b[at] = (k % 251) as u8;
        }
        b
    }

    #[test]
    fn reads_a_synthetic_card() {
        let b = synthetic();
        let card = MemoryCard::parse(&b).unwrap();
        assert_eq!(card.superblock.stride, 512);
        let root = card.root().unwrap();
        assert_eq!(root.len(), 1);
        assert_eq!(root[0].name, "GDL");
        let f = card.file("GDL", "save").unwrap().unwrap();
        assert_eq!(f.len(), 1500);
        assert!(f.iter().enumerate().all(|(k, &v)| v == (k % 251) as u8));
        assert!(card.file("GDL", "missing").unwrap().is_none());
    }

    #[test]
    fn synthetic_image_round_trips() {
        let big: Vec<u8> = (0..4212u32).map(|k| (k % 253) as u8).collect();
        let b = synthetic_image("DIR", &[("a", &big), ("b", &[1, 2, 3]), ("c", &[9; 1024])]);
        let card = MemoryCard::parse(&b).unwrap();
        assert_eq!(card.root().unwrap().len(), 1);
        assert_eq!(card.file("DIR", "a").unwrap().unwrap(), big);
        assert_eq!(card.file("DIR", "b").unwrap().unwrap(), [1, 2, 3]);
        assert_eq!(card.file("DIR", "c").unwrap().unwrap(), [9; 1024]);
        assert_eq!(card.list(&card.root().unwrap()[0]).unwrap().len(), 3);
    }

    #[test]
    fn rejects_cycles_and_bad_magic() {
        let mut b = synthetic();
        // Chain 2 → 3 → 2.
        b[2048 + 12..2048 + 16].copy_from_slice(&0x8000_0002u32.to_le_bytes());
        let card = MemoryCard::parse(&b).unwrap();
        assert!(card.file("GDL", "save").is_ok(), "length-bounded read stops before the cycle");
        assert!(card.chain(2).is_err());
        b[0] = b'X';
        assert!(MemoryCard::parse(&b).is_err());
    }
}
