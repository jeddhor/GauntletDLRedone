//! Gameplay camera: the "StdCam" module (EE 0x113358..0x116720) and the pb
//! view state it drives (research/camera.md).
//!
//! main calls StdCamUpdate 0x115fe8 during play because new_camera_switch
//! 0x20eaa0 is initialised to 1 and never written; the older Cam array /
//! StandardCamera system still runs for trigger and boss cameras (not ported
//! here). The StdCam record (StdCamera 0x2bc740, 0x1b0 bytes) is kept in its
//! original layout so reference traces compare byte for byte.
//!
//! Index loops mirror the original per-component loops.
//!
//! Arithmetic: FPU instructions use the guarded-add / RZ-multiply / RN-divide
//! subset; VU0 macro instructions (sceVu0*, pbApplyMatrix) use the VU RZ subset.
#![allow(clippy::needless_range_loop)]
use super::critter_ai::{pitch_vec3, slow_normal_vector, yaw_vec3};
use super::enemy_ai::fix_angle;
use super::record::Record;
use super::{unsupported, Unsupported, R};
use crate::libm::{atanf, cosf, sinf, tanf};
use crate::numeric::{
    add_guarded_rz as add, add_rz as vadd, div_rn as div, div_rz as vdiv, mul_rz as mul, sqrt_rn, sqrt_rz as vsqrt,
    sub_guarded_rz as sub, sub_rz as vsub,
};
use gdl_formats::elf::Elf;
use gdl_formats::wdata::CameraParams;
use gdl_formats::world::Locator;
use serde::{Deserialize, Serialize};

type V3 = [f32; 3];
type V4 = [f32; 4];
type M4 = [V4; 4];

const PI: f32 = f32::from_bits(0x4049_0fdb);
const TWO_PI: f32 = f32::from_bits(0x40c9_0fdb);
/// 180 / π as the code writes it: × 180.0 × 0.31830987.
const INV_PI: f32 = f32::from_bits(0x3ea2_f983);
const UNIT: M4 = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
/// fvector 0x2a1ea0, bvector 0x2a1eb0, uvector 0x2a1e60, lvector 0x2a1e90.
const FVECTOR: V3 = [0.0, 0.0, 1.0];
/// MboxWinPtr +0x14 (window width), +0x4c and +0x58 (region left and
/// bottom, MBWindowSetRegion via ChangeWindow: 0 and NTSC's 0x40); both
/// captures hold these with the single game window.
const MBOX_WIDTH: f32 = 640.0;
const MBOX_LEFT: f32 = 0.0;
const MBOX_BOTTOM: f32 = 64.0;
const BVECTOR: V3 = [0.0, 0.0, -1.0];
const UVECTOR: V3 = [0.0, 1.0, 0.0];
const LVECTOR: V3 = [-1.0, 0.0, 0.0];

/// StdCam record offsets (CamReset 0x113358, CamInit 0x114d18, UpdateCam
/// 0x116070, CamUpdate 0x1140f0).
pub mod o {
    pub const SIZE: usize = 0x1b0;
    /// +0x00..+0x3c: camera matrix (rows right, up, forward; row 3 = eye).
    pub const MAT: usize = 0x00;
    pub const EYE: usize = 0x30;
    /// +0x40..+0x7c: four frustum side normals (CalcFrustrumNormals).
    pub const PLANES: usize = 0x40;
    pub const TARGET: usize = 0xa4;
    /// +0xbc..+0xc4: per-frame target step during the stage-start zoom.
    pub const STEP: usize = 0xbc;
    pub const MAX_RADIUS: usize = 0xdc;
    pub const LOOK: usize = 0xe0;
    pub const YAW: usize = 0xec;
    pub const DYAW: usize = 0xf0;
    pub const DIST: usize = 0xf4;
    pub const DDIST: usize = 0xf8;
    pub const PITCH: usize = 0x104;
    pub const DPITCH: usize = 0x108;
    /// +0x10c: horizontal view angle (MBWindowSetHang); +0x110: aspect.
    pub const HANG: usize = 0x10c;
    pub const ASPECT: usize = 0x110;
    /// +0x114: nine target positions; +0x180: nine distances.
    pub const HIST: usize = 0x114;
    pub const DIST_HIST: usize = 0x180;
    pub const HIST_INDEX: usize = 0x1a4;
    pub const LAST_TRANSMITTER: usize = 0x1a8;
    pub const ROT_COUNT: usize = 0x1ac;
}

/// Camera tuning globals 0x21f0d8..0x21f140, read from the ELF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CamConsts {
    pub hist_size: i32,
    pub max_zoom_frames: i32,
    pub zoom_adjust: f32,
    pub max_zoom_adjust: f32,
    pub adjust_spf: i32,
    pub adjust_max_rad: f32,
    pub adjust_max_rad_1player: f32,
    pub rot_frames: i32,
    pub epsilon: f32,
    pub perf_test: i32,
}

impl CamConsts {
    pub fn from_elf(elf: &Elf) -> Result<Self, Box<dyn std::error::Error>> {
        let word = |n: &str| -> Result<[u8; 4], Box<dyn std::error::Error>> {
            let b = elf.object(n)?;
            Ok(b.get(..4).ok_or("short camera global")?.try_into()?)
        };
        let i = |n: &str| -> Result<i32, Box<dyn std::error::Error>> { Ok(i32::from_le_bytes(word(n)?)) };
        let f = |n: &str| -> Result<f32, Box<dyn std::error::Error>> { Ok(f32::from_le_bytes(word(n)?)) };
        let c = Self {
            hist_size: i("CAM_HIST_SIZE")?,
            max_zoom_frames: i("MAX_ZOOM_FRAMES")?,
            zoom_adjust: f("ZOOM_ADJUST")?,
            max_zoom_adjust: f("MAX_ZOOM_ADJUST")?,
            adjust_spf: i("ADJUST_SPF")?,
            adjust_max_rad: f("ADJUST_MAX_RAD")?,
            adjust_max_rad_1player: f("ADJUST_MAX_RAD_1PLAYER")?,
            rot_frames: i("CAM_ROT_FRAMES")?,
            epsilon: f("CAM_EPSILON")?,
            perf_test: i("camPerformanceTest")?,
        };
        // The record holds nine history slots (+0x114..+0x180, +0x180..+0x1a4).
        if c.hist_size != 9 || c.rot_frames <= 0 || c.max_zoom_frames <= 0 {
            return Err("camera history/frame constants outside the record layout".into());
        }
        Ok(c)
    }
}

/// One Transmitters entry (0x40 bytes, AddTransmitter 0x150768).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transmitter {
    /// +0: 0 camera (locator 1/2), 1 attention (3/4), 2 trigger (9); start
    /// transmitters (locator 1) are re-marked 3 and never chosen.
    pub flag: u8,
    /// +1: locator +1, the per-transmitter radius (multi-player UpdateCam).
    pub radius: u8,
    /// +2: 1 from AddTransmitter; init_game_cam clears the start transmitter's.
    pub word_02: u16,
    /// +0x10 position, +0x20 pitch, +0x24 yaw, +0x28 locator +0x18.
    pub pos: V3,
    pub pitch: f32,
    pub yaw: f32,
    pub roll: f32,
    pub locator: u32,
    /// Locator +2 (trigger link id for kind 9).
    pub id: i16,
}

/// Transmitters[-1] (0x2344c0): the words before the table are
/// PlayerInitStartPos entries 9–13. +0x24 is entry 12's y, which
/// AddLocatorInstList sets to −100000 every level; +0x20 is entry 12's x, 0
/// because no level (Tower included) has a start locator with id 12. The
/// radius byte (+1) is byte 1 of entry 9's y (−100000 outside the Tower).
pub fn transmitter_before_table() -> Transmitter {
    let y9 = (-100000.0f32).to_le_bytes();
    Transmitter { flag: y9[0], radius: y9[1], word_02: 0, pos: [0.0; 3], pitch: 0.0, yaw: -100000.0, roll: 0.0, locator: u32::MAX, id: -1 }
}

/// AddLocatorInstList 0x150ac8's transmitter part: transmitters in list
/// order, StartTransmitterList (by start id) and StartAttTransmitter.
pub struct TransmitterSet {
    pub list: Vec<Transmitter>,
    pub start_list: Vec<Option<usize>>,
    pub start_att: Option<usize>,
    pub found: i32,
}

pub fn build_transmitters(locators: &[Locator], cur_world: i32) -> R<TransmitterSet> {
    let mut set = TransmitterSet { list: Vec::new(), start_list: vec![None; 0xe], start_att: None, found: 0 };
    for l in locators {
        let (mode, start) = match l.kind {
            1 => (0u8, true),
            2 => (0, false),
            3 => (1, true),
            4 => (1, false),
            9 => (2, false),
            _ => continue,
        };
        if 0x100 <= set.list.len() {
            return unsupported("AddTransmitter: more than 0x100 transmitters (FatalErrorC)");
        }
        // AddTransmitter: mode 1 negates the pitch, others turn the yaw by π.
        let (mut pitch, mut yaw) = (l.rotation[0], l.rotation[1]);
        if mode == 1 {
            pitch = -pitch;
        } else {
            yaw = fix_angle(add(yaw, PI)?)?;
        }
        let k = set.list.len();
        set.list.push(Transmitter { flag: mode, radius: l.byte_01, word_02: 1, pos: l.position, pitch, yaw, roll: l.rotation[2], locator: l.index, id: l.id });
        match l.kind {
            1 | 2 => {
                set.found += 1;
                if start {
                    let id = if cur_world != 0xd { 0 } else { l.id };
                    if !(0..0xe).contains(&id) {
                        return unsupported("start transmitter id outside StartTransmitterList");
                    }
                    set.start_list[id as usize] = Some(k);
                    set.list[k].flag = 3;
                }
            }
            3 if start => set.start_att = Some(k),
            _ => {}
        }
    }
    Ok(set)
}

/// Window and pb parameters that stay constant in single-window play.
/// Values are the ones both reference captures hold (pbg +0x10 window record,
/// pbg +4 camera block, MboxWinPtr); research/camera.md lists their sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Window {
    /// pbg+0x10: +0x20/+0x24 frame size, +0x28/+0x2c screen size, +0x30/+0x34
    /// depth range, +0x38/+0x3c screen offset, flag word +0 bit 1.
    pub frame: [i32; 2],
    pub screen: [i32; 2],
    pub depth: [i32; 2],
    pub offset: [f32; 2],
    pub flag_double: bool,
    /// pbg+4: +0x60 near, +0x64 far, +0x78..+0x84 viewport insets, +0x40 up.
    pub near: f32,
    pub far: f32,
    pub viewport: V4,
    pub up: V3,
    /// MboxWinPtr +0x18 (window height used by MBWindowSetHang).
    pub mbox_height: f32,
}

impl Window {
    /// The single-player window (HYPOTHESIS for other player counts).
    pub fn single() -> Self {
        Self {
            frame: [640, 448],
            screen: [512, 384],
            depth: [1, 0xff_ffff],
            offset: [2048.0, 2048.0],
            flag_double: false,
            near: 1.0,
            far: 65536.0,
            viewport: [0.0, 0.0, 0.0, 64.0],
            up: UVECTOR,
            mbox_height: 384.0,
        }
    }
}

/// pb camera/projection state (pbg+4) as last computed by MBEndFrame's
/// pbWinSetup (pbProjCalc, pbCameraCalc, calcMatrices).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pb {
    /// +0x20 eye, +0x30 view direction (pbCameraUpdate), +0x58 fov degrees,
    /// +0x5c aspect (MBWindowProjection).
    pub eye: V3,
    pub dir: V3,
    pub fov: f32,
    pub aspect: f32,
    /// MboxWinPtr +0x1c / +0x20 (MBWindowSetAng).
    pub ang: [f32; 2],
    /// +0x210 world→camera, +0x250 camera→world, +0x90 projection, +0xd0
    /// viewport, +0x290 projection × view, +0x2d0 viewport × that.
    pub view: M4,
    pub inv: M4,
    pub proj: M4,
    pub vp: M4,
    pub pv: M4,
    pub wts: M4,
    /// MboxWinPtr +0x70..+0x9c: the camera matrix rows MBCameraUpdate copies.
    pub cam_rows: [V3; 3],
    /// pbg+4 +3 (projection) and +2 (camera) dirty bytes: MBWorldToScreen /
    /// MBScreenToWorld recompute those parts lazily but not +0x2d0.
    pub proj_dirty: bool,
    pub cam_dirty: bool,
}

/// DoShake 0x16fc80 state (ShakeCamera 0x16fc20).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Shake {
    pub shaking: i32,
    pub kind: i32,
    pub priority: i32,
    pub delay: i32,
    pub count: i32,
    pub radius: f32,
}

/// Inputs the camera reads from the rest of the game each update.
pub struct CamCtx<'a> {
    pub players: &'a [Record; 4],
    pub num_in_game_players: i32,
    pub spf: f32,
    pub vb_elapsed: i32,
    pub paused: i32,
    pub help: i32,
    pub game_mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Camera {
    pub std: Record,
    pub cur3d: i32,
    /// ZOOM_FRAMES 0x21f0e4, resetStdCam 0x21f144.
    pub zoom_frames: i32,
    pub reset_std: i32,
    pub transmitters: Vec<Transmitter>,
    pub start_transmitter: Option<usize>,
    /// CAMS record of the level (bounds possibly derived from the world).
    pub min: V3,
    pub max: V3,
    pub pitch_08: f32,
    pub dist_2c: f32,
    pub radius_30: f32,
    pub consts: CamConsts,
    pub window: Window,
    pub pb: Pb,
    pub shake: Shake,
    /// Cam[0] eye/attention copies CamUpdate writes (0x23e560 / 0x23e630).
    pub cam0_eye: V3,
    pub cam0_attention: V3,
    /// Cam[0]'s matrix 0x23e500 (rows right, up, forward, eye), CamUpdate's
    /// CopyMat4 of the StdCam matrix; get_screen_pos(0) projects with it.
    /// None until this level's first CamUpdate (the guest still holds the
    /// previous level's camera then).
    #[serde(default)]
    pub cam0_mat: Option<[V3; 4]>,
    /// StartTransmitterList (by start id) and found_transmitter, kept for
    /// SetPlayerStartPos.
    #[serde(default)]
    pub start_list: Vec<Option<usize>>,
    #[serde(default)]
    pub found: i32,
}

fn wrap(v: f32) -> R<f32> {
    Ok(if PI < v {
        sub(v, TWO_PI)?
    } else if v <= -PI {
        add(v, TWO_PI)?
    } else {
        v
    })
}

// ---- VU0 macro helpers (libvu0 0x1e4db8.., pbApplyMatrix 0x1d8474) ----

