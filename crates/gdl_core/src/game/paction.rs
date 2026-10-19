//! DoPlayerAction 0x177c28 on the player record (research/player.md).
//!
//! The player's animation instance is embedded at +0x90; its AnimInfo at
//! +0x94 holds the clock (sequence +0xa2, frame +0xac, loop word +0xc8, mode
//! bits +0xca, …) and the playback scale +0xbc. The action map at +0x1e8
//! gives each action's sequence (8 bytes per action). Current/requested
//! actions are +0x1e0/+0x1e4. The animation event word +0x8d8 and the
//! status/flag words it writes are consumed by PlayerMotion.
//!
//! Both jump tables were read from the ELF (first switch 0x2aad40, 0x96
//! entries; second 0x2aafa0, 0x66 entries) and the decompiler's case lists
//! checked against them. AnimFixPos (DoAnimateTree return bit 8) moves the
//! presentation root only; DoSeqTexMods/AnimateNode are presentation.
use super::record::Record;
use super::{unsupported, Event, Game, R};
use crate::animation_clock::{select, ClipTiming, ClockState, SelectionInput};
use crate::numeric::{add_guarded_rz, mul_rz};
use crate::player_animation::attack_type;

/// AnimInfo (+0x94) clock fields.
pub fn clock_of(r: &Record) -> ClockState {
    ClockState {
        sequence: r.u16_(0xa2),
        previous_sequence: r.u16_(0xb0),
        reset_byte: r.u8_(0xa6),
        option_bits: r.u8_(0xa7),
        enabled_word: r.u16_(0xb2),
        loop_word: r.u16_(0xc8),
        mode_bits: r.u16_(0xca),
        frame: r.f32_(0xac),
        blend_fraction: r.f32_(0xa8),
        start_time: r.f32_(0xb4),
        blend_end_time: r.f32_(0xb8),
        period_scale: r.f32_(0xc0),
        last_time: r.f32_(0xc4),
    }
}

pub fn set_clock(r: &mut Record, c: &ClockState) {
    r.set_u16(0xa2, c.sequence);
    r.set_u16(0xb0, c.previous_sequence);
    r.set_u8(0xa6, c.reset_byte);
    r.set_u8(0xa7, c.option_bits);
    r.set_u16(0xb2, c.enabled_word);
    r.set_u16(0xc8, c.loop_word);
    r.set_u16(0xca, c.mode_bits);
    r.set_f32(0xac, c.frame);
    r.set_f32(0xa8, c.blend_fraction);
    r.set_f32(0xb4, c.start_time);
    r.set_f32(0xb8, c.blend_end_time);
    r.set_f32(0xc0, c.period_scale);
    r.set_f32(0xc4, c.last_time);
}

/// The first-stage result: uVar13 (selected), uVar15 (mode), uVar4 (loop
/// word), iStack_bc (start frame), bVar3 (run the second jump table) and the
/// side effects the cases perform.
struct Choice {
    selected: u32,
    mode: i32,
    loop_word: u16,
    start: i32,
    table: bool,
    /// param_1[0x1f7] (+0x7dc) = 1 (cases 0x21, 0x25, 0x3c).
    set_7dc: bool,
    /// MBTreeSetFlags (true) / MBTreeClearFlags (false) (+0x6b8 tree, 2).
    tree_flag: Option<bool>,
}

/// 3π/4 and π/3 (0x17882c / 0x178864 literals).
const DEG135: f32 = f32::from_bits(0x4016_cbe5);
const DEG60: f32 = f32::from_bits(0x3f86_0a92);
/// The idle blend (0x1787ec literal).
const IDLE_BLEND: f32 = f32::from_bits(0x3d88_88b5);

impl Game {
    /// The player's sequence timing (the class ANIM tree's clips).
    fn player_clip(&self, p: usize, seq: u16) -> R<ClipTiming> {
        match self.player_clips.get(p).and_then(|c| c.get(seq as usize)) {
            Some(c) => Ok(*c),
            None => unsupported(format!("player {p} sequence {seq} has no clip timing")),
        }
    }

    /// The +0x6b8 tree exists and the class (+8) is neither x&3 == 2 nor 3.
    fn tree_6b8_usable(&self, p: usize) -> bool {
        let r = &self.players[p];
        r.u32_(0x6b8) != 0 && r.u32_(8) & 3 != 2 && r.u32_(8) != 3
    }

