//! The Audio* cues that were emitted as named placeholders, transcribed to
//! their sound requests (each function's EE address in its doc). Tables:
//! snd_turboA.215 0x23d0e0, snd_turboB.216 0x23d160, snd_turboC.217
//! 0x23d180, snd_walk.267 0x23d3f8, snd.281 0x23d428 (Click2), snd.145
//! 0x23ce30 (map dots), snd.149 0x23ce68 (tally loops), snd.153 0x23cea0
//! (rune counts), snd.524/531/535 0x23dac8/0x23db00/0x23db38 (transport,
//! world explosion, world hit), lvl_snd 0x23d7c8 (16 characters × 9),
//! gwiz_sidx 0x23dbe0 (11 bosses × 8), snd1–4.517–520 0x23da08.. (legend
//! items by world).
use super::audio_queue::QueEntry;
use super::{Game, R};

type V3 = [f32; 3];

#[derive(Debug, Clone, Default)]
pub struct CueTables {
    pub turbo_a: Vec<i32>,
    pub turbo_b: Vec<i32>,
    pub turbo_c: Vec<i32>,
    pub walk: Vec<i32>,
    pub click2: Vec<i32>,
    /// snd.277 0x23d420 (AudioClick).
    pub click: Vec<i32>,
    pub map_dot: Vec<i32>,
    pub tally: Vec<i32>,
    pub runes: Vec<i32>,
    pub transport: Vec<i32>,
    pub world_explosion: Vec<i32>,
    pub world_hit: Vec<i32>,
    pub lvl_snd: Vec<i32>,
    pub gwiz: Vec<i32>,
    pub legend: [Vec<i32>; 4],
    /// snd.566 0x23dda0 / snd.571 0x23ddb8 (AudioMissedLegend /
    /// AudioMissedRune: four of Sumner's lines each) and their indices
    /// missed_idx.565 / missed_idx.570 (0 in the data; never reset).
    pub missed_legend: Vec<i32>,
    pub missed_rune: Vec<i32>,
    pub missed_legend_idx: usize,
    pub missed_rune_idx: usize,
    /// AudioWeaponReflect's ricochet_time.
    pub ricochet_time: f32,
}

fn at(v: &[i32], k: i64) -> i32 {
    usize::try_from(k).ok().and_then(|k| v.get(k)).copied().unwrap_or(-1)
}

impl Game {
    /// DoWaveEndAudio 0x177848 (do_exit, after AudioPlayerExit, once
    /// per level end): outside worlds 5 and 8, a level whose quest item
    /// (QuestItem: kind 1, subtype 0xd) no player holds — PlayerHasLegendItem
    /// (−1, its +0xf4) — gets AudioMissedLegend; else a level whose rune
    /// (RuneItem: kind 1, subtype 10) no player has — PlayerHasRune(−1, its
    /// definition +0x40) — gets AudioMissedRune. Each queues the next of its
    /// four lines (AudioMissedLegend 0x161488 / AudioMissedRune 0x1614d8):
    /// AudioVQueAdd4(−1, 0.5, line, 0xe0, 0x7f, 2).
    pub(crate) fn do_wave_end_audio(&mut self) {
        if self.globals.cur_world == 5 || self.globals.cur_world == 8 {
            return;
        }
        let kind = |g: &Game, slot: usize| {
            let d = &g.items.definitions[g.items.records[slot].definition()];
            (d.kind(), d.subtype())
        };
        let line = if let Some(q) = self.items.quest_item.filter(|&q| kind(self, q) == (1, 0xd)) {
            let world = self.items.records[q].i32_(0xf4);
            if (0..4).any(|k| self.player_has_legend_item(k, world)) {
                return;
            }
            let c = &mut self.sounds.cues;
            let id = at(&c.missed_legend, c.missed_legend_idx as i64);
            c.missed_legend_idx = (c.missed_legend_idx + 1) % 4;
            id
        } else if let Some(r) = self.items.rune_item.filter(|&r| kind(self, r) == (1, 10)) {
            let rune = i32::from(self.items.definitions[self.items.records[r].definition()].i16_(0x40));
            if self.player_has_rune(-1, rune) {
                return;
            }
            let c = &mut self.sounds.cues;
            let id = at(&c.missed_rune, c.missed_rune_idx as i64);
            c.missed_rune_idx = (c.missed_rune_idx + 1) % 4;
            id
        } else {
            return;
        };
        self.vque_add(0.5, QueEntry::new(line, 0xe0, 0x7f, 2));
    }

