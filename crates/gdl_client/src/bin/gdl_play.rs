//! INTERIM PLAYTEST build with the front end (M17 interim) and native
//! saves (M18): title → player select (saved characters, a new character
//! of any of the eight base classes and colours, or the PS2 capture's
//! character imported) → the Tower and every realm, with the original pause
//! menus and saving through the Tower's Manage Character entry.
//!
//! Characters: the original save region and class records (game::profile:
//! player_get_from_save / player_store_in_save / level start), the class
//! model players/<cls>/<colour><level/10>0, the class clips
//! players/<cls>/anim and the thrown missile <PlayerMissileDesc>_THROW<n>.
//! Saves: gdl_core::save (versioned envelopes, backups, recovery).
//!
//! Simulation: game_main order with the ported item/enemy/effect systems and
//! the record-based players (PlayerControls → do_players → PlayerMotion →
//! DoPlayerAction; research/player.md). Host keys and gamepads fill the
//! joyGetStatus entries of ports 0–3 (HYPOTHESIS mapping, below).
//! Presentation only (labelled on screen): unlit diagnostic materials, alpha
//! cutout, rest-state follow camera, item models at their tree rest pose,
//! enemy per-frame objects chosen by a frame-count match of object stems to
//! sequences (the OANIM channel is not decoded), missiles drawn with the
//! SFXYEL WEAP_TW_Y rest pose, hit sparks as short glows. Audio: original
//! banks and music through gdl_client::audio (approximate mixing).
use bevy::{core_pipeline::tonemapping::Tonemapping, ecs::system::RunSystemOnce, ecs::world::World as EcsWorld, prelude::*, sprite::Anchor};
use gdl_client::menu::{Action, Entry, Key, Menu};
use gdl_core::{
    game::{effects::Visual, pad::PadStatus, pickups::ClassData, profile, record::Record, setup, Event, Game},
    player_class::ClassTables,
    save::SaveStore,
};
use gdl_formats::{animation::Animations, objects::Objects, texdef::TextureTable, world::World};
use serde_json::Value;
use std::{collections::HashMap, error::Error, path::Path};

/// Presentation-only handedness mirror (original right × up = forward).
const MIRROR: Vec3 = Vec3::new(-1.0, 1.0, 1.0);

/// The pb projection (pbProjCalc +0x90): separate horizontal and vertical
/// half-angle tangents (60° over 640 columns, 2·atan(1/2.88675) over 384
/// rows), near 1 / far 65536, as Bevy's reverse-Z infinite perspective. The
/// 3D viewport is the top 384 of 448 frame rows (viewport inset 64).
#[derive(Debug, Clone)]
struct Ps2Projection {
    tan_h: f32,
    tan_v: f32,
    near: f32,
    far: f32,
}

impl bevy::camera::CameraProjection for Ps2Projection {
    fn get_clip_from_view(&self) -> Mat4 {
        Mat4::from_cols(
            Vec4::new(1.0 / self.tan_h, 0.0, 0.0, 0.0),
            Vec4::new(0.0, 1.0 / self.tan_v, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, -1.0),
            Vec4::new(0.0, 0.0, self.near, 0.0),
        )
    }
    fn get_clip_from_view_for_sub(&self, _sub_view: &bevy::camera::SubCameraView) -> Mat4 {
        self.get_clip_from_view()
    }
    fn update(&mut self, _width: f32, _height: f32) {}
    fn far(&self) -> f32 {
        self.far
    }
    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [bevy::math::Vec3A; 8] {
        let c = |z: f32| {
            let (x, y) = (z.abs() * self.tan_h, z.abs() * self.tan_v);
            [(x, -y), (x, y), (-x, y), (-x, -y)].map(|(a, b)| bevy::math::Vec3A::new(a, b, z))
        };
        let (n, f) = (c(z_near), c(z_far));
        [n[0], n[1], n[2], n[3], f[0], f[1], f[2], f[3]]
    }
}

/// The 3D viewport: the top 384/448 of the window (the PS2 frame's rows
/// above the 64-row viewport inset).
fn ps2_viewport(windows: Query<&Window>, mut cams: Query<&mut Camera, With<FollowCamera>>) {
    let Ok(w) = windows.single() else { return };
    let (width, height) = (w.physical_width(), w.physical_height());
    let rows = ((height as u64 * 384) / 448) as u32;
    if let Ok(mut cam) = cams.single_mut() {
        let want = bevy::camera::Viewport { physical_position: UVec2::ZERO, physical_size: UVec2::new(width.max(1), rows.max(1)), ..default() };
        if cam.viewport.as_ref().is_none_or(|v| v.physical_size != want.physical_size) {
            cam.viewport = Some(want);
        }
    }
}

type Parts = Vec<(Handle<Mesh>, Handle<gdl_client::GsMaterial>)>;
/// Spark sphere mesh, spark material, blast material.
type SparkAssets = (Handle<Mesh>, Handle<StandardMaterial>, Handle<StandardMaterial>);
/// Model parts with node offsets and the tree node's MB flags (+0x30,
/// AtreeNodeInit's MBTreeSetFlags; 1 for DUMMY nodes).
type Model = Vec<(Vec3, Parts, u32, Option<usize>, Option<TreePsys>)>;

/// A type-4 tree node's particle system (AtreeNodeInit → InitPsysData): the
/// 0x138-byte record at the tree's header[0] base + node +0x34, the
/// direction at node +0x14.
#[derive(Clone)]
struct TreePsys {
    record: Vec<u8>,
    dir: [f32; 3],
}

/// The psys of a tree node of type 4, if its record is in range.
fn tree_node_psys(anim_bytes: &[u8], tree: &gdl_formats::animation::Tree, node: &gdl_formats::animation::Node) -> Option<TreePsys> {
    if node.node_type != 4 || node.raw.len() < 0x38 {
        return None;
    }
    let base = tree.file_offset as usize;
    let header0 = u32::from_le_bytes(anim_bytes.get(base..base + 4)?.try_into().ok()?) as usize;
    let at = base.checked_add(header0)?.checked_add(node.sequence_info_offset as usize)?;
    let record = anim_bytes.get(at..at + gdl_core::psys::RECORD)?.to_vec();
    let f = |o: usize| f32::from_le_bytes(node.raw[o..o + 4].try_into().unwrap());
    Some(TreePsys { record, dir: [f(0x14), f(0x18), f(0x1c)] })
}

/// The MB flags of a package tree node (AtreeNodeInit / AtreeInitSub).
fn tree_node_flags(node: &gdl_formats::animation::Node) -> u32 {
    let mut f = node.model_flags;
    if node.label == "DUMMY" || node.flags & 1 != 0 {
        f |= 1;
    }
    f
}

/// One OBJECTS/TEXDEF/TEXTURES(/ANIM) package.
struct Package {
    objects: Objects,
    table: TextureTable,
    textures: Vec<u8>,
    anim: Option<Animations>,
    anim_bytes: Vec<u8>,
}

/// GPU-side view of a package: object parts by definition label.
#[derive(Default)]
struct Library {
    by_label: HashMap<String, Parts>,
    /// Decoded base images by upper-case TEXDEF label (MBOX_FindTexture).
    textures: HashMap<String, Handle<Image>>,
    /// Every bitmap record's image and records by label (texture modifiers).
    record_images: HashMap<u16, Handle<Image>>,
    label_index: HashMap<String, u16>,
    /// The package's texture modifiers (ANIM.PS2 header +8/+0xc).
    texmods: Vec<gdl_formats::animation::TexMod>,
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

/// One playable level (config entry).
#[derive(Clone)]
struct LevelSource {
    name: String,
    /// LEVL record index (CurLevel; the exit wave's low byte).
    index: usize,
    /// CurWorld (the exit wave's high byte), its WDATA and realm label.
    world: i32,
    wdata: std::path::PathBuf,
    title: String,
    /// Monster packages of the level's realm (names in `packages`).
    monsters: Vec<String>,
    worlds: std::path::PathBuf,
    package: Value,
    /// Item packages, level-specific first (names in `packages`).
    items: Vec<String>,
    /// Critter files (CRITTER/*.WAD) with their model ANIM.PS2 and the
    /// library (package name) that draws the model.
    critters: Vec<(String, Option<String>, Option<String>)>,
}

/// Inputs to (re)build the native game for a level; the current geometry.
#[derive(Resource)]
struct Session {
    elf_bytes: Vec<u8>,
    /// Package config entries by name; packages load per level.
    package_cfg: HashMap<String, Value>,
    /// Start-state JSON (v5); later levels replace player 0 and CurLevel.
    start: Value,
    levels: Vec<LevelSource>,
    /// CurWorld of the realm being played (7 = The Poison Fields).
    world_number: i32,
    /// Realm label for the HUD (config "title").
    title: String,
    /// CurWorld of the level played before this one (LastWorldPlayed).
    last_world: i32,
    current: usize,
    world: World,
    level: Package,
    action_map: Vec<[i64; 2]>,
    clip_timings: Vec<gdl_core::animation_clock::ClipTiming>,
    /// (enemy type, package prefix, ANIM.PS2 path).
    monster_paths: Vec<(i32, String, String)>,
    /// ANIM.PS2 path by package name (ITEMS, WEAPONS, POWERUPS).
    anim_paths: HashMap<String, String>,
    audio_rom: Option<gdl_formats::audio::AudioRom>,
    /// Object labels of the shared packages (HYPOTHESIS: the original's
    /// loaded MBOX tables); the level package's labels are added per level.
    object_labels: std::collections::HashSet<String>,
    /// Seconds left on the level-complete banner, then the next level loads.
    banner: Option<(f32, String)>,
    /// Pending level index to build after the banner.
    next: Option<usize>,
    finished: Option<String>,
    /// The played character: save slot, controller options (save
    /// +0x190c..), class record, class records of every class, heart_rate.
    slot: u32,
    controls: profile::ControlOptions,
    class_data: ClassData,
    pdats: profile::Pdats,
    heart_rate: u8,
    /// Character packages: the SFX package name, the thrown-missile tree
    /// (package, label) and the model tier (level / 10) the body uses.
    sfx: String,
    throw: (String, String),
    tier: i32,
    class: u32,
    colour: usize,
    /// Session globals carried from level to level: TowerCounter,
    /// LastFinishedWorld, LastFinishedLevel.
    carried: [i32; 5],
}

#[derive(Component)]
struct LevelEntity;

/// Which part of the program is active.
#[derive(Resource, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Front,
    Playing,
}

fn playing(s: Res<Screen>) -> bool {
    *s == Screen::Playing
}

/// Configuration parsed once; every game starts from clones of it.
struct Base {
    package_cfg: HashMap<String, Value>,
    start: Value,
    levels: Vec<LevelSource>,
    monster_paths: Vec<(i32, String, String)>,
    monsters: HashMap<i32, String>,
    anim_paths: HashMap<String, String>,
    audio_rom: Option<gdl_formats::audio::AudioRom>,
    shared_labels: std::collections::HashSet<String>,
}

/// Front end: menu, save store, class tables and the asset catalog.
#[derive(Resource)]
struct Front {
    menu: Menu,
    store: SaveStore,
    catalog: gdl_client::catalog::Catalog,
    elf_bytes: Vec<u8>,
    tables: ClassTables,
    pdats: profile::Pdats,
    /// Per class: the PDATA WAD's SFXX and DAMG chunks.
    class_fx: Vec<(Vec<u8>, Vec<u8>)>,
    /// PlayerMissileDesc 0x24db80: per class "<NAME>\0" + ten tier chars.
    missile_desc: Vec<Vec<u8>>,
    heart_rate: u8,
    /// The PS2 capture's player 0 save region and its sha256 (import source).
    capture: Option<(Vec<u8>, String)>,
    game_opts: Vec<u8>,
    /// SHPDATA/SHOP.WAD ITEM records.
    shop: Vec<gdl_core::game::powerups::ShopItem>,
    /// TEXT/ENGLISH.ROM (GameText) and TEXT/SCROLL_E.ROM.
    english: gdl_formats::textrom::TextRom,
    scrolls: gdl_formats::textrom::TextRom,
    /// Front-end blit layer (STATIC textures, fonts), the OPTMENU style,
    /// the menu records by symbol and the shown page's vblank count (+0x7c).
    ui_hud: gdl_core::game::hud::Hud,
    opt_style: Option<gdl_core::game::optmenu::OptStyle>,
    menus: HashMap<&'static str, gdl_core::game::optmenu::OptMenu>,
    static_pkg: Package,
    /// TITLE (title blits, GLOWCROP) and the title screen's vblank count.
    title_pkg: Package,
    title_clock: f32,
    /// SELECT (player-select columns, class art).
    select_pkg: Package,
    menu_clock: (Option<(&'static str, usize)>, f32),
    base: Base,
}

/// Front-end text (menus) drawn over everything.
#[derive(Component)]
struct FrontText;
/// The native text menus' dark panel (hidden under original menus).
#[derive(Component)]
struct FrontPanel;

/// The title picture (TITLE00–03) and the panel behind the menu text,
/// shown on the front end only.
#[derive(Component)]
struct FrontBackdrop;


/// A critter instance slot (CritterInsts index) and the type it shows.
#[derive(Component)]
struct CritterSlot {
    index: usize,
    shown: Option<(usize, usize)>,
}

/// One tree node's meshes of a critter slot.
#[derive(Component)]
struct CritterPart {
    node: usize,
}

/// Critters drawn from the simulation skeleton: each tree node with an
/// object "<tree prefix><node label>" (15 characters) in the critter's model
/// package, posed by gdl_core::tree_pose (presentation of the native pose;
/// skinned parts and effects attached to the tree are not drawn).
fn present_critters(
    mut commands: Commands,
    sim: Res<Sim>,
    session: Res<Session>,
    libs: Res<Libraries>,
    mut slots: Query<(Entity, &mut CritterSlot, &mut Visibility, Option<&Children>)>,
    mut parts: Query<(&CritterPart, &mut Transform)>,
) {
    let g = &sim.game;
    for (entity, mut slot, mut vis, children) in &mut slots {
        let Some(Some(cr)) = g.critters.get(slot.index) else {
            *vis = Visibility::Hidden;
            continue;
        };
        // Children (chimera heads) share their root's tree and pose; the
        // root's slot draws every node.
        if cr.parent.is_some() {
            *vis = Visibility::Hidden;
            continue;
        }
        let file = &g.critter_data.files[cr.ty.0];
        let (Some(tree), Some(model)) = (file.trees[cr.ty.1], file.model.as_ref()) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let t = &model.0.trees[tree];
        if slot.shown != Some(cr.ty) {
            commands.entity(entity).despawn_related::<Children>();
            // The configured model package (golems, gargoyles, generals),
            // else a package named after the tree prefix (bosses).
            let lib_name = session.levels.get(session.current).and_then(|l| l.critters.get(cr.ty.0)).and_then(|c| c.2.clone()).unwrap_or_else(|| t.prefix.clone());
            if let Some(lib) = libs.0.get(&lib_name) {
                for node in &t.nodes {
                    let label: String = (t.prefix.clone() + &node.label).chars().take(15).collect();
                    if let Some(list) = lib.by_label.get(&label) {
                        let holder = commands.spawn((CritterPart { node: node.index as usize }, Transform::default(), Visibility::default())).id();
                        for (mesh, material) in list {
                            let child = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default())).id();
                            commands.entity(holder).add_child(child);
                        }
                        commands.entity(entity).add_child(holder);
                    }
                }
            }
            slot.shown = Some(cr.ty);
        }
        *vis = Visibility::Visible;
        let r = &cr.rec;
        let mut root = gdl_core::tree_pose::IDENTITY;
        for row in 0..3 {
            root[4 * row..4 * row + 3].copy_from_slice(&r.v3(0x10 + 16 * row));
        }
        root[12..15].copy_from_slice(&r.v3(0x40));
        let Some(children) = children else { continue };
        for child in children.iter() {
            if let Ok((part, mut tf)) = parts.get_mut(child)
                && let Ok(m) = gdl_core::tree_pose::node_world(&cr.parents, &cr.locals, root, part.node, None)
            {
                let mat = Mat4::from_cols_array(&m);
                if mat.is_finite() {
                    *tf = Transform::from_matrix(mat);
                }
            }
        }
    }
}

/// Sumner (SumnerInit's GWIZ tree) and one of his tree nodes.
#[derive(Component)]
struct SumnerRoot;
#[derive(Component)]
struct SumnerPart(usize);

/// Sumner drawn from the GWIZ tree of the level's item package at his
/// lookout matrix, posed by his animation clock (presentation of
/// game::tower::SumnerState).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn present_sumner(
    mut commands: Commands,
    sim: Res<Sim>,
    session: Res<Session>,
    sources: Res<Sources>,
    libs: Res<Libraries>,
    root: Query<Entity, With<MirrorRoot>>,
    mut body: Query<(Entity, &mut Transform), (With<SumnerRoot>, Without<SumnerPart>)>,
    mut parts: Query<(&SumnerPart, &mut Transform), Without<SumnerRoot>>,
) {
    let Some(s) = sim.game.tower.sumner.as_ref() else { return };
    let items = &session.levels[session.current].items;
    let Some((pkg_name, pkg, ti)) = items.iter().find_map(|n| {
        let p = sources.packages.get(n)?;
        let ti = p.anim.as_ref()?.trees.iter().position(|t| t.label == "GWIZ")?;
        Some((n.clone(), p, ti))
    }) else {
        return;
    };
    let anim = pkg.anim.as_ref().unwrap();
    let tree = &anim.trees[ti];
    let m = s.matrix;
    let tf = record_transform([Vec3::new(m[0][0], m[0][1], m[0][2]), Vec3::new(m[1][0], m[1][1], m[1][2]), Vec3::new(m[2][0], m[2][1], m[2][2])], Vec3::new(m[3][0], m[3][1], m[3][2]));
    if body.is_empty() {
        let (Ok(root), Some(lib)) = (root.single(), libs.0.get(&pkg_name)) else { return };
        let b = commands.spawn((SumnerRoot, tf, Visibility::default(), LevelEntity)).id();
        commands.entity(root).add_child(b);
        for node in &tree.nodes {
            let label: String = (tree.prefix.clone() + &node.label).chars().take(15).collect();
            if let Some(list) = lib.by_label.get(&label) {
                let holder = commands.spawn((SumnerPart(node.index as usize), Transform::default(), Visibility::default())).id();
                for (mesh, material) in list {
                    let c = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default())).id();
                    commands.entity(holder).add_child(c);
                }
                commands.entity(b).add_child(holder);
            }
        }
        return;
    }
    for (_, mut t) in &mut body {
        *t = tf;
    }
    if let Ok(pose) = gdl_client::tree_pose(anim, ti, &pkg.anim_bytes, s.current as usize, s.clock.frame.max(0.0), true) {
        for (part, mut t) in &mut parts {
            if let Some(mat) = pose.get(part.0) {
                *t = Transform::from_matrix(*mat);
            }
        }
    }
}