    /// DoPlayerAction 0x177c28.
    pub fn do_player_action(&mut self, p: usize) -> R<()> {
        let r = &self.players[p];
        let cur = r.u32_(0x1e0);
        let req = r.u32_(0x1e4);
        let o = r.u8_(0xa7) | 2;
        self.players[p].set_u8(0xa7, o);
        let rt = attack_type(req);
        let cur_type = attack_type(cur);
        let mut a7 = cur;
        if 0x72 < req && cur_type.wrapping_sub(1) <= 9 {
            a7 = 0;
        }
        let mut mode = 0i32;
        if req.wrapping_sub(0x1d) < 3 {
            mode = 2;
        }
        let mut u14 = 0u32;
        if req.wrapping_sub(0x83) < 0x12 {
            mode = 2;
            u14 = if req == 0x94 { 2 } else { 1 };
        }
        let t7 = attack_type(a7);
        if t7.wrapping_sub(1) < 10 && 10 < rt {
            a7 = 0;
            mode = 2;
        }
        if 4 < t7.wrapping_sub(2) && t7 != 8 {
            self.players[p].set_i32(0x8e0, 0);
        }
        self.players[p].set_i32(0x7dc, 0);
        let mut c = Choice { selected: req, mode, loop_word: 0, start: 0, table: false, set_7dc: false, tree_flag: None };
        self.action_first_switch(p, a7, cur, req, rt, u14, &mut c)?;
        if c.set_7dc {
            self.players[p].set_i32(0x7dc, 1);
        }
        if let Some(set) = c.tree_flag {
            self.events.push(Event::PlayerTreeFlags { player: p, flags: 2, set });
        }
        let class = self.players[p].i32_(8);
        let mut blend = 0.0;
        if c.table {
            blend = self.action_jump_table(p, cur, class, &mut c);
        }
        // LAB_00178a94: 0x54 / 0x55 fall back to 0x23 / 0x24 without a sequence.
        let sel = c.selected;
        let mut seq_action = sel;
        if sel == 0x54 && self.players[p].i32_(0x488) < 0 {
            seq_action = 0x23;
        } else if sel == 0x55 && self.players[p].i32_(0x490) < 0 {
            seq_action = 0x24;
        }
        self.players[p].set_u16(0xc8, c.loop_word);
        let sequence = self.players[p].i32_(0x1e8 + 8 * seq_action as usize).max(0) as u16;
        let scale = self.action_playback_scale(p, sel, rt)?;
        self.players[p].set_f32(0xbc, scale);
        // DoAnimateTree(blend, +0x90, sequence, start, mode, 1).
        let clock = clock_of(&self.players[p]);
        let current = self.player_clip(p, clock.sequence)?;
        let requested = self.player_clip(p, sequence)?;
        let Ok(start_frame) = i16::try_from(c.start) else {
            return unsupported(format!("DoPlayerAction start frame {}", c.start));
        };
        let out = select(
            clock,
            current,
            requested,
            SelectionInput {
                sequence,
                mode: c.mode,
                start_frame,
                blend_duration: blend,
                playback_scale: scale,
                now: self.globals.atime,
                anim_time_scale: self.globals.anim_time_scale,
            },
        )?;
        set_clock(&mut self.players[p], &out.state);
        if out.return_flags & 1 != 0 {
            // InitAnim: AnimInfo +0x10 (+0xa4) = the sequence's frame count.
            self.players[p].set_i16(0xa4, requested.frames);
        }
        // DoAnimateTree(…, 1) evaluates the tree: DoAnimation 0x10fcb0 turns a
        // nonzero reset byte into 2 (CalcAnimInfo clears 2 next update).
        if self.players[p].u8_(0xa6) != 0 {
            self.players[p].set_u8(0xa6, 2);
        }
        let changed = out.return_flags != 0;
        self.players[p].set_u32(0x8c8, attack_type(if changed { sel } else { cur }));
        if changed {
            self.action_events(p, cur, sel, class);
        }
        let frame = self.players[p].f32_(0xac);
        self.action_scales(p, cur, class, frame);
        self.players[p].set_u32(0x1e0, if changed { sel } else { cur });
        Ok(())
    }

