//! INTERIM PLAYTEST, not the M12 slice: walk the Yellow Sorceress through the
//! Poison Fields with original geometry, textures and animation clips.
//!
//! Native and reference-compared: control tables, motion code/run timer,
//! displacement request, facing turn, static wall/floor collision and position.
//! Interim/approximate (labelled on screen): start state copied from the user's
//! save, observed code→action pairs, matrix rebuild, presentation-only animation
//! clock and transitions, static rest-state follow camera (no camera logic),
//! diagnostic unlit materials, no enemies/items/damage/projectiles/audio/HUD.
use bevy::{core_pipeline::tonemapping::Tonemapping, prelude::*, sprite::Anchor};
use gdl_core::{
    collision::CollisionWorld,
    controls::{direction_levels, tables_from_elf, JoyTables},
    player_animation::{choose, ActionContext},
    player_sim::PlayerSim,
};
use gdl_formats::{animation::Animations, objects::Objects, texdef::TextureTable, world::World};
use std::{error::Error, path::Path};

/// Presentation-only handedness mirror: the original camera basis satisfies
/// right × up = forward, so original coordinates are mirrored on x for Bevy.
const MIRROR: Vec3 = Vec3::new(-1.0, 1.0, 1.0);

struct Package {
    objects: Objects,
    table: TextureTable,
    textures: Vec<u8>,
}

#[derive(Resource)]
struct Sources {
    level: Package,
    world: World,
    character: Package,
    rig: Animations,
    clips: Animations,
    clip_bytes: Vec<u8>,
}

#[derive(Resource)]
struct Game {
    sim: PlayerSim,
    collision: CollisionWorld,
    tables: JoyTables,
    previous_levels: u32,
    last: String,
    error: Option<String>,
    updates: u64,
}

#[derive(Resource)]
struct Presentation {
    action_map: Vec<[i64; 2]>,
    fov: f32,
    basis: [Vec3; 3],
    distance: f32,
    attention: Vec3,
}

#[derive(Resource)]
struct AnimState {
    action: u32,
    requested: u32,
    clip: usize,
    frame: f32,
}

#[derive(Component)]
struct CharacterRoot;
#[derive(Component)]
struct RigNode(usize);
#[derive(Component)]
struct FollowCamera;
#[derive(Component)]
struct Hud;

fn package(paths: &[std::ffi::OsString]) -> Result<Package, Box<dyn Error>> {
    Ok(Package {
        objects: Objects::parse(&gdl_client::bounded_read(Path::new(&paths[0]))?)?,
        table: TextureTable::parse(&gdl_client::bounded_read(Path::new(&paths[1]))?)?,
        textures: gdl_client::bounded_read(Path::new(&paths[2]))?,
    })
}

