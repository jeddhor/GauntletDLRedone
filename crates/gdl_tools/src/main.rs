use gdl_formats::texdef::TextureTable;
use gdl_formats::wad::{Entry, Limits, Wad};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

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
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>, Box<dyn Error>> {
    use std::io::Read;
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > limit {
        return Err("input exceeds size policy".into());
    }
    let mut data = Vec::new();
    file.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        return Err("input grew beyond size policy".into());
    }
    Ok(data)
}

fn texdef_export(
    source: &Path,
    textures: Option<&Path>,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    if output.exists() {
        return Err("output already exists; choose a new path".into());
    }
    let bytes = bounded_read(source, 64 * 1024 * 1024)?;
    let table = TextureTable::parse(&bytes)?;
    let pixels = textures
        .map(|p| bounded_read(p, 1024 * 1024 * 1024))
        .transpose()?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = Staging(parent.join(format!(".gdl-staging-{}", std::process::id())));
    fs::create_dir(&stage.0)?;
    let mut outcomes = Vec::new();
    let mut decoded = 0;
    for bitmap in &table.bitmaps {
        let mut png_name = None;
        let mut error = None;
        if let Some(data) = &pixels {
            match bitmap.decode_base_rgba(data) {
                Ok(rgba) => {
                    let name = format!("{:06}-source-alpha.png", bitmap.index);
                    let file = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(stage.0.join(&name))?;
                    let mut encoder = png::Encoder::new(
                        std::io::BufWriter::new(file),
                        bitmap.width as u32,
                        bitmap.height as u32,
                    );
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(png::BitDepth::Eight);
                    let mut writer = encoder.write_header()?;
                    writer.write_image_data(&rgba)?;
                    writer.finish()?;
                    fs::File::open(stage.0.join(&name))?.sync_all()?;
                    png_name = Some(name);
                    decoded += 1;
                }
                Err(e) => error = Some(e.to_string()),
            }
        }
        outcomes
            .push(serde_json::json!({"bitmap_index":bitmap.index,"png":png_name,"error":error}));
    }
    let manifest = serde_json::json!({"schema_version":1,"converter_version":env!("CARGO_PKG_VERSION"),
        "texdef_sha256":hash(&bytes),"textures_sha256":pixels.as_ref().map(|p|hash(p)),
        "decode_scope":"candidate base-level pixels only; source alpha preserved; material blending and mipmaps unverified",
        "table":table,"outcomes":outcomes});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(stage.0.join("manifest.json"))?;
    serde_json::to_writer_pretty(&mut file, &manifest)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(&stage.0, output)?;
    println!(
        "{} bitmap records; {decoded} base PNGs; unsupported formats retained in manifest",
        table.bitmaps.len()
    );
    Ok(())
}

/// Removing an unpublished staging directory cannot affect an existing cache.
struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn write_manifest(path: &Path, manifest: &Manifest) -> Result<(), Box<dyn Error>> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    serde_json::to_writer_pretty(&mut file, manifest)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 3 && (args[0] == "objects-inspect" || args[0] == "world-inspect") {
        let bytes = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let report = if args[0] == "world-inspect" {
            let world = gdl_formats::world::World::parse(&bytes)?;
            let placements = world.placements()?;
            serde_json::json!({"source_sha256":hash(&bytes),"world":world,"candidate_placements":placements})
        } else {
            let objects = gdl_formats::objects::Objects::parse(&bytes)?;
            let mut outcomes = Vec::new();
            for object in &objects.objects {
                for part in &object.parts {
                    let result = part.decode_integer_geometry();
                    outcomes.push(match result {
                        Ok(geometry) => serde_json::json!({"object":object.index,"part":part.index,
                            "geometry":geometry,"error":null}),
                        Err(error) => serde_json::json!({"object":object.index,"part":part.index,
                            "geometry":null,"error":error.to_string()}),
                    });
                }
            }
            serde_json::json!({"source_sha256":hash(&bytes),"objects":objects,"outcomes":outcomes})
        };
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        let mut buffered = std::io::BufWriter::new(&mut file);
        serde_json::to_writer_pretty(&mut buffered, &report)?;
        buffered.flush()?;
        drop(buffered);
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "texdef-inspect" {
        return texdef_export(Path::new(&args[1]), None, Path::new(&args[2]));
    }
    if args.len() == 4 && args[0] == "texdef-extract" {
        return texdef_export(
            Path::new(&args[1]),
            Some(Path::new(&args[2])),
            Path::new(&args[3]),
        );
    }
    if args.len() != 3 || (args[0] != "wad-inspect" && args[0] != "wad-extract") {
        return Err("Usage: gdl_tools wad-inspect WAD.BIN NEW_MANIFEST.json\n       gdl_tools wad-extract WAD.BIN NEW_DIRECTORY\n       gdl_tools texdef-inspect TEXDEF.PS2 NEW_DIRECTORY\n       gdl_tools texdef-extract TEXDEF.PS2 TEXTURES.PS2 NEW_DIRECTORY\n       gdl_tools objects-inspect OBJECTS.PS2 NEW_REPORT.json\n       gdl_tools world-inspect WORLDS.PS2 NEW_REPORT.json".into());
    }
    let extracting = args[0] == "wad-extract";
    let source = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    if output.exists() {
        return Err("output already exists; use a new path (existing data is preserved)".into());
    }
    let limits = Limits::default();
    let bytes = bounded_read(source, limits.archive_bytes as u64)?;
    let source_sha256 = hash(&bytes);
    let wad = Wad::parse(&bytes, limits)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage_path = parent.join(format!(".gdl-staging-{}", std::process::id()));
    fs::create_dir(&stage_path)?;
    let stage = Staging(stage_path);
    let mut manifest = Manifest {
        schema_version: 1,
        converter_version: env!("CARGO_PKG_VERSION"),
        source_sha256: source_sha256.clone(),
        declared_decoded_bytes: wad.declared_decoded_bytes,
        duplicate_hashes: wad.duplicate_hashes.clone(),
        entries: Vec::with_capacity(wad.entries().len()),
    };
    let mut failures = 0;
    for entry in wad.entries() {
        let identity = format!("{source_sha256}:{}:{:08x}", entry.index, entry.path_hash);
        let mut row = Outcome {
            entry: entry.clone(),
            identity,
            stored_sha256: hash(wad.stored(entry.index as usize)?),
            decoded_sha256: None,
            output_file: None,
            error: None,
        };
        match wad.decode(entry.index as usize) {
            Ok(data) => {
                row.decoded_sha256 = Some(hash(&data));
                if extracting {
                    let name = entry.output_filename();
                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(stage.0.join(&name))?;
                    file.write_all(&data)?;
                    file.sync_all()?;
                    row.output_file = Some(name);
                }
            }
            Err(e) => {
                failures += 1;
                row.error = Some(e.to_string());
            }
        }
        manifest.entries.push(row);
    }
    write_manifest(&stage.0.join("manifest.json"), &manifest)?;
    if extracting && failures != 0 {
        return Err(format!(
            "{failures} decode failures; no cache published. Run wad-inspect to retain diagnostics."
        )
        .into());
    }
    if extracting {
        fs::rename(&stage.0, output)?;
    } else {
        fs::rename(stage.0.join("manifest.json"), output)?;
    }
    println!(
        "{} entries; {} successful; {failures} failures; {} declared decoded bytes",
        manifest.entries.len(),
        manifest.entries.len() - failures,
        manifest.declared_decoded_bytes
    );
    if failures != 0 {
        return Err("inspection recorded decode failures".into());
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
