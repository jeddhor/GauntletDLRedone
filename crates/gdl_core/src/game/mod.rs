//! Native gameplay state transcribed from game_main 0x125280 (game_mode
//! 0x4010): world_update, do_items, do_enemies, enemy_update, do_players,
//! ProcessEffects. Records keep their original layouts (see `record`);
//! presentation side effects (help text, audio, FX, HUD notices, vibration)
//! are emitted as [`Event`]s instead of being performed.
pub mod boss;
pub mod critter_ai;
pub mod critter_hits;
pub mod critters;
pub mod damage;
pub mod effects;
pub mod enemies;
pub mod enemy_ai;
pub mod item_runtime;
pub mod pickups;
pub mod players;
pub mod progress;
pub mod record;
pub mod setup;
pub mod sound;

use crate::collision::{CollisionWorld, UnsupportedCollision};
use crate::item_collision::DynGrid;
use crate::items::{ItemWorld, UnsupportedItem};
use crate::numeric::UnsupportedNumericDomain;
use crate::rng::Rand;
use record::Record;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub struct Unsupported(pub String);
impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported gameplay path: {}", self.0)
    }
}
impl std::error::Error for Unsupported {}
impl From<UnsupportedNumericDomain> for Unsupported {
    fn from(e: UnsupportedNumericDomain) -> Self {
        Self(format!("numeric domain: {e:?}"))
    }
}
impl From<UnsupportedCollision> for Unsupported {
    fn from(e: UnsupportedCollision) -> Self {
        Self(e.to_string())
    }
}
impl From<UnsupportedItem> for Unsupported {
    fn from(e: UnsupportedItem) -> Self {
        Self(e.0)
    }
}
pub type R<T> = Result<T, Unsupported>;
pub fn unsupported<T>(s: impl Into<String>) -> R<T> {
    Err(Unsupported(s.into()))
}

/// Session globals (original symbol names in comments). Values are read from
/// the fixture or set by the native level start; none are invented defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Globals {
    /// Time 0x20f904 (seconds), SPF 0x20f908, ITime 0x20f910, vb_elapsed 0x20f91c.
    pub time: f32,
    pub spf: f32,
    pub itime: i32,
    pub vb_elapsed: i32,
    pub game_mode: u32,
    pub cur_world: i32,
    pub cur_level: i32,
    pub boss_type: i32,
    pub boss_index: i32,
    pub boss_active: i32,
    pub boss_is_dead: i32,
    pub good_wizard: i32,
    pub quest_item_state: u32,
    pub world_bottom: f32,
    /// AuxLevels 0x210788 (low word).
    pub aux_levels: u32,
    /// Options 0x20f948 first word (difficulty consulted by damage_player).
    pub options: i32,
    /// 0x20f94c, the Options word read by the potion-less magic help check.
    pub options_4: u32,
    pub num_active_players: i32,
    pub num_in_game_players: i32,
    pub num_virtual_players: i32,
    pub num_visible_enemies: i32,
    /// 0x20f958: forced player count for show_item.
    pub global_958: i32,
    pub enemy_shrink: f32,
    pub secret_world: i32,
    pub max_keys_per_player: i32,
    pub max_potions_per_player: i32,
    pub trig_cam_active: i32,
    pub start_of_stage: i32,
    pub end_of_stage: i32,
    pub cam_wait: i32,
    pub paused: i32,
    pub help: i32,
    pub skip_wave: i32,
    pub quit_level: i32,
    pub demo_mode: i32,
    pub active_players: i32,
    pub auto_exit_flag: i32,
    pub someone_wants_to_exit: i32,
    pub firstgetidx: i32,
    pub it: i32,
    pub rune_disp_timer: i32,
    pub wave_timer: f32,
    pub wave_complete: i32,
    pub max_level_coins: i32,
    /// 0x21edd4: player-vs-player damage mode (2 enables PlayerDamagePlayer).
    pub pvp_mode: i32,
    /// MboxWinPtr camera forward x/z (+0x90/+0x98) read by PlayerMotion.
    pub camera_90: f32,
    pub camera_98: f32,
    /// ATime 0x20f914 and AnimTimeScale 0x2bc3a0.
    pub atime: f32,
    pub anim_time_scale: f32,
}

