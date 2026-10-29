//! PSS movie playback (presentation; docs/decisions/0003-movies.md): the
//! MPEG-2 video is decoded by the system ffmpeg (a child process writing
//! 640×480 RGBA frames to a pipe), the private-stream PS-ADPCM audio by
//! gdl_formats::pss. Frames are shown at the stream's 30 fps from the start
//! of playback (ServeMovie's pacing on the PS2 is the IPU/vsync loop; the
//! colour conversion is ffmpeg's BT.601 limited-range one, HYPOTHESIS: the
//! IPU's CSC gives the same values). Without ffmpeg the movie does not
//! start, as when InitMovie fails.
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Mutex;

pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 480;
const FPS: f64 = 30.0;

pub struct MoviePlayer {
    child: Child,
    /// Behind a Mutex so the player can live in a Sync resource.
    frames: Mutex<Receiver<Vec<u8>>>,
    /// Frames handed out so far and the playback clock (seconds).
    shown: u64,
    clock: f64,
    ended: bool,
    /// Interleaved PCM, rate and channels of the movie's audio.
    pub audio: Option<(Vec<i16>, u32, u32)>,
}

/// The ffmpeg executable: GDL_FFMPEG, else one beside the game's
/// executable (an installation may carry it), else `ffmpeg` on PATH.
fn ffmpeg() -> String {
    if let Ok(f) = std::env::var("GDL_FFMPEG") {
        return f;
    }
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" })));
    match beside.filter(|p| p.is_file()) {
        Some(p) => p.to_string_lossy().into_owned(),
        None => "ffmpeg".into(),
    }
}

impl MoviePlayer {
    /// InitMovie: open the file, decode its audio and start the video
    /// decoder. Err when the file or ffmpeg is unavailable.
    pub fn start(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let audio = gdl_formats::pss::decode_audio(&bytes).map_err(|e| e.to_string())?;
        let mut child = Command::new(ffmpeg())
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-i"])
            .arg(path)
            .args(["-an", "-vf", &format!("scale={WIDTH}:{HEIGHT}"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("ffmpeg: {e}"))?;
        let mut out = child.stdout.take().ok_or("ffmpeg: no stdout")?;
        // A few frames of look-ahead; the reader ends with the stream.
        let (tx, rx) = sync_channel::<Vec<u8>>(6);
        std::thread::spawn(move || {
            let size = (WIDTH * HEIGHT * 4) as usize;
            loop {
                let mut frame = vec![0u8; size];
                if out.read_exact(&mut frame).is_err() || tx.send(frame).is_err() {
                    break;
                }
            }
        });
        Ok(Self { child, frames: Mutex::new(rx), shown: 0, clock: 0.0, ended: false, audio })
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

impl Drop for MoviePlayer {
    /// KillMovie: stop the decoder.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
