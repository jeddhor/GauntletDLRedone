//! Background music: BGMusicStart 0x15cc50 (init_next_level), BGMusicUpdate
//! 0x15cd40 (game_main) with BGPlayStream 0x15c7a0, BGMusicStop 0x15cd00,
//! the attention areas (ProcessItems' kind-0xd music items, boss
//! activation), and the map / shop music (MapMusicStart 0x15cf00,
//! ShopMusicStart 0x15cfd0, their ends). The stream itself plays in the
//! host: `Event::Music` starts a stage stream part (looping or once), the
//! host reports `stream_playing`, `Event::MusicVolume` sets its volume.
//! volMusic (Game::vol_music) is 100 in the data until OptionsSetup's
//! AudioSetVolMusic applies GamePrefs word 1.
use super::{Event, Game};

#[derive(Debug, Clone, Default)]
pub struct BgMusic {
    /// BGMusicIdx ≥ 0 while the stage music runs.
    pub running: bool,
    pub area: i32,
    pub cur_vol: i32,
    pub part: i32,
    pub old_part: i32,
    pub loop_count: i32,
    pub attn_area: i32,
    pub attn_change_now: i32,
    /// stream_playing(): the host's report for the current part.
    pub stream_playing: bool,
    /// A map/shop bank sound playing as music (killed at its end).
    pub bank_music: Option<i32>,
}

/// LevelLetter(0) 0x162768: worlddatas[world_data_idx] +0xf (1–11 A–K, 12
/// S, 13 L; the test world's T reads G), 'A' without a world.
pub(crate) fn level_letter(cur_world: i32) -> char {
    match cur_world {
        12 => 'S',
        13 => 'L',
        w @ 1..=11 => (b'A' + (w - 1) as u8) as char,
        _ => 'A',
    }
}

impl Game {
    /// AudioSetVolMusic 0x120590: volMusic = v clamped to 0..255.
    pub fn audio_set_vol_music(&mut self, v: i32) {
        self.vol_music = v.clamp(0, 0xff);
    }

    fn level_letter(&self) -> char {
        level_letter(self.globals.cur_world)
    }

    /// BGMusicStart: area 0, part 0, the volume at volMusic, then the first
    /// part (BGPlayStream).
    pub fn bg_music_start(&mut self) {
        self.bg_music = BgMusic { running: true, cur_vol: self.vol_music, ..Default::default() };
        self.sounds.cues.ricochet_time = 0.0;
        self.bg_play_stream();
    }

    /// BGMusicStop.
    pub fn bg_music_stop(&mut self) {
        self.bg_music.running = false;
        self.bg_music.area = -1;
        self.events.push(Event::MusicStop);
    }

    /// The attention area a music item or the boss asks for.
    pub fn set_attention_area(&mut self, area: i32, change: i32) {
        self.bg_music.attn_area = area;
        self.bg_music.attn_change_now = change;
    }

    fn stage_areas(&self) -> i32 {
        self.sounds.stage.as_ref().map_or(0, |s| i32::from(s.areas))
    }

    /// BGPlayStream: (re)start the stream for the attention area once the
    /// change may happen (now, or after the fade to ≤ 3); multi-part areas
    /// play their parts in turn, the last one looping.
    fn bg_play_stream(&mut self) {
        let areas = self.stage_areas();
        let m = &mut self.bg_music;
        if m.stream_playing {
            if m.attn_area == m.area || m.attn_change_now < 1 || (m.attn_change_now < 2 && 3 < m.cur_vol) {
                return;
            }
            if areas < 2 {
                m.attn_area = 0;
                m.attn_change_now = 0;
                return;
            }
        }
        let Some(st) = self.sounds.stage.clone() else { return };
        let m = &mut self.bg_music;
        let mut a = m.attn_area.max(0);
        if areas <= a {
            a = areas - 1;
        }
        if a == m.area {
            if m.part == m.old_part {
                m.loop_count += 1;
            } else {
                m.loop_count = 0;
            }
        } else {
            m.part = 0;
            m.loop_count = 0;
        }
        m.area = a;
        m.old_part = m.part;
        let parts = i32::from(st.parts.get(a.max(0) as usize).copied().unwrap_or(1));
        let last = m.part + 1 == parts || parts < 2;
        let name = st.stream_name(a.max(0) as usize, m.part.max(0) as usize);
        m.attn_change_now = 0;
        // AudioStreamPlay(volume × LEVL +0x94, loop): only the last part loops.
        let volume = (m.cur_vol as f32 * self.level_f32(0x94)) as i32;
        self.bg_music.stream_playing = true;
        self.events.push(Event::Music { name, volume, looping: last });
    }

    /// PlayStreamComplete 0x120f88 (a part played once ended): the next
    /// part; the host calls it.
    pub fn bg_stream_complete(&mut self) {
        self.bg_music.part += 1;
        self.bg_music.stream_playing = false;
    }

    /// BGMusicUpdate: BGPlayStream, then the volume eases (8 per frame)
    /// towards the target: down to 3 before an area change, ducked by
    /// LowerBGMusic, else volMusic.
    pub fn bg_music_update(&mut self) {
        if !self.bg_music.running || self.sounds.rom.is_none() {
            return;
        }
        self.bg_play_stream();
        let areas = self.stage_areas();
        let m = &self.bg_music;
        let target = if m.attn_change_now == 1 && m.attn_area != m.area && 1 < areas {
            if 3 < m.cur_vol { m.cur_vol - 3 } else { m.cur_vol }
        } else if self.globals.time < self.bg_duck.until {
            (self.vol_music as f32 * self.bg_duck.amount) as i32
        } else {
            self.vol_music
        };
        let cur = m.cur_vol;
        if target != cur {
            let next = cur + (target - cur).clamp(-8, 8);
            self.bg_music.cur_vol = next;
            let volume = (next as f32 * self.level_f32(0x94)) as i32;
            self.events.push(Event::MusicVolume { volume });
        }
    }

    fn bank_music_start(&mut self, bank: &str, sound: &str) {
        if let Some(id) = self.bg_music.bank_music.take() {
            self.kill_sound(id);
        }
        self.kill_sound(0xc_0000);
        self.bg_music_stop();
        let _ = bank; // AudioLoadStage(bank): the host loads banks on demand.
        let id = self.sounds.rom.as_ref().and_then(|r| r.find(sound));
        if let Some(id) = id {
            self.sfx_flat(id, self.vol_music, 1);
            self.bg_music.bank_music = Some(id);
        }
    }

    /// MapMusicStart: outside the secret world and the Tower, bank MAP_<L>
    /// sound S_MAP_<L> as the music.
    pub fn map_music_start(&mut self) {
        let l = self.level_letter();
        if l != 'S' && l != 'L' {
            self.bank_music_start(&format!("MAP_{l}"), &format!("S_MAP_{l}"));
        }
    }

    /// ShopMusicStart: bank SHOP_<L> (letters past K use A) sound S_SHOP_<L>.
    pub fn shop_music_start(&mut self) {
        let mut l = self.level_letter();
        if 'K' < l {
            l = 'A';
        }
        self.bank_music_start(&format!("SHOP_{l}"), &format!("S_SHOP_{l}"));
    }

    /// MapMusicEnd / ShopMusicEnd: the music tracks killed.
    pub fn bank_music_end(&mut self) {
        if let Some(id) = self.bg_music.bank_music.take() {
            self.kill_sound(id);
        }
    }
}
