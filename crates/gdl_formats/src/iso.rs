//! Bounded ISO 9660 reader (ECMA-119) over a seekable source: an image
//! file, a raw optical device or a BIN dump. Sectors are 2048-byte user
//! data, taken from cooked images (2048) or raw dumps (2352: MODE1 data at
//! +16, MODE2 form 1 at +24), detected from the primary volume descriptor
//! at sector 16. Only what the importer needs: the volume label, the
//! directory tree (names without the ";1" version, compared without case)
//! and file extents. Interleaved and multi-extent files are refused.
use std::io::{Read, Seek, SeekFrom, Write};

#[derive(Debug)]
pub struct IsoError(pub String);

impl std::fmt::Display for IsoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ISO 9660: {}", self.0)
    }
}

impl std::error::Error for IsoError {}

impl From<std::io::Error> for IsoError {
    fn from(e: std::io::Error) -> Self {
        Self(format!("read failed: {e}"))
    }
}

fn err<T>(reason: impl Into<String>) -> Result<T, IsoError> {
    Err(IsoError(reason.into()))
}

/// Directory size and tree bounds (the supported disc: 7 directories, 56
/// files, the largest directory one sector).
const MAX_DIRECTORY_BYTES: u32 = 1 << 20;
const MAX_DEPTH: usize = 8;
const MAX_ENTRIES: usize = 100_000;

/// One file or directory: its path from the root ('/'-separated, upper
/// case as recorded, no version), first sector and byte length.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IsoEntry {
    pub path: String,
    pub lba: u32,
    pub size: u32,
    pub directory: bool,
}

/// Sector layout of the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectorFormat {
    /// 2048-byte sectors (an .iso or an optical drive's block device).
    Cooked,
    /// 2352-byte raw sectors with the user data at this offset.
    Raw { data_offset: u64 },
}

pub struct IsoFs<R> {
    source: R,
    format: SectorFormat,
    volume_id: String,
    volume_blocks: u32,
    entries: Vec<IsoEntry>,
}

impl<R: Read + Seek> IsoFs<R> {
    pub fn open(mut source: R) -> Result<Self, IsoError> {
        let mut format = None;
        for f in [SectorFormat::Cooked, SectorFormat::Raw { data_offset: 16 }, SectorFormat::Raw { data_offset: 24 }] {
            let mut pvd = [0u8; 2048];
            if read_sector(&mut source, f, 16, &mut pvd).is_ok() && pvd[0] == 1 && &pvd[1..6] == b"CD001" && pvd[6] == 1 {
                format = Some((f, pvd));
                break;
            }
        }
        let Some((format, pvd)) = format else {
            return err("no primary volume descriptor at sector 16 (not an ISO 9660 image or disc)");
        };
        let block = u16::from_le_bytes([pvd[128], pvd[129]]);
        if block != 2048 {
            return err(format!("logical block size {block}, only 2048 is supported"));
        }
        let volume_id = String::from_utf8_lossy(&pvd[40..72]).trim_end_matches([' ', '\0']).to_string();
        let volume_blocks = u32::from_le_bytes(pvd[80..84].try_into().unwrap());
        let root = parse_record(&pvd[156..190]).ok_or_else(|| IsoError("root directory record".into()))?;
        let mut fs = Self { source, format, volume_id, volume_blocks, entries: Vec::new() };
        let mut stack = vec![(String::new(), root.lba, root.size, 0usize)];
        while let Some((prefix, lba, size, depth)) = stack.pop() {
            if depth > MAX_DEPTH {
                return err(format!("directory depth over {MAX_DEPTH} at {prefix}"));
            }
            if size > MAX_DIRECTORY_BYTES {
                return err(format!("directory {prefix} of {size} bytes"));
            }
            fs.check_extent(lba, size)?;
            let sectors = size.div_ceil(2048);
            let mut data = vec![0u8; sectors as usize * 2048];
            for k in 0..sectors {
                let mut s = [0u8; 2048];
                read_sector(&mut fs.source, fs.format, lba + k, &mut s)?;
                data[k as usize * 2048..(k as usize + 1) * 2048].copy_from_slice(&s);
            }
            data.truncate(size as usize);
            let mut at = 0usize;
            while at < data.len() {
                let len = data[at] as usize;
                if len == 0 {
                    // Records do not cross sectors: skip to the next one.
                    at = (at / 2048 + 1) * 2048;
                    continue;
                }
                let rec = data.get(at..at + len).ok_or_else(|| IsoError(format!("directory record past {prefix}")))?;
                at += len;
                let Some(r) = parse_record(rec) else {
                    return err(format!("malformed directory record in {prefix}"));
                };
                if r.name == "\0" || r.name == "\u{1}" {
                    continue;
                }
                if r.flags & 0x80 != 0 {
                    return err(format!("multi-extent file {prefix}{}", r.name));
                }
                if r.interleaved {
                    return err(format!("interleaved file {prefix}{}", r.name));
                }
                let name = r.name.split(';').next().unwrap_or("").trim_end_matches('.').to_string();
                let path = format!("{prefix}{name}");
                let directory = r.flags & 0x02 != 0;
                fs.check_extent(r.lba, r.size)?;
                if fs.entries.len() >= MAX_ENTRIES {
                    return err("too many directory entries");
                }
                fs.entries.push(IsoEntry { path: path.clone(), lba: r.lba, size: r.size, directory });
                if directory {
                    stack.push((format!("{path}/"), r.lba, r.size, depth + 1));
                }
            }
        }
        fs.entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(fs)
    }

