//! The attract flyby (init_flyby 0x182ed0 / do_flyby 0x182f50, game mode
//! 0x8008): a level runs without players while the old camera system's
//! Cam[0] (Cam 0x23e4f0, 0x1a0 bytes per camera) flies along the level's
//! attract transmitters (locator kinds 3/4, AddTransmitter flag 1).
//! Transcribed from instructions: InitCamera 0x167748's flyby branch,
//! do_camera 0x1738e8 → change_camera_mode 0x1733a0 → move_camera 0x172b28
//! (mode 2) → MoveCam_flyby_mode 0x16d510 with att_rotate_camera 0x16d198,
//! att_receive_yaw 0x16c9c8, att_dest_pyr 0x16c868 (its xyprintf is a debug
//! print), get_pitch 0x168aa0 and fix_radius 0x1670e0; cam_orient_to
//! 0x169e40 (CameraSupervisor returns at once for camera mode 2 and no
//! shake runs in the attract), ProcCamera 0x1671e0 and MBCameraUpdate.
//!
//! Every attract level on the disc (LEVL +4 bit 2: A2 B5 C4 D1 F1 G1 H2 I4
//! J2 K1 L1) has CAMS +0x26 = −1 and a StartAttTransmitter, so only
//! MoveCam_flyby_mode's transmitter path runs; attract_cam_mode 0 (drift)
//! and 1 (orbit) are reported Unsupported. set_logo(1)'s LOGO_B blit
//! (MBOX_NewBlit at 0x170, −0x130) is not drawn: it is not on screen in the
//! reference capture (private/reports/reference-attract-flyby-v1-*.png).
use super::camera::{o, Transmitter};
use super::critter_ai::slow_normal_vector;
use super::enemies::point_line_coll;
use super::enemy_ai::{create_ypr_matrix, fix_angle, get_yaw};
use super::{unsupported, Game, R};
use crate::libm::atan2f;
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sqrt_rn, sub_guarded_rz as sub};
use crate::player_collision::world_vector;

type V3 = [f32; 3];
const PI: f32 = f32::from_bits(0x4049_0fdb);
const TWO_PI: f32 = f32::from_bits(0x40c9_0fdb);
/// att_move_*_vel step and limit (0.05° / 0.5° per vblank).
const VEL_STEP: f32 = f32::from_bits(0x3a64_c389);
const VEL_MAX: f32 = f32::from_bits(0x3c0e_fa36);

fn wrap(v: f32) -> R<f32> {
    Ok(if PI < v {
        sub(v, TWO_PI)?
    } else if v <= -PI {
        add(v, TWO_PI)?
    } else {
        v
    })
}

fn dist3(a: V3, b: V3) -> R<f32> {
    let d = [sub(a[0], b[0])?, sub(a[1], b[1])?, sub(a[2], b[2])?];
    Ok(sqrt_rn(add(add(mul(d[0], d[0])?, mul(d[1], d[1])?)?, mul(d[2], d[2])?)?)?)
}

fn rows_of(m: &[[f32; 4]; 4]) -> [V3; 3] {
    [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]]
}

/// get_pitch 0x168aa0(a, b): the pitch of a − b (negative when a is above).
pub fn get_pitch(a: V3, b: V3) -> R<f32> {
    let dx = sub(a[0], b[0])?;
    let dy = sub(a[1], b[1])?;
    let dz = sub(a[2], b[2])?;
    let mut h = sqrt_rn(add(mul(dx, dx)?, mul(dz, dz)?)?)?;
    if h <= f32::from_bits(0x3a83_126f) {
        h = f32::from_bits(0x3a83_126f);
    }
    let mut p = atan2f(dy.abs(), h)?;
    if 0.0 <= dy {
        p = -p;
    }
    fix_angle(p)
}

/// fix_radius 0x1670e0(r, a, b): b moves along a→b to distance r from a.
fn fix_radius(r: f32, a: V3, b: &mut V3) -> R<()> {
    let mut d = [sub(b[0], a[0])?, sub(b[1], a[1])?, sub(b[2], a[2])?];
    slow_normal_vector(&mut d)?;
    for k in 0..3 {
        b[k] = add(mul(r, d[k])?, a[k])?;
    }
    Ok(())
}

