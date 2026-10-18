//! Native save storage (M18). Saves are versioned separately from the
//! original memory-card data: each character file is a JSON envelope that
//! keeps the original 0x1074-byte save region (game::profile) byte for byte
//! — unknown bytes included — plus native metadata and a SHA-256 of the
//! region.
//!
//! Layout of a save directory:
//! - `save0001.json` … : one character each; the number follows the
//!   original's BASLUS-20047save%04d (slot + 1).
//! - `<name>.bak`: the previous valid version, kept on every write.
//! - `<name>.tmp`: a write in progress; never trusted unless valid.
//! - `options.json`: the GameOpts block (0x20 bytes from GamePrefs 0x21edb8).
//!
//! Writes go to `.tmp`, are flushed, the old file becomes `.bak` and the
//! `.tmp` is renamed into place, so an interrupted write leaves either the
//! old or the new version readable. Loading verifies the format, schema and
//! checksum and falls back to `.tmp` (complete and valid) and then `.bak`.
//!
//! Schema history: 0 = a raw original save file (the memory-card file's
//! 0x1074 bytes, also what a PS2 card holds), migrated on import; 1 = this
//! envelope.
use crate::game::profile::{summary, Summary, SAVE_LEN};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FORMAT: &str = "gdl-native-save";
pub const SCHEMA: u32 = 1;
pub const REVISION: &str = "SLUS-20047";
/// GameOpts length (saveSave writes 0x20 bytes from 0x21edb8).
pub const GAME_OPTS_LEN: usize = 0x20;
/// Envelope size policy for reads.
const MAX_FILE: u64 = 64 * 1024;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Result<Vec<u8>, String> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return Err("bad hex length".into());
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string())).collect()
}

pub fn sha256(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SaveEnvelope {
    pub format: String,
    pub schema: u32,
    /// Game revision the region belongs to.
    pub revision: String,
    /// save%04d number (1-based).
    pub slot: u32,
    /// Where the character came from: "new", "migrated:raw:<sha256>",
    /// "imported:capture:<sha256>" …
    pub source: String,
    /// Increments on every write of this slot.
    pub sequence: u64,
    /// The original save region (hex).
    pub region_hex: String,
    pub region_sha256: String,
}

impl SaveEnvelope {
    pub fn new(slot: u32, source: &str, region: &[u8]) -> Result<Self, String> {
        if region.len() != SAVE_LEN {
            return Err(format!("region is {:#x} bytes", region.len()));
        }
        Ok(Self { format: FORMAT.into(), schema: SCHEMA, revision: REVISION.into(), slot, source: source.into(), sequence: 0, region_hex: hex(region), region_sha256: sha256(region) })
    }

    pub fn region(&self) -> Result<Vec<u8>, String> {
        let r = unhex(&self.region_hex)?;
        if r.len() != SAVE_LEN {
            return Err(format!("region is {:#x} bytes", r.len()));
        }
        if sha256(&r) != self.region_sha256 {
            return Err("region checksum mismatch".into());
        }
        Ok(r)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err(format!("not a native save ({})", self.format));
        }
        if self.schema != SCHEMA {
            return Err(format!("unsupported schema {}", self.schema));
        }
        if self.revision != REVISION {
            return Err(format!("save is for revision {}", self.revision));
        }
        self.region().map(|_| ())
    }

    pub fn summary(&self) -> Option<Summary> {
        self.region().ok().and_then(|r| summary(&r))
    }
}

/// Result of loading one slot.
#[derive(Debug)]
pub struct Loaded {
    pub save: SaveEnvelope,
    /// Some(file) when the primary file was missing or invalid and this
    /// fallback was used, with the reason.
    pub recovered: Option<(PathBuf, String)>,
}

/// A slot listing entry (DirInfo equivalent).
#[derive(Debug, Clone)]
pub struct Listed {
    pub slot: u32,
    pub summary: Option<Summary>,
    pub problem: Option<String>,
}

pub struct SaveStore {
    pub dir: PathBuf,
}