    /// Playback scale (AnimInfo +0x28 = player +0xbc), 0x178b0c..0x178c00.
    fn action_playback_scale(&self, p: usize, sel: u32, rt: u32) -> R<f32> {
        let r = &self.players[p];
        if r.u16_(0x93c) & 0xd0 != 0 || 0xb <= rt || rt == 1 || sel.wrapping_sub(0x58) <= 2 || sel.wrapping_sub(0x88) <= 0xb {
            return Ok(1.0);
        }
        if r.u32_(0x8ac) & 0x8000 != 0 && 0x82 <= sel {
            return Ok(2.0);
        }
        if r.u32_(0x130) & 0x2000_0000 != 0 && rt.wrapping_sub(9) <= 1 {
            return Ok(0.75);
        }
        if sel == 0x78 {
            let v = mul_rz(r.f32_(0x11c), f32::from_bits(0x3e4c_cccd))?;
            return Ok(if v < 0.25 { 0.25 } else { v });
        }
        Ok(if r.u32_(0x138) & 0x10000 != 0 { 0.75 } else { 1.0 })
    }

    /// The first switch on the (possibly zeroed) current action `a7`
    /// (0x177d6c; table 0x2aad40). Leaves `c.table` = bVar3.
    #[allow(clippy::too_many_arguments)]
    fn action_first_switch(&self, p: usize, a7: u32, cur: u32, req: u32, rt: u32, u14: u32, c: &mut Choice) -> R<()> {
        let r = &self.players[p];
        let frame = r.f32_(0xac);
        // (int)(frame + 0.5): add.s then cvt.w.s (truncation).
        let start_here = |c: &mut Choice| -> R<()> {
            c.start = add_guarded_rz(frame, 0.5)? as i32;
            c.mode = 2;
            Ok(())
        };
        let f8d0 = r.u32_(0x8d0);
        let e0 = r.i32_(0x8e0);
        let e4 = r.u32_(0x8e4);
        let cc = r.i32_(0x8cc);
        enum End {
            /// `break`: bVar3 = selected < 0x66.
            Break,
            /// goto LAB_00178774 with an explicit bVar3.
            Direct(bool),
        }
        let d22 = |c: &mut Choice| {
            if rt == 1 {
                c.mode = 2;
            }
        };
        // LAB_0017812c / LAB_001782c4 for a nonzero combo counter +0x8e0.
        let combo_follow = |c: &mut Choice| {
            c.selected = match e0 {
                1 => 0x23,
                2 => 0x25,
                _ => 0x3c,
            };
        };
        // The 0x27..0x29 / 0x3e..0x40 follow-up choice (0x178154 / 0x178258).
        let swing = |c: &mut Choice, low: (u32, u32), alt_cur: u32| {
            if f8d0 & 0x400 != 0 && e0 != 0 {
                combo_follow(c);
                return;
            }
            let u7 = if (f8d0 == 0 && cc == 0) || e4 & 8 != 0 {
                c.selected = low.0;
                low.1
            } else if e4 & 4 == 0 {
                c.selected = 0x28;
                0x29
            } else {
                c.selected = 0x3f;
                0x40
            };
            if cur == alt_cur {
                c.selected = u7;
            }
        };
        let end = match a7 {
            0 => {
                c.loop_word = 1;
                c.mode = 2;
                if req != 0 {
                    End::Break
                } else if r.i32_(0x80c) != 0 {
                    End::Direct(true)
                } else {
                    // AuxLevels 0x210788 bit 0x10 shortens the idle thresholds.
                    let debug = self.globals.aux_levels & 0x10 != 0;
                    let (long, short) = if debug { (0xb4, 0x3c) } else { (0x708, 600) };
                    if long < r.i16_(0x1d8) {
                        c.selected = 1;
                        c.mode = 1;
                        End::Break
                    } else if r.i16_(0x1da) <= short {
                        End::Direct(true)
                    } else {
                        c.selected = 2;
                        c.mode = 1;
                        End::Break
                    }
                }
            }
            1 | 2 => {
                c.mode = 2;
                if req == 0 {
                    c.selected = if a7 == 1 { 0 } else { 3 };
                    c.mode = 1;
                }
                End::Break
            }
            3 => {
                c.loop_word = 1;
                c.mode = 2;
                if req == 0 {
                    c.selected = 3;
                    c.mode = 0;
                }
                End::Break
            }
            4..=7 | 0x77 | 0x78 => {
                c.mode = 1;
                c.selected = if a7 == 0x77 { 0x78 } else { 0x79 };
                End::Break
            }
            8 => {
                c.loop_word = 1;
                if 0x1f < req {
                    c.mode = 2;
                }
                End::Break
            }
            9..=0x10 => {
                if 0x1f < req || rt != 0 {
                    c.mode = 2;
                }
                // (first request, second request) → (selection, selection).
                let (q1, q2, s1, s2) = match a7 {
                    9 | 0xb => (9, 0xb, 10, 0xc),
                    10 | 0xc => (9, 0xb, 9, 0xb),
                    0xd | 0xf => (0xd, 0xf, 0xe, 0x10),
                    _ => (0xd, 0xf, 0xd, 0xf),
                };
                if req == q1 {
                    c.selected = s1;
                }
                if req == q2 {
                    c.selected = s2;
                }
                End::Break
            }
            0x11..=0x14 => {
                let force = if a7 == 0x14 { req == 0x1b } else { 0x1a < req };
                if force || rt != 0 {
                    c.mode = 2;
                }
                let (q, s) = match a7 {
                    0x11 => (0x11, 0x12),
                    0x12 => (0x11, 0x11),
                    0x13 => (0x13, 0x14),
                    _ => (0x13, 0x13),
                };
                if req == q {
                    c.selected = s;
                }
                End::Break
            }
            0x15 => {
                c.loop_word = 1;
                c.mode = 2;
                End::Break
            }
            0x17 | 0x18 | 0x7d => {
                c.mode = 2;
                End::Break
            }
            0x16 => {
                if req < 0x20 {
                    if rt != 0 {
                        c.mode = 2;
                    }
                    End::Direct(req < 0x66)
                } else {
                    c.mode = 2;
                    End::Break
                }
            }
            0x19 | 0x1a => {
                if req != 0 {
                    c.loop_word = 1;
                    c.mode = 2;
                }
                End::Break
            }
            0x1b => {
                c.mode = if 0x81 < req { 3 } else { 2 };
                if req != 0 && req != 0x11 {
                    if req == 0x13 {
                        c.mode = 0;
                    }
                    End::Direct(req < 0x66)
                } else {
                    c.mode = 0;
                    End::Break
                }
            }
            0x7c => {
                c.mode = 0;
                End::Break
            }
            0x1c => {
                if req == cur {
                    c.selected = 0;
                }
                End::Break
            }
            0x1d | 0x1e => {
                if req == 0x1d {
                    c.selected = 0x1e;
                    c.loop_word = 1;
                } else {
                    c.selected = 0x1f;
                }
                End::Break
            }
            0x20 => {
                if rt < 0xb && rt != 9 && rt != 10 {
                    c.selected = 0x21;
                } else {
                    start_here(c)?;
                }
                End::Break
            }
            0x21 => {
                c.set_7dc = true;
                c.selected = 0x22;
                End::Break
            }
            0x22 | 0x2e | 0x2f | 0x32 | 0x33 | 0x36 | 0x37 | 0x3a | 0x3b | 0x3d | 0x43..=0x46 | 0x61 | 0x62 => {
                d22(c);
                End::Break
            }
            0x23 | 0x25 | 0x54 | 99 => {
                if a7 == 0x25 {
                    c.set_7dc = true;
                }
                if u14 < 2 {
                    c.mode = 0;
                }
                c.selected = match a7 {
                    0x23 => 0x24,
                    0x25 => 0x26,
                    0x54 => 0x55,
                    _ => 100,
                };
                End::Break
            }
            0x24 | 0x26 | 0x55 | 100 => {
                if u14 == 0 {
                    c.mode = 0;
                }
                End::Break
            }
            0x27..=0x29 => {
                swing(c, (0x2b, 0x2a), 0x28);
                End::Break
            }
            0x3e..=0x40 => {
                swing(c, (0x42, 0x41), 0x3f);
                End::Break
            }
            0x2a | 0x2b => {
                if f8d0 & 0x400 != 0 && e0 != 0 {
                    combo_follow(c);
                } else if f8d0 != 0 && frame <= 2.0 && e4 & 1 != 0 {
                    c.selected = if cur == 0x2a { 0x29 } else { 0x28 };
                    c.mode = 2;
                }
                d22(c);
                End::Break
            }
            0x2c | 0x2d | 0x30 | 0x31 | 0x34 | 0x35 | 0x38 | 0x39 => {
                c.selected = a7 + 2;
                End::Break
            }
            0x3c => {
                c.set_7dc = true;
                c.selected = if f8d0 & 0x400 != 0 && e0 != 0 { 0x25 } else { 0x3d };
                End::Break
            }
            0x41 | 0x42 => {
                if f8d0 & 0x400 != 0 && e0 != 0 {
                    combo_follow(c);
                } else {
                    d22(c);
                }
                End::Break
            }
            0x47..=0x4e => {
                // 0x47/0x49 and 0x4b/0x4d advance to the second half of the
                // swing; 0x48/0x4a and 0x4c/0x4e return to the first.
                let (q1, q2) = if a7 <= 0x4a { (0x47, 0x49) } else { (0x4b, 0x4d) };
                let first = matches!(a7, 0x47 | 0x49 | 0x4b | 0x4d);
                if req == q1 {
                    c.selected = if first { q1 + 1 } else { q1 };
                }
                if req == q2 {
                    c.selected = if first { q2 + 1 } else { q2 };
                }
                if self.tree_6b8_usable(p) {
                    c.tree_flag = Some(frame < 2.0);
                }
                End::Break
            }
            0x4f | 0x50 => {
                c.selected = if req != 0x4f {
                    0x51
                } else if cur == 0x4f {
                    0x50
                } else {
                    0x4f
                };
                End::Break
            }
            0x51 => {
                if rt == 1 {
                    c.mode = 2;
                } else if req == 0x4f {
                    c.selected = if r.i32_(0x468) < 0 { req } else { 0x50 };
                } else {
                    c.mode = 1;
                }
                End::Break
            }
            0x52 => {
                c.selected = if f8d0 & 0x400 != 0 && e0 != 0 { 0x54 } else { 0x53 };
                End::Break
            }
            0x53 => {
                if f8d0 & 0x400 != 0 && e0 != 0 {
                    c.selected = 0x54;
                } else {
                    c.mode = 1;
                    d22(c);
                }
                End::Break
            }
            0x56 | 0x57 | 0x5a | 0x88 | 0x8a..=0x8e | 0x90..=0x93 => {
                if u14 < 2 {
                    c.mode = 0;
                }
                End::Break
            }
            0x58 => {
                c.mode = 1;
                if 0 <= r.i32_(0x4b0) {
                    c.selected = 0x59;
                } else if 0 <= r.i32_(0x4b8) {
                    c.selected = 0x5a;
                } else if u14 < 2 {
                    c.mode = 0;
                }
                End::Break
            }
            0x59 | 0x89 | 0x8f => {
                c.loop_word = 1;
                if req != a7 {
                    if 0 <= r.i32_(0x1f0 + 8 * a7 as usize) {
                        c.selected = a7 + 1;
                    }
                    c.mode = 2;
                } else {
                    c.mode = 0;
                }
                End::Break
            }
            0x5b..=0x5e => {
                if rt < 2 || rt == 9 || rt == 10 {
                    if req == 0x73 || req == 0x75 {
                        c.mode = 2;
                    } else if req != 0x65 {
                        if req != 0x5b && 2.0 <= frame {
                            c.mode = 2;
                        }
                        c.selected = if a7 >= 0x5d { 0x60 } else { 0x5f };
                    } else {
                        start_here(c)?;
                    }
                } else {
                    start_here(c)?;
                }
                End::Break
            }
            0x5f | 0x60 => {
                c.selected = if cur == 0x5f { 0x61 } else { 0x62 };
                End::Break
            }
            0x65 | 0x66 => {
                if rt < 2 || rt == 9 || rt == 10 {
                    if req == 0x73 || req == 0x75 {
                        c.mode = 2;
                        End::Break
                    } else if req != 0x65 {
                        End::Direct(req < 0x66)
                    } else {
                        c.selected = if cur == 0x65 { 0x66 } else { 0x65 };
                        End::Break
                    }
                } else {
                    start_here(c)?;
                    End::Break
                }
            }
            0x67 | 0x68 | 0x6e | 0x70 => {
                if u14 == 0 {
                    c.mode = 0;
                }
                c.selected = match a7 {
                    0x67 => 0x69,
                    0x68 => 0x6a,
                    0x6e => 0x6f,
                    _ => 0x71,
                };
                End::Break
            }
            0x6b | 0x6c => {
                if req == 0x6b {
                    c.selected = 0x6c;
                    c.loop_word = 1;
                } else if rt == 0 {
                    c.selected = 0x6d;
                } else {
                    c.mode = 1;
                }
                End::Break
            }
            0x73 => {
                c.selected = 0x75;
                if req == 0x75 {
                    c.mode = 2;
                } else if r.u16_(0x92e) & 4 == 0 {
                    c.mode = 0;
                } else {
                    c.selected = 0x74;
                }
                End::Break
            }
            0x75 => {
                c.selected = 0x76;
                End::Break
            }
            0x7a => {
                c.loop_word = 1;
                if req == 0 {
                    c.mode = 0;
                    End::Break
                } else {
                    c.mode = 2;
                    if frame < 10.0 && r.i16_(0xca) != 0 {
                        End::Direct(req < 0x66)
                    } else {
                        if frame < 10.0 {
                            c.mode = 0;
                        }
                        End::Break
                    }
                }
            }
            0x7b => {
                if u14 == 0 {
                    c.mode = 0;
                }
                c.loop_word = 1;
                End::Break
            }
            0x7e | 0x84 | 0x86 | 0x7f | 0x81 | 0x82 => {
                c.mode = if matches!(a7, 0x7f | 0x81 | 0x82) && 0x83 <= req { 3 } else { 0 };
                if req == cur {
                    c.selected = 0;
                }
                End::Break
            }
            0x80 => {
                c.loop_word = 1;
                if req != 0 {
                    c.mode = 2;
                }
                End::Break
            }
            0x83 | 0x87 => {
                c.mode = 0;
                if req != a7 {
                    c.selected = 0x84;
                    c.mode = 1;
                    End::Break
                } else {
                    End::Direct(req < 0x66)
                }
            }
            0x85 => {
                c.mode = 0;
                if req != 0x85 {
                    c.selected = 0x86;
                    c.mode = 1;
                }
                End::Break
            }
            0x94 => {
                c.mode = 2;
                c.loop_word = 1;
                End::Break
            }
            // 0x1f, 0x69, 0x6a, 0x6d, 0x6f, 0x71, 0x72, 0x74, 0x76, 0x79,
            // 0x95 and anything ≥ 0x96 go straight to the tail.
            _ => End::Break,
        };
        c.table = match end {
            End::Break => c.selected < 0x66,
            End::Direct(b) => b,
        };
        Ok(())
    }

