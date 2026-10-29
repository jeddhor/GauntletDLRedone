//! Audio resources: AUDIO/AUDATPS2.ROM (sound/bank tables), AUDIO/*.VBK
//! (VBNK call lists + VAGp samples, interpreted by IRX/DCS.IRX BankPlay /
//! voicePlayNextSample) and STREAMS/*.ADS (SShd/SSbd music streams), with a
//! PS-ADPCM decoder. Raw bytes of every structure are retained.
//!
//! VBNK v1.06 (DCS.IRX BankReadHeader): "VBNK", u32 call-list bytes, u16
//! 0x106, u16, u32 call count, u32 sample count, then the call list as u16
//! words. A call is a run of sample words (low 12 bits = sample index, 0x4000
//! loop start, 0x2000 loop back to the start marker); the word with 0x8000
//! ends the call and is followed by three words: volume (0..0x7f scale),
//! duck and priority.
use serde::Serialize;

#[derive(Debug)]
pub struct AudioError(pub String);
impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "audio: {}", self.0)
    }
}
impl std::error::Error for AudioError {}
type R<T> = Result<T, AudioError>;
fn err<T>(s: impl Into<String>) -> R<T> {
    Err(AudioError(s.into()))
}

fn u16le(b: &[u8], o: usize) -> R<u16> {
    b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| AudioError(format!("read u16 at {o:#x}")))
}
fn u32le(b: &[u8], o: usize) -> R<u32> {
    b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| AudioError(format!("read u32 at {o:#x}")))
}
fn u32be(b: &[u8], o: usize) -> R<u32> {
    b.get(o..o + 4).map(|s| u32::from_be_bytes(s.try_into().unwrap())).ok_or_else(|| AudioError(format!("read u32 at {o:#x}")))
}
fn cstr(b: &[u8]) -> String {
    String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned()
}

/// PS-ADPCM (SPU) decoder state for one channel.
#[derive(Default, Clone, Copy)]
pub struct AdpcmState {
    s1: i32,
    s2: i32,
}

const COEF: [[i32; 2]; 5] = [[0, 0], [60, 0], [115, -52], [98, -55], [122, -60]];

/// Decode 16-byte SPU ADPCM frames; stops at a frame with the end flag (1)
/// when `stop_at_end`. Returns the PCM samples.
pub fn decode_adpcm(data: &[u8], state: &mut AdpcmState, stop_at_end: bool) -> Vec<i16> {
    let mut out = Vec::with_capacity(data.len() / 16 * 28);
    for frame in data.chunks_exact(16) {
        let shift = (frame[0] & 0xf) as i32;
        let pred = ((frame[0] >> 4) as usize).min(4);
        let flags = frame[1];
        for &byte in &frame[2..] {
            for nib in [byte & 0xf, byte >> 4] {
                let raw = (((nib as i32) << 28) >> 28) << 12;
                let s = if shift <= 12 { raw >> shift } else { raw >> 12 };
                let v = s + ((state.s1 * COEF[pred][0] + state.s2 * COEF[pred][1] + 32) >> 6);
                let v = v.clamp(-32768, 32767);
                state.s2 = state.s1;
                state.s1 = v;
                out.push(v as i16);
            }
        }
        if stop_at_end && flags & 1 != 0 {
            break;
        }
    }
    out
}

/// One VAGp sample in a bank.
#[derive(Debug, Clone, Serialize)]
pub struct VagSample {
    pub index: u32,
    pub name: String,
    pub rate: u32,
    pub offset: u32,
    pub size: u32,
}

