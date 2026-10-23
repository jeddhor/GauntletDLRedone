//! The stage-start title: init_stage_info 0x16b7d0 (init_game_cam, with
//! cam_wait 0x5b), write_stage_info 0x16b918 (StdCamStartStage, every
//! update of the start while game_mode is 0x4010) and del_msgs_at_start
//! 0x1670a8 (research/hud.md "Stage title").
//!
//! The level name (LEVL +0x14) grows from scale 0.05 by 0.05 per vblank
//! (DrawTextScaled font 6, white, centred, y 0x30 − scale × 16) and settles
//! at 2.0 for the last two cam_wait steps. LEVL +0 flag 1 adds
//! SHOTS_STUN (ENGLISH.ROM 0xaf), flag 4 (the timed secret levels)
//! GRAB_COINS (0xb0), each in colour 0x160c03 at y 0x6c (centred) over a
//! SCROLL_A box sized to the text + (0x3c, 0x10); the line's announcer cue
//! (AudioStunPlayers / AudioGrabGold) is queued when the scale first
//! reaches 2.0. FIND_EXIT (0xb1, AudioFindExit) needs disp_find_exit, which
//! only InitCamera and init_stage_info write (0): never shown.
use super::audio_queue::QueEntry;
use super::{Game, R};

/// stage_scale 0x2c32f8, stage_info_timer 0x2c33bc and stageinst_blit
/// 0x2c32f4 (NATIVE: the box is drawn per frame from `entry` — the client
/// loads the HUD textures and text after init_game_cam).
#[derive(Debug, Clone, Default)]
pub struct StageInfo {
    pub scale: f32,
    pub timer: i32,
    pub entry: Option<i32>,
}

impl Game {
    /// init_stage_info 0x16b7d0.
    pub(crate) fn init_stage_info(&mut self) {
        self.stage_info.timer = 0x5b;
        self.stage_info.scale = f32::from_bits(0x3ccc_cccd);
        let flags = self.level_u32(0);
        self.stage_info.entry = if flags & 1 != 0 {
            Some(0xaf)
        } else if flags & 4 != 0 {
            Some(0xb0)
        } else {
            None
        };
    }

    /// del_msgs_at_start 0x1670a8: the box blit removed.
    pub(crate) fn del_msgs_at_start(&mut self) {
        self.stage_info.entry = None;
    }

    /// stageinst_blit: SCROLL_A (MBOX_NewBlit at 0x100 − w/2, 0x6c − h/2,
    /// MBBlitSetSize) around the entry's text (StringTextWidth/Height at
    /// 1.0 + (0x3c, 0x10)).
    fn stage_box(&mut self, entry: i32) {
        let w = self.hud.string_text_width(1.0, entry, -1);
        let h = self.hud.string_text_height(1.0, entry, -1, 0);
        if 0 < w {
            let (bw, bh) = (w + 0x3c, h + 0x10);
            let tex = self.hud.lookup("SCROLL_A");
            self.hud.temp_blit(tex, 0x100 - bw / 2, 0x6c - bh / 2, bw, bh);
        }
    }

    /// write_stage_info 0x16b918(cam_wait), after StdCamStartStage's clamp
    /// to 2.
    pub(crate) fn write_stage_info(&mut self, cam_wait: i32) -> R<()> {
        let prev = self.stage_info.scale;
        let vb = self.globals.vb_elapsed as f32;
        self.stage_info.scale += vb * 0.05;
        if cam_wait < 2 || 2.0 < self.stage_info.scale {
            self.stage_info.scale = 2.0;
        }
        if cam_wait < 1 {
            self.del_msgs_at_start();
            return Ok(());
        }
        let scale = self.stage_info.scale;
        let name: String = self.level.get(0x14..).unwrap_or(&[]).iter().take_while(|c| **c != 0).map(|c| char::from(*c)).collect();
        self.hud.draw_text(scale, -0x100, 0x30 - (scale * 16.0) as i32, 6, 0xffffff, &name);
        let flags = self.level_u32(0);
        let (entry, line) = if flags & 1 != 0 {
            // AudioStunPlayers 0x160750: AudioVQueAdd4(−1, 10, 0x10006, 0xe0, 0x7f, 2).
            (0xaf, (10.0, 0x1_0006))
        } else if flags & 4 != 0 {
            // AudioGrabGold 0x15e278: AudioVQueAdd4(−1, 1, 0x10029, 0xe0, 0x7f, 2).
            (0xb0, (1.0, 0x1_0029))
        } else {
            return Ok(());
        };
        if self.stage_info.entry.is_some() {
            self.stage_box(entry);
        }
        self.hud.draw_string_text(-0x100, 0x106c, -1, 0x160c03, entry, 0, &[]);
        if prev != scale && scale == 2.0 {
            self.vque_add(line.0, QueEntry::new(line.1, 0xe0, 0x7f, 2));
        }
        Ok(())
    }
}

/// do_show_path 0x124220's state: show_path_flag 0x220148, show_path_old
/// 0x2c1200, show_path_count 0x22014c, lost_timer1 0x220140.
#[derive(Debug, Clone, Default)]
pub struct ShowPath {
    pub flag: bool,
    pub old: bool,
    pub count: i32,
    pub lost_timer: i32,
}

impl Game {
    /// do_show_path 0x124220 (game_main, after ProcessEffects): with the
    /// debug arrows off (ShowMilestones(−1) 0) and stage_playtime past
    /// 0x4af, a combat-free stretch (did_action: damage_enemy,
    /// do_enemy_attack, enemy_attack_control, PlayerDamageItem) of 600
    /// vblanks — 0x1a4 after the first — "shows the path": the milestones'
    /// nodes (+0x60, null unless ShowMilestones made arrows, so nothing is
    /// drawn) and, the first time after a hidden interval, NewHelp(0x1d).
    /// Combat hides it at the next interval. No player in state 1 counts as
    /// action. Boss levels (BossType ≥ 0) and pauses skip the timer.
    pub(crate) fn do_show_path(&mut self) {
        if self.stage_playtime <= 0x4af {
            return;
        }
        let interval = if self.show_path.count == 0 { 600 } else { 0x1a4 };
        if !(0..4).any(|k| self.players[k].i32_(0xfc) == 1) {
            self.did_action = true;
            self.show_path.lost_timer = interval;
        }
        if self.globals.paused != 0 || 0 <= self.globals.boss_type {
            return;
        }
        if self.did_action {
            self.show_path.flag = false;
        }
        self.show_path.lost_timer += self.globals.vb_elapsed;
        if interval <= self.show_path.lost_timer {
            if self.show_path.flag {
                if !self.milestones.is_empty() && !self.show_path.old {
                    self.help_at(0x1d, usize::MAX, None);
                }
                self.show_path.count += 1;
            }
            self.show_path.old = self.show_path.flag;
            self.show_path.flag = true;
            self.show_path.lost_timer -= interval;
        }
    }
}
