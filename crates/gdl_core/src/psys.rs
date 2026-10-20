//! MB particle systems (docs/research/rendering.md "Particle systems"):
//! allocPsys 0x1c9780, setWorldParms 0x1ca5e0, setupParms 0x1cb488,
//! setupNewPMode 0x1cb778, MBDrawPsys 0x1cd8a8 with its emitter helpers
//! (getNewPos*/getNewDir*/getPPos*, getUniqueIdx, getRandomIdx, getPosRect,
//! getConeDir, getOrthoVecs, getCurrentDir) and DrawPsysSub 0x1cd358.
//!
//! A presentation model, independent of Bevy and of the game update: the
//! renderer advances it once per psys frame and draws the returned sprites.
//! Particles live in the original's ring of 16-bit records (position index,
//! direction index, age gap). Arithmetic is host f32; fqdist/inv_sqrtf are
//! host square roots (presentation, not VU-exact). The random source is the
//! executable's rand algorithm on a psys-owned state: the original shares
//! one rand state with the whole game, so particle placement matches the
//! algorithm, not a capture.
use crate::rng::Rand;

/// World/preset psys record size (WORLDS header words 0x1c/0x1d; preset_list).
pub const RECORD: usize = 0x138;
/// initGlobalDirPos: the shared sphere-direction table size.
const GLOBAL_DIRS: usize = 0x3ff;
/// MB_psys_parmscale 0x29a468: (scale, min, max, default) per parameter
/// slot (blue, green, red, alpha, size).
const PARMSCALE: [[f32; 4]; 5] = [
    [255.0, 0.0, 255.0, 255.0],
    [255.0, 0.0, 255.0, 255.0],
    [255.0, 0.0, 255.0, 255.0],
    [255.0, 0.0, 255.0, 255.0],
    [450.0, 1.0, 100000.0, 225.0],
];

fn word(r: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(r[i * 4..i * 4 + 4].try_into().unwrap())
}
fn float(r: &[u8], i: usize) -> f32 {
    f32::from_bits(word(r, i))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PosFn {
    Single1,
    Single2,
    Frame,
    RectShare,
    RectUnique,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirFn {
    Single1,
    Single2,
    Frame,
    Sphere,
    ConeShare,
    ConeUnique,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PPosFn {
    Linear,
    Speed,
    Grav,
    SpeedGrav,
}

/// One particle sprite (DrawPsysSub): a camera-facing square of side
/// `size` at `position`, GS colour bytes (R, G, B, A with A already halved
/// to the GS 0x80 scale).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    pub position: [f32; 3],
    pub size: f32,
    pub rgba: [u8; 4],
}

/// The draw state a psys's sprites use (pbBlitSetDrawRegs with the psys
/// node's flags).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Blend {
    /// ALPHA_1 0x44.
    Normal,
    /// ALPHA_1 0x48 (node flag 0x800000).
    Additive,
    /// ALPHA_1 0x89 (psys flag 0x10).
    Multiply,
}

/// Global psys state: the psys frame counter (MBPsysStartFrame's
/// DAT_0029a3dc), the random source and the shared direction table.
pub struct PsysGlobal {
    pub frame: u32,
    pub rand: Rand,
    dirs: Vec<[f32; 3]>,
    dir_share: Vec<u8>,
    /// DAT_0029a41c / DAT_0029a41e: per-MBDrawPsys frame-mode caches.
    frame_dir: i32,
    frame_pos: i32,
    /// psys_num_rendered (DrawPsysSub draws at most 0x800 per frame).
    rendered: u32,
}

impl PsysGlobal {
    pub fn new(seed: u32) -> Self {
        PsysGlobal {
            frame: 0,
            rand: Rand::new(seed),
            dirs: vec![[0.0; 3]; GLOBAL_DIRS],
            dir_share: vec![0; GLOBAL_DIRS],
            frame_dir: -1,
            frame_pos: -1,
            rendered: 0,
        }
    }

    /// MBPsysStartFrame with one psys frame elapsed (HYPOTHESIS: psys frames
    /// advance at 30 Hz; the original derives them from ticks_elapsed / 1e7).
    pub fn start_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.rendered = 0;
    }

    fn rand15(&mut self) -> f32 {
        (self.rand.rand() & 0x7fff) as f32
    }
}

/// A psys (the 0x130-byte allocPsys object) with its node's flags.
#[derive(Clone, Debug)]
pub struct Psys {
    /// The MB node flags (+0x60) the psys's parameters adjust.
    pub node_flags: u32,
    flags: u16,
    max_particles: u16,
    dir_count: u16,
    pos_count: u16,
    age_bits: u8,
    dir_bits: u8,
    pos_bits: u8,
    pub state: u8,
    start_delay: u16,
    emit_frames: u16,
    decay_frames: u16,
    cone: f32,
    dir: [f32; 3],
    rect: [f32; 3],
    fade_in: u8,
    fade_out: u8,
    pos_fn: PosFn,
    dir_fn: DirFn,
    ppos_fn: PPosFn,
    ring: Vec<u16>,
    dirs: Vec<[f32; 3]>,
    global_dirs: bool,
    poss: Vec<[f32; 3]>,
    dir_share: Option<Vec<u8>>,
    pos_share: Option<Vec<u8>>,
    dir_cursor: u16,
    dir_end: u16,
    pos_cursor: u16,
    pos_end: u16,
    head: Option<usize>,
    tail: usize,
    oldest_age: u16,
    newest_age: u16,
    accum: f32,
    drawn: u16,
    pub visible: bool,
    speed: f32,
    /// +0x88: the texture name (MBPsysSetPTexName) or None for the default.
    pub texture: Option<String>,
    last_frame: u32,
    state_frames: u32,
    gravity: f32,
    pub radius: f32,
    rate_random: f32,
    rates: [f32; 4],
    params: [[f32; 4]; 5],
}

