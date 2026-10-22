//! INTERIM PLAYTEST build with the front end (M17 interim) and native
//! saves (M18): title → the original player-select screen (saved
//! characters on the native card, new characters, Hidden codes; the PS2
//! capture's character import behind GDL_DEBUG_NATIVE_SELECT) → the Tower
//! and every realm, with the original pause menus, shop, map and Manage
//! Character.
//!
//! Characters: the original save region and class records (game::profile:
//! player_get_from_save / player_store_in_save / level start), the class
//! model players/<cls>/<colour><level/10>0, the class clips
//! players/<cls>/anim, the SFX package players/<cls>/sfx<colour> and the
//! thrown missile <PlayerMissileDesc>_THROW<n>.
//! Saves: gdl_core::save (versioned envelopes, backups, recovery).
//!
//! Simulation: game_main order with the ported item/enemy/effect systems and
//! the record-based players (PlayerControls → do_players → PlayerMotion →
//! DoPlayerAction; research/player.md). Host keys and gamepads fill the
//! joyGetStatus entries of ports 0–3 (HYPOTHESIS mapping, below).
//! Presentation: GS-equivalent materials and display-space blending
//! (gs.wgsl, blit.wgsl), the StdCam camera, original HUD/menus/front end
//! (game::hud, optmenu, select); enemy frame objects from OANIM entries.
//! Audio: original banks and music through gdl_client::audio (approximate
//! mixing).
use bevy::{core_pipeline::tonemapping::Tonemapping, ecs::system::RunSystemOnce, ecs::world::World as EcsWorld, prelude::*, sprite::Anchor};
use gdl_client::menu::{Action, Entry, Key, Menu};
use gdl_core::{
    game::{effects::Visual, pad::PadStatus, pickups::ClassData, profile, record::Record, setup, Event, Game},
    player_class::ClassTables,
    save::SaveStore,
};
use gdl_core::game::exitflow::LevelEnd;
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

/// The 3D viewport: the PS2 frame's rows above the window's bottom inset
/// (64 of 448 in play — InitCamera's WindowBottomLim; none in the flyby).
fn ps2_viewport(windows: Query<&Window>, sim: Option<Res<Sim>>, mut cams: Query<&mut Camera, With<FollowCamera>>) {
    let Ok(w) = windows.single() else { return };
    let (width, height) = (w.physical_width(), w.physical_height());
    let (frame, inset) = sim
        .as_ref()
        .and_then(|s| s.game.std_camera.as_ref())
        .map_or((448.0, 64.0), |c| (c.window.frame[1] as f32, c.window.viewport[3] + c.window.viewport[2]));
    let rows = (height as f32 * (frame - inset) / frame).round() as u32;
    if let Ok(mut cam) = cams.single_mut() {
        let want = bevy::camera::Viewport { physical_position: UVec2::ZERO, physical_size: UVec2::new(width.max(1), rows.max(1)), ..default() };
        if cam.viewport.as_ref().is_none_or(|v| v.physical_size != want.physical_size) {
            cam.viewport = Some(want);
        }
    }
}

type Parts = Vec<(Handle<Mesh>, Handle<gdl_client::GsMaterial>)>;
/// Model parts with node offsets and the tree node's MB flags (+0x30,
/// AtreeNodeInit's MBTreeSetFlags; 1 for DUMMY nodes).
type Model = Vec<(Vec3, Parts, u32, Option<usize>, Option<NodeExtra>)>;

/// What a tree node draws besides its object: a particle system (type 4) or
/// one frame object of an OANIM node (type 2, DoObjAnimation).
#[derive(Clone)]
enum NodeExtra {
    Psys(TreePsys),
    Frame(OFrame),
}

/// One object of a type-2 node's OANIM entry for `seq` (AtreeNodeInit:
/// tree header +8 base + node +0x34): DoObjAnimation shows object k when
/// (int)(frame + 0.5) − start == k inside [start, start + count − 1]; a
/// one-frame entry always shows it.
#[derive(Component, Clone, Copy)]
struct OFrame {
    seq: u16,
    start: i16,
    k: i16,
    single: bool,
}

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
    // AtreeNodeInit adds node +0x34 as a signed word (records may precede
    // the header-0 base, e.g. the SFX packages' WEAP_HOLD/WEAP_TW psys).
    let at = usize::try_from((base as i64).checked_add(header0 as i64)?.checked_add(i64::from(node.sequence_info_offset))?).ok()?;
    let record = anim_bytes.get(at..at.checked_add(gdl_core::psys::RECORD)?)?.to_vec();
    let f = |o: usize| f32::from_le_bytes(node.raw[o..o + 4].try_into().unwrap());
    Some(TreePsys { record, dir: [f(0x14), f(0x18), f(0x1c)] })
}

/// The MB flags of a package tree node (AtreeNodeInit / AtreeInitSub): a
/// node without its object (+0x2e bit 0, DUMMY) is hidden unless it is an
/// OANIM node (type 2), whose object DoObjAnimation sets.
fn tree_node_flags(node: &gdl_formats::animation::Node) -> u32 {
    let mut f = node.model_flags;
    if node.node_type != 2 && (node.label == "DUMMY" || node.flags & 1 != 0) {
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
    /// Object parts by object index and object indices by upper-case
    /// definition label (OANIM frames are consecutive object indices).
    by_index: HashMap<u32, Parts>,
    index_of: HashMap<String, u32>,
    /// Bitmap records whose image could not be decoded (placeholders).
    unresolved: std::collections::HashSet<u16>,
}

#[derive(Resource)]
struct Sources {
    /// Each player slot's loaded character (load_player / load_player_model).
    players: [Option<PlayerChar>; 4],
    packages: HashMap<String, Package>,
    /// Enemy type → monster package name (prefix).
    monsters: HashMap<i32, String>,
}

/// What a player's character was loaded for: class +0xc, colour +4, the
/// model tier (level / 10, base classes only) and the descriptor +0x104 (a
/// guest pointer to a model code string: a Hidden entry or "sum").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CharKey {
    class: u32,
    colour: usize,
    tier: i32,
    desc: u32,
}

impl CharKey {
    fn of(r: &Record) -> Self {
        let class = r.u32_(0xc).min(16);
        let tier = if class < 8 && r.u32_(0x104) == 0 { (r.i32_(0x2ac0) / 10).clamp(0, 9) } else { 0 };
        Self { class, colour: r.u32_(4).min(3) as usize, tier, desc: r.u32_(0x104) }
    }
}

/// One player's character: model package, rig, the base class's clips,
/// the SFX package and the throw tree.
struct PlayerChar {
    key: CharKey,
    character: Package,
    rig: Animations,
    clips: Animations,
    clip_bytes: Vec<u8>,
    model_name: String,
    sfx_name: String,
    throw: (String, String),
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
    /// LEVL +0x34: the movie init_gamemovie plays before the level ("" none).
    movie: String,
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
    /// TowerStartPos (TowerSetStartPos before the Tower menu's shop): the
    /// start locator of the next Tower entry, else the realm's wing.
    tower_start: Option<i16>,
    /// GamePrefs words (difficulty +0x14 → ResolveWorldData's
    /// diff_mul_tab index; multiplayer +0x1c → pvp_mode).
    prefs: [i32; 8],
    current: usize,
    world: World,
    level: Package,
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
    pdats: profile::Pdats,
    heart_rate: u8,
    /// Character packages: the SFX package name, the thrown-missile tree