/// A world-geometry node drawn at `rest` (summed file position).
#[derive(Component)]
struct WorldNode {
    node: usize,
    rest: Vec3,
}

/// The node's draw-visibility inputs: itself then its ancestors with their
/// file MB flags (CreateWorldNode); game code overrides them through
/// Game::node_mb_set / node_mb_clear (MBTreeSetFlags / MBTreeClearFlags).
#[derive(Component)]
struct NodeChain(Vec<(usize, u32)>);

/// Presentation of moving world objects (elevators' item-driven y, rotators'
/// YawMat3): a node follows the nearest moved object among itself and its
/// ancestors, displaced and rotated relative to that object's rest matrix.
fn present_world_nodes(sim: Res<Sim>, mut nodes: Query<(&WorldNode, &mut Transform)>) {
    let c = &sim.game.collision;
    if !(0..c.objects.len()).any(|o| c.object_moved(o)) {
        return;
    }
    for (n, mut tf) in &mut nodes {
        let mut k = Some(n.node);
        let mut mover = None;
        let mut guard = 0;
        while let Some(i) = k.filter(|&i| i < c.objects.len()) {
            if c.object_moved(i) {
                mover = Some(i);
                break;
            }
            k = c.objects[i].parent;
            guard += 1;
            if 256 < guard {
                break;
            }
        }
        let Some(m) = mover else { continue };
        let (Ok(cur), Ok(rest)) = (c.object_matrix(m), c.object_rest_matrix(m)) else { continue };
        let rows = Mat3::from_cols(Vec3::new(cur[0][0], cur[0][1], cur[0][2]), Vec3::new(cur[1][0], cur[1][1], cur[1][2]), Vec3::new(cur[2][0], cur[2][1], cur[2][2]));
        let rel = n.rest - Vec3::new(rest[3][0], rest[3][1], rest[3][2]);
        tf.translation = rows * rel + Vec3::new(cur[3][0], cur[3][1], cur[3][2]);
        tf.rotation = Quat::from_mat3(&rows);
    }
}

/// A world node drawn under CameraFace 0x1c07e0 (MB flags & 0xf000000 of
/// the node or its nearest ancestor): `anchor` is that node's position,
/// `offset` this node's rest position relative to it.
#[derive(Component)]
struct CameraFace {
    mode: u32,
    anchor: Vec3,
    offset: Vec3,
}

/// CameraFace's matrix modes on a translation-only node, in original
/// coordinates (columns = the PS2 matrix rows). FaceCamMat (0x1c7d48):
/// YawMat3 by the eye's yaw, PitchMat3 by its pitch clamped to ±limit
/// (unclamped for 0x3000000); QuickYawMat: yaw to the camera forward + π;
/// 0x4000000: diag(−1, 1, −1) × the camera→world rows; TopFaceMat: keeps z,
/// x = normalize((eye − p) × z), y = z × x.
fn camera_face_rotation(mode: u32, p: Vec3, eye: Vec3, rows: [Vec3; 3]) -> Mat3 {
    let to = eye - p;
    let yaw_pitch = |limit: Option<f32>| {
        let yaw = to.x.atan2(to.z);
        let mut m = Mat3::from_rotation_y(yaw);
        if let Some(limit) = limit {
            let mut pitch = to.y.atan2((to.x * to.x + to.z * to.z).sqrt());
            if 0.0 < limit {
                pitch = pitch.clamp(-limit, limit);
            }
            m *= Mat3::from_rotation_x(-pitch);
        }
        m
    };
    match mode {
        0x1000000 => yaw_pitch(None),
        0x2000000 => {
            let yaw = rows[2].x.atan2(rows[2].z) + std::f32::consts::PI;
            Mat3::from_rotation_y(yaw)
        }
        0x3000000 => yaw_pitch(Some(-1.0)),
        0x5000000 => yaw_pitch(Some(f32::from_bits(0x3e860a92))),
        0x6000000 => yaw_pitch(Some(f32::from_bits(0x3f060a92))),
        0x7000000 => yaw_pitch(Some(f32::from_bits(0x3f490fdb))),
        0x4000000 => Mat3::from_cols(-rows[0], rows[1], -rows[2]),
        0x8000000 => {
            let z = Vec3::Z;
            let x = to.cross(z);
            if x.length() < 0.01 {
                Mat3::IDENTITY
            } else {
                let x = x.normalize();
                Mat3::from_cols(x, z.cross(x), z)
            }
        }
        _ => Mat3::IDENTITY,
    }
}

fn present_camera_face(sim: Res<Sim>, mut nodes: Query<(&CameraFace, &mut Transform)>) {
    let Some(cam) = &sim.game.std_camera else { return };
    let eye = Vec3::from_array(cam.std.v3(gdl_core::game::camera::o::EYE));
    let rows = cam.pb.cam_rows.map(Vec3::from_array);
    for (face, mut tf) in &mut nodes {
        let m = camera_face_rotation(face.mode, face.anchor, eye, rows);
        tf.translation = face.anchor + m * face.offset;
        tf.rotation = Quat::from_mat3(&m);
    }
}

/// MBSetupObject 0x1c7fe8 for flags 0x100400: node alpha +0x53 =
/// 255 / ALPHADISTMAX (100, or ALPHADIST2MAX 200 for 0x100000) × view
/// depth, clamped to [0, 255]; setPrimAlpha draws with As = +0x53 × 0.502.
#[derive(Component)]
struct DistanceFade {
    max: f32,
    at: Vec3,
}

fn present_distance_fade(sim: Res<Sim>, q: Query<(&DistanceFade, &MeshMaterial3d<gdl_client::GsMaterial>)>, mut materials: ResMut<Assets<gdl_client::GsMaterial>>) {
    let Some(cam) = &sim.game.std_camera else { return };
    let eye = Vec3::from_array(cam.std.v3(gdl_core::game::camera::o::EYE));
    let forward = Vec3::from_array(cam.pb.cam_rows[2]);
    for (fade, m) in &q {
        let depth = (fade.at - eye).dot(forward);
        let alpha = ((255.0 / fade.max) * depth).clamp(0.0, 255.0).floor();
        let a = alpha * 0.5019608 / 128.0;
        if let Some(mat) = materials.get_mut(&m.0)
            && (mat.params.tint.w - a).abs() > 1e-4
        {
            mat.params.tint.w = a;
        }
    }
}

/// InitLighting / DoLighting: Ambient = LEVL +0xec × GlobalInten (1.0),
/// one directional light (MBAddLight: intensity +0x108 × colour +0xfc,
/// direction +0xf0, normalised). Lit materials get the light towards which
/// normals shade (−direction, mirrored into render space).
fn present_scene_light(sim: Res<Sim>, mut last: Local<Option<([f32; 7], usize)>>, mut materials: ResMut<Assets<gdl_client::GsMaterial>>) {
    let g = &sim.game;
    if g.level.len() < 0x10c {
        return;
    }
    let ambient = g.level_f32(0xec).clamp(0.0, 1.0);
    let dir = Vec3::new(g.level_f32(0xf0), g.level_f32(0xf4), g.level_f32(0xf8));
    let colour = Vec3::new(g.level_f32(0xfc), g.level_f32(0x100), g.level_f32(0x104)) * g.level_f32(0x108);
    let towards = if dir.length() > 0.0 { -dir.normalize() * MIRROR } else { Vec3::Y };
    let ids: Vec<_> = materials.iter().filter(|(_, m)| m.lit).map(|(id, _)| id).collect();
    let key = ([ambient, towards.x, towards.y, towards.z, colour.x, colour.y, colour.z], ids.len());
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    for id in ids {
        if let Some(m) = materials.get_mut(id) {
            m.params.light_dir = towards.extend(1.0);
            m.params.light = colour.extend(ambient);
        }
    }
}

/// pbTraverseDrawObjects: MB flag 2 skips a node and its subtree, flag 1
/// only the node; `set`/`clear` are the game's per-node overrides.
fn node_hidden(chain: &[(usize, u32)], set: &[u32], clear: &[u32]) -> bool {
    chain.iter().enumerate().any(|(depth, &(n, f))| {
        let f = (f | set.get(n).copied().unwrap_or(0)) & !clear.get(n).copied().unwrap_or(0);
        f & 2 != 0 || (depth == 0 && f & 1 != 0)
    })
}

/// Apply the game's node flag overrides when they change.
fn present_node_flags(sim: Res<Sim>, mut seen: Local<Option<u32>>, mut q: Query<(&NodeChain, &mut Visibility), With<WorldNode>>) {
    let g = &sim.game;
    if *seen == Some(g.node_state_version) {
        return;
    }
    *seen = Some(g.node_state_version);
    for (chain, mut vis) in &mut q {
        let hidden = node_hidden(&chain.0, &g.node_mb_set, &g.node_mb_clear);
        let want = if hidden { Visibility::Hidden } else { Visibility::Inherited };
        if (hidden || *vis != Visibility::Visible) && *vis != want {
            *vis = want;
        }
    }
}

/// World objects removed by WorldObjectExplode (object +0x10 bit
/// 0x10000000) are hidden with their meshes.
fn hide_exploded_nodes(sim: Res<Sim>, mut nodes: Query<(Entity, &WorldNode, &NodeChain, &mut Visibility)>, mut was: Local<HashMap<Entity, bool>>) {
    let objs = &sim.game.items.objects;
    let visible = &sim.game.items.object_visible;
    let g = &sim.game;
    for (e, n, chain, mut vis) in &mut nodes {
        let gone = objs.get(n.node).is_some_and(|o| o.flags_10 & 0x1000_0000 != 0);
        // ProcessItemWobjs' fade objects (trigger kind 0x10xx) hide with
        // their subtree (MBTreeSetAlpha on the object's tree).
        let faded = chain.0.iter().any(|&(i, _)| visible.get(i).is_some_and(|v| !*v));
        let off = gone || faded;
        if off && *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        } else if !off && was.get(&e).copied().unwrap_or(false) {
            *vis = if node_hidden(&chain.0, &g.node_mb_set, &g.node_mb_clear) { Visibility::Hidden } else { Visibility::Inherited };
        }
        was.insert(e, off);
    }
}

/// Per monster package: tree label → sequence index → frame object labels.
#[derive(Resource, Default)]
struct EnemyFrames(HashMap<String, HashMap<String, Vec<Vec<String>>>>);

#[derive(Resource)]
struct Sim {
    game: Game,
    error: Option<String>,
    updates: u64,
    exp: i64,
    kills: u32,
    sparks: Vec<(Vec3, f32)>,
    last_event: String,
    /// Co-op players 1–3 in play (PLACEHOLDER join, see `simulate`).
    coop: [bool; 3],
    /// ScrollMessageBox: the game waits until the box is dismissed.
    /// ScrollMessageBox (modal; the simulation waits).
    message_box: Option<gdl_core::game::hud::ScrollBox>,
    scroll_clock: f32,
    /// Sumner's hint pad asked for hint_menu.
    hint_menu: bool,
}

/// One port's joyGetStatus entries this update. HYPOTHESIS mapping (the
/// physical pad → entry assignment is not measured; research/unknowns.md):
/// entries 0–3 d-pad (left, right, up, down; observed level bits), 0xd
/// attack, 0xb magic/potion, 0xc turbo, 0xa the second attack button
/// (GetPlayerDMotion 0x400), 0x12 strafe, 0x13 code 2, 8 dash, 9 team move,
/// 0x14/0x15 start/select. Port 0: WASD/arrows, Space/K, E/L, Shift/J,
/// Q/I, Ctrl, Enter/Tab, or gamepad 0 (stick as analog entries 4–7); port
/// 1: TFGH, R, Y; ports 2–3: gamepads.
fn player_input(slot: usize, keys: &ButtonInput<KeyCode>, pads: &[&Gamepad]) -> PadStatus {
    let mut s = PadStatus { connected: true, ..Default::default() };
    let press = |s: &mut PadStatus, i: usize, on: bool| {
        if on {
            s.press(i);
        }
    };
    let k = |a: KeyCode| keys.pressed(a);
    match slot {
        0 => {
            press(&mut s, 0, k(KeyCode::ArrowLeft) || k(KeyCode::KeyA));
            press(&mut s, 1, k(KeyCode::ArrowRight) || k(KeyCode::KeyD));
            press(&mut s, 2, k(KeyCode::ArrowUp) || k(KeyCode::KeyW));
            press(&mut s, 3, k(KeyCode::ArrowDown) || k(KeyCode::KeyS));
            press(&mut s, 0xd, k(KeyCode::Space) || k(KeyCode::KeyK));
            press(&mut s, 0xb, k(KeyCode::KeyE) || k(KeyCode::KeyL));
            press(&mut s, 0xc, k(KeyCode::ShiftLeft) || k(KeyCode::KeyJ));
            press(&mut s, 0xa, k(KeyCode::KeyQ) || k(KeyCode::KeyI));
            press(&mut s, 0x12, k(KeyCode::ControlLeft));
            press(&mut s, 0x14, k(KeyCode::Enter));
            press(&mut s, 0x15, k(KeyCode::Tab));
        }
        1 => {
            press(&mut s, 0, k(KeyCode::KeyF));
            press(&mut s, 1, k(KeyCode::KeyH));
            press(&mut s, 2, k(KeyCode::KeyT));
            press(&mut s, 3, k(KeyCode::KeyG));
            press(&mut s, 0xd, k(KeyCode::KeyR));
            press(&mut s, 0xb, k(KeyCode::KeyY));
        }
        _ => {}
    }
    if let Some(g) = pads.get(slot) {
        let b = |x: GamepadButton| g.pressed(x);
        press(&mut s, 0, b(GamepadButton::DPadLeft));
        press(&mut s, 1, b(GamepadButton::DPadRight));
        press(&mut s, 2, b(GamepadButton::DPadUp));
        press(&mut s, 3, b(GamepadButton::DPadDown));
        press(&mut s, 0xd, b(GamepadButton::South));
        press(&mut s, 0xb, b(GamepadButton::East));
        press(&mut s, 0xc, b(GamepadButton::West));
        press(&mut s, 0xa, b(GamepadButton::North));
        press(&mut s, 0x12, b(GamepadButton::LeftTrigger));
        press(&mut s, 0x13, b(GamepadButton::RightTrigger));
        press(&mut s, 8, b(GamepadButton::LeftTrigger2));
        press(&mut s, 9, b(GamepadButton::RightTrigger2));
        press(&mut s, 0x14, b(GamepadButton::Start));
        press(&mut s, 0x15, b(GamepadButton::Select));
        // The left stick as pressure-style entries 4–7 (left, right, up, down).
        let st = g.left_stick();
        let v = |x: f32| (x.clamp(0.0, 1.0) * 255.0) as u8;
        s.analog = true;
        s.has_stick = true;
        s.entries[4] = [3, v(-st.x)];
        s.entries[5] = [3, v(st.x)];
        s.entries[6] = [3, v(st.y)];
        s.entries[7] = [3, v(-st.y)];
    }
    s
}

#[derive(Resource)]
struct Presentation {
    fov: f32,
    basis: [Vec3; 3],
    attention: Vec3,
}

#[derive(Component)]
struct MirrorRoot;
#[derive(Component)]
/// The character body of player slot `.0`.
struct CharacterRoot(usize);
/// A rig node (tree index) of player slot `.1`.
#[derive(Component)]
struct RigNode(usize, usize);
#[derive(Component)]
struct FollowCamera;
#[derive(Component)]
struct Hud;
/// The player panels' render layer: a fixed 512×384 orthographic view (the
/// MB blit screen; MBBlitSetPos scales it to the 640×448 frame, i.e. the
/// window).
const HUD_LAYER: usize = 1;
/// A pooled blit sprite (index into the frame's sorted blit list).
#[derive(Component)]
struct HudSprite(usize);
/// The HUD's MBOX texture sets (search order; "" = the player's character
/// package) and their decoded images.
#[derive(Resource, Default)]
struct HudSets {
    keys: Vec<String>,
    images: HashMap<(u16, u16), Option<Handle<Image>>>,
}
/// TriggerCamUpdate's letterbox blits (TextureWhite, colour 0): screen rows
/// 0..BORDER_HT (0x30) and 0x130..0x180 of the 512×384 screen space.
#[derive(Component)]
struct LetterBox;
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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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
        // A level ANIM.PS2 may hold only texture modifiers (no trees).
        anim: match p["anim"].as_str() {
            Some(a) => Animations::parse(&gdl_client::bounded_read(Path::new(a))?).ok(),
            None => None,
        },
        anim_bytes: match p["anim"].as_str() {
            Some(a) => gdl_client::bounded_read(Path::new(a))?,
            None => Vec::new(),
        },
    })
}

fn build_library(
    pkg: &Package,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<gdl_client::GsMaterial>,
) -> Library {
    let set = gdl_client::materials_for(&pkg.objects, &pkg.table, &pkg.textures, images, materials);
    let (parts, _) = gdl_client::object_meshes(&pkg.objects, &set, materials, meshes);
    let mut by_label = HashMap::new();
    for d in &pkg.objects.definitions {
        if let Some(list) = parts.get(&(d.object_index as u32)) {
            by_label.entry(d.label.clone()).or_insert_with(|| list.clone());
        }
    }
    let texmods = gdl_formats::animation::texmods(&pkg.anim_bytes).unwrap_or_default();
    Library { by_label, textures: set.labels, record_images: set.record_images, label_index: set.label_index, texmods }
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

/// Cameras and text overlays (always present).
fn setup_static(mut commands: Commands, presentation: Res<Presentation>) {
    commands.spawn((Sprite { color: Color::linear_rgba(0.0, 0.0, 0.0, 0.6), custom_size: Some(Vec2::new(900.0, 290.0)), ..default() }, Transform::from_xyz(0.0, -300.0, -1.0), FrontBackdrop, FrontPanel));
    // Until a level's camera exists the session start camera's window
    // angle (MboxWinPtr +0x1c, horizontal) stands in; the 640×384 viewport
    // has square pixels.
    let tan_v = (presentation.fov * 0.5).tan() * 384.0 / 640.0;
    commands.spawn((
        Camera3d::default(),
        Tonemapping::None,
        bevy::render::view::Hdr,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        Projection::custom(Ps2Projection { tan_h: tan_v * 640.0 / 384.0, tan_v, near: 1.0, far: 65536.0 }),
        Transform::default(),
        FollowCamera,
    ));
    commands.spawn((Camera2d, Camera { order: 1, clear_color: ClearColorConfig::None, ..default() }, Tonemapping::None, bevy::render::view::Hdr, bevy::core_pipeline::tonemapping::DebandDither::Disabled));
    commands.spawn((
        Camera2d,
        Camera { order: 2, clear_color: ClearColorConfig::None, ..default() },
        Tonemapping::None,
        bevy::render::view::Hdr,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        // The last camera: decode the frame's display values once.
        gdl_client::DisplayToLinear { enabled: 1.0 },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::Fixed { width: 512.0, height: 384.0 },
            ..OrthographicProjection::default_2d()
        }),
        bevy::camera::visibility::RenderLayers::layer(HUD_LAYER),
    ));
    // TriggerCamUpdate's temp blits (TextureWhite, MBBlitSetColor 0 → GS
    // colour 1/128, z 0.5: in front of the panels) on the blit layer.
    for (top, bottom) in [(0.0f32, 48.0f32), (304.0, 384.0)] {
        commands.spawn((
            Sprite { color: Color::linear_rgb(1.0 / 128.0, 1.0 / 128.0, 1.0 / 128.0), custom_size: Some(Vec2::new(512.0, bottom - top)), ..default() },
            Transform::from_xyz(0.0, 192.0 - (top + bottom) * 0.5, 100.0),
            Visibility::Hidden,
            bevy::camera::visibility::RenderLayers::layer(HUD_LAYER),
            LetterBox,
        ));
    }
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
        TextColor(Color::linear_rgb(1.0, 0.9, 0.4)),
        Anchor::CENTER,
        Transform::from_xyz(0.0, 40.0, 0.0),
        Banner,
    ));
    commands.spawn((
        Text2d::new(String::new()),
        TextFont { font_size: 26.0, ..default() },
        TextColor(Color::linear_rgb(1.0, 0.95, 0.7)),
        Anchor::CENTER,
        Transform::from_xyz(0.0, 0.0, 1.0),
        FrontText,
    ));
}

