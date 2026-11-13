//! Native gameplay state transcribed from game_main 0x125280 (game_mode
//! 0x4010): world_update, do_items, do_enemies, enemy_update, do_players,
//! ProcessEffects. Records keep their original layouts (see `record`);
//! presentation side effects (help text, audio, FX, HUD notices, vibration)
//! are emitted as [`Event`]s instead of being performed.
pub mod attract;
pub mod audio_queue;
pub mod bgmusic;
pub mod boss;
pub mod camera;
pub mod critter_ai;
pub mod critter_hits;
pub mod critter_move;
pub mod critters;
pub mod doplayers;
pub mod damage;
pub mod effects;
pub mod exitflow;
pub mod flyby;
pub mod movie;
pub mod goodwiz;
pub mod hints;
pub mod heal;
pub mod healthmeter;
pub mod hud;
pub mod optmenu;
pub mod select;
pub mod enemies;
pub mod enemy_ai;
pub mod item_runtime;
pub mod lighting;
pub mod mapscreen;
pub mod lookouts;
pub mod pad;
pub mod pcontact;
pub mod pmotion;
pub mod paction;
pub mod pickups;
pub mod players;
pub mod powerups;
pub mod profile;
pub mod pselect;
pub mod progress;
pub mod psfx;
pub mod record;
pub mod setup;
pub mod shopflow;
pub mod skin;
pub mod thermometer;
pub mod wavetimer;
pub mod tower;
pub mod world_anim;
pub mod sound;
pub mod sound_cues;
pub mod stage;
pub mod teammove;
pub mod weaptrail;
pub mod seethru;
pub mod saver;
pub mod stageinfo;

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
    /// `?` sites report where the refused domain was met.
    #[track_caller]
    fn from(e: UnsupportedNumericDomain) -> Self {
        let at = std::panic::Location::caller();
        Self(format!("numeric domain: {e:?} at {}:{}", at.file(), at.line()))
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
fn minus_one() -> i32 {
    -1
}
fn tower_world() -> i32 {
    0xd
}
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
    /// 0x20f960 (Options +0x18): GetPlayerDMotion's debug-move switch.
    #[serde(default)]
    pub options_18: u32,
    pub num_active_players: i32,
    pub num_in_game_players: i32,
    /// trig_cam_delay 0x25d544 (DoWorldAnimation gate); serde default 0.
    #[serde(default)]
    pub trig_cam_delay: i32,
    /// TowerCounter 0x23f41c (Tower entries this session; EnterTower adds 1),
    /// WelcomeFlag (init_next_level: 0 unless DemoMode; −1 once handled),
    /// LastFinishedWorld/Level (GarmMessage). Carried across levels.
    #[serde(default)]
    pub tower_counter: i32,
    /// Cur3DTransmitter 0x21f120 / CurTransmitter 0x21f11c: never reset, so
    /// the previous level's index carries over (ELF initial value −1).
    #[serde(default = "minus_one")]
    pub cur3d_transmitter: i32,
    #[serde(default = "minus_one")]
    pub cur_transmitter: i32,
    /// Transmitters 0x234500 as the levels so far left it (camera.rs
    /// `transmitter_table`): entries past a level's count are earlier levels'.
    /// Empty: the boot state (the ELF's zero bytes).
    #[serde(default)]
    pub transmitter_table: Vec<camera::Transmitter>,
    /// NumEnemies 0x233590: the enemy slots in use. ResetEnemies sets it to
    /// LEVL +0x8e, init_game_cam (and InitCamera) to the level's CAMS
    /// record +0x34 — 25 in most levels, 20 in J4 and J6 (their captures).
    /// do_enemies, find_enemy_slot, SetupDynGrid, DamageItem's generator
    /// links, OnWobj, activate_player and set_suicide_bomber_index loop to
    /// it. 0 (older start states): all records.
    #[serde(default)]
    pub num_enemies: i32,
    /// CheckNum 0x21f698 (WorldCollide's and ItemObjectCol's query counter)
    /// as the previous level left it: never reset, so it carries over (the
    /// sweep's captures, all reached from B1's exit, sit at native's load
    /// count + 2). The triangle stamps are a fresh array per level.
    #[serde(default)]
    pub check_num: u8,
    #[serde(default)]
    pub welcome_flag: i32,
    #[serde(default)]
    pub last_finished_world: i32,
    #[serde(default)]
    pub last_finished_level: i32,
    /// LastWorldPlayed 0x23e190 / LastLevelPlayed 0x23e194: the last
    /// non-Tower level loaded (LoadGauntWorldStart); init_titlescreen sets
    /// 0xd / 0.
    #[serde(default = "tower_world")]
    pub last_world_played: i32,
    #[serde(default)]
    pub last_level_played: i32,
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
    /// codes_enabled 0x22019c (0 in the ELF; Hidden entries with +0x20
    /// need > 1).
    #[serde(default)]
    pub codes_enabled: i32,
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

/// An object DamageItem leaves where an item was destroyed:
/// MBOX_NewObject(name, item node, the node's parent, 0x80800) — a new node
/// under the item's parent with the item node's local matrix (MBNewObject →
/// MBNewNode copies it), so it stays when the item is deleted. BAREXP0
/// (SCORCHED_EARTH) and BARPOI0 (GREEN_BARREL_UNDERSPLAT) are ground decals
/// under exploded barrels; CHESTGEXP0 / CHESTSEXP0 a burst chest's remains;
/// ITEMEXP0 an item destroyed by a blast. Here: the item record's world
/// matrix at that moment. APPROXIMATION: under a moving floor the original's
/// node would ride the floor (its parent); this one stays put.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ItemDebris {
    pub name: &'static str,
    pub rows: [[f32; 3]; 3],
    pub position: [f32; 3],
}