    /// pantab 0x23ccc0[player] (0x7f for every player on this disc).
    fn pan_of(&self, p: usize) -> i32 {
        self.sounds.pantab.get(p).copied().unwrap_or(0x7f)
    }

    /// AudioMenuBurn 0x1606e8 (StartFireScroll: a scroll or option menu
    /// starts burning away): AudioFX(0x13, 0xe0, 3).
    pub fn audio_menu_burn(&mut self) {
        self.sfx_flat(0x13, 0xe0, 3);
    }

    /// AudioCursorSelect 0x1606c8: AudioFX(0x11, 0x7f, 3).
    pub fn audio_cursor_select(&mut self) {
        self.sfx_flat(0x11, 0x7f, 3);
    }

    /// AudioCursorV 0x160698 / AudioCursorH 0x1606a8 / AudioCursorChar
    /// 0x1606b8 / AudioMenuExit 0x1606d8: AudioFX(0xd / 0xe / 0xf / 0x10,
    /// 0x7f, 3).
    pub fn audio_cursor_v(&mut self) {
        self.sfx_flat(0xd, 0x7f, 3);
    }
    pub fn audio_cursor_h(&mut self) {
        self.sfx_flat(0xe, 0x7f, 3);
    }
    pub fn audio_cursor_char(&mut self) {
        self.sfx_flat(0xf, 0x7f, 3);
    }
    pub fn audio_menu_exit(&mut self) {
        self.sfx_flat(0x10, 0x7f, 3);
    }

    /// AudioClick 0x15f008(player, k): AudioSFX2(snd.277[k], pan, 0x7f, 0x7d).
    pub fn audio_click(&mut self, p: usize, k: i32) {
        let id = at(&self.sounds.cues.click, i64::from(k));
        self.sfx2(id, self.pan_of(p), 0x7f, 0x7d);
    }

    /// AudioSelect 0x15ce80(k): the select-screen music 0xc0000 (AudioFX at
    /// volMusic, priority 1) for k = 1, killed otherwise
    /// (audio_select_state; AudioLoadStage of the select bank is the
    /// host's).
    pub fn audio_select(&mut self, k: i32) {
        if self.sounds.audio_select_state != k {
            if k == 1 {
                self.sfx_flat(0xc_0000, 100, 1);
            } else {
                self.kill_sound(0xc_0000);
            }
            self.sounds.audio_select_state = k;
        }
    }

    /// AudioBuzzer 0x160688: AudioFX(10, 0x7f, 0x30).
    pub fn audio_buzzer(&mut self) {
        self.sfx_flat(10, 0x7f, 0x30);
    }

    /// AudioClick2 0x15f040(player, k): AudioSFX2(snd.281[k], pan, 0x7f, 0x7d).
    pub fn audio_click2(&mut self, p: usize, k: i32) {
        let id = at(&self.sounds.cues.click2, i64::from(k));
        self.sfx2(id, self.pan_of(p), 0x7f, 0x7d);
    }

    /// AudioTowerFX 0x161408(k): 1 → 0xe00a3, 2 → 0x16, 10 → 0xe00aa,
    /// 0xe → 0xe00ab, 0x15 → 0xe00a9; AudioSFX(id, 0, 0xff, 10).
    pub fn audio_tower_fx(&mut self, k: i32) {
        let id = match k {
            1 => 0xe_00a3,
            2 => 0x16,
            10 => 0xe_00aa,
            0xe => 0xe_00ab,
            0x15 => 0xe_00a9,
            _ => -1,
        };
        self.sfx_flat(id, 0xff, 10);
    }