/// ACC = m0·x; ACC += m1·y; ACC += m2·z; out = ACC + m3·w.
fn vu_apply(m: &M4, v: V4) -> R<V4> {
    let mut out = [0.0; 4];
    for (lane, o) in out.iter_mut().enumerate() {
        let a = mul(m[0][lane], v[0])?;
        let a = vadd(a, mul(m[1][lane], v[1])?)?;
        let a = vadd(a, mul(m[2][lane], v[2])?)?;
        *o = vadd(a, mul(m[3][lane], v[3])?)?;
    }
    Ok(out)
}

/// sceVu0MulMatrix(out, a, b): out[j] = a applied to b[j].
fn vu_mul(a: &M4, b: &M4) -> R<M4> {
    Ok([vu_apply(a, b[0])?, vu_apply(a, b[1])?, vu_apply(a, b[2])?, vu_apply(a, b[3])?])
}

/// sceVu0OuterProduct: a × b (VOPMULA / VOPMSUB), w = 0.
fn vu_outer(a: V3, b: V3) -> R<V3> {
    Ok([
        vsub(mul(a[1], b[2])?, mul(b[1], a[2])?)?,
        vsub(mul(a[2], b[0])?, mul(b[2], a[0])?)?,
        vsub(mul(a[0], b[1])?, mul(b[0], a[1])?)?,
    ])
}

/// sceVu0Normalize: (x² + y²) + z², SQRT, ADDq, DIV 1/len, MULq.
fn vu_normalize(v: V3) -> R<V3> {
    let s = vadd(vadd(mul(v[0], v[0])?, mul(v[1], v[1])?)?, mul(v[2], v[2])?)?;
    let len = vadd(0.0, vsqrt(s)?)?;
    let q = vdiv(1.0, len)?;
    Ok([mul(v[0], q)?, mul(v[1], q)?, mul(v[2], q)?])
}

/// sceVu0InversMatrix: transposed rotation, row 3 = 0 − Rᵀ·p (w kept).
fn vu_invers(m: &M4) -> R<M4> {
    let t = |k: usize| [m[0][k], m[1][k], m[2][k], 0.0];
    let (r0, r1, r2) = (t(0), t(1), t(2));
    let p = m[3];
    let mut row3 = [0.0, 0.0, 0.0, p[3]];
    for lane in 0..3 {
        let a = mul(r0[lane], p[0])?;
        let a = vadd(a, mul(r1[lane], p[1])?)?;
        let a = vadd(a, mul(r2[lane], p[2])?)?;
        row3[lane] = vsub(0.0, a)?;
    }
    Ok([r0, r1, r2, row3])
}

/// sceVu0CameraMatrix 0x1e53e8(m, p, zd, yd).
fn vu_camera_matrix(p: V3, zd: V3, yd: V3) -> R<M4> {
    let x = vu_normalize(vu_outer(yd, zd)?)?;
    let z = vu_normalize(zd)?;
    let y = vu_outer(z, x)?;
    // sceVu0TransMatrix: unit row 3 plus p (VADD.xyz).
    let row3 = [vadd(0.0, p[0])?, vadd(0.0, p[1])?, vadd(0.0, p[2])?, 1.0];
    vu_invers(&[[x[0], x[1], x[2], 0.0], [y[0], y[1], y[2], 0.0], [z[0], z[1], z[2], 0.0], row3])
}

impl Pb {
    /// pbProjCalc 0x1abe60 (the parts the camera reads: +0x90 and +0xd0).
    fn proj_calc(&mut self, w: &Window) -> R<()> {
        let (fw, fh) = (w.frame[0] as f32, w.frame[1] as f32);
        let v = w.viewport;
        let left = if v[0] < 0.0 { add(v[0], fw)? } else { add(v[0], 0.0)? };
        let right = sub(if v[1] < 0.0 { 0.0 } else { fw }, v[1])?;
        let top = add(v[2], if v[2] < 0.0 { fh } else { 0.0 })?;
        let bottom = sub(if v[3] < 0.0 { 0.0 } else { fh }, v[3])?;
        let vw = sub(right, left)?;
        let vh = sub(bottom, top)?;
        let cx = add(mul(sub(add(left, right)?, fw)?, 0.5)?, w.offset[0])?;
        let cy = add(mul(sub(add(top, bottom)?, fh)?, 0.5)?, w.offset[1])?;
        let mut ratio = div(mul(self.aspect, vw)?, vh)?;
        if w.flag_double {
            ratio = add(ratio, ratio)?;
        }
        let half = mul(self.fov, f32::from_bits(0x3c0e_fa35))?;
        let tan = div(sinf(half)?, cosf(half)?)?;
        let tan_v = div(tan, ratio)?;
        let (near, far) = (w.near, w.far);
        let range = sub(far, near)?;
        let c34 = div(mul(mul(near, -2.0)?, far)?, range)?;
        let c38 = div(add(near, far)?, range)?;
        let mut p = UNIT;
        p[0][0] = div(1.0, tan)?;
        p[1][1] = -div(1.0, tan_v)?;
        p[2][2] = c38;
        p[2][3] = 1.0;
        p[3][2] = c34;
        p[3][3] = 0.0;
        self.proj = p;
        let (z0, z1) = (w.depth[0] as f32, w.depth[1] as f32);
        let mut m = UNIT;
        m[0][0] = mul(vw, 0.5)?;
        m[1][1] = mul(vh, 0.5)?;
        m[2][2] = mul(sub(z0, z1)?, 0.5)?;
        m[3] = [cx, cy, mul(add(z0, z1)?, 0.5)?, 1.0];
        self.vp = m;
        Ok(())
    }

    /// pbCameraCalc 0x1ab558: normalise the direction, camera matrix, inverse.
    fn camera_calc(&mut self, w: &Window) -> R<()> {
        self.dir = vu_normalize(self.dir)?;
        self.view = vu_camera_matrix(self.eye, self.dir, w.up)?;
        self.inv = vu_invers(&self.view)?;
        Ok(())
    }

    /// pbUpdateMatricies 0x1accd8 / MBEndFrame's pbWinSetup.
    pub fn update(&mut self, w: &Window) -> R<()> {
        self.proj_calc(w)?;
        self.camera_calc(w)?;
        self.proj_dirty = false;
        self.cam_dirty = false;
        self.pv = vu_mul(&self.proj, &self.view)?;
        self.wts = vu_mul(&self.vp, &self.pv)?;
        Ok(())
    }

    fn refresh(&mut self, w: &Window) -> R<()> {
        if self.proj_dirty {
            self.proj_calc(w)?;
            self.proj_dirty = false;
        }
        if self.cam_dirty {
            self.camera_calc(w)?;
            self.cam_dirty = false;
        }
        Ok(())
    }

    /// MBWorldToScreen 0x1c4018: screen x, y and depth.
    pub fn world_to_screen(&mut self, w: &Window, p: V3) -> R<V3> {
        self.refresh(w)?;
        let c = vu_apply(&self.wts, [p[0], p[1], p[2], 1.0])?;
        let k = div(1.0, c[3])?;
        let (fw, fh) = (w.frame[0] as f32, w.frame[1] as f32);
        let x = sub(add(mul(k, c[0])?, mul(fw, 0.5)?)?, w.offset[0])?;
        let y = sub(add(mul(k, c[1])?, mul(fh, 0.5)?)?, w.offset[1])?;
        Ok([mul(div(w.screen[0] as f32, fw)?, x)?, mul(div(w.screen[1] as f32, fh)?, y)?, c[3]])
    }

    /// MBScreenToWorld 0x1c4160.
    pub fn screen_to_world(&mut self, w: &Window, s: V3) -> R<V3> {
        self.refresh(w)?;
        let (fw, fh) = (w.frame[0] as f32, w.frame[1] as f32);
        let d = s[2];
        let x = add(sub(mul(div(fw, w.screen[0] as f32)?, s[0])?, mul(fw, 0.5)?)?, w.offset[0])?;
        let y = add(sub(mul(div(fh, w.screen[1] as f32)?, s[1])?, mul(fh, 0.5)?)?, w.offset[1])?;
        let (x, y) = (mul(x, d)?, mul(y, d)?);
        let cx = div(sub(x, mul(self.vp[3][0], d)?)?, mul(self.proj[0][0], self.vp[0][0])?)?;
        let cy = div(sub(y, mul(self.vp[3][1], d)?)?, mul(self.proj[1][1], self.vp[1][1])?)?;
        let r = vu_apply(&self.inv, [cx, cy, d, 1.0])?;
        Ok([r[0], r[1], r[2]])
    }
}

/// CamIsPlayerValid 0x1147e8: status +0x93c bit 0x20 clear, state 1 or 4.
fn player_valid(p: &Record) -> bool {
    p.u16_(0x93c) & 0x20 == 0 && matches!(p.i32_(0xfc), 1 | 4)
}

impl Camera {
    pub fn new(consts: CamConsts, set: TransmitterSet, params: &CameraParams, world_min: V3, world_max: V3) -> R<Self> {
        // init_game_cam 0x16fec8: a CAMS record without bounds takes the
        // world box inset by 8 (4 above).
        let (min, max) = if params.bounds_set == 0 {
            (
                [add(world_min[0], 8.0)?, add(world_min[1], 0.0)?, add(world_min[2], 8.0)?],
                [add(world_max[0], -8.0)?, add(world_max[1], 4.0)?, add(world_max[2], -8.0)?],
            )
        } else {
            (params.min, params.max)
        };
        let mut list = set.list;
        let start = set.start_list[0];
        // init_game_cam: found_transmitter != 0 → StartTransmitter +2 = 0
        // (receive_yaw / default_pitch feed the old camera only).
        if 0 < set.found
            && let Some(k) = start {
                list[k].word_02 = 0;
            }
        Ok(Self {
            std: Record::zeroed(o::SIZE),
            cur3d: -1,
            zoom_frames: 0,
            reset_std: 0,
            transmitters: list,
            // SetPlayerStartPos 0x150858 (start id 0 in single player).
            start_transmitter: start,
            min,
            max,
            pitch_08: params.pitch_08,
            dist_2c: params.dist_2c,
            radius_30: params.radius_30,
            consts,
            window: Window::single(),
            pb: Pb { fov: 60.0, aspect: 1.0, ..Pb::default() },
            shake: Shake::default(),
            cam0_eye: [0.0; 3],
            cam0_attention: [0.0; 3],
            cam0_mat: None,
            start_list: set.start_list,
            found: set.found,
        })
    }

    /// SetPlayerStartPos 0x150858's StartTransmitter: StartTransmitterList[id],
    /// or entry 0 when that is empty. init_game_cam (after the players) then
    /// clears the chosen transmitter's +2 when a transmitter was found
    /// (Camera::new did so for entry 0; that one gets AddTransmitter's 1 back).
    pub fn set_start_transmitter(&mut self, id: usize) {
        let first = self.start_list.first().copied().flatten();
        let chosen = self.start_list.get(id).copied().flatten().or(first);
        if chosen == self.start_transmitter {
            return;
        }
        if 0 < self.found {
            if let Some(k) = self.start_transmitter {
                self.transmitters[k].word_02 = 1;
            }
            if let Some(k) = chosen {
                self.transmitters[k].word_02 = 0;
            }
        }
        self.start_transmitter = chosen;
    }

    /// The window region InitCamera turns into LLIM/RLIM/ULIM/DLIM
    /// (0x23f2a8..0x23f2b4): MboxWinPtr +8 ∓ +0x14 × 0.5 and +0xc ± +0x18 ×
    /// 0.5, truncated. Both captures hold (0, 640, 448, 64).
    pub fn screen_limits(&self) -> R<[i32; 4]> {
        let (cx, cy) = self.mbox_centre()?;
        let hw = mul(MBOX_WIDTH, 0.5)?;
        let hh = mul(self.window.mbox_height, 0.5)?;
        Ok([sub(cx, hw)? as i32, add(hw, cx)? as i32, add(hh, cy)? as i32, sub(cy, hh)? as i32])
    }

    /// MboxWinPtr +8 / +0xc (MBWindowSetAng: +0x14 × 0.5 + +0x4c and +0x18 ×
    /// 0.5 + +0x58).
    fn mbox_centre(&self) -> R<(f32, f32)> {
        Ok((add(mul(MBOX_WIDTH, 0.5)?, MBOX_LEFT)?, add(mul(self.window.mbox_height, 0.5)?, MBOX_BOTTOM)?))
    }

    /// get_screen_pos 0x173ea0(0, x, y, p): MBWindowProject 0x1d1278 with
    /// Cam[0]'s matrix — the body-space vector (BodyVector 0x1d5838) of p −
    /// eye, depth at least 3, scaled by MboxWinPtr +0/+4 (MBWindowSetAng:
    /// +0x14 × 0.5 / tan(h/2), +0x18 × 0.5 / tan(v/2)) about +8/+0xc, each
    /// clamped to ±32767 and truncated to a short.
    pub fn screen_pos(&self, p: V3) -> R<(f32, f32)> {
        let Some(m) = self.cam0_mat else {
            return unsupported("get_screen_pos before this level's first CamUpdate (Cam[0] holds the previous camera)");
        };
        if self.pb.ang[0] == 0.0 || self.pb.ang[1] == 0.0 {
            return unsupported("get_screen_pos before MBWindowSetAng");
        }
        let v = [sub(p[0], m[3][0])?, sub(p[1], m[3][1])?, sub(p[2], m[3][2])?];
        let dot = |r: V3| -> R<f32> { Ok(add(add(mul(v[0], r[0])?, mul(v[1], r[1])?)?, mul(v[2], r[2])?)?) };
        let (bx, by, mut bz) = (dot(m[0])?, dot(m[1])?, dot(m[2])?);
        if bz < 3.0 {
            bz = 3.0;
        }
        let inv_tan = |a: f32| -> R<f32> {
            let h = mul(a, 0.5)?;
            Ok(div(1.0, div(sinf(h)?, cosf(h)?)?)?)
        };
        let s0 = mul(mul(MBOX_WIDTH, 0.5)?, inv_tan(self.pb.ang[0])?)?;
        let s1 = mul(mul(self.window.mbox_height, 0.5)?, inv_tan(self.pb.ang[1])?)?;
        let (cx, cy) = self.mbox_centre()?;
        let k = div(1.0, bz)?;
        let x = add(mul(mul(k, bx)?, s0)?, cx)?;
        let y = add(mul(mul(k, by)?, s1)?, cy)?;
        let short = |f: f32| -> f32 {
            if f < -32767.0 {
                -32767.0
            } else if 32767.0 < f {
                32767.0
            } else {
                f as i32 as i16 as f32
            }
        };
        Ok((short(x), short(y)))
    }

