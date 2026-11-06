//! PSS movie playback (presentation; docs/decisions/0007-native-movie-video.md):
//! the MPEG-2 video is decoded natively (gdl_formats::mpeg2) on a thread
//! of its own, the private-stream PS-ADPCM audio by gdl_formats::pss.
//! Frames are shown at the stream's 30 fps from the start of playback
//! (ServeMovie's pacing on the PS2 is the IPU/vsync loop). The colour
//! conversion is PCSX2's model of the IPU's CSC (an upstream claim, not
//! compared with a capture of the original). A stream the decoder does not
//! cover does not start, as when InitMovie fails.
use gdl_formats::mpeg2::{Decoder, Picture};
use std::path::Path;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Mutex;

pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 480;
const FPS: f64 = 30.0;

pub struct MoviePlayer {
    /// Behind a Mutex so the player can live in a Sync resource.
    frames: Mutex<Receiver<Vec<u8>>>,
    /// Frames handed out so far and the playback clock (seconds).
    shown: u64,
    clock: f64,
    ended: bool,
    /// Interleaved PCM, rate and channels of the movie's audio.
    pub audio: Option<(Vec<i16>, u32, u32)>,
}

/// YCbCr to RGBA as PCSX2 models the IPU's colour space conversion
/// (pcsx2/IPU/yuv2rgb.cpp yuv2rgb_reference: BT.601 in 6-bit fixed point,
/// one chroma sample for each 2×2 pixels), opaque.
pub fn ipu_csc(p: &Picture) -> Vec<u8> {
    let mut out = vec![255u8; p.width * p.height * 4];
    let cw = p.width / 2;
    for (k, px) in out.chunks_exact_mut(4).enumerate() {
        let (x, y) = (k % p.width, k / p.width);
        let lum = (0x95 * (i32::from(p.y[k]) - 16).max(0)) >> 6;
        let c = (y / 2) * cw + x / 2;
        let (cb, cr) = (i32::from(p.cb[c]) - 128, i32::from(p.cr[c]) - 128);
        let rgb = [lum + ((0xcc * cr) >> 6), lum + ((-0x68 * cr) >> 6) + ((-0x32 * cb) >> 6), lum + ((0x102 * cb) >> 6)];
        for (o, v) in px.iter_mut().zip(rgb) {
            *o = ((v + 1) >> 1).clamp(0, 255) as u8;
        }
    }
    out
}

impl MoviePlayer {
    /// InitMovie: open the file, decode its audio and start the video
    /// decoder. Err when the file is unavailable or its video is not one
    /// the decoder covers (checked on the first picture).
    pub fn start(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let audio = gdl_formats::pss::decode_audio(&bytes).map_err(|e| e.to_string())?;
        let mut decoder = Decoder::new(gdl_formats::pss::demux_video(&bytes).map_err(|e| e.to_string())?);
        let first = match decoder.next() {
            Some(Ok(p)) if (p.width, p.height) == (WIDTH as usize, HEIGHT as usize) => p,
            Some(Ok(p)) => return Err(format!("{}x{} video", p.width, p.height)),
            Some(Err(e)) => return Err(e.to_string()),
            None => return Err("no video pictures".into()),
        };
        // A few frames of look-ahead; the decoder stops at the end of the
        // stream, at a decoding error or when the player is dropped
        // (KillMovie).
        let (tx, rx) = sync_channel::<Vec<u8>>(6);
        std::thread::Builder::new()
            .name("movie".into())
            .spawn(move || {
                for p in std::iter::once(Ok(first)).chain(decoder) {
                    match p {
                        Ok(p) if (p.width, p.height) == (WIDTH as usize, HEIGHT as usize) => {
                            if tx.send(ipu_csc(&p)).is_err() {
                                break;
                            }
                        }
                        _ => break,
                    }
                }
            })
            .map_err(|e| format!("movie thread: {e}"))?;
        Ok(Self { frames: Mutex::new(rx), shown: 0, clock: 0.0, ended: false, audio })
    }

    /// Advance the playback clock by `dt` seconds; returns the newest frame
    /// that became due (frames due together are dropped but the last).
    pub fn advance(&mut self, dt: f64) -> Option<Vec<u8>> {
        if self.ended {
            return None;
        }
        self.clock += dt.max(0.0);
        let due = (self.clock * FPS).floor() as u64 + 1;
        let mut last = None;
        let Ok(frames) = self.frames.lock() else {
            self.ended = true;
            return None;
        };
        while self.shown < due {
            match frames.try_recv() {
                Ok(f) => {
                    self.shown += 1;
                    last = Some(f);
                }
                // The decoder is behind: show what there is, catch up later.
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.ended = true;
                    break;
                }
            }
        }
        last
    }

    /// ServeMovie's "still playing".
    pub fn playing(&self) -> bool {
        !self.ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipu_csc_follows_the_pcsx2_reference() {
        // 2×2 pixels sharing one chroma sample.
        let p = Picture { width: 2, height: 2, y: vec![16, 235, 128, 10], cb: vec![90], cr: vec![200] };
        let rgba = ipu_csc(&p);
        // Y 235, Cb -38, Cr 72: lum (149 × 219) >> 6 = 509; R 509 + 229,
        // G 509 - 117 + 29, B 509 - 154 (the shifts round down), halved
        // with rounding and clamped.
        assert_eq!(&rgba[4..8], &[255, 211, 178, 255]);
        // Y 16 and Y 10 (below black): lum 0.
        assert_eq!(&rgba[0..4], &[115, 0, 0, 255]);
        assert_eq!(&rgba[12..16], &[115, 0, 0, 255]);
        // Y 128: lum (149 × 112) >> 6 = 260.
        assert_eq!(&rgba[8..12], &[245, 86, 53, 255]);
    }
}
