//! INTERIM PLAYTEST, not the M12 slice: the Yellow Sorceress in the Poison
//! Fields with the native gameplay simulation (gdl_core::game): generators,
//! zombies and mages, items and pickups, chests, missiles and damage.
//!
//! Simulation: game_main order with the ported item/enemy/effect systems and
//! the interim player driver (see gdl_core::game::players for its labels).
//! Presentation only (labelled on screen): unlit diagnostic materials, alpha
//! cutout, rest-state follow camera, item models at their tree rest pose,
//! enemy per-frame objects chosen by a frame-count match of object stems to
//! sequences (the OANIM channel is not decoded), missiles drawn with the
//! SFXYEL WEAP_TW_Y rest pose, hit sparks as short glows. Audio: original
//! banks and music through gdl_client::audio (approximate mixing).
use bevy::{core_pipeline::tonemapping::Tonemapping, prelude::*, sprite::Anchor};
use gdl_core::{
    controls::{direction_levels, tables_from_elf},
    game::{effects::Visual, players::PlayerDriver, setup, Event, Game},
    player_sim::PlayerSim,
};
use gdl_formats::{animation::Animations, objects::Objects, texdef::TextureTable, world::World};
use serde_json::Value;
use std::{collections::HashMap, error::Error, path::Path};

/// Presentation-only handedness mirror (original right × up = forward).
const MIRROR: Vec3 = Vec3::new(-1.0, 1.0, 1.0);

type Parts = Vec<(Handle<Mesh>, Handle<StandardMaterial>)>;
/// Spark sphere mesh, spark material, blast material.
type SparkAssets = (Handle<Mesh>, Handle<StandardMaterial>, Handle<StandardMaterial>);
/// Model parts with node offsets.
type Model = Vec<(Vec3, Parts)>;

/// One OBJECTS/TEXDEF/TEXTURES(/ANIM) package.
struct Package {
    objects: Objects,
    table: TextureTable,
    textures: Vec<u8>,
    anim: Option<Animations>,
}

/// GPU-side view of a package: object parts by definition label.
#[derive(Default)]
struct Library {
    by_label: HashMap<String, Parts>,
}

#[derive(Resource)]
struct Sources {
    character: Package,
    clips: Animations,
    clip_bytes: Vec<u8>,
    rig: Animations,
    packages: HashMap<String, Package>,
    /// Enemy type → monster package name (prefix).
    monsters: HashMap<i32, String>,
}

#[derive(Resource, Default)]
struct Libraries(HashMap<String, Library>);

/// One playable level of the realm (config entry).
struct LevelSource {
    name: String,
    worlds: std::path::PathBuf,
    package: Value,
}

/// Inputs to (re)build the native game for a level; the current geometry.
#[derive(Resource)]
struct Session {
    elf_bytes: Vec<u8>,
    wdata_bytes: Vec<u8>,
    /// Start-state JSON (v5); later levels replace player 0 and CurLevel.
    start: Value,
    levels: Vec<LevelSource>,
    current: usize,
    world: World,
    level: Package,
    sim_start: PlayerSim,
    action_map: Vec<[i64; 2]>,
    clip_timings: Vec<gdl_core::animation_clock::ClipTiming>,
    /// (enemy type, package prefix, ANIM.PS2 path).
    monster_paths: Vec<(i32, String, String)>,
    /// ANIM.PS2 path by package name (ITEMS, WEAPONS, POWERUPS).
    anim_paths: HashMap<String, String>,
    audio_rom: Option<gdl_formats::audio::AudioRom>,
    /// Seconds left on the level-complete banner, then the next level loads.
    banner: Option<(f32, String)>,
    /// Pending level index to build after the banner.
    next: Option<usize>,
    finished: Option<String>,
}

#[derive(Component)]
struct LevelEntity;

/// Per monster package: tree label → sequence index → frame object labels.
#[derive(Resource, Default)]
struct EnemyFrames(HashMap<String, HashMap<String, Vec<Vec<String>>>>);

#[derive(Resource)]
struct Sim {
    game: Game,
    driver: PlayerDriver,
    error: Option<String>,
    updates: u64,
    exp: i64,
    kills: u32,
    sparks: Vec<(Vec3, f32)>,
    last_event: String,
    magic_held: bool,
}

#[derive(Resource)]
struct Presentation {
    fov: f32,
    basis: [Vec3; 3],
    distance: f32,
    attention: Vec3,
}

#[derive(Component)]
struct MirrorRoot;
#[derive(Component)]
struct CharacterRoot;
#[derive(Component)]
struct RigNode(usize);
#[derive(Component)]
struct FollowCamera;
#[derive(Component)]
struct Hud;
/// Centered banner text (title, pause, death, level complete).
#[derive(Component)]
struct Banner;

