//! The realm map between levels (game mode 0x400f): init_mapscreen
//! 0x1649d8 and do_mapscreen 0x164d78. The next level's map
//! (MAPS/level<code>: map_<code>_00..03 at screenblitxy, the ldmap_ layer
//! above it, dash_<code>_n and map_<code>glow) from its WDATA MAPS record
//! (LEVL +0x5c); without one (or when skipping) TRANSITION_SCREEN for 0x78
//! vblanks. NATIVE: the loading steps (LoadGauntWorldDone, AudioLoadStage)
//! complete one per frame.
use super::hud::Blit;
use super::Game;

/// screenblitxy 0x249630.
const SCREEN_XY: [(i32, i32); 4] = [(0, 0), (256, 0), (0, 256), (256, 256)];
/// mapglow_tottime 0x23e268, map2_tottime 0x23e26c, mapglow_fade
/// 0x23e260, mapglow_fade_hold 0x23e264 (vblanks).
const GLOW_TOTTIME: i32 = 210;
const MAP2_TOTTIME: i32 = 180;
const GLOW_FADE: i32 = 60;
const GLOW_HOLD: i32 = 10;

/// What next_world's LoadWorldData of the destination (init_mapscreen,
/// after MapMusicStart) gives the map: CurWorld for AudioMapDot and
/// AudioEnterNextStage's lines (sound::enter_next_stage_lines). The port
/// keeps the finished level's Game until the map ends.
#[derive(Clone, Copy, Debug, Default)]
pub struct MapDest {
    pub world: i32,
    pub enter: Option<(i32, i32)>,
}

#[derive(Clone, Debug)]
pub struct MapScreen {
    pub code: String,
    pub dest: MapDest,
    /// The MAPS record when the map is shown (param 2 = 0 and LEVL +0x68).
    pub map: Option<[f32; 18]>,
    pub time: i32,
    pub time_left: i32,
    pub load_done: i32,
    pub load_delay: i32,
    pub glow_timer: i32,
    pub map2_alpha: i32,
    pub dashes: [bool; 8],
    pub skip: bool,
    pub glow_shown: bool,
    pub glow_alpha: i32,
}

/// MBBlitSetAlpha(b, a): GS alpha 0x80 − a/2.
fn alpha(b: &mut Blit, a: i32) {
    b.rgba[3] = (0x80 - (a.clamp(0, 0xff) >> 1)) as u8;
}

impl Game {
    /// init_mapscreen(time, skip) for the next level `code` with its MAPS
    /// record.
    pub fn init_mapscreen(&mut self, time: i32, skip: bool, code: &str, map: Option<[f32; 18]>, dest: MapDest) -> MapScreen {
        self.map_music_start();
        self.globals.game_mode = 0x400f;
        self.hud.reset_models();
        self.hud_reset_players();
        for k in 0..4 {
            self.setup_player_display(k);
        }
        let map = if skip { None } else { map };
        MapScreen {
            code: code.to_string(),
            dest,
            map,
            time: 0,
            time_left: time,
            load_done: 0,
            load_delay: 2,
            glow_timer: 0,
            map2_alpha: 0,
            dashes: [false; 8],
            skip,
            glow_shown: false,
            glow_alpha: 0xff,
        }
    }