    /// Session globals carried from level to level: TowerCounter,
    /// LastFinishedWorld, LastFinishedLevel, Cur3DTransmitter,
    /// CurTransmitter, LastWorldPlayed, LastLevelPlayed.
    carried: [i32; 7],
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
    /// The disc's movies ("MPEG/MOVIEA1", "MPEG2/MIDWAY" → .PSS path).
    movies: HashMap<String, std::path::PathBuf>,
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
    /// The select screen's ELF tables (sel menus, Hidden, Cheats, names).
    sel: gdl_core::game::pselect::SelTables,
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
    /// POWERUPS (powerup_anim: the option menus' ICON_ARROW tree).
    powerups_pkg: Package,
    /// The Control Style screen's tables.
    controls_menu: Option<gdl_core::game::optmenu::ControlsMenu>,
    menu_clock: (Option<(&'static str, usize)>, f32),
    base: Base,
    /// The attract sequence (init_attract_mode), its running entry and the
    /// entry's blits; SONY, GROUP1 and CREDITS as front texture sets 3–5.
    attract: gdl_core::game::attract::Attract,
    attract_run: Option<AttractRun>,
    attract_blits: Vec<gdl_core::game::hud::Blit>,
    credits: gdl_core::game::attract::Credits,
    attract_pkgs: Vec<Package>,
    /// titlescreen_timeout in vblanks (init_titlescreen_timer: 0x708) and
    /// the attract modes' 30 Hz step clock (SetMaxFPS(0x1e)).
    title_timeout: f32,
    attract_clock: f32,
    /// TEXT/HINTS_E.ROM and the hint pad's state (HintMenu, next_*_hint).
    hints_rom: gdl_formats::textrom::TextRom,
    hints: gdl_core::game::hints::Hints,
    /// init_flyby: the attract level to build (front_system has the world).
    flyby_request: Option<usize>,
    /// end_fullscreen_inventory's vblank clock while the exit swirl runs.
    inventory_close: Option<f32>,
}

/// The running attract entry (game modes 0x8000..0x8008; the title screen
/// is the front end's Press Start page).
enum AttractRun {
    /// Screen2D table 0: the SONY screen during the card check.
    Card(gdl_core::game::attract::CardCheck),
    /// Screen2D tables 1/2: the group shot for screen2dtimeleft vblanks
    /// (NATIVE: the next flyby level loads when the flyby starts).
    Group { left: i32 },
    /// init_movie / do_movie: "/MPEG2/<name>.pss".
    Movie { name: String, player: Option<gdl_client::movie::MoviePlayer>, tried: bool },
    /// init_credits / do_credits.
    Credits,
    /// init_flyby / do_flyby: the level runs on the Playing screen.
    Flyby,
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

/// Sumner (SumnerInit's GWIZ tree, 0), his ghost (SumnerGhostStart's
/// WIZARD tree, 1) or the good wizard after a boss (DoGoodWizard's WIZARD
/// tree, 2) and one of their tree nodes.
#[derive(Component)]
struct SumnerRoot(u8);
#[derive(Component)]
struct SumnerPart(u8, usize);

/// Sumner drawn from the GWIZ tree of the level's item package at his
/// lookout matrix, while he speaks his ghost from the WIZARD tree at the
/// waypoint, and after a boss the good wizard (WIZARD, faded in by
/// MBTreeSetAlpha), each posed by its animation clock (presentation of
/// game::tower::SumnerState / GhostState and game::goodwiz::GoodWiz).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn present_sumner(
    mut commands: Commands,
    sim: Res<Sim>,
    session: Res<Session>,
    sources: Res<Sources>,
    libs: Res<Libraries>,
    root: Query<Entity, With<MirrorRoot>>,
    mut body: Query<(&SumnerRoot, &mut Transform, &mut Visibility), Without<SumnerPart>>,
    mut parts: Query<(Entity, &SumnerPart, &mut Transform), Without<SumnerRoot>>,
    (children, mut req): (Query<&Children>, ResMut<MatRequests>),
) {
    let t = &sim.game.tower;
    let w = &sim.game.good_wiz;
    let shown = [
        t.sumner.as_ref().map(|s| (s.matrix, s.current, s.clock.frame)),
        t.ghost.as_ref().map(|g| (g.matrix, g.current, g.clock.frame)),
        w.matrix.and_then(|m| w.clock.map(|(c, cur)| (m, cur, c.frame))),
    ];
    let items = &session.levels[session.current].items;
    for (which, label) in [(0u8, "GWIZ"), (1, "WIZARD"), (2, "WIZARD")] {
        let Some((pkg_name, pkg, ti)) = items.iter().find_map(|n| {
            let p = sources.packages.get(n)?;
            let ti = p.anim.as_ref()?.trees.iter().position(|t| t.label == label)?;
            Some((n.clone(), p, ti))
        }) else {
            continue;
        };
        let state = shown[which as usize];
        let anim = pkg.anim.as_ref().unwrap();
        let tree = &anim.trees[ti];
        let spawned = body.iter().any(|(r, _, _)| r.0 == which);
        let Some((m, current, frame)) = state else {
            for (r, _, mut vis) in &mut body {
                if r.0 == which && *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
            continue;
        };
        let tf = record_transform([Vec3::new(m[0][0], m[0][1], m[0][2]), Vec3::new(m[1][0], m[1][1], m[1][2]), Vec3::new(m[2][0], m[2][1], m[2][2])], Vec3::new(m[3][0], m[3][1], m[3][2]));
        if !spawned {
            let (Ok(root), Some(lib)) = (root.single(), libs.0.get(&pkg_name)) else { continue };
            let b = commands.spawn((SumnerRoot(which), tf, Visibility::default(), LevelEntity)).id();
            commands.entity(root).add_child(b);
            for node in &tree.nodes {
                let label: String = (tree.prefix.clone() + &node.label).chars().take(15).collect();
                if let Some(list) = lib.by_label.get(&label) {
                    let holder = commands.spawn((SumnerPart(which, node.index as usize), Transform::default(), Visibility::default())).id();
                    for (mesh, material) in list {
                        let c = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default())).id();
                        commands.entity(holder).add_child(c);
                    }
                    commands.entity(b).add_child(holder);
                }
            }
            continue;
        }
        for (r, mut t, mut vis) in &mut body {
            if r.0 == which {
                *t = tf;
                if *vis == Visibility::Hidden {
                    *vis = Visibility::Inherited;
                }
            }
        }
        if let Ok(pose) = gdl_client::tree_pose(anim, ti, &pkg.anim_bytes, current as usize, frame.max(0.0), true) {
            for (holder, part, mut t) in &mut parts {
                if part.0 != which {
                    continue;
                }
                if let Some(mat) = pose.get(part.1) {
                    *t = Transform::from_matrix(*mat);
                }
                // The good wizard's MBTreeSetAlpha (0 opaque … 0xff clear).
                if which == 2
                    && let Ok(meshes) = children.get(holder)
                {
                    let a = (255 - w.alpha.clamp(0, 255)) as f32 / 255.0;
                    for m in meshes.iter() {
                        req.alpha(m, a);
                    }
                }
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
    let eye = Vec3::from_array(cam.pb.eye);
    let rows = cam.pb.cam_rows.map(Vec3::from_array);
    for (face, mut tf) in &mut nodes {
        let m = camera_face_rotation(face.mode, face.anchor, eye, rows);
        tf.translation = face.anchor + m * face.offset;
        tf.rotation = Quat::from_mat3(&m);
    }
}

/// CritterAddShadow's object per critter with a shadow (Critter::shadow):
/// SHADOW<n>L1 from the critter's model package at the simulated position,
/// world-aligned.
#[derive(Component)]
struct CritterShadow(usize);

#[allow(clippy::type_complexity)]
fn present_critter_shadows(
    mut commands: Commands,
    sim: Res<Sim>,
    session: Res<Session>,
    libs: Res<Libraries>,
    root: Query<Entity, With<MirrorRoot>>,
    mut shadows: Query<(Entity, &CritterShadow, &mut Transform, &mut Visibility)>,
) {
    let g = &sim.game;
    let mut have = std::collections::HashSet::new();
    for (e, cs, mut tf, mut vis) in &mut shadows {
        match g.critters.get(cs.0).and_then(Option::as_ref).and_then(|c| c.shadow) {
            Some((_, at)) => {
                tf.translation = Vec3::from_array(at);
                if *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
                have.insert(cs.0);
            }
            None => commands.entity(e).despawn(),
        }
    }
    let Ok(r) = root.single() else { return };
    for (i, c) in g.critters.iter().enumerate() {
        let Some(c) = c else { continue };
        let Some((n, at)) = c.shadow else { continue };
        if have.contains(&i) {
            continue;
        }
        let file = &g.critter_data.files[c.ty.0];
        let prefix = file.trees[c.ty.1].and_then(|t| file.model.as_ref().map(|m| m.0.trees[t].prefix.clone())).unwrap_or_default();
        let lib_name = session.levels.get(session.current).and_then(|l| l.critters.get(c.ty.0)).and_then(|x| x.2.clone()).unwrap_or(prefix);
        let label = format!("SHADOW{n}L1");
        let Some(parts) = libs.0.get(&lib_name).and_then(|l| l.by_label.get(&label)) else { continue };
        let e = commands.spawn((CritterShadow(i), Transform::from_translation(Vec3::from_array(at)), Visibility::Inherited, LevelEntity)).id();
        commands.entity(r).add_child(e);
        for (mesh, material) in parts {
            let ch = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default())).id();
            commands.entity(e).add_child(ch);
        }
    }
}

/// The compass (StartCompass: POWERUPS' COMPASS object under the world
/// root; UpdateCompass: shown with GamePrefs +0x18 outside menus, at
/// MBWindowTo3D(CompassZ 10, CompassPos (64, 128)), scale CompassScale 1.5,
/// MBTreeSetAlpha(CompassAlpha 128)); its rotation stays world-aligned.
/// HYPOTHESIS: the window point maps over the 640 × 384 viewport.
#[derive(Component)]
struct CompassObj;

#[allow(clippy::too_many_arguments)]
fn present_compass(
    mut commands: Commands,
    sim: Res<Sim>,
    session: Res<Session>,
    ui: Res<Ui>,
    presentation: Res<Presentation>,
    libs: Res<Libraries>,
    root: Query<Entity, With<MirrorRoot>>,
    mut compass: Query<(Entity, &mut Transform, &mut Visibility, Option<&Children>), With<CompassObj>>,
    mut req: ResMut<MatRequests>,
) {
    let show = session.prefs[6] != 0 && !ui.paused && sim.shop.is_none() && sim.map.is_none();
    let Some(cam) = sim.game.std_camera.as_ref() else { return };
    let eye = Vec3::from_array(cam.pb.eye);
    let rows = cam.pb.cam_rows.map(Vec3::from_array);
    let tan_v = (presentation.fov * 0.5).tan() * 384.0 / 640.0;
    let tan_h = tan_v * 640.0 / 384.0;
    let (nx, ny) = (64.0 / 320.0 - 1.0, 1.0 - 128.0 / 192.0);
    let z = 10.0;
    let at = eye + rows[2] * z + rows[0] * (nx * z * tan_h) + rows[1] * (ny * z * tan_v);
    match compass.single_mut() {
        Ok((_, mut tf, mut vis, kids)) => {
            *tf = Transform::from_translation(at).with_scale(Vec3::splat(1.5));
            let want = if show { Visibility::Inherited } else { Visibility::Hidden };
            if *vis != want {
                *vis = want;
            }
            if let Some(kids) = kids {
                for k in kids.iter() {
                    req.alpha(k, (255.0 - 128.0) / 255.0);
                }
            }
        }
        Err(_) => {
            let (Ok(r), Some(parts)) = (root.single(), libs.0.get("POWERUPS").and_then(|l| l.by_label.get("COMPASS"))) else { return };
            let e = commands.spawn((CompassObj, Transform::from_translation(at), Visibility::Hidden, LevelEntity)).id();
            commands.entity(r).add_child(e);
            for (mesh, material) in parts {
                let c = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default())).id();
                commands.entity(e).add_child(c);
            }
        }
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
    let eye = Vec3::from_array(cam.pb.eye);
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

/// Per-mesh material requests for this frame: texture swaps (target image →
/// alternate image: MBTreeSetAltTex) and an opacity factor (MBTreeSetAlpha
/// and texture fades); apply_material_requests realises them on a material
/// of the mesh's own.
#[derive(Resource, Default)]
struct MatRequests(HashMap<Entity, MatReq>);

#[derive(Default)]
struct MatReq {
    swap: Vec<(AssetId<Image>, Handle<Image>)>,
    /// MBTreeSetAltTex −2/−3: every texture replaced (pbObjTexSub).
    replace: Option<Handle<Image>>,
    /// MBTreeSetAltTex −4: the own textures plus the alternate one in the
    /// keep-A pass (GsMaterial::overlay).
    overlay: Option<Handle<Image>>,
    /// MBTreeSetAltTex −3: the replacement drawn with setChrome's
    /// environment mapping (draw flag 0x80000).
    chrome: bool,
    /// MBTreeSetUVScaleAdd(U scale, U add, V scale, V add); each axis
    /// overrides only its own pair (the other stays identity).
    uv: Option<Vec4>,
    alpha: Option<f32>,
    /// MBTreeSetColor (0..1 per channel). HYPOTHESIS: the node colour
    /// (flag 0x100) scales the vertex colour like MBBlitSetColor.
    color: Option<Vec3>,
    /// MBTreeSetAmbientAdd / 255.
    ambient: f32,
}

impl MatRequests {
    fn get(&mut self, e: Entity) -> &mut MatReq {
        self.0.entry(e).or_default()
    }
    fn alpha(&mut self, e: Entity, f: f32) {
        let r = self.get(e);
        r.alpha = Some(r.alpha.unwrap_or(1.0) * f);
    }
    fn swap(&mut self, e: Entity, target: AssetId<Image>, alt: Handle<Image>) {
        self.get(e).swap.push((target, alt));
    }
}

/// A mesh drawn through a material of its own; `base` is restored when no
/// request remains.
#[derive(Component)]
struct MatOverride {
    base: Handle<gdl_client::GsMaterial>,
}

/// MBTreeSetAlpha on item trees (Game::item_alpha) and world nodes with
/// their subtrees (Game::node_alpha): node +0x53 = ~alpha with the
/// translucent flag 0x200. HYPOTHESIS: the drawn alpha is the material's
/// times (255 − alpha) / 255 (the +0x53 byte's use in the draw path is not
/// traced).
fn present_tree_alpha(sim: Res<Sim>, slots: Query<(&Slot, &Children)>, children: Query<&Children>, world: Query<(Entity, &NodeChain), With<WorldNode>>, mut req: ResMut<MatRequests>) {
    let g = &sim.game;
    for (slot, kids) in &slots {
        if slot.kind != SlotKind::Item {
            continue;
        }
        let a = g.item_alpha.get(&slot.index).copied().unwrap_or(0);
        if a <= 0 {
            continue;
        }
        let f = (255 - a.min(255)) as f32 / 255.0;
        let mut stack: Vec<Entity> = kids.iter().collect();
        while let Some(e) = stack.pop() {
            req.alpha(e, f);
            if let Ok(c) = children.get(e) {
                stack.extend(c.iter());
            }
        }
    }
    if g.node_alpha.iter().any(Option::is_some) {
        for (e, chain) in &world {
            if let Some(a) = chain.0.iter().find_map(|&(n, _)| g.node_alpha.get(n).copied().flatten())
                && 0 < a
            {
                req.alpha(e, (255 - u32::from(a)) as f32 / 255.0);
            }
        }
    }
}

/// The image of a skin texture (InitEffects' names) at `frame`: WEAPONS
/// texmod frame bases (DEATH*), POWERUPS' AAAWHITE, WEAPONS' CHROME*
/// textures, or the enemy packages' DEATHALT / DEATHGOLEM texmods.
fn skin_image(libs: &Libraries, monsters: &[String], v: &gdl_core::game::skin::SkinView) -> Option<Handle<Image>> {
    let tex = gdl_core::game::skin::SkinTex::from_code(v.tex)?;
    let (kind, name) = tex.source();
    let texmod = |lib: &Library| -> Option<Handle<Image>> {
        let t = lib.texmods.iter().find(|t| t.target_name == name)?;
        let r = u16::try_from(t.frame_base + v.frame.max(0)).ok()?;
        lib.record_images.get(&r).cloned()
    };
    match kind {
        0 => texmod(libs.0.get("WEAPONS")?),
        1 => libs.0.get("POWERUPS")?.textures.get(name).cloned(),
        2 => libs.0.get("WEAPONS")?.textures.get(name).cloned(),
        _ => monsters.iter().find_map(|m| libs.0.get(m).and_then(texmod)),
    }
}

/// Enemy and player trees' skins (MBTreeSetAltTex −4 → the skin image in
/// the keep-A pass over the own textures, −3 → the skin image replacing
/// them with setChrome's environment mapping) and ambient adds
/// (MBTreeSetAmbientAdd / 255).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn present_skins(
    sim: Res<Sim>,
    session: Res<Session>,
    libs: Res<Libraries>,
    slots: Query<(&Slot, &Children)>,
    bodies: Query<(&CharacterRoot, &Children)>,
    children: Query<&Children>,
    mut req: ResMut<MatRequests>,
    (critter_slots, critter_parts): (Query<(&CritterSlot, &Children)>, Query<(&CritterPart, &Children)>),
) {
    let g = &sim.game;
    let monsters = &session.levels[session.current].monsters;
    let apply = |root: &Children, v: &gdl_core::game::skin::SkinView, req: &mut MatRequests| {
        let image = if v.mode < -1 { skin_image(&libs, monsters, v) } else { None };
        if image.is_none() && v.ambient == 0 {
            return;
        }
        let mut stack: Vec<Entity> = root.iter().collect();
        while let Some(e) = stack.pop() {
            let r = req.get(e);
            if let Some(i) = &image {
                // pbObjTexSub: −4 keeps the own textures and adds the keep-A
                // pass; −3 (chrome) and −2 replace every texture.
                if v.mode == -4 {
                    r.overlay = Some(i.clone());
                } else {
                    r.replace = Some(i.clone());
                    r.chrome = v.mode == -3;
                }
            }
            r.ambient += v.ambient as f32 / 255.0;
            if let Ok(c) = children.get(e) {
                stack.extend(c.iter());
            }
        }
    };
    for (slot, kids) in &slots {
        if slot.kind == SlotKind::Enemy
            && let Some(v) = g.skin_views.enemies.get(slot.index)
        {
            apply(kids, v, &mut req);
        }
    }
    for (root, kids) in &bodies {
        if let Some(v) = g.skin_views.players.get(root.0) {
            apply(kids, v, &mut req);
        }
    }
    // Critters: a node belongs to the child instance whose animation root is
    // its ancestor, else to the root; a flashing collision part whitens its
    // reference node's subtree.
    let white = gdl_core::game::skin::SkinView { mode: -4, tex: gdl_core::game::skin::SkinTex::White as i32, frame: 0, ambient: 0xff };
    for (slot, kids) in &critter_slots {
        let Some(Some(cr)) = g.critters.get(slot.index) else { continue };
        if cr.parent.is_some() {
            continue;
        }
        let under = |mut n: i32, a: usize| -> bool {
            let mut guard = 0;
            while 0 <= n && guard < 512 {
                if n as usize == a {
                    return true;
                }
                n = cr.parents.get(n as usize).copied().unwrap_or(-1);
                guard += 1;
            }
            false
        };
        let children: Vec<usize> = g.critters.iter().enumerate().filter(|(_, c)| c.as_ref().is_some_and(|c| c.parent == Some(slot.index))).map(|(k, _)| k).collect();
        for holder in kids.iter() {
            let Ok((part, meshes)) = critter_parts.get(holder) else { continue };
            let owner = children.iter().copied().find(|&k| g.critters[k].as_ref().is_some_and(|c| c.anim_root != 0 && under(part.node as i32, c.anim_root))).unwrap_or(slot.index);
            let Some(v) = g.skin_views.critters.get(&owner) else { continue };
            let Some(Some(oc)) = g.critters.get(owner) else { continue };
            let flash = v.parts.iter().enumerate().any(|(k, &on)| on && oc.colnode_nodes.get(k).and_then(|c| c.1).is_some_and(|r| under(part.node as i32, r)));
            apply(meshes, if flash { &white } else { &v.tree }, &mut req);
        }
    }
}

/// setChrome 0x1bef68 under pb_debug_chrome's defaults (0x2721f0: +0x18,
/// +0x1c, +0x20 set, +0x24 clear): e = camera − object (camera block +0x20,
/// object matrix +0x30), u = e × up (camera block +0x40, (0, 1, 0) from
/// pbInitCamera), v = u × e, taken into the object's frame, normalised and
/// scaled by 0.13333334; the camera block gets atan(e.y / |e.xz|) / π
/// (+0x50) and atan2(e.x, e.z) / 2π (+0x54). Computed in MB space (the
/// render root mirrors x) and returned in render space; the dot product
/// with a world normal equals the object-frame one for rotations.
/// HYPOTHESIS: the offsets pair as (+0x54 → u, +0x50 → v).
fn chrome_axes(eye: Vec3, object: Vec3) -> (Vec4, Vec4) {
    let e = (eye - object) * MIRROR;
    let u = e.cross(Vec3::Y).normalize_or_zero() * f32::from_bits(0x3e08_8889);
    let v = e.cross(Vec3::Y).cross(e).normalize_or_zero() * f32::from_bits(0x3e08_8889);
    let el = (e.y / (e.x * e.x + e.z * e.z).sqrt()).atan() * std::f32::consts::FRAC_1_PI;
    let az = e.x.atan2(e.z) * (0.5 * std::f32::consts::FRAC_1_PI);
    ((u * MIRROR).extend(az), (v * MIRROR).extend(el))
}

/// Realise this frame's material requests (clone on first use, restore the
/// shared material when nothing is requested).
fn apply_material_requests(
    mut commands: Commands,
    mut req: ResMut<MatRequests>,
    mut meshes: Query<(Entity, &mut MeshMaterial3d<gdl_client::GsMaterial>, Option<&MatOverride>, &GlobalTransform)>,
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    let eye = camera.iter().next().map(GlobalTransform::translation);
    for (e, mut m, ov, at) in &mut meshes {
        let r = req.0.remove(&e);
        let base = ov.map_or_else(|| m.0.clone(), |o| o.base.clone());
        let Some(b) = materials.get(&base).cloned() else { continue };
        let (image, overlay, alpha, ambient) = match &r {
            Some(r) => {
                let image = r.replace.clone().or_else(|| b.base.as_ref().and_then(|img| r.swap.iter().find(|(t, _)| *t == img.id()).map(|(_, alt)| alt.clone())));
                (image, r.overlay.clone(), r.alpha.unwrap_or(1.0), r.ambient)
            }
            None => (None, None, 1.0, 0.0),
        };
        if image.is_none() && overlay.is_none() && 0.999 <= alpha && ambient == 0.0 && r.as_ref().is_none_or(|r| r.uv.is_none() && r.color.is_none()) {
            if ov.is_some() {
                m.0 = base;
                commands.entity(e).remove::<MatOverride>();
            }
            continue;
        }
        if ov.is_none() {
            let mut own = b.clone();
            if own.alpha_mode == AlphaMode::Opaque || matches!(own.alpha_mode, AlphaMode::Mask(_)) {
                own.alpha_mode = AlphaMode::Blend;
            }
            m.0 = materials.add(own);
            commands.entity(e).insert(MatOverride { base: base.clone() });
        }
        if let Some(own) = materials.get_mut(&m.0) {
            let replaced = image.is_some();
            let img = image.or_else(|| b.base.clone());
            if own.base != img {
                own.base = img;
            }
            if own.overlay != overlay {
                own.overlay = overlay.clone();
            }
            // The shared material's scene state, then this tree's.
            let mut p = b.params;
            if replaced {
                p.options.x = 1.0;
            }
            p.extra.x = if overlay.is_some() { 1.0 } else { 0.0 };
            p.extra.y = 0.0;
            p.extra.z = 0.0;
            if let Some(x) = r.as_ref().and_then(|r| r.uv) {
                p.uv_xform = x;
                p.extra.z = 1.0;
            }
            if let (Some(eye), true) = (eye, r.as_ref().is_some_and(|r| r.chrome)) {
                (p.chrome_u, p.chrome_v) = chrome_axes(eye, at.translation());
                p.extra.y = 1.0;
            }
            p.tint.w = b.params.tint.w * alpha;
            if let Some(c) = r.as_ref().and_then(|r| r.color) {
                p.tint = (b.params.tint.truncate() * c).extend(p.tint.w);
            }
            if b.lit {
                p.scale.y += ambient;
            } else {
                p.scale.x += ambient;
            }
            if own.params.tint != p.tint || own.params.scale != p.scale || own.params.light != p.light || own.params.light_dir != p.light_dir || own.params.options != p.options || own.params.extra != p.extra || own.params.chrome_u != p.chrome_u || own.params.chrome_v != p.chrome_v || own.params.uv_xform != p.uv_xform {
                own.params = p;
            }
        }
    }
    req.0.clear();
}

/// A type-3 tree node's texture modifier (AtreeNodeInit: the 0x58-byte
/// record at the tree's header[0] base + node +0x34), applied to the node's
/// subtree each frame by DoTexModNode: frame-base ≥ 0 swaps texture record
/// `target` for frame base + clamp((frame − start) / divisor, 0, count − 1)
/// (MBTreeSetAltTex); −4 fades in and −5 out over `count` frames from
/// `start` (DoTexFadeSub → MBTreeSetAlpha). Scrolls (−2/−3) are not ported.
#[derive(Clone)]
struct NodeTexMod {
    subtree: Vec<usize>,
    target: i32,
    frame_base: i32,
    count: i16,
    start: i16,
    divisor: i32,
}

fn tree_texmods(p: &Package, tree: &gdl_formats::animation::Tree) -> Vec<NodeTexMod> {
    let base = tree.file_offset as usize;
    let Some(h0) = p.anim_bytes.get(base..base + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize) else { return Vec::new() };
    let mut out = Vec::new();
    for node in tree.nodes.iter().filter(|n| n.node_type == 3) {
        let at = base + h0 + node.sequence_info_offset as usize;
        let Ok(t) = gdl_formats::animation::texmod_at(&p.anim_bytes, at) else { continue };
        let subtree = tree
            .nodes
            .iter()
            .filter(|n| {
                let mut k = n.index as i32;
                let mut guard = 0;
                while 0 <= k && guard < 256 {
                    if k == node.index as i32 {
                        return true;
                    }
                    k = tree.nodes.get(k as usize).map_or(-1, |m| m.parent);
                    guard += 1;
                }
                false
            })
            .map(|n| n.index as usize)
            .collect();
        out.push(NodeTexMod { subtree, target: t.target, frame_base: t.frame_base, count: t.count, start: t.start, divisor: t.divisor });
    }
    out
}

/// DoTexModNode for the meshes of one tree at `frame`.
/// CalcTexScroll 0x110988(t, divisor, count, frame) → (add, scale): with
/// min(count, divisor) = 0 the add is (frame mod count) / count and the
/// scale 0 (f0 stays cleared, as the instructions show); otherwise a wipe
/// over the divisor's frames around `count`.
fn calc_tex_scroll(t: f32, p2: f32, p3: f32, frame: i32) -> (f32, f32) {
    let m = if p3 >= p2 { p2 } else { p3 };
    if m == 0.0 {
        let n = p3 as i32;
        if n == 0 {
            return (0.0, 0.0);
        }
        return ((frame % n) as f32 / p3, 0.0);
    }
    let (add, scale);
    if t <= 0.0 {
        add = 0.0;
        scale = 0.0;
    } else if t < m {
        let q = p3 * (1.0 / m);
        let a = -q;
        let a = a + a;
        add = a + (t - 0.0) * (1.0 / m) * ((1.0 - q) - a);
        scale = 1.0;
    } else if p3 < p2 {
        return (0.0, 1.0);
    } else if t < p3 {
        let q = p3 * (1.0 / m);
        let g = (t - m) / (p3 - m);
        let a = 1.0 - q;
        add = a + g * (0.0 - a);
        scale = g * (q - 1.0) + 1.0;
    } else {
        add = if t < p3 + m { (t - p3) * (1.0 / m) + 0.0 } else { 1.0 };
        scale = p3 * (1.0 / m);
    }
    (add, scale - add)
}

fn request_tree_texmods(mods: &[NodeTexMod], lib: &Library, frame: f32, meshes: &[(Entity, usize)], req: &mut MatRequests) {
    let f = (frame + 0.5) as i32;
    for m in mods {
        let d = f - i32::from(m.start);
        match m.frame_base {
            -4 | -5 => {
                let (t, c) = (d as f32, f32::from(m.count));
                let mut v = 0.0;
                if 0.0 < t && 0.0 < c {
                    v = if t < c { t / c } else { 1.0 };
                }
                // MBTreeSetAlpha(255 · v) for −5, 255 · (1 − v) for −4.
                let opacity = if m.frame_base == -4 { v } else { 1.0 - v };
                for &(e, n) in meshes {
                    if m.subtree.contains(&n) {
                        req.alpha(e, opacity);
                    }
                }
            }
            -2 | -3 => {
                // DoTexScrollSub: CalcTexScroll(frame − start, divisor,
                // count, frame) → MBTreeSetUVScaleAdd on U (−2) or V (−3).
                let (add, scale) = calc_tex_scroll(d as f32, m.divisor as f32, f32::from(m.count), f);
                let x = if m.frame_base == -2 { Vec4::new(scale, add, 1.0, 0.0) } else { Vec4::new(1.0, 0.0, scale, add) };
                for &(e, n) in meshes {
                    if m.subtree.contains(&n) {
                        req.get(e).uv = Some(x);
                    }
                }
            }
            b if 0 <= b => {
                let mut k = d.max(0);
                if 0 <= d {
                    if 0 < m.divisor {
                        k = d / m.divisor;
                    }
                    if i32::from(m.count) <= k {
                        k = i32::from(m.count) - 1;
                    }
                }
                let (Ok(t), Ok(a)) = (u16::try_from(m.target), u16::try_from(b + k)) else { continue };
                if lib.unresolved.contains(&a) {
                    continue;
                }
                let (Some(target), Some(alt)) = (lib.record_images.get(&t), lib.record_images.get(&a)) else { continue };
                for &(e, n) in meshes {
                    if m.subtree.contains(&n) {
                        req.swap(e, target.id(), alt.clone());
                    }
                }
            }
            _ => {}
        }
    }
}

/// InitLighting / DoLighting: Ambient = LEVL +0xec × GlobalInten (1.0),
/// one directional light (MBAddLight: intensity +0x108 × colour +0xfc,
/// direction +0xf0, normalised). Lit materials get the light towards which
/// normals shade (−direction, mirrored into render space).
fn present_scene_light(sim: Res<Sim>, mut last: Local<Option<([f32; 8], usize)>>, mut materials: ResMut<Assets<gdl_client::GsMaterial>>) {
    let g = &sim.game;
    if g.level.len() < 0x10c {
        return;
    }
    // DoLighting: MBSetAmbient(clamp(Ambient · GlobalInten + special)) and
    // MBLightmapMod(1, special) for unlit draws.
    let ambient = g.lighting.ambient_out;
    let add = g.lighting.lightmap_add;
    let dir = Vec3::new(g.level_f32(0xf0), g.level_f32(0xf4), g.level_f32(0xf8));
    let colour = Vec3::new(g.level_f32(0xfc), g.level_f32(0x100), g.level_f32(0x104)) * g.level_f32(0x108);
    let towards = if dir.length() > 0.0 { -dir.normalize() * MIRROR } else { Vec3::Y };
    let key = ([ambient, towards.x, towards.y, towards.z, colour.x, colour.y, colour.z, add], materials.len());
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    for (_, m) in materials.iter_mut() {
        if m.lit {
            m.params.light_dir = towards.extend(1.0);
            m.params.light = colour.extend(ambient);
        } else {
            m.params.scale.x = add;
        }
    }
}

/// ProcessEffects' MBPosLightNew for effects with an SfxSetLight radius
/// (slot +0x10 × sfx_lightradmul 1.0, attenuation sfx_lightattn 2.0, at the
/// effect + 1 up, colour slot +0..+8) and game code's lights (do_players'
/// dark-level light), nearest 8 to the camera, on every material
/// (pbDrawObject runs pbSetupPosLights for each object). HYPOTHESIS: the
/// life fade of the radius is not applied.
fn present_point_lights(sim: Res<Sim>, mut last: Local<Vec<[f32; 8]>>, mut materials: ResMut<Assets<gdl_client::GsMaterial>>) {
    let g = &sim.game;
    let mut lights: Vec<[f32; 8]> = Vec::new();
    for (i, n) in g.effects.nodes.iter().enumerate() {
        let Some(n) = n else { continue };
        let r = &g.effects.slots[i];
        let radius = r.f32_(0x10);
        if radius <= 0.0 || r.f32_(0x68) <= 0.0 {
            continue;
        }
        let p = Vec3::new(n.position[0], n.position[1] + 1.0, n.position[2]) * MIRROR;
        let c = r.v3(0);
        lights.push([p.x, p.y, p.z, radius, c[0], c[1], c[2], 2.0]);
    }
    // Game code's MBPosLightNew (do_players' dark-level light).
    for l in &g.lighting.pos_lights {
        let p = Vec3::from_array(l.position) * MIRROR;
        lights.push([p.x, p.y, p.z, l.radius, l.color[0], l.color[1], l.color[2], l.attenuation]);
    }
    if let Some(cam) = &g.std_camera {
        let eye = Vec3::from_array(cam.pb.eye) * MIRROR;
        lights.sort_by(|a, b| (Vec3::new(a[0], a[1], a[2]) - eye).length_squared().total_cmp(&(Vec3::new(b[0], b[1], b[2]) - eye).length_squared()));
    }
    lights.truncate(8);
    if *last == lights {
        return;
    }
    *last = lights.clone();
    let mut pos = [Vec4::ZERO; 8];
    let mut col = [Vec4::ZERO; 8];
    for (k, l) in lights.iter().enumerate() {
        pos[k] = Vec4::new(l[0], l[1], l[2], l[3]);
        col[k] = Vec4::new(l[4], l[5], l[6], l[7]);
    }
    let stale: Vec<_> = materials.iter().filter(|(_, m)| m.params.plight_pos != pos || m.params.plight_col != col).map(|(id, _)| id).collect();
    for id in stale {
        if let Some(m) = materials.get_mut(id) {
            m.params.plight_pos = pos;
            m.params.plight_col = col;
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
    let g = &sim.game;
    for (e, n, chain, mut vis) in &mut nodes {
        let off = objs.get(n.node).is_some_and(|o| o.flags_10 & 0x1000_0000 != 0);
        if off && *vis != Visibility::Hidden {
            *vis = Visibility::Hidden;
        } else if !off && was.get(&e).copied().unwrap_or(false) {
            *vis = if node_hidden(&chain.0, &g.node_mb_set, &g.node_mb_clear) { Visibility::Hidden } else { Visibility::Inherited };
        }
        was.insert(e, off);
    }
}

/// Per sequence: the OANIM start frame and the frame objects' labels.
type SeqFrames = Vec<(i16, Vec<String>)>;
/// Per monster package: tree label → per-sequence OANIM frames.
#[derive(Resource, Default)]
struct EnemyFrames(HashMap<String, HashMap<String, SeqFrames>>);

#[derive(Resource)]
struct Sim {
    game: Game,
    error: Option<String>,
    updates: u64,
    exp: i64,
    sparks: Vec<(Vec3, f32)>,
    last_event: String,
    /// ScrollMessageBox: the game waits until the box is dismissed.
    /// ScrollMessageBox (modal; the simulation waits).
    message_box: Option<gdl_core::game::hud::ScrollBox>,
    scroll_clock: f32,
    /// Sumner's hint pad asked for hint_menu.
    hint_menu: bool,
    /// game mode 0x4012 between a realm and the Tower (init_shop/do_shop),
    /// the Tower level it leads to, and do_shop's result once finished.
    shop: Option<(gdl_core::game::shopflow::ShopFlow, usize)>,
    /// game mode 0x400f before the next level of a realm (init_mapscreen).
    map: Option<(gdl_core::game::mapscreen::MapScreen, usize)>,
    /// F9 (DEBUG): the pending exit skips game_main's level end.
    debug_skip: bool,
    /// game mode 0x400b: the select screen, and its end (Done: the Tower;
    /// abort: the title).
    select: Option<gdl_core::game::pselect::PSelect>,
    select_done: bool,
    select_quit: bool,
    /// The level init_next_level enters after the select screen.
    select_next: Option<usize>,
    /// The level to load once the shop finished (after the select screen).
    shop_done: Option<usize>,
    /// The level the map screen leads to (init_gamemovie, init_next_level).
    map_done: Option<usize>,
    /// game mode 0x400e: an in-game movie and where play goes on after it.
    movie: Option<MovieRun>,
    movie_done: Option<MovieThen>,
    /// game mode 0x4014 (init_over / do_over) and its end.
    over: Option<gdl_core::game::movie::GameOver>,
    over_done: bool,
    /// game mode 0x8008: the attract flyby and its end (exit_attract).
    flyby: Option<gdl_core::game::flyby::Flyby>,
    flyby_done: bool,
}

/// The full-screen movie frame (HUD layer, above the other blits).
#[derive(Component)]
struct MovieQuad;

/// The assets a movie frame touches: images, blit materials, meshes.
type MovieAssets<'w> = (ResMut<'w, Assets<Image>>, ResMut<'w, Assets<gdl_client::BlitMaterial>>, ResMut<'w, Assets<Mesh>>);

/// The movie frame's image (shared by the in-game and attract movies).
#[derive(Resource, Default)]
struct MovieScreen {
    image: Option<Handle<Image>>,
    material: Option<Handle<gdl_client::BlitMaterial>>,
}

/// Show `frame` (640×480 RGBA, GS alpha forced to 0x80) on the full-screen
/// movie quad, creating the quad and its image on first use.
#[allow(clippy::too_many_arguments)]
fn show_movie_frame(
    commands: &mut Commands,
    screen: &mut MovieScreen,
    images: &mut Assets<Image>,
    mats: &mut Assets<gdl_client::BlitMaterial>,
    meshes: &mut Assets<Mesh>,
    quad: &mut Query<&mut Visibility, With<MovieQuad>>,
    frame: Option<Vec<u8>>,
) {
    let img = screen
        .image
        .get_or_insert_with(|| {
            let size = (gdl_client::movie::WIDTH * gdl_client::movie::HEIGHT * 4) as usize;
            let mut im = Image::new(
                bevy::render::render_resource::Extent3d { width: gdl_client::movie::WIDTH, height: gdl_client::movie::HEIGHT, depth_or_array_layers: 1 },
                bevy::render::render_resource::TextureDimension::D2,
                [0, 0, 0, 0x80].repeat(size / 4),
                bevy::render::render_resource::TextureFormat::Rgba8Unorm,
                bevy::asset::RenderAssetUsages::default(),
            );
            im.sampler = bevy::image::ImageSampler::linear();
            images.add(im)
        })
        .clone();
    if let Some(mut f) = frame {
        if std::env::var_os("GDL_DEBUG_ATTRACT").is_some() {
            let sum: u64 = f.iter().step_by(4).map(|&v| u64::from(v)).sum();
            eprintln!("MOVIE frame mean red {}", sum / (f.len() as u64 / 4).max(1));
        }
        for px in f.chunks_exact_mut(4) {
            px[3] = 0x80;
        }
        if let Some(im) = images.get_mut(&img) {
            im.data = Some(f);
        }
        // The material's bind group holds the texture it was prepared with:
        // mark it changed so the new frame is bound.
        if let Some(m) = screen.material.as_ref() {
            let _ = mats.get_mut(m);
        }
    }
    if quad.is_empty() {
        let params = gdl_client::BlitParams { rgba: Vec4::ONE, crop: Vec4::new(0.0, 1.0, 0.0, 1.0), mask_rect: Vec4::ZERO };
        let mat = mats.add(gdl_client::BlitMaterial { params, texture: Some(img), mask: None });
        screen.material = Some(mat.clone());
        commands.spawn((
            Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
            MeshMaterial2d(mat),
            Transform::from_xyz(0.0, 0.0, 50.0).with_scale(Vec3::new(512.0, 384.0, 1.0)),
            Visibility::Visible,
            bevy::camera::visibility::RenderLayers::layer(HUD_LAYER),
            MovieQuad,
        ));
    } else {
        for mut v in quad.iter_mut() {
            *v = Visibility::Visible;
        }
    }
}

fn hide_movie(quad: &mut Query<&mut Visibility, With<MovieQuad>>) {
    for mut v in quad.iter_mut() {
        *v = Visibility::Hidden;
    }
}

/// InitMovie / ServeMovie / KillMovie on the host for init_gamemovie: on a
/// movie's first frame the decoder starts (gdl_client::movie; a missing
/// file or decoder ends the movie as a failed InitMovie does) and its
/// soundtrack plays on the music channel; each frame shows the newest due
/// video frame over the whole screen; afterwards the quad hides and the
/// soundtrack stops.
#[allow(clippy::too_many_arguments)]
fn present_movie(
    mut commands: Commands,
    time: Res<Time>,
    mut sim: ResMut<Sim>,
    front: Res<Front>,
    mut screen: ResMut<MovieScreen>,
    (mut images, mut mats, mut meshes): MovieAssets,
    mut quad: Query<&mut Visibility, With<MovieQuad>>,
    mut audio: Option<NonSendMut<gdl_client::audio::AudioOut>>,
    mut showing: Local<bool>,
) {
    let mut note = None;
    let Some(run) = sim.movie.as_mut() else {
        if std::mem::take(&mut *showing) {
            hide_movie(&mut quad);
            if let Some(a) = audio.as_mut() {
                a.stream_stop();
            }
        }
        return;
    };
    if !run.tried {
        run.tried = true;
        if let Some(req) = &run.game.request {
            let key = format!("{}/{}", req.dir, req.name.to_uppercase());
            match front.base.movies.get(&key) {
                Some(path) => match gdl_client::movie::MoviePlayer::start(path) {
                    Ok(p) => run.player = Some(p),
                    Err(e) => note = Some(format!("movie {key}: {e}")),
                },
                None => note = Some(format!("movie {key} is not on the disc")),
            }
        }
        if let (Some(p), Some(a)) = (run.player.as_mut(), audio.as_mut())
            && let Some((pcm, rate, ch)) = p.audio.take()
        {
            a.play_pcm(pcm, rate, ch);
        }
    }
    let frame = run.player.as_mut().and_then(|p| p.advance(f64::from(time.delta_secs())));
    if let Some(n) = note {
        sim.last_event = n;
    }
    show_movie_frame(&mut commands, &mut screen, &mut images, &mut mats, &mut meshes, &mut quad, frame);
    *showing = true;
}

/// joyBeginShake / joyEndShake on the host: the large motor of the port's
/// gamepad (port k is the k-th gamepad, as player_input reads them; the
/// keyboard ports have no actuator). A new game (ResetModels'
/// vibrators_off at a level change) stops every motor.
fn apply_rumble(sim: Option<ResMut<Sim>>, gamepads: Query<Entity, With<Gamepad>>, mut out: MessageWriter<bevy::input::gamepad::GamepadRumbleRequest>) {
    use bevy::input::gamepad::{GamepadRumbleIntensity, GamepadRumbleRequest};
    let Some(mut sim) = sim else { return };
    let mut pads: Vec<Entity> = gamepads.iter().collect();
    pads.sort();
    if sim.is_added() {
        for &g in &pads {
            out.write(GamepadRumbleRequest::Stop { gamepad: g });
        }
    }
    for (port, intensity) in std::mem::take(&mut sim.game.rumble) {
        let Some(&gamepad) = usize::try_from(port).ok().and_then(|k| pads.get(k)) else { continue };
        out.write(GamepadRumbleRequest::Stop { gamepad });
        if let Some(i) = intensity {
            // The motor runs until joyEndShake; the duration only bounds it.
            out.write(GamepadRumbleRequest::Add { gamepad, intensity: GamepadRumbleIntensity::strong_motor(i), duration: std::time::Duration::from_secs(10) });
        }
    }
}

/// init_attract_mode(param) and the entry it starts. The flyby's level
/// (attract_wave) is built by front_system; a wave without a configured
/// level is passed over. The title screen is the front end's Press Start
/// page (init_titlescreen and its timer 0x708).
fn start_attract(front: &mut Front, param: i32) {
    use gdl_core::game::attract::AttractEntry;
    let mut p = param;
    for _ in 0..32 {
        let entry = front.attract.init_attract_mode(p, false);
        p = -1;
        front.attract_clock = 0.0;
        front.attract_run = match entry {
            AttractEntry::Flyby { wave } => match front.base.levels.iter().position(|l| l.world == wave >> 8 && l.index as i32 == wave & 0xff) {
                Some(i) => {
                    front.flyby_request = Some(i);
                    Some(AttractRun::Flyby)
                }
                None => continue,
            },
            AttractEntry::Titlescreen => {
                front.menu.page = gdl_client::menu::Page::PressStart;
                front.title_timeout = 1800.0;
                None
            }
            AttractEntry::Screen2D { table: 0 } => Some(AttractRun::Card(Default::default())),
            AttractEntry::Screen2D { .. } => Some(AttractRun::Group { left: 60 }),
            AttractEntry::Movie { name } => Some(AttractRun::Movie { name, player: None, tried: false }),
            AttractEntry::Credits => {
                front.credits.scroll = 0;
                front.credits.timer = 0;
                Some(AttractRun::Credits)
            }
        };
        return;
    }
}

/// ExitAttract / init_titlescreen: Start (or accept) during an attract
/// entry goes to the title screen.
fn exit_attract(front: &mut Front) {
    front.attract_run = None;
    front.attract_blits.clear();
    front.menu.page = gdl_client::menu::Page::PressStart;
    front.title_timeout = 1800.0;
}

/// The attract entries per frame at 30 Hz (SetMaxFPS(0x1e), vb_elapsed 2),
/// and the title screen's timeout back into the sequence.
#[allow(clippy::too_many_arguments)]
fn run_attract(
    mut commands: Commands,
    time: Res<Time>,
    screen_mode: Res<Screen>,
    mut front: ResMut<Front>,
    mut screen: ResMut<MovieScreen>,
    (mut images, mut mats, mut meshes): MovieAssets,
    mut quad: Query<&mut Visibility, With<MovieQuad>>,
    mut audio: Option<NonSendMut<gdl_client::audio::AudioOut>>,
    mut last: Local<(bool, bool)>,
) {
    let dt = time.delta_secs();
    if *screen_mode != Screen::Front {
        return;
    }
    let front = &mut *front;
    let movie_now = matches!(front.attract_run, Some(AttractRun::Movie { .. }));
    let credits_now = matches!(front.attract_run, Some(AttractRun::Credits));
    // Leaving a movie or the credits: the quad hides, the sound stops.
    if last.0 && !movie_now {
        hide_movie(&mut quad);
        if let Some(a) = audio.as_mut() {
            a.stream_stop();
        }
    }
    if last.1 && !credits_now
        && let Some(a) = audio.as_mut()
    {
        a.stream_stop();
    }
    if credits_now && !last.1
        && let Some(a) = audio.as_mut()
    {
        // AudioStreamStart("castle2b", 1).
        a.stream("CASTLE2B", true, 100);
    }
    *last = (movie_now, credits_now);
    if front.attract_run.is_none() {
        front.attract_blits.clear();
        if matches!(front.menu.page, gdl_client::menu::Page::PressStart) {
            front.title_timeout -= dt * 59.94;
            if front.title_timeout <= 0.0 {
                // do_titlescreen: the timeout → init_attract_mode(-1).
                start_attract(front, -1);
            }
        }
        return;
    }
    front.attract_clock += dt * 30.0;
    let steps = front.attract_clock.floor() as i32;
    front.attract_clock -= steps as f32;
    let vb = 2 * steps;
    let mut next = false;
    match front.attract_run.as_mut() {
        Some(AttractRun::Card(check)) => {
            front.attract_blits = front.ui_hud.screen_blits("SONY_SCREEN");
            // get_card_state: the native card is port 0 slot 0, formatted
            // with room (decisions: host saves).
            let card = |p: i32, s: i32| if (p, s) == (0, 0) { (2, true) } else { (0, false) };
            for _ in 0..steps {
                match check.step(&card) {
                    Ok(true) => {
                        front.attract.did_mc_check = 2;
                        next = true;
                        break;
                    }
                    Ok(false) => {}
                    Err(e) => {
                        front.menu.status = e;
                        next = true;
                        break;
                    }
                }
            }
        }
        Some(AttractRun::Group { left }) => {
            // do_screen2d (tables 1/2): the group shot and
            // DrawGlowTextMLines(1.0, −0x1b0, −0x140, "Press\nStart").
            front.attract_blits = front.ui_hud.screen_blits("GROUP_SHOT_MIDWAY");
            front.title_clock += vb as f32;
            let glow = front.ui_hud.glow_text_lines(1.0, -0x1b0, -0x140, "Press\nStart", front.title_clock as u32);
            front.attract_blits.extend(glow);
            *left -= vb;
            next = *left < 1;
        }
        Some(AttractRun::Movie { name, player, tried }) => {
            front.attract_blits.clear();
            if !*tried {
                *tried = true;
                let key = format!("MPEG2/{}", name.to_uppercase());
                match front.base.movies.get(&key).map(|p| gdl_client::movie::MoviePlayer::start(p)) {
                    Some(Ok(p)) => *player = Some(p),
                    Some(Err(e)) => front.menu.status = format!("movie {key}: {e}"),
                    None => front.menu.status = format!("movie {key} is not on the disc"),
                }
                if let (Some(p), Some(a)) = (player.as_mut(), audio.as_mut())
                    && let Some((pcm, rate, ch)) = p.audio.take()
                {
                    a.play_pcm(pcm, rate, ch);
                }
            }
            match player.as_mut() {
                Some(p) if p.playing() => {
                    let frame = p.advance(f64::from(dt));
                    show_movie_frame(&mut commands, &mut screen, &mut images, &mut mats, &mut meshes, &mut quad, frame);
                }
                _ => next = true,
            }
        }
        Some(AttractRun::Credits) => {
            let (blits, done) = front.ui_hud.credits_frame(&mut front.credits, vb);
            front.attract_blits = blits;
            front.credits.timer += vb;
            next = done || 0x1517 < front.credits.timer;
        }
        Some(AttractRun::Flyby) | None => {}
    }
    if std::env::var_os("GDL_DEBUG_ATTRACT").is_some() {
        let kind = match &front.attract_run {
            Some(AttractRun::Card(_)) => "card".to_string(),
            Some(AttractRun::Group { left }) => format!("group {left}"),
            Some(AttractRun::Movie { name, player, .. }) => format!("movie {name} {}", player.as_ref().map_or("none", |p| if p.playing() { "playing" } else { "ended" })),
            Some(AttractRun::Credits) => format!("credits {}", front.credits.scroll),
            Some(AttractRun::Flyby) => "flyby".into(),
            None => "title".into(),
        };
        let b = front.attract_blits.last().map(|b| (b.tex, b.x, b.y, b.w, b.h, b.rgba, b.z, b.crop));
        eprintln!("ATTRACT {kind} blits {} first {b:?} next {next}", front.attract_blits.len());
    }
    if next {
        start_attract(front, -1);
    }
}

/// init_gamemovie's movie with the host's decoder (started by
/// present_movie on its first frame; None once it could not start).
struct MovieRun {
    game: gdl_core::game::movie::GameMovie,
    player: Option<gdl_client::movie::MoviePlayer>,
    tried: bool,
    then: MovieThen,
}

/// do_gamemovie's result: movietype 1 → init_next_level(0) of the level,
/// 2 → init_shop (then the Tower).
#[derive(Clone, Copy)]
enum MovieThen {
    Level(usize),
    Shop(usize),
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
            // NATIVE: R2 (entry 9: the team move, PlayerControl 0x20000).
            press(&mut s, 9, k(KeyCode::KeyX));
        }
        1 => {
            press(&mut s, 0, k(KeyCode::KeyF));
            press(&mut s, 1, k(KeyCode::KeyH));
            press(&mut s, 2, k(KeyCode::KeyT));
            press(&mut s, 3, k(KeyCode::KeyG));
            press(&mut s, 0xd, k(KeyCode::KeyR));
            press(&mut s, 0xb, k(KeyCode::KeyY));
            press(&mut s, 0x14, k(KeyCode::KeyU));
            press(&mut s, 9, k(KeyCode::KeyB));
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
    let mut index_of = HashMap::new();
    for d in &pkg.objects.definitions {
        index_of.entry(d.label.to_uppercase()).or_insert(d.object_index as u32);
        if let Some(list) = parts.get(&(d.object_index as u32)) {
            by_label.entry(d.label.clone()).or_insert_with(|| list.clone());
        }
    }
    let texmods = gdl_formats::animation::texmods(&pkg.anim_bytes).unwrap_or_default();
    let unresolved = set.unresolved.iter().map(|&i| i as u16).collect();
    let (mut textures, mut label_index) = (set.labels, set.label_index);
    // A texmod target without TEXDEF texels (record flags 0x20, e.g. the SFX
    // packages' REDWEAPTEX, filled from WEAPONS' frames) is still found by
    // its name (+0x04): MBOX_FindTexture for particle textures.
    for tm in &texmods {
        let Ok(t) = u16::try_from(tm.target) else { continue };
        let name = tm.target_name.to_uppercase();
        if name.is_empty() {
            continue;
        }
        if let Some(img) = set.record_images.get(&t) {
            textures.entry(name.clone()).or_insert_with(|| img.clone());
        }
        label_index.entry(name).or_insert(t);
    }
    Library { by_label, textures, record_images: set.record_images, label_index, texmods, by_index: parts, index_of, unresolved }
}

/// The OANIM frames of a package's trees (InitOAnimList / DoObjAnimation):
/// per sequence the start frame and the object labels of the consecutive
/// object indices from the entry's named object (first type-2 node).
fn enemy_frames(pkg: &Package) -> HashMap<String, Vec<(i16, Vec<String>)>> {
    let mut out = HashMap::new();
    let Some(anim) = &pkg.anim else { return out };
    let mut by_index: HashMap<i16, String> = HashMap::new();
    let mut index_of: HashMap<String, i16> = HashMap::new();
    for d in &pkg.objects.definitions {
        by_index.entry(d.object_index).or_insert_with(|| d.label.clone());
        index_of.entry(d.label.to_uppercase()).or_insert(d.object_index);
    }
    for tree in &anim.trees {
        let Some(node) = tree.nodes.iter().find(|n| n.node_type == 2) else { continue };
        let seqs = node
            .oanim
            .iter()
            .map(|e| {
                let base = index_of.get(&e.name.to_uppercase()).copied();
                let labels = base.map_or(Vec::new(), |b| (0..e.count.max(1)).filter_map(|f| by_index.get(&(b + f)).cloned()).collect());
                (e.start, labels)
            })
            .collect();
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
        // GDL_NO_DITHER=1: present without the GS frame's 16-bit quantisation.
        gdl_client::DisplayToLinear { enabled: 1.0, offset_x: 0.0, offset_y: 0.0, dither: if std::env::var_os("GDL_NO_DITHER").is_some() { 0.0 } else { 1.0 } },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::Fixed { width: 512.0, height: 384.0 },
            ..OrthographicProjection::default_2d()
        }),
        bevy::camera::visibility::RenderLayers::layer(HUD_LAYER),
    ));
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