/// The character body of every player slot (co-op players use player 1's
/// model: their character select is not ported).
/// player_rhand[class] (ELF table of label pointers): the hand node the
/// held weapon hangs from (load_player).
fn player_rhand(elf_bytes: &[u8], class: u32) -> Option<String> {
    let elf = gdl_formats::elf::Elf::parse(elf_bytes).ok()?;
    let table = elf.object("player_rhand").ok()?;
    let p = u32::from_le_bytes(table.get(class as usize * 4..class as usize * 4 + 4)?.try_into().ok()?);
    let s = elf.read(p, 16).ok()?;
    Some(s.iter().take_while(|b| **b != 0).map(|b| char::from(*b)).collect())
}

/// The ground shadow object of a player slot (load_player: SHADOWL1, MB
/// flags 0x880, z modifier −36; UpdatePlayerShadow places it).
#[derive(Component)]
struct PlayerShadow(usize);

fn spawn_bodies(commands: &mut Commands, root: Entity, sources: &Sources, session: &Session, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>, materials: &mut Assets<gdl_client::GsMaterial>) {
    let character = &sources.character;
    let set = gdl_client::materials_for(&character.objects, &character.table, &character.textures, images, materials);
    let (parts, _) = gdl_client::object_meshes(&character.objects, &set, materials, meshes);
    let object_parts = |label: &str| -> Vec<(Handle<Mesh>, Handle<gdl_client::GsMaterial>)> {
        character.objects.definitions.iter().filter(|d| d.label == label).flat_map(|d| parts.get(&(d.object_index as u32)).cloned().unwrap_or_default()).collect()
    };
    let rhand = player_rhand(&session.elf_bytes, session.class);
    // load_player: base classes (player_multiple_models) hold "WEAP_HOLD"
    // of the model package, an MBNewObject under the right hand (0x810).
    let weapon = object_parts("WEAP_HOLD");
    let shadow: Vec<_> = object_parts("SHADOWL1")
        .into_iter()
        .map(|(mesh, m)| {
            let v = materials.get(&m).cloned().map(|x| materials.add(x.with_draw(0x880)));
            (mesh, v.unwrap_or(m))
        })
        .collect();
    for slot in 0..4 {
        let vis = if slot == 0 { Visibility::default() } else { Visibility::Hidden };
        let body = commands.spawn((CharacterRoot(slot), Transform::default(), vis)).id();
        commands.entity(root).add_child(body);
        let tree = &sources.rig.trees[0];
        // Rig nodes pose by the class clip node of the same label (the root,
        // "UNNAMED" in the model, is clip node 0).
        let clip_nodes = &sources.clips.trees[0].nodes;
        for node in &tree.nodes {
            let pose = if node.index == 0 { Some(0) } else { clip_nodes.iter().position(|c| c.label == node.label) };
            let Some(pose) = pose else { continue };
            let label: String = (tree.prefix.clone() + &node.label).chars().take(16).collect();
            for def in character.objects.definitions.iter().filter(|d| d.label == label) {
                for (mesh, material) in parts.get(&(def.object_index as u32)).into_iter().flatten() {
                    let child = commands
                        .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), RigNode(pose, slot)))
                        .id();
                    commands.entity(body).add_child(child);
                }
            }
            if rhand.as_deref() == Some(node.label.as_str()) {
                for (mesh, material) in &weapon {
                    let child = commands
                        .spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), RigNode(pose, slot)))
                        .id();
                    commands.entity(body).add_child(child);
                }
            }
        }
        let shadow_root = commands.spawn((PlayerShadow(slot), Transform::default(), Visibility::Hidden)).id();
        commands.entity(root).add_child(shadow_root);
        for (mesh, material) in &shadow {
            let child = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default())).id();
            commands.entity(shadow_root).add_child(child);
        }
    }
}

/// UpdatePlayerShadow 0x147fa0: the shadow at the player's x/z and the
/// higher of floor (+0x88c) and water (+0x8a4) heights + 0.1 (the water
/// case's splash alternate texture is not ported).
fn present_player_shadow(sim: Res<Sim>, mut q: Query<(&PlayerShadow, &mut Transform, &mut Visibility)>) {
    let active: [bool; 4] = [true, sim.coop[0], sim.coop[1], sim.coop[2]];
    for (s, mut tf, mut vis) in &mut q {
        let r = &sim.game.players[s.0];
        if !active[s.0] || matches!(r.i32_(0xfc), 0 | 5 | 0xb) {
            *vis = Visibility::Hidden;
            continue;
        }
        let p = r.v3(0x50);
        let (floor, water) = (r.f32_(0x88c), r.f32_(0x8a4));
        let y = if water <= floor { floor } else { water } + 0.1;
        tf.translation = Vec3::new(p[0], y, p[2]);
        *vis = Visibility::Inherited;
    }
}

/// The game scene: level geometry, entity slots, bodies and libraries.
#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    sources: Res<Sources>,
    session: Res<Session>,
    sim: Res<Sim>,
    mut libraries: ResMut<Libraries>,
    mut frames: ResMut<EnemyFrames>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
) {
    let root = commands.spawn((Transform::from_scale(MIRROR), Visibility::default(), MirrorRoot)).id();
    spawn_level(&mut commands, root, &session, sim.game.items.records.len(), &mut meshes, &mut images, &mut materials);
    spawn_bodies(&mut commands, root, &sources, &session, &mut meshes, &mut images, &mut materials);
    for (name, pkg) in &sources.packages {
        if !libraries.0.contains_key(name) {
            libraries.0.insert(name.clone(), build_library(pkg, &mut meshes, &mut images, &mut materials));
        }
        if sources.monsters.values().any(|m| m == name) && !frames.0.contains_key(name) {
            frames.0.insert(name.clone(), enemy_frames(pkg));
        }
    }
    spawn_package_texmods(&mut commands, &libraries, &level_packages(&session.levels[session.current]));
}

/// Level geometry (original translation-only placements) and entity slots.
fn spawn_level(
    commands: &mut Commands,
    root: Entity,
    session: &Session,
    item_count: usize,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<gdl_client::GsMaterial>,
) {
    let level = &session.level;
    let set = gdl_client::materials_for(&level.objects, &level.table, &level.textures, images, materials);
    let (parts, _) = gdl_client::object_meshes(&level.objects, &set, materials, meshes);
    let names: HashMap<_, _> = level.objects.definitions.iter().map(|d| (d.label.as_str(), d.object_index as u32)).collect();
    let nodes = &session.world.nodes;
    // CreateWorldNode: MB flags = record +0x18, | 4 for file flags & 0x1001
    // == 1, | 1 (not drawn) for file flag 0x8000.
    let mb_flags = |i: usize| {
        let n = &nodes[i];
        let mut f = n.render_flags;
        if n.flags & 0x1001 == 1 {
            f |= 4;
        }
        if n.flags & 0x8000 != 0 {
            f |= 1;
        }
        f
    };
    let mut variants: HashMap<(AssetId<gdl_client::GsMaterial>, u32), Handle<gdl_client::GsMaterial>> = HashMap::new();
    if let Ok(placements) = session.world.placements() {
        let parent: HashMap<usize, usize> =
            placements.iter().filter_map(|p| p.parent.map(|q| (p.node_index as usize, q as usize))).collect();
        let position: HashMap<usize, Vec3> = placements.iter().map(|p| (p.node_index as usize, Vec3::from_array(p.summed_position))).collect();
        for p in &placements {
            let i = p.node_index as usize;
            let node = &nodes[i];
            // NewWorldObject sets a model only on non-particle nodes with
            // +0x28 == 1 (or file flag 0x80000000); pbTraverseDrawObjects skips
            // MB flag 2 with its subtree and flag 1 for the node itself.
            let has_object = node.flags & 0x800 == 0
                && (i32::from_le_bytes(node.raw[40..44].try_into().unwrap()) == 1 || node.flags & 0x80000000 != 0);
            let flags = mb_flags(i);
            let mut face = None;
            let mut chain = Vec::new();
            let mut k = Some(i);
            while let Some(a) = k {
                let f = mb_flags(a);
                chain.push((a, f));
                if face.is_none() && f & 0xf000000 != 0 {
                    face = Some((a, f & 0xf000000));
                }
                k = parent.get(&a).copied();
            }
            if !has_object {
                continue;
            }
            let hidden = node_hidden(&chain, &[], &[]);
            let Some(list) = names.get(node.label.as_str()).and_then(|i| parts.get(i)) else { continue };
            let rest = Vec3::from_array(p.summed_position);
            for (mesh, material) in list {
                let key = (material.id(), flags & (0x800000 | 0x4000 | 0x80 | 0x40));
                let material = variants
                    .entry(key)
                    .or_insert_with(|| {
                        let m = materials.get(material).expect("part material").with_draw(flags);
                        materials.add(m)
                    })
                    .clone();
                let mut entity = commands.spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material),
                    Transform::from_translation(rest),
                    WorldNode { node: i, rest },
                    NodeChain(chain.clone()),
                    if hidden { Visibility::Hidden } else { Visibility::Inherited },
                    LevelEntity,
                ));
                if let Some((anchor, mode)) = face {
                    let at = position.get(&anchor).copied().unwrap_or(rest);
                    entity.insert(CameraFace { mode, anchor: at, offset: rest - at });
                }
                if flags & 0x100400 != 0 {
                    // A material of its own: the alpha follows the distance.
                    let mut m = materials.get(&variants[&key]).cloned().expect("variant");
                    if m.alpha_mode == AlphaMode::Opaque || matches!(m.alpha_mode, AlphaMode::Mask(_)) {
                        m.alpha_mode = AlphaMode::Blend;
                    }
                    let own = materials.add(m);
                    entity.insert((MeshMaterial3d(own), DistanceFade { max: if flags & 0x100000 != 0 { 200.0 } else { 100.0 }, at: rest }));
                }
                let child = entity.id();
                commands.entity(root).add_child(child);
            }
        }
    }
    spawn_world_psys(commands, root, session, &set, meshes);
    spawn_texmods(commands, &level.anim_bytes, &set);
    for index in 0..gdl_core::game::critters::MAX_INSTS {
        let e = commands.spawn((CritterSlot { index, shown: None }, Transform::default(), Visibility::Hidden, LevelEntity)).id();
        commands.entity(root).add_child(e);
    }
    for (kind, n) in [(SlotKind::Enemy, 25), (SlotKind::Item, item_count), (SlotKind::Effect, 64)] {
        for index in 0..n {
            let e = commands.spawn((Slot { kind, index, key: None }, Transform::default(), Visibility::Hidden, LevelEntity)).id();
            commands.entity(root).add_child(e);
        }
    }
}

/// The GDL_DEBUG_CAMERA override: [eye, up, forward] in original space.
fn debug_camera() -> Option<[Vec3; 3]> {
    static CAMERA: std::sync::OnceLock<Option<[Vec3; 3]>> = std::sync::OnceLock::new();
    *CAMERA.get_or_init(|| {
        let path = std::env::var("GDL_DEBUG_CAMERA").ok()?;
        let v: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        let vec = |k: &str| -> Option<Vec3> { Some(Vec3::from_array(serde_json::from_value::<[f32; 3]>(v[k].clone()).ok()?)) };
        Some([vec("eye")?, vec("up")?, vec("forward")?])
    })
}

/// The world's texture modifiers (GauntWorldInit → InitTexMods; stepped by
/// DoWorldAnimation → DoTexMods once per game update).
#[derive(Component)]
struct TexMods {
    mods: Vec<TexModState>,
    aframe: u32,
    accumulated: f32,
    /// Materials by base image, built on first use (scroll targets).
    by_image: Option<HashMap<AssetId<Image>, Vec<AssetId<gdl_client::GsMaterial>>>>,
}

enum TexModKind {
    /// MBSetRomTexture(target, frame base + counter).
    Frames { target: Handle<Image>, frames: Vec<Handle<Image>> },
    /// DoTexScroll2Sub: the target's U (−2) or V (−3) shift add.
    Scroll { target: AssetId<Image>, v: bool },
}

struct TexModState {
    kind: TexModKind,
    count: i16,
    start: i16,
    divisor: i32,
    counter: i32,
}

/// InitTexMod 0x110f20 for the level package: frame modifiers resolve their
/// frame base (+0x48, or by name for −1: MBOX_FindTexture + MBCopyTexture);
/// −2/−3 become texture shifts; fades (−4/−5) and −6 are not ported.
fn spawn_texmods(commands: &mut Commands, anim: &[u8], set: &gdl_client::MaterialSet) {
    let list = gdl_formats::animation::texmods(anim).unwrap_or_default();
    spawn_texmod_list(commands, &list, &set.label_index, &set.record_images);
}

/// SetupItemTexMods / SetupWeaponPowerupTexMods: the texture modifiers of
/// the level's item packages and of WEAPONS/POWERUPS (stepped by do_items).
fn spawn_package_texmods(commands: &mut Commands, libraries: &Libraries, names: &[String]) {
    for name in names {
        if let Some(lib) = libraries.0.get(name) {
            spawn_texmod_list(commands, &lib.texmods, &lib.label_index, &lib.record_images);
        }
    }
}

fn spawn_texmod_list(
    commands: &mut Commands,
    list: &[gdl_formats::animation::TexMod],
    label_index: &HashMap<String, u16>,
    record_images: &HashMap<u16, Handle<Image>>,
) {
    let mut mods = Vec::new();
    for tm in list.iter().filter(|t| t.owner == -1) {
        let Ok(target) = u16::try_from(tm.target) else { continue };
        let Some(target_image) = record_images.get(&target) else { continue };
        let kind = match tm.frame_base {
            -2 | -3 => TexModKind::Scroll { target: target_image.id(), v: tm.frame_base == -3 },
            -6..=-4 => continue,
            base => {
                let base = if base == -1 { label_index.get(&tm.frame_name.to_uppercase()).map(|&i| i32::from(i)) } else { Some(base) };
                let Some(base) = base else { continue };
                let n = i32::from(tm.count).abs().max(1);
                let frames: Vec<_> = (0..n).filter_map(|k| u16::try_from(base + k).ok().and_then(|i| record_images.get(&i)).cloned()).collect();
                if frames.is_empty() {
                    continue;
                }
                TexModKind::Frames { target: target_image.clone(), frames }
            }
        };
        mods.push(TexModState { kind, count: tm.count, start: tm.start, divisor: tm.divisor, counter: tm.counter });
    }
    if !mods.is_empty() {
        commands.spawn((TexMods { mods, aframe: 0, accumulated: 0.0, by_image: None }, LevelEntity));
    }
}

