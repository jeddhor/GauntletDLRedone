//! Native audio output for the playtest: a small mixer on an ALSA PCM
//! (Linux; the Windows backend belongs to M20 platform work).
//!
//! Sound IDs (bank << 16 | call) resolve through AUDATPS2.ROM to a VBK bank;
//! a call plays its sample chain in order (0x4000/0x2000 loop markers loop
//! the chain) at the VAG sample rate, as DCS.IRX voicePlayNextSample does.
//! Requests pass AudioTrack4's EE step (volume × volSFX >> 8, the pan word,
//! mono_flag's 0x7f) into the ported DCS voice model (gdl_core::dcs:
//! BankPlay, voiceFind's 12 voices, ducking, VoiceVolume/VoicePan ramps,
//! voiceSetVolPan's SPU volumes with the pan table read from IRX/DCS.IRX);
//! each voice's output is its VOLL/VOLR word / 0x4000. Music: the decoded
//! ADS stream, AdsSetVolume's per-channel word ((v × 0x1fff) >> 8) / 0x4000
//! for AudioStreamSetVolume(v). Not modelled (presentation): the SPU's
//! Gaussian resampling and its mix saturation (DCS sets an on/off envelope,
//! no reverb, and its pitch skew is dormant: research/audio.md).
use gdl_core::dcs::{Dcs, PanTable};
use gdl_formats::audio::{Ads, AudioRom, Vbk};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const OUT_RATE: u32 = 48_000;

/// A parsed bank with its decoded samples.
type LoadedBank = (Vbk, Vec<Arc<Vec<i16>>>);

struct Voice {
    id: i32,
    chain: Vec<Arc<Vec<i16>>>,
    rates: Vec<u32>,
    loop_from: Option<usize>,
    loops: bool,
    part: usize,
    pos: f64,
    /// The DCS voice (0..11) it plays on.
    slot: usize,
    /// An owner's voice (DoItemSound's per-item track): updated in place.
    tag: Option<u32>,
}

/// The music channel: a stage stream (AudioStreamSetVolume word) or a
/// movie's soundtrack.
struct Music {
    pcm: Arc<Vec<i16>>,
    rate: u32,
    channels: u32,
    pos: f64,
    /// The stage music loops (its last part) or plays once.
    loops: bool,
    /// Per-channel amplitude.
    gain: f32,
}

struct Mixer {
    voices: Vec<Voice>,
    dcs: Dcs,
    pan_table: PanTable,
    music: Option<Music>,
    /// AudioStreamSetVolume's last value.
    music_vol: i32,
    /// volSFX 0x21f6c4 (AudioSetVolSfx: GamePrefs word 0, 0..255; the
    /// ELF's data holds 128).
    vol_sfx: i32,
    /// mono_flag 0x21f6f4 (AudioSetStereo: GamePrefs word 2 == 0).
    mono: bool,
    out_rate: u32,
}

/// AdsSetVolume's SPU word for AudioStreamSetVolume(v) (the RPC's
/// (v & 0xffff) × 0x1fff >> 8) as an amplitude.
fn stream_gain(v: i32) -> f32 {
    (((v & 0xffff) * 0x1fff) >> 8) as f32 / 16384.0
}

impl Mixer {
    fn new(pan_table: PanTable) -> Self {
        Self { voices: Vec::new(), dcs: Dcs::default(), pan_table, music: None, music_vol: 0, vol_sfx: 128, mono: false, out_rate: OUT_RATE }
    }