impl Psys {
    /// createPsysNode + allocPsys defaults; `frame` is the global psys frame.
    pub fn new(frame: u32) -> Self {
        Psys {
            node_flags: 0,
            flags: 0,
            max_particles: 0,
            dir_count: 0,
            pos_count: 0,
            age_bits: 0,
            dir_bits: 0,
            pos_bits: 0,
            state: 0,
            start_delay: 0,
            emit_frames: 300,
            decay_frames: 300,
            cone: -1.0,
            dir: [0.0, 1.0, 0.0],
            rect: [0.0; 3],
            fade_in: 0x14,
            fade_out: 10,
            pos_fn: PosFn::Single1,
            dir_fn: DirFn::Single1,
            ppos_fn: PPosFn::Linear,
            ring: Vec::new(),
            dirs: Vec::new(),
            global_dirs: false,
            poss: Vec::new(),
            dir_share: None,
            pos_share: None,
            dir_cursor: 0,
            dir_end: 0,
            pos_cursor: 0,
            pos_end: 0,
            head: None,
            tail: 0,
            oldest_age: 0,
            newest_age: 0,
            accum: 0.0,
            drawn: 0,
            visible: false,
            speed: f32::from_bits(0x3d75c28f),
            texture: None,
            last_frame: frame,
            state_frames: 0,
            gravity: 0.0,
            radius: 10.0,
            rate_random: 0.0,
            rates: [0.0, 1.0, 1.0, 1.0],
            params: std::array::from_fn(|k| [PARMSCALE[k][3]; 4]),
        }
    }

    /// MBNewWorldPsys 0x1cb218 for a world psys node (WorldPsysActivate):
    /// createPsysNode receives the caller's a3, the node's file flag 0x1000
    /// (nonzero → +0x2c |= 0xc0, per-frame position/direction); then
    /// setWorldParms with the record (its preset first; `dir` = the negated
    /// first collision normal); a named psys loops (+0x2c |= 2).
    pub fn new_world(frame: u32, record: &[u8], presets: &[u8], dir: Option<[f32; 3]>, file_flag_1000: bool) -> Result<Self, String> {
        let mut p = Psys::new(frame);
        if file_flag_1000 {
            p.flags |= 0xc0;
        }
        p.set_world_parms(record, presets, dir, 0)?;
        p.flags |= 2;
        Ok(p)
    }

    /// The draw state of this psys's sprites.
    pub fn blend(&self) -> Blend {
        if self.node_flags & 0x800000 != 0 {
            Blend::Additive
        } else if self.flags & 0x10 != 0 {
            Blend::Multiply
        } else {
            Blend::Normal
        }
    }
    /// pbBlitSetDrawRegs: psys flag 0x20 replaces As with FIX 0x80.
    pub fn fixed_alpha(&self) -> bool {
        self.flags & 0x20 != 0
    }
    /// ZBUF ZMSK (node flag 0x80) and ZTST ALWAYS (node flag 0x40).
    pub fn depth_flags(&self) -> (bool, bool) {
        (self.node_flags & 0x80 != 0, self.node_flags & 0x40 != 0)
    }
    pub fn finished(&self) -> bool {
        self.state >= 7
    }

    fn set_parm(&mut self, v: [f32; 4], slot: usize) {
        // MBPsysSetPParm: clamp(v × scale, min, max).
        let [scale, min, max, _] = PARMSCALE[slot];
        for (k, x) in v.into_iter().enumerate() {
            let x = x * scale;
            self.params[slot][k] = if x < min {
                min
            } else if max < x {
                max
            } else {
                x
            };
        }
    }

