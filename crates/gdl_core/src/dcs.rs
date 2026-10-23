//! DCS.IRX's sound-effect voices (the IOP sound driver; research/audio.md
//! "DCS voices"), ported from the instructions of the disc's IRX/DCS.IRX
//! (private/reports/ghidra-dcs1, ghidra-dcs2): BankPlay 0x5b10, voiceFind
//! 0x4864, voiceInUse 0x3944, voiceDuck 0x43f4, voicePlayNextSample's end
//! of chain 0x4490, VoiceStop 0x5f94, VoiceVolume 0x5d90, VoicePan 0x5e44
//! and voiceSetVolPan 0x39d4 (the SPU VOLL/VOLR words). Pure integer state;
//! the host's mixer plays the samples and asks for each voice's volumes.
//!
//! Not modelled: the SPU envelope (ADSR 0x000f/0x1fc0 set by
//! samplePlayOnVoice) — a voice counts as sounding until its chain ends or
//! it is stopped — and sample residency (pool/SPU transfers).

/// voiceFind and voiceDuck walk voices 0..11.
pub const VOICES: usize = 12;

/// Offset of the pan table (u16[256], quarter sine) in DCS.IRX's loaded
/// image (voiceSetVolPan's 0x10000 − 0x4e78) and its SHA-256 on the disc.
pub const PAN_TABLE_ADDR: u32 = 0xb188;
pub const PAN_TABLE_SHA256: &str = "ac069de9634eef15862a2eef911fd85e995884689b5b9db2087c9f1d46b1352d";

/// The pan table read from the user's DCS.IRX.
#[derive(Clone, Debug)]
pub struct PanTable(pub [u16; 256]);

impl PanTable {
    /// From the IRX image: the first PT_LOAD segment (vaddr 0) holds the
    /// module; the table's bytes must hash to PAN_TABLE_SHA256.
    pub fn from_irx(b: &[u8]) -> Result<Self, String> {
        let u16_at = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]));
        let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]));
        if b.get(..4) != Some(b"\x7fELF".as_slice()) {
            return Err("DCS.IRX: not an ELF image".into());
        }
        let phoff = u32_at(0x1c).ok_or("DCS.IRX: header")? as usize;
        let n = u16_at(0x2c).ok_or("DCS.IRX: header")? as usize;
        let load = (0..n)
            .filter_map(|k| {
                let p = phoff + 32 * k;
                (u32_at(p)? == 1).then_some((u32_at(p + 4)? as usize, u32_at(p + 8)?, u32_at(p + 16)?))
            })
            .next()
            .ok_or("DCS.IRX: no loadable segment")?;
        let (off, vaddr, filesz) = load;
        if vaddr != 0 || filesz < PAN_TABLE_ADDR + 512 {
            return Err("DCS.IRX: unexpected segment layout".into());
        }
        let at = off + PAN_TABLE_ADDR as usize;
        let bytes = b.get(at..at + 512).ok_or("DCS.IRX: short image")?;
        let hash = crate::save::sha256(bytes);
        if hash != PAN_TABLE_SHA256 {
            return Err(format!("DCS.IRX: pan table identity differs ({hash})"));
        }
        let mut t = [0u16; 256];
        for (k, v) in t.iter_mut().enumerate() {
            *v = u16::from_le_bytes([bytes[2 * k], bytes[2 * k + 1]]);
        }
        Ok(Self(t))
    }
}

/// gVoice (0x5a2e8, 0x14 bytes each) and the state voiceInUse reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Voice {
    /// +0: volume (s16; voiceSetVolPan clamps negatives to 0).
    pub vol: i16,
    /// +2: pan word (low byte position, bits 8..9 sign mode).
    pub pan: u16,
    /// +4: the call's duck amount while it plays.
    pub duck: u16,
    /// +8: the request's priority << 16 | the call's priority word.
    pub prio: u32,
    /// +0xc ≥ 0: the chain has another sample to play.
    pub chained: bool,
    /// The SPU envelope (ENVX) is non-zero: the voice still sounds.
    pub sounding: bool,
}

#[derive(Clone, Debug)]
pub struct Dcs {
    pub voices: [Voice; VOICES],
    /// gVolDuck 0xb130.
    pub vol_duck: i32,
    /// gVolDuckScale 0xb134: BankInit copies the init block's +0x1c, which
    /// aud_init's RPC 0x13 sets to 0x100.
    pub duck_scale: i32,
    /// voice_40 0xb148: voiceFind's round-robin start.
    pub next: usize,
}

impl Default for Dcs {
    fn default() -> Self {
        Self { voices: [Voice::default(); VOICES], vol_duck: 0, duck_scale: 0x100, next: 0 }
    }
}

impl Dcs {
    /// voiceInUse 0x3944: a next sample, a duck held, or the envelope on.
    pub fn in_use(&self, v: usize) -> bool {
        let x = &self.voices[v];
        x.chained || x.duck != 0 || x.sounding
    }