/// Cam[0]'s fields the flyby uses.
#[derive(Debug, Clone, Default)]
pub struct OldCam {
    /// +0x10 matrix rows (right, up, forward) and row 3 at +0x40.
    pub rows: [V3; 3],
    pub row3: V3,
    /// +0x70 eye, +0xb0 pitch/yaw/roll, +0xd0 radius, +0x140 attention.
    pub eye: V3,
    pub pyr: V3,
    pub radius: f32,
    pub attention: V3,
    /// +0xd4 (move_camera sets −1 on its first pass).
    pub word_d4: i32,
    /// +0xdc: change_camera_mode's start counter (vb_elapsed 1 while < 2).
    pub start_count: i32,
}

/// The flyby's state: Cam[0] and the att_* globals.
#[derive(Debug, Clone, Default)]
pub struct Flyby {
    pub cam: OldCam,
    /// transmitter1/2 0x23f2b8/0x23f2bc (globals that outlive a flyby) and
    /// current_transmitter 0x23f2c0.
    pub transmitter1: i32,
    pub transmitter2: i32,
    pub current: i32,
    /// att_move_pitch/yaw 0x2c3334/0x2c3338, their velocities 0x2c332c/0x2c3330.
    pub move_pitch: f32,
    pub move_yaw: f32,
    pub move_pitch_vel: f32,
    pub move_yaw_vel: f32,
    /// adp/ady 0x2c3350/0x2c3354: pitch/yaw from transmitter2 to the eye.
    pub adp: f32,
    pub ady: f32,
    /// att_unit_dy/dp 0x2c3358/0x2c335c and their previous values.
    pub unit_dy: f32,
    pub unit_dp: f32,
    pub unit_dy_old: f32,
    pub unit_dp_old: f32,
    /// yaw_dest 0x23f284, dest_pitch 0x23f288, default_pitch 0x2c32f0.
    pub yaw_dest: f32,
    pub dest_pitch: f32,
    pub default_pitch: f32,
    /// rot_dir 0x2c32e4, pitch_dir 0x2c32ec.
    pub rot_dir: i32,
    pub pitch_dir: i32,
    /// gravity 0x2c3368: the pull of transmitter1 over transmitter2.
    pub gravity: f32,
    /// yaw_stats 0x2c3310 and the two before it.
    pub yaw_stats: [f32; 3],
    /// flyby_seq_done 0x249c64, timer1, exit_attract.
    pub seq_done: i32,
    pub timer1: i32,
    pub exit: bool,
}

