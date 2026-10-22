//! The Audio* cues that were emitted as named placeholders, transcribed to
//! their sound requests (each function's EE address in its doc). Tables:
//! snd_turboA.215 0x23d0e0, snd_turboB.216 0x23d160, snd_turboC.217
//! 0x23d180, snd_walk.267 0x23d3f8, snd.281 0x23d428 (Click2), snd.145
//! 0x23ce30 (map dots), snd.149 0x23ce68 (tally loops), snd.153 0x23cea0
//! (rune counts), snd.524/531/535 0x23dac8/0x23db00/0x23db38 (transport,
//! world explosion, world hit), lvl_snd 0x23d7c8 (16 characters × 9),
//! gwiz_sidx 0x23dbe0 (11 bosses × 8), snd1–4.517–520 0x23da08.. (legend
//! items by world).
use super::{Game, R};

type V3 = [f32; 3];

#[derive(Debug, Clone, Default)]
pub struct CueTables {
    pub turbo_a: Vec<i32>,
    pub turbo_b: Vec<i32>,
    pub turbo_c: Vec<i32>,
    pub walk: Vec<i32>,
    pub click2: Vec<i32>,
    pub map_dot: Vec<i32>,
    pub tally: Vec<i32>,
    pub runes: Vec<i32>,
    pub transport: Vec<i32>,
    pub world_explosion: Vec<i32>,
    pub world_hit: Vec<i32>,
    pub lvl_snd: Vec<i32>,
    pub gwiz: Vec<i32>,
    pub legend: [Vec<i32>; 4],
    /// AudioWeaponReflect's ricochet_time.
    pub ricochet_time: f32,
}

fn at(v: &[i32], k: i64) -> i32 {
    usize::try_from(k).ok().and_then(|k| v.get(k)).copied().unwrap_or(-1)
}

impl Game {
    fn pan_of(&self, p: usize) -> i32 {
        // pantab by player (presentation pan; Event::Sound has no pan field
        // for SFX2: the client pans by position or centres).
        p as i32
    }

    /// AudioCursorSelect 0x1606c8: AudioFX(0x11, 0x7f, 3).
    pub fn audio_cursor_select(&mut self) {
        self.sfx(0x11, None, 0x7f, 3);
    }

    /// AudioBuzzer 0x160688: AudioFX(10, 0x7f, 0x30).
    pub fn audio_buzzer(&mut self) {
        self.sfx(10, None, 0x7f, 0x30);
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
        self.sfx(id, None, 0xff, 10);
    }

    /// DoAudioTallySFX 0x15e058(on): the world's tally loop snd.149[PrevGameWorld]
    /// started (AudioLOOP, 0xe0, 0x7b) or killed.
    pub fn do_audio_tally_sfx(&mut self, on: bool, prev_world: i32) {
        let id = at(&self.sounds.cues.tally, i64::from(prev_world));
        if id < 0 {
            return;
        }
        let playing = self.sounds.loops.contains(&id);
        if on && !playing {
            self.loop_sound(id, None, 0xe0, 0x7b);
        } else if !on && playing {
            self.kill_sound(id);
        }
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
        if n == 1 {
            self.queue(0x1_0030, 0xe0, 2);
        } else if (2..0xd).contains(&n) {
            let id = at(&self.sounds.cues.runes, i64::from(n - 2));
            self.queue(id, 0xe0, 2);
            self.queue(0x1_0031, 0xe0, 2);
        }
    }

    /// AudioEnterNextStage 0x15df80: the stage record's world sound
    /// (+0x10 → SNDS id) and +0x14, queued, when both exist.
    pub fn audio_enter_next_stage(&mut self) {
        let Some(st) = self.sounds.stage.as_ref() else { return };
        let (w, second) = (st.word_10, st.word_14);
        let Some(&(id, _, _)) = usize::try_from(w).ok().and_then(|k| self.sounds.world.get(k)) else { return };
        if 0 <= second {
            self.queue(id, 0xe0, 2);
            self.queue(second, 0xe0, 2);
        }
    }

    /// AudioMapDot 0x15e020: AudioFX(snd.145[CurWorld], 0xe0, 0x1e).
    pub fn audio_map_dot(&mut self) {
        let id = at(&self.sounds.cues.map_dot, i64::from(self.globals.cur_world));
        self.sfx(id, None, 0xe0, 0x1e);
    }

    /// AudioDeathHurt 0x15d268(position): 0x65 unless playing (0x7f, 0x16).
    pub fn audio_death_hurt(&mut self, pos: V3) {
        self.sfx(0x65, Some(pos), 0x7f, 0x16);
    }

