//! The import: the supported disc's files verified and copied, WAD.BIN
//! extracted with every decoded entry checked against the shipped catalog,
//! the configuration generated — all in a staging directory inside the
//! installation that replaces `data/` only when complete. Progress goes
//! to stdout as JSON lines for the installer (see main.rs).
use crate::source::Source;
use gdl_formats::wad::{Entry, Limits, Wad};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

type R<T> = Result<T, Box<dyn Error>>;

pub fn hex(d: impl AsRef<[u8]>) -> String {
    d.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn emit(v: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

/// The shipped description of the supported disc (supported-disc.json).
pub struct Supported {
    pub json: Value,
}

impl Supported {
    pub fn load(path: &Path) -> R<Self> {
        Ok(Self { json: serde_json::from_slice(&fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)? })
    }

    pub fn files(&self) -> Vec<(String, u64, String, String)> {
        self.json["files"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| Some((f["path"].as_str()?.to_string(), f["size"].as_u64()?, f["sha256"].as_str()?.to_string(), f["use"].as_str()?.to_string())))
            .collect()
    }

    fn file(&self, path: &str) -> Option<(u64, String)> {
        self.files().into_iter().find(|f| f.0 == path).map(|f| (f.1, f.2))
    }
}

/// What the disc is: SYSTEM.CNF's boot line and the executable's identity.
/// Ok(message) when it is the supported disc, Err(message) otherwise.
pub fn identify(src: &mut Source, sup: &Supported) -> Result<String, String> {
    let title = sup.json["title"].as_str().unwrap_or("Gauntlet: Dark Legacy");
    let serial = sup.json["serial"].as_str().unwrap_or("SLUS_200.47");
    let cnf = src.read("SYSTEM.CNF").map_err(|_| "No PlayStation 2 game was found there (no SYSTEM.CNF).".to_string())?;
    let cnf = String::from_utf8_lossy(&cnf).to_string();
    let boot = cnf.lines().find_map(|l| l.split_once('=').filter(|(k, _)| k.trim().eq_ignore_ascii_case("BOOT2")).map(|(_, v)| v.trim().to_string())).unwrap_or_default();
    let boot_file = boot.rsplit(['\\', '/', ':']).next().unwrap_or("").split(';').next().unwrap_or("").to_uppercase();
    if boot_file != serial.to_uppercase() {
        return Err(format!("This is a different PlayStation 2 game ({}), not {title} ({serial}).", if boot_file.is_empty() { "unknown" } else { &boot_file }));
    }
    let (size, sha) = sup.file(serial).ok_or("supported-disc.json lists no executable")?;
    let elf = src.read(serial).map_err(|e| format!("The game's executable could not be read: {e}"))?;
    if elf.len() as u64 != size || hex(Sha256::digest(&elf)) != sha {
        return Err(format!("This is {title} ({serial}), but a release this installer does not support (its executable differs from the supported one)."));
    }
    let missing: Vec<String> = sup.files().into_iter().filter(|f| src.size(&f.0).is_none()).map(|f| f.0).collect();
    if !missing.is_empty() {
        return Err(format!("The disc is missing files: {}", missing.join(", ")));
    }
    Ok(format!("{title} ({serial}, NTSC-U) — supported"))
}

#[derive(Serialize)]
struct Outcome {
    #[serde(flatten)]
    entry: Entry,
    identity: String,
    stored_sha256: String,
    decoded_sha256: Option<String>,
    output_file: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct Manifest {
    schema_version: u32,
    converter_version: &'static str,
    source_sha256: String,
    declared_decoded_bytes: u64,
    duplicate_hashes: std::collections::BTreeMap<u32, Vec<u32>>,
    entries: Vec<Outcome>,
}

/// Throttled progress: (step, done, total) and the overall fraction.
struct Progress {
    last: std::time::Instant,
}

impl Progress {
    fn report(&mut self, step: &str, done: u64, total: u64, lo: f64, hi: f64) {
        if self.last.elapsed().as_millis() < 100 && done < total {
            return;
        }
        self.last = std::time::Instant::now();
        let f = if total == 0 { 1.0 } else { done as f64 / total as f64 };
        emit(json!({"event": "progress", "step": step, "done": done, "total": total, "overall": lo + (hi - lo) * f}));
    }
}

/// Leftover staging directories of an interrupted run.
pub fn clean_leftovers(dest: &Path) {
    if let Ok(rd) = fs::read_dir(dest) {
        for d in rd.flatten() {
            let n = d.file_name().to_string_lossy().to_string();
            if n.starts_with(".gdl-import-") || n.starts_with(".gdl-old-") {
                let _ = fs::remove_dir_all(d.path());
            }
        }
    }
}

struct Staging(PathBuf);

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn install(src: &mut Source, dest: &Path, reference: &Path) -> R<()> {
    let sup = Supported::load(&reference.join("supported-disc.json"))?;
    let catalog_bytes = fs::read(reference.join("catalog.json")).map_err(|e| format!("catalog.json: {e}"))?;
    let catalog: Value = serde_json::from_slice(&catalog_bytes)?;
    fs::create_dir_all(dest)?;
    clean_leftovers(dest);
    emit(json!({"event": "step", "step": "identify", "title": "Checking the disc"}));
    let message = identify(src, &sup)?;
    emit(json!({"event": "log", "message": message}));
    let stage = Staging(dest.join(format!(".gdl-import-{}", std::process::id())));
    fs::create_dir(&stage.0)?;
    let data = stage.0.clone();
    let mut prog = Progress { last: std::time::Instant::now() };

    // Copy (and verify) the files the game reads in place.
    let copies: Vec<_> = sup.files().into_iter().filter(|f| f.3 == "copy").collect();
    let copy_total: u64 = copies.iter().map(|f| f.1).sum();
    emit(json!({"event": "step", "step": "copy", "title": "Copying the game's files"}));
    let mut copied = 0u64;
    let mut movies = Vec::new();
    for (path, size, sha, _) in &copies {
        let out_path = data.join("disc").join(path);
        fs::create_dir_all(out_path.parent().ok_or("path")?)?;
        let mut hasher = Sha256::new();
        {
            let file = fs::OpenOptions::new().write(true).create_new(true).open(&out_path).map_err(|e| format!("cannot write {}: {e}", out_path.display()))?;
            let mut w = HashingWriter { inner: std::io::BufWriter::with_capacity(1 << 20, file), hasher: &mut hasher };
            let base = copied;
            src.copy(path, &mut w, |n| {
                prog.report("copy", base + n, copy_total, 0.0, 0.15);
                true
            })
            .map_err(|e| format!("{path}: {e}"))?;
            w.inner.flush()?;
            w.inner.get_ref().sync_all()?;
        }
        if hex(hasher.finalize()) != *sha {
            return Err(format!("{path} differs from the supported disc's (a damaged disc or image?)").into());
        }
        copied += size;
        if path.starts_with("MPEG") {
            movies.push(path.clone());
        }
    }

    // WAD.BIN: read and verified whole, then extracted entry by entry.
    let (wad_path, wad_size, wad_sha, _) = sup.files().into_iter().find(|f| f.3 == "extract").ok_or("supported-disc.json lists no archive")?;
    emit(json!({"event": "step", "step": "read", "title": "Reading the game archive"}));
    let mut bytes = Vec::with_capacity(wad_size as usize);
    let mut hasher = Sha256::new();
    {
        let mut w = HashingWriter { inner: &mut bytes, hasher: &mut hasher };
        src.copy(&wad_path, &mut w, |n| {
            prog.report("read", n, wad_size, 0.15, 0.40);
            true
        })
        .map_err(|e| format!("{wad_path}: {e}"))?;
    }
    let source_sha256 = hex(hasher.finalize());
    if source_sha256 != wad_sha || catalog["source_sha256"].as_str() != Some(&wad_sha) {
        return Err(format!("{wad_path} differs from the supported disc's (a damaged disc or image?)").into());
    }
    let wad = Wad::parse(&bytes, Limits::default())?;
    let expected: std::collections::HashMap<u64, String> = catalog["entries"]
        .as_array()
        .ok_or("catalog entries")?
        .iter()
        .filter_map(|e| Some((e["index"].as_u64()?, e["decoded_sha256"].as_str()?.to_string())))
        .collect();
    if expected.len() != wad.entries().len() {
        return Err("the catalog does not describe this archive".into());
    }
    emit(json!({"event": "step", "step": "extract", "title": "Extracting the game data"}));
    let wad_dir = data.join("wad");
    fs::create_dir(&wad_dir)?;
    let mut manifest = Manifest {
        schema_version: 1,
        converter_version: env!("CARGO_PKG_VERSION"),
        source_sha256: source_sha256.clone(),
        declared_decoded_bytes: wad.declared_decoded_bytes,
        duplicate_hashes: wad.duplicate_hashes.clone(),
        entries: Vec::with_capacity(wad.entries().len()),
    };
    let total = wad.entries().len() as u64;
    for (k, entry) in wad.entries().iter().enumerate() {
        let decoded = wad.decode(entry.index as usize).map_err(|e| format!("archive entry {}: {e}", entry.index))?;
        let sha = hex(Sha256::digest(&decoded));
        if expected.get(&u64::from(entry.index)) != Some(&sha) {
            return Err(format!("archive entry {} decodes differently from the supported disc's", entry.index).into());
        }
        let name = entry.output_filename();
        let mut f = fs::OpenOptions::new().write(true).create_new(true).open(wad_dir.join(&name)).map_err(|e| format!("cannot write {name}: {e}"))?;
        f.write_all(&decoded).map_err(|e| format!("cannot write {name}: {e}"))?;
        manifest.entries.push(Outcome {
            entry: entry.clone(),
            identity: format!("{source_sha256}:{}:{:08x}", entry.index, entry.path_hash),
            stored_sha256: hex(Sha256::digest(wad.stored(entry.index as usize)?)),
            decoded_sha256: Some(sha),
            output_file: Some(name),
            error: None,
        });
        prog.report("extract", k as u64 + 1, total, 0.40, 0.95);
    }
    drop(bytes);
    let mut mf = fs::OpenOptions::new().write(true).create_new(true).open(wad_dir.join("manifest.json"))?;
    serde_json::to_writer_pretty(&mut mf, &manifest)?;
    mf.write_all(b"\n")?;
    mf.sync_all()?;
    fs::write(data.join("catalog.json"), &catalog_bytes)?;
    fs::copy(reference.join("supported-disc.json"), data.join("supported-disc.json"))?;

    emit(json!({"event": "step", "step": "config", "title": "Configuring the game"}));
    write_config(&data, &catalog)?;
    prog.report("config", 1, 1, 0.95, 1.0);

    // Publish: the old data directory (a previous install) aside, the new
    // one in place, the old one removed.
    let target = dest.join("data");
    let old = dest.join(format!(".gdl-old-{}", std::process::id()));
    if target.exists() {
        fs::rename(&target, &old)?;
    }
    fs::rename(&stage.0, &target)?;
    let _ = fs::remove_dir_all(&old);
    emit(json!({"event": "done", "data": target.to_string_lossy()}));
    Ok(())
}

/// data/config.json from the data directory's catalog and manifest.
pub fn write_config(data: &Path, catalog: &Value) -> R<()> {
    let manifest: Value = serde_json::from_slice(&fs::read(data.join("wad/manifest.json"))?)?;
    let files: Vec<String> = manifest["entries"].as_array().ok_or("manifest entries")?.iter().map(|e| e["output_file"].as_str().unwrap_or("").to_string()).collect();
    let names = crate::config::Names::new(catalog, files)?;
    let elf_bytes = fs::read(data.join("disc/SLUS_200.47"))?;
    let elf = gdl_formats::elf::Elf::parse(&elf_bytes)?;
    let mut movies = Vec::new();
    for d in ["MPEG", "MPEG2"] {
        if let Ok(rd) = fs::read_dir(data.join("disc").join(d)) {
            let mut v: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".PSS")).map(|n| format!("{d}/{n}")).collect();
            v.sort();
            movies.extend(v);
        }
    }
    let cfg = crate::config::generate(data, &names, &elf, &movies)?;
    let tmp = data.join("config.json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&cfg)?)?;
    fs::rename(&tmp, data.join("config.json"))?;
    Ok(())
}

/// Every installed file against its recorded identity: disc copies against
/// supported-disc.json, archive entries against the catalog.
pub fn verify(data: &Path) -> R<(usize, Vec<String>)> {
    let sup = Supported::load(&data.join("supported-disc.json"))?;
    let catalog: Value = serde_json::from_slice(&fs::read(data.join("catalog.json"))?)?;
    let manifest: Value = serde_json::from_slice(&fs::read(data.join("wad/manifest.json"))?)?;
    let mut bad = Vec::new();
    let mut checked = 0;
    for (path, _, sha, use_) in sup.files() {
        if use_ != "copy" {
            continue;
        }
        checked += 1;
        match fs::read(data.join("disc").join(&path)) {
            Ok(b) if hex(Sha256::digest(&b)) == sha => {}
            _ => bad.push(format!("disc/{path}")),
        }
    }
    let files = manifest["entries"].as_array().ok_or("manifest entries")?;
    for e in catalog["entries"].as_array().ok_or("catalog entries")? {
        let index = e["index"].as_u64().ok_or("index")? as usize;
        let file = files.get(index).and_then(|f| f["output_file"].as_str()).ok_or("manifest file")?;
        checked += 1;
        match fs::read(data.join("wad").join(file)) {
            Ok(b) if Some(hex(Sha256::digest(&b)).as_str()) == e["decoded_sha256"].as_str() => {}
            _ => bad.push(format!("wad/{file}")),
        }
    }
    Ok((checked, bad))
}

struct HashingWriter<'h, W: Write> {
    inner: W,
    hasher: &'h mut Sha256,
}

impl<W: Write> Write for HashingWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}