impl Flyby {
    /// att_receive_yaw 0x16c9c8: the two nearest unvisited attract
    /// transmitters to the eye, gravity, the destination yaw/pitch by the
    /// eye's progress along them, and the turn rates when the current one
    /// changes; a transmitter is marked visited (+2 = 0) once passed.
    fn receive_yaw(&mut self, ts: &mut [Transmitter], vb: i32) -> R<()> {
        let prev_current = self.current;
        let prev_t1 = self.transmitter1;
        let eye = self.cam.eye;
        let (mut d1, mut d2) = (100000.0f32, 100000.0f32);
        let (mut y1, mut p1, mut y2, mut p2) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
        let mut n = 0;
        for (k, t) in ts.iter().enumerate() {
            if t.flag != 1 || t.word_02 == 0 {
                continue;
            }
            let d = dist3(eye, t.pos)?;
            if d < d1 {
                self.transmitter2 = self.transmitter1;
                (d2, y2, p2) = (d1, y1, p1);
                self.transmitter1 = k as i32;
                (d1, y1, p1) = (d, t.yaw, t.pitch);
            } else if d < d2 {
                self.transmitter2 = k as i32;
                (d2, y2, p2) = (d, t.yaw, t.pitch);
            } else {
                continue;
            }
            n += 1;
        }
        if n == 1 {
            self.transmitter2 = self.transmitter1;
            (d2, y2, p2) = (d1, y1, p1);
        }
        if n != 0 {
            let sum = add(d1, d2)?;
            if n == 1 || sum == 0.0 {
                self.seq_done += vb;
                return Ok(());
            }
            let (t1, t2) = (self.transmitter1 as usize, self.transmitter2 as usize);
            let (a, b) = (ts[t1].pos, ts[t2].pos);
            let (_, closest) = point_line_coll(eye, a, b)?;
            let seg = dist3(a, b)?;
            let ratio = div(dist3(a, closest)?, seg)?;
            let mut g = div(d1, sum)?;
            if f32::from_bits(0x3e99_999a) <= g {
                g = 1.0;
            } else if f32::from_bits(0x3e4c_cccd) <= g {
                g = sub(1.0, mul(sub(g, f32::from_bits(0x3e99_999a))?, 8.0)?)?;
            }
            self.gravity = g;
            let pick = if ratio <= 0.5 {
                Some((y1, p1, self.transmitter1))
            } else if 0.5 < ratio {
                Some((y2, p2, self.transmitter2))
            } else {
                None
            };
            if let Some((y, p, t)) = pick {
                self.yaw_dest = y;
                self.dest_pitch = p;
                self.current = t;
                self.pitch_dir = if self.default_pitch < p { 1 } else { -1 };
            }
            let d = sub(self.yaw_dest, self.cam.pyr[1])?;
            self.rot_dir = if d < -PI {
                1
            } else if d < 0.0 {
                -1
            } else if d < PI {
                1
            } else {
                -1
            };
            if prev_current != self.current {
                let mut dd = mul(dist3(eye, ts[self.current as usize].pos)?, 5.0)?;
                if dd == 0.0 {
                    (self.unit_dy, self.unit_dy_old, self.unit_dp, self.unit_dp_old) = (0.0, 0.0, 0.0, 0.0);
                } else {
                    if dd < 1.0 {
                        dd = 1.0;
                    }
                    let mut dy = sub(self.yaw_dest, self.cam.pyr[1])?;
                    if PI < dy {
                        dy = sub(TWO_PI, dy)?;
                    }
                    let limit = |v: f32, old: f32, max: f32, step: f32| -> R<f32> {
                        let v = if max <= v { max } else { v };
                        Ok(if step <= sub(v, old)?.abs() {
                            if old < v {
                                add(old, step)?
                            } else {
                                sub(old, step)?
                            }
                        } else {
                            v
                        })
                    };
                    self.unit_dy = limit(div(dy, dd)?.abs(), self.unit_dy_old, f32::from_bits(0x3c2b_92a7), VEL_STEP)?;
                    let dp = div(sub(self.dest_pitch, self.default_pitch)?, dd)?.abs();
                    self.unit_dp = limit(dp, self.unit_dp_old, f32::from_bits(0x3bcd_e32f), f32::from_bits(0x3a09_421f))?;
                    self.unit_dy_old = self.unit_dy;
                    self.unit_dp_old = self.unit_dp;
                }
            }
        }
        if 0 <= prev_t1
            && prev_t1 == self.transmitter2
            && let Some(t) = ts.get_mut(prev_t1 as usize)
        {
            t.word_02 = 0;
        }
        Ok(())
    }

    /// att_dest_pyr 0x16c868: pitch/yaw from transmitter2 to the eye.
    fn dest_pyr(&mut self, ts: &[Transmitter]) -> R<()> {
        if self.transmitter1 != self.transmitter2 {
            let t2 = ts[self.transmitter2 as usize].pos;
            self.adp = get_pitch(self.cam.eye, t2)?;
            self.ady = get_yaw(self.cam.eye, t2)?;
        }
        Ok(())
    }

    /// att_rotate_camera 0x16d198: turn the yaw toward yaw_dest by vb ×
    /// att_unit_dy in rot_dir, snapping on arrival or on a ±π crossing.
    fn rotate_camera(&mut self, ts: &mut [Transmitter], vb: i32) -> R<()> {
        let old = self.cam.pyr[1];
        self.yaw_stats = [old, self.yaw_stats[0], self.yaw_stats[1]];
        self.receive_yaw(ts, vb)?;
        let step = mul(vb as f32, self.unit_dy)?;
        let dest = self.yaw_dest;
        fn arrive(f: &mut Flyby) {
            f.cam.pyr[1] = f.yaw_dest;
            f.rot_dir = 0;
        }
        if 0 < self.rot_dir && self.cam.pyr[1] != dest {
            let y = wrap(add(step, self.cam.pyr[1])?)?;
            self.cam.pyr[1] = y;
            if y < old {
                if old < dest || dest <= y {
                    arrive(self);
                }
            } else if sub(y, dest)? < PI && dest <= y {
                arrive(self);
            }
        } else if self.rot_dir < 0 && self.cam.pyr[1] != dest {
            let y = wrap(sub(self.cam.pyr[1], step)?)?;
            self.cam.pyr[1] = y;
            if old < y {
                if dest < old || y <= dest {
                    arrive(self);
                }
            } else if sub(dest, y)? < PI && y <= dest {
                arrive(self);
            }
        } else {
            arrive(self);
        }
        let y = self.cam.pyr[1];
        let crossed = (0.0 < y && 0.0 < self.yaw_stats[1] && self.yaw_stats[0] < 0.0) || (y < 0.0 && self.yaw_stats[1] < 0.0 && 0.0 < self.yaw_stats[0]);
        if crossed {
            arrive(self);
        }
        Ok(())
    }