    fn render(&mut self, out: &mut [f32], channels: usize) {
        out.fill(0.0);
        let frames = out.len() / channels.max(1);
        let out_rate = f64::from(self.out_rate.max(1));
        if let Some(m) = self.music.as_mut() {
            let ch = m.channels.max(1) as usize;
            let n = m.pcm.len() / ch;
            let step = f64::from(m.rate) / out_rate;
            // AdsSetVolume: stereo — each channel on its own side; mono
            // output mode — both sides, each channel × 181/256.
            let (g, cross) = if self.mono { (m.gain * 181.0 / 256.0, true) } else { (m.gain, false) };
            for f in 0..frames {
                let i = (m.pos as usize) % n.max(1);
                let l = f32::from(m.pcm[i * ch]) / 32768.0;
                let r = f32::from(m.pcm[i * ch + ch - 1]) / 32768.0;
                let (ol, or) = if cross { ((l + r) * g, (l + r) * g) } else { (l * g, r * g) };
                out[f * channels] += ol;
                if 1 < channels {
                    out[f * channels + 1] += or;
                }
                m.pos += step;
                if n as f64 <= m.pos {
                    if !m.loops {
                        // PlayStreamComplete: the part ended.
                        m.pos = n as f64;
                        break;
                    }
                    m.pos -= n as f64;
                }
            }
            if !m.loops && n as f64 <= m.pos {
                self.music = None;
            }
        }
        let mut ended = Vec::new();
        for (k, v) in self.voices.iter_mut().enumerate() {
            let [l, r] = self.dcs.vol_pan(v.slot, &self.pan_table);
            let gain = [f32::from(l) / 16384.0, f32::from(r) / 16384.0];
            let mut f = 0;
            while f < frames && v.part < v.chain.len() {
                let pcm = &v.chain[v.part];
                let step = f64::from(v.rates[v.part]) / out_rate;
                let i = v.pos as usize;
                if pcm.len() <= i {
                    v.part += 1;
                    v.pos = 0.0;
                    if v.part == v.chain.len()
                        && let Some(start) = v.loop_from.filter(|_| v.loops)
                    {
                        v.part = start;
                    }
                    continue;
                }
                let s = f32::from(pcm[i]) / 32768.0;
                out[f * channels] += s * gain[0];
                if 1 < channels {
                    out[f * channels + 1] += s * gain[1];
                }
                v.pos += step;
                f += 1;
            }
            if v.chain.len() <= v.part {
                ended.push(k);
            }
        }
        for &k in ended.iter().rev() {
            // voicePlayNextSample with no next sample: the duck undone; the
            // envelope then reaches zero.
            let slot = self.voices.remove(k).slot;
            self.dcs.chain_end(slot);
            self.dcs.silent(slot);
        }
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
        }
    }

    /// VoiceStop for the voices `keep` rejects.
    fn stop_where(&mut self, stop: impl Fn(&Voice) -> bool) {
        let mut mask = 0u32;
        self.voices.retain(|v| {
            let s = stop(v);
            if s {
                mask |= 1 << v.slot;
            }
            !s
        });
        if mask != 0 {
            self.dcs.stop(mask);
        }
    }
}

/// Bank/sample cache and the output stream.
pub struct AudioOut {
    mixer: Arc<Mutex<Mixer>>,
    rom: AudioRom,
    bank_paths: HashMap<String, PathBuf>,
    stream_paths: HashMap<String, PathBuf>,
    banks: HashMap<usize, Option<LoadedBank>>,
    _stream: Option<std::thread::JoinHandle<()>>,
    pub status: String,
}

impl AudioOut {
    /// `dcs`: IRX/DCS.IRX's bytes (its pan table); without them the pan
    /// curve is a computed quarter sine (labelled in `status`).
    pub fn new(rom: AudioRom, bank_paths: HashMap<String, PathBuf>, stream_paths: HashMap<String, PathBuf>, dcs: Option<&[u8]>) -> Self {
        let (table, note) = match dcs.map(PanTable::from_irx) {
            Some(Ok(t)) => (t, ""),
            Some(Err(_)) | None => {
                let mut t = [0u16; 256];
                for (k, v) in t.iter_mut().enumerate() {
                    *v = (16384.0 * (std::f64::consts::FRAC_PI_2 * k as f64 / 256.0).sin()) as u16;
                }
                (PanTable(t), "; DCS pan table unavailable: APPROXIMATE pan curve")
            }
        };
        let mixer = Arc::new(Mutex::new(Mixer::new(table)));
        let mut status = String::from("audio: no output device");
        let silent = std::env::var_os("GDL_NO_AUDIO").is_some();
        if silent {
            status = "audio: disabled (GDL_NO_AUDIO)".into();
        }
        let stream = if silent { None } else { start_alsa(mixer.clone()) };
        if stream.is_some() {
            status = "audio: on (ALSA)".into();
        }
        status.push_str(note);
        Self { mixer, rom, bank_paths, stream_paths, banks: HashMap::new(), _stream: stream, status }
    }

