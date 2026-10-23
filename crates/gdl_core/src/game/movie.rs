//! In-game movies: init_gamemovie 0x1657d0 / do_gamemovie 0x165908 (game
//! mode 0x400e). The MPEG decoding itself (InitMovie / ServeMovie /
//! KillMovie: IPU video, private-stream audio) is the host's; the game only
//! chooses the movie, takes the skip edge and decides where play goes on.
use super::Game;

/// The movie a level change plays (InitMovie's "/<dir>/<name>.pss").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieRequest {
    pub dir: &'static str,
    pub name: String,
}

/// init_gamemovie's state: movietype 1 (a level's movie, skippable; then
/// init_next_level) or 2 (victory/garm, not skippable; then init_shop).
#[derive(Debug, Clone)]
pub struct GameMovie {
    pub request: Option<MovieRequest>,
    pub movietype: i32,
    /// kill_gamemovie.
    pub kill: bool,
}

impl Game {
    /// active_player_edge 0x107458: a player in any state but 0 whose
    /// PlayerControl +8 edge word has one of `mask`'s bits.
    pub fn active_player_edge(&self, mask: u32) -> bool {
        (0..4).any(|k| self.players[k].i32_(0xfc) != 0 && self.controls.control[k].u32_(8) & mask != 0)
    }

    /// init_gamemovie(param): game mode 0x400e, BGMusicStop, AudioStopAll;
    /// unless NoMovies (0 in this build), AuxLevels 0x10 or a skip_wave:
    /// param 0x2c → MPEG/victory, 0x2b → MPEG/garm (movietype 2), else the
    /// level's own movie at LEVL +0x34 (`level_movie`, the level just
    /// loaded by LoadWorldData) when named. MBOX_ResetUnlockedModels and
    /// delete_map_blits are the host's.
    pub fn init_gamemovie(&mut self, param: i32, level_movie: &str) -> GameMovie {
        self.globals.game_mode = 0x400e;
        self.bg_music_stop();
        self.audio_stop_all();
        let mut m = GameMovie { request: None, movietype: 1, kill: false };
        if self.globals.aux_levels & 0x10 == 0 && self.globals.skip_wave == 0 {
            let name = match param {
                0x2c => Some("victory".to_string()),
                0x2b => Some("garm".to_string()),
                _ => (!level_movie.is_empty()).then(|| level_movie.to_string()),
            };
            if matches!(param, 0x2c | 0x2b) {
                m.movietype = 2;
            }
            m.request = name.map(|name| MovieRequest { dir: "MPEG", name });
        }
        m
    }

    /// do_gamemovie per frame: an accept edge (0x2000000) kills a
    /// movietype-1 movie; `serving` is ServeMovie's "still playing" (false
    /// once the host's movie ended or could not start). Some(movietype)
    /// when the movie is over (1: init_next_level(0), 2: init_shop).
    pub fn do_gamemovie(&mut self, m: &mut GameMovie, serving: bool) -> Option<i32> {
        if m.request.is_none() {
            return Some(m.movietype);
        }
        if self.active_player_edge(0x200_0000) && m.movietype == 1 {
            m.kill = true;
        }
        if m.kill || !serving {
            m.request = None;
            return Some(m.movietype);
        }
        None
    }
}

/// init_over / do_over (game mode 0x4014): timer1 counts down from 0xf0
/// vblanks, timer2 up (AudioGameOver at 60, the "GAME OVER" letters one per
/// 8 vblanks after that).
#[derive(Debug, Clone, Copy)]
pub struct GameOver {
    pub timer1: i32,
    pub timer2: i32,
}

impl Game {
    /// init_over 0x127e70 (no players left, or Quit Game): game mode 0x4014,
    /// BGMusicStop, AudioStopAll, AudioEndGame (empty), the timers, every
    /// player inactivated, active_players 0, TriggerCameraEnd.
    pub fn init_over(&mut self) -> GameOver {
        self.globals.game_mode = 0x4014;
        self.globals.paused = 0;
        self.bg_music_stop();
        self.audio_stop_all();
        for k in 0..4 {
            self.inactivate_player(k);
        }
        self.globals.active_players = 0;
        self.trigger_camera_end();
        GameOver { timer1: 0xf0, timer2: 0 }
    }

    /// do_over 0x127ee8 after world_update/do_items/do_enemies/do_players:
    /// AudioGameOver (0x1005a, 0xe0, 0x14) as timer2 passes 60, DrawStringText
    /// (centred, y 0x78, font 6, white, entry 0xa9) at DrawStringScale 2.0
    /// cut to (timer2 − 60) >> 3 letters (0..9), and timer1. True once
    /// timer1 ran out: EndTower, fail_movie_show = 1, init_attract_mode
    /// (0x8002) — the host's.
    pub fn do_over(&mut self, over: &mut GameOver) -> bool {
        let vb = self.globals.vb_elapsed;
        let before = over.timer2;
        over.timer2 += vb;
        if before < 0x3c && 0x3b < over.timer2 {
            // AudioGameOver 0x15e228: AudioFX(0x1005a, 0xe0, 0x14).
            self.sfx_flat(0x1_005a, 0xe0, 0x14);
        }
        let n = ((over.timer2 - 0x3c) >> 3).clamp(0, 9) as usize;
        let blits = self.hud.string_text_prefix_blits(-0x100, 0x78, 6, 0xffffff, 0xa9, 0, 2.0, n);
        self.hud.push_temp(blits);
        if self.globals.paused == 0 {
            over.timer1 -= vb;
            if over.timer1 < 1 {
                return true;
            }
        }
        false
    }
}