    /// MboxWinPtr +0x90 / +0x98 (camera forward x, z) read by PlayerMotion.
    pub fn forward_xz(&self) -> (f32, f32) {
        (self.pb.cam_rows[2][0], self.pb.cam_rows[2][2])
    }

    /// CamReset 0x113358 (fields it does not touch keep their values).
    fn reset(c: &mut Record, consts: &CamConsts) {
        c.set_f32(o::YAW, PI);
        for off in [o::DYAW, o::PITCH, o::DPITCH, o::DIST, o::DDIST, 0xd4, 0xd8, 0x100, o::ASPECT] {
            c.set_f32(off, 0.0);
        }
        for off in [o::LOOK, o::TARGET, o::STEP, 0xc8] {
            c.set_v3(off, [0.0; 3]);
        }
        for k in 0..4 {
            c.set_v3(o::PLANES + 0x10 * k, [0.0; 3]);
        }
        for k in 0..consts.hist_size as usize {
            c.set_v3(o::HIST + 0xc * k, [0.0; 3]);
        }
        c.set_i32(o::HIST_INDEX, 0);
        c.set_f32(o::HANG, f32::from_bits(0x3f86_0a92));
        c.set_i32(o::LAST_TRANSMITTER, -1);
        c.set_f32(o::ROT_COUNT, 0.0);
    }

    /// CamGetPlayerAvgPos 0x114820: flag 1 screen space, 2 position +0x50,
    /// 4 position +0x60; the result is clamped to the CAMS box.
    pub(crate) fn player_avg(&mut self, ctx: &CamCtx, flags: u32) -> R<(V3, bool)> {
        let mut lo = [f32::from_bits(0x60ad_78ec); 3];
        let mut hi = [f32::from_bits(0xe0ad_78ec); 3];
        let mut count = 0;
        for p in ctx.players.iter() {
            if !player_valid(p) {
                continue;
            }
            count += 1;
            for (bit, off) in [(2, 0x50), (4, 0x60)] {
                if flags & bit == 0 {
                    continue;
                }
                let v = if flags & 1 != 0 {
                    let w = self.window.clone();
                    self.pb.world_to_screen(&w, p.v3(off))?
                } else {
                    p.v3(off)
                };
                for k in 0..3 {
                    // CamMinMaxAvgPos 0x114708.
                    if v[k] <= lo[k] {
                        lo[k] = v[k];
                    }
                    if hi[k] <= v[k] {
                        hi[k] = v[k];
                    }
                }
            }
        }
        let mut mid = [0.0; 3];
        for k in 0..3 {
            mid[k] = mul(add(lo[k], hi[k])?, 0.5)?;
        }
        let mut out = if flags & 1 != 0 {
            let w = self.window.clone();
            self.pb.screen_to_world(&w, mid)?
        } else {
            mid
        };
        for ((v, lo), hi) in out.iter_mut().zip(self.min).zip(self.max) {
            *v = if *v < lo {
                lo
            } else if hi < *v {
                hi
            } else {
                *v
            };
        }
        Ok((out, 0 < count))
    }

    /// GetTransmitter3D 0x1143a8: nearest flag-0 transmitter other than the
    /// current one, switching when it is nearer than 2/3 of the current
    /// distance (squared × 0.44444445).
    fn transmitter_3d(&mut self, p: V3) -> R<Option<usize>> {
        if self.transmitters.is_empty() {
            return Ok(None);
        }
        let d2 = |t: &Transmitter| -> R<f32> {
            let (x, y, z) = (sub(p[0], t.pos[0])?, sub(p[1], t.pos[1])?, sub(p[2], t.pos[2])?);
            Ok(add(add(mul(x, x)?, mul(y, y)?)?, mul(z, z)?)?)
        };
        let (mut best, mut best_d) = (-1i32, 0.0f32);
        for (k, t) in self.transmitters.iter().enumerate() {
            if t.flag != 0 || k as i32 == self.cur3d {
                continue;
            }
            let d = d2(t)?;
            if best < 0 || d < best_d {
                best = k as i32;
                best_d = d;
            }
        }
        if self.cur3d < 0 {
            self.cur3d = best;
        } else {
            let dc = d2(&self.transmitters[self.cur3d as usize])?;
            if best_d <= mul(dc, f32::from_bits(0x3ee3_8e39))? {
                // MBTreeSetAlpha on the old arrow tree: presentation.
                self.cur3d = best;
            }
        }
        // Index −1 reads the words before the table (transmitter_before_table).
        Ok(Some(usize::try_from(self.cur3d).unwrap_or(usize::MAX)))
    }

    /// TransGetYawPitch 0x116268.
    /// The transmitter at an index (usize::MAX: Transmitters[-1]).
    fn transmitter(&self, k: usize) -> Transmitter {
        self.transmitters.get(k).cloned().unwrap_or_else(transmitter_before_table)
    }

    fn trans_yaw_pitch(&self, t: Option<usize>, ctx: &CamCtx) -> R<(f32, f32)> {
        let (yaw, mut pitch) = match t {
            None => (0.0, 0.0),
            Some(k) => {
                let tr = self.transmitter(k);
                (wrap(sub(tr.yaw, PI)?)?, -tr.pitch)
            }
        };
        if 1 < ctx.num_in_game_players {
            let lim = -self.pitch_08;
            if lim <= pitch {
                pitch = lim;
            }
        }
        Ok((yaw, pitch))
    }

    /// CalcLookdir 0x113c18.
    fn calc_lookdir(yaw: f32, pitch: f32) -> R<V3> {
        pitch_vec3(-pitch, yaw_vec3(-yaw, FVECTOR)?)
    }

    /// CamLookInDir 0x113a38: matrix rows from a direction.
    fn look_in_dir(c: &mut Record, dir: V3) -> R<()> {
        let mut d = dir;
        let len = slow_normal_vector(&mut d)?;
        c.set_v3(o::MAT + 0x20, d);
        if len < f32::from_bits(0x3a83_126f) {
            for (k, row) in UNIT.iter().take(3).enumerate() {
                c.set_v3(o::MAT + 0x10 * k, [row[0], row[1], row[2]]);
            }
            return Ok(());
        }
        let flat = add(mul(d[0], d[0])?, mul(d[2], d[2])?)?;
        let u = if flat < f32::from_bits(0x38d1_b717) {
            if 0.0 < d[1] {
                FVECTOR
            } else {
                BVECTOR
            }
        } else {
            UVECTOR
        };
        let mut r = [
            sub(mul(u[1], d[2])?, mul(u[2], d[1])?)?,
            sub(mul(u[2], d[0])?, mul(u[0], d[2])?)?,
            sub(mul(u[0], d[1])?, mul(u[1], d[0])?)?,
        ];
        slow_normal_vector(&mut r)?;
        c.set_v3(o::MAT, r);
        let up = [
            sub(mul(d[1], r[2])?, mul(d[2], r[1])?)?,
            sub(mul(d[2], r[0])?, mul(d[0], r[2])?)?,
            sub(mul(d[0], r[1])?, mul(d[1], r[0])?)?,
        ];
        c.set_v3(o::MAT + 0x10, up);
        Ok(())
    }

    /// MBCameraUpdate 0x1c3bc0 + MBWindowSetHang 0x1d0eb8 (+ the optional
    /// MBWindowProjection): window camera rows and pb eye/direction.
    fn camera_update(&mut self, c: &mut Record) -> R<()> {
        self.mb_camera_update(c);
        self.pb.proj_dirty = true;
        let hang = c.f32_(o::HANG);
        let k = mul(div(self.window.mbox_height, self.window.frame[1] as f32)?, 0.75)?;
        if 0.0 < hang {
            let t = tanf(mul(hang, 0.5)?)?;
            let v = atanf(mul(k, t)?)?;
            self.pb.ang = [hang, add(v, v)?];
        } else {
            let t = tanf(mul(-hang, 0.5)?)?;
            let v = atanf(mul(div(self.window.frame[1] as f32, self.window.frame[0] as f32)?, t)?)?;
            self.pb.ang = [-hang, add(v, v)?];
        }
        let deg = mul(mul(hang, 180.0)?, INV_PI)?;
        self.pb.fov = deg;
        self.pb.aspect = 1.0;
        let aspect = c.f32_(o::ASPECT);
        if 0.0 < aspect {
            self.pb.fov = deg;
            self.pb.aspect = div(1.0, aspect)?;
        }
        Ok(())
    }

    /// DoShake 0x16fc80 on the eye and target.
    fn do_shake(&mut self, ctx: &CamCtx, eye: &mut V3, target: &mut V3) -> R<()> {
        Self::shake_step(&mut self.shake, ctx.paused, ctx.help, ctx.vb_elapsed, eye, target)
    }

    fn shake_step(s: &mut Shake, paused: i32, help: i32, vb_elapsed: i32, eye: &mut V3, target: &mut V3) -> R<()> {
        if s.shaking == 0 || paused != 0 || help != 0 {
            return Ok(());
        }
        s.count -= vb_elapsed;
        if s.count < 0 {
            s.shaking = 0;
            s.priority = 0;
        }
        s.delay -= vb_elapsed;
        if s.delay < 0 {
            s.delay = 0;
        }
        if !(0..=2).contains(&s.kind) || 0 < s.delay {
            return Ok(());
        }
        let a = fix_angle(mul(s.count as f32, f32::from_bits(0x3f29_c920))?)?;
        let (c, sn) = (cosf(a)?, sinf(a)?);
        if s.kind != 1 {
            target[0] = add(mul(c, s.radius)?, target[0])?;
            target[2] = add(mul(sn, s.radius)?, target[2])?;
        }
        if s.kind != 0 {
            eye[0] = add(mul(c, s.radius)?, eye[0])?;
            eye[2] = add(mul(sn, s.radius)?, eye[2])?;
        }
        Ok(())
    }

    /// CamUpdate 0x1140f0.
    fn cam_update(&mut self, ctx: &CamCtx, c: &mut Record) -> R<()> {
        let look = Self::calc_lookdir(c.f32_(o::YAW), c.f32_(o::PITCH))?;
        c.set_v3(o::LOOK, look);
        let (mut eye, mut target) = (c.v3(o::EYE), c.v3(o::TARGET));
        self.do_shake(ctx, &mut eye, &mut target)?;
        c.set_v3(o::TARGET, target);
        let nd = -c.f32_(o::DIST);
        let eye = [add(mul(nd, look[0])?, target[0])?, add(mul(nd, look[1])?, target[1])?, add(mul(nd, look[2])?, target[2])?];
        c.set_v3(o::EYE, eye);
        Self::look_in_dir(c, look)?;
        self.cam0_mat = Some([c.v3(o::MAT), c.v3(o::MAT + 0x10), c.v3(o::MAT + 0x20), c.v3(o::EYE)]);
        self.cam0_eye = eye;
        self.cam0_attention = target;
        self.camera_update(c)
    }

    /// MBCameraUpdate 0x1c3bc0 alone: matrix w lanes, window camera rows,
    /// pb eye and direction (camera part dirty).
    fn mb_camera_update(&mut self, c: &mut Record) {
        for k in 0..3 {
            c.set_f32(o::MAT + 0x10 * k + 0xc, 0.0);
        }
        c.set_f32(o::MAT + 0x3c, 1.0);
        self.pb.eye = c.v3(o::EYE);
        self.pb.dir = c.v3(o::MAT + 0x20);
        self.pb.cam_rows = [c.v3(o::MAT), c.v3(o::MAT + 0x10), c.v3(o::MAT + 0x20)];
        self.pb.cam_dirty = true;
    }

    /// LimitYawPitch 0x113c68.
    fn limit_yaw_pitch(&self, c: &mut Record, yaw: f32, pitch: f32, spf: f32) -> R<bool> {
        let k = if self.consts.adjust_spf == 0 || spf <= 0.0 { 1.0 } else { mul(spf, 30.0)? };
        let frames = self.consts.rot_frames as f32;
        if self.cur3d != c.i32_(o::LAST_TRANSMITTER) {
            c.set_i32(o::LAST_TRANSMITTER, self.cur3d);
            c.set_f32(o::DYAW, div(wrap(sub(yaw, c.f32_(o::YAW))?)?, frames)?);
            c.set_f32(o::DPITCH, div(wrap(sub(pitch, c.f32_(o::PITCH))?)?, frames)?);
            c.set_f32(o::ROT_COUNT, 0.0);
        }
        if c.f32_(o::ROT_COUNT) < frames {
            c.set_f32(o::YAW, wrap(add(mul(k, c.f32_(o::DYAW))?, c.f32_(o::YAW))?)?);
            c.set_f32(o::PITCH, wrap(add(mul(k, c.f32_(o::DPITCH))?, c.f32_(o::PITCH))?)?);
            c.set_f32(o::ROT_COUNT, add(k, c.f32_(o::ROT_COUNT))?);
            return Ok(true);
        }
        Ok(false)
    }

    /// History slots in LimitTranslate/LimitDist order: from the low byte of
    /// the index (sign-extended) up to index + size − 1, modulo size.
    fn hist_order(c: &Record, n: i32) -> Vec<usize> {
        let idx = c.i32_(o::HIST_INDEX);
        let mut k = i32::from(c.i8_(o::HIST_INDEX));
        let mut out = Vec::new();
        while k < idx + n {
            out.push(k.rem_euclid(n) as usize);
            k = i32::from((k + 1) as i8);
        }
        out
    }

    /// LimitTranslate 0x113f28.
    fn limit_translate(&self, c: &mut Record) -> R<bool> {
        let n = self.consts.hist_size;
        let t = c.v3(o::TARGET);
        let mut s = [0.0f32; 3];
        for k in Self::hist_order(c, n) {
            let h = c.v3(o::HIST + 0xc * k);
            for i in 0..3 {
                s[i] = add(s[i], sub(h[i], t[i])?)?;
            }
        }
        let inv = div(1.0, n as f32)?;
        let d = [mul(inv, s[0])?, mul(inv, s[1])?, mul(inv, s[2])?];
        c.set_v3(o::TARGET, [add(t[0], d[0])?, add(t[1], d[1])?, add(t[2], d[2])?]);
        Ok(d[0] != 0.0 || d[1] != 0.0 || d[2] != 0.0)
    }

    /// LimitDist 0x115688.
    fn limit_dist(&self, c: &mut Record) -> R<bool> {
        let n = self.consts.hist_size;
        let dist = c.f32_(o::DIST);
        let mut s = 0.0f32;
        for k in Self::hist_order(c, n) {
            s = add(s, sub(c.f32_(o::DIST_HIST + 4 * k), dist)?)?;
        }
        let s = mul(s, div(1.0, n as f32)?)?;
        c.set_f32(o::DIST, add(s, dist)?);
        Ok(s != 0.0)
    }

