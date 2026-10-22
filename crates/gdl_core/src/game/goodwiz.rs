//! The good wizard after a boss: StartGoodWizard 0x165cf0 / DoGoodWizard
//! 0x165d08 (world_update, game mode 0x4010), with calc_wizard_pos
//! 0x1659b8, calc_good_wiz_attn 0x165ab8, hide_rune_stones 0x165c48,
//! AudioGoodWizard 0x1611d0, GetBossRunes 0x189cc8 / GetBossNumRunes
//! 0x189d18 (worldseltab 0x24a460) and CaptionText 0x165568 over GameText.
//!
//! GoodWizard runs 1 (5 s wait, 10 s for 0x2a/0x2c) → 3 (the WIZARD tree of
//! the item animations at the players' mean with BossStartPos, 3 up; at
//! (0, −12, 6) for 0x2a/0x2b; turned toward a player) → 4 (fading in by 4 a
//! frame) → 5 (the boss's speech caption, GameText 0x93..0xa3 line by line,
//! each held 0x3c vblanks once revealed) → 6 (the realm's rune phrase
//! 0x9b..0x9e by the runes found, or Skorne's 0xa0/0xa1) → 7 → 8 (the auto
//! exit timer: WizDelayGoldLeft 600, WizDelayNoGold 120 without gold and
//! without all rune stones) → 9 (until AudioStillPlaying clears) → 10
//! (TextureExit skins at EXIT_SKIN_TIME 35) → 11. Speech lines are queued
//! through audio_queue; the wizard's model, alpha and captions are
//! presentation, his position also feeds the boss camera (good_wiz_objgrp
//! +0x40). add_target (the standard camera's target list) is not modelled:
//! boss levels run BossCameraUpdate.
use super::{Game, R};
use crate::animation_clock::{ClipTiming, ClockState};
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sub_guarded_rz as sub};

/// WizDelayGoldLeft 0x23e284, WizDelayNoGold 0x23e288, EXIT_SKIN_TIME
/// 0x23e270 (vblanks).
pub const WIZ_DELAY_GOLD_LEFT: i32 = 600;
pub const WIZ_DELAY_NO_GOLD: i32 = 120;
const EXIT_SKIN_TIME: i32 = 35;

#[derive(Debug, Clone, Default)]
pub struct GoodWiz {
    pub timer: f32,
    pub alpha: i32,
    pub speech_idx: i32,
    pub speech_frame: i32,
    pub speech_pause: i32,
    pub yaw: f32,
    pub plyr_attn: i32,
    pub all_rune_stones: bool,
    /// good_wiz_objgrp's matrix (rows 0..2 rotation, row 3 position) once
    /// the node exists (+0x60).
    pub matrix: Option<[[f32; 4]; 4]>,
    /// good_wiz_objgrp +0x40: the attention point (10 above the node).
    pub attn: [f32; 3],
    /// good_wiz_atree's clock and sequence (AnimateATree(…, 0, 0)).
    pub clock: Option<(ClockState, u16)>,
    /// The WIZARD tree's clips from the level's item animations.
    pub clips: Option<Vec<ClipTiming>>,
    /// optmenu_disable 0x2c3d10 (set by StartGoodWizard, cleared by the
    /// default case): the pause menu is off.
    pub optmenu_disable: bool,
}

/// worldseltab 0x24a460: 13 × 0x24 records (+0 world, +0x10 rune bits,
/// +0x14 rune count).
pub fn worldsel_from_elf(elf: &gdl_formats::elf::Elf) -> Vec<(i32, u32, i32)> {
    (0..13)
        .filter_map(|k| {
            let b = elf.read(0x24_a460 + 0x24 * k, 0x24).ok()?;
            let w = |o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
            Some((w(0), w(0x10) as u32, w(0x14)))
        })
        .collect()
}

impl Game {
    /// DEBUG (headless checks, not a game feature): CritterDamage(1e6) on
    /// the boss critter from player 0, so BossDeath and the good wizard run
    /// through the ported paths. Returns CritterDamage's result.
    pub fn debug_damage_boss(&mut self) -> R<i32> {
        let Some(b) = usize::try_from(self.boss_crit_inst - 1).ok().filter(|&b| self.critters.get(b).is_some_and(|c| c.is_some())) else { return Ok(-1) };
        self.critter_damage(1.0e6, b, 0, 0, None, None, 0)
    }

    /// StartGoodWizard.
    pub(crate) fn start_good_wizard(&mut self) {
        self.globals.good_wizard = 1;
        self.good_wiz.optmenu_disable = true;
    }

