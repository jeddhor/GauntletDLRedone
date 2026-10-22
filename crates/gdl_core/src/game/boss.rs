//! Boss completion: StartGoodWizard 0x165cf0 / DoGoodWizard 0x165d08 timing,
//! do_auto_exit 0x124458 and the auto exit (research/critters.md).
//!
//! The wizard's appearance, captions and speech are presentation; their
//! durations come from CaptionText/AudioStillPlaying, which are not
//! simulated: each speech phase waits a fixed SPEECH_SECONDS (HYPOTHESIS).
use super::critter_ai::yaw_vec3;
use super::damage::pd;
use super::effects::{fx, Visual};
use super::{Event, Game, R};
use crate::numeric::{add_guarded_rz as add, div_rn as div, mul_rz as mul, sub_guarded_rz as sub};

/// spew_tab 0x25d1a8: (bronze, silver, gold) coins per active player by CurWorld.
const SPEW_TAB: [[i32; 3]; 14] = [
    [0, 0, 0], [2, 1, 1], [0, 5, 0], [2, 1, 2], [2, 0, 2], [0, 0, 0], [0, 0, 4],
    [4, 1, 0], [0, 0, 0], [0, 3, 2], [0, 0, 3], [0, 4, 1], [0, 0, 0], [0, 0, 0],
];
/// COIN_BRONZE/SILVER/GOLD (0x2adee8/f8, 0x2adf08) and their values.
const COINS: [(&[u8], i32); 3] = [(b"COIN_BRONZE", 500), (b"COIN_SILVER", 1000), (b"COIN_GOLD", 5000)];

/// TowerWave (GauntInitWorldData 0x161568: 0xd00), the hub after a boss.
pub const TOWER_WAVE: i32 = 0xd00;
/// WizDelayGoldLeft 0x23e284 = 600, WizDelayNoGold 0x23e288 = 120 (vblanks).
const WIZ_DELAY_GOLD_LEFT: i32 = 600;
const WIZ_DELAY_NO_GOLD: i32 = 120;
/// Placeholder length of each good-wizard speech phase (not from the source).
const SPEECH_SECONDS: f32 = 6.0;

impl Game {
    /// do_auto_exit 0x124458 (game_main, before world_update).
    pub(crate) fn do_auto_exit(&mut self) {
        let g = &self.globals;
        if g.paused == 0 && g.help == 0 && g.cam_wait == 0 && 0 < self.auto_exit_timer {
            self.auto_exit_timer -= g.vb_elapsed;
            if self.auto_exit_timer < 1 {
                self.globals.auto_exit_flag = 1;
            }
        }
    }

    /// IsGoldActive 0x154e78: any live kind-1 subtype-1 item (gold).
    fn is_gold_active(&self) -> bool {
        self.items.records.iter().any(|r| {
            let d = &self.items.definitions[r.definition()];
            r.active() && d.kind() == 1 && d.subtype() == 1
        })
    }

    /// world_update's boss part (game_mode 0x4010): DoGoodWizard, and
    /// QuestItemState = 99 once the boss is dead.
    pub(crate) fn boss_world_update(&mut self) -> R<()> {
        if self.globals.boss_type < 0 || self.globals.game_mode != 0x4010 {
            return Ok(());
        }
        if self.globals.good_wizard != 0 {
            self.do_good_wizard()?;
        }
        self.process_spew_items()?;
        if self.globals.boss_is_dead != 0 {
            self.globals.quest_item_state = 99;
        }
        Ok(())
    }

    /// world_update's QuestItemState machine (0x1639ac..0x163c60): 2 → 3
    /// after a delay (1 s for 0x23 and 0x29/0x2a, else 3 s), 4 → 5 (timer),
    /// 5 → 6 after 29 s for 0x24/0x26/0x27/0x28/0x2a, with the quest weapon
    /// effect removed. The ambient flash and the wall effect's fade are
    /// presentation.
    pub(crate) fn quest_item_update(&mut self) -> R<()> {
        if self.globals.boss_is_dead != 0 {
            self.globals.quest_item_state = 99;
        }
        let time = self.globals.time;
        let bt = self.globals.boss_type;
        match self.globals.quest_item_state {
            3 => {}
            4 => {
                self.globals.quest_item_state = 5;
                self.quest_item_timer = time;
                return Ok(());
            }
            5 => {
                let left = sub(29.0, sub(time, self.quest_item_timer)?)?;
                let mut done = false;
                match bt {
                    0x24 => done = left <= 0.0,
                    0x26 => {
                        if left <= 0.0 {
                            // MBSetObject of the boss's +0xd8 part: presentation.
                            if let Ok(b) = usize::try_from(self.boss_crit_inst - 1) {
                                self.crm_pub(b).set_i16(0xb4e, 0);
                            }
                            self.globals.quest_item_state = 6;
                        }
                    }
                    0x27 | 0x28 | 0x2a => {
                        if left <= 0.0 {
                            if let Ok(b) = usize::try_from(self.boss_crit_inst - 1) {
                                self.crm_pub(b).set_f32(0xb50, 0.0);
                            }
                            done = true;
                        }
                    }
                    _ => {}
                }
                if done {
                    if 0 <= self.quest_weapon_sfx {
                        self.delete_effect(self.quest_weapon_sfx as usize, true);
                        self.quest_weapon_sfx = -1;
                        self.audio("AudioLegendItem", None, 3);
                        self.audio("AudioLegendItem", None, 4);
                    }
                    self.globals.quest_item_state = 6;
                }
                return Ok(());
            }
            2 => {
                if self.quest_item_timer == 0.0 {
                    let wait = if bt == 0x23 || (0x29..=0x2a).contains(&bt) { 1.0 } else { 3.0 };
                    self.quest_item_timer = add(time, wait)?;
                } else if self.quest_item_timer <= time {
                    self.globals.quest_item_state = 3;
                    self.quest_item_timer = 0.0;
                }
            }
            _ => return Ok(()),
        }
        // SetAmbientSpecial(0.1, -0.8): the darkened screen while the item is up.
        self.set_ambient_special(f32::from_bits(0x3dcc_cccd), f32::from_bits(0xbf4c_cccd))?;
        Ok(())
    }