    fn bank(&mut self, b: usize) -> Option<&LoadedBank> {
        if !self.banks.contains_key(&b) {
            let loaded = (|| {
                let name = self.rom.banks.get(b)?.name.to_uppercase();
                let path = self.bank_paths.get(&name)?;
                let bytes = std::fs::read(path).ok()?;
                let vbk = Vbk::parse(&bytes).ok()?;
                let pcm = (0..vbk.samples.len()).map(|k| Arc::new(vbk.decode(&bytes, k).unwrap_or_default())).collect();
                Some((vbk, pcm))
            })();
            self.banks.insert(b, loaded);
        }
        self.banks.get(&b).and_then(Option::as_ref)
    }

    /// AudioTrack4 (AudioSFX/SFX2/SFX3, the queue): sound `id` with the
    /// request volume, pan word (AudioAng or a fixed pan) and priority.
    pub fn play(&mut self, id: i32, volume: i32, pan: i32, priority: i32) {
        self.play_ex(id, volume, pan, priority, false);
    }

    /// AudioLOOP: the call's chain repeats until stopped.
    pub fn play_loop(&mut self, id: i32, volume: i32, pan: i32, priority: i32) {
        self.play_ex(id, volume, pan, priority, true);
    }

    /// DoItemSound: the owner's looping voice started (AudioLOOP), else
    /// AudioTrackVol (volume × volSFX >> 8 → VoiceVolume's ramp) and
    /// AudioTrackPan (→ VoicePan's ramp; 0x7f when mono).
    pub fn play_tagged(&mut self, tag: u32, id: i32, volume: i32, pan: i32, priority: i32) {
        if let Ok(mut m) = self.mixer.lock()
            && let Some(slot) = m.voices.iter().find(|v| v.tag == Some(tag) && v.id == id).map(|v| v.slot)
        {
            let vol = volume.wrapping_mul(m.vol_sfx) >> 8;
            let pan = if m.mono { 0x7f } else { pan };
            m.dcs.voice_volume(1 << slot, vol & 0xffff);
            m.dcs.voice_pan(1 << slot, pan);
            return;
        }
        if let Some(k) = self.play_ex(id, volume, pan, priority, true)
            && let Ok(mut m) = self.mixer.lock()
            && let Some(v) = m.voices.iter_mut().find(|v| v.slot == k)
        {
            v.tag = Some(tag);
        }
    }

    /// KillItemSound for an owner.
    pub fn stop_tagged(&mut self, tag: u32) {
        if let Ok(mut m) = self.mixer.lock() {
            m.stop_where(|v| v.tag == Some(tag));
        }
    }