    /// voiceFind 0x4864: the first free voice from the round-robin start;
    /// when all are busy, a voice whose priority word is a new running
    /// maximum not above `prio` is stomped (its duck undone). None: the
    /// request is dropped.
    pub fn find(&mut self, prio: u32) -> Option<usize> {
        let start = self.next;
        let mut best: i64 = -1;
        let mut pick = None;
        loop {
            let v = self.next;
            if !self.in_use(v) {
                return Some(v);
            }
            let p = i64::from(self.voices[v].prio as i32);
            if best < p {
                best = p;
                if p <= i64::from(prio as i32) {
                    pick = Some(v);
                }
            }
            self.next = (self.next + 1) % VOICES;
            if self.next == start {
                break;
            }
        }
        if let Some(v) = pick {
            // "Voice %d STOMPED": spuIntRemoveOne, then the duck undone.
            let d = self.voices[v].duck;
            if d != 0 {
                self.duck(-i32::from(d));
            }
        }
        pick
    }

    /// voiceDuck 0x43f4: gVolDuck += d × scale >> 8 and every voice in use
    /// loses the same amount.
    pub fn duck(&mut self, d: i32) {
        let k = d.wrapping_mul(self.duck_scale) >> 8;
        self.vol_duck = self.vol_duck.wrapping_add(k);
        for v in 0..VOICES {
            if self.in_use(v) {
                self.voices[v].vol = self.voices[v].vol.wrapping_sub(k as i16);
            }
        }
    }

    /// BankPlay 0x5b10 for a call (volume, duck, priority words) with the
    /// request word (volume << 16 | pan) and priority: the voice, or None
    /// when voiceFind refuses.
    pub fn bank_play(&mut self, request: u32, prio: u32, call_volume: u16, call_duck: u16, call_prio: u16) -> Option<usize> {
        let v = self.find(prio)?;
        let req = (request as i32) >> 16;
        let base = (req.wrapping_mul(i32::from(call_volume)) / 0x7f) as i16;
        self.voices[v] = Voice {
            vol: base.wrapping_sub(self.vol_duck as i16),
            pan: request as u16,
            duck: call_duck,
            prio: prio << 16 | u32::from(call_prio),
            chained: true,
            sounding: true,
        };
        if call_duck != 0 {
            self.duck(i32::from(call_duck));
        }
        Some(v)
    }

    /// voicePlayNextSample at the chain's end (no next sample): the duck
    /// undone and cleared; the envelope still sounds until `silent`.
    pub fn chain_end(&mut self, v: usize) {
        self.voices[v].chained = false;
        let d = self.voices[v].duck;
        if d != 0 {
            self.duck(-i32::from(d));
        }
        self.voices[v].duck = 0;
    }

    /// The voice's envelope reached zero (the SPU stopped it).
    pub fn silent(&mut self, v: usize) {
        self.voices[v].sounding = false;
    }

    /// VoiceStop 0x5f94(mask): key off, the duck undone, no next sample.
    pub fn stop(&mut self, mask: u32) {
        for v in 0..VOICES {
            if mask >> v & 1 == 0 {
                continue;
            }
            let d = self.voices[v].duck;
            if d != 0 {
                self.duck(-i32::from(d));
                self.voices[v].duck = 0;
            }
            self.voices[v].chained = false;
            self.voices[v].sounding = false;
        }
    }

    /// VoiceVolume 0x5d90(mask, volume): each voice moves towards
    /// volume − gVolDuck by at most 8 per call.
    pub fn voice_volume(&mut self, mask: u32, volume: i32) {
        let target = volume.wrapping_sub(self.vol_duck);
        for v in 0..VOICES.min(32) {
            if mask >> v & 1 == 0 {
                continue;
            }
            let d = target.wrapping_sub(i32::from(self.voices[v].vol));
            if d != 0 {
                self.voices[v].vol = self.voices[v].vol.wrapping_add(d.clamp(-8, 8) as i16);
            }
        }
    }

    /// VoicePan 0x5e44(mask, pan): the shorter way round (±0x200), at most
    /// 8 per call, the word kept within −0x200..0x1ff.
    pub fn voice_pan(&mut self, mask: u32, pan: i32) {
        for v in 0..VOICES {
            if mask >> v & 1 == 0 {
                continue;
            }
            let cur = i32::from(self.voices[v].pan);
            let mut d = pan.wrapping_sub(cur);
            if (d as i16) == 0 {
                continue;
            }
            if 0x100 < i32::from(d as i16) {
                d -= 0x200;
            }
            if i32::from(d as i16) < -0x100 {
                d += 0x200;
            }
            if (d as i16) == 0 {
                continue;
            }
            let step = (d as i16).clamp(-8, 8);
            let mut p = (self.voices[v].pan as i16).wrapping_add(step);
            if 0x1ff < p {
                p = p.wrapping_sub(0x400);
            }
            if p < -0x200 {
                p = p.wrapping_add(0x400);
            }
            self.voices[v].pan = p as u16;
        }
    }

