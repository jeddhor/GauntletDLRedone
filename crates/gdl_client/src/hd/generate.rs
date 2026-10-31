//! NATIVE ENHANCEMENT (HD textures, graphics plan step 9): the generated
//! pack. Every texture of the import (hd::census) except those that keep
//! their originals (hd::classify) is upscaled 4× (hd::prep around
//! hd::esrgan) and written as `<census name>.png` — texture_pack's file
//! name and format (alpha doubled) — into a directory named after the
//! generator (model hash and processing version: the cache key with each
//! file's name, a hash of its source texels). Files are written atomically
//! (a temporary file, then a rename), so an interrupted run resumes where it
//! stopped; manifest.json is written last and marks the pack complete.
use super::{census, classify, esrgan, prep};
use crate::catalog::Catalog;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The processing's version (hd::prep and the network's use): a change to
/// what is written takes a new pack directory.
pub const PROCESSING: u32 = 1;

pub fn model_sha256() -> String {
    Sha256::digest(esrgan::GENERAL_DN05).iter().map(|b| format!("{b:02x}")).collect()
}

/// The generator's identity: model, its weights' hash and the processing.
pub fn generator_id() -> String {
    format!("realesr-general-x4v3-dn05-{}-p{PROCESSING}", &model_sha256()[..12])
}

/// The pack directory: the game's data directory's hd-textures/<generator>
/// (DEBUG GDL_HD_DIR: another root, for test runs with their own HOME).
pub fn pack_dir() -> Result<PathBuf, Box<dyn Error>> {
    let root = match std::env::var_os("GDL_HD_DIR") {
        Some(d) => PathBuf::from(d),
        None => crate::data_home()?.join("gdl-native").join("SLUS-20047").join("hd-textures"),
    };
    Ok(root.join(generator_id()))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub generator: String,
    pub model_sha256: String,
    pub processing: u32,
    pub complete: bool,
    /// Census textures, those generated (now or before) and those kept.
    pub textures: usize,
    pub generated: usize,
    pub kept: BTreeMap<String, classify::Keep>,
    pub adapter: String,
    pub seconds: f64,
}