    /// DoAudioTallySFX 0x15e058(on): the world's tally loop snd.149[PrevGameWorld]
    /// started (AudioLOOP, 0xe0, 0x7b) or killed.
    pub fn do_audio_tally_sfx(&mut self, on: bool, prev_world: i32) -> R<()> {
        let id = at(&self.sounds.cues.tally, i64::from(prev_world));
        if id < 0 {
            return Ok(());
        }
        let playing = self.sounds.loops.contains(&id);
        if on && !playing {
            self.loop_sound(id, None, 0xe0, 0x7b)?;
        } else if !on && playing {
            self.kill_sound(id);
        }
        Ok(())
    }

    /// AudioPickupRune 0x160af8(player): AudioSFX2(0x28, pan, 0x7f, 0x28);
    /// the music lowered for the sound's length − 1 s to half (LowerBGMusic).
    pub fn audio_pickup_rune(&mut self, p: usize) {
        self.sfx2(0x28, self.pan_of(p), 0x7f, 0x28);
        let len = self.sounds.rom.as_ref().and_then(|r| r.length_of(0x28)).unwrap_or(1.0);
        self.lower_bg_music(len - 1.0, 0.5);
    }

    /// AudioNumRunesFound 0x15e0e8(n): one → 0x10030; 2..12 → snd.153[n − 2]
    /// then 0x10031 (queued voice lines).
    pub fn audio_num_runes_found(&mut self, n: i32) {
        // AudioVQueAdd4(−1, −1, id, 0xe0, 0x7f, 2).
        if n == 1 {
            self.vque_add(-1.0, QueEntry::new(0x1_0030, 0xe0, 0x7f, 2));
        } else if (2..0xd).contains(&n) {
            let id = at(&self.sounds.cues.runes, i64::from(n - 2));
            self.vque_add(-1.0, QueEntry::new(id, 0xe0, 0x7f, 2));
            self.vque_add(-1.0, QueEntry::new(0x1_0031, 0xe0, 0x7f, 2));
        }
    }

    /// AudioEnterNextStage 0x15df80: the stage record's world sound
    /// (+0x10 → SNDS id) and +0x14, queued, when both exist.
    pub fn audio_enter_next_stage(&mut self) {
        let Some(st) = self.sounds.stage.as_ref() else { return };
        let (w, second) = (st.word_10, st.word_14);
        let Some(&(id, _, _)) = usize::try_from(w).ok().and_then(|k| self.sounds.world.get(k)) else { return };
        if 0 <= second {
            self.vque_add(-1.0, QueEntry::new(id, 0xe0, 0x7f, 2));
            self.vque_add(-1.0, QueEntry::new(second, 0xe0, 0x7f, 2));
        }
    }

    /// AudioMapDot 0x15e020: AudioFX(snd.145[CurWorld], 0xe0, 0x1e).
    pub fn audio_map_dot(&mut self) {
        let id = at(&self.sounds.cues.map_dot, i64::from(self.globals.cur_world));
        self.sfx_flat(id, 0xe0, 0x1e);
    }

    /// AudioDeathHurt 0x15d268(position): 0x65 unless playing (0x7f, 0x16).
    pub fn audio_death_hurt(&mut self, pos: V3) -> R<()> {
        self.sfx(0x65, Some(pos), 0x7f, 0x16)?;
        Ok(())
    }

    /// do_death_sound 0x15d198(position): the 0x66 loop (0x7f, 0x6f) at
    /// Death; its pan follows Death (presentation).
    pub fn do_death_sound(&mut self, pos: V3) -> R<()> {
        if !self.sounds.loops.contains(&0x66) {
            self.loop_sound(0x66, Some(pos), 0x7f, 0x6f)?;
        }
        Ok(())
    }

