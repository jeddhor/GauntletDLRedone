//! Controller input: ControlsUpdate 0x104130, ReadControls 0x104b60,
//! PlayerControls 0x105718, CheckSpecials 0x104878, ClearPlayerControl
//! 0x105b28 and InitPlayerControls 0x105c48 (research/player.md).
//!
//! PlayerControl (0x2114f0, 0x3c per player): +0 lockout count, +4 levels,
//! +8 edges, +0xc repeat edges, +0x10 special cooldown, +0x14 special,
//! +0x18 last special, +0x1c/+0x20 left angle/magnitude, +0x24/+0x28 right
//! angle/magnitude, +0x2c mode (remap set; 2 = right stick drives, 3 =
//! right stick aims), +0x30, +0x34, +0x38 (InitPlayerControls: 0, 2, 1, 1).
//!
//! The native front end supplies each port's joyGetStatus entries
//! ([`PadStatus`]); joyGetStatus/libpad themselves (0x1063f8) are the
//! platform layer and are not ported. Which physical button fills which
//! entry is recorded in research/player.md (observed directions only).
use super::record::Record;
use super::{Game, R};
use crate::controls::JoyTables;
use crate::libm::atan2f;
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sqrt_rn};
use serde::{Deserialize, Serialize};

/// joyGetStatus's entry list for one port: (flags, value) pairs. Flag bit 0
/// = present, bit 1 = pressure/analog value. Entries 0..3 are the d-pad
/// (left, right, up, down), 4..7 the left stick, 0xe..0x11 the right stick;
/// 8..0xd and 0x12..0x15 are buttons (get_button).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PadStatus {
    pub connected: bool,
    pub analog: bool,
    pub has_stick: bool,
    pub entries: [[u8; 2]; 0x16],
}

impl PadStatus {
    /// A digital press of entry `i`.
    pub fn press(&mut self, i: usize) {
        self.entries[i] = [1, 1];
    }
}

/// get_dir 0x103c90.
fn get_dir(e: [u8; 2], digital: bool) -> u32 {
    if e[0] & 1 == 0 {
        return 0;
    }
    if e[0] & 2 == 0 {
        return if e[1] == 0 { 0 } else { 3 };
    }
    let v = e[1];
    if !digital {
        if v < 0x81 {
            return if 0x70 < v {
                2
            } else if v < 0x61 {
                0
            } else {
                1
            };
        }
    } else if v < 0x21 {
        return 0;
    }
    3
}

/// get_button 0x103d48.
fn get_button(e: [u8; 2]) -> u32 {
    if e[0] & 1 == 0 {
        return 0;
    }
    u32::from(if e[0] & 2 == 0 { e[1] != 0 } else { 5 < e[1] })
}

/// get_analog_dir 0x103d10.
fn get_analog_dir(e: [u8; 2]) -> i32 {
    if e[0] & 1 == 0 {
        return 0;
    }
    if e[0] & 2 == 0 {
        return if e[1] == 0 { 0 } else { 0xff };
    }
    i32::from(e[1])
}

/// calc_analog_stick 0x103fa8: (left, right, up, down) → (angle, magnitude).
fn calc_analog_stick(l: i32, r: i32, u: i32, d: i32) -> R<(f32, f32)> {
    let k = f32::from_bits(0x3b80_8081);
    let v = mul(if u < d { d } else { -u } as f32, k)?;
    let h = mul(if r < l { l } else { -r } as f32, k)?;
    let a = atan2f(h, v)?;
    let m = sqrt_rn(add(mul(v, v)?, mul(h, h)?)?)?;
    Ok((a, m))
}

/// dir_edge 0x103f80.
fn dir_edge(cur: u32, prev: u32, mask: u32, shift: u32) -> u32 {
    let v = (cur & mask) >> shift;
    if v != 0 && prev & mask == 0 {
        v << shift
    } else {
        0
    }
}

/// ELF tables: remap 0x2112a8 (10 entries × 4 sets × (mask, require)),
/// rep_speed 0x2107b8 (14 words), SpecialData 0x210880 (13 × 200 bytes).
#[derive(Debug, Clone)]
pub struct ControlTables {
    pub remap: [[[u32; 2]; 4]; 10],
    pub rep_speed: [i32; 14],
    pub special_data: Vec<u8>,
    pub joy: JoyTables,
}