fn read_envelope(path: &Path) -> Result<SaveEnvelope, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    if MAX_FILE < meta.len() {
        return Err("file larger than the save size policy".into());
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let e: SaveEnvelope = serde_json::from_slice(&bytes).map_err(|e| format!("unreadable: {e}"))?;
    e.validate()?;
    Ok(e)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    let bak = path.with_extension("bak");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    if path.exists() {
        fs::rename(path, &bak).map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    if let Some(d) = path.parent()
        && let Ok(f) = fs::File::open(d)
    {
        let _ = f.sync_all();
    }
    Ok(())
}

impl SaveStore {
    pub fn open(dir: impl Into<PathBuf>) -> Result<Self, String> {
        let dir = dir.into();
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self { dir })
    }

    pub fn path(&self, slot: u32) -> PathBuf {
        self.dir.join(format!("save{slot:04}.json"))
    }

    /// Load a slot: the primary file, else a complete `.tmp`, else `.bak`.
    pub fn load(&self, slot: u32) -> Result<Loaded, String> {
        let main = self.path(slot);
        let first = match read_envelope(&main) {
            Ok(save) => return Ok(Loaded { save, recovered: None }),
            Err(e) => e,
        };
        for alt in [main.with_extension("tmp"), main.with_extension("bak")] {
            if let Ok(save) = read_envelope(&alt)
                && save.slot == slot
            {
                return Ok(Loaded { save, recovered: Some((alt, first)) });
            }
        }
        Err(format!("slot {slot}: {first}"))
    }

    /// Write a slot (sequence = previous + 1).
    pub fn store(&self, slot: u32, source: &str, region: &[u8]) -> Result<SaveEnvelope, String> {
        let mut e = SaveEnvelope::new(slot, source, region)?;
        if let Ok(old) = self.load(slot) {
            e.sequence = old.save.sequence + 1;
            if source.is_empty() {
                e.source = old.save.source;
            }
        }
        let bytes = serde_json::to_vec_pretty(&e).map_err(|e| e.to_string())?;
        write_atomic(&self.path(slot), &bytes)?;
        Ok(e)
    }

    /// Schema 0 → 1: a raw original save file (0x1074 bytes) into a new
    /// slot. The source file is only read.
    pub fn import_raw(&self, raw: &[u8]) -> Result<SaveEnvelope, String> {
        if raw.len() != SAVE_LEN {
            return Err(format!("raw save is {:#x} bytes, expected {SAVE_LEN:#x}", raw.len()));
        }
        summary(raw).ok_or("raw save has no valid class")?;
        let slot = self.free_slot()?;
        self.store(slot, &format!("migrated:raw:{}", sha256(raw)), raw)
    }

    pub fn free_slot(&self) -> Result<u32, String> {
        (1..=999).find(|&s| !self.path(s).exists() && !self.path(s).with_extension("bak").exists()).ok_or_else(|| "no free save slot".into())
    }

    /// Every slot with a file (or recoverable leftovers), in slot order.
    pub fn list(&self) -> Vec<Listed> {
        let mut slots: Vec<u32> = fs::read_dir(&self.dir)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let n = e.file_name().into_string().ok()?;
                        let stem = n.strip_prefix("save")?.split('.').next()?.to_string();
                        stem.parse::<u32>().ok()
                    })
                    .collect()
            })
            .unwrap_or_default();
        slots.sort_unstable();
        slots.dedup();
        slots
            .into_iter()
            .map(|slot| match self.load(slot) {
                Ok(l) => Listed { slot, summary: l.save.summary(), problem: l.recovered.map(|(p, why)| format!("recovered from {} ({why})", p.display())) },
                Err(e) => Listed { slot, summary: None, problem: Some(e) },
            })
            .collect()
    }

    /// options.json: the GameOpts block, if present and valid.
    pub fn load_options(&self) -> Option<Vec<u8>> {
        let bytes = fs::read(self.dir.join("options.json")).ok()?;
        let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        if v["format"] != "gdl-native-options" || v["schema"] != 1 {
            return None;
        }
        let b = unhex(v["game_opts_hex"].as_str()?).ok()?;
        (b.len() == GAME_OPTS_LEN && v["game_opts_sha256"].as_str() == Some(&sha256(&b))).then_some(b)
    }

    pub fn store_options(&self, opts: &[u8]) -> Result<(), String> {
        if opts.len() != GAME_OPTS_LEN {
            return Err("GameOpts length".into());
        }
        let v = serde_json::json!({"format": "gdl-native-options", "schema": 1, "revision": REVISION, "game_opts_hex": hex(opts), "game_opts_sha256": sha256(opts)});
        write_atomic(&self.dir.join("options.json"), &serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::profile::SAVE_START;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("gdl-save-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn region(name: &[u8], gold: i32) -> Vec<u8> {
        // Synthetic region: name, class 6, gold of class 6.
        let mut r = vec![0u8; SAVE_LEN];
        r[..name.len()].copy_from_slice(name);
        r[0x9a4 - SAVE_START..0x9a6 - SAVE_START].copy_from_slice(&6i16.to_le_bytes());
        let g = 0xcec + 6 * 0xb4 + 0x30 - SAVE_START;
        r[g..g + 4].copy_from_slice(&gold.to_le_bytes());
        r
    }

    #[test]
    fn store_load_and_backup() {
        let s = SaveStore::open(dir("basic")).unwrap();
        let e = s.store(1, "new", &region(b"PELE", 10)).unwrap();
        assert_eq!(e.sequence, 0);
        let e = s.store(1, "", &region(b"PELE", 20)).unwrap();
        assert_eq!((e.sequence, e.source.as_str()), (1, "new"));
        let l = s.load(1).unwrap();
        assert!(l.recovered.is_none());
        assert_eq!(l.save.summary().unwrap().gold, 20);
        // The previous version is the backup.
        let b = read_envelope(&s.path(1).with_extension("bak")).unwrap();
        assert_eq!(b.summary().unwrap().gold, 10);
        let list = s.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].summary.as_ref().unwrap().name, "PELE");
        fs::remove_dir_all(&s.dir).unwrap();
    }

    #[test]
    fn corruption_and_interruption_recover() {
        let s = SaveStore::open(dir("corrupt")).unwrap();
        s.store(2, "new", &region(b"ONE", 1)).unwrap();
        s.store(2, "", &region(b"TWO", 2)).unwrap();
        // Corrupted primary (one flipped hex digit): the backup is used.
        let p = s.path(2);
        let mut text = fs::read_to_string(&p).unwrap();
        let at = text.find("region_hex").unwrap() + 20;
        let c = if &text[at..at + 1] == "0" { "1" } else { "0" };
        text.replace_range(at..at + 1, c);
        fs::write(&p, &text).unwrap();
        let l = s.load(2).unwrap();
        assert_eq!(l.save.summary().unwrap().name, "ONE");
        assert!(l.recovered.as_ref().unwrap().1.contains("checksum"));
        // Interrupted write: primary renamed away, complete .tmp left behind.
        fs::remove_file(&p).unwrap();
        let tmp = SaveEnvelope::new(2, "new", &region(b"THREE", 3)).unwrap();
        fs::write(p.with_extension("tmp"), serde_json::to_vec(&tmp).unwrap()).unwrap();
        assert_eq!(s.load(2).unwrap().save.summary().unwrap().name, "THREE");
        // Truncated .tmp is ignored in favour of the backup.
        fs::write(p.with_extension("tmp"), b"{\"format\":").unwrap();
        assert_eq!(s.load(2).unwrap().save.summary().unwrap().name, "ONE");
        fs::remove_dir_all(&s.dir).unwrap();
    }

    #[test]
    fn migrates_raw_original_saves_and_rejects_others() {
        let s = SaveStore::open(dir("raw")).unwrap();
        let raw = region(b"RAW", 5);
        let e = s.import_raw(&raw).unwrap();
        assert_eq!(e.slot, 1);
        assert!(e.source.starts_with("migrated:raw:"));
        assert_eq!(s.load(1).unwrap().save.region().unwrap(), raw);
        assert!(s.import_raw(&raw[..100]).is_err());
        // Future schema and other revisions are refused, not reinterpreted.
        let mut f = SaveEnvelope::new(3, "new", &raw).unwrap();
        f.schema = 2;
        fs::write(s.path(3), serde_json::to_vec(&f).unwrap()).unwrap();
        assert!(s.load(3).unwrap_err().contains("schema"));
        let mut r = SaveEnvelope::new(4, "new", &raw).unwrap();
        r.revision = "SLES-50XXX".into();
        fs::write(s.path(4), serde_json::to_vec(&r).unwrap()).unwrap();
        assert!(s.load(4).unwrap_err().contains("revision"));
        let opts = [7u8; GAME_OPTS_LEN];
        s.store_options(&opts).unwrap();
        assert_eq!(s.load_options().unwrap(), opts);
        fs::remove_dir_all(&s.dir).unwrap();
    }
}
