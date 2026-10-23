//! The screen saver: ScreenSaver 0x12acf8 (main, every frame before
//! DoOptions) and ScreenSaverStart/Update/End with the four flying weapons
//! (ScreenSaverInitWeap/CreateWeap/ResetWeap/StartWeap/UpdateWeap/EndWeap
//! 0x129ff0..0x12aaa0; research/hud.md "Screen saver").
//!
//! Outside the attract modes (game_mode & 0x8000), movies (0x400e) and mode
//! 0x4015, saver_timer counts vblanks while no player's PlayerControl +4
//! level word changes; at 36000 (ten minutes) ScreenSaverStart stops the
//! audio (AudioStopAll, AudioStreamStop), hides every node and the game
//! halts in ScreenSaver's own loop (ClockOncePerFrame, ScreenSaverUpdate,
//! PlayerControls, MBEndFrame) until a level word changes;
//! ScreenSaverEnd removes the weapons, shows the nodes again and clears the
//! controls (ClearAllPlayerControls(−2)); game mode 0x400b AudioSelect(1),
//! 0x4012 ShopMusicStart. The weapons (POWERUPS trees SAVER_AXE, SAVER_HAM,
//! SAVER_SWD, SAVER_WND) fly in the window camera's space: positions and
//! directions from ssave_initpos / ssave_initdir, ssave_speed 10 units per
//! 30 vblanks, pitch turning at ssave_dpitch 2π per 30 vblanks, turned back
//! at the frustum sides (MBWorldSphereClip(−1)) and the z limits
//! ssave_min_z 5 / ssave_max_z 40, restarted after ssave_endtime.
//! Presentation arithmetic (not compared): plain f32/f64 as decompiled.
use super::{Game, R};

type V3 = [f32; 3];

/// ssave_initpos 0x2209f8 / ssave_initdir 0x220a28 (ELF data).
const INIT_POS: [V3; 4] = [[20.0, 0.0, 1.0], [-20.0, 0.0, 1.0], [0.0, 20.0, 1.0], [0.0, -20.0, 1.0]];
const INIT_DIR: [V3; 4] = [[-1.0, 0.0, 1.0], [1.0, 0.0, 1.0], [0.0, -1.0, 1.0], [0.0, 1.0, 1.0]];
/// ssave_delay, ssave_endtime (seconds), ssave_speed, ssave_dpitch,
/// ssave_rand, ssave_min_z, ssave_max_z (0x2209cc..0x2209e4).
const DELAY: f32 = 3.0;
const END_TIME: f32 = 10.0;
const SPEED: f32 = 10.0;
const DPITCH: f32 = std::f32::consts::TAU;
const RAND: f32 = 0.1;
const MIN_Z: f32 = 5.0;
const MAX_Z: f32 = 40.0;
/// ssave_desc 0x220a58.
pub const TREES: [&str; 4] = ["SAVER_AXE", "SAVER_HAM", "SAVER_SWD", "SAVER_WND"];

/// One ssave_weap entry (0x90 bytes at 0x2c12c0).
#[derive(Debug, Clone, Default)]
pub struct SaverWeap {
    /// +0 position and +0x10 direction in the window camera's space.
    pub pos: V3,
    pub dir: V3,
    /// +0x20 pitch, +0x24 bounce state (0 waits to be inside, > 0 armed,
    /// < 0 just turned), +0x28 vblanks, +0x2c start delay, +0x30 end time.
    pub pitch: f32,
    pub bounce: i32,
    pub timer: i32,
    pub delay: i32,
    pub end: i32,
    /// +0x34 / +0x38 (set by StartWeap, unused by UpdateWeap).
    pub speed: f32,
    pub dpitch: f32,
    /// +0x3c node (shown) and the AnimateATree vblanks.
    pub shown: bool,
    pub anim_vb: i32,
    /// The node's rotation (CreateDirMatrix(dir) then PitchMat3(pitch), in
    /// camera space; the window matrix turns it into the world).
    pub rot: [V3; 3],
}

/// saver_timer 0x2209c8, saver_levels 0x220cb0 and the running saver.
#[derive(Debug, Clone, Default)]
pub struct ScreenSaver {
    pub timer: i32,
    pub levels: [u32; 4],
    pub active: bool,
    pub weaps: [SaverWeap; 4],
}

impl Game {
    fn saver_levels(&self) -> [u32; 4] {
        [0, 1, 2, 3].map(|k| self.controls.control[k].u32_(4))
    }