/// One call of a bank: sample words and (volume, duck, priority).
#[derive(Debug, Clone, Serialize)]
pub struct BankCall {
    pub words: Vec<u16>,
    pub volume: u16,
    pub duck: u16,
    pub priority: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct Vbk {
    pub version: u16,
    pub word_0a: u16,
    pub calls: Vec<BankCall>,
    pub samples: Vec<VagSample>,
    #[serde(skip)]
    pub raw_header: Vec<u8>,
}

impl Vbk {
    pub fn parse(b: &[u8]) -> R<Self> {
        if b.get(0..4) != Some(b"VBNK") {
            return err("not a VBNK bank");
        }
        let list = u32le(b, 4)? as usize;
        let version = u16le(b, 8)?;
        let word_0a = u16le(b, 10)?;
        let ncalls = u32le(b, 12)? as usize;
        if version != 0x106 || 0x500 <= ncalls {
            return err(format!("unsupported bank version {version:#x} / {ncalls} calls"));
        }
        let start = 0x14;
        let words: Vec<u16> = (0..list / 2).map(|k| u16le(b, start + 2 * k)).collect::<R<_>>()?;
        let mut calls = Vec::new();
        let mut cur = Vec::new();
        let mut k = 0;
        while k < words.len() && calls.len() < ncalls {
            let w = words[k];
            cur.push(w);
            if w & 0x8000 != 0 {
                let p = words.get(k + 1..k + 4).ok_or_else(|| AudioError("call parameters truncated".into()))?;
                calls.push(BankCall { words: std::mem::take(&mut cur), volume: p[0], duck: p[1], priority: p[2] });
                k += 4;
            } else {
                k += 1;
            }
        }
        if calls.len() != ncalls {
            return err(format!("call list has {} calls, header {ncalls}", calls.len()));
        }
        let mut samples = Vec::new();
        let mut o = start + list;
        while let Some(pos) = b.get(o..).and_then(|s| s.windows(4).position(|w| w == b"VAGp")) {
            let at = o + pos;
            let size = u32be(b, at + 12)?;
            let rate = u32be(b, at + 16)?;
            let name = cstr(b.get(at + 32..at + 48).unwrap_or(&[]));
            let data = at as u32 + 48;
            if b.len() < (data + size) as usize {
                return err(format!("VAG {name} truncated"));
            }
            samples.push(VagSample { index: samples.len() as u32, name, rate, offset: data, size });
            o = (data + size) as usize;
        }
        Ok(Self { version, word_0a, calls, samples, raw_header: b[..start.min(b.len())].to_vec() })
    }

    /// Decode one sample to PCM.
    pub fn decode(&self, bytes: &[u8], sample: usize) -> R<Vec<i16>> {
        let s = self.samples.get(sample).ok_or_else(|| AudioError(format!("sample {sample}")))?;
        let data = &bytes[s.offset as usize..(s.offset + s.size) as usize];
        Ok(decode_adpcm(data, &mut AdpcmState::default(), true))
    }
}

/// AUDATPS2.ROM bank entry (0x2c bytes at header word +0x10).
#[derive(Debug, Clone, Serialize)]
pub struct RomBank {
    pub name: String,
    pub label: String,
    pub size: u32,
    pub count: u16,
    pub first: u16,
    pub raw: Vec<u8>,
}

/// AUDATPS2.ROM: banks and the sound name table (0x1c bytes each).
#[derive(Debug, Clone, Serialize)]
pub struct AudioRom {
    pub header: Vec<u32>,
    pub banks: Vec<RomBank>,
    /// (name, duration seconds) by global sound index.
    pub sounds: Vec<(String, f32)>,
    /// Sound ID stored in each name record (+0x10).
    pub ids: Vec<i32>,
}

impl AudioRom {
    pub fn parse(b: &[u8]) -> R<Self> {
        let header: Vec<u32> = (0..6).map(|k| u32le(b, 4 * k)).collect::<R<_>>()?;
        let (banks_at, names_at) = (header[4] as usize, header[5] as usize);
        if !(banks_at < names_at && names_at <= b.len() && (names_at - banks_at) % 0x2c == 0) {
            return err("unexpected AUDATPS2 table layout");
        }
        let banks = b[banks_at..names_at]
            .chunks_exact(0x2c)
            .map(|e| RomBank {
                name: cstr(&e[..0x10]),
                label: cstr(&e[0x10..0x20]),
                size: u32::from_le_bytes(e[0x20..0x24].try_into().unwrap()),
                count: u16::from_le_bytes([e[0x24], e[0x25]]),
                first: u16::from_le_bytes([e[0x26], e[0x27]]),
                raw: e.to_vec(),
            })
            .collect();
        let recs: Vec<&[u8]> = b[names_at..].chunks_exact(0x1c).collect();
        let sounds = recs.iter().map(|e| (cstr(&e[..0x10]), f32::from_le_bytes(e[0x14..0x18].try_into().unwrap()))).collect();
        let ids = recs.iter().map(|e| i32::from_le_bytes(e[0x10..0x14].try_into().unwrap())).collect();
        Ok(Self { header, banks, sounds, ids })
    }

    /// AudioFindSound 0x1203e8: banks in order, names compared with
    /// strncmp(…, 15); returns the record's stored ID (none for an empty
    /// name).
    pub fn find(&self, name: &str) -> Option<i32> {
        if name.is_empty() {
            return None;
        }
        for bank in &self.banks {
            for g in bank.first as usize..(bank.first + bank.count) as usize {
                if self.sounds.get(g).is_some_and(|(n, _)| n.bytes().take(15).eq(name.bytes().take(15))) {
                    return self.ids.get(g).copied();
                }
            }
        }
        None
    }

    /// The name record's duration (seconds) of an ID, found as
    /// AudioAnyQueAdd4 / AudioTrackSet do: bank `id >> 16`'s first name
    /// index (+0x26) + `id & 0xfff`, record +0x14.
    pub fn length_of(&self, id: i32) -> Option<f32> {
        let bank = self.banks.get(usize::try_from(id >> 16).ok()?)?;
        self.sounds.get(usize::from(bank.first) + (id & 0xfff) as usize).map(|s| s.1)
    }

