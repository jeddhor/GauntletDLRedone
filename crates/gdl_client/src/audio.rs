//! Native audio output for the playtest: a small mixer on a cpal stream.
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
                out[f * channels] += s * v.gain[0];
                if 1 < channels {
                    out[f * channels + 1] += s * v.gain[1];
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
    _stream: Option<cpal::Stream>,
    pub status: String,
}

impl AudioOut {
    pub fn new(rom: AudioRom, bank_paths: HashMap<String, PathBuf>, stream_paths: HashMap<String, PathBuf>) -> Self {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let mixer = Arc::new(Mutex::new(Mixer { music_gain: 0.5, ..Default::default() }));
        let mut status = String::from("audio: no output device");
        let silent = std::env::var_os("GDL_NO_AUDIO").is_some();
        if silent {
            status = "audio: disabled (GDL_NO_AUDIO)".into();
        }
        let stream = (|| -> Option<cpal::Stream> {
            if silent {
                return None;
            }
            let host = cpal::default_host();
            let device = host.default_output_device()?;
            let config = device.default_output_config().ok()?;
            let format = config.sample_format();
            let cfg: cpal::StreamConfig = config.into();
            let channels = cfg.channels as usize;
            mixer.lock().ok()?.out_rate = cfg.sample_rate.0;
            let m = mixer.clone();
            let err = |e| eprintln!("audio stream error: {e}");
            let stream = match format {
                cpal::SampleFormat::F32 => device
                    .build_output_stream(&cfg, move |d: &mut [f32], _| m.lock().map_or((), |mut m| m.render(d, channels)), err, None)
                    .ok()?,
                cpal::SampleFormat::I16 => {
                    let mut buf = Vec::new();
                    device
                        .build_output_stream(
                            &cfg,
                            move |d: &mut [i16], _| {
                                buf.resize(d.len(), 0.0);
                                if let Ok(mut m) = m.lock() {
                                    m.render(&mut buf, channels);
                                }
                                for (o, s) in d.iter_mut().zip(&buf) {
                                    *o = (*s * 32767.0) as i16;
                                }
                            },
                            err,
                            None,
                        )
                        .ok()?
                }
                _ => return None,
            };
            stream.play().ok()?;
            Some(stream)
        })();
        if stream.is_some() {
            status = "audio: on".into();
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
