//! The EE voice queue and the track end times the game waits on:
//! AudioAnyQueAdd4 0x122ea0 (AudioQueAdd4 0x123170 = channel 0,
//! AudioVQueAdd4 0x15c460 = channel 1 while GoodWizard < 3), AudioQueUpdate
//! 0x122ab0 (main before game_main, world_update's end of stage,
//! AudioStillPlaying 0x122d50), AudioQueEmpty 0x122e58 and AudioSoundPlaying
//! 0x121e38 over audioTrackData 0x21f798 (AudioTrackSet 0x122138).
//!
//! audioQue 0x2c01c8 holds two channels of up to 16 entries (id, volume,
//! pan, priority, duration in vblanks = the ROM record's +0x14 seconds ×
//! 60); audQuePos 0x2c01b8 counts them and audQueEndtime 0x2c01b0 is the
//! head's end vblank (0 = not started). The head starts (AudioTrack4 → an
//! `Event::Sound` for the host) when its end time is 0 and leaves once
//! vb_counter reaches it; the next call starts the next one. The queue
//! state is game-visible: AudioStillPlaying is AudioQueUpdate's "any entry
//! left", which holds the level exit (game_main), the shop exit, the map
//! loader and the good wizard.
//!
//! AudioTrackSet runs as the IOP's reply to AudioTrack4 (from AudioPoll in
//! main) and records (id, vb_counter + duration × 60) on the voices the
//! IOP chose. HYPOTHESIS: every queued sound gets a track, and the reply
//! arrives at the next frame's AudioPoll. AudioLoading / AudioBusy /
//! audQueMod are taken as clear (no bank loading during play).
use super::{Event, Game};

/// One audioQue entry (0x14 bytes: id, volume, pan, priority, duration).
/// `at`: the position an AudioAng pan was taken from (the host pans by
/// it; presentation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QueEntry {
    pub id: i32,
    pub volume: i32,
    pub pan: i32,
    pub priority: i32,
    pub duration: f32,
    pub at: Option<[f32; 3]>,
}