/// Front-end state: title prompt, pause, diagnostics toggle (F1).
#[derive(Resource, Default)]
struct Ui {
    started: bool,
    paused: bool,
    details: bool,
}
#[derive(Component)]
struct Slot {
    kind: SlotKind,
    index: usize,
    key: Option<String>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotKind {
    Enemy,
    Item,
    Effect,
}
#[derive(Component)]
struct Spark;

fn load_package(p: &Value) -> Result<Package, Box<dyn Error>> {
    let path = |k: &str| -> Result<std::path::PathBuf, Box<dyn Error>> {
        Ok(p[k].as_str().ok_or_else(|| format!("package {k} path"))?.into())
    };
    Ok(Package {
        objects: Objects::parse(&gdl_client::bounded_read(&path("objects")?)?)?,
        table: TextureTable::parse(&gdl_client::bounded_read(&path("texdef")?)?)?,
        textures: gdl_client::bounded_read(&path("textures")?)?,
        anim: match p["anim"].as_str() {
            Some(a) => Some(Animations::parse(&gdl_client::bounded_read(Path::new(a))?)?),
            None => None,
        },
    })
}

fn build_library(
    pkg: &Package,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Library {
    let (cache, unknown, _) = gdl_client::materials_for(&pkg.objects, &pkg.table, &pkg.textures, images, materials, true);
    let (parts, _) = gdl_client::object_meshes(&pkg.objects, &cache, &unknown, meshes);
    let mut by_label = HashMap::new();
    for d in &pkg.objects.definitions {
        if let Some(list) = parts.get(&(d.object_index as u32)) {
            by_label.entry(d.label.clone()).or_insert_with(|| list.clone());
        }
    }
    Library { by_label }
}

/// Presentation heuristic: frame objects "<TREE>…<stem>F<nn>" grouped by stem,
/// stems ordered by their trailing number, matched to sequences in order by
/// equal frame count.
fn enemy_frames(pkg: &Package) -> HashMap<String, Vec<Vec<String>>> {
    let mut out = HashMap::new();
    let Some(anim) = &pkg.anim else { return out };
    for tree in &anim.trees {
        if !tree.nodes.iter().any(|n| n.node_type == 2) {
            continue;
        }
        let mut stems: Vec<(String, Vec<(u32, String)>)> = Vec::new();
        for d in &pkg.objects.definitions {
            let Some(rest) = d.label.strip_prefix(&tree.label) else { continue };
            if rest.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
            let Some(fpos) = d.label.rfind('F') else { continue };
            let (stem, frame) = d.label.split_at(fpos);
            let Ok(n) = frame[1..].parse::<u32>() else { continue };
            match stems.iter_mut().find(|(s, _)| s == stem) {
                Some((_, v)) => v.push((n, d.label.clone())),
                None => stems.push((stem.to_string(), vec![(n, d.label.clone())])),
            }
        }
        let number = |s: &str| -> u32 {
            let digits: String = s.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
            digits.chars().rev().collect::<String>().parse().unwrap_or(u32::MAX)
        };
        stems.sort_by_key(|(s, _)| number(s));
        let mut used = vec![false; stems.len()];
        let mut seqs = Vec::new();
        for seq in &tree.sequences {
            let want = seq.frames.max(0) as usize;
            let pick = stems.iter().enumerate().position(|(k, (_, v))| !used[k] && v.len() == want);
            let frames = match pick {
                Some(k) => {
                    used[k] = true;
                    let mut v = stems[k].1.clone();
                    v.sort_by_key(|(n, _)| *n);
                    v.into_iter().map(|(_, l)| l).collect()
                }
                None => Vec::new(),
            };
            seqs.push(frames);
        }
        out.insert(tree.label.clone(), seqs);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    sources: Res<Sources>,
    session: Res<Session>,
    sim: Res<Sim>,
    presentation: Res<Presentation>,
    mut libraries: ResMut<Libraries>,
    mut frames: ResMut<EnemyFrames>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let root = commands.spawn((Transform::from_scale(MIRROR), Visibility::default(), MirrorRoot)).id();
    spawn_level(&mut commands, root, &session, sim.game.items.records.len(), &mut meshes, &mut images, &mut materials);
    let character = &sources.character;
    let (cache, unknown, _) = gdl_client::materials_for(&character.objects, &character.table, &character.textures, &mut images, &mut materials, true);
    let (parts, _) = gdl_client::object_meshes(&character.objects, &cache, &unknown, &mut meshes);
    let body = commands.spawn((CharacterRoot, Transform::default(), Visibility::default())).id();
    commands.entity(root).add_child(body);
    let tree = &sources.rig.trees[0];
    for node in &tree.nodes {
        let label: String = (tree.prefix.clone() + &node.label).chars().take(16).collect();
        for def in character.objects.definitions.iter().filter(|d| d.label == label) {
            for (mesh, material) in parts.get(&(def.object_index as u32)).into_iter().flatten() {
                let child = commands
                    .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), RigNode(node.index as usize)))
                    .id();
                commands.entity(body).add_child(child);
            }
        }
    }
    for (name, pkg) in &sources.packages {
        libraries.0.insert(name.clone(), build_library(pkg, &mut meshes, &mut images, &mut materials));
        if sources.monsters.values().any(|m| m == name) {
            frames.0.insert(name.clone(), enemy_frames(pkg));
        }
    }
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        Projection::Perspective(PerspectiveProjection { fov: presentation.fov, ..default() }),
        Transform::default(),
        FollowCamera,
    ));
    commands.spawn((Camera2d, Camera { order: 1, clear_color: ClearColorConfig::None, ..default() }));
    commands.spawn((
        Text2d::new(String::new()),
        TextFont { font_size: 15.0, ..default() },
        TextColor(Color::WHITE),
        Anchor::TOP_LEFT,
        Transform::from_xyz(-620.0, 430.0, 0.0),
        Hud,
    ));
    commands.spawn((
        Text2d::new(String::new()),
        TextFont { font_size: 30.0, ..default() },
        TextColor(Color::srgb(1.0, 0.9, 0.4)),
        Anchor::CENTER,
        Transform::from_xyz(0.0, 40.0, 0.0),
        Banner,
    ));
}

