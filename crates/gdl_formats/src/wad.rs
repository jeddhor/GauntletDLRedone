//! SLUS_200.47 WAD.BIN table and bounded zlib payload reader.
//!
//! Source identities and original table fields survive name recovery. Names
//! must never be used as output paths. See docs/research/wad.md for evidence.
use flate2::{Decompress, FlushDecompress, Status};
use serde::Serialize;
use std::{collections::BTreeMap, error::Error, fmt, ops::Range};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub entries: u32,
    pub entry_bytes: u32,
    pub total_decoded_bytes: u64,
    pub archive_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            entries: 100_000,
            entry_bytes: 64 * 1024 * 1024,
            total_decoded_bytes: 2 * 1024 * 1024 * 1024,
            archive_bytes: 1024 * 1024 * 1024,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub entry_index: Option<u32>,
    pub byte_offset: usize,
    pub reason: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WAD at {:#x}, entry {:?}: {}",
            self.byte_offset, self.entry_index, self.reason
        )
    }
}
impl Error for ParseError {}

fn fail(index: Option<u32>, offset: usize, reason: impl Into<String>) -> ParseError {
    ParseError {
        entry_index: index,
        byte_offset: offset,
        reason: reason.into(),
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub index: u32,
    pub decoded_size: u32,
    pub file_offset: u32,
    pub path_hash: u32,
    pub raw_compressed_size: i32,
}

impl Entry {
    pub fn stored_size(&self) -> u32 {
        if self.raw_compressed_size == -1 {
            self.decoded_size
        } else {
            self.raw_compressed_size as u32
        }
    }
    fn range(&self) -> Range<usize> {
        self.file_offset as usize..self.file_offset as usize + self.stored_size() as usize
    }
    pub fn output_filename(&self) -> String {
        format!("{:06}-{:08x}.dat", self.index, self.path_hash)
    }
}

pub struct Wad<'a> {
    bytes: &'a [u8],
    entries: Vec<Entry>,
    pub duplicate_hashes: BTreeMap<u32, Vec<u32>>,
    pub declared_decoded_bytes: u64,
}