    /// AudioTrack4's request (volume × volSFX >> 8 << 16 | pan, 0x7f when
    /// mono) into BankPlay; the DCS voice, or None when voiceFind drops it.
    fn play_ex(&mut self, id: i32, volume: i32, pan: i32, priority: i32, force_loop: bool) -> Option<usize> {
        let (b, call) = AudioRom::split(id);
        let (vbk, pcm) = self.bank(b)?;
        let c = vbk.calls.get(call)?;
        let mut chain = Vec::new();
        let mut rates = Vec::new();
        let mut loop_from = None;
        let mut loops = false;
        for &w in &c.words {
            let s = (w & 0xfff) as usize;
            if w & 0x4000 != 0 {
                loop_from = Some(chain.len());
            }
            if w & 0x2000 != 0 {
                loops = true;
            }
            if let (Some(p), Some(meta)) = (pcm.get(s), vbk.samples.get(s)) {
                chain.push(p.clone());
                rates.push(meta.rate);
            }
        }
        if chain.is_empty() {
            return None;
        }
        let (cv, cd, cp) = (c.volume, c.duck, c.priority);
        let mut m = self.mixer.lock().ok()?;
        let vol = volume.wrapping_mul(m.vol_sfx) >> 8;
        let pan = if m.mono { 0x7f } else { pan };
        let request = ((vol as u32) << 16) | (pan as u32 & 0xffff);
        let slot = m.dcs.bank_play(request, priority as u32, cv, cd, cp)?;
        // A stomped voice's sound ends here.
        m.voices.retain(|v| v.slot != slot);
        m.voices.push(Voice { id, chain, rates, loop_from: loop_from.or(Some(0)), loops: loops | force_loop, part: 0, pos: 0.0, slot, tag: None });
        Some(slot)
    }

    /// AudioSoundKill / StopSound: VoiceStop on the ID's voices.
    pub fn stop(&mut self, id: i32) {
        if let Ok(mut m) = self.mixer.lock() {
            m.stop_where(|v| v.id == id);
        }
    }

    /// aud_stop_all_sounds (AudioStopAll): every SFX voice; the music
    /// channel goes on.
    pub fn stop_voices(&mut self) {
        if let Ok(mut m) = self.mixer.lock() {
            m.stop_where(|_| true);
        }
    }

    pub fn stop_all(&mut self) {
        if let Ok(mut m) = self.mixer.lock() {
            m.stop_where(|_| true);
            m.music = None;
        }
    }

    /// AudioSetVolSfx (GamePrefs word 0, clamped 0..255) and AudioSetStereo
    /// (word 2; mono when 0). volMusic belongs to the simulation
    /// (Game::audio_set_vol_music).
    pub fn set_options(&mut self, sfx: i32, stereo: i32) {
        if let Ok(mut m) = self.mixer.lock() {
            m.vol_sfx = sfx.clamp(0, 0xff);
            m.mono = stereo == 0;
        }
    }

    /// AudioStreamPlay: a STREAMS/<name> part, looping or once, at
    /// AudioStreamSetVolume(volume). False when the file is missing.
    pub fn stream(&mut self, name: &str, looping: bool, volume: i32) -> bool {
        let Some(path) = self.stream_paths.get(&name.to_uppercase()) else { return false };
        let Ok(bytes) = std::fs::read(path) else { return false };
        let Ok(ads) = Ads::parse(&bytes) else { return false };
        let pcm = Arc::new(ads.decode(&bytes));
        if let Ok(mut m) = self.mixer.lock() {
            m.music_vol = volume;
            m.music = Some(Music { pcm, rate: ads.rate, channels: ads.channels, pos: 0.0, loops: looping, gain: stream_gain(volume) });
        }
        true
    }

    /// A movie's soundtrack (the PSS private stream) on the music channel,
    /// once. Its level (audioDec* reads volMusic) is not ported: 0.5,
    /// APPROXIMATE.
    pub fn play_pcm(&mut self, pcm: Vec<i16>, rate: u32, channels: u32) {
        if let Ok(mut m) = self.mixer.lock() {
            m.music = Some(Music { pcm: Arc::new(pcm), rate, channels, pos: 0.0, loops: false, gain: 0.5 });
        }
    }

    /// stream_playing(): the stage music still sounds.
    pub fn stream_playing(&self) -> bool {
        self.mixer.lock().is_ok_and(|m| m.music.is_some())
    }

    /// AudioStreamSetVolume.
    pub fn stream_volume(&mut self, volume: i32) {
        if let Ok(mut m) = self.mixer.lock() {
            m.music_vol = volume;
            if let Some(music) = m.music.as_mut() {
                music.gain = stream_gain(volume);
            }
        }
    }