    /// voiceSetVolPan 0x39d4: the SPU VOLL/VOLR words (15-bit, bit 14 the
    /// sign: phase-inverted when negative) for voice `v`, sign-extended.
    /// The sample's base volumes (+0x34/+0x36) are VagParseHeader's 0x3fff.
    pub fn vol_pan(&self, v: usize, tab: &PanTable) -> [i16; 2] {
        let x = &self.voices[v];
        let vol = i32::from(x.vol).max(0);
        let mut l = 0x3fff * vol / 0xff;
        let mut r = 0x3fff * vol / 0xff;
        if 0x4000 <= l & 0xffff {
            r = r + l - 0x3fff;
            l = 0x3fff;
        }
        if 0x4000 <= r & 0xffff {
            l = l + r - 0x3fff;
            r = 0x3fff;
        }
        let p = usize::from(x.pan & 0xff);
        let (s, c) = (i32::from(tab.0[p]), i32::from(tab.0[0xff - p]));
        // (left factor, right factor) by sign mode (pan bits 8..9).
        let (fl, fr) = match (x.pan >> 8) & 3 {
            0 => (c, s),
            1 => (-s, c),
            2 => (-c, -s),
            _ => (s, -c),
        };
        let word = |a: i32, f: i32| {
            let w = ((a & 0xffff).wrapping_mul(f) >> 14) & 0x7fff;
            (if w & 0x4000 != 0 { w - 0x8000 } else { w }) as i16
        };
        [word(l, fl), word(r, fr)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic table: a quarter sine of amplitude 0x4000 (the disc's own
    /// table is checked by its hash, not by this formula).
    fn tab() -> PanTable {
        let mut t = [0u16; 256];
        for (k, v) in t.iter_mut().enumerate() {
            *v = (16384.0 * (std::f64::consts::FRAC_PI_2 * k as f64 / 256.0).sin()) as u16;
        }
        PanTable(t)
    }

    #[test]
    fn centre_and_hard_pans() {
        let t = tab();
        let mut d = Dcs::default();
        let v = d.bank_play(0xff << 16 | 0x7f, 0x42, 0x7f, 0, 0x10).unwrap();
        let [l, r] = d.vol_pan(v, &t);
        assert!((l - r).abs() < 0x80 && 0x2c00 < l, "centre is balanced: {l} {r}");
        d.voices[v].pan = 0;
        let [l, r] = d.vol_pan(v, &t);
        assert!(0x3f00 < l && r == 0, "pan 0: left only ({l}, {r})");
        d.voices[v].pan = 0xff;
        let [l, r] = d.vol_pan(v, &t);
        assert!(l < 0x80 && 0x3f00 < r, "pan 0xff: right ({l}, {r})");
        // AudioAng's negated pans (sources behind): mode 3, right inverted.
        d.voices[v].pan = (-0x40i16) as u16;
        let [l, r] = d.vol_pan(v, &t);
        assert!(0 < l && r < 0, "behind: right phase-inverted ({l}, {r})");
    }

    #[test]
    fn duck_lowers_others_and_is_undone() {
        let mut d = Dcs::default();
        let a = d.bank_play(100 << 16 | 0x7f, 1, 0x7f, 0, 0).unwrap();
        assert_eq!(d.voices[a].vol, 100);
        let b = d.bank_play(100 << 16 | 0x7f, 1, 0x7f, 20, 0).unwrap();
        assert_eq!((d.vol_duck, d.voices[a].vol, d.voices[b].vol), (20, 80, 80));
        d.chain_end(b);
        assert_eq!((d.vol_duck, d.voices[a].vol, d.voices[b].vol, d.voices[b].duck), (0, 100, 100, 0));
        // A voice started while ducked starts lowered by gVolDuck.
        let _c = d.bank_play(100 << 16 | 0x7f, 1, 0x7f, 20, 0).unwrap();
        let e = d.bank_play(100 << 16 | 0x7f, 1, 0x7f, 0, 0).unwrap();
        assert_eq!(d.voices[e].vol, 80);
    }

    #[test]
    fn find_drops_when_full_unless_a_lower_word_exists() {
        let mut d = Dcs::default();
        for _ in 0..VOICES {
            assert!(d.bank_play(1 << 16, 0x42, 0x7f, 0, 0x8000).is_some());
        }
        // Every stored word (0x42 << 16 | 0x8000) exceeds the raw request
        // priority: dropped.
        assert_eq!(d.bank_play(1 << 16, 0x42, 0x7f, 0, 0), None);
        // A finished voice is free again.
        d.chain_end(3);
        d.silent(3);
        assert_eq!(d.find(0x42), Some(3));
    }

    #[test]
    fn volume_and_pan_ramp_by_eight() {
        let mut d = Dcs::default();
        let v = d.bank_play(0x40 << 16 | 0x7f, 1, 0x7f, 0, 0).unwrap();
        d.voice_volume(1 << v, 0x60);
        assert_eq!(d.voices[v].vol, 0x48);
        d.voice_pan(1 << v, 0x7f - 3);
        assert_eq!(d.voices[v].pan, 0x7c);
        d.voice_pan(1 << v, 0xff);
        assert_eq!(d.voices[v].pan, 0x84);
        // Past 0x1ff the word wraps to −0x200..
        d.voices[v].pan = 0x1fc;
        d.voice_pan(1 << v, 0x204);
        assert_eq!(d.voices[v].pan as i16, -0x1fc);
    }
}