    /// CalcFrustrumNormals 0x1150f0 (planes through the origin).
    fn frustum_normals(&self, yaw: f32, l: V3) -> R<[V3; 4]> {
        let r0 = yaw_vec3(-yaw, LVECTOR)?;
        let u0 = [
            sub(mul(l[1], r0[2])?, mul(l[2], r0[1])?)?,
            sub(mul(l[2], r0[0])?, mul(l[0], r0[2])?)?,
            sub(mul(l[0], r0[1])?, mul(l[1], r0[0])?)?,
        ];
        let th = tanf(mul(self.pb.ang[0], 0.5)?)?;
        let tv = tanf(mul(self.pb.ang[1], 0.5)?)?;
        let r = [mul(th, r0[0])?, mul(th, r0[1])?, mul(th, r0[2])?];
        let u = [mul(tv, u0[0])?, mul(tv, u0[1])?, mul(tv, u0[2])?];
        let a = [add(l[0], r[0])?, add(l[1], r[1])?, add(l[2], r[2])?];
        let b = [sub(l[0], r[0])?, sub(l[1], r[1])?, sub(l[2], r[2])?];
        let p0 = [add(a[0], u[0])?, add(a[1], u[1])?, add(a[2], u[2])?];
        let p1 = [add(b[0], u[0])?, add(b[1], u[1])?, add(b[2], u[2])?];
        let p2 = [sub(a[0], u[0])?, sub(a[1], u[1])?, sub(a[2], u[2])?];
        let p3 = [sub(b[0], u[0])?, sub(b[1], u[1])?, sub(b[2], u[2])?];
        let cross = |a: V3, b: V3| -> R<V3> {
            Ok([
                sub(mul(a[1], b[2])?, mul(a[2], b[1])?)?,
                sub(mul(a[2], b[0])?, mul(a[0], b[2])?)?,
                sub(mul(a[0], b[1])?, mul(a[1], b[0])?)?,
            ])
        };
        Ok([cross(p1, p0)?, cross(p0, p2)?, cross(p3, p1)?, cross(p2, p3)?])
    }

    fn dot(a: V3, b: V3) -> R<f32> {
        Ok(add(add(mul(a[0], b[0])?, mul(a[1], b[1])?)?, mul(a[2], b[2])?)?)
    }

    /// CalcDist 0x115350: the distance wanted for one history entry; also
    /// stores the frustum normals at +0x40.
    fn calc_dist(&self, ctx: &CamCtx, c: &mut Record, max_radius: f32, hist_pos: V3) -> R<f32> {
        let look = c.v3(o::LOOK);
        let planes = self.frustum_normals(c.f32_(o::YAW), look)?;
        for (k, p) in planes.iter().enumerate() {
            c.set_v3(o::PLANES + 0x10 * k, *p);
        }
        if ctx.num_in_game_players == 1 {
            return Ok(if self.consts.perf_test == 0 { self.dist_2c } else { add(max_radius, self.consts.max_zoom_adjust)? });
        }
        let mut need = 0.0f32;
        for plane in planes.iter() {
            let k = div(1.0, Self::dot(*plane, look)?)?;
            let base = Self::dot(*plane, hist_pos)?;
            for p in ctx.players.iter().filter(|p| player_valid(p)) {
                for off in [0x60, 0x50] {
                    let v = mul(sub(base, Self::dot(*plane, p.v3(off))?)?, k)?;
                    if need <= v {
                        need = v;
                    }
                }
            }
        }
        let d = self.dist_2c;
        Ok(if sub(d, self.consts.zoom_adjust)? < need || max_radius <= d {
            if need <= sub(max_radius, self.consts.zoom_adjust)? {
                add(need, self.consts.zoom_adjust)?
            } else if sub(max_radius, self.consts.max_zoom_adjust)? < need {
                add(need, self.consts.max_zoom_adjust)?
            } else {
                max_radius
            }
        } else {
            d
        })
    }

    /// UpdateCam 0x116070 on a record; false once nothing moved.
    fn update_cam(&mut self, ctx: &CamCtx, c: &mut Record) -> R<bool> {
        let (avg, any) = self.player_avg(ctx, 5)?;
        if !any {
            return Ok(true);
        }
        let n = self.consts.hist_size;
        let idx = (c.i32_(o::HIST_INDEX) + 1) % n;
        c.set_i32(o::HIST_INDEX, idx);
        c.set_v3(o::HIST + 0xc * idx as usize, avg);
        let at = self.player_avg(ctx, 2)?.0;
        let t = self.transmitter_3d(at)?;
        // CurTransmitterBlink / StdCamMoveAvgArrow: arrow alpha and position
        // (presentation only).
        let (yaw, pitch) = self.trans_yaw_pitch(t, ctx)?;
        let rotating = self.limit_yaw_pitch(c, yaw, pitch, ctx.spf)?;
        let moving = self.limit_translate(c)?;
        let radius = if ctx.num_in_game_players == 1 && self.consts.perf_test == 0 {
            mul(self.dist_2c, self.consts.adjust_max_rad_1player)?
        } else {
            match t.map(|k| self.transmitter(k).radius) {
                Some(r) if r != 0 => r as f32,
                _ => mul(self.radius_30, self.consts.adjust_max_rad)?,
            }
        };
        c.set_f32(o::MAX_RADIUS, radius);
        let hist = c.v3(o::HIST + 0xc * idx as usize);
        let d = self.calc_dist(ctx, c, radius, hist)?;
        c.set_f32(o::DIST_HIST + 4 * idx as usize, d);
        let zooming = self.limit_dist(c)?;
        self.cam_update(ctx, c)?;
        Ok(zooming || rotating || moving)
    }

    /// CalcDestination 0x114ed8: update until nine quiet updates (≤ 100).
    fn calc_destination(&mut self, ctx: &CamCtx, c: &mut Record) -> R<()> {
        let (mut quiet, mut total) = (0, 0);
        while quiet < self.consts.hist_size && total < 100 {
            let moved = self.update_cam(ctx, c)?;
            self.pb.update(&self.window)?;
            quiet = if moved { 0 } else { quiet + 1 };
            total += 1;
        }
        Ok(())
    }

    /// CamInit 0x114d18 with a transmitter (or none).
    fn cam_init(&mut self, ctx: &CamCtx, c: &mut Record, t: Option<usize>) -> R<()> {
        let (yaw, pitch) = self.trans_yaw_pitch(t, ctx)?;
        let look = Self::calc_lookdir(yaw, pitch)?;
        c.set_v3(o::LOOK, look);
        c.set_f32(o::YAW, yaw);
        c.set_f32(o::PITCH, pitch);
        if let Some(k) = t {
            c.set_v3(o::EYE, self.transmitters[k].pos);
        }
        c.set_f32(o::MAX_RADIUS, self.radius_30);
        Self::look_in_dir(c, look)?;
        self.camera_update(c)?;
        let p = self.player_avg(ctx, 4)?.0;
        let eye = c.v3(o::EYE);
        let (x, y, z) = (sub(eye[0], p[0])?, sub(eye[1], p[1])?, sub(eye[2], p[2])?);
        let d = sqrt_rn(add(add(mul(x, x)?, mul(y, y)?)?, mul(z, z)?)?)?;
        c.set_f32(o::DIST, d);
        c.set_v3(o::TARGET, [add(mul(look[0], d)?, eye[0])?, add(mul(look[1], d)?, eye[1])?, add(mul(look[2], d)?, eye[2])?]);
        Ok(())
    }

    /// StdCamInit 0x114f68 (param 0: the level start; 1: a restored stage).
    pub fn std_init(&mut self, ctx: &CamCtx, restored: bool) -> R<()> {
        let mut c = std::mem::replace(&mut self.std, Record::zeroed(0));
        let r = (|| -> R<()> {
            Self::reset(&mut c, &self.consts);
            self.reset_std = 0;
            self.zoom_frames = self.consts.max_zoom_frames;
            if !restored {
                return self.cam_init(ctx, &mut c, self.start_transmitter);
            }
            let at = self.player_avg(ctx, 2)?.0;
            let t = self.transmitter_3d(at)?;
            let (yaw, pitch) = self.trans_yaw_pitch(t, ctx)?;
            c.set_f32(o::YAW, yaw);
            c.set_f32(o::PITCH, pitch);
            let look = Self::calc_lookdir(yaw, pitch)?;
            c.set_v3(o::LOOK, look);
            c.set_v3(o::TARGET, at);
            c.set_f32(o::DIST, self.dist_2c);
            c.set_f32(o::MAX_RADIUS, self.radius_30);
            let nd = -self.dist_2c;
            c.set_v3(o::EYE, [add(mul(nd, look[0])?, at[0])?, add(mul(nd, look[1])?, at[1])?, add(mul(nd, look[2])?, at[2])?]);
            Self::look_in_dir(&mut c, look)?;
            self.camera_update(&mut c)?;
            self.pb.update(&self.window)?;
            self.calc_destination(ctx, &mut c)
        })();
        self.std = c;
        r
    }

    /// StdCamStartUpdate 0x115ab0: the stage-start zoom from the start
    /// transmitter to the follow position. Returns true once finished
    /// (start_of_stage and cam_wait are cleared by the caller).
    fn start_update(&mut self, ctx: &CamCtx) -> R<bool> {
        let mut c = std::mem::replace(&mut self.std, Record::zeroed(0));
        let r = (|| -> R<bool> {
            let n = self.consts.max_zoom_frames as f32;
            if self.zoom_frames == self.consts.max_zoom_frames {
                // HYPOTHESIS: the stack record's untouched bytes (matrix,
                // distance history) start as zero; CalcDestination replaces
                // the history before it settles.
                let mut tmp = Record::zeroed(o::SIZE);
                Self::reset(&mut tmp, &self.consts);
                self.cam_init(ctx, &mut tmp, self.start_transmitter)?;
                self.calc_destination(ctx, &mut tmp)?;
                c.set_f32(o::DYAW, div(wrap(sub(tmp.f32_(o::YAW), c.f32_(o::YAW))?)?, n)?);
                c.set_f32(o::DPITCH, div(wrap(sub(tmp.f32_(o::PITCH), c.f32_(o::PITCH))?)?, n)?);
                c.set_f32(o::DDIST, div(sub(tmp.f32_(o::DIST), c.f32_(o::DIST))?, n)?);
                let (a, b) = (tmp.v3(o::TARGET), c.v3(o::TARGET));
                let inv = div(1.0, n)?;
                let step = [mul(inv, sub(a[0], b[0])?)?, mul(inv, sub(a[1], b[1])?)?, mul(inv, sub(a[2], b[2])?)?];
                c.set_v3(o::STEP, step);
            }
            c.set_f32(o::YAW, wrap(add(c.f32_(o::YAW), c.f32_(o::DYAW))?)?);
            c.set_f32(o::PITCH, wrap(add(c.f32_(o::PITCH), c.f32_(o::DPITCH))?)?);
            c.set_f32(o::DIST, add(c.f32_(o::DIST), c.f32_(o::DDIST))?);
            let (t, s) = (c.v3(o::TARGET), c.v3(o::STEP));
            let t = [add(t[0], s[0])?, add(t[1], s[1])?, add(t[2], s[2])?];
            c.set_v3(o::TARGET, t);
            let idx = (c.i32_(o::HIST_INDEX) + 1) % self.consts.hist_size;
            c.set_i32(o::HIST_INDEX, idx);
            c.set_v3(o::HIST + 0xc * idx as usize, t);
            c.set_f32(o::DIST_HIST + 4 * idx as usize, c.f32_(o::DIST));
            self.cam_update(ctx, &mut c)?;
            let mut done = false;
            if self.zoom_frames < 1 {
                for _ in 0..9 {
                    self.update_cam(ctx, &mut c)?;
                    self.pb.update(&self.window)?;
                }
                self.zoom_frames -= 1;
                done = true;
            }
            self.zoom_frames -= 1;
            Ok(done)
        })();
        self.std = c;
        r
    }

    /// StdCamUpdate 0x115fe8 (with StdCamStartStage 0x115f70), then the
    /// frame's pbWinSetup. `cam_wait` / `start_of_stage` are the globals.
    pub fn std_update(&mut self, ctx: &CamCtx, cam_wait: &mut i32, start_of_stage: &mut i32) -> R<()> {
        if ctx.paused == 0 && ctx.vb_elapsed != 0 {
            let waiting = if ctx.game_mode == 0x4010 {
                if 2 < *cam_wait {
                    *cam_wait = 2;
                }
                // write_stage_info: stage name text and its sounds (presentation).
                if *cam_wait == 1 && self.start_update(ctx)? {
                    *start_of_stage = 0;
                    *cam_wait = 0;
                }
                0 < *cam_wait
            } else {
                true
            };
            if !waiting {
                if self.reset_std != 0 {
                    // StdCamReturn: re-apply the record's camera to the window.
                    self.reset_std = 0;
                    let mut c = std::mem::replace(&mut self.std, Record::zeroed(0));
                    let r = self.camera_update(&mut c);
                    self.std = c;
                    r?;
                } else {
                    let mut c = std::mem::replace(&mut self.std, Record::zeroed(0));
                    let r = self.update_cam(ctx, &mut c);
                    self.std = c;
                    r?;
                }
            }
        }
        Ok(())
    }

    /// MBCameraUpdate of the record followed by the frame's pbWinSetup (a
    /// record taken from a capture becomes the current view).
    pub fn refresh_view(&mut self) -> R<()> {
        let mut c = std::mem::replace(&mut self.std, Record::zeroed(0));
        let r = self.camera_update(&mut c);
        self.std = c;
        r?;
        self.pb.update(&self.window)
    }

    /// One UpdateCam on the record (checks); true when something moved.
    pub fn update_once(&mut self, ctx: &CamCtx) -> R<bool> {
        let mut c = std::mem::replace(&mut self.std, Record::zeroed(0));
        let r = self.update_cam(ctx, &mut c);
        self.std = c;
        r
    }

    /// ShakeCamera 0x16fc20.
    pub fn shake(&mut self, radius: f32, kind: i32, delay: i32, count: i32, priority: i32) {
        let s = &mut self.shake;
        if s.shaking == 0 || s.priority <= priority {
            *s = Shake { shaking: 1, kind, priority, delay, count, radius };
        }
    }