/// DoTexModSub 0x110e10 per AFrame (30 Hz presentation clock): every
/// `divisor` frames the counter advances modulo |count|; frame modifiers
/// copy frame `counter` into the target (initially frame 0, InitTexMod),
/// scrolls set the shift add sign·(counter mod |count|)/|count|.
fn present_texmods(time: Res<Time>, mut q: Query<&mut TexMods>, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<gdl_client::GsMaterial>>) {
    for mut tm in &mut q {
        let tm = &mut *tm;
        let first = tm.by_image.is_none();
        if first {
            let mut map: HashMap<AssetId<Image>, Vec<AssetId<gdl_client::GsMaterial>>> = HashMap::new();
            for (id, m) in materials.iter() {
                if let Some(b) = &m.base {
                    map.entry(b.id()).or_default().push(id);
                }
            }
            tm.by_image = Some(map);
        }
        tm.accumulated += time.delta_secs();
        let mut changed = first;
        let mut steps = 0;
        while tm.accumulated >= 1.0 / 30.0 && steps < 4 {
            tm.accumulated -= 1.0 / 30.0;
            steps += 1;
            tm.aframe = tm.aframe.wrapping_add(1);
            for m in &mut tm.mods {
                if 0 < m.divisor && tm.aframe % m.divisor as u32 != 0 {
                    continue;
                }
                m.counter += 1;
                if i32::from(m.count).abs() <= m.counter {
                    m.counter = 0;
                }
                changed = true;
            }
        }
        if !changed {
            continue;
        }
        let by_image = tm.by_image.as_ref().unwrap();
        for m in &tm.mods {
            match &m.kind {
                TexModKind::Frames { target, frames } => {
                    let k = if first && steps == 0 { 0 } else { (m.counter.max(0) as usize).min(frames.len() - 1) };
                    let Some(mut frame) = images.get(&frames[k]).cloned() else { continue };
                    if let Some(old) = images.get(target) {
                        frame.sampler = old.sampler.clone();
                    }
                    let _ = images.insert(target.id(), frame);
                }
                TexModKind::Scroll { target, v } => {
                    let n = f32::from(m.count).abs().max(1.0);
                    let sign = if m.count < 0 { -1.0 } else { 1.0 };
                    let _ = m.start;
                    let shift = sign * (m.counter.rem_euclid(n as i32) as f32 / n);
                    for id in by_image.get(target).into_iter().flatten() {
                        if let Some(mat) = materials.get_mut(*id) {
                            if *v {
                                mat.params.options.w = shift;
                            } else {
                                mat.params.options.z = shift;
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A world particle system (WorldPsysActivate → MBNewWorldPsys) with its
/// node's world matrix (row-major PS2 layout) and the sprites of the last
/// psys frame.
#[derive(Component)]
struct Emitter {
    psys: gdl_core::psys::Psys,
    /// A tree node anchor (item/effect psys) the emitter follows; the world
    /// psys have none.
    anchor: Option<Entity>,
    matrix: [f32; 16],
    sprites: Vec<gdl_core::psys::Sprite>,
    /// Level texture by name, resolved against the STATIC package later.
    texture: Option<Handle<Image>>,
    material: Option<Handle<gdl_client::GsMaterial>>,
}

/// MBPsysStartFrame's frame counter, the psys random source and the
/// shared direction table, stepped at 30 Hz of presentation time.
#[derive(Resource)]
struct PsysClock {
    global: gdl_core::psys::PsysGlobal,
    accumulated: f32,
}

/// World psys nodes (file flag 0x800, not 0x10000000; NewWorldObject):
/// the WORLDS psys record whose +6 letter follows "PSYS" in the node label
/// (GetWorldPsysIdx), the negated first collision normal as direction.
fn spawn_world_psys(commands: &mut Commands, root: Entity, session: &Session, set: &gdl_client::MaterialSet, meshes: &mut Assets<Mesh>) {
    let world = &session.world;
    let bytes = &world.bytes;
    let header = |i: usize| bytes.get(i * 4..i * 4 + 4).map_or(0, |b| u32::from_le_bytes(b.try_into().unwrap()));
    let version = header(0x18);
    if version & 0xf00bab00 != 0xf00bab00 || version & 0xff < 2 {
        return;
    }
    let (count, offset) = (header(0x1c) as usize, header(0x1d) as usize);
    let Some(table) = bytes.get(offset..offset + count * gdl_core::psys::RECORD) else { return };
    let presets = gdl_formats::elf::Elf::parse(&session.elf_bytes)
        .ok()
        .and_then(|e| e.object("preset_list").ok().map(|b| b.to_vec()))
        .unwrap_or_default();
    let Ok(placements) = world.placements() else { return };
    for p in placements {
        let node = &world.nodes[p.node_index as usize];
        if node.flags & 0x800 == 0 || node.flags & 0x10000000 != 0 {
            continue;
        }
        let Some(at) = node.label.find("PSYS") else { continue };
        let Some(letter) = node.label.as_bytes().get(at + 4).copied() else { continue };
        let Some(record) = table.chunks_exact(gdl_core::psys::RECORD).find(|r| r[6] == letter) else { continue };
        let dir = (0 < node.collision_count)
            .then(|| world.collisions.get(node.collision_index as usize))
            .flatten()
            .map(|c| c.normal.map(|v| -v));
        let Ok(psys) = gdl_core::psys::Psys::new_world(0, record, &presets, dir, node.flags & 0x1000 != 0) else { continue };
        let [x, y, z] = p.summed_position;
        let matrix = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0];
        let texture = psys.texture.as_ref().and_then(|t| set.labels.get(&t.to_uppercase())).cloned();
        let mesh = meshes.add(Mesh::new(bevy::render::render_resource::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default()));
        let e = commands
            .spawn((
                Emitter { psys, anchor: None, matrix, sprites: Vec::new(), texture, material: None },
                Mesh3d(mesh),
                Transform::default(),
                Visibility::Hidden,
                LevelEntity,
            ))
            .id();
        commands.entity(root).add_child(e);
    }
}

type EmitterQuery<'w, 's> = Query<'w, 's, (&'static mut Emitter, &'static Mesh3d, &'static mut Visibility, Option<&'static MeshMaterial3d<gdl_client::GsMaterial>>, Entity)>;

/// Tree psys created by spawn_parts, waiting for their render entity.
#[derive(Resource, Default)]
struct PendingTreePsys(Vec<(Entity, gdl_core::psys::Psys)>);

fn spawn_tree_psys(mut commands: Commands, mut pending: ResMut<PendingTreePsys>, root: Query<Entity, With<MirrorRoot>>, mut meshes: ResMut<Assets<Mesh>>) {
    let Ok(root) = root.single() else { return };
    for (anchor, psys) in pending.0.drain(..) {
        let mesh = meshes.add(Mesh::new(bevy::render::render_resource::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default()));
        let e = commands
            .spawn((
                Emitter { psys, anchor: Some(anchor), matrix: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0], sprites: Vec::new(), texture: None, material: None },
                Mesh3d(mesh),
                Transform::default(),
                Visibility::Hidden,
                LevelEntity,
            ))
            .id();
        commands.entity(root).add_child(e);
    }
}

/// Follow tree anchors: original-space matrix = MIRROR × the anchor's render
/// transform; emitters of despawned anchors go away.
fn follow_tree_psys(mut commands: Commands, anchors: Query<(&GlobalTransform, &InheritedVisibility)>, mut q: Query<(Entity, &mut Emitter)>) {
    for (e, mut em) in &mut q {
        let Some(a) = em.anchor else { continue };
        let Ok((g, vis)) = anchors.get(a) else {
            commands.entity(e).despawn();
            continue;
        };
        let m = Mat4::from_scale(MIRROR) * g.to_matrix();
        em.matrix = [
            m.x_axis.x, m.x_axis.y, m.x_axis.z, 0.0, m.y_axis.x, m.y_axis.y, m.y_axis.z, 0.0, m.z_axis.x, m.z_axis.y, m.z_axis.z, 0.0, m.w_axis.x,
            m.w_axis.y, m.w_axis.z, 1.0,
        ];
        if !vis.get() {
            em.psys.node_flags |= 0x200000;
        } else {
            em.psys.node_flags &= !0x200000;
        }
    }
}

/// MBDrawPsys per psys frame (30 Hz) for every emitter whose sphere
/// (radius +0xa4) is in the camera frustum or that drew sprites last time
/// (pbRenderNode case 0xc), then DrawPsysSub's camera-facing sprites
/// rebuilt with the current window camera rows.
#[allow(clippy::too_many_arguments)]
fn present_particles(
    time: Res<Time>,
    sim: Res<Sim>,
    mut clock: ResMut<PsysClock>,
    libraries: Res<Libraries>,
    cams: Query<&bevy::camera::primitives::Frustum, With<FollowCamera>>,
    mut emitters: EmitterQuery,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
    mut commands: Commands,
) {
    let Some(cam) = &sim.game.std_camera else { return };
    let rows = cam.pb.cam_rows.map(Vec3::from_array);
    let frustum = cams.iter().next();
    clock.accumulated += time.delta_secs();
    let mut steps = 0;
    while clock.accumulated >= 1.0 / 30.0 && steps < 4 {
        clock.accumulated -= 1.0 / 30.0;
        steps += 1;
        let PsysClock { global, .. } = &mut *clock;
        global.start_frame();
        for (mut em, ..) in &mut emitters {
            let em = &mut *em;
            if em.psys.finished() {
                em.sprites.clear();
                continue;
            }
            let center = Vec3::new(em.matrix[12], em.matrix[13], em.matrix[14]);
            let visible = frustum.is_none_or(|f| {
                f.intersects_sphere(&bevy::camera::primitives::Sphere { center: (center * MIRROR).into(), radius: em.psys.radius }, true)
            });
            if !visible && em.sprites.is_empty() {
                continue;
            }
            em.sprites.clear();
            em.psys.draw(global, &em.matrix, 1.0, visible, &mut em.sprites);
        }
    }
    if 4 <= steps {
        clock.accumulated = 0.0;
    }
    let (right, up) = (rows[0], rows[1]);
    for (mut em, mesh, mut vis, material, entity) in &mut emitters {
        if em.material.is_none() {
            // MBOX_FindTexture searches the loaded packages in load order: the
            // global STATIC, WEAPONS and POWERUPS before the level (the Tower
            // dump's flame sprites bind WEAPONS' 64×64 P_TORCH, not the items
            // package's). setupParms: no texture → particle2_a (particle2_xp for
            // additive nodes).
            let name = em.psys.texture.clone().map(|t| t.to_uppercase()).unwrap_or_else(|| {
                if em.psys.node_flags & 0x800000 != 0 { "PARTICLE2_XP".into() } else { "PARTICLE2_A".into() }
            });
            let named = ["STATIC", "WEAPONS", "POWERUPS"]
                .iter()
                .find_map(|p| libraries.0.get(*p).and_then(|l| l.textures.get(&name)).cloned())
                .or_else(|| em.texture.clone())
                .or_else(|| libraries.0.values().find_map(|l| l.textures.get(&name).cloned()));
            if named.is_none() && !libraries.0.contains_key("STATIC") {
                continue;
            }
            let blend = em.psys.blend();
            let (no_depth_write, depth_always) = em.psys.depth_flags();
            let m = gdl_client::GsMaterial {
                params: gdl_client::GsParams { tint: Vec4::ONE, options: Vec4::new(if named.is_some() { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0), ..default() },
                lit: false,
                base: named,
                lightmap: None,
                alpha_mode: if blend == gdl_core::psys::Blend::Additive { AlphaMode::Add } else { AlphaMode::Blend },
                draw: gdl_client::GsDraw {
                    additive: blend == gdl_core::psys::Blend::Additive,
                    multiply: blend == gdl_core::psys::Blend::Multiply,
                    fixed_alpha: em.psys.fixed_alpha(),
                    no_depth_write,
                    depth_always,
                },
            };
            let handle = materials.add(m);
            em.material = Some(handle.clone());
            if material.is_none() {
                commands.entity(entity).insert(MeshMaterial3d(handle));
            }
        }
        if em.sprites.is_empty() {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Inherited;
        let Some(m) = meshes.get_mut(&mesh.0) else { continue };
        let n = em.sprites.len();
        let mut positions = Vec::with_capacity(n * 4);
        let mut colors = Vec::with_capacity(n * 4);
        let mut uvs = Vec::with_capacity(n * 4);
        let mut indices = Vec::with_capacity(n * 6);
        for (k, sp) in em.sprites.iter().enumerate() {
            let c = Vec3::from_array(sp.position);
            let h = sp.size * 0.5;
            // GS sprite from (c − h·(R+U), UV 0) to (c + h·(R+U), UV size).
            for (dx, dy, u, v) in [(-1.0, -1.0, 0.0, 0.0), (1.0, -1.0, 1.0, 0.0), (1.0, 1.0, 1.0, 1.0), (-1.0, 1.0, 0.0, 1.0)] {
                positions.push((c + right * (dx * h) + up * (dy * h)).to_array());
                uvs.push([u, v]);
                colors.push([
                    f32::from(sp.rgba[0]) / 128.0,
                    f32::from(sp.rgba[1]) / 128.0,
                    f32::from(sp.rgba[2]) / 128.0,
                    f32::from(sp.rgba[3]) / 128.0,
                ]);
            }
            let b = (k * 4) as u32;
            indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; n * 4]);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        m.insert_indices(bevy::mesh::Indices::U32(indices));
    }
}

/// The interim player simulation of a character: the captured template
/// with the record's class values (speed +0x124, collision offset +0x81c,
/// radius +0x828, height +0x82c, scales +0x964/+0x968/+0x96c).
/// Build the native game for level `idx` with player 0's record `carry`,
/// entered at the level's start locator (get_player_pos). init_next_level's
/// record steps run first: load_player_geo/load_player (profile::level_start)
/// and PlayerSaveState (the Tower: without the backup copy; the
/// Underworld 0xc: not called).
fn build_game(session: &Session, idx: usize, carry: &Record) -> Result<Game, Box<dyn Error>> {
    let elf = gdl_formats::elf::Elf::parse(&session.elf_bytes)?;
    let lv = &session.levels[idx];
    let wdata = gdl_formats::wdata::WorldData::parse(&gdl_client::bounded_read(&lv.wdata)?)?;
    let mut start = session.start.clone();
    let mut record = carry.clone();
    if lv.world == 0xd && record.i32_(0xfc) == 0xb {
        // init_next_level: a player who died (state 0xb) gets the level-start
        // backup back (PlayerRestoreState) and plays again.
        profile::player_restore_state(&mut record, &session.pdats, &gdl_core::player_class::ClassTables::from_elf(&elf)?).map_err(|e| e.to_string())?;
        record.set_i32(0xfc, 1);
    }
    let game_wave = lv.world << 8 | lv.index as i32;
    profile::level_start(&mut record, &session.pdats, &session.action_map, game_wave, session.heart_rate).map_err(|e| e.to_string())?;
    if lv.world != 0xc {
        profile::player_save_state(&mut record, lv.world != 0xd, lv.world, lv.index as i32, &session.controls);
    }
    start["players"][0] = Value::String(record.0.iter().map(|b| format!("{b:02x}")).collect());
    start["classes"][0] = serde_json::to_value(&session.class_data)?;
    start["globals"]["cur_world"] = Value::from(lv.world);
    start["globals"]["tower_counter"] = Value::from(session.carried[0]);
    start["globals"]["last_finished_world"] = Value::from(session.carried[1]);
    start["globals"]["last_finished_level"] = Value::from(session.carried[2]);
    start["globals"]["cur3d_transmitter"] = Value::from(session.carried[3]);
    start["globals"]["cur_transmitter"] = Value::from(session.carried[4]);
    start["globals"]["cur_level"] = Value::from(lv.index as i64);
    let start: setup::StartState = serde_json::from_value(start)?;
    let mut enemy_anims = gdl_core::game::enemy_ai::EnemyAnimData::default();
    for (t, prefix, path) in session.monster_paths.iter().filter(|m| lv.monsters.contains(&m.1)) {
        let anim = Animations::parse(&gdl_client::bounded_read(Path::new(path))?)?;
        setup::enemy_trees(&mut enemy_anims, *t, prefix, &anim);
    }
    let anim = |pkg: &str| -> Result<Animations, Box<dyn Error>> {
        let path = session.anim_paths.get(pkg).ok_or_else(|| format!("{pkg} anim path"))?;
        Ok(Animations::parse(&gdl_client::bounded_read(Path::new(path))?)?)
    };
    let mut item_anims = HashMap::new();
    for pkg in &lv.items {
        let items_anim = anim(pkg)?;
        for (k, t) in items_anim.trees.iter().enumerate() {
            item_anims.entry(t.label.clone()).or_insert_with(|| setup::clips_of(&items_anim, k));
        }
    }
    let weapons = anim("WEAPONS")?;
    let powerups = anim("POWERUPS")?;
    let effect_tables = gdl_core::game::effects::EffectTables::from_elf(&elf, &weapons, &powerups)?;
    let mut game = Game::new_level(setup::LevelInputs {
        world: &session.world,
        wdata: &wdata,
        level: lv.index,
        elf: &elf,
        start,
        enemy_anims,
        item_anims,
        effect_tables,
        audio_rom: session.audio_rom.clone(),
        mb_objects: Some(session.object_labels.iter().cloned().chain(session.level.objects.definitions.iter().map(|d| d.label.clone())).collect()),
        critters: lv
            .critters
            .iter()
            .map(|(wad, model, _)| -> Result<_, Box<dyn Error>> {
                let w = gdl_formats::critter::CritterWad::parse(&gdl_client::bounded_read(Path::new(wad))?)?;
                let m = match model {
                    Some(m) => {
                        let bytes = gdl_client::bounded_read(Path::new(m))?;
                        Some(std::sync::Arc::new((Animations::parse(&bytes)?, bytes)))
                    }
                    None => None,
                };
                Ok((wad.clone(), w, m))
            })
            .collect::<Result<Vec<_>, _>>()?,
    })?;
    {
        // EnterTower: SetPlayerStartPos(GetWorldOrder(LastWorldPlayed)) puts
        // a returning player at that realm's wing (WorldOrder 0x23f4c8);
        // WorldOpen's check is not ported (HYPOTHESIS: the start is open).
        let mut id = 0;
        if lv.world == 0xd
            && let Ok(order) = elf.object("WorldOrder")
        {
            let order: Vec<i32> = order.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
            id = order.iter().position(|&w| w == session.last_world).unwrap_or(0) as i16;
        }
        let loc = session
            .world
            .locators
            .iter()
            .find(|l| l.kind == 7 && l.id == id)
            .or_else(|| session.world.locators.iter().find(|l| l.kind == 7 && l.id == 0))
            .ok_or("level has no player start locator")?;
        game.place_player(0, loc.position, loc.rotation[1]).map_err(|e| e.0)?;
        game.players[0].set_u32(0x1e0, 0);
        game.players[0].set_u32(0x1e4, 0);
        game.player_clips[0] = session.clip_timings.clone();
        game.init_player_anim(0).map_err(|e| e.0)?;
        game.controls.assign(0, 0);
        // init_next_level: init_game_cam + StdCamInit(0) after the players.
        game.start_camera().map_err(|e| e.0)?;
    }
    Ok(game)
}

/// PLACEHOLDER co-op join (the original's character select, LoadPlyrData
/// and can_join timers are not ported): port `k`'s player enters as a copy
/// of `from`'s record beside player 0, sharing its class animation.
fn join_copy(game: &mut Game, k: usize, from: usize) -> Result<(), String> {
    let mut r = game.players[from].clone();
    r.set_u32(0, k as u32);
    game.players[k] = r;
    let mut at = game.players[0].v3(0x50);
    at[0] += 3.0 * k as f32;
    let yaw = game.players[0].f32_(0x86c);
    game.place_player(k, at, yaw).map_err(|e| e.0)?;
    game.players[k].set_u32(0x1e0, 0);
    game.players[k].set_u32(0x1e4, 0);
    game.player_clips[k] = game.player_clips[0].clone();
    game.init_player_anim(k).map_err(|e| e.0)?;
    game.controls.assign(k, k);
    game.set_player_vars();
    // PLACEHOLDER (with the join itself): the slot counts as active for the
    // panels and gets its box (load_player_geo → setup_player_display).
    game.globals.active_players |= 1 << k;
    game.setup_player_display(k);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn simulate(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<(Entity, &Gamepad)>,
    mut sim: ResMut<Sim>,
    session: Res<Session>,
    presentation: Res<Presentation>,
    ui: Res<Ui>,
    mut audio: Option<NonSendMut<gdl_client::audio::AudioOut>>,
    front: Res<Front>,
) {
    if sim.error.is_some() || session.banner.is_some() || session.finished.is_some() || !ui.started || ui.paused || sim.message_box.is_some() {
        return;
    }
    let mut pads: Vec<(Entity, &Gamepad)> = gamepads.iter().collect();
    pads.sort_by_key(|(e, _)| *e);
    let pads: Vec<&Gamepad> = pads.into_iter().map(|(_, g)| g).collect();
    let mut ports = [PadStatus::default(); 8];
    for (k, port) in ports.iter_mut().take(4).enumerate() {
        *port = player_input(k, &keys, &pads);
        // Only ports with a device are connected (keyboard: ports 0 and 1).
        port.connected = k < 2 || pads.get(k).is_some();
    }
    for (k, port) in ports.iter().enumerate().take(4).skip(1) {
        let attack = port.entries[0xd][0] & 1 != 0;
        if !sim.coop[k - 1] && attack && port.connected && 0.0 < sim.game.players[0].f32_(0x1a10) {
            match join_copy(&mut sim.game, k, 0) {
                Ok(()) => {
                    sim.coop[k - 1] = true;
                    sim.last_event = format!("player {} joined (PLACEHOLDER copy of player 1)", k + 1);
                }
                Err(e) => sim.last_event = format!("join failed: {e}"),
            }
        }
    }
    let Sim { game, .. } = &mut *sim;
    let before = game.events.len();
    game.step_with(None, &mut |g| {
        g.update_controls(&ports)?;
        g.do_players().map(|_| ())
    });
    sim.updates += 1;
    // DEBUG (presentation checks): GDL_DEBUG_HELP=<id> raises NewHelp(id,
    // player 0) once, 60 updates into the level. Not a game feature.
    if sim.updates == 60
        && let Some(id) = std::env::var("GDL_DEBUG_HELP").ok().and_then(|v| i32::from_str_radix(v.trim_start_matches("0x"), 16).ok())
    {
        sim.game.help(id, 0);
    }
    // DEBUG: GDL_DEBUG_GOTIT=<kind>:<amount> raises add_got_it(0, …) once.
    if sim.updates == 60
        && let Some((k, a)) = std::env::var("GDL_DEBUG_GOTIT").ok().and_then(|v| v.split_once(':').and_then(|(k, a)| Some((k.parse().ok()?, a.parse().ok()?))))
    {
        sim.game.got_it(0, k, a);
    }
    let new: Vec<Event> = sim.game.events.drain(before..).collect();
    sim.game.events.clear();
    let me = Vec3::from_array(sim.game.players[0].v3(0x50));
    let right = sim.game.std_camera.as_ref().map_or(presentation.basis[0], |c| Vec3::from_array(c.pb.cam_rows[0]));
    for e in &new {
        if let Some(a) = audio.as_mut() {
            match e {
                Event::Sound { id, position, volume, priority, queue } => {
                    // Presentation pan: offset along the camera's right axis.
                    let pan = position.map_or(0.0, |p| ((Vec3::from_array(p) - me).dot(right) / 20.0).clamp(-1.0, 1.0));
                    a.play(*id, *volume, pan, *priority, *queue);
                }
                Event::LoopSound { id, position, volume, priority } => {
                    let pan = ((Vec3::from_array(*position) - me).dot(right) / 20.0).clamp(-1.0, 1.0);
                    a.play_loop(*id, *volume, pan, *priority);
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
            // NewHelp draws its own box (Game::new_help / DoHelp).
            Event::HintMenu { .. } => sim.hint_menu = true,
            Event::ScrollMessage { text, .. } => {
                // A scroll item: line `text` of SCROLLS<level code>
                // (SetScrollLevelMsgList(level_data + 8)).
                let code = format!("SCROLLS{}", session.levels[session.current].name);
                if let Some(e) = front.scrolls.entry(&code)
                    && let Some(line) = usize::try_from(text).ok().and_then(|k| e.lines.get(k))
                {
                    let sb = scroll_box(&sim.game, &front.scrolls, e, vec![line.replace('\r', "")]);
                    sim.message_box = Some(sb);
                }
            }
            Event::ScrollNamed { name, line, .. } => {
                // FindScrollMessage(name): SCROLL_E.ROM entries are upper case.
                if let Some(e) = front.scrolls.entry(&name.to_uppercase()) {
                    let pages: Vec<String> = match line {
                        Some(k) => e.lines.get(k).into_iter().cloned().collect(),
                        None => e.lines.clone(),
                    };
                    let pages: Vec<String> = pages.into_iter().map(|p| p.replace('\r', "")).collect();
                    if !pages.is_empty() {
                        let sb = scroll_box(&sim.game, &front.scrolls, e, pages);
                        sim.message_box = Some(sb);
                    }
                }
            }
            Event::CameraRequest { kind, .. } => sim.last_event = format!("camera: {kind} (trigger cameras not ported yet)"),
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
fn item_parts(sources: &Sources, libs: &Libraries, items: &[String], name: &str) -> Model {
    for pkg in items.iter().map(String::as_str).chain(["POWERUPS"]) {
        let (Some(p), Some(lib)) = (sources.packages.get(pkg), libs.0.get(pkg)) else { continue };
        if let Some(tree) = p.anim.as_ref().and_then(|a| a.trees.iter().find(|t| t.label == name)) {
            let mut out = Vec::new();
            let mut sums: Vec<Vec3> = Vec::new();
            for node in &tree.nodes {
                let parent = usize::try_from(node.parent).ok().and_then(|i| sums.get(i).copied()).unwrap_or(Vec3::ZERO);
                let at = parent + Vec3::from_array(node.initial_position);
                sums.push(at);
                let label: String = (tree.prefix.clone() + &node.label).chars().take(15).collect();
                if let Some(parts) = lib.by_label.get(&label) {
                    out.push((at, parts.clone(), tree_node_flags(node), Some(node.index as usize), None));
                } else if let Some(ps) = tree_node_psys(&p.anim_bytes, tree, node) {
                    out.push((at, Vec::new(), 0, Some(node.index as usize), Some(ps)));
                }
            }
            if !out.is_empty() {
                return out;
            }
        }
        for label in [format!("{name}L1"), name.to_string()] {
            if let Some(parts) = lib.by_label.get(&label) {
                return vec![(Vec3::ZERO, parts.clone(), 0, None, None)];
            }
        }
    }
    Vec::new()
}

/// Spawn a model's parts under `parent`: MB flags 1/2 hide a node, the draw
/// flags select a GsMaterial variant (cached), face flags add CameraFaceLocal.
fn spawn_parts(commands: &mut Commands, parent: Entity, list: Model, materials: &mut Assets<gdl_client::GsMaterial>, cache: &mut PartVariants, pending: &mut PendingTreePsys) {
    commands.entity(parent).despawn_related::<Children>();
    for (offset, parts, flags, node, psys) in list {
        if let Some(ps) = psys {
            // An anchor in the slot's hierarchy; the sprites draw in render
            // space from a root-level emitter that follows it.
            let mut anchor = commands.spawn((Transform::from_translation(offset), Visibility::Inherited));
            if let Some(i) = node {
                anchor.insert(TreeNodeIdx(i));
            }
            let anchor = anchor.id();
            commands.entity(parent).add_child(anchor);
            if let Ok(p) = gdl_core::psys::Psys::new_world(0, &ps.record, &[], Some(ps.dir), true) {
                pending.0.push((anchor, p));
            }
            continue;
        }
        if flags & 3 != 0 {
            continue;
        }
        for (mesh, material) in parts {
            let draw = flags & (0x800000 | 0x4000 | 0x80 | 0x40);
            let material = if draw == 0 {
                material
            } else {
                cache
                    .0
                    .entry((material.id(), draw))
                    .or_insert_with(|| {
                        let m = materials.get(&material).cloned().map(|m| m.with_draw(flags));
                        m.map_or(material.clone(), |m| materials.add(m))
                    })
                    .clone()
            };
            let mut child = commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::from_translation(offset)));
            if let Some(i) = node {
                child.insert(TreeNodeIdx(i));
            }
            if flags & 0xf000000 != 0 {
                child.insert(CameraFaceLocal { mode: flags & 0xf000000 });
            }
            let child = child.id();
            commands.entity(parent).add_child(child);
        }
    }
}

/// Draw-flag material variants of package parts.
#[derive(Resource, Default)]
struct PartVariants(HashMap<(AssetId<gdl_client::GsMaterial>, u32), Handle<gdl_client::GsMaterial>>);

/// CameraFace on a model node under an item/effect slot (the slot's
/// Transform is in original coordinates).
#[derive(Component)]
struct CameraFaceLocal {
    mode: u32,
}

fn present_camera_face_local(sim: Res<Sim>, parents: Query<&Transform, Without<CameraFaceLocal>>, mut q: Query<(&CameraFaceLocal, &ChildOf, &mut Transform)>) {
    let Some(cam) = &sim.game.std_camera else { return };
    let eye = Vec3::from_array(cam.std.v3(gdl_core::game::camera::o::EYE));
    let rows = cam.pb.cam_rows.map(Vec3::from_array);
    for (face, child_of, mut tf) in &mut q {
        let Ok(p) = parents.get(child_of.parent()) else { continue };
        let at = p.transform_point(tf.translation);
        let m = camera_face_rotation(face.mode, at, eye, rows);
        tf.rotation = p.rotation.inverse() * Quat::from_mat3(&m);
    }
}

type SlotQuery<'w, 's> = Query<'w, 's, (Entity, &'static mut Slot, &'static mut Transform, &'static mut Visibility), Without<CharacterRoot>>;

#[allow(clippy::too_many_arguments)]
fn present_world(
    mut commands: Commands,
    sim: Res<Sim>,
    sources: Res<Sources>,
    session: Res<Session>,
    libs: Res<Libraries>,
    frames: Res<EnemyFrames>,
    mut slots: SlotQuery,
    (mut materials, mut variants, mut pending): (ResMut<Assets<gdl_client::GsMaterial>>, ResMut<PartVariants>, ResMut<PendingTreePsys>),
) {
    let g = &sim.game;
    let items = &session.levels[session.current].items;
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
                        let parts = libs.0.get(pkg).and_then(|lib| lib.by_label.get(&l)).cloned().map(|p| vec![(Vec3::ZERO, p, 0, None, None)]);
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
                // 0x800: a Tower crystal still hidden (its tree alpha fades in).
                if !r.active() || r.u16_(0xd8) & 0x8900 != 0 || kind == 4 || kind == 13 || kind == 5 || kind == 7 {
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
                    let parts = libs.0.get(pkg).and_then(|lib| lib.by_label.get(&key)).cloned().map(|p| vec![(Vec3::ZERO, p, 0, None, None)]);
                    (Some(key), parts, tf)
                } else {
                    let name = String::from_utf8_lossy(d.name()).into_owned();
                    let key = format!("{name}#{}", r.u16_(0xd8) & 1);
                    if slot.key.as_deref() == Some(key.as_str()) {
                        (Some(key), None, tf)
                    } else {
                        (Some(key), Some(item_parts(&sources, &libs, items, &name)), tf)
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
                        let key = session.throw.1.clone();
                        let parts = (slot.key.as_deref() != Some(key.as_str())).then(|| missile_parts(&sources, &libs, &session.throw));
                        (Some(key), parts, tf)
                    }
                    // EffectHeaders[h]: a tree of WEAPONS (0), POWERUPS (1) or
                    // critter file n's model (2 + n); posed per frame below.
                    Visual::Header(h) => {
                        let Some(Some(head)) = g.effects.tables.headers.get(h) else {
                            *vis = Visibility::Hidden;
                            continue;
                        };
                        let pkg = match head.source {
                            0 => Some("WEAPONS".to_string()),
                            1 => Some("POWERUPS".to_string()),
                            n => session.levels[session.current].critters.get(n as usize - 2).and_then(|c| c.2.clone()),
                        };
                        let Some(pkg) = pkg else {
                            *vis = Visibility::Hidden;
                            continue;
                        };
                        let key = format!("H{h}:{}", node.start.to_bits());
                        if slot.key.as_deref() != Some(key.as_str()) {
                            commands.entity(entity).insert(EffectTree { pkg: pkg.clone(), tree: head.tree, start: node.start, timing: head.timing });
                        }
                        let parts = (slot.key.as_deref() != Some(key.as_str())).then(|| tree_index_parts(&sources, &libs, &pkg, head.tree));
                        (Some(key), parts, tf)
                    }
                    // FamiliarSpit: the class SFX package's FAMILIAR_SPIT tree
                    // (or WEAPONS' PHOENIX_FBALL for the phoenix).
                    Visual::Familiar { phoenix, .. } => {
                        let (pkg, label) = if phoenix { ("WEAPONS".to_string(), "PHOENIX_FBALL") } else { (session.sfx.clone(), "FAMILIAR_SPIT") };
                        let key = format!("F{pkg}:{label}");
                        let parts = (slot.key.as_deref() != Some(key.as_str())).then(|| tree_parts(&sources, &libs, &pkg, label));
                        (Some(key), parts, tf)
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
                spawn_parts(&mut commands, entity, list, &mut materials, &mut variants, &mut pending);
            } else {
                commands.entity(entity).despawn_related::<Children>();
            }
            slot.key = key;
        }
    }
}

/// A package tree by index (EffectHeaders' tree numbers) at rest.
fn tree_index_parts(sources: &Sources, libs: &Libraries, pkg: &str, tree: usize) -> Model {
    let label = sources.packages.get(pkg).and_then(|p| p.anim.as_ref()).and_then(|a| a.trees.get(tree)).map(|t| t.label.clone());
    label.map_or_else(Vec::new, |l| tree_parts(sources, libs, pkg, &l))
}

/// The node index of a model part within its package tree.
#[derive(Component)]
struct TreeNodeIdx(usize);

/// An effect slot's animated tree: frames advance every timing/900 s from
/// the node's start ATime (StartFXTree).
#[derive(Component)]
struct EffectTree {
    pkg: String,
    tree: usize,
    start: f32,
    timing: i16,
}

/// Pose effect trees' nodes (sequence 0) at the effect's current frame;
/// camera-facing nodes keep their translation and take CameraFaceLocal's
/// rotation afterwards.
fn present_effect_trees(sim: Res<Sim>, sources: Res<Sources>, slots: Query<(&EffectTree, &Children, &Visibility)>, mut parts: Query<(&TreeNodeIdx, &mut Transform)>) {
    let atime = sim.game.globals.atime;
    for (fx, children, vis) in &slots {
        if *vis == Visibility::Hidden {
            continue;
        }
        let Some(pkg) = sources.packages.get(&fx.pkg) else { continue };
        let Some(anim) = pkg.anim.as_ref() else { continue };
        let frame = ((atime - fx.start) * 900.0 / f32::from(fx.timing.max(1))).max(0.0);
        let Ok(pose) = gdl_client::tree_pose(anim, fx.tree, &pkg.anim_bytes, 0, frame, true) else { continue };
        for child in children.iter() {
            if let Ok((idx, mut tf)) = parts.get_mut(child)
                && let Some(m) = pose.get(idx.0)
            {
                *tf = Transform::from_matrix(*m);
            }
        }
    }
}

/// A package tree's nodes at their initial positions (presentation).
fn tree_parts(sources: &Sources, libs: &Libraries, pkg: &str, label: &str) -> Model {
    let (Some(p), Some(lib)) = (sources.packages.get(pkg), libs.0.get(pkg)) else { return Vec::new() };
    let Some(tree) = p.anim.as_ref().and_then(|a| a.trees.iter().find(|t| t.label == label)) else { return Vec::new() };
    let mut out = Vec::new();
    for node in &tree.nodes {
        let l: String = (tree.prefix.clone() + &node.label).chars().take(15).collect();
        if let Some(parts) = lib.by_label.get(&l) {
            out.push((Vec3::from_array(node.initial_position), parts.clone(), tree_node_flags(node), Some(node.index as usize), None));
        } else if let Some(ps) = tree_node_psys(&p.anim_bytes, tree, node) {
            out.push((Vec3::from_array(node.initial_position), Vec::new(), 0, Some(node.index as usize), Some(ps)));
        }
    }
    out
}

/// The character's thrown missile tree (InitPlayerMissiles: the
/// "<PlayerMissileDesc>_THROW<n>" tree of the SFX or model package) at rest
/// pose (presentation; the tree is not animated).
fn missile_parts(sources: &Sources, libs: &Libraries, throw: &(String, String)) -> Model {
    let (Some(p), Some(lib)) = (sources.packages.get(&throw.0), libs.0.get(&throw.0)) else { return Vec::new() };
    let Some(tree) = p.anim.as_ref().and_then(|a| a.trees.iter().find(|t| t.label == throw.1)) else { return Vec::new() };
    let mut out = Vec::new();
    for node in &tree.nodes {
        let label: String = (tree.prefix.clone() + &node.label).chars().take(15).collect();
        if let Some(parts) = lib.by_label.get(&label) {
            out.push((Vec3::from_array(node.initial_position), parts.clone(), tree_node_flags(node), Some(node.index as usize), None));
        } else if let Some(ps) = tree_node_psys(&p.anim_bytes, tree, node) {
            out.push((Vec3::from_array(node.initial_position), Vec::new(), 0, Some(node.index as usize), Some(ps)));
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
                materials.add(StandardMaterial { base_color: Color::linear_rgb(1.0, 0.9, 0.3), unlit: true, ..default() }),
                materials.add(StandardMaterial {
                    base_color: Color::linear_rgba(0.8, 0.5, 1.0, 0.25),
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

type BodyQuery<'w, 's> = Query<'w, 's, (&'static CharacterRoot, &'static mut Transform, &'static mut Visibility), (Without<RigNode>, Without<FollowCamera>, Without<Slot>)>;
type NodeQuery<'w, 's> = Query<'w, 's, (&'static RigNode, &'static mut Transform), (Without<CharacterRoot>, Without<FollowCamera>, Without<Slot>)>;
type CameraQuery<'w, 's> = Query<'w, 's, (&'static mut Transform, &'static mut Projection), (With<FollowCamera>, Without<CharacterRoot>, Without<RigNode>, Without<Slot>)>;

#[allow(clippy::too_many_arguments)]
fn present_player(
    sim: Res<Sim>,
    session: Res<Session>,
    sources: Res<Sources>,
    _presentation: Res<Presentation>,
    mut body: BodyQuery,
    mut nodes: NodeQuery,
    mut camera: CameraQuery,
    mut hud: Query<&mut Text2d, (With<Hud>, Without<Banner>)>,
    mut banner: Query<&mut Text2d, (With<Banner>, Without<Hud>)>,
    ui: Res<Ui>,
    mut bars: Query<&mut Visibility, (With<LetterBox>, Without<CharacterRoot>)>,
) {
    // ScrollMessageBox runs (modal) before the trigger camera it precedes
    // starts (do_players: welcome box, then CrystalCamActivate).
    let barred = sim.game.trigger_view_active() && sim.message_box.is_none();
    for mut v in &mut bars {
        *v = if barred { Visibility::Inherited } else { Visibility::Hidden };
    }
    // Players in play: slot 0 and joined slots; AnimInfo +0xa2 sequence and
    // +0xac frame select the pose (presentation sampling of the class clips).
    let active: [bool; 4] = [true, sim.coop[0], sim.coop[1], sim.coop[2]];
    let poses: Vec<_> = (0..4)
        .map(|k| {
            let r = &sim.game.players[k];
            active[k].then(|| gdl_client::rig_pose(&sources.clips, &sources.clip_bytes, r.u16_(0xa2) as usize, r.f32_(0xac)).ok()).flatten()
        })
        .collect();
    for (root, mut t, mut vis) in &mut body {
        if !active.get(root.0).copied().unwrap_or(false) || matches!(sim.game.players[root.0].i32_(0xfc), 0 | 5 | 0xb) {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Inherited;
        let r = &sim.game.players[root.0];
        let (m0, m1, m2, p) = (r.v3(0x20), r.v3(0x30), r.v3(0x40), r.v3(0x50));
        *t = Transform::from_matrix(Mat4::from_cols_array(&[
            m0[0], m0[1], m0[2], 0.0, m1[0], m1[1], m1[2], 0.0, m2[0], m2[1], m2[2], 0.0, p[0], p[1], p[2], 1.0,
        ]));
    }
    for (node, mut t) in &mut nodes {
        if let Some(p) = poses.get(node.1).and_then(|p| p.as_ref()).and_then(|pose| pose.get(node.0)) {
            *t = Transform::from_matrix(*p);
        }
    }
    // The StdCam window camera (research/camera.md): eye and matrix rows.
    if let (Some(cam), Ok((mut t, mut projection))) = (&sim.game.std_camera, camera.single_mut()) {
        // pbProjCalc's +0x90 projection: x scale 1/tan_h, y scale −1/tan_v
        // (reference: 1.73205 / −2.88675, a 554.27-pixel focal length over the
        // 640×384 viewport; the 60° window angle is horizontal).
        let (sx, sy) = (cam.pb.proj[0][0], cam.pb.proj[1][1]);
        if 0.0 < sx.abs() && 0.0 < sy.abs()
            && let Projection::Custom(custom) = &mut *projection
            && let Some(p) = custom.get_mut::<Ps2Projection>()
        {
            p.tan_h = 1.0 / sx.abs();
            p.tan_v = 1.0 / sy.abs();
        }
        let eye = Vec3::from_array(cam.std.v3(gdl_core::game::camera::o::EYE));
        let [_, up, forward] = cam.pb.cam_rows.map(Vec3::from_array);
        *t = Transform::from_translation(eye * MIRROR).looking_to(forward * MIRROR, up * MIRROR);
        // DEBUG (render comparisons, tools/python/reference_camera.py):
        // GDL_DEBUG_CAMERA=<json> pins the window camera to a reference's.
        if let Some(v) = debug_camera() {
            *t = Transform::from_translation(v[0] * MIRROR).looking_to(v[2] * MIRROR, v[1] * MIRROR);
        }
    }
    let g = &sim.game;
    let p = &g.players[0];
    let health = p.f32_(0x1a10);
    if let Ok(mut text) = banner.single_mut() {
        text.0 = if ui.paused {
            String::new()
        } else if let Some(f) = &session.finished {
            f.clone()
        } else if let Some((_, b)) = &session.banner {
            b.clone()
        } else if let Some(e) = &sim.error {
            format!("SIMULATION STOPPED\n{e}")
        } else if health <= 0.0 {
            "YOU DIED".to_string()
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
            "{} {}   level {}   exp {}   slot {}\n\
             HEALTH {:>5.0}   GOLD {:>5}   KEYS {}   POTIONS {}\n\
             {} {}   kills {}   enemies {}   generators {}\n\
             {}\n\
             P1 move: arrows/WASD  throw: Space/K  potion: E/L | P2 (join: R) TFGH, R, Y | pads: D-pad, South, East | Esc/P/Start: menu  F1",
            gdl_client::menu::COLOUR_NAMES.get(session.colour).copied().unwrap_or("?"),
            gdl_client::menu::CLASS_NAMES.get(session.class as usize).copied().unwrap_or("?"),
            p.i32_(0x2ac0),
            p.i32_(0x1a1c),
            session.slot,
            health,
            p.i32_(0x1a20),
            p.i32_(0x1a14),
            p.i32_(0x1a18),
            session.title,
            session.levels[session.current].name,
            sim.kills,
            live,
            generators,
            sim.last_event,
        );
        if !ui.details {
            t.clear();
        }
        if let Some(boss) = usize::try_from(g.boss_crit_inst - 1).ok().and_then(|k| g.critters.get(k)).and_then(Option::as_ref) {
            // Boss health (the original's health meter blits are presentation).
            let max = g.critter_type_rec_pub(boss.ty).map_or(0.0, |v| v);
            t.push_str(&format!("\nBOSS {:>6.0} / {:.0}", boss.rec.f32_(0x4f4).max(0.0), max));
        }
        if ui.details {
            t.push_str(&format!(
                "\n\nINTERIM PLAYTEST (not the M12 slice). player updates {}\n\
                 Native: item/enemy/generator/missile/damage/pickup/magic/exp/exit/sound-cue systems, movement + collision\n\
                 Interim/presentation: session globals from the capture, player clip clock, camera, unlit materials, menus,\n\
                 enemy frame objects, item rest poses, approximate audio mixing, magic without animation, melee holds the player still, boss pose without blending\n\
                 Enter after death: interim continue   F9: debug skip to next level{}",
                sim.updates,
                g.log.unsupported.iter().find(|(_, n)| 0 < **n).map_or(String::new(), |(k, _)| format!("\nunported path hit: {k}")),
            ));
        }
        text.0 = t;
    }
}

/// MBOX texture search order for the panels (STATIC, the character package,
/// then the level's packages: a presentation choice — the panel labels are
/// unique across them) and the FONTS/*.FNT files of font_desc, then the
/// panels' reset/level-start setup.
fn init_hud(game: &mut Game, sources: &Sources, session: &Session, catalog: &gdl_client::catalog::Catalog, english: &gdl_formats::textrom::TextRom, sets: &mut HudSets) {
    let mut keys = vec!["STATIC".to_string(), String::new()];
    keys.extend(level_packages(&session.levels[session.current]).into_iter().filter(|k| k != "STATIC"));
    keys.retain(|k| k.is_empty() || sources.packages.contains_key(k));
    let tex: Vec<_> = keys
        .iter()
        .map(|k| {
            let pkg = if k.is_empty() { &sources.character } else { &sources.packages[k] };
            gdl_client::hud_texture_set(k, &pkg.objects)
        })
        .collect();
    let fonts = load_fonts(catalog, game.hud.tables.as_ref().map(|t| t.font_desc.clone()).unwrap_or_default());
    game.hud_init(tex, fonts, Some(english.clone()));
    if !game.hud.missing.is_empty() {
        eprintln!("panel textures not found: {:?}", game.hud.missing);
    }
    sets.keys = keys;
    sets.images.clear();
}

/// ScrollMessageBox pages of a SCROLL_E entry: its FONT name resolved
/// against font_desc (StringInitSub), scale and space scale.
fn scroll_box(game: &Game, rom: &gdl_formats::textrom::TextRom, e: &gdl_formats::textrom::TextEntry, pages: Vec<String>) -> gdl_core::game::hud::ScrollBox {
    let name = rom.fonts.get(e.font as usize).cloned().unwrap_or_default();
    let font = game.hud.tables.as_ref().and_then(|t| t.font_desc.iter().position(|n| *n == name)).unwrap_or(0);
    gdl_core::game::hud::ScrollBox::new(pages, font, e.scale, e.space)
}

/// FONTS/<font_desc>.FNT (font_init_desc).
fn load_fonts(catalog: &gdl_client::catalog::Catalog, names: Vec<String>) -> Vec<Option<gdl_formats::fnt::Font>> {
    names
        .iter()
        .map(|n| match catalog.read(&format!("FONTS/{n}.FNT")).map_err(|e| e.to_string()).and_then(|b| gdl_formats::fnt::Font::parse(&b).map_err(|e| e.to_string())) {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!("font {n}: {e}");
                None
            }
        })
        .collect()
}

/// The player-select columns for the front end's character pages (player
/// 1's column follows the page; the others are empty frames).
fn select_columns(front: &Front, screen: Screen) -> Option<[gdl_core::game::select::Column; 4]> {
    use gdl_client::menu::Page;
    use gdl_core::game::select::Column;
    if screen != Screen::Front {
        return None;
    }
    let first = match &front.menu.page {
        Page::Select { at } => Column::Menu { title: Vec::new(), entries: front.menu.select_entries(), selected: *at },
        Page::Class { at } => Column::Class { class: *at, colour: 0 },
        Page::Colour { class, at } => Column::Class { class: *class as usize, colour: *at },
        Page::Name { class, colour, name } => Column::Name { class: *class as usize, colour: *colour as usize, name: name.clone() },
        _ => return None,
    };
    Some([first, Column::Empty, Column::Empty, Column::Empty])
}

/// The page drawn with an original menu record (front end or pause).
fn menu_view(front: &Front, screen: Screen, ui: &Ui) -> Option<(&'static str, usize)> {
    if !(screen == Screen::Front || ui.paused) {
        return None;
    }
    gdl_client::menu::page_record(&front.menu.page).filter(|(k, _)| front.menus.contains_key(k))
}

type HudInputs<'w> = (Option<Res<'w, Sim>>, Option<Res<'w, Sources>>, ResMut<'w, Front>, Res<'w, Screen>, Res<'w, Ui>, Res<'w, Time>);

type HudSpriteQuery<'w, 's> = Query<'w, 's, (&'static HudSprite, &'static mut Transform, &'static mut Visibility, &'static MeshMaterial2d<gdl_client::BlitMaterial>)>;

/// The frame's panel blits (Game::hud draw list) as pooled sprites: sorted
/// far to near by blit z (ZTST GEQUAL; equal z keeps draw order).
#[allow(clippy::too_many_arguments)]
fn present_hud(
    mut commands: Commands,
    (sim, sources, mut front, screen, ui, time): HudInputs,
    mut sets: ResMut<HudSets>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<gdl_client::BlitMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut pool: HudSpriteQuery,
    mut quad: Local<Option<Handle<Mesh>>>,
) {
    let playing = *screen == Screen::Playing;
    let mut list = Vec::new();
    if let Some(sim) = sim.as_ref().filter(|_| playing) {
        list = sim.game.hud.draw_list();
        if let Some(sb) = &sim.message_box {
            list.extend(sim.game.hud.scroll_blits(sb));
        }
    }
    // The menu page: show_optmenu with the page's vblank count.
    let view = menu_view(&front, *screen, &ui);
    if front.menu_clock.0.map(|v| v.0) != view.map(|v| v.0) {
        front.menu_clock.1 = 0.0;
    }
    front.menu_clock.0 = view;
    front.menu_clock.1 += time.delta_secs() * 59.94;
    // Front-end blits use the front Hud's sets (STATIC, TITLE) as 100 + i.
    let mut front_list = Vec::new();
    if let (Some((key, at)), Some(style)) = (view, front.opt_style.as_ref()) {
        let sliders = if key == "audio_options" { vec![(0, front.menu.music), (1, front.menu.sfx)] } else { Vec::new() };
        let state = gdl_core::game::optmenu::MenuState { selected: at, frames: front.menu_clock.1 as i32, finishing: 0, player: Some(0), sliders };
        front_list.extend(front.ui_hud.optmenu_blits(style, &front.menus[key], &state));
    }
    if let (Some(cols), Some(style), Some(def)) = (select_columns(&front, *screen), front.opt_style.as_ref(), front.menus.get("sel_def_menu")) {
        front_list.extend(front.ui_hud.select_blits(style, def, &cols, front.menu_clock.1 as i32));
    }
    if *screen == Screen::Front {
        front.title_clock += time.delta_secs() * 59.94;
        let page = &front.menu.page;
        let glow = matches!(page, gdl_client::menu::Page::PressStart | gdl_client::menu::Page::Title { .. });
        let press = matches!(page, gdl_client::menu::Page::PressStart);
        front_list.extend(front.ui_hud.title_blits(front.title_clock as i32, glow, press));
    } else {
        front.title_clock = 0.0;
    }
    for mut b in front_list {
        if let Some(t) = b.tex.as_mut() {
            t.set += 100;
        }
        if let Some((t, _)) = b.mask.as_mut() {
            t.set += 100;
        }
        list.push(b);
    }
    list.retain(|b| b.tex.is_some() && 0.0 < b.w && 0.0 < b.h && 1 < b.rgba[3]);
    list.sort_by(|a, b| b.z.total_cmp(&a.z));
    let quad = quad.get_or_insert_with(|| meshes.add(Rectangle::new(1.0, 1.0))).clone();
    let mut slots: Vec<_> = pool.iter_mut().collect();
    slots.sort_by_key(|s| s.0.0);
    for (i, b) in list.iter().enumerate() {
        let mut resolve = |t: gdl_core::game::hud::TexRef| -> Option<Handle<Image>> {
            let key = (t.set, t.index);
            if !sets.images.contains_key(&key) {
                let key_name = sets.keys.get(t.set as usize).cloned().unwrap_or_else(|| "STATIC".into());
            let pkg = if 100 <= t.set {
                [&front.static_pkg, &front.title_pkg, &front.select_pkg].get(t.set as usize - 100).copied()
            } else if key_name == "STATIC" {
                Some(&front.static_pkg)
            } else if key_name.is_empty() {
                sources.as_ref().map(|s| &s.character)
            } else {
                sources.as_ref().and_then(|s| s.packages.get(&key_name))
            };
            let img = pkg.and_then(|pkg| gdl_client::hud_image(&pkg.objects, &pkg.table, &pkg.textures, t.index as usize)).map(|im| images.add(im));
                sets.images.insert(key, img);
            }
            sets.images[&key].clone()
        };
        let Some(image) = resolve(b.tex.unwrap()) else { continue };
        let (mask, mask_rect) = match b.mask {
            Some((t, r)) => (resolve(t), Vec4::from_array(r)),
            None => (None, Vec4::ZERO),
        };
        let params = gdl_client::BlitParams { rgba: Vec4::from_array(b.rgba.map(|c| f32::from(c) / 128.0)), crop: Vec4::from_array(b.crop), mask_rect };
        let transform = Transform::from_xyz(b.x + b.w * 0.5 - 256.0, 192.0 - b.y - b.h * 0.5, i as f32 * 0.001).with_scale(Vec3::new(b.w, b.h, 1.0));
        if let Some((_, tr, vis, mat)) = slots.get_mut(i) {
            **tr = transform;
            **vis = Visibility::Visible;
            if let Some(m) = mats.get(&mat.0)
                && (m.params != params || m.texture.as_ref() != Some(&image) || m.mask != mask)
                && let Some(m) = mats.get_mut(&mat.0)
            {
                m.params = params;
                m.texture = Some(image);
                m.mask = mask;
            }
        } else {
            let mat = mats.add(gdl_client::BlitMaterial { params, texture: Some(image), mask });
            commands.spawn((Mesh2d(quad.clone()), MeshMaterial2d(mat), transform, Visibility::Visible, bevy::camera::visibility::RenderLayers::layer(HUD_LAYER), HudSprite(i)));
        }
    }
    for (_, _, vis, _) in slots.iter_mut().skip(list.len()) {
        **vis = Visibility::Hidden;
    }
}

/// Verified file paths of one package label prefix as a package config entry.
fn package_entry(cat: &gdl_client::catalog::Catalog, prefix: &str, anim: bool) -> Result<Value, Box<dyn Error>> {
    let mut v = serde_json::Map::new();
    let mut files = vec![("objects", "OBJECTS.PS2"), ("texdef", "TEXDEF.PS2"), ("textures", "TEXTURES.PS2")];
    if anim {
        files.push(("anim", "ANIM.PS2"));
    }
    for (k, f) in files {
        let label = format!("{prefix}/{f}");
        cat.read(&label)?;
        v.insert(k.into(), Value::String(cat.path(&label)?.to_string_lossy().into_owned()));
    }
    Ok(Value::Object(v))
}

/// A character's packages (load_player_model_sub / load_player_model):
/// players/<cls>/<colour><tier>0 (model and rig), players/<cls>/anim (clips),
/// players/<cls>/sfx<colour>; the thrown-missile tree (InitPlayerMissiles).
struct CharacterAssets {
    model_name: String,
    model: Value,
    sfx_name: String,
    sfx: Value,
    clips_label: String,
    throw: (String, String),
}

const COLOUR_TAGS: [&str; 4] = ["YEL", "BLU", "RED", "GRE"];

fn character_assets(front: &Front, class: u32, colour: usize, tier: i32) -> Result<CharacterAssets, Box<dyn Error>> {
    let cls = gdl_core::player_class::CLASS_TAGS.get(class as usize).ok_or("class")?.to_uppercase();
    let col = COLOUR_TAGS.get(colour).ok_or("colour")?;
    let model_name = format!("PLAYERS/{cls}/{col}{tier}0");
    let sfx_name = format!("PLAYERS/{cls}/SFX{col}");
    let clips_label = format!("PLAYERS/{cls}/ANIM/ANIM.PS2");
    front.catalog.read(&clips_label)?;
    let desc = front.missile_desc.get(class as usize).ok_or("missile desc")?;
    let name = String::from_utf8_lossy(&desc[..desc.iter().position(|&c| c == 0).unwrap_or(3)]).into_owned();
    let ch = desc.get(4 + tier as usize).copied().unwrap_or(b'1');
    let throw = if ch == b'0' { (model_name.clone(), format!("{name}_THROW0")) } else { (sfx_name.clone(), format!("{name}_THROW{}", ch as char)) };
    Ok(CharacterAssets {
        model: package_entry(&front.catalog, &model_name, true)?,
        sfx: package_entry(&front.catalog, &sfx_name, true)?,
        model_name,
        sfx_name,
        clips_label,
        throw,
    })
}

/// The rig/clip pair of a character: the model ANIM tree sharing the most
/// node labels with the class clip tree (the model package also holds the
/// missile tree; some tiers omit nodes, e.g. VAL BLU30 lacks FLAP04/FLAP08).
/// Model package, rig, class clips and the clip file bytes.
type Rig = (Package, Animations, Animations, Vec<u8>);

fn character_rig(front: &Front, ch: &CharacterAssets) -> Result<Rig, Box<dyn Error>> {
    let character = load_package(&ch.model)?;
    let clip_bytes = front.catalog.read(&ch.clips_label)?;
    let clips = Animations::parse(&clip_bytes)?;
    let mut rig = Animations::parse(&gdl_client::bounded_read(Path::new(ch.model["anim"].as_str().ok_or("rig path")?))?)?;
    let clip_labels: std::collections::HashSet<&str> = clips.trees.first().ok_or("class clips without a tree")?.nodes.iter().map(|n| n.label.as_str()).collect();
    let overlap = |t: &gdl_formats::animation::Tree| t.nodes.iter().filter(|n| clip_labels.contains(n.label.as_str())).count();
    let best = (0..rig.trees.len()).max_by_key(|&k| overlap(&rig.trees[k])).ok_or("model without trees")?;
    if overlap(&rig.trees[best]) * 2 < clip_labels.len() {
        return Err(format!("{}: no rig tree matching the class clips", ch.model_name).into());
    }
    let tree = rig.trees.swap_remove(best);
    rig.trees = vec![tree];
    Ok((character, rig, clips, clip_bytes))
}

fn refresh_saves(front: &mut Front) {
    let mut imported = false;
    front.menu.saves = front
        .store
        .list()
        .into_iter()
        .map(|l| {
            if let (Ok(x), Some((_, sha))) = (front.store.load(l.slot), front.capture.as_ref())
                && x.save.source == format!("imported:capture:{sha}")
            {
                imported = true;
            }
            let label = match &l.summary {
                Some(s) => format!(
                    "{:<8} {} {}  level {}{}",
                    s.name,
                    gdl_client::menu::COLOUR_NAMES.get(s.colour as usize).copied().unwrap_or("?"),
                    gdl_client::menu::CLASS_NAMES.get(s.class as usize).copied().unwrap_or("SECRET CHARACTER"),
                    s.level,
                    l.problem.as_ref().map_or(String::new(), |_| "  (recovered from backup)".into())
                ),
                None => format!("slot {} unreadable: {}", l.slot, l.problem.unwrap_or_default()),
            };
            Entry { slot: l.slot, label }
        })
        .collect();
    front.menu.import = match &front.capture {
        Some((region, _)) if !imported => profile::summary(region).map(|s| format!("{} (PS2 savestate capture)", s.name)),
        _ => None,
    };
}

/// Load save `slot` and enter the Tower with that character.
fn start_game(world: &mut EcsWorld, slot: u32) -> Result<(), Box<dyn Error>> {
    let mut hud_sets = HudSets::default();
    let (session, sources, game, recovered) = {
        let front = world.resource::<Front>();
        let loaded = front.store.load(slot)?;
        let region = loaded.save.region()?;
        let (record, controls) = profile::load_character(0, &region, &front.pdats, &front.tables).map_err(|e| e.to_string())?;
        let class = record.u32_(0xc);
        let colour = record.u32_(4) as usize;
        if 8 <= class {
            return Err("this character is one of the secret characters, which are not supported yet".into());
        }
        let tier = (record.i32_(0x2ac0) / 10).clamp(0, 9);
        let ch = character_assets(front, class, colour, tier)?;
        let (character, rig, clips, clip_bytes) = character_rig(front, &ch)?;
        let b = &front.base;
        // DEBUG (automated render comparisons): GDL_DEBUG_LEVEL=<name> starts
        // in that level instead of the Tower. Not a game feature.
        let debug_level = std::env::var("GDL_DEBUG_LEVEL").ok();
        let idx = debug_level
            .and_then(|n| b.levels.iter().position(|l| l.name == n))
            .or_else(|| b.levels.iter().position(|l| l.world == 0xd))
            .unwrap_or(0);
        let first = &b.levels[idx];
        let mut packages = HashMap::new();
        let first_labels = load_level_packages(&mut packages, &b.package_cfg, first)?;
        let sfx_pkg = load_package(&ch.sfx)?;
        let model_pkg = load_package(&ch.model)?;
        let character_labels: Vec<String> = sfx_pkg.objects.definitions.iter().chain(character.objects.definitions.iter()).map(|d| d.label.clone()).collect();
        packages.insert(ch.sfx_name.clone(), sfx_pkg);
        packages.insert(ch.model_name.clone(), model_pkg);
        let session = Session {
            elf_bytes: front.elf_bytes.clone(),
            package_cfg: b.package_cfg.clone(),
            start: b.start.clone(),
            world: World::parse(&gdl_client::bounded_read(&first.worlds)?)?,
            level: load_package(&first.package)?,
            levels: b.levels.clone(),
            world_number: first.world,
            title: first.title.clone(),
            last_world: 0,
            current: idx,
            action_map: front.tables.action_map(&clips),
            clip_timings: setup::clips_of(&clips, 0),
            monster_paths: b.monster_paths.clone(),
            anim_paths: b.anim_paths.clone(),
            audio_rom: b.audio_rom.clone(),
            object_labels: first_labels.into_iter().chain(b.shared_labels.iter().cloned()).chain(character_labels).collect(),
            banner: None,
            next: None,
            finished: None,
            slot,
            controls,
            class_data: {
                let mut c = ClassData::from_pdat(front.pdats.get(class).ok_or("PDAT")?)?;
                let (sfxx, damg) = front.class_fx.get(class as usize).cloned().unwrap_or_default();
                c.sfxx = sfxx;
                c.damg = damg;
                c
            },
            pdats: front.pdats.clone(),
            heart_rate: front.heart_rate,
            sfx: ch.sfx_name.clone(),
            throw: ch.throw.clone(),
            tier,
            class,
            colour,
            carried: [0, -1, -1, -1, -1],
        };
        let mut game = build_game(&session, idx, &record)?;
        let sources = Sources { character, clips, clip_bytes, rig, packages, monsters: b.monsters.clone() };
        init_hud(&mut game, &sources, &session, &front.catalog, &front.english, &mut hud_sets);
        (session, sources, game, loaded.recovered)
    };
    let attention = Vec3::from_array(game.players[0].v3(0x810));
    let stage = game.sounds.stage.as_ref().map(|s| s.stream_name(0, 0));
    world.resource_mut::<Presentation>().attention = attention;
    world.insert_resource(sources);
    world.insert_resource(session);
    world.insert_resource(hud_sets);
    world.insert_resource(Sim { game, error: None, updates: 0, exp: 0, kills: 0, sparks: Vec::new(), last_event: String::new(), coop: [false; 3], message_box: None, hint_menu: false, scroll_clock: 0.0 });
    world.run_system_once(setup).map_err(|e| format!("{e:?}"))?;
    if let Some(mut a) = world.get_non_send_resource_mut::<gdl_client::audio::AudioOut>() {
        a.stop_all();
        if let Some(st) = stage {
            a.music(&st);
        }
    }
    if let Some((path, why)) = recovered {
        world.resource_mut::<Sim>().last_event = format!("save recovered from {} ({why})", path.display());
    }
    *world.resource_mut::<Ui>() = Ui { started: true, paused: false, details: false };
    *world.resource_mut::<Screen>() = Screen::Playing;
    Ok(())
}

/// Quit Game (init_over: every player inactivated) → the title.
fn end_game(world: &mut EcsWorld) {
    let roots: Vec<Entity> = world.query_filtered::<Entity, With<MirrorRoot>>().iter(world).collect();
    for r in roots {
        world.despawn(r);
    }
    world.remove_resource::<Sim>();
    world.remove_resource::<Session>();
    world.remove_resource::<Sources>();
    if let Some(mut a) = world.get_non_send_resource_mut::<gdl_client::audio::AudioOut>() {
        a.stop_all();
    }
    *world.resource_mut::<Ui>() = Ui::default();
    *world.resource_mut::<Screen>() = Screen::Front;
    let mut front = world.resource_mut::<Front>();
    front.menu.page = gdl_client::menu::Page::Title { at: 0 };
    refresh_saves(&mut front);
}

/// Menu keys this frame (keyboard and every gamepad), and the pause toggle.
fn menu_keys(world: &mut EcsWorld, naming: bool) -> (Vec<Key>, bool) {
    let mut out = Vec::new();
    let mut toggle = false;
    let keys = world.resource::<ButtonInput<KeyCode>>();
    let pressed = |k: KeyCode| keys.just_pressed(k);
    if naming {
        for (k, c) in [
            (KeyCode::KeyA, 'A'), (KeyCode::KeyB, 'B'), (KeyCode::KeyC, 'C'), (KeyCode::KeyD, 'D'), (KeyCode::KeyE, 'E'), (KeyCode::KeyF, 'F'), (KeyCode::KeyG, 'G'),
            (KeyCode::KeyH, 'H'), (KeyCode::KeyI, 'I'), (KeyCode::KeyJ, 'J'), (KeyCode::KeyK, 'K'), (KeyCode::KeyL, 'L'), (KeyCode::KeyM, 'M'), (KeyCode::KeyN, 'N'),
            (KeyCode::KeyO, 'O'), (KeyCode::KeyP, 'P'), (KeyCode::KeyQ, 'Q'), (KeyCode::KeyR, 'R'), (KeyCode::KeyS, 'S'), (KeyCode::KeyT, 'T'), (KeyCode::KeyU, 'U'),
            (KeyCode::KeyV, 'V'), (KeyCode::KeyW, 'W'), (KeyCode::KeyX, 'X'), (KeyCode::KeyY, 'Y'), (KeyCode::KeyZ, 'Z'), (KeyCode::Digit0, '0'), (KeyCode::Digit1, '1'),
            (KeyCode::Digit2, '2'), (KeyCode::Digit3, '3'), (KeyCode::Digit4, '4'), (KeyCode::Digit5, '5'), (KeyCode::Digit6, '6'), (KeyCode::Digit7, '7'),
            (KeyCode::Digit8, '8'), (KeyCode::Digit9, '9'),
        ] {
            if pressed(k) {
                out.push(Key::Char(c));
            }
        }
        if pressed(KeyCode::Backspace) {
            out.push(Key::Erase);
        }
        if pressed(KeyCode::Enter) {
            out.push(Key::Accept);
        }
        if pressed(KeyCode::Escape) {
            out.push(Key::Back);
        }
    } else {
        for (ks, key) in [
            (&[KeyCode::ArrowUp, KeyCode::KeyW][..], Key::Up),
            (&[KeyCode::ArrowDown, KeyCode::KeyS][..], Key::Down),
            (&[KeyCode::ArrowLeft, KeyCode::KeyA][..], Key::Left),
            (&[KeyCode::ArrowRight, KeyCode::KeyD][..], Key::Right),
            (&[KeyCode::Enter, KeyCode::Space][..], Key::Accept),
            (&[KeyCode::Escape, KeyCode::Backspace][..], Key::Back),
        ] {
            if ks.iter().any(|&k| pressed(k)) {
                out.push(key);
            }
        }
        toggle = pressed(KeyCode::Escape) || pressed(KeyCode::KeyP);
    }
    let mut pads = world.query::<&Gamepad>();
    for g in pads.iter(world) {
        for (b, key) in [
            (GamepadButton::DPadUp, Key::Up),
            (GamepadButton::DPadDown, Key::Down),
            (GamepadButton::DPadLeft, Key::Left),
            (GamepadButton::DPadRight, Key::Right),
            (GamepadButton::South, Key::Accept),
            (GamepadButton::East, Key::Back),
        ] {
            if g.just_pressed(b) {
                out.push(key);
            }
        }
        toggle |= g.just_pressed(GamepadButton::Start);
    }
    (out, toggle)
}

/// Shop lines: entry 0 leaves; others show the item (its SHP_ model name),
/// price and "-" when make_skiplist excludes it.
fn refresh_shop(world: &mut EcsWorld) -> Result<(), Box<dyn Error>> {
    let items = world.resource::<Front>().shop.clone();
    let (skip, gold) = {
        let sim = world.resource::<Sim>();
        (sim.game.shop_skiplist(0, &items).map_err(|e| e.0)?, sim.game.players[0].i32_(0x1a20))
    };
    let lines = items
        .iter()
        .enumerate()
        .map(|(k, it)| {
            if k == 0 {
                "Done".to_string()
            } else {
                let name = it.name.trim_start_matches("SHP_").trim_start_matches("shp_").replace('_', " ").to_uppercase();
                format!("{name:<20} {:>5} {}", it.price, if skip[k] { "-" } else { " " })
            }
        })
        .collect();
    let mut front = world.resource_mut::<Front>();
    front.menu.shop = lines;
    front.menu.gold = gold;
    Ok(())
}

/// Inventory page (NATIVE summary): level/experience, health, attributes,
/// gold, keys, potions, runestones (+0x1a26 bits), shards (+0x1a24 bits),
/// timed powerups (+0x144 slots) and completed levels per realm.
fn inventory_text(r: &Record) -> String {
    let level = r.i32_(0x2ac0);
    let next = gdl_core::player_class::level_to_exp(level + 1);
    let powerups = (0..8).filter(|k| 0.0 != r.f32_(0x144 + 16 * k)).count();
    // worlddatas 0x23deb0 names, by CurWorld.
    let realms = ["", "castle", "mount", "desert", "forest", "temple", "hell", "town", "battle", "ice", "dream", "sky"];
    let class = r.u32_(0xc) as usize;
    let mut waves = Vec::new();
    for (w, name) in realms.iter().enumerate().skip(1) {
        let bits = r.u8_(0x182c + 0xe * class + w);
        if bits != 0 {
            waves.push(format!("{name} {}", bits.count_ones()));
        }
    }
    format!(
        "level {level}   experience {} (next level at {next})\n\
         health {:.0} / {:.0}\n\
         strength {:.0}  armor {:.0}  magic {:.0}  speed {:.0}\n\
         gold {}   keys {}   potions {}\n\
         runestones {}   shards {}   timed powerups {powerups}\n\
         levels completed: {}",
        r.i32_(0x1a1c),
        r.f32_(0x1a10),
        gdl_core::player_class::max_health(level),
        r.f32_(0x108),
        r.f32_(0x10c),
        r.f32_(0x110),
        r.f32_(0x114),
        r.i32_(0x1a20),
        r.i32_(0x1a14),
        r.i32_(0x1a18),
        r.u16_(0x1a26).count_ones(),
        r.u16_(0x1a24).count_ones(),
        if waves.is_empty() { "none".to_string() } else { waves.join(", ") },
    )
}

fn act(world: &mut EcsWorld, action: Action) {
    let result: Result<(), Box<dyn Error>> = (|| {
        match action {
            Action::Play { slot } => start_game(world, slot)?,
            Action::Create { class, colour, name } => {
                let slot = {
                    let front = world.resource::<Front>();
                    let r = profile::new_character(0, &name, class, colour, &front.pdats, &front.tables).map_err(|e| e.to_string())?;
                    let region = profile::save_region(&r, &[0, 2, 1, 1]);
                    let slot = front.store.free_slot()?;
                    front.store.store(slot, "new", &region)?;
                    slot
                };
                start_game(world, slot)?;
            }
            Action::Import => {
                let mut front = world.resource_mut::<Front>();
                let (region, sha) = front.capture.clone().ok_or("no capture to import")?;
                let slot = front.store.free_slot()?;
                front.store.store(slot, &format!("imported:capture:{sha}"), &region)?;
                front.menu.status = format!("imported into slot {slot} (the capture file is unchanged)");
                refresh_saves(&mut front);
            }
            Action::Resume => world.resource_mut::<Ui>().paused = false,
            Action::Save => {
                let (slot, region) = {
                    let session = world.resource::<Session>();
                    let mut r = world.resource::<Sim>().game.players[0].clone();
                    // init_player_select → player_store_in_save, then the file write.
                    profile::player_store_in_save(&mut r, &session.controls);
                    (session.slot, r.0[profile::SAVE_START..profile::SAVE_START + profile::SAVE_LEN].to_vec())
                };
                let mut front = world.resource_mut::<Front>();
                let e = front.store.store(slot, "", &region)?;
                front.menu.status = format!("saved to slot {slot} (version {})", e.sequence + 1);
            }
            Action::QuitLevel => {
                world.resource_mut::<Ui>().paused = false;
                let mut session = world.resource_mut::<Session>();
                if session.world_number != 0xd
                    && let Some(k) = session.levels.iter().position(|l| l.world == 0xd)
                {
                    session.next = Some(k);
                    session.banner = Some((1.5, "Returning to the Tower".into()));
                }
            }
            Action::QuitGame => end_game(world),
            Action::OpenInventory => {
                let text = inventory_text(&world.resource::<Sim>().game.players[0]);
                let mut front = world.resource_mut::<Front>();
                front.menu.inventory = text;
                front.menu.status.clear();
                front.menu.page = gdl_client::menu::Page::Inventory;
            }
            Action::OpenShop => {
                refresh_shop(world)?;
                let mut front = world.resource_mut::<Front>();
                front.menu.status.clear();
                front.menu.page = gdl_client::menu::Page::Shop { at: 0 };
            }
            Action::Buy { idx } => {
                let items = world.resource::<Front>().shop.clone();
                let result = world.resource_mut::<Sim>().game.shop_buy(0, &items, idx).map_err(|e| e.0)?;
                use gdl_core::game::powerups::Purchase;
                {
                    let mut front = world.resource_mut::<Front>();
                    match result {
                        Purchase::Leave => front.menu.page = gdl_client::menu::Page::Paused { tower: true, at: 2 },
                        Purchase::Bought => front.menu.status = format!("bought {}", items[idx].name),
                        Purchase::Refused => front.menu.status = "not available (gold, room or full health)".into(),
                    }
                }
                refresh_shop(world)?;
            }
            Action::Exit => {
                world.write_message(AppExit::Success);
            }
            Action::Volumes { music, sfx } => {
                if let Some(mut a) = world.get_non_send_resource_mut::<gdl_client::audio::AudioOut>() {
                    a.set_volumes(music, sfx);
                }
                let mut front = world.resource_mut::<Front>();
                front.game_opts[0..4].copy_from_slice(&i32::from(sfx).to_le_bytes());
                front.game_opts[4..8].copy_from_slice(&i32::from(music).to_le_bytes());
                let opts = front.game_opts.clone();
                front.store.store_options(&opts)?;
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        world.resource_mut::<Front>().menu.status = format!("ERROR: {e}");
    }
}

/// Front end and pause menus (exclusive: it starts and ends games).
fn front_system(world: &mut EcsWorld) {
    let screen = *world.resource::<Screen>();
    let naming = matches!(world.resource::<Front>().menu.page, gdl_client::menu::Page::Name { .. });
    let (keys, toggle) = menu_keys(world, naming);
    if screen == Screen::Playing {
        if world.get_resource::<Sim>().is_some_and(|s| s.message_box.is_some()) {
            // ScrollMessageBox's own loop: vblanks from wall time (NTSC).
            let dt = world.resource::<Time>().delta_secs();
            let pressed = keys.contains(&Key::Accept) || keys.contains(&Key::Back);
            let mut sim = world.resource_mut::<Sim>();
            sim.scroll_clock += dt * 59.94;
            let vb = sim.scroll_clock.floor() as i32;
            sim.scroll_clock -= vb as f32;
            if let Some(sb) = sim.message_box.as_mut()
                && !sb.step(vb, pressed)
            {
                sim.message_box = None;
            }
            return;
        }
        if std::mem::take(&mut world.resource_mut::<Sim>().hint_menu) && !world.resource::<Ui>().paused {
            world.resource_mut::<Front>().menu.page = gdl_client::menu::Page::Hints { at: 0 };
            world.resource_mut::<Ui>().paused = true;
            return;
        }
        let paused = world.resource::<Ui>().paused;
        let blocked = world.get_resource::<Session>().is_some_and(|s| s.banner.is_some() || s.finished.is_some());
        if !paused {
            if toggle && !blocked {
                let tower = world.resource::<Session>().world_number == 0xd;
                let mut front = world.resource_mut::<Front>();
                front.menu.status.clear();
                front.menu.pause(tower);
                world.resource_mut::<Ui>().paused = true;
            }
            if world.resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::F1) {
                let mut ui = world.resource_mut::<Ui>();
                ui.details = !ui.details;
            }
            return;
        }
        if toggle && matches!(world.resource::<Front>().menu.page, gdl_client::menu::Page::Paused { .. }) {
            world.resource_mut::<Ui>().paused = false;
            return;
        }
    } else if keys.contains(&Key::Back) && matches!(world.resource::<Front>().menu.page, gdl_client::menu::Page::PressStart) {
        world.write_message(AppExit::Success);
        return;
    }
    for k in keys {
        let action = world.resource_mut::<Front>().menu.input(k);
        if let Some(a) = action {
            act(world, a);
        }
        if *world.resource::<Screen>() != screen {
            break;
        }
    }
    if *world.resource::<Screen>() == Screen::Playing && !world.resource::<Front>().menu.is_paused() {
        world.resource_mut::<Ui>().paused = false;
    }
}

fn front_text(
    screen: Res<Screen>,
    ui: Res<Ui>,
    front: Res<Front>,
    mut text: Query<(&mut Text2d, &mut Transform), With<FrontText>>,
    mut backdrop: Query<(&mut Visibility, Has<FrontPanel>), With<FrontBackdrop>>,
) {
    let original = menu_view(&front, *screen, &ui).is_some() || select_columns(&front, *screen).is_some();
    for (mut v, panel) in &mut backdrop {
        let want = if *screen == Screen::Front && !(panel && original) { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    let Ok((mut t, mut tf)) = text.single_mut() else { return };
    // On the title the menu sits in the lower part, below the logo.
    tf.translation.y = if *screen == Screen::Front { -300.0 } else { 0.0 };
    let show = (*screen == Screen::Front || ui.paused) && menu_view(&front, *screen, &ui).is_none() && select_columns(&front, *screen).is_none();
    let want = if show { front.menu.text() } else { String::new() };
    if t.0 != want {
        t.0 = want;
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
    let pr = &sim_start["presentation"];
    let vec3 = |r: &Value| -> Result<Vec3, Box<dyn Error>> { Ok(Vec3::from_array(serde_json::from_value::<[f32; 3]>(r.clone())?)) };
    let presentation = Presentation {
        basis: [vec3(&pr["camera_basis"][0])?, vec3(&pr["camera_basis"][1])?, vec3(&pr["camera_basis"][2])?],
        fov: serde_json::from_value(pr["camera_fov"].clone())?,
        attention: vec3(&pr["attention_offset_810"])?,
    };
    let mut anim_paths = HashMap::new();
    for (name, v) in cfg["packages"].as_object().ok_or("config packages")? {
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
            index: l["index"].as_u64().ok_or("level index")? as usize,
            world: l["world"].as_i64().ok_or("level world")? as i32,
            wdata: l["wdata"].as_str().ok_or("level wdata")?.into(),
            title: l["title"].as_str().unwrap_or("").to_string(),
            monsters: l["monsters"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default(),
            worlds: l["worlds"].as_str().ok_or("level worlds")?.into(),
            package: l["level"].clone(),
            items: l["items"].as_array().ok_or("level items")?.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
            critters: l["critters"]
                .as_array()
                .map(|a| a.iter().filter_map(|c| Some((c["wad"].as_str()?.to_string(), c["anim"].as_str().map(String::from), c["lib"].as_str().map(String::from)))).collect())
                .unwrap_or_default(),
        });
    }
    if !levels.iter().any(|l| l.world == 0xd) {
        return Err("the configuration has no Tower level (run_play.py builds the campaign by default)".into());
    }
    let package_cfg: HashMap<String, Value> = cfg["packages"].as_object().ok_or("config packages")?.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let audio_cfg = &cfg["audio"];
    let audio_rom = match audio_cfg["rom"].as_str() {
        Some(p) => Some(gdl_formats::audio::AudioRom::parse(&gdl_client::bounded_read(Path::new(p))?)?),
        None => None,
    };
    let paths = |k: &str| -> HashMap<String, std::path::PathBuf> {
        audio_cfg[k].as_object().map_or(HashMap::new(), |m| m.iter().filter_map(|(n, v)| Some((n.to_uppercase(), v.as_str()?.into()))).collect())
    };
    let mut audio = audio_rom.clone().map(|rom| gdl_client::audio::AudioOut::new(rom, paths("banks"), paths("streams")));
    let elf_bytes = gdl_client::bounded_read(&p("elf")?)?;
    let elf = gdl_formats::elf::Elf::parse(&elf_bytes)?;
    let tables = ClassTables::from_elf(&elf)?;
    let heart_rate = elf.object("heart_rate")?.first().copied().unwrap_or(0);
    let missile_desc: Vec<Vec<u8>> = elf.object("PlayerMissileDesc")?.chunks_exact(0x14).map(<[u8]>::to_vec).collect();
    let cat_cfg = &cfg["catalog"];
    let catalog = gdl_client::catalog::Catalog::open(
        Path::new(cat_cfg["names"].as_str().ok_or("catalog names")?),
        Path::new(cat_cfg["archive"].as_str().ok_or("catalog archive")?),
        Path::new(cat_cfg["root"].as_str().ok_or("catalog root")?),
    )?;
    let mut pdats = Vec::new();
    let mut class_fx = Vec::new();
    for tag in &gdl_core::player_class::CLASS_TAGS[..16] {
        let label = format!("PDATA/{}.WAD", tag.to_uppercase());
        let data = gdl_formats::pdata::PlayerData::parse(&catalog.read(&label)?)?;
        // PlayerData +4/+8: the SFXX and DAMG chunks (LoadPlyrData).
        let chunk = |t: &str| data.chunk(t).map(|c| c.bytes.clone()).unwrap_or_default();
        class_fx.push((chunk("SFXX"), chunk("DAMG")));
        pdats.push(data.pdat);
    }
    let start: Value = serde_json::from_slice(&gdl_client::bounded_read(&p("game_start")?)?)?;
    // The PS2 savestate capture's player 0 save region (import source).
    let capture = start["players"][0].as_str().and_then(|h| {
        let b: Vec<u8> = (0..h.len()).step_by(2).filter_map(|i| u8::from_str_radix(h.get(i..i + 2)?, 16).ok()).collect();
        let r = b.get(profile::SAVE_START..profile::SAVE_START + profile::SAVE_LEN)?.to_vec();
        let sha = gdl_core::save::sha256(&r);
        Some((r, sha))
    });
    let shop = gdl_core::game::powerups::shop_items(&catalog.read("SHPDATA/SHOP.WAD")?)?;
    let english = gdl_formats::textrom::TextRom::parse(&catalog.read("TEXT/ENGLISH.ROM")?)?;
    let scrolls = gdl_formats::textrom::TextRom::parse(&catalog.read("TEXT/SCROLL_E.ROM")?)?;
    let store = SaveStore::open(p("saves")?)?;
    // GameOpts: the stored block, else the captured session's GamePrefs
    // (sfx 128, music 128, stereo 1, …).
    let game_opts = store.load_options().unwrap_or_else(|| {
        let mut o = vec![0u8; gdl_core::save::GAME_OPTS_LEN];
        for (k, v) in [128i32, 128, 1, 0, 0, 1, 0, 0].iter().enumerate() {
            o[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
        o
    });
    let word = |k: usize| i32::from_le_bytes(game_opts[4 * k..4 * k + 4].try_into().unwrap()).clamp(0, 255) as u8;
    let (sfx_v, music_v) = (word(0), word(1));
    if let Some(a) = audio.as_mut() {
        a.set_volumes(music_v, sfx_v);
    }
    let shared_labels = std::collections::HashSet::new();
    let static_pkg = load_package(&package_entry(&catalog, "STATIC", true)?)?;
    let mut ui_hud = gdl_core::game::hud::HudTables::from_elf(&elf).map(gdl_core::game::hud::Hud::new).unwrap_or_default();
    let fonts = load_fonts(&catalog, ui_hud.tables.as_ref().map(|t| t.font_desc.clone()).unwrap_or_default());
    let title_pkg = load_package(&package_entry(&catalog, "TITLE", true)?)?;
    let select_pkg = load_package(&package_entry(&catalog, "SELECT", true)?)?;
    ui_hud.set_resources(
        vec![gdl_client::hud_texture_set("STATIC", &static_pkg.objects), gdl_client::hud_texture_set("TITLE", &title_pkg.objects), gdl_client::hud_texture_set("SELECT", &select_pkg.objects)],
        fonts,
    );
    let opt_style = gdl_core::game::optmenu::OptStyle::from_elf(&elf).ok();
    let mut menus = HashMap::new();
    for n in ["title_menu", "title_options", "tower_menu", "game_menu", "tower_options", "game_options", "audio_options", "quitgame_question", "quitlevel_question", "hint_menu", "hint_general_menu", "sel_def_menu"] {
        if let Some(m) = elf.symbol(n).and_then(|sym| gdl_core::game::optmenu::OptMenu::from_elf(&elf, sym.address).ok()) {
            menus.insert(n, m);
        }
    }
    let mut front = Front {
        menu: Menu::new(music_v, sfx_v),
        store,
        catalog,
        elf_bytes,
        tables,
        pdats: profile::Pdats(pdats),
        class_fx,
        missile_desc,
        heart_rate,
        capture,
        game_opts,
        shop,
        english,
        scrolls,
        ui_hud,
        opt_style,
        menus,
        static_pkg,
        title_pkg,
        title_clock: 0.0,
        select_pkg,
        menu_clock: (None, 0.0),
        base: Base { package_cfg, start, levels, monster_paths, monsters, anim_paths, audio_rom, shared_labels },
    };
    refresh_saves(&mut front);
    // hint_menu categories → HINTS_E.ROM lists (every line of every entry,
    // in list order; NATIVE: next_*_hint's progress-based choice is not ported).
    let hints_rom = gdl_formats::textrom::TextRom::parse(&front.catalog.read("TEXT/HINTS_E.ROM")?)?;
    front.menu.hints = ["GENERAL_HINTS", "BOSS_HINTS", "LEGEND_HINTS", "RUNE_HINTS"]
        .iter()
        .map(|n| hints_rom.list(n).unwrap_or(&[]).iter().flat_map(|&k| hints_rom.entries[k].lines.iter().map(|l| l.replace('\r', ""))).collect())
        .collect();
    let mut app = App::new();
    if let Some(a) = audio {
        app.insert_non_send_resource(a);
    }
    app
        .insert_resource(front)
        .insert_resource(Screen::Front)
        .insert_resource(presentation)
        .init_resource::<Libraries>()
        .init_resource::<PartVariants>()
        .init_resource::<PendingTreePsys>()
        .insert_resource(PsysClock { global: gdl_core::psys::PsysGlobal::new(1), accumulated: 0.0 })
        .init_resource::<EnemyFrames>()
        .insert_resource(Time::<Fixed>::from_hz(30.0))
        .insert_resource(ClearColor(Color::linear_rgb(0.035, 0.04, 0.05)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Gauntlet: Dark Legacy - INTERIM native build".into(),
                resolution: (1280, 896).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(gdl_client::GsMaterialPlugin)
        .init_resource::<Ui>()
        .init_resource::<HudSets>()
        .add_systems(Startup, setup_static)
        .add_systems(FixedUpdate, simulate.run_if(playing))
        .add_systems(
            Update,
            (
                front_system,
                ps2_viewport,
                (advance_level, present_world, present_world_nodes, present_node_flags, present_scene_light, present_distance_fade, present_camera_face, present_effect_trees, present_camera_face_local, spawn_tree_psys, follow_tree_psys, present_particles, present_texmods, hide_exploded_nodes, present_critters, present_sumner, present_sparks, present_player, present_player_shadow).chain().run_if(playing),
                front_text,
                present_hud,
            )
                .chain(),
        )
        .run();
    Ok(())
}

/// Package names a level draws or reads: the shared packages, its item
/// packages, its realm's monsters and its critter models.
fn level_packages(lv: &LevelSource) -> Vec<String> {
    let mut v: Vec<String> = ["POWERUPS", "WEAPONS", "STATIC"].iter().map(|s| s.to_string()).collect();
    v.extend(lv.items.iter().cloned());
    v.extend(lv.monsters.iter().cloned());
    v.extend(lv.critters.iter().filter_map(|c| c.2.clone()));
    v
}

/// Loads the level's packages that are not cached yet; returns the object
/// labels of the level's packages (the MBOX tables the level searches).
fn load_level_packages(cache: &mut HashMap<String, Package>, cfg: &HashMap<String, Value>, lv: &LevelSource) -> Result<std::collections::HashSet<String>, Box<dyn Error>> {
    let mut labels = std::collections::HashSet::new();
    for name in level_packages(lv) {
        if !cache.contains_key(&name) {
            let v = cfg.get(&name).ok_or_else(|| format!("package {name} not configured"))?;
            cache.insert(name.clone(), load_package(v)?);
        }
        labels.extend(cache[&name].objects.definitions.iter().map(|d| d.label.clone()));
    }
    Ok(labels)
}

/// Level complete → banner → next level (exit destination wave), or the
/// end of the playable realm. Enter continues after death (interim).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn advance_level(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<Session>,
    mut sim: ResMut<Sim>,
    (old, root, bodies): (Query<Entity, With<LevelEntity>>, Query<Entity, With<MirrorRoot>>, Query<Entity, With<CharacterRoot>>),
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<gdl_client::GsMaterial>>),
    mut audio: Option<NonSendMut<gdl_client::audio::AudioOut>>,
    mut sources: ResMut<Sources>,
    mut libraries: ResMut<Libraries>,
    mut frames: ResMut<EnemyFrames>,
    (front, ui, mut hud_sets): (Res<Front>, Res<Ui>, ResMut<HudSets>),
) {
    // do_players state 8 plays the dying action (0x7e, +0x8f8) and calls
    // kill_player; in the Tower PlayerRestoreState puts the player back.
    // With nobody left playing (states 1/4/8), game_main goes to TowerWave.
    if !ui.paused
        && session.banner.is_none()
        && sim.game.players[0].i32_(0xfc) == 0xb
        && !(0..4).any(|k| matches!(sim.game.players[k].i32_(0xfc), 1 | 4 | 8))
        && let Some(k) = session.levels.iter().position(|l| l.world == 0xd)
    {
        session.next = Some(k);
        session.banner = Some((3.0, "YOU DIED - back to the Tower (the level's gains are lost)".into()));
    }
    if !ui.paused && keys.just_pressed(KeyCode::F9) && session.banner.is_none() && sim.game.level_done.is_none() {
        // DEBUG: act as if the exit portal (destination: next level) was used.
        let next = session.levels.get(session.current + 1).map_or(session.world_number << 8 | 0xff, |l| l.world << 8 | l.index as i32);
        sim.game.level_done = Some(next);
    }
    if session.banner.is_none()
        && session.finished.is_none()
        && let Some(wave) = sim.game.level_done.take()
    {
        let world = wave >> 8;
        let level = (wave & 0xff) as usize;
        let name = session.levels[session.current].name.clone();
        // game_main → PlayersSetWaveFinished(PrevWorld, PrevLevel) for the
        // level just completed (realm progress, tower exits).
        let (cw, cl) = (session.world_number, session.levels[session.current].index as i32);
        let (l90, l92) = (sim.game.level_i16(0x90), sim.game.level_i16(0x92));
        sim.game.players_set_wave_finished(cw, cl, l90, l92);
        if let Some(k) = session.levels.iter().position(|l| l.index == level && l.world == world) {
            session.next = Some(k);
            let to = if session.levels[k].world == session.world_number { session.levels[k].name.clone() } else { format!("{} {}", session.levels[k].title, session.levels[k].name) };
            session.banner = Some((3.0, format!("{name} COMPLETE - entering {to}")));
        } else if wave == gdl_core::game::boss::TOWER_WAVE {
            session.finished = Some(format!(
                "{} COMPLETE!\nThe boss is defeated; the game would return to the hub (not ported yet).\nThanks for testing! (Esc to quit)",
                session.title
            ));
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
        // Packages of a realm not seen yet load now (libraries and frames too).
        let labels = load_level_packages(&mut sources.packages, &session.package_cfg, &session.levels[idx])?;
        for name in level_packages(&session.levels[idx]) {
            if !libraries.0.contains_key(&name) {
                let pkg = &sources.packages[&name];
                libraries.0.insert(name.clone(), build_library(pkg, &mut meshes, &mut images, &mut materials));
                if sources.monsters.values().any(|m| *m == name) {
                    frames.0.insert(name.clone(), enemy_frames(pkg));
                }
            }
        }
        let character: Vec<String> = sources.character.objects.definitions.iter().map(|d| d.label.clone()).collect();
        let sfx: Vec<String> = sources.packages.get(&session.sfx).map(|p| p.objects.definitions.iter().map(|d| d.label.clone()).collect()).unwrap_or_default();
        session.object_labels = labels.into_iter().chain(character).chain(sfx).collect();
        if session.world_number != 0xd {
            session.last_world = session.world_number;
        }
        session.world_number = session.levels[idx].world;
        session.title = session.levels[idx].title.clone();
        Ok(())
    })();
    // Session globals into the next level; leaving the Tower runs EndTower.
    if sim.game.globals.cur_world == 0xd && session.levels[idx].world != 0xd {
        sim.game.end_tower();
    }
    let g = &sim.game.globals;
    session.carried = [g.tower_counter, g.last_finished_world, g.last_finished_level, g.cur3d_transmitter, g.cur_transmitter];
    let carry = sim.game.players[0].clone();
    let coop_carry: Vec<Option<gdl_core::game::record::Record>> = (1..4).map(|k| sim.coop[k - 1].then(|| sim.game.players[k].clone())).collect();
    let controls = sim.game.controls.clone();
    // load_player: the model follows the level tier (level / 10).
    let tier = (carry.i32_(0x2ac0) / 10).clamp(0, 9);
    let mut new_body = false;
    let loaded = loaded.and_then(|_| {
        if tier != session.tier {
            let ch = character_assets(&front, session.class, session.colour, tier)?;
            let (character, rig, clips, clip_bytes) = character_rig(&front, &ch)?;
            sources.packages.insert(ch.model_name.clone(), load_package(&ch.model)?);
            if let Some(pkg) = sources.packages.get(&ch.model_name)
                && !libraries.0.contains_key(&ch.model_name)
            {
                libraries.0.insert(ch.model_name.clone(), build_library(pkg, &mut meshes, &mut images, &mut materials));
            }
            sources.character = character;
            sources.rig = rig;
            sources.clips = clips;
            sources.clip_bytes = clip_bytes;
            session.throw = ch.throw;
            session.tier = tier;
            new_body = true;
        }
        Ok(())
    });
    match loaded.and_then(|_| build_game(&session, idx, &carry)) {
        Ok(mut game) => {
            // The controls state carries across levels; co-op players keep
            // their records and enter beside player 1.
            game.controls = controls;
            for (k, rec) in coop_carry.iter().enumerate() {
                let Some(rec) = rec else { continue };
                game.players[k + 1] = rec.clone();
                sim.coop[k] = join_copy(&mut game, k + 1, k + 1).is_ok();
            }
            if let Some(a) = audio.as_mut() {
                a.stop_all();
                if let Some(stage) = game.sounds.stage.as_ref() {
                    a.music(&stage.stream_name(0, 0));
                }
            }
            session.current = idx;
            init_hud(&mut game, &sources, &session, &front.catalog, &front.english, &mut hud_sets);
            sim.game = game;
            for e in &old {
                commands.entity(e).despawn();
            }
            if let Ok(root) = root.single() {
                let n = sim.game.items.records.len();
                spawn_level(&mut commands, root, &session, n, &mut meshes, &mut images, &mut materials);
                spawn_package_texmods(&mut commands, &libraries, &level_packages(&session.levels[session.current]));
                if new_body {
                    for b in &bodies {
                        commands.entity(b).despawn();
                    }
                    spawn_bodies(&mut commands, root, &sources, &session, &mut meshes, &mut images, &mut materials);
                }
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