/// Level geometry (original translation-only placements) and entity slots.
fn spawn_level(
    commands: &mut Commands,
    root: Entity,
    session: &Session,
    item_count: usize,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) {
    let level = &session.level;
    let (cache, unknown, _) = gdl_client::materials_for(&level.objects, &level.table, &level.textures, images, materials, true);
    let (parts, _) = gdl_client::object_meshes(&level.objects, &cache, &unknown, meshes);
    let names: HashMap<_, _> = level.objects.definitions.iter().map(|d| (d.label.as_str(), d.object_index as u32)).collect();
    if let Ok(placements) = session.world.placements() {
        for p in placements {
            let node = &session.world.nodes[p.node_index as usize];
            if let Some(list) = names.get(node.label.as_str()).and_then(|i| parts.get(i)) {
                for (mesh, material) in list {
                    let child = commands
                        .spawn((
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::from_translation(Vec3::from_array(p.summed_position)),
                            LevelEntity,
                        ))
                        .id();
                    commands.entity(root).add_child(child);
                }
            }
        }
    }
    for (kind, n) in [(SlotKind::Enemy, 25), (SlotKind::Item, item_count), (SlotKind::Effect, 64)] {
        for index in 0..n {
            let e = commands.spawn((Slot { kind, index, key: None }, Transform::default(), Visibility::Hidden, LevelEntity)).id();
            commands.entity(root).add_child(e);
        }
    }
}

/// Build the native game for level `idx`; later levels carry player 0's
/// record and are entered at the level's start locator (get_player_pos).
fn build_game(session: &Session, idx: usize, carry: Option<&gdl_core::game::record::Record>) -> Result<(Game, PlayerDriver), Box<dyn Error>> {
    let elf = gdl_formats::elf::Elf::parse(&session.elf_bytes)?;
    let wdata = gdl_formats::wdata::WorldData::parse(&session.wdata_bytes)?;
    let mut start = session.start.clone();
    if let Some(r) = carry {
        start["players"][0] = Value::String(r.0.iter().map(|b| format!("{b:02x}")).collect());
        start["globals"]["cur_level"] = Value::from(idx as i64);
    }
    let start: setup::StartState = serde_json::from_value(start)?;
    let mut enemy_anims = gdl_core::game::enemy_ai::EnemyAnimData::default();
    for (t, prefix, path) in &session.monster_paths {
        let anim = Animations::parse(&gdl_client::bounded_read(Path::new(path))?)?;
        setup::enemy_trees(&mut enemy_anims, *t, prefix, &anim);
    }
    let anim = |pkg: &str| -> Result<Animations, Box<dyn Error>> {
        let path = session.anim_paths.get(pkg).ok_or_else(|| format!("{pkg} anim path"))?;
        Ok(Animations::parse(&gdl_client::bounded_read(Path::new(path))?)?)
    };
    let items_anim = anim("ITEMS")?;
    let mut item_anims = HashMap::new();
    for (k, t) in items_anim.trees.iter().enumerate() {
        item_anims.insert(t.label.clone(), setup::clips_of(&items_anim, k));
    }
    let weapons = anim("WEAPONS")?;
    let powerups = anim("POWERUPS")?;
    let effect_tables = gdl_core::game::effects::EffectTables::from_elf(&elf, &weapons, &powerups)?;
    let mut game = Game::new_level(setup::LevelInputs {
        world: &session.world,
        wdata: &wdata,
        level: idx,
        elf: &elf,
        start,
        enemy_anims,
        item_anims,
        effect_tables,
        audio_rom: session.audio_rom.clone(),
    })?;
    let tables = tables_from_elf(&elf)?;
    let mut sim = session.sim_start.clone();
    if carry.is_some() {
        let loc = session.world.locators.iter().find(|l| l.kind == 7 && l.id == 0).ok_or("level has no player start locator")?;
        game.place_player_start(0, &mut sim, loc.position, loc.rotation[1]).map_err(|e| e.to_string())?;
        sim.current_action = 0;
        sim.requested_action = 0;
    }
    let driver = PlayerDriver::new(sim, tables, session.action_map.clone(), session.clip_timings.clone());
    Ok((game, driver))
}

fn keyboard_levels(keys: &ButtonInput<KeyCode>) -> u32 {
    let any = |a: KeyCode, b: KeyCode| keys.pressed(a) || keys.pressed(b);
    let mut levels = direction_levels(
        any(KeyCode::ArrowUp, KeyCode::KeyW),
        any(KeyCode::ArrowDown, KeyCode::KeyS),
        any(KeyCode::ArrowLeft, KeyCode::KeyA),
        any(KeyCode::ArrowRight, KeyCode::KeyD),
    );
    if any(KeyCode::Space, KeyCode::KeyK) {
        // Observed reference level word for the attack button (Cross / K).
        levels |= 0x0200_0200;
    }
    levels
}