    /// NewCamLimitPlayerDpos 0x116358 (via StdCamLimitPlayerDpos 0x1166e8):
    /// clips a displacement against the frustum normals through the apex
    /// target − max radius × look, testing position +0x70 against planes 0..2
    /// and +0x50 against plane 3. `collide(from, to)` is PlayerWallCollide
    /// then PlayerCollideFloor and returns true when the slide is refused.
    pub fn limit_player_dpos(&self, player: &Record, dpos: &mut V3, slide: bool, collide: &mut dyn FnMut(V3, V3) -> R<bool>) -> R<LimitResult> {
        let c = &self.std;
        let nr = -c.f32_(o::MAX_RADIUS);
        let (t, l) = (c.v3(o::TARGET), c.v3(o::LOOK));
        let apex = [add(mul(nr, l[0])?, t[0])?, add(mul(nr, l[1])?, t[1])?, add(mul(nr, l[2])?, t[2])?];
        let (p50, p70) = (player.v3(0x50), player.v3(0x70));
        let rel = |p: V3, d: &V3| -> R<V3> { Ok([sub(add(p[0], d[0])?, apex[0])?, sub(add(p[1], d[1])?, apex[1])?, sub(add(p[2], d[2])?, apex[2])?]) };
        let base = rel(p50, dpos)?;
        let mut v = rel(p70, dpos)?;
        let mut out = LimitResult { code: 1, floor_reset: false };
        let mut y = dpos[1];
        for k in 0..4 {
            if k == 3 {
                v = base;
            }
            let n = c.v3(o::PLANES + 0x10 * k);
            if Self::dot(v, n)? <= 0.0 {
                continue;
            }
            out.code = 5;
            let mut nf = [n[0], 0.0, n[2]];
            crate::collision::normal_vector_pub(&mut nf)?;
            if Self::dot(*dpos, n)? <= 0.0 {
                continue;
            }
            y = 0.0;
            if !slide {
                *dpos = [0.0; 3];
                continue;
            }
            let k2 = -Self::dot(*dpos, nf)?;
            *dpos = [add(mul(k2, nf[0])?, dpos[0])?, add(mul(k2, nf[1])?, dpos[1])?, add(mul(k2, nf[2])?, dpos[2])?];
            let to = [add(p70[0], dpos[0])?, add(p70[1], dpos[1])?, add(p70[2], dpos[2])?];
            if collide(p70, to)? {
                *dpos = [0.0; 3];
            }
            let along = Self::dot(*dpos, n)?;
            let len2 = add(add(mul(dpos[0], dpos[0])?, mul(dpos[1], dpos[1])?)?, mul(dpos[2], dpos[2])?)?;
            if mul(len2, mul(self.consts.epsilon, self.consts.epsilon)?)? < mul(along, along)? {
                *dpos = [0.0; 3];
            }
            // +0x8a4 = player_water_y when player_water_wobj is set, else +0x88c.
            out.floor_reset = true;
        }
        dpos[1] = y;
        Ok(out)
    }
}

/// NewCamLimitPlayerDpos result: 1 or 5 (a plane was crossed), and whether
/// the slide path rewrote player +0x8a4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitResult {
    pub code: i32,
    pub floor_reset: bool,
}

// ---- Boss camera (BossCameraUpdate 0x1a1bc0 and helpers) ----

/// Boss camera tuning globals 0x25d4f0..0x25d524, read from the ELF.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BossCamConsts {
    pub min_flip_angle: f32,
    pub dist_factor: f32,
    pub max_yaw_vel: f32,
    pub yaw_accel: f32,
    pub max_pitch_vel: f32,
    pub pitch_accel: f32,
    pub max_dist_vel: f32,
    pub dist_accel: f32,
    pub max_dir_vel: f32,
    pub dir_accel: f32,
}

impl BossCamConsts {
    pub fn from_elf(elf: &Elf) -> Result<Self, Box<dyn std::error::Error>> {
        let f = |n: &str| -> Result<f32, Box<dyn std::error::Error>> {
            let b = elf.object(n)?;
            Ok(f32::from_le_bytes(b.get(..4).ok_or("short boss camera global")?.try_into()?))
        };
        Ok(Self {
            min_flip_angle: f("BOSS_MIN_FLIP_ANGLE")?,
            dist_factor: f("BOSS_DIST_FACTOR")?,
            max_yaw_vel: f("CAMMAXYAWVEL")?,
            yaw_accel: f("CAMYAWACCEL")?,
            max_pitch_vel: f("CAMMAXPITCHVEL")?,
            pitch_accel: f("CAMPITCHACCEL")?,
            max_dist_vel: f("CAMMAXDISTVEL")?,
            dist_accel: f("CAMDISTACCEL")?,
            max_dir_vel: f("CAMMAXDIRVEL")?,
            dir_accel: f("CAMDIRACCEL")?,
        })
    }
}

/// Boss-camera inputs from the game for one update.
pub struct BossCtx<'a> {
    pub players: &'a [Record; 4],
    pub spf: f32,
    pub game_mode: u32,
    /// BossCritInst record (+0x50 attention, +0x420 home matrix, +0x460
    /// home position) and its type's +0x78 radius.
    pub boss: Option<(&'a Record, f32)>,
    pub boss_active: i32,
    pub boss_is_dying: i32,
    pub good_wizard: i32,
    /// good_wiz_objgrp +0x40 while its +0x60 word is set.
    pub good_wiz_pos: Option<V3>,
    /// Effects[BossKeySfx] tree +0x30 while the key effect lives.
    pub boss_key_pos: Option<V3>,
}

/// BossCameraInit 0x19f270 (per level via reset_game_data) state and the
/// level's BCAM record (LEVL +0x6c; 0x54 bytes, rewritten by
/// BossCameraStart / the calculators as the original does).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BossCamera {
    pub rec: Record,
    /// BossCameraMode 0x25d4f8.
    pub mode: i32,
    pub data: Option<Vec<u8>>,
    pub consts: BossCamConsts,
    /// CurTransmitter 0x21f11c (GetTransmitter's 2-D choice).
    pub cur_transmitter: i32,
}

/// Window constants MBWindowSetAng / MBWindowInit leave in MboxWinPtr:
/// +0x24/+0x28 tan of the half angles, +0x34/+0x38 their cosines, +0x5c 1.0.
#[derive(Debug, Clone, Copy)]
struct WinConsts {
    tan_h: f32,
    tan_v: f32,
    cos_h: f32,
    cos_v: f32,
    near: f32,
}

impl Pb {
    fn win_consts(&self) -> R<WinConsts> {
        let (h, v) = (mul(self.ang[0], 0.5)?, mul(self.ang[1], 0.5)?);
        let (ch, sh, cv, sv) = (cosf(h)?, sinf(h)?, cosf(v)?, sinf(v)?);
        Ok(WinConsts { tan_h: div(sh, ch)?, tan_v: div(sv, cv)?, cos_h: ch, cos_v: cv, near: 1.0 })
    }
}