impl ControlTables {
    pub fn from_elf(elf: &gdl_formats::elf::Elf) -> Result<Self, Box<dyn std::error::Error>> {
        let words = |name: &str| -> Result<Vec<u32>, Box<dyn std::error::Error>> {
            Ok(elf.object(name)?.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect())
        };
        let r = words("remap")?;
        let s = words("rep_speed")?;
        if r.len() != 80 || s.len() != 14 {
            return Err("remap/rep_speed sizes differ from the supported executable".into());
        }
        let special_data = elf.object("SpecialData")?.to_vec();
        if special_data.len() != 13 * 200 {
            return Err("SpecialData size differs from the supported executable".into());
        }
        Ok(Self {
            remap: std::array::from_fn(|i| std::array::from_fn(|set| [r[i * 8 + set * 2], r[i * 8 + set * 2 + 1]])),
            rep_speed: std::array::from_fn(|i| s[i] as i32),
            special_data,
            joy: crate::controls::tables_from_elf(elf)?,
        })
    }

    fn sd(&self, o: usize) -> i32 {
        i32::from_le_bytes(self.special_data[o..o + 4].try_into().unwrap())
    }
}

/// The controls module's globals (accum_*, plr_*, analog_*, joy_remap …).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Controls {
    /// PlayerControl[4].
    pub control: [Record; 4],
    pub accum_levels: [u32; 8],
    pub accum_right: [u32; 8],
    /// analog_left/right (angle, magnitude) per port.
    pub analog_left: [[f32; 2]; 8],
    pub analog_right: [[f32; 2]; 8],
    pub plr_levels: [u32; 8],
    pub right_jstick: [u32; 8],
    pub plr_edges: [u32; 8],
    pub plr_repedges: [u32; 8],
    pub plr_sw_cnt: [u32; 8],
    pub plr_sw_idx: [i32; 8],
    /// JoyLevels/JoyEdges/JoyRepEdges/JoyPrevLevels for ports 0 and 4, and
    /// the sw_cnt/sw_idx rows they use.
    /// JoyLevels has four entries; ReadControls fills 0 and 1 only.
    pub joy_levels: [u32; 4],
    pub joy_edges: [u32; 2],
    pub joy_rep_edges: [u32; 2],
    pub joy_prev_levels: [u32; 2],
    pub sw: [[i32; 2]; 2],
    /// joy_remap 0x2107f0 (player → port, −1 unassigned) and joy_assigned
    /// 0x210800 (port → player); both −1 in the ELF data.
    pub joy_remap: [i32; 4],
    pub joy_assigned: [i32; 8],
    /// controller_lost 0x2107ac.
    pub controller_lost: u32,
    pub soft_reset: [i32; 8],
    pub updated: bool,
    /// SpecialMoves (per player 13 × (step, count)).
    pub special_moves: [[[i32; 2]; 13]; 4],
    pub joy_any: [u32; 3],
}

impl Default for Controls {
    fn default() -> Self {
        let mut c = Self {
            control: std::array::from_fn(|_| Record::zeroed(0x3c)),
            accum_levels: [0; 8],
            accum_right: [0; 8],
            analog_left: [[0.0; 2]; 8],
            analog_right: [[0.0; 2]; 8],
            plr_levels: [0; 8],
            right_jstick: [0; 8],
            plr_edges: [0; 8],
            plr_repedges: [0; 8],
            plr_sw_cnt: [0; 8],
            plr_sw_idx: [0; 8],
            joy_levels: [0; 4],
            joy_edges: [0; 2],
            joy_rep_edges: [0; 2],
            joy_prev_levels: [0; 2],
            sw: [[0; 2]; 2],
            joy_remap: [-1; 4],
            joy_assigned: [-1; 8],
            controller_lost: 0,
            soft_reset: [0; 8],
            updated: false,
            special_moves: [[[0; 2]; 13]; 4],
            joy_any: [0; 3],
        };
        c.init_player_controls();
        c
    }
}