/// Whether `dir` holds this generator's complete pack.
pub fn is_complete(dir: &Path) -> bool {
    std::fs::read(dir.join("manifest.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<Manifest>(&b).ok())
        .is_some_and(|m| m.complete && m.generator == generator_id())
}

/// Progress: the stage ("scan", "generate") and textures done of total.
#[derive(Clone, Copy, Debug)]
pub struct Progress {
    pub stage: &'static str,
    pub done: usize,
    pub total: usize,
}

struct Item {
    name: String,
    prepared: prep::Prepared,
}

fn write_png(path: &Path, width: usize, height: usize, rgba: &[u8]) -> Result<(), Box<dyn Error + Send + Sync>> {
    let tmp = path.with_extension("png.tmp");
    {
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(&tmp)?), width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Balanced);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
        writer.finish()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Generate the pack into `out`. `progress` is called after the census and
/// after every batch; returning false stops after the batch (what is written
/// stays, a later run resumes).
pub fn generate(catalog: &Catalog, presets: &[u8], effect_info: &[String], out: &Path, mut progress: impl FnMut(Progress) -> bool) -> Result<Manifest, Box<dyn Error>> {
    let t = std::time::Instant::now();
    std::fs::create_dir_all(out)?;
    if !progress(Progress { stage: "scan", done: 0, total: 0 }) {
        return Err("cancelled".into());
    }
    let entries = census::census(catalog, presets)?;
    let kept = classify::classify(&entries, effect_info);
    let wanted: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).filter(|n| !kept.contains_key(*n)).collect();
    let total = wanted.len();
    let mut todo: HashSet<String> = wanted.iter().filter(|n| !out.join(format!("{n}.png")).exists()).map(|n| n.to_string()).collect();
    // DEBUG GDL_HD_LIMIT=N: at most N textures this run (pipeline checks).
    if let Some(n) = std::env::var("GDL_HD_LIMIT").ok().and_then(|v| v.parse::<usize>().ok()) {
        let mut names: Vec<String> = todo.into_iter().collect();
        names.sort();
        names.truncate(n);
        todo = names.into_iter().collect();
    }
    let mut done = total - todo.len();
    let mut cancelled = !progress(Progress { stage: "generate", done, total });
    let up = esrgan::Upscaler::new(&esrgan::Weights::parse(esrgan::GENERAL_DN05)?)?;
    let adapter = up.adapter.clone();
    let failed: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get().saturating_sub(1)).clamp(1, 8);
    let (tx, rx) = std::sync::mpsc::sync_channel::<(PathBuf, usize, usize, Vec<u8>)>(threads * 4);
    let rx = Arc::new(Mutex::new(rx));
    std::thread::scope(|scope| -> Result<(), Box<dyn Error>> {
        for _ in 0..threads {
            let (rx, failed) = (rx.clone(), failed.clone());
            scope.spawn(move || {
                loop {
                    let job = rx.lock().map(|r| r.recv());
                    let Ok(Ok((path, w, h, rgba))) = job else { break };
                    if let Err(e) = write_png(&path, w, h, &rgba) {
                        let _ = std::fs::remove_file(path.with_extension("png.tmp"));
                        failed.lock().unwrap().get_or_insert_with(|| format!("{}: {e}", path.display()));
                    }
                }
            });
        }
        let result = (|| -> Result<(), Box<dyn Error>> {
        let mut buckets: HashMap<(usize, usize), Vec<Item>> = HashMap::new();
        let flush = |items: Vec<Item>, w: usize, h: usize, done: &mut usize| -> Result<(), Box<dyn Error>> {
            let (pw, ph) = (w + 2 * prep::PAD, h + 2 * prep::PAD);
            let mut inputs: Vec<&[f32]> = items.iter().map(|i| i.prepared.rgb.as_slice()).collect();
            inputs.extend(items.iter().filter_map(|i| i.prepared.alpha.as_deref()));
            let outs = up.run(&inputs, pw, ph, prep::PAD)?;
            let mut alpha_k = items.len();
            for (k, item) in items.iter().enumerate() {
                let alpha = item.prepared.alpha.as_ref().map(|_| {
                    alpha_k += 1;
                    outs[alpha_k - 1].as_slice()
                });
                let rgba = prep::finish(&item.prepared, &outs[k], alpha);
                tx.send((out.join(format!("{}.png", item.name)), w * prep::SCALE, h * prep::SCALE, rgba)).map_err(|_| "writer stopped")?;
            }
            *done += items.len();
            Ok(())
        };
        'packages: for prefix in census::package_prefixes(catalog) {
            if todo.is_empty() || cancelled {
                break;
            }
            let Ok(textures) = census::package_textures(catalog, &prefix) else { continue };
            for ((_, _, rgba, _), (w, h, flags)) in textures {
                let name = crate::texture_pack::name(w, h, &rgba);
                if !todo.remove(&name) {
                    continue;
                }
                let (w, h) = (usize::from(w), usize::from(h));
                let prepared = prep::prepare(&prep::dump_rgba(&rgba), w, h, flags);
                let bucket = buckets.entry((w, h)).or_default();
                bucket.push(Item { name, prepared });
                if up.batch_size(w + 2 * prep::PAD, h + 2 * prep::PAD) <= bucket.len() {
                    let items = std::mem::take(bucket);
                    flush(items, w, h, &mut done)?;
                    if failed.lock().unwrap().is_some() || !progress(Progress { stage: "generate", done, total }) {
                        cancelled = true;
                        break 'packages;
                    }
                }
            }
        }
        if !cancelled {
            let mut sizes: Vec<_> = buckets.keys().copied().collect();
            sizes.sort();
            for (w, h) in sizes {
                let mut items = buckets.remove(&(w, h)).unwrap_or_default();
                while !items.is_empty() {
                    let n = up.batch_size(w + 2 * prep::PAD, h + 2 * prep::PAD).min(items.len());
                    let rest = items.split_off(n);
                    flush(items, w, h, &mut done)?;
                    items = rest;
                    if failed.lock().unwrap().is_some() || !progress(Progress { stage: "generate", done, total }) {
                        cancelled = true;
                        break;
                    }
                }
                if cancelled {
                    break;
                }
            }
        }
        Ok(())
        })();
        drop(tx);
        result
    })?;
    if let Some(e) = failed.lock().unwrap().take() {
        return Err(e.into());
    }
    let complete = !cancelled && wanted.iter().all(|n| out.join(format!("{n}.png")).exists());
    let manifest = Manifest {
        generator: generator_id(),
        model_sha256: model_sha256(),
        processing: PROCESSING,
        complete,
        textures: entries.len(),
        generated: wanted.iter().filter(|n| out.join(format!("{n}.png")).exists()).count(),
        kept: kept.into_iter().collect(),
        adapter,
        seconds: t.elapsed().as_secs_f64(),
    };
    let tmp = out.join("manifest.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&manifest)?)?;
    std::fs::rename(&tmp, out.join("manifest.json"))?;
    Ok(manifest)
}
