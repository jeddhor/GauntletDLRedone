//! Native gameplay state transcribed from game_main 0x125280 (game_mode
//! 0x4010): world_update, do_items, do_enemies, enemy_update, do_players,
//! ProcessEffects. Records keep their original layouts (see `record`);
//! presentation side effects (help text, audio, FX, HUD notices, vibration)
//! are emitted as [`Event`]s instead of being performed.
pub mod boss;
pub mod camera;
pub mod critter_ai;
pub mod critter_hits;
pub mod critter_move;
pub mod critters;
pub mod doplayers;
pub mod damage;
pub mod effects;
pub mod hud;
pub mod optmenu;
pub mod select;
pub mod enemies;
pub mod enemy_ai;
pub mod item_runtime;
pub mod lookouts;
pub mod pad;
pub mod pcontact;
pub mod pmotion;
pub mod paction;
pub mod pickups;
pub mod players;
pub mod powerups;
pub mod profile;
pub mod progress;
pub mod psfx;
pub mod record;
pub mod setup;
pub mod tower;
pub mod world_anim;
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
    #[serde(default)]
    pub welcome_flag: i32,
    #[serde(default)]
    pub last_finished_world: i32,
    #[serde(default)]
    pub last_finished_level: i32,
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
    /// ReplaceItemAtree(item, powerup_anim, tree): the item's model changes.
    ItemTree { slot: usize, tree: &'static str },
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
    /// A trigger camera the original activates here (camera system not
    /// ported yet): "crystal", "sumner", "window", "rune".
    CameraRequest { kind: &'static str, arg: i32 },
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
    /// MBTreeSetFlags (set) / MBTreeClearFlags of the player's +0x6b8 tree
    /// (DoPlayerAction swing/throw actions; the tree's role is not
    /// identified yet, research/player.md).
    PlayerTreeFlags { player: usize, flags: u32, set: bool },
    /// do_players' MBPosLightNew for the player (levels with LEVL flag 8):
    /// player_lightrad/attn at the attention point + player_lightdy, colour
    /// player_light_color[+4].
    PlayerLight { player: usize },
    /// SetAmbientSpecial(1/30, value): the class special's ambient flash.
    AmbientSpecial { value: f32 },
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
    /// MaxStopTime 0x22c4f8 (time-stop powerup duration) and OldEnemyShrink.
    pub max_stop_time: f32,
    /// WorldCrystal 0x23f500: the crystal index each realm's door needs.
    pub world_crystal: Vec<i32>,
    /// WorldNumLevels(CurWorld): HYPOTHESIS = the world's LEVL record count
    /// (the worlddatas field is filled at run time).
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
    /// SumnerHintTime (tower hint pad timer).
    pub sumner_hint_time: f32,
    pub old_enemy_shrink: f32,
    pub boss_is_dying: i32,
    pub suicide_bomber_index: i32,
    /// enemy_wall_colobj after EnemyWallCollide.
    pub enemy_wall_hit: Option<usize>,
    /// did_action (set when a player damages an enemy).
    pub did_action: bool,
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
    /// Each player's sequence timings (the class ANIM tree DoPlayerAction's
    /// AnimInfo at +0x94 plays).
    pub player_clips: [Vec<crate::animation_clock::ClipTiming>; 4],
    /// The world object whose node the player's node is parented to
    /// (PlayerCheckMovingFloor's MBNodeSetParent); None = PlayerRootNode.
    pub player_floor_parent: [Option<usize>; 4],
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
    /// Culling view (MBWorldSphereClip inputs) derived from the StdCam.
    pub camera: Option<item_runtime::Camera>,
    /// The start camera parameters (the culling planes come from here).
    pub camera_rest: Option<setup::StartCamera>,
    /// The gameplay camera (StdCam module, research/camera.md).
    pub std_camera: Option<camera::Camera>,
    /// The boss camera (BossCameraInit per level; used while BossCritInst).
    pub boss_camera: Option<camera::BossCamera>,
    /// Trigger camera state, the Tower's linked cameras and
    /// disable_player_controls 0x211464 (PlayerControls zeroes the inputs).
    pub trig_cam: camera::TriggerCam,
    pub tower_cams: camera::TowerCams,
    pub disable_player_controls: i32,
    /// Controller state (PlayerControl records, plr_* words) and the ELF
    /// control tables (remap, rep_speed, SpecialData, JoyAng/JoyMag).
    pub controls: pad::Controls,
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
    pub sounds: sound::SoundTables,
    pub log: setup::FrameLog,
}

impl Game {
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
    pub fn audio(&mut self, cue: &'static str, player: Option<usize>, arg: i32) {
        self.events.push(Event::Audio { cue, player, arg });
    }
    pub fn got_it(&mut self, player: usize, kind: i32, amount: i32) {
        self.events.push(Event::GotIt { player, kind, amount });
        self.hud.add_got_it(player, kind, amount);
    }
}
