//! Native save storage (M18). Saves are versioned separately from the
//! original memory-card data: each character file is a JSON envelope that
//! keeps the original 0x1074-byte save region (game::profile) byte for byte
//! — unknown bytes included — plus native metadata and a SHA-256 of the
//! region.
//!
//! Layout of a save directory:
//! - `save0001.json` … : one character each; the number follows the
//!   original's BASLUS-20047save%04d (slot + 1): the select screen's card
//!   files 0..7 are save0001..save0008.
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
    /// Slots loaded from a backup or a leftover `.tmp` this session (each
    /// reported once), for the client's recovery notice.
    recoveries: std::sync::Mutex<(Vec<(u32, PathBuf, String)>, std::collections::HashSet<u32>)>,
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
        Ok(Self { dir, recoveries: Default::default() })
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
                if let Ok(mut r) = self.recoveries.lock()
                    && r.1.insert(slot)
                {
                    r.0.push((slot, alt.clone(), first.clone()));
                }
                return Ok(Loaded { save, recovered: Some((alt, first)) });
            }
        }
        Err(format!("slot {slot}: {first}"))
    }

    /// Recoveries not reported yet: (slot, file used, why the primary failed).
    pub fn take_recoveries(&self) -> Vec<(u32, PathBuf, String)> {
        self.recoveries.lock().map(|mut r| std::mem::take(&mut r.0)).unwrap_or_default()
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

    /// Interim builds (before 2026-10-23) mapped select-screen file k to
    /// slot k, writing file 0 to save0000.json. Move such a character to the
    /// first free card slot (1..=8, else any free slot); the old files are
    /// kept as migrated-save0000.* so nothing is deleted. Returns the new slot.
    pub fn migrate_slot_zero(&self) -> Result<Option<u32>, String> {
        let zero = self.path(0);
        if !zero.exists() && !zero.with_extension("bak").exists() && !zero.with_extension("tmp").exists() {
            return Ok(None);
        }
        let old = self.load(0)?;
        let region = old.save.region()?;
        let slot = match (1..=CARD_SAVES).find(|&s| !self.path(s).exists() && !self.path(s).with_extension("bak").exists()) {
            Some(s) => s,
            None => self.free_slot()?,
        };
        let mut e = SaveEnvelope::new(slot, &old.save.source, &region)?;
        e.sequence = old.save.sequence + 1;
        write_atomic(&self.path(slot), &serde_json::to_vec_pretty(&e).map_err(|e| e.to_string())?)?;
        for ext in ["json", "bak", "tmp"] {
            let f = zero.with_extension(ext);
            if f.exists() {
                fs::rename(&f, self.dir.join(format!("migrated-save0000.{ext}"))).map_err(|e| e.to_string())?;
            }
        }
        Ok(Some(slot))
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

    /// Import the original's saves from a PS2 memory-card image (read only).
    /// Each non-empty `save%04d` region becomes a native slot — its own
    /// number when free, else the next free one — with source
    /// "imported:card:<card sha256>:save%04d"; a region already in the store
    /// is not imported again. GameOpts is imported only when the store has
    /// no options.json. DirInfo is compared with each region, not trusted.
    pub fn import_card(&self, card: &[u8]) -> Result<CardReport, String> {
        let mc = gdl_formats::memcard::MemoryCard::parse(card).map_err(|e| e.to_string())?;
        let file = |n: &str| mc.file(CARD_DIR, &format!("{CARD_PREFIX}{n}")).map_err(|e| e.to_string());
        let card_sha = sha256(card);
        let dirinfo = match file("DirInfo")? {
            Some(b) => Some(parse_dirinfo(&b)?),
            None => None,
        };
        if dirinfo.is_none() && (1..=CARD_SAVES).all(|n| matches!(file(&format!("save{n:04}")), Ok(None))) {
            return Err(format!("no /{CARD_DIR} saves on this card"));
        }
        let have: Vec<String> = self.list().iter().filter_map(|l| self.load(l.slot).ok()).map(|l| l.save.region_sha256).collect();
        let mut saves = Vec::new();
        for n in 1..=CARD_SAVES {
            let name = format!("save{n:04}");
            let info = dirinfo.as_ref().map(|d| d[n as usize - 1].clone());
            let mut e = CardSave { number: n, dirinfo: info.clone(), sha256: None, summary: None, dirinfo_agrees: None, slot: None, outcome: String::new() };
            let raw = match file(&name)? {
                Some(raw) => raw,
                None => {
                    e.outcome = "no file".into();
                    saves.push(e);
                    continue;
                }
            };
            e.sha256 = Some(sha256(&raw));
            if raw.len() != SAVE_LEN {
                e.outcome = format!("refused: {:#x} bytes, expected {SAVE_LEN:#x}", raw.len());
            } else if raw.iter().all(|&b| b == 0) {
                e.outcome = "empty (all zero)".into();
            } else if let Some(s) = summary(&raw) {
                e.dirinfo_agrees = info.as_ref().map(|d| !d.empty && d.name_bytes == raw[..8] && d.class == s.class as u32 && d.level_sum == s.level_sum as u32);
                e.summary = Some(s);
                if have.contains(&sha256(&raw)) {
                    e.outcome = "already in the store".into();
                } else {
                    let slot = if self.path(n).exists() || self.path(n).with_extension("bak").exists() { self.free_slot()? } else { n };
                    self.store(slot, &format!("imported:card:{card_sha}:{name}"), &raw)?;
                    e.slot = Some(slot);
                    e.outcome = format!("imported into slot {slot}");
                }
            } else {
                e.outcome = "refused: no valid class".into();
            }
            saves.push(e);
        }
        let opts = file("GameOpts")?;
        let options = match &opts {
            None => "no GameOpts file".to_string(),
            Some(o) if o.len() != GAME_OPTS_LEN => format!("refused: GameOpts is {:#x} bytes", o.len()),
            Some(_) if self.dir.join("options.json").exists() => "kept the store's options.json".into(),
            Some(o) => {
                self.store_options(o)?;
                "imported into options.json".into()
            }
        };
        Ok(CardReport { card_sha256: card_sha, dirinfo, saves, game_opts_sha256: opts.as_deref().map(sha256), options })
    }
}

/// The original's card directory and file prefix (getSaveFileName 0x107a80).
pub const CARD_DIR: &str = "BASLUS-20047GDL";
pub const CARD_PREFIX: &str = "BASLUS-20047";
/// save0001 … save0008 (DirInfo has eight entries).
pub const CARD_SAVES: u32 = 8;

/// One BASLUS-20047DirInfo entry: 16 bytes {u32 level sum (+0x9aa), u32
/// class, name[8]}. Empty entries hold ffffffff ffffffff "Empty" (observed on
/// a card the original wrote: card-scratch-jake-v1).
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DirInfoEntry {
    pub level_sum: u32,
    pub class: u32,
    pub name: String,
    #[serde(skip)]
    pub name_bytes: Vec<u8>,
    pub empty: bool,
}

pub fn parse_dirinfo(b: &[u8]) -> Result<Vec<DirInfoEntry>, String> {
    if b.len() != 16 * CARD_SAVES as usize {
        return Err(format!("DirInfo is {:#x} bytes, expected 0x80", b.len()));
    }
    Ok(b.chunks(16)
        .map(|e| {
            let w = |o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap());
            let n = &e[8..16];
            let name = String::from_utf8_lossy(&n[..n.iter().position(|&c| c == 0).unwrap_or(8)]).into_owned();
            DirInfoEntry { level_sum: w(0), class: w(4), empty: w(0) == u32::MAX && w(4) == u32::MAX, name, name_bytes: n.to_vec() }
        })
        .collect())
}

