use gdl_formats::texdef::TextureTable;
use gdl_formats::wad::{Entry, Limits, Wad};
use serde::{Deserialize, Serialize};
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
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AnimationSampleRequest {
    tree: usize,
    node: u32,
    sequence: u32,
    frame: f32,
    transition: Option<TransitionSample>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransitionSample {
    from: [f32; 9],
    fraction: f32,
    mirror: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClockRequest {
    state: gdl_core::animation_clock::ClockState,
    now: f32,
    frames: i16,
    anim_time_scale: f32,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClockStartRequest {
    state: gdl_core::animation_clock::ClockState,
    input: gdl_core::animation_clock::ClockInit,
    evaluate: bool,
    sampled: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AnimationSelectionRequest {
    state: gdl_core::animation_clock::ClockState,
    current: gdl_core::animation_clock::ClipTiming,
    requested: gdl_core::animation_clock::ClipTiming,
    input: gdl_core::animation_clock::SelectionInput,
    sampled: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlayerAnimationRequest {
    state: gdl_core::animation_clock::ClockState,
    context: gdl_core::player_animation::ActionContext,
    action_map: Vec<i32>,
    now: f32,
    anim_time_scale: f32,
    sampled: bool,
    events: Option<gdl_core::player_animation::ActionEventInput>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorldMatrixRequest {
    leaf: gdl_core::world_transform::MatrixBox,
    parents: Vec<gdl_core::world_transform::MatrixBox>,
    offset: Option<[f32; 3]>,
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
                    let channel = if matches!(bitmap.format, 0 | 16 | 48) {
                        "binary-texa-selector"
                    } else {
                        "source-alpha"
                    };
                    let name = format!("{:06}-{channel}.png", bitmap.index);
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
        "pixel_decoder_version":gdl_formats::texdef::BASE_RGBA_DECODER_VERSION,
        "texdef_sha256":hash(&bytes),"textures_sha256":pixels.as_ref().map(|p|hash(p)),
        "decode_scope":"candidate base pixels; 8-bit alpha preserved; CT16 alpha selector shown binary, TEXA/material blending/mipmaps unverified",
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
    if args.len() == 3 && args[0] == "critter-inspect" {
        // critter-inspect CRITTER.WAD NEW_REPORT.json: chunk table and record labels.
        use gdl_formats::critter::{label, CritterWad};
        let b = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let w = CritterWad::parse(&b)?;
        let i16_ = |r: &[u8], o: usize| i16::from_le_bytes([r[o], r[o + 1]]);
        let i32_ = |r: &[u8], o: usize| i32::from_le_bytes(r[o..o + 4].try_into().unwrap());
        let report = serde_json::json!({
            "source_sha256":hash(&b),"chunks":w.chunks,
            "types":w.types.iter().map(|t| serde_json::json!({"name":label(t,0,16),"nodes":[label(t,0x20,16),label(t,0x30,16),label(t,0x40,16)],
                "desc":i16_(t,0x50),"etype_slot":i16_(t,0x52),"flags_5c":format!("{:#x}",i32_(t,0x5c)),
                "moves":[i16_(t,0x110),i16_(t,0x112)],"patterns":[i16_(t,0x114),i16_(t,0x116)],"colnodes":[i16_(t,0x118),i16_(t,0x11a)],
                "child_type":i16_(t,0x11c),"alias_type":i16_(t,0x11e)})).collect::<Vec<_>>(),
            "descs":w.descs.iter().map(|d| serde_json::json!({"name":label(d,0,16),"prefix":label(d,0x10,16),"kind":i16_(d,0x20)})).collect::<Vec<_>>(),
            "addas":w.addas.iter().map(|a| serde_json::json!({"type":i16_(a,0),"flags":i16_(a,2),"tree":label(a,0x10,8),"node":label(a,0x18,8)})).collect::<Vec<_>>(),
            "nodes":w.nodes.iter().map(|n| label(n,0,16)).collect::<Vec<_>>(),
            "moves":w.moves.iter().map(|m| serde_json::json!({"type":i32_(m,0),"flags":format!("{:#x}",i32_(m,4)),"priority":i32_(m,8),
                "name":label(m,0x10,16),"seq":label(m,0x20,16),"node":label(m,0x30,16)})).collect::<Vec<_>>(),
            "patterns":w.patterns.iter().map(|p| (0..8).map(|k| i16_(p,0x20 + 2 * k)).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "damages":w.damages.iter().map(|d| i16_(d,0)).collect::<Vec<_>>(),
            "sfx":w.sfx.iter().map(|x| serde_json::json!({"flags":format!("{:#x}",i32_(x,0)),"name":label(x,0x10,16),"sound":label(x,0x20,32)})).collect::<Vec<_>>(),
            "scope":"structure of CritterInitHeader tables; labels only, semantics in research/critters.md"});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "audio-inspect" {
        // audio-inspect FILE(.VBK|.ADS|AUDATPS2.ROM) NEW_REPORT.json: tables + decoded lengths.
        let b = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let report = if b.starts_with(b"VBNK") {
            let v = gdl_formats::audio::Vbk::parse(&b)?;
            let lens: Vec<_> = (0..v.samples.len()).map(|k| v.decode(&b, k).map(|p| p.len())).collect::<Result<_, _>>()?;
            serde_json::json!({"bank":v,"decoded_samples":lens})
        } else if b.starts_with(b"SShd") {
            let a = gdl_formats::audio::Ads::parse(&b)?;
            let pcm = a.decode(&b);
            let peak = pcm.iter().map(|s| i32::from(*s).abs()).max().unwrap_or(0);
            serde_json::json!({"stream":a,"frames":pcm.len() / a.channels as usize,"peak":peak})
        } else {
            serde_json::json!({"rom":gdl_formats::audio::AudioRom::parse(&b)?})
        };
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &serde_json::json!({"source_sha256":hash(&b),"report":report}))?;
        return Ok(());
    }
    if args.len() == 20 && args[0] == "profile-check" {
        // profile-check GAME_START.json SLUS_200.47 CLASS_ANIM.PS2 PDATA×16 (class order):
        // rebuild player 0 from its save region (PlayerLoadSaveFile path) and
        // list the record offsets that differ from the captured record (stdout).
        use gdl_core::game::profile;
        let start: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?)?;
        let elf_bytes = bounded_read(Path::new(&args[2]), 16 * 1024 * 1024)?;
        let elf = gdl_formats::elf::Elf::parse(&elf_bytes)?;
        let tables = gdl_core::player_class::ClassTables::from_elf(&elf)?;
        let mut pdats = Vec::new();
        for a in &args[4..20] {
            pdats.push(gdl_formats::pdata::PlayerData::parse(&bounded_read(Path::new(a), 1024 * 1024)?)?.pdat);
        }
        let pdats = profile::Pdats(pdats);
        let h = start["players"][0].as_str().ok_or("players[0]")?;
        let captured: Vec<u8> = (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16)).collect::<Result<_, _>>()?;
        let region = &captured[profile::SAVE_START..profile::SAVE_START + profile::SAVE_LEN];
        println!("summary {:?}", profile::summary(region));
        let (mut r, controls) = profile::load_character(0, region, &pdats, &tables).map_err(|e| e.to_string())?;
        let anim = gdl_formats::animation::Animations::parse(&bounded_read(Path::new(&args[3]), 64 * 1024 * 1024)?)?;
        let heart = elf.object("heart_rate")?.first().copied().unwrap_or(0);
        // init_next_level for G1 (game_wave 0x700): level start, then PlayerSaveState(k, 1).
        profile::level_start(&mut r, &pdats, &tables.action_map(&anim), 0x700, heart).map_err(|e| e.to_string())?;
        profile::player_save_state(&mut r, true, 7, 0, &controls);
        println!("controls {controls:?} heart_rate {heart}");
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for o in (0..captured.len()).step_by(4) {
            if captured[o..o + 4] != r.0[o..o + 4] {
                match runs.last_mut() {
                    Some(l) if l.1 == o => l.1 = o + 4,
                    _ => runs.push((o, o + 4)),
                }
            }
        }
        for (a, b) in runs {
            let w = |v: &[u8], o: usize| u32::from_le_bytes(v[o..o + 4].try_into().unwrap());
            println!("{a:#06x}..{b:#06x} captured {:#010x} native {:#010x}", w(&captured, a), w(&r.0, a));
        }
        return Ok(());
    }
    if args.len() == 3 && args[0] == "objects-bitmaps" {
        // objects-bitmaps OBJECTS.PS2 TEXDEF.PS2: each object bitmap record with
        // its flags and the TEXDEF candidates (offset/format/size match) (stdout).
        let o = gdl_formats::objects::Objects::parse(&bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?)?;
        let t = gdl_formats::texdef::TextureTable::parse(&bounded_read(Path::new(&args[2]), 64 * 1024 * 1024)?)?;
        for b in &o.bitmap_records {
            let c: Vec<String> = t
                .bitmaps
                .iter()
                .filter(|x| x.texture_offset == b.texture_offset && x.format == b.format && x.width == b.width && x.height == b.height)
                .map(|x| x.label.clone())
                .collect();
            println!("{} fmt {} flags {:#x} {}x{} off {:#x} candidates {:?}", b.index, b.format, b.flags, b.width, b.height, b.texture_offset, c);
        }
        return Ok(());
    }
    if args.len() == 2 && args[0] == "memcard-list" {
        // memcard-list CARD.ps2: read-only directory listing with file hashes (stdout).
        let b = bounded_read(Path::new(&args[1]), 16 * 1024 * 1024)?;
        let card = gdl_formats::memcard::MemoryCard::parse(&b)?;
        println!("card sha256 {} stride {} flags {:#x}", hash(&b), card.superblock.stride, card.superblock.card_flags);
        for d in card.root()? {
            println!("{:#06x} {:>8} {}", d.mode, d.length, d.name);
            if d.is_dir() {
                for f in card.list(&d)? {
                    let h = if f.is_dir() { String::new() } else { hash(&card.read(&f)?) };
                    println!("    {:#06x} {:>8} {} {}", f.mode, f.length, f.name, h);
                }
            }
        }
        return Ok(());
    }
    if args.len() == 2 && args[0] == "anim-trees" {
        // anim-trees ANIM.PS2: tree labels with sequence label/frames/timing (stdout).
        let anim = gdl_formats::animation::Animations::parse(&bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?)?;
        for t in &anim.trees {
            let seqs: Vec<String> = t.sequences.iter().map(|s| format!("{}:{}x{}", s.label, s.frames, s.timing_factor)).collect();
            println!("{}\t{}\t{}\t{}", t.index, t.label, t.nodes.len(), seqs.join(" "));
        }
        return Ok(());
    }
    if args.len() == 6 && args[0] == "player-sim" {
        // player-sim WORLDS.PS2 SLUS_200.47 START.json INPUTS.json NEW_REPORT.json
        // INPUTS: [[levels, edges], ...] one per game update.
        let worlds = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let mut world = gdl_core::collision::CollisionWorld::from_world(&gdl_formats::world::World::parse(&worlds)?)?;
        let elf_bytes = bounded_read(Path::new(&args[2]), 64 * 1024 * 1024)?;
        let tables = gdl_core::controls::tables_from_elf(&gdl_formats::elf::Elf::parse(&elf_bytes)?)?;
        let start: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(&args[3]), 4 * 1024 * 1024)?)?;
        let mut sim: gdl_core::player_sim::PlayerSim = serde_json::from_value(start["sim"].clone())?;
        let inputs: Vec<[u32; 2]> = serde_json::from_slice(&bounded_read(Path::new(&args[4]), 4 * 1024 * 1024)?)?;
        if inputs.len() > 100_000 {
            return Err("simulation input policy exceeded".into());
        }
        let mut updates = Vec::new();
        for [levels, edges] in inputs {
            match sim.step(&mut world, &tables, levels, edges) {
                Ok(report) => updates.push(serde_json::json!({"report":report,
                    "position_bits":sim.position.map(f32::to_bits),"facing_bits":sim.facing.to_bits(),
                    "floor_y_bits":sim.floor_y_88c.to_bits(),"error":null})),
                Err(e) => {
                    updates.push(serde_json::json!({"error":e.to_string()}));
                    break;
                }
            }
        }
        let report = serde_json::json!({"updates":updates,"scope":"interim closed-loop PlayerSim; see player_sim.rs labels"});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[5])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 4 && args[0] == "level-enemies" {
        // level-enemies WDATA.wad LEVEL_INDEX NEW_REPORT.json
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let world = gdl_formats::wdata::WorldData::parse(&raw)?;
        let index: usize = args[2].to_str().ok_or("level index")?.parse()?;
        let level = world.levels.get(index).ok_or("level index out of range")?;
        let world_kind = 0; // world_data[0] is checked only against tower (0xd).
        let types = gdl_core::level::enemy_types(&world, level, world_kind, 3)?;
        let report = serde_json::json!({"wdata_sha256":hash(&raw),"level":level,"enemies":world.enemies,
            "chunks":world.chunks,"enemy_types":types});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[3])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 7 && args[0] == "item-records" {
        // item-records WORLDS.PS2 WDATA.wad LEVEL SLUS_200.47 SESSION.json NEW_REPORT.json
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let world = gdl_formats::world::World::parse(&raw)?;
        let wraw = bounded_read(Path::new(&args[2]), 4 * 1024 * 1024)?;
        let wdata = gdl_formats::wdata::WorldData::parse(&wraw)?;
        let index: usize = args[3].to_str().ok_or("level index")?.parse()?;
        let level = wdata.levels.get(index).ok_or("level index out of range")?;
        let elf_raw = bounded_read(Path::new(&args[4]), 64 * 1024 * 1024)?;
        let elf = gdl_formats::elf::Elf::parse(&elf_raw)?;
        let session: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(&args[5]), 1024 * 1024)?)?;
        let int = |k: &str| -> Result<i64, Box<dyn std::error::Error>> {
            session[k].as_i64().ok_or_else(|| format!("session field {k}").into())
        };
        let cur_world = int("cur_world")? as i32;
        let world_kind = if cur_world == 0xd { 0xd } else { 0 };
        let types = gdl_core::level::enemy_types(&wdata, level, world_kind, int("mode_950")? as i32)?;
        let level_raw = gdl_core::level::resolved_level(level, &elf, int("difficulty")? as i32)?;
        let session = gdl_core::items::ItemSession {
            multipliers: gdl_core::level::multipliers_cc(&level_raw),
            enemy_kind: types.kind,
            gen_kind: types.gen_kind,
            boss_type: int("boss_type")? as i32,
            cur_world,
            num_virtual_players: int("num_virtual_players")? as i32,
            game_mode: int("game_mode")? as u32,
            global_958: int("global_958")? as i32,
            global_970: int("global_970")? as i32,
            random_item_num: int("random_item_num")? as u32,
            golem_statue_anim: int("golem_statue_anim")? != 0,
            death_statue_anim: int("death_statue_anim")? != 0,
            critter_3_loaded: int("critter_3_loaded")? != 0,
            critter_7_loaded: int("critter_7_loaded")? != 0,
        };
        let tables = gdl_core::items::ItemTables::from_elf(&elf)?;
        let mut collision = gdl_core::collision::CollisionWorld::from_world(&world)?;
        let seed = int("rng_seed")? as u32;
        let loaded = gdl_core::items::load_items(&world, &mut collision, tables, session, seed)?;
        let w = &loaded.world;
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        let report = serde_json::json!({
            "worlds_sha256":hash(&raw),"wdata_sha256":hash(&wraw),"elf_sha256":hash(&elf_raw),
            "records":w.records.iter().map(|r| hex(&r.0)).collect::<Vec<_>>(),
            "outcome":loaded.outcome.iter().map(|o| match o { Ok(s) => serde_json::json!({"slot":s}), Err(e) => serde_json::json!({"error":e}) }).collect::<Vec<_>>(),
            "definitions":w.definitions.iter().map(|d| hex(&d.0)).collect::<Vec<_>>(),
            "wobjs":w.wobjs.iter().map(|o| serde_json::json!({"object":o.object,"inity":o.inity.to_bits(),"y":o.y.to_bits(),
                "starty":o.starty.to_bits(),"endy":o.endy.to_bits(),"snd":o.snd.to_bits()})).collect::<Vec<_>>(),
            "objects":w.objects.iter().map(|o| [o.flags_10, u32::from(o.word_14), u32::from(o.byte_16), u32::from(o.byte_17)]).collect::<Vec<_>>(),
            "world_has_gates":w.world_has_gates,"max_level_coins":loaded.max_level_coins,"random_calls":loaded.random_calls,
            "rune_item":w.rune_item,"quest_item":w.quest_item,
            "scope":"native AddItemInstList through PlaceItem; presentation bytes zero; ProcessItemWobjs not applied; rand seeded from rng_seed (values not comparable)"});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[6])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() >= 9 && args[0] == "game-run" {
        // game-run WORLDS.PS2 WDATA.wad LEVEL SLUS_200.47 START_V4.json ITEMS_ANIM.PS2 FRAMES NEW_REPORT.json [TYPE=MONSTER_ANIM.PS2 ...]
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let world = gdl_formats::world::World::parse(&raw)?;
        let wraw = bounded_read(Path::new(&args[2]), 4 * 1024 * 1024)?;
        let wdata = gdl_formats::wdata::WorldData::parse(&wraw)?;
        let level: usize = args[3].to_str().ok_or("level")?.parse()?;
        let elf_raw = bounded_read(Path::new(&args[4]), 64 * 1024 * 1024)?;
        let elf = gdl_formats::elf::Elf::parse(&elf_raw)?;
        let mut start: gdl_core::game::setup::StartState = serde_json::from_slice(&bounded_read(Path::new(&args[5]), 16 * 1024 * 1024)?)?;
        let iraw = bounded_read(Path::new(&args[6]), 64 * 1024 * 1024)?;
        let ianim = gdl_formats::animation::Animations::parse(&iraw)?;
        let frames: usize = args[7].to_str().ok_or("frames")?.parse()?;
        let mut item_anims = std::collections::HashMap::new();
        for (k, t) in ianim.trees.iter().enumerate() {
            item_anims.insert(t.label.clone(), gdl_core::game::setup::clips_of(&ianim, k));
        }
        let mut enemy_anims = gdl_core::game::enemy_ai::EnemyAnimData::default();
        let mut opts = std::collections::HashMap::new();
        let mut critters = Vec::new();
        for a in &args[9..] {
            let a = a.to_str().ok_or("monster arg")?;
            let (t, path) = a.split_once('=').ok_or("TYPE=PATH:PREFIX expected")?;
            if t == "critter" {
                // critter=CRITTER.WAD[:MODEL_ANIM.PS2]
                let (wad, model) = match path.split_once(':') {
                    Some((w, m)) => (w, Some(m)),
                    None => (path, None),
                };
                let w = gdl_formats::critter::CritterWad::parse(&bounded_read(Path::new(wad), 4 * 1024 * 1024)?)?;
                let m = match model {
                    Some(m) => {
                        let bytes = bounded_read(Path::new(m), 64 * 1024 * 1024)?;
                        Some(std::sync::Arc::new((gdl_formats::animation::Animations::parse(&bytes)?, bytes)))
                    }
                    None => None,
                };
                critters.push((wad.to_string(), w, m));
                continue;
            }
            if t.parse::<i32>().is_err() {
                opts.insert(t.to_string(), path.to_string());
                continue;
            }
            let (path, prefix) = path.rsplit_once(':').ok_or("TYPE=PATH:PREFIX expected")?;
            let m = gdl_formats::animation::Animations::parse(&bounded_read(Path::new(path), 64 * 1024 * 1024)?)?;
            gdl_core::game::setup::enemy_trees(&mut enemy_anims, t.parse()?, prefix, &m);
        }
        if let Some(extra) = opts.get("items2") {
            // Further item packages (the realm's, after the level's own).
            let more = gdl_formats::animation::Animations::parse(&bounded_read(Path::new(extra), 64 * 1024 * 1024)?)?;
            for (k, t) in more.trees.iter().enumerate() {
                item_anims.entry(t.label.clone()).or_insert_with(|| gdl_core::game::setup::clips_of(&more, k));
            }
        }
        if let Some(w) = opts.get("world") {
            // Other realms: CurWorld/CurLevel for the session tables (the
            // player record is still the one from the save).
            start.globals.cur_world = w.parse()?;
            start.globals.cur_level = level as i32;
        }
        let anim_of = |k: &str| -> Result<gdl_formats::animation::Animations, Box<dyn Error>> {
            let path = opts.get(k).ok_or_else(|| format!("{k}=PATH required"))?;
            Ok(gdl_formats::animation::Animations::parse(&bounded_read(Path::new(path), 64 * 1024 * 1024)?)?)
        };
        let effect_tables = gdl_core::game::effects::EffectTables::from_elf(&elf, &anim_of("weapons")?, &anim_of("powerups")?)?;
        let mut game = gdl_core::game::Game::new_level(gdl_core::game::setup::LevelInputs {
            world: &world, wdata: &wdata, level, elf: &elf, start, enemy_anims, item_anims, effect_tables,
            audio_rom: match opts.get("rom") {
                Some(p) => Some(gdl_formats::audio::AudioRom::parse(&bounded_read(Path::new(p), 4 * 1024 * 1024)?)?),
                None => None,
            },
            mb_objects: match opts.get("objects") {
                Some(list) => {
                    let mut set = std::collections::HashSet::new();
                    for p in list.split(',') {
                        let o = gdl_formats::objects::Objects::parse(&bounded_read(Path::new(p), 64 * 1024 * 1024)?)?;
                        set.extend(o.definitions.iter().map(|d| d.label.clone()));
                    }
                    Some(set)
                }
                None => None,
            },
            critters })?;
        // Optional scripted player: sim=START_SIM.json clips=SOR_CLIPS.PS2 script=LEVELS_HEX:UPDATES,...
        let mut driver = match (opts.get("sim"), opts.get("script")) {
            (Some(sim_path), Some(_)) => {
                let st: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(sim_path), 4 * 1024 * 1024)?)?;
                let sim: gdl_core::player_sim::PlayerSim = serde_json::from_value(st["sim"].clone())?;
                let map: Vec<[i64; 2]> = serde_json::from_value(st["presentation"]["action_map"].clone())?;
                let clips = anim_of("clips")?;
                let tables = gdl_core::controls::tables_from_elf(&elf)?;
                Some(gdl_core::game::players::PlayerDriver::new(sim, tables, map, gdl_core::game::setup::clips_of(&clips, 0)))
            }
            _ => None,
        };
        if let (Some(d), true) = (driver.as_mut(), 0 < level || opts.contains_key("world")) {
            // Later levels: enter at the start locator (get_player_pos path).
            let loc = world.locators.iter().find(|l| l.kind == 7 && l.id == 0).ok_or("no start locator")?;
            game.place_player_start(0, &mut d.sim, loc.position, loc.rotation[1]).map_err(|e| e.to_string())?;
            d.sim.current_action = 0;
            d.sim.requested_action = 0;
        }
        if let (Some(t), Some(d)) = (opts.get("teleport"), driver.as_mut()) {
            // DEBUG: start player 0 at x,y,z through the start-locator path
            // (not a fidelity input; recorded in the report).
            let v: Vec<f32> = t.split(',').map(str::parse).collect::<Result<_, _>>()?;
            let p = [*v.first().ok_or("teleport x")?, *v.get(1).ok_or("teleport y")?, *v.get(2).ok_or("teleport z")?];
            game.place_player_start(0, &mut d.sim, p, 0.0).map_err(|e| e.to_string())?;
            d.sim.current_action = 0;
            d.sim.requested_action = 0;
        }
        if let Some(h) = opts.get("boss_health") {
            // DEBUG: start the boss with this health (not a fidelity input).
            let h: f32 = h.parse()?;
            for c in game.critters.iter_mut().flatten() {
                c.rec.set_f32(0x4f4, h);
            }
        }
        if let Some(n) = opts.get("potions") {
            // DEBUG override recorded in the report scope.
            game.players[0].set_i32(0x1a18, n.parse()?);
        }
        let mut script: Vec<u32> = Vec::new();
        if let Some(sc) = opts.get("script") {
            for part in sc.split(',') {
                let (lv, n) = part.split_once(':').ok_or("script LEVELS_HEX:UPDATES")?;
                let lv = u32::from_str_radix(lv, 16)?;
                script.extend(std::iter::repeat_n(lv, n.parse::<usize>()?));
            }
        }
        let mut samples = Vec::new();
        for f in 0..frames {
            let mut levels = script.get(f).copied().unwrap_or(0);
            if levels & 0x8000_0000 != 0 {
                // Tool-only token: magic button edge (interim use_potion).
                levels &= 0x7fff_ffff;
                if let Some(d) = driver.as_mut() {
                    d.magic_pressed = true;
                }
            }
            match driver.as_mut() {
                Some(d) => game.step_with(None, &mut |g| g.drive_player(0, d, levels)),
                None => game.step(None),
            }
            if f % 15 == 0 || f + 1 == frames {
                let enemies: Vec<_> = game.enemies.iter().enumerate().filter(|(_, e)| e.i32_(0xc8) != 0)
                    .map(|(k, e)| serde_json::json!({"slot":k,"type":e.i32_(0),"state":e.i32_(0xc8),"pos":e.v3(0x40),
                        "action":e.u32_(0xe0),"logic":e.i16_(0x324),"health":e.f32_(0x214),"target":e.i16_(0x288)})).collect();
                let effects: Vec<_> = (0..game.effects.num).filter(|&k| 0.0 < game.effects.slots[k].f32_(0x68))
                    .map(|k| { let r = &game.effects.slots[k]; serde_json::json!({"slot":k,"node":game.effects.nodes[k],"flags":r.u32_(0x64),
                        "kind":r.u32_(0x60),"end":r.f32_(0x68),"vel":r.v3(0x80),"grav":r.f32_(0xa0),"dflags":r.u32_(0xb4),"owner":r.i16_(0xbc),"link":r.i16_(0xe8)}) }).collect();
                let critters: Vec<_> = game.critters.iter().enumerate().filter_map(|(k, c)| c.as_ref().map(|c| serde_json::json!({
                    "slot":k,"state":c.rec.i32_(8),"health":c.rec.f32_(0x4f4),"move":c.rec.i16_(0x124),"next":c.rec.i16_(0x126),
                    "seq":c.clock.sequence,"frame":c.clock.frame,"pos":c.rec.v3(0x40),"target":c.rec.i16_(0x130),"ntargets":c.rec.i16_(0x136)}))).collect();
                samples.push(serde_json::json!({"frame":f,"enemies":enemies,"critters":critters,"player_health":game.players[0].f32_(0x1a10),
                    "level_done":game.level_done,"good_wizard":game.globals.good_wizard,"auto_exit":[game.auto_exit_timer,game.globals.auto_exit_flag],"player_state":game.players[0].i32_(0xfc),
                    "player_pos":game.players[0].v3(0x50),"events":game.events.len(),"effects":effects,
                    "driver":driver.as_ref().map(|d| serde_json::json!({"anim":d.anim,"missiles":d.missiles_started,"event_error":d.event_error,"event_word":d.event_word}))}));
            }
        }
        let sounds: Vec<String> = game.events.iter().filter(|e| matches!(e, gdl_core::game::Event::Sound { .. })).take(200).map(|e| format!("{e:?}")).collect();
        let events: Vec<String> = game.events.iter().filter(|e| !matches!(e, gdl_core::game::Event::Audio { .. } | gdl_core::game::Event::Sound { .. })).take(400).map(|e| format!("{e:?}")).collect();
        let generators: Vec<_> = game.items.records.iter().enumerate()
            .filter(|(_, r)| r.active() && game.items.definitions[r.definition()].kind() == 3)
            .map(|(k, r)| serde_json::json!({"slot":k,"name":String::from_utf8_lossy(game.items.definitions[r.definition()].name()),
                "type":r.i16_(0xf0),"count":r.i8_(0xf2),"max":r.i8_(0xf3),"level":r.i8_(0xf6),"logic":r.i8_(0xf7),
                "d8":r.u16_(0xd8),"pos":r.v3(0x60)})).collect();
        let report = serde_json::json!({"frames":frames,"samples":samples,"events":events,"sounds":sounds,"generators":generators,
            "level_multipliers_cc":(0..5).map(|k| game.level_f32(0xcc + 4 * k)).collect::<Vec<_>>(),
            "critter_files":game.critter_data.files.iter().map(|f| serde_json::json!({"name":f.name,"trees":f.trees,"model":f.model.is_some()})).collect::<Vec<_>>(),
            "critter_etypes":game.critter_data.etypes.iter().map(|r| r.iter().map(|t| t.map(|t| [t.0, t.1])).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "lookouts":game.lookouts.iter().map(|l| serde_json::json!({"pos":l.position,"next":l.next,"param":l.param})).collect::<Vec<_>>(),
            "unsupported":game.log.unsupported.iter().map(|(k, v)| serde_json::json!({"path":k,"count":v})).collect::<Vec<_>>(),
            "scope":"INTERIM native game run: fresh items, no initial enemies, optional scripted interim player (0x80000000 = magic edge), optional potions= debug override; not a fidelity comparison",
            "potions_override":opts.get("potions"),"boss_health_override":opts.get("boss_health"),"teleport_override":opts.get("teleport")});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[8])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "place-items" {
        // place-items WORLDS.PS2 NEW_REPORT.json — PlaceItem 0x153068 heights.
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let world = gdl_formats::world::World::parse(&raw)?;
        let mut collision = gdl_core::collision::CollisionWorld::from_world(&world)?;
        let mut out = Vec::new();
        for item in &world.items {
            let def = &world.item_definitions[item.definition as usize];
            let flags_0a = u16::from_le_bytes([def.raw[10], def.raw[11]]);
            if flags_0a & 1 != 0 {
                out.push(serde_json::json!({"index":item.index,"skipped":"definition +0xa bit 0"}));
                continue;
            }
            let p = item.position;
            out.push(match collision.floor_pos(p[1], 1.0, p, 0) {
                Ok((y, hit)) => {
                    let placed = gdl_core::numeric::add_guarded_rz(y, f32::from_bits(0x3dcc_cccd))?;
                    serde_json::json!({"index":item.index,"y_bits":placed.to_bits(),
                        "hit_object":hit.map(|h| h.object),"error":null})
                }
                Err(e) => serde_json::json!({"index":item.index,"error":e.to_string()}),
            });
        }
        let report = serde_json::json!({"worlds_sha256":hash(&raw),"items":out,
            "scope":"PlaceItem FloorPos+0.1 with static WorldCollide; dynamic objects excluded"});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "joy-tables" {
        // joy-tables SLUS_200.47 NEW_REPORT.json
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let elf = gdl_formats::elf::Elf::parse(&raw)?;
        let tables = gdl_core::controls::tables_from_elf(&elf)?;
        let (angle, magnitude) = tables.tables();
        let report = serde_json::json!({"elf_sha256":hash(&raw),
            "angle_bits":angle.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),"magnitude_bits":magnitude.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            "scope":"native InitJoyAng from ELF magval/angidx"});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 4 && args[0] == "player-displacement" {
        // player-displacement WORLDS.PS2 STEPS.json NEW_REPORT.json
        #[derive(serde::Deserialize, serde::Serialize, Clone, Copy)]
        #[serde(deny_unknown_fields)]
        struct Step {
            request: gdl_core::player_motion::MotionRequestInput,
            matrix: [[f32; 3]; 3],
            collision_offset_81c: [f32; 3],
            offset_flags_84: u32,
            position: [f32; 3],
            radius_828: f32,
            height_82c: f32,
            floor_y_88c: f32,
            floor_object_89c: Option<usize>,
            flags_8ac: u32,
            word_100: i32,
            y_54: f32,
            wall_object_8a0: Option<usize>,
        }
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let mut world = gdl_core::collision::CollisionWorld::from_world(&gdl_formats::world::World::parse(&raw)?)?;
        let raw_steps = bounded_read(Path::new(&args[2]), 4 * 1024 * 1024)?;
        let steps: Vec<Step> = serde_json::from_slice(&raw_steps)?;
        if steps.len() > 4096 {
            return Err("displacement request policy exceeded".into());
        }
        let mut outcomes = Vec::new();
        for step in steps {
            let result = (|| -> Result<serde_json::Value, Box<dyn Error>> {
                let request = gdl_core::player_motion::motion_request(step.request)?;
                let mut state = gdl_core::player_collision::PlayerFloorState {
                    floor_y_88c: step.floor_y_88c,
                    floor_object_89c: step.floor_object_89c,
                    flags_8ac: step.flags_8ac,
                    word_100: step.word_100,
                    y_54: step.y_54,
                    wall_object_8a0: step.wall_object_8a0,
                    floor_contact: None,
                };
                let out = gdl_core::player_collision::player_displacement(
                    &mut world,
                    gdl_core::player_collision::DisplacementInput {
                        matrix: step.matrix,
                        collision_offset_81c: step.collision_offset_81c,
                        offset_flags_84: step.offset_flags_84,
                        position: step.position,
                        request: request.request,
                        speed: request.speed,
                        radius_828: step.radius_828,
                        height_82c: step.height_82c,
                        flags_8b0: step.request.flags_8b0,
                        current_action: step.request.current_action_1e0,
                        spf: step.request.spf,
                        world_bottom: step.request.world_bottom,
                    },
                    &mut state,
                )?;
                Ok(serde_json::json!({"displacement_bits":out.displacement.map(f32::to_bits),
                    "position_bits":out.position.map(f32::to_bits),"wall_result":out.wall_result,
                    "floor_result":out.floor_result,"floor_y_bits":state.floor_y_88c.to_bits(),
                    "floor_object":state.floor_object_89c,"wall_object":state.wall_object_8a0}))
            })();
            outcomes.push(match result {
                Ok(v) => serde_json::json!({"input":step,"output":v,"error":null}),
                Err(e) => serde_json::json!({"input":step,"output":null,"error":e.to_string()}),
            });
        }
        let report = serde_json::json!({"worlds_sha256":hash(&raw),"steps_sha256":hash(&raw_steps),"outcomes":outcomes,
            "scope":"request + static walls/floor/floor-height/slope + position add; items, players, enemies, camera limit, moving floors and transporters excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[3])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if (args.len() == 3 || args.len() == 4) && args[0] == "collision-query" {
        // collision-query WORLDS.PS2 NEW_REPORT.json [QUERIES.json]
        #[derive(serde::Deserialize, serde::Serialize, Clone, Copy)]
        #[serde(deny_unknown_fields)]
        struct Request {
            radius: f32,
            first: [f32; 3],
            last: [f32; 3],
            check_flags: u32,
            min_normal_y: f32,
            max_normal_y: f32,
            object_mask: u32,
            exclusion_mask: i32,
        }
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let world = gdl_formats::world::World::parse(&raw)?;
        let mut native = gdl_core::collision::CollisionWorld::from_world(&world)?;
        let objects: Vec<_> = native
            .objects
            .iter()
            .map(|o| serde_json::json!({"position_bits":o.position.map(f32::to_bits),"parent":o.parent}))
            .collect();
        let mut outcomes = Vec::new();
        let mut requests_hash = serde_json::Value::Null;
        if args.len() == 4 {
            let raw_requests = bounded_read(Path::new(&args[3]), 4 * 1024 * 1024)?;
            requests_hash = serde_json::json!(hash(&raw_requests));
            let requests: Vec<Request> = serde_json::from_slice(&raw_requests)?;
            if requests.len() > 4096 {
                return Err("collision request policy exceeded".into());
            }
            for r in requests {
                let query = gdl_core::collision::Query {
                    radius: r.radius,
                    first: r.first,
                    last: r.last,
                    check_flags: r.check_flags,
                    min_normal_y: r.min_normal_y,
                    max_normal_y: r.max_normal_y,
                    object_mask: r.object_mask,
                    exclusion_mask: r.exclusion_mask,
                };
                let hit_json = |h: &Option<gdl_core::collision::Hit>| {
                    h.map(|h| serde_json::json!({"object":h.object,"triangle":h.triangle,
                        "distance_bits":h.distance.to_bits(),"position_bits":h.position.map(f32::to_bits)}))
                };
                outcomes.push(match native.world_collide(query) {
                    Ok((c, state)) => {
                        let basis = match c.solid {
                            Some(h) => {
                                let t = native.triangles[h.triangle];
                                gdl_core::collision::create_mat3_norm(t.scale, t.normal)
                                    .map(|m| serde_json::json!(m.map(|row| row.map(f32::to_bits))))
                                    .unwrap_or(serde_json::Value::Null)
                            }
                            None => serde_json::Value::Null,
                        };
                        serde_json::json!({"request":r,"solid":hit_json(&c.solid),"water":hit_json(&c.water),
                            "basis_bits":basis,"direction_bits":state.direction.map(f32::to_bits),
                            "length_bits":state.length.to_bits(),"min_y_bits":state.min_y.to_bits(),
                            "max_y_bits":state.max_y.to_bits(),"check_num":state.check_num,"error":null})
                    }
                    Err(e) => serde_json::json!({"request":r,"error":e.to_string()}),
                });
            }
        }
        let report = serde_json::json!({"worlds_sha256":hash(&raw),"requests_sha256":requests_hash,
            "objects":objects,"outcomes":outcomes,
            "scope":"static WorldCollide subset; dynamic objects, matrix/offset objects and TriLineCol excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "vu0-fqdist" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<[f32; 2]> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("fqdist request policy exceeded".into());
        }
        let outcomes: Vec<_> = requests
            .into_iter()
            .map(|[x, z]| match gdl_core::vu0::fqdist(x, z) {
                Ok((q, zz)) => serde_json::json!({"x":x,"z":z,"x_lane":q,"y_lane":zz,"error":null}),
                Err(e) => serde_json::json!({"x":x,"z":z,"x_lane":null,"y_lane":null,"error":e.to_string()}),
            })
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"fqdist VU0 microprogram 0x44 subset: finite operands, toward-zero VU multiply/add/sqrt"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "motion-request" {
        #[derive(serde::Deserialize, serde::Serialize, Clone, Copy)]
        #[serde(deny_unknown_fields)]
        struct Step {
            request: gdl_core::player_motion::MotionRequestInput,
            forward_40: f32,
            forward_48: f32,
            turn_968: f32,
            position_50: [f32; 3],
        }
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let steps: Vec<Step> = serde_json::from_slice(&raw)?;
        if steps.len() > 4096 {
            return Err("motion request policy exceeded".into());
        }
        let outcomes: Vec<_> = steps
            .into_iter()
            .map(|step| {
                let result = gdl_core::player_motion::motion_request(step.request).and_then(|output| {
                    let facing = gdl_core::player_motion::facing_turn(gdl_core::player_motion::FacingInput {
                        forward_40: step.forward_40,
                        forward_48: step.forward_48,
                        target: output.direction,
                        turn_968: step.turn_968,
                        spf: step.request.spf,
                    })?;
                    // Diagnostic: EE 0x14c14c's scalar add if the request were the
                    // final displacement; collision stages are not evaluated.
                    let mut position = [0.0f32; 3];
                    for ((out, base), delta) in position.iter_mut().zip(step.position_50).zip(output.request) {
                        *out = gdl_core::numeric::add_guarded_rz(base, delta)?;
                    }
                    Ok(serde_json::json!({"request_output":output,"facing":facing,"position_if_request_final":position}))
                });
                match result {
                    Ok(output) => serde_json::json!({"input":step,"output":output,"error":null}),
                    Err(e) => serde_json::json!({"input":step,"output":null,"error":e.to_string()}),
                }
            })
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"ordinary PlayerMotion pre-collision displacement request (+0x860..+0x868) and facing turn (+0x86c); collision, pushes, camera limits, final displacement, pad polling and requested-action mapping excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "motion-code" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<gdl_core::player_input::MotionCodeInput> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("motion code request policy exceeded".into());
        }
        let outcomes: Vec<_> = requests
            .into_iter()
            .map(
                |request| match gdl_core::player_input::motion_code(request) {
                    Ok(output) => {
                        serde_json::json!({"request":request,"output":output,"error":null})
                    }
                    Err(e) => {
                        serde_json::json!({"request":request,"output":null,"error":e.to_string()})
                    }
                },
            )
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"ordinary logical motion-code and run-delay timer subset; pad polling, angle-dependent branches, collision/forced modes, requested-action mapping and displacement excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "world-matrix" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<WorldMatrixRequest> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096
            || requests
                .iter()
                .map(|r| (r.parents.len() + 1) * 64)
                .sum::<usize>()
                > 1_000_000
        {
            return Err("world-matrix request/work policy exceeded".into());
        }
        let outcomes: Vec<_> = requests.into_iter().map(|request| {
            let mut parents = request.parents.clone();
            match gdl_core::world_transform::compose(request.leaf,&mut parents,request.offset) {
                Ok(output) => serde_json::json!({"request":request,"output":output,"parents_after":parents,"error":null}),
                Err(e) => serde_json::json!({"request":request,"output":null,"error":e.to_string()}),
            }
        }).collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"source-derived finite GetWorldMat/MulMat4 experiment, configured VU RZ arithmetic; actual guest world outputs, scale consumers and render composition unaccepted"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 4 && args[0] == "player-anim-select" {
        let source = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let animations = gdl_formats::animation::Animations::parse(&source)?;
        let tree = animations.trees.first().ok_or("animation tree missing")?;
        let raw = bounded_read(Path::new(&args[2]), 4 * 1024 * 1024)?;
        let requests: Vec<PlayerAnimationRequest> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("player animation request policy exceeded".into());
        }
        let timing = |id: u16| -> Result<_, Box<dyn Error>> {
            let clip = tree
                .sequences
                .get(usize::from(id))
                .ok_or("sequence ID out of range")?;
            Ok(gdl_core::animation_clock::ClipTiming {
                frames: clip.frames,
                timing_factor: clip.timing_factor,
                repeat: clip.repeat,
                root_transfer_flags: clip.root_transfer_flags,
            })
        };
        let outcomes: Vec<_> = requests.into_iter().map(|request| {
            let result = (|| -> Result<_, Box<dyn Error>> {
                if request.action_map.len() != 150 || request.context.frame.to_bits() != request.state.frame.to_bits() {
                    return Err("action map/frame differs from supported request layout".into());
                }
                let chosen = gdl_core::player_animation::choose(request.context)?;
                let mapped = request.action_map[chosen.action as usize].max(0);
                let sequence = u16::try_from(mapped)?;
                let mut state = request.state;
                state.option_bits |= 2;
                state.loop_word = chosen.loop_word_before_selection;
                let mut output = gdl_core::animation_clock::select(state, timing(state.sequence)?, timing(sequence)?,
                    gdl_core::animation_clock::SelectionInput { sequence, mode: chosen.mode,
                        start_frame: chosen.start_frame, blend_duration: chosen.blend_duration,
                        playback_scale: chosen.playback_scale, now: request.now,
                        anim_time_scale: request.anim_time_scale })?;
                if request.sampled && output.state.reset_byte != 0 {
                    output.state.reset_byte = 2;
                }
                let current_action = if output.return_flags != 0 { chosen.action } else { request.context.current_action };
                let events = request.events.map(|input|
                    gdl_core::player_animation::consume_events(request.context, chosen, output.return_flags, input)
                ).transpose()?;
                Ok(serde_json::json!({"selection":chosen,"animation":output,"current_action":current_action,
                    "attack_type":gdl_core::player_animation::attack_type(current_action),"events":events}))
            })();
            match result {
                Ok(output) => serde_json::json!({"request":request,"output":output,"error":null}),
                Err(e) => serde_json::json!({"request":request,"output":null,"error":e.to_string()}),
            }
        }).collect();
        let report = serde_json::json!({"source_sha256":hash(&source),"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"bounded DoPlayerAction animation-input branches + valid AnimateTree/clock, optional sampling handshake and ordinary Sorceress event-word/missile-request timing; requested action producer, projectile creation/hits, other gameplay side effects and world/root transforms excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[3])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "anim-select" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<AnimationSelectionRequest> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("animation selection request policy exceeded".into());
        }
        let outcomes: Vec<_> = requests
            .into_iter()
            .map(|request| {
                match gdl_core::animation_clock::select(
                    request.state,
                    request.current,
                    request.requested,
                    request.input,
                ) {
                    Ok(mut output) => {
                        if request.sampled && output.state.reset_byte != 0 {
                            output.state.reset_byte = 2;
                        }
                        serde_json::json!({"request":request,"output":output,"error":null})
                    }
                    Err(e) => {
                        serde_json::json!({"request":request,"output":null,"error":e.to_string()})
                    }
                }
            })
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"valid-sequence AnimateTree selection with supplied action/clip/mode/blend inputs and optional tagged-node sampling reset handshake; cache setup, root transfer, player action choices and events excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "anim-clock-start" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<ClockStartRequest> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("clock start request policy exceeded".into());
        }
        let outcomes: Vec<_> = requests
            .into_iter()
            .map(|request| {
                let result = (|| {
                    let mut state =
                        gdl_core::animation_clock::initialize(request.state, request.input)?;
                    if request.evaluate {
                        state = gdl_core::animation_clock::advance(
                            state,
                            request.input.now,
                            request.input.frames,
                            request.input.anim_time_scale,
                        )?
                        .state;
                    }
                    // Source DoAnimation's reset handshake, only requested when the
                    // fixture proves tagged skeletal nodes were actually sampled.
                    if request.sampled && state.reset_byte != 0 {
                        state.reset_byte = 2;
                    }
                    Ok::<_, gdl_core::numeric::UnsupportedNumericDomain>(state)
                })();
                match result {
                    Ok(state) => serde_json::json!({"request":request,"state":state,"error":null}),
                    Err(e) => {
                        serde_json::json!({"request":request,"state":null,"error":e.to_string()})
                    }
                }
            })
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"valid-sequence InitAnim, optional CalcAnimInfo and source sampling reset handshake; requested action/sequence/blend inputs must be supplied, not predicted"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "anim-clock" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<ClockRequest> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("clock request policy exceeded".into());
        }
        let outcomes: Vec<_> = requests
            .into_iter()
            .map(|request| {
                let result = gdl_core::animation_clock::advance(
                    request.state,
                    request.now,
                    request.frames,
                    request.anim_time_scale,
                );
                match result {
                    Ok(output) => {
                        serde_json::json!({"request":request,"output":output,"error":null})
                    }
                    Err(e) => {
                        serde_json::json!({"request":request,"output":null,"error":e.to_string()})
                    }
                }
            })
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"finite CalcAnimInfo clock state; input/action transitions, InitAnim, sampling, events and game loop excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if (args.len() == 4 && args[0] == "anim-samples")
        || (args.len() == 5 && args[0] == "anim-transforms")
    {
        let transforms = args[0] == "anim-transforms";
        let bytes = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let rig_bytes = if transforms {
            Some(bounded_read(Path::new(&args[2]), 64 * 1024 * 1024)?)
        } else {
            None
        };
        let rig = rig_bytes
            .as_ref()
            .map(|b| gdl_formats::animation::Animations::parse(b))
            .transpose()?;
        let requests_raw = bounded_read(
            Path::new(&args[if transforms { 3 } else { 2 }]),
            4 * 1024 * 1024,
        )?;
        let requests: Vec<AnimationSampleRequest> = serde_json::from_slice(&requests_raw)?;
        if requests.len() > 4096 {
            return Err("animation request count policy exceeded".into());
        }
        let animations = gdl_formats::animation::Animations::parse(&bytes)?;
        let mut outcomes = Vec::new();
        let mut work = 0;
        for request in requests {
            let tree = animations
                .trees
                .get(request.tree)
                .ok_or("tree index out of range")?;
            let track = tree.key_track(&bytes, request.node, request.sequence)?;
            let rig_node = if let Some(rig) = &rig {
                let rig_tree = rig
                    .trees
                    .get(request.tree)
                    .ok_or("rig tree index out of range")?;
                if rig_tree.nodes.len() != tree.nodes.len()
                    || !rig_tree.nodes.iter().zip(&tree.nodes).all(|(a, b)| {
                        a.label == b.label && a.parent == b.parent && a.node_type == b.node_type
                    })
                {
                    return Err("animation/rig node correspondence differs".into());
                }
                Some(
                    rig_tree
                        .nodes
                        .get(request.node as usize)
                        .ok_or("rig node index out of range")?,
                )
            } else {
                None
            };
            work += track.key_frames.len();
            if work > 1_000_000 {
                return Err("animation sample work policy exceeded".into());
            }
            let result: Result<[f32; 9], Box<dyn Error>> =
                gdl_core::animation::sample_channels(&track, request.frame)
                    .map_err(|e| Box::new(e) as Box<dyn Error>);
            let result = result.and_then(|values| {
                if let Some(blend) = &request.transition
                    && track.transform_flags & 0x0fff != 0
                {
                    let from = if blend.fraction == 0.0 {
                        values
                    } else {
                        blend.from
                    };
                    let fraction = if blend.fraction == 0.0 {
                        1.0
                    } else {
                        blend.fraction
                    };
                    return gdl_core::animation::blend_channels(
                        from,
                        values,
                        fraction,
                        blend.mirror,
                    )
                    .map_err(|e| Box::new(e) as Box<dyn Error>);
                }
                Ok(values)
            });
            let sample = match result {
                Ok(values) => {
                    if let Some(node) = rig_node {
                        match gdl_core::transform::local_transform(
                            values,
                            node.initial_position,
                            track.transform_flags,
                        ) {
                            Ok((matrix, scale)) => {
                                serde_json::json!({"values":values,"bits":values.map(f32::to_bits),"matrix":matrix,
                            "matrix_bits":matrix.map(f32::to_bits),"scale":scale,"scale_bits":scale.map(f32::to_bits),"error":null})
                            }
                            Err(e) => {
                                serde_json::json!({"values":null,"bits":null,"matrix":null,"matrix_bits":null,"error":e.to_string()})
                            }
                        }
                    } else {
                        serde_json::json!({"values":values,"bits":values.map(f32::to_bits),"error":null})
                    }
                }
                Err(e) => serde_json::json!({"values":null,"bits":null,"error":e.to_string()}),
            };
            outcomes.push(serde_json::json!({"request":request,"sample":sample}));
        }
        let report = serde_json::json!({"source_sha256":hash(&bytes),"requests_sha256":hash(&requests_raw),"rig_sha256":rig_bytes.as_ref().map(|b|hash(b)),
            "scope":"fresh channels and optional local matrices; finite guarded/RZ arithmetic; spacing1..255; clocks/blending/mirror/world transforms/events excluded",
            "outcomes":outcomes});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[if transforms { 4 } else { 3 }])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 5 && args[0] == "anim-keys" {
        let bytes = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let animations = gdl_formats::animation::Animations::parse(&bytes)?;
        let tree_index: usize = args[2].to_str().ok_or("tree index is not UTF-8")?.parse()?;
        let sequence_index: u32 = args[3]
            .to_str()
            .ok_or("sequence index is not UTF-8")?
            .parse()?;
        let tree = animations
            .trees
            .get(tree_index)
            .ok_or("tree index out of range")?;
        let sequence = tree
            .sequences
            .get(sequence_index as usize)
            .ok_or("sequence index out of range")?;
        let mut outcomes = Vec::new();
        let mut key_count = 0;
        for node in tree.nodes.iter().filter(|n| n.node_type == 1) {
            let track = tree.key_track(&bytes, node.index, sequence_index)?;
            key_count += track.key_frames.len();
            if key_count > 100_000 {
                return Err("aggregate decoded-key policy exceeded".into());
            }
            let mut keys = Vec::new();
            let result = gdl_core::animation::visit_keys(&track, |frame, values| {
                keys.push(serde_json::json!({"frame":frame,"values":values,"bits":values.map(f32::to_bits)}));
            });
            outcomes.push(serde_json::json!({"node_index":node.index,"node_label":node.label,
                "transform_flags":track.transform_flags,"initial_only":track.initial_only,"compressed":track.compressed,
                "bitmap_file_offset":track.bitmap_file_offset,"bitmap_raw":track.bitmap_raw,
                "initial_file_offset":track.initial_file_offset,"initial_raw":track.initial_raw,
                "rows_file_offset":track.rows_file_offset,"rows_raw":track.rows_raw,
                "keys":keys,"error":result.err().map(|e|e.to_string())}));
        }
        let report = serde_json::json!({"source_sha256":hash(&bytes),"tree_index":tree_index,
            "sequence_index":sequence_index,"sequence":sequence,"outcomes":outcomes,
            "numeric_contract":"normal finite/zero guarded addition toward zero; PCSX2 2.6.3 FPU.Roundmode=3 reference subset",
            "scope":"exact encoded keys; interpolation, wrap, matrices, transitions and events excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[4])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3
        && (args[0] == "objects-inspect"
            || args[0] == "world-inspect"
            || args[0] == "anim-inspect"
            || args[0] == "anim-initial-frames")
    {
        let bytes = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let report = if args[0] == "anim-initial-frames" {
            let animations = gdl_formats::animation::Animations::parse(&bytes)?;
            let mut samples = Vec::new();
            let mut retained = 0usize;
            for tree in &animations.trees {
                let tree_samples = tree.initial_channels(&bytes)?;
                retained += tree_samples
                    .iter()
                    .map(|s| s.bitmap_raw.len() + s.initial_values_raw.len())
                    .sum::<usize>();
                if retained > 64 * 1024 * 1024 {
                    return Err("aggregate initial-channel byte policy exceeded".into());
                }
                samples.push(tree_samples);
            }
            serde_json::json!({"source_sha256":hash(&bytes),"initial_channels":samples,
                "scope":"fresh frame-zero values only; compressed deltas/interpolation/matrix assembly/playback/events not implemented"})
        } else if args[0] == "anim-inspect" {
            let animations = gdl_formats::animation::Animations::parse(&bytes)?;
            serde_json::json!({"source_sha256":hash(&bytes),"animations":animations,
                "scope":"metadata and translation-only hierarchy; frame sampling/events not implemented"})
        } else if args[0] == "world-inspect" {
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
        return Err("Usage: gdl_tools wad-inspect WAD.BIN NEW_MANIFEST.json\n       gdl_tools wad-extract WAD.BIN NEW_DIRECTORY\n       gdl_tools texdef-inspect TEXDEF.PS2 NEW_DIRECTORY\n       gdl_tools texdef-extract TEXDEF.PS2 TEXTURES.PS2 NEW_DIRECTORY\n       gdl_tools objects-inspect OBJECTS.PS2 NEW_REPORT.json\n       gdl_tools world-inspect WORLDS.PS2 NEW_REPORT.json\n       gdl_tools anim-inspect ANIM.PS2 NEW_REPORT.json\n       gdl_tools anim-initial-frames ANIM.PS2 NEW_REPORT.json\n       gdl_tools anim-keys ANIM.PS2 TREE_INDEX SEQUENCE_INDEX NEW_REPORT.json\n       gdl_tools anim-samples ANIM.PS2 REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-transforms ANIM.PS2 INITIAL_POSITION_ANIM.PS2 REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-clock REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-clock-start START_REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-select SELECTION_REQUESTS.json NEW_REPORT.json\n       gdl_tools player-anim-select ANIM.PS2 PLAYER_REQUESTS.json NEW_REPORT.json\n       gdl_tools world-matrix MATRIX_REQUESTS.json NEW_REPORT.json\n       gdl_tools motion-code MOTION_REQUESTS.json NEW_REPORT.json\n       gdl_tools motion-request MOTION_STEPS.json NEW_REPORT.json\n       gdl_tools vu0-fqdist FQDIST_REQUESTS.json NEW_REPORT.json\n       gdl_tools collision-query WORLDS.PS2 NEW_REPORT.json [QUERIES.json]\n       gdl_tools player-displacement WORLDS.PS2 STEPS.json NEW_REPORT.json".into());
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