    fn check_extent(&self, lba: u32, size: u32) -> Result<(), IsoError> {
        if u64::from(lba) + u64::from(size).div_ceil(2048) > u64::from(self.volume_blocks) {
            return err(format!("extent at sector {lba} ({size} bytes) beyond the volume's {} sectors", self.volume_blocks));
        }
        Ok(())
    }

    pub fn volume_id(&self) -> &str {
        &self.volume_id
    }

    pub fn format(&self) -> SectorFormat {
        self.format
    }

    pub fn entries(&self) -> &[IsoEntry] {
        &self.entries
    }

    /// A file by path ('/' or '\' separated, any case, ";1" optional).
    pub fn find(&self, path: &str) -> Option<&IsoEntry> {
        let want = path.replace('\\', "/").trim_start_matches('/').split(';').next().unwrap_or("").to_uppercase();
        self.entries.iter().find(|e| !e.directory && e.path.to_uppercase() == want)
    }

    /// Copy a file's bytes to `out`; `progress(bytes so far)` after each
    /// chunk (returning false cancels).
    pub fn copy(&mut self, entry: &IsoEntry, out: &mut impl Write, mut progress: impl FnMut(u64) -> bool) -> Result<(), IsoError> {
        let mut left = u64::from(entry.size);
        let mut sector = entry.lba;
        let mut done = 0u64;
        let mut buf = vec![0u8; 2048 * 256];
        while left > 0 {
            let sectors = (left.div_ceil(2048)).min(256) as u32;
            let n = (u64::from(sectors) * 2048).min(left) as usize;
            match self.format {
                SectorFormat::Cooked => {
                    self.source.seek(SeekFrom::Start(u64::from(sector) * 2048))?;
                    self.source.read_exact(&mut buf[..n])?;
                }
                SectorFormat::Raw { .. } => {
                    for k in 0..sectors {
                        let mut s = [0u8; 2048];
                        read_sector(&mut self.source, self.format, sector + k, &mut s)?;
                        buf[k as usize * 2048..(k as usize + 1) * 2048].copy_from_slice(&s);
                    }
                }
            }
            out.write_all(&buf[..n])?;
            left -= n as u64;
            done += n as u64;
            sector += sectors;
            if !progress(done) {
                return err("cancelled");
            }
        }
        Ok(())
    }

    /// A whole file in memory (bounded by its recorded size).
    pub fn read(&mut self, entry: &IsoEntry) -> Result<Vec<u8>, IsoError> {
        let mut v = Vec::with_capacity(entry.size as usize);
        self.copy(entry, &mut v, |_| true)?;
        Ok(v)
    }
}

fn read_sector<R: Read + Seek>(source: &mut R, format: SectorFormat, lba: u32, out: &mut [u8; 2048]) -> Result<(), IsoError> {
    let at = match format {
        SectorFormat::Cooked => u64::from(lba) * 2048,
        SectorFormat::Raw { data_offset } => u64::from(lba) * 2352 + data_offset,
    };
    source.seek(SeekFrom::Start(at))?;
    source.read_exact(out)?;
    Ok(())
}

struct Record {
    lba: u32,
    size: u32,
    flags: u8,
    interleaved: bool,
    name: String,
}