    /// The jump table at 0x2aafa0 (selected < 0x66); returns the blend.
    fn action_jump_table(&self, p: usize, cur: u32, class: i32, c: &mut Choice) -> f32 {
        let r = &self.players[p];
        let rel = r.f32_(0x8dc);
        // 0x178828 / 0x17896c (back 0x34/0x38, front side 0x2c/0x30) and
        // 0x178888 / 0x1789f4 (the second-swing variants, +1).
        let quadrant = |sel: &mut u32, second: bool| {
            let k = u32::from(second);
            if DEG135 < rel {
                *sel = 0x34 + k;
            } else if rel < -DEG135 {
                *sel = 0x38 + k;
            } else if DEG60 < rel {
                *sel = 0x2c + k;
            } else if rel < -DEG60 {
                *sel = 0x30 + k;
            }
        };
        let mut blend = 0.0;
        match c.selected {
            0 => {
                if r.u32_(0x134) & 0x62_0000 != 0 {
                    c.selected = 0x15;
                }
                if cur != 0 && cur.wrapping_sub(0x56) > 0x3d && cur != 0x1b && cur != 0x81 && cur != 0x82 && (class != 3 || cur != 0x2a) {
                    blend = IDLE_BLEND;
                }
            }
            0x11..=0x14 => {
                if r.u32_(0x134) & 0x62_0000 != 0 {
                    c.selected = 0x16;
                    if cur == 0x16 {
                        c.mode = 0;
                    }
                    c.loop_word = 1;
                }
            }
            0x27 | 0x29 | 0x40 => quadrant(&mut c.selected, false),
            0x28 | 0x3f => quadrant(&mut c.selected, true),
            0x3e => {
                if cur == 0x12 || cur == 0x14 {
                    c.selected = 0x45;
                } else if cur == 0x27 || cur == 0x29 {
                    c.selected = 0x43;
                }
                quadrant(&mut c.selected, false);
            }
            0x5b | 0x5c | 0x65 => {
                if cur == 0x11 || cur == 0x13 {
                    c.selected = match c.selected {
                        0x5b => 0x5d,
                        0x5c => 0x5e,
                        _ => 0x66,
                    };
                }
            }
            0x23 => {
                if r.u32_(0x8e4) & 2 != 0 {
                    c.selected = 0x54;
                }
            }
            _ => {}
        }
        blend
    }