fn bf(d: &[u8], k: usize) -> f32 {
    f32::from_le_bytes(d[4 * k..4 * k + 4].try_into().unwrap())
}
fn set_bf(d: &mut [u8], k: usize, v: f32) {
    d[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
}
fn bu(d: &[u8]) -> u32 {
    u32::from_le_bytes(d[..4].try_into().unwrap())
}
fn sqrt3(v: V3) -> R<f32> {
    Ok(sqrt_rn(add(add(mul(v[0], v[0])?, mul(v[1], v[1])?)?, mul(v[2], v[2])?)?)?)
}
fn vsub3(a: V3, b: V3) -> R<V3> {
    Ok([sub(a[0], b[0])?, sub(a[1], b[1])?, sub(a[2], b[2])?])
}
fn vadd3(a: V3, b: V3) -> R<V3> {
    Ok([add(a[0], b[0])?, add(a[1], b[1])?, add(a[2], b[2])?])
}
/// NormalVector2D 0x1d5eb0 (fqdist length).
fn normal_vector_2d(v: &mut V3) -> R<f32> {
    let (d, _) = crate::vu0::fqdist(v[0], v[2])?;
    let k = if d <= 0.0 { 1.0 } else { div(1.0, d)? };
    v[0] = mul(k, v[0])?;
    v[1] = 0.0;
    v[2] = mul(k, v[2])?;
    Ok(d)
}
/// SlowNormalVector2D 0x1d5f38 (sqrt.S length).
fn slow_normal_vector_2d(v: &mut V3) -> R<f32> {
    let d = sqrt_rn(add(mul(v[0], v[0])?, mul(v[2], v[2])?)?)?;
    let k = if d <= 0.0 { 1.0 } else { div(1.0, d)? };
    v[0] = mul(k, v[0])?;
    v[1] = 0.0;
    v[2] = mul(k, v[2])?;
    Ok(d)
}
/// GetYawPitch 0x1d6258.
fn get_yaw_pitch(v: V3) -> R<(f32, f32)> {
    let yaw = crate::libm::atan2f(v[0], v[2])?;
    let (xz, _) = crate::vu0::fqdist(v[0], v[2])?;
    Ok((yaw, crate::libm::atan2f(v[1], xz)?))
}
/// One-step wrap and the flip by π used by GetActualAvgVec.
fn flip(a: f32) -> R<f32> {
    let f1 = add(a, PI)?;
    if PI < f1 {
        return Ok(sub(f1, TWO_PI)?);
    }
    let f12 = add(a, PI)?;
    if f12 <= -PI {
        Ok(add(f12, TWO_PI)?)
    } else {
        Ok(f12)
    }
}

/// LimitCamVal2 0x19f098(cur, target, min, max, accel, lead, velocity,
/// wrap): a velocity-limited approach; returns cur + v × SPF.
#[allow(clippy::too_many_arguments)]
fn limit_cam_val2(spf: f32, cur: f32, target: f32, min: f32, max: f32, accel: f32, lead: f32, vel: &mut f32, wrap_angle: bool) -> R<f32> {
    let step = mul(accel, spf)?;
    let mut d = sub(target, cur)?;
    if wrap_angle {
        d = wrap(d)?;
    }
    let ad = d.abs();
    let mut v = *vel;
    let near = mul(mul(lead, max)?, spf)?;
    let av = v.abs();
    if 0.0 < lead && av < near && ad < near {
        v = 0.0;
    } else if ad < mul(add(av, step)?, spf)? {
        // FPS 0x20f90c = 1 / SPF (ClockOncePerFrame).
        v = mul(d, div(1.0, spf)?)?;
    } else {
        // |d| / |v| with v = 0: the EE's divide-by-zero result (±FLT_MAX).
        if crate::numeric::div_rn_ee(ad, av)? <= crate::numeric::div_rn_ee(av, accel)? {
            if 0.0 < v {
                v = sub(v, step)?;
                if v < 0.0 {
                    v = 0.0;
                }
            } else {
                v = add(v, step)?;
                if 0.0 < v {
                    v = 0.0;
                }
            }
        } else if 0.0 < d {
            v = add(v, step)?;
        } else {
            v = sub(v, step)?;
        }
        if v < min {
            v = min;
        } else if max < v {
            v = max;
        }
    }
    *vel = v;
    Ok(add(cur, mul(v, spf)?)?)
}

impl BossCamera {
    pub fn new(consts: BossCamConsts, data: Option<Vec<u8>>, cur_transmitter: i32, hist: &CamConsts) -> Self {
        let mut rec = Record::zeroed(o::SIZE);
        Camera::reset(&mut rec, hist);
        Self { rec, mode: 0, data, consts, cur_transmitter }
    }

    /// GetPlayerAvgPos 0x114a28(mode): players in state 1 (+0xf0 while
    /// grabbed, else +0x60); mode 0 averages, otherwise the min/max midpoint
    /// (mode 2 also clamps to the CAMS box). Returns (point, min, max, 1/count).
    fn player_avg_pos(players: &[Record; 4], mode: u32, cam: &Camera) -> R<(V3, V3, V3, f32)> {
        let mut out = [0.0f32; 3];
        let mut lo = [f32::from_bits(0x60ad_78ec); 3];
        let mut hi = [f32::from_bits(0xe0ad_78ec); 3];
        let mut n = 0.0f32;
        for p in players.iter() {
            if p.i32_(0xfc) != 1 {
                continue;
            }
            let at = if p.u16_(0x93c) & 0x20 != 0 { p.v3(0xf0) } else { p.v3(0x60) };
            if mode == 0 {
                out = vadd3(at, out)?;
            } else {
                for k in 0..3 {
                    if at[k] <= lo[k] {
                        lo[k] = at[k];
                    }
                    if hi[k] <= at[k] {
                        hi[k] = at[k];
                    }
                }
            }
            n = add(n, 1.0)?;
        }
        if n == 0.0 {
            return unsupported("GetPlayerAvgPos with no player in state 1 (PlayerStartPos)");
        }
        let inv = div(1.0, n)?;
        if mode == 0 {
            out = [mul(inv, out[0])?, mul(inv, out[1])?, mul(inv, out[2])?];
        } else {
            for k in 0..3 {
                out[k] = mul(add(lo[k], hi[k])?, 0.5)?;
            }
        }
        if mode == 2 {
            for k in 0..3 {
                out[k] = if out[k] < cam.min[k] {
                    cam.min[k]
                } else if cam.max[k] < out[k] {
                    cam.max[k]
                } else {
                    out[k]
                };
            }
        }
        Ok((out, lo, hi, inv))
    }

    /// GetBossAvgPos 0x19fb38(weight, point, min, max, mode).
    fn boss_avg_pos(weight: f32, point: V3, lo: V3, hi: V3, mode: u32, boss: V3) -> R<V3> {
        if weight <= 0.0 {
            return Ok(boss);
        }
        if mode == 0 {
            let w = add(weight, 1.0)?;
            let k = div(1.0, w)?;
            let m = mul(sub(w, 1.0)?, k)?;
            let mut out = [0.0; 3];
            for i in 0..3 {
                out[i] = add(mul(k, boss[i])?, mul(m, point[i])?)?;
            }
            return Ok(out);
        }
        let mut out = [0.0; 3];
        for i in 0..3 {
            let l = if boss[i] <= lo[i] { boss[i] } else { lo[i] };
            let h = if h_le(hi[i], boss[i]) { boss[i] } else { hi[i] };
            out[i] = mul(add(l, h)?, 0.5)?;
        }
        Ok(out)
    }

    /// PointInView 0x1a2148 → MulBodyVecMat4 + PointViewDist 0x1a1fc8.
    fn point_in_view(rec: &Record, w: WinConsts, r: f32, p: V3) -> R<(f32, u32)> {
        let d = vsub3(p, rec.v3(o::EYE))?;
        let row = |k: usize| -> R<f32> { Camera::dot(d, rec.v3(o::MAT + 0x10 * k)) };
        let (x, y, z) = (row(0)?, row(1)?, row(2)?);
        let mut mask = 0u32;
        let mut best = f32::from_bits(0x60ad_78ec);
        let f = sub(sub(z, w.near)?, r)?;
        if f < best {
            best = f;
            if f < 0.0 {
                mask = 1;
            }
        }
        let (zh, zv) = (mul(z, w.tan_h)?, mul(z, w.tan_v)?);
        for (val, bit) in [
            (sub(mul(add(zh, x)?, w.cos_h)?, r)?, 0x10),
            (sub(mul(sub(zh, x)?, w.cos_h)?, r)?, 0x20),
            (sub(mul(add(zv, y)?, w.cos_v)?, r)?, 0x200),
            (sub(mul(sub(zv, y)?, w.cos_v)?, r)?, 0x100),
        ] {
            if val < best {
                best = val;
                if val < 0.0 {
                    mask |= bit;
                }
            }
        }
        Ok((best, mask))
    }

    /// GetPlayerViewDist 0x19fd18: nearest frustum margin of the players in
    /// state 1 (position +0x60 + displacement +0x860, radius +0x82c).
    fn player_view_dist(&self, players: &[Record; 4], w: WinConsts) -> R<f32> {
        let mut best = f32::from_bits(0x60ad_78ec);
        for p in players.iter().filter(|p| p.i32_(0xfc) == 1) {
            let at = vadd3(p.v3(0x60), p.v3(0x860))?;
            let (d, _) = Self::point_in_view(&self.rec, w, p.f32_(0x82c), at)?;
            if d < best {
                best = d;
            }
        }
        Ok(best)
    }

    /// GetActualAvgVec 0x19f490: the direction from the two farthest
    /// entities (players, optionally the boss) toward the attention point.
    fn actual_avg_vec(&self, ctx: &BossCtx, attn: V3, with_boss: bool) -> R<(V3, f32)> {
        let mut count = 0.0f32;
        let mut notable = 0;
        let (mut first, mut second) = ([0.0f32; 3], [0.0f32; 3]);
        let (mut max, mut max2) = (0.0f32, 0.0f32);
        let mut targets: Vec<V3> = Vec::new();
        if with_boss {
            targets.push(ctx.boss.map(|(b, _)| b.v3(0x50)).ok_or_else(|| super::Unsupported("GetActualAvgVec without BossCritInst".into()))?);
        }
        for p in ctx.players.iter().filter(|p| p.i32_(0xfc) == 1) {
            targets.push(if p.u16_(0x93c) & 0x20 != 0 { p.v3(0xf0) } else { p.v3(0x50) });
        }
        for t in targets {
            let mut d = vsub3(attn, t)?;
            let len = normal_vector_2d(&mut d)?;
            if max < len {
                if 0.0 < max {
                    second = first;
                    max2 = max;
                }
                first = d;
                max = len;
                notable += 1;
            } else if max2 < len {
                second = d;
                max2 = len;
                notable += 1;
            }
            count = add(count, 1.0)?;
        }
        if notable == 0 {
            return Ok(([0.0; 3], 0.0));
        }
        let mut a = crate::libm::atan2f(first[0], first[2])?;
        if notable != 1 {
            let a1 = a;
            let a2 = crate::libm::atan2f(second[0], second[2])?;
            let mut mid = wrap(mul(add(a1, a2)?, 0.5)?)?;
            if f32::from_bits(0x3fc9_0fdb) < wrap(sub(mid, a1)?)?.abs() {
                mid = flip(mid)?;
            }
            let lim = mul(mul(self.consts.min_flip_angle, PI)?, f32::from_bits(0x3bb6_0b61))?;
            if lim < wrap(sub(a1, a2)?)?.abs() && f32::from_bits(0x3fc9_0fdb) < wrap(sub(self.rec.f32_(o::YAW), mid)?)?.abs() {
                mid = flip(mid)?;
            }
            a = mid;
        }
        Ok(([-sinf(a)?, 0.0, -cosf(a)?], count))
    }

    /// GetTransmitter 0x114570 (x/z fqdist; switches below 0.667 × the
    /// current distance).
    fn transmitter_2d(&mut self, cam: &Camera, p: V3) -> R<Option<usize>> {
        if cam.transmitters.is_empty() {
            return Ok(None);
        }
        let d = |t: &Transmitter| -> R<f32> { Ok(crate::vu0::fqdist(sub(p[0], t.pos[0])?, sub(p[2], t.pos[2])?)?.0) };
        let (mut best, mut bd) = (-1i32, 0.0f32);
        for (k, t) in cam.transmitters.iter().enumerate() {
            if t.flag != 0 || k as i32 == self.cur_transmitter {
                continue;
            }
            let dk = d(t)?;
            if best < 0 || dk < bd {
                best = k as i32;
                bd = dk;
            }
        }
        if self.cur_transmitter < 0 {
            self.cur_transmitter = best;
        } else {
            let dc = d(&cam.transmitters[self.cur_transmitter as usize])?;
            if bd <= mul(dc, f32::from_bits(0x3f2a_c083))? {
                self.cur_transmitter = best;
            }
        }
        // Index −1 reads the words before the table (transmitter_before_table).
        Ok(Some(usize::try_from(self.cur_transmitter).unwrap_or(usize::MAX)))
    }

    /// BossCamGetCurrent 0x1a03a8: distance, yaw and pitch from the window eye.
    fn get_current(&mut self, window_eye: V3) -> R<()> {
        let d = vsub3(self.rec.v3(o::TARGET), window_eye)?;
        self.rec.set_f32(o::DIST, sqrt3(d)?);
        let (y, p) = get_yaw_pitch(d)?;
        self.rec.set_f32(o::YAW, y);
        self.rec.set_f32(o::PITCH, p);
        Ok(())
    }

    /// BossCamLimitAttn 0x1a0460: moves the attention point (+0xa4) from its
    /// base (+0xb0) along a velocity-limited direction (+0xbc).
    fn limit_attn(&mut self, spf: f32, target: V3) -> R<()> {
        let c = &self.consts;
        let r = &mut self.rec;
        let mut v = vsub3(target, r.v3(0xb0))?;
        let len = crate::collision::normal_vector_pub(&mut v)?;
        if len <= f32::from_bits(0x3a83_126f) {
            for off in [0xd4, 0xd8] {
                r.set_f32(off, 0.0);
            }
            r.set_v3(0xbc, [0.0; 3]);
            r.set_v3(0xc8, [0.0; 3]);
            r.set_v3(o::TARGET, target);
            r.set_v3(0xb0, target);
            return Ok(());
        }
        if r.f32_(0xd4) <= 0.0 {
            let mut vel = r.f32_(0xd8);
            let along = limit_cam_val2(spf, r.f32_(0xd4), len, -c.max_dist_vel, c.max_dist_vel, c.dist_accel, 0.0, &mut vel, false)?;
            r.set_f32(0xd8, vel);
            r.set_f32(0xd4, along);
            r.set_v3(0xbc, v);
            r.set_v3(0xc8, [0.0; 3]);
            let b = r.v3(0xb0);
            r.set_v3(o::TARGET, [add(mul(v[0], along)?, b[0])?, add(mul(v[1], along)?, b[1])?, add(mul(v[2], along)?, b[2])?]);
            return Ok(());
        }
        if Camera::dot(v, r.v3(0xbc))? < f32::from_bits(0x3f77_0a3d) {
            r.set_f32(0xd4, 0.0);
            r.set_v3(0xb0, r.v3(o::TARGET));
        } else {
            let mut vel = r.f32_(0xd8);
            let along = limit_cam_val2(spf, r.f32_(0xd4), len, -c.max_dist_vel, c.max_dist_vel, c.dist_accel, 0.0, &mut vel, false)?;
            r.set_f32(0xd8, vel);
            r.set_f32(0xd4, along);
        }
        for k in 0..3 {
            let mut vel = r.f32_(0xc8 + 4 * k);
            let x = limit_cam_val2(spf, r.f32_(0xbc + 4 * k), v[k], -c.max_dir_vel, c.max_dir_vel, c.dir_accel, 0.0, &mut vel, false)?;
            r.set_f32(0xc8 + 4 * k, vel);
            r.set_f32(0xbc + 4 * k, x);
        }
        let (dir, along, b) = (r.v3(0xbc), r.f32_(0xd4), r.v3(0xb0));
        r.set_v3(o::TARGET, [add(mul(dir[0], along)?, b[0])?, add(mul(dir[1], along)?, b[1])?, add(mul(dir[2], along)?, b[2])?]);
        Ok(())
    }

    /// BossCamLimit 0x1a0818: yaw, distance and pitch toward their targets.
    fn limit(&mut self, spf: f32, yaw: f32, dist: f32, pitch: f32) -> R<()> {
        let c = self.consts.clone();
        let r = &mut self.rec;
        let lead = f32::from_bits(0x3dcc_cccd);
        let mut v = r.f32_(o::DYAW);
        let y = limit_cam_val2(spf, r.f32_(o::YAW), yaw, -c.max_yaw_vel, c.max_yaw_vel, c.yaw_accel, lead, &mut v, true)?;
        r.set_f32(o::DYAW, v);
        r.set_f32(o::YAW, y);
        let mut v = r.f32_(o::DDIST);
        let d = limit_cam_val2(spf, r.f32_(o::DIST), dist, mul(-c.max_dist_vel, 0.25)?, c.max_dist_vel, c.dist_accel, lead, &mut v, false)?;
        r.set_f32(o::DDIST, v);
        r.set_f32(o::DIST, d);
        let mut v = r.f32_(o::DPITCH);
        let p = limit_cam_val2(spf, r.f32_(o::PITCH), pitch, -c.max_pitch_vel, c.max_pitch_vel, c.pitch_accel, lead, &mut v, true)?;
        r.set_f32(o::DPITCH, v);
        r.set_f32(o::PITCH, p);
        Ok(())
    }

    /// BossCameraStart 0x19fe68.
    fn start(&mut self) -> R<()> {
        let factor = self.consts.dist_factor;
        if let Some(d) = self.data.as_mut() {
            let c = cosf(bf(d, 1))?;
            set_bf(d, 2, c);
            if bf(d, 6) == 0.0 {
                let v = bf(d, 5);
                set_bf(d, 6, v);
            }
            if f32::from_bits(0x4974_23e0) <= bf(d, 0xd) {
                for k in 0..3 {
                    let v = bf(d, 9 + k);
                    set_bf(d, 0xc + k, v);
                }
            }
            // new_camera_switch is 1: × BOSS_DIST_FACTOR.
            self.rec.set_f32(o::MAX_RADIUS, mul(mul(bf(d, 6), 1.5)?, factor)?);
        }
        for off in [o::DYAW, o::DDIST, o::DPITCH, 0xd4, 0xd8, 0x100] {
            self.rec.set_f32(off, 0.0);
        }
        self.rec.set_v3(0xbc, [0.0; 3]);
        self.rec.set_v3(0xc8, [0.0; 3]);
        Ok(())
    }

    /// BossCamStartCalc 0x1a0998.
    fn start_calc(&mut self, cam: &Camera, ctx: &BossCtx, start_of_stage: &mut i32) -> R<()> {
        *start_of_stage = 1;
        let t = cam.start_transmitter.ok_or_else(|| super::Unsupported("BossCamStartCalc without StartTransmitter".into()))?;
        let (p, ..) = Self::player_avg_pos(ctx.players, 1, cam)?;
        let d = vsub3(p, cam.transmitters[t].pos)?;
        let dist = sqrt3(d)?;
        let (y, pi) = get_yaw_pitch(d)?;
        self.rec.set_v3(o::TARGET, p);
        self.rec.set_v3(0xb0, p);
        self.rec.set_f32(o::YAW, y);
        self.rec.set_f32(o::PITCH, pi);
        self.rec.set_f32(o::DIST, dist);
        Ok(())
    }

    /// The view-distance adjustment shared by the player and boss
    /// calculators (thresholds differ; see research/camera.md).
    fn player_calc(&mut self, cam: &Camera, ctx: &BossCtx, w: WinConsts, start_of_stage: &mut i32, window_eye: V3) -> R<()> {
        let d = self.data.clone().ok_or_else(|| super::Unsupported("boss camera without BCAM".into()))?;
        let full = mul(bf(&d, 6), 1.5)?;
        self.rec.set_f32(o::MAX_RADIUS, mul(full, self.consts.dist_factor)?);
        let (p, ..) = Self::player_avg_pos(ctx.players, 2, cam)?;
        let t = if *start_of_stage != 0 { cam.start_transmitter } else { self.transmitter_2d(cam, p)? };
        let (yaw, pitch) = match t {
            Some(k) => {
                let tr = cam.transmitter(k);
                (wrap(sub(tr.yaw, PI)?)?, -tr.pitch)
            }
            None => (0.0, 0.0),
        };
        let mut flags = bu(&d) & 0xffff_fcff;
        let vd = self.player_view_dist(ctx.players, w)?;
        self.rec.set_f32(0x100, vd);
        let mut dist = self.rec.f32_(o::DIST);
        if vd < 0.0 {
            flags |= 0x200;
            dist = add(dist, 10.0)?;
        } else if (vd < 2.0 && dist < full) || (vd < 2.25 && dist < full && bu(&d) & 0x200 != 0) {
            flags |= 0x200;
            let t = sub(2.5, vd)?;
            dist = add(dist, add(t, t)?)?;
        } else if 2.5 < vd || (2.25 < vd && bu(&d) & 0x100 != 0) {
            flags |= 0x100;
            let t = sub(vd, if 2.5 < vd { 2.5 } else { 2.25 })?;
            dist = sub(dist, add(t, t)?)?;
        }
        let data = self.data.as_mut().unwrap();
        data[..4].copy_from_slice(&flags.to_le_bytes());
        let lo = bf(data, 4);
        if dist < lo {
            dist = lo;
        } else if add(full, full)? < dist {
            dist = add(full, full)?;
        }
        if *start_of_stage != 0 && sub(dist, self.rec.f32_(o::DIST))?.abs() < 5.0 {
            *start_of_stage = 0;
        }
        self.rec.set_v3(o::TARGET, p);
        self.rec.set_v3(0xb0, p);
        if self.mode == 0 {
            self.get_current(window_eye)
        } else {
            self.limit(ctx.spf, yaw, dist, pitch)
        }
    }

    /// BossCamBossCalc 0x1a0e90.
    fn boss_calc(&mut self, cam: &Camera, ctx: &BossCtx, w: WinConsts, start_of_stage: &mut i32, window_eye: V3) -> R<()> {
        *start_of_stage = 0;
        let d = self.data.clone().ok_or_else(|| super::Unsupported("boss camera without BCAM".into()))?;
        let (boss, radius) = ctx.boss.ok_or_else(|| super::Unsupported("BossCamBossCalc without BossCritInst".into()))?;
        let home_fwd = [boss.f32_(0x440), 0.0, boss.f32_(0x448)];
        self.rec.set_f32(o::MAX_RADIUS, mul(bf(&d, 5), self.consts.dist_factor)?);
        let t = self.rec.f32_(0xfc);
        let mut off = [0.0f32; 3];
        for k in 0..3 {
            off[k] = add(mul(sub(bf(&d, 0xc + k), bf(&d, 9 + k))?, t)?, bf(&d, 9 + k))?;
        }
        let flags0 = bu(&d);
        let mut snap = 0u32;
        let (target, view_point, view_r);
        if self.mode == 0 {
            target = vadd3(boss.v3(0x460), off)?;
            view_point = target;
            view_r = radius;
        } else {
            let mut kind = 1u32;
            let (tg, vp, vr);
            if let (true, Some(g)) = (ctx.good_wizard != 0, ctx.good_wiz_pos) {
                tg = vadd3(g, [bf(&d, 0x12), bf(&d, 0x13), bf(&d, 0x14)])?;
                vp = g;
                vr = 4.0;
                kind = 2;
            } else if let Some(k) = ctx.boss_key_pos {
                tg = vadd3(k, [bf(&d, 0xf), bf(&d, 0x10), bf(&d, 0x11)])?;
                vp = k;
                vr = 5.0;
                kind = 2;
            } else if ctx.good_wizard != 0 || ctx.boss_is_dying != 0 {
                tg = self.rec.v3(o::TARGET);
                vp = tg;
                vr = 5.0;
            } else {
                let base = if flags0 & 0x10 != 0 {
                    let (p, lo, hi, inv) = Self::player_avg_pos(ctx.players, 1, cam)?;
                    Self::boss_avg_pos(inv, p, lo, hi, 1, boss.v3(0x50))?
                } else if flags0 & 1 != 0 {
                    boss.v3(0x50)
                } else {
                    boss.v3(0x460)
                };
                tg = vadd3(base, off)?;
                vp = boss.v3(0x50);
                vr = radius;
            }
            target = tg;
            view_point = vp;
            view_r = vr;
            let far = sqrt3(vsub3(self.rec.v3(o::TARGET), target)?)?;
            if 0.0 < ctx.spf && mul(self.consts.max_dist_vel, ctx.spf)? < far {
                snap = kind;
            }
        }
        // The direction the camera looks along.
        let (mut v, weight) = if flags0 & 0x20 != 0 {
            let (p, lo, hi, inv) = Self::player_avg_pos(ctx.players, 1, cam)?;
            let p = if flags0 & 8 != 0 { Self::boss_avg_pos(inv, p, lo, hi, 1, boss.v3(0x50))? } else { p };
            let mut v = vsub3(self.rec.v3(o::TARGET), p)?;
            let len = normal_vector_2d(&mut v)?;
            (v, if len < 1.0 { 0.0 } else { inv })
        } else {
            self.actual_avg_vec(ctx, self.rec.v3(o::TARGET), flags0 & 8 != 0)?
        };
        let yaw = if weight == 0.0 {
            self.rec.f32_(o::YAW)
        } else {
            let cos_lim = bf(&d, 2);
            let mut f3;
            if self.mode != 0 {
                let mut e = vsub3(self.rec.v3(o::EYE), self.rec.v3(o::TARGET))?;
                e[1] = 0.0;
                slow_normal_vector_2d(&mut e)?;
                let mut f5 = -1.0;
                if flags0 & 4 == 0 {
                    f3 = add(mul(v[0], home_fwd[0])?, mul(v[2], home_fwd[2])?)?;
                    if cos_lim <= f3 {
                        f5 = add(mul(e[0], v[0])?, mul(e[2], v[2])?)?;
                    }
                } else {
                    f3 = -1.0;
                }
                if flags0 & 2 == 0 {
                    let w2 = [-v[0], -v[1], -v[2]];
                    let f4 = add(mul(w2[0], home_fwd[0])?, mul(w2[2], home_fwd[2])?)?;
                    let f1 = if cos_lim <= f4 { add(mul(e[0], w2[0])?, mul(e[2], w2[2])?)? } else { -1.0 };
                    if f5 < f1 {
                        v = w2;
                        f3 = f4;
                    }
                }
            } else {
                f3 = add(mul(v[0], home_fwd[0])?, mul(v[2], home_fwd[2])?)?;
            }
            let y0 = if f3 < cos_lim {
                let a = crate::libm::atan2f(home_fwd[0], home_fwd[2])?;
                let cross = sub(mul(v[2], home_fwd[0])?, mul(v[0], home_fwd[2])?)?;
                if 0.0 <= cross {
                    sub(a, bf(&d, 1))?
                } else {
                    add(a, bf(&d, 1))?
                }
            } else {
                crate::libm::atan2f(v[0], v[2])?
            };
            wrap(add(y0, PI)?)?
        };
        let dist;
        if self.mode == 0 {
            dist = sqrt3(vsub3(self.rec.v3(o::TARGET), window_eye)?)?;
            self.rec.set_f32(0x100, 0.0);
        } else {
            let mut full = bf(&d, 5);
            let mut flags = flags0 & 0xffff_fcff;
            // GetBossCamViewDist 0x19fdf8: min(players, the viewed point).
            let pd = self.player_view_dist(ctx.players, w)?;
            let (pv, _) = Self::point_in_view(&self.rec, w, view_r, view_point)?;
            let vd = if pv < pd { pv } else { pd };
            self.rec.set_f32(0x100, vd);
            if ctx.good_wizard != 0 {
                full = add(full, full)?;
            }
            let mut f20 = self.rec.f32_(o::DIST);
            if vd < 0.0 {
                flags |= 0x200;
                f20 = add(f20, 10.0)?;
            } else if (vd < 2.0 && f20 < full) || (vd < 2.25 && f20 < full && flags0 & 0x200 != 0) {
                flags |= 0x200;
                let t = sub(2.5, vd)?;
                f20 = add(f20, add(t, t)?)?;
            } else if 4.0 < vd || (2.5 < vd && flags0 & 0x100 != 0) {
                flags |= 0x100;
                f20 = sub(f20, sub(vd, 2.5)?)?;
            }
            let data = self.data.as_mut().unwrap();
            data[..4].copy_from_slice(&flags.to_le_bytes());
            let lo = bf(data, 3);
            dist = if f20 < lo {
                lo
            } else if add(full, full)? < f20 {
                add(full, full)?
            } else {
                f20
            };
        }
        let cur = self.rec.f32_(o::DIST);
        let span = sub(bf(&d, 5), bf(&d, 3))?;
        let t = if f32::from_bits(0x3c23_d70a) < span { div(sub(cur, bf(&d, 3))?, span)? } else { 1.0 };
        let t = t.clamp(0.0, 1.0);
        self.rec.set_f32(0xfc, t);
        let pitch = if sub(cur, dist)?.abs() < 10.0 {
            -add(mul(sub(bf(&d, 8), bf(&d, 7))?, t)?, bf(&d, 7))?
        } else {
            self.rec.f32_(o::PITCH)
        };
        if self.mode == 0 {
            self.rec.set_v3(o::TARGET, target);
            self.rec.set_v3(0xb0, target);
            return self.get_current(window_eye);
        }
        self.limit_attn(ctx.spf, target)?;
        if snap == 0 {
            return self.limit(ctx.spf, yaw, dist, pitch);
        }
        let dv = vsub3(self.rec.v3(o::TARGET), self.rec.v3(o::EYE))?;
        let len = sqrt3(dv)?;
        let (y, p) = get_yaw_pitch(dv)?;
        self.rec.set_f32(o::YAW, y);
        self.rec.set_f32(o::PITCH, p);
        if self.rec.f32_(o::DIST) < len || 2 <= snap {
            self.rec.set_f32(o::DIST, len);
        }
        Ok(())
    }
}

impl BossCamera {
    /// BossCameraUpdate 0x1a1bc0; false when the level has no BCAM record
    /// (main then runs do_camera, not ported).
    pub fn update(&mut self, cam: &mut Camera, ctx: &BossCtx, sh: (i32, i32, i32), cam_wait: &mut i32, start_of_stage: &mut i32) -> R<bool> {
        if *start_of_stage == 0 {
            *cam_wait = 0;
        }
        // WindowScaleH = 0 (presentation).
        if self.mode == 0 {
            self.start()?;
        }
        let w = cam.pb.win_consts()?;
        let window_eye = cam.pb.eye;
        if matches!(ctx.game_mode, 0x4013 | 0x400d | 0x4017) {
            if self.mode == 0 {
                self.mode = 1;
            }
        } else {
            if self.data.is_none() {
                return Ok(false);
            }
            let ctxerr = |stage: &'static str| move |e: Unsupported| Unsupported(format!("{stage}: {}", e.0));
            if self.mode == 0 {
                self.start_calc(cam, ctx, start_of_stage).map_err(ctxerr("BossCamStartCalc"))?;
            } else if ctx.boss_active == 0 {
                self.player_calc(cam, ctx, w, start_of_stage, window_eye).map_err(ctxerr("BossCamPlayerCalc"))?;
            } else {
                self.boss_calc(cam, ctx, w, start_of_stage, window_eye).map_err(ctxerr("BossCamBossCalc"))?;
            }
        }
        let r = &mut self.rec;
        r.set_f32(o::YAW, wrap(r.f32_(o::YAW))?);
        r.set_f32(o::PITCH, wrap(r.f32_(o::PITCH))?);
        let look = Camera::calc_lookdir(r.f32_(o::YAW), r.f32_(o::PITCH))?;
        r.set_v3(o::LOOK, look);
        let (mut eye, mut target) = (r.v3(o::EYE), r.v3(o::TARGET));
        Camera::shake_step(&mut cam.shake, sh.0, sh.1, sh.2, &mut eye, &mut target)?;
        r.set_v3(o::TARGET, target);
        let nd = -r.f32_(o::DIST);
        r.set_v3(o::EYE, [add(mul(nd, look[0])?, target[0])?, add(mul(nd, look[1])?, target[1])?, add(mul(nd, look[2])?, target[2])?]);
        // LookInDirection 0x173cc0 is CamLookInDir's code.
        Camera::look_in_dir(r, look)?;
        // Old Cam[0] copies and ExtractYPR feed the unported Cam array.
        cam.mb_camera_update(&mut self.rec);
        cam.cam0_eye = self.rec.v3(o::EYE);
        cam.cam0_attention = self.rec.v3(o::TARGET);
        self.mode = 1;
        Ok(true)
    }
}

/// `hi <= v` for the max side of GetBossAvgPos (c.lt hi, v → v).
fn h_le(hi: f32, v: f32) -> bool {
    hi <= v
}

// ---- Trigger cameras (TriggerCameraActivate 0x1a2550, TriggerCamUpdate 0x1a26a8) ----

/// trig_cam_mat 0x25d7a0 (rows + eye 0x25d7d0), trig_cam_wait 0x25d540,
/// trig_cam_hold 0x25d548, trig_cam_wobj 0x25d794 (active/delay are globals).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TriggerCam {
    pub rows: [V3; 3],
    pub eye: V3,
    pub wait: i32,
    pub hold: i32,
    /// The bound world object whose animation (+0x10 bit 0x8000000) holds it.
    pub wobj: Option<usize>,
}