/// player_rhand[class] (ELF table of label pointers): the hand node the
/// held weapon hangs from (load_player).
fn player_rhand(elf_bytes: &[u8], class: u32) -> Option<String> {
    let elf = gdl_formats::elf::Elf::parse(elf_bytes).ok()?;
    let table = elf.object("player_rhand").ok()?;
    let p = u32::from_le_bytes(table.get(class as usize * 4..class as usize * 4 + 4)?.try_into().ok()?);
    let s = elf.read(p, 16).ok()?;
    Some(s.iter().take_while(|b| **b != 0).map(|b| char::from(*b)).collect())
}

/// A special texture modifier (AddSpecialTexmod 0x111180 / DoSpecialTexmods
/// 0x1112b0): the mesh's texture becomes `base` + k of `package`, k
/// stepping 0..frames every `rate` AFrames.
#[derive(Component)]
struct SpecialTexmod {
    package: String,
    base: String,
    frames: u32,
    rate: u32,
}

/// PlayerProcessPowerups' held-weapon effect (+0x6bc): with a weapon
/// powerup (+0x130 & 0xf), ReplaceTree of WeapHoldFxTree[player][type] —
/// the SFX package's "WEAP_HOLD_<DmgTypeDesc>" — under the right-hand node
/// (+0x6a8) at PlayerData +0x68 + tier × 12, scaled by +0xe0 + tier × 12
/// when its x is non-zero (tier = level / 10); deleted without one. The
/// trees are static (no frames on the disc).
#[derive(Component)]
struct HoldFx {
    slot: usize,
    key: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn present_hold_fx(
    mut commands: Commands,
    sim: Res<Sim>,
    sources: Res<Sources>,
    libs: Res<Libraries>,
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
    mut variants: ResMut<PartVariants>,
    mut pending: ResMut<PendingTreePsys>,
    mut q: Query<(Entity, &mut HoldFx, &mut Visibility)>,
) {
    let g = &sim.game;
    for (e, mut h, mut vis) in &mut q {
        let r = &g.players[h.slot];
        let t = (r.u32_(0x130) & 0xf) as usize;
        let tier = (r.i32_(0x2ac0) / 10).clamp(0, 9) as usize;
        let label = g.effects.tables.dmg_type_desc.get(t).filter(|d| !d.is_empty()).map(|d| format!("WEAP_HOLD_{d}"));
        let key = label.as_ref().map(|l| format!("{l}:{tier}"));
        if h.key != key {
            commands.entity(e).despawn_related::<Children>();
            if let (Some(label), Some(pc)) = (&label, sources.players[h.slot].as_ref()) {
                let raw = &g.classes[h.slot].raw;
                let f = |o: usize| raw.get(o..o + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
                let at = Vec3::new(f(0x68 + 12 * tier), f(0x6c + 12 * tier), f(0x70 + 12 * tier));
                let sc = Vec3::new(f(0xe0 + 12 * tier), f(0xe4 + 12 * tier), f(0xe8 + 12 * tier));
                let tf = Transform { translation: at, scale: if sc.x != 0.0 { sc } else { Vec3::ONE }, ..default() };
                let holder = commands.spawn((tf, Visibility::Inherited)).id();
                commands.entity(e).add_child(holder);
                let parts = tree_parts(&sources, &libs, &pc.sfx_name, label);
                if std::env::var("GDL_DEBUG_FX").is_ok() {
                    eprintln!("HoldFx {} {label} in {}: {} parts at {at:?} scale {sc:?}", h.slot, pc.sfx_name, parts.len());
                }
                spawn_parts(&mut commands, holder, parts, &mut materials, &mut variants, &mut pending);
            }
            h.key = key;
        }
        *vis = if h.key.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    }
}

fn present_special_texmods(sim: Res<Sim>, libs: Res<Libraries>, q: Query<(Entity, &SpecialTexmod)>, mut req: ResMut<MatRequests>, mut step: Local<(u64, u32)>) {
    // AFrame: one step per game update (DoSpecialTexmods runs per frame).
    let aframe = sim.updates;
    for (e, t) in &q {
        if step.0 != aframe {
            *step = (aframe, if aframe.is_multiple_of(u64::from(t.rate.max(1))) { (step.1 + 1) % t.frames.max(1) } else { step.1 });
        }
        let Some(lib) = libs.0.get(&t.package) else { continue };
        let Some(&base) = lib.label_index.get(&t.base.to_uppercase()) else { continue };
        if let Some(img) = lib.record_images.get(&(base + step.1 as u16)) {
            req.get(e).replace = Some(img.clone());
        }
    }
}

/// The ground shadow object of a player slot (load_player: SHADOWL1, MB
/// flags 0x880, z modifier −36; UpdatePlayerShadow places it).
#[derive(Component)]
struct PlayerShadow(usize);

fn spawn_bodies(commands: &mut Commands, root: Entity, sources: &Sources, session: &Session, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>, materials: &mut Assets<gdl_client::GsMaterial>) {
    for slot in 0..4 {
        if let Some(pc) = &sources.players[slot] {
            spawn_body(commands, root, slot, pc, session, meshes, images, materials);
        }
    }
}

/// load_player's body for one slot: the model's rig nodes posed by the
/// base class's clip nodes, WEAP_HOLD under the right hand, SHADOWL1.
#[allow(clippy::too_many_arguments)]
fn spawn_body(commands: &mut Commands, root: Entity, slot: usize, pc: &PlayerChar, session: &Session, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>, materials: &mut Assets<gdl_client::GsMaterial>) {
    let character = &pc.character;
    let set = gdl_client::materials_for(&character.objects, &character.table, &character.textures, images, materials);
    let (parts, _) = gdl_client::object_meshes(&character.objects, &set, materials, meshes);
    let object_parts = |label: &str| -> Vec<(Handle<Mesh>, Handle<gdl_client::GsMaterial>)> {
        character.objects.definitions.iter().filter(|d| d.label == label).flat_map(|d| parts.get(&(d.object_index as u32)).cloned().unwrap_or_default()).collect()
    };
    let rhand = player_rhand(&session.elf_bytes, pc.key.class);
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
    let vis = if slot == 0 { Visibility::default() } else { Visibility::Hidden };
    let body = commands.spawn((CharacterRoot(slot), Transform::default(), vis)).id();
    commands.entity(root).add_child(body);
    let tree = &pc.rig.trees[0];
    // Rig nodes pose by the class clip node of the same label (the root,
    // "UNNAMED" in the model, is clip node 0).
    let clip_nodes = &pc.clips.trees[0].nodes;
    for node in &tree.nodes {
        let pose = if node.index == 0 { Some(0) } else { clip_nodes.iter().position(|c| c.label == node.label) };
        let Some(pose) = pose else { continue };
        let label: String = (tree.prefix.clone() + &node.label).chars().take(16).collect();
        for def in character.objects.definitions.iter().filter(|d| d.label == label) {
            for (mesh, material) in parts.get(&(def.object_index as u32)).into_iter().flatten() {
                let child = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), RigNode(pose, slot))).id();
                commands.entity(body).add_child(child);
            }
        }
        if rhand.as_deref() == Some(node.label.as_str()) {
            let hold = commands.spawn((HoldFx { slot, key: None }, RigNode(pose, slot), Transform::default(), Visibility::Hidden)).id();
            commands.entity(body).add_child(hold);
            for (mesh, material) in &weapon {
                let child = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), RigNode(pose, slot))).id();
                // load_player: the Jester's (class & 7 == 7) held bomb gets
                // AddSpecialTexmod(model, "BOMB", SFX model, "BOMB00", 5, 1).
                if pc.key.class & 7 == 7 {
                    commands.entity(child).insert(SpecialTexmod { package: pc.sfx_name.clone(), base: "BOMB00".into(), frames: 5, rate: 1 });
                }
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