    /// setWorldParms 0x1ca5e0 (depth 0 = the record, 1 = its preset).
    fn set_world_parms(&mut self, r: &[u8], presets: &[u8], dir: Option<[f32; 3]>, depth: u32) -> Result<(), String> {
        if r.len() < RECORD {
            return Err("psys record truncated".into());
        }
        if word(r, 0) < 0x100 {
            return Err("psys record version below 0x100".into());
        }
        let has = |bit: u32| word(r, 4) & bit != 0;
        if has(1) && depth == 0 {
            let id = i16::from_le_bytes(r[4..6].try_into().unwrap());
            for preset in presets.chunks_exact(RECORD) {
                let pid = i16::from_le_bytes(preset[4..6].try_into().unwrap());
                if pid == -1 {
                    break;
                }
                if pid == id {
                    self.set_world_parms(preset, presets, None, 1)?;
                    break;
                }
            }
        }
        if has(2) {
            self.max_particles = word(r, 5) as u16;
        }
        if has(8) {
            self.pos_count = word(r, 7) as u16;
        }
        if has(4) {
            self.dir_count = word(r, 6) as u16;
        }
        if has(0x10) {
            let to_u16 = |x: f32, low: u16| -> u16 {
                if x < low as f32 {
                    low
                } else if x <= 65535.0 {
                    x as i32 as u16
                } else {
                    0xffff
                }
            };
            let (e, d) = (float(r, 8), float(r, 9));
            self.emit_frames = to_u16(e * 30.0, 1);
            self.decay_frames = if d * 30.0 < 0.0 { 0 } else { to_u16(d * 30.0, 0) };
            if e < 0.0 {
                self.emit_frames = 0xffff;
                self.flags |= 2;
            } else if d < 0.0 {
                self.decay_frames = 0xffff;
                self.flags |= 2;
            }
        }
        if has(0x20) {
            let to_u8 = |x: f32, low: u8| -> u8 {
                if x < low as f32 {
                    low
                } else if x <= 255.0 {
                    x as i32 as u8
                } else {
                    0xff
                }
            };
            self.fade_in = to_u8(float(r, 10) * 30.0, 1);
            let o = float(r, 11) * 30.0;
            self.fade_out = if o < 0.0 { 0 } else { to_u8(o, 0) };
        }
        if has(0x40) {
            // setAngle: half-angle in radians; ≥ 359° (or < 0) → −1 (sphere).
            let a = float(r, 0xe);
            self.cone = if 0.0 <= a {
                if a < 1.0 {
                    0.0
                } else if a < 359.0 {
                    a * std::f32::consts::PI / 360.0
                } else {
                    -1.0
                }
            } else {
                -1.0
            };
        }
        if has(0x80000) {
            let d = float(r, 0x2e) * 30.0;
            self.start_delay = if 0.0 < d { d as i32 as u16 } else { 0 };
        }
        if has(0x80) {
            self.dir = [float(r, 0x18), float(r, 0x19), float(r, 0x1a)];
        } else if depth == 0 {
            self.dir = match dir {
                Some(d) if d != [0.0; 3] => d,
                _ => [0.0, 1.0, 0.0],
            };
        }
        if has(0x100) {
            self.rect = [float(r, 0x1b), float(r, 0x1c), float(r, 0x1d)];
        }
        if has(0x200) {
            for k in 0..4 {
                self.rates[k] = float(r, 0x1e + k) * f32::from_bits(0x3d088889);
            }
        }
        if has(0x400) {
            self.rate_random = float(r, 0x22) * 0.01;
        }
        if has(0x800) {
            self.gravity = float(r, 0x23) * -0.035555556;
        }
        if has(0x2000) {
            self.speed = float(r, 0x25) * f32::from_bits(0x3d088889);
        }
        let colour = |k: usize, c: usize| f32::from(r[0x98 + 4 * k + c]) * 0.003921569;
        if has(0x10000) {
            for slot in [2usize, 1, 0] {
                self.set_parm(std::array::from_fn(|k| colour(k, slot)), slot);
            }
        }
        if has(0x20000) {
            self.set_parm(std::array::from_fn(|k| colour(k, 3)), 3);
        }
        if has(0x40000) {
            self.set_parm(std::array::from_fn(|k| float(r, 0x2a + k)), 4);
        }
        if has(0x4000) {
            let name = &r[0x40..0x60];
            let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            self.texture = Some(String::from_utf8_lossy(&name[..end]).into_owned());
        }
        let values = word(r, 2);
        let mask = word(r, 3);
        if values & 8 != 0 && self.gravity == 0.0 {
            self.gravity = f32::from_bits(0xbd11a2b4);
        }
        let mut f = self.flags;
        if mask & 1 != 0 {
            f &= 0xff3f;
            if values & 1 != 0 {
                f |= 0xc0;
            }
        }
        if mask & 2 != 0 {
            f = (f & 0xfffe) | ((values >> 1) & 1) as u16;
        }
        if mask & 4 != 0 {
            f &= 0xfffd;
            if values & 4 != 0 {
                f |= 2;
            }
        }
        if mask & 0x40 != 0 {
            f &= 0xffdf;
            if values & 0x40 != 0 {
                f |= 0x20;
            }
        }
        if mask & 0x20 != 0 {
            f &= 0xffef;
            if values & 0x20 != 0 {
                f |= 0x10;
            }
        }
        self.flags = f;
        let mut n = self.node_flags;
        for (m, node) in [(0x80u32, 0x800000u32), (0x100, 0x40000000), (0x200, 0x800), (0x400, 0x40), (0x800, 0x80)] {
            if mask & m != 0 {
                n &= !node;
                if values & m != 0 {
                    n |= node;
                }
            }
        }
        self.node_flags = n;
        Ok(())
    }

    /// setupParms 0x1cb488: rate and parameter slopes.
    fn setup_parms(&mut self) {
        if self.emit_frames == 0 {
            self.emit_frames = 1;
        }
        let inv_decay = if self.decay_frames == 0 { 0.0 } else { 1.0 / self.decay_frames as f32 };
        let emit = self.emit_frames as f32;
        let old = self.rates[2];
        self.rates[1] = (self.rates[1] - self.rates[0]) * (1.0 / emit);
        let mut slope2 = 0.0;
        if inv_decay != 0.0 {
            self.rates[2] = (old - self.rates[3]) * emit * inv_decay + old;
            slope2 = (self.rates[3] - old) * inv_decay;
        }
        self.rates[3] = slope2;
        if self.fade_in == 0 {
            self.fade_in = 1;
        }
        let inv_out = if self.fade_out == 0 { 0.0 } else { 1.0 / self.fade_out as f32 };
        let fade_in = self.fade_in as f32;
        let inv_in = 1.0 / fade_in;
        for slot in (0..5).rev() {
            let [v0, v1, v2, v3] = self.params[slot];
            let p = &mut self.params[slot];
            p[0] = v0;
            p[1] = if inv_in == 0.0 || v0 == v1 { 0.0 } else { (v1 - v0) * inv_in };
            if inv_out == 0.0 || v2 == v3 {
                p[2] = v2;
                p[3] = 0.0;
            } else {
                p[2] = (v2 - v3) * fade_in * inv_out + v2;
                p[3] = (v3 - v2) * inv_out;
            }
        }
    }