    /// AudioHeartBeat 0x15ed78(player): sound 0 by health (≤ 10: 0xca,
    /// < 25: 0xb1, < 100: 0x98, else 0x7f), priority 9.
    pub fn audio_heart_beat(&mut self, p: usize) {
        let h = self.players[p].f32_(0x1a10);
        let vol = if h <= 10.0 {
            0xca
        } else if h < 25.0 {
            0xb1
        } else if h < 100.0 {
            0x98
        } else {
            0x7f
        };
        self.sfx2(0, self.pan_of(p), vol, 9);
    }

    /// AudioWaitToExit 0x15df40(player): AudioWithName(5.0, −1, player,
    /// 0x30014 in the Tower, else 0x2000c).
    pub fn audio_wait_to_exit(&mut self, p: usize) {
        let m = if self.globals.cur_world == 0xd { 0x3_0014 } else { 0x2_000c };
        self.audio_with_name(5.0, -1, p, m, -1);
    }

    /// AudioSuchBravery 0x15deb0 / AudioHeroicEffort 0x15dee8(player):
    /// AudioVQueAdd4(−1, 3.0, 0x10000 / 0x10001, 0xe0, pantab[player], 2).
    pub fn audio_praise(&mut self, heroic: bool, p: i32) {
        let pan = usize::try_from(p).map_or(0x7f, |p| self.pan_of(p));
        self.vque_add(3.0, QueEntry::new(if heroic { 0x1_0001 } else { 0x1_0000 }, 0xe0, pan, 2));
    }

    /// AudioItSwap 0x15fd08(position): AudioSFX(0x3a, position, 0xb4, 0x14).
    pub fn audio_it_swap(&mut self, pos: V3) -> R<()> {
        self.sfx(0x3a, Some(pos), 0xb4, 0x14)?;
        Ok(())
    }

    /// AudioSFX(id, player +0x60, 0xe0, 0x28): AudioHorn 0x5a, AudioMask
    /// 0x5b, AudioGauntletL 0x5c, AudioGauntletR 0x5d, AudioThunderHammer
    /// 0x50; AudioPlayerHalo 0x52 (priority 0x3c).
    pub fn audio_player_sfx(&mut self, p: usize, id: i32, priority: i32) -> R<()> {
        let pos = self.players[p].v3(0x60);
        self.sfx(id, Some(pos), 0xe0, priority)?;
        Ok(())
    }

    /// AudioPlayerBreath 0x15eaf0(player): +0x138 bit 0x10 → 0x47, 0x20 →
    /// 0x48, 0x40 → 0x49 at +0x60 (0xe0, 0x28).
    pub fn audio_player_breath(&mut self, p: usize) -> R<()> {
        let f = self.players[p].u32_(0x138);
        let id = if f & 0x10 != 0 {
            0x47
        } else if f & 0x20 != 0 {
            0x48
        } else if f & 0x40 != 0 {
            0x49
        } else {
            return Ok(());
        };
        self.audio_player_sfx(p, id, 0x28)?;
        Ok(())
    }

    /// AudioTurboDefense 0x15ec00(player): AudioSFX(5, +0x70, 0x7f, 0x13).
    pub fn audio_turbo_defense(&mut self, p: usize) -> R<()> {
        let pos = self.players[p].v3(0x70);
        self.sfx(5, Some(pos), 0x7f, 0x13)?;
        Ok(())
    }