/// Presentation side effects requested by the simulation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Event {
    /// NewHelp(id, player): help/hint text; player usize::MAX = −1 (all).
    Help { id: i32, player: usize },
    /// Sumner's hint animation starts / the HintMenu opens (tower UI).
    SumnerAnim { player: usize },
    HintMenu { player: usize },
    /// HideExits disabled a tower exit (the inactive model is presentation).
    ExitHidden { slot: usize },
    /// A world object hidden by WorldObjectExplode (presentation).
    ObjectHidden { object: usize },
    /// ScrollMessageBox(player mask, -1, text, -1): line `text` of the
    /// level's SCROLLS<code> entry (SetScrollLevelMsgList); the original
    /// pauses the game while the box is shown.
    ScrollMessage { player_mask: u32, text: i32 },
    /// ScrollMessageBox(mask, FindScrollMessage(name), line, audio): a named
    /// SCROLL_E.ROM entry, one line or every line (None) as pages.
    ScrollNamed { player_mask: u32, name: &'static str, line: Option<usize>, audio: i32 },
    /// inactivate_player's clear_player(k, 1): the host rebuilds the slot's
    /// record (it holds every class's PDAT).
    PlayerInactivated { player: usize },
    /// add_got_it(player, kind, amount): HUD pickup notice.
    GotIt { player: usize, kind: i32, amount: i32 },
    /// A sound request (AudioSFX/SFX2/SFX3/queue): ID = bank << 16 | call.
    /// `pan`: AudioTrack4's pan word — AudioAng of the position (−0x100..
    /// 0xff, 0x7f centre) or the fixed pan (AudioSFX2, the queue).
    Sound { id: i32, position: Option<[f32; 3]>, volume: i32, priority: i32, pan: i32 },
    /// change_player: player `player`'s class, colour or descriptor
    /// changed (the host swaps the model; replace_player_geo).
    PlayerModel { player: usize },
    /// AudioLOOP(id, position, volume, priority): a looping sound (pan
    /// from AudioAng).
    LoopSound { id: i32, position: Option<[f32; 3]>, volume: i32, priority: i32, pan: i32 },
    /// BossDeath's boss key effect (BossKeySfx) at a position.
    BossKey { position: [f32; 3] },
    /// AudioSuicideYellKill-style stop of every playing instance of an ID.
    StopSound { id: i32 },
    /// AudioStreamPlay: a STREAMS/<name> stage music part (looping or once).
    Music { name: String, volume: i32, looping: bool },
    /// DoItemSound: item `slot`'s looping sound (started or its volume and
    /// pan updated; one voice per item).
    /// `pan`: AudioAng(position), sent each update (AudioTrackPan).
    ItemSound { slot: usize, id: i32, volume: i32, position: [f32; 3], priority: i32, pan: i32 },
    /// KillItemSound for item `slot`.
    ItemSoundStop { slot: usize },
    /// AudioStreamSetVolume (0..0x7f × LEVL +0x94).
    MusicVolume { volume: i32 },
    /// AudioStreamStop.
    MusicStop,
    /// Expired item (ExpireItem 0x1537b0) with its fade mode.
    ItemExpired { slot: usize, mode: i32 },
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
    /// MBTreeSetFlags (set) / MBTreeClearFlags of the player's +0x6b8 tree
    /// (DoPlayerAction swing/throw actions; the tree's role is not
    /// identified yet, research/player.md).
    PlayerTreeFlags { player: usize, flags: u32, set: bool },
}