    /// setupNewPMode 0x1cb778: ring/table sizes and emitter functions.
    fn setup_new_pmode(&mut self, global: &PsysGlobal) -> bool {
        let flags = self.flags;
        let mut avg = 0.0f32;
        let mut life = 0.0f32;
        let mut count: i32;
        if flags & 1 == 0 && self.max_particles == 0 {
            let emit = self.emit_frames as f32;
            let decay = self.decay_frames as f32;
            life = self.fade_in as f32 + self.fade_out as f32;
            let total = emit + decay;
            let (r0, s0, r2, s2) = (self.rates[0], self.rates[1], self.rates[2], self.rates[3]);
            let est: f32;
            if life < total {
                if s0 <= 0.0 && s2 < 0.0 {
                    if emit < life {
                        let rest = life - emit;
                        let a = calc_line_avg(r0, s0, emit, emit, 2) * emit;
                        let b = calc_line_avg(r2, s2, decay, rest, 0) * rest;
                        avg = (a + b) / life;
                    } else {
                        avg = calc_line_avg(r0, s0, emit, life, 0);
                    }
                    est = avg * life;
                } else if 0.0 < s0 {
                    if 0.0 <= s2 {
                        if life <= decay {
                            avg = calc_line_avg(r2, s2, decay, life, 1);
                        } else {
                            let span = life - decay;
                            let a = calc_line_avg(r0, s0, emit, span, 1) * span;
                            let b = calc_line_avg(r2, s2, decay, decay, 2) * decay;
                            avg = (a + b) / life;
                        }
                    } else {
                        let mut t1 = life * -(s2 / r2);
                        let mut t2 = life - t1;
                        if emit < t1 {
                            t2 = life - emit;
                            t1 = emit;
                        } else if decay < t2 {
                            t1 = life - decay;
                            t2 = decay;
                        }
                        let a = calc_line_avg(r0, s0, emit, t1, 1) * t1;
                        let b = calc_line_avg(r2, s2, decay, t2, 0) * t2;
                        avg = (a + b) / life;
                    }
                    est = avg * life;
                } else {
                    let first = if emit < life {
                        let rest = life - emit;
                        let x = calc_line_avg(r0, s0, emit, emit, 2);
                        let y = calc_line_avg(r2, s2, decay, rest, 0);
                        (x * emit + y * rest) / life
                    } else {
                        calc_line_avg(r0, s0, emit, life, 0)
                    };
                    let second = if decay < life {
                        let rest = life - decay;
                        let x = calc_line_avg(r0, s0, emit, rest, 1);
                        let y = calc_line_avg(r2, s2, decay, decay, 2);
                        (x * rest + y * decay) / life
                    } else {
                        calc_line_avg(r2, s2, decay, life, 1)
                    };
                    avg = if second < first { first } else { second };
                    est = avg * life;
                }
            } else {
                let a = calc_line_avg(r0, s0, emit, emit, 2) * emit;
                let b = calc_line_avg(r2, s2, decay, decay, 2) * decay;
                life = total;
                avg = (a + b) / life;
                est = avg * life;
            }
            let limit = if self.max_particles != 0 { self.max_particles as i32 } else { 300 };
            count = est as i32;
            if limit <= count {
                count = limit;
            }
        } else {
            count = self.max_particles as i32;
            if count == 0 {
                count = (self.rates[0] * 30.0) as i32;
            }
        }
        if count < 1 {
            count = 1;
        }
        let rect = self.rect != [0.0; 3];
        let mut dir_mode = (flags >> 7) & 1;
        if self.cone != 0.0 {
            dir_mode |= if self.cone == -1.0 { 2 } else { 4 };
        }
        let mut pos_mode = (flags >> 6) & 1;
        if rect {
            pos_mode |= 2;
        }
        if flags & 1 != 0 {
            dir_mode = 0;
            if self.cone != 0.0 {
                dir_mode = if self.cone == -1.0 { 3 } else { 5 };
            }
            pos_mode = if rect { 3 } else { 0 };
        }
        let mut pos_limit = if self.pos_count != 0 { self.pos_count as i32 } else { 100 };
        if count < pos_limit {
            pos_limit = count;
        }
        let shared_size = |x: f32| -> i32 {
            let mut n = count;
            if 10.0 <= life {
                n = (avg * 10.0) as i32;
            }
            if n < 8 {
                n = 8;
            }
            let _ = x;
            n
        };
        let mut pos_share = false;
        let mut npos = count;
        match pos_mode {
            0 => {
                self.pos_fn = PosFn::Single1;
                npos = 1;
            }
            1 => {
                npos = life as i32;
                self.pos_fn = PosFn::Frame;
            }
            2 => {
                let n = shared_size(life);
                npos = if n < 1 {
                    1
                } else if n <= count {
                    n
                } else {
                    count
                };
                if self.pos_count != 0 {
                    npos = self.pos_count as i32;
                }
                self.pos_fn = PosFn::RectShare;
                pos_share = true;
            }
            _ => self.pos_fn = PosFn::RectUnique,
        }
        self.flags |= 4;
        if pos_limit < npos {
            npos = pos_limit;
        }
        let mut own_dirs = true;
        let mut dir_limit = if self.dir_count != 0 { self.dir_count as i32 } else { 0x1f };
        if count < dir_limit {
            dir_limit = count;
        }
        let mut dir_share = false;
        let ndirs: i32;
        match dir_mode {
            0 => {
                self.dir_fn = DirFn::Single1;
                self.flags |= 8;
                ndirs = 1;
            }
            1 => {
                self.dir_fn = DirFn::Frame;
                self.flags |= 8;
                ndirs = life as i32;
            }
            2 | 3 => {
                self.dir_fn = DirFn::Sphere;
                self.global_dirs = true;
                own_dirs = false;
                dir_limit = global.dirs.len() as i32;
                ndirs = global.dirs.len() as i32;
            }
            4 => {
                let n = shared_size(life);
                let mut v = if n < 1 {
                    1
                } else if n <= count {
                    n
                } else {
                    count
                };
                if self.dir_count != 0 {
                    v = self.dir_count as i32;
                }
                ndirs = v;
                self.dir_fn = DirFn::ConeShare;
                self.flags |= 8;
                dir_share = true;
            }
            _ => {
                self.dir_fn = DirFn::ConeUnique;
                self.flags |= 8;
                ndirs = count;
            }
        }
        let mut ndirs = if dir_limit < ndirs { dir_limit } else { ndirs };
        ndirs = ndirs.clamp(1, 0x1f);
        if self.pos_count == 0 && 100 < npos {
            npos = 100;
        }
        let pos_cap = if self.pos_count != 0 { self.pos_count as i32 } else { 100 };
        if pos_cap <= npos {
            npos = pos_cap;
        }
        let mut sel = u8::from(self.global_dirs);
        if self.gravity != 0.0 {
            sel |= 4;
        }
        self.ppos_fn = match sel {
            1 => PPosFn::Speed,
            4 => PPosFn::Grav,
            5 => PPosFn::SpeedGrav,
            _ => PPosFn::Linear,
        };
        self.max_particles = count as u16;
        self.pos_count = npos as u16;
        self.dir_count = ndirs as u16;
        self.ring = vec![0; ((count + 1) & !1) as usize + 1];
        if own_dirs {
            self.dirs = vec![[0.0; 3]; ndirs as usize];
        }
        self.poss = vec![[0.0; 3]; npos.max(1) as usize];
        self.dir_share = dir_share.then(|| vec![0; ndirs as usize]);
        self.pos_share = pos_share.then(|| vec![0; npos as usize]);
        let bits = |n: i32| -> u8 {
            let mut b = 0u32;
            while (1i32 << b) < n + 1 {
                b += 1;
            }
            b as u8
        };
        self.dir_bits = bits(ndirs);
        self.pos_bits = bits(npos);
        let age_bits = 16 - self.pos_bits as i32 - self.dir_bits as i32;
        if age_bits < 1 {
            return false;
        }
        self.age_bits = age_bits as u8;
        self.dir_cursor = (ndirs - 1) as u16;
        self.pos_cursor = (npos - 1) as u16;
        self.dir_end = 0xffff;
        self.pos_end = 0xffff;
        true
    }