    /// AudioPlayerTurbo 0x15e980(player, kind, k): the chicken 0x5f; kind 0
    /// snd_turboA[class × 4 + k] (0x13), 1 snd_turboB[class] (0x11), 2
    /// snd_turboC[class] (0x10); at +0x50, volume 0xe0.
    pub fn audio_player_turbo(&mut self, p: usize, kind: i32, k: i32) -> R<()> {
        let pos = self.players[p].v3(0x50);
        let class = i64::from(self.players[p].u32_(8));
        if self.players[p].u32_(0x138) & 0x400 != 0 {
            self.sfx(0x5f, Some(pos), 0xe0, 0x10)?;
            return Ok(());
        }
        let (id, pri) = match kind {
            0 => (at(&self.sounds.cues.turbo_a, i64::from(k) + class * 4), 0x13),
            1 => (at(&self.sounds.cues.turbo_b, class), 0x11),
            2 => (at(&self.sounds.cues.turbo_c, class), 0x10),
            _ => return Ok(()),
        };
        self.sfx(id, Some(pos), 0xe0, pri)?;
        Ok(())
    }

    /// AudioWalk 0x15ef40(player, side, surface): AudioSFX3(snd_walk[surface
    /// + side × 5], +0x50, 0x7f, 0x7d).
    pub fn audio_walk(&mut self, p: usize, side: i32, surface: i32) -> R<()> {
        let id = at(&self.sounds.cues.walk, i64::from(surface + side * 5));
        let pos = self.players[p].v3(0x50);
        self.sfx3(id, pos, 0x7f, 0x7d)
    }

    /// AudioWeaponReflect 0x15ef88(_, position): AudioSFX3(0x41, 0x7f, 0x28)
    /// at most once a second (ricochet_time).
    pub fn audio_weapon_reflect(&mut self, pos: V3) -> R<()> {
        let t = self.globals.time;
        if self.sounds.cues.ricochet_time + 1.0 <= t {
            self.sfx3(0x41, pos, 0x7f, 0x28)?;
            self.sounds.cues.ricochet_time = t;
        }
        Ok(())
    }

    /// AudioExp 0x160b78(player, character, level): lvl_snd[character × 9 +
    /// level / 10 − 1] below 99, else 0x1003f, with the name.
    pub fn audio_exp(&mut self, p: usize, character: i32, level: i32) -> i32 {
        let id = if level < 99 { at(&self.sounds.cues.lvl_snd, i64::from(level / 10 - 1) + i64::from(character) * 9) } else { 0x1_003f };
        self.audio_with_name(5.0, -1, p, id, -1);
        id
    }

    /// AudioHasItem 0x15dc88(player, sound): AudioWithName(4.0 (1.0 for
    /// 0x1003d), −1, player, 0x20010, sound) — `move a3,a1` passes the
    /// sound as the extra line.
    pub fn audio_has_item(&mut self, p: usize, id: i32) {
        let delay = if id == 0x1_003d { 1.0 } else { 4.0 };
        self.audio_with_name(delay, -1, p, 0x2_0010, id);
    }

    /// AudioHelp 0x160c08(player, sound, _) with one active player:
    /// 0x1003d and 0x2002c → AudioHasItem, 0x1002c and 0x2000f with the
    /// name (AudioWithName delay 5), otherwise AudioVQueAdd4(−1, 0.5, sound,
    /// 0xe0, pantab[player], 2). With more players the shared item lines
    /// also go through AudioHasItem.
    pub fn audio_help(&mut self, p: usize, id: i32) {
        let multi = 1 < self.globals.num_active_players;
        let has_item = matches!(id, 0x1_003d | 0x2_002c)
            || (multi && (matches!(id, 0x25_0044 | 0x29_0042 | 0x27_0043 | 0x26_0035 | 0x28_0039 | 0x2c_0053 | 0x2b_0051 | 0x2d_0042 | 0x36_0023) || (0x2_0011..=0x2_0028).contains(&id) || (0x2_002b..=0x2_0031).contains(&id)));
        if has_item {
            self.audio_has_item(p, id);
        } else if matches!(id, 0x1_002c | 0x2_000f) {
            self.audio_with_name(5.0, -1, p, id, -1);
        } else {
            let pan = self.pan_of(p);
            self.vque_add(0.5, QueEntry::new(id, 0xe0, pan, 2));
        }
    }