    /// The two switches after a new animation took over (0x178c64..0x178f0c).
    fn action_events(&mut self, p: usize, cur: u32, sel: u32, class: i32) {
        let usable = self.tree_6b8_usable(p);
        let r = &mut self.players[p];
        r.set_f32(0x974, -1.0);
        // Previous action (every path reaches the +0x93c mask at 0x178e14).
        let mut ev = 0u32;
        let mut tree = None;
        match cur {
            1 => {
                r.set_i16(0x1d8, 0);
                r.set_i16(0x1da, 1);
            }
            3 if sel != 3 => {
                r.set_i16(0x1d8, 0);
                r.set_i16(0x1da, 0);
            }
            8 | 0x11 | 0x13 | 0x16 => r.or_u16(0x93a, 1),
            0x12 | 0x14 => r.or_u16(0x93a, 2),
            0x21 | 0x3e..=0x40 | 0x43 | 0x45 => ev = 4,
            0x23 | 0x25 if class != 6 => ev = 0x10,
            0x54 => ev = 0x10,
            0x27..=0x29 | 0x2c | 0x2d | 0x30 | 0x31 | 0x34 | 0x35 | 0x38 | 0x39 | 0x3c | 0x4f | 0x50 => ev = 2,
            0x47..=0x4e | 0x5f | 0x60 | 0x65 | 0x66 => ev = 0x100,
            0x52 => ev = 8,
            0x61 | 0x62 | 100 if usable => tree = Some(false),
            99 if !(class == 6 && r.i32_(0x80c) < 2) => ev = 0x1000,
            0x67 if sel == 0x69 => ev = 0x2000,
            0x68 if sel == 0x6a => ev = 0x4000,
            0x6b | 0x6c if sel.wrapping_sub(0x6c) <= 1 => ev = 0x800,
            _ => {}
        }
        if ev != 0 {
            r.or_u32(0x8d8, ev);
        }
        r.and_u16(0x93c, 0x38fd);
        // New action; the default (and most cases) clear +0x8d0.
        let mut clear_8d0 = true;
        match sel {
            4..=7 | 0x78 => {
                r.or_u16(0x93c, 0x200);
                clear_8d0 = false;
            }
            8 => r.or_u16(0x93c, 0x400),
            9..=0x10 => {
                r.or_u16(0x93c, 0x4000);
                clear_8d0 = false;
            }
            0x20 | 0x21 | 0x27..=0x29 | 0x2c | 0x2d | 0x30 | 0x31 | 0x34 | 0x35 | 0x38 | 0x39 | 0x3c | 0x3e..=0x40 | 0x43 | 0x45 | 0x4f | 0x50 | 0x52 => {
                r.or_u32(0x8d8, 1);
                let n = if r.i32_(0x8d0) == 0 { 0 } else { r.i32_(0x8e0) + 1 };
                r.set_i32(0x8e0, n);
            }
            0x22 | 0x2a | 0x2b | 0x2e | 0x2f | 0x32 | 0x33 | 0x36 | 0x37 | 0x3a | 0x3b | 0x3d | 0x41 | 0x42 | 0x44 | 0x46 | 0x51 | 0x53 => clear_8d0 = false,
            0x47..=0x4e => {
                r.or_u16(0x93c, 0x8000);
                r.or_u32(0x8d8, 1);
            }
            0x5b..=0x5e | 0x65 | 0x66 | 0x6b | 0x6c => r.or_u32(0x8d8, 1),
            0x61 | 0x62 | 100 => {
                if usable {
                    tree = Some(true);
                }
                clear_8d0 = false;
            }
            0x6e => r.or_u32(0x8d8, 0x100_0000),
            0x71 => r.or_u32(0x8d8, 0x200_0000),
            0x73 => r.or_u32(0x8d8, 0x10000),
            0x74 => r.or_u32(0x8d8, 0x20000),
            0x76 => r.or_u32(0x8d8, 0x40000),
            0x75 => {
                r.or_u32(0x8d8, 0x10000);
                r.set_i16(0x930, 0);
            }
            0x77 => {
                r.or_u16(0x93c, 0x100);
                clear_8d0 = false;
            }
            0x7b => {
                r.or_u16(0x93c, 0x802);
                clear_8d0 = false;
            }
            0x7e | 0x83..=0x87 => {
                r.or_u16(0x93c, 2);
                clear_8d0 = false;
            }
            _ => {}
        }
        if clear_8d0 {
            r.set_i32(0x8d0, 0);
        }
        if let Some(set) = tree {
            self.events.push(Event::PlayerTreeFlags { player: p, flags: 2, set });
        }
    }