    /// setupPsys 0x1cc268.
    fn setup(&mut self, global: &PsysGlobal) -> bool {
        self.setup_parms();
        if !self.setup_new_pmode(global) {
            return false;
        }
        self.head = None;
        self.oldest_age = 0;
        self.newest_age = 0;
        self.tail = self.max_particles as usize - 1;
        self.accum = 0.0;
        true
    }

    fn current_dir(&self, m: &[f32; 16]) -> [f32; 3] {
        // getCurrentDir: dir through the matrix rotation, renormalised when
        // |d|² is outside [0.7, 1.3], × speed.
        let [x, y, z] = self.dir;
        let d = [
            x * m[0] + y * m[4] + z * m[8],
            x * m[1] + y * m[5] + z * m[9],
            x * m[2] + y * m[6] + z * m[10],
        ];
        let l = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        let s = if !(0.7..=1.3).contains(&l) { self.speed / l.sqrt() } else { self.speed };
        d.map(|v| v * s)
    }

    fn unique(count: u16, cursor: &mut u16, end: u16) -> i32 {
        // getUniqueIdx.
        if *cursor == end {
            return -1;
        }
        let v = *cursor;
        *cursor = if v == 0 { count } else { v } - 1;
        v as i32
    }

    fn random_index(global: &mut PsysGlobal, count: u16, share: &mut [u8]) -> i32 {
        // getRandomIdx.
        let count = count as i32;
        let mut start = (global.rand15() * count as f32 * 3.051851e-05) as i32;
        if count <= start {
            start = count - 1;
        }
        let mut i = start;
        loop {
            let c = share[i as usize];
            if c != 0xff {
                share[i as usize] = c.wrapping_add(1);
                return i;
            }
            if i == 0 {
                i = count;
            }
            i -= 1;
            if i == start {
                return 0;
            }
        }
    }

    fn pos_rect(&self, global: &mut PsysGlobal, m: &[f32; 16]) -> [f32; 3] {
        let a = (global.rand15() * 3.051851e-05 - 0.5) * self.rect[0];
        let b = (global.rand15() * 3.051851e-05 - 0.5) * self.rect[1];
        let c = (global.rand15() * 3.051851e-05 - 0.5) * self.rect[2];
        [
            a * m[0] + b * m[4] + c * m[8] + m[12],
            a * m[1] + b * m[5] + c * m[9] + m[13],
            a * m[2] + b * m[6] + c * m[10] + m[14],
        ]
    }

    fn new_pos(&mut self, global: &mut PsysGlobal, m: &[f32; 16]) -> i32 {
        match self.pos_fn {
            PosFn::Single1 => {
                self.pos_fn = PosFn::Single2;
                self.flags &= 0xfff7;
                self.poss[0] = [m[12], m[13], m[14]];
                0
            }
            PosFn::Single2 => 0,
            PosFn::Frame => {
                if global.frame_pos >= 0 {
                    return global.frame_pos;
                }
                let i = Self::unique(self.pos_count, &mut self.pos_cursor, self.pos_end);
                if i >= 0 {
                    self.poss[i as usize] = [m[12], m[13], m[14]];
                    global.frame_pos = i;
                }
                i
            }
            PosFn::RectShare => {
                let mut share = self.pos_share.take().unwrap_or_default();
                let i = Self::random_index(global, self.pos_count, &mut share);
                if i >= 0 && share[i as usize] < 2 {
                    self.poss[i as usize] = self.pos_rect(global, m);
                }
                self.pos_share = Some(share);
                i
            }
            PosFn::RectUnique => {
                let i = Self::unique(self.pos_count, &mut self.pos_cursor, self.pos_end);
                if i >= 0 {
                    self.poss[i as usize] = self.pos_rect(global, m);
                }
                i
            }
        }
    }

    fn cone_dir(&self, global: &mut PsysGlobal, m: &[f32; 16]) -> [f32; 3] {
        // getCurrentDir, getOrthoVecs, getConeDir.
        let c = self.current_dir(m);
        let (a, b) = ortho_vecs(c);
        let (s1, c1) = sin_cos(global.rand15() * self.cone * 3.051851e-05);
        let (s2, c2) = sin_cos(global.rand15() * 9.587673e-05);
        let s2 = if global.rand.rand() & 4 != 0 { -s2 } else { s2 };
        let u = c2 * s1;
        let v = s2 * s1;
        std::array::from_fn(|k| c1 * c[k] + u * a[k] + v * b[k])
    }

