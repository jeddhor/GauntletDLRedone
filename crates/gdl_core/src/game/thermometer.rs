//! The rune thermometer: init_thermometer 0x1244b8 (init_next_level) and
//! modify_thermometer 0x124760 (game_main every frame). Shown in a level
//! with a rune item (RuneItem) once a present player holds the required
//! shard (PlayerHasShard(p, GetWorldOrder(5)); in world 8 the class record's
//! +0x8 word bit 0x1000) and none holds the rune itself: THERMBASE at
//! (0x188, −1) and the THERMCOL column, filled by the camera attention's
//! closeness to RunePos (therm_const = 1 / (|world extent| × 0.7)), with
//! AudioRuneGetCloser / AudioRuneIsNear once each.
use super::hud::Blit;
use super::Game;

#[derive(Clone, Debug, Default)]
pub struct Thermometer {
    pub shown: bool,
    pub therm_const: f32,
    pub hot1: bool,
    pub hot2: bool,
    /// The column's (y, height, crop v0) this frame.
    pub column: (i32, i32, f32),
}

impl Game {
    /// init_thermometer for the level just set up.
    pub fn init_thermometer(&mut self, world_min: [f32; 3], world_max: [f32; 3]) {
        let mut shown = false;
        if self.globals.game_mode & 0x4000 != 0
            && let Some(slot) = self.items.rune_item
        {
            let def = self.items.records[slot].definition();
            let rune = i32::from(self.items.definitions[def].i16_(0x40));
            let order = self.world_order.clone();
            let shard = order.iter().position(|&w| w == 5).unwrap_or(0) as u32;
            for k in 0..4 {
                if self.player_has_rune(k as i32, rune) {
                    shown = false;
                    break;
                }
                let have = if self.globals.cur_world == 8 {
                    let c = self.players[k].u32_(0xc) as usize;
                    self.players[k].u16_(0xcf8 + 0xb4 * c) & 0x1000 != 0
                } else {
                    self.player_has_shard(k, shard)
                };
                if have {
                    shown = true;
                }
            }
        }
        let e = [world_max[0] - world_min[0], world_max[1] - world_min[1], world_max[2] - world_min[2]];
        let len = (e[0] * e[0] + e[1] * e[1] + e[2] * e[2]).sqrt();
        self.thermometer = Thermometer { shown, therm_const: if 0.0 < len { 1.0 / (len * 0.7) } else { 0.0 }, hot1: false, hot2: false, column: (0x66, 0x1b, f32::from_bits(0x3f4a_0000)) };
    }

    /// modify_thermometer(RunePos, Cam[0] attention).
    pub fn modify_thermometer(&mut self) {
        if !self.thermometer.shown || self.items.rune_item.is_none() {
            self.thermometer.shown = false;
            return;
        }
        let a = self.items.rune_pos;
        let b = self.std_camera.as_ref().map_or([0.0; 3], |c| c.cam0_attention);
        let d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt();
        let f = ((d - 8.0) * self.thermometer.therm_const).clamp(0.0, 1.0);
        let f = 1.0 - (1.0 - f) * (1.0 - f);
        let t = &mut self.thermometer;
        let mut cue = None;
        if !t.hot2 && f < 0.25 {
            t.hot2 = true;
            cue = Some("AudioRuneGetCloser");
        } else if (t.hot2 || 0.25 <= f) && !t.hot1 && f < 0.025 {
            t.hot1 = true;
            cue = Some("AudioRuneIsNear");
        }
        let h = (1.0 - f) * 73.0;
        let r = h.round() as i32;
        t.column = (0x66 - r, r + 0x1b, (101.0 - h) * 0.007_812_5);
        if let Some(c) = cue {
            self.audio_rune_hint(c == "AudioRuneIsNear");
        }
    }

    /// The thermometer's blits (THERMBASE z 63912, THERMCOL z 63911).
    pub fn thermometer_blits(&self) -> Vec<Blit> {
        if !self.thermometer.shown {
            return Vec::new();
        }
        let (y, h, v0) = self.thermometer.column;
        let mut base = self.hud.make_pub(self.hud.lookup("THERMBASE"), 0x188, -1, -1, -1);
        base.z = f32::from_bits(0x4779_a800);
        let mut col = self.hud.make_pub(self.hud.lookup("THERMCOL"), 0x188, y, -1, -1);
        col.z = f32::from_bits(0x4779_a700);
        col.h = h as f32;
        col.crop[2] = v0;
        vec![base, col]
    }
}