#[derive(Debug, Serialize)]
pub struct CardSave {
    /// save%04d number.
    pub number: u32,
    pub dirinfo: Option<DirInfoEntry>,
    pub sha256: Option<String>,
    pub summary: Option<Summary>,
    /// DirInfo's name, class and level sum equal the region's.
    pub dirinfo_agrees: Option<bool>,
    pub slot: Option<u32>,
    pub outcome: String,
}

#[derive(Debug, Serialize)]
pub struct CardReport {
    pub card_sha256: String,
    pub dirinfo: Option<Vec<DirInfoEntry>>,
    pub saves: Vec<CardSave>,
    pub game_opts_sha256: Option<String>,
    pub options: String,
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
        // Reported once per slot and session.
        s.load(2).unwrap();
        let r = s.take_recoveries();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].0, 2);
        assert!(s.take_recoveries().is_empty());
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

    /// DirInfo as the original writes it (synthetic values): `named` entries
    /// {level sum, class, name}, the rest ffffffff ffffffff "Empty".
    fn dirinfo(named: &[(u32, u32, &[u8])]) -> Vec<u8> {
        let mut d = Vec::new();
        for k in 0..CARD_SAVES as usize {
            let (l, c, n): (u32, u32, &[u8]) = named.get(k).copied().unwrap_or((u32::MAX, u32::MAX, b"Empty"));
            d.extend(l.to_le_bytes());
            d.extend(c.to_le_bytes());
            let mut name = [0u8; 8];
            name[..n.len()].copy_from_slice(n);
            d.extend(name);
        }
        d
    }

    fn card(saves: &[Vec<u8>], info: &[u8], opts: &[u8]) -> Vec<u8> {
        let names: Vec<String> = (1..=CARD_SAVES).map(|n| format!("{CARD_PREFIX}save{n:04}")).collect();
        let empty = vec![0u8; SAVE_LEN];
        let mut files: Vec<(&str, &[u8])> = vec![("BASLUS-20047DirInfo", info)];
        for (k, n) in names.iter().enumerate() {
            files.push((n.as_str(), saves.get(k).unwrap_or(&empty)));
        }
        files.push(("BASLUS-20047GameOpts", opts));
        gdl_formats::memcard::synthetic_image(CARD_DIR, &files)
    }

    #[test]
    fn slot_zero_moves_to_the_first_free_card_slot() {
        let s = SaveStore::open(dir("zero")).unwrap();
        assert_eq!(s.migrate_slot_zero().unwrap(), None);
        s.store(0, "new", &region(b"OLD", 1)).unwrap();
        s.store(0, "", &region(b"OLD", 2)).unwrap();
        s.store(1, "new", &region(b"ONE", 1)).unwrap();
        assert_eq!(s.migrate_slot_zero().unwrap(), Some(2));
        let l = s.load(2).unwrap().save;
        assert_eq!((l.summary().unwrap().gold, l.source.as_str(), l.sequence), (2, "new", 2));
        assert!(!s.path(0).exists() && s.dir.join("migrated-save0000.json").exists() && s.dir.join("migrated-save0000.bak").exists());
        assert_eq!(s.list().iter().map(|l| l.slot).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(s.migrate_slot_zero().unwrap(), None);
        fs::remove_dir_all(&s.dir).unwrap();
    }

    #[test]
    fn imports_original_card_saves() {
        let s = SaveStore::open(dir("card")).unwrap();
        let jake = region(b"JAKE", 0);
        let level_sum = summary(&jake).unwrap().level_sum as u32;
        let opts = [0x80u8; GAME_OPTS_LEN];
        let image = card(&[jake.clone()], &dirinfo(&[(level_sum, 6, b"JAKE")]), &opts);
        let r = s.import_card(&image).unwrap();
        assert_eq!(r.saves.len(), CARD_SAVES as usize);
        assert_eq!((r.saves[0].slot, r.saves[0].dirinfo_agrees), (Some(1), Some(true)));
        assert!(r.saves[1..].iter().all(|e| e.outcome == "empty (all zero)" && e.dirinfo.as_ref().unwrap().empty));
        assert_eq!(r.options, "imported into options.json");
        let l = s.load(1).unwrap();
        assert_eq!(l.save.region().unwrap(), jake);
        assert_eq!(l.save.source, format!("imported:card:{}:save0001", sha256(&image)));
        assert_eq!(s.load_options().unwrap(), opts);
        // Importing the same card again adds nothing and keeps the options.
        let r = s.import_card(&image).unwrap();
        assert_eq!(r.saves[0].outcome, "already in the store");
        assert_eq!(r.options, "kept the store's options.json");
        assert_eq!(s.list().len(), 1);
        // Another character in an occupied number goes to the next free slot;
        // a DirInfo that disagrees with the region is reported.
        let pele = region(b"PELE", 3);
        let other = card(&[pele.clone()], &dirinfo(&[(1, 2, b"PELE")]), &opts);
        let r = s.import_card(&other).unwrap();
        assert_eq!((r.saves[0].slot, r.saves[0].dirinfo_agrees), (Some(2), Some(false)));
        assert_eq!(s.load(2).unwrap().save.region().unwrap(), pele);
        // Wrong sizes and classless regions are refused, not imported.
        let mut bad = region(b"BAD", 0);
        bad[0x9a4 - SAVE_START] = 40;
        let r = s.import_card(&card(&[bad, pele[..100].to_vec()], &dirinfo(&[]), &opts)).unwrap();
        assert_eq!(r.saves[0].outcome, "refused: no valid class");
        assert!(r.saves[1].outcome.starts_with("refused: 0x64 bytes"));
        // A card without the game's directory is an error.
        let none = gdl_formats::memcard::synthetic_image("OTHER", &[("x", &[1])]);
        assert!(s.import_card(&none).unwrap_err().contains("no /BASLUS-20047GDL"));
        fs::remove_dir_all(&s.dir).unwrap();
    }
}