    fn new_dir(&mut self, global: &mut PsysGlobal, m: &[f32; 16]) -> i32 {
        match self.dir_fn {
            DirFn::Single1 => {
                self.dir_fn = DirFn::Single2;
                self.flags &= 0xfff7;
                self.dirs[0] = self.current_dir(m);
                0
            }
            DirFn::Single2 => 0,
            DirFn::Frame => {
                if global.frame_dir >= 0 {
                    return global.frame_dir;
                }
                let i = Self::unique(self.dir_count, &mut self.dir_cursor, self.dir_end);
                if i >= 0 {
                    self.dirs[i as usize] = self.current_dir(m);
                    global.frame_dir = i;
                }
                i
            }
            DirFn::Sphere => {
                let n = global.dirs.len() as u16;
                let mut share = std::mem::take(&mut global.dir_share);
                let i = Self::random_index(global, n, &mut share);
                if i >= 0 && share[i as usize] < 2 {
                    let x = global.rand15() * 6.103702e-06 - 0.1;
                    let y = global.rand15() * 6.103702e-06 - 0.1;
                    let z = global.rand15() * 6.103702e-06 - 0.1;
                    let l = (x * x + z * z + y * y).sqrt();
                    global.dirs[i as usize] = if 0.0 < l { [x / l, y / l, z / l] } else { [x, y, z] };
                }
                global.dir_share = share;
                i
            }
            DirFn::ConeShare => {
                let mut share = self.dir_share.take().unwrap_or_default();
                let i = Self::random_index(global, self.dir_count, &mut share);
                if i >= 0 && share[i as usize] < 2 {
                    self.dirs[i as usize] = self.cone_dir(global, m);
                }
                self.dir_share = Some(share);
                i
            }
            DirFn::ConeUnique => {
                let i = Self::unique(self.dir_count, &mut self.dir_cursor, self.dir_end);
                if i >= 0 {
                    self.dirs[i as usize] = self.cone_dir(global, m);
                }
                i
            }
        }
    }

    fn dir_entry(&self, global: &PsysGlobal, i: usize) -> [f32; 3] {
        if self.global_dirs { global.dirs.get(i).copied().unwrap_or_default() } else { self.dirs.get(i).copied().unwrap_or_default() }
    }

    fn particle_pos(&self, age: f32, d: [f32; 3], p: [f32; 3]) -> [f32; 3] {
        match self.ppos_fn {
            PPosFn::Linear => std::array::from_fn(|k| age * d[k] + p[k]),
            PPosFn::Grav => [age * d[0] + p[0], (age * self.gravity + d[1]) * age + p[1], age * d[2] + p[2]],
            PPosFn::Speed => std::array::from_fn(|k| self.speed * d[k] * age + p[k]),
            PPosFn::SpeedGrav => [
                self.speed * d[0] * age + p[0],
                (age * self.gravity + self.speed * d[1]) * age + p[1],
                self.speed * d[2] * age + p[2],
            ],
        }
    }

