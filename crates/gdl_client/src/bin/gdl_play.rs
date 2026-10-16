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
//! SFXYEL WEAP_TW_Y rest pose, hit sparks as short glows, no audio.
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
    level: Package,
    world: World,
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
        if tree.nodes.len() != 1 || tree.nodes[0].node_type != 2 {
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
    sim: Res<Sim>,
    presentation: Res<Presentation>,
    mut libraries: ResMut<Libraries>,
    mut frames: ResMut<EnemyFrames>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let root = commands.spawn((Transform::from_scale(MIRROR), Visibility::default(), MirrorRoot)).id();
    let level = &sources.level;
    let (cache, unknown, _) = gdl_client::materials_for(&level.objects, &level.table, &level.textures, &mut images, &mut materials, true);
    let (parts, _) = gdl_client::object_meshes(&level.objects, &cache, &unknown, &mut meshes);
    let names: HashMap<_, _> = level.objects.definitions.iter().map(|d| (d.label.as_str(), d.object_index as u32)).collect();
    if let Ok(placements) = sources.world.placements() {
        for p in placements {
            let node = &sources.world.nodes[p.node_index as usize];
            if let Some(list) = names.get(node.label.as_str()).and_then(|i| parts.get(i)) {
                for (mesh, material) in list {
                    let child = commands
                        .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::from_translation(Vec3::from_array(p.summed_position))))
                        .id();
                    commands.entity(root).add_child(child);
                }
            }
        }
    }
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
    for (kind, n) in [(SlotKind::Enemy, 25), (SlotKind::Item, sim.game.items.records.len()), (SlotKind::Effect, 64)] {
        for index in 0..n {
            let e = commands.spawn((Slot { kind, index, key: None }, Transform::default(), Visibility::Hidden)).id();
            commands.entity(root).add_child(e);
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

fn simulate(keys: Res<ButtonInput<KeyCode>>, mut sim: ResMut<Sim>) {
    if sim.error.is_some() {
        return;
    }
    let levels = keyboard_levels(&keys);
    let Sim { game, driver, .. } = &mut *sim;
    let before = game.events.len();
    game.step_with(None, &mut |g| g.drive_player(0, driver, levels));
    sim.updates += 1;
    let new: Vec<Event> = sim.game.events.drain(before..).collect();
    sim.game.events.clear();
    for e in new {
        match e {
            Event::Exp { amount, .. } => sim.exp += i64::from(amount),
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
                let tree = format!("{pkg}{level}");
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
    mut cache: Local<Option<(Handle<Mesh>, Handle<StandardMaterial>)>>,
) {
    for e in &old {
        commands.entity(e).despawn();
    }
    let (mesh, mat) = cache
        .get_or_insert_with(|| {
            (
                meshes.add(Sphere::new(0.5)),
                materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.9, 0.3), unlit: true, ..default() }),
            )
        })
        .clone();
    let Ok(root) = root.single() else { return };
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
    sources: Res<Sources>,
    presentation: Res<Presentation>,
    mut body: BodyQuery,
    mut nodes: NodeQuery,
    mut camera: CameraQuery,
    mut hud: Query<&mut Text2d, With<Hud>>,
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
    if let Ok(mut text) = hud.single_mut() {
        let g = &sim.game;
        let p = &g.players[0];
        let live = g.enemies.iter().filter(|e| matches!(e.i32_(0xc8), 1 | 6)).count();
        let generators = g
            .items
            .records
            .iter()
            .filter(|r| r.active() && g.items.definitions[r.definition()].kind() == 3 && r.u8_(0xf6) != 0)
            .count();
        let health = p.f32_(0x1a10);
        text.0 = format!(
            "INTERIM PLAYTEST (not the M12 slice) - Yellow Sorceress, The Poison Fields\n\
             Arrows/WASD: move   Space/K: throw   Esc: quit\n\
             HEALTH {:>5.0}   GOLD {:>5}   KEYS {}   POTIONS {}   EXP +{}   KILLS {}\n\
             enemies active {}   generators standing {}   missiles thrown {}\n\
             {}\n\
             Native: item/enemy/generator/missile/damage/pickup systems, movement + collision\n\
             Interim/presentation: start state from your save, player action map + clip clock, camera, unlit materials,\n\
             enemy frame objects, item rest poses, no audio, no exp/levels, no potions/magic, no exit{}{}",
            health,
            p.i32_(0x1a20),
            p.i32_(0x1a14),
            p.i32_(0x1a18),
            sim.exp,
            sim.kills,
            live,
            generators,
            sim.driver.missiles_started,
            if health <= 0.0 { "*** YOU DIED (Esc to quit) ***".to_string() } else { sim.last_event.clone() },
            sim.error.as_ref().map_or(String::new(), |e| format!("\nSIMULATION STOPPED: {e}")),
            g.log.unsupported.keys().next().map_or(String::new(), |k| format!("\nfirst unported path hit: {k}")),
        );
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
    let elf_bytes = gdl_client::bounded_read(&p("elf")?)?;
    let elf = gdl_formats::elf::Elf::parse(&elf_bytes)?;
    let world = World::parse(&gdl_client::bounded_read(&p("worlds")?)?)?;
    let wdata = gdl_formats::wdata::WorldData::parse(&gdl_client::bounded_read(&p("wdata")?)?)?;
    let start: setup::StartState = serde_json::from_slice(&gdl_client::bounded_read(&p("game_start")?)?)?;
    let level = load_package(&cfg["level"])?;
    let character = load_package(&cfg["character"])?;
    let rig = Animations::parse(&gdl_client::bounded_read(&p("character_rig")?)?)?;
    let clip_bytes = gdl_client::bounded_read(&p("character_clips")?)?;
    let clips = Animations::parse(&clip_bytes)?;
    if rig.trees.len() != 1 || clips.trees.len() != 1 || rig.trees[0].nodes.len() != clips.trees[0].nodes.len() {
        return Err("character rig and clip trees must correspond".into());
    }
    let mut packages = HashMap::new();
    for (name, v) in cfg["packages"].as_object().ok_or("config packages")? {
        packages.insert(name.clone(), load_package(v)?);
    }
    let mut monsters = HashMap::new();
    let mut enemy_anims = gdl_core::game::enemy_ai::EnemyAnimData::default();
    for m in cfg["monsters"].as_array().ok_or("config monsters")? {
        let t = m["type"].as_i64().ok_or("monster type")? as i32;
        let name = m["package"].as_str().ok_or("monster package")?.to_string();
        let anim = packages.get(&name).and_then(|p| p.anim.as_ref()).ok_or("monster package anim")?;
        setup::enemy_trees(&mut enemy_anims, t, &name, anim);
        monsters.insert(t, name);
    }
    let items_anim = packages.get("ITEMS").and_then(|p| p.anim.as_ref()).ok_or("ITEMS anim")?;
    let mut item_anims = HashMap::new();
    for (k, t) in items_anim.trees.iter().enumerate() {
        item_anims.insert(t.label.clone(), setup::clips_of(items_anim, k));
    }
    let weapons = packages.get("WEAPONS").and_then(|p| p.anim.as_ref()).ok_or("WEAPONS anim")?;
    let powerups = packages.get("POWERUPS").and_then(|p| p.anim.as_ref()).ok_or("POWERUPS anim")?;
    let effect_tables = gdl_core::game::effects::EffectTables::from_elf(&elf, weapons, powerups)?;
    let game = Game::new_level(setup::LevelInputs { world: &world, wdata: &wdata, level: 0, elf: &elf, start, enemy_anims, item_anims, effect_tables })?;
    let tables = tables_from_elf(&elf)?;
    let driver = PlayerDriver::new(player, tables, action_map, setup::clips_of(&clips, 0));
    App::new()
        .insert_resource(Sources { level, world, character, clips, clip_bytes, rig, packages, monsters })
        .insert_resource(Sim { game, driver, error: None, updates: 0, exp: 0, kills: 0, sparks: Vec::new(), last_event: String::new() })
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
        .add_systems(Update, (present_world, present_sparks, present_player))
        .run();
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