    /// The name record of an ID (as length_of finds it).
    pub fn name_of(&self, id: i32) -> Option<&str> {
        let bank = self.banks.get(usize::try_from(id >> 16).ok()?)?;
        self.sounds.get(usize::from(bank.first) + (id & 0xfff) as usize).map(|s| s.0.as_str())
    }

    /// (bank, call) of an ID: bank = id >> 16, call = id & 0xfff.
    pub fn split(id: i32) -> (usize, usize) {
        ((id >> 16) as usize, (id & 0xfff) as usize)
    }
}

/// STREAMS/*.ADS: SShd (type 0x10 = ADPCM) + SSbd interleaved data.
#[derive(Debug, Clone, Serialize)]
pub struct Ads {
    pub kind: u32,
    pub rate: u32,
    pub channels: u32,
    pub interleave: u32,
    pub data_offset: u32,
    pub data_size: u32,
}

impl Ads {
    pub fn parse(b: &[u8]) -> R<Self> {
        if b.get(0..4) != Some(b"SShd") {
            return err("not an ADS stream");
        }
        let hdr = u32le(b, 4)? as usize;
        let kind = u32le(b, 8)?;
        let rate = u32le(b, 12)?;
        let channels = u32le(b, 16)?;
        let interleave = u32le(b, 20)?;
        let bd = 8 + hdr;
        if b.get(bd..bd + 4) != Some(b"SSbd") {
            return err("SSbd missing");
        }
        let data_size = u32le(b, bd + 4)?;
        if kind != 0x10 || channels == 0 || 2 < channels || interleave == 0 || interleave % 16 != 0 {
            return err(format!("unsupported ADS kind {kind:#x} channels {channels} interleave {interleave}"));
        }
        Ok(Self { kind, rate, channels, interleave, data_offset: (bd + 8) as u32, data_size })
    }

    /// Decode to interleaved stereo-or-mono PCM (frames × channels).
    pub fn decode(&self, b: &[u8]) -> Vec<i16> {
        let end = ((self.data_offset + self.data_size) as usize).min(b.len());
        let data = &b[self.data_offset as usize..end];
        let ch = self.channels as usize;
        let il = self.interleave as usize;
        let mut st = vec![AdpcmState::default(); ch];
        let mut per: Vec<Vec<i16>> = vec![Vec::new(); ch];
        for (k, block) in data.chunks(il).enumerate() {
            let c = k % ch;
            per[c].extend(decode_adpcm(block, &mut st[c], false));
        }
        let n = per.iter().map(Vec::len).min().unwrap_or(0);
        let mut out = Vec::with_capacity(n * ch);
        for i in 0..n {
            for p in &per {
                out.push(p[i]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adpcm_zero_frame_decodes_to_silence_and_stops_at_end_flag() {
        let mut data = vec![0u8; 32];
        data[1] = 1; // end flag on the first frame
        let pcm = decode_adpcm(&data, &mut AdpcmState::default(), true);
        assert_eq!(pcm.len(), 28);
        assert!(pcm.iter().all(|&s| s == 0));
    }

    #[test]
    fn adpcm_filter_zero_reproduces_shifted_nibbles() {
        // shift 12, filter 0: nibble n → n (sign-extended).
        let mut f = [0u8; 16];
        f[0] = 0x0c;
        f[2] = 0x71; // low nibble 1, high nibble 7
        f[3] = 0x0f; // -1, 0
        let pcm = decode_adpcm(&f, &mut AdpcmState::default(), false);
        assert_eq!(&pcm[..4], &[1, 7, -1, 0]);
    }

    #[test]
    fn vbnk_call_list_splits_on_end_words() {
        let mut b = b"VBNK".to_vec();
        let words: [u16; 9] = [0x8003, 0x6e, 0, 0x64, 0x0004, 0x8000, 0x7d, 0x0a, 0x8000];
        b.extend((words.len() as u32 * 2).to_le_bytes());
        b.extend(0x106u16.to_le_bytes());
        b.extend(0x107u16.to_le_bytes());
        b.extend(2u32.to_le_bytes());
        b.extend(0u32.to_le_bytes()); // v1.06 sample count word
        for w in words {
            b.extend(w.to_le_bytes());
        }
        let v = Vbk::parse(&b).unwrap();
        assert_eq!(v.calls.len(), 2);
        assert_eq!(v.calls[1].words, vec![0x0004, 0x8000]);
        assert_eq!((v.calls[1].volume, v.calls[1].duck, v.calls[1].priority), (0x7d, 0x0a, 0x8000));
    }
}
