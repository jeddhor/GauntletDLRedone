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
        return Err("Usage: gdl_tools wad-inspect WAD.BIN NEW_MANIFEST.json\n       gdl_tools wad-extract WAD.BIN NEW_DIRECTORY\n       gdl_tools texdef-inspect TEXDEF.PS2 NEW_DIRECTORY\n       gdl_tools texdef-extract TEXDEF.PS2 TEXTURES.PS2 NEW_DIRECTORY\n       gdl_tools objects-inspect OBJECTS.PS2 NEW_REPORT.json\n       gdl_tools world-inspect WORLDS.PS2 NEW_REPORT.json\n       gdl_tools anim-inspect ANIM.PS2 NEW_REPORT.json\n       gdl_tools anim-initial-frames ANIM.PS2 NEW_REPORT.json\n       gdl_tools anim-keys ANIM.PS2 TREE_INDEX SEQUENCE_INDEX NEW_REPORT.json\n       gdl_tools anim-samples ANIM.PS2 REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-transforms ANIM.PS2 INITIAL_POSITION_ANIM.PS2 REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-clock REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-clock-start START_REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-select SELECTION_REQUESTS.json NEW_REPORT.json\n       gdl_tools player-anim-select ANIM.PS2 PLAYER_REQUESTS.json NEW_REPORT.json\n       gdl_tools world-matrix MATRIX_REQUESTS.json NEW_REPORT.json".into());
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