/// LinkTriggerToCam 0x150328's Tower cameras (transmitter indices):
/// CrystalCam 0x234308 (id 0xc6), RuneCam 0x23cba0 (0xc9, 0xcb, 0xcd),
/// WindowCam 0x23cbb0 (0xca, 0xcc), SumnerCam 0x23cbb8 tables 0 (0xf0..),
/// 1 (0xdc..) and 2 (0xaa..), 14 entries each.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TowerCams {
    pub crystal: Option<usize>,
    pub rune: [Option<usize>; 3],
    pub window: [Option<usize>; 2],
    pub sumner: [[Option<usize>; 14]; 3],
}

/// LinkTriggerToCam 0x150328 over the kind-9 transmitters: the Tower's
/// camera table, and the (transmitter, id) pairs left for trigger items.
pub fn tower_camera_links(transmitters: &[Transmitter], cur_world: i32) -> (TowerCams, Vec<(usize, i32)>) {
    let mut tc = TowerCams::default();
    let mut items = Vec::new();
    for (k, t) in transmitters.iter().enumerate().filter(|(_, t)| t.flag == 2) {
        let id = i32::from(t.id);
        if cur_world == 0xd {
            if 200 < id {
                match id {
                    0xc9 => tc.rune[0] = Some(k),
                    0xca => tc.window[0] = Some(k),
                    0xcb => tc.rune[1] = Some(k),
                    0xcc => tc.window[1] = Some(k),
                    0xcd => tc.rune[2] = Some(k),
                    _ => {}
                }
                if (0xf0..0xfe).contains(&id) {
                    tc.sumner[0][(id - 0xf0) as usize] = Some(k);
                }
                if (0xdc..0xea).contains(&id) {
                    tc.sumner[1][(id - 0xdc) as usize] = Some(k);
                }
                continue;
            }
            if id == 0xc6 {
                tc.crystal = Some(k);
                continue;
            }
            if (0xaa..0xb8).contains(&id) {
                tc.sumner[2][(id - 0xaa) as usize] = Some(k);
                continue;
            }
        }
        items.push((k, id));
    }
    (tc, items)
}

/// TriggerCameraActivate's matrix: PitchVec3(pitch, YawVec3(π − yaw,
/// (0,0,1))) through LookInDirection 0x173cc0 (trig_cam_mat rows).
pub fn trigger_camera_rows(pitch_yaw: [f32; 2]) -> R<[V3; 3]> {
    let dir = pitch_vec3(pitch_yaw[0], yaw_vec3(sub(PI, pitch_yaw[1])?, FVECTOR)?)?;
    let mut m = Record::zeroed(0x40);
    Camera::look_in_dir(&mut m, dir)?;
    Ok([m.v3(0), m.v3(0x10), m.v3(0x20)])
}

/// RuneCamActivate / WindowCamActivate hold times (0x2a7f98, 0x2a7fa8).
const RUNE_HOLD: [i32; 3] = [300, 300, 300];
const WINDOW_HOLD: [i32; 2] = [300, 300];

