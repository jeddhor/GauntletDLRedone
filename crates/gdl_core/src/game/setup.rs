//! Native game construction and the per-frame step.
//!
//! INTERIM START (labelled): player records and session globals come from the
//! user's save (export_game_state.py v3); items are loaded fresh with the
//! verified AddItemInstList port; the enemy array starts empty. The frame
//! order follows game_main's 0x4010 path: world_update (SetupDynGrid),
//! do_items, do_enemies, do_players. Player motion still uses the interim
//! PlayerSim until the full PlayerMotion port lands; its pickups use the
//! ported PlayerCollideItems/PlayerItemCollision/GrabPowerup chain.
use super::enemy_ai::{init_actions, EnemyAnim, EnemyAnimData};
use super::pickups::ClassData;
use super::record::{Record, PLAYER};
use super::{enemies, Game, Globals, R};
use crate::animation_clock::{ClipTiming, ClockState};
use crate::collision::CollisionWorld;
use crate::item_collision::DynGrid;
use crate::items::{load_items, ItemSession, ItemTables};
use crate::rng::Rand;
use gdl_formats::animation::Animations;
use gdl_formats::elf::Elf;
use gdl_formats::wdata::WorldData;
use gdl_formats::world::World;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

type V3 = [f32; 3];

#[derive(Deserialize, Clone, Debug)]
pub struct StartCamera {
    pub basis: [V3; 3],
    pub distance: f32,
    pub planes: [f32; 6],
    pub newcam_mode: i32,
}

#[derive(Deserialize)]
pub struct StartExtra {
    pub item_active_dist: f32,
    pub mode_950: i32,
    pub globals_954: i32,
    pub start_enemyinfo: i32,
    pub random_item_num: u32,
    pub vb_counter: u32,
    pub mode_970: i32,
    pub stop_time: i32,
    pub boss_is_dying: i32,
    pub max_enemy_radius: f32,
    pub max_item_radius: f32,
    pub boss_crit_inst: i32,
    pub target_cone_mode: i32,
    pub randpottype: i32,
}

/// export_game_state.py v6.
#[derive(Deserialize)]
pub struct StartState {
    pub schema_version: u32,
    pub classes: Vec<Option<ClassData>>,
    pub players: Vec<String>,
    pub globals: Globals,
    pub extra: StartExtra,
    pub camera: StartCamera,
    pub enemy_load: enemies::EnemyLoad,
}

fn hex(s: &str) -> Result<Vec<u8>, String> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| e.to_string())).collect()
}

/// Clip timings of one animation tree, by sequence index.
pub fn clips_of(anim: &Animations, tree: usize) -> Vec<ClipTiming> {
    anim.trees[tree]
        .sequences
        .iter()
        .map(|s| ClipTiming { frames: s.frames, timing_factor: s.timing_factor, repeat: s.repeat, root_transfer_flags: s.root_transfer_flags })
        .collect()
}

/// Add a package's item trees in AtreeMatchAnyHeader order (the first
/// package defining a label keeps it); `nodes` gets per label whether the tree
/// has a type-1 node (AtreeNodeInit +0x2c: AnimateNode passes those to
/// DoAnimation) and its node count (AtreeInitSub's AnimInfo +0x38).
pub fn add_item_trees(lib: &mut HashMap<String, Vec<ClipTiming>>, nodes: &mut HashMap<String, (bool, i32)>, anim: &Animations) {
    for (k, t) in anim.trees.iter().enumerate() {
        if lib.contains_key(&t.label) {
            continue;
        }
        lib.insert(t.label.clone(), clips_of(anim, k));
        nodes.insert(t.label.clone(), (t.nodes.iter().any(|n| n.node_type == 1), t.nodes.len() as i32));
    }
}

/// EnemyDesc 0x136ff8 level suffixes 4..7: offset_step5[level + 0x94]
/// (ELF bytes at 0x22d858 = "ABSF"); other levels print as numbers.
pub const LEVEL_LETTERS: &[u8; 4] = b"ABSF";

/// EnemyDesc(type, level) tree label: "<PREFIX><level>" (level 0 → 1).
pub fn enemy_desc(prefix: &str, level: i32) -> String {
    let l = if level == 0 { 1 } else { level };
    if (4..=7).contains(&l) {
        format!("{prefix}{}", LEVEL_LETTERS[(l - 4) as usize] as char)
    } else {
        format!("{prefix}{l}")
    }
}

/// Enemy trees EnemyDesc(type, level) from a MONSTERS/<NAME>/ANIM.PS2 package.
pub fn enemy_trees(data: &mut EnemyAnimData, t: i32, prefix: &str, anim: &Animations) {
    for (k, tree) in anim.trees.iter().enumerate() {
        let s = tree.sequences.first();
        data.other_trees.insert((t, tree.label.clone()), (s.map_or(0, |s| s.frames), s.map_or(0, |s| s.timing_factor)));
        data.tree_index.insert((t, tree.label.clone()), k);
    }
    for (k, tree) in anim.trees.iter().enumerate() {
        let Some(level) = (1..=7).find(|&l| enemy_desc(prefix, l) == tree.label) else { continue };
        let labels: Vec<(String, i16)> = tree
            .sequences
            .iter()
            .map(|s| (s.label.clone(), i16::from_le_bytes([s.raw[0x20], s.raw[0x21]])))
            .collect();
        data.trees.insert((t, level), (clips_of(anim, k), init_actions(&labels)));
        data.tree_nodes.insert((t, level), tree.nodes.len() as i32);
        if tree.nodes.iter().any(|n| n.node_type == 1) {
            data.tree_animated.insert((t, level));
        }
    }
}