    /// ScreenSaver's test for one frame of `vb` vblanks (vb_elapsed_ctl):
    /// true when the saver starts (the caller runs it until
    /// screen_saver_frame returns false).
    pub fn screen_saver_check(&mut self, vb: i32) -> R<bool> {
        let mode = self.globals.game_mode;
        if mode & 0x8000 != 0 || mode == 0x400e || mode == 0x4015 {
            self.saver.timer = 0;
            return Ok(false);
        }
        self.saver.timer += vb;
        let levels = self.saver_levels();
        if levels != self.saver.levels {
            self.saver.timer = 0;
        }
        if self.saver.timer < 36000 {
            self.saver.levels = levels;
            return Ok(false);
        }
        self.screen_saver_start()?;
        Ok(true)
    }

    /// ScreenSaverStart 0x12aaf8: AudioStopAll, AudioStreamStop (the host
    /// stops its voices and the stream), optmenu_active 100 (saved), every
    /// node hidden, the four weapons initialised.
    fn screen_saver_start(&mut self) -> R<()> {
        self.audio_stop_all();
        self.events.push(super::Event::MusicStop);
        self.saver.active = true;
        for k in 0..4 {
            self.saver_init_weap(k)?;
        }
        Ok(())
    }

    /// One frame of ScreenSaver's loop: ClockOncePerFrame (the caller's
    /// `vb`), ScreenSaverUpdate (DoTexMods is the host's, then the four
    /// weapons), PlayerControls; false (after ScreenSaverEnd) once a level
    /// word differs from saver_levels.
    pub fn screen_saver_frame(&mut self, vb: i32, ports: &[super::pad::PadStatus; 8]) -> R<bool> {
        for k in 0..4 {
            self.saver_update_weap(k, vb)?;
        }
        self.update_controls(ports)?;
        if self.saver_levels() == self.saver.levels {
            return Ok(true);
        }
        self.screen_saver_end()?;
        Ok(false)
    }

    /// ScreenSaverEnd 0x12abe8 (then ScreenSaver's saver_timer = 0).
    fn screen_saver_end(&mut self) -> R<()> {
        for w in &mut self.saver.weaps {
            w.shown = false;
        }
        self.saver.active = false;
        self.saver.timer = 0;
        self.controls.clear_all_player_controls(-2)?;
        match self.globals.game_mode {
            0x400b => self.audio_select(1),
            0x4012 => self.shop_music_start(),
            _ => {}
        }
        Ok(())
    }

    /// ScreenSaverInitWeap 0x12a290: CreateWeap, ResetWeap, then the start
    /// delay k × 30 + RandInt(15) + 1.
    fn saver_init_weap(&mut self, k: usize) -> R<()> {
        self.saver_create_weap(k);
        self.saver_reset_weap(k)?;
        let r = self.rng.rand_int(0xf).unwrap_or(0) as i32;
        self.saver.weaps[k].delay = k as i32 * 0x1e + r + 1;
        Ok(())
    }

    /// ScreenSaverCreateWeap 0x12a158: the node and tree, the initial
    /// position and direction, +0xc and +0x1c cleared.
    fn saver_create_weap(&mut self, k: usize) {
        let w = &mut self.saver.weaps[k];
        w.shown = true;
        w.anim_vb = 0;
        w.pos = INIT_POS[k];
        w.dir = INIT_DIR[k];
    }

    /// ScreenSaverResetWeap 0x129ff0: back to the start position, timer 0,
    /// delay ssave_delay × (Random(1) + 0.5) × 60 + 1, end time
    /// ssave_endtime × (Random(1) + 0.5) × 60, the tree and node removed.
    fn saver_reset_weap(&mut self, k: usize) -> R<()> {
        let a = self.random_pub(1.0)?;
        let b = self.random_pub(1.0)?;
        let w = &mut self.saver.weaps[k];
        w.pos = INIT_POS[k];
        w.timer = 0;
        w.delay = (f64::from(DELAY) * (f64::from(a) + 0.5) * 60.0) as i32 + 1;
        w.end = (f64::from(END_TIME) * (f64::from(b) + 0.5) * 60.0) as i32;
        w.shown = false;
        Ok(())
    }