/// change_player's replace_player_geo on the select screen: a player whose
/// class, colour, tier or descriptor no longer matches its loaded
/// character gets the new model, and the game its class clips and data
/// (level loads do the same in advance_level).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh_characters(
    mut commands: Commands,
    front: Res<Front>,
    mut sim: ResMut<Sim>,
    mut sources: ResMut<Sources>,
    session: Res<Session>,
    mut libraries: ResMut<Libraries>,
    (mut meshes, mut images, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<Image>>, ResMut<Assets<gdl_client::GsMaterial>>),
    (root, bodies, shadows): (Query<Entity, With<MirrorRoot>>, Query<(Entity, &CharacterRoot)>, Query<(Entity, &PlayerShadow)>),
    mut failed: Local<Vec<(usize, CharKey)>>,
) {
    if sim.select.is_none() {
        return;
    }
    for k in 0..4 {
        let r = &sim.game.players[k];
        if k != 0 && r.i32_(0xfc) == 0 {
            continue;
        }
        let key = CharKey::of(r);
        if sources.players[k].as_ref().map(|p| p.key) == Some(key) || failed.contains(&(k, key)) {
            continue;
        }
        if let Err(e) = install_char(&front, &mut sources, &mut libraries, (&mut meshes, &mut images, &mut materials), k, key) {
            sim.last_event = format!("player {} model: {e}", k + 1);
            failed.push((k, key));
            continue;
        }
        let Some(pc) = sources.players[k].as_ref() else { continue };
        sim.game.player_clips[k] = setup::clips_of(&pc.clips, 0);
        if let Ok(c) = class_data_of(&front, key.class) {
            sim.game.classes[k] = with_sfx_trees(c, &sources, k);
        }
        for (e, b) in &bodies {
            if b.0 == k {
                commands.entity(e).despawn();
            }
        }
        for (e, sh) in &shadows {
            if sh.0 == k {
                commands.entity(e).despawn();
            }
        }
        if let Ok(root) = root.single() {
            spawn_body(&mut commands, root, k, pc, &session, &mut meshes, &mut images, &mut materials);
        }
    }
}