fn setup(
    mut commands: Commands,
    sources: Res<Sources>,
    presentation: Res<Presentation>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let root = commands.spawn((Transform::from_scale(MIRROR), Visibility::default(), Name::new("mirror root"))).id();
    // Level geometry: original translation-only placements (as the inspector).
    let level = &sources.level;
    let (cache, unknown, _) = gdl_client::materials_for(&level.objects, &level.table, &level.textures, &mut images, &mut materials, true);
    let (parts, _) = gdl_client::object_meshes(&level.objects, &cache, &unknown, &mut meshes);
    let names: std::collections::HashMap<_, _> =
        level.objects.definitions.iter().map(|d| (d.label.as_str(), d.object_index as u32)).collect();
    if let Ok(placements) = sources.world.placements() {
        for p in placements {
            let node = &sources.world.nodes[p.node_index as usize];
            if let Some(list) = names.get(node.label.as_str()).and_then(|i| parts.get(i)) {
                for (mesh, material) in list {
                    let child = commands
                        .spawn((
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::from_translation(Vec3::from_array(p.summed_position)),
                        ))
                        .id();
                    commands.entity(root).add_child(child);
                }
            }
        }
    }
    // Character: YEL00 parts on SOR nodes, labels joined as in the rig inspector.
    let character = &sources.character;
    let (cache, unknown, _) =
        gdl_client::materials_for(&character.objects, &character.table, &character.textures, &mut images, &mut materials, true);
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
        Transform::from_xyz(-620.0, 390.0, 0.0),
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

fn simulate(
    keys: Res<ButtonInput<KeyCode>>,
    mut game: ResMut<Game>,
    mut anim: ResMut<AnimState>,
    sources: Res<Sources>,
    presentation: Res<Presentation>,
) {
    if game.error.is_some() {
        return;
    }
    let levels = keyboard_levels(&keys);
    let edges = levels & !game.previous_levels;
    game.previous_levels = levels;
    let Game { sim, collision, tables, .. } = &mut *game;
    match sim.step(collision, tables, levels, edges) {
        Ok(report) => {
            game.updates += 1;
            game.last = format!(
                "update {} | code {} | requested action {} | floor {} wall {}",
                game.updates, report.motion_code, report.requested_action, report.floor_result, report.wall_result
            );
        }
        Err(e) => {
            game.error = Some(e.to_string());
            return;
        }
    }
    advance_animation(&mut game.sim, &mut anim, &sources, &presentation);
}

/// Presentation clock: original clip timing factor and SPF, native choose()
/// for transitions; blending, events and CalcAnimInfo state are not modelled.
fn advance_animation(sim: &mut PlayerSim, anim: &mut AnimState, sources: &Sources, presentation: &Presentation) {
    let clips = &sources.clips.trees[0].sequences;
    let frames = f32::from(clips[anim.clip].frames.max(1));
    let mut choose_now = sim.requested_action != anim.requested;
    if !choose_now {
        let period = gdl_core::animation::frame_period(clips[anim.clip].timing_factor, 1.0, sim.spf).unwrap_or(sim.spf);
        anim.frame += sim.spf / period;
        if anim.frame >= frames {
            anim.frame -= frames;
            choose_now = true;
        }
    }
    if choose_now {
        let context = ActionContext {
            current_action: anim.action,
            requested_action: sim.requested_action,
            frame: anim.frame,
            flags_130: 0,
            flags_134: 0,
            flags_138: sim.flags_138,
            flags_8ac: sim.flags_8ac,
            status_93c: sim.status_93c,
            idle_counter_1d8: 0,
            idle_counter_1da: 0,
            word_80c: sim.word_80c,
        };
        let action = choose(context).map_or(sim.requested_action, |s| s.action);
        if action != anim.action {
            anim.frame = 0.0;
        }
        anim.action = action;
        anim.requested = sim.requested_action;
        if let Some(&[clip, _]) = presentation.action_map.get(action as usize)
            && clip >= 0
            && (clip as usize) < clips.len()
        {
            anim.clip = clip as usize;
        }
    }
    sim.current_action = anim.action;
}

type BodyQuery<'w, 's> = Query<'w, 's, &'static mut Transform, (With<CharacterRoot>, Without<RigNode>, Without<FollowCamera>)>;
type NodeQuery<'w, 's> = Query<'w, 's, (&'static RigNode, &'static mut Transform), (Without<CharacterRoot>, Without<FollowCamera>)>;
type CameraQuery<'w, 's> =
    Query<'w, 's, &'static mut Transform, (With<FollowCamera>, Without<CharacterRoot>, Without<RigNode>)>;

#[allow(clippy::too_many_arguments)]
fn present(
    game: Res<Game>,
    anim: Res<AnimState>,
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
    let s = &game.sim;
    let m = s.matrix;
    let world = Mat4::from_cols_array(&[
        m[0][0], m[0][1], m[0][2], 0.0, m[1][0], m[1][1], m[1][2], 0.0, m[2][0], m[2][1], m[2][2], 0.0,
        s.position[0], s.position[1], s.position[2], 1.0,
    ]);
    if let Ok(mut t) = body.single_mut() {
        *t = Transform::from_matrix(world);
    }
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
        let clip = &sources.clips.trees[0].sequences[anim.clip];
        text.0 = format!(
            "INTERIM PLAYTEST (not the M12 slice) - Yellow Sorceress, The Poison Fields\n\
             Arrows/WASD: move   Space/K: attack (animation only; no projectile/damage yet)   Esc: quit\n\
             Native & reference-compared: controls, run timer, movement request, facing, static wall/floor collision\n\
             Interim: start state from your save, action map, matrix rebuild, animation clock, rest-state camera, unlit materials, alpha cutout\n\
             {}\naction {} clip {} {} frame {:.1}/{}{}",
            game.last,
            anim.action,
            clip.index,
            clip.label,
            anim.frame,
            clip.frames,
            game.error.as_ref().map_or(String::new(), |e| format!("\nSIMULATION STOPPED (unsupported original branch): {e}"))
        );
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    // gdl_play START.json SLUS_200.47 LEVEL_OBJECTS WORLDS LEVEL_TEXDEF LEVEL_TEXTURES
    //           CHAR_OBJECTS RIG_ANIM CLIP_ANIM CHAR_TEXDEF CHAR_TEXTURES
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 11 {
        return Err("Usage: gdl_play START.json SLUS_200.47 OBJECTS WORLDS TEXDEF TEXTURES CHAR_OBJECTS RIG_ANIM CLIP_ANIM CHAR_TEXDEF CHAR_TEXTURES".into());
    }
    let start: serde_json::Value = serde_json::from_slice(&gdl_client::bounded_read(Path::new(&args[0]))?)?;
    let sim: PlayerSim = serde_json::from_value(start["sim"].clone())?;
    let p = &start["presentation"];
    let row = |r: &serde_json::Value| -> Result<Vec3, Box<dyn Error>> {
        let v: [f32; 3] = serde_json::from_value(r.clone())?;
        Ok(Vec3::from_array(v))
    };
    let presentation = Presentation {
        action_map: serde_json::from_value(p["action_map"].clone())?,
        basis: [row(&p["camera_basis"][0])?, row(&p["camera_basis"][1])?, row(&p["camera_basis"][2])?],
        distance: serde_json::from_value(p["camera_distance"].clone())?,
        fov: serde_json::from_value(p["camera_fov"].clone())?,
        attention: row(&p["attention_offset_810"])?,
    };
    let elf_bytes = gdl_client::bounded_read(Path::new(&args[1]))?;
    let tables = tables_from_elf(&gdl_formats::elf::Elf::parse(&elf_bytes)?)?;
    let world_bytes = gdl_client::bounded_read(Path::new(&args[3]))?;
    let world = World::parse(&world_bytes)?;
    let collision = CollisionWorld::from_world(&world)?;
    let level = package(&[args[2].clone(), args[4].clone(), args[5].clone()])?;
    let character = package(&[args[6].clone(), args[9].clone(), args[10].clone()])?;
    let rig = Animations::parse(&gdl_client::bounded_read(Path::new(&args[7]))?)?;
    let clip_bytes = gdl_client::bounded_read(Path::new(&args[8]))?;
    let clips = Animations::parse(&clip_bytes)?;
    if rig.trees.len() != 1 || clips.trees.len() != 1 || rig.trees[0].nodes.len() != clips.trees[0].nodes.len() {
        return Err("character rig and clip trees must correspond".into());
    }
    let initial_action = sim.current_action;
    let clip = presentation
        .action_map
        .get(initial_action as usize)
        .map_or(0, |[c, _]| (*c).max(0) as usize);
    App::new()
        .insert_resource(Sources { level, world, character, rig, clips, clip_bytes })
        .insert_resource(Game { sim, collision, tables, previous_levels: 0, last: String::new(), error: None, updates: 0 })
        .insert_resource(AnimState { action: initial_action, requested: initial_action, clip, frame: 0.0 })
        .insert_resource(presentation)
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
        .add_systems(Update, present)
        .run();
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
