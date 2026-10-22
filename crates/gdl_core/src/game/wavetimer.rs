//! Timed levels (LEVL +0 flag 4, the treasure rooms): init_wave_timer
//! 0x124b.. (init_next_level), do_wave_timer 0x124db0 (game_main) and
//! modify_sandglass 0x124c88. WaveTimer counts LEVL +0xc + 0.99 seconds
//! down while no stage start, pause or help is up; at 0 skip_wave becomes
//! 0xd (back to recorded_wave) and the notices go. The sandglass: TIMER at
//! (1, 1) (z 63913), the TIMER_SAND halves (z 63911) cropped by the spent
//! fraction, SAND_ANIM (0x3f, 0x3a, z 63912) while the timer runs.
use super::hud::Blit;
use super::{Game, R};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub};

#[derive(Clone, Debug, Default)]
pub struct WaveTimer {
    pub max: f32,
    pub shown: bool,
    pub anim: bool,
    /// modify_sandglass's spent fraction.
    pub spent: f32,
}

impl Game {
    fn timed_level(&self) -> bool {
        self.level.len() >= 0x10 && self.level_u32(0) & 4 != 0
    }

    /// init_wave_timer: WaveTimerMax = LEVL +0xc + 0.99 on timed levels;
    /// WaveComplete = 0; the sandglass is shown on timed levels.
    pub fn init_wave_timer(&mut self) -> R<()> {
        let timed = self.timed_level();
        if timed {
            let max = add(f32::from(self.level_i16(0xc)), f32::from_bits(0x3f7d_70a4))?;
            self.wave_timer.max = max;
            self.globals.wave_timer = max;
        }
        self.globals.wave_complete = 0;
        self.wave_timer.shown = timed;
        self.wave_timer.anim = false;
        self.wave_timer.spent = 0.0;
        Ok(())
    }

    /// do_wave_timer (AuxLevels 0x10, the debug timer, not modelled).
    pub fn do_wave_timer(&mut self) -> R<()> {
        let running = self.globals.start_of_stage == 0;
        let timed = self.timed_level();
        if running && timed {
            self.wave_timer.anim = self.wave_timer.shown;
        }
        if add(f32::from(self.level_i16(0xc)), f32::from_bits(0x3f7d_70a4))? < self.globals.wave_timer {
            self.wave_timer.max = 5.0;
            self.globals.wave_timer = 5.0;
        }
        if !(timed && self.globals.paused == 0 && self.globals.help == 0 && running) {
            return Ok(());
        }
        let before = self.globals.wave_timer as i32;
        self.globals.wave_timer = sub(self.globals.wave_timer, self.globals.spf)?;
        if self.globals.wave_timer <= 0.0 {
            self.wave_timer.shown = false;
            self.wave_timer.anim = false;
            self.globals.wave_timer = 0.0;
            self.globals.skip_wave = 0xd;
            self.hud.init_got_it();
            return Ok(());
        }
        let now = self.globals.wave_timer as i32;
        if self.globals.wave_complete == 0 && before != now {
            // AudioSecretTimer(n): 0x19 at 0, else 0x17 / 0x18 alternating.
            let tick = if now == 0 { 0x19 } else if now & 1 != 0 { 0x18 } else { 0x17 };
            self.sfx(tick, None, 0x7f, 4);
            if now == 8 {
                // AudioTimeRunsOut: AudioFX(0xc0085, 0xe0, 0x14).
                self.sfx(0xc_0085, None, 0xe0, 0x14);
            } else if now < 6 && self.globals.skip_wave == 0 {
                // AudioCountDown(n): snd.157[n] = 0x2000b − n, priority n + 0x15.
                self.sfx(0x2_000b - now, None, 0xe0, now + 0x15);
            }
        }
        // modify_sandglass(max × 60, timer × 60).
        let (m, t) = (mul(self.wave_timer.max, 60.0)?, mul(self.globals.wave_timer, 60.0)?);
        self.wave_timer.spent = crate::numeric::div_rn(sub(m, t)?, m)?;
        Ok(())
    }

    /// The sandglass blits (modify_sandglass's crops, sizes and y).
    pub fn wave_timer_blits(&self) -> Vec<Blit> {
        if !self.wave_timer.shown {
            return Vec::new();
        }
        let f = self.wave_timer.spent;
        let round = |v: f32| (v + if v < 0.0 { -0.5 } else { 0.5 }) as i32;
        let mut out = Vec::new();
        let mut base = self.hud.make_pub(self.hud.lookup("TIMER"), 1, 1, -1, -1);
        base.z = f32::from_bits(0x4779_a900);
        out.push(base);
        let a = round(f * 39.0);
        let mut top = self.hud.make_pub(self.hud.lookup("TIMER_SAND"), 1, a + 0x18, -1, 0x29 - a);
        top.z = f32::from_bits(0x4779_a700);
        top.crop[2] = (f * 41.0 + 23.0) * 0.007_812_5;
        top.crop[3] = 0.5;
        out.push(top);
        let b = round(f * 38.0);
        let mut bottom = self.hud.make_pub(self.hud.lookup("TIMER_SAND"), 1, 0x6a - b, -1, b + 0x17);
        bottom.z = f32::from_bits(0x4779_a700);
        bottom.crop[2] = (105.0 - f * 38.0) * 0.007_812_5;
        bottom.crop[3] = 1.0;
        out.push(bottom);
        if self.wave_timer.anim {
            let mut s = self.hud.make_pub(self.hud.lookup("SAND_ANIM"), 0x3f, 0x3a, -1, -1);
            s.z = f32::from_bits(0x4779_a800);
            out.push(s);
        }
        out
    }
}