    fn boss_runes(&self) -> (u32, i32) {
        let w = self.globals.cur_world;
        self.worldsel.iter().find(|r| r.0 == w).map_or((0, 0), |r| (r.1, r.2))
    }

    /// The class record word +off (0xcf0 shards, 0xcf2 runes) ORed over
    /// players in state 1.
    fn class_bits(&self, off: usize) -> u32 {
        (0..4)
            .filter(|&k| self.players[k].i32_(0xfc) == 1)
            .map(|k| u32::from(self.players[k].u16_(off + 0xb4 * self.players[k].u32_(0xc) as usize)))
            .fold(0, |a, b| a | b)
    }

    /// hide_rune_stones: all twelve rune bits (0..11) present.
    fn hide_rune_stones(&self) -> bool {
        let bits = self.class_bits(0xcf2);
        (0..12).all(|b| bits & (1 << b) != 0)
    }

    /// calc_wizard_pos: (BossStartPos + Σ players in state 1/8) / (n + 1),
    /// with the first player's y counted twice instead of BossStartPos's.
    fn calc_wizard_pos(&self) -> R<[f32; 3]> {
        let mut v = self.boss_start_pos;
        let mut n = 1.0f32;
        for k in 0..4 {
            if matches!(self.players[k].i32_(0xfc), 1 | 8) {
                let p = self.players[k].v3(0x50);
                v[0] = add(p[0], v[0])?;
                v[1] = add(p[1], v[1])?;
                v[2] = add(p[2], v[2])?;
                if n == 1.0 {
                    v[1] = add(p[1], p[1])?;
                }
                n = add(n, 1.0)?;
            }
        }
        Ok([div(v[0], n)?, div(v[1], n)?, div(v[2], n)?])
    }

    /// calc_good_wiz_attn(snap, next): turn toward good_wiz_plyr_attn (the
    /// next player in state 1 when `next` or that one left), at most
    /// SPF·π/2 a frame unless `snap`.
    fn calc_good_wiz_attn(&mut self, snap: bool, next: bool) -> R<()> {
        let Some(m) = self.good_wiz.matrix else { return Ok(()) };
        let alive = |g: &Game, k: i32| usize::try_from(k).ok().is_some_and(|k| k < 4 && g.players[k].i32_(0xfc) == 1);
        if next || self.good_wiz.plyr_attn < 0 || !alive(self, self.good_wiz.plyr_attn) {
            let mut found = false;
            for _ in 0..4 {
                let w = &mut self.good_wiz;
                w.plyr_attn += 1;
                if 3 < w.plyr_attn {
                    w.plyr_attn = 0;
                }
                if alive(self, self.good_wiz.plyr_attn) {
                    found = true;
                    break;
                }
            }
            if !found {
                return Ok(());
            }
        }
        let p = self.players[self.good_wiz.plyr_attn as usize].v3(0x50);
        let mut a = sub(crate::libm::atan2f(sub(p[0], m[3][0])?, sub(p[2], m[3][2])?).map_err(|e| super::Unsupported(format!("calc_good_wiz_attn atan2f: {e:?}")))?, self.good_wiz.yaw)?;
        if !snap {
            let lim = mul(self.globals.spf, f32::from_bits(0x3fc9_0fdb))?;
            if lim < a {
                a = lim;
            }
            if a < -lim {
                a = -lim;
            }
        }
        let w = &mut self.good_wiz;
        w.yaw = add(a, w.yaw)?;
        let rows = super::world_anim::create_pyr_matrix([0.0, w.yaw, 0.0])?;
        let mut m = m;
        for r in 0..3 {
            m[r] = [rows[r][0], rows[r][1], rows[r][2], 0.0];
        }
        w.matrix = Some(m);
        Ok(())
    }

    /// AnimateATree(&good_wiz_atree, 0, 0) and calc_good_wiz_attn(0, 0).
    fn good_wiz_animate(&mut self) -> R<()> {
        let (atime, scale) = (self.globals.atime, self.globals.anim_time_scale);
        if let (Some((clock, current)), Some(clips)) = (self.good_wiz.clock, self.good_wiz.clips.as_ref())
            && let Some(&clip) = clips.first()
        {
            let out = crate::animation_clock::select(
                clock,
                clips.get(current as usize).copied().unwrap_or(clip),
                clip,
                crate::animation_clock::SelectionInput { sequence: 0, mode: 0, start_frame: 0, blend_duration: 0.0, playback_scale: 1.0, now: atime, anim_time_scale: scale },
            )?;
            let current = if out.return_flags & 1 != 0 { 0 } else { current };
            self.good_wiz.clock = Some((out.state, current));
        }
        self.calc_good_wiz_attn(false, false)
    }

