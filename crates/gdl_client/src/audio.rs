//! Native audio output for the playtest: a small mixer on an ALSA PCM
//! (Linux; the Windows backend belongs to M20 platform work).
//!
//! Sound IDs (bank << 16 | call) resolve through AUDATPS2.ROM to a VBK bank;
//! a call plays its sample chain in order (0x4000/0x2000 loop markers loop
//! the chain) at the VAG sample rate, as DCS.IRX voicePlayNextSample does.
//! Volume = request volume × call volume / 0x7f (DCS BankPlay), scaled by
//! 1/256; pan, ducking, priority-based voice stealing and the SPU envelope
//! are approximations (labelled presentation). Music: a decoded ADS stream
//! looping at its own rate.
use gdl_formats::audio::{Ads, AudioRom, Vbk};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const MAX_VOICES: usize = 24;
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
    gain: [f32; 2],
    priority: i32,
    queued: bool,
}

#[derive(Default)]
struct Mixer {
    voices: Vec<Voice>,
    queue: Vec<Voice>,
    music: Option<(Arc<Vec<i16>>, u32, u32, f64)>,
    music_gain: f32,
    /// Settings → Audio sound volume (GamePrefs word 0) / 128.
    sfx_gain: f32,
    out_rate: u32,
}

impl Mixer {
    fn render(&mut self, out: &mut [f32], channels: usize) {
        out.fill(0.0);
        let frames = out.len() / channels.max(1);
        if !self.voices.iter().any(|v| v.queued)
            && let Some(next) = (!self.queue.is_empty()).then(|| self.queue.remove(0))
        {
            self.voices.push(next);
        }
        let out_rate = f64::from(self.out_rate.max(1));
        if let Some((pcm, rate, ch, pos)) = self.music.as_mut() {
            let ch = (*ch).max(1) as usize;
            let n = pcm.len() / ch;
            let step = f64::from(*rate) / out_rate;
            for f in 0..frames {
                let i = (*pos as usize) % n.max(1);
                let l = f32::from(pcm[i * ch]) / 32768.0;
                let r = f32::from(pcm[i * ch + ch - 1]) / 32768.0;
                out[f * channels] += l * self.music_gain;
                if 1 < channels {
                    out[f * channels + 1] += r * self.music_gain;
                }
                *pos += step;
                if n as f64 <= *pos {
                    *pos -= n as f64;
                }
            }
        }
        for v in &mut self.voices {
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
                out[f * channels] += s * v.gain[0] * self.sfx_gain;
                if 1 < channels {
                    out[f * channels + 1] += s * v.gain[1] * self.sfx_gain;
                }
                v.pos += step;
                f += 1;
            }
        }
        self.voices.retain(|v| v.part < v.chain.len());
        for s in out.iter_mut() {
            *s = s.clamp(-1.0, 1.0);
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
    pub fn new(rom: AudioRom, bank_paths: HashMap<String, PathBuf>, stream_paths: HashMap<String, PathBuf>) -> Self {
        let mixer = Arc::new(Mutex::new(Mixer { music_gain: 0.5, sfx_gain: 1.0, out_rate: OUT_RATE, ..Default::default() }));
        let mut status = String::from("audio: no output device");
        let silent = std::env::var_os("GDL_NO_AUDIO").is_some();
        if silent {
            status = "audio: disabled (GDL_NO_AUDIO)".into();
        }
        let stream = if silent { None } else { start_alsa(mixer.clone()) };
        if stream.is_some() {
            status = "audio: on (ALSA)".into();
        }
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

    /// Start sound `id` with request volume, pan (−1..1) and priority.
    pub fn play(&mut self, id: i32, volume: i32, pan: f32, priority: i32, queued: bool) {
        self.play_ex(id, volume, pan, priority, queued, false);
    }

    /// AudioLOOP: the call's chain repeats until stopped.
    pub fn play_loop(&mut self, id: i32, volume: i32, pan: f32, priority: i32) {
        self.play_ex(id, volume, pan, priority, false, true);
    }

    fn play_ex(&mut self, id: i32, volume: i32, pan: f32, priority: i32, queued: bool, force_loop: bool) {
        let (b, call) = AudioRom::split(id);
        let Some((vbk, pcm)) = self.bank(b) else { return };
        let Some(c) = vbk.calls.get(call) else { return };
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
            return;
        }
        loops |= force_loop;
        let g = (volume.max(0) as f32 / 256.0) * (f32::from(c.volume) / 127.0);
        let pan = pan.clamp(-1.0, 1.0);
        let gain = [g * (1.0 - pan.max(0.0)), g * (1.0 + pan.min(0.0))];
        let v = Voice { id, chain, rates, loop_from: loop_from.or(Some(0)), loops, part: 0, pos: 0.0, gain, priority, queued };
        if let Ok(mut m) = self.mixer.lock() {
            if queued {
                if m.queue.len() < 8 {
                    m.queue.push(v);
                }
                return;
            }
            if MAX_VOICES <= m.voices.len() {
                // Steal the lowest-priority voice (approximation of DCS voiceFind).
                if let Some((k, _)) = m.voices.iter().enumerate().filter(|(_, x)| !x.queued).min_by_key(|(_, x)| x.priority) {
                    m.voices.remove(k);
                } else {
                    return;
                }
            }
            m.voices.push(v);
        }
    }

    pub fn stop(&mut self, id: i32) {
        if let Ok(mut m) = self.mixer.lock() {
            m.voices.retain(|v| v.id != id);
        }
    }

    pub fn stop_all(&mut self) {
        if let Ok(mut m) = self.mixer.lock() {
            m.voices.clear();
            m.queue.clear();
            m.music = None;
        }
    }

    /// Settings → Audio: GamePrefs music (+4) and sound (+0) volumes, 128 =
    /// the captured session's value (presentation scaling: gain × v / 128).
    pub fn set_volumes(&mut self, music: u8, sfx: u8) {
        if let Ok(mut m) = self.mixer.lock() {
            m.music_gain = 0.5 * f32::from(music) / 128.0;
            m.sfx_gain = f32::from(sfx) / 128.0;
        }
    }

    /// Loop a STREAMS/<name> music file.
    pub fn music(&mut self, name: &str) {
        let Some(path) = self.stream_paths.get(&name.to_uppercase()) else { return };
        let Ok(bytes) = std::fs::read(path) else { return };
        let Ok(ads) = Ads::parse(&bytes) else { return };
        let pcm = Arc::new(ads.decode(&bytes));
        if let Ok(mut m) = self.mixer.lock() {
            m.music = Some((pcm, ads.rate, ads.channels, 0.0));
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

    fn voice(pcm: Vec<i16>, rate: u32, loops: bool, queued: bool) -> Voice {
        Voice {
            id: 1,
            chain: vec![Arc::new(pcm)],
            rates: vec![rate],
            loop_from: Some(0),
            loops,
            part: 0,
            pos: 0.0,
            gain: [1.0, 0.5],
            priority: 0,
            queued,
        }
    }

    #[test]
    fn mixer_plays_a_chain_once_and_resamples() {
        // Synthetic: 4 samples at 24 kHz into a 48 kHz output → 8 frames.
        let mut m = Mixer { out_rate: 48_000, sfx_gain: 1.0, ..Default::default() };
        m.voices.push(voice(vec![16384; 4], 24_000, false, false));
        let mut out = vec![0f32; 2 * 10];
        m.render(&mut out, 2);
        assert_eq!(out[0], 0.5);
        assert_eq!(out[1], 0.25);
        assert_eq!(out[14], 0.5);
        assert_eq!(out[16], 0.0);
        assert!(m.voices.is_empty());
    }

    #[test]
    fn queued_lines_play_one_at_a_time_and_loops_persist() {
        let mut m = Mixer { out_rate: 48_000, sfx_gain: 1.0, ..Default::default() };
        m.queue.push(voice(vec![100; 2], 48_000, false, true));
        m.queue.push(voice(vec![100; 2], 48_000, false, true));
        m.voices.push(voice(vec![1; 2], 48_000, true, false));
        let mut out = vec![0f32; 2 * 4];
        m.render(&mut out, 2);
        assert_eq!(m.queue.len(), 1);
        assert_eq!(m.voices.len(), 1); // the queued line finished; the loop persists
    }
}