    /// Speed (+0x964), turn (+0x968), control (+0x96c) and +0x970 scales
    /// from the action current at entry (0x178f0c..0x179310).
    fn action_scales(&mut self, p: usize, cur: u32, class: i32, frame: f32) {
        let r = &mut self.players[p];
        r.set_f32(0x970, 1.0);
        r.set_f32(0x96c, 1.0);
        let two_thirds = f32::from_bits(0x3f2a_c083);
        // (speed, turn, clear +0x970)
        let (s, t, z): (f32, f32, bool) = match cur {
            0x6b..=0x71 => (0.0, 0.5, true),
            0x67..=0x6a => (0.25, 1.0, false),
            0x65 | 0x66 => (1.0, 1.0, false),
            99 | 100 => (0.25, 1.0, false),
            0x5b..=0x62 => (0.0, 0.5, false),
            0x58..=0x5a => (0.0, 0.0, true),
            0x57 => (0.0, if class != 6 || frame <= 11.0 { 0.25 } else { 0.0 }, true),
            0x56 => (0.0, 1.0, true),
            0x54 | 0x55 => (0.25, 1.0, true),
            0x4f..=0x53 => (1.0, 1.0, false),
            0x47..=0x4e => (two_thirds, 1.0, false),
            0x3e..=0x46 => (1.0, 0.25, false),
            0x3c | 0x3d => (0.5, 1.0, false),
            0x2c..=0x3b => (1.0, 1.0, false),
            0x27..=0x2b => (0.25, 0.0, false),
            0x25 => {
                if class == 6 || class == 7 {
                    (0.0, 0.0, true)
                } else {
                    (if class == 2 || class == 3 { 0.25 } else { 0.5 }, 1.0, true)
                }
            }
            0x23 | 0x24 | 0x26 => {
                if class == 5 || class == 6 {
                    (0.0, 0.0, true)
                } else {
                    (if class == 2 { 0.25 } else { 0.5 }, 1.0, true)
                }
            }
            0x20..=0x22 => (0.0, 1.0, false),
            0x80 => (f32::from_bits(0x3ecc_cccd), 0.5, false),
            0x13 | 0x14 | 0x16 => (f32::from_bits(0x3fa6_6666), 1.0, false),
            0x8f => (1.5, 0.5, false),
            8 => (1.5, 1.0, false),
            9..=0x10 => (two_thirds, 1.0, false),
            _ => {
                if cur != 0x7b && 0x72 < cur {
                    r.set_f32(0x96c, 0.0);
                }
                (1.0, 1.0, false)
            }
        };
        r.set_f32(0x964, s);
        r.set_f32(0x968, t);
        if z {
            r.set_f32(0x970, 0.0);
        }
    }
}