    /// do_death_sound 0x15d198(position): the 0x66 loop (0x7f, 0x6f) at
    /// Death; its pan follows Death (presentation).
    pub fn do_death_sound(&mut self, pos: V3) {
        if !self.sounds.loops.contains(&0x66) {
            self.loop_sound(0x66, Some(pos), 0x7f, 0x6f);
        }
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

    /// AudioWaitToExit 0x15df40(player): AudioWithName(0x30014 in the
    /// Tower, else 0x2000c).
    pub fn audio_wait_to_exit(&mut self, p: usize) {
        let m = if self.globals.cur_world == 0xd { 0x3_0014 } else { 0x2_000c };
        self.audio_with_name(-1, p, m, -1);
    }

    /// AudioSuchBravery 0x15deb0 / AudioHeroicEffort 0x15dee8: 0x10000 /
    /// 0x10001 queued.
    pub fn audio_praise(&mut self, heroic: bool) {
        self.queue(if heroic { 0x1_0001 } else { 0x1_0000 }, 0xe0, 2);
    }

    /// AudioItSwap 0x15fd08(position): AudioSFX(0x3a, position, 0xb4, 0x14).
    pub fn audio_it_swap(&mut self, pos: V3) {
        self.sfx(0x3a, Some(pos), 0xb4, 0x14);
    }

    /// AudioSFX(id, player +0x60, 0xe0, 0x28): AudioHorn 0x5a, AudioMask
    /// 0x5b, AudioGauntletL 0x5c, AudioGauntletR 0x5d, AudioThunderHammer
    /// 0x50; AudioPlayerHalo 0x52 (priority 0x3c).
    pub fn audio_player_sfx(&mut self, p: usize, id: i32, priority: i32) {
        let pos = self.players[p].v3(0x60);
        self.sfx(id, Some(pos), 0xe0, priority);
    }

    /// AudioPlayerBreath 0x15eaf0(player): +0x138 bit 0x10 → 0x47, 0x20 →
    /// 0x48, 0x40 → 0x49 at +0x60 (0xe0, 0x28).
    pub fn audio_player_breath(&mut self, p: usize) {
        let f = self.players[p].u32_(0x138);
        let id = if f & 0x10 != 0 {
            0x47
        } else if f & 0x20 != 0 {
            0x48
        } else if f & 0x40 != 0 {
            0x49
        } else {
            return;
        };
        self.audio_player_sfx(p, id, 0x28);
    }

    /// AudioTurboDefense 0x15ec00(player): AudioSFX(5, +0x70, 0x7f, 0x13).
    pub fn audio_turbo_defense(&mut self, p: usize) {
        let pos = self.players[p].v3(0x70);
        self.sfx(5, Some(pos), 0x7f, 0x13);
    }

    /// AudioPlayerTurbo 0x15e980(player, kind, k): the chicken 0x5f; kind 0
    /// snd_turboA[class × 4 + k] (0x13), 1 snd_turboB[class] (0x11), 2
    /// snd_turboC[class] (0x10); at +0x50, volume 0xe0.
    pub fn audio_player_turbo(&mut self, p: usize, kind: i32, k: i32) {
        let pos = self.players[p].v3(0x50);
        let class = i64::from(self.players[p].u32_(8));
        if self.players[p].u32_(0x138) & 0x400 != 0 {
            self.sfx(0x5f, Some(pos), 0xe0, 0x10);
            return;
        }
        let (id, pri) = match kind {
            0 => (at(&self.sounds.cues.turbo_a, i64::from(k) + class * 4), 0x13),
            1 => (at(&self.sounds.cues.turbo_b, class), 0x11),
            2 => (at(&self.sounds.cues.turbo_c, class), 0x10),
            _ => return,
        };
        self.sfx(id, Some(pos), 0xe0, pri);
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
    pub fn audio_exp(&mut self, p: usize, character: i32, level: i32) {
        let id = if level < 99 { at(&self.sounds.cues.lvl_snd, i64::from(level / 10 - 1) + i64::from(character) * 9) } else { 0x1_003f };
        self.audio_with_name(-1, p, id, -1);
    }

    /// AudioHelp 0x160c08(player, sound, _) with one active player:
    /// 0x1003d and 0x2002c → AudioHasItem (AudioWithName 0x20010), 0x1002c
    /// and 0x2000f with the name, otherwise the line queued. With more
    /// players the shared item lines also go through AudioHasItem.
    pub fn audio_help(&mut self, p: usize, id: i32) {
        let multi = 1 < self.globals.num_active_players;
        let has_item = matches!(id, 0x1_003d | 0x2_002c)
            || (multi && (matches!(id, 0x25_0044 | 0x29_0042 | 0x27_0043 | 0x26_0035 | 0x28_0039 | 0x2c_0053 | 0x2b_0051 | 0x2d_0042 | 0x36_0023) || (0x2_0011..=0x2_0028).contains(&id) || (0x2_002b..=0x2_0031).contains(&id)));
        if has_item {
            self.audio_with_name(-1, p, 0x2_0010, -1);
        } else if matches!(id, 0x1_002c | 0x2_000f) {
            self.audio_with_name(-1, p, id, -1);
        } else {
            self.queue(id, 0xe0, 2);
        }
    }

    /// AudioLegendItem 0x160ed0(n, position) (jump table 0x2a90c0): 0 →
    /// 100; 1..4 → snd1–4[CurWorld − 1] (3 first kills snd2's); AudioSFX(…,
    /// 0xe0, 10).
    pub fn audio_legend_item(&mut self, n: i32, pos: Option<V3>) {
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
            _ => return,
        };
        self.sfx(id, pos, 0xe0, 10);
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
    /// queued (AudioAnyQueAdd4, delay 10).
    pub fn audio_good_wizard(&mut self, boss: i32, k: i32) {
        let id = at(&self.sounds.cues.gwiz, i64::from(boss - 0x22) * 8 + i64::from(k));
        self.queue(id, 0xe0, 2);
    }

    /// AudioRuneGetCloser 0x15de70 / AudioRuneIsNear 0x15de30: 0x20033 /
    /// 0x20034 queued.
    pub fn audio_rune_hint(&mut self, near: bool) {
        self.queue(if near { 0x2_0034 } else { 0x2_0033 }, 0xe0, 2);
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