#[allow(clippy::too_many_arguments)]
fn simulate(
    keys: Res<ButtonInput<KeyCode>>,
    mut sim: ResMut<Sim>,
    session: Res<Session>,
    presentation: Res<Presentation>,
    ui: Res<Ui>,
    mut audio: Option<NonSendMut<gdl_client::audio::AudioOut>>,
) {
    if sim.error.is_some() || session.banner.is_some() || session.finished.is_some() || !ui.started || ui.paused {
        return;
    }
    let levels = keyboard_levels(&keys);
    if keys.pressed(KeyCode::KeyE) || keys.pressed(KeyCode::KeyL) {
        if !sim.magic_held {
            sim.driver.magic_pressed = true;
        }
        sim.magic_held = true;
    } else {
        sim.magic_held = false;
    }
    let Sim { game, driver, .. } = &mut *sim;
    let before = game.events.len();
    game.step_with(None, &mut |g| g.drive_player(0, driver, levels));
    sim.updates += 1;
    let new: Vec<Event> = sim.game.events.drain(before..).collect();
    sim.game.events.clear();
    let me = Vec3::from_array(sim.driver.sim.position);
    let right = presentation.basis[0];
    for e in &new {
        if let Some(a) = audio.as_mut() {
            match e {
                Event::Sound { id, position, volume, priority, queue } => {
                    // Presentation pan: offset along the camera's right axis.
                    let pan = position.map_or(0.0, |p| ((Vec3::from_array(p) - me).dot(right) / 20.0).clamp(-1.0, 1.0));
                    a.play(*id, *volume, pan, *priority, *queue);
                }
                Event::StopSound { id } => a.stop(*id),
                _ => {}
            }
        }
    }
    for e in new {
        match e {
            Event::Exp { amount, .. } => sim.exp += i64::from(amount),
            Event::LevelUp { level, .. } => sim.last_event = format!("LEVEL UP! now level {level}"),
            Event::Effect { position, .. } => sim.sparks.push((Vec3::from_array(position), 0.3)),
            Event::GeneratorHit { destroyed: true, .. } => sim.last_event = "generator destroyed".into(),
            Event::GotIt { kind, amount, .. } => sim.last_event = format!("picked up (kind {kind}, amount {amount})"),
            Event::PlayerDied { .. } => sim.last_event = "YOU DIED".into(),
            Event::Audio { cue: "AudioEnemyDies", .. } => sim.kills += 1,
            _ => {}
        }
    }
    let spf = sim.game.globals.spf;
    sim.sparks.retain_mut(|(_, t)| {
        *t -= spf;
        0.0 < *t
    });
}

fn row(r: &gdl_core::game::record::Record, o: usize) -> Vec3 {
    Vec3::from_array(r.v3(o))
}

fn record_transform(rows: [Vec3; 3], pos: Vec3) -> Transform {
    Transform::from_matrix(Mat4::from_cols(rows[0].extend(0.0), rows[1].extend(0.0), rows[2].extend(0.0), pos.extend(1.0)))
}

