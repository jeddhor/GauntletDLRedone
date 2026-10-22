//! game_main 0x125280's level end (after do_players reports every player
//! done and AudioStillPlaying is clear) and next_world 0x1250f0, which
//! init_mapscreen calls for the map's destination.
//!
//! The level just played is PrevWorld/PrevLevel (LoadGauntWorldStart sets
//! them from the level being loaded, so they are the current level here).
//! With no active players: init_over. Leaving the Tower: EndTower, then the
//! map (init_mapscreen(0x78, 0)). Otherwise, unless every player is dead
//! (state 0/0xb) or a stage return (skip_wave 13..0xffff → recorded_wave)
//! or quit_level: skip_wave 0 or 0xc → BossType 0x2c / 0x2b play their
//! movie (init_gamemovie; game_wave = TowerWave), world 12 and levels
//! (5, 0) / (6, 0) go on to the next level index, everything else to
//! TowerWave; any other skip_wave → −1 (game_wave, i.e. next_world's
//! choice). PlayersSetWaveFinished(PrevWorld, PrevLevel) unless all dead.
//! TowerWave → init_shop(0) (the tally and shop, then init_player_select(2)
//! or init_next_level into the Tower); anything else → LoadWorldData and
//! init_mapscreen(0x78, stage return).
use super::boss::TOWER_WAVE;
use super::{unsupported, Game, R};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelEnd {
    /// init_over: nobody active.
    Over,
    /// init_shop(0) on the way to the Tower.
    Shop,
    /// init_gamemovie(boss type), then game_wave = TowerWave.
    Movie(i32),
    /// init_mapscreen(0x78, skip) towards next_world's wave.
    Map { skip: bool },
}

impl Game {
    /// game_main's level-end branch (see the module doc). Also runs
    /// PlayersSetWaveFinished for the level just played when it applies.
    pub fn game_main_level_end(&mut self) -> LevelEnd {
        let g = &self.globals;
        if g.active_players == 0 {
            return LevelEnd::Over;
        }
        let here = g.cur_world << 8 | (g.cur_level & 0xff);
        if here == TOWER_WAVE {
            // EndTower; next_world (LoadWorldData); init_mapscreen(0x78, 0).
            return LevelEnd::Map { skip: false };
        }
        let returning = (13..=0xffff).contains(&g.skip_wave);
        let all_dead = (0..4).all(|k| matches!(self.players[k].i32_(0xfc), 0 | 0xb));
        let (w, l) = (g.cur_world, g.cur_level);
        let mut to = TOWER_WAVE;
        let mut movie = None;
        if !all_dead {
            to = self.stage_save.as_ref().map_or(-1, |s| s.recorded_wave);
            if !returning {
                to = TOWER_WAVE;
                if g.quit_level == 0 && (g.skip_wave == 0 || g.skip_wave == 0xc) {
                    if g.skip_wave == 0xc {
                        to = -1;
                    }
                    if g.boss_type == 0x2c || g.boss_type == 0x2b {
                        movie = Some(g.boss_type);
                    } else if w == 0xc || (w == 5 && l == 0) {
                        to = here + 1;
                    } else {
                        to = TOWER_WAVE;
                        if w == 6 && l == 0 {
                            to = here + 1;
                        }
                    }
                } else if g.quit_level == 0 {
                    to = -1;
                }
            }
            let (l90, l92) = (self.level_i16(0x90), self.level_i16(0x92));
            self.players_set_wave_finished(w, l, l90, l92);
        }
        if let Some(b) = movie {
            return LevelEnd::Movie(b);
        }
        if to == TOWER_WAVE {
            return LevelEnd::Shop;
        }
        self.map_preload = to;
        LevelEnd::Map { skip: returning }
    }

    /// next_world: a stage return goes to recorded_wave; otherwise the
    /// largest +0x808 (do_exit's destination) of the players not in state
    /// 0 or 2, TowerWave when none. game_main's explicit wave (world 12,
    /// (5, 0), (6, 0)) is LoadWorldData'd before init_mapscreen, whose
    /// next_world then takes the same players' destination; the explicit
    /// wave is used when no player recorded one. LEVL +4 bit 0 is set on
    /// every record of this disc, so NextWorldLevel's step is guarded.
    pub fn next_world_wave(&self) -> R<i32> {
        if (13..=0xffff).contains(&self.globals.skip_wave) {
            return Ok(self.stage_save.as_ref().map_or(TOWER_WAVE, |s| s.recorded_wave));
        }
        let mut to = -1;
        for k in 0..4 {
            let st = self.players[k].i32_(0xfc);
            let d = self.players[k].i32_(0x808);
            if st != 0 && st != 2 && to < d {
                to = d;
            }
        }
        if to < 0 {
            to = if 0 <= self.map_preload { self.map_preload } else { TOWER_WAVE };
        }
        if 0xd < to >> 8 {
            return unsupported(format!("next_world: wave {to:#x} past world 13 (first_wave)"));
        }
        Ok(to)
    }
}