    /// CaptionText(−1, message, line, frame, 0x10) over GameText: −1 no
    /// such line, 1 revealed, 0 still revealing (CaptionTextSub at the
    /// entry's scale × 0.667, its font).
    fn wiz_caption(&mut self, message: usize, line: i32, frame: i32) -> i32 {
        self.tower.caption.0 = line;
        self.tower.caption.1 = 0;
        let h = &self.hud;
        let Some(e) = h.text.as_ref().and_then(|r| r.entries.get(message)) else { return -1 };
        let Some(text) = usize::try_from(line).ok().and_then(|k| e.lines.get(k)).cloned() else { return -1 };
        let font = super::tower::r_font(h, super::tower::r_fonts(h), e.font).unwrap_or(0);
        let Ok(scale) = mul(e.scale, f32::from_bits(0x3f2a_c083)) else { return -1 };
        i32::from(self.hud.caption(scale, &text, font, frame, 0x10))
    }

    /// The speech-line step shared by GoodWizard 5 and 6: −1 → the phase's
    /// end at `timer` (+0.5 s / +1 s past the last line), held lines.
    fn wiz_speech_step(&mut self, message: i32, end_after: f32) -> R<()> {
        if message < 0 {
            self.good_wiz.speech_idx = -1;
            self.good_wiz.timer = self.globals.time;
            return Ok(());
        }
        let (idx, frame) = (self.good_wiz.speech_idx, self.good_wiz.speech_frame);
        let r = self.wiz_caption(message as usize, idx, frame >> 1);
        if r < 0 {
            self.good_wiz.timer = add(self.globals.time, end_after)?;
            self.good_wiz.speech_idx = -1;
        } else if 0 < r {
            let w = &mut self.good_wiz;
            if w.speech_pause < 0x3c {
                w.speech_pause += self.globals.vb_elapsed;
            } else {
                w.speech_idx += 1;
                w.speech_pause = 0;
                w.speech_frame = 0;
                self.calc_good_wiz_attn(false, true)?;
            }
        }
        Ok(())
    }

    fn wiz_restart_speech(&mut self) {
        let w = &mut self.good_wiz;
        w.speech_idx = 0;
        w.speech_frame = 0;
        w.speech_pause = 0;
    }