    /// MoveCam_flyby_mode 0x16d510, StartAttTransmitter path: steer the
    /// eye (0.2 per vblank) toward the gravity blend of transmitters 1 and
    /// 2 with eased pitch/yaw velocities, ease default_pitch toward
    /// dest_pitch, and look `radius` ahead along the camera's pitch/yaw.
    fn move_flyby(&mut self, ts: &mut [Transmitter], vb: i32) -> R<()> {
        if self.cam.start_count < 2 {
            return Ok(());
        }
        let step = mul(vb as f32, f32::from_bits(0x3e4c_cccd))?;
        self.cam.pyr[0] = -self.cam.pyr[0];
        self.rotate_camera(ts, vb)?;
        self.cam.pyr[0] = -self.cam.pyr[0];
        self.dest_pyr(ts)?;
        let (t1, t2) = (self.transmitter1, self.transmitter2);
        let eye = self.cam.eye;
        let v = if t1 != t2 {
            let (a, b) = (ts[t1 as usize].pos, ts[t2 as usize].pos);
            let g = self.gravity;
            let h = sub(1.0, g)?;
            let mut v = [0.0; 3];
            for k in 0..3 {
                v[k] = add(mul(sub(a[k], eye[k])?, g)?, mul(h, sub(b[k], eye[k])?)?)?;
            }
            v
        } else {
            self.seq_done += vb;
            [0.0; 3]
        };
        let len = sqrt_rn(add(add(mul(v[0], v[0])?, mul(v[1], v[1])?)?, mul(v[2], v[2])?)?)?;
        if 4.0 <= len || t1 != t2 {
            let vbf = vb as f32;
            // Pitch toward adp with an eased velocity.
            let settle;
            if self.move_pitch < self.adp {
                if self.move_pitch_vel < VEL_MAX {
                    self.move_pitch_vel = add(self.move_pitch_vel, VEL_STEP)?;
                }
                self.move_pitch = add(mul(vbf, self.move_pitch_vel)?, self.move_pitch)?;
                settle = self.adp <= self.move_pitch;
            } else if self.adp < self.move_pitch {
                if -VEL_MAX < self.move_pitch_vel {
                    self.move_pitch_vel = sub(self.move_pitch_vel, VEL_STEP)?;
                }
                self.move_pitch = add(mul(vbf, self.move_pitch_vel)?, self.move_pitch)?;
                settle = self.move_pitch <= self.adp;
            } else {
                settle = true;
            }
            if settle {
                if 0.0 < self.move_pitch_vel {
                    self.move_pitch_vel = sub(self.move_pitch_vel, VEL_STEP)?;
                } else if self.move_pitch_vel < 0.0 {
                    self.move_pitch_vel = add(self.move_pitch_vel, VEL_STEP)?;
                }
            }
            // Yaw toward ady; the step uses the velocity before this update.
            let d = wrap(sub(self.ady, self.move_yaw)?)?;
            let turn = mul(vbf, self.move_yaw_vel)?;
            let mut settle = false;
            if 0.0 < d {
                if self.move_yaw_vel < VEL_MAX {
                    self.move_yaw_vel = add(self.move_yaw_vel, VEL_STEP)?;
                }
                settle = 0.0 <= turn && d <= turn;
            } else if d < 0.0 {
                if -VEL_MAX < self.move_yaw_vel {
                    self.move_yaw_vel = sub(self.move_yaw_vel, VEL_STEP)?;
                }
                settle = turn <= 0.0 && turn <= d;
            } else if 0.0 < self.move_yaw_vel {
                self.move_yaw_vel = sub(self.move_yaw_vel, VEL_STEP)?;
            } else if self.move_yaw_vel < 0.0 {
                self.move_yaw_vel = add(self.move_yaw_vel, VEL_STEP)?;
            }
            if settle {
                if 0.0 < self.move_yaw_vel {
                    self.move_yaw_vel = sub(self.move_yaw_vel, VEL_STEP)?;
                } else if self.move_yaw_vel < 0.0 {
                    self.move_yaw_vel = add(self.move_yaw_vel, VEL_STEP)?;
                }
            }
            self.move_yaw = wrap(add(turn, self.move_yaw)?)?;
            let m = create_ypr_matrix([self.move_pitch, wrap(add(self.move_yaw, PI)?)?, 0.0])?;
            let d = world_vector([0.0, 0.0, step], &rows_of(&m))?;
            for (e, d) in self.cam.eye.iter_mut().zip(d) {
                *e = add(*e, d)?;
            }
        }
        // default_pitch eases toward dest_pitch at att_unit_dp.
        let dp = mul(vb as f32, self.unit_dp)?;
        let done = if 0.0 < sub(self.dest_pitch, self.default_pitch)? {
            self.default_pitch = add(dp, self.default_pitch)?;
            self.dest_pitch <= self.default_pitch
        } else {
            self.default_pitch = sub(self.default_pitch, dp)?;
            self.default_pitch <= self.dest_pitch
        };
        if done {
            self.default_pitch = self.dest_pitch;
        }
        self.cam.pyr[0] = self.default_pitch;
        self.look_ahead()
    }