    /// MBDrawPsys 0x1cd8a8 for one traversal: retire, emit and return the
    /// sprites (newest first). `m` is the psys node's world matrix (row-major
    /// PS2 layout), `scale` the node's +0x44 scale, `visible` MBDrawPsysTest's
    /// frustum result; the caller skips invisible psys with no sprite drawn
    /// last time (pbRenderNode case 0xc).
    pub fn draw(&mut self, global: &mut PsysGlobal, m: &[f32; 16], scale: f32, visible: bool, out: &mut Vec<Sprite>) {
        global.frame_dir = -1;
        global.frame_pos = -1;
        self.visible = visible || self.state >= 6;
        let mut dt = global.frame.wrapping_sub(self.last_frame);
        if 0xf < dt {
            dt = 1;
        }
        let mut t = dt + self.state_frames;
        self.last_frame = global.frame;
        let mut rate: f32;
        // The emission state machine (+0x37).
        'state: loop {
            match self.state {
                0 => {
                    if self.start_delay != 0 {
                        if t <= self.start_delay as u32 {
                            self.state_frames = t;
                            return;
                        }
                        t -= self.start_delay as u32;
                        dt = t;
                    }
                    self.state = 1;
                }
                1 => {
                    if !self.setup(global) || self.ring.is_empty() {
                        self.state = 8;
                        return;
                    }
                    if dt == 0 {
                        dt = 1;
                        t += 1;
                    }
                    self.state = 2;
                    if self.flags & 1 != 0 {
                        rate = 100000.0;
                        dt = 1;
                        t = 1;
                        self.state = 6;
                        break 'state;
                    }
                    if self.flags & 2 != 0 && self.emit_frames == 0xffff {
                        self.state = 4;
                        continue;
                    }
                }
                2 => {
                    if t <= self.emit_frames as u32 {
                        rate = t as f32 * self.rates[1] + self.rates[0];
                        break 'state;
                    }
                    if self.flags & 2 != 0 && self.decay_frames == 0xffff {
                        self.state = 5;
                        continue;
                    }
                    self.state = 3;
                }
                3 => {
                    if t <= self.emit_frames as u32 + self.decay_frames as u32 {
                        rate = t as f32 * self.rates[3] + self.rates[2];
                        break 'state;
                    }
                    if self.flags & 2 != 0 {
                        // Looping: back to emitting (state 2) at t = 0 with
                        // this frame's rate the start rate.
                        t = 0;
                        self.state = 2;
                        rate = self.rates[0];
                        break 'state;
                    }
                    self.state = 6;
                }
                4 => {
                    rate = self.rates[0];
                    break 'state;
                }
                5 => {
                    rate = self.rates[2];
                    break 'state;
                }
                6 => {
                    rate = 0.0;
                    self.accum = 0.0;
                    if self.head.is_none() {
                        self.state = 8;
                        return;
                    }
                    break 'state;
                }
                _ => return,
            }
        }
        if !self.visible || self.node_flags & 0x200000 != 0 {
            rate = 0.0;
        }
        self.state_frames = t;
        if 0.0 < rate && 0.0 < self.rate_random {
            rate *= (global.rand15() * 6.103702e-05 - 1.0) * self.rate_random + 1.0;
        }
        let n = self.max_particles as usize;
        let age_mask = (1u32 << self.age_bits) - 1;
        let dir_mask = (1u32 << self.dir_bits) - 1;
        let e8 = self.age_bits as u32 + self.dir_bits as u32;
        let ec = self.age_bits as u32;
        let life = self.fade_in as i32 + self.fade_out as i32;
        // Retire particles older than fade-in + fade-out.
        let mut left = dt as i32 + self.oldest_age as i32 - life;
        if 0 <= left
            && let Some(mut h) = self.head
        {
            let mut seg_end = if h <= self.tail { self.tail } else { n - 1 };
            let mut entry = self.ring[h] as u32;
            let mut alive = None;
            loop {
                if let Some(s) = self.pos_share.as_mut() {
                    let i = (entry >> e8) as usize;
                    if let Some(c) = s.get_mut(i) {
                        *c = c.wrapping_sub(1);
                    }
                }
                if self.global_dirs {
                    let i = ((entry >> ec) & dir_mask) as usize;
                    if let Some(c) = global.dir_share.get_mut(i) {
                        *c = c.wrapping_sub(1);
                    }
                } else if let Some(s) = self.dir_share.as_mut() {
                    let i = ((entry >> ec) & dir_mask) as usize;
                    if let Some(c) = s.get_mut(i) {
                        *c = c.wrapping_sub(1);
                    }
                }
                let was_end = h == seg_end;
                h += 1;
                if was_end {
                    let at_tail = seg_end == self.tail;
                    seg_end = self.tail;
                    h = 0;
                    if at_tail {
                        break;
                    }
                }
                entry = self.ring[h] as u32;
                left -= (entry & age_mask) as i32;
                if left < 0 {
                    alive = Some(h);
                    break;
                }
            }
            self.head = alive;
            match alive {
                None => {
                    self.tail = n - 1;
                    self.pos_end = 0xffff;
                    self.dir_end = 0xffff;
                }
                Some(_) => {
                    self.pos_end = (entry >> e8) as u16;
                    self.dir_end = ((entry >> ec) & dir_mask) as u16;
                }
            }
        }
        self.oldest_age = (life + left) as u16;
        // Emit.
        let first = if self.tail == n - 1 { 0 } else { self.tail + 1 };
        let total = rate * dt as f32 + self.accum;
        if self.head == Some(first) {
            self.accum = 0.0;
            self.newest_age = self.newest_age.wrapping_add(dt as u16);
        } else {
            let mut seg_lim = match self.head {
                Some(h) if self.tail < h => h,
                _ => n,
            };
            let mut remaining = total;
            let mut p = first;
            let mut one_past;
            'emit: loop {
                loop {
                    one_past = p;
                    if remaining <= 0.0 {
                        break 'emit;
                    }
                    let pos = self.new_pos(global, m);
                    if pos < 0 {
                        break 'emit;
                    }
                    let dir = self.new_dir(global, m);
                    if dir < 0 {
                        // The original decrements pos_share[dir] (index −1,
                        // outside the table); that byte is not modelled.
                        break 'emit;
                    }
                    self.ring[p] = ((pos as u32) << e8 | (dir as u32) << ec) as u16;
                    p += 1;
                    one_past = p;
                    remaining -= 1.0;
                    if p == seg_lim {
                        break;
                    }
                }
                if seg_lim != n {
                    break;
                }
                if self.head.is_none() {
                    self.head = Some(first);
                }
                seg_lim = self.head.unwrap();
                p = 0;
                if seg_lim == 0 {
                    break;
                }
            }
            self.accum = if 0.0 < remaining { 0.0 } else { remaining };
            if remaining == total {
                self.newest_age = self.newest_age.wrapping_add(dt as u16);
            } else {
                if self.head.is_none() {
                    self.head = Some(first);
                }
                self.tail = if one_past == 0 { n } else { one_past } - 1;
                // Age gaps of the new particles, newest first.
                let mut spent = 0u32;
                let mut q = one_past as isize - 1;
                let mut lower: isize = if first < one_past { first as isize } else { -1 };
                let mut f = rate - 1.0;
                loop {
                    while q != lower {
                        if f <= 0.0 {
                            let mut gap = 0u32;
                            loop {
                                f += rate;
                                gap += 1;
                                spent += 1;
                                if 0.0 < f || gap == age_mask {
                                    break;
                                }
                            }
                            self.ring[q as usize] |= gap as u16;
                        }
                        q -= 1;
                        f -= 1.0;
                    }
                    if lower == first as isize {
                        break;
                    }
                    q = n as isize - 1;
                    lower = first as isize;
                }
                if self.head == Some(first) {
                    self.oldest_age = spent as u16;
                    self.newest_age = 0;
                } else {
                    let mut gap = self.newest_age as i32 + dt as i32 - spent as i32;
                    self.newest_age = 0;
                    let mut q = first;
                    let mut lim = if q < one_past { one_past } else { n };
                    let mut e = self.ring[q] as u32;
                    if (age_mask as i32) < gap {
                        'spread: loop {
                            self.ring[q] = (e | age_mask) as u16;
                            q += 1;
                            if q == lim {
                                if lim == one_past {
                                    // Original: the slot after the newest
                                    // gives newest_age and keeps its bits.
                                    let v = self.ring.get(lim).copied().unwrap_or(0) as u32 & age_mask;
                                    self.newest_age = v as u16;
                                    e = 0;
                                    gap = v as i32;
                                    break 'spread;
                                }
                                lim = one_past;
                                q = 0;
                            }
                            e = self.ring[q] as u32;
                            gap += (e & age_mask) as i32 - age_mask as i32;
                            if gap <= age_mask as i32 {
                                e &= !age_mask;
                                break;
                            }
                        }
                    } else {
                        e &= !age_mask;
                    }
                    if let Some(slot) = self.ring.get_mut(q) {
                        *slot = (e | gap as u32) as u16;
                    }
                }
            }
        }
        // Draw newest → oldest.
        self.drawn = 0;
        let Some(head) = self.head else { return };
        let fade_in = self.fade_in as f32;
        let mut age = self.newest_age as f32;
        let mut phase2 = age >= fade_in;
        let mut t_idx = self.tail;
        let mut seg_stop: isize = if head <= self.tail { head as isize - 1 } else { -1 };
        let mut sprites = 0;
        loop {
            if !phase2 && fade_in <= age {
                phase2 = true;
            }
            let (base, slope) = if phase2 { (2, 3) } else { (0, 1) };
            let v = |slot: usize| self.params[slot][base] + age * self.params[slot][slope];
            let c = |slot: usize| {
                let x = if self.params[slot][slope] == 0.0 { self.params[slot][base] } else { v(slot) };
                x as i32 as u32 & 0xff
            };
            let entry = self.ring[t_idx] as u32;
            let d = self.dir_entry(global, ((entry >> ec) & dir_mask) as usize);
            let p = self.poss.get((entry >> e8) as usize).copied().unwrap_or_default();
            let pos = self.particle_pos(age, d, p);
            let packed = c(0) | c(1) << 8 | c(2) << 16 | c(3) << 24;
            let size = v(4) / PARMSCALE[4][0] * scale;
            self.drawn = self.drawn.wrapping_add(1);
            if global.rendered < 0x801 {
                global.rendered += 1;
                sprites += 1;
                out.push(Sprite {
                    position: pos,
                    size,
                    rgba: [(packed >> 16) as u8, (packed >> 8) as u8, packed as u8, (packed >> 25) as u8],
                });
            }
            let prev = t_idx as isize;
            let mut next = prev - 1;
            if next == seg_stop {
                if next == head as isize - 1 {
                    break;
                }
                next = n as isize - 1;
                seg_stop = head as isize - 1;
            }
            t_idx = next as usize;
            age += (entry & age_mask) as f32;
        }
        let _ = sprites;
    }
}