    /// DoGoodWizard's state machine with placeholder speech lengths.
    fn do_good_wizard(&mut self) -> R<()> {
        let time = self.globals.time;
        let bt = self.globals.boss_type;
        match self.globals.good_wizard {
            1 => {
                let wait = if bt == 0x2a || bt == 0x2c { 10.0 } else { 5.0 };
                self.good_wiz_timer = add(time, wait)?;
                self.globals.good_wizard = 2;
            }
            2 => {
                if self.good_wiz_timer <= time {
                    self.globals.good_wizard = 3;
                }
            }
            3 => {
                self.good_wiz_alpha = 0xff;
                self.events.push(Event::Audio { cue: "GoodWizardAppears", player: None, arg: bt });
                self.globals.good_wizard = 4;
            }
            4 => {
                self.good_wiz_alpha = (self.good_wiz_alpha - 4).max(0);
                if self.good_wiz_alpha == 0 {
                    self.globals.good_wizard = 5;
                    self.events.push(Event::Audio { cue: "AudioGoodWizard", player: None, arg: bt });
                    self.good_wiz_timer = add(time, SPEECH_SECONDS)?;
                }
            }
            5..=7 => {
                if self.good_wiz_timer <= time {
                    self.globals.good_wizard += 1;
                    self.good_wiz_timer = add(time, SPEECH_SECONDS)?;
                }
            }
            8 => {
                self.auto_exit_timer = if self.is_gold_active() { WIZ_DELAY_GOLD_LEFT } else { WIZ_DELAY_NO_GOLD };
                self.globals.good_wizard = 9;
            }
            9 => self.globals.good_wizard = 10,
            10 => {
                if self.auto_exit_timer <= 35 {
                    // SetSkinFX(0.5, TextureExit, 10, 1) on players in state 1.
                    for k in 0..4 {
                        if self.players[k].i32_(0xfc) == 1 {
                            super::skin::set_skin_fx(&mut self.players[k], 0x7b4, 0.5, super::skin::SkinTex::Exit, 10, 1.0);
                        }
                    }
                    self.globals.good_wizard = 11;
                }
            }
            _ => {}
        }
        if 8 < self.globals.good_wizard && !self.is_gold_active() && WIZ_DELAY_NO_GOLD < self.auto_exit_timer {
            self.auto_exit_timer = WIZ_DELAY_NO_GOLD;
        }
        Ok(())
    }

    /// BossSpewCoins 0x19eb80(spread, position, velocity): an arena-wide item
    /// blast, then coins fanned over ±spread.
    pub(crate) fn boss_spew_coins(&mut self, spread: f32, pos: [f32; 3], v: [f32; 3]) -> R<()> {
        // StartFXSub(5.0, 0, position, 0x22, 0) + SfxSetDamage(1000, 1000).
        if let Some(h) = self.effects.tables.headers.first().copied().flatten() {
            let k = self.start_fx_tree(5.0, Visual::Header(0), h.frames, h.timing, Some(pos), 0x22)?;
            let r = self.fx_mut(k);
            r.set_u32(fx::KIND, 0);
            r.set_f32(fx::DAMAGE, 1000.0);
            r.set_f32(fx::BLAST, 1000.0);
            r.set_f32(fx::FADE, 0.0);
            r.set_u32(fx::DAMAGE_FLAGS, 0);
            r.set_i16(fx::OWNER, 0);
        }
        if self.globals.boss_type == 0x2a {
            return self.skorne_spew_items(spread, pos, v);
        }
        let nap = self.globals.num_active_players;
        if nap == 0 {
            return Ok(());
        }
        let world = self.globals.cur_world.clamp(0, 13) as usize;
        for (pass, (name, value)) in COINS.iter().enumerate() {
            let n = SPEW_TAB[world][pass] * nap;
            if n == 0 {
                continue;
            }
            let step = div(add(spread, spread)?, n as f32)?;
            let mut a = add(-spread, mul(step, 0.5)?)?;
            let base = if pass == 2 { 0.75 } else if pass == 1 { 0.8 } else { 0.85 };
            for _ in 0..n {
                let mut w = [0.0f32; 3];
                for (c, wc) in w.iter_mut().enumerate() {
                    let r = self.random_pub(f32::from_bits(0x3dcc_cccd))?;
                    *wc = mul(add(r, base)?, v[c])?;
                }
                // Passes 0/1 rotate the original vector (the scaled one is
                // overwritten), pass 2 the scaled one.
                let out = yaw_vec3(a, if pass == 2 { w } else { v })?;
                self.start_spew_item(0, 1, 1, name, *value, 0, pos, out)?;
                a = add(a, step)?;
            }
        }
        Ok(())
    }