/// Item model key and parts: tree rest pose (nodes at summed initial
/// positions), else "<NAME>L1", else "<NAME>".
fn item_parts(sources: &Sources, libs: &Libraries, name: &str) -> Vec<(Vec3, Parts)> {
    for pkg in ["ITEMS", "POWERUPS"] {
        let (Some(p), Some(lib)) = (sources.packages.get(pkg), libs.0.get(pkg)) else { continue };
        if let Some(tree) = p.anim.as_ref().and_then(|a| a.trees.iter().find(|t| t.label == name)) {
            let mut out = Vec::new();
            let mut sums: Vec<Vec3> = Vec::new();
            for node in &tree.nodes {
                let parent = usize::try_from(node.parent).ok().and_then(|i| sums.get(i).copied()).unwrap_or(Vec3::ZERO);
                let at = parent + Vec3::from_array(node.initial_position);
                sums.push(at);
                // Camera-facing glow/sprite nodes ("CF…") need billboarding and
                // blending that are not modelled; they are omitted.
                if node.label.starts_with("CF") {
                    continue;
                }
                let label: String = (tree.prefix.clone() + &node.label).chars().take(15).collect();
                if let Some(parts) = lib.by_label.get(&label) {
                    out.push((at, parts.clone()));
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
        for label in [format!("{name}L1"), name.to_string()] {
            if let Some(parts) = lib.by_label.get(&label) {
                return vec![(Vec3::ZERO, parts.clone())];
            }
        }
    }
    Vec::new()
}

fn spawn_parts(commands: &mut Commands, parent: Entity, list: Vec<(Vec3, Parts)>) {
    commands.entity(parent).despawn_related::<Children>();
    for (offset, parts) in list {
        for (mesh, material) in parts {
            let child = commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::from_translation(offset))).id();
            commands.entity(parent).add_child(child);
        }
    }
}

type SlotQuery<'w, 's> = Query<'w, 's, (Entity, &'static mut Slot, &'static mut Transform, &'static mut Visibility), Without<CharacterRoot>>;

fn present_world(
    mut commands: Commands,
    sim: Res<Sim>,
    sources: Res<Sources>,
    libs: Res<Libraries>,
    frames: Res<EnemyFrames>,
    mut slots: SlotQuery,
) {
    let g = &sim.game;
    for (entity, mut slot, mut transform, mut vis) in &mut slots {
        let (key, parts, tf): (Option<String>, Option<Model>, Transform) = match slot.kind {
            SlotKind::Enemy => {
                let r = &g.enemies[slot.index];
                let st = r.i32_(0xc8);
                let Some(anim) = g.enemy_anims[slot.index].as_ref().filter(|_| st != 0 && st != 8) else {
                    *vis = Visibility::Hidden;
                    continue;
                };
                let (t, level) = anim.tree;
                let tf = record_transform([row(r, 0x10), row(r, 0x20), row(r, 0x30)], row(r, 0x40));
                let Some(pkg) = sources.monsters.get(&t) else {
                    *vis = Visibility::Hidden;
                    continue;
                };
                let tree = setup::enemy_desc(pkg, level);
                let seq = anim.clock.sequence as usize;
                let list = frames.0.get(pkg).and_then(|m| m.get(&tree)).and_then(|s| s.get(seq));
                let label = list.and_then(|l| {
                    if l.is_empty() {
                        None
                    } else {
                        Some(l[(anim.clock.frame.max(0.0) as usize).min(l.len() - 1)].clone())
                    }
                });
                let label = label.or_else(|| frames.0.get(pkg).and_then(|m| m.get(&tree)).and_then(|s| s.first()).and_then(|l| l.first().cloned()));
                match label {
                    Some(l) => {
                        let parts = libs.0.get(pkg).and_then(|lib| lib.by_label.get(&l)).cloned().map(|p| vec![(Vec3::ZERO, p)]);
                        (Some(l), parts, tf)
                    }
                    None => (None, None, tf),
                }
            }
            SlotKind::Item => {
                let Some(r) = g.items.records.get(slot.index) else {
                    *vis = Visibility::Hidden;
                    continue;
                };
                let d = &g.items.definitions[r.definition()];
                let kind = d.kind();
                if !r.active() || r.u16_(0xd8) & 0x8100 != 0 || kind == 4 || kind == 13 || kind == 5 || kind == 7 {
                    *vis = Visibility::Hidden;
                    continue;
                }
                let m = r.matrix();
                let tf = record_transform([Vec3::from_array(m[0]), Vec3::from_array(m[1]), Vec3::from_array(m[2])], Vec3::from_array(r.position()));
                if kind == 3 {
                    let level = r.u8_(0xf6);
                    let t = r.i16_(0xf0) as i32;
                    let Some(pkg) = sources.monsters.get(&t) else {
                        *vis = Visibility::Hidden;
                        continue;
                    };
                    let key = format!("GEN_{pkg}{level}");
                    let parts = libs.0.get(pkg).and_then(|lib| lib.by_label.get(&key)).cloned().map(|p| vec![(Vec3::ZERO, p)]);
                    (Some(key), parts, tf)
                } else {
                    let name = String::from_utf8_lossy(d.name()).into_owned();
                    let key = format!("{name}#{}", r.u16_(0xd8) & 1);
                    if slot.key.as_deref() == Some(key.as_str()) {
                        (Some(key), None, tf)
                    } else {
                        (Some(key), Some(item_parts(&sources, &libs, &name)), tf)
                    }
                }
            }
            SlotKind::Effect => {
                let Some(node) = g.effects.nodes[slot.index].filter(|_| 0.0 < g.effects.slots[slot.index].f32_(0x68)) else {
                    *vis = Visibility::Hidden;
                    continue;
                };
                let m = node.matrix;
                let mut tf = record_transform([Vec3::from_array(m[0]), Vec3::from_array(m[1]), Vec3::from_array(m[2])], Vec3::from_array(node.position));
                tf.scale *= node.scale;
                match node.visual {
                    Visual::Missile { .. } => {
                        let key = "WEAP_TW_Y".to_string();
                        let parts = (slot.key.as_deref() != Some("WEAP_TW_Y")).then(|| missile_parts(&sources, &libs));
                        (Some(key), parts, tf)
                    }
                    Visual::Header(_) => {
                        *vis = Visibility::Hidden;
                        continue;
                    }
                    Visual::EnemyMissile { t, kind } => {
                        let Some(pkg) = sources.monsters.get(&t) else {
                            *vis = Visibility::Hidden;
                            continue;
                        };
                        let key = format!("{pkg}_{}", g.effects.tables.enemy_missile_desc[kind as usize]);
                        let parts = (slot.key.as_deref() != Some(key.as_str())).then(|| tree_parts(&sources, &libs, pkg, &key));
                        (Some(key), parts, tf)
                    }
                }
            }
        };
        *transform = tf;
        *vis = Visibility::Inherited;
        if slot.key != key {
            if let Some(list) = parts {
                spawn_parts(&mut commands, entity, list);
            } else {
                commands.entity(entity).despawn_related::<Children>();
            }
            slot.key = key;
        }
    }
}

/// A package tree's nodes at their initial positions (presentation).
fn tree_parts(sources: &Sources, libs: &Libraries, pkg: &str, label: &str) -> Model {
    let (Some(p), Some(lib)) = (sources.packages.get(pkg), libs.0.get(pkg)) else { return Vec::new() };
    let Some(tree) = p.anim.as_ref().and_then(|a| a.trees.iter().find(|t| t.label == label)) else { return Vec::new() };
    let mut out = Vec::new();
    for node in &tree.nodes {
        if node.label.starts_with("CF") {
            continue;
        }
        let l: String = (tree.prefix.clone() + &node.label).chars().take(15).collect();
        if let Some(parts) = lib.by_label.get(&l) {
            out.push((Vec3::from_array(node.initial_position), parts.clone()));
        }
    }
    out
}

/// The Yellow Sorceress missile tree (SFXYEL WEAP_TW_Y) at rest pose.
fn missile_parts(sources: &Sources, libs: &Libraries) -> Vec<(Vec3, Parts)> {
    let (Some(p), Some(lib)) = (sources.packages.get("SFXYEL"), libs.0.get("SFXYEL")) else { return Vec::new() };
    let Some(tree) = p.anim.as_ref().and_then(|a| a.trees.iter().find(|t| t.label == "WEAP_TW_Y")) else { return Vec::new() };
    let mut out = Vec::new();
    for node in &tree.nodes {
        let label: String = (tree.prefix.clone() + &node.label).chars().take(15).collect();
        if let Some(parts) = lib.by_label.get(&label) {
            out.push((Vec3::from_array(node.initial_position), parts.clone()));
        }
    }
    out
}

fn present_sparks(
    mut commands: Commands,
    sim: Res<Sim>,
    old: Query<Entity, With<Spark>>,
    root: Query<Entity, With<MirrorRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Option<SparkAssets>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let (mesh, mat, blast) = cache
        .get_or_insert_with(|| {
            (
                meshes.add(Sphere::new(0.5)),
                materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.9, 0.3), unlit: true, ..default() }),
                materials.add(StandardMaterial {
                    base_color: Color::srgba(0.8, 0.5, 1.0, 0.25),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    cull_mode: None,
                    ..default()
                }),
            )
        })
        .clone();
    let Ok(root) = root.single() else { return };
    // Area effects (potion magic): a translucent sphere at the blast radius.
    for n in sim.game.effects.nodes.iter().flatten() {
        if 0.0 < n.radius && matches!(n.visual, Visual::Header(_)) {
            let s = commands
                .spawn((Spark, Mesh3d(mesh.clone()), MeshMaterial3d(blast.clone()), Transform::from_translation(Vec3::from_array(n.position)).with_scale(Vec3::splat(2.0 * n.radius))))
                .id();
            commands.entity(root).add_child(s);
        }
    }
    for (p, t) in &sim.sparks {
        let s = commands.spawn((Spark, Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::from_translation(*p).with_scale(Vec3::splat(0.5 + *t * 3.0)))).id();
        commands.entity(root).add_child(s);
    }
}

