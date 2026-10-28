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
/// NATIVE ENHANCEMENT (Settings::widescreen): `wide` keeps the vertical
/// angle and widens the horizontal one to the viewport's aspect (Hor+).
#[derive(Debug, Clone)]
struct Ps2Projection {
    tan_h: f32,
    tan_v: f32,
    near: f32,
    far: f32,
    /// The viewport's width / height (CameraProjection::update).
    aspect: f32,
    wide: bool,
}

impl Ps2Projection {
    fn new(tan_h: f32, tan_v: f32, near: f32, far: f32) -> Self {
        Self { tan_h, tan_v, near, far, aspect: tan_h / tan_v, wide: false }
    }

    /// The horizontal half-angle tangent drawn.
    fn shown_tan_h(&self) -> f32 {
        if self.wide && self.aspect.is_finite() && 0.0 < self.aspect { self.tan_v * self.aspect } else { self.tan_h }
    }
}

impl bevy::camera::CameraProjection for Ps2Projection {
    fn get_clip_from_view(&self) -> Mat4 {
        Mat4::from_cols(
            Vec4::new(1.0 / self.shown_tan_h(), 0.0, 0.0, 0.0),
            Vec4::new(0.0, 1.0 / self.tan_v, 0.0, 0.0),
            Vec4::new(0.0, 0.0, 0.0, -1.0),
            Vec4::new(0.0, 0.0, self.near, 0.0),
        )
    }
    fn get_clip_from_view_for_sub(&self, _sub_view: &bevy::camera::SubCameraView) -> Mat4 {
        self.get_clip_from_view()
    }
    fn update(&mut self, width: f32, height: f32) {
        if 0.0 < width && 0.0 < height {
            self.aspect = width / height;
        }
    }
    fn far(&self) -> f32 {
        self.far
    }
    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [bevy::math::Vec3A; 8] {
        let c = |z: f32| {
            let (x, y) = (z.abs() * self.shown_tan_h(), z.abs() * self.tan_v);
            [(x, -y), (x, y), (-x, y), (-x, -y)].map(|(a, b)| bevy::math::Vec3A::new(a, b, z))
        };
        let (n, f) = (c(z_near), c(z_far));
        [n[0], n[1], n[2], n[3], f[0], f[1], f[2], f[3]]
    }
}

/// The PS2 frame's place in the window: the largest 640:448 rectangle,
/// centred (x, y, width, height in physical pixels). The original picture
/// fills it (borders outside); a window of exactly that shape is all frame.
fn frame_rect(width: u32, height: u32) -> (u32, u32, u32, u32) {
    let (w, h) = (width.max(1), height.max(1));
    if u64::from(w) * 448 > u64::from(h) * 640 {
        let fw = ((u64::from(h) * 640 + 224) / 448) as u32;
        ((w - fw.min(w)) / 2, 0, fw.min(w), h)
    } else {
        let fh = ((u64::from(w) * 448 + 320) / 640) as u32;
        (0, (h - fh.min(h)) / 2, w, fh.min(h))
    }
}

/// The HUD layer's camera (the original 512×384 blit screen).
#[derive(Component)]
struct HudCamera;

/// Where the settings overlay stores settings.json (None under
/// GDL_BASELINE / GDL_SETTINGS: test runs never write the player's file).
#[derive(Resource)]
struct SettingsDir(Option<std::path::PathBuf>);

/// The render layer of the native overlay camera (settings, frame rate).
const OVERLAY_LAYER: usize = 2;

#[derive(Component)]
struct OverlayCamera;
#[derive(Component)]
struct OverlayBackdrop;
#[derive(Component)]
struct OverlayText;
#[derive(Component)]
struct FpsText;

/// The native overlay's camera (last, window pixels, crisp text outside
/// the GS display decode) and its entities, hidden until used.
fn setup_overlay(mut commands: Commands) {
    let layer = bevy::camera::visibility::RenderLayers::layer(OVERLAY_LAYER);
    commands.spawn((
        Camera2d,
        Camera { order: 3, clear_color: ClearColorConfig::None, is_active: false, ..default() },
        Tonemapping::None,
        bevy::render::view::Hdr,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        layer.clone(),
        OverlayCamera,
    ));
    commands.spawn((Sprite { color: Color::srgba(0.0, 0.0, 0.0, 0.82), custom_size: Some(Vec2::ONE), ..default() }, Transform::default(), Visibility::Hidden, layer.clone(), OverlayBackdrop));
    commands.spawn((
        Text2d::new(String::new()),
        TextFont { font_size: 20.0, ..default() },
        TextColor(Color::srgb(0.95, 0.92, 0.8)),
        TextLayout::new_with_justify(bevy::text::Justify::Left),
        Anchor::TOP_LEFT,
        Transform::from_xyz(0.0, 0.0, 1.0),
        Visibility::Hidden,
        layer.clone(),
        OverlayText,
    ));
    commands.spawn((
        Text2d::new(String::new()),
        TextFont { font_size: 16.0, ..default() },
        TextColor(Color::srgb(1.0, 1.0, 0.6)),
        Anchor::TOP_RIGHT,
        Transform::from_xyz(0.0, 0.0, 1.0),
        Visibility::Hidden,
        layer,
        FpsText,
    ));
}

/// NATIVE ENHANCEMENT keys: F10 the settings overlay (its keys while
/// open), F11 fullscreen, F3 the frame-rate counter. Changes are stored at
/// once (settings.json beside the saves).
fn enhancement_input(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut overlay: ResMut<gdl_client::enhance::Overlay>,
    mut settings: ResMut<gdl_client::settings::Settings>,
    mut bindings: ResMut<gdl_client::controls::Bindings>,
    dir: Res<SettingsDir>,
) {
    use gdl_client::enhance::{Effect, Input};
    if overlay.held && keys.get_pressed().next().is_none() {
        overlay.held = false;
    }
    let mut changed = false;
    if keys.just_pressed(KeyCode::F11) {
        settings.fullscreen = !settings.fullscreen;
        changed = true;
    }
    if keys.just_pressed(KeyCode::F3) {
        settings.show_fps = !settings.show_fps;
        changed = true;
    }
    if keys.just_pressed(KeyCode::F10) {
        overlay.toggle();
    } else if overlay.open {
        let mut inputs = Vec::new();
        if overlay.capturing() {
            if keys.just_pressed(KeyCode::Escape) {
                inputs.push(Input::Back);
            } else if let Some(&k) = keys.get_just_pressed().next() {
                inputs.push(Input::Key(k));
            }
        } else {
            for (ks, i) in [
                (&[KeyCode::ArrowUp, KeyCode::KeyW][..], Input::Up),
                (&[KeyCode::ArrowDown, KeyCode::KeyS][..], Input::Down),
                (&[KeyCode::ArrowLeft, KeyCode::KeyA][..], Input::Left),
                (&[KeyCode::ArrowRight, KeyCode::KeyD][..], Input::Right),
                (&[KeyCode::Enter, KeyCode::Space, KeyCode::NumpadEnter][..], Input::Accept),
                (&[KeyCode::Escape][..], Input::Back),
                (&[KeyCode::Backspace, KeyCode::Delete][..], Input::Reset),
            ] {
                if ks.iter().any(|&k| keys.just_pressed(k)) {
                    inputs.push(i);
                }
            }
        }
        let (s, b) = (settings.bypass_change_detection(), bindings.bypass_change_detection());
        for i in inputs {
            if overlay.input(i, s, b) == Effect::Changed {
                changed = true;
            }
        }
        if changed {
            settings.set_changed();
            bindings.set_changed();
        }
        // The keys the overlay took are not seen by the menus this frame.
        let taken: Vec<KeyCode> = keys.get_just_pressed().copied().collect();
        for k in taken {
            keys.clear_just_pressed(k);
        }
    }
    if changed && let Some(d) = &dir.0
        && let Err(e) = settings.save(d)
    {
        eprintln!("settings: not stored ({e})");
    }
}

/// The settings that live outside the systems reading them each frame:
/// the window's sync and mode, every window camera's MSAA.
fn apply_settings(settings: Res<gdl_client::settings::Settings>, mut windows: Query<&mut Window>, mut cams: Query<(&mut Msaa, Option<&bevy::camera::RenderTarget>), With<Camera>>) {
    if !settings.is_changed() {
        return;
    }
    for mut w in &mut windows {
        let present = if settings.vsync { bevy::window::PresentMode::AutoVsync } else { bevy::window::PresentMode::AutoNoVsync };
        if w.present_mode != present {
            w.present_mode = present;
        }
        let mode = if settings.fullscreen { bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current) } else { bevy::window::WindowMode::Windowed };
        if w.mode != mode {
            w.mode = mode;
        }
    }
    let msaa = Msaa::from_samples(settings.msaa);
    for (mut m, target) in &mut cams {
        if !matches!(target, Some(bevy::camera::RenderTarget::Image(_))) && *m != msaa {
            *m = msaa;
        }
    }
}

/// The settings overlay and the frame-rate counter, placed in the PS2
/// frame's rectangle (window pixels).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn present_overlay(
    windows: Query<&Window>,
    time: Res<Time<Real>>,
    overlay: Res<gdl_client::enhance::Overlay>,
    settings: Res<gdl_client::settings::Settings>,
    bindings: Res<gdl_client::controls::Bindings>,
    mut frames: Local<std::collections::VecDeque<f32>>,
    mut cam: Query<&mut Camera, With<OverlayCamera>>,
    mut backdrop: Query<(&mut Sprite, &mut Transform, &mut Visibility), (With<OverlayBackdrop>, Without<OverlayText>, Without<FpsText>)>,
    mut text: Query<(&mut Text2d, &mut TextFont, &mut Transform, &mut Visibility), (With<OverlayText>, Without<FpsText>)>,
    mut fps: Query<(&mut Text2d, &mut TextFont, &mut Transform, &mut Visibility), (With<FpsText>, Without<OverlayText>)>,
) {
    frames.push_back(time.delta_secs());
    while 60 < frames.len() {
        frames.pop_front();
    }
    let Ok(w) = windows.single() else { return };
    let (_, _, fw, fh) = frame_rect(w.physical_width(), w.physical_height());
    let scale = w.scale_factor().max(0.1);
    let (fw, fh) = (fw as f32 / scale, fh as f32 / scale);
    let active = overlay.open || settings.show_fps;
    for mut c in &mut cam {
        if c.is_active != active {
            c.is_active = active;
        }
    }
    let show = |v: &mut Visibility, on: bool| {
        let want = if on { Visibility::Visible } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    };
    if let Ok((mut t, mut font, mut tf, mut v)) = text.single_mut() {
        show(&mut v, overlay.open);
        if overlay.open {
            let lines = overlay.lines(&settings, &bindings);
            let size = (fh / 34.0).clamp(10.0, 40.0);
            let (bw, bh) = ((fw * 0.86).min(size * 40.0), (lines.len() as f32 + 1.5) * size * 1.25);
            let joined = lines.join("\n");
            if t.0 != joined {
                t.0 = joined;
            }
            if font.font_size != size {
                font.font_size = size;
            }
            tf.translation = Vec3::new(-bw * 0.5 + size, bh * 0.5 - size * 0.75, 1.0);
            if let Ok((mut sprite, mut btf, mut bv)) = backdrop.single_mut() {
                show(&mut bv, true);
                sprite.custom_size = Some(Vec2::new(bw, bh));
                btf.translation = Vec3::ZERO;
            }
        } else if let Ok((_, _, mut bv)) = backdrop.single_mut() {
            show(&mut bv, false);
        }
    }
    if let Ok((mut t, mut font, mut tf, mut v)) = fps.single_mut() {
        show(&mut v, settings.show_fps);
        if settings.show_fps && !frames.is_empty() {
            let mean = frames.iter().sum::<f32>() / frames.len() as f32;
            let worst = frames.iter().copied().fold(0.0, f32::max);
            let line = format!("{:.0} fps  {:.1} ms  (worst {:.1})", 1.0 / mean.max(1e-6), mean * 1000.0, worst * 1000.0);
            if t.0 != line {
                t.0 = line;
            }
            let size = (fh / 48.0).clamp(9.0, 28.0);
            if font.font_size != size {
                font.font_size = size;
            }
            tf.translation = Vec3::new(fw * 0.5 - size * 0.5, fh * 0.5 - size * 0.4, 1.0);
        }
    }
}