impl Controls {
    /// InitPlayerControls 0x105c48 (InitJoyAng's tables live in ControlTables).
    pub fn init_player_controls(&mut self) {
        for k in 0..4 {
            self.special_moves[k] = [[0; 2]; 13];
            self.clear_player_control(k, 0);
            let c = &mut self.control[k];
            c.set_i32(0x2c, 0);
            c.set_i32(0x30, 2);
            c.set_i32(0x34, 1);
            c.set_i32(0x38, 1);
        }
        self.sw = [[0; 2]; 2];
        self.joy_prev_levels = [0; 2];
    }

    /// ClearPlayerControl 0x105b28.
    pub fn clear_player_control(&mut self, k: usize, lockout: i32) {
        let c = &mut self.control[k];
        c.set_i32(0, lockout);
        for o in [4, 8, 0xc, 0x14, 0x18, 0x10, 0x1c, 0x20, 0x24, 0x28] {
            c.set_u32(o, 0);
        }
    }

    /// controls_remove_active_player 0x107268 (one player): its port's
    /// edges cleared and both mappings released (controls_set_active_players
    /// recomputes from joy_remap on the next update).
    pub fn remove_active_player(&mut self, k: usize) {
        if let Ok(port) = usize::try_from(self.joy_remap[k]) {
            self.plr_edges[port] = 0;
            self.plr_repedges[port] = 0;
            self.joy_assigned[port] = -1;
        }
        self.joy_remap[k] = -1;
    }

    /// Bind player `k` to `port` (what ControlsUpdate does when a port's
    /// button 0x14 claims a lost controller slot).
    pub fn assign(&mut self, k: usize, port: usize) {
        self.joy_remap[k] = port as i32;
        self.joy_assigned[port] = k as i32;
        self.controller_lost &= !(1 << k);
    }

    /// ControlsUpdate 0x104130 (one call per vblank batch; ctrls_initialized).
    pub fn controls_update(&mut self, pads: &[PadStatus; 8]) -> R<()> {
        for (port, pad) in pads.iter().enumerate() {
            if !pad.connected {
                continue;
            }
            let e = &pad.entries;
            let mut player = self.joy_assigned[port];
            let start = get_button(e[0x14]);
            let select = get_button(e[0x15]);
            if start == 0 || select == 0 {
                self.soft_reset[port] = 0;
            } else {
                self.soft_reset[port] += 1;
            }
            if player < 0 && self.controller_lost & 0xf != 0 {
                if start != 0 {
                    if let Some(k) = (0..4).find(|k| self.controller_lost >> k & 1 != 0) {
                        self.joy_remap[k] = port as i32;
                        self.joy_assigned[port] = k as i32;
                        self.controller_lost &= !(1 << k);
                        player = k as i32;
                    }
                    self.controller_lost &= 0xffff_f0ff;
                }
            } else if self.controller_lost & 0xf00 != 0 && start != 0 {
                self.controller_lost &= 0xffff_f0ff;
            }
            // The mode of the (possibly just claimed) player; 0 when unassigned.
            let mode = if player < 0 { 0 } else { self.control[player as usize].i32_(0x2c) };
            let dpad = [get_dir(e[0], true), get_dir(e[1], true), get_dir(e[2], true), get_dir(e[3], true)];
            let (left, right) = if pad.has_stick {
                ([get_dir(e[4], false), get_dir(e[5], false), get_dir(e[6], false), get_dir(e[7], false)], [get_dir(e[0xe], false), get_dir(e[0xf], false), get_dir(e[0x10], false), get_dir(e[0x11], false)])
            } else {
                ([0; 4], [0; 4])
            };
            // Mode 2 drives with the right stick unless all four of its
            // entries are centred.
            let mut dir = if mode == 2 && right != [0; 4] { right } else { left };
            for i in 0..4 {
                if dir[i] == 0 {
                    dir[i] = dpad[i];
                }
            }
            self.accum_right[port] |= right[1] << 2 | right[0] | right[3] << 6 | right[2] << 4;
            let b = |i: usize| get_button(e[i]);
            self.accum_levels[port] |= dir[1] << 2
                | dir[0]
                | dir[3] << 6
                | dir[2] << 4
                | start << 0x12
                | select << 0x13
                | b(9) << 0x15
                | b(8) << 0x14
                | b(0x13) << 0x17
                | b(0x12) << 0x16
                | b(0xc) << 0x1b
                | b(0xb) << 0x18
                | b(0xd) << 0x19
                | b(10) << 0x1a
                | u32::from(dpad[1] != 0) << 0x1d
                | u32::from(dpad[0] != 0) << 0x1c
                | u32::from(dpad[3] != 0) << 0x1f
                | u32::from(dpad[2] != 0) << 0x1e;
            if !pad.analog {
                self.analog_left[port] = [0.0; 2];
                self.analog_right[port] = [0.0; 2];
            } else {
                let a = |i: usize| get_analog_dir(e[i]);
                let (la, lm) = calc_analog_stick(a(4), a(5), a(6), a(7))?;
                let (ra, rm) = calc_analog_stick(a(0xe), a(0xf), a(0x10), a(0x11))?;
                self.analog_left[port] = [la, lm];
                self.analog_right[port] = [ra, rm];
                if mode == 2 && rm != 0.0 {
                    self.analog_left[port] = [ra, rm];
                }
            }
        }
        self.updated = true;
        Ok(())
    }