/// calcLineAvg 0x1cb718.
fn calc_line_avg(mut base: f32, mut slope: f32, len: f32, t: f32, mode: i32) -> f32 {
    if mode == 1 {
        base += slope * len;
        slope = -slope * t;
    } else if mode < 2 {
        slope *= t;
    } else {
        if mode == 2 {
            return base + slope * len * 0.5;
        }
        slope *= t;
    }
    base + slope * 0.5
}

/// getSinCos 0x1cc390: (sin x, cos x) by the original series.
fn sin_cos(mut x: f32) -> (f32, f32) {
    let c = std::f32::consts::FRAC_PI_2 - x;
    if std::f32::consts::FRAC_PI_2 < x {
        x = std::f32::consts::PI - x;
    }
    let c2 = c * c;
    let x2 = x * x;
    let cos = c + c * c2 * (c2 * (c2 * -0.0001984127 + 0.008333334) + -0.16666667);
    let sin = x + x * x2 * (x2 * (x2 * -0.0001984127 + 0.008333334) + -0.16666667);
    (sin, cos)
}

/// getOrthoVecs 0x1cc548.
fn ortho_vecs(v: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let [x, y, z] = v;
    if x < 0.1 && y < 0.1 && z != 0.0 {
        return ([-z, 0.0, x], [y * x, -(z * z) - x * x, y * z]);
    }
    ([y, -x, 0.0], [z * x, z * y, -(x * x) - y * y])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(flags4: u32) -> Vec<u8> {
        let mut r = vec![0u8; RECORD];
        let mut put = |i: usize, v: u32| r[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        put(0, 0x101);
        put(1, 0x0041_0001);
        // Values without bit 8 (no default gravity); additive and sorted.
        put(2, 0x280);
        put(3, 0x288);
        put(4, flags4);
        put(10, 0.2f32.to_bits());
        put(11, 0.2f32.to_bits());
        for k in 0..4 {
            put(0x1e + k, 30.0f32.to_bits());
        }
        put(0x25, 3.0f32.to_bits());
        r
    }

    #[test]
    fn calc_line_avg_modes() {
        assert_eq!(calc_line_avg(1.0, 2.0, 3.0, 4.0, 2), 4.0);
        assert_eq!(calc_line_avg(1.0, 2.0, 3.0, 4.0, 0), 5.0);
        assert_eq!(calc_line_avg(1.0, 2.0, 3.0, 4.0, 1), 3.0);
    }

    #[test]
    fn world_psys_emits_and_retires() {
        // fade 0.2 s + 0.2 s (6 + 6 frames), 30 particles/s, rates bit 0x200,
        // speed bit 0x2000, fades bit 0x20, a 0° cone (bit 0x40).
        let r = record(0x2260);
        let mut g = PsysGlobal::new(1);
        let mut p = Psys::new_world(0, &r, &[], None, false).unwrap();
        assert_eq!(p.node_flags & 0x800800, 0x800800);
        assert_eq!(p.blend(), Blend::Additive);
        let m: [f32; 16] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 5.0, 0.0, 0.0, 1.0];
        let mut counts = Vec::new();
        for _ in 0..40 {
            g.start_frame();
            let mut out = Vec::new();
            p.draw(&mut g, &m, 1.0, true, &mut out);
            counts.push(out.len());
            // Particles rise from the node along +y at speed 0.1/frame.
            assert!(out.iter().all(|s| s.position[1] >= 0.0 && (s.position[0] - 5.0).abs() < 1e-4));
        }
        // One particle per frame with a 12-frame life → a steady ~12.
        assert!(counts[30..].iter().all(|&c| (11..=13).contains(&c)), "{counts:?}");
    }
}