    /// AudioLegendItem 0x160ed0(n, position) (jump table 0x2a90c0): 0 →
    /// 100; 1..4 → snd1–4[CurWorld − 1] (3 first kills snd2's); AudioSFX(…,
    /// 0xe0, 10).
    pub fn audio_legend_item(&mut self, n: i32, pos: Option<V3>) -> R<()> {
        let w = i64::from(self.globals.cur_world - 1);
        let id = match n {
            0 => 100,
            1 => at(&self.sounds.cues.legend[0], w),
            2 => at(&self.sounds.cues.legend[1], w),
            3 => {
                let k = at(&self.sounds.cues.legend[1], w);
                self.kill_sound(k);
                at(&self.sounds.cues.legend[2], w)
            }
            4 => at(&self.sounds.cues.legend[3], w),
            _ => return Ok(()),
        };
        self.sfx(id, pos, 0xe0, 10)?;
        Ok(())
    }

    /// AudioTransport 0x161030 / AudioWorldExplosion 0x1610a0 /
    /// AudioWorldHitPlyr 0x1610d8(position): the world's sound, AudioSFX3
    /// (0x7f, 0x44 / 0x65 / 100).
    pub fn audio_world_cue(&mut self, which: u8, pos: V3) -> R<()> {
        let w = i64::from(self.globals.cur_world);
        let (id, pri) = match which {
            0 => (at(&self.sounds.cues.transport, w), 0x44),
            1 => (at(&self.sounds.cues.world_explosion, w), 0x65),
            _ => (at(&self.sounds.cues.world_hit, w), 100),
        };
        self.sfx3(id, pos, 0x7f, pri)
    }

    /// AudioGoodWizard 0x1611d0(boss, k): gwiz_sidx[(boss − 0x22) × 8 + k]
    /// by AudioAnyQueAdd4(−1, 10.0, 1, id, 0xe0, 0x7f, 2).
    pub fn audio_good_wizard(&mut self, boss: i32, k: i32) {
        let id = at(&self.sounds.cues.gwiz, i64::from(boss - 0x22) * 8 + i64::from(k));
        self.speech_line(id);
    }

    /// AudioAnyQueAdd4(−1, 10.0, 1, id, 0xe0, 0x7f, 2): the shard, rune,
    /// crystal, gargoyle and good-wizard speeches (not held by GoodWizard).
    pub fn speech_line(&mut self, id: i32) {
        if 0 <= id {
            self.que_add(10.0, 1, QueEntry::new(id, 0xe0, 0x7f, 2));
        }
    }

    /// AudioRuneGetCloser 0x15de70 / AudioRuneIsNear 0x15de30:
    /// AudioVQueAdd4(−1, 3.0, 0x20033 / 0x20034, 0xe0, AudioAng(_), 2)
    /// (the pan's position is not passed here: centre).
    pub fn audio_rune_hint(&mut self, near: bool) {
        self.vque_add(3.0, QueEntry::new(if near { 0x2_0034 } else { 0x2_0033 }, 0xe0, 0x7f, 2));
    }
}

/// BGMusicLowerTime / BGMusicLowerAmt (LowerBGMusic); the rest of the
/// background-music state lives with BGMusicUpdate.
#[derive(Debug, Clone, Default)]
pub struct BgDuck {
    pub until: f32,
    pub amount: f32,
}

impl Game {
    /// LowerBGMusic 0x15d0d0(seconds, amount): the music at amount (≥ 0.2,
    /// ≤ 1; negative: 0) of its volume for that long.
    pub fn lower_bg_music(&mut self, seconds: f32, amount: f32) {
        self.bg_duck.until = seconds + self.globals.time;
        self.bg_duck.amount = if amount < 0.0 {
            0.0
        } else if amount < 0.2 {
            0.2
        } else {
            amount.min(1.0)
        };
    }
}