    /// ReadControls 0x104b60 (ctrls_initialized; vb = vb_elapsed_ctl).
    pub fn read_controls(&mut self, t: &ControlTables, vb: u32) {
        let prev = self.plr_levels;
        if !self.updated {
            return;
        }
        self.updated = false;
        self.plr_levels = self.accum_levels;
        self.right_jstick = self.accum_right;
        self.joy_any = [0; 3];
        for k in 0..4 {
            let port = if self.joy_remap[k] == -1 { k } else { self.joy_remap[k] as usize };
            let old = prev[port];
            let mut lv = self.plr_levels[port];
            if lv == 0 {
                self.plr_edges[port] = 0;
                self.plr_repedges[port] = 0;
                continue;
            }
            let set = self.control[k].i32_(0x2c) as usize & 3;
            for (i, row) in t.remap.iter().enumerate() {
                let [mask, need] = row[set];
                if mask & lv != 0 && (need == 0 || need & lv != 0) {
                    lv |= 1 << (i + 8);
                }
            }
            self.plr_levels[port] = lv;
            let (c, o) = (lv & 0xff, old & 0xff);
            let mut edges = dir_edge(c, o, 3, 0) | dir_edge(c, o, 0xc, 2) | dir_edge(c, o, 0x30, 4) | dir_edge(c, o, 0xc0, 6);
            for bit in 8..32 {
                let m = 1u32 << bit;
                if lv & m != 0 && old & m == 0 {
                    edges |= m;
                }
            }
            self.plr_edges[port] = edges;
            self.plr_repedges[port] = edges;
            if lv == 0 || lv != old {
                self.plr_sw_cnt[port] = 0;
                self.plr_sw_idx[port] = 0;
            } else {
                let cnt = self.plr_sw_cnt[port].wrapping_add(vb);
                self.plr_sw_cnt[port] = cnt;
                let idx = self.plr_sw_idx[port] as usize;
                let speed = t.rep_speed[idx] as u32;
                if speed <= cnt {
                    self.plr_sw_cnt[port] = cnt - speed;
                    self.plr_sw_idx[port] = idx as i32 + 1;
                    if t.rep_speed.get(idx + 1).is_none_or(|v| *v < 1) {
                        self.plr_sw_idx[port] = idx as i32;
                    }
                    self.plr_repedges[port] |= lv;
                }
            }
        }
        for j in 0..2 {
            let lv = self.plr_levels[j * 4];
            let old = self.joy_prev_levels[j];
            let edges = (old ^ lv) & lv;
            self.joy_levels[j] = lv;
            self.joy_edges[j] = edges;
            self.joy_rep_edges[j] = edges;
            if lv == 0 || lv != old {
                self.sw[j] = [0, 0];
            } else {
                let cnt = self.sw[j][0] + vb as i32;
                self.sw[j][0] = cnt;
                let idx = self.sw[j][1] as usize;
                let speed = t.rep_speed[idx];
                if speed <= cnt {
                    self.sw[j][0] = cnt - speed;
                    self.sw[j][1] = idx as i32 + 1;
                    if t.rep_speed.get(idx + 1).is_none_or(|v| *v < 1) {
                        self.sw[j][1] = idx as i32;
                    }
                    self.joy_rep_edges[j] |= lv;
                }
            }
            self.joy_any[0] |= self.joy_edges[j];
            self.joy_any[1] |= lv;
            self.joy_any[2] |= self.joy_rep_edges[j];
            self.joy_prev_levels[j] = lv;
        }
        self.accum_levels = [0; 8];
        self.accum_right = [0; 8];
    }