/// UpdatePlayerShadow 0x147fa0: the shadow at the player's x/z and the
/// higher of floor (+0x88c) and water (+0x8a4) heights + 0.1; above water
/// MBTreeSetAltTex(−2, TextureSplash = POWERUPS "SPLASH", InitEffects)
/// replaces its texture.
fn present_player_shadow(sim: Res<Sim>, libs: Res<Libraries>, mut req: ResMut<MatRequests>, mut q: Query<(&PlayerShadow, &mut Transform, &mut Visibility, &Children)>) {
    let active: [bool; 4] = std::array::from_fn(|k| k == 0 || (1 << k) & sim.game.globals.active_players != 0);
    let splash = libs.0.get("POWERUPS").and_then(|l| l.textures.get("SPLASH")).cloned();
    for (s, mut tf, mut vis, kids) in &mut q {
        let r = &sim.game.players[s.0];
        if !active[s.0] || matches!(r.i32_(0xfc), 0 | 5 | 0xb) {
            *vis = Visibility::Hidden;
            continue;
        }
        let p = r.v3(0x50);
        let (floor, water) = (r.f32_(0x88c), r.f32_(0x8a4));
        let y = if water <= floor { floor } else { water } + 0.1;
        tf.translation = Vec3::new(p[0], y, p[2]);
        if floor < water
            && let Some(img) = &splash
        {
            for e in kids.iter() {
                req.get(e).replace = Some(img.clone());
            }
        }
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
    spawn_package_texmods(&mut commands, &libraries, &texmod_packages(&session, &sources));
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
            if !has_object || debug_hidden(&node.label) {
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

/// GDL_DEBUG_HIDE=<label,...>: world nodes not spawned (render research).
fn debug_hidden(label: &str) -> bool {
    static HIDE: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    HIDE.get_or_init(|| std::env::var("GDL_DEBUG_HIDE").map(|v| v.split(',').map(str::to_owned).collect()).unwrap_or_default())
        .iter()
        .any(|h| h == label)
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
    spawn_texmod_list(commands, &list, &set.label_index, &set.record_images, &[]);
}

/// SetupItemTexMods / SetupWeaponPowerupTexMods: the texture modifiers of
/// the level's item packages and of WEAPONS/POWERUPS (stepped by do_items).
fn spawn_package_texmods(commands: &mut Commands, libraries: &Libraries, names: &[String]) {
    for name in names {
        if let Some(lib) = libraries.0.get(name) {
            // MBCopyTexture's by-name frames may be another package's (the
            // SFX packages' weapon textures are WEAPONS' REDWEAPTEX01..).
            let others: Vec<&Library> = names.iter().filter(|n| *n != name).filter_map(|n| libraries.0.get(n)).collect();
            spawn_texmod_list(commands, &lib.texmods, &lib.label_index, &lib.record_images, &others);
        }
    }
}

fn spawn_texmod_list(
    commands: &mut Commands,
    list: &[gdl_formats::animation::TexMod],
    label_index: &HashMap<String, u16>,
    record_images: &HashMap<u16, Handle<Image>>,
    others: &[&Library],
) {
    let mut mods = Vec::new();
    for tm in list.iter().filter(|t| t.owner == -1) {
        let Ok(target) = u16::try_from(tm.target) else { continue };
        let Some(target_image) = record_images.get(&target) else { continue };
        let kind = match tm.frame_base {
            -2 | -3 => TexModKind::Scroll { target: target_image.id(), v: tm.frame_base == -3 },
            -6..=-4 => continue,
            base => {
                // −1: MBOX_FindTexture by name — this package first, then the
                // others in load order (HYPOTHESIS: the search order).
                let name = tm.frame_name.to_uppercase();
                let found = if base == -1 {
                    label_index
                        .get(&name)
                        .map(|&i| (i32::from(i), record_images))
                        .or_else(|| others.iter().find_map(|l| l.label_index.get(&name).map(|&i| (i32::from(i), &l.record_images))))
                } else {
                    Some((base, record_images))
                };
                let Some((base, images)) = found else { continue };
                let n = i32::from(tm.count).abs().max(1);
                let frames: Vec<_> = (0..n).filter_map(|k| u16::try_from(base + k).ok().and_then(|i| images.get(&i)).cloned()).collect();
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
                overlay: None,
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

/// Build the native game for level `idx` with player 0's record `carry`,
/// entered at the level's start locator (get_player_pos). init_next_level's
/// record steps run first: load_player_geo/load_player (profile::level_start)
/// and PlayerSaveState (the Tower: without the backup copy; the
/// Underworld 0xc: not called).
fn build_game(session: &Session, idx: usize, slots: &[Option<SlotStart>; 4], returning: bool, load_in_game_players: Option<i32>) -> Result<Game, Box<dyn Error>> {
    let elf = gdl_formats::elf::Elf::parse(&session.elf_bytes)?;
    let lv = &session.levels[idx];
    let wdata = gdl_formats::wdata::WorldData::parse(&gdl_client::bounded_read(&lv.wdata)?)?;
    let mut start = session.start.clone();
    let tables = gdl_core::player_class::ClassTables::from_elf(&elf)?;
    let game_wave = lv.world << 8 | lv.index as i32;
    let mut records: [Option<Record>; 4] = Default::default();
    for (k, slot) in slots.iter().enumerate() {
        let Some(slot) = slot else { continue };
        let mut record = slot.record.clone();
        if lv.world == 0xd && record.i32_(0xfc) == 0xb {
            // init_next_level: a player who died (state 0xb) gets the
            // level-start backup back (PlayerRestoreState) and plays again.
            profile::player_restore_state(&mut record, &session.pdats, &tables).map_err(|e| e.to_string())?;
            record.set_i32(0xfc, 1);
        }
        profile::level_start(&mut record, &session.pdats, &slot.action_map, game_wave, session.heart_rate).map_err(|e| e.to_string())?;
        // load_player: the team-move node +0x6b4 (native: tree node index + 1).
        record.set_u32(0x6b4, slot.rig.dummy.map_or(0, |n| n as u32 + 1));
        // init_next_level: PlayerSaveState except in a treasure room (world
        // 12) and on the return from one (LoadStage).
        if lv.world != 0xc && !returning {
            profile::player_save_state(&mut record, lv.world != 0xd, lv.world, lv.index as i32, &slot.controls);
        }
        records[k] = Some(record);
    }
    let mut record = records[0].clone().ok_or("player 1 not in play")?;
    // DEBUG (Sumner shard/rune speech checks): GDL_DEBUG_SHARDS=<hex> /
    // GDL_DEBUG_RUNES=<hex> add live shard/rune bits (+0x1a24 / +0x1a26)
    // before a Tower entry; GDL_DEBUG_OLD_SHARDS / GDL_DEBUG_OLD_RUNES add
    // delivered ones (class record +0xcf0/+0xcf2 and backup +0x1d7c/+0x1d7e).
    if lv.world == 0xd || std::env::var("GDL_DEBUG_ANY_LEVEL").is_ok() {
        let c = 0xb4 * record.u32_(0xc) as usize;
        for (var, offsets) in [("GDL_DEBUG_SHARDS", vec![0x1a24]), ("GDL_DEBUG_RUNES", vec![0x1a26]), ("GDL_DEBUG_OLD_SHARDS", vec![c + 0xcf0, c + 0x1d7c]), ("GDL_DEBUG_OLD_RUNES", vec![c + 0xcf2, c + 0x1d7e])] {
            if let Some(bits) = std::env::var(var).ok().and_then(|v| u16::from_str_radix(v.trim_start_matches("0x"), 16).ok()) {
                for o in offsets {
                    record.set_u16(o, record.u16_(o) | bits);
                }
            }
        }
    }
    records[0] = Some(record.clone());
    for (k, r) in records.iter().enumerate() {
        if let (Some(r), Some(slot)) = (r, &slots[k]) {
            start["players"][k] = Value::String(r.0.iter().map(|b| format!("{b:02x}")).collect());
            start["classes"][k] = serde_json::to_value(&slot.class)?;
        }
    }
    start["globals"]["cur_world"] = Value::from(lv.world);
    start["globals"]["tower_counter"] = Value::from(session.carried[0]);
    start["globals"]["last_finished_world"] = Value::from(session.carried[1]);
    start["globals"]["last_finished_level"] = Value::from(session.carried[2]);
    start["globals"]["cur3d_transmitter"] = Value::from(session.carried[3]);
    start["globals"]["cur_transmitter"] = Value::from(session.carried[4]);
    // LoadGauntWorldStart: LastWorldPlayed / LastLevelPlayed follow every
    // non-Tower level.
    let (lwp, llp) = if lv.world != 0xd { (lv.world, lv.index as i32) } else { (session.carried[5], session.carried[6]) };
    start["globals"]["last_world_played"] = Value::from(lwp);
    start["globals"]["last_level_played"] = Value::from(llp);
    start["globals"]["cur_level"] = Value::from(lv.index as i64);
    // GamePrefs: the difficulty word 0x21edcc and multiplayer mode 0x21edd4.
    start["extra"]["target_cone_mode"] = Value::from(session.prefs[5]);
    start["globals"]["pvp_mode"] = Value::from(session.prefs[7]);
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
    let weapons = anim("WEAPONS")?;
    let powerups = anim("POWERUPS")?;
    let item_pkgs = lv.items.iter().map(|p| anim(p)).collect::<Result<Vec<_>, _>>()?;
    // AtreeMatchAnyHeader 0x14fa60 (an item definition's tree, +0x4c):
    // item_anim (the realm's package, first in lv.items when loaded),
    // powerup_anim, then item_anim2 (the level's own). Generators would then
    // try the enemy packages (not modelled; no G1 generator needs it).
    let realm = format!("ITEMS_{}", lv.name.get(..1).unwrap_or(""));
    let mut order: Vec<&Animations> = item_pkgs.iter().collect();
    order.insert(usize::from(lv.items.first().is_some_and(|p| *p == realm)), &powerups);
    let mut item_anims = HashMap::new();
    let mut item_anim_nodes = std::collections::HashMap::new();
    for a in order {
        setup::add_item_trees(&mut item_anims, &mut item_anim_nodes, a);
    }
    let mut effect_tables = gdl_core::game::effects::EffectTables::from_elf(&elf, &weapons, &powerups)?;
    for (n, a) in item_pkgs.iter().enumerate().take(2) {
        effect_tables.add_item_package(n as u8, a);
    }
    let mut game = Game::new_level(setup::LevelInputs {
        world: &session.world,
        wdata: &wdata,
        level: lv.index,
        elf: &elf,
        start,
        enemy_anims,
        item_anims,
        item_anim_nodes,
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
        load_in_game_players,
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
            if let Some(t) = session.tower_start {
                id = t;
            }
        }
        let loc = session
            .world
            .locators
            .iter()
            .find(|l| l.kind == 7 && l.id == id)
            .or_else(|| session.world.locators.iter().find(|l| l.kind == 7 && l.id == 0))
            .ok_or("level has no player start locator")?;
        // SetPlayerStartPos: StartTransmitter = StartTransmitterList[id].
        if let Some(cam) = game.std_camera.as_mut() {
            cam.set_start_transmitter(if loc.id == id { id.max(0) as usize } else { 0 });
        }
        game.place_player(0, loc.position, loc.rotation[1]).map_err(|e| e.0)?;
        game.players[0].set_u32(0x1e0, 0);
        game.players[0].set_u32(0x1e4, 0);
        game.player_clips[0] = slots[0].as_ref().map(|s| s.clips.clone()).unwrap_or_default();
        game.player_rigs[0] = slots[0].as_ref().map(|s| s.rig.clone());
        game.init_player_anim(0).map_err(|e| e.0)?;
        game.controls.assign(0, 0);
        game.control_options[0] = slots[0].as_ref().map_or([0; 4], |s| s.controls);
        // load_player → InitPlayerMissiles (WeapThrowFx).
        game.init_player_missiles(0);
        game.resolve_plyr_data(0);
        // The other players in play: load_player_geo → get_player_pos(k, 1)
        // beside the first earlier one (new_offset_start), else at the start
        // (place_player first sets up the record and node at the start).
        for (k, slot) in slots.iter().enumerate().skip(1) {
            let Some(slot) = slot else { continue };
            game.place_player(k, loc.position, loc.rotation[1]).map_err(|e| e.0)?;
            game.load_player_geo_place(k).map_err(|e| e.0)?;
            game.players[k].set_u32(0x1e0, 0);
            game.players[k].set_u32(0x1e4, 0);
            game.player_clips[k] = slot.clips.clone();
            game.player_rigs[k] = Some(slot.rig.clone());
            game.init_player_anim(k).map_err(|e| e.0)?;
            game.control_options[k] = slot.controls;
            game.init_player_missiles(k);
            game.resolve_plyr_data(k);
            game.globals.active_players |= 1 << k;
        }
        // init_next_level: init_game_cam + StdCamInit(0) after the players.
        game.start_camera().map_err(|e| e.0)?;
    }
    Ok(game)
}

/// NATIVE (docs/decisions: memory cards become host saves): the save store
/// as the select screen's card — files 0..7 are store slots 0..7.
struct NativeCard<'a> {
    store: &'a gdl_core::save::SaveStore,
}

impl gdl_core::game::pselect::SaveCard for NativeCard<'_> {
    fn directory(&self) -> [Option<gdl_core::game::pselect::CardFile>; 8] {
        std::array::from_fn(|k| {
            let region = self.store.load(k as u32).ok()?.save.region().ok()?;
            let name = String::from_utf8_lossy(&region[..region.iter().take(8).position(|&c| c == 0).unwrap_or(8)]).into_owned();
            let w = |o: usize| i32::from(i16::from_le_bytes([region[o], region[o + 1]]));
            // +0x9a4 class and +0x9aa level sum (region offsets 8 / 0xe).
            Some(gdl_core::game::pselect::CardFile { name, levels: w(0xe), class: w(8) })
        })
    }
    fn load(&mut self, k: usize) -> Option<Vec<u8>> {
        self.store.load(k as u32).ok()?.save.region().ok()
    }
    fn save(&mut self, k: usize, region: &[u8]) -> bool {
        self.store.store(k as u32, "", region).is_ok()
    }
}

/// NATIVE: a letter typed on the keyboard this frame (name entry).
fn typed_letter(keys: &ButtonInput<KeyCode>) -> Option<char> {
    let letters = [
        (KeyCode::KeyA, 'A'), (KeyCode::KeyB, 'B'), (KeyCode::KeyC, 'C'), (KeyCode::KeyD, 'D'), (KeyCode::KeyE, 'E'), (KeyCode::KeyF, 'F'), (KeyCode::KeyG, 'G'),
        (KeyCode::KeyH, 'H'), (KeyCode::KeyI, 'I'), (KeyCode::KeyJ, 'J'), (KeyCode::KeyK, 'K'), (KeyCode::KeyL, 'L'), (KeyCode::KeyM, 'M'), (KeyCode::KeyN, 'N'),
        (KeyCode::KeyO, 'O'), (KeyCode::KeyP, 'P'), (KeyCode::KeyQ, 'Q'), (KeyCode::KeyR, 'R'), (KeyCode::KeyS, 'S'), (KeyCode::KeyT, 'T'), (KeyCode::KeyU, 'U'),
        (KeyCode::KeyV, 'V'), (KeyCode::KeyW, 'W'), (KeyCode::KeyX, 'X'), (KeyCode::KeyY, 'Y'), (KeyCode::KeyZ, 'Z'), (KeyCode::Digit0, '0'), (KeyCode::Digit1, '1'),
        (KeyCode::Digit2, '2'), (KeyCode::Digit3, '3'), (KeyCode::Digit4, '4'), (KeyCode::Digit5, '5'), (KeyCode::Digit6, '6'), (KeyCode::Digit7, '7'), (KeyCode::Digit8, '8'),
        (KeyCode::Digit9, '9'),
    ];
    letters.iter().find(|(k, _)| keys.just_pressed(*k)).map(|&(_, c)| c)
}

/// One player slot's inputs to a level: its record, its class's clip
/// timings, action map and ClassData, its controller options.
struct SlotStart {
    record: Record,
    clips: Vec<gdl_core::animation_clock::ClipTiming>,
    /// Tree 0 of the character's ANIM package and its DUMMY node (team
    /// moves: load_player's +0x6b4).
    rig: std::sync::Arc<gdl_core::game::teammove::PlayerRig>,
    action_map: Vec<[i64; 2]>,
    class: ClassData,
    controls: profile::ControlOptions,
}

/// The slots for a level: player 1 always, the others while in play, each
/// with its loaded character's clips and its class data.
fn slot_starts(front: &Front, sources: &Sources, records: &[Record; 4], controls: &[profile::ControlOptions; 4]) -> Result<[Option<SlotStart>; 4], Box<dyn Error>> {
    let mut out: [Option<SlotStart>; 4] = Default::default();
    for k in 0..4 {
        if k != 0 && records[k].i32_(0xfc) == 0 {
            continue;
        }
        let Some(pc) = &sources.players[k] else { continue };
        // load_player: +0x6b4 = AtreeFindMbidxNode(tree, "<+0x698>DUMMY") —
        // the tree node "DUMMY" when the model package has that object.
        let r = &records[k];
        let tag = gdl_core::player_class::CLASS_TAGS.get(r.u32_(0xc) as usize).copied().unwrap_or("").to_ascii_uppercase();
        let object = format!("{tag}_{}DUMMY", ["YEL", "BLU", "RED", "GRE"].get(r.u32_(4) as usize).copied().unwrap_or("YEL"));
        let has_object = sources.packages.get(&pc.model_name).is_some_and(|m| m.objects.definitions.iter().any(|d| d.label == object));
        let dummy = has_object.then(|| pc.clips.trees.first().and_then(|t| t.nodes.iter().position(|n| n.label == "DUMMY"))).flatten();
        let rig = std::sync::Arc::new(gdl_core::game::teammove::PlayerRig { anim: Animations::parse(&pc.clip_bytes)?, bytes: pc.clip_bytes.clone(), dummy });
        out[k] = Some(SlotStart {
            record: records[k].clone(),
            rig,
            clips: setup::clips_of(&pc.clips, 0),
            action_map: front.tables.action_map(&pc.clips),
            class: with_sfx_trees(class_data_of(front, records[k].u32_(0xc).min(16))?, sources, k),
            controls: controls[k],
        });
    }
    Ok(out)
}


#[allow(clippy::too_many_arguments)]
/// The simulation's audio requests: sounds, loops, kills and the stage
/// music stream (BGPlayStream / AudioStreamSetVolume / AudioStreamStop).
fn play_audio(audio: &mut Option<NonSendMut<gdl_client::audio::AudioOut>>, events: &[Event], me: Vec3, right: Vec3) {
    let Some(a) = audio.as_mut() else { return };
    for e in events {
        match e {
            Event::Sound { id, position, volume, priority, pan } => {
                // Presentation pan: offset along the camera's right axis, or
                // the fixed pan (0..0xfe, 0x7f centre).
                let pan = match (position, pan) {
                    (Some(p), _) => ((Vec3::from_array(*p) - me).dot(right) / 20.0).clamp(-1.0, 1.0),
                    (None, Some(v)) => ((*v - 0x7f) as f32 / 127.0).clamp(-1.0, 1.0),
                    (None, None) => 0.0,
                };
                a.play(*id, *volume, pan, *priority);
            }
            Event::LoopSound { id, position, volume, priority } => {
                let pan = position.map_or(0.0, |p| ((Vec3::from_array(p) - me).dot(right) / 20.0).clamp(-1.0, 1.0));
                a.play_loop(*id, *volume, pan, *priority);
            }
            Event::StopSound { id } => a.stop(*id),
            Event::Music { name, volume, looping } => {
                // A missing part counts as playing (BGPlayStream's error
                // path keeps the loop flag set).
                a.stream(name, *looping, *volume);
            }
            Event::ItemSound { slot, id, volume, position, priority } => {
                let pan = ((Vec3::from_array(*position) - me).dot(right) / 20.0).clamp(-1.0, 1.0);
                a.play_tagged(*slot as u32, *id, *volume, pan, *priority);
            }
            Event::ItemSoundStop { slot } => a.stop_tagged(*slot as u32),
            Event::MusicVolume { volume } => a.stream_volume(*volume),
            Event::MusicStop => a.stream_stop(),
            _ => {}
        }
    }
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
    if sim.error.is_some() || session.banner.is_some() || session.finished.is_some() || !ui.started {
        return;
    }
    if ui.paused || sim.message_box.is_some() {
        // main keeps running AudioQueUpdate / AudioPoll (DoOptions,
        // ScrollMessageBox): queued lines go on while the game waits.
        let vb = sim.game.globals.vb_elapsed;
        sim.game.main_audio(vb);
        let new: Vec<Event> = sim.game.events.drain(..).collect();
        let me = Vec3::from_array(sim.game.players[0].v3(0x50));
        play_audio(&mut audio, &new, me, presentation.basis[0]);
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
    // PlayStreamComplete: a stage music part played once has ended.
    if sim.game.bg_music.stream_playing && audio.as_ref().is_some_and(|a| !a.stream_playing()) {
        sim.game.bg_stream_complete();
    }
    let me = Vec3::from_array(sim.game.players[0].v3(0x50));
    let right = sim.game.std_camera.as_ref().map_or(presentation.basis[0], |c| Vec3::from_array(c.pb.cam_rows[0]));
    // game mode 0x8008: the attract flyby (game_main's flyby case, do_camera).
    if let Some(mut f) = sim.flyby.take() {
        if let Err(e) = sim.game.update_controls(&ports) {
            sim.error = Some(format!("controls: {}", e.0));
        }
        sim.game.globals.vb_elapsed = 2;
        if sim.game.flyby_step(&mut f) {
            sim.flyby_done = true;
        }
        sim.flyby = Some(f);
        let new: Vec<Event> = sim.game.events.drain(..).collect();
        play_audio(&mut audio, &new, me, right);
        return;
    }
    // game mode 0x400e: do_gamemovie (controls polled; the game waits).
    if let Some(mut run) = sim.movie.take() {
        let vb = sim.game.globals.vb_elapsed;
        sim.game.main_audio(vb);
        if let Err(e) = sim.game.update_controls(&ports) {
            sim.error = Some(format!("controls: {}", e.0));
        }
        let serving = !run.tried || run.player.as_ref().is_some_and(|p| p.playing());
        match sim.game.do_gamemovie(&mut run.game, serving) {
            Some(_) => sim.movie_done = Some(run.then),
            None => sim.movie = Some(run),
        }
        return;
    }
    // game mode 0x400f: do_mapscreen until the next level loads.
    if let Some((mut m, next)) = sim.map.take() {
        let g = &mut sim.game;
        g.hud.begin_frame();
        g.main_audio(g.globals.vb_elapsed);
        if g.do_mapscreen(&mut m) {
            sim.map_done = Some(next);
        } else {
            sim.map = Some((m, next));
        }
        let new: Vec<Event> = sim.game.events.drain(..).collect();
        play_audio(&mut audio, &new, me, right);
        return;
    }
    // game mode 0x4012: game_main runs do_shop (and WritePlayerInfo) only.
    if let Some((mut flow, tower)) = sim.shop.take() {
        let g = &mut sim.game;
        g.hud.begin_frame();
        g.main_audio(g.globals.vb_elapsed);
        let r = g.update_controls(&ports).and_then(|_| g.do_shop(&mut flow, &front.shop));
        match r {
            Ok(0) => sim.shop = Some((flow, tower)),
            Ok(_) => sim.shop_done = Some(tower),
            Err(e) => sim.error = Some(format!("do_shop: {}", e.0)),
        }
        let new: Vec<Event> = sim.game.events.drain(..).collect();
        play_audio(&mut audio, &new, me, right);
        return;
    }
    // game mode 0x400b: the player-select screen (do_player_select).
    if let Some(mut ps) = sim.select.take() {
        let typed = typed_letter(&keys);
        let g = &mut sim.game;
        g.hud.begin_frame();
        g.main_audio(g.globals.vb_elapsed);
        let mut card = NativeCard { store: &front.store };
        let r = g.update_controls(&ports).and_then(|_| {
            let mut cx = gdl_core::game::pselect::SelectCtx { pdats: &session.pdats, card: &mut card, typed: [typed, None, None, None] };
            g.do_player_select(&mut ps, &front.sel, &mut cx)
        });
        let blits = sim.game.select_blits(&ps, &front.sel);
        sim.game.hud.push_temp(blits);
        use gdl_core::game::pselect::SelectEnd;
        match r {
            Ok(SelectEnd::Running) => sim.select = Some(ps),
            // init_next_level: the Tower with the players chosen.
            Ok(SelectEnd::Done) => sim.select_done = true,
            Ok(SelectEnd::Title | SelectEnd::Attract) => sim.select_quit = true,
            Err(e) => sim.error = Some(format!("do_player_select: {}", e.0)),
        }
        let new: Vec<Event> = sim.game.events.drain(..).collect();
        play_audio(&mut audio, &new, me, right);
        return;
    }
    // game_main in the Tower: a controller pressing Start joins through
    // the select screen (check_active_players → init_player_select(1)).
    // (Not in game mode 0x4014: do_over's branch runs no join check.)
    if sim.game.globals.cur_world == 0xd && sim.game.hud.optmenu_active == 0 && sim.game.globals.start_of_stage == 0 && sim.over.is_none() {
        let g = &mut sim.game;
        let joined = g.update_controls(&ports).and_then(|_| g.join_check(&session.pdats));
        match joined {
            Ok(true) => match sim.game.init_player_select(1, &session.pdats) {
                Ok(ps) => {
                    sim.select = Some(ps);
                    return;
                }
                Err(e) => sim.error = Some(format!("init_player_select: {}", e.0)),
            },
            Ok(false) => {}
            Err(e) => sim.error = Some(format!("check_active_players: {}", e.0)),
        }
    }
    let Sim { game, .. } = &mut *sim;
    let before = game.events.len();
    game.step_with(None, &mut |g| {
        g.update_controls(&ports)?;
        g.do_players().map(|_| ())
    });
    sim.updates += 1;
    // game mode 0x4014: do_over after the world, items, enemies, players.
    if let Some(mut over) = sim.over.take() {
        if sim.game.do_over(&mut over) {
            sim.over_done = true;
        }
        sim.over = Some(over);
    }
    // DEBUG (presentation checks): GDL_DEBUG_HELP=<id> raises NewHelp(id,
    // player 0) once, 60 updates into the level. Not a game feature.
    if sim.updates == 60
        && let Some(id) = std::env::var("GDL_DEBUG_HELP").ok().and_then(|v| i32::from_str_radix(v.trim_start_matches("0x"), 16).ok())
    {
        sim.game.help(id, 0);
    }
    // DEBUG: GDL_DEBUG_EXP=<exp>:<update> sets player 1's experience (+0x1a1c
    // and the level +0x2ac0) once, e.g. to see Sumner's level speech.
    if let Some((exp, at)) = std::env::var("GDL_DEBUG_EXP").ok().and_then(|v| v.split_once(':').and_then(|(e, u)| Some((e.parse::<i32>().ok()?, u.parse::<u64>().ok()?))))
        && sim.updates == at
    {
        sim.game.players[0].set_i32(0x1a1c, exp);
        sim.game.players[0].set_i32(0x2ac0, gdl_core::player_class::exp_to_level(exp));
    }
    // DEBUG: GDL_DEBUG_HINTS=<update> opens Sumner's hint menu once at that
    // update (1: update 60).
    if let Ok(v) = std::env::var("GDL_DEBUG_HINTS")
        && sim.updates == v.parse::<u64>().ok().filter(|&u| 1 < u).unwrap_or(60)
    {
        sim.hint_menu = true;
    }
    // DEBUG: GDL_DEBUG_GOTIT=<kind>:<amount> raises add_got_it(0, …) once.
    if sim.updates == 60
        && let Some((k, a)) = std::env::var("GDL_DEBUG_GOTIT").ok().and_then(|v| v.split_once(':').and_then(|(k, a)| Some((k.parse().ok()?, a.parse().ok()?))))
    {
        sim.game.got_it(0, k, a);
    }
    // DEBUG: GDL_DEBUG_SHIELD=<flags hex>:<seconds> puts a kind-6 (shield)
    // timer with those flags in player 1's first powerup slot (+0x144) once
    // (update 600, after the level start), e.g. 110000:30 for gold chrome
    // (PlayerProcessPowerups shows gold only with a 0x10000 timer too).
    if sim.updates == 600
        && let Some((f, t)) = std::env::var("GDL_DEBUG_SHIELD").ok().and_then(|v| v.split_once(':').and_then(|(f, t)| Some((u32::from_str_radix(f, 16).ok()?, t.parse::<f32>().ok()?))))
    {
        let r = &mut sim.game.players[0];
        r.set_f32(0x144, t);
        r.set_i32(0x148, 6);
        r.set_f32(0x14c, 0.0);
        r.set_u32(0x150, f);
    }
    // DEBUG: GDL_DEBUG_FX=1 logs the live effect slots every 10 updates.
    if std::env::var("GDL_DEBUG_FX").is_ok() && sim.updates.is_multiple_of(10) {
        let g = &sim.game;
        let live: Vec<String> = (0..g.effects.num)
            .filter(|&k| 0.0 < g.effects.slots[k].f32_(0x68))
            .map(|k| {
                let n = g.effects.nodes[k];
                format!("{k}:{:?}/{:?}/{:?}", n.map(|n| n.visual), n.and_then(|n| n.follow), n.map(|n| n.position))
            })
            .collect();
        if !live.is_empty() {
            eprintln!("FX {} {}", sim.updates, live.join(" "));
        }
    }
    // DEBUG: GDL_DEBUG_WEAPON=<type 1..4>:<seconds> gives player 1 a kind-5
    // weapon powerup once (update 600), e.g. 1:60 for the red weapon.
    if sim.updates == 600
        && let Some((t, secs)) = std::env::var("GDL_DEBUG_WEAPON").ok().and_then(|v| v.split_once(':').and_then(|(t, s)| Some((t.parse::<u32>().ok()?, s.parse::<f32>().ok()?))))
    {
        let class = sim.game.classes[0].clone();
        if let Err(e) = sim.game.add_powerup(0.0, secs, 0, 5, t, &class) {
            sim.error = Some(format!("GDL_DEBUG_WEAPON: {}", e.0));
        }
        eprintln!("GDL_DEBUG_WEAPON: type {t} for {secs} s; WeapThrowFx {:?}", sim.game.effects.weap_throw_fx[0]);
    }
    // DEBUG: GDL_DEBUG_BOSS_KILL=<update> deals lethal CritterDamage to the
    // boss (Game::debug_damage_boss) from that update on, once a second
    // until it dies (it is not damageable before it activates), e.g. to see
    // the good wizard.
    if let Some(at) = std::env::var("GDL_DEBUG_BOSS_KILL").ok().and_then(|v| v.parse::<u64>().ok())
        && at <= sim.updates
        && (sim.updates - at).is_multiple_of(60)
        && sim.game.globals.boss_is_dead == 0
    {
        let r = sim.game.debug_damage_boss();
        eprintln!("GDL_DEBUG_BOSS_KILL at {at}: {r:?}");
    }
    // DEBUG: GDL_DEBUG_TELEPORT=x,y,z moves player 1 there once (update
    // 600), e.g. onto a secret exit. Not a game feature.
    if sim.updates == 600
        && let Some(v) = std::env::var("GDL_DEBUG_TELEPORT").ok().map(|v| v.split(',').filter_map(|c| c.trim().parse::<f32>().ok()).collect::<Vec<_>>())
        && v.len() == 3
    {
        let yaw = sim.game.players[0].f32_(0x86c);
        if let Err(e) = sim.game.place_player(0, [v[0], v[1], v[2]], yaw) {
            sim.error = Some(format!("teleport: {}", e.0));
        }
    }
    let _ = before;
    // Every request since the last frame (level setup's BGMusicStart, the
    // shop and map screens' leftovers included).
    let new: Vec<Event> = sim.game.events.drain(..).collect();
    play_audio(&mut audio, &new, me, right);
    for e in new {
        match e {
            Event::Exp { amount, .. } => sim.exp += i64::from(amount),
            Event::LevelUp { level, .. } => sim.last_event = format!("LEVEL UP! now level {level}"),
            Event::GeneratorHit { destroyed: true, .. } => sim.last_event = "generator destroyed".into(),
            Event::GotIt { kind, amount, .. } => sim.last_event = format!("picked up (kind {kind}, amount {amount})"),
            Event::PlayerDied { .. } => sim.last_event = "YOU DIED".into(),
            Event::PlayerInactivated { player } => {
                // inactivate_player's clear_player(k, 1) with every PDAT.
                let demo = sim.game.globals.demo_mode != 0;
                if let Err(e) = profile::clear_player(&mut sim.game.players[player], true, demo, &session.pdats) {
                    sim.error = Some(format!("clear_player: {e}"));
                }
            }
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
                tree_node_model(p, lib, tree, node, at, &mut out);
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
    for (offset, parts, flags, node, extra) in list {
        let frame = match extra {
            Some(NodeExtra::Frame(f)) => Some(f),
            _ => None,
        };
        if let Some(NodeExtra::Psys(ps)) = extra {
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
            if let Some(f) = frame {
                child.insert((f, Visibility::Hidden));
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
    let eye = Vec3::from_array(cam.pb.eye);
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
                let Some(anim) = g.enemy_anims[slot.index].as_ref().filter(|_| st != 0) else {
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
                // DoObjAnimation: frame (int)(f + 0.5) inside [start, start
                // + count − 1] → base + frame − start; a one-frame entry
                // always shows its object. (Reverse-order sequences, flag
                // +0x2a bit 0, are not modelled here.)
                let list = frames.0.get(pkg).and_then(|m| m.get(&tree)).and_then(|s| s.get(seq));
                let label = list.and_then(|(start, l)| {
                    let f = (anim.clock.frame + 0.5) as i32 - i32::from(*start);
                    if l.is_empty() {
                        None
                    } else if 0 <= f && (f as usize) < l.len() {
                        Some(l[f as usize].clone())
                    } else {
                        Some(l[0].clone())
                    }
                });
                let label = label.or_else(|| frames.0.get(pkg).and_then(|m| m.get(&tree)).and_then(|s| s.first()).and_then(|(_, l)| l.first().cloned()));
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
                    // ReplaceItemAtree (Game::item_trees) overrides the
                    // definition's tree.
                    let name = g.item_trees.get(&slot.index).cloned().unwrap_or_else(|| String::from_utf8_lossy(d.name()).into_owned());
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
                let node_tf = |n: &gdl_core::game::effects::EffectNode| {
                    let m = n.matrix;
                    let mut tf = record_transform([Vec3::from_array(m[0]), Vec3::from_array(m[1]), Vec3::from_array(m[2])], Vec3::from_array(n.position));
                    tf.scale *= n.scale * Vec3::from_array(n.stretch);
                    tf
                };
                let mut tf = node_tf(&node);
                // MBNodeSetParent onto another effect's node (the throw effect
                // on its missile): that node's transform first.
                if let Some(parent) = node.follow.and_then(|k| g.effects.nodes.get(k).copied().flatten()) {
                    tf = node_tf(&parent).mul_transform(tf);
                }
                match node.visual {
                    Visual::Missile { player } => {
                        let Some(throw) = sources.players.get(player).and_then(|p| p.as_ref()).or(sources.players[0].as_ref()).map(|p| p.throw.clone()) else {
                            *vis = Visibility::Hidden;
                            continue;
                        };
                        let key = format!("{}:{}", throw.1, node.start.to_bits());
                        let fresh = slot.key.as_deref() != Some(key.as_str());
                        if fresh {
                            label_effect_tree(&mut commands, (entity, slot.index), &sources, &throw.0, &throw.1, node.start);
                        }
                        let parts = fresh.then(|| missile_parts(&sources, &libs, &throw));
                        (Some(key), parts, tf)
                    }
                    // EffectHeaders[h] (or a tree outside them): a tree of
                    // WEAPONS (0), POWERUPS (1), critter file n's model
                    // (2 + n), player p's SFX package (0x40 + p) or an item
                    // package (0x80 + n); posed per frame below.
                    Visual::Header(_) | Visual::Tree(_) => {
                        let (h, head) = match node.visual {
                            Visual::Tree(t) => (usize::MAX, t),
                            Visual::Header(h) => match g.effects.tables.headers.get(h) {
                                Some(Some(head)) => (h, *head),
                                _ => {
                                    *vis = Visibility::Hidden;
                                    continue;
                                }
                            },
                            _ => unreachable!(),
                        };
                        let pkg = match head.source {
                            0 => Some("WEAPONS".to_string()),
                            1 => Some("POWERUPS".to_string()),
                            n if 0xc0 <= n => sources.monsters.get(&i32::from(n - 0xc0)).cloned(),
                            n if 0x80 <= n => items.get(usize::from(n - 0x80)).cloned(),
                            n if 0x40 <= n => sources.players.get(usize::from(n - 0x40)).and_then(|p| p.as_ref()).map(|p| p.sfx_name.clone()),
                            n => session.levels[session.current].critters.get(n as usize - 2).and_then(|c| c.2.clone()),
                        };
                        let Some(pkg) = pkg else {
                            *vis = Visibility::Hidden;
                            continue;
                        };
                        let key = format!("H{h}:{}:{}:{}", head.source, head.tree, node.start.to_bits());
                        if slot.key.as_deref() != Some(key.as_str()) {
                            commands.entity(entity).insert(EffectTree { pkg: pkg.clone(), tree: head.tree, start: node.start, timing: head.timing, slot: slot.index });
                        }
                        let parts = (slot.key.as_deref() != Some(key.as_str())).then(|| tree_index_parts(&sources, &libs, &pkg, head.tree));
                        (Some(key), parts, tf)
                    }
                    // FamiliarSpit: the class SFX package's FAMILIAR_SPIT tree
                    // (or WEAPONS' PHOENIX_FBALL for the phoenix).
                    Visual::Familiar { phoenix, player } => {
                        let sfx = sources.players.get(player).and_then(|p| p.as_ref()).or(sources.players[0].as_ref()).map(|p| p.sfx_name.clone()).unwrap_or_default();
                        let (pkg, label) = if phoenix { ("WEAPONS".to_string(), "PHOENIX_FBALL") } else { (sfx, "FAMILIAR_SPIT") };
                        let key = format!("F{pkg}:{label}:{}", node.start.to_bits());
                        let fresh = slot.key.as_deref() != Some(key.as_str());
                        if fresh {
                            label_effect_tree(&mut commands, (entity, slot.index), &sources, &pkg, label, node.start);
                        }
                        let parts = fresh.then(|| tree_parts(&sources, &libs, &pkg, label));
                        (Some(key), parts, tf)
                    }
                    Visual::EnemyMissile { t, kind } => {
                        let Some(pkg) = sources.monsters.get(&t) else {
                            *vis = Visibility::Hidden;
                            continue;
                        };
                        let label = format!("{pkg}_{}", g.effects.tables.enemy_missile_desc[kind as usize]);
                        let key = format!("{label}:{}", node.start.to_bits());
                        let fresh = slot.key.as_deref() != Some(key.as_str());
                        if fresh {
                            label_effect_tree(&mut commands, (entity, slot.index), &sources, pkg, &label, node.start);
                        }
                        let parts = fresh.then(|| tree_parts(&sources, &libs, pkg, &label));
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

/// init_mapscreen for level `k`: its WDATA MAPS record (LEVL +0x5c) and
/// the MAPS/level<code> package as a HUD texture set. False when the map
/// package cannot be loaded (the caller goes straight on).
fn start_map(sim: &mut Sim, sources: &mut Sources, sets: &mut HudSets, front: &Front, session: &Session, k: usize, skip: bool) -> bool {
    let lv = &session.levels[k];
    let code = lv.name.clone();
    let map = (|| -> Option<[f32; 18]> {
        let w = gdl_formats::wdata::WorldData::parse(&gdl_client::bounded_read(&lv.wdata).ok()?).ok()?;
        let rec = w.levels.get(lv.index)?;
        let i = i16::from_le_bytes([rec.raw[0x5c], rec.raw[0x5d]]);
        w.maps.get(usize::try_from(i).ok()?).copied()
    })();
    let key = format!("MAPS/level{code}");
    if !sources.packages.contains_key(&key) {
        let Ok(pkg) = package_entry(&front.catalog, &key, false).and_then(|v| load_package(&v)) else { return false };
        sources.packages.insert(key.clone(), pkg);
    }
    let set = gdl_client::hud_texture_set(&key, &sources.packages[&key].objects);
    sim.game.hud.add_texture_set(set);
    sets.keys.push(key);
    let m = sim.game.init_mapscreen(0x78, skip, &code, map);
    sim.map = Some((m, k));
    true
}

/// init_shop(0) on the finished realm's game, leading to Tower level `k`.
fn start_shop(sim: &mut Sim, front: &Front, k: usize) {
    match sim.game.init_shop(0, &front.shop) {
        Ok(flow) => sim.shop = Some((flow, k)),
        Err(e) => sim.error = Some(format!("init_shop: {}", e.0)),
    }
}

/// Animate a slot's tree `label` of `pkg` from `start` (HYPOTHESIS: sequence
/// 0 at the sequence's timing factor, as StartFXTree trees); without the
/// tree the slot's previous EffectTree is removed.
fn label_effect_tree(commands: &mut Commands, (entity, slot): (Entity, usize), sources: &Sources, pkg: &str, label: &str, start: f32) {
    let tree = sources.packages.get(pkg).and_then(|p| p.anim.as_ref()).and_then(|a| a.trees.iter().position(|t| t.label == label).map(|i| (i, a.trees[i].sequences.first().map_or(30, |q| q.timing_factor))));
    match tree {
        Some((tree, timing)) => {
            commands.entity(entity).insert(EffectTree { pkg: pkg.to_string(), tree, start, timing: timing.max(1), slot });
        }
        None => {
            commands.entity(entity).remove::<EffectTree>();
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
    /// The effect slot (its node's MBTreeSetColor / MBTreeSetAlpha).
    slot: usize,
}

/// Pose effect trees' nodes (sequence 0) at the effect's current frame;
/// camera-facing nodes keep their translation and take CameraFaceLocal's
/// rotation afterwards.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn present_effect_trees(
    sim: Res<Sim>,
    sources: Res<Sources>,
    libs: Res<Libraries>,
    slots: Query<(&EffectTree, &Children, &Visibility)>,
    mut parts: Query<(Entity, &TreeNodeIdx, &mut Transform, Option<&OFrame>, &mut Visibility), Without<EffectTree>>,
    mut req: ResMut<MatRequests>,
    mut texmods: Local<HashMap<(String, usize), Vec<NodeTexMod>>>,
) {
    let atime = sim.game.globals.atime;
    for (fx, children, vis) in &slots {
        if *vis == Visibility::Hidden {
            continue;
        }
        let Some(pkg) = sources.packages.get(&fx.pkg) else { continue };
        let Some(anim) = pkg.anim.as_ref() else { continue };
        let frame = ((atime - fx.start) * 900.0 / f32::from(fx.timing.max(1))).max(0.0);
        let Ok(pose) = gdl_client::tree_pose(anim, fx.tree, &pkg.anim_bytes, 0, frame, true) else { continue };
        let mut meshes = Vec::new();
        for child in children.iter() {
            if let Ok((e, idx, mut tf, of, mut v)) = parts.get_mut(child) {
                if let Some(m) = pose.get(idx.0) {
                    *tf = Transform::from_matrix(*m);
                }
                if let Some(f) = of {
                    show_oframe(f, 0, frame, &mut v);
                }
                meshes.push((e, idx.0));
            }
        }
        // MBTreeSetAlpha (setPrimAlpha: (255 − v) / 255) and MBTreeSetColor.
        if let Some(Some(n)) = sim.game.effects.nodes.get(fx.slot) {
            for &(e, _) in &meshes {
                if let Some(a) = n.alpha {
                    req.alpha(e, (255 - a.clamp(0, 255)) as f32 / 255.0);
                }
                if let Some(c) = n.tint {
                    req.get(e).color = Some(Vec3::new((c >> 16 & 0xff) as f32, (c >> 8 & 0xff) as f32, (c & 0xff) as f32) / 255.0);
                }
                // MBTreeSetAmbientAdd: the colour scale's add / 255.
                if let Some(a) = n.ambient {
                    req.get(e).ambient += a as f32 / 255.0;
                }
            }
        }
        let mods = texmods.entry((fx.pkg.clone(), fx.tree)).or_insert_with(|| anim.trees.get(fx.tree).map(|t| tree_texmods(pkg, t)).unwrap_or_default());
        if let Some(lib) = libs.0.get(&fx.pkg)
            && !mods.is_empty()
        {
            request_tree_texmods(mods, lib, frame, &meshes, &mut req);
        }
    }
}

/// DoObjAnimation for one OANIM frame object of sequence `seq` at `frame`.
fn show_oframe(f: &OFrame, seq: u16, frame: f32, v: &mut Visibility) {
    let at = (frame + 0.5) as i32 - i32::from(f.start);
    let on = f.seq == seq && (f.single || at == i32::from(f.k));
    let want = if on { Visibility::Inherited } else { Visibility::Hidden };
    if *v != want {
        *v = want;
    }
}

/// Item trees posed by the simulation's item animation clocks
/// (ProcessItems' AnimateATree: the current sequence and frame).
#[allow(clippy::type_complexity)]
fn present_item_trees(
    sim: Res<Sim>,
    sources: Res<Sources>,
    session: Res<Session>,
    slots: Query<(&Slot, &Children, &Visibility)>,
    mut parts: Query<(Entity, &TreeNodeIdx, &mut Transform, Option<&OFrame>, &mut Visibility), Without<Slot>>,
    mut trees: Local<HashMap<String, Option<(String, usize)>>>,
    (libs, mut req, mut texmods): (Res<Libraries>, ResMut<MatRequests>, Local<HashMap<(String, usize), Vec<NodeTexMod>>>),
) {
    let items = &session.levels[session.current].items;
    for (slot, children, vis) in &slots {
        if slot.kind != SlotKind::Item || *vis == Visibility::Hidden {
            continue;
        }
        let Some(Some(anim)) = sim.game.item_anims.get(slot.index) else { continue };
        let Some(name) = slot.key.as_deref().and_then(|k| k.split('#').next()) else { continue };
        let found = trees
            .entry(name.to_string())
            .or_insert_with(|| {
                items.iter().map(String::as_str).chain(["POWERUPS"]).find_map(|pkg| {
                    let t = sources.packages.get(pkg)?.anim.as_ref()?.trees.iter().position(|t| t.label == name)?;
                    Some((pkg.to_string(), t))
                })
            })
            .clone();
        let Some((pkg, tree)) = found else { continue };
        let Some(p) = sources.packages.get(&pkg) else { continue };
        let Some(a) = p.anim.as_ref() else { continue };
        let frame = anim.clock.frame.max(0.0);
        let Ok(pose) = gdl_client::tree_pose(a, tree, &p.anim_bytes, anim.current as usize, frame, true) else { continue };
        let mut meshes = Vec::new();
        for child in children.iter() {
            if let Ok((e, idx, mut tf, of, mut v)) = parts.get_mut(child) {
                if let Some(m) = pose.get(idx.0) {
                    *tf = Transform::from_matrix(*m);
                }
                if let Some(f) = of {
                    show_oframe(f, anim.current, frame, &mut v);
                }
                meshes.push((e, idx.0));
            }
        }
        let mods = texmods.entry((pkg.clone(), tree)).or_insert_with(|| a.trees.get(tree).map(|t| tree_texmods(p, t)).unwrap_or_default());
        if let Some(lib) = libs.0.get(&pkg)
            && !mods.is_empty()
        {
            request_tree_texmods(mods, lib, frame, &meshes, &mut req);
        }
    }
}

/// A package tree's nodes at their initial positions (presentation).
fn tree_parts(sources: &Sources, libs: &Libraries, pkg: &str, label: &str) -> Model {
    let (Some(p), Some(lib)) = (sources.packages.get(pkg), libs.0.get(pkg)) else { return Vec::new() };
    let Some(tree) = p.anim.as_ref().and_then(|a| a.trees.iter().find(|t| t.label == label)) else { return Vec::new() };
    let mut out = Vec::new();
    for node in &tree.nodes {
        tree_node_model(p, lib, tree, node, Vec3::from_array(node.initial_position), &mut out);
    }
    out
}

/// A package tree node's drawables at `at`: its object, its particle system
/// (type 4) or its OANIM frame objects (type 2, every sequence's entry).
fn tree_node_model(p: &Package, lib: &Library, tree: &gdl_formats::animation::Tree, node: &gdl_formats::animation::Node, at: Vec3, out: &mut Model) {
    let label: String = (tree.prefix.clone() + &node.label).chars().take(15).collect();
    // AtreeNodeInit: the named object only without +0x2e bit 0; an OANIM
    // node's object is DoObjAnimation's frame object.
    let named = (node.flags & 1 == 0 && node.node_type != 2).then(|| lib.by_label.get(&label)).flatten();
    if let Some(parts) = named {
        out.push((at, parts.clone(), tree_node_flags(node), Some(node.index as usize), None));
    } else if let Some(ps) = tree_node_psys(&p.anim_bytes, tree, node) {
        out.push((at, Vec::new(), 0, Some(node.index as usize), Some(NodeExtra::Psys(ps))));
    } else if node.node_type == 2 {
        for (seq, e) in node.oanim.iter().enumerate() {
            let Some(&base) = lib.index_of.get(&e.name.to_uppercase()) else { continue };
            for k in 0..e.count.max(1) {
                if let Some(parts) = lib.by_index.get(&(base + k as u32)) {
                    let f = OFrame { seq: seq as u16, start: e.start, k, single: e.count <= 1 };
                    out.push((at, parts.clone(), tree_node_flags(node), Some(node.index as usize), Some(NodeExtra::Frame(f))));
                }
            }
        }
    }
}


/// The character's thrown missile tree (InitPlayerMissiles: the
/// "<PlayerMissileDesc>_THROW<n>" tree of the SFX or model package) at rest
/// pose (presentation; the tree is not animated).
fn missile_parts(sources: &Sources, libs: &Libraries, throw: &(String, String)) -> Model {
    let (Some(p), Some(lib)) = (sources.packages.get(&throw.0), libs.0.get(&throw.0)) else { return Vec::new() };
    let Some(tree) = p.anim.as_ref().and_then(|a| a.trees.iter().find(|t| t.label == throw.1)) else { return Vec::new() };
    let mut out = Vec::new();
    for node in &tree.nodes {
        tree_node_model(p, lib, tree, node, Vec3::from_array(node.initial_position), &mut out);
    }
    out
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
    let active: [bool; 4] = std::array::from_fn(|k| k == 0 || (1 << k) & sim.game.globals.active_players != 0);
    let poses: Vec<_> = (0..4)
        .map(|k| {
            let r = &sim.game.players[k];
            let pc = sources.players[k].as_ref()?;
            active[k].then(|| gdl_client::rig_pose(&pc.clips, &pc.clip_bytes, r.u16_(0xa2) as usize, r.f32_(0xac)).ok()).flatten()
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
        // MBCameraUpdate's window eye and rows (the StdCam or, on boss
        // levels, the BossCam record copied into the pb camera).
        let eye = Vec3::from_array(cam.pb.eye);
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
             P1 move: arrows/WASD  throw: Space/K  potion: E/L | P2 (join: U = Start) TFGH, R, Y | pads: D-pad, South, East | Esc/P/Start: menu  F1",
            gdl_client::menu::COLOUR_NAMES.get(p.u32_(4) as usize).copied().unwrap_or("?"),
            gdl_client::menu::CLASS_NAMES.get(p.u32_(0xc) as usize).copied().unwrap_or("SECRET"),
            p.i32_(0x2ac0),
            p.i32_(0x1a1c),
            session.slot,
            health,
            p.i32_(0x1a20),
            p.i32_(0x1a14),
            p.i32_(0x1a18),
            session.title,
            session.levels[session.current].name,
            // The class kill counters (+0xb2c / +0xb3c, as the tally reads them).
            {
                let c = p.u32_(0xc) as usize;
                p.i32_(0xb2c + 0x1c * c) + p.i32_(0xb3c + 0x1c * c)
            },
            live,
            generators,
            sim.last_event,
        );
        if !ui.details {
            t.clear();
        }
        if ui.details {
            t.push_str(&format!(
                "\n\nINTERIM BUILD (the M12 slice is not accepted). player updates {}\n\
                 Native, compared with reference traces: players, enemies, items, effects, world animation, camera\n\
                 Approximate: session globals start from the capture, audio mixing, the GS's per-draw truncation in lighting\n\
                 F9: debug skip to next level{}",
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
fn init_hud(game: &mut Game, sources: &Sources, session: &Session, front: &Front, sets: &mut HudSets, panels: bool) {
    let (catalog, english) = (&front.catalog, &front.english);
    let mut keys = vec!["STATIC".to_string()];
    // The players' model packages ("#k"), then the level's.
    keys.extend((0..4).filter(|&k| sources.players[k].is_some()).map(|k| format!("#{k}")));
    keys.extend(level_packages(&session.levels[session.current]).into_iter().filter(|k| k != "STATIC"));
    keys.retain(|k| k.starts_with('#') || sources.packages.contains_key(k));
    // init_shop's SelectLoadStart: the shop and select art (S1/S2_PLYRn,
    // SHOP_TOP_*, SHP_*) last in the search order.
    keys.push("SELECT".to_string());
    let tex: Vec<_> = keys
        .iter()
        .map(|k| {
            let pkg = if let Some(n) = k.strip_prefix('#') {
                &sources.players[n.parse::<usize>().unwrap_or(0)].as_ref().expect("player character").character
            } else if k == "SELECT" {
                &front.select_pkg
            } else {
                &sources.packages[k]
            };
            gdl_client::hud_texture_set(k, &pkg.objects)
        })
        .collect();
    let fonts = load_fonts(catalog, game.hud.tables.as_ref().map(|t| t.font_desc.clone()).unwrap_or_default());
    if panels {
        game.hud_init(tex, fonts, Some(english.clone()));
    } else {
        game.hud_resources(tex, fonts, Some(english.clone()));
    }
    game.hud.scrolls = Some(front.scrolls.clone());
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
        Page::Select { at } => Column::Menu { title: Vec::new(), entries: front.menu.select_entries(), selected: *at, file_list: true },
        Page::Class { at } => Column::Class { class: *at, colour: 0 },
        Page::Colour { class, at } => Column::Class { class: *class as usize, colour: *at },
        Page::Name { class, colour, name } => Column::Name { class: *class as usize, colour: *colour as usize, name: name.clone() },
        _ => return None,
    };
    Some([first, Column::Empty, Column::Empty, Column::Empty])
}

/// The pause menu's Inventory page (draw_fullscreen_inventory).
fn inventory_view(front: &Front, screen: Screen, ui: &Ui) -> bool {
    screen == Screen::Playing && ui.paused && matches!(front.menu.page, gdl_client::menu::Page::Inventory | gdl_client::menu::Page::InventoryClose | gdl_client::menu::Page::Shop { .. })
}

/// The option menus' ICON_ARROW selector (start_optmenu_nostack: an
/// MBNewNode parent of POWERUPS' ICON_ARROW tree, scale +0x3c;
/// show_optmenu: MBScreenToWorld(item x + icon x, selected entry y +
/// FontHeight/2, optmenu_icon_z 1.1), the camera's inverse rotation, then
/// PitchMat3(π for odd selections + π·t/OPTMENU_ICON_TIME while gliding to
/// a new entry over OPTMENU_ICON_TIME vblanks), AnimateATree). Drawn by an
/// offscreen camera in PS2 view space and composited as a HUD quad over the
/// menu blits (the arrow's depth 1.1 is in front of every blit).
#[derive(Resource, Default)]
struct MenuIcon {
    shown: bool,
    key: &'static str,
    /// Screen point of this frame (512 × 384 units).
    x: i32,
    y: i32,
    /// +0xd4 settled y, +0xd8 glide vblanks (0: settled).
    last_y: i32,
    timer: i32,
    vb: f32,
    pitch: f32,
    scale: f32,
    z: f32,
    /// AnimateATree's clock (seconds shown).
    clock: f32,
}

impl MenuIcon {
    fn update(&mut self, target: Option<(i32, i32)>, key: &'static str, selected: usize, style: &gdl_core::game::optmenu::OptStyle, scale: f32, dt: f32) {
        let Some((x, ty)) = target else {
            self.shown = false;
            self.key = "";
            return;
        };
        if self.key != key || !self.shown {
            // start_optmenu_nostack: +0xd4 = 0, +0xd8 = OPTMENU_ICON_TIME.
            self.key = key;
            self.last_y = 0;
            self.timer = style.icon_time;
            self.clock = 0.0;
            self.vb = 0.0;
        }
        self.shown = true;
        self.clock += dt;
        self.vb += dt * 59.94;
        let vb = self.vb.floor() as i32;
        self.vb -= vb as f32;
        let n = style.icon_time.max(1);
        let mut pitch = if selected & 1 != 0 { std::f32::consts::PI } else { 0.0 };
        let mut y = ty;
        if self.timer == 0 {
            if ty != self.last_y {
                self.timer = 1;
                y = self.last_y;
            }
        } else if self.timer < n {
            self.timer += vb;
            y = (ty - self.last_y) * self.timer / n + self.last_y;
            pitch += self.timer as f32 * std::f32::consts::PI / n as f32;
        } else {
            self.last_y = ty;
            self.timer = 0;
        }
        self.x = x;
        self.y = y;
        self.pitch = pitch;
        self.scale = scale;
        self.z = style.icon_z;
    }
}

/// The icon's offscreen view: render layer, target and parts.
const ICON_LAYER: usize = 2;
#[derive(Component)]
struct MenuIconNode;
#[derive(Component)]
struct MenuIconQuad;
#[derive(Resource)]
struct MenuIconTree {
    tree: usize,
    timing: i16,
}

/// The PS2 window: focal length 554.27 (pb +0x90 × viewport scale). The
/// icon is drawn over the whole 640 × 448 frame centred on row 224;
/// MBScreenToWorld's in-game centre is row 192 (viewport 2016). Placement
/// round-trips through either; only the arrow's slight perspective differs
/// (front-end window values are not captured).
const ICON_FOCAL: f32 = 554.2563;

fn setup_menu_icon(
    mut commands: Commands,
    front: Res<Front>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
    mut blits: ResMut<Assets<gdl_client::BlitMaterial>>,
) {
    let pkg = &front.powerups_pkg;
    let Some((tree_index, tree)) = pkg.anim.as_ref().and_then(|a| a.trees.iter().enumerate().find(|(_, t)| t.label == "ICON_ARROW")) else { return };
    let lib = build_library(pkg, &mut meshes, &mut images, &mut materials);
    let mut model = Vec::new();
    for node in &tree.nodes {
        tree_node_model(pkg, &lib, tree, node, Vec3::from_array(node.initial_position), &mut model);
    }
    let target = images.add(Image::new_target_texture(640, 448, bevy::render::render_resource::TextureFormat::Rgba8Unorm, None));
    let layer = bevy::camera::visibility::RenderLayers::layer(ICON_LAYER);
    commands.spawn((
        Camera3d::default(),
        Camera { order: -1, clear_color: ClearColorConfig::Custom(Color::NONE), ..default() },
        bevy::camera::RenderTarget::Image(target.clone().into()),
        Tonemapping::None,
        bevy::render::view::Hdr,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        Projection::custom(Ps2Projection { tan_h: 320.0 / ICON_FOCAL, tan_v: 224.0 / ICON_FOCAL, near: 0.05, far: 100.0 }),
        Transform::default(),
        layer.clone(),
    ));
    // Placed in MB view space (x right, y up, z forward) mirrored into Bevy's
    // (−z forward) by present_menu_icon.
    let root = commands.spawn((Transform::default(), Visibility::Hidden, MenuIconNode, layer.clone())).id();
    for (at, parts, _, idx, extra) in model {
        if matches!(extra, Some(NodeExtra::Psys(_))) {
            continue;
        }
        let Some(idx) = idx else { continue };
        let holder = commands.spawn((TreeNodeIdx(idx), Transform::from_translation(at), Visibility::Inherited, layer.clone())).id();
        commands.entity(root).add_child(holder);
        for (mesh, material) in parts {
            let c = commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default(), layer.clone(), bevy::camera::visibility::NoFrustumCulling)).id();
            commands.entity(holder).add_child(c);
        }
    }
    // HYPOTHESIS (as effect trees): AnimateATree plays sequence 0 at its
    // timing factor.
    commands.insert_resource(MenuIconTree { tree: tree_index, timing: tree.sequences.first().map_or(30, |q| q.timing_factor) });
    // The whole 640 × 448 frame (menu entries reach into the panels' 64-row
    // inset) in the 512 × 384 HUD space.
    let h = 384.0;
    let mat = blits.add(gdl_client::BlitMaterial {
        params: gdl_client::BlitParams { rgba: Vec4::new(1.0, 1.0, 1.0, 128.0 / 255.0), crop: Vec4::new(0.0, 1.0, 0.0, 1.0), mask_rect: Vec4::ZERO },
        texture: Some(target),
        mask: None,
    });
    let quad = meshes.add(Rectangle::new(1.0, 1.0));
    commands.spawn((
        Mesh2d(quad),
        MeshMaterial2d(mat),
        Transform::from_xyz(0.0, 192.0 - h * 0.5, 50.0).with_scale(Vec3::new(512.0, h, 1.0)),
        Visibility::Hidden,
        bevy::camera::visibility::RenderLayers::layer(HUD_LAYER),
        MenuIconQuad,
    ));
}

/// Place, turn and animate the icon (sequence 0 at the tree's timing).
#[allow(clippy::type_complexity)]
fn present_menu_icon(
    icon: Res<MenuIcon>,
    front: Res<Front>,
    tree: Option<Res<MenuIconTree>>,
    mut root: Query<(&mut Transform, &mut Visibility, &Children), (With<MenuIconNode>, Without<MenuIconQuad>, Without<TreeNodeIdx>)>,
    mut parts: Query<(&TreeNodeIdx, &mut Transform), (Without<MenuIconNode>, Without<MenuIconQuad>)>,
    mut quad: Query<&mut Visibility, (With<MenuIconQuad>, Without<MenuIconNode>)>,
) {
    let want = if icon.shown { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut quad {
        if *v != want {
            *v = want;
        }
    }
    let (Ok((mut tf, mut vis, kids)), Some(tree)) = (root.single_mut(), tree) else { return };
    if *vis != want {
        *vis = want;
    }
    if !icon.shown {
        return;
    }
    // MBScreenToWorld into view space (window 640 × 448 over the 512 × 384
    // screen; viewport centre 2048/2016 against the 2048 offsets).
    let z = icon.z;
    let xv = (1.25 * icon.x as f32 - 320.0) * z / ICON_FOCAL;
    let yv = (224.0 - 448.0 / 384.0 * icon.y as f32) * z / ICON_FOCAL;
    // PitchMat3 on the camera-aligned rows (local y → (0, c, −s): Rx(−θ) in
    // MB view space); conjugated by the view mirror diag(1, 1, −1):
    // T(x, y, −z) · Rx(θ) · diag(s, s, −s).
    *tf = Transform::from_translation(Vec3::new(xv, yv, -z)).with_rotation(Quat::from_rotation_x(icon.pitch)).with_scale(Vec3::new(icon.scale, icon.scale, -icon.scale));
    let Some(anim) = front.powerups_pkg.anim.as_ref() else { return };
    let frame = icon.clock * 900.0 / f32::from(tree.timing.max(1));
    let Ok(pose) = gdl_client::tree_pose(anim, tree.tree, &front.powerups_pkg.anim_bytes, 0, frame, true) else { return };
    for k in kids.iter() {
        if let Ok((idx, mut t)) = parts.get_mut(k)
            && let Some(m) = pose.get(idx.0)
        {
            *t = Transform::from_matrix(*m);
        }
    }
}

/// pbScreenSetAdjust(screen_dx, screen_dy) as the final pass's shift.
fn present_screen_adjust(front: Res<Front>, mut q: Query<&mut gdl_client::DisplayToLinear>) {
    let (dx, dy) = front.menu.screen;
    let (x, y) = (dx as f32 / 4.0 / 640.0, dy as f32 * 2.0 / 448.0);
    for mut d in &mut q {
        if d.offset_x != x || d.offset_y != y {
            d.offset_x = x;
            d.offset_y = y;
        }
    }
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
    mut icon: ResMut<MenuIcon>,
) {
    let playing = *screen == Screen::Playing;
    let mut list = Vec::new();
    if let Some(sim) = sim.as_ref().filter(|_| playing) {
        list = sim.game.hud.draw_list();
        if sim.shop.is_none() && sim.map.is_none() {
            list.extend(sim.game.health_meter_blits());
            list.extend(sim.game.thermometer_blits());
            list.extend(sim.game.wave_timer_blits());
        }
        // ScrollMessageBox runs (modal) before the trigger camera it precedes.
        if sim.game.trigger_view_active() && sim.message_box.is_none() {
            list.extend(sim.game.hud.letterbox_blits());
        }
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
        let mut menu = front.menus[key].clone();
        // start_optmenu's marks: the current choice (DoOptions 0xa..0x11).
        if let gdl_client::menu::Page::Choice { key: k, .. } = &front.menu.page {
            let cur = front.menu.choice_value(k);
            for (i, e) in menu.entries.iter_mut().enumerate() {
                e.mark = i32::from(i == cur);
            }
        }
        if let gdl_client::menu::Page::ControlStyle { style, .. } = &front.menu.page
            && let Some(ctl) = front.controls_menu.as_ref()
        {
            // DoOptions 0xe: the entry shows opt_controls_desc[style].
            if let (Some(e), Some(name)) = (menu.entries.first_mut(), ctl.styles.get(*style)) {
                e.label = name.clone();
            }
            front_list.extend(front.ui_hud.controls_blits(ctl, *style));
        }
        if let gdl_client::menu::Page::Screen { .. } = &front.menu.page {
            front_list.extend(front.ui_hud.screen_adjust_blits());
        }
        if let gdl_client::menu::Page::HintText { list, at } = &front.menu.page {
            // DoOptions case 0x16: "A Hint for You" (general) or the
            // category's *HINTDESCS line (the guardian, legend item, rune).
            menu.title = Some(if *list == 0 { "A Hint for You".to_string() } else { front.menu.hint_title.clone().unwrap_or_default() });
            if let Some(text) = front.menu.hints.get(*list).and_then(|h| h.get(*at)) {
                // DrawScrollListText(hints, x −256, font 6) at DrawStringScale
                // = the menu scale × the ROM entry's scale, in the menu's
                // "off" colour (NATIVE: the block is centred at 0xd0).
                let c = menu.rgb[0];
                let colour = (c[0].clamp(0, 255) as u32) << 16 | (c[1].clamp(0, 255) as u32) << 8 | c[2].clamp(0, 255) as u32;
                let lines: Vec<&str> = text.split('\n').collect();
                front_list.extend(front.ui_hud.text_block_blits(menu.scale * front.menu.hint_scale, 0xd0, 6, colour, &lines, 0));
            }
        }
        front_list.extend(front.ui_hud.optmenu_blits(style, &menu, &state));
        icon.update(front.ui_hud.optmenu_icon_target(style, &menu, &state), key, at, style, menu.icon_scale, time.delta_secs());
    } else {
        icon.shown = false;
        icon.key = "";
    }
    if let (Some(cols), Some(style), Some(def)) = (select_columns(&front, *screen), front.opt_style.as_ref(), front.menus.get("sel_def_menu")) {
        front_list.extend(front.ui_hud.select_blits(style, def, &cols, front.menu_clock.1 as i32));
    }
    if inventory_view(&front, *screen, &ui)
        && let Some(sim) = sim.as_ref()
    {
        let max = front.ui_hud.tables.as_ref().map(|t| t.max_crystals.clone()).unwrap_or_default();
        let rate = front.ui_hud.tables.as_ref().map_or(1, |t| t.swirl_rate);
        if let gdl_client::menu::Page::Shop { at } = front.menu.page {
            front_list.extend(front.ui_hud.shop_blits(&front.shop, &sim.game.players, 0, at, front.menu_clock.1 as i32));
        } else {
            // end_fullscreen_inventory: panel_fx 2 for 15 SWIRL_RATE units,
            // then the Tower menu.
            let closing = matches!(front.menu.page, gdl_client::menu::Page::InventoryClose);
            let t = front.menu_clock.1 as i32 * rate;
            let exit = if closing {
                let c = front.inventory_close.get_or_insert(0.0);
                let e = *c as i32 * rate;
                *c += time.delta_secs() * 59.94;
                Some(e)
            } else {
                front.inventory_close = None;
                None
            };
            front_list.extend(front.ui_hud.inventory_blits(&sim.game.players, &max, t, exit));
            if exit.is_some_and(|e| 0xe < e) {
                front.menu.page = gdl_client::menu::Page::Paused { tower: true, at: 3 };
            }
        }
    }
    if *screen == Screen::Front && front.attract_run.is_some() {
        front_list.extend(front.attract_blits.clone());
    } else if *screen == Screen::Front {
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
                // Front sets: STATIC, TITLE, SELECT, then SONY, GROUP1, CREDITS.
                let k = t.set as usize - 100;
                [&front.static_pkg, &front.title_pkg, &front.select_pkg].get(k).copied().or_else(|| front.attract_pkgs.get(k - 3))
            } else if key_name == "STATIC" {
                Some(&front.static_pkg)
            } else if key_name == "SELECT" {
                Some(&front.select_pkg)
            } else if let Some(n) = key_name.strip_prefix('#') {
                sources.as_ref().and_then(|s| s.players.get(n.parse::<usize>().unwrap_or(0))?.as_ref()).map(|p| &p.character)
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

/// load_player_model_sub / load_player_model: the model package
/// players/<class>/<colour><tier>0 (base classes), players/<class>/<colour>
/// (classes 8..16) or players/<class>/<descriptor code> (Hidden entries,
/// Sumner's "sum"); the clips and SFX of the base class (class & 7):
/// players/<base>/anim, players/<base>/sfx<colour>; the throw tree from
/// PlayerMissileDesc[class].
fn character_assets(front: &Front, key: CharKey) -> Result<CharacterAssets, Box<dyn Error>> {
    let class = key.class;
    let base = if 7 < class { class - 8 } else { class };
    let cls = gdl_core::player_class::CLASS_TAGS.get(class as usize).ok_or("class")?.to_uppercase();
    let bcls = gdl_core::player_class::CLASS_TAGS.get(base as usize).ok_or("class")?.to_uppercase();
    let col = COLOUR_TAGS.get(key.colour).ok_or("colour")?;
    let code = (key.desc != 0)
        .then(|| {
            let elf = gdl_formats::elf::Elf::parse(&front.elf_bytes).ok()?;
            let b = elf.read(key.desc, 8).ok()?;
            Some(String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(8)]).to_uppercase())
        })
        .flatten();
    let model_name = match &code {
        Some(c) => format!("PLAYERS/{cls}/{c}"),
        None if class < 8 => format!("PLAYERS/{cls}/{col}{}0", key.tier),
        None => format!("PLAYERS/{cls}/{col}"),
    };
    let sfx_name = format!("PLAYERS/{bcls}/SFX{col}");
    let clips_label = format!("PLAYERS/{bcls}/ANIM/ANIM.PS2");
    front.catalog.read(&clips_label)?;
    let desc = front.missile_desc.get(class as usize).ok_or("missile desc")?;
    let name = String::from_utf8_lossy(&desc[..desc.iter().position(|&c| c == 0).unwrap_or(3)]).into_owned();
    let ch = desc.get(4 + key.tier as usize).copied().unwrap_or(b'1');
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

/// Load player `k`'s character for `key` into the sources (packages and
/// libraries for its model and SFX).
fn install_char(front: &Front, sources: &mut Sources, libraries: &mut Libraries, (meshes, images, materials): (&mut Assets<Mesh>, &mut Assets<Image>, &mut Assets<gdl_client::GsMaterial>), k: usize, key: CharKey) -> Result<(), Box<dyn Error>> {
    let (pc, model, sfx) = load_char(front, key)?;
    for (name, pkg) in [(pc.model_name.clone(), model), (pc.sfx_name.clone(), sfx)] {
        if !libraries.0.contains_key(&name) {
            libraries.0.insert(name.clone(), build_library(&pkg, meshes, images, materials));
        }
        sources.packages.entry(name).or_insert(pkg);
    }
    sources.players[k] = Some(pc);
    Ok(())
}

/// The object labels of the players' models and SFX packages (MBOX tables
/// the level searches).
fn character_labels(sources: &Sources) -> Vec<String> {
    let mut out = Vec::new();
    for pc in sources.players.iter().flatten() {
        out.extend(pc.character.objects.definitions.iter().map(|d| d.label.clone()));
        if let Some(p) = sources.packages.get(&pc.sfx_name) {
            out.extend(p.objects.definitions.iter().map(|d| d.label.clone()));
        }
    }
    out
}

/// A class's ClassData: its PDAT and SFXX/DAMG tables.
fn class_data_of(front: &Front, class: u32) -> Result<ClassData, Box<dyn Error>> {
    let mut c = ClassData::from_pdat(front.pdats.get(class).ok_or("PDAT")?)?;
    let (sfxx, damg) = front.class_fx.get(class as usize).cloned().unwrap_or_default();
    c.sfxx = sfxx;
    c.damg = damg;
    Ok(c)
}

/// ClassData with player `k`'s SFX package trees (InitCustomEffect's own
/// package for WeapThrowFx).
fn with_sfx_trees(mut c: ClassData, sources: &Sources, k: usize) -> ClassData {
    c.sfx_trees = sources.players[k]
        .as_ref()
        .and_then(|pc| sources.packages.get(&pc.sfx_name))
        .and_then(|pkg| pkg.anim.as_ref())
        .map(|a| a.trees.iter().map(|t| (t.label.clone(), t.sequences.first().map_or(0, |s| s.frames), t.sequences.first().map_or(0, |s| s.timing_factor))).collect())
        .unwrap_or_default();
    c
}

/// A player's character loaded for `key`, with its model and SFX packages
/// (for the caller's package cache and libraries).
fn load_char(front: &Front, key: CharKey) -> Result<(PlayerChar, Package, Package), Box<dyn Error>> {
    let ch = character_assets(front, key)?;
    let (character, rig, clips, clip_bytes) = character_rig(front, &ch)?;
    let sfx = load_package(&ch.sfx)?;
    let model = load_package(&ch.model)?;
    Ok((PlayerChar { key, character, rig, clips, clip_bytes, model_name: ch.model_name, sfx_name: ch.sfx_name, throw: ch.throw }, model, sfx))
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
    start_session(world, Some(slot))
}

/// The title's Start (do_titlescreen → init_player_select(0)): the Tower
/// loads behind the select screen with player 1 (port 0) in, choosing.
fn start_select(world: &mut EcsWorld) -> Result<(), Box<dyn Error>> {
    start_session(world, None)
}

/// A session, its sources and the slot starts build_game takes.
type SessionParts = (Session, Sources, [Option<SlotStart>; 4]);

/// The session for level `idx` with player 1's `record` (the other players
/// out): packages, sources and the slot starts build_game takes.
fn session_for(front: &Front, record: &Record, controls: profile::ControlOptions, idx: usize, slot: u32) -> Result<SessionParts, Box<dyn Error>> {
    let (pc, model_pkg, sfx_pkg) = load_char(front, CharKey::of(record))?;
    let b = &front.base;
    let first = &b.levels[idx];
    let mut packages = HashMap::new();
    let first_labels = load_level_packages(&mut packages, &b.package_cfg, first)?;
    let character_labels: Vec<String> = sfx_pkg.objects.definitions.iter().chain(pc.character.objects.definitions.iter()).map(|d| d.label.clone()).collect();
    packages.insert(pc.sfx_name.clone(), sfx_pkg);
    packages.insert(pc.model_name.clone(), model_pkg);
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
        tower_start: None,
        prefs: front.menu.prefs,
        current: idx,
        monster_paths: b.monster_paths.clone(),
        anim_paths: b.anim_paths.clone(),
        audio_rom: b.audio_rom.clone(),
        object_labels: first_labels.into_iter().chain(b.shared_labels.iter().cloned()).chain(character_labels).collect(),
        banner: None,
        next: None,
        finished: None,
        slot,
        controls,
        pdats: front.pdats.clone(),
        heart_rate: front.heart_rate,
        carried: [0, -1, -1, -1, -1, 0xd, 0],
    };
    let sources = Sources { players: [Some(pc), None, None, None], packages, monsters: b.monsters.clone() };
    let mut records: [Record; 4] = std::array::from_fn(|_| Record::zeroed(record.0.len()));
    records[0] = record.clone();
    let mut controls4 = [[0u32; 4]; 4];
    controls4[0] = session.controls;
    let slots = slot_starts(front, &sources, &records, &controls4)?;
    Ok((session, sources, slots))
}

/// Build the session and the Tower game: from save `slot`, or (None) with a
/// cleared player 1 who enters the select screen.
fn start_session(world: &mut EcsWorld, from: Option<u32>) -> Result<(), Box<dyn Error>> {
    let mut hud_sets = HudSets::default();
    let (session, sources, game, recovered) = {
        let front = world.resource::<Front>();
        let (record, controls, recovered, slot) = match from {
            Some(slot) => {
                let loaded = front.store.load(slot)?;
                let region = loaded.save.region()?;
                let (record, controls) = profile::load_character(0, &region, &front.pdats, &front.tables).map_err(|e| e.to_string())?;
                (record, controls, loaded.recovered, slot)
            }
            None => {
                let mut record = Record::zeroed(0x2b00);
                profile::clear_player(&mut record, true, false, &front.pdats).map_err(|e| e.to_string())?;
                (record, [0, 2, 1, 1], None, 0)
            }
        };
        let b = &front.base;
        // DEBUG (automated render comparisons): GDL_DEBUG_LEVEL=<name> starts
        // in that level instead of the Tower. Not a game feature.
        let debug_level = std::env::var("GDL_DEBUG_LEVEL").ok();
        let idx = debug_level
            .and_then(|n| b.levels.iter().position(|l| l.name == n))
            .or_else(|| b.levels.iter().position(|l| l.world == 0xd))
            .unwrap_or(0);
        if let Ok(n) = std::env::var("GDL_DEBUG_LEVEL") {
            eprintln!("DEBUG level {n}: starting in {} (index {idx})", b.levels[idx].name);
        }
        let (session, sources, slots) = session_for(front, &record, controls, idx, slot)?;
        // The Tower is entered from the select screen: no player is in the
        // game when GauntWorldInit's world_update runs.
        let from_select = (b.levels[idx].world == 0xd).then_some(0);
        let mut game = build_game(&session, idx, &slots, false, from_select)?;
        // player_get_from_save: the saved control options into PlayerControl
        // (+0x2c style, +0x30 vibration, +0x38 auto attack, +0x34 auto aim).
        for (o, v) in [0x2c, 0x30, 0x38, 0x34].into_iter().zip(session.controls) {
            game.controls.control[0].set_u32(o, v);
        }
        init_hud(&mut game, &sources, &session, front, &mut hud_sets, true);
        (session, sources, game, recovered)
    };
    let attention = Vec3::from_array(game.players[0].v3(0x810));
    let stage = game.sounds.stage.as_ref().map(|s| s.stream_name(0, 0));
    world.resource_mut::<Presentation>().attention = attention;
    world.insert_resource(sources);
    world.insert_resource(session);
    world.insert_resource(hud_sets);
    let mut game = game;
    let mut select = None;
    if from.is_none() {
        // init_player_select(0): player 1's port pressed Start on the title.
        // The Tower only loads behind the select screen (LoadTowerAndSelect);
        // init_next_level enters it afterwards, so this build's EnterTower
        // does not count.
        game.globals.tower_counter = world.resource::<Session>().carried[0];
        game.globals.active_players |= 1;
        game.players[0].set_i32(0xfc, 0);
        let pdats = world.resource::<Session>().pdats.clone();
        select = Some(game.init_player_select(0, &pdats).map_err(|e| e.0)?);
    }
    world.insert_resource(Sim { game, error: None, updates: 0, exp: 0, sparks: Vec::new(), last_event: String::new(), message_box: None, hint_menu: false, scroll_clock: 0.0, shop: None, map: None, shop_done: None, debug_skip: false, select, select_done: false, select_quit: false, select_next: None, map_done: None, movie: None, movie_done: None, over: None, over_done: false, flyby: None, flyby_done: false });
    world.run_system_once(setup).map_err(|e| format!("{e:?}"))?;
    // The stage music starts from the level's BGMusicStart request.
    let _ = stage;
    if let Some(mut a) = world.get_non_send_resource_mut::<gdl_client::audio::AudioOut>() {
        a.stop_all();
    }
    if let Some((path, why)) = recovered {
        world.resource_mut::<Sim>().last_event = format!("save recovered from {} ({why})", path.display());
    }
    *world.resource_mut::<Ui>() = Ui { started: true, paused: false, details: false };
    *world.resource_mut::<Screen>() = Screen::Playing;
    Ok(())
}

/// init_attract_mode's flyby entry (init_flyby): the attract level `idx`
/// with no player in play (player 1's record cleared and inactive; the
/// game's HUD gets textures and fonts only) under the old camera.
fn start_flyby(world: &mut EcsWorld, idx: usize) -> Result<(), Box<dyn Error>> {
    let mut hud_sets = HudSets::default();
    let (session, sources, game, flyby) = {
        let front = world.resource::<Front>();
        let mut record = Record::zeroed(0x2b00);
        profile::clear_player(&mut record, true, false, &front.pdats).map_err(|e| e.to_string())?;
        let (session, sources, slots) = session_for(front, &record, [0, 2, 1, 1], idx, 0)?;
        let mut game = build_game(&session, idx, &slots, false, None)?;
        init_hud(&mut game, &sources, &session, front, &mut hud_sets, false);
        let elf = gdl_formats::elf::Elf::parse(&front.elf_bytes)?;
        let yaw_off0 = elf.object("yaw_off").ok().and_then(|b| b.get(..4)).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap_or([0; 4])));
        // SetMaxFPS(30): vb_elapsed 2 at the attract's 30 Hz.
        game.globals.vb_elapsed = 2;
        let flyby = game.init_flyby(front.attract.transmitters, yaw_off0).map_err(|e| e.0)?;
        (session, sources, game, flyby)
    };
    world.insert_resource(sources);
    world.insert_resource(session);
    world.insert_resource(hud_sets);
    world.insert_resource(Sim { game, error: None, updates: 0, exp: 0, sparks: Vec::new(), last_event: String::new(), message_box: None, hint_menu: false, scroll_clock: 0.0, shop: None, map: None, shop_done: None, debug_skip: false, select: None, select_done: false, select_quit: false, select_next: None, map_done: None, movie: None, movie_done: None, over: None, over_done: false, flyby: Some(flyby), flyby_done: false });
    world.run_system_once(setup).map_err(|e| format!("{e:?}"))?;
    if let Some(mut a) = world.get_non_send_resource_mut::<gdl_client::audio::AudioOut>() {
        a.stop_all();
    }
    *world.resource_mut::<Ui>() = Ui { started: true, paused: false, details: false };
    *world.resource_mut::<Screen>() = Screen::Playing;
    Ok(())
}

/// The flyby's end (exit_attract / Start): the level goes, the transmitter
/// globals stay with the attract sequence.
fn end_flyby(world: &mut EcsWorld) {
    let t = world.get_resource::<Sim>().and_then(|s| s.flyby.as_ref().map(|f| (f.transmitter1, f.transmitter2)));
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
    if let Some(t) = t {
        world.resource_mut::<Front>().attract.transmitters = t;
    }
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
            // NATIVE: the circle button's Center (gamepad East is Back here).
            (&[KeyCode::KeyC][..], Key::Center),
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
            Action::StartSelect => {
                // do_titlescreen's start: HintMenu(-1) (hints_inited = 0).
                world.resource_mut::<Front>().hints = Default::default();
                start_select(world)?
            }
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
            Action::ManageCharacter => {
                // DoOptions 0xd: init_player_select(1); the player who chose
                // it starts at menu 1 (state 2).
                world.resource_mut::<Ui>().paused = false;
                let pdats = world.resource::<Session>().pdats.clone();
                let mut sim = world.resource_mut::<Sim>();
                match sim.game.init_player_select(1, &pdats) {
                    Ok(ps) => {
                        sim.game.players[0].set_i32(0xfc, 2);
                        sim.game.players[0].set_i32(0x2ad4, 1);
                        sim.select = Some(ps);
                    }
                    Err(e) => sim.error = Some(format!("init_player_select: {}", e.0)),
                }
            }
            Action::ShowHint { list } => {
                // DoOptions case 0x16: next_*_hint(1), the title (GetScrollText
                // of BOSSHINTDESCS / LEGENDHINTDESCS / RUNEHINTDESCS) and the
                // lines DrawScrollListText draws (HINTS_E.ROM lists 0..3).
                let mut hints = std::mem::take(&mut world.resource_mut::<Front>().hints);
                let shown = {
                    let front = world.resource::<Front>();
                    let rom = &front.hints_rom;
                    let general = |i: i32| -> i32 {
                        rom.lists.get(3).and_then(|(_, l)| l.get(usize::try_from(i).ok()?)).and_then(|&e| rom.entries.get(e)).map_or(0, |e| e.lines.len() as i32)
                    };
                    world.get_resource::<Sim>().map(|sim| sim.game.next_hint(&mut hints, list, &general))
                };
                let mut front = world.resource_mut::<Front>();
                front.hints = hints;
                if let Some(h) = shown {
                    let rom = &front.hints_rom;
                    let entry = rom.lists.get(h.list).and_then(|(_, l)| l.get(h.entry)).and_then(|&e| rom.entries.get(e));
                    let lines: Vec<String> = h.lines.iter().filter_map(|&k| entry.and_then(|e| e.lines.get(k)).map(|l| l.replace('\r', ""))).collect();
                    let title = h.title.and_then(|(e, l)| rom.entries.get(e).and_then(|e| e.lines.get(l)).map(|t| t.replace('\r', "")));
                    let scale = entry.map_or(1.0, |e| e.scale);
                    // NATIVE spacing: one empty line between hint lines (the
                    // original steps stringtext_y + 16).
                    let text = lines.join("\n\n");
                    let n = front.menu.hints.len().max(list + 1);
                    front.menu.hints.resize(n, Vec::new());
                    front.menu.hints[list] = vec![text];
                    front.menu.hint_title = title;
                    front.menu.hint_scale = scale;
                    front.menu.page = gdl_client::menu::Page::HintText { list, at: 0 };
                }
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
            // quit_game → OptionsDone → init_over (the game-over sequence).
            Action::QuitGame => {
                world.resource_mut::<Ui>().paused = false;
                if let Some(mut sim) = world.get_resource_mut::<Sim>() {
                    if sim.over.is_none() {
                        let over = sim.game.init_over();
                        sim.over = Some(over);
                    }
                } else {
                    end_game(world);
                }
            }
            Action::OpenInventory => {
                let text = inventory_text(&world.resource::<Sim>().game.players[0]);
                let mut front = world.resource_mut::<Front>();
                front.menu.inventory = text;
                front.menu.status.clear();
                front.menu.page = gdl_client::menu::Page::Inventory;
            }
            Action::OpenShop => {
                // The Tower menu's shop: TowerSetStartPos, init_shop(1), and
                // init_next_level (the Tower again) once do_shop is done.
                let items = world.resource::<Front>().shop.clone();
                let (starts, current) = {
                    let session = world.resource::<Session>();
                    let starts: Vec<(i16, [f32; 3])> = session.world.locators.iter().filter(|l| l.kind == 7).map(|l| (l.id, l.position)).collect();
                    (starts, session.current)
                };
                let order: Vec<i32> = gdl_formats::elf::Elf::parse(&world.resource::<Session>().elf_bytes)
                    .ok()
                    .and_then(|e| e.object("WorldOrder").ok().map(|o| o.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()))
                    .unwrap_or_default();
                let id = world.resource::<Sim>().game.closest_start_pos(&starts, &order);
                world.resource_mut::<Session>().tower_start = Some(id);
                let flow = world.resource_mut::<Sim>().game.init_shop(1, &items).map_err(|e| e.0)?;
                world.resource_mut::<Sim>().shop = Some((flow, current));
                world.resource_mut::<Front>().menu.status.clear();
                world.resource_mut::<Ui>().paused = false;
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
            Action::Choose { key, idx } => {
                // DoOptions' choice menus: GamePrefs +0x14 difficulty, +0x18
                // compass, +0x1c multiplayer (saved with the options); the
                // player's PlayerControl +0x30 vibration, +0x34 auto aim and
                // +0x38 auto attack (On = entry 0 → 1), saved with the
                // character (+0x190c..). AudioPlayerStart is a cue.
                let pref = match key {
                    "difficulty_options" => Some(5),
                    "compass_options" => Some(6),
                    "multiplayer_options" => Some(7),
                    _ => None,
                };
                if let Some(w) = pref {
                    let mut front = world.resource_mut::<Front>();
                    front.menu.prefs[w] = idx as i32;
                    front.game_opts[4 * w..4 * w + 4].copy_from_slice(&(idx as i32).to_le_bytes());
                    let opts = front.game_opts.clone();
                    front.store.store_options(&opts)?;
                    let prefs = front.menu.prefs;
                    if let Some(mut session) = world.get_resource_mut::<Session>() {
                        session.prefs = prefs;
                    }
                }
                let ctl = match key {
                    "controlstyle_options" => Some((0, 0x2c, idx as u32)),
                    "control_rumble_options" => Some((1, 0x30, idx as u32)),
                    "control_autoattack_options" => Some((2, 0x38, u32::from(idx == 0))),
                    "control_autoaim_options" => Some((3, 0x34, u32::from(idx == 0))),
                    _ => None,
                };
                if let Some((i, off, v)) = ctl {
                    world.resource_mut::<Front>().menu.controls[i] = v;
                    if let Some(mut session) = world.get_resource_mut::<Session>() {
                        session.controls[i] = v;
                    }
                    if let Some(mut sim) = world.get_resource_mut::<Sim>() {
                        sim.game.controls.control[0].set_u32(off, v);
                    }
                }
            }
            Action::Screen { dx, dy, keep } => {
                // pbScreenSetAdjust; Accept stores GamePrefs words 3/4.
                if keep {
                    let mut front = world.resource_mut::<Front>();
                    front.game_opts[12..16].copy_from_slice(&dx.to_le_bytes());
                    front.game_opts[16..20].copy_from_slice(&dy.to_le_bytes());
                    let opts = front.game_opts.clone();
                    front.store.store_options(&opts)?;
                }
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
    if screen == Screen::Front
        && let Some(idx) = world.resource_mut::<Front>().flyby_request.take()
    {
        if let Err(e) = start_flyby(world, idx) {
            let mut front = world.resource_mut::<Front>();
            front.menu.status = format!("flyby: {e}");
            start_attract(&mut front, -1);
        }
        return;
    }
    if screen == Screen::Playing && world.get_resource::<Sim>().is_some_and(|s| s.flyby.is_some()) {
        // do_flyby: exit_attract → init_attract_mode(−1); Start (or accept)
        // leaves the attract sequence for the title screen.
        let start = toggle || keys.contains(&Key::Accept);
        if start || world.resource::<Sim>().flyby_done {
            end_flyby(world);
            let mut front = world.resource_mut::<Front>();
            if start {
                exit_attract(&mut front);
            } else {
                start_attract(&mut front, -1);
            }
        }
        return;
    }
    if screen == Screen::Front {
        // attract_advance / do_movie's new_start: Start (or accept) leaves
        // the attract sequence for the title screen.
        if world.resource::<Front>().attract_run.is_some() {
            if toggle || keys.contains(&Key::Accept) {
                exit_attract(&mut world.resource_mut::<Front>());
            }
            return;
        }
        // init_titlescreen_timer on any input on the title.
        if toggle || !keys.is_empty() {
            world.resource_mut::<Front>().title_timeout = 1800.0;
        }
    }
    if screen == Screen::Playing {
        // game_main: no active player left (each declined to continue) →
        // init_over; do_over's end: EndTower, fail_movie_show and
        // init_attract_mode(0x8002) (the FAIL movie, then the sequence).
        if world.get_resource::<Sim>().is_some_and(|s| s.over_done) {
            end_game(world);
            let mut front = world.resource_mut::<Front>();
            front.attract.fail_movie_show = true;
            start_attract(&mut front, 0x8002);
            return;
        }
        if world.get_resource::<Sim>().is_some_and(|s| s.over.is_none() && (0..4).all(|k| s.game.players[k].i32_(0xfc) == 0)) {
            let mut sim = world.resource_mut::<Sim>();
            let over = sim.game.init_over();
            sim.over = Some(over);
            return;
        }
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
            // HintMenu: next_general_hint(0) once, the others every time.
            let mut hints = std::mem::take(&mut world.resource_mut::<Front>().hints);
            world.resource::<Sim>().game.hint_menu_open(&mut hints);
            world.resource_mut::<Front>().hints = hints;
            world.resource_mut::<Front>().menu.page = gdl_client::menu::Page::Hints { at: 0 };
            world.resource_mut::<Ui>().paused = true;
            return;
        }
        let paused = world.resource::<Ui>().paused;
        // DoOptions: no menu before WelcomeFlag < 0, during the stage start,
        // or while player controls are disabled (PlayerControl's Start edge
        // is zero then: trigger cameras, speeches); nor in the shop or map.
        let blocked = world.get_resource::<Session>().is_some_and(|s| s.banner.is_some() || s.finished.is_some())
            || world.get_resource::<Sim>().is_some_and(|s| {
                let g = &s.game;
                s.shop.is_some() || s.map.is_some() || 0 <= g.globals.welcome_flag || g.globals.start_of_stage != 0 || g.disable_player_controls != 0 || g.good_wiz.optmenu_disable
            });
        if !paused {
            if toggle && !blocked {
                let tower = world.resource::<Session>().world_number == 0xd;
                let controls = world.resource::<Session>().controls;
                let mut front = world.resource_mut::<Front>();
                front.menu.controls = controls;
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
    let attract = front.attract_run.is_some();
    for (mut v, panel) in &mut backdrop {
        let want = if *screen == Screen::Front && !attract && !(panel && original) { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    let Ok((mut t, mut tf)) = text.single_mut() else { return };
    // On the title the menu sits in the lower part, below the logo.
    tf.translation.y = if *screen == Screen::Front { -300.0 } else { 0.0 };
    let show = (*screen == Screen::Front || ui.paused) && !attract && menu_view(&front, *screen, &ui).is_none() && select_columns(&front, *screen).is_none() && !inventory_view(&front, *screen, &ui);
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
    let mut wdata_cache: HashMap<String, gdl_formats::wdata::WorldData> = HashMap::new();
    for l in cfg["levels"].as_array().ok_or("config levels")? {
        let wd_path = l["wdata"].as_str().ok_or("level wdata")?.to_string();
        if !wdata_cache.contains_key(&wd_path) {
            wdata_cache.insert(wd_path.clone(), gdl_formats::wdata::WorldData::parse(&gdl_client::bounded_read(Path::new(&wd_path))?)?);
        }
        let movie = wdata_cache[&wd_path]
            .levels
            .get(l["index"].as_u64().ok_or("level index")? as usize)
            .and_then(|r| r.raw.get(0x34..0x44))
            .map(|m| String::from_utf8_lossy(&m[..m.iter().position(|&b| b == 0).unwrap_or(m.len())]).into_owned())
            .unwrap_or_default();
        levels.push(LevelSource {
            movie,
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
    let sel = gdl_core::game::pselect::SelTables::from_elf(&elf)?;
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
    // TEXT/ENGLISH.ROM (StringInit) for the front end's string lists.
    ui_hud.text = Some(english.clone());
    let fonts = load_fonts(&catalog, ui_hud.tables.as_ref().map(|t| t.font_desc.clone()).unwrap_or_default());
    let title_pkg = load_package(&package_entry(&catalog, "TITLE", true)?)?;
    let select_pkg = load_package(&package_entry(&catalog, "SELECT", true)?)?;
    let powerups_pkg = load_package(&package_entry(&catalog, "POWERUPS", true)?)?;
    ui_hud.set_resources(
        vec![gdl_client::hud_texture_set("STATIC", &static_pkg.objects), gdl_client::hud_texture_set("TITLE", &title_pkg.objects), gdl_client::hud_texture_set("SELECT", &select_pkg.objects)],
        fonts,
    );
    let opt_style = gdl_core::game::optmenu::OptStyle::from_elf(&elf).ok();
    let mut menus = HashMap::new();
    for n in [
        "title_menu",
        "title_options",
        "tower_menu",
        "game_menu",
        "tower_options",
        "game_options",
        "audio_options",
        "quitgame_question",
        "quitlevel_question",
        "hint_menu",
        "hint_general_menu",
        "sel_def_menu",
        "gameoption_options",
        "controls_options",
        "compass_options",
        "difficulty_options",
        "multiplayer_options",
        "control_rumble_options",
        "control_autoaim_options",
        "control_autoattack_options",
        "controlstyle_options",
        "screen_options",
    ] {
        if let Some(m) = elf.symbol(n).and_then(|sym| gdl_core::game::optmenu::OptMenu::from_elf(&elf, sym.address).ok()) {
            menus.insert(n, m);
        }
    }
    let controls_menu = gdl_core::game::optmenu::ControlsMenu::from_elf(&elf).ok();
    let catalog_hints = catalog.read("TEXT/HINTS_E.ROM")?;
    // The attract screens' packages (init_screen2d's "sony" / "group1",
    // init_credits' "credits") as front texture sets 3–5, and the credits
    // font (FontInitSpecial("credits", 8): font 0xd).
    let mut attract_pkgs = Vec::new();
    for name in ["SONY", "GROUP1", "CREDITS"] {
        let pkg = load_package(&package_entry(&catalog, name, true)?)?;
        ui_hud.add_texture_set(gdl_client::hud_texture_set(name, &pkg.objects));
        attract_pkgs.push(pkg);
    }
    if let Ok(f) = catalog.read("FONTS/CREDITS.FNT").map_err(|e| e.to_string()).and_then(|b| gdl_formats::fnt::Font::parse(&b).map_err(|e| e.to_string())) {
        let tex = ui_hud.lookup("CREDITS");
        ui_hud.font_init_special(f, tex);
    }
    let mut attract = gdl_core::game::attract::Attract::from_elf(&elf)?;
    // worlddatas +0x10: the worlds whose WDATA is loaded, with their LEVL
    // +4 flags (NextAttractWave's attract levels).
    for w in attract.worlds.iter_mut() {
        if let Some(l) = levels.iter().find(|l| l.world == w.number) {
            let wd = gdl_formats::wdata::WorldData::parse(&gdl_client::bounded_read(&l.wdata)?)?;
            w.levels = Some(wd.levels.iter().map(|r| r.word_04).collect());
        }
    }
    let credits = gdl_core::game::attract::Credits::from_elf(&elf)?;
    let mut front = Front {
        menu: {
            let mut m = Menu::new(music_v, sfx_v);
            for (k, p) in m.prefs.iter_mut().enumerate() {
                *p = game_opts.get(4 * k..4 * k + 4).map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap()));
            }
            // The saved screen adjust (GamePrefs words 3/4) applies at boot.
            m.screen = (m.prefs[3].clamp(-0x80, 0x80), m.prefs[4].clamp(-0x28, 0x28));
            m
        },
        store,
        catalog,
        elf_bytes,
        tables,
        pdats: profile::Pdats(pdats),
        class_fx,
        missile_desc,
        sel,
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
        powerups_pkg,
        controls_menu,
        menu_clock: (None, 0.0),
        attract,
        attract_run: None,
        attract_blits: Vec::new(),
        credits,
        attract_pkgs,
        title_timeout: 1800.0,
        attract_clock: 0.0,
        hints_rom: gdl_formats::textrom::TextRom::parse(&catalog_hints)?,
        hints: Default::default(),
        flyby_request: None,
        inventory_close: None,
        base: Base {
            package_cfg,
            start,
            levels,
            monster_paths,
            monsters,
            anim_paths,
            audio_rom,
            shared_labels,
            movies: cfg["movies"].as_object().map_or(HashMap::new(), |m| m.iter().filter_map(|(n, v)| Some((n.to_uppercase(), v.as_str()?.into()))).collect()),
        },
    };
    refresh_saves(&mut front);
    // main → init_attract_mode(-1): the boot starts the attract sequence
    // (the SONY screen and card check, the MIDWAY, OPENING and TITLE2
    // movies, then the title screen). DEBUG: GDL_NO_ATTRACT starts at the
    // title.
    // GDL_DEBUG_CREDITS starts with init_attract_mode(0x8000) (the
    // credits, as both shoulder buttons held do).
    // GDL_DEBUG_FLYBY=<level name> starts with that level's flyby.
    if let Some(i) = std::env::var("GDL_DEBUG_FLYBY").ok().and_then(|n| front.base.levels.iter().position(|l| l.name == n)) {
        front.attract.did_mc_check = 2;
        front.flyby_request = Some(i);
        front.attract_run = Some(AttractRun::Flyby);
    } else if std::env::var_os("GDL_DEBUG_CREDITS").is_some() {
        start_attract(&mut front, 0x8000);
    } else if std::env::var_os("GDL_NO_ATTRACT").is_none() {
        start_attract(&mut front, -1);
    }
    // hint_menu categories → HINTS_E.ROM lists (every line of every entry,
    // in list order; replaced per category by next_*_hint when shown).
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
        .init_resource::<MatRequests>()
        .init_resource::<PendingTreePsys>()
        .init_resource::<MovieScreen>()
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
        .init_resource::<MenuIcon>()
        .add_systems(Startup, (setup_static, setup_menu_icon).chain())
        .add_systems(FixedUpdate, (simulate, apply_rumble).chain().run_if(playing))
        .add_systems(
            Update,
            (
                front_system,
                run_attract,
                ps2_viewport,
                (advance_level, refresh_characters, present_world, present_world_nodes, present_node_flags, (present_scene_light, present_point_lights).chain(), present_distance_fade, present_camera_face, present_effect_trees, present_item_trees, present_camera_face_local, spawn_tree_psys, follow_tree_psys, present_particles, present_texmods, hide_exploded_nodes, present_critters, present_critter_shadows, (present_sumner, present_special_texmods, present_hold_fx, present_movie, present_tree_alpha, present_skins, present_compass, present_player_shadow, apply_material_requests).chain(), present_player).chain().run_if(playing),
                front_text,
                present_hud,
                present_menu_icon,
                present_screen_adjust,
            )
                .chain(),
        )
        .run();
    Ok(())
}

/// Package names a level draws or reads: the shared packages, its item
/// packages, its realm's monsters and its critter models.
/// SetupItemTexMods / SetupWeaponPowerupTexMods / SetupEnemyTexMods and
/// SetupPlayerTexMods' class SFX package: the packages whose texture
/// modifiers run.
fn texmod_packages(session: &Session, sources: &Sources) -> Vec<String> {
    let mut v = level_packages(&session.levels[session.current]);
    for pc in sources.players.iter().flatten() {
        if !v.contains(&pc.sfx_name) {
            v.push(pc.sfx_name.clone());
        }
    }
    v
}

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
/// end of the playable realm.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn advance_level(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut session: ResMut<Session>,
    mut sim: ResMut<Sim>,
    (old, root, bodies, shadows): (Query<Entity, With<LevelEntity>>, Query<Entity, With<MirrorRoot>>, Query<Entity, With<CharacterRoot>>, Query<Entity, With<PlayerShadow>>),
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
    // game_main: the level is over with every player dead (state 0/0xb) →
    // TowerWave → init_shop(0), which skips without a player in state 1/5.
    if !ui.paused
        && session.banner.is_none()
        && sim.shop.is_none()
        && sim.shop_done.is_none()
        && sim.game.players[0].i32_(0xfc) == 0xb
        && !(0..4).any(|k| matches!(sim.game.players[k].i32_(0xfc), 1 | 4 | 8))
        && let Some(k) = session.levels.iter().position(|l| l.world == 0xd)
    {
        start_shop(&mut sim, &front, k);
    }
    // do_shop finished: init_player_select(2) (game_main: unless the shop
    // was skipped), then init_next_level into the Tower.
    if let Some(k) = sim.shop_done.take() {
        match sim.game.init_player_select(2, &session.pdats) {
            Ok(ps) => {
                sim.select = Some(ps);
                sim.select_next = Some(k);
            }
            Err(e) => sim.error = Some(format!("init_player_select: {}", e.0)),
        }
    }
    // do_mapscreen finished: LoadWorldData, init_gamemovie (the level's
    // LEVL +0x34 movie), else init_next_level.
    if let Some(k) = sim.map_done.take() {
        let skip = i32::from((13..=0xffff).contains(&sim.game.globals.skip_wave));
        let gm = sim.game.init_gamemovie(skip, &session.levels[k].movie);
        if gm.request.is_some() {
            sim.movie = Some(MovieRun { game: gm, player: None, tried: false, then: MovieThen::Level(k) });
        } else {
            session.next = Some(k);
            session.banner = Some((0.0, String::new()));
        }
    }
    // do_gamemovie finished.
    match sim.movie_done.take() {
        Some(MovieThen::Level(k)) => {
            session.next = Some(k);
            session.banner = Some((0.0, String::new()));
        }
        Some(MovieThen::Shop(k)) => start_shop(&mut sim, &front, k),
        None => {}
    }
    // do_player_select aborted: init_titlescreen (every player left).
    if std::mem::take(&mut sim.select_quit) {
        session.finished = Some("Every player left the select screen; the title screen follows (Esc).".into());
    }
    // do_player_select done: init_next_level (the Tower).
    if std::mem::take(&mut sim.select_done) {
        // DEBUG: GDL_DEBUG_LEVEL=<name> also replaces the Tower here.
        let debug = std::env::var("GDL_DEBUG_LEVEL").ok().and_then(|n| session.levels.iter().position(|l| l.name == n));
        let tower = sim.select_next.take().or(debug).or_else(|| session.levels.iter().position(|l| l.world == 0xd));
        session.next = tower;
        session.banner = Some((0.0, String::new()));
    }
    if !ui.paused && keys.just_pressed(KeyCode::F9) && session.banner.is_none() && sim.game.level_done.is_none() {
        // DEBUG: act as if the exit portal (destination: next level; with
        // Shift: the Tower) was used.
        let next = if keys.pressed(KeyCode::ShiftLeft) {
            gdl_core::game::boss::TOWER_WAVE
        } else {
            session.levels.get(session.current + 1).map_or(session.world_number << 8 | 0xff, |l| l.world << 8 | l.index as i32)
        };
        sim.game.level_done = Some(next);
        sim.game.exit_ready = true;
        sim.debug_skip = true;
    }
    if session.banner.is_none()
        && session.finished.is_none()
        // game_main: the exit waits for do_players and the voice queue.
        && sim.game.exit_ready
        && let Some(wave) = sim.game.level_done.take()
    {
        let name = session.levels[session.current].name.clone();
        let tower = session.levels.iter().position(|l| l.world == 0xd);
        // game_main's level end (game::exitflow); F9 (DEBUG) goes straight
        // to its wave instead.
        let (wave, skip) = if std::mem::take(&mut sim.debug_skip) {
            let (cw, cl) = (session.world_number, session.levels[session.current].index as i32);
            let (l90, l92) = (sim.game.level_i16(0x90), sim.game.level_i16(0x92));
            sim.game.players_set_wave_finished(cw, cl, l90, l92);
            (Some(wave), false)
        } else {
            match sim.game.game_main_level_end() {
                LevelEnd::Over => (None, false),
                // TowerWave → init_shop(0): the tally and shop, then the Tower.
                LevelEnd::Shop => {
                    if let Some(k) = tower {
                        start_shop(&mut sim, &front, k);
                    }
                    (None, false)
                }
                // init_gamemovie (movies are not ported), then game_wave =
                // TowerWave: init_next_level into the Tower.
                // init_gamemovie(BossType): victory / garm (movietype 2),
                // then init_shop and the Tower.
                LevelEnd::Movie(b) => {
                    let gm = sim.game.init_gamemovie(b, "");
                    if let Some(k) = tower {
                        sim.movie = Some(MovieRun { game: gm, player: None, tried: false, then: MovieThen::Shop(k) });
                    }
                    (None, false)
                }
                LevelEnd::Map { skip } => match sim.game.next_world_wave() {
                    Ok(w) => (Some(w), skip),
                    Err(e) => {
                        sim.error = Some(format!("next_world: {}", e.0));
                        (None, false)
                    }
                },
            }
        };
        if let Some(wave) = wave {
            let world = wave >> 8;
            let level = (wave & 0xff) as usize;
            if let Some(k) = session.levels.iter().position(|l| l.index == level && l.world == world)
                && session.levels[k].world == 0xd
                && session.world_number != 0xd
            {
                // (F9) an exit to TowerWave goes through init_shop(0).
                start_shop(&mut sim, &front, k);
            } else if let Some(k) = session.levels.iter().position(|l| l.index == level && l.world == world)
                && start_map(&mut sim, &mut sources, &mut hud_sets, &front, &session, k, skip)
            {
                // init_mapscreen(0x78, skip) towards next_world's wave.
            } else if let Some(k) = session.levels.iter().position(|l| l.index == level && l.world == world) {
                session.next = Some(k);
                let to = if session.levels[k].world == session.world_number { session.levels[k].name.clone() } else { format!("{} {}", session.levels[k].title, session.levels[k].name) };
                session.banner = Some((3.0, format!("{name} COMPLETE - entering {to}")));
            } else {
                session.finished = Some(format!(
                    "{name} COMPLETE. The exit leads to wave {wave:#x}, which is not in this build's level list. Thanks for testing! (Esc to quit)"
                ));
            }
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
        session.object_labels = labels.into_iter().chain(character_labels(&sources)).collect();
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
    session.carried = [g.tower_counter, g.last_finished_world, g.last_finished_level, g.cur3d_transmitter, g.cur_transmitter, g.last_world_played, g.last_level_played];
    let carry: [Record; 4] = sim.game.players.clone();
    let control_options = sim.game.control_options;
    let controls = sim.game.controls.clone();
    // load_player: each player in play gets the model of its class, colour,
    // tier (level / 10) and descriptor.
    let mut new_body = false;
    let loaded = loaded.and_then(|_| {
        for (k, rec) in carry.iter().enumerate() {
            if k != 0 && rec.i32_(0xfc) == 0 {
                continue;
            }
            let key = CharKey::of(rec);
            if sources.players[k].as_ref().map(|p| p.key) != Some(key) {
                install_char(&front, &mut sources, &mut libraries, (&mut meshes, &mut images, &mut materials), k, key)?;
                new_body = true;
            }
        }
        session.object_labels.extend(character_labels(&sources));
        Ok(())
    });
    // skip_wave 13..0xffff: the return from a treasure room to recorded_wave
    // (init_next_level → LoadStage); the stage record crosses the load.
    let skip = sim.game.globals.skip_wave;
    let stage = sim.game.stage_save.clone();
    let returning = (13..=0xffff).contains(&skip) && stage.is_some();
    let built = loaded.and_then(|_| {
        let slots = slot_starts(&front, &sources, &carry, &control_options)?;
        build_game(&session, idx, &slots, returning, None)
    });
    // EnterTower: TowerStartPos = −1 once used.
    if session.levels[idx].world == 0xd {
        session.tower_start = None;
    }
    match built {
        Ok(mut game) => {
            // The controls state carries across levels; co-op players keep
            // their records and enter beside player 1.
            game.controls = controls;
            match (&stage, returning) {
                (Some(st), true) => {
                    if let Err(e) = game.load_stage(st) {
                        sim.error = Some(format!("LoadStage: {}", e.0));
                    }
                }
                (st, false) => game.stage_save = st.clone(),
                _ => {}
            }
            if let Some(a) = audio.as_mut() {
                a.stop_all();
            }
            session.current = idx;
            init_hud(&mut game, &sources, &session, &front, &mut hud_sets, true);
            sim.game = game;
            for e in &old {
                commands.entity(e).despawn();
            }
            if let Ok(root) = root.single() {
                let n = sim.game.items.records.len();
                spawn_level(&mut commands, root, &session, n, &mut meshes, &mut images, &mut materials);
                spawn_package_texmods(&mut commands, &libraries, &texmod_packages(&session, &sources));
                if new_body {
                    for b in &bodies {
                        commands.entity(b).despawn();
                    }
                    for b in &shadows {
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