    /// attention = eye + WorldVector((0, 0, radius), YPR(pyr)), then
    /// fix_radius.
    fn look_ahead(&mut self) -> R<()> {
        let m = create_ypr_matrix(self.cam.pyr)?;
        let v = world_vector([0.0, 0.0, self.cam.radius], &rows_of(&m))?;
        for (k, v) in v.into_iter().enumerate() {
            self.cam.attention[k] = add(self.cam.eye[k], v)?;
        }
        fix_radius(self.cam.radius, self.cam.eye, &mut self.cam.attention)
    }
}

impl Game {
    /// init_flyby → new_attract_mode(0x8008, attract_wave) → InitCamera(0)
    /// on the level already built (GauntWorldInit). `transmitters` are the
    /// transmitter1/2 globals from the previous flyby, `yaw_off0` is
    /// yaw_off[0] (InitCamera's yaw_dest for yaw_mode 0).
    pub fn init_flyby(&mut self, transmitters: (i32, i32), yaw_off0: f32) -> R<Flyby> {
        let g = &mut self.globals;
        g.game_mode = 0x8008;
        g.paused = 0;
        g.skip_wave = 0;
        g.cam_wait = 0;
        g.start_of_stage = 0;
        g.end_of_stage = 0;
        g.active_players = 0;
        for k in 0..4 {
            self.inactivate_player(k);
        }
        let Some(cam) = self.std_camera.as_mut() else { return unsupported("init_flyby without a camera") };
        let mode = cam.attract_cam_mode;
        let Some(start) = cam.start_att.filter(|_| mode < 0) else {
            return unsupported(format!("flyby attract_cam_mode {mode} (no disc attract level uses it)"));
        };
        let mut f = Flyby {
            transmitter1: transmitters.0,
            transmitter2: transmitters.1,
            current: -1,
            rot_dir: 1,
            pitch_dir: 1,
            default_pitch: f32::from_bits(0x3f33_3333),
            dest_pitch: f32::from_bits(0x3f33_3333),
            ..Default::default()
        };
        let t = cam.transmitters[start].clone();
        f.cam.rows = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        f.cam.eye = t.pos;
        f.cam.pyr = [t.pitch, t.yaw, t.roll];
        f.cam.radius = 20.0;
        let m = create_ypr_matrix(f.cam.pyr)?;
        let v = world_vector([0.0, 0.0, 20.0], &rows_of(&m))?;
        for (k, v) in v.into_iter().enumerate() {
            f.cam.attention[k] = add(f.cam.eye[k], v)?;
        }
        // set_camera_mode(0, 2, 1); +0xd4 = 0; cam_orient_to; Cam = 1.
        f.cam.word_d4 = 0;
        Self::orient(&mut f)?;
        f.receive_yaw(&mut cam.transmitters, self.globals.vb_elapsed)?;
        f.dest_pyr(&cam.transmitters)?;
        f.move_pitch = f.adp;
        f.move_yaw = f.ady;
        f.dest_pitch = f.cam.pyr[0];
        f.default_pitch = f.cam.pyr[0];
        // LAB_00168308: yaw_dest = yaw_off[yaw_mode 0], row 3 = eye,
        // yaw_stats cleared, the 60° window, ProcCamera(0, 0).
        f.yaw_dest = yaw_off0;
        f.yaw_stats = [0.0; 3];
        f.cam.row3 = f.cam.eye;
        self.flyby_camera_update(&f, true)?;
        Ok(f)
    }