impl QueEntry {
    pub fn new(id: i32, volume: i32, pan: i32, priority: i32) -> Self {
        Self { id, volume, pan, priority, duration: 0.0, at: None }
    }
    /// A line panned by AudioAng(position).
    pub fn at(id: i32, volume: i32, at: [f32; 3], priority: i32) -> Self {
        Self { id, volume, pan: 0x7f, priority, duration: 0.0, at: Some(at) }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AudioQueue {
    pub entries: [Vec<QueEntry>; 2],
    pub endtime: [f32; 2],
    /// AudioTrack4 requests awaiting their AudioTrackSet reply.
    pending: Vec<i32>,
    /// audioTrackData: (id, end vblank) of the started queue tracks
    /// (12 tracks; the oldest is replaced).
    tracks: Vec<(i32, f32)>,
}

const MAX_TRACKS: usize = 12;

impl AudioQueue {
    /// AudioAnyQueAdd4's body at vblank `vb` with the entry's duration
    /// (vblanks; −1 for id < 0) already known.
    pub fn add(&mut self, vb: f32, max_wait: f32, chan: usize, e: QueEntry) -> f32 {
        let n = self.entries[chan].len();
        if 0xf < n {
            return 0.0;
        }
        if 0 < n {
            let mut t = self.endtime[chan];
            if t == 0.0 {
                t = vb + self.entries[chan][0].duration;
            }
            for q in &self.entries[chan][1..] {
                t += q.duration;
            }
            if 0.0 <= max_wait && max_wait * 60.0 < t - vb {
                return 0.0;
            }
        }
        self.entries[chan].push(e);
        e.duration
    }

    /// AudioQueUpdate's body: the heads started (returned) or retired.
    pub fn update(&mut self, vb: f32) -> (bool, Vec<QueEntry>) {
        let mut busy = false;
        let mut started = Vec::new();
        for c in 0..2 {
            let Some(&head) = self.entries[c].first() else { continue };
            busy = true;
            if self.endtime[c] == 0.0 {
                if 0 <= head.id {
                    started.push(head);
                    self.pending.push(head.id);
                }
                self.endtime[c] = vb + head.duration;
            } else if self.endtime[c] <= vb {
                self.entries[c].remove(0);
                self.endtime[c] = 0.0;
            }
        }
        (busy, started)
    }

    /// AudioPoll's replies (AudioTrackSet) at vblank `now`.
    pub fn poll(&mut self, now: f32, seconds: impl Fn(i32) -> f32) {
        for id in std::mem::take(&mut self.pending) {
            let end = now + seconds(id) * 60.0;
            self.tracks.retain(|&(_, e)| now < e);
            if MAX_TRACKS <= self.tracks.len() {
                self.tracks.remove(0);
            }
            self.tracks.push((id, end));
        }
    }

    pub fn playing(&self, now: f32, id: i32) -> bool {
        self.tracks.iter().any(|&(t, end)| t == id && now < end)
    }
}

impl Game {
    fn sound_seconds(&self, id: i32) -> Option<f32> {
        self.sounds.rom.as_ref().and_then(|r| r.length_of(id))
    }

    /// AudioAnyQueAdd4(−1, max_wait, channel, id, volume, pan, priority):
    /// queue `id` behind the channel's entries unless the wait before it
    /// would pass `max_wait` seconds (negative: no limit). Returns the
    /// entry's duration (vblanks), 0 when refused. Without the audio ROM
    /// (NO_AUDIO) nothing is queued.
    pub fn que_add(&mut self, max_wait: f32, chan: usize, mut e: QueEntry) -> f32 {
        if self.sounds.rom.is_none() || chan > 1 || (self.globals.game_mode & 0x8000 != 0 && !self.attract_audio) {
            return 0.0;
        }
        e.duration = if 0 <= e.id { self.sound_seconds(e.id).map_or(-1.0, |s| s * 60.0) } else { -1.0 };
        let vb = self.vb_counter as f32;
        self.audio_que.add(vb, max_wait, chan, e)
    }

    /// AudioQueAdd4 (channel 0).
    pub fn que_add0(&mut self, max_wait: f32, e: QueEntry) -> f32 {
        self.que_add(max_wait, 0, e)
    }

    /// AudioVQueAdd4: channel 1 before the good wizard appears.
    pub fn vque_add(&mut self, max_wait: f32, e: QueEntry) -> f32 {
        if self.globals.good_wizard < 3 {
            self.que_add(max_wait, 1, e)
        } else {
            0.0
        }
    }

    /// AudioQueUpdate: start (AudioTrack4(−1, id, volume, 0, pan,
    /// priority) → `Event::Sound`) or retire each channel's head; true
    /// while any entry is queued.
    pub fn que_update(&mut self) -> bool {
        if self.sounds.rom.is_none() {
            return false;
        }
        let (busy, started) = self.audio_que.update(self.vb_counter as f32);
        for h in started {
            let pan = h.at.is_none().then_some(h.pan);
            self.events.push(Event::Sound { id: h.id, position: h.at, volume: h.volume, priority: h.priority, pan });
        }
        busy
    }

    /// AudioStillPlaying: AudioQueUpdate's result.
    pub fn audio_still_playing(&mut self) -> bool {
        self.que_update()
    }

    /// DEBUG: the voice queue's end times and entry count with vb_counter.
    pub fn audio_que_debug(&self) -> (u32, [f32; 2], usize) {
        (self.vb_counter, self.audio_que.endtime, self.audio_que.entries.iter().map(Vec::len).sum())
    }

    /// AudioSoundPlaying(id): a track of `id` not yet at its end.
    pub fn audio_sound_playing(&self, id: i32) -> bool {
        self.audio_que.playing(self.vb_counter as f32, id)
    }

    /// AudioQueEmpty.
    pub fn que_empty(&mut self) {
        self.audio_que.entries = Default::default();
        self.audio_que.endtime = [0.0; 2];
    }

    /// audio_stop_all: the queue emptied and the tracks killed (the host
    /// stops its voices itself at level changes).
    pub fn audio_stop_all(&mut self) {
        self.que_empty();
        self.audio_que.pending.clear();
        self.audio_que.tracks.clear();
    }

    /// main's per-frame audio: vb_counter advanced by the frame's vblanks,
    /// the replies to the previous frame's requests (AudioTrackSet), then
    /// AudioQueUpdate. Runs in every game mode (map, shop, play, pause).
    pub fn main_audio(&mut self, vb: i32) {
        self.vb_counter = self.vb_counter.wrapping_add(vb.max(0) as u32);
        let now = self.vb_counter as f32;
        let rom = self.sounds.rom.as_ref();
        self.audio_que.poll(now, |id| rom.and_then(|r| r.length_of(id)).unwrap_or(0.0));
        self.que_update();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: i32, duration: f32) -> QueEntry {
        QueEntry { duration, ..QueEntry::new(id, 0xe0, 0x7f, 2) }
    }

    /// Synthetic: two 30-vblank lines play back to back; the queue stays
    /// busy until the second one's end, and a third that would wait more
    /// than 0.5 s is refused.
    #[test]
    fn lines_serialise_and_max_wait_refuses() {
        let mut q = AudioQueue::default();
        assert_eq!(q.add(100.0, -1.0, 1, line(1, 30.0)), 30.0);
        assert_eq!(q.add(100.0, -1.0, 1, line(2, 30.0)), 30.0);
        assert_eq!(q.add(100.0, 0.5, 1, line(3, 30.0)), 0.0); // 60 vblanks ahead > 30
        let (busy, s) = q.update(100.0);
        assert!(busy && s.len() == 1 && s[0].id == 1);
        assert!(q.update(129.0).1.is_empty());
        let (busy, s) = q.update(130.0); // retired; the next call starts line 2
        assert!(busy && s.is_empty());
        let (_, s) = q.update(131.0);
        assert_eq!(s[0].id, 2);
        q.update(161.0);
        let (busy, _) = q.update(162.0);
        assert!(!busy);
    }

    /// Synthetic: a started line's track ends its duration after the reply.
    #[test]
    fn tracks_end_after_their_duration() {
        let mut q = AudioQueue::default();
        q.add(0.0, -1.0, 0, line(7, 60.0));
        q.update(0.0);
        q.poll(1.0, |_| 1.0);
        assert!(q.playing(60.0, 7));
        assert!(!q.playing(61.0, 7));
    }
}