/// What PlayerProcessPowerups / PlayerProcessFamiliar ask AnimateATree for,
/// per player and update: the pojo's sequence by action with uVar14 (true:
/// 2, the immediate switch), and counters of the updates that carried
/// PlayerMotion's attack signals (+0x8d8 0x10000000 for the familiar and the
/// phoenix-style trees, the pojo's 0x20000000) so a slower renderer sees
/// every pulse.
#[derive(Clone, Copy, Debug, Default)]
pub struct TreeAnimRequest {
    pub pojo_seq: u16,
    pub pojo_now: bool,
    pub attacks: u32,
    pub pojo_attacks: u32,
}

#[derive(Clone)]
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
    /// MaxStopTime 0x22c4f8 (time-stop powerup duration) and OldEnemyShrink.
    pub max_stop_time: f32,
    /// WorldCrystal 0x23f500: the crystal index each realm's door needs.
    pub world_crystal: Vec<i32>,
    /// WorldOrder 0x23f4c8 (GetWorldOrder: a world's index).
    pub world_order: Vec<i32>,
    /// The rune thermometer (init_thermometer / modify_thermometer).
    pub thermometer: thermometer::Thermometer,
    /// WorldNumLevels(CurWorld): worlddatas +0x14, which LoadWorldData
    /// 0x161b98 fills from the WRLD chunk's +0x18 short — equal to the LEVL
    /// record count in all 14 world files, which is what is stored here.
    pub world_num_levels: i32,
    /// World animations of the level (WORLDS.PS2 table).
    pub world_anims: world_anim::WorldAnimSet,
    /// Tower state (TowerInit, welcome, crystal fade, deliveries, Sumner).
    pub tower: tower::TowerState,
    /// Player panels (MB blits and font messages; hud.rs).
    pub hud: hud::Hud,
    /// crystal_sidx / garg_sidx sound tables (Sumner's delivery speeches).
    pub speech_tables: std::collections::HashMap<&'static str, Vec<i32>>,
    /// MBTreeGetAlpha of item trees the simulation reads (Tower crystals).
    pub item_alpha: std::collections::HashMap<usize, i32>,
    /// Type-10 items (secret walls, barrels) drawing ProcessItems' hit flash:
    /// MBTreeSetAltTex(node, −2, the level's AAAWHITE) with MBTreeSetFlags
    /// 0x4000 (presentation).
    pub item_white: std::collections::HashSet<usize>,
    /// ReplaceItemAtree: item slots drawing another tree (presentation).
    pub item_trees: std::collections::HashMap<usize, String>,
    /// DamageItem's MBOX_NewObject leftovers, in creation order
    /// (presentation; drawn until the level ends).
    pub item_debris: Vec<ItemDebris>,
    /// SumnerHintTime (tower hint pad timer).
    pub sumner_hint_time: f32,
    /// PlayerProcessPowerups' animation requests for each player's attached
    /// powerup tree (+0x768) and familiar (+0x720); the trees and their
    /// clocks are the client's.
    pub tree_anim: [TreeAnimRequest; 4],
    /// DoRockFly / DidRockFly (DoWorldEffects counts boss 0x2a's rock
    /// launches; ProcessItems launches subtype-0x33 rocks on a change).
    /// ResetItems zeroes both at level start.
    pub do_rock_fly: i32,
    pub did_rock_fly: i32,
    pub old_enemy_shrink: f32,
    pub boss_is_dying: i32,
    pub suicide_bomber_index: i32,
    /// enemy_wall_colobj after EnemyWallCollide.
    pub enemy_wall_hit: Option<usize>,
    /// did_action (set when a player damages an enemy).
    pub did_action: bool,
    /// stage_playtime 0x2201a8 (vblanks of play; the tally's time) and
    /// do_show_path's state.
    pub stage_playtime: i32,
    /// RestartReq 0x24951c: see Game::check_soft_reset.
    pub restart_req: bool,
    pub show_path: stageinfo::ShowPath,
    /// death_touch: set while Death drains a player (do_enemy_attack).
    pub death_touch: bool,
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
    /// Each enemy node's matrix under EnemyRootNode as UpdateObjWorldMat last
    /// stored it (−0.0 normalised); GetObjWorldMat copies it back into the
    /// record. None: not yet stored (taken from the record).
    pub enemy_node: Vec<Option<[[f32; 4]; 4]>>,
    /// The world object whose node each enemy's node hangs from
    /// (do_enemy_collide's MBNodeSetParent onto a 0x1000 floor object);
    /// None = EnemyRootNode.
    pub enemy_floor_parent: Vec<Option<usize>>,
    /// SetEnemyObj's ground shadow per enemy slot (presentation).
    pub enemy_shadow: Vec<Option<enemies::EnemyShadow>>,
    /// tid.165 0x2bd000: AudioTempTrackSet's request counter (item loops
    /// only; other sound starts do not advance it natively).
    pub audio_tid: u16,
    /// Each player's sequence timings (the class ANIM tree DoPlayerAction's
    /// AnimInfo at +0x94 plays).
    pub player_clips: [Vec<crate::animation_clock::ClipTiming>; 4],
    /// The world object whose node the player's node is parented to
    /// (PlayerCheckMovingFloor's MBNodeSetParent); None = PlayerRootNode.
    pub player_floor_parent: [Option<usize>; 4],
    /// PlayerSetParent's team parent: the player whose +0x6b4 node carries
    /// this one (game/teammove.rs).
    pub player_team_parent: [Option<usize>; 4],
    /// The players' trees for posing nodes (the team-move DUMMY node).
    pub player_rigs: [Option<std::sync::Arc<teammove::PlayerRig>>; 4],
    /// The player node's local matrix (UpdatePlayerWorldMat writes it,
    /// GetObjWorldMat reads it back); None until the first update.
    pub player_node: [Option<[[f32; 4]; 4]>; 4],
    /// floor_col +0x34 after DoTransporter's FloorCollide (PlayerMotion
    /// reads it when the transporter arrives).
    pub transporter_floor_y: Option<f32>,
    /// player_water_wobj / player_water_y 0x2341f0/0x2341f4.
    pub player_water: Option<(usize, f32)>,
    /// partner_parent (PlyrSfxDoDamage → DoPlyrSfxSub's flag 0x2000 owner).
    pub partner_parent: Option<usize>,
    /// QuestWeaponSfx 0x23e218 / QuestWallSfx 0x23e234 (effect slots, −1)
    /// and QuestItemTimer.
    pub quest_weapon_sfx: i32,
    pub quest_wall_sfx: i32,
    pub quest_item_timer: f32,
    /// ComboEffects 0x2407f8 (EffectHeaders indices, ELF data).
    pub combo_effects: Vec<i32>,
    pub item_anims: Vec<Option<item_runtime::ItemAnim>>,
    /// The item trees' clip timings by label in AtreeMatchAnyHeader order
    /// (SetItemGeo attaches one to every item created later as well).
    pub item_anim_lib: std::collections::HashMap<String, Vec<crate::animation_clock::ClipTiming>>,
    /// Item tree labels whose AnimateNode reaches DoAnimation (a type-1 node).
    /// (also the NULL1 node index or −1).
    pub item_anim_nodes: std::collections::HashMap<String, (bool, i32, i32)>,
    /// Culling view (MBWorldSphereClip inputs) derived from the StdCam.
    pub camera: Option<item_runtime::Camera>,
    /// The start camera parameters (the culling planes come from here).
    pub camera_rest: Option<setup::StartCamera>,
    /// newcam_mode 0x21f130 as main's camera dispatch sets it every frame:
    /// 2 under StdCamUpdate, 0 with a boss (BossCritInst, BossCameraUpdate)
    /// or the old camera; unchanged while a trigger camera runs. ItemVisible
    /// tests the camera target's distance only in modes 1 and 2.
    pub newcam_mode: i32,
    /// The gameplay camera (StdCam module, research/camera.md).
    pub std_camera: Option<camera::Camera>,
    /// The boss camera (BossCameraInit per level; used while BossCritInst).
    pub boss_camera: Option<camera::BossCamera>,
    /// Trigger camera state, the Tower's linked cameras and
    /// disable_player_controls 0x211464 (PlayerControls zeroes the inputs).
    pub trig_cam: camera::TriggerCam,
    pub tower_cams: camera::TowerCams,
    pub disable_player_controls: i32,
    /// vibes 0x2116c8 (do_vibe / serve_vibes: on, frames left) per player,
    /// and the actuator requests for the host: (joy_remap port, large motor
    /// 0..1 from joyBeginShake, None from joyEndShake).
    pub vibes: [(bool, i32); 4],
    pub rumble: Vec<(i32, Option<f32>)>,
    /// Vibes started in this update before the host's update_controls:
    /// main runs PlayerControls (serve_vibes) before game_main, so they are
    /// first served in the next update.
    pub vibe_fresh: [bool; 4],
    /// Controller state (PlayerControl records, plr_* words) and the ELF
    /// control tables (remap, rep_speed, SpecialData, JoyAng/JoyMag).
    pub controls: pad::Controls,
    /// Each player's controller options (style, vibration, auto aim, auto
    /// attack) as player_store_in_save stores them.
    pub control_options: [profile::ControlOptions; 4],
    pub control_tables: Option<pad::ControlTables>,
    /// WorldStartTime 0x2c2c88 (GauntWorldInit: Time at level init).
    pub world_start_time: f32,
    /// ITEM_ACTIVE_DIST 0x234364, global 0x20f950 (mode), 0x20f954, start_enemyinfo, ExitState.
    pub item_active_dist: f32,
    pub mode_950: i32,
    pub globals_954: i32,
    pub start_enemyinfo: bool,
    pub exit_state: i32,
    pub enemy_anim_data: enemy_ai::EnemyAnimData,
    /// PlayerData[player]: the class record of each player slot.
    pub classes: [pickups::ClassData; 4],
    /// fight_range … missilespd_range and PActionDesc (check_player_atts,
    /// PlayerProcessPowerups).
    pub class_tables: crate::player_class::ClassTables,
    pub effects: effects::Effects,
    /// BossCritInst 0x25d2c0 and the difficulty word 0x21edcc (index of
    /// diff_mul_tab.15 in ResolveWorldData; also PlayerGetTarget's cone).
    pub boss_crit_inst: i32,
    /// The freed boss instance BossCritInst still points at (critter_ai's
    /// CritterDelInst).
    pub boss_stale: Option<(record::Record, f32)>,
    pub target_cone_mode: i32,
    /// Destination wave (world << 8 | level) once a player finished do_exit.
    pub level_done: Option<i32>,
    /// do_players' last result and game_main's exit test (do_players
    /// done and AudioStillPlaying clear): the level may end.
    pub players_done: bool,
    pub exit_ready: bool,
    /// DIAGNOSTIC (M20, presentation timing only): the milliseconds
    /// since the start of the last step_with at the end of each phase.
    /// Nothing in the simulation reads it.
    pub step_phases: Vec<(&'static str, f32)>,
    /// game_main's explicit next wave before init_mapscreen (−1 none).
    pub map_preload: i32,
    /// vb_counter 0x2a1c78 (vblanks; the fixture's value at load).
    pub vb_counter: u32,
    pub audio_que: audio_queue::AudioQueue,
    /// attract_audio_flag 0x21f6c0 (queued lines allowed in attract mode).
    pub attract_audio: bool,
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
    /// auto_exit_timer (vblanks) and the good wizard (goodwiz.rs).
    pub auto_exit_timer: i32,
    pub good_wiz: goodwiz::GoodWiz,
    /// worldseltab (world, rune bits, rune count).
    pub worldsel: Vec<(i32, u32, i32)>,
    /// Players parented to a critter node by a grab: (critter, node, offset).
    pub player_grab: [Option<critters::Grab>; 4],
    /// SpewItems (item slot, velocity), at most 0x20.
    pub spew_items: Vec<(Option<usize>, [f32; 3])>,
    /// boss1_items 0x25d250 (SkorneSpewItems): name, +0xf4 value, +0xf0
    /// word, +0xf8 word.
    pub boss1_items: Vec<(Vec<u8>, i32, u32, u32)>,
    /// SaveStage's record of the level left through a secret exit (the
    /// host carries it to the return; LoadStage).
    pub stage_save: Option<stage::StageSave>,
    /// WaveTimerMax and the sandglass view (wavetimer.rs).
    pub wave_timer: wavetimer::WaveTimer,
    /// secret_char 0x23f5a8: the secret character each treasure room's
    /// coins unlock (by level index).
    pub secret_char: Vec<i32>,
    /// LowerBGMusic's duck (sound_cues.rs).
    pub bg_duck: sound_cues::BgDuck,
    /// BGMusic* state (bgmusic.rs).
    pub bg_music: bgmusic::BgMusic,
    /// volMusic 0x21f6c8: the ELF's data holds 100; OptionsSetup's
    /// AudioSetVolMusic sets GamePrefs word 1 (clamped 0..255), which the
    /// host applies (Game::audio_set_vol_music).
    pub vol_music: i32,
    /// PlayerDoWeapTrail's weapon copies per player (slots +0x7e0..+0x7fc)
    /// and weaptrail_dalpha 0x234298 (32 in the ELF data).
    pub weap_trail: [[Option<weaptrail::TrailCopy>; 8]; 4],
    pub weaptrail_dalpha: i32,
    /// The X-ray view per player (seethru.rs).
    pub see_thru: [seethru::SeeThru; 4],
    /// ScreenSaver 0x12acf8's timer, levels and weapons.
    pub saver: saver::ScreenSaver,
    /// init_stage_info / write_stage_info's title state.
    pub stage_info: stageinfo::StageInfo,
    /// Where BossDeath placed the boss key effect.
    pub boss_key_position: Option<[f32; 3]>,
    /// LockCamReq 0x23f2c4: camera-lock request. Only LockCamera (from the
    /// old camera's screen_limitation) sets it nonzero, and play runs
    /// StdCamUpdate (new_camera_switch 1), so it stays 0 (do_players,
    /// InitCamera clear it).
    pub lock_cam_req: i32,
    /// MBTreeGetAlpha of each enemy tree (+0x70; enemy_mod_alpha reads it):
    /// 255 for a new tree (SetEnemyObj), then the last MBTreeSetAlpha value.
    pub enemy_alpha: Vec<i32>,
    /// The drawn state of each enemy tree (presentation: alpha, MB flag 2).
    pub enemy_look: Vec<enemies::TreeLook>,
    /// sfx_ene_bloodidx 0x2407.. (AddEnemyHitCol's alternating blood).
    pub sfx_ene_bloodidx: u32,
    /// GenHitEffects 0x244ec0 / GenDieEffects 0x244f78 (InitEffects: the
    /// custom effects GENHIT / GENDIE of each loaded enemy type's package; −1
    /// otherwise), 45 types.
    pub gen_hit_effects: [i32; 45],
    pub gen_die_effects: [i32; 45],
    /// Boss health meters (HealthMeterStart; HealthMeterInit per level).
    pub health_meters: healthmeter::HealthMeters,
    /// Enemy and player trees' skin state (MBTreeSetAltTex / AmbientAdd).
    pub skin_views: skin::SkinViews,
    /// Labels of the loaded MBOX object tables (MBOX_FindObject_Sub).
    pub mb_objects: Option<std::collections::HashSet<String>>,
    /// LevelEnemyDesc 0x1626c8 inputs: EnemyType slots (kind, ENMY +0x10
    /// name) and world_data +4.
    pub enemy_slot_names: Vec<(i32, String)>,
    pub world_dir: String,
    /// Lookouts 0x23bd20 / NumLookouts 0x23c720 (game/lookouts.rs).
    pub lookouts: Vec<lookouts::Lookout>,
    /// Milestones 0x238510 (+0x30 position): AddLocatorInstList's kind-5
    /// locators through add_arrow(0, …) (FloorPos(y, 0.2, position, 0) + 0.5).
    pub milestones: Vec<[f32; 3]>,
    /// PlayerStartPos 0x22c158 / PlayerStartYaw 0x22c164 (the level-start
    /// placement) and PlayerInitStartPos[0] 0x234450 (the id-0 start locator).
    pub player_start: ([f32; 3], f32),
    pub player_init_start: [f32; 3],
    /// World node labels (FindWorldObject 0x11c318 / FindWORLDOBJ).
    pub world_labels: Vec<String>,
    /// Presentation state of world MB nodes changed by game code:
    /// MBTreeSetFlags / MBTreeClearFlags bits (node only) and MBTreeSetAlpha
    /// values, with a counter bumped on every change.
    pub node_mb_set: Vec<u32>,
    pub node_mb_clear: Vec<u32>,
    pub node_alpha: Vec<Option<u8>>,
    pub node_state_version: u32,
    /// InitLighting / DoLighting / SetAmbientSpecial state (game::lighting).
    pub lighting: lighting::Lighting,
    pub player_light: lighting::PlayerLightTables,
    pub sounds: sound::SoundTables,
    pub log: setup::FrameLog,
}