    /// cam_orient_to 0x169e40: LookInDirection(attention − eye) into the
    /// Cam matrix.
    fn orient(f: &mut Flyby) -> R<()> {
        let c = &f.cam;
        let dir = [sub(c.attention[0], c.eye[0])?, sub(c.attention[1], c.eye[1])?, sub(c.attention[2], c.eye[2])?];
        let mut r = super::record::Record::zeroed(o::SIZE);
        super::camera::Camera::look_in_dir_pub(&mut r, dir)?;
        f.cam.rows = [r.v3(o::MAT), r.v3(o::MAT + 0x10), r.v3(o::MAT + 0x20)];
        Ok(())
    }

    /// MBCameraUpdate(row 3, matrix) into the window camera (and the StdCam
    /// record's eye/target for the item and enemy visibility tests).
    fn flyby_camera_update(&mut self, f: &Flyby, init: bool) -> R<()> {
        let Some(cam) = self.std_camera.as_mut() else { return Ok(()) };
        let c = &f.cam;
        for (k, r) in c.rows.iter().enumerate() {
            cam.std.set_v3(o::MAT + 0x10 * k, *r);
        }
        cam.std.set_v3(o::EYE, c.row3);
        cam.std.set_v3(o::TARGET, c.attention);
        if init {
            // InitCamera's window: WindowBottomLim is 0 outside the level
            // branch (0x40 there), so ChangeWindow → MBWindowSetRegion(0,
            // width, height, 0) leaves no viewport inset and the window
            // height is the frame's; then MBWindowSetHang(60°).
            cam.window.viewport = [0.0; 4];
            cam.window.mbox_height = cam.window.frame[1] as f32;
            cam.set_window_hang(f32::from_bits(0x3f86_0a92))?;
        }
        let mut m = cam.std.clone();
        cam.mb_camera_update_pub(&mut m);
        // MBEndFrame's pbWinSetup.
        let w = cam.window.clone();
        cam.pb.update(&w)?;
        self.sync_view_pub();
        Ok(())
    }

    /// One flyby update: game_main's 0x8008 case (world_update, do_items,
    /// do_enemies, enemy_update — DoTexMods, presentation — and do_flyby),
    /// then main's do_camera. True when do_flyby leaves (exit_attract: the
    /// host calls init_attract_mode(−1)).
    pub fn flyby_step(&mut self, f: &mut Flyby) -> bool {
        self.frame_main += 1;
        self.main_audio(self.globals.vb_elapsed);
        self.do_lighting();
        self.hud.begin_frame();
        self.set_player_vars();
        let spf = self.globals.spf;
        self.globals.time += spf;
        self.globals.atime += spf;
        self.globals.itime = (self.globals.time * 1000.0) as i32;
        // world_update: start_enemyinfo, then nothing while paused or in help.
        self.start_enemyinfo = self.globals.start_of_stage == 0 || self.level.get(0x48..0x4c).is_some_and(|b| b != [0; 4]);
        if self.globals.paused == 0 && self.globals.help == 0 {
            if let Err(e) = self.do_world_animation() {
                self.log.note("world_update", &e);
            }
            self.sync_collision();
            if let Err(e) = self.setup_dyn_grid() {
                self.log.note("world_update", &e);
            }
            if let Err(e) = self.world_update_lightning() {
                self.log.note("world_update", &e);
            }
        }
        if let Err(e) = self.process_items() {
            self.log.note("do_items", &e);
        }
        if let Err(e) = self.do_enemies_camera() {
            self.log.note("do_enemies", &e);
        }
        let done = self.do_flyby(f);
        if !done
            && let Err(e) = self.flyby_camera(f)
        {
            self.log.note("flyby camera", &e);
        }
        done
    }