    /// SkorneSpewItems 0x19ea..(spread, position, velocity): Skorne's four
    /// boss1_items (0x25d250, 0x1c each: name, +0x10 value, +0x14 the
    /// +0xf0 word, +0x18 the +0xf8 word) as kind-1 subtype-9 spew items
    /// fanned over ±spread (step spread / 2).
    fn skorne_spew_items(&mut self, spread: f32, pos: [f32; 3], v: [f32; 3]) -> R<()> {
        let step = mul(add(spread, spread)?, 0.25)?;
        let mut a = add(-spread, mul(step, 0.5)?)?;
        for k in 0..4 {
            let (name, value, f0, f8) = self.boss1_items[k].clone();
            let out = yaw_vec3(a, v)?;
            self.start_spew_item(f8, 1, 9, &name, value, f0, pos, out)?;
            a = add(a, step)?;
        }
        Ok(())
    }

    /// StartSpewItem 0x19e740(+0xf8 word, kind, subtype, name, +0xf4,
    /// +0xf0, position, velocity): AddItem at the position; kind 1 also
    /// sets +0x100 = 0x78.
    #[allow(clippy::too_many_arguments)]
    fn start_spew_item(&mut self, f8: u32, kind: i32, sub_: i32, name: &[u8], value: i32, f0: u32, pos: [f32; 3], vel: [f32; 3]) -> R<()> {
        if 0x20 <= self.spew_items.len() {
            return Ok(());
        }
        let Some(d) = self.items.find_info_index(kind, sub_, name) else { return Ok(()) };
        let k = self.add_item_sub(d, Some(([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], pos)))?;
        if kind == 1 {
            let r = &mut self.items.records[k];
            r.set_i32(0xf4, value);
            r.set_u32(0xf0, f0);
            r.set_u32(0xf8, f8);
            r.set_i16(0x100, 0x78);
        }
        self.spew_items.push((Some(k), vel));
        Ok(())
    }

    /// ProcessSpewItems 0x19e8a0: spewed items fly, bounce and slide to rest.
    pub(crate) fn process_spew_items(&mut self) -> R<()> {
        let spf = self.globals.spf;
        for s in 0..self.spew_items.len() {
            let (Some(k), mut v) = self.spew_items[s] else { continue };
            if self.items.records[k].u16_(0xd8) & 0x100 != 0 {
                self.spew_items[s].0 = None;
                continue;
            }
            let p = self.items.records[k].v3(0x40);
            let x = add(mul(v[0], spf)?, p[0])?;
            let mut y = add(mul(v[1], spf)?, p[1])?;
            let z = add(mul(v[2], spf)?, p[2])?;
            let mut fric = mul(spf, 0.5)?;
            let mut above = 10.0;
            if v[1] <= 0.0 {
                let wb = self.globals.world_bottom;
                let (floor, _) = self.collision.floor_pos(wb, 1.0, [x, y, z], 0)?;
                above = sub(y, add(floor, 1.0)?)?;
                if above < 0.1 {
                    let b = mul(-v[1], 0.4)?;
                    v[1] = if b < 0.1 { 0.0 } else { b };
                    y = add(floor, 1.0)?;
                    fric = mul(spf, 4.0)?;
                }
            }
            if 0.1 <= above {
                v[1] = sub(v[1], mul(spf, 32.0)?)?;
            }
            for c in [0, 2] {
                if fric < v[c] || v[c] < -fric {
                    v[c] = sub(v[c], mul(fric, v[c])?)?;
                } else {
                    v[c] = 0.0;
                }
            }
            let r = &mut self.items.records[k];
            r.set_f32(0x40, x);
            r.set_f32(0x44, y);
            r.set_f32(0x48, z);
            self.spew_items[s].1 = v;
        }
        Ok(())
    }

    /// do_players' auto exit: state-4 players count as finished once
    /// auto_exit_flag is set; game_main then continues to TowerWave.
    pub(crate) fn auto_exit_done(&mut self) -> bool {
        if self.globals.auto_exit_flag == 0 {
            return false;
        }
        let waiting = (0..4).any(|k| self.players[k].i32_(pd::STATE) == 1);
        if !waiting && self.level_done.is_none() {
            self.level_done = Some(TOWER_WAVE);
        }
        !waiting
    }
}