fn parse_record(r: &[u8]) -> Option<Record> {
    if r.len() < 34 {
        return None;
    }
    let name_len = r[32] as usize;
    let name = r.get(33..33 + name_len)?;
    Some(Record {
        lba: u32::from_le_bytes(r[2..6].try_into().ok()?),
        size: u32::from_le_bytes(r[10..14].try_into().ok()?),
        flags: r[25],
        interleaved: r[26] != 0 || r[27] != 0,
        name: String::from_utf8_lossy(name).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn record(name: &[u8], lba: u32, size: u32, dir: bool) -> Vec<u8> {
        let len = 33 + name.len() + (1 - name.len() % 2);
        let mut r = vec![0u8; len];
        r[0] = len as u8;
        r[2..6].copy_from_slice(&lba.to_le_bytes());
        r[6..10].copy_from_slice(&lba.to_be_bytes());
        r[10..14].copy_from_slice(&size.to_le_bytes());
        r[14..18].copy_from_slice(&size.to_be_bytes());
        r[25] = if dir { 2 } else { 0 };
        r[32] = name.len() as u8;
        r[33..33 + name.len()].copy_from_slice(name);
        r
    }

    /// A 24-sector image: PVD at 16, root at 18, SUB at 19, files at 20, 21.
    fn image() -> Vec<u8> {
        let mut b = vec![0u8; 24 * 2048];
        let pvd = 16 * 2048;
        b[pvd] = 1;
        b[pvd + 1..pvd + 6].copy_from_slice(b"CD001");
        b[pvd + 6] = 1;
        b[pvd + 40..pvd + 48].copy_from_slice(b"GAUNTLET");
        b[pvd + 80..pvd + 84].copy_from_slice(&24u32.to_le_bytes());
        b[pvd + 128..pvd + 130].copy_from_slice(&2048u16.to_le_bytes());
        b[pvd + 156..pvd + 190].copy_from_slice(&record(b"\0", 18, 2048, true));
        let mut root = Vec::new();
        root.extend(record(b"\0", 18, 2048, true));
        root.extend(record(b"\x01", 18, 2048, true));
        root.extend(record(b"SYSTEM.CNF;1", 20, 5, false));
        root.extend(record(b"SUB", 19, 2048, true));
        b[18 * 2048..18 * 2048 + root.len()].copy_from_slice(&root);
        let mut sub = Vec::new();
        sub.extend(record(b"\0", 19, 2048, true));
        sub.extend(record(b"\x01", 18, 2048, true));
        sub.extend(record(b"DATA.BIN;1", 21, 3000, false));
        b[19 * 2048..19 * 2048 + sub.len()].copy_from_slice(&sub);
        b[20 * 2048..20 * 2048 + 5].copy_from_slice(b"BOOT2");
        for k in 0..3000 {
            b[21 * 2048 + k] = (k % 251) as u8;
        }
        b
    }

    #[test]
    fn cooked_tree_and_files() {
        let mut fs = IsoFs::open(Cursor::new(image())).unwrap();
        assert_eq!(fs.volume_id(), "GAUNTLET");
        let paths: Vec<_> = fs.entries().iter().map(|e| (e.path.as_str(), e.directory)).collect();
        assert_eq!(paths, [("SUB", true), ("SUB/DATA.BIN", false), ("SYSTEM.CNF", false)]);
        let e = fs.find("\\sub\\data.bin;1").unwrap().clone();
        let data = fs.read(&e).unwrap();
        assert_eq!(data.len(), 3000);
        assert!(data.iter().enumerate().all(|(k, &v)| v == (k % 251) as u8));
        assert_eq!(fs.read(&fs.find("SYSTEM.CNF").unwrap().clone()).unwrap(), b"BOOT2");
    }

    #[test]
    fn raw_mode2_sectors() {
        let cooked = image();
        let mut raw = vec![0u8; 24 * 2352];
        for k in 0..24 {
            raw[k * 2352 + 24..k * 2352 + 24 + 2048].copy_from_slice(&cooked[k * 2048..(k + 1) * 2048]);
        }
        let mut fs = IsoFs::open(Cursor::new(raw)).unwrap();
        assert_eq!(fs.format(), SectorFormat::Raw { data_offset: 24 });
        let e = fs.find("SUB/DATA.BIN").unwrap().clone();
        assert_eq!(fs.read(&e).unwrap(), cooked[21 * 2048..21 * 2048 + 3000]);
    }

    #[test]
    fn refusals() {
        assert!(IsoFs::open(Cursor::new(vec![0u8; 40 * 2048])).is_err());
        let mut b = image();
        // A file extent past the volume.
        let off = 18 * 2048 + 34 + 34 + 2;
        b[off..off + 4].copy_from_slice(&400u32.to_le_bytes());
        assert!(IsoFs::open(Cursor::new(b)).is_err());
        // Truncated image.
        assert!(IsoFs::open(Cursor::new(image()[..17 * 2048].to_vec())).is_err());
    }
}