impl super::Game {
    fn cam_ctx(&self) -> CamCtx<'_> {
        let g = &self.globals;
        CamCtx {
            players: &self.players,
            num_in_game_players: g.num_in_game_players,
            spf: g.spf,
            vb_elapsed: g.vb_elapsed,
            paused: g.paused,
            help: g.help,
            game_mode: g.game_mode,
        }
    }

    /// init_next_level's camera part once the players are placed:
    /// init_game_cam 0x16fec8 (cam_wait / start_of_stage for a start
    /// transmitter) then StdCamInit(0) and the frame's pbWinSetup.
    pub fn start_camera(&mut self) -> R<()> {
        let Some(mut cam) = self.std_camera.take() else { return Ok(()) };
        let mode = self.globals.game_mode;
        if mode != 0x8006 && mode != 0x8003 && cam.start_transmitter.is_some() {
            // init_stage_info: stage-name text (presentation).
            self.globals.cam_wait = 0x5b;
            self.globals.start_of_stage = 1;
        }
        let r = (|| -> R<()> {
            cam.std_init(&self.cam_ctx(), false)?;
            cam.pb.update(&cam.window.clone())
        })();
        self.std_camera = Some(cam);
        r?;
        self.sync_view();
        Ok(())
    }

    /// HYPOTHESIS (runs that start from a mid-level fixture, which holds no
    /// StdCam record): StdCamInit(1), the restored-stage path that settles on
    /// the players, without the stage-start sequence.
    pub fn start_camera_settled(&mut self) -> R<()> {
        let Some(mut cam) = self.std_camera.take() else { return Ok(()) };
        let r = (|| -> R<()> {
            cam.std_init(&self.cam_ctx(), true)?;
            cam.pb.update(&cam.window.clone())
        })();
        self.std_camera = Some(cam);
        r?;
        self.sync_view();
        Ok(())
    }

    /// main's camera dispatch (0x100600..0x10075c) and MBEndFrame for one
    /// update: front-end modes keep a fixed camera, boss modes and levels with
    /// a BossCritInst run BossCameraUpdate, otherwise StdCamUpdate
    /// (new_camera_switch is 1). TriggerCamUpdate is not ported yet.
    pub(crate) fn camera_step(&mut self) -> R<()> {
        if self.trigger_cam_update()? {
            let w = self.std_camera.as_ref().map(|c| c.window.clone());
            if let (Some(cam), Some(w)) = (self.std_camera.as_mut(), w) {
                cam.pb.update(&w)?;
            }
            self.sync_view();
            return Ok(());
        }
        let Some(mut cam) = self.std_camera.take() else { return Ok(()) };
        let (mut wait, mut start) = (self.globals.cam_wait, self.globals.start_of_stage);
        let mode = self.globals.game_mode;
        let boss_path = matches!(mode, 0x4013 | 0x400d | 0x4017) || (mode & 0x8000 == 0 && self.boss_crit_inst != 0);
        let r = (|| -> R<()> {
            if matches!(mode, 0x8009 | 0x400b | 0x4012) {
                return unsupported("front-end fixed camera (MBCameraUpdate of 0x2a1e40)");
            }
            if !boss_path && mode & 0x8000 != 0 {
                return unsupported("do_camera (game_mode & 0x8000)");
            }
            if boss_path {
                let mut bc = self.boss_camera.take().ok_or_else(|| Unsupported("no boss camera state".into()))?;
                let g = &self.globals;
                if g.spf <= 0.0 {
                    // LimitCamVal2's FPS = 1/SPF (ClockOncePerFrame keeps SPF > 0).
                    self.boss_camera = Some(bc);
                    return unsupported("BossCameraUpdate with SPF 0");
                }
                let boss = usize::try_from(self.boss_crit_inst - 1)
                    .ok()
                    .filter(|&i| self.critters.get(i).is_some_and(|c| c.is_some()))
                    .map(|i| (&self.crit(i).rec, self.critter_type_f32(i, 0x78)))
                    .or_else(|| self.boss_stale.as_ref().map(|(r, f)| (r, *f)));
                let ctx = BossCtx {
                    players: &self.players,
                    spf: g.spf,
                    game_mode: mode,
                    boss,
                    boss_active: g.boss_active,
                    boss_is_dying: self.boss_is_dying,
                    good_wizard: g.good_wizard,
                    // good_wiz_objgrp +0x40 once DoGoodWizard made the node.
                    // HYPOTHESIS: the key effect is taken as living until
                    // level end.
                    good_wiz_pos: self.good_wiz.matrix.map(|_| self.good_wiz.attn),
                    boss_key_pos: self.boss_key_position,
                };
                let done = bc.update(&mut cam, &ctx, (g.paused, g.help, g.vb_elapsed), &mut wait, &mut start);
                self.globals.cur_transmitter = bc.cur_transmitter;
                self.boss_camera = Some(bc);
                if !done? {
                    return unsupported("do_camera (boss level without BCAM)");
                }
                return Ok(());
            }
            cam.std_update(&self.cam_ctx(), &mut wait, &mut start)
        })();
        let r = r.and_then(|_| cam.pb.update(&cam.window.clone()));
        self.globals.cam_wait = wait;
        self.globals.start_of_stage = start;
        self.globals.cur3d_transmitter = cam.cur3d;
        self.std_camera = Some(cam);
        r?;
        self.sync_view();
        Ok(())
    }

    /// The window camera other systems read: MboxWinPtr +0x90/+0x98 for
    /// PlayerMotion and the MBWorldSphereClip view (planes from the start
    /// state's window constants).
    fn sync_view(&mut self) {
        let Some(cam) = &self.std_camera else { return };
        let (fx, fz) = cam.forward_xz();
        self.globals.camera_90 = fx;
        self.globals.camera_98 = fz;
        if let Some(rest) = &self.camera_rest {
            self.camera = Some(super::item_runtime::Camera {
                basis: cam.pb.cam_rows,
                eye: cam.std.v3(o::EYE),
                target: cam.std.v3(o::TARGET),
                planes: rest.planes,
                newcam_mode: 2,
            });
        }
    }

    /// LinkTriggerToCam 0x150328 for every kind-9 locator (AddLocatorInstList
    /// order): Tower camera ids, otherwise trigger items (definition kind 5)
    /// whose +0xf6 byte equals the id get +0x102 = the transmitter index.
    pub fn link_trigger_cameras(&mut self) -> R<()> {
        let Some(cam) = &self.std_camera else { return Ok(()) };
        let (tc, items) = tower_camera_links(&cam.transmitters, self.globals.cur_world);
        self.tower_cams = tc;
        for (k, id) in items {
            for slot in 0..self.items.records.len() {
                let r = &self.items.records[slot];
                // (char)+0xf6 == id: signed byte compare; a second link
                // overwrites the first (Errorf only reports it).
                if r.i16_(0xd8) != -1 && self.items.definitions[r.definition()].kind() == 5 && i32::from(r.i8_(0xf6)) == id {
                    self.items.records[slot].set_i16(0x102, k as i16);
                }
            }
        }
        Ok(())
    }

    /// TriggerCameraActivate 0x1a2550(wobj, position, angles, delay, wait,
    /// hold): a fixed camera, player controls disabled.
    pub fn trigger_camera_activate(&mut self, wobj: Option<usize>, pos: V3, pitch_yaw: [f32; 2], delay: i32, wait: i32, hold: i32) -> R<()> {
        self.globals.trig_cam_delay = match delay {
            d if d < 0 => 1_000_000,
            0 => 0x28,
            d => d * 6,
        };
        self.globals.trig_cam_active = 2;
        // DisablePlayerControls 0x105e20 (ResetPlayerControls clears inputs).
        self.disable_player_controls = 1;
        let rows = trigger_camera_rows(pitch_yaw)?;
        self.trig_cam = TriggerCam { rows, eye: pos, wait, hold, wobj };
        // vibrators_off: presentation.
        Ok(())
    }

    /// TriggerCamUpdate owns the view (its letterbox bars are drawn).
    pub fn trigger_view_active(&self) -> bool {
        self.globals.trig_cam_active != 0 && self.trig_cam.wait <= 0
    }

    /// ShakeCamera 0x16fc20 on the shared shake state.
    pub fn shake_camera(&mut self, radius: f32, kind: i32, delay: i32, count: i32, priority: i32) {
        if let Some(c) = self.std_camera.as_mut() {
            c.shake(radius, kind, delay, count, priority);
        }
    }

    /// TriggerCameraEnd 0x1a2680.
    pub fn trigger_camera_end(&mut self) {
        self.trig_cam.wait = 0;
        self.globals.trig_cam_delay = 0;
        self.trig_cam.hold = 0;
        self.trig_cam.wobj = None;
    }

    fn activate_transmitter_cam(&mut self, k: usize, delay: i32, wait: i32, hold: i32, wobj: Option<usize>) -> R<()> {
        let Some(t) = self.std_camera.as_ref().map(|c| c.transmitter(k)) else { return Ok(()) };
        self.trigger_camera_activate(wobj, t.pos, [t.pitch, t.yaw], delay, wait, hold)
    }

    /// CrystalCamActivate 0x150740.
    pub fn crystal_cam_activate(&mut self) -> R<()> {
        match self.tower_cams.crystal {
            Some(k) => self.activate_transmitter_cam(k, 0x32, 0, 0, None),
            None => unsupported("CrystalCamActivate without CrystalCam"),
        }
    }

    /// SumnerCamActivate 0x1506b0(table, index): the table's camera, else
    /// the nearest lower table's; held until TriggerCameraEnd.
    pub fn sumner_cam_activate(&mut self, table: usize, index: usize) -> R<()> {
        let pick = (0..=table.min(2)).rev().find_map(|t| self.tower_cams.sumner[t].get(index).copied().flatten());
        match pick {
            Some(k) => self.activate_transmitter_cam(k, -1, 0, 0, None),
            None => Ok(()),
        }
    }

    /// RuneCamActivate 0x150590(k): the k-th rune camera or a lower one.
    pub fn rune_cam_activate(&mut self, k: usize) -> R<()> {
        let pick = (0..=k.min(2)).rev().find_map(|i| self.tower_cams.rune[i].map(|t| (i, t)));
        match pick {
            Some((i, t)) => self.activate_transmitter_cam(t, 0, 0, RUNE_HOLD[i], None),
            None => Ok(()),
        }
    }

    /// WindowCamActivate 0x150638(k).
    pub fn window_cam_activate(&mut self, k: usize) -> R<()> {
        match self.tower_cams.window.get(k).copied().flatten().or(self.tower_cams.window[0]) {
            Some(t) => self.activate_transmitter_cam(t, 0, 0, WINDOW_HOLD[k.min(1)], None),
            None => Ok(()),
        }
    }

    /// TriggerCamUpdate 0x1a26a8: true while the trigger camera owns the view
    /// (main then skips its camera dispatch).
    fn trigger_cam_update(&mut self) -> R<bool> {
        if self.globals.trig_cam_active == 0 {
            return Ok(false);
        }
        let vb = self.globals.vb_elapsed;
        if 0 < self.trig_cam.wait {
            self.trig_cam.wait -= vb;
            return Ok(false);
        }
        let Some(cam) = self.std_camera.as_mut() else { return Ok(true) };
        // StdCamFreeze; MBCameraUpdate(trig_cam eye, matrix).
        cam.reset_std = 1;
        let mut m = Record::zeroed(0x40);
        for (k, r) in self.trig_cam.rows.iter().enumerate() {
            m.set_v3(0x10 * k, *r);
        }
        m.set_v3(o::EYE, self.trig_cam.eye);
        cam.mb_camera_update(&mut m);
        cam.cam0_eye = self.trig_cam.eye;
        cam.cam0_attention = self.trig_cam.eye;
        // Letterbox blits and the sandglass hide are presentation.
        self.globals.trig_cam_delay -= vb;
        if 0 < self.globals.trig_cam_delay {
            return Ok(true);
        }
        if 0 < self.trig_cam.hold {
            self.trig_cam.hold -= vb;
            return Ok(true);
        }
        if let Some(o) = self.trig_cam.wobj
            && self.items.objects.get(o).is_some_and(|w| w.flags_10 & 0x800_0000 != 0)
        {
            return Ok(true);
        }
        if self.globals.trig_cam_active == 2 {
            // EnablePlayerControls 0x105e50.
            self.disable_player_controls = 0;
        }
        self.globals.trig_cam_active = 0;
        // AuxLevels & 2 is a debug option: do_camera() (old Cam array) and
        // CameraNewPos(10) are not ported; StdCamUpdate's StdCamReturn below
        // re-applies the gameplay camera.
        Ok(false)
    }

    /// world_update 0x163480's start-of-stage block (game modes 0x4010 and
    /// 0x400c): the entry effect and sound once, then cam_wait 1 three
    /// seconds after WorldStartTime. Fog (InitFog / FogFadeIn) is presentation.
    pub(crate) fn world_update_stage_start(&mut self) -> R<()> {
        let g = &self.globals;
        if g.paused != 0 || g.help != 0 || !(g.game_mode == 0x4010 || g.game_mode == 0x400c) || g.start_of_stage == 0 {
            return Ok(());
        }
        let t = sub(g.time, self.world_start_time)?;
        if t < f32::from_bits(0x3dcc_cccd) {
            return Ok(());
        }
        if self.globals.start_of_stage == 1 {
            self.globals.start_of_stage = 2;
            let at = match &mut self.std_camera {
                Some(cam) => {
                    let ctx = CamCtx {
                        players: &self.players,
                        num_in_game_players: self.globals.num_in_game_players,
                        spf: self.globals.spf,
                        vb_elapsed: self.globals.vb_elapsed,
                        paused: self.globals.paused,
                        help: self.globals.help,
                        game_mode: self.globals.game_mode,
                    };
                    let (p, any) = cam.player_avg(&ctx, 2)?;
                    any.then_some(p)
                }
                None => None,
            };
            // CamGetPlayerAvgPos(2) == 0 → PlayerStartPos 0x22c158.
            let at = match at {
                Some(p) => p,
                None => return unsupported("start of stage with no valid player (PlayerStartPos)"),
            };
            self.start_enter_fx(at)?;
            // AudioPlayerEnter 0x1607a0: AudioFX(2, 0xe0, 9).
            self.sfx2(2, 0, 0xe0, 9);
        }
        if 3.0 <= t && 1 < self.globals.cam_wait {
            self.globals.cam_wait = 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vu_inverse_of_a_rotation_translation_round_trips() {
        // Synthetic: a 90° yaw with a translation inverts exactly.
        let m: M4 = [[0.0, 0.0, -1.0, 0.0], [0.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [3.0, 4.0, 5.0, 1.0]];
        let inv = vu_invers(&m).unwrap();
        assert_eq!(inv[3], [5.0, -4.0, -3.0, 1.0]);
        let back = vu_apply(&inv, [3.0, 4.0, 5.0, 1.0]).unwrap();
        assert_eq!(&back[..3], &[0.0, 0.0, 0.0]);
    }

    #[test]
    fn history_order_starts_at_the_index_and_wraps() {
        let mut c = Record::zeroed(o::SIZE);
        c.set_i32(o::HIST_INDEX, 7);
        assert_eq!(Camera::hist_order(&c, 9), vec![7, 8, 0, 1, 2, 3, 4, 5, 6]);
    }
}