/// Presentation side effects requested by the simulation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Event {
    /// NewHelp(id, player): help/hint text; player usize::MAX = −1 (all).
    Help { id: i32, player: usize },
    /// add_got_it(player, kind, amount): HUD pickup notice.
    GotIt { player: usize, kind: i32, amount: i32 },
    /// Audio* calls, by original function name (cues not yet transcribed).
    Audio { cue: &'static str, player: Option<usize>, arg: i32 },
    /// A sound request (AudioSFX/SFX2/SFX3/queue): ID = bank << 16 | call.
    Sound { id: i32, position: Option<[f32; 3]>, volume: i32, priority: i32, queue: bool },
    /// AudioLOOP(id, position, volume, priority): a looping sound.
    LoopSound { id: i32, position: [f32; 3], volume: i32, priority: i32 },
    /// BossDeath's boss key effect (BossKeySfx) at a position.
    BossKey { position: [f32; 3] },
    /// AudioSuicideYellKill-style stop of every playing instance of an ID.
    StopSound { id: i32 },
    /// Expired item (ExpireItem 0x1537b0) with its fade mode.
    ItemExpired { slot: usize, mode: i32 },
    Vibrate { player: usize, strength: i32, time: i32 },
    PlayerDied { player: usize },
    /// StartFX* of EffectHeaders[header] at a position (hit sparks etc.).
    Effect { header: usize, position: [f32; 3] },
    /// AddExp request from PlayerDamagedEnemy/Item (applied by add_exp).
    Exp { player: usize, amount: i32, attack: bool },
    /// ModifyExp raised the player's level.
    LevelUp { player: usize, level: i32 },
    /// Item model/tree swap (generator level `level`, GEN_<prefix><level>).
    ItemModel { slot: usize, level: u8 },
    /// StartGenHitFx (destroyed = GenDieEffects).
    GeneratorHit { slot: usize, destroyed: bool },
}