impl<'a> Wad<'a> {
    pub fn parse(bytes: &'a [u8], limits: Limits) -> Result<Self, ParseError> {
        if bytes.len() > limits.archive_bytes {
            return Err(fail(None, 0, "archive size limit exceeded"));
        }
        let count_bytes: [u8; 4] = bytes
            .get(..4)
            .ok_or_else(|| fail(None, 0, "truncated count"))?
            .try_into()
            .unwrap();
        let count = u32::from_le_bytes(count_bytes);
        if count > limits.entries {
            return Err(fail(None, 0, "entry count limit exceeded"));
        }
        let table_end = (count as usize)
            .checked_mul(16)
            .and_then(|n| n.checked_add(4))
            .ok_or_else(|| fail(None, 0, "table size overflow"))?;
        if table_end > bytes.len() {
            return Err(fail(None, 4, "truncated table"));
        }
        let mut entries = Vec::with_capacity(count as usize);
        let mut hashes: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        let mut total = 0u64;
        for index in 0..count {
            let pos = 4 + index as usize * 16;
            let u = |off| u32::from_le_bytes(bytes[pos + off..pos + off + 4].try_into().unwrap());
            let size = u(0);
            let offset = u(4);
            let compressed = u(12) as i32;
            if size > i32::MAX as u32 || offset > i32::MAX as u32 || compressed < -1 {
                return Err(fail(
                    Some(index),
                    pos,
                    "negative size/offset or unknown compression sentinel",
                ));
            }
            if size > limits.entry_bytes {
                return Err(fail(Some(index), pos, "entry output limit exceeded"));
            }
            total = total
                .checked_add(size as u64)
                .ok_or_else(|| fail(Some(index), pos, "decoded total overflow"))?;
            if total > limits.total_decoded_bytes {
                return Err(fail(Some(index), pos, "total output limit exceeded"));
            }
            let entry = Entry {
                index,
                decoded_size: size,
                file_offset: offset,
                path_hash: u(8),
                raw_compressed_size: compressed,
            };
            let end = (offset as usize)
                .checked_add(entry.stored_size() as usize)
                .ok_or_else(|| fail(Some(index), pos + 4, "payload range overflow"))?;
            if (offset as usize) < table_end || end > bytes.len() {
                return Err(fail(
                    Some(index),
                    pos + 4,
                    "payload intersects table or exceeds archive",
                ));
            }
            hashes.entry(entry.path_hash).or_default().push(index);
            entries.push(entry);
        }
        let mut ranges: Vec<&Entry> = entries.iter().filter(|e| e.stored_size() != 0).collect();
        ranges.sort_by_key(|e| (e.file_offset, e.index));
        for pair in ranges.windows(2) {
            if pair[0].range().end > pair[1].range().start {
                return Err(fail(
                    Some(pair[1].index),
                    pair[1].file_offset as usize,
                    format!("payload overlaps entry {}", pair[0].index),
                ));
            }
        }
        hashes.retain(|_, indices| indices.len() > 1);
        Ok(Self {
            bytes,
            entries,
            duplicate_hashes: hashes,
            declared_decoded_bytes: total,
        })
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Use an index into this validated archive so callers cannot forge ranges.
    pub fn stored(&self, index: usize) -> Result<&[u8], ParseError> {
        let e = self
            .entries
            .get(index)
            .ok_or_else(|| fail(None, 0, "entry index out of bounds"))?;
        Ok(&self.bytes[e.range()])
    }

    pub fn decode(&self, index: usize) -> Result<Vec<u8>, ParseError> {
        let input = self.stored(index)?;
        let e = &self.entries[index];
        if e.raw_compressed_size == -1 {
            return Ok(input.to_vec());
        }
        // One extra byte distinguishes output exceeding the declared length
        // from an exactly sized valid stream. Output allocation is policy bounded.
        let mut output = vec![0; e.decoded_size as usize + 1];
        let mut z = Decompress::new(true);
        let status = z
            .decompress(input, &mut output, FlushDecompress::Finish)
            .map_err(|err| {
                fail(
                    Some(e.index),
                    e.file_offset as usize,
                    format!("zlib: {err}"),
                )
            })?;
        if status != Status::StreamEnd || z.total_out() != e.decoded_size as u64 {
            return Err(fail(
                Some(e.index),
                e.file_offset as usize,
                format!(
                    "incomplete stream or length mismatch: expected {}, got {} ({status:?})",
                    e.decoded_size,
                    z.total_out()
                ),
            ));
        }
        if z.total_in() != input.len() as u64 {
            return Err(fail(
                Some(e.index),
                e.file_offset as usize,
                "trailing bytes after zlib stream",
            ));
        }
        output.truncate(e.decoded_size as usize);
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    fn fixture(payloads: &[(&[u8], bool, u32)]) -> Vec<u8> {
        let mut bytes = vec![0; 4 + payloads.len() * 16];
        bytes[..4].copy_from_slice(&(payloads.len() as u32).to_le_bytes());
        for (i, (payload, compressed, hash)) in payloads.iter().enumerate() {
            let stored = if *compressed {
                let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
                z.write_all(payload).unwrap();
                z.finish().unwrap()
            } else {
                payload.to_vec()
            };
            let row = [
                payload.len() as u32,
                bytes.len() as u32,
                *hash,
                if *compressed {
                    stored.len() as u32
                } else {
                    u32::MAX
                },
            ];
            for (j, v) in row.iter().enumerate() {
                bytes[4 + i * 16 + j * 4..8 + i * 16 + j * 4].copy_from_slice(&v.to_le_bytes());
            }
            bytes.extend(stored);
        }
        bytes
    }
    #[test]
    fn duplicate_hashes_keep_distinct_entries_and_decode() {
        let b = fixture(&[
            (b"original", false, 7),
            (b"compressed", true, 7),
            (b"", true, 9),
        ]);
        let wad = Wad::parse(&b, Limits::default()).unwrap();
        assert_eq!(wad.duplicate_hashes[&7], vec![0, 1]);
        assert_ne!(
            wad.entries()[0].output_filename(),
            wad.entries()[1].output_filename()
        );
        assert_eq!(wad.decode(0).unwrap(), b"original");
        assert_eq!(wad.decode(1).unwrap(), b"compressed");
        assert_eq!(wad.decode(2).unwrap(), b"");
    }
    #[test]
    fn rejects_all_truncations_and_impossible_counts() {
        let b = fixture(&[(b"payload", false, 1)]);
        for end in 0..b.len() {
            assert!(Wad::parse(&b[..end], Limits::default()).is_err(), "{end}");
        }
        assert!(Wad::parse(&u32::MAX.to_le_bytes(), Limits::default()).is_err());
    }
    #[test]
    fn rejects_invalid_ranges_sentinels_and_overlap() {
        let valid = fixture(&[(b"first", false, 1), (b"second", false, 2)]);
        for (position, value) in [
            (4, u32::MAX),
            (8, u32::MAX),
            (16, u32::MAX - 1),
            (8, 0),
            (24, 36),
        ] {
            let mut b = valid.clone();
            b[position..position + 4].copy_from_slice(&value.to_le_bytes());
            assert!(
                Wad::parse(&b, Limits::default()).is_err(),
                "{position}: {value}"
            );
        }
    }
    #[test]
    fn rejects_bombs_wrong_lengths_checksums_and_trailing_bytes() {
        let valid = fixture(&[(&[0; 4096], true, 3)]);
        for declared in [0u32, 1, 4095, 4097] {
            let mut b = valid.clone();
            b[4..8].copy_from_slice(&declared.to_le_bytes());
            assert!(
                Wad::parse(&b, Limits::default())
                    .unwrap()
                    .decode(0)
                    .is_err()
            );
        }
        let mut corrupt = valid.clone();
        *corrupt.last_mut().unwrap() ^= 255;
        assert!(
            Wad::parse(&corrupt, Limits::default())
                .unwrap()
                .decode(0)
                .is_err()
        );
        let mut extra = valid.clone();
        extra.push(0);
        extra[16..20].copy_from_slice(&((valid.len() - 20 + 1) as u32).to_le_bytes());
        assert!(
            Wad::parse(&extra, Limits::default())
                .unwrap()
                .decode(0)
                .is_err()
        );
        let limits = Limits {
            entry_bytes: 4095,
            ..Limits::default()
        };
        assert!(Wad::parse(&valid, limits).is_err());
        let limits = Limits {
            total_decoded_bytes: 4095,
            ..Limits::default()
        };
        assert!(Wad::parse(&valid, limits).is_err());
    }
}