/// GauntWorldInit 0x162e38's LoadEnemy loop for the level's GetEnemyTypes:
/// types other than 0x1d, below 0x20 (critters 0x20..0x2c excluded), get
/// enemy_loaded = their subtype and a model when a tree package was supplied
/// (LoadModel success is assumed from that); genfx is 1 for loaded types
/// (InitEffects; observed for every G1 type). When the captured G1 table is
/// for the same level, any disagreement is reported in the frame log.
fn derive_enemy_load(types: &crate::level::EnemyTypes, anims: &EnemyAnimData, captured: &enemies::EnemyLoad, note: &mut Option<String>) -> enemies::EnemyLoad {
    let n = captured.model.len();
    let mut out = enemies::EnemyLoad { model: vec![-1; n], loaded: vec![0; n], genfx: vec![0; n] };
    for (k, &t) in types.kind.iter().enumerate() {
        if t < 0 || t == 0x1d || (0x20..=0x2c).contains(&t) || n <= t as usize {
            continue;
        }
        if anims.trees.keys().any(|(tt, _)| *tt == t) {
            out.model[t as usize] = 0x100 + k as i32;
            out.genfx[t as usize] = 1;
        }
        out.loaded[t as usize] = types.subtype[k];
    }
    let differ: Vec<usize> = (0..n)
        .filter(|&t| (0 <= out.model[t]) != (0 <= captured.model[t]) || out.loaded[t] != captured.loaded[t] || out.genfx[t] != captured.genfx[t])
        .collect();
    if !differ.is_empty() {
        *note = Some(format!("derived enemy_load differs from the captured G1 table for types {differ:?} (no package supplied, or another level)"));
    }
    out
}

/// A fresh animation clock that the first AnimateTree selection initialises.
pub fn fresh_clock() -> ClockState {
    ClockState {
        sequence: u16::MAX,
        previous_sequence: u16::MAX,
        reset_byte: 0,
        option_bits: 0,
        enabled_word: 0,
        loop_word: 0,
        mode_bits: 0xff,
        frame: 0.0,
        blend_fraction: 0.0,
        start_time: 0.0,
        blend_end_time: 0.0,
        period_scale: 1.0,
        last_time: 0.0,
    }
}

/// Inputs for [`Game::new_level`].
pub struct LevelInputs<'a> {
    pub world: &'a World,
    pub wdata: &'a WorldData,
    pub level: usize,
    pub elf: &'a Elf<'a>,
    pub start: StartState,
    pub enemy_anims: EnemyAnimData,
    /// Item animation trees by definition name.
    pub item_anims: HashMap<String, Vec<ClipTiming>>,
    /// Per item tree label: a type-1 node, and the node count (add_item_trees).
    pub item_anim_nodes: HashMap<String, (bool, i32)>,
    pub effect_tables: super::effects::EffectTables,
    /// AUDIO/AUDATPS2.ROM; None disables sound events.
    pub audio_rom: Option<gdl_formats::audio::AudioRom>,
    /// Object labels of every loaded package (OBJECTS definitions); None
    /// makes the paths that search them unsupported.
    pub mb_objects: Option<std::collections::HashSet<String>>,
    /// Critter files the level loads (LoadGauntWorldStart) with their model
    /// package ANIM.PS2 (bytes kept for key sampling).
    pub critters: Vec<super::critters::CritterInput>,
    /// NumInGamePlayers as GauntWorldInit's world_update sees it (before
    /// AddItemInstList's SetPlayerVars): 0 when the level is entered from the
    /// select screen (the Tower of a new or loaded game); None = the start
    /// state's.
    pub load_in_game_players: Option<i32>,
}

/// Whether a loaded critter file has a TYPE in CritterETypes[kind][0] (TYPE
/// +0x52 slot 0, DESC +0x20 kind).
fn critter_slot0_loaded(critters: &[super::critters::CritterInput], kind: i16) -> bool {
    critters.iter().any(|(_, wad, _)| {
        wad.types.iter().any(|t| {
            let d = i16::from_le_bytes([t[0x50], t[0x51]]);
            i16::from_le_bytes([t[0x52], t[0x53]]) == 0
                && usize::try_from(d).ok().and_then(|d| wad.descs.get(d)).is_some_and(|d| i16::from_le_bytes([d[0x20], d[0x21]]) == kind)
        })
    })
}

/// SetItem's Tower crystal test at load: DemoMode or
/// AnyPlayerHasAllCrystals(1) over the start records (state != 0; the class
/// record's crystal count +0xd0a + 2 (i16): < 0 delivered, == MAX_CRYSTALS[1]
/// complete, or the largest count >= MAX_CRYSTALS[1]).
fn tower_crystals_taken(players: &[String], elf: &Elf, demo_mode: i32) -> Result<bool, Box<dyn std::error::Error>> {
    if demo_mode != 0 {
        return Ok(true);
    }
    let max = elf.object("MAX_CRYSTALS")?.get(4..8).map(|b| i32::from_le_bytes(b.try_into().unwrap())).ok_or("MAX_CRYSTALS")?;
    let mut most = 0;
    for h in players {
        let b = hex(h)?;
        if b.len() < PLAYER {
            return Err("player record size".into());
        }
        let r = Record(b);
        if r.i32_(0xfc) == 0 {
            continue;
        }
        let c = i32::from(r.i16_(0xd0a + 0xb4 * r.u32_(0xc) as usize + 2));
        if c < 0 || c == max {
            return Ok(true);
        }
        most = most.max(c);
    }
    Ok(max <= most)
}