pub struct Game {
    pub globals: Globals,
    /// LEVL record of the current level (level_data 0x23e158 points to it).
    pub level: Vec<u8>,
    pub players: [Record; 4],
    pub enemies: Vec<Record>,
    pub items: ItemWorld,
    pub grid: DynGrid,
    pub collision: CollisionWorld,
    pub rng: Rand,
    pub events: Vec<Event>,
    pub enemy_tables: enemies::EnemyTables,
    pub enemy_load: enemies::EnemyLoad,
    /// no_enemy_slot 0x23c74c, StopTime, BossIsDying, SuicideBomberIndex.
    pub no_enemy_slot: bool,
    pub stop_time: i32,
    pub boss_is_dying: i32,
    pub suicide_bomber_index: i32,
    /// enemy_wall_colobj after EnemyWallCollide.
    pub enemy_wall_hit: Option<usize>,
    /// did_action (set when a player damages an enemy).
    pub did_action: bool,
    /// FWRDVAL 0x2c15b8: per-type step this update (do_enemies).
    pub fwrdval: Vec<f32>,
    /// angoff07 0x22d910 (8 floats) and e_actpri 0x23f950 (33 ints).
    pub angoff07: Vec<f32>,
    /// angoff00 (move_logic00 probe offsets).
    pub angoff00: Vec<f32>,
    /// offset_step5 0x22d7c0 (move_logic18 weave offsets, first 16 floats).
    pub offset_step5: Vec<f32>,
    pub enemy_action_priority: Vec<i32>,
    /// faceang 0x2c166c and frame_main 0x220110.
    pub faceang: f32,
    pub frame_main: i32,
    /// enemy_wall_collp 0x2c1878 and the wall_col record of the last EnemyWallCollide.
    pub enemy_wall_point: [f32; 3],
    pub enemy_wall_contact: Option<crate::collision::Hit>,
    pub enemy_anims: Vec<Option<enemy_ai::EnemyAnim>>,
    pub item_anims: Vec<Option<item_runtime::ItemAnim>>,
    pub camera: Option<item_runtime::Camera>,
    /// ITEM_ACTIVE_DIST 0x234364, global 0x20f950 (mode), 0x20f954, start_enemyinfo, ExitState.
    pub item_active_dist: f32,
    pub mode_950: i32,
    pub globals_954: i32,
    pub start_enemyinfo: bool,
    pub exit_state: i32,
    pub enemy_anim_data: enemy_ai::EnemyAnimData,
    pub class: pickups::ClassData,
    pub effects: effects::Effects,
    /// BossCritInst 0x25d2c0 and the targeting-cone global 0x21edcc.
    pub boss_crit_inst: i32,
    pub target_cone_mode: i32,
    /// Destination wave (world << 8 | level) once a player finished do_exit.
    pub level_done: Option<i32>,
    /// randpottype 0x221540: rotating colour for untyped potions.
    pub randpottype: i32,
    /// ActivePlayerMask 0x220154 (SetPlayerVars 0x125ef8).
    pub active_player_mask: u32,
    /// CritterInsts (index = instance slot) and the loaded critter types.
    pub critters: Vec<Option<critters::Critter>>,
    pub critter_data: critters::CritterData,
    /// CritterNewID, BossStartPos.
    pub critter_new_id: u16,
    pub boss_start_pos: [f32; 3],
    /// CritterCounter, CritterNActivePlayers, CritterPlayerNTargets.
    pub critter_counter: u16,
    pub critter_nactive: usize,
    pub critter_ntargets: [f32; 4],
    /// BossActivateTime, BossCamMode, SafeRockNum/Idx, SafeRockItemIdx,
    /// SafeRockActiveCounter.
    pub boss_activate_time: f32,
    pub boss_cam_mode: i32,
    pub safe_rock_num: i32,
    pub safe_rock_idx: i32,
    pub safe_rock_items: Vec<usize>,
    pub safe_rock_counter: Vec<f32>,
    /// good_wiz_timer, good_wiz_alpha, auto_exit_timer (vblanks).
    pub good_wiz_timer: f32,
    pub good_wiz_alpha: i32,
    pub auto_exit_timer: i32,
    /// Players parented to a critter node by a grab: (critter, node, offset).
    pub player_grab: [Option<critters::Grab>; 4],
    /// SpewItems (item slot, velocity), at most 0x20.
    pub spew_items: Vec<(Option<usize>, [f32; 3])>,
    /// Where BossDeath placed the boss key effect.
    pub boss_key_position: Option<[f32; 3]>,
    /// LockCamReq 0x23f2c4: camera-lock request (camera logic not ported; 0).
    pub lock_cam_req: i32,
    /// Enemy tree alpha (presentation; see enemy_mod_alpha).
    pub enemy_alpha: Vec<i32>,
    /// Labels of the loaded MBOX object tables (MBOX_FindObject_Sub).
    pub mb_objects: Option<std::collections::HashSet<String>>,
    pub sounds: sound::SoundTables,
    pub log: setup::FrameLog,
}

impl Game {
    pub fn level_f32(&self, o: usize) -> f32 {
        f32::from_le_bytes(self.level[o..o + 4].try_into().unwrap())
    }
    pub fn level_u32(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.level[o..o + 4].try_into().unwrap())
    }
    pub fn help(&mut self, id: i32, player: usize) {
        self.events.push(Event::Help { id, player });
    }
    pub fn audio(&mut self, cue: &'static str, player: Option<usize>, arg: i32) {
        self.events.push(Event::Audio { cue, player, arg });
    }
    pub fn got_it(&mut self, player: usize, kind: i32, amount: i32) {
        self.events.push(Event::GotIt { player, kind, amount });
    }
}