    /// DoGoodWizard (see the module doc).
    pub(crate) fn do_good_wizard(&mut self) -> R<()> {
        let runes = self.class_bits(0xcf2);
        let shards = self.class_bits(0xcf0);
        let (br, nr) = self.boss_runes();
        let found = if runes & br == br {
            if 1 < nr { 3 } else { 2 }
        } else {
            i32::from(runes & br != 0)
        };
        let bt = self.globals.boss_type;
        let time = self.globals.time;
        let vb = self.globals.vb_elapsed;
        let mut gw = self.globals.good_wizard;
        if gw == 1 {
            let wait = if bt == 0x2a || bt == 0x2c { 10.0 } else { 5.0 };
            self.good_wiz.timer = add(time, wait)?;
            gw = 2;
        }
        match gw {
            2 => {
                if self.good_wiz.timer <= time {
                    gw = 3;
                }
            }
            3 | 4 => {
                if gw == 3 {
                    let pos = if (0x2a..0x2c).contains(&bt) {
                        [0.0, -12.0, 6.0]
                    } else {
                        let p = self.calc_wizard_pos()?;
                        [p[0], add(p[1], 3.0)?, p[2]]
                    };
                    self.good_wiz.matrix = Some([[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [pos[0], pos[1], pos[2], 1.0]]);
                    self.good_wiz.attn = [pos[0], add(pos[1], 10.0)?, pos[2]];
                    self.good_wiz.clock = self.good_wiz.clips.as_ref().map(|_| (super::setup::fresh_clock(), 0));
                    self.good_wiz.yaw = 0.0;
                    // good_wiz_plyr_attn = CurWorld % 4 (truncating).
                    self.good_wiz.plyr_attn = self.globals.cur_world % 4;
                    self.calc_good_wiz_attn(true, true)?;
                    self.good_wiz.all_rune_stones = bt == 0x2a && self.hide_rune_stones();
                    self.good_wiz.alpha = 0xff;
                    gw = 4;
                }
                let w = &mut self.good_wiz;
                if 0 < w.alpha {
                    w.alpha -= 4;
                }
                w.alpha = w.alpha.max(0);
                self.good_wiz_animate()?;
                if self.good_wiz.alpha == 0 {
                    gw = 5;
                    self.wiz_restart_speech();
                    let k = if bt < 0x2a { 0 } else { i32::from(self.good_wiz.all_rune_stones) };
                    self.audio_good_wizard(bt, k);
                    self.globals.good_wizard = gw;
                    return self.good_wiz_speech5(found);
                }
            }
            5 => {
                self.globals.good_wizard = gw;
                return self.good_wiz_speech5(found);
            }
            6 => {
                self.good_wiz.speech_frame += vb;
                if 0 <= self.good_wiz.speech_idx {
                    let message = match bt {
                        b if b < 0x22 => -1,
                        b if b < 0x2a => [0x9b, 0x9c, 0x9d, 0x9e][found as usize],
                        0x2a => {
                            if self.good_wiz.all_rune_stones { 0xa0 } else { 0xa1 }
                        }
                        _ => -1,
                    };
                    self.wiz_speech_step(message, 1.0)?;
                } else if self.good_wiz.timer <= time {
                    gw = 7;
                    self.wiz_restart_speech();
                    if bt < 0x2a {
                        let k = if shards & 0x3fe == 0x3fe && runes == 0xfff {
                            7
                        } else if shards & 0x1fe == 0x1fe {
                            6
                        } else {
                            5
                        };
                        self.audio_good_wizard(bt, k);
                    }
                }
                self.good_wiz_animate()?;
            }
            7 => {
                self.good_wiz.speech_frame += vb;
                if self.good_wiz.speech_idx < 0 {
                    if self.good_wiz.timer <= time {
                        gw = 8;
                        self.wiz_restart_speech();
                    }
                } else {
                    self.good_wiz.speech_idx = -1;
                    self.good_wiz.timer = time;
                }
                self.good_wiz_animate()?;
            }
            8 | 9 => {
                if gw == 8 {
                    self.auto_exit_timer = if !self.is_gold_active() && !self.good_wiz.all_rune_stones { WIZ_DELAY_NO_GOLD } else { WIZ_DELAY_GOLD_LEFT };
                    gw = 9;
                }
                self.good_wiz_animate()?;
                if !self.audio_still_playing() {
                    gw = 10;
                }
            }
            10 => {
                self.good_wiz_animate()?;
                if self.auto_exit_timer <= EXIT_SKIN_TIME {
                    // SetSkinFX(0.5, TextureExit, 10, 1) on players in state 1.
                    for k in 0..4 {
                        if self.players[k].i32_(0xfc) == 1 {
                            super::skin::set_skin_fx(&mut self.players[k], 0x7b4, 0.5, super::skin::SkinTex::Exit, 10, 1.0);
                        }
                    }
                    gw = 11;
                }
            }
            11 => self.good_wiz_animate()?,
            _ => self.good_wiz.optmenu_disable = false,
        }
        self.globals.good_wizard = gw;
        self.good_wiz_clamp_exit();
        Ok(())
    }

    /// GoodWizard 5: the boss's speech (GameText by BossType), then the
    /// rune phrase line AudioGoodWizard(boss, found + 1).
    fn good_wiz_speech5(&mut self, found: i32) -> R<()> {
        let bt = self.globals.boss_type;
        self.good_wiz.speech_frame += self.globals.vb_elapsed;
        if self.good_wiz.speech_idx < 0 {
            if self.good_wiz.timer <= self.globals.time {
                self.globals.good_wizard = 6;
                self.wiz_restart_speech();
                if bt < 0x2a {
                    self.audio_good_wizard(bt, found + 1);
                }
            }
        } else {
            let message = match bt {
                0x22 => 0x94,
                0x23 => 0x93,
                0x24 => 0x95,
                0x25 => 0x96,
                0x26 => 0x98,
                0x27 => 0x97,
                0x28 => 0x9a,
                0x29 => 0x99,
                0x2a => 0x9f,
                0x2b => 0xa2,
                0x2c => 0xa3,
                _ => -1,
            };
            self.wiz_speech_step(message, 0.5)?;
        }
        self.good_wiz_animate()?;
        self.good_wiz_clamp_exit();
        Ok(())
    }

    /// DoGoodWizard's tail: past 8 without gold the exit waits at most
    /// WizDelayNoGold.
    fn good_wiz_clamp_exit(&mut self) {
        if 8 < self.globals.good_wizard && !self.is_gold_active() && WIZ_DELAY_NO_GOLD < self.auto_exit_timer {
            self.auto_exit_timer = WIZ_DELAY_NO_GOLD;
        }
    }
}