    /// do_mapscreen: true when the map is done (loaded, the glow phase
    /// over and maptimeleft run out).
    pub fn do_mapscreen(&mut self, m: &mut MapScreen) -> bool {
        let vb = self.globals.vb_elapsed;
        let _ = self.write_player_info(-1);
        match m.load_done {
            0 => {
                if !m.skip {
                    self.audio_enter_next_stage(m.dest.enter);
                }
                m.load_done = 1;
            }
            1 => {
                if m.load_delay == 0 {
                    m.load_done = 2;
                } else {
                    m.load_delay -= 1;
                }
            }
            // AudioStillPlaying(10): the entry lines finish first.
            2 => {
                if !self.audio_still_playing() {
                    m.load_done = 3;
                }
            }
            // AudioLoadStage(stage, 1); AudioStillLoading (banks load on
            // demand in the host).
            3 | 4 => m.load_done += 1,
            _ => m.load_done = 99,
        }
        m.time += vb;
        let mut glow_done = true;
        let mut out = Vec::new();
        let code = m.code.clone();
        let hud = &self.hud;
        let tex = |name: &str| hud.lookup(name);
        match m.map {
            Some(map) => {
                for (k, &(x, y)) in SCREEN_XY.iter().enumerate() {
                    let mut b = hud.make_pub(tex(&format!("map_{code}_{k:02}")), x, y, -1, -1);
                    b.z = f32::from_bits(0x477a_6400);
                    out.push(b);
                }
                let mut i = 0;
                while i < 8 {
                    let x = map[2 + 2 * i];
                    if x < 0.0 {
                        i = 8;
                        break;
                    }
                    if !m.dashes[i] {
                        if m.time <= i as i32 * 0x1e {
                            break;
                        }
                        m.dashes[i] = true;
                        self.audio_map_dot(m.dest.world);
                    }
                    i += 1;
                }
                let has_glow = 0.0 <= map[0];
                let mut skip_reset = false;
                if has_glow && i == 8 {
                    m.glow_timer += vb;
                    if GLOW_TOTTIME < m.glow_timer {
                        glow_done = MAP2_TOTTIME <= m.glow_timer - GLOW_TOTTIME;
                        m.map2_alpha = ((m.glow_timer - GLOW_TOTTIME) * 4).clamp(0, 0xff);
                        m.glow_alpha = m.map2_alpha;
                        if m.map2_alpha == 0xff {
                            skip_reset = true;
                        }
                    } else {
                        let period = GLOW_FADE * 2 + GLOW_HOLD;
                        let mut k = m.glow_timer % period;
                        if GLOW_FADE * 2 < k {
                            k = 0;
                        } else if GLOW_FADE < k {
                            k = GLOW_FADE * 2 - k;
                        }
                        let v = ((k * 0xff + GLOW_FADE - 1) / GLOW_FADE).clamp(4, 0xfa);
                        m.glow_shown = true;
                        m.glow_alpha = 0xff - v;
                    }
                }
                let hud = &self.hud;
                for (k, &(x, y)) in SCREEN_XY.iter().enumerate() {
                    let mut b = hud.make_pub(hud.lookup(&format!("ldmap_{code}_{k:02}")), x, y, -1, -1);
                    b.z = f32::from_bits(0x4779_ce00);
                    alpha(&mut b, 0xff - m.map2_alpha);
                    out.push(b);
                }
                for (k, on) in m.dashes.iter().enumerate() {
                    if *on {
                        let mut b = hud.make_pub(hud.lookup(&format!("dash_{code}_{}", k + 1)), map[2 + 2 * k] as i32, map[3 + 2 * k] as i32, -1, -1);
                        b.z = f32::from_bits(0x477a_3c00);
                        out.push(b);
                    }
                }
                if has_glow && m.glow_shown {
                    let mut b = hud.make_pub(hud.lookup(&format!("map_{code}glow")), map[0] as i32, map[1] as i32, -1, -1);
                    b.z = f32::from_bits(0x477a_5000);
                    alpha(&mut b, m.glow_alpha);
                    out.push(b);
                }
                if skip_reset {
                    out.extend(hud.glow_text(1.0, 0x154, 0x140, "Loading...", m.time as u32));
                } else {
                    m.time_left = 0x3c;
                }
            }
            None => {
                let mut b = hud.make_pub(tex("TRANSITION_SCREEN"), 0, 0, 0x200, 0x140);
                b.z = f32::from_bits(0x477a_6400);
                out.push(b);
            }
        }
        self.hud.push_temp(out);
        m.time_left -= vb;
        let done = self.globals.paused == 0 && m.load_done == 99 && glow_done && m.time_left < 1;
        if done {
            self.bank_music_end();
        }
        done
    }
}