type BodyQuery<'w, 's> = Query<'w, 's, &'static mut Transform, (With<CharacterRoot>, Without<RigNode>, Without<FollowCamera>, Without<Slot>)>;
type NodeQuery<'w, 's> = Query<'w, 's, (&'static RigNode, &'static mut Transform), (Without<CharacterRoot>, Without<FollowCamera>, Without<Slot>)>;
type CameraQuery<'w, 's> = Query<'w, 's, &'static mut Transform, (With<FollowCamera>, Without<CharacterRoot>, Without<RigNode>, Without<Slot>)>;

#[allow(clippy::too_many_arguments)]
fn present_player(
    sim: Res<Sim>,
    session: Res<Session>,
    sources: Res<Sources>,
    presentation: Res<Presentation>,
    mut body: BodyQuery,
    mut nodes: NodeQuery,
    mut camera: CameraQuery,
    mut hud: Query<&mut Text2d, (With<Hud>, Without<Banner>)>,
    mut banner: Query<&mut Text2d, (With<Banner>, Without<Hud>)>,
    ui: Res<Ui>,
    keys: Res<ButtonInput<KeyCode>>,
    mut exit: MessageWriter<AppExit>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
    let s = &sim.driver.sim;
    let m = s.matrix;
    let world = Mat4::from_cols_array(&[
        m[0][0], m[0][1], m[0][2], 0.0, m[1][0], m[1][1], m[1][2], 0.0, m[2][0], m[2][1], m[2][2], 0.0, s.position[0], s.position[1], s.position[2], 1.0,
    ]);
    if let Ok(mut t) = body.single_mut() {
        *t = Transform::from_matrix(world);
    }
    let anim = sim.driver.anim;
    if let Ok(pose) = gdl_client::rig_pose(&sources.clips, &sources.clip_bytes, anim.clip, anim.frame) {
        for (node, mut t) in &mut nodes {
            if let Some(p) = pose.get(node.0) {
                *t = Transform::from_matrix(*p);
            }
        }
    }
    let target = Vec3::from_array(s.position) + presentation.attention;
    let [_, up, forward] = presentation.basis;
    let eye = target - forward * presentation.distance;
    if let Ok(mut t) = camera.single_mut() {
        *t = Transform::from_translation(eye * MIRROR).looking_to(forward * MIRROR, up * MIRROR);
    }
    let g = &sim.game;
    let p = &g.players[0];
    let health = p.f32_(0x1a10);
    if let Ok(mut text) = banner.single_mut() {
        text.0 = if !ui.started {
            "GAUNTLET: DARK LEGACY\nnative reconstruction - interim playtest\n\nYellow Sorceress - The Poison Fields\n\nPress Enter to start".to_string()
        } else if let Some(f) = &session.finished {
            f.clone()
        } else if let Some((_, b)) = &session.banner {
            b.clone()
        } else if let Some(e) = &sim.error {
            format!("SIMULATION STOPPED\n{e}")
        } else if health <= 0.0 {
            "YOU DIED\npress Enter to continue".to_string()
        } else if ui.paused {
            "PAUSED (P)".to_string()
        } else {
            String::new()
        };
    }
    if let Ok(mut text) = hud.single_mut() {
        let live = g.enemies.iter().filter(|e| matches!(e.i32_(0xc8), 1 | 6)).count();
        let generators = g
            .items
            .records
            .iter()
            .filter(|r| r.active() && g.items.definitions[r.definition()].kind() == 3 && r.u8_(0xf6) != 0)
            .count();
        let mut t = format!(
            "YELLOW SORCERESS   level {}   exp {}\n\
             HEALTH {:>5.0}   GOLD {:>5}   KEYS {}   POTIONS {}\n\
             The Poison Fields {}   kills {}   enemies {}   generators {}\n\
             {}\n\
             move: arrows/WASD   throw: Space/K   potion: E/L   pause: P   details: F1   quit: Esc",
            p.i32_(0x2ac0),
            p.i32_(0x1a1c),
            health,
            p.i32_(0x1a20),
            p.i32_(0x1a14),
            p.i32_(0x1a18),
            session.levels[session.current].name,
            sim.kills,
            live,
            generators,
            sim.last_event,
        );
        if ui.details {
            t.push_str(&format!(
                "\n\nINTERIM PLAYTEST (not the M12 slice). missiles thrown {}\n\
                 Native: item/enemy/generator/missile/damage/pickup/magic/exp/exit/sound-cue systems, movement + collision\n\
                 Interim/presentation: start state from your save, player action map + clip clock, camera, unlit materials,\n\
                 enemy frame objects, item rest poses, approximate audio mixing, magic without animation, melee holds the player still, no boss\n\
                 Enter after death: interim continue   F9: debug skip to next level{}",
                sim.driver.missiles_started,
                g.log.unsupported.iter().find(|(_, n)| 0 < **n).map_or(String::new(), |(k, _)| format!("\nunported path hit: {k}")),
            ));
        }
        text.0 = t;
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    // gdl_play CONFIG.json (written by tools/python/run_play.py).
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        return Err("Usage: gdl_play CONFIG.json".into());
    }
    let cfg: Value = serde_json::from_slice(&gdl_client::bounded_read(Path::new(&args[0]))?)?;
    let p = |k: &str| -> Result<std::path::PathBuf, Box<dyn Error>> { Ok(cfg[k].as_str().ok_or_else(|| format!("config {k}"))?.into()) };
    let sim_start: Value = serde_json::from_slice(&gdl_client::bounded_read(&p("sim_start")?)?)?;
    let player: PlayerSim = serde_json::from_value(sim_start["sim"].clone())?;
    let pr = &sim_start["presentation"];
    let vec3 = |r: &Value| -> Result<Vec3, Box<dyn Error>> { Ok(Vec3::from_array(serde_json::from_value::<[f32; 3]>(r.clone())?)) };
    let presentation = Presentation {
        basis: [vec3(&pr["camera_basis"][0])?, vec3(&pr["camera_basis"][1])?, vec3(&pr["camera_basis"][2])?],
        distance: serde_json::from_value(pr["camera_distance"].clone())?,
        fov: serde_json::from_value(pr["camera_fov"].clone())?,
        attention: vec3(&pr["attention_offset_810"])?,
    };
    let action_map: Vec<[i64; 2]> = serde_json::from_value(pr["action_map"].clone())?;
    let character = load_package(&cfg["character"])?;
    let rig = Animations::parse(&gdl_client::bounded_read(&p("character_rig")?)?)?;
    let clip_bytes = gdl_client::bounded_read(&p("character_clips")?)?;
    let clips = Animations::parse(&clip_bytes)?;
    if rig.trees.len() != 1 || clips.trees.len() != 1 || rig.trees[0].nodes.len() != clips.trees[0].nodes.len() {
        return Err("character rig and clip trees must correspond".into());
    }
    let mut packages = HashMap::new();
    let mut anim_paths = HashMap::new();
    for (name, v) in cfg["packages"].as_object().ok_or("config packages")? {
        packages.insert(name.clone(), load_package(v)?);
        if let Some(a) = v["anim"].as_str() {
            anim_paths.insert(name.clone(), a.to_string());
        }
    }
    let mut monsters = HashMap::new();
    let mut monster_paths = Vec::new();
    for m in cfg["monsters"].as_array().ok_or("config monsters")? {
        let t = m["type"].as_i64().ok_or("monster type")? as i32;
        let name = m["package"].as_str().ok_or("monster package")?.to_string();
        let path = anim_paths.get(&name).ok_or("monster package anim")?.clone();
        monster_paths.push((t, name.clone(), path));
        monsters.insert(t, name);
    }
    let mut levels = Vec::new();
    for l in cfg["levels"].as_array().ok_or("config levels")? {
        levels.push(LevelSource {
            name: l["name"].as_str().ok_or("level name")?.to_string(),
            worlds: l["worlds"].as_str().ok_or("level worlds")?.into(),
            package: l["level"].clone(),
        });
    }
    let first = levels.first().ok_or("no levels")?;
    let audio_cfg = &cfg["audio"];
    let audio_rom = match audio_cfg["rom"].as_str() {
        Some(p) => Some(gdl_formats::audio::AudioRom::parse(&gdl_client::bounded_read(Path::new(p))?)?),
        None => None,
    };
    let paths = |k: &str| -> HashMap<String, std::path::PathBuf> {
        audio_cfg[k].as_object().map_or(HashMap::new(), |m| m.iter().filter_map(|(n, v)| Some((n.to_uppercase(), v.as_str()?.into()))).collect())
    };
    let audio = audio_rom.clone().map(|rom| gdl_client::audio::AudioOut::new(rom, paths("banks"), paths("streams")));
    let session = Session {
        elf_bytes: gdl_client::bounded_read(&p("elf")?)?,
        wdata_bytes: gdl_client::bounded_read(&p("wdata")?)?,
        start: serde_json::from_slice(&gdl_client::bounded_read(&p("game_start")?)?)?,
        world: World::parse(&gdl_client::bounded_read(&first.worlds)?)?,
        level: load_package(&first.package)?,
        levels,
        current: 0,
        sim_start: player,
        action_map,
        clip_timings: setup::clips_of(&clips, 0),
        monster_paths,
        anim_paths,
        audio_rom,
        banner: None,
        next: None,
        finished: None,
    };
    let (game, driver) = build_game(&session, 0, None)?;
    let mut audio = audio;
    if let (Some(a), Some(stage)) = (audio.as_mut(), game.sounds.stage.as_ref()) {
        a.music(&stage.stream_name(0, 0));
    }
    let mut app = App::new();
    if let Some(a) = audio {
        app.insert_non_send_resource(a);
    }
    app
        .insert_resource(Sources { character, clips, clip_bytes, rig, packages, monsters })
        .insert_resource(Sim { game, driver, error: None, updates: 0, exp: 0, kills: 0, sparks: Vec::new(), last_event: String::new(), magic_held: false })
        .insert_resource(session)
        .insert_resource(presentation)
        .init_resource::<Libraries>()
        .init_resource::<EnemyFrames>()
        .insert_resource(Time::<Fixed>::from_hz(30.0))
        .insert_resource(ClearColor(Color::srgb(0.035, 0.04, 0.05)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gauntlet: Dark Legacy - INTERIM native playtest".into(),
                resolution: (1280, 896).into(),
                ..default()
            }),
            ..default()
        }))
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, simulate)
        .init_resource::<Ui>()
        .add_systems(Update, (ui_input, advance_level, present_world, present_sparks, present_player).chain())
        .run();
    Ok(())
}