    /// AudioStreamStop.
    pub fn stream_stop(&mut self) {
        if let Ok(mut m) = self.mixer.lock() {
            m.music = None;
        }
    }
}

/// Open the default ALSA device (S16 stereo, 48 kHz) and feed it from the
/// mixer on a background thread. Returns None when no device is available.
fn start_alsa(mixer: Arc<Mutex<Mixer>>) -> Option<std::thread::JoinHandle<()>> {
    use alsa::pcm::{Access, Format, HwParams, PCM};
    let pcm = PCM::new("default", alsa::Direction::Playback, false).ok()?;
    {
        let hw = HwParams::any(&pcm).ok()?;
        hw.set_channels(2).ok()?;
        hw.set_rate(OUT_RATE, alsa::ValueOr::Nearest).ok()?;
        hw.set_format(Format::s16()).ok()?;
        hw.set_access(Access::RWInterleaved).ok()?;
        hw.set_buffer_size_near(4096).ok()?;
        pcm.hw_params(&hw).ok()?;
    }
    let rate = pcm.hw_params_current().ok()?.get_rate().ok()?;
    if let Ok(mut m) = mixer.lock() {
        m.out_rate = rate;
    }
    std::thread::Builder::new()
        .name("gdl-audio".into())
        .spawn(move || {
            let Ok(io) = pcm.io_i16() else { return };
            let mut f = vec![0f32; 1024 * 2];
            let mut out = vec![0i16; 1024 * 2];
            loop {
                if let Ok(mut m) = mixer.lock() {
                    m.render(&mut f, 2);
                }
                for (o, s) in out.iter_mut().zip(&f) {
                    *o = (*s * 32767.0) as i16;
                }
                if io.writei(&out).is_err() {
                    let _ = pcm.prepare();
                }
            }
        })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> PanTable {
        let mut t = [0u16; 256];
        for (k, v) in t.iter_mut().enumerate() {
            *v = (16384.0 * (std::f64::consts::FRAC_PI_2 * k as f64 / 256.0).sin()) as u16;
        }
        PanTable(t)
    }

    /// A voice on DCS slot `slot` at full volume, pan 0 (left only).
    fn voice(m: &mut Mixer, pcm: Vec<i16>, rate: u32, loops: bool) -> usize {
        let slot = m.dcs.bank_play(0xff << 16, 1, 0x7f, 0, 0).unwrap();
        m.voices.push(Voice { id: 1, chain: vec![Arc::new(pcm)], rates: vec![rate], loop_from: Some(0), loops, part: 0, pos: 0.0, slot, tag: None });
        slot
    }

    #[test]
    fn mixer_plays_a_chain_once_and_resamples() {
        // Synthetic: 4 samples at 24 kHz into a 48 kHz output → 8 frames.
        let mut m = Mixer::new(table());
        let slot = voice(&mut m, vec![16384; 4], 24_000, false);
        let [l, r] = m.dcs.vol_pan(slot, &m.pan_table);
        let mut out = vec![0f32; 2 * 10];
        m.render(&mut out, 2);
        assert_eq!(out[0], 0.5 * f32::from(l) / 16384.0);
        assert_eq!((r, out[1]), (0, 0.0));
        assert_eq!(out[14], out[0]);
        assert_eq!(out[16], 0.0);
        assert!(m.voices.is_empty());
        assert!(!m.dcs.in_use(slot), "the DCS voice is free again");
    }

    #[test]
    fn loops_persist_after_a_one_shot_ends() {
        let mut m = Mixer::new(table());
        voice(&mut m, vec![100; 2], 48_000, false);
        voice(&mut m, vec![1; 2], 48_000, true);
        let mut out = vec![0f32; 2 * 4];
        m.render(&mut out, 2);
        assert_eq!(m.voices.len(), 1); // the one-shot finished; the loop persists
    }

    #[test]
    fn stream_gain_follows_ads_set_volume() {
        // (100 × 0x1fff) >> 8 = 3199.
        assert_eq!(stream_gain(100), 3199.0 / 16384.0);
    }
}