    /// CheckSpecials 0x104878 (`levels` = JoyLevels[k]).
    fn check_specials(&mut self, t: &ControlTables, k: usize, levels: u32) -> R<i32> {
        for m in 0..13 {
            let row = m * 200;
            let lv = if t.sd(row + 4) != 0 { levels & 0xff } else { levels };
            loop {
                let step = self.special_moves[k][m][0];
                if 0xf < step {
                    return super::unsupported(format!("CheckSpecials move {m} step {step}"));
                }
                let e = row + step as usize * 0xc;
                let pat = t.sd(e + 8) as u32;
                let masked = pat & 0xffff_7fff;
                let done = |s: &mut Self| {
                    let r = t.sd(row);
                    s.special_moves[k] = [[0; 2]; 13];
                    r
                };
                if masked == 0xfff {
                    let r = done(self);
                    if r != 0 {
                        return Ok(r);
                    }
                    break;
                }
                let hit = lv == masked || ((pat as i32) >> 0xf & 1 != 0 && lv & masked != 0) || ((pat as i32) >> 0xe & 1 != 0 && lv & masked == 0);
                if hit {
                    if t.sd(e + 0x10) == -1 {
                        let r = done(self);
                        if r != 0 {
                            return Ok(r);
                        }
                        break;
                    }
                    let n = self.special_moves[k][m][1] + 1;
                    self.special_moves[k][m][1] = n;
                    let limit = match t.sd(e + 0x10) {
                        0 => 6,
                        v => v,
                    };
                    if limit < n {
                        self.special_moves[k][m] = [0, 0];
                    }
                    break;
                }
                if self.special_moves[k][m][1] < t.sd(e + 0xc) {
                    self.special_moves[k][m] = [0, 0];
                    break;
                }
                self.special_moves[k][m][0] += 1;
                self.special_moves[k][m][1] = 0;
            }
        }
        Ok(0)
    }

    /// PlayerControls 0x105718 after ControlsUpdate (serve_vibes and
    /// serve_memcard are platform work).
    pub fn player_controls(&mut self, t: &ControlTables, disabled: bool, vb: u32) -> R<()> {
        if disabled {
            for k in 0..4 {
                self.clear_player_control(k, 0);
            }
            return Ok(());
        }
        self.read_controls(t, vb);
        for k in 0..4 {
            let port = self.joy_remap[k];
            let lock = self.control[k].i32_(0);
            if port == -1 || lock != 0 {
                if lock != 0 {
                    self.control[k].set_i32(0, lock - 1);
                }
                for o in [4, 8, 0xc, 0x1c, 0x20, 0x24, 0x28] {
                    self.control[k].set_u32(o, 0);
                }
            } else {
                let port = port as usize;
                let lv = self.plr_levels[port];
                let c = &mut self.control[k];
                c.set_u32(4, lv);
                c.set_u32(8, self.plr_edges[port]);
                c.set_u32(0xc, self.plr_repedges[port]);
                let [la, lm] = self.analog_left[port];
                if 0.33 < lm {
                    c.set_f32(0x1c, la);
                    c.set_f32(0x20, lm);
                } else {
                    let (a, m) = t.joy.digital(lv);
                    c.set_f32(0x1c, a);
                    c.set_f32(0x20, m);
                }
                if c.i32_(0x2c) == 3 {
                    let [ra, rm] = self.analog_right[port];
                    if 0.5 < rm {
                        c.set_f32(0x24, ra);
                        c.set_f32(0x28, rm);
                    } else {
                        let rj = self.right_jstick[port];
                        if rj == 0 {
                            c.set_f32(0x28, 0.0);
                        } else {
                            let (a, m) = t.joy.digital(rj);
                            c.set_f32(0x24, a);
                            c.set_f32(0x28, m);
                        }
                    }
                }
            }
            let cool = self.control[k].i32_(0x10);
            if cool < 1 {
                let jl = self.joy_levels[k];
                let s = self.check_specials(t, k, jl)?;
                self.control[k].set_i32(0x14, s);
            } else {
                self.control[k].set_i32(0x14, 0);
                self.control[k].set_i32(0x10, cool - 1);
            }
            let s = self.control[k].i32_(0x14);
            if s != 0 {
                self.control[k].set_i32(0x10, 10);
                self.control[k].set_i32(0x18, s);
            }
        }
        Ok(())
    }
}

