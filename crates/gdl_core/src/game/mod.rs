//! Native gameplay state transcribed from game_main 0x125280 (game_mode
//! 0x4010): world_update, do_items, do_enemies, enemy_update, do_players,
//! ProcessEffects. Records keep their original layouts (see `record`);
//! presentation side effects (help text, audio, FX, HUD notices, vibration)
//! are emitted as [`Event`]s instead of being performed.
pub mod damage;
pub mod enemies;
pub mod pickups;
pub mod record;

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
}

/// Presentation side effects requested by the simulation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Event {
    /// NewHelp(id, player): help/hint text.
    Help { id: i32, player: usize },
    /// add_got_it(player, kind, amount): HUD pickup notice.
    GotIt { player: usize, kind: i32, amount: i32 },
    /// Audio* calls, by original function name.
    Audio { cue: &'static str, player: Option<usize>, arg: i32 },
    /// Expired item (ExpireItem 0x1537b0) with its fade mode.
    ItemExpired { slot: usize, mode: i32 },
    Vibrate { player: usize, strength: i32, time: i32 },
    PlayerDied { player: usize },
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