    /// ScreenSaverStartWeap 0x12a2f0: the node (CreateWeap when removed),
    /// the start position, the normalised start direction, pitch and bounce
    /// 0, +0x34 / +0x38 from ssave_speed / ssave_dpitch × (Random(0.2) +
    /// 0.9), delay 0, the node shown.
    fn saver_start_weap(&mut self, k: usize) -> R<()> {
        if !self.saver.weaps[k].shown {
            self.saver_create_weap(k);
        }
        let a = self.random_pub(0.2)?;
        let b = self.random_pub(0.2)?;
        let w = &mut self.saver.weaps[k];
        w.pos = INIT_POS[k];
        w.dir = normal(INIT_DIR[k]);
        w.pitch = 0.0;
        w.bounce = 0;
        w.speed = (f64::from(SPEED) * (f64::from(a) + 0.9)) as f32;
        w.dpitch = (f64::from(DPITCH) * (f64::from(b) + 0.9)) as f32;
        w.delay = 0;
        w.shown = true;
        Ok(())
    }

    /// ScreenSaverUpdateWeap 0x12a488 for weapon `k` over `vb` vblanks.
    fn saver_update_weap(&mut self, k: usize, vb: i32) -> R<()> {
        let t = (f64::from(vb) / 30.0) as f32;
        let step = t * SPEED;
        {
            let w = &mut self.saver.weaps[k];
            w.timer += vb;
            if w.timer < w.delay {
                return Ok(());
            }
        }
        if 0 < self.saver.weaps[k].delay {
            self.saver_start_weap(k)?;
        }
        let w = &mut self.saver.weaps[k];
        for i in 0..3 {
            w.pos[i] += step * w.dir[i];
        }
        let p = -DPITCH * t + w.pitch;
        w.pitch = if std::f64::consts::PI < f64::from(p) {
            (f64::from(p) - std::f64::consts::TAU) as f32
        } else if f64::from(p) <= -std::f64::consts::PI {
            (f64::from(p) + std::f64::consts::TAU) as f32
        } else {
            p
        };
        let code = if MAX_Z < w.pos[2] {
            5
        } else if w.pos[2] < MIN_Z {
            6
        } else {
            match &self.camera {
                Some(cam) => cam.sphere_clip(-1.0, cam.to_world(w.pos))?,
                None => 0,
            }
        };
        if w.bounce < 1 {
            if w.bounce < 0 {
                w.bounce = vb;
            } else if code == 0 {
                w.bounce = 1;
            }
        } else {
            let turn = |g: &mut Game, axis: usize, sign: f32, other: usize| -> R<()> {
                let r = g.random_pub(0.5)?;
                let j = g.random_pub(2.0)?;
                let w = &mut g.saver.weaps[k];
                w.dir[axis] = if 0.0 < sign { r + 0.5 } else { 0.0 - (r + 0.5) };
                w.dir[other] += -RAND + j * RAND;
                Ok(())
            };
            match code {
                1 => turn(self, 0, 1.0, 1)?,
                2 => turn(self, 0, -1.0, 1)?,
                3 => turn(self, 1, -1.0, 0)?,
                4 => turn(self, 1, 1.0, 0)?,
                5 => {
                    let r = self.random_pub(0.5)?;
                    let w = &mut self.saver.weaps[k];
                    w.dir[2] = 0.0 - (r + 0.5);
                    if w.end < w.timer {
                        return self.saver_reset_weap(k);
                    }
                }
                6 => {
                    let r = self.random_pub(0.5)?;
                    self.saver.weaps[k].dir[2] = r + 0.5;
                }
                _ => {}
            }
            if code != 0 {
                let w = &mut self.saver.weaps[k];
                w.dir = normal(w.dir);
                w.bounce = -10;
            }
        }
        let w = &mut self.saver.weaps[k];
        w.rot = rot_of(w.dir, w.pitch);
        w.anim_vb += vb;
        Ok(())
    }
}

/// NormalVector (presentation precision).
fn normal(v: V3) -> V3 {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l == 0.0 {
        return v;
    }
    [v[0] / l, v[1] / l, v[2] / l]
}

/// CreateDirMatrix(dir, 0) (CreateYPRMatrix(atan2(y, |xz|) × 0.5,
/// atan2(x, z), 0)) then PitchMat3(pitch) (presentation precision).
fn rot_of(dir: V3, pitch: f32) -> [V3; 3] {
    let h = (dir[0] * dir[0] + dir[2] * dir[2]).sqrt();
    let m = super::enemy_ai::create_ypr_matrix([dir[1].atan2(h) * 0.5, dir[0].atan2(dir[2]), 0.0]).unwrap_or([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0; 4]]);
    let mut r = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
    let _ = crate::items::pitch_mat3(&mut r, pitch);
    r
}