fn ui_input(keys: Res<ButtonInput<KeyCode>>, mut ui: ResMut<Ui>) {
    if !ui.started {
        if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
            ui.started = true;
        }
        return;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        ui.paused = !ui.paused;
    }
    if keys.just_pressed(KeyCode::F1) {
        ui.details = !ui.details;
    }
}

/// Level complete → banner → next level (exit destination wave), or the
/// end of the playable realm. Enter continues after death (interim).
#[allow(clippy::too_many_arguments)]
fn advance_level(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<Session>,
    mut sim: ResMut<Sim>,
    old: Query<Entity, With<LevelEntity>>,
    root: Query<Entity, With<MirrorRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut audio: Option<NonSendMut<gdl_client::audio::AudioOut>>,
) {
    if keys.just_pressed(KeyCode::Enter) && sim.game.players[0].f32_(0x1a10) <= 0.0 {
        // INTERIM continue: restore the start-state health and play on.
        let start_health = session.start["players"][0]
            .as_str()
            .and_then(|h| h.get(0x1a10 * 2..0x1a10 * 2 + 8))
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .map(|v| f32::from_bits(v.swap_bytes()));
        if let Some(h) = start_health {
            let p = &mut sim.game.players[0];
            p.set_f32(0x1a10, h);
            p.set_i32(0xfc, 1);
            sim.last_event = "continued (interim: start health restored)".into();
        }
    }
    if keys.just_pressed(KeyCode::F9) && session.banner.is_none() && sim.game.level_done.is_none() {
        // DEBUG: act as if the exit portal (destination: next level) was used.
        sim.game.level_done = Some(0x700 | (session.current as i32 + 1));
    }
    if session.banner.is_none()
        && session.finished.is_none()
        && let Some(wave) = sim.game.level_done.take()
    {
        let world = wave >> 8;
        let level = (wave & 0xff) as usize;
        let name = session.levels[session.current].name.clone();
        if world == 7 && level < session.levels.len() {
            session.next = Some(level);
            session.banner = Some((3.0, format!("{name} COMPLETE - entering {}", session.levels[level].name)));
        } else {
            session.finished = Some(format!(
                "{name} COMPLETE. The exit leads to wave {wave:#x}; that level (boss/hub) is not ported yet. Thanks for testing! (Esc to quit)"
            ));
        }
    }
    let Some((left, text)) = session.banner.clone() else { return };
    let left = left - time.delta_secs();
    if 0.0 < left {
        session.banner = Some((left, text));
        return;
    }
    session.banner = None;
    let Some(idx) = session.next.take() else { return };
    let loaded = (|| -> Result<(), Box<dyn Error>> {
        session.world = World::parse(&gdl_client::bounded_read(&session.levels[idx].worlds)?)?;
        session.level = load_package(&session.levels[idx].package)?;
        Ok(())
    })();
    let carry = sim.game.players[0].clone();
    match loaded.and_then(|_| build_game(&session, idx, Some(&carry))) {
        Ok((game, mut driver)) => {
            if let Some(a) = audio.as_mut() {
                a.stop_all();
                if let Some(stage) = game.sounds.stage.as_ref() {
                    a.music(&stage.stream_name(0, 0));
                }
            }
            driver.missiles_started = sim.driver.missiles_started;
            sim.game = game;
            sim.driver = driver;
            session.current = idx;
            for e in &old {
                commands.entity(e).despawn();
            }
            if let Ok(root) = root.single() {
                let n = sim.game.items.records.len();
                spawn_level(&mut commands, root, &session, n, &mut meshes, &mut images, &mut materials);
            }
        }
        Err(e) => sim.error = Some(format!("loading level {}: {e}", session.levels[idx].name)),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
