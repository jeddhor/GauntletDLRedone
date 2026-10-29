//! gdl_import: the installer's back end (M21, decisions/0005-installer.md).
//!
//!   gdl_import probe SOURCE [--reference DIR]
//!   gdl_import install SOURCE DEST [--reference DIR]
//!   gdl_import config DATA_DIR
//!   gdl_import verify DATA_DIR
//!   gdl_import make-supported SOURCE PROFILE.json NEW.json      (packaging)
//!   gdl_import make-catalog WAD_INDEPENDENT.json NEW.json        (packaging)
//!
//! SOURCE: a disc image (.iso, a raw .bin dump), an optical drive's device
//! or a mounted disc's root (a drive letter on Windows). DEST: the
//! installation; its data/ directory is replaced only by a complete,
//! verified import. --reference: the shipped catalog.json and
//! supported-disc.json (default: import/ beside this executable).
//!
//! Output is one JSON object per line: {"event": "probe", "supported",
//! "message", ...}, {"event": "step", "step", "title"}, {"event":
//! "progress", "step", "done", "total", "overall"}, {"event": "log",
//! "message"}, {"event": "done"} and on failure {"event": "error",
//! "message"} with exit status 1.
mod config;
mod install;
mod source;

use serde_json::json;
use std::error::Error;
use std::path::{Path, PathBuf};

fn reference_dir(args: &[String]) -> PathBuf {
    if let Some(i) = args.iter().position(|a| a == "--reference")
        && let Some(d) = args.get(i + 1)
    {
        return PathBuf::from(d);
    }
    std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("import"))).unwrap_or_else(|| PathBuf::from("import"))
}

fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let pos: Vec<&String> = {
        let mut v = Vec::new();
        let mut skip = false;
        for a in args {
            if skip {
                skip = false;
            } else if a == "--reference" {
                skip = true;
            } else {
                v.push(a);
            }
        }
        v
    };
    match pos.as_slice() {
        [cmd, src] if *cmd == "probe" => {
            let sup = install::Supported::load(&reference_dir(args).join("supported-disc.json"))?;
            let out = match source::Source::open(Path::new(src)) {
                Ok(mut s) => {
                    let kind = s.kind();
                    let volume = s.volume();
                    match install::identify(&mut s, &sup) {
                        Ok(m) => json!({"event": "probe", "supported": true, "kind": kind, "volume": volume, "message": m}),
                        Err(m) => json!({"event": "probe", "supported": false, "kind": kind, "volume": volume, "message": m}),
                    }
                }
                Err(e) => json!({"event": "probe", "supported": false, "message": format!("Not a readable disc or disc image: {e}")}),
            };
            install::emit(out);
            Ok(())
        }
        [cmd, src, dest] if *cmd == "install" => {
            let mut s = source::Source::open(Path::new(src))?;
            install::install(&mut s, Path::new(dest), &reference_dir(args))
        }
        [cmd, data] if *cmd == "config" => {
            let data = Path::new(data);
            let catalog: serde_json::Value = serde_json::from_slice(&std::fs::read(data.join("catalog.json"))?)?;
            install::write_config(data, &catalog)?;
            install::emit(json!({"event": "done", "config": data.join("config.json").to_string_lossy()}));
            Ok(())
        }
        [cmd, data] if *cmd == "verify" => {
            let (checked, bad) = install::verify(Path::new(data))?;
            install::emit(json!({"event": "verify", "checked": checked, "differing": bad}));
            if bad.is_empty() { Ok(()) } else { Err(format!("{} installed files differ", bad.len()).into()) }
        }
        [cmd, src, profile, out] if *cmd == "make-supported" => make_supported(Path::new(src), Path::new(profile), Path::new(out)),
        [cmd, names, out] if *cmd == "make-catalog" => make_catalog(Path::new(names), Path::new(out)),
        _ => Err("usage: gdl_import probe SOURCE | install SOURCE DEST | config DATA_DIR | verify DATA_DIR [--reference DIR]".into()),
    }
}

/// supported-disc.json from the supported disc (packaging; the profile
/// gives the identity the files must match).
fn make_supported(src: &Path, profile: &Path, out: &Path) -> Result<(), Box<dyn Error>> {
    use sha2::{Digest, Sha256};
    let profile: serde_json::Value = serde_json::from_slice(&std::fs::read(profile)?)?;
    let mut s = source::Source::open(src)?;
    let mut paths = vec![("SLUS_200.47".to_string(), "copy"), ("WAD.BIN".to_string(), "extract"), ("IRX/DCS.IRX".to_string(), "copy")];
    if let source::Source::Image(fs) = &s {
        for e in fs.entries() {
            if !e.directory && (e.path.starts_with("MPEG/") || e.path.starts_with("MPEG2/")) && e.path.ends_with(".PSS") {
                paths.push((e.path.clone(), "copy"));
            }
        }
    } else {
        return Err("make-supported reads an image".into());
    }
    let mut files = Vec::new();
    for (p, use_) in paths {
        let b = s.read(&p)?;
        files.push(json!({"path": p, "size": b.len(), "sha256": install::hex(Sha256::digest(&b)), "use": use_}));
    }
    let check = |p: &str, key: &str| -> Result<(), Box<dyn Error>> {
        let f = files.iter().find(|f| f["path"] == p).ok_or("file")?;
        if f["sha256"] != profile[key]["sha256"] {
            return Err(format!("{p} differs from the profile").into());
        }
        Ok(())
    };
    check("SLUS_200.47", "executable")?;
    check("WAD.BIN", "archive")?;
    let v = json!({
        "schema_version": 1,
        "title": "Gauntlet: Dark Legacy",
        "serial": "SLUS_200.47",
        "region": "NTSC-U",
        "profile_id": profile["profile_id"],
        "image_sha256": profile["image"]["sha256"],
        "note": "Files are checked one by one (a disc read through a drive need not give the image's bytes); the image hash is informational.",
        "files": files,
    });
    std::fs::write(out, serde_json::to_vec_pretty(&v)?)?;
    Ok(())
}

/// The shipped name catalog: the name proposals' identity fields only.
fn make_catalog(names: &Path, out: &Path) -> Result<(), Box<dyn Error>> {
    let w: serde_json::Value = serde_json::from_slice(&std::fs::read(names)?)?;
    let entries: Vec<serde_json::Value> = w["entries"]
        .as_array()
        .ok_or("entries")?
        .iter()
        .map(|e| json!({"index": e["index"], "path_hash": e["path_hash"], "decoded_sha256": e["decoded_sha256"], "proposed_names": e["proposed_names"]}))
        .collect();
    let v = json!({
        "schema_version": 1,
        "source_sha256": w["source_sha256"],
        "names_source": "haekb/gdl_wad_decoder's name dictionary (MIT; commit 981f4ec9586903257e8f2161e918bbbe5263a6eb), matched by path hash; see THIRD_PARTY.md",
        "entries": entries,
    });
    std::fs::write(out, serde_json::to_vec(&v)?)?;
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        install::emit(json!({"event": "error", "message": e.to_string()}));
        std::process::exit(1);
    }
}