/// The 3D viewport: the PS2 frame's rows above its bottom inset (64 of 448
/// in play — InitCamera's WindowBottomLim; none in the flyby), in the frame
/// rectangle; the HUD's 512×384 screen mapped onto that rectangle, and the
/// final pass told where it is. NATIVE ENHANCEMENT (Settings::widescreen,
/// a window wider than 640:448, no movie showing): the 3D view takes the
/// window's full width at the frame's height (Hor+), the HUD stays centred.
#[allow(clippy::type_complexity)]
fn ps2_viewport(
    windows: Query<&Window>,
    sim: Option<Res<Sim>>,
    settings: Res<gdl_client::settings::Settings>,
    movie: Query<&Visibility, With<MovieQuad>>,
    mut cams: Query<(&mut Camera, &mut Projection), With<FollowCamera>>,
    mut hud: Query<&mut Projection, (With<HudCamera>, Without<FollowCamera>)>,
    mut finals: Query<&mut gdl_client::DisplayToLinear>,
) {
    let Ok(w) = windows.single() else { return };
    let (width, height) = (w.physical_width().max(1), w.physical_height().max(1));
    let (fx, fy, fw, fh) = frame_rect(width, height);
    let movie = movie.iter().any(|v| *v == Visibility::Visible);
    let wide = settings.widescreen && !movie && fw < width;
    let (frame, inset) = sim
        .as_ref()
        .and_then(|s| s.game.std_camera.as_ref())
        .map_or((448.0, 64.0), |c| (c.window.frame[1] as f32, c.window.viewport[3] + c.window.viewport[2]));
    let rows = (fh as f32 * (frame - inset) / frame).round() as u32;
    if let Ok((mut cam, mut projection)) = cams.single_mut() {
        let (x, vw) = if wide { (0, width) } else { (fx, fw) };
        let want = bevy::camera::Viewport { physical_position: UVec2::new(x, fy), physical_size: UVec2::new(vw.max(1), rows.max(1)), ..default() };
        if cam.viewport.as_ref().is_none_or(|v| v.physical_size != want.physical_size || v.physical_position != want.physical_position) {
            cam.viewport = Some(want);
        }
        if let Projection::Custom(custom) = &*projection
            && custom.get::<Ps2Projection>().is_some_and(|p| p.wide != wide)
            && let Projection::Custom(custom) = &mut *projection
            && let Some(p) = custom.get_mut::<Ps2Projection>()
        {
            p.wide = wide;
        }
    }
    let (sw, sh) = (512.0 * width as f32 / fw as f32, 384.0 * height as f32 / fh as f32);
    for mut p in &mut hud {
        if let Projection::Orthographic(o) = &*p
            && !matches!(o.scaling_mode, bevy::camera::ScalingMode::Fixed { width, height } if width == sw && height == sh)
            && let Projection::Orthographic(o) = &mut *p
        {
            o.scaling_mode = bevy::camera::ScalingMode::Fixed { width: sw, height: sh };
        }
    }
    let rect = Vec4::new(fx as f32 / width as f32, fy as f32 / height as f32, fw as f32 / width as f32, fh as f32 / height as f32);
    let wide = if wide { 1.0 } else { 0.0 };
    for mut d in &mut finals {
        if d.frame != rect || d.wide != wide {
            d.frame = rect;
            d.wide = wide;
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
    /// The current level's enemy type → (monster package name, tree prefix):
    /// GetEnemyDir's directory by the type's subtype (IMP2, IMPAUX, ...) and
    /// the enemy_table name its EnemyDesc labels use.
    monsters: HashMap<i32, (String, String)>,
    /// Every monster package name of the campaign (enemy frame tables).
    monster_packages: std::collections::HashSet<String>,
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

impl Libraries {
    /// The libraries in `order` (search_order), then any other cached one by
    /// name: a by-name search's result never depends on the map's
    /// per-process iteration order (C2's sprite texture did, run to run).
    fn ordered<'a>(&'a self, order: &'a [String]) -> impl Iterator<Item = &'a Library> + 'a {
        let mut rest: Vec<&String> = self.0.keys().filter(|k| !order.contains(k)).collect();
        rest.sort();
        order.iter().chain(rest).filter_map(|n| self.0.get(n))
    }
}

/// The loaded packages a by-name search (MBOX_FindTexture, the object
/// lookups) visits, in load order: the globals STATIC, WEAPONS, POWERUPS
/// (rendering.md "Particle systems"), the current level's item, monster
/// and critter packages (level_packages; HYPOTHESIS: the original's order
/// among them is unread), the level's own package, then the players' model
/// and SFX packages. Libraries caches every package loaded so far; an
/// earlier realm's comes only after these.
fn search_order(session: &Session, sources: &Sources) -> Vec<String> {
    const GLOBALS: [&str; 3] = ["STATIC", "WEAPONS", "POWERUPS"];
    let mut out: Vec<String> = GLOBALS.iter().map(|s| s.to_string()).collect();
    out.extend(level_packages(&session.levels[session.current]).into_iter().filter(|n| !GLOBALS.contains(&n.as_str())));
    out.push(LEVEL_LIB.to_string());
    for pc in sources.players.iter().flatten() {
        out.extend([pc.model_name.clone(), pc.sfx_name.clone()]);
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|n| seen.insert(n.clone()));
    out
}

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
    /// Monster packages of the level (names in `packages`).
    monsters: Vec<String>,
    /// The level's LoadEnemy types: (type, package, tree prefix).
    monster_types: Vec<(i32, String, String)>,
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
    monster_paths: Vec<(i32, String, String, String)>,
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
    /// The static Transmitters table as the last level left it (entries past
    /// a level's count are earlier levels'; camera.rs `transmitter_table`).
    transmitter_table: Vec<gdl_core::game::camera::Transmitter>,
    /// CheckNum (WorldCollide's query counter), carried like the indices.
    check_num: u8,
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
    monster_paths: Vec<(i32, String, String, String)>,
    monster_packages: std::collections::HashSet<String>,
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
    /// What end_optmenu leaves behind: menus fading out and the fire scroll
    /// over a closed menu's background (main pauses the game until both
    /// are done).
    ending: MenuEnding,
    /// AudioFX requests of the menus (id, volume, priority), drained by
    /// play_menu_cues.
    sfx: Vec<(i32, i32, i32)>,
    /// The audio menu's vblank remainder and snd.286 0x23d438 (the SFX
    /// slider's two samples).
    audio_clock: f32,
    vol_sfx_samples: [i32; 2],
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
/// end_optmenu 0x10b390's leftovers. A menu with flag 0x20 that is left
/// (pushed under another, popped, or closed) keeps drawing through
/// finish_optmenu 0x10b288 with +0x70 counting vblanks up to
/// OPTMENU_FINISH_FRAMES, without its background (+0xdc), LOGO_BURN
/// (+0xe0) or icon (+0x88). A pop to a menu without a background, or the
/// last menu closed (end_optmenu(−1, 0) / (−1, −1)), starts the fire scroll
/// over the background (StartFireScroll(1.0, +0x44, −1, +0x48..+0x54)),
/// served by DoOptions (ServeFireScroll: 21 frames of two vblanks,
/// ClearAllPlayerControls(2) each). DoOptions returns nonzero meanwhile,
/// so main pauses the game. end_all_optmenus (end_optmenu(−1, 1): Manage
/// Character, Shop) removes menus at once.
#[derive(Default)]
struct MenuEnding {
    /// (record, selected entry, its frame count +0x7c, +0x70).
    finishing: Vec<(gdl_core::game::optmenu::OptMenu, usize, i32, f32)>,
    /// The burning background (texture name, rectangle) and its vblanks.
    burn: Option<(String, [i32; 4], f32)>,
}

impl MenuEnding {
    fn active(&self) -> bool {
        !self.finishing.is_empty() || self.burn.is_some()
    }
}

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
    /// Broken collision nodes drawn so far (CritterDamage's MBSetObject).
    damaged: usize,
    /// The simulation update, part count and first part last posed: the
    /// pose changes only with an update (frames between them keep it; the
    /// presentation blending moves them); respawned parts are new entities.
    posed: Option<(u64, usize, Option<Entity>)>,
}

/// One tree node's meshes of a critter slot.
#[derive(Component)]
struct CritterPart {
    node: usize,
}

/// Critters drawn from the simulation skeleton: each tree node's object
/// "<tree prefix><node label>" (15 characters; AtreeNodeInit: not for node
/// flag bit 0 nor OANIM nodes) in the critter's model package, and an OANIM
/// node's frame objects (DoObjAnimation: the critter's sequence and frame
/// pick one; Skorne's lower body), posed by gdl_core::tree_pose
/// (presentation of the native pose). Effects started on the critter are
/// the effect slots'.
#[allow(clippy::type_complexity)]
fn present_critters(
    mut commands: Commands,
    sim: Res<Sim>,
    session: Res<Session>,
    libs: Res<Libraries>,
    mut slots: Query<(Entity, &mut CritterSlot, &mut Visibility, Option<&Children>)>,
    mut parts: Query<(&CritterPart, &mut Transform, Option<&OFrame>, &mut Visibility), Without<CritterSlot>>,
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
        if slot.shown != Some(cr.ty) || slot.damaged != cr.damaged_nodes.len() {
            commands.entity(entity).despawn_related::<Children>();
            // The configured model package (golems, gargoyles, generals),
            // else a package named after the tree prefix (bosses).
            let lib_name = session.levels.get(session.current).and_then(|l| l.critters.get(cr.ty.0)).and_then(|c| c.2.clone()).unwrap_or_else(|| t.prefix.clone());
            if let Some(lib) = libs.0.get(&lib_name) {
                let add = |commands: &mut Commands, node: usize, list: &Parts, frame: Option<OFrame>| {
                    let holder = commands.spawn((CritterPart { node }, Transform::default(), Visibility::default())).id();
                    if let Some(f) = frame {
                        commands.entity(holder).insert(f);
                    }
                    for (mesh, material) in list {
                        let child = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default())).id();
                        commands.entity(holder).add_child(child);
                    }
                    commands.entity(entity).add_child(holder);
                };
                for node in &t.nodes {
                    if node.node_type == 2 {
                        for (seq, e) in node.oanim.iter().enumerate() {
                            let Some(&base) = lib.index_of.get(&e.name.to_uppercase()) else { continue };
                            for k in 0..e.count.max(1) {
                                if let Some(list) = lib.by_index.get(&(base + k as u32)) {
                                    let f = OFrame { seq: seq as u16, start: e.start, k, single: e.count <= 1 };
                                    add(&mut commands, node.index as usize, list, Some(f));
                                }
                            }
                        }
                        continue;
                    }
                    if node.flags & 1 != 0 || cr.removed_nodes.contains(&(node.index as usize)) {
                        continue;
                    }
                    let label: String = (t.prefix.clone() + &node.label).chars().take(15).collect();
                    // A broken node: CritterDamage's MBSetObject to "%sD%s"
                    // (HYPOTHESIS: DESC name = tree prefix, the collision
                    // node's name = the tree node's label).
                    let damaged: String = (t.prefix.clone() + "D" + &node.label).chars().take(15).collect();
                    let label = if cr.damaged_nodes.contains(&(node.index as usize)) && lib.by_label.contains_key(&damaged) { damaged } else { label };
                    if let Some(list) = lib.by_label.get(&label) {
                        add(&mut commands, node.index as usize, list, None);
                    }
                }
            }
            slot.shown = Some(cr.ty);
            slot.damaged = cr.damaged_nodes.len();
        }
        *vis = Visibility::Visible;
        let r = &cr.rec;
        let mut root = gdl_core::tree_pose::IDENTITY;
        for row in 0..3 {
            root[4 * row..4 * row + 3].copy_from_slice(&r.v3(0x10 + 16 * row));
        }
        root[12..15].copy_from_slice(&r.v3(0x40));
        let Some(children) = children else { continue };
        // Posed once per update: every frame between updates recomputed
        // each holder's whole parent chain and decomposed it (3.4 ms a
        // frame in J5, M20 profile). Each node's transform is computed once
        // (holders of OANIM frames share their node).
        let key = (sim.updates, children.len(), children.first().copied());
        if slot.posed == Some(key) {
            continue;
        }
        slot.posed = Some(key);
        let mut by_node: HashMap<usize, Option<Transform>> = HashMap::new();
        for child in children.iter() {
            if let Ok((part, mut tf, of, mut v)) = parts.get_mut(child) {
                if let Some(f) = of {
                    // No critter package sequence reverses its frames.
                    show_oframe(f, cr.clip, object_frame(cr.clock.frame.max(0.0), None), &mut v);
                }
                let pose = *by_node.entry(part.node).or_insert_with(|| {
                    gdl_core::tree_pose::node_world(&cr.parents, &cr.locals, root, part.node, None).ok().map(|m| Mat4::from_cols_array(&m)).filter(Mat4::is_finite).map(Transform::from_matrix)
                });
                if let Some(t) = pose {
                    *tf = t;
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
        // Compare before get_mut: it marks the material modified (re-prepared).
        if materials.get(&m.0).is_some_and(|mat| (mat.params.tint.w - a).abs() > 1e-4)
            && let Some(mat) = materials.get_mut(&m.0)
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

/// Nodes MBTreeSetAlpha skips (MB flag 0x10).
type Flag10Query<'w, 's> = Query<'w, 's, (), Or<(With<HandObject>, With<ItSign>, With<HeldWeapon>)>>;

/// PlayerProcessPowerups' MBTreeSetAlpha(player node, v, 1) (recursive:
/// the held weapon and the trees hung on the player go with it): 0x5f
/// during action 0x92; with the invisibility powerup (+0x138 4) a shimmer
/// (int)(sin(2π t) × 16) + 0xa0, t the longest kind-9 flag-4 slot time —
/// solid (0) on the even eighths of its last 3 s, a negative (unlimited)
/// time shimmers; otherwise 0. Drawn alpha (255 − v) / 255 (setPrimAlpha).
fn present_player_alpha(sim: Res<Sim>, bodies: Query<(&CharacterRoot, &Children)>, children: Query<&Children>, flag10: Flag10Query, mut req: ResMut<MatRequests>) {
    let g = &sim.game;
    for (root, kids) in &bodies {
        let r = &g.players[root.0];
        let v = if r.u32_(0x1e0) == 0x92 {
            0x5f
        } else if r.u32_(0x138) & 4 != 0 {
            let mut t = 0.0f32;
            for k in 0..8 {
                let o = 0x144 + 16 * k;
                let (time, kind, flags) = (r.f32_(o), r.i32_(o + 4), r.u32_(o + 12));
                if time != 0.0 && kind == 9 && flags & 4 != 0 && (time < 0.0 || t < time) {
                    t = time;
                }
            }
            if !(0.0..=3.0).contains(&t) || ((t * 8.0) as i32) & 1 != 0 {
                ((t * std::f32::consts::TAU).sin() * 16.0) as i32 + 0xa0
            } else {
                0
            }
        } else {
            0
        };
        if v == 0 {
            continue;
        }
        let a = (255 - v.clamp(0, 255)) as f32 / 255.0;
        // MBTreeSetAlpha's recursion skips nodes with MB flag 0x10 and their
        // subtrees: the attached objects (MBNewObject flags 0x810 / 0x9010 /
        // 0x10), IT_SIGN and the held weapon (load_player's 0x810) keep
        // their alpha.
        let mut stack: Vec<Entity> = kids.iter().filter(|e| !flag10.contains(*e)).collect();
        while let Some(e) = stack.pop() {
            req.alpha(e, a);
            if let Ok(c) = children.get(e) {
                stack.extend(c.iter());
            }
        }
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
/// scaled by 0.13333334 — which the VU's normals (ITOF4 − 7.5, i.e. 7.5 ×
/// the unit normal) cancel, so unit axes here; the camera block gets
/// atan(e.y / |e.xz|) / π (+0x50) and atan2(e.x, e.z) / 2π (+0x54), which
/// setTexShift 0x1beac0 adds to the v and u offsets (+0x2c, +0x28).
/// Computed in MB space (the render root mirrors x) and returned in render
/// space; the dot product with a world normal equals the object-frame one
/// for rotations.
fn chrome_axes(eye: Vec3, object: Vec3) -> (Vec4, Vec4) {
    let e = (eye - object) * MIRROR;
    let u = e.cross(Vec3::Y).normalize_or_zero();
    let v = e.cross(Vec3::Y).cross(e).normalize_or_zero();
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
    overridden: Query<Entity, With<MatOverride>>,
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
) {
    let eye = camera.iter().next().map(GlobalTransform::translation);
    // Only the meshes with a request this frame or an override to keep or
    // undo (every mesh with a cloned material each frame took about 1 ms).
    let mut todo: Vec<Entity> = req.0.keys().copied().collect();
    todo.extend(overridden.iter().filter(|e| !req.0.contains_key(e)));
    for e in todo {
        let Ok((e, mut m, ov, at)) = meshes.get_mut(e) else { continue };
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
        // Assets::get_mut marks the material modified (re-prepared for the
        // GPU) whether or not a field changes: compare first and take it
        // mutably only for a change. Every overridden material — an effect
        // tree's nodes with an alpha, a tint or a texture modifier — was
        // re-prepared every frame (the Sorceress's square attack, effects
        // 0x77/0x78: about twice the frame time of a cross throw).
        if let Some(cur) = materials.get(&m.0) {
            let replaced = image.is_some();
            let img = image.or_else(|| b.base.clone());
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
            let params_changed = cur.params.tint != p.tint || cur.params.scale != p.scale || cur.params.light != p.light || cur.params.light_dir != p.light_dir || cur.params.options != p.options || cur.params.extra != p.extra || cur.params.chrome_u != p.chrome_u || cur.params.chrome_v != p.chrome_v || cur.params.uv_xform != p.uv_xform;
            if (cur.base != img || cur.overlay != overlay || params_changed)
                && let Some(own) = materials.get_mut(&m.0)
            {
                if own.base != img {
                    own.base = img;
                }
                if own.overlay != overlay {
                    own.overlay = overlay.clone();
                }
                if params_changed {
                    own.params = p;
                }
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
/// `start` (DoTexFadeSub → MBTreeSetAlpha); −2/−3 scroll U or V
/// (DoTexScrollSub, request_tree_texmods).
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

fn request_tree_texmods(mods: &[NodeTexMod], lib: &Library, f: i32, meshes: &[(Entity, usize)], req: &mut MatRequests) {
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
fn present_scene_light(sim: Res<Sim>, settings: Res<gdl_client::settings::Settings>, mut last: Local<Option<([f32; 8], usize)>>, mut materials: ResMut<Assets<gdl_client::GsMaterial>>) {
    let g = &sim.game;
    if g.level.len() < 0x10c {
        return;
    }
    // DoLighting: MBSetAmbient(clamp(Ambient · GlobalInten + special)) and
    // MBLightmapMod(1, special) for unlit draws. NATIVE ENHANCEMENT
    // (Settings::reduce_flashes, accessibility): a brightening special
    // (lightning, SetAmbientSpecial flashes) shown at 30 %.
    let (mut ambient, mut add) = (g.lighting.ambient_out, g.lighting.lightmap_add);
    let special = g.lighting.special_cur;
    if settings.reduce_flashes && 0.0 < special {
        ambient = (g.lighting.ambient * g.lighting.global_inten + special * 0.3).clamp(0.0, 1.0);
        add = special * 0.3;
    }
    let dir = Vec3::new(g.level_f32(0xf0), g.level_f32(0xf4), g.level_f32(0xf8));
    let colour = Vec3::new(g.level_f32(0xfc), g.level_f32(0x100), g.level_f32(0x104)) * g.level_f32(0x108);
    let towards = if dir.length() > 0.0 { -dir.normalize() * MIRROR } else { Vec3::Y };
    let key = ([ambient, towards.x, towards.y, towards.z, colour.x, colour.y, colour.z, add], materials.len());
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    // Only the materials whose values differ are taken mutably: Assets'
    // iter_mut marks every material modified, and a new override material
    // (an effect's alpha or tint, about once a second in G1) changed the
    // count, so all ~3,000 were re-prepared with their bind groups — a 70 ms
    // frame twice a second (M20 profile, prof-g1-radv-v3).
    let light_dir = towards.extend(1.0);
    let light = colour.extend(ambient);
    let stale: Vec<_> = materials
        .iter()
        .filter(|(_, m)| if m.lit { m.params.light_dir != light_dir || m.params.light != light } else { m.params.scale.x != add })
        .map(|(id, _)| id)
        .collect();
    for id in stale {
        if let Some(m) = materials.get_mut(id) {
            if m.lit {
                m.params.light_dir = light_dir;
                m.params.light = light;
            } else {
                m.params.scale.x = add;
            }
        }
    }
}

/// ProcessEffects' MBPosLightNew for effects with an SfxSetLight radius
/// (slot +0x10 × sfx_lightradmul 1.0, attenuation sfx_lightattn 2.0, at the
/// effect + 1 up, colour slot +0..+8) and game code's lights (do_players'
/// dark-level light), nearest 8 to the camera, on every material
/// (pbDrawObject runs pbSetupPosLights for each object). HYPOTHESIS: the
/// life fade of the radius is not applied.
fn present_point_lights(sim: Res<Sim>, mut last: Local<Vec<[f32; 8]>>, queue: Res<bevy::render::renderer::RenderQueue>) {
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
    // TEST switch: no point lights (renderer comparisons).
    if std::env::var_os("GDL_NO_POINT_LIGHTS").is_some() {
        lights.clear();
    }
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
    gdl_client::write_point_lights(&queue, &pos, &col);
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
    /// GDL_FREEZE_AT reached: no simulation step, and the 30 Hz
    /// presentation clocks (texmods, particles) hold too.
    frozen: bool,
    /// GDL_FREEZE_AT given: those clocks advance one AFrame per stepped
    /// update (SPF 1/30), none dropped, instead of by render time, so a
    /// frozen frame is the same whatever the frame timing before it.
    sim_clock: bool,
    /// The held weapon's tree flag 2 (hidden) per player (Event::PlayerTreeFlags).
    weapon_hidden: [bool; 4],
    exp: i64,
    sparks: Vec<(Vec3, f32)>,
    last_event: String,
    /// ScrollMessageBox: the game waits until the box is dismissed.
    /// ScrollMessageBox (modal; the simulation waits).
    message_box: Option<gdl_core::game::hud::ScrollBox>,
    /// The message box is ControllerMessageBox's (closed by reassignment).
    controller_box: bool,
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
    /// do_shop's end: the Tower's level index and whether the shop was
    /// skipped (init_shop's skip_shop).
    shop_done: Option<(usize, bool)>,
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
        let params = gdl_client::BlitParams { rgba: Vec4::ONE, crop: Vec4::new(0.0, 1.0, 0.0, 1.0), mask_rect: Vec4::ZERO, corners: [Vec4::ZERO; 4] };
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
    let vol_music = sim.game.vol_music;
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
            a.play_pcm(pcm, rate, ch, vol_music);
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
                    // OptionsSetup's AudioSetVolMusic(GamePrefs word 1).
                    a.play_pcm(pcm, rate, ch, i32::from(front.menu.music));
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

/// One port's joyGetStatus entries this update. Physical buttons measured in
/// the reference (private/reports/trace-buttons-{a,b}-v1.json, research/
/// player.md "Pad buttons"): entries 0–3 d-pad (left, right, up, down), 0xd
/// cross (attack), 0xb circle (magic), 0xc triangle (turbo; closes
/// scrolls), 0xa square, 8 L1 (dash), 9 L2 (team move), 0x12 R1 (strafe),
/// 0x13 R2, 0x14/0x15 start/select; L3/R3 set nothing. NATIVE keyboard
/// choice — port 0: WASD/arrows, Space/K cross, E/L circle, Shift/I
/// triangle, Q/J square, Z L1, X L2, Ctrl R1, V R2, Enter start, Tab
/// select (K/L/I/J as the reference's PCSX2 bindings); port 1: TFGH, R
/// cross, Y circle, U start, B L2; gamepads by their face/shoulder
/// buttons (left stick as analog entries 4–7).
fn player_input(slot: usize, keys: &ButtonInput<KeyCode>, pads: &[&Gamepad], bindings: &gdl_client::controls::Bindings, keyboard: bool) -> PadStatus {
    let mut s = PadStatus { connected: true, ..Default::default() };
    let press = |s: &mut PadStatus, i: usize, on: bool| {
        if on {
            s.press(i);
        }
    };
    // `keyboard` false: the settings overlay has the keys.
    let k = |a: KeyCode| keyboard && keys.pressed(a);
    match slot {
        0 => {
            // Settings::keys may rebind them (gdl_client::controls).
            for (a, ks) in gdl_client::controls::ACTIONS.iter().zip(&bindings.keys) {
                press(&mut s, a.entry, ks.iter().any(|&key| k(key)));
            }
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
        press(&mut s, 0xc, b(GamepadButton::North));
        press(&mut s, 0xa, b(GamepadButton::West));
        press(&mut s, 8, b(GamepadButton::LeftTrigger));
        press(&mut s, 9, b(GamepadButton::LeftTrigger2));
        press(&mut s, 0x12, b(GamepadButton::RightTrigger));
        press(&mut s, 0x13, b(GamepadButton::RightTrigger2));
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
    attention: Vec3,
}

/// TEST harness (render-FPS independence, research/loop-input.md):
/// GDL_INPUT_SCRIPT is a JSON list of [first update, last update, [entries]]
/// pressing those joyGetStatus entries on port 0 for every stepped update in
/// the range (the keyboard is ignored); GDL_EXIT_AFTER=N writes the
/// simulation state's SHA-256 to GDL_STATE_OUT after N stepped updates and
/// exits; GDL_RENDER_FPS caps the render loop; GDL_FREEZE_AT=N stops
/// stepping the level after N updates while frames keep rendering (renderer
/// A/B captures at one simulation moment). Not a game feature.
#[derive(Resource, Default)]
struct TestHarness {
    script: Option<Vec<(u64, u64, Vec<usize>)>>,
    exit_after: Option<u64>,
    freeze_at: Option<u64>,
    state_out: Option<std::path::PathBuf>,
    hashes: Vec<String>,
    autostarted: bool,
    /// GDL_TEST_ACTIONS: [{"at": update, "teleport": [x, y, z]} |
    /// {"at": u, "health": h} | {"at": u, "gold": n} | {"at": u, "dump": "label"} |
    /// {"at": u, "boss_kill": true} (DEBUG) |
    /// {"at": u, "save": card file}] for player 1.
    actions: Vec<serde_json::Value>,
    /// GDL_MENU_SCRIPT: like GDL_INPUT_SCRIPT, indexed by the ticks spent
    /// outside play (tally, shop, select screen, map, message boxes).
    menu_script: Option<Vec<(u64, u64, Vec<usize>)>>,
    menu_ticks: u64,
    /// {"at": u, "disconnect": port, "reconnect_after": ticks}: the port
    /// reports no device until `ticks` simulate ticks of the controller
    /// message have passed, then START is held on it for a few ticks.
    disconnect: Option<(usize, u64)>,
    lost_ticks: u64,
}

impl TestHarness {
    fn from_env() -> Self {
        let script = std::env::var("GDL_INPUT_SCRIPT").ok().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok());
        let exit_after = std::env::var("GDL_EXIT_AFTER").ok().and_then(|v| v.parse().ok());
        let freeze_at = std::env::var("GDL_FREEZE_AT").ok().and_then(|v| v.parse().ok());
        let state_out = std::env::var_os("GDL_STATE_OUT").map(std::path::PathBuf::from);
        let actions = std::env::var("GDL_TEST_ACTIONS").ok().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let menu_script = std::env::var("GDL_MENU_SCRIPT").ok().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok());
        Self { script, exit_after, freeze_at, state_out, hashes: Vec::new(), autostarted: false, actions, menu_script, menu_ticks: 0, disconnect: None, lost_ticks: 0 }
    }
}

/// SHA-256 of the simulation records (players, enemies, items, effects,
/// rand state, Time).
fn sim_state_hash(g: &Game) -> String {
    let mut b = Vec::new();
    for r in &g.players {
        b.extend_from_slice(&r.0);
    }
    for r in &g.enemies {
        b.extend_from_slice(&r.0);
    }
    for r in &g.items.records {
        b.extend_from_slice(&r.0);
    }
    for r in &g.effects.slots {
        b.extend_from_slice(&r.0);
    }
    b.extend_from_slice(&g.rng.state.to_le_bytes());
    b.extend_from_slice(&g.globals.time.to_bits().to_le_bytes());
    gdl_core::save::sha256(&b)
}

/// Player 1's progression words and save region hash (TEST dumps).
fn progress_dump(g: &Game, label: &str, update: u64) -> serde_json::Value {
    let r = &g.players[0];
    // +0xc: the class (dumps before 2026-10-27 read +0, the player index,
    // so their gold was class 0's).
    let class = r.u32_(0xc) as usize;
    let rec = profile::CLASS_RECORD + profile::CLASS_STRIDE * class.min(15);
    let waves: Vec<String> = (0..16).map(|c| r.0[0x182c + 0xe * c..0x182c + 0xe * c + 0xe].iter().map(|b| format!("{b:02x}")).collect()).collect();
    let region = profile::save_region(r, &[0, 2, 1, 1]);
    serde_json::json!({"label": label, "update": update, "world": g.globals.cur_world, "level": g.globals.cur_level, "state": r.i32_(0xfc),
        "health": r.f32_(0x1a10), "exp": r.i32_(0x1a1c), "gold": r.i32_(rec + 0x30), "keys": r.i32_(0x1a14), "potions": r.i32_(0x1a18),
        "shards": r.u16_(0x1a24), "runes": r.u16_(0x1a26), "waves": waves, "region_sha256": gdl_core::save::sha256(&region)})
}

/// GDL_RENDER_FPS: sleep to the next frame deadline (TEST harness); with
/// GDL_STATE_OUT the rendered frames and wall time go to <out>.frames.
/// GDL_UI_TRACE=FILE (TEST): one line per change of the screen, the
/// front-end page or the in-game modal state (select screen, shop, map,
/// movie, message box, pause), with the frame number — menu flow tests
/// assert the sequence a key script produces.
fn ui_trace(front: Option<Res<Front>>, screen: Option<Res<Screen>>, ui: Res<Ui>, sim: Option<Res<Sim>>, mut state: Local<(u64, String, Option<std::path::PathBuf>, bool)>) {
    if !state.3 {
        state.3 = true;
        state.2 = std::env::var_os("GDL_UI_TRACE").map(std::path::PathBuf::from);
    }
    state.0 += 1;
    let Some(path) = state.2.clone() else { return };
    let page = front.as_ref().map_or(String::new(), |f| format!("{:?}", f.menu.page));
    let screen = screen.map_or("none", |s| if *s == Screen::Playing { "playing" } else { "front" });
    let modal = sim.as_ref().map_or(String::new(), |s| {
        let mut v = Vec::new();
        if s.select.is_some() { v.push("select") }
        if s.shop.is_some() { v.push("shop") }
        if s.map.is_some() { v.push("map") }
        if s.movie.is_some() { v.push("movie") }
        if s.message_box.is_some() { v.push("message_box") }
        if s.flyby.is_some() { v.push("flyby") }
        if s.game.saver.active { v.push("saver") }
        let mut line = format!("{} world {} level {}", v.join("+"), s.game.globals.cur_world, s.game.globals.cur_level);
        if let Some(e) = &s.error {
            line += &format!(" | ERROR {e}");
        }
        let unsupported: Vec<String> = s.game.log.unsupported.iter().filter(|(_, n)| 0 < **n).map(|(k, _)| k.clone()).collect();
        if !unsupported.is_empty() {
            line += &format!(" | UNSUPPORTED {}", unsupported.join("; "));
        }
        line
    });
    let mut line = format!("{screen} paused={} {page} | {modal}", ui.paused);
    if let Some(f) = front.as_ref().filter(|f| f.ending.active()) {
        // end_optmenu's leftovers: fading menus and the fire scroll.
        line += &format!(" | ending finish {}{}", f.ending.finishing.len(), if f.ending.burn.is_some() { " burn" } else { "" });
    }
    if line != state.1 {
        state.1 = line.clone();
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(f, "{} {line}", state.0);
        }
    }
}

fn frame_limiter(mut next: Local<Option<std::time::Instant>>, mut count: Local<(u64, Option<std::time::Instant>)>, harness: Res<TestHarness>, settings: Res<gdl_client::settings::Settings>) {
    let t0 = *count.1.get_or_insert_with(std::time::Instant::now);
    count.0 += 1;
    if count.0.is_multiple_of(30)
        && let Some(out) = harness.state_out.as_ref()
    {
        let _ = std::fs::write(out.with_extension("frames"), format!("{} {:.3}\n", count.0, t0.elapsed().as_secs_f64()));
    }
    let fps = std::env::var("GDL_RENDER_FPS").ok().and_then(|v| v.parse::<f64>().ok()).or((0 < settings.fps_cap).then_some(f64::from(settings.fps_cap)));
    let Some(fps) = fps.filter(|f| *f > 0.0) else { return };
    let period = std::time::Duration::from_secs_f64(1.0 / fps);
    let now = std::time::Instant::now();
    let due = next.map_or(now + period, |d| d);
    if now < due {
        std::thread::sleep(due - now);
    }
    *next = Some(due.max(now) + period);
}

/// DIAGNOSTIC (performance): GDL_FRAME_LOG=PATH appends one line per
/// rendered frame — frame, milliseconds since the previous frame, entity
/// count, the simulation's update count, player 0's current action, the
/// material count, the visible 3D meshes and their distinct materials, the
/// shown camera position, the frame's Time delta (ms) and player 1's shown
/// body position — for frame-time and motion-uniformity comparisons of game
/// situations. Not a game feature.
fn frame_log(
    mut state: Local<Option<(std::fs::File, std::time::Instant, u64)>>,
    entities: Query<Entity>,
    sim: Option<Res<Sim>>,
    materials: Res<Assets<gdl_client::GsMaterial>>,
    visible: Query<(&ViewVisibility, &MeshMaterial3d<gdl_client::GsMaterial>)>,
    camera: Query<&GlobalTransform, With<FollowCamera>>,
    (time, bodies): (Res<Time>, Query<(&CharacterRoot, &GlobalTransform)>),
) {
    use std::io::Write;
    if state.is_none() {
        let Some(path) = std::env::var_os("GDL_FRAME_LOG") else { return };
        let Ok(f) = std::fs::OpenOptions::new().create(true).append(true).open(path) else { return };
        *state = Some((f, std::time::Instant::now(), 0));
        return;
    }
    let (f, last, n) = state.as_mut().unwrap();
    let now = std::time::Instant::now();
    let ms = now.duration_since(*last).as_secs_f64() * 1000.0;
    *last = now;
    *n += 1;
    let (updates, action) = sim.map_or((0, 0), |s| (s.updates, s.game.players[0].u32_(0x1e0)));
    // Also the GsMaterial assets, the visible 3D meshes and their distinct
    // materials (draw batching).
    let mut seen = std::collections::HashSet::new();
    let shown = visible.iter().filter(|(v, _)| v.get()).inspect(|(_, m)| {
        seen.insert(m.0.id());
    }).count();
    // And the shown camera position (presentation blending check).
    let eye = camera.iter().next().map_or(Vec3::ZERO, GlobalTransform::translation);
    // And the frame's Time delta (what the blending and the fixed clock
    // use) and player 1's shown body position (motion uniformity).
    let body = bodies.iter().find(|(c, _)| c.0 == 0).map_or(Vec3::ZERO, |(_, g)| g.translation());
    let _ = writeln!(
        f,
        "{} {:.2} {} {} {} {} {} {} {:.4} {:.4} {:.4} {:.3} {:.4} {:.4} {:.4}",
        n,
        ms,
        entities.iter().count(),
        updates,
        action,
        materials.len(),
        shown,
        seen.len(),
        eye.x,
        eye.y,
        eye.z,
        time.delta_secs_f64() * 1000.0,
        body.x,
        body.y,
        body.z
    );
}

/// DIAGNOSTIC: GDL_SHOT_FRAMES=N,M,... with GDL_SHOT_DIR=DIR saves rendered
/// frames N, M, ... (counted from the start) as DIR/frame-NNNNN.png through
/// Bevy's screenshot capture — the GPU's own output, where the private
/// display's window import cannot read a GPU-presented window. Not a game
/// feature.
fn diagnostic_shots(mut commands: Commands, mut frame: Local<u64>) {
    static SHOTS: std::sync::OnceLock<Option<(Vec<u64>, std::path::PathBuf)>> = std::sync::OnceLock::new();
    *frame += 1;
    let Some((frames, dir)) = SHOTS.get_or_init(|| {
        let frames = std::env::var("GDL_SHOT_FRAMES").ok()?.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        Some((frames, std::env::var_os("GDL_SHOT_DIR")?.into()))
    }) else {
        return;
    };
    if frames.contains(&*frame) {
        let path = dir.join(format!("frame-{:05}.png", *frame));
        commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(path));
    }
}

/// DIAGNOSTIC (GDL_SCENE_DUMP=FRAME[,FRAME…], GDL_SHOT_DIR): every drawn
/// mesh's world transform, mesh, textures and material parameters at those
/// frames, one sorted line each, to scene-NNNNN.txt — two runs' dumps diff
/// to the entities that render differently.
#[allow(clippy::type_complexity)]
fn diagnostic_scene_dump(
    mut frame: Local<u64>,
    q: Query<(&GlobalTransform, &ViewVisibility, &Mesh3d, &MeshMaterial3d<gdl_client::GsMaterial>)>,
    materials: Res<Assets<gdl_client::GsMaterial>>,
    (images, meshes): (Res<Assets<Image>>, Res<Assets<Mesh>>),
) {
    static DUMPS: std::sync::OnceLock<Option<(Vec<u64>, std::path::PathBuf)>> = std::sync::OnceLock::new();
    *frame += 1;
    let Some((frames, dir)) = DUMPS.get_or_init(|| {
        let frames = std::env::var("GDL_SCENE_DUMP").ok()?.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        Some((frames, std::env::var_os("GDL_SHOT_DIR")?.into()))
    }) else {
        return;
    };
    if !frames.contains(&*frame) {
        return;
    }
    // Asset indices follow load order: textures and meshes by content.
    let digest = |bytes: &[u8]| {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        h.finish()
    };
    let image = |h: Option<&Handle<Image>>| h.and_then(|h| images.get(h)).map(|i| (i.width(), i.height(), i.data.as_deref().map(digest)));
    let mut lines: Vec<String> = q
        .iter()
        .filter(|(_, v, ..)| v.get())
        .map(|(g, _, mesh, mat)| {
            let m = materials.get(&mat.0);
            let p = m.map(|m| m.params);
            let mesh = meshes.get(&mesh.0).map(|m| {
                let mut bytes: Vec<u8> = m.attributes().flat_map(|(_, a)| a.get_bytes().to_vec()).collect();
                if let Some(i) = m.indices() {
                    bytes.extend(i.iter().flat_map(|k| (k as u32).to_le_bytes()));
                }
                (m.count_vertices(), digest(&bytes))
            });
            format!(
                "{:.3?} mesh {:?} base {:?} lm {:?} ov {:?} {:?} {:?}",
                g.to_matrix().to_cols_array(),
                mesh,
                image(m.and_then(|m| m.base.as_ref())),
                image(m.and_then(|m| m.lightmap.as_ref())),
                image(m.and_then(|m| m.overlay.as_ref())),
                m.map(|m| m.alpha_mode),
                p
            )
        })
        .collect();
    lines.sort();
    let _ = std::fs::write(dir.join(format!("scene-{:05}.txt", *frame)), lines.join("\n") + "\n");
}

/// M19 presentation blending (NATIVE ENHANCEMENT; Settings::interpolate):
/// a 3D entity's transform at the last two simulation updates.
#[derive(Component)]
struct Interp {
    prev: Transform,
    cur: Transform,
}

/// A step longer than this (units) between two updates is a cut, not motion.
const INTERP_CUT: f32 = 6.0;

/// The 3D scene (MirrorRoot's descendants and the follow camera) shown
/// between the last two simulation updates at the display rate: after the
/// presentation systems set each transform from the simulation, the shown
/// transform is prev + (cur − prev) × the fixed clock's overstep fraction —
/// one update (33 ms) behind, smooth at any refresh rate. The simulation
/// keeps its 30 Hz update and reads none of this. A transform the
/// presentation writes without a new update and to another value (a
/// render-rate pose) is shown as written; one not written stays put; a cut
/// (more than INTERP_CUT units in one update) is not blended. Off
/// (Settings::interpolate false, GDL_BASELINE=1): every update shown as it
/// is, the original 30 Hz picture.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn interpolate_transforms(
    mut commands: Commands,
    sim: Option<Res<Sim>>,
    settings: Res<gdl_client::settings::Settings>,
    fixed: Res<Time<Fixed>>,
    mut last: Local<Option<u64>>,
    mut was_on: Local<bool>,
    roots: Query<(), With<MirrorRoot>>,
    tracked: Query<(), With<Interp>>,
    fresh: Query<(Entity, &Transform, Option<&ChildOf>, Has<FollowCamera>), Without<Interp>>,
    mut q: Query<(&mut Transform, &mut Interp)>,
) {
    let Some(sim) = sim else { return };
    if !settings.interpolate {
        if std::mem::take(&mut *was_on) {
            // Turned off: every transform back to its latest update.
            for (mut tf, ip) in &mut q {
                if !tf.is_changed() && *tf != ip.cur {
                    *tf = ip.cur;
                }
            }
        }
        return;
    }
    *was_on = true;
    let stepped = last.replace(sim.updates) != Some(sim.updates);
    let alpha = fixed.overstep_fraction().clamp(0.0, 1.0);
    for (e, tf, parent, camera) in &fresh {
        if camera || parent.is_some_and(|p| roots.contains(p.parent()) || tracked.contains(p.parent())) {
            commands.entity(e).insert(Interp { prev: *tf, cur: *tf });
        }
    }
    for (mut tf, mut ip) in &mut q {
        if tf.is_changed() {
            let new = *tf;
            if stepped {
                ip.prev = if ip.cur.translation.distance(new.translation) > INTERP_CUT { new } else { ip.cur };
                ip.cur = new;
            } else if new != ip.cur {
                ip.prev = new;
                ip.cur = new;
            }
        } else if stepped {
            ip.prev = ip.cur;
        }
        let shown = if ip.prev == ip.cur {
            ip.cur
        } else {
            Transform {
                translation: ip.prev.translation.lerp(ip.cur.translation, alpha),
                rotation: ip.prev.rotation.slerp(ip.cur.rotation, alpha),
                scale: ip.prev.scale.lerp(ip.cur.scale, alpha),
            }
        };
        if *tf != shown {
            *tf = shown;
        }
    }
}

#[derive(Component)]
struct MirrorRoot;
#[derive(Component)]
/// The character body of player slot `.0`.
struct CharacterRoot(usize);
/// A rig node (tree index) of player slot `.1`.
#[derive(Component)]
struct RigNode(usize, usize);

/// A mesh of the player's HEAD node: PlayerProcessPowerups at level ≥ 99
/// sets its MB flag 8 and node scale (+0x40..+0x48) to 1.5. HYPOTHESIS: the
/// scale reaches only the node's own object (GetWorldMat ignores +0x40, so
/// the hair, braids and anything hung on the head keep their size).
#[derive(Component)]
struct HeadMesh;

/// A mesh of the left-hand node (false) or its parent (true): hidden (MB
/// flag 1) while the left hand holds an object — the parent for base
/// classes 1 and 5, the hand for class 7.
#[derive(Component)]
struct LeftHandMesh(bool);

/// PlayerProcessPowerups' "It" sign: MBOX_NewObject(IT_SIGN, 0, player node,
/// 0x10) while +0x92c (the It enemy's tag) is set and +0x93c bit 2 is clear,
/// removed otherwise (the realm's ITEMS package holds IT_SIGN).
#[derive(Component)]
struct ItSign {
    slot: usize,
    spawned: bool,
}

#[allow(clippy::too_many_arguments)]
fn present_it_sign(
    mut commands: Commands,
    sim: Res<Sim>,
    libs: Res<Libraries>,
    (session, sources): (Res<Session>, Res<Sources>),
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
    mut variants: ResMut<PartVariants>,
    mut pending: ResMut<PendingTreePsys>,
    mut q: Query<(Entity, &mut ItSign, &mut Visibility)>,
) {
    for (e, mut sign, mut vis) in &mut q {
        let r = &sim.game.players[sign.slot];
        let show = r.i16_(0x92c) != 0 && r.u16_(0x93c) & 2 == 0;
        if show && !sign.spawned {
            if let Some(parts) = libs.ordered(&search_order(&session, &sources)).find_map(|l| l.by_label.get("IT_SIGN")) {
                spawn_parts(&mut commands, e, vec![(Vec3::ZERO, parts.clone(), 0x10, None, None)], &mut materials, &mut variants, &mut pending);
            }
            sign.spawned = true;
        }
        *vis = if show { Visibility::Inherited } else { Visibility::Hidden };
    }
}

/// A part of player slot `.0`'s held weapon (load_player's +0x6b8 WEAP_HOLD):
/// DoPlayerAction's MBTreeSetFlags/ClearFlags(+0x6b8, 2) hide and show it.
#[derive(Component)]
struct HeldWeapon(usize);

/// Player slot `.0`'s weapon-trail copies (PlayerDoWeapTrail), under the
/// mirror root in PS2 world space.
#[derive(Component)]
struct WeaponTrail(usize);

/// Trail copy slot (player, slot) at its world matrix.
#[derive(Component)]
struct TrailCopyNode(usize, usize);

/// One weapon part of a trail copy: its own material (alpha × (255 −
/// fade) / 255, the MBTreeSetAlpha reading of rendering.md).
#[derive(Component)]
struct TrailPart {
    slot: usize,
    copy: usize,
    base_alpha: f32,
}
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
    assemble_library(pkg, set, parts)
}

/// A package's library from its materials and object meshes.
fn assemble_library(pkg: &Package, set: gdl_client::MaterialSet, parts: HashMap<u32, Parts>) -> Library {
    let mut by_label = HashMap::new();
    let mut index_of = HashMap::new();
    for d in &pkg.objects.definitions {
        index_of.entry(d.label.to_uppercase()).or_insert(d.object_index as u32);
        if let Some(list) = parts.get(&(d.object_index as u32)) {
            by_label.entry(d.label.clone()).or_insert_with(|| list.clone());
        }
    }
    let texmods = gdl_formats::animation::texmods(&pkg.anim_bytes).unwrap_or_default();
    let unresolved: std::collections::HashSet<u16> = set.unresolved.iter().map(|&i| i as u16).collect();
    // DIAGNOSTIC: GDL_LOG_UNRESOLVED lists each package's bitmap records
    // drawn with the magenta placeholder, the TEXDEF labels at their offset
    // and the object definitions whose parts use them.
    if std::env::var_os("GDL_LOG_UNRESOLVED").is_some() && !unresolved.is_empty() {
        for b in pkg.objects.bitmap_records.iter().filter(|b| unresolved.contains(&(b.index as u16))) {
            let labels: Vec<&str> = pkg.table.bitmaps.iter().filter(|t| t.texture_offset == b.texture_offset).map(|t| t.label.as_str()).collect();
            let users: Vec<&str> = pkg
                .objects
                .definitions
                .iter()
                .filter(|d| pkg.objects.objects.get(d.object_index as usize).is_some_and(|o| o.parts.iter().any(|p| u32::from(p.texture_index) == b.index)))
                .map(|d| d.label.as_str())
                .collect();
            eprintln!("UNRESOLVED record {} format {} flags {:#x} {}x{} labels {:?} used by {:?}", b.index, b.format, b.flags, b.width, b.height, labels, users);
        }
    }
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
        Projection::custom(Ps2Projection::new(tan_v * 640.0 / 384.0, tan_v, 1.0, 65536.0)),
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
        gdl_client::DisplayToLinear { dither: if std::env::var_os("GDL_NO_DITHER").is_some() { 0.0 } else { 1.0 }, ..default() },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::Fixed { width: 512.0, height: 384.0 },
            ..OrthographicProjection::default_2d()
        }),
        bevy::camera::visibility::RenderLayers::layer(HUD_LAYER),
        HudCamera,
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
/// A per-class node name table of the ELF (player_rhand / player_lhand).
fn player_hand(elf_bytes: &[u8], table: &str, class: u32) -> Option<String> {
    let elf = gdl_formats::elf::Elf::parse(elf_bytes).ok()?;
    let table = elf.object(table).ok()?;
    let p = u32::from_le_bytes(table.get(class as usize * 4..class as usize * 4 + 4)?.try_into().ok()?);
    let s = elf.read(p, 16).ok()?;
    Some(s.iter().take_while(|b| **b != 0).map(|b| char::from(*b)).collect())
}

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
        // No held-weapon effect while the right hand holds an object.
        let right = !right_hand_frozen(g, h.slot) && hand_objects(g, h.slot)[2].is_some();
        let label = g.effects.tables.dmg_type_desc.get(t).filter(|d| !d.is_empty() && !right).map(|d| format!("WEAP_HOLD_{d}"));
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

/// PlayerProcessFamiliar 0x12cc18: from level 30 the player's familiar —
/// InitPlayerMissiles' FamiliarTree, the SFX package's FAMILIAR1 (FAMILIAR2
/// from level 80; one OANIM node, sequences READY 20 frames repeating and
/// ATTACK 9) — under the player's node (ReplaceTree flag 0x800), placed at
/// PDAT +0x164..+0x16c (× 1.2 at level 99: the tree's +0x10 and its root
/// node's +0x30..+0x38, the local matrix's translation row), hidden in play
/// (game mode 0x4010) while +0x138 bit 0x80 is set, animated every update
/// on its TreeClock: AnimateATree(tree, attack, attack ? 2 : 0) with the
/// attack signal +0x8d8 0x10000000 (Game::tree_anim's counter).
#[derive(Component)]
struct Familiar {
    slot: usize,
    key: Option<&'static str>,
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn present_familiars(
    mut commands: Commands,
    sim: Res<Sim>,
    sources: Res<Sources>,
    libs: Res<Libraries>,
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
    mut variants: ResMut<PartVariants>,
    mut pending: ResMut<PendingTreePsys>,
    mut q: Query<(Entity, &mut Familiar, &mut Transform, &mut Visibility, Option<&mut TreeClock>, Option<&mut EffectTree>)>,
) {
    let g = &sim.game;
    for (e, mut f, mut tf, mut vis, clock, fx) in &mut q {
        let r = &g.players[f.slot];
        let level = r.i32_(0x2ac0);
        let key = if 80 <= level {
            Some("FAMILIAR2")
        } else if 30 <= level {
            Some("FAMILIAR1")
        } else {
            None
        };
        if f.key != key {
            commands.entity(e).despawn_related::<Children>();
            commands.entity(e).remove::<(EffectTree, TreeClock)>();
            if let (Some(label), Some(pc)) = (key, sources.players[f.slot].as_ref()) {
                let parts = tree_parts(&sources, &libs, &pc.sfx_name, label);
                if std::env::var("GDL_DEBUG_FX").is_ok() {
                    let trees: Vec<String> = sources.packages.get(&pc.sfx_name).and_then(|p| p.anim.as_ref()).map(|a| a.trees.iter().map(|t| t.label.clone()).collect()).unwrap_or_default();
                    eprintln!("Familiar {} {label} in {}: {} parts; trees {trees:?}", f.slot, pc.sfx_name, parts.len());
                }
                spawn_parts(&mut commands, e, parts, &mut materials, &mut variants, &mut pending);
                let tree = sources.packages.get(&pc.sfx_name).and_then(|p| p.anim.as_ref()).and_then(|a| a.trees.iter().position(|t| t.label == label).map(|i| (i, gdl_core::game::setup::clips_of(a, i))));
                if let Some((tree, clips)) = tree {
                    commands.entity(e).insert((
                        EffectTree { pkg: pc.sfx_name.clone(), tree, start: g.globals.atime, timing: 1, slot: usize::MAX, looping: true, seq: 0, fixed: Some((0, 0.0)) },
                        TreeClock::new(clips, &g.tree_anim[f.slot]),
                    ));
                }
            }
            f.key = key;
        } else if let (Some(mut c), Some(mut fx)) = (clock, fx) {
            // PlayerProcessFamiliar: AnimateATree(tree, attack, attack ? 2 : 0)
            // with the attack signal +0x8d8 0x10000000.
            let req = &g.tree_anim[f.slot];
            let attack = req.attacks != c.seen_attacks;
            c.seen_attacks = req.attacks;
            fx.fixed = c.step(u16::from(attack), if attack { 2 } else { 0 }, g).or(fx.fixed);
        }
        let raw = &g.classes[f.slot].raw;
        let at = |o: usize| raw.get(o..o + 4).map_or(0.0, |b| f32::from_le_bytes(b.try_into().unwrap()));
        tf.translation = Vec3::new(at(0x164), at(0x168), at(0x16c)) * if 99 <= level { 1.2 } else { 1.0 };
        let hidden = f.key.is_none() || (g.globals.game_mode == 0x4010 && r.u32_(0x138) & 0x80 != 0);

        *vis = if hidden { Visibility::Hidden } else { Visibility::Inherited };
    }
}

/// Where PlayerProcessPowerups hangs the attached powerup tree (+0x768):
/// the player's node (+0x80), its HEAD node (+0x6ac, load_player's
/// "<prefix>HEAD") or, for WINGS, +0x80's first child's first child (MB
/// node +0x78: MBNodeInsert 0x1cf528 appends siblings, so the first child
/// created — ROOT, then its LOWERTORSO for the Sorceress).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PowerupAt {
    Node,
    Head,
    Wings,
}

/// One of a player's three anchors for the attached powerup tree.
#[derive(Component)]
struct PowerupTree {
    slot: usize,
    at: PowerupAt,
    key: Option<&'static str>,
}

/// PlayerProcessPowerups 0x12d040's choice (InitPlayerMissiles' trees): the
/// pojo (+0x138 0x400), the fire shield (+0x134 0x200000 during action
/// 0x16; WEAPONS' FW_SHLD_ACTIVE), the phoenix (0x80), fire/acid/electric
/// breath (0x10/0x20/0x40, HEAD_BREATHEF/A/E) or the wings (1), else none.
fn powerup_tree_choice(r: &Record) -> Option<(&'static str, &'static str, PowerupAt)> {
    let (r134, r138) = (r.u32_(0x134), r.u32_(0x138));
    if r138 & 0x400 != 0 {
        return Some(("POWERUPS", "POJO", PowerupAt::Node));
    }
    if r134 & 0x20_0000 != 0 && r.u32_(0x1e0) == 0x16 {
        return Some(("WEAPONS", "FW_SHLD_ACTIVE", PowerupAt::Node));
    }
    [(0x80, "PHOENIX", PowerupAt::Node), (0x10, "HEAD_BREATHEF", PowerupAt::Head), (0x20, "HEAD_BREATHEA", PowerupAt::Head), (0x40, "HEAD_BREATHEE", PowerupAt::Head), (1, "WINGS", PowerupAt::Wings)]
        .into_iter()
        .find(|(bit, _, _)| r138 & bit != 0)
        .map(|(_, label, at)| ("POWERUPS", label, at))
}

/// The attached powerup tree: spawned on the matching anchor, animated
/// on its own AnimateATree clock (TreeClock) from the core's requests
/// (Game::tree_anim), faded with MBTreeSetAlpha((1 − t) × 255) over the last
/// second of the longest such powerup (t: kind-6 0x200000 / kind-9 0x4f1
/// slots' remaining time).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn present_powerup_trees(
    mut commands: Commands,
    sim: Res<Sim>,
    sources: Res<Sources>,
    libs: Res<Libraries>,
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
    mut variants: ResMut<PartVariants>,
    mut pending: ResMut<PendingTreePsys>,
    mut req: ResMut<MatRequests>,
    mut q: Query<(Entity, &mut PowerupTree, &mut Visibility, Option<&Children>, Option<&mut TreeClock>, Option<&mut EffectTree>)>,
) {
    let g = &sim.game;
    for (e, mut t, mut vis, children, clock, fx) in &mut q {
        let r = &g.players[t.slot];
        let choice = powerup_tree_choice(r).filter(|c| c.2 == t.at);
        let want = choice.map(|c| c.1);
        if t.key != want {
            commands.entity(e).despawn_related::<Children>();
            commands.entity(e).remove::<(EffectTree, TreeClock)>();
            if let Some((pkg, label, _)) = choice {
                let parts = tree_parts(&sources, &libs, pkg, label);
                if std::env::var("GDL_DEBUG_FX").is_ok() {
                    eprintln!("PowerupTree {} {:?} {label} in {pkg}: {} parts {:?}", t.slot, t.at, parts.len(), parts.iter().map(|p| (p.3, p.2, p.1.len(), p.0)).collect::<Vec<_>>());
                }
                spawn_parts(&mut commands, e, parts, &mut materials, &mut variants, &mut pending);
                let tree = sources.packages.get(pkg).and_then(|p| p.anim.as_ref()).and_then(|a| a.trees.iter().position(|x| x.label == label).map(|i| (i, gdl_core::game::setup::clips_of(a, i))));
                if let Some((tree, clips)) = tree {
                    commands.entity(e).insert((
                        EffectTree { pkg: pkg.to_string(), tree, start: g.globals.atime, timing: 1, slot: usize::MAX, looping: true, seq: 0, fixed: Some((0, 0.0)) },
                        TreeClock::new(clips, &g.tree_anim[t.slot]),
                    ));
                }
            }
            t.key = want;
        } else if let (Some((_, label, _)), Some(mut c), Some(mut fx)) = (choice, clock, fx) {
            // PlayerProcessPowerups' AnimateATree(+0x768, uVar7, uVar13):
            // the pojo's sequence by action (2 on its attack signal), the
            // others' 1 on the familiar signal when the tree has two
            // sequences (+0x778, HYPOTHESIS: the AnimInfo at +0x76c, so +0x778
            // its sequence count and +0x77a its current sequence); mode 2
            // while the current sequence is 0, else uVar14.
            let req = &g.tree_anim[t.slot];
            let attack = req.attacks != c.seen_attacks;
            let pojo_attack = req.pojo_attacks != c.seen_pojo;
            (c.seen_attacks, c.seen_pojo) = (req.attacks, req.pojo_attacks);
            let (seq, now) = if label == "POJO" {
                if pojo_attack { (2, true) } else { (req.pojo_seq, req.pojo_now) }
            } else if attack && 2 <= c.clips.len() {
                (1, true)
            } else {
                (0, false)
            };
            let mode = if c.current != 0 && !now { 0 } else { 2 };
            fx.fixed = c.step(seq, mode, g).or(fx.fixed);
        }
        *vis = if t.key.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        // fVar18: the longest remaining time of the tree powerups.
        let mut fade = -1.0f32;
        for k in 0..8 {
            let o = 0x144 + 16 * k;
            let (time, kind, flags) = (r.f32_(o), r.i32_(o + 4), r.u32_(o + 12));
            if time != 0.0 && ((kind == 6 && flags & 0x20_0000 != 0) || (kind == 9 && flags & 0x4f1 != 0)) && fade < time {
                fade = time;
            }
        }
        if t.key.is_some()
            && (0.0..1.0).contains(&fade)
            && let Some(children) = children
        {
            let v = ((1.0 - fade) * 255.0) as i32;
            for c in children.iter() {
                req.alpha(c, (255 - v) as f32 / 255.0);
            }
        }
    }
}

/// PlayerProcessPowerups' attached objects (MBNewObject / MBSetObject /
/// MBRemoveNode): [left hand +0x704 on +0x6a4, head +0x70c on +0x6ac, right
/// hand +0x708 on +0x6a8] with their MB flags. Left: the left boss gauntlet
/// (+0x138 0x8000), else RF_SHLD (+0x134 0x20000), FW_SHLD (0x200000),
/// L_SHLD (0x400000). Head: BOSSHORNS (+0x138 0x1000), BOSSMASK (0x2000),
/// HEAD_HALO (+0x134 0x80000), HEAD_GAS (+0x134 0x2000), HEAD_XRAY (+0x138
/// 2). Right (left as it was — None here — while QuestItemState and the
/// legend stage +0x80c are set): the right boss gauntlet (+0x138 0x4000),
/// SUPERXBOW (+0x130 0x100000), HAMMER_HD (+0x130 0x10000000).
fn hand_objects(g: &Game, k: usize) -> [Option<(&'static str, u32)>; 3] {
    let r = &g.players[k];
    let (r130, r134, r138) = (r.u32_(0x130), r.u32_(0x134), r.u32_(0x138));
    let left = if r138 & 0x8000 != 0 {
        Some(("BOSSGAUNTL", 0x9010))
    } else if r134 & 0x2_0000 != 0 {
        Some(("RF_SHLD", 0x810))
    } else if r134 & 0x20_0000 != 0 {
        Some(("FW_SHLD", 0x810))
    } else if r134 & 0x40_0000 != 0 {
        Some(("L_SHLD", 0x810))
    } else {
        None
    };
    let head = if r138 & 0x1000 != 0 {
        Some(("BOSSHORNS", 0x9010))
    } else if r138 & 0x2000 != 0 {
        Some(("BOSSMASK", 0x9010))
    } else if r134 & 0x8_0000 != 0 {
        Some(("HEAD_HALO", 0x810))
    } else if r134 & 0x2000 != 0 {
        Some(("HEAD_GAS", 0x810))
    } else if r138 & 2 != 0 {
        Some(("HEAD_XRAY", 0x810))
    } else {
        None
    };
    let right = if r138 & 0x4000 != 0 {
        Some(("BOSSGAUNTR", 0x9010))
    } else if r130 & 0x10_0000 != 0 {
        Some(("SUPERXBOW", 0x810))
    } else if r130 & 0x1000_0000 != 0 {
        Some(("HAMMER_HD", 0x10))
    } else {
        None
    };
    [left, head, right]
}

/// The right-hand slot is kept as it was while QuestItemState and the
/// player's legend stage (+0x80c) are set.
fn right_hand_frozen(g: &Game, k: usize) -> bool {
    g.globals.quest_item_state != 0 && g.players[k].i32_(0x80c) != 0
}

/// One of a player's three attached-object anchors (0 left hand, 1 head, 2
/// right hand).
#[derive(Component)]
struct HandObject {
    slot: usize,
    at: usize,
    key: Option<&'static str>,
}

#[allow(clippy::too_many_arguments)]
fn present_hand_objects(
    mut commands: Commands,
    sim: Res<Sim>,
    libs: Res<Libraries>,
    (session, sources): (Res<Session>, Res<Sources>),
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
    mut variants: ResMut<PartVariants>,
    mut pending: ResMut<PendingTreePsys>,
    mut q: Query<(Entity, &mut HandObject, &mut Visibility)>,
) {
    for (e, mut h, mut vis) in &mut q {
        if h.at == 2 && right_hand_frozen(&sim.game, h.slot) {
            continue;
        }
        let want = hand_objects(&sim.game, h.slot)[h.at.min(2)];
        if h.key != want.map(|w| w.0) {
            commands.entity(e).despawn_related::<Children>();
            if let Some((label, flags)) = want
                && let Some(parts) = libs.ordered(&search_order(&session, &sources)).find_map(|l| l.by_label.get(label))
            {
                spawn_parts(&mut commands, e, vec![(Vec3::ZERO, parts.clone(), flags, None, None)], &mut materials, &mut variants, &mut pending);
            }
            h.key = want.map(|w| w.0);
        }
        *vis = if h.key.is_some() { Visibility::Inherited } else { Visibility::Hidden };
    }
}

fn present_special_texmods(sim: Res<Sim>, libs: Res<Libraries>, q: Query<(Entity, &SpecialTexmod)>, mut req: ResMut<MatRequests>, mut step: Local<(u64, u32)>) {
    // AFrame: one step per game update (DoSpecialTexmods runs per frame);
    // a render frame after several updates takes every multiple of the
    // rate among them (it took at most one).
    let aframe = sim.updates;
    for (e, t) in &q {
        if step.0 != aframe {
            let rate = u64::from(t.rate.max(1));
            let crossed = aframe / rate - step.0.min(aframe) / rate;
            *step = (aframe, ((u64::from(step.1) + crossed) % u64::from(t.frames.max(1))) as u32);
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
    // DoPlayerTexMods 0x12f0a8 (do_items): the model package's texture modifiers
    // (PlayerModelData +0x24 — the magic classes' <colour>_HANDGLOW, 5
    // frames; the Jester's BOMB); the SFX package's (+0x48) run with the
    // level's lists (texmod_packages); the clip packages hold none.
    if !character.anim_bytes.is_empty() {
        let list = gdl_formats::animation::texmods(&character.anim_bytes).unwrap_or_default();
        spawn_texmod_list(commands, &list, &set.label_index, &set.record_images, &[]);
    }
    let (parts, _) = gdl_client::object_meshes(&character.objects, &set, materials, meshes);
    let object_parts = |label: &str| -> Vec<(Handle<Mesh>, Handle<gdl_client::GsMaterial>)> {
        character.objects.definitions.iter().filter(|d| d.label == label).flat_map(|d| parts.get(&(d.object_index as u32)).cloned().unwrap_or_default()).collect()
    };
    let rhand = player_rhand(&session.elf_bytes, pc.key.class);
    let lhand = player_hand(&session.elf_bytes, "player_lhand", pc.key.class);
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
    // The left hand's parent node (classes 1 and 5 hide it while the left
    // hand holds an object; class 7 hides the hand itself).
    let lhand_parent = lhand.as_ref().and_then(|l| tree.nodes.iter().find(|n| &n.label == l)).and_then(|n| tree.nodes.iter().find(|m| m.index as i32 == n.parent)).map(|m| m.label.clone());
    // Rig nodes pose by the class clip node of the same label (the root,
    // "UNNAMED" in the model, is clip node 0).
    let clip_nodes = &pc.clips.trees[0].nodes;
    let first_child = |parent: i32| tree.nodes.iter().find(|n| n.parent == parent).map(|n| n.index as i32);
    let wings_node = first_child(0).and_then(first_child);
    // AtreeNodeInit's MB flags apply to the player's tree as to package
    // trees: flag 1 (a DUMMY node, or +0x2e bit 0) hides the node's own
    // object, flag 2 its subtree. The DUMMY node stays (the team moves'
    // attach point) but draws nothing: its object (e.g. WIZ_GREDUMMY, the
    // hand-glow texture GRE_HANDGLOW without texels, flags 0xac) had shown
    // as a magenta triangle at the Wizard's feet, turning with him
    // (playtest 2026-10-27).
    let mut hidden_subtree: std::collections::HashSet<i32> = std::collections::HashSet::new();
    for node in &tree.nodes {
        if tree_node_flags(node) & 2 != 0 || hidden_subtree.contains(&node.parent) {
            hidden_subtree.insert(node.index as i32);
        }
    }
    for node in &tree.nodes {
        let pose = if node.index == 0 { Some(0) } else { clip_nodes.iter().position(|c| c.label == node.label) };
        let Some(pose) = pose else { continue };
        let label: String = (tree.prefix.clone() + &node.label).chars().take(16).collect();
        let drawn = tree_node_flags(node) & 1 == 0 && !hidden_subtree.contains(&(node.index as i32));
        for def in character.objects.definitions.iter().filter(|d| drawn && d.label == label) {
            for (mesh, material) in parts.get(&(def.object_index as u32)).into_iter().flatten() {
                let child = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), RigNode(pose, slot))).id();
                if node.label == "HEAD" {
                    commands.entity(child).insert(HeadMesh);
                }
                if lhand.as_deref() == Some(node.label.as_str()) {
                    commands.entity(child).insert(LeftHandMesh(false));
                } else if lhand_parent.as_deref() == Some(node.label.as_str()) {
                    commands.entity(child).insert(LeftHandMesh(true));
                }
                commands.entity(body).add_child(child);
            }
        }
        // PlayerProcessPowerups' attached-object anchors (left hand, head,
        // right hand).
        let hand_at = if lhand.as_deref() == Some(node.label.as_str()) {
            Some(0)
        } else if node.label == "HEAD" {
            Some(1)
        } else if rhand.as_deref() == Some(node.label.as_str()) {
            Some(2)
        } else {
            None
        };
        if let Some(at) = hand_at {
            let anchor = commands.spawn((HandObject { slot, at, key: None }, RigNode(pose, slot), Transform::default(), Visibility::Hidden)).id();
            commands.entity(body).add_child(anchor);
        }
        // PlayerProcessPowerups' head and wings anchors.
        let at = if node.label == "HEAD" {
            Some(PowerupAt::Head)
        } else if Some(node.index as i32) == wings_node {
            Some(PowerupAt::Wings)
        } else {
            None
        };
        if let Some(at) = at {
            let anchor = commands.spawn((PowerupTree { slot, at, key: None }, RigNode(pose, slot), Transform::default(), Visibility::Hidden)).id();
            commands.entity(body).add_child(anchor);
        }
        if rhand.as_deref() == Some(node.label.as_str()) {
            let hold = commands.spawn((HoldFx { slot, key: None }, RigNode(pose, slot), Transform::default(), Visibility::Hidden)).id();
            commands.entity(body).add_child(hold);
            for (mesh, material) in &weapon {
                let child = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(material.clone()), Transform::default(), RigNode(pose, slot), HeldWeapon(slot))).id();
                // load_player: the Jester's (class & 7 == 7) held bomb gets
                // AddSpecialTexmod(model, "BOMB", SFX model, "BOMB00", 5, 1).
                if pc.key.class & 7 == 7 {
                    commands.entity(child).insert(SpecialTexmod { package: pc.sfx_name.clone(), base: "BOMB00".into(), frames: 5, rate: 1 });
                }
                commands.entity(body).add_child(child);
            }
        }
    }
    let familiar = commands.spawn((Familiar { slot, key: None }, Transform::default(), Visibility::Hidden)).id();
    commands.entity(body).add_child(familiar);
    let anchor = commands.spawn((PowerupTree { slot, at: PowerupAt::Node, key: None }, Transform::default(), Visibility::Hidden)).id();
    commands.entity(body).add_child(anchor);
    let sign = commands.spawn((ItSign { slot, spawned: false }, Transform::default(), Visibility::Hidden)).id();
    commands.entity(body).add_child(sign);
    // PlayerDoWeapTrail's eight copies of the held weapon (MBNewObject of
    // the weapon object at its world matrix, fading out).
    let trail = commands.spawn((WeaponTrail(slot), Transform::default(), Visibility::Inherited)).id();
    commands.entity(root).add_child(trail);
    for i in 0..8 {
        let copy = commands.spawn((TrailCopyNode(slot, i), Transform::default(), Visibility::Hidden)).id();
        commands.entity(trail).add_child(copy);
        for (mesh, material) in &weapon {
            let Some(mut m) = materials.get(material).cloned() else { continue };
            if matches!(m.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
                m.alpha_mode = AlphaMode::Blend;
            }
            let base_alpha = m.params.tint.w;
            let part = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(materials.add(m)), Transform::default(), TrailPart { slot, copy: i, base_alpha })).id();
            commands.entity(copy).add_child(part);
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
    (root, bodies, shadows, trails): (Query<Entity, With<MirrorRoot>>, Query<(Entity, &CharacterRoot)>, Query<(Entity, &PlayerShadow)>, Query<(Entity, &WeaponTrail)>),
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
        for (e, t) in &trails {
            if t.0 == k {
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
    let level_lib = spawn_level(&mut commands, root, &session, sim.game.items.records.len(), &mut meshes, &mut images, &mut materials);
    libraries.0.insert(LEVEL_LIB.to_string(), level_lib);
    spawn_bodies(&mut commands, root, &sources, &session, &mut meshes, &mut images, &mut materials);
    for (name, pkg) in &sources.packages {
        if !libraries.0.contains_key(name) {
            libraries.0.insert(name.clone(), build_library(pkg, &mut meshes, &mut images, &mut materials));
        }
        if sources.monster_packages.contains(name) && !frames.0.contains_key(name) {
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
) -> Library {
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
    let mut lm_variants: HashMap<AssetId<gdl_client::GsMaterial>, Handle<gdl_client::GsMaterial>> = HashMap::new();
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
                let key = (material.id(), flags & (0x800000 | 0x4000 | 0x1000 | 0x80 | 0x40));
                // A lightmapped part whose texture is not opaque draws its
                // lightmap as a pass of its own (GsDraw::lightmap_pass): the
                // base keeps the texture alone.
                let split = materials.get(material).is_some_and(|m| m.lightmap.is_some() && m.alpha_mode != AlphaMode::Opaque) && flags & 0x4000 == 0;
                let material = variants
                    .entry(key)
                    .or_insert_with(|| {
                        let mut m = materials.get(material).expect("part material").with_draw(flags);
                        if split {
                            m.params.options.y = 0.0;
                        }
                        materials.add(m)
                    })
                    .clone();
                let lm_pass = split
                    .then(|| {
                        lm_variants
                            .entry(material.id())
                            .or_insert_with(|| {
                                let mut base = materials.get(&material).cloned().expect("variant");
                                base.params.options.y = 1.0;
                                materials.add(base.lightmap_pass().expect("lightmapped"))
                            })
                            .clone()
                    });
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
                // The lightmap pass: the same geometry under the part (its
                // transform and visibility), drawn just after it.
                if let Some(lm) = lm_pass {
                    let pass = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(lm), Transform::IDENTITY, Visibility::Inherited)).id();
                    commands.entity(child).add_child(pass);
                }
            }
        }
    }
    spawn_world_psys(commands, root, session, &set, meshes);
    spawn_texmods(commands, &level.anim_bytes, &set);
    for index in 0..gdl_core::game::critters::MAX_INSTS {
        let e = commands.spawn((CritterSlot { index, shown: None, damaged: 0, posed: None }, Transform::default(), Visibility::Hidden, LevelEntity)).id();
        commands.entity(root).add_child(e);
    }
    for (kind, n) in [(SlotKind::Enemy, 25), (SlotKind::Item, item_count), (SlotKind::Effect, 64)] {
        for index in 0..n {
            let e = commands.spawn((Slot { kind, index, key: None }, Transform::default(), Visibility::Hidden, LevelEntity)).id();
            commands.entity(root).add_child(e);
        }
    }
    // The level's own objects as a library (LEVEL_LIB): ItemFindMBObjectL1
    // searches every loaded model table (E2's E2ROCKFALL#nn falling rocks).
    assemble_library(level, set, parts)
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
    /// The stepped-update count at the first run (GDL_FREEZE_AT clock).
    born: Option<u64>,
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
/// −2/−3 become texture shifts. Fades (−4/−5) occur on the disc only in
/// tree-owned records (owner ≥ 0: ITEMS/LEVELL, WEAPONS; drawn per node by
/// request_tree_texmods) and −6 in none, so the owner −1 list has no
/// such entries.
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
        commands.spawn((TexMods { mods, aframe: 0, accumulated: 0.0, born: None, by_image: None }, LevelEntity));
    }
}

/// DoTexModSub 0x110e10 per AFrame (30 Hz presentation clock): every
/// `divisor` frames the counter advances modulo |count|; frame modifiers
/// copy frame `counter` into the target (initially frame 0, InitTexMod),
/// scrolls set the shift add sign·(counter mod |count|)/|count|.
fn present_texmods(time: Res<Time>, sim: Res<Sim>, mut q: Query<&mut TexMods>, mut images: ResMut<Assets<Image>>, mut materials: ResMut<Assets<gdl_client::GsMaterial>>) {
    let dt = time.delta_secs();
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
        // Under GDL_FREEZE_AT one AFrame per update stepped since this
        // list's first run, whatever frame its spawn landed in, all of them
        // (render time drops the steps past four in one frame).
        let due = if sim.sim_clock {
            let born = *tm.born.get_or_insert(sim.updates);
            Some((sim.updates - born) as u32)
        } else {
            tm.accumulated += dt;
            None
        };
        // Only the modifiers that stepped are re-applied: every frame target
        // re-inserted (a texture upload each) and every scroll material
        // re-prepared on each step cost about 1.6 ms a frame (M20 profile).
        let mut dirty = vec![first; tm.mods.len()];
        let mut steps = 0;
        while match due {
            Some(n) => tm.aframe < n,
            None => tm.accumulated >= 1.0 / 30.0 && steps < 4,
        } {
            if due.is_none() {
                tm.accumulated -= 1.0 / 30.0;
            }
            steps += 1;
            tm.aframe = tm.aframe.wrapping_add(1);
            for (k, m) in tm.mods.iter_mut().enumerate() {
                if 0 < m.divisor && tm.aframe % m.divisor as u32 != 0 {
                    continue;
                }
                m.counter += 1;
                if i32::from(m.count).abs() <= m.counter {
                    m.counter = 0;
                }
                dirty[k] = true;
            }
        }
        if !dirty.contains(&true) {
            continue;
        }
        let by_image = tm.by_image.as_ref().unwrap();
        for (m, _) in tm.mods.iter().zip(&dirty).filter(|(_, d)| **d) {
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
                        let cur = materials.get(*id).map(|mat| if *v { mat.params.options.w } else { mat.params.options.z });
                        if cur.is_some_and(|c| c != shift)
                            && let Some(mat) = materials.get_mut(*id)
                        {
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

/// PlayerDoWeapTrail's copies (Game::weap_trail) at their world matrices,
/// faded by MBTreeSetAlpha's (255 − fade) / 255; the held weapon hidden while
/// DoPlayerAction holds its tree flag 2.
type TrailCopyQuery<'w, 's> = Query<'w, 's, (&'static TrailCopyNode, &'static mut Transform, &'static mut Visibility), (Without<HeldWeapon>, Without<TrailPart>)>;

fn present_weapon_trails(
    sim: Res<Sim>,
    mut copies: TrailCopyQuery,
    parts: Query<(&TrailPart, &MeshMaterial3d<gdl_client::GsMaterial>)>,
    mut held: Query<(&HeldWeapon, &mut Visibility), Without<TrailCopyNode>>,
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
) {
    for (node, mut t, mut vis) in &mut copies {
        match sim.game.weap_trail.get(node.0).and_then(|s| s[node.1]) {
            Some(c) => {
                let m = c.matrix;
                *t = Transform::from_matrix(Mat4::from_cols_array(&std::array::from_fn(|i| m[i / 4][i % 4])));
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for (part, handle) in &parts {
        let Some(c) = sim.game.weap_trail.get(part.slot).and_then(|s| s[part.copy]) else { continue };
        let a = part.base_alpha * (255 - c.fade.clamp(0, 255)) as f32 / 255.0;
        if let Some(m) = materials.get(&handle.0)
            && m.params.tint.w != a
            && let Some(m) = materials.get_mut(&handle.0)
        {
            m.params.tint.w = a;
        }
    }
    for (h, mut vis) in &mut held {
        // PlayerProcessPowerups hides the held weapon (+0x6b8,
        // MBTreeSetFlags 2) while the right hand holds an object (+0x708).
        let right = h.0 < 4 && !right_hand_frozen(&sim.game, h.0) && hand_objects(&sim.game, h.0)[2].is_some();
        let hide = right || sim.weapon_hidden.get(h.0).copied().unwrap_or(false);
        let want = if hide { Visibility::Hidden } else { Visibility::Inherited };
        if *vis != want {
            *vis = want;
        }
    }
}

/// The missile streaks (SfxSetStreak / UpdateFXStreak): one mesh of every
/// live effect's quad (WEAPONS' WEP_STREAK, vertex colour streak_color / 128,
/// alpha the poly's GS alpha / 128, blended without depth writes).
#[derive(Component)]
struct StreakMesh;

fn present_streaks(
    sim: Res<Sim>,
    libraries: Res<Libraries>,
    root: Query<Entity, With<MirrorRoot>>,
    mut existing: Query<(&Mesh3d, &mut Visibility), With<StreakMesh>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
    mut commands: Commands,
) {
    let g = &sim.game;
    let quads: Vec<gdl_core::game::effects::Streak> = (0..g.effects.num)
        .filter(|&k| 0.0 < g.effects.slots[k].f32_(0x68))
        .filter_map(|k| g.effects.nodes[k].and_then(|n| n.streak))
        .filter(|s| s.verts.iter().any(|v| *v != [0.0; 3]))
        .collect();
    let Some((mesh, mut vis)) = existing.iter_mut().next() else {
        let Some(tex) = libraries.0.get("WEAPONS").and_then(|l| l.textures.get("WEP_STREAK")).cloned() else { return };
        let Ok(parent) = root.single() else { return };
        let material = materials.add(gdl_client::GsMaterial {
            params: gdl_client::GsParams { tint: Vec4::ONE, options: Vec4::new(1.0, 0.0, 0.0, 0.0), ..default() },
            lit: false,
            base: Some(tex),
            lightmap: None,
            overlay: None,
            lights: gdl_client::point_light_buffer(),
            alpha_mode: AlphaMode::Blend,
            draw: gdl_client::GsDraw {
                additive: false,
                multiply: false,
                fixed_alpha: false,
                no_depth_write: true,
                depth_always: false,
                lightmap_pass: false,
                depth_only: false,
                sort_late: true,
                sort_first: false,
                cull: false,
            },
        });
        let mesh = meshes.add(Mesh::new(bevy::render::render_resource::PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::default()));
        let e = commands.spawn((StreakMesh, Mesh3d(mesh), MeshMaterial3d(material), Transform::default(), Visibility::Hidden, LevelEntity, bevy::camera::visibility::NoFrustumCulling)).id();
        commands.entity(parent).add_child(e);
        return;
    };
    if quads.is_empty() {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Inherited;
    let Some(m) = meshes.get_mut(&mesh.0) else { return };
    let n = quads.len();
    let mut positions = Vec::with_capacity(n * 4);
    let mut colors = Vec::with_capacity(n * 4);
    let mut uvs = Vec::with_capacity(n * 4);
    let mut indices = Vec::with_capacity(n * 6);
    for (k, q) in quads.iter().enumerate() {
        let c = q.colour;
        let rgba = [((c >> 16) & 0xff) as f32 / 128.0, ((c >> 8) & 0xff) as f32 / 128.0, (c & 0xff) as f32 / 128.0, f32::from(q.alpha) / 128.0];
        for (v, uv) in q.verts.iter().zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]) {
            positions.push(*v);
            uvs.push(uv);
            colors.push(rgba);
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
    mut seen: Local<u64>,
    mut clock: ResMut<PsysClock>,
    (libraries, session, sources): (Res<Libraries>, Res<Session>, Res<Sources>),
    cams: Query<&bevy::camera::primitives::Frustum, With<FollowCamera>>,
    mut emitters: EmitterQuery,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<gdl_client::GsMaterial>>,
    mut commands: Commands,
) {
    let Some(cam) = &sim.game.std_camera else { return };
    let rows = cam.pb.cam_rows.map(Vec3::from_array);
    let frustum = cams.iter().next();
    // Under GDL_FREEZE_AT one psys frame per update stepped since the last
    // run, all of them (render time drops the steps past four in one frame).
    let due = sim.sim_clock.then(|| sim.updates.saturating_sub(*seen));
    if due.is_none() {
        clock.accumulated += time.delta_secs();
    }
    *seen = sim.updates;
    let mut steps = 0;
    while match due {
        Some(n) => steps < n,
        None => clock.accumulated >= 1.0 / 30.0 && steps < 4,
    } {
        if due.is_none() {
            clock.accumulated -= 1.0 / 30.0;
        }
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
    if due.is_none() && 4 <= steps {
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
            let order = search_order(&session, &sources);
            let named = ["STATIC", "WEAPONS", "POWERUPS"]
                .iter()
                .find_map(|p| libraries.0.get(*p).and_then(|l| l.textures.get(&name)).cloned())
                .or_else(|| em.texture.clone())
                .or_else(|| libraries.ordered(&order).find_map(|l| l.textures.get(&name).cloned()));
            if std::env::var_os("GDL_DEBUG_FX").is_some() {
                let holders: Vec<&String> = order.iter().filter(|n| libraries.0.get(*n).is_some_and(|l| l.textures.contains_key(&name))).collect();
                eprintln!("psys texture {name}: own {} in {holders:?}", em.texture.is_some());
            }
            if named.is_none() && !libraries.0.contains_key("STATIC") {
                continue;
            }
            let blend = em.psys.blend();
            let (no_depth_write, depth_always) = em.psys.depth_flags();
            let m = gdl_client::GsMaterial {
                params: gdl_client::GsParams {
                    tint: Vec4::ONE,
                    options: Vec4::new(if named.is_some() { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0),
                    extra: Vec4::new(0.0, 0.0, 0.0, gdl_client::frame_writes()),
                    ..default()
                },
                lit: false,
                base: named,
                lightmap: None,
                overlay: None,
                lights: gdl_client::point_light_buffer(),
                alpha_mode: if blend == gdl_core::psys::Blend::Additive { AlphaMode::Add } else { AlphaMode::Blend },
                draw: gdl_client::GsDraw {
                    additive: blend == gdl_core::psys::Blend::Additive,
                    multiply: blend == gdl_core::psys::Blend::Multiply,
                    fixed_alpha: em.psys.fixed_alpha(),
                    no_depth_write,
                    depth_always,
                    lightmap_pass: false,
                    depth_only: false,
                    sort_late: false,
                    sort_first: false,
                    cull: false,
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
    start["globals"]["transmitter_table"] = serde_json::to_value(&session.transmitter_table)?;
    start["globals"]["check_num"] = Value::from(session.check_num);
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
    for (t, _, prefix, path) in session.monster_paths.iter().filter(|m| lv.monster_types.iter().any(|(t, n, _)| *t == m.0 && *n == m.1)) {
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
    // powerup_anim, then item_anim2 (the level's own). Generators then try
    // the enemy packages (setup::add_generator_trees: MONSTERS/WAR's GEN_WAR
    // trees for J3 and J4's generators).
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
        // InitEffects' item_anim (the realm's) or item_anim2 (the level's).
        effect_tables.set_item_role(usize::from(lv.items.get(n) != Some(&realm)), n as u8);
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
    // OptionsSetup's AudioSetVolMusic (GamePrefs word 1).
    game.audio_set_vol_music(session.prefs[1]);
    {
        // EnterTower: SetPlayerStartPos(GetWorldOrder(LastWorldPlayed)) puts
        // a returning player at that realm's wing (WorldOrder 0x23f4c8);
        // SetPlayerStartPos 0x150858 falls back to start 0 when
        // WorldOpen 0x177778(WorldOrder[id]) is false.
        let mut id = 0;
        if lv.world == 0xd
            && let Ok(order) = elf.object("WorldOrder")
        {
            let order: Vec<i32> = order.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect();
            id = order.iter().position(|&w| w == session.last_world).unwrap_or(0) as i16;
            if let Some(t) = session.tower_start {
                id = t;
            }
            if !order.get(id.max(0) as usize).is_some_and(|&w| game.world_open(w)) {
                id = 0;
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
/// as the select screen's card — file k is store slot k + 1, the original's
/// BASLUS-20047save%04d (getSaveFileName 0x107a80: slot + 1).
struct NativeCard<'a> {
    store: &'a gdl_core::save::SaveStore,
}

impl gdl_core::game::pselect::SaveCard for NativeCard<'_> {
    fn directory(&self) -> [Option<gdl_core::game::pselect::CardFile>; 8] {
        std::array::from_fn(|k| {
            let region = self.store.load(k as u32 + 1).ok()?.save.region().ok()?;
            let name = String::from_utf8_lossy(&region[..region.iter().take(8).position(|&c| c == 0).unwrap_or(8)]).into_owned();
            let w = |o: usize| i32::from(i16::from_le_bytes([region[o], region[o + 1]]));
            // +0x9a4 class and +0x9aa level sum (region offsets 8 / 0xe).
            Some(gdl_core::game::pselect::CardFile { name, levels: w(0xe), class: w(8) })
        })
    }
    fn load(&mut self, k: usize) -> Option<Vec<u8>> {
        self.store.load(k as u32 + 1).ok()?.save.region().ok()
    }
    fn save(&mut self, k: usize, region: &[u8]) -> bool {
        self.store.store(k as u32 + 1, "", region).is_ok()
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
        let elf = gdl_formats::elf::Elf::parse(&front.elf_bytes)?;
        let label = gdl_core::game::teammove::player_rhand_label(&elf, r.u32_(0xc).min(16));
        let rhand = gdl_core::game::teammove::tree_node_by_label(&pc.clips, label.as_deref());
        let rig = std::sync::Arc::new(gdl_core::game::teammove::PlayerRig { anim: Animations::parse(&pc.clip_bytes)?, bytes: pc.clip_bytes.clone(), dummy, rhand });
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
fn play_audio(audio: &mut Option<NonSendMut<gdl_client::audio::AudioOut>>, events: &[Event]) {
    let Some(a) = audio.as_mut() else { return };
    for e in events {
        match e {
            // The simulation's pan words (AudioAng or fixed pans).
            Event::Sound { id, volume, priority, pan, .. } => a.play(*id, *volume, *pan, *priority),
            Event::LoopSound { id, volume, priority, pan, .. } => a.play_loop(*id, *volume, *pan, *priority),
            Event::StopSound { id } => a.stop(*id),
            Event::Music { name, volume, looping } => {
                // A missing part counts as playing (BGPlayStream's error
                // path keeps the loop flag set).
                a.stream(name, *looping, *volume);
            }
            Event::ItemSound { slot, id, volume, priority, pan, .. } => a.play_tagged(*slot as u32, *id, *volume, *pan, *priority),
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
    ui: Res<Ui>,
    mut audio: Option<NonSendMut<gdl_client::audio::AudioOut>>,
    front: Res<Front>,
    time: Res<Time>,
    (mut harness, mut exit): (ResMut<TestHarness>, MessageWriter<AppExit>),
    (bindings, overlay): (Res<gdl_client::controls::Bindings>, Res<gdl_client::enhance::Overlay>),
) {
    let closing = front.ending.active();
    if sim.error.is_some() || session.banner.is_some() || session.finished.is_some() || !ui.started {
        return;
    }
    let mut pads: Vec<(Entity, &Gamepad)> = gamepads.iter().collect();
    pads.sort_by_key(|(e, _)| *e);
    let pads: Vec<&Gamepad> = pads.into_iter().map(|(_, g)| g).collect();
    let mut ports = [PadStatus::default(); 8];
    for (k, port) in ports.iter_mut().take(4).enumerate() {
        *port = player_input(k, &keys, &pads, &bindings, !overlay.open && !overlay.held);
        // Only ports with a device are connected (keyboard: ports 0 and 1).
        port.connected = k < 2 || pads.get(k).is_some();
    }
    // Only while the level plays: the select screen, shop, map and movies
    // keep the keyboard (they read the same PlayerControl ports).
    let in_play = sim.select.is_none() && sim.shop.is_none() && sim.map.is_none() && sim.movie.is_none() && sim.message_box.is_none();
    if in_play && !harness.actions.is_empty() {
        let now = sim.updates;
        let due: Vec<serde_json::Value> = harness.actions.iter().filter(|a| a["at"].as_u64() == Some(now)).cloned().collect();
        for a in due {
            if let Some(v) = a["teleport"].as_array() {
                let v: Vec<f32> = v.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
                let yaw = sim.game.players[0].f32_(0x86c);
                if let (3, Err(e)) = (v.len(), sim.game.place_player(0, [v[0], v[1], v[2]], yaw)) {
                    sim.error = Some(format!("TEST teleport: {}", e.0));
                }
            }
            if let Some(h) = a["health"].as_f64() {
                sim.game.players[0].set_f32(0x1a10, h as f32);
            }
            if let Some(gold) = a["gold"].as_i64() {
                sim.game.players[0].set_i32(0x1a20, gold as i32);
            }
            // {"boss_kill": true}: DEBUG lethal CritterDamage on the boss
            // (Game::debug_damage_boss, as game_run's boss-kill); the death,
            // good wizard, auto exit and what follows are the game's own.
            if a["boss_kill"].as_bool() == Some(true) && sim.game.globals.boss_is_dead == 0
                && let Err(e) = sim.game.debug_damage_boss()
            {
                sim.error = Some(format!("TEST boss_kill: {}", e.0));
            }
            if let Some(port) = a["disconnect"].as_u64() {
                harness.disconnect = Some((port as usize, a["reconnect_after"].as_u64().unwrap_or(120)));
                harness.lost_ticks = 0;
            }
            if let Some(k) = a["save"].as_u64() {
                // PlayerWriteSaveFile(0, k) as the select screen does it:
                // player_store_in_save, the region to card file k, saved flag.
                use gdl_core::game::pselect::SaveCard;
                let controls = sim.game.control_options[0];
                match profile::player_store_in_save(&mut sim.game.players[0], &controls, &front.pdats, &front.tables) {
                    Ok(Some(c)) => sim.game.control_options[0] = c,
                    Ok(None) => {}
                    Err(e) => sim.error = Some(format!("TEST save: {e}")),
                }
                let region = sim.game.players[0].0[profile::SAVE_START..profile::SAVE_START + profile::SAVE_LEN].to_vec();
                if (NativeCard { store: &front.store }).save(k as usize, &region) {
                    sim.game.players[0].set_u8(0x9a7, 1);
                } else {
                    sim.error = Some("TEST save failed".into());
                }
            }
            if let (Some(label), Some(out)) = (a["dump"].as_str(), harness.state_out.as_ref()) {
                let mut d = progress_dump(&sim.game, label, now);
                d["controller_lost"] = serde_json::Value::from(sim.game.controls.controller_lost);
                // Every player: state +0xfc, health, name (+0x99c), class, the
                // card file it owns (+0x2af4) and its port.
                d["players"] = serde_json::json!((0..4).map(|k| { let r = &sim.game.players[k]; serde_json::json!({
                    "state": r.i32_(0xfc), "health": r.f32_(0x1a10), "class": r.u32_(0xc),
                    "name": String::from_utf8_lossy(&r.0[0x99c..0x9a4]).trim_end_matches('\0').to_string(),
                    "file": r.i32_(0x2af4), "port": sim.game.controls.joy_remap[k] }) }).collect::<Vec<_>>());
                d["joy_remap"] = serde_json::json!(sim.game.controls.joy_remap);
                // The client's status line (e.g. "save recovered from …").
                d["last_event"] = serde_json::Value::from(sim.last_event.clone());
                let path = out.with_extension(format!("{label}.json"));
                let _ = std::fs::write(path, serde_json::to_vec_pretty(&d).unwrap_or_default());
            }
        }
    }
    if !in_play && let Some(script) = harness.menu_script.as_ref() {
        let n = harness.menu_ticks;
        let mut p = PadStatus { connected: true, ..Default::default() };
        for (first, last, entries) in script {
            if (*first..=*last).contains(&n) {
                for &i in entries.iter().filter(|&&i| i < 0x16) {
                    p.press(i);
                }
            }
        }
        ports[0] = p;
        harness.menu_ticks += 1;
    }
    if let Some(script) = harness.script.as_ref().filter(|_| in_play) {
        let n = sim.updates;
        let mut p = PadStatus { connected: true, ..Default::default() };
        for (first, last, entries) in script {
            if (*first..=*last).contains(&n) {
                for &i in entries.iter().filter(|&&i| i < 0x16) {
                    p.press(i);
                }
            }
        }
        ports[0] = p;
    }
    // TEST: a port without its device, then back with START held.
    if let Some((port, after)) = harness.disconnect
        && let Some(pd) = ports.get_mut(port)
    {
        // Ticks count while the message is up, then until START is released.
        if sim.game.controls.controller_lost & 0xf != 0 || after <= harness.lost_ticks {
            harness.lost_ticks += 1;
        }
        if harness.lost_ticks < after {
            pd.connected = false;
        } else if harness.lost_ticks < after + 14 {
            if after + 10 <= harness.lost_ticks {
                pd.press(0x14);
            }
        } else {
            harness.disconnect = None;
        }
    }
    // DoOptions → ControllerMessageBox 0x128668 while a player's controller
    // is lost: the game waits; ControlsUpdate keeps polling until START on
    // an unassigned port claims the player (CONTROLLERMISSING from
    // ENGLISH.ROM; the box layout is the scroll box's, not the original's).
    if sim.game.controls.controller_lost & 0xf != 0 || sim.controller_box {
        if let Err(e) = sim.game.update_controls(&ports) {
            sim.error = Some(format!("controls: {}", e.0));
        }
        let lost = sim.game.controls.controller_lost & 0xf;
        if lost != 0 && !sim.controller_box {
            if let Some(e) = front.english.entry("CONTROLLERMISSING") {
                let k = lost.trailing_zeros() + 1;
                let text = e.lines.iter().map(|l| l.replace("%d", &k.to_string())).collect::<Vec<_>>().join("\n");
                let mut sb = scroll_box(&sim.game, &front.english, e, vec![text], 0);
                sb.no_prompt = true;
                sim.message_box = Some(sb);
            }
            sim.controller_box = true;
            sim.last_event = format!("controller removed (player {})", lost.trailing_zeros() + 1);
        } else if lost == 0 && sim.controller_box {
            sim.message_box = None;
            sim.controller_box = false;
            sim.last_event = "controller reassigned".into();
        }
        if lost != 0 || harness.disconnect.is_some() {
            return;
        }
    }
    // NATIVE recovery notice (M18): a slot loaded from its backup or a
    // leftover .tmp is reported once in a message box (the original has no
    // such message; its card code never sees a damaged native file).
    if sim.message_box.is_none() {
        let notes = front.store.take_recoveries();
        if !notes.is_empty()
            && let Some(e) = front.scrolls.entries.first()
        {
            let pages: Vec<String> = notes
                .iter()
                .map(|(slot, used, _)| {
                    let what = if used.extension().is_some_and(|x| x == "tmp") { "an interrupted save" } else { "its backup copy" };
                    format!("NOTICE (native)\nThe saved character in\nslot {slot} was damaged.\nIt was restored from\n{what}.")
                })
                .collect();
            sim.last_event = format!("save recovered: {}", notes.iter().map(|(s, p, why)| format!("slot {s} from {} ({why})", p.display())).collect::<Vec<_>>().join("; "));
            let sb = scroll_box(&sim.game, &front.scrolls, e, pages, 1);
            sim.game.scroll_box_open();
            sim.message_box = Some(sb);
        }
    }
    // game_main / ScrollMessageBox: CheckSoftReset (front_system ends the
    // game on RestartReq).
    if sim.game.globals.game_mode & 0x4000 != 0 || sim.message_box.is_some() {
        sim.game.check_soft_reset();
    }
    // main: ScreenSaver before DoOptions (not inside ScrollMessageBox's
    // loop); while it runs the game halts in its own loop.
    if sim.message_box.is_none() {
        let vb = sim.game.globals.vb_elapsed;
        let r = if sim.game.saver.active {
            sim.game.screen_saver_frame(vb, &ports).map(|_| true)
        } else {
            sim.game.screen_saver_check(vb)
        };
        match r {
            Ok(true) => {
                // AudioStopAll / AudioStreamStop on the host's voices.
                if sim.game.saver.active && let Some(a) = audio.as_mut() && a.stream_playing() {
                    a.stop_all();
                }
                let new: Vec<Event> = sim.game.events.drain(..).collect();
                play_audio(&mut audio, &new);
                return;
            }
            Ok(false) => {}
            Err(e) => sim.error = Some(format!("screen saver: {}", e.0)),
        }
    }
    if ui.paused || closing || sim.message_box.is_some() || overlay.open {
        // main keeps running AudioQueUpdate / AudioPoll (DoOptions,
        // ScrollMessageBox): queued lines go on while the game waits (also
        // under the native settings overlay).
        let vb = sim.game.globals.vb_elapsed;
        if ui.paused && gdl_client::menu::page_record(&front.menu.page).is_some() {
            // DoOptions with a menu up: LowerBGMusic(0.1, −1) (the music
            // fades out), in the audio menu LowerBGMusic(0, 1).
            if matches!(front.menu.page, gdl_client::menu::Page::Audio { .. }) {
                sim.game.lower_bg_music(0.0, 1.0);
            } else {
                sim.game.lower_bg_music(0.1, -1.0);
            }
        }
        // game_main runs BGMusicUpdate while paused (mode 0x4010);
        // ScrollMessageBox runs it in its loop.
        if sim.message_box.is_some() || sim.game.globals.game_mode == 0x4010 {
            sim.game.bg_music_update();
        }
        sim.game.main_audio(vb);
        if overlay.open {
            // The native settings overlay: the game waits.
        } else if !ui.paused && !closing {
            step_message_box(&mut sim, &ports, time.delta_secs());
        } else if let Err(e) = sim.game.update_controls(&ports) {
            // DoOptions reads PlayerControl each frame, so the press that
            // closes a menu is an old level when the game resumes.
            sim.error = Some(format!("controls: {}", e.0));
        } else if front.ending.burn.is_some() {
            // ServeFireScroll: ClearAllPlayerControls(2) per frame.
            let _ = sim.game.controls.clear_all_player_controls(2);
        }
        let new: Vec<Event> = sim.game.events.drain(..).collect();
        play_audio(&mut audio, &new);
        return;
    }
    // PlayStreamComplete: a stage music part played once has ended.
    if sim.game.bg_music.stream_playing && audio.as_ref().is_some_and(|a| !a.stream_playing()) {
        sim.game.bg_stream_complete();
    }
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
        play_audio(&mut audio, &new);
        return;
    }
    // game mode 0x400e: do_gamemovie (controls polled; the game waits).
    if let Some(mut run) = sim.movie.take() {
        let vb = sim.game.globals.vb_elapsed;
        sim.game.main_audio(vb);
        if let Err(e) = sim.game.update_controls(&ports) {
            sim.error = Some(format!("controls: {}", e.0));
        }
        // game_main: SetPlayerVars in every mode, before the dispatch.
        sim.game.set_player_vars();
        let serving = !run.tried || run.player.as_ref().is_some_and(|p| p.playing());
        match sim.game.do_gamemovie(&mut run.game, serving) {
            Some(_) => sim.movie_done = Some(run.then),
            None => sim.movie = Some(run),
        }
        return;
    }
    // game mode 0x400f: SetPlayerVars, do_mapscreen (WritePlayerInfo
    // first) until the next level loads, else do_players.
    if let Some((mut m, next)) = sim.map.take() {
        let g = &mut sim.game;
        g.hud.begin_frame();
        g.main_audio(g.globals.vb_elapsed);
        g.set_player_vars();
        if g.do_mapscreen(&mut m) {
            sim.map_done = Some(next);
        } else {
            if let Err(e) = g.do_players() {
                sim.error = Some(format!("do_players: {}", e.0));
            }
            sim.map = Some((m, next));
        }
        let new: Vec<Event> = sim.game.events.drain(..).collect();
        play_audio(&mut audio, &new);
        return;
    }
    // game mode 0x4012: game_main's SetPlayerVars (the panels' written
    // bits), do_shop (WritePlayerInfo first), and do_players while the shop
    // runs.
    if let Some((mut flow, tower)) = sim.shop.take() {
        let g = &mut sim.game;
        g.hud.begin_frame();
        g.main_audio(g.globals.vb_elapsed);
        g.set_player_vars();
        let r = g.update_controls(&ports).and_then(|_| g.do_shop(&mut flow, &front.shop)).and_then(|r| if r == 0 { g.do_players().map(|_| 0) } else { Ok(r) });
        match r {
            Ok(0) => sim.shop = Some((flow, tower)),
            Ok(_) => sim.shop_done = Some((tower, flow.skip)),
            Err(e) => sim.error = Some(format!("do_shop: {}", e.0)),
        }
        let new: Vec<Event> = sim.game.events.drain(..).collect();
        play_audio(&mut audio, &new);
        return;
    }
    // game mode 0x400b: the player-select screen (do_player_select).
    if let Some(mut ps) = sim.select.take() {
        let typed = typed_letter(&keys);
        let g = &mut sim.game;
        g.hud.begin_frame();
        g.main_audio(g.globals.vb_elapsed);
        let mut card = NativeCard { store: &front.store };
        // game_main (0x400b): SetPlayerVars, do_players unless paused, then
        // do_player_select (its WritePlayerInfo gate, 0x20f974 bit 0, is
        // unnamed; do_players writes the panels in this mode).
        g.set_player_vars();
        let r = g.update_controls(&ports).and_then(|_| if g.globals.paused == 0 { g.do_players().map(|_| ()) } else { Ok(()) }).and_then(|_| {
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
        play_audio(&mut audio, &new);
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
    sim.sim_clock = harness.freeze_at.is_some();
    if harness.freeze_at.is_some_and(|n| n <= sim.updates) {
        sim.frozen = true;
        return;
    }
    let Sim { game, .. } = &mut *sim;
    let before = game.events.len();
    game.step_with(None, &mut |g| {
        g.update_controls(&ports)?;
        g.do_players().map(|_| ())
    });
    sim.updates += 1;
    // DIAGNOSTIC (M20 performance report): the time from process start to
    // the first stepped update (loading), on stderr.
    if sim.updates == 1 && std::env::var_os("GDL_FRAME_LOG").is_some() {
        eprintln!("PERF load_to_first_update_ms {:.1}", process_start().elapsed().as_secs_f64() * 1000.0);
    }
    if harness.exit_after.is_some() {
        let h = sim_state_hash(&sim.game);
        harness.hashes.push(h);
        if harness.exit_after == Some(sim.updates) {
            if let Some(out) = harness.state_out.as_ref() {
                let _ = std::fs::write(out, harness.hashes.join("\n") + "\n");
            }
            // DIAGNOSTIC (M20): the peak resident memory (Linux VmHWM).
            if let Some(line) = std::fs::read_to_string("/proc/self/status").ok().and_then(|s| s.lines().find(|l| l.starts_with("VmHWM")).map(str::to_string)) {
                eprintln!("PERF {}", line.split_whitespace().collect::<Vec<_>>().join(" "));
            }
            exit.write(AppExit::Success);
        }
    }
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
    // DEBUG: GDL_DEBUG_POWERUP=<kind>:<flags hex>:<seconds> puts that powerup
    // in player 1's second slot (+0x154) once at update 600, e.g. 9:2:60 for
    // the X-ray view (+0x138 2). Not a game feature.
    if sim.updates == 600
        && let Some(v) = std::env::var("GDL_DEBUG_POWERUP").ok()
        && let [k, f, t] = v.split(':').collect::<Vec<_>>()[..]
        && let (Ok(k), Ok(f), Ok(t)) = (k.parse::<i32>(), u32::from_str_radix(f, 16), t.parse::<f32>())
    {
        let r = &mut sim.game.players[0];
        r.set_f32(0x154, t);
        r.set_i32(0x158, k);
        r.set_f32(0x15c, 0.0);
        r.set_u32(0x160, f);
    }
    // DEBUG: GDL_DEBUG_SAVER=<vblanks> starts the screen saver's count at
    // 36000 − vblanks once at update 600 (the original's AuxLevels
    // 0x100000000 starts it at once). Not a game feature.
    if sim.updates == 600
        && let Some(n) = std::env::var("GDL_DEBUG_SAVER").ok().and_then(|v| v.parse::<i32>().ok())
    {
        sim.game.saver.timer = 36000 - n;
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
    play_audio(&mut audio, &new);
    for e in new {
        match e {
            Event::Exp { amount, .. } => sim.exp += i64::from(amount),
            Event::LevelUp { level, .. } => sim.last_event = format!("LEVEL UP! now level {level}"),
            Event::GeneratorHit { destroyed: true, .. } => sim.last_event = "generator destroyed".into(),
            Event::GotIt { kind, amount, .. } => sim.last_event = format!("picked up (kind {kind}, amount {amount})"),
            Event::PlayerDied { .. } => sim.last_event = "YOU DIED".into(),
            Event::PlayerTreeFlags { player, flags, set } if flags & 2 != 0 && player < 4 => sim.weapon_hidden[player] = set,
            Event::PlayerInactivated { player } => {
                // inactivate_player's clear_player(k, 1) with every PDAT.
                let demo = sim.game.globals.demo_mode != 0;
                if let Err(e) = profile::clear_player(&mut sim.game.players[player], true, demo, &session.pdats) {
                    sim.error = Some(format!("clear_player: {e}"));
                }
            }
            // NewHelp draws its own box (Game::new_help / DoHelp).
            Event::HintMenu { .. } => sim.hint_menu = true,
            Event::ScrollMessage { player_mask, text } => {
                // A scroll item: line `text` of SCROLLS<level code>
                // (SetScrollLevelMsgList(level_data + 8)).
                let code = format!("SCROLLS{}", session.levels[session.current].name);
                if let Some(e) = front.scrolls.entry(&code)
                    && let Some(line) = usize::try_from(text).ok().and_then(|k| e.lines.get(k))
                {
                    let sb = scroll_box(&sim.game, &front.scrolls, e, vec![line.replace('\r', "")], player_mask);
                    sim.game.scroll_box_open();
                    sim.message_box = Some(sb);
                }
            }
            Event::ScrollNamed { player_mask, name, line, .. } => {
                // FindScrollMessage(name): SCROLL_E.ROM entries are upper case.
                if let Some(e) = front.scrolls.entry(&name.to_uppercase()) {
                    let pages: Vec<String> = match line {
                        Some(k) => e.lines.get(k).into_iter().cloned().collect(),
                        None => e.lines.clone(),
                    };
                    let pages: Vec<String> = pages.into_iter().map(|p| p.replace('\r', "")).collect();
                    if !pages.is_empty() {
                        let sb = scroll_box(&sim.game, &front.scrolls, e, pages, player_mask);
                        sim.game.scroll_box_open();
                        sim.message_box = Some(sb);
                    }
                }
            }
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
/// The packages searched for an item's tree: the level's item packages and
/// POWERUPS (AtreeMatchAnyHeader's order), then the other loaded packages in
/// name order (statue trees are in the critter and monster packages).
fn item_tree_packages<'a>(sources: &'a Sources, items: &'a [String]) -> Vec<&'a str> {
    let mut out: Vec<&str> = items.iter().map(String::as_str).chain(["POWERUPS"]).collect();
    let mut rest: Vec<&str> = sources.packages.keys().map(String::as_str).filter(|k| !out.contains(k)).collect();
    rest.sort_unstable();
    out.extend(rest);
    out
}

/// The current level's object library key (not a configured package).
const LEVEL_LIB: &str = "#LEVEL";

fn item_parts(sources: &Sources, libs: &Libraries, items: &[String], name: &str) -> Model {
    let own = items.len() + 1;
    // Static models: the item packages and POWERUPS, then the level's own
    // table (HYPOTHESIS: the search order across tables; names rarely recur).
    let level = libs.0.get(LEVEL_LIB);
    for (i, pkg) in item_tree_packages(sources, items).into_iter().enumerate() {
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
        // Static models by label only in the item packages and POWERUPS.
        if own <= i {
            continue;
        }
        // ItemFindMBObjectL1: the name, then +"L1", then +"L1ROOT".
        for label in [name.to_string(), format!("{name}L1"), format!("{name}L1ROOT")] {
            if let Some(parts) = lib.by_label.get(&label) {
                return vec![(Vec3::ZERO, parts.clone(), 0, None, None)];
            }
        }
    }
    if let Some(lib) = level
        && !name.is_empty()
    {
        for label in [name.to_string(), format!("{name}L1"), format!("{name}L1ROOT")] {
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
            let draw = flags & (0x800000 | 0x4000 | 0x1000 | 0x80 | 0x40);
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
                let Some((pkg, prefix)) = sources.monsters.get(&t) else {
                    *vis = Visibility::Hidden;
                    continue;
                };
                let tree = setup::enemy_desc(prefix, level);
                let seq = anim.clock.sequence as usize;
                // DoObjAnimation: frame (int)(f + 0.5) inside [start, start
                // + count − 1] → base + frame − start; a one-frame entry
                // always shows its object. (No enemy package sequence sets
                // the reverse flag +0x2a bit 0; object_frame.)
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
                // +0xe1 ≠ 0: hidden for this player count (show_item; ProcessItems
                // sets the tree's hide flag 2 whenever +0xe1 becomes 1 and
                // clears it when +0xe1 returns to 0).
                // SetItem's geometry (ItemWorld::geo): not for kinds 4 (statues
                // aside: GAR_STATUE, GOL_STATUE, DEATHSTATUE1/2) and 13, kind 12
                // only for subtype 2, never with the instance's flag bit 2.
                // Triggers (kind 5) draw their tree (realm switches TRIGGER2)
                // or model (I5's BRIDGEPADL1) when one exists.
                let geo = g.items.geo.get(slot.index).copied().unwrap_or(false);
                let statue = (kind == 4 && geo).then(|| g.items.statue_tree.get(slot.index).copied().flatten()).flatten();
                if !r.active() || r.u16_(0xd8) & 0x8900 != 0 || r.u8_(0xe1) != 0 || !geo {
                    *vis = Visibility::Hidden;
                    continue;
                }
                let m = r.matrix();
                let tf = record_transform([Vec3::from_array(m[0]), Vec3::from_array(m[1]), Vec3::from_array(m[2])], Vec3::from_array(r.position()));
                if kind == 3 {
                    let level = r.u8_(0xf6);
                    let t = r.i16_(0xf0) as i32;
                    let Some((pkg, prefix)) = sources.monsters.get(&t) else {
                        *vis = Visibility::Hidden;
                        continue;
                    };
                    let key = format!("GEN_{prefix}{level}");
                    // ItemFindMBObjectL1: the name, then +"L1", then +"L1ROOT".
                    let parts = libs.0.get(pkg).and_then(|lib| [key.clone(), format!("{key}L1"), format!("{key}L1ROOT")].iter().find_map(|l| lib.by_label.get(l)).cloned()).map(|p| vec![(Vec3::ZERO, p, 0, None, None)]);
                    (Some(key), parts, tf)
                } else {
                    // ReplaceItemAtree (Game::item_trees) overrides the
                    // definition's tree.
                    // Rocks (kind 10 type 0x29): SetItem's "<name><+0xf2>" (the
                    // damage stage; SetSafeRock restages it, −1 hides the tree).
                    let rock_stage = (kind == 10 && r.i16_(0xf0) == 0x29).then(|| r.i16_(0xf2));
                    if rock_stage.is_some_and(|s| s < 0) {
                        *vis = Visibility::Hidden;
                        continue;
                    }
                    let name = match (statue, rock_stage) {
                        (Some(s), _) => s.to_string(),
                        (None, Some(s)) => format!("{}{s}", String::from_utf8_lossy(d.name())),
                        // ReplaceItemAtree's tree; the definition's tree (the
                        // slot has a clock); else SetItem's model name, the
                        // instance's own (+8) or the definition's.
                        (None, None) => {
                            let has_tree = g.item_anims.get(slot.index).is_some_and(|a| a.is_some());
                            g.item_trees
                                .get(&slot.index)
                                .cloned()
                                .or_else(|| if has_tree { None } else { g.items.geo_name.get(slot.index).cloned().flatten() })
                                .unwrap_or_else(|| String::from_utf8_lossy(d.name()).into_owned())
                        }
                    };
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
                            n if 0xc0 <= n => sources.monsters.get(&i32::from(n - 0xc0)).map(|m| m.0.clone()),
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
                            commands.entity(entity).insert(EffectTree { pkg: pkg.clone(), tree: head.tree, start: node.start, timing: head.timing, slot: slot.index, looping: false, seq: 0, fixed: None });
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
                        let Some((pkg, prefix)) = sources.monsters.get(&t) else {
                            *vis = Visibility::Hidden;
                            continue;
                        };
                        let label = format!("{prefix}_{}", g.effects.tables.enemy_missile_desc[kind as usize]);
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

/// Player `.0`'s X-ray chest view (Game::see_thru): the contents' icon tree
/// at 0.65 on a node at the chest's matrix (do_see_thru's AtreeInit under
/// seethru_node).
#[derive(Component)]
struct SeeThruView(usize, Option<String>);

/// InitItemInfoData's "SEETHRU" object (POWERUPS, texture XRAY_A, MB flags
/// 0x4200000: camera-facing; node Z modifier −800) for player `.0`. The
/// reference (xray-ref-v1, a labelled powerup write beside G1's chest 43)
/// shows its rounded window cut through the chest's front, the contents'
/// icon and the ground behind visible: drawn after the icon and before the
/// chest, it writes only depth, nearer than the chest. Here: depth only,
/// sorted first among translucent draws, offset towards the camera by the
/// chest definition's radius (+0xc; HYPOTHESIS for the Z modifier, whose
/// VU1 use is not read), with the chest's parts sorted after it.
#[derive(Component)]
struct SeeThruFrame(usize);

/// A chest part drawn through an X-ray window: its own material.
#[derive(Component)]
struct ThroughChest(Handle<gdl_client::GsMaterial>);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn present_see_thru(
    mut commands: Commands,
    sim: Res<Sim>,
    sources: Res<Sources>,
    session: Res<Session>,
    libs: Res<Libraries>,
    root: Query<Entity, With<MirrorRoot>>,
    mut views: Query<(Entity, &mut SeeThruView, &mut Transform, &mut Visibility), Without<SeeThruFrame>>,
    mut frames: Query<(&SeeThruFrame, &mut CameraFace, &mut Transform, &mut Visibility), Without<SeeThruView>>,
    (slots, children, mut parts, through): (
        Query<(Entity, &Slot)>,
        Query<&Children>,
        Query<&mut MeshMaterial3d<gdl_client::GsMaterial>>,
        Query<(Entity, &ThroughChest)>,
    ),
    (mut materials, mut variants, mut pending): (ResMut<Assets<gdl_client::GsMaterial>>, ResMut<PartVariants>, ResMut<PendingTreePsys>),
) {
    let Ok(root) = root.single() else { return };
    let g = &sim.game;
    if views.is_empty() {
        let frame_parts = libs.0.get("POWERUPS").and_then(|lib| lib.by_label.get("SEETHRU")).cloned().unwrap_or_default();
        for p in 0..4 {
            let v = commands.spawn((SeeThruView(p, None), Transform::default(), Visibility::Hidden, LevelEntity)).id();
            commands.entity(root).add_child(v);
            let f = commands.spawn((SeeThruFrame(p), CameraFace { mode: 0x400_0000, anchor: Vec3::ZERO, offset: Vec3::ZERO }, Transform::default(), Visibility::Hidden, LevelEntity)).id();
            commands.entity(root).add_child(f);
            for (mesh, material) in &frame_parts {
                let Some(mut m) = materials.get(material).cloned() else { continue };
                m.draw.depth_only = true;
                m.alpha_mode = AlphaMode::Blend;
                let child = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(materials.add(m)), Transform::default())).id();
                commands.entity(f).add_child(child);
            }
        }
        return;
    }
    let items = &session.levels[session.current].items;
    let shown = |p: usize| -> Option<(usize, String)> {
        let s = g.see_thru.get(p)?;
        let c = s.chest?;
        let name = match s.icon.as_ref()? {
            gdl_core::game::seethru::SeeThruIcon::Death => "DEATH_ICON".to_string(),
            gdl_core::game::seethru::SeeThruIcon::Keys => "KEYRING".to_string(),
            gdl_core::game::seethru::SeeThruIcon::Tree(d) => String::from_utf8_lossy(g.items.definitions.get(*d)?.name()).into_owned(),
        };
        Some((c, name))
    };
    let mut chests = std::collections::HashSet::new();
    for (entity, mut view, mut tf, mut vis) in &mut views {
        let Some((c, name)) = shown(view.0) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let Some(r) = g.items.records.get(c) else { continue };
        chests.insert(c);
        let m = r.matrix();
        let mut t = record_transform([Vec3::from_array(m[0]), Vec3::from_array(m[1]), Vec3::from_array(m[2])], Vec3::from_array(r.position()));
        t.scale *= 0.65;
        *tf = t;
        *vis = Visibility::Inherited;
        if view.1.as_deref() != Some(name.as_str()) {
            spawn_parts(&mut commands, entity, item_parts(&sources, &libs, items, &name), &mut materials, &mut variants, &mut pending);
            view.1 = Some(name);
        }
    }
    for (frame, mut face, mut ftf, mut vis) in &mut frames {
        match shown(frame.0).and_then(|(c, _)| g.items.records.get(c)) {
            Some(r) => {
                // seethru_node takes the chest node's 3×3 (its scale too).
                let m = r.matrix();
                let scale = Vec3::from_array(m[0]).length();
                face.anchor = Vec3::from_array(r.position());
                let radius = g.items.definitions[r.definition()].f32_(0xc);
                face.offset = Vec3::new(0.0, 0.0, radius);
                // Moved towards the camera only for depth: keep the size it
                // has on screen at the chest's centre.
                let eye = g.std_camera.as_ref().map(|c| Vec3::from_array(c.pb.eye));
                let d = eye.map_or(0.0, |e| e.distance(face.anchor));
                let k = if radius < d { (d - radius) / d } else { 1.0 };
                ftf.scale = Vec3::splat(scale * k);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    // The shown chests' parts sorted after the window, the views' icon parts
    // before it; others restored.
    let mut under = Vec::new();
    let mut icons = Vec::new();
    for (e, ..) in &views {
        let mut stack: Vec<Entity> = children.get(e).map(|c| c.iter().collect()).unwrap_or_default();
        while let Some(x) = stack.pop() {
            icons.push(x);
            if let Ok(c) = children.get(x) {
                stack.extend(c.iter());
            }
        }
    }
    for (e, slot) in &slots {
        if slot.kind == SlotKind::Item && chests.contains(&slot.index) {
            let mut stack = vec![e];
            while let Some(x) = stack.pop() {
                under.push(x);
                if let Ok(c) = children.get(x) {
                    stack.extend(c.iter());
                }
            }
        }
    }
    let icon_set: std::collections::HashSet<Entity> = icons.iter().copied().collect();
    for e in icons {
        if through.contains(e) {
            continue;
        }
        let Ok(mut m) = parts.get_mut(e) else { continue };
        let Some(mut own) = materials.get(&m.0).cloned() else { continue };
        own.draw.sort_first = true;
        if matches!(own.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
            own.alpha_mode = AlphaMode::Blend;
        }
        let orig = m.0.clone();
        m.0 = materials.add(own);
        commands.entity(e).insert(ThroughChest(orig));
    }
    let under_set: std::collections::HashSet<Entity> = under.iter().copied().chain(icon_set.iter().copied()).collect();
    for (e, orig) in &through {
        if !under_set.contains(&e) {
            if let Ok(mut m) = parts.get_mut(e) {
                m.0 = orig.0.clone();
            }
            commands.entity(e).remove::<ThroughChest>();
        }
    }
    for e in under {
        if through.contains(e) {
            continue;
        }
        let Ok(mut m) = parts.get_mut(e) else { continue };
        let Some(mut own) = materials.get(&m.0).cloned() else { continue };
        own.draw.sort_late = true;
        if matches!(own.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
            own.alpha_mode = AlphaMode::Blend;
        }
        let orig = m.0.clone();
        m.0 = materials.add(own);
        commands.entity(e).insert(ThroughChest(orig));
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
            commands.entity(entity).insert(EffectTree { pkg: pkg.to_string(), tree, start, timing: timing.max(1), slot, looping: false, seq: 0, fixed: None });
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
    /// AnimateATree semantics (a tree a game function animates every
    /// update): the sequence wraps at its frame count when its repeat word
    /// is set (the familiar's READY) and holds its last frame otherwise.
    looping: bool,
    /// The sequence played (StartFXTree trees: 0).
    seq: usize,
    /// A tree on its own AnimateATree clock (TreeClock): its sequence and
    /// frame instead of the ATime-derived ones.
    fixed: Option<(usize, f32)>,
}

/// An AnimateATree clock of a presentation tree (the familiar, the attached
/// powerup tree): the original selection (animation_clock::select) with
/// the tree's clips, fed once per frame with the core's requests.
#[derive(Component)]
struct TreeClock {
    clock: gdl_core::animation_clock::ClockState,
    current: u16,
    clips: Vec<gdl_core::animation_clock::ClipTiming>,
    seen_attacks: u32,
    seen_pojo: u32,
}

impl TreeClock {
    fn new(clips: Vec<gdl_core::animation_clock::ClipTiming>, req: &gdl_core::game::TreeAnimRequest) -> Self {
        Self { clock: gdl_core::game::setup::fresh_clock(), current: 0, clips, seen_attacks: req.attacks, seen_pojo: req.pojo_attacks }
    }

    /// AnimateATree(tree, seq, mode) at the game's ATime; the sequence and
    /// frame to pose.
    fn step(&mut self, seq: u16, mode: i32, g: &Game) -> Option<(usize, f32)> {
        let seq = seq.min(self.clips.len().saturating_sub(1) as u16);
        let (current, requested) = (*self.clips.get(usize::from(self.current)).or(self.clips.get(usize::from(seq)))?, *self.clips.get(usize::from(seq))?);
        let input = gdl_core::animation_clock::SelectionInput { sequence: seq, mode, start_frame: 0, blend_duration: 0.0, playback_scale: 1.0, now: g.globals.atime, anim_time_scale: g.globals.anim_time_scale };
        let out = gdl_core::animation_clock::select(self.clock, current, requested, input).ok()?;
        self.clock = out.state;
        if out.return_flags & 1 != 0 {
            self.current = seq;
        }
        Some((usize::from(self.clock.sequence.min(self.clips.len().saturating_sub(1) as u16)), self.clock.frame))
    }
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
        let mut frame = ((atime - fx.start) * 900.0 / f32::from(fx.timing.max(1))).max(0.0);
        let seq = fx.fixed.map_or(fx.seq, |f| f.0);
        let sequence = anim.trees.get(fx.tree).and_then(|t| t.sequences.get(seq));
        if let Some((_, f)) = fx.fixed {
            frame = f;
        } else if fx.looping
            && let Some(s) = sequence
        {
            let n = f32::from(s.frames.max(1));
            frame = if s.repeat != 0 { frame % n } else { frame.min(n - 1.0) };
        }
        let Ok(pose) = gdl_client::tree_pose(anim, fx.tree, &pkg.anim_bytes, seq, frame, true) else { continue };
        let index = object_frame(frame, sequence);
        let mut meshes = Vec::new();
        for child in children.iter() {
            if let Ok((e, idx, mut tf, of, mut v)) = parts.get_mut(child) {
                if let Some(m) = pose.get(idx.0) {
                    *tf = Transform::from_matrix(*m);
                }
                if let Some(f) = of {
                    show_oframe(f, seq as u16, index, &mut v);
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
            request_tree_texmods(mods, lib, index, &meshes, &mut req);
        }
    }
}

/// AnimateNode / DoAnimateTree's object frame (EE 0x112754, 0x11298c):
/// (int)(frame + 0.5), or AnimInfo +0x10 (the clip's frame count, InitAnim)
/// − that − 1 when the sequence's +0x2a bit 0 reverses it. Only 49 item
/// sequences on the disc set the bit (research/animation.md); OANIM frames
/// and texture modifiers use the index, skeletal channels do not.
fn object_frame(frame: f32, seq: Option<&gdl_formats::animation::Sequence>) -> i32 {
    let f = (frame + 0.5) as i32;
    match seq {
        Some(s) if s.frame_order_flags & 1 != 0 => i32::from(s.frames) - f - 1,
        _ => f,
    }
}

/// DoObjAnimation for one OANIM frame object of sequence `seq` at object
/// frame `index` ([`object_frame`]).
fn show_oframe(f: &OFrame, seq: u16, index: i32, v: &mut Visibility) {
    let at = index - i32::from(f.start);
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
                item_tree_packages(&sources, items).into_iter().find_map(|pkg| {
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
        let index = object_frame(frame, a.trees.get(tree).and_then(|t| t.sequences.get(anim.current as usize)));
        let mut meshes = Vec::new();
        for child in children.iter() {
            if let Ok((e, idx, mut tf, of, mut v)) = parts.get_mut(child) {
                if let Some(m) = pose.get(idx.0) {
                    *tf = Transform::from_matrix(*m);
                }
                if let Some(f) = of {
                    show_oframe(f, anim.current, index, &mut v);
                }
                meshes.push((e, idx.0));
            }
        }
        let mods = texmods.entry((pkg.clone(), tree)).or_insert_with(|| a.trees.get(tree).map(|t| tree_texmods(p, t)).unwrap_or_default());
        if let Some(lib) = libs.0.get(&pkg)
            && !mods.is_empty()
        {
            request_tree_texmods(mods, lib, index, &meshes, &mut req);
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
type BarQuery<'w, 's> = Query<'w, 's, &'static mut Visibility, (With<LetterBox>, Without<CharacterRoot>, Without<RigNode>)>;
type NodeQuery<'w, 's> = Query<'w, 's, (&'static RigNode, &'static mut Transform, Has<HeadMesh>, &'static mut Visibility, Option<&'static LeftHandMesh>), (Without<CharacterRoot>, Without<FollowCamera>, Without<Slot>)>;
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
    mut bars: BarQuery,
    mut pojo_was: Local<[bool; 4]>,
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
    // PlayerProcessPowerups: the pojo (+0x138 0x400) hides the player's
    // own model tree (+0x90, MBTreeSetFlags 2) — the player becomes the
    // pojo; shown again (once) when it wears off.
    let pojo: [bool; 4] = std::array::from_fn(|k| sim.game.players[k].u32_(0x138) & 0x400 != 0);
    // The left hand's object hides the hand (class 7) or its parent
    // (base classes 1, 5) — MB flag 1, the node alone.
    let left_hide: [Option<bool>; 4] = std::array::from_fn(|k| {
        let held = hand_objects(&sim.game, k)[0].is_some();
        match sim.game.players[k].u32_(8) {
            1 | 5 if held => Some(true),
            7 if held => Some(false),
            _ => None,
        }
    });
    for (node, mut t, head, mut vis, left) in &mut nodes {
        let k = node.1.min(3);
        let left_hidden = left.is_some_and(|l| left_hide[k] == Some(l.0));
        if pojo[k] || left_hidden {
            *vis = Visibility::Hidden;
        } else if pojo_was[k] || left.is_some() {
            *vis = Visibility::Inherited;
        }
        if let Some(p) = poses.get(node.1).and_then(|p| p.as_ref()).and_then(|pose| pose.get(node.0)) {
            // Level ≥ 99: the HEAD node's scale 1.5 (HeadMesh).
            let big = head && 99 <= sim.game.players[node.1].i32_(0x2ac0);
            *t = Transform::from_matrix(if big { *p * Mat4::from_scale(Vec3::splat(1.5)) } else { *p });
        }
    }
    *pojo_was = pojo;
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
             {} {}   kills {}   enemies {}   generators {}   at {:.1} {:.1} {:.1}\n\
             {}\n\
             P1 move: arrows/WASD  cross Space/K  circle E/L  triangle Shift/I  square Q/J | P2 (join: U = Start) TFGH, R, Y | Esc/P/Start: menu  F1",
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
            p.f32_(0x50),
            p.f32_(0x54),
            p.f32_(0x58),
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

/// One ScrollMessageBox loop pass per elapsed vblank batch (wall time at
/// the NTSC rate): on the pages PlayerControls runs and, once 15 vblanks
/// have passed, the masked players' triangle edge turns the page
/// (ClearAllPlayerControls(2) after it); the fire scroll polls no controls
/// and clears them per frame; the end clears them with lockout 4.
fn step_message_box(sim: &mut Sim, ports: &[PadStatus; 8], dt: f32) {
    sim.scroll_clock += dt * 59.94;
    let vb = sim.scroll_clock.floor() as i32;
    if vb < 1 {
        return;
    }
    sim.scroll_clock -= vb as f32;
    let Some(mut sb) = sim.message_box.take() else { return };
    let burning = sb.burn.is_some();
    let mut pressed = false;
    if !burning {
        if let Err(e) = sim.game.update_controls(ports) {
            sim.error = Some(format!("controls: {}", e.0));
        }
        pressed = sb.wait < 1 && sim.game.scroll_box_pressed(sb.mask);
    }
    let page = sb.page;
    let open = sb.step(vb, pressed);
    // StartFireScroll after the last page: AudioMenuBurn.
    if !burning && sb.burn.is_some() {
        sim.game.audio_menu_burn();
    }
    if burning || sb.page != page {
        sim.game.scroll_box_page_end();
    }
    if open {
        sim.message_box = Some(sb);
    } else {
        sim.game.scroll_box_close();
    }
}

/// ScrollMessageBox pages of a SCROLL_E entry: its FONT name resolved
/// against font_desc (StringInitSub), scale and space scale.
fn scroll_box(game: &Game, rom: &gdl_formats::textrom::TextRom, e: &gdl_formats::textrom::TextEntry, pages: Vec<String>, mask: u32) -> gdl_core::game::hud::ScrollBox {
    let name = rom.fonts.get(e.font as usize).cloned().unwrap_or_default();
    let font = game.hud.tables.as_ref().and_then(|t| t.font_desc.iter().position(|n| *n == name)).unwrap_or(0);
    let mut sb = gdl_core::game::hud::ScrollBox::new(pages, font, e.scale, e.space);
    sb.mask = mask;
    sb
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
/// The screen saver's weapons: the scene camera renders only this layer
/// while ScreenSaverStart has hidden every node.
const SAVER_LAYER: usize = 3;

/// A screen-saver weapon's root (ssave_weap index) and its tree parts.
#[derive(Component)]
struct SaverNode(usize);
/// POWERUPS' SAVER_AXE / SAVER_HAM / SAVER_SWD / SAVER_WND (tree, timing).
#[derive(Resource, Default)]
struct SaverTrees([Option<(usize, i16)>; 4]);
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
    // ScreenSaverCreateWeap's trees (AtreeMatch(powerup_anim, ssave_desc[k])).
    let mut saver = SaverTrees::default();
    let saver_layer = bevy::camera::visibility::RenderLayers::layer(SAVER_LAYER);
    for (k, name) in gdl_core::game::saver::TREES.iter().enumerate() {
        let Some((ti, t)) = pkg.anim.as_ref().and_then(|a| a.trees.iter().enumerate().find(|(_, t)| t.label == *name)) else { continue };
        let mut model = Vec::new();
        for node in &t.nodes {
            tree_node_model(pkg, &lib, t, node, Vec3::from_array(node.initial_position), &mut model);
        }
        let root = commands.spawn((Transform::default(), Visibility::Hidden, SaverNode(k), saver_layer.clone())).id();
        for (at, parts, _, idx, extra) in model {
            if matches!(extra, Some(NodeExtra::Psys(_))) {
                continue;
            }
            let Some(idx) = idx else { continue };
            let holder = commands.spawn((TreeNodeIdx(idx), Transform::from_translation(at), Visibility::Inherited, saver_layer.clone())).id();
            commands.entity(root).add_child(holder);
            for (mesh, material) in parts {
                let c = commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::default(), saver_layer.clone(), bevy::camera::visibility::NoFrustumCulling)).id();
                commands.entity(holder).add_child(c);
            }
        }
        saver.0[k] = Some((ti, t.sequences.first().map_or(30, |q| q.timing_factor)));
    }
    commands.insert_resource(saver);
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
        Projection::custom(Ps2Projection::new(320.0 / ICON_FOCAL, 224.0 / ICON_FOCAL, 0.05, 100.0)),
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
        params: gdl_client::BlitParams { rgba: Vec4::new(1.0, 1.0, 1.0, 128.0 / 255.0), crop: Vec4::new(0.0, 1.0, 0.0, 1.0), mask_rect: Vec4::ZERO, corners: [Vec4::ZERO; 4] },
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

/// The screen saver (Game::saver): the scene camera shows only the
/// weapons' layer; each weapon sits at its camera-space position and
/// rotation (MB view space, mirrored into Bevy's −z-forward view) and plays
/// its tree's sequence 0 (AnimateATree; HYPOTHESIS as effect trees).
#[allow(clippy::type_complexity)]
fn present_saver(
    sim: Option<Res<Sim>>,
    front: Res<Front>,
    trees: Option<Res<SaverTrees>>,
    mut camera: Query<(&GlobalTransform, Option<&bevy::camera::visibility::RenderLayers>, Entity), With<FollowCamera>>,
    mut roots: Query<(&SaverNode, &mut Transform, &mut Visibility, &Children), Without<TreeNodeIdx>>,
    mut parts: Query<(&TreeNodeIdx, &mut Transform), Without<SaverNode>>,
    mut commands: Commands,
) {
    let active = sim.as_ref().is_some_and(|s| s.game.saver.active);
    let Ok((cam, layers, cam_entity)) = camera.single_mut() else { return };
    let saver_layer = bevy::camera::visibility::RenderLayers::layer(SAVER_LAYER);
    match (active, layers) {
        (true, l) if l != Some(&saver_layer) => {
            commands.entity(cam_entity).insert(saver_layer.clone());
        }
        (false, Some(_)) => {
            commands.entity(cam_entity).remove::<bevy::camera::visibility::RenderLayers>();
        }
        _ => {}
    }
    let cam = cam.to_matrix();
    for (node, mut tf, mut vis, kids) in &mut roots {
        let w = sim.as_ref().filter(|_| active).map(|s| &s.game.saver.weaps[node.0]).filter(|w| w.shown);
        let want = if w.is_some() { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        let Some(w) = w else { continue };
        // MB view (x right, y up, z forward; row vectors) → Bevy view: S =
        // diag(1, 1, −1) on the left; rows of rot are the local axes.
        let s = Mat4::from_scale(Vec3::new(1.0, 1.0, -1.0));
        let r = Mat4::from_cols(
            Vec4::new(w.rot[0][0], w.rot[0][1], w.rot[0][2], 0.0),
            Vec4::new(w.rot[1][0], w.rot[1][1], w.rot[1][2], 0.0),
            Vec4::new(w.rot[2][0], w.rot[2][1], w.rot[2][2], 0.0),
            Vec4::new(w.pos[0], w.pos[1], w.pos[2], 1.0),
        );
        *tf = Transform::from_matrix(cam * s * r);
        let (Some(anim), Some(Some((tree, timing)))) = (front.powerups_pkg.anim.as_ref(), trees.as_ref().map(|t| t.0[node.0])) else { continue };
        let frame = w.anim_vb as f32 / 59.94 * 900.0 / f32::from(timing.max(1));
        let Ok(pose) = gdl_client::tree_pose(anim, tree, &front.powerups_pkg.anim_bytes, 0, frame, true) else { continue };
        for k in kids.iter() {
            if let Ok((idx, mut t)) = parts.get_mut(k)
                && let Some(m) = pose.get(idx.0)
            {
                *t = Transform::from_matrix(*m);
            }
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
    // MBDrawBlits' HILITE_FRAC clock (vb_counter).
    let vb_counter = sim.as_ref().map_or(0, |s| s.game.vb_counter);
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
        let state = gdl_core::game::optmenu::MenuState { selected: at, frames: front.menu_clock.1 as i32, finishing: 0, player: Some(0), sliders, ended: false };
        let mut menu = front.menus[key].clone();
        // do_audiomenu: the Mono entry (0x19) +0x1c = stereo + 1, +0x20 =
        // stereo (the check mark and highlight on Mono or Stereo).
        if key == "audio_options" {
            let stereo = front.menu.prefs[2].clamp(0, 1);
            for e in menu.entries.iter_mut().filter(|e| e.action == 0x19) {
                e.mark = stereo + 1;
                e.state = stereo;
            }
        }
        // OptionsStart: game_menu's Quit Level (0x26) disabled in world 0xc.
        if key == "game_menu" && front.menu.quit_level_disabled {
            for e in menu.entries.iter_mut().filter(|e| e.action == 0x26) {
                e.state = -1;
            }
        }
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
    // finish_optmenu's fading menus and the fire scroll (end_optmenu).
    if let Some(style) = front.opt_style.as_ref() {
        let mut over = Vec::new();
        for (m, at, frames, f) in &front.ending.finishing {
            let state = gdl_core::game::optmenu::MenuState { selected: *at, frames: *frames, finishing: (*f as i32).max(1), player: Some(0), sliders: Vec::new(), ended: true };
            over.extend(front.ui_hud.optmenu_blits(style, m, &state));
        }
        match &front.ending.burn {
            Some((back, rect, vb)) => front_list.extend(front.ui_hud.optmenu_burn_blits(style, back, *rect, *vb as i32, over)),
            None => front_list.extend(over),
        }
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
        let gs = |c: [u8; 4]| Vec4::from_array(c.map(|v| f32::from(v) / 128.0));
        let (rgba, corners) = match b.corner_colours(vb_counter) {
            Some(c) => (gs(c[0]), [gs(c[1]), gs(c[2]), gs(c[3]), Vec4::X]),
            None => (gs(b.rgba), [Vec4::ZERO; 4]),
        };
        let params = gdl_client::BlitParams { rgba, crop: Vec4::from_array(b.crop), mask_rect, corners };
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
        .map(|a| a.trees.iter().map(|t| (t.label.clone(), t.sequences.first().map_or(0, |s| s.frames), t.sequences.first().map_or(0, |s| s.timing_factor), t.sequences.first().map_or(0, |s| s.repeat))).collect())
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
        transmitter_table: Vec::new(),
        check_num: 0,
    };
    let sources = Sources { players: [Some(pc), None, None, None], packages, monsters: level_monster_map(first), monster_packages: b.monster_packages.clone() };
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
        // PlayerLoadSaveFile's +0x2af4: the card file player 1 owns (file k
        // is slot k + 1), which the select screen's verify_vmu_file_ok
        // keeps other players from loading or overwriting.
        if let Some(slot) = from.filter(|s| (1..=8).contains(s)) {
            game.players[0].set_i32(0x2af4, slot as i32 - 1);
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
        // The original loads the Tower after init_player_select(0), so its
        // init_next_level → setup_player_display sees player 1 choosing
        // (state 2, column 0: display mode 0 — no coin, heart, gold, health
        // or name) until update_class_spec redraws the panel. Built before
        // the select, the panel kept mode 1 and drew the name being typed
        // over the name entry's letters (playtest 2026-10-27). setup_player_display's
        // callers in SLUS_200.47: init_next_level, load_player_geo,
        // update_class_spec, do_players, init_mapscreen, init_shop and others
        // — none on the name-entry path.
        for k in 0..4 {
            game.setup_player_display(k);
        }
    }
    world.insert_resource(Sim { game, error: None, updates: 0, frozen: false, sim_clock: false, weapon_hidden: [false; 4], exp: 0, sparks: Vec::new(), last_event: String::new(), message_box: None, controller_box: false, hint_menu: false, scroll_clock: 0.0, shop: None, map: None, shop_done: None, debug_skip: false, select, select_done: false, select_quit: false, select_next: None, map_done: None, movie: None, movie_done: None, over: None, over_done: false, flyby: None, flyby_done: false });
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
    world.insert_resource(Sim { game, error: None, updates: 0, frozen: false, sim_clock: false, weapon_hidden: [false; 4], exp: 0, sparks: Vec::new(), last_event: String::new(), message_box: None, controller_box: false, hint_menu: false, scroll_clock: 0.0, shop: None, map: None, shop_done: None, debug_skip: false, select: None, select_done: false, select_quit: false, select_next: None, map_done: None, movie: None, movie_done: None, over: None, over_done: false, flyby: Some(flyby), flyby_done: false });
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

/// Left / right held (keyboard or d-pad): do_optmenu's level bits.
fn menu_held(world: &mut EcsWorld) -> (bool, bool) {
    let keys = world.resource::<ButtonInput<KeyCode>>();
    let mut left = keys.pressed(KeyCode::ArrowLeft) || keys.pressed(KeyCode::KeyA);
    let mut right = keys.pressed(KeyCode::ArrowRight) || keys.pressed(KeyCode::KeyD);
    let mut pads = world.query::<&Gamepad>();
    for g in pads.iter(world) {
        left |= g.pressed(GamepadButton::DPadLeft);
        right |= g.pressed(GamepadButton::DPadRight);
    }
    (left, right)
}

/// Menu keys this frame (keyboard and every gamepad), and the pause toggle.
fn menu_keys(world: &mut EcsWorld, naming: bool) -> (Vec<Key>, bool) {
    let mut out = Vec::new();
    let mut toggle = false;
    // The settings overlay has the keyboard while open (and until its keys
    // are released); the menus take player 1's bound keys too.
    let blocked = world.get_resource::<gdl_client::enhance::Overlay>().is_some_and(|o| o.open || o.held);
    let bindings = world.get_resource::<gdl_client::controls::Bindings>().cloned().unwrap_or_default();
    let keys = world.resource::<ButtonInput<KeyCode>>();
    let pressed = |k: KeyCode| !blocked && keys.just_pressed(k);
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
        // The original's menu edges: cross 0x2000000 accepts, triangle
        // 0x8000000 goes back, circle 0x1000000 (Center); fixed keys
        // (arrows, Enter/Escape/Backspace/C) plus player 1's bound ones
        // (by default WASD, Space/K, Shift/I, E/L).
        for (fixed, action, key) in [
            (&[KeyCode::ArrowUp][..], "up", Key::Up),
            (&[KeyCode::ArrowDown][..], "down", Key::Down),
            (&[KeyCode::ArrowLeft][..], "left", Key::Left),
            (&[KeyCode::ArrowRight][..], "right", Key::Right),
            (&[KeyCode::Enter][..], "cross", Key::Accept),
            (&[KeyCode::Escape, KeyCode::Backspace][..], "triangle", Key::Back),
            (&[KeyCode::KeyC][..], "circle", Key::Center),
        ] {
            if fixed.iter().chain(bindings.of(action)).any(|&k| pressed(k)) {
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
            (GamepadButton::North, Key::Back),
            (GamepadButton::East, Key::Center),
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

/// The menus' AudioFX requests: do_optmenu / DoOptions cues
/// (Game::audio_cursor_v and the others: AudioFX(id, volume, priority),
/// centre pan) and StartFireScroll's AudioMenuBurn.
fn play_menu_cues(mut front: ResMut<Front>, mut audio: Option<NonSendMut<gdl_client::audio::AudioOut>>) {
    use gdl_client::menu::Cue;
    let cues = std::mem::take(&mut front.menu.cues);
    let mut sfx = std::mem::take(&mut front.sfx);
    for c in cues {
        sfx.push(match c {
            Cue::CursorV => (0xd, 0x7f, 3),
            Cue::CursorH => (0xe, 0x7f, 3),
            Cue::Select => (0x11, 0x7f, 3),
            Cue::Exit => (0x10, 0x7f, 3),
            Cue::Buzzer => (10, 0x7f, 0x30),
            Cue::PlayerStart => (0xc, 0xe0, 4),
            // AudioVolSfx: AudioSFX2(id, pan 0x7f, 0x7f, 1).
            Cue::VolSfxEnd => (0x14, 0x7f, 1),
            Cue::VolSfxSample(k) => (front.vol_sfx_samples[k % 2], 0x7f, 1),
        });
    }
    if let Some(a) = audio.as_mut() {
        for (id, volume, priority) in sfx {
            a.play(id, volume, 0x7f, priority);
        }
    }
}

/// The original menu shown now: (record, selected entry, stack level).
fn shown_menu(world: &EcsWorld) -> Option<(&'static str, usize, Option<usize>)> {
    let front = world.resource::<Front>();
    let (key, at) = menu_view(front, *world.resource::<Screen>(), world.resource::<Ui>())?;
    Some((key, at, gdl_client::menu::page_depth(&front.menu.page)))
}

/// end_optmenu for the menu `before` once the page has changed (a push
/// keeps the stack below, a pop or close removes it): see MenuEnding.
fn end_menu(world: &mut EcsWorld, before: Option<(&'static str, usize, Option<usize>)>) {
    let Some((key, at, depth)) = before else { return };
    let after = shown_menu(world);
    if after.map(|a| a.0) == Some(key) {
        return;
    }
    let mut front = world.resource_mut::<Front>();
    let Some(old) = front.menus.get(key).cloned() else { return };
    let push = matches!((after, depth), (Some((_, _, Some(n))), Some(d)) if d < n);
    let lower_back = after.and_then(|a| front.menus.get(a.0)).is_some_and(|m| m.back.is_some());
    let rect = front.opt_style.as_ref().map(|style| front.ui_hud.optmenu_back_rect(style, &old));
    if !push
        && !lower_back
        && let (Some(back), Some(rect)) = (old.back.clone(), rect)
    {
        // StartFireScroll: AudioMenuBurn (AudioFX(0x13, 0xe0, 3)) always;
        // a new burn only when none is being served (fire_zmask 0).
        front.sfx.push((0x13, 0xe0, 3));
        if front.ending.burn.is_none() {
            front.ending.burn = Some((back, rect, 0.0));
        }
    }
    if old.flags & 0x20 != 0 {
        let frames = front.menu_clock.1 as i32;
        front.ending.finishing.push((old, at, frames, 1.0));
    }
}

/// finish_optmenu and ServeFireScroll for `vb` vblanks.
fn serve_menu_ending(front: &mut Front, vb: f32) {
    let finish = front.opt_style.as_ref().map_or(30, |s| s.finish_frames) as f32;
    for f in &mut front.ending.finishing {
        f.3 += vb;
    }
    front.ending.finishing.retain(|f| f.3 <= finish);
    if let Some(b) = front.ending.burn.as_mut() {
        b.2 += vb;
        if 0x15 <= (b.2 as i32) >> 1 {
            front.ending.burn = None;
        }
    }
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
            // DoOptions 0x14: quit_level = 1; game_main aborts every player
            // once the menu and its fire scroll are done (Game::
            // quit_level_check), and the level ends as if all had died.
            Action::QuitLevel => {
                world.resource_mut::<Ui>().paused = false;
                if let Some(mut sim) = world.get_resource_mut::<Sim>() {
                    sim.game.globals.quit_level = 1;
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
            Action::Volumes { music, sfx, store } => {
                // OptionsSetup: AudioSetVolSfx / AudioSetStereo (host) and
                // AudioSetVolMusic (the simulation's volMusic).
                let stereo = world.resource::<Front>().menu.prefs[2];
                if let Some(mut a) = world.get_non_send_resource_mut::<gdl_client::audio::AudioOut>() {
                    a.set_options(i32::from(sfx), stereo);
                }
                if let Some(mut sim) = world.get_resource_mut::<Sim>() {
                    sim.game.audio_set_vol_music(i32::from(music));
                }
                let mut front = world.resource_mut::<Front>();
                front.game_opts[0..4].copy_from_slice(&i32::from(sfx).to_le_bytes());
                front.game_opts[4..8].copy_from_slice(&i32::from(music).to_le_bytes());
                front.menu.prefs[0] = i32::from(sfx);
                front.menu.prefs[1] = i32::from(music);
                if store {
                    let opts = front.game_opts.clone();
                    front.store.store_options(&opts)?;
                }
                let prefs = front.menu.prefs;
                if let Some(mut session) = world.get_resource_mut::<Session>() {
                    session.prefs = prefs;
                }
            }
            Action::Stereo { on } => {
                // AudioSetStereo (GamePrefs word 2; mono when 0).
                let mut front = world.resource_mut::<Front>();
                let (sfx, stereo) = (front.menu.prefs[0], i32::from(on));
                front.menu.prefs[2] = stereo;
                front.game_opts[8..12].copy_from_slice(&stereo.to_le_bytes());
                let opts = front.game_opts.clone();
                front.store.store_options(&opts)?;
                let prefs = front.menu.prefs;
                if let Some(mut session) = world.get_resource_mut::<Session>() {
                    session.prefs = prefs;
                }
                if let Some(mut a) = world.get_non_send_resource_mut::<gdl_client::audio::AudioOut>() {
                    a.set_options(sfx, stereo);
                }
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
    // TEST harness: GDL_TEST_AUTOSTART creates a default character (TEST,
    // the Sorceress, yellow) and starts at once — no menu time before the
    // level, so runs start from the same state.
    if screen == Screen::Front && std::env::var_os("GDL_TEST_AUTOSTART").is_some() && !world.resource::<TestHarness>().autostarted {
        world.resource_mut::<TestHarness>().autostarted = true;
        // GDL_TEST_LOAD_SLOT: start with that stored slot instead.
        match std::env::var("GDL_TEST_LOAD_SLOT").ok().and_then(|v| v.parse::<u32>().ok()) {
            Some(slot) => act(world, Action::Play { slot }),
            None => {
                // GDL_TEST_CLASS=CLASS:COLOUR: another class and colour.
                let (class, colour) = std::env::var("GDL_TEST_CLASS")
                    .ok()
                    .and_then(|v| {
                        let (c, k) = v.split_once(':')?;
                        Some((c.parse().ok()?, k.parse().ok()?))
                    })
                    .unwrap_or((6, 0));
                act(world, Action::Create { class, colour, name: "TEST".into() })
            }
        }
        return;
    }
    // DoOptions: finish_optmenu / ServeFireScroll each frame.
    let vb = world.resource::<Time>().delta_secs() * 59.94;
    serve_menu_ending(&mut world.resource_mut::<Front>(), vb);
    let naming = matches!(world.resource::<Front>().menu.page, gdl_client::menu::Page::Name { .. });
    let (keys, mut toggle) = menu_keys(world, naming);
    // optmenu_active stays 1 while the last menu finishes (no OptionsStart).
    if !world.resource::<Front>().ending.finishing.is_empty() && !world.resource::<Ui>().paused {
        toggle = false;
    }
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
    if screen == Screen::Playing && world.get_resource::<Sim>().is_some_and(|s| s.game.restart_req && s.game.globals.game_mode != 0x400e) {
        // game_main's RestartReq: everything ends, init_attract_mode(0x8009)
        // (the title screen).
        end_game(world);
        world.resource_mut::<Front>().menu.page = gdl_client::menu::Page::PressStart;
        return;
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
            // ScrollMessageBox is modal (simulate steps it): no menus.
            return;
        }
        if std::mem::take(&mut world.resource_mut::<Sim>().hint_menu) && !world.resource::<Ui>().paused {
            // HintMenu: next_general_hint(0) once, the others every time.
            let mut hints = std::mem::take(&mut world.resource_mut::<Front>().hints);
            world.resource::<Sim>().game.hint_menu_open(&mut hints);
            world.resource_mut::<Front>().hints = hints;
            world.resource_mut::<Front>().menu.page = gdl_client::menu::Page::Hints { at: 0 };
            world.resource_mut::<Ui>().paused = true;
            // start_optmenu: ClearAllPlayerControls(2).
            let _ = world.resource_mut::<Sim>().game.controls.clear_all_player_controls(2);
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
                let cur_world = world.resource::<Sim>().game.globals.cur_world;
                let mut front = world.resource_mut::<Front>();
                front.menu.controls = controls;
                front.menu.status.clear();
                front.menu.pause(tower);
                // OptionsStart: Quit Level disabled in world 0xc.
                front.menu.quit_level_disabled = cur_world == 0xc;
                world.resource_mut::<Ui>().paused = true;
                // start_optmenu: ClearAllPlayerControls(2); OptionsStart's
                // AudioStopAll (the queue, the tracks and every SFX voice;
                // the music stream goes on).
                let mut sim = world.resource_mut::<Sim>();
                let _ = sim.game.controls.clear_all_player_controls(2);
                sim.game.audio_stop_all();
                if let Some(mut a) = world.get_non_send_resource_mut::<gdl_client::audio::AudioOut>() {
                    a.stop_voices();
                }
            }
            if world.resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::F2)
                && let Some(sim) = world.get_resource::<Sim>()
            {
                // DEBUG: the items within 6 units of player 0 and its class
                // record's crystal counts, to the log.
                let g = &sim.game;
                let at = g.players[0].v3(0x50);
                let cls = 0xb4 * g.players[0].u32_(0xc) as usize;
                let counts: Vec<i16> = (0..9).map(|n| g.players[0].i16_(0xd0a + cls + 2 * n)).collect();
                eprintln!("DEBUG F2 player {at:?} crystals {counts:?} max {:?} time {} garg_mtime {:?} door_mtime {:?}", g.items.tables.max_crystals, g.globals.time, g.tower.garg_mtime, g.tower.door_mtime);
                for (k, c) in g.critters.iter().enumerate() {
                    if let Some(c) = c {
                        eprintln!("DEBUG F2 critter {k} type {:?} clip {} frame {} clips {}", c.ty, c.clip, c.clock.frame, c.clips.len());
                    }
                }
                if world.resource::<ButtonInput<KeyCode>>().pressed(KeyCode::ShiftLeft) {
                    // Shift+F2: every active item.
                    for (s, r) in g.items.records.iter().enumerate().filter(|(_, r)| r.active()) {
                        let def = &g.items.definitions[r.definition()];
                        eprintln!(
                            "DEBUG F2 all {s} {} kind {} sub {} pos {:?} d8 {:#06x} geo {:?} geo_name {:?} f0 {} f2 {}",
                            String::from_utf8_lossy(def.name()),
                            def.kind(),
                            def.subtype(),
                            r.position(),
                            r.u16_(0xd8),
                            g.items.geo.get(s),
                            g.items.geo_name.get(s),
                            r.i16_(0xf0),
                            r.i16_(0xf2)
                        );
                    }
                }
                eprintln!(
                    "DEBUG F2 state world {:#x} level {} welcome {} new_player {} game_mode {:#x} active_players {:#x} mask {:#x} states {:?} continue {:?} level_done {:?} exit_ready {} updates {} error {:?} unsupported {:?}",
                    g.globals.cur_world,
                    g.globals.cur_level,
                    g.globals.welcome_flag,
                    g.tower.new_player,
                    g.globals.game_mode,
                    g.globals.active_players,
                    g.active_player_mask,
                    (0..4).map(|k| g.players[k].i32_(0xfc)).collect::<Vec<_>>(),
                    (0..4).map(|k| g.players[k].i32_(0x2ad4)).collect::<Vec<_>>(),
                    g.level_done,
                    g.exit_ready,
                    sim.updates,
                    sim.error,
                    g.log.unsupported.iter().filter(|(_, n)| 0 < **n).map(|(k, n)| format!("{k} x{n}")).collect::<Vec<_>>()
                );
                eprintln!(
                    "DEBUG F2 flow shop {:?} select {:?} map {} movie {} over {} shop_done {:?} select_next {:?} que {:?} box {} help {} paused {}",
                    sim.shop.as_ref().map(|(f, k)| (f.skip, f.from_menu, *k)),
                    sim.select.as_ref().map(|ps| (ps.mode, ps.load_audio, ps.abort_req)),
                    sim.map.is_some(),
                    sim.movie.is_some(),
                    sim.over.is_some(),
                    sim.shop_done,
                    sim.select_next,
                    g.audio_que_debug(),
                    sim.message_box.is_some(),
                    g.globals.help,
                    g.globals.paused
                );
                if let Some(s) = world.get_resource::<Session>() {
                    eprintln!("DEBUG F2 session banner {:?} finished {:?} next {:?} ui_paused {}", s.banner, s.finished, s.next, world.resource::<Ui>().paused);
                }
                for (s, r) in g.items.records.iter().enumerate() {
                    let p = r.position();
                    let d = ((p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2) + (p[2] - at[2]).powi(2)).sqrt();
                    let def = &g.items.definitions[r.definition()];
                    if r.active() && (d < 6.0 || def.name().starts_with(b"GEM")) {
                        eprintln!(
                            "DEBUG F2 item {s} {} kind {} sub {} d {d:.2} pos {p:?} d8 {:#06x} dc {} de {} e2 {:#04x} f4 {:#06x} f6 {:#04x}",
                            String::from_utf8_lossy(def.name()),
                            def.kind(),
                            def.subtype(),
                            r.u16_(0xd8),
                            r.u8_(0xdc),
                            r.i8_(0xde),
                            r.u8_(0xe2),
                            r.u16_(0xf4),
                            r.u8_(0xf6)
                        );
                    }
                }
            }
            if world.resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::F1) {
                let mut ui = world.resource_mut::<Ui>();
                ui.details = !ui.details;
            }
            return;
        }
        if toggle && matches!(world.resource::<Front>().menu.page, gdl_client::menu::Page::Paused { .. }) {
            let before = shown_menu(world);
            world.resource_mut::<Front>().menu.cues.push(gdl_client::menu::Cue::Exit);
            world.resource_mut::<Ui>().paused = false;
            end_menu(world, before);
            return;
        }
    } else if world.resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape) && matches!(world.resource::<Front>().menu.page, gdl_client::menu::Page::PressStart) {
        // NATIVE: Escape on "Press Start" quits the client.
        world.write_message(AppExit::Success);
        return;
    }
    for k in keys {
        let before = shown_menu(world);
        let action = world.resource_mut::<Front>().menu.input(k);
        // end_all_optmenus (Manage Character, Shop), the inventory's
        // hide_optmenu and the title's Start: no fade or burn.
        let removes = matches!(action, Some(Action::ManageCharacter | Action::OpenShop | Action::OpenInventory | Action::StartSelect | Action::Play { .. } | Action::Create { .. } | Action::Import));
        if let Some(a) = action {
            act(world, a);
        }
        if !removes {
            end_menu(world, before);
        }
        if *world.resource::<Screen>() != screen {
            break;
        }
    }
    // The audio menu's sliders while left/right is held (vblank steps).
    if matches!(world.resource::<Front>().menu.page, gdl_client::menu::Page::Audio { .. }) {
        let (left, right) = menu_held(world);
        let action = {
            let mut front = world.resource_mut::<Front>();
            front.audio_clock += vb;
            let steps = front.audio_clock.floor() as i32;
            front.audio_clock -= steps as f32;
            front.menu.hold(left, right, steps)
        };
        if let Some(a) = action {
            act(world, a);
        }
    } else {
        world.resource_mut::<Front>().audio_clock = 0.0;
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
        fov: serde_json::from_value(pr["camera_fov"].clone())?,
        attention: vec3(&pr["attention_offset_810"])?,
    };
    let mut anim_paths = HashMap::new();
    for (name, v) in cfg["packages"].as_object().ok_or("config packages")? {
        if let Some(a) = v["anim"].as_str() {
            anim_paths.insert(name.clone(), a.to_string());
        }
    }
    let mut monster_packages = std::collections::HashSet::new();
    let mut monster_paths = Vec::new();
    for m in cfg["monsters"].as_array().ok_or("config monsters")? {
        let t = m["type"].as_i64().ok_or("monster type")? as i32;
        let name = m["package"].as_str().ok_or("monster package")?.to_string();
        let prefix = m["prefix"].as_str().ok_or("monster prefix (rerun run_play.py)")?.to_string();
        let path = anim_paths.get(&name).ok_or("monster package anim")?.clone();
        monster_paths.push((t, name.clone(), prefix, path));
        monster_packages.insert(name);
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
            monster_types: l["monster_types"]
                .as_array()
                .map(|a| a.iter().filter_map(|m| Some((m["type"].as_i64()? as i32, m["package"].as_str()?.to_string(), m["prefix"].as_str()?.to_string()))).collect())
                .unwrap_or_default(),
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
    // IRX/DCS.IRX (the IOP sound driver): its pan table.
    let dcs = match audio_cfg["dcs"].as_str() {
        Some(p) => Some(gdl_client::bounded_read(Path::new(p))?),
        None => None,
    };
    let mut audio = audio_rom.clone().map(|rom| gdl_client::audio::AudioOut::new(rom, paths("banks"), paths("streams"), dcs.as_deref()));
    if let Some(a) = audio.as_ref() {
        println!("{}", a.status);
    }
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
    // M19 enhancement settings beside the saves (GDL_BASELINE=1: the
    // original presentation).
    let settings = gdl_client::settings::Settings::load(&p("saves")?);
    let settings_dir = (std::env::var_os("GDL_BASELINE").is_none() && std::env::var_os("GDL_SETTINGS").is_none()).then(|| p("saves")).transpose()?;
    gdl_client::texture_pack::configure(settings.texture_pack.clone());
    if let Some(slot) = store.migrate_slot_zero()? {
        println!("save0000 (interim slot numbering) moved to save{slot:04}; the old file is kept as migrated-save0000");
    }
    // GameOpts: the stored block, else the captured session's GamePrefs
    // (sfx 128, music 128, stereo 1, …).
    let game_opts = store.load_options().unwrap_or_else(|| {
        let mut o = vec![0u8; gdl_core::save::GAME_OPTS_LEN];
        for (k, v) in [128i32, 128, 1, 0, 0, 1, 0, 0].iter().enumerate() {
            o[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
        }
        o
    });
    let raw = |k: usize| i32::from_le_bytes(game_opts[4 * k..4 * k + 4].try_into().unwrap());
    let word = |k: usize| raw(k).clamp(0, 255) as u8;
    let (sfx_v, music_v) = (word(0), word(1));
    // OptionsSetup at boot: AudioSetVolSfx(word 0), AudioSetStereo(word 2);
    // AudioSetVolMusic(word 1) goes to each level's simulation (build_game).
    if let Some(a) = audio.as_mut() {
        a.set_options(raw(0), raw(2));
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
    // snd.286 0x23d438: AudioVolSfx(0)'s two samples.
    let vol_sfx_samples = elf
        .symbol("snd.286")
        .and_then(|sym| elf.read(sym.address, 8).ok())
        .map_or([-1, -1], |b| [0, 1].map(|k| i32::from_le_bytes(b[4 * k..4 * k + 4].try_into().unwrap())));
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
        ending: MenuEnding::default(),
        sfx: Vec::new(),
        audio_clock: 0.0,
        vol_sfx_samples,
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
            monster_packages,
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
        .insert_resource(settings.clone())
        .insert_resource(gdl_client::controls::Bindings::from_settings(&settings.keys))
        .init_resource::<gdl_client::enhance::Overlay>()
        .insert_resource(SettingsDir(settings_dir))
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
        .add_plugins({
            let plugins = DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Gauntlet: Dark Legacy - INTERIM native build".into(),
                    resolution: (1280, 896).into(),
                    present_mode: if settings.vsync { bevy::window::PresentMode::AutoVsync } else { bevy::window::PresentMode::AutoNoVsync },
                    mode: if settings.fullscreen { bevy::window::WindowMode::BorderlessFullscreen(bevy::window::MonitorSelection::Current) } else { bevy::window::WindowMode::Windowed },
                    ..default()
                }),
                ..default()
            });
            // GDL_NO_GAMEPADS: automated runs (client_driver.py) ignore the
            // machine's physical controllers.
            if std::env::var_os("GDL_NO_GAMEPADS").is_some() { plugins.disable::<bevy::gilrs::GilrsPlugin>() } else { plugins }
        })
        .add_plugins(gdl_client::GsMaterialPlugin)
        .init_resource::<Ui>()
        .init_resource::<HudSets>()
        .init_resource::<MenuIcon>()
        .add_systems(Startup, (setup_static, setup_menu_icon, setup_overlay).chain())
        .insert_resource(TestHarness::from_env())
        .add_systems(Last, (frame_limiter, ui_trace, frame_log, diagnostic_shots, diagnostic_scene_dump))
        .add_systems(PostUpdate, interpolate_transforms.before(bevy::transform::TransformSystems::Propagate))
        .add_systems(FixedUpdate, (simulate, apply_rumble).chain().run_if(playing))
        .add_systems(
            Update,
            (
                enhancement_input,
                apply_settings,
                front_system,
                play_menu_cues,
                run_attract,
                ps2_viewport,
                (advance_level, refresh_characters, (present_world, present_see_thru).chain(), present_world_nodes, present_node_flags, (present_scene_light, present_point_lights).chain(), present_distance_fade, present_camera_face, present_effect_trees, present_item_trees, present_camera_face_local, spawn_tree_psys, follow_tree_psys, present_particles, present_texmods, hide_exploded_nodes, present_critters, present_critter_shadows, (present_sumner, present_special_texmods, present_hold_fx, present_familiars, present_powerup_trees, present_it_sign, present_hand_objects, present_movie, present_tree_alpha, present_skins, present_player_alpha, present_compass, present_player_shadow, apply_material_requests, present_weapon_trails, present_streaks).chain(), present_player).chain().run_if(playing),
                front_text,
                present_hud,
                present_menu_icon,
                present_saver,
                present_screen_adjust,
                present_overlay,
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

/// The level's enemy type → (package, tree prefix) (Sources::monsters).
fn level_monster_map(lv: &LevelSource) -> HashMap<i32, (String, String)> {
    lv.monster_types.iter().map(|(t, n, p)| (*t, (n.clone(), p.clone()))).collect()
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
        && session.next.is_none()
        && sim.game.globals.game_mode == 0x4010
        && sim.shop.is_none()
        && sim.shop_done.is_none()
        && sim.game.players[0].i32_(0xfc) == 0xb
        && !(0..4).any(|k| matches!(sim.game.players[k].i32_(0xfc), 1 | 4 | 8))
        && let Some(k) = session.levels.iter().position(|l| l.world == 0xd)
    {
        start_shop(&mut sim, &front, k);
    }
    // do_shop finished: init_player_select(2), then init_next_level into the
    // Tower; game_main goes to init_next_level at once when the shop was
    // skipped (skip_shop: nobody in state 1/5, e.g. every player dead).
    if let Some((k, skipped)) = sim.shop_done.take() {
        if skipped {
            session.next = Some(k);
            session.banner = Some((0.0, String::new()));
        } else {
            match sim.game.init_player_select(2, &session.pdats) {
                Ok(ps) => {
                    sim.select = Some(ps);
                    sim.select_next = Some(k);
                }
                Err(e) => sim.error = Some(format!("init_player_select: {}", e.0)),
            }
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
    // game_main's level end: do_players reported done and the voice queue
    // is quiet (Game::exit_ready, once per level) — a portal exit (do_exit's
    // countdown), the auto exit after a boss's good wizard, or everyone
    // dead; game_main_level_end picks the destination. level_done carries
    // only the F9 (DEBUG) destination.
    if session.banner.is_none()
        && session.finished.is_none()
        && sim.game.globals.game_mode == 0x4010
        && sim.shop.is_none()
        && sim.map.is_none()
        && sim.movie.is_none()
        && sim.over.is_none()
        && std::mem::take(&mut sim.game.exit_ready)
    {
        let wave = sim.game.level_done.take().unwrap_or(gdl_core::game::boss::TOWER_WAVE);
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
            let pkg = &sources.packages[&name];
            if !libraries.0.contains_key(&name) {
                libraries.0.insert(name.clone(), build_library(pkg, &mut meshes, &mut images, &mut materials));
            }
            if sources.monster_packages.contains(&name) && !frames.0.contains_key(&name) {
                frames.0.insert(name.clone(), enemy_frames(pkg));
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
    session.transmitter_table = g.transmitter_table.clone();
    session.check_num = sim.game.collision.check_num();
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
            sources.monsters = level_monster_map(&session.levels[idx]);
            init_hud(&mut game, &sources, &session, &front, &mut hud_sets, true);
            sim.game = game;
            for e in &old {
                commands.entity(e).despawn();
            }
            if let Ok(root) = root.single() {
                let n = sim.game.items.records.len();
                let level_lib = spawn_level(&mut commands, root, &session, n, &mut meshes, &mut images, &mut materials);
                libraries.0.insert(LEVEL_LIB.to_string(), level_lib);
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

/// The process start (the M20 loading time).
fn process_start() -> std::time::Instant {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *START.get_or_init(std::time::Instant::now)
}

fn main() {
    let _ = process_start();
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