    /// do_flyby 0x182f50. JoyLevels & 0x300000 held stops the timer
    /// (newcam_mode 0). attract_advance's Start / exit buttons are the
    /// host's (the front end leaves the attract sequence on Start).
    fn do_flyby(&mut self, f: &mut Flyby) -> bool {
        if self.globals.paused != 0 {
            return false;
        }
        let vb = self.globals.vb_elapsed;
        let hold = self.controls.joy_levels[0] & 0x30_0000 != 0;
        if !hold {
            f.timer1 += vb;
        }
        if f.exit {
            self.audio_stop_all();
            return true;
        }
        if !hold && ((0x1e < f.seq_done && self.globals.aux_levels & 4 == 0) || 0xe10 < f.timer1) {
            f.exit = true;
        }
        if let Err(e) = self.process_effects() {
            self.log.note("ProcessEffects", &e);
        }
        if !f.exit {
            let blits = self.hud.glow_text(1.0, -0x100, 0x140, "Press Start", self.frame_main as u32);
            self.hud.push_temp(blits);
        }
        false
    }

    /// do_camera 0x1738e8 for Cam[0] in mode 2: change_camera_mode (the
    /// first two updates force vb_elapsed 1), move_camera (MoveCam_flyby_mode,
    /// get_attn_pos keeps the attention, cam_orient_to), ProcCamera (zero
    /// deltas; row 3 = eye) and MBCameraUpdate.
    fn flyby_camera(&mut self, f: &mut Flyby) -> R<()> {
        if self.globals.paused != 0 || self.globals.help != 0 {
            return self.flyby_camera_update(f, false);
        }
        self.globals.cam_wait = 0;
        if (0..2).contains(&f.cam.start_count) {
            self.globals.vb_elapsed = 1;
            f.cam.start_count += 1;
        }
        let vb = self.globals.vb_elapsed;
        if f.cam.word_d4 == 0 {
            f.cam.word_d4 = -1;
        }
        let Some(cam) = self.std_camera.as_mut() else { return Ok(()) };
        f.move_flyby(&mut cam.transmitters, vb)?;
        Self::orient(f)?;
        f.cam.row3 = f.cam.eye;
        self.flyby_camera_update(f, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(pos: V3) -> Transmitter {
        Transmitter { flag: 1, radius: 0, word_02: 1, pos, pitch: -0.5, yaw: 1.0, roll: 0.0, locator: 0, id: 0 }
    }

    /// Synthetic: the two nearest unvisited attract transmitters are chosen;
    /// gravity is 1 once the nearer holds 30 % of the summed distance and
    /// 1 − (g − 0.3)·8 (up to 1.8) from 20 %; the transmitter left behind is
    /// marked visited.
    #[test]
    fn receive_yaw_picks_and_visits() {
        let mut ts = vec![t([0.0, 0.0, 0.0]), t([0.0, 0.0, 10.0]), t([0.0, 0.0, 40.0])];
        let mut f = Flyby { transmitter1: 0, transmitter2: 0, current: -1, ..Default::default() };
        f.cam.eye = [0.0, 0.0, 4.0];
        f.receive_yaw(&mut ts, 2).unwrap();
        assert_eq!((f.transmitter1, f.transmitter2), (0, 1));
        assert_eq!(f.gravity, 1.0);
        assert_eq!(f.current, 0);
        f.cam.eye = [0.0, 0.0, 2.0];
        f.receive_yaw(&mut ts, 2).unwrap();
        assert!((f.gravity - 1.8).abs() < 1e-5);
        // Past the midpoint toward 1: transmitter 0 becomes transmitter2,
        // the previous transmitter1, and is visited.
        f.cam.eye = [0.0, 0.0, 8.0];
        f.receive_yaw(&mut ts, 2).unwrap();
        assert_eq!((f.transmitter1, f.transmitter2), (1, 0));
        assert_eq!(ts[0].word_02, 0);
        // Only transmitter 1 and 2 remain: the path continues to 2.
        f.receive_yaw(&mut ts, 2).unwrap();
        assert_eq!((f.transmitter1, f.transmitter2), (1, 2));
    }

    /// Synthetic: get_pitch is negative looking up at a higher point.
    #[test]
    fn pitch_sign() {
        assert!(get_pitch([0.0, 10.0, 0.0], [0.0, 0.0, 10.0]).unwrap() < 0.0);
        assert!(get_pitch([0.0, 0.0, 0.0], [0.0, 10.0, 10.0]).unwrap() > 0.0);
    }
}