impl Game {
    /// NumEnemies (Globals::num_enemies), at most the 25 records.
    pub(crate) fn num_enemies(&self) -> usize {
        match usize::try_from(self.globals.num_enemies) {
            Ok(n) if 0 < n => n.min(self.enemies.len()),
            _ => self.enemies.len(),
        }
    }
    pub fn level_f32(&self, o: usize) -> f32 {
        f32::from_le_bytes(self.level[o..o + 4].try_into().unwrap())
    }
    pub fn level_i16(&self, o: usize) -> i16 {
        i16::from_le_bytes([self.level[o], self.level[o + 1]])
    }
    pub fn level_u32(&self, o: usize) -> u32 {
        u32::from_le_bytes(self.level[o..o + 4].try_into().unwrap())
    }
    /// NewHelp(id, player, point): most callers pass the player record's
    /// +0x60 (attention point); `usize::MAX` = −1 with no point.
    pub fn help(&mut self, id: i32, player: usize) {
        let at = self.players.get(player).map(|r| r.v3(0x60));
        self.help_at(id, player, at);
    }
    pub fn help_at(&mut self, id: i32, player: usize, at: Option<[f32; 3]>) {
        self.events.push(Event::Help { id, player });
        let p = (player < 4).then_some(player);
        if let Err(e) = self.new_help(id, p, at) {
            self.log.note("NewHelp", &e);
        }
    }
    pub fn got_it(&mut self, player: usize, kind: i32, amount: i32) {
        self.events.push(Event::GotIt { player, kind, amount });
        self.hud.add_got_it(player, kind, amount);
    }
}
