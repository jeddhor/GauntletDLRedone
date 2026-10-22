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