/// PlayerWaves(player, 8) & 1 over the start records with +0xfc != 0
/// (the wave byte +0x182c + class × 0xe + 8).
fn battle_wave_1_done(players: &[String]) -> Result<bool, Box<dyn std::error::Error>> {
    for h in players {
        let r = Record(hex(h)?);
        if r.0.len() == PLAYER && r.i32_(0xfc) != 0 && r.u8_(0x182c + 0xe * r.u32_(0xc) as usize + 8) & 1 != 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Per-enemy (visible, near-visible) query.
pub type Visibility = dyn Fn(&Game, usize) -> (bool, bool);

/// Frame report for logging: the first message of each unsupported path.
#[derive(Default, Debug)]
pub struct FrameLog {
    pub unsupported: BTreeMap<String, u64>,
}

impl FrameLog {
    pub fn note(&mut self, system: &str, e: &super::Unsupported) {
        *self.unsupported.entry(format!("{system}: {}", e.0)).or_insert(0) += 1;
    }
}

impl Game {
    pub fn new_level(inp: LevelInputs) -> Result<Self, Box<dyn std::error::Error>> {
        let level = inp.wdata.levels.get(inp.level).ok_or("level index")?;
        let s = &inp.start;
        if s.schema_version != 6 || s.players.len() != 4 {
            return Err("start state v6 expected".into());
        }
        let class0 = s.classes.first().cloned().flatten().ok_or("player 0 class data")?;
        let classes: [ClassData; 4] = std::array::from_fn(|k| s.classes.get(k).cloned().flatten().unwrap_or_else(|| class0.clone()));
        let g = &s.globals;
        let world_kind = if g.cur_world == 0xd { 0xd } else { 0 };
        let types = crate::level::enemy_types(inp.wdata, level, world_kind, s.extra.mode_950)?;
        // ResolveWorldData 0x161738: the runtime record (difficulty word 0x21edcc).
        let level_raw = crate::level::resolved_level(level, inp.elf, s.extra.target_cone_mode)?;
        let multipliers = crate::level::multipliers_cc(&level_raw);
        let session = ItemSession {
            multipliers,
            enemy_kind: types.kind,
            gen_kind: types.gen_kind,
            // LoadWorldData 0x161b98: BossType = level_data +0x44.
            boss_type: level.boss_type,
            cur_world: g.cur_world,
            num_virtual_players: g.num_virtual_players,
            game_mode: g.game_mode,
            global_958: g.global_958,
            global_970: s.extra.mode_970,
            random_item_num: s.extra.vb_counter,
            // enemy_anim[0x1d] / [0x1e] (0x233598 +0x74/+0x78): the type's
            // enemy package is loaded.
            golem_statue_anim: inp.enemy_anims.trees.keys().any(|k| k.0 == 0x1d),
            death_statue_anim: inp.enemy_anims.trees.keys().any(|k| k.0 == 0x1e),
            // CritterTypeLoaded(kind, 0): CritterETypes[kind][0], registered by
            // CritterAllocType when LoadGauntWorldStart loads the files (before
            // the items).
            critter_3_loaded: critter_slot0_loaded(&inp.critters, 3),
            critter_7_loaded: critter_slot0_loaded(&inp.critters, 7),
            tower_crystals_taken: tower_crystals_taken(&s.players, inp.elf, s.globals.demo_mode)?,
            battle_wave_1_done: battle_wave_1_done(&s.players)?,
            sound_names: inp.audio_rom.as_ref().map(|r| r.sounds.iter().zip(&r.ids).map(|((n, _), id)| (n.to_uppercase().into_bytes(), *id)).collect()).unwrap_or_default(),
            tower_new_player: g.cur_world == 0xd && {
                let recs: Vec<Record> = s.players.iter().map(|h| hex(h).map(Record)).collect::<Result<_, _>>()?;
                super::tower::tower_new_player(g.tower_counter, &recs)
            },
        };
        let mut collision = CollisionWorld::from_world(inp.world)?;
        let mut world_anims = match &inp.world.anims {
            Some(a) => super::world_anim::WorldAnimSet::new(inp.world.bytes.clone(), a),
            None => Default::default(),
        };
        // GauntWorldInit 0x163338: world_update before AddItemInstList. Its
        // DoWorldAnimation poses the moving floors the items are placed and
        // parented on (fixture: item 307's node local relative to object 821
        // holds the posed parent; 23 live items parented). InitWorldInfo's
        // 0x2000000 comes first (NewWorld). The rest of that world_update
        // (fog, start_of_stage, SetupDynGrid on no items) is not modelled
        // (HYPOTHESIS: nothing it changes reaches the simulation).
        let g0 = &s.globals;
        let animate = (0 < inp.load_in_game_players.unwrap_or(g0.num_in_game_players) || g0.game_mode & 0x8000 != 0)
            && 0x18694 < (g0.trig_cam_delay.wrapping_sub(0xb) as u32)
            && g0.paused == 0
            && g0.help == 0;
        let spf = g0.spf;
        let mut load_nodes = super::item_runtime::RecordedWobjNodes::default();
        let anim_objects = world_anims.anim_objects();
        let loaded = load_items(inp.world, &mut collision, ItemTables::from_elf(inp.elf)?, session, s.extra.vb_counter, &mut |w, col| {
            for e in world_anims.entries.iter().filter(|e| e.active) {
                if let Some(o) = w.objects.get_mut(e.object) {
                    o.flags_10 |= 0x200_0000;
                }
            }
            if animate {
                for k in 0..world_anims.entries.len() {
                    if world_anims.entries[k].active {
                        let mut explode = None;
                        super::world_anim::world_anim_sub(&mut world_anims, k, &mut w.objects, col, spf, false, &mut explode)
                            .map_err(|e| crate::items::UnsupportedItem(format!("load-time DoWorldAnimation: {}", e.0)))?;
                        if explode.is_some() {
                            return Err(crate::items::UnsupportedItem("load-time WorldObjectExplode".into()));
                        }
                    }
                }
            }
            // world_update's CreateDynobjGrid (after SetupDynGrid): the poses
            // AddItemInstList's PlaceItem tests matrix objects at.
            w.sync_collision(col);
            col.create_dynobj_grid().map_err(|e| crate::items::UnsupportedItem(format!("load-time CreateDynobjGrid: {e:?}")))?;
            Ok(())
        }, &mut |w, col| {
            // AddItemInstList's ProcessItemWobjs. OnWobj is 0 for every
            // object (inferred: no player or enemy has a floor in the new
            // level yet); SPF, start_of_stage and vb_elapsed are the start
            // state's (the load-time values are not captured; they matter
            // only for an elevator away from its target and for the fade
            // transparency). The node writes are replayed on the game below.
            // SetItem's kind-13 FindWorldAnimNode at the instance positions
            // and the load-time poses (nothing moves them before PlaceItem).
            for slot in 0..w.records.len() {
                w.attach_sound_node(slot, col, &anim_objects)?;
            }
            let ons = vec![0; w.wobjs.len()];
            let g = super::item_runtime::WobjGlobals { spf, start_of_stage: g0.start_of_stage != 0, vb_elapsed: g0.vb_elapsed };
            super::item_runtime::process_item_wobjs_sub(w, &ons, g, &mut load_nodes).map_err(|e| crate::items::UnsupportedItem(format!("load-time ProcessItemWobjs: {}", e.0)))?;
            w.sync_collision(col);
            Ok(())
        })?;
        // GauntWorldInit's SetupDynGrid/CreateDynobjGrid after AddItemInstList.
        loaded.world.sync_collision(&mut collision);
        collision.create_dynobj_grid().map_err(|e| format!("CreateDynobjGrid: {e:?}"))?;
        let mut players: [Record; 4] = std::array::from_fn(|_| Record::zeroed(PLAYER));
        for (k, h) in s.players.iter().enumerate() {
            let b = hex(h)?;
            if b.len() != PLAYER {
                return Err("player record size".into());
            }
            players[k] = Record(b);
        }
        let floats = |n: &str| -> Result<Vec<f32>, Box<dyn std::error::Error>> {
            Ok(inp.elf.object(n)?.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect())
        };
        let ints = |n: &str| -> Result<Vec<i32>, Box<dyn std::error::Error>> {
            Ok(inp.elf.object(n)?.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect())
        };
        let grid = DynGrid::new(inp.world.min, inp.world.max, s.extra.max_enemy_radius, s.extra.max_item_radius)?;
        let item_count = loaded.world.records.len();
        let mut load_note = None;
        let mut globals = s.globals.clone();
        // GauntWorldInit 0x162e90: WldBtm = world min y − 4.5.
        globals.world_bottom = crate::numeric::sub_guarded_rz(inp.world.min[1], 4.5)?;
        globals.boss_type = level.boss_type;
        let mut game = Game {
            globals,
            level: level_raw.clone(),
            players,
            enemies: enemies::new_enemy_records(),
            items: loaded.world,
            grid,
            collision,
            rng: Rand::new(s.extra.vb_counter),
            events: Vec::new(),
            enemy_tables: enemies::EnemyTables::from_elf(inp.elf)?,
            enemy_load: derive_enemy_load(&types, &inp.enemy_anims, &s.enemy_load, &mut load_note),
            no_enemy_slot: false,
            stop_time: s.extra.stop_time,
            max_stop_time: 0.0,
            sumner_hint_time: 0.0,
            do_rock_fly: 0,
            did_rock_fly: 0,
            world_num_levels: inp.wdata.levels.len() as i32,
            tower: super::tower::TowerState::default(),
            hud: match super::hud::HudTables::from_elf(inp.elf) {
                Ok(t) => super::hud::Hud::new(t),
                Err(e) => {
                    load_note = Some(format!("player panel tables: {e}"));
                    super::hud::Hud::default()
                }
            },
            item_alpha: std::collections::HashMap::new(),
            item_trees: std::collections::HashMap::new(),
            speech_tables: ["crystal_sidx", "garg_sidx", "shard_sidx"]
                .into_iter()
                .map(|n| (n, inp.elf.object(n).map(|b| b.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()).unwrap_or_default()))
                .collect(),
            world_anims,
            world_order: inp.elf.object("WorldOrder").map(|b| b.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()).unwrap_or_default(),
            thermometer: Default::default(),
            world_crystal: inp.elf.object("WorldCrystal").map(|b| b.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()).unwrap_or_default(),
            old_enemy_shrink: 1.0,
            boss_is_dying: s.extra.boss_is_dying,
            suicide_bomber_index: -1,
            enemy_wall_hit: None,
            did_action: false,
            death_touch: false,
            fwrdval: vec![0.0; 34],
            angoff07: floats("angoff07")?,
            angoff00: floats("angoff00")?,
            offset_step5: floats("offset_step5")?,
            enemy_action_priority: ints("e_actpri")?,
            faceang: 0.0,
            frame_main: 0,
            enemy_wall_point: [0.0; 3],
            enemy_wall_contact: None,
            enemy_anims: vec![None; 25],
            enemy_node: vec![None; 25],
            player_clips: Default::default(),
            player_floor_parent: [None; 4],
            player_team_parent: [None; 4],
            player_rigs: Default::default(),
            player_node: [None; 4],
            transporter_floor_y: None,
            player_water: None,
            partner_parent: None,
            quest_weapon_sfx: -1,
            quest_wall_sfx: -1,
            quest_item_timer: 0.0,
            combo_effects: inp.elf.object("ComboEffects").map(|b| b.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()).unwrap_or_default(),
            item_anims: vec![None; item_count],
            item_anim_lib: inp.item_anims.clone(),
            item_anim_nodes: inp.item_anim_nodes.clone(),
            camera: None,
            camera_rest: None,
            std_camera: {
                let set = super::camera::build_transmitters(&inp.world.locators, g.cur_world).map_err(|e| format!("transmitters: {}", e.0))?;
                match inp.wdata.cameras.get(level.camera_index as usize) {
                    Some(params) => {
                        let mut cam = super::camera::Camera::new(super::camera::CamConsts::from_elf(inp.elf)?, set, params, inp.world.min, inp.world.max)
                            .map_err(|e| format!("camera: {}", e.0))?;
                        cam.cur3d = g.cur3d_transmitter;
                        Some(cam)
                    }
                    None => None,
                }
            },
            boss_camera: Some(super::camera::BossCamera::new(
                super::camera::BossCamConsts::from_elf(inp.elf)?,
                usize::try_from(level.boss_camera_index).ok().and_then(|k| inp.wdata.boss_cameras.get(k)).cloned(),
                g.cur_transmitter,
                &super::camera::CamConsts::from_elf(inp.elf)?,
            )),
            world_start_time: g.time,
            trig_cam: Default::default(),
            tower_cams: Default::default(),
            disable_player_controls: 0,
            vibes: Default::default(),
            rumble: Vec::new(),
            vibe_fresh: [false; 4],
            // InitPlayerControls state; the front end carries it across levels.
            controls: Default::default(),
            control_options: [[0; 4]; 4],
            control_tables: Some(super::pad::ControlTables::from_elf(inp.elf)?),
            item_active_dist: s.extra.item_active_dist,
            mode_950: s.extra.mode_950,
            globals_954: s.extra.globals_954,
            start_enemyinfo: s.extra.start_enemyinfo != 0,
            exit_state: 0,
            enemy_anim_data: inp.enemy_anims,
            classes,
            class_tables: crate::player_class::ClassTables::from_elf(inp.elf)?,
            effects: super::effects::Effects::new(inp.effect_tables),
            boss_crit_inst: s.extra.boss_crit_inst,
            boss_stale: None,
            target_cone_mode: s.extra.target_cone_mode,
            level_done: None,
            players_done: false,
            exit_ready: false,
            map_preload: -1,
            vb_counter: s.extra.vb_counter,
            audio_que: Default::default(),
            attract_audio: false,
            randpottype: s.extra.randpottype,
            active_player_mask: 0,
            enemy_alpha: vec![0xff; 25],
            skin_views: Default::default(),
            health_meters: Default::default(),
            sfx_ene_bloodidx: 0,
            gen_hit_effects: [-1; 45],
            gen_die_effects: [-1; 45],
            lock_cam_req: 0,
            critters: Vec::new(),
            critter_data: super::critters::CritterData {
                files: inp
                    .critters
                    .into_iter()
                    .map(|(name, wad, model)| {
                        let n = wad.types.len();
                        super::critters::CritterFile { name, wad, trees: vec![None; n], model }
                    })
                    .collect(),
                ..Default::default()
            },
            critter_new_id: 1,
            boss_start_pos: [0.0; 3],
            critter_counter: 0,
            critter_nactive: 0,
            critter_ntargets: [0.0; 4],
            boss_activate_time: 0.0,
            boss_cam_mode: 0,
            safe_rock_num: 0,
            safe_rock_idx: -1,
            safe_rock_items: Vec::new(),
            safe_rock_counter: Vec::new(),
            boss_key_position: None,
            good_wiz: super::goodwiz::GoodWiz { clips: inp.item_anims.get("WIZARD").cloned(), ..Default::default() },
            worldsel: super::goodwiz::worldsel_from_elf(inp.elf),
            auto_exit_timer: 0,
            spew_items: Vec::new(),
            stage_save: None,
            wave_timer: Default::default(),
            bg_duck: Default::default(),
            bg_music: Default::default(),
            secret_char: inp.elf.object("secret_char")?.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect(),
            boss1_items: {
                let b = inp.elf.object("boss1_items")?;
                b.chunks_exact(0x1c)
                    .take(4)
                    .map(|r| {
                        let w = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
                        let name = r[..0x10].iter().copied().take_while(|&c| c != 0).collect();
                        (name, w(0x10) as i32, w(0x14), w(0x18))
                    })
                    .collect()
            },
            player_grab: [None; 4],
            mb_objects: inp.mb_objects,
            lookouts: super::lookouts::build(&inp.world.locators),
            milestones: Vec::new(),
            player_start: inp.world.locators.iter().find(|l| l.kind == 7 && l.id == 0).map_or(([0.0; 3], 0.0), |l| (l.position, l.rotation[1])),
            player_init_start: inp.world.locators.iter().find(|l| l.kind == 7 && l.id == 0).map_or([0.0; 3], |l| l.position),
            world_labels: inp.world.nodes.iter().map(|n| n.label.clone()).collect(),
            node_mb_set: vec![0; inp.world.nodes.len()],
            node_mb_clear: vec![0; inp.world.nodes.len()],
            node_alpha: vec![None; inp.world.nodes.len()],
            node_state_version: 0,
            lighting: Default::default(),
            player_light: super::lighting::PlayerLightTables::from_elf(inp.elf)?,
            enemy_slot_names: (0..8)
                .map(|k| {
                    let name = types.data[k].and_then(|d| inp.wdata.enemies.get(d)).map(|e| {
                        let b = &e.raw[0x10..0x18];
                        String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(8)]).into_owned()
                    });
                    (types.kind[k], name.unwrap_or_default())
                })
                .collect(),
            world_dir: inp.wdata.world_dir.clone().unwrap_or_default(),
            sounds: {
                let slots: Vec<(i32, i32, String)> = (0..8)
                    .map(|k| {
                        let name = types.data[k].and_then(|d| inp.wdata.enemies.get(d)).map_or(String::new(), |e| e.name.clone());
                        (types.kind[k], types.subtype[k], name)
                    })
                    .collect();
                let snds: Vec<(String, i16, i16)> = inp.wdata.sounds.iter().map(|w| (w.name.clone(), w.volume, w.priority)).collect();
                let stage = inp.wdata.stages.get(inp.level).map(|a| super::sound::Stage {
                    bank: a.bank.clone(),
                    word_10: a.word_10,
                    enter_sound: a.enter_sound,
                    word_14: a.word_14,
                    stream: a.stream.clone(),
                    areas: a.areas,
                    word_2a: a.word_2a,
                    parts: a.parts.clone(),
                });
                super::sound::SoundTables::new(inp.elf, inp.audio_rom, &slots, level.boss_type, super::bgmusic::level_letter(g.cur_world), &snds, stage)?
            },
            log: FrameLog::default(),
        };
        // AddItemInstList's ProcessItemWobjs node writes (fade objects).
        load_nodes.apply(&mut game);
        // init_next_level: InitLighting(1); GauntWorldInit's lightning.
        game.init_lighting();
        game.init_lightning().map_err(|e| format!("lightning: {}", e.0))?;
        // InitWorldInfo: objects driven by a world animation get flag 0x2000000.
        for e in game.world_anims.entries.iter().filter(|e| e.active) {
            if let Some(o) = game.items.objects.get_mut(e.object) {
                o.flags_10 |= 0x200_0000;
            }
        }
        // TowerInit (init_next_level, before the items) and the crystals
        // SetItem hid for a new player (tree alpha 0xff).
        game.tower.new_player = game.items.session.tower_new_player;
        game.tower.crystal_active_time = if game.tower.new_player { 0.0 } else { -1.0 };
        for slot in std::mem::take(&mut game.items.alpha_full) {
            game.item_alpha.insert(slot, 0xff);
        }
        // init_next_level: WelcomeFlag = 0 outside DemoMode.
        if game.globals.demo_mode == 0 {
            game.globals.welcome_flag = 0;
        }
        // GauntWorldInit → SumnerInit (Tower: the GWIZ tree).
        if game.globals.cur_world == 0xd
            && let Some(clips) = inp.item_anims.get("GWIZ")
        {
            game.sumner_init(clips.clone(), inp.item_anims.get("WIZARD").cloned()).map_err(|e| format!("SumnerInit: {}", e.0))?;
        }
        // SetItem's opened Tower triggers: their world animation at the end.
        for o in std::mem::take(&mut game.items.wanim_to_end) {
            if let Some(k) = game.find_wobj_wanim(o) {
                let e = &mut game.world_anims.entries[k];
                e.frame = (i32::from(e.frames) - 1) as f32;
            }
        }
        // AddItemInstList calls SetPlayerVars before placing the items.
        game.set_player_vars();
        // CritterLoadAllTypes / CritterInitAllMoves, then AddLocatorInstList's
        // boss locators (kind 6, skipped in game_mode 0x400b).
        // GauntWorldInit: InitEffects (the generator effects are the first
        // custom effects, 0x61..) before CritterInitAllMoves.
        game.init_gen_effects();
        game.critter_load_types().map_err(|e| format!("critter types: {}", e.0))?;
        if game.globals.game_mode != 0x400b {
            for loc in inp.world.locators.iter().filter(|l| l.kind == 6) {
                let m = super::enemy_ai::create_ypr_matrix(loc.rotation).map_err(|e| format!("boss locator: {}", e.0))?;
                let mut mat = [0.0f32; 16];
                for r in 0..3 {
                    mat[4 * r..4 * r + 4].copy_from_slice(&m[r]);
                }
                mat[12..15].copy_from_slice(&loc.position);
                mat[15] = 1.0;
                game.add_boss(mat).map_err(|e| format!("AddBoss: {}", e.0))?;
            }
        }
        for slot in 0..item_count {
            game.set_item_geo(slot).map_err(|e| format!("SetItemGeo: {}", e.0))?;
        }
        if let Some(n) = load_note {
            game.log.unsupported.insert(n, 0);
        }
        // The culling view comes from the StdCam once start_camera runs
        // (init_next_level: StdCamInit after the players are placed).
        game.camera_rest = Some(s.camera.clone());
        // GauntWorldInit 0x162e38: QuestItemState 1 for the first active
        // player holding this world's legend item (PlayerHasLegendItem).
        game.globals.quest_item_state = 0;
        game.quest_item_timer = 0.0;
        if game.globals.boss_type < 0x2b {
            for k in 0..4 {
                if !matches!(game.players[k].i32_(0xfc), 1 | 3 | 5) {
                    continue;
                }
                let has = game.globals.quest_item_state == 0 && game.player_has_legend_item(k, game.globals.cur_world);
                game.players[k].set_i32(0x80c, i32::from(has));
                if has {
                    game.globals.quest_item_state = 1;
                }
            }
        }
        // AddLocatorInstList: milestones (kind 5, at most 0x80; FatalErrorC past it).
        for l in inp.world.locators.iter().filter(|l| l.kind == 5) {
            if 0x80 <= game.milestones.len() {
                return Err("more than 0x80 milestones (FatalErrorC)".into());
            }
            let (y, _) = game.collision.floor_pos(l.position[1], f32::from_bits(0x3e4c_cccd), l.position, 0)?;
            let y = crate::numeric::add_guarded_rz(y, 0.5)?;
            game.milestones.push([l.position[0], y, l.position[2]]);
        }
        // AddLocatorInstList: LinkTriggerToCam for the kind-9 locators.
        game.link_trigger_cameras().map_err(|e| format!("LinkTriggerToCam: {}", e.0))?;
        if game.globals.cur_world == 0xd {
            // init_next_level → EnterTower after the items are placed.
            game.enter_tower().map_err(|e| format!("EnterTower: {}", e.0))?;
        }
        // init_next_level: BGMusicStart, init_wave_timer, init_thermometer.
        game.bg_music_start();
        game.init_wave_timer().map_err(|e| format!("init_wave_timer: {}", e.0))?;
        game.init_thermometer(inp.world.min, inp.world.max);
        // init_next_level: RuneDispTimer = 300 outside world 0xc (the rune
        // stones show at the level start; show_crystals waits for it).
        if game.globals.cur_world != 0xc {
            game.globals.rune_disp_timer = 300;
        }
        Ok(game)
    }

    /// SetEnemyObj's animation part: tree EnemyDesc(type, level) and InitActions.
    pub fn attach_enemy_anim(&mut self, i: usize, t: i32, level: i32) -> R<()> {
        let lvl = if level == 0 { 1 } else { level };
        let Some((clips, table)) = self.enemy_anim_data.trees.get(&(t, lvl)).cloned() else {
            return super::unsupported(format!("no animation tree for enemy type {t} level {lvl}"));
        };
        for (k, (idx, v)) in table.iter().enumerate() {
            self.enemies[i].set_i32(0xe8 + 8 * k, *idx);
            self.enemies[i].set_i32(0xec + 8 * k, *v);
        }
        let mut anim = EnemyAnim { clock: fresh_clock(), clips, current: 0, tree: (t, lvl) };
        // AtreeInit's InitAnimInfo 0x110750 zeroes the AnimInfo (sequence and
        // previous sequence 0) and runs InitAnim(0); generate_enemy's
        // InitAnim(START) on success then leaves the previous sequence 0
        // (trace-fight-g1-v3 steps 198/474).
        anim.clock.sequence = 0;
        anim.clock.previous_sequence = 0;
        self.enemy_anims[i] = Some(anim);
        self.enemy_init_anim(i, 0)?;
        self.enemies[i].set_u32(0x80, 1); // atree present (native marker)
        // AtreeInitSub's InitAnimInfo: AnimInfo +0x28 = 1.0 (+0xac); the
        // atree's +0x3c node count (+0xbc).
        let nodes = self.enemy_anim_data.tree_nodes.get(&(t, lvl)).copied().unwrap_or(0);
        self.enemies[i].set_f32(0xac, 1.0);
        self.enemies[i].set_i32(0xbc, nodes);
        Ok(())
    }

    /// InitAnim(sequence) on an enemy's clock, with its AnimInfo mirrors:
    /// +0x90 the tree's sequence count, +0x92 the sequence, +0x94 its frame
    /// count (trace-fight-g1-v3: type 13 levels 1/2/3 hold 9/12/10, type 12
    /// holds 7).
    pub(crate) fn enemy_init_anim(&mut self, i: usize, seq: u16) -> R<()> {
        let (now, scale) = (self.globals.atime, self.globals.anim_time_scale);
        let Some(anim) = self.enemy_anims.get_mut(i).and_then(|a| a.as_mut()) else { return Ok(()) };
        let Some(clip) = anim.clips.get(seq as usize).copied() else { return Ok(()) };
        anim.clock = crate::animation_clock::initialize(
            anim.clock,
            crate::animation_clock::ClockInit {
                sequence: seq,
                frames: clip.frames,
                timing_factor: clip.timing_factor,
                repeat: clip.repeat,
                start_frame: 0,
                enabled_word: 1,
                playback_scale: 1.0,
                blend_duration: 0.0,
                now,
                anim_time_scale: scale,
            },
        )?;
        anim.current = seq;
        let count = anim.clips.len() as i16;
        let r = &mut self.enemies[i];
        r.set_i16(0x90, count);
        r.set_u16(0x92, seq);
        r.set_i16(0x94, clip.frames);
        Ok(())
    }

    /// One game update without players (headless runs).
    pub fn step(&mut self, visible_override: Option<&Visibility>) {
        self.step_with(visible_override, &mut |_| Ok(()));
    }

    /// One game update (game_main 0x4010 order): world_update, do_items,
    /// do_enemies, do_players (`players`), ProcessEffects.
    pub fn step_with(&mut self, visible_override: Option<&Visibility>, players: &mut dyn FnMut(&mut Game) -> R<()>) {
        self.frame_main += 1;
        self.vibe_fresh = [false; 4];
        // main: AudioQueUpdate / AudioPoll before game_main.
        self.main_audio(self.globals.vb_elapsed);
        // main: DoLighting after AudioQueUpdate/AudioPoll, before game_main.
        self.do_lighting();
        // MBResetBlits after the previous frame's render: temp blits and
        // font messages are per frame.
        self.hud.begin_frame();
        // game_main: SetPlayerVars every frame (StopTime, EnemyShrink,
        // ActivePlayerMask, the +0x93e panel-written bits, player counts)
        // after the quit/restart checks, before the mode dispatch.
        self.set_player_vars();
        let spf = self.globals.spf;
        self.globals.time += spf;
        self.globals.atime += spf;
        self.globals.itime = (self.globals.time * 1000.0) as i32;
        // game_main: do_auto_exit before world_update.
        self.do_auto_exit();
        // world_update 0x163480: start_enemyinfo = start_of_stage == 0 or
        // level_data +0x48; then it returns at once while paused or in help;
        // DoWorldAnimation, SetupDynGrid and CreateDynobjGrid come first.
        self.start_enemyinfo = self.globals.start_of_stage == 0 || self.level.get(0x48..0x4c).is_some_and(|b| b != [0; 4]);
        if self.globals.paused == 0 && self.globals.help == 0 {
            if let Err(e) = self.do_world_animation() {
                self.log.note("world_update", &e);
            }
            self.sync_collision();
            if let Err(e) = self.setup_dyn_grid() {
                self.log.note("world_update", &e);
            }
            if let Err(e) = self.boss_world_update() {
                self.log.note("world_update", &e);
            }
            if let Err(e) = self.quest_item_update() {
                self.log.note("world_update", &e);
            }
            if let Err(e) = self.world_update_stage_start() {
                self.log.note("world_update", &e);
            }
            if let Err(e) = self.world_update_lightning() {
                self.log.note("world_update", &e);
            }
            if let Err(e) = self.world_update_end_of_stage() {
                self.log.note("world_update", &e);
            }
        }
        if let Err(e) = self.process_items() {
            self.log.note("do_items", &e);
        }
        let camera_visible = |g: &Game, k: usize| -> (bool, bool) {
            let Some(cam) = &g.camera else { return (true, true) };
            let rec = &g.enemies[k];
            let r = rec.f32_(enemies::e::WIDTH) + rec.f32_(enemies::e::WIDTH);
            let p = rec.v3(enemies::e::ATTENTION);
            (cam.sphere_visible(r, p).unwrap_or(false), cam.sphere_visible(r + 15.0, p).unwrap_or(false))
        };
        let vis: &Visibility = match visible_override {
            Some(f) => f,
            None => &camera_visible,
        };
        if let Err(e) = self.do_enemies(vis) {
            self.log.note("do_enemies", &e);
        }
        // do_players: the welcome block first.
        self.welcome_check();
        if let Err(e) = self.tower_check_messages(false) {
            self.log.note("do_players", &e);
        }
        if let Err(e) = players(self) {
            self.log.note("do_players", &e);
        }
        // game_main: do_players done and AudioStillPlaying clear → the
        // level's end (the host's transition); ProcessEffects is skipped.
        if self.players_done && !self.audio_still_playing() {
            self.exit_ready = true;
        } else if let Err(e) = self.process_effects() {
            self.log.note("ProcessEffects", &e);
        }
        // main's camera dispatch (StdCamUpdate) and MBEndFrame.
        if let Err(e) = self.camera_step() {
            self.log.note("camera", &e);
        }
        // game_main: do_wave_timer; modify_thermometer before world_update
        // (drawn here).
        if let Err(e) = self.do_wave_timer() {
            self.log.note("do_wave_timer", &e);
        }
        self.modify_thermometer();
        // game_main: BGMusicUpdate.
        self.bg_music_update();
    }

    /// do_enemies with the camera's visibility tests (the flyby's update).
    pub(crate) fn do_enemies_camera(&mut self) -> R<()> {
        let camera_visible = |g: &Game, k: usize| -> (bool, bool) {
            let Some(cam) = &g.camera else { return (true, true) };
            let rec = &g.enemies[k];
            let r = rec.f32_(enemies::e::WIDTH) + rec.f32_(enemies::e::WIDTH);
            let p = rec.v3(enemies::e::ATTENTION);
            (cam.sphere_visible(r, p).unwrap_or(false), cam.sphere_visible(r + 15.0, p).unwrap_or(false))
        };
        self.do_enemies(&camera_visible)
    }

    /// Copy the item system's runtime object state into the collision world.
    /// The original's collision reads the object words (+0x10 flags, +0x35
    /// exclusion, the node y) live, so every writer syncs before the next
    /// query in the same update.
    pub fn sync_collision(&mut self) {
        self.items.sync_collision(&mut self.collision);
    }

    /// sync_collision for one object.
    pub(crate) fn sync_collision_object(&mut self, o: usize) {
        self.collision.runtime_flags[o] = self.items.objects[o].flags_10;
        self.collision.runtime_exclusion[o] = self.items.object_exclusion[o];
        if self.items.wobjs.iter().any(|w| w.object == o) {
            self.collision.node_y[o] = Some(self.items.object_node_y[o]);
        }
    }

    /// The pickup part of PlayerMotion/do_players for the interim player:
    /// PlayerCollideItems at the new position, then GrabPowerup.
    pub fn player_pickups(&mut self, player: usize, from: V3) -> R<()> {
        let to = self.players[player].v3(0x50);
        let r = self.players[player].f32_(0x828);
        let h = self.players[player].f32_(0x82c);
        let near = self.grid.items_near(&self.items, r, to)?;
        let env = self.collide_env();
        let mut best: Option<(usize, f32)> = None;
        for slot in near {
            if crate::item_collision::player_skip_item(&self.items, &self.items.records[slot]) {
                continue;
            }
            let d = crate::item_collision::item_coll_sub(&self.items, &mut self.collision, &env, slot, from, to, None, r, h)?;
            if d < 0.0 {
                continue;
            }
            let res = self.player_item_collision(player, slot)?;
            let k = self.items.definitions[self.items.records[slot].definition()].kind();
            if res != 0 && (k == 9 || k == 0xb) {
                self.players[player].set_u32(0x884, slot as u32 + 1);
            }
            if best.is_none_or(|(_, bd)| d < bd) {
                best = Some((slot, d));
            }
        }
        self.players[player].set_u32(0x880, best.map_or(0, |(s, _)| s as u32 + 1));
        if let Some(g) = self.players[player].u32_(0x888).checked_sub(1) {
            if self.players[player].i32_(0xfc) == 1 {
                let class = self.classes[player].clone();
                self.grab_powerup(player, g as usize, &class)?;
            }
            self.players[player].set_u32(0x888, 0);
        }
        Ok(())
    }
}
