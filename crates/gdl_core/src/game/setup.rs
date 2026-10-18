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
use super::item_runtime::{Camera, ItemAnim};
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
    for tree in &anim.trees {
        let s = tree.sequences.first();
        data.other_trees.insert((t, tree.label.clone()), (s.map_or(0, |s| s.frames), s.map_or(0, |s| s.timing_factor)));
    }
    for (k, tree) in anim.trees.iter().enumerate() {
        let Some(level) = (1..=7).find(|&l| enemy_desc(prefix, l) == tree.label) else { continue };
        let labels: Vec<(String, i16)> = tree
            .sequences
            .iter()
            .map(|s| (s.label.clone(), i16::from_le_bytes([s.raw[0x20], s.raw[0x21]])))
            .collect();
        data.trees.insert((t, level), (clips_of(anim, k), init_actions(&labels)));
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
    pub effect_tables: super::effects::EffectTables,
    /// AUDIO/AUDATPS2.ROM; None disables sound events.
    pub audio_rom: Option<gdl_formats::audio::AudioRom>,
    /// Object labels of every loaded package (OBJECTS definitions); None
    /// makes the paths that search them unsupported.
    pub mb_objects: Option<std::collections::HashSet<String>>,
    /// Critter files the level loads (LoadGauntWorldStart) with their model
    /// package ANIM.PS2 (bytes kept for key sampling).
    pub critters: Vec<super::critters::CritterInput>,
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
        };
        let mut collision = CollisionWorld::from_world(inp.world)?;
        let loaded = load_items(inp.world, &mut collision, ItemTables::from_elf(inp.elf)?, session, s.extra.vb_counter)?;
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
            world_num_levels: inp.wdata.levels.len() as i32,
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
            item_anims: vec![None; item_count],
            camera: None,
            camera_rest: None,
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
            target_cone_mode: s.extra.target_cone_mode,
            level_done: None,
            randpottype: s.extra.randpottype,
            active_player_mask: 0,
            enemy_alpha: vec![0xff; 25],
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
            good_wiz_timer: 0.0,
            good_wiz_alpha: 0,
            auto_exit_timer: 0,
            spew_items: Vec::new(),
            player_grab: [None; 4],
            mb_objects: inp.mb_objects,
            lookouts: super::lookouts::build(&inp.world.locators),
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
                    stream: a.stream.clone(),
                    areas: a.areas,
                    word_2a: a.word_2a,
                    parts: a.parts.clone(),
                });
                super::sound::SoundTables::new(inp.elf, inp.audio_rom, &slots, level.boss_type, &snds, stage)?
            },
            log: FrameLog::default(),
        };
        // AddItemInstList calls SetPlayerVars before placing the items.
        game.set_player_vars();
        // CritterLoadAllTypes / CritterInitAllMoves, then AddLocatorInstList's
        // boss locators (kind 6, skipped in game_mode 0x400b).
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
            let name = String::from_utf8_lossy(game.items.definitions[game.items.records[slot].definition()].name()).into_owned();
            if let Some(clips) = inp.item_anims.get(&name) {
                game.item_anims[slot] = Some(ItemAnim { clock: fresh_clock(), clips: clips.clone(), current: 0 });
            }
        }
        if let Some(n) = load_note {
            game.log.unsupported.insert(n, 0);
        }
        game.update_camera(&s.camera);
        game.camera_rest = Some(s.camera.clone());
        if game.globals.cur_world == 0xd {
            // init_next_level → EnterTower after the items are placed.
            game.enter_tower().map_err(|e| format!("EnterTower: {}", e.0))?;
        }
        Ok(game)
    }

    /// StdCamera rest pose (hypothesis until do_camera is ported):
    /// target = player 0 position + attention offset, eye = target − forward × distance.
    pub fn update_camera(&mut self, c: &StartCamera) {
        // HYPOTHESIS (co-op): the average of the players in state 1 (player
        // 0 alone otherwise); the original's multi-player camera is not ported.
        let active: Vec<usize> = (0..4).filter(|&k| self.players[k].i32_(0xfc) == 1).collect();
        let pick: Vec<usize> = if active.len() < 2 { vec![0] } else { active };
        let n = pick.len() as f32;
        let mut target = [0.0f32; 3];
        for &k in &pick {
            let p = self.players[k].v3(0x50);
            let a = self.players[k].v3(0x810);
            for i in 0..3 {
                target[i] += (p[i] + a[i]) / n;
            }
        }
        let f = c.basis[2];
        let eye = [target[0] - f[0] * c.distance, target[1] - f[1] * c.distance, target[2] - f[2] * c.distance];
        self.camera = Some(Camera { basis: c.basis, eye, target, planes: c.planes, newcam_mode: c.newcam_mode });
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
        // generate_enemy: InitAnim(START) when the type has one.
        let start = table.get(1).map_or(-1, |e| e.0);
        let first = if 0 <= start { start as u16 } else { 0 };
        if let Some(clip) = anim.clips.get(first as usize).copied() {
            anim.clock = crate::animation_clock::initialize(
                anim.clock,
                crate::animation_clock::ClockInit {
                    sequence: first,
                    frames: clip.frames,
                    timing_factor: clip.timing_factor,
                    repeat: clip.repeat,
                    start_frame: 0,
                    enabled_word: 1,
                    playback_scale: 1.0,
                    blend_duration: 0.0,
                    now: self.globals.atime,
                    anim_time_scale: self.globals.anim_time_scale,
                },
            )?;
            anim.current = first;
        }
        self.enemies[i].set_u32(0x80, 1); // atree present (native marker)
        self.enemy_anims[i] = Some(anim);
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
        let spf = self.globals.spf;
        self.globals.time += spf;
        self.globals.atime += spf;
        self.globals.itime = (self.globals.time * 1000.0) as i32;
        self.sync_collision();
        self.do_auto_exit();
        if let Err(e) = self.boss_world_update() {
            self.log.note("world_update", &e);
        }
        if let Err(e) = self.setup_dyn_grid() {
            self.log.note("world_update", &e);
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
        if let Err(e) = players(self) {
            self.log.note("do_players", &e);
        }
        if let Err(e) = self.process_effects() {
            self.log.note("ProcessEffects", &e);
        }
        // HYPOTHESIS (do_camera not ported): the camera follows player 0's
        // rest pose after each update; the next update's visibility tests
        // (placed enemies, item activation) use it.
        if let Some(c) = self.camera_rest.clone() {
            self.update_camera(&c);
        }
    }

    /// Copy the item system's runtime object state into the collision world.
    pub fn sync_collision(&mut self) {
        for o in 0..self.collision.objects.len() {
            self.collision.runtime_flags[o] = self.items.objects[o].flags_10;
            self.collision.runtime_exclusion[o] = self.items.object_exclusion[o];
        }
        for w in &self.items.wobjs {
            self.collision.node_y[w.object] = Some(self.items.object_node_y[w.object]);
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