impl Game {
    /// PlayerControls for this update from the ports' statuses.
    pub fn update_controls(&mut self, pads: &[PadStatus; 8]) -> R<()> {
        let Some(t) = self.control_tables.clone() else {
            return super::unsupported("control tables not loaded");
        };
        self.controls.controls_update(pads)?;
        let vb = self.globals.vb_elapsed as u32;
        let disabled = self.disable_player_controls != 0;
        self.controls.player_controls(&t, disabled, vb)?;
        // PlayerControls: serve_vibes after ReadControls (controls enabled).
        if !disabled {
            self.serve_vibes();
        }
        Ok(())
    }

    /// do_vibe 0x1069a8(player, strength, frames) from damage_player: with
    /// the player's vibration option (PlayerControl +0x30) on,
    /// joyBeginShake(vibe_inten[strength − 2 + option], clamped to 4) on
    /// its port and vibes[player] = (1, frames).
    pub(crate) fn do_vibe(&mut self, p: usize, strength: i32, frames: i32) {
        // vibe_inten 0x211468: 0.2, 0.4, 0.6, 0.8, 1.0.
        const VIBE_INTEN: [f32; 5] = [f32::from_bits(0x3e4c_cccd), f32::from_bits(0x3ecc_cccd), f32::from_bits(0x3f19_999a), f32::from_bits(0x3f4c_cccd), 1.0];
        let option = self.controls.control[p].i32_(0x30);
        if option == 0 {
            return;
        }
        let k = strength - 2 + option;
        if k < 0 {
            return;
        }
        self.rumble.push((self.controls.joy_remap[p], Some(VIBE_INTEN[k.min(4) as usize])));
        self.vibes[p] = (true, frames);
        self.vibe_fresh[p] = true;
    }

    /// serve_vibes 0x106a90: one frame off each running vibe; vibe_off once
    /// it runs out.
    fn serve_vibes(&mut self) {
        for p in 0..4 {
            if std::mem::take(&mut self.vibe_fresh[p]) {
                continue;
            }
            if self.vibes[p].0 {
                self.vibes[p].1 -= 1;
                if self.vibes[p].1 < 0 {
                    self.vibe_off(p);
                }
            }
        }
    }

    /// vibe_off 0x106a58: joyEndShake on the player's port.
    fn vibe_off(&mut self, p: usize) {
        self.vibes[p] = (false, 0);
        self.rumble.push((self.controls.joy_remap[p], None));
    }

    /// vibrators_off 0x106b10 (ResetModels, TriggerCameraActivate).
    pub(crate) fn vibrators_off(&mut self) {
        for p in 0..4 {
            if self.vibes[p].0 {
                self.vibe_off(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn digital_direction_entries_set_the_observed_level_bits() {
        let mut c = Controls::default();
        let mut pads = [PadStatus::default(); 8];
        pads[0].connected = true;
        pads[0].press(0);
        c.controls_update(&pads).unwrap();
        assert_eq!(c.accum_levels[0], 0x1000_0003);
        pads[0].entries[0] = [0, 0];
        pads[0].press(2);
        c.accum_levels = [0; 8];
        c.controls_update(&pads).unwrap();
        assert_eq!(c.accum_levels[0], 0x4000_0030);
    }
    #[test]
    fn edges_need_a_released_previous_frame() {
        assert_eq!(dir_edge(3, 0, 3, 0), 3);
        assert_eq!(dir_edge(3, 1, 3, 0), 0);
        assert_eq!(dir_edge(0xc, 0, 0xc, 2), 0xc);
    }
}
