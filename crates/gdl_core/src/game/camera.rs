//! Gameplay camera: the "StdCam" module (EE 0x113358..0x116720) and the pb
//! view state it drives (research/camera.md).
//!
//! main calls StdCamUpdate 0x115fe8 during play because new_camera_switch
//! 0x20eaa0 is initialised to 1 and never written; the older Cam array /
//! StandardCamera system still runs for trigger and boss cameras (not ported
//! here). The StdCam record (StdCamera 0x2bc740, 0x1b0 bytes) is kept in its
//! original layout so reference traces compare byte for byte.
//!
//! Arithmetic: FPU instructions use the guarded-add / RZ-multiply / RN-divide
//! subset; VU0 macro instructions (sceVu0*, pbApplyMatrix) use the VU RZ subset.
use super::critter_ai::{pitch_vec3, slow_normal_vector, yaw_vec3};
use super::enemy_ai::fix_angle;
use super::record::Record;
use super::{unsupported, R};
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
        })
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
    fn player_avg(&mut self, ctx: &CamCtx, flags: u32) -> R<(V3, bool)> {
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
        if self.cur3d < 0 {
            // Transmitters[-1] reads the words before the table.
            return unsupported("GetTransmitter3D: no other camera transmitter (index -1)");
        }
        Ok(Some(self.cur3d as usize))
    }

    /// TransGetYawPitch 0x116268.
    fn trans_yaw_pitch(&self, t: Option<usize>, ctx: &CamCtx) -> R<(f32, f32)> {
        let (yaw, mut pitch) = match t {
            None => (0.0, 0.0),
            Some(k) => {
                let tr = &self.transmitters[k];
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
        for k in 0..3 {
            c.set_f32(o::MAT + 0x10 * k + 0xc, 0.0);
        }
        c.set_f32(o::MAT + 0x3c, 1.0);
        let eye = c.v3(o::EYE);
        self.pb.eye = eye;
        self.pb.dir = c.v3(o::MAT + 0x20);
        self.pb.cam_rows = [c.v3(o::MAT), c.v3(o::MAT + 0x10), c.v3(o::MAT + 0x20)];
        self.pb.cam_dirty = true;
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
        let s = &mut self.shake;
        if s.shaking == 0 || ctx.paused != 0 || ctx.help != 0 {
            return Ok(());
        }
        s.count -= ctx.vb_elapsed;
        if s.count < 0 {
            s.shaking = 0;
            s.priority = 0;
        }
        s.delay -= ctx.vb_elapsed;
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
        self.cam0_eye = eye;
        self.cam0_attention = target;
        self.camera_update(c)
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
            match t.map(|k| self.transmitters[k].radius) {
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
        // MBEndFrame → pbWinSetup recomputes the pb matrices every frame.
        self.pb.update(&self.window)
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
