//! Network play (NATIVE: the original has none; research/network.md):
//! deterministic lockstep over TCP. Every machine runs the whole game and
//! only inputs travel. For each 30 Hz step a machine sends the host its
//! local controllers' pad states and its menu events; the host merges every
//! machine's input for that step into one [`Frame`] and sends it to all,
//! itself included, so every machine applies the same frame at the same
//! step. A machine sends its input for step N + delay when it applies step
//! N, so no machine runs more than `delay` steps ahead of the slowest.
//!
//! Machines connect to the host only (a star). Machine 0 is the host; the
//! others are numbered in the order they joined. Each owns some of the
//! game's controller ports 0..3 ([`ports_of`]), so a remote player joins on
//! the select screen by pressing Start on one of them, as a second
//! controller does on the couch.
//!
//! No Bevy types: std::net, threads and channels. The game side (gdl_play)
//! decides what the pads and events mean.
use gdl_core::game::pad::PadStatus;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

/// The wire format's version; machines with another one are refused (2:
/// round-trip pings and the host's arrival times, release 1.3.0).
pub const PROTOCOL: u16 = 2;
/// NATIVE: the port a host listens on unless the player picks another.
pub const DEFAULT_PORT: u16 = 28780;
/// Machines in one game, the host included.
pub const MAX_MACHINES: usize = 4;
/// The game's controller ports (two multitaps, PlayerControl's eight).
pub const PORTS: usize = 8;
/// Steps between state hash comparisons (one second).
pub const HASH_EVERY: u32 = 30;
/// Host flags, each in one frame only (the host raises it once per event):
/// its music stream part has ended, its game movie has ended. Every
/// machine's own playback ends at its own time; the game follows the
/// host's.
pub const FLAG_STREAM_DONE: u8 = 1;
pub const FLAG_MOVIE_DONE: u8 = 2;
const MAX_MESSAGE: usize = 1 << 20;
/// A machine silent this long while the others wait is dropped.
const SILENCE: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);
/// A client pings the host this often.
const PING_EVERY: Duration = Duration::from_millis(500);
/// The host reports arrival times every this many frames.
const SYNC_EVERY: u32 = 10;

/// The controller ports machine `m` of `n` owns, in its local players'
/// order: ports 0..3 dealt in turn (two machines: 0 and 2, 1 and 3), so each
/// machine's first player is player m + 1. ReadControls 0x104b60 makes Start
/// edges only on the ports of players 0..3 (an unassigned player k reads
/// port k), so a player on ports 4..7 could never join; single play uses
/// 0..3 too.
pub fn ports_of(m: usize, n: usize) -> Vec<usize> {
    (m..4).step_by(n.max(1)).collect()
}

/// What the host tells every machine when the game starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Start {
    pub machines: u8,
    /// This machine's number (0: the host).
    pub you: u8,
    /// Steps between a machine sampling its input and the game using it.
    pub delay: u8,
    /// The host's GamePrefs words (difficulty, multiplayer mode, …).
    pub prefs: [i32; 8],
    /// The host's memory card: eight save regions (None: empty file).
    pub card: Vec<Option<Vec<u8>>>,
}

/// One machine's input for one step: its ports' pads (its block, in order)
/// and its menu events (bytes the game encodes).
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    pub tick: u32,
    pub pads: Vec<PadStatus>,
    pub events: Vec<u8>,
}

/// Every machine's input for one step, as the host merged it.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub tick: u32,
    pub pads: [PadStatus; PORTS],
    /// Per machine, its menu events.
    pub events: Vec<Vec<u8>>,
    /// FLAG_* from the host (each in one frame).
    pub flags: u8,
    /// Machines that have left (bit m); their ports are disconnected.
    pub gone: u8,
}

/// What [`Link::poll`] reports.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Machines in the lobby, the host included.
    Lobby { machines: u8 },
    /// The game starts (the host's own start is returned by [`Link::start`]).
    Started(Start),
    /// A machine left a running game.
    Left { machine: u8 },
    /// The game is over for this machine (the host left, a refusal, the
    /// states differ, …).
    Ended(String),
}

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Hello { protocol: u16, version: String, data: [u8; 32] },
    Welcome,
    Refuse(String),
    Lobby { machines: u8 },
    Start(Start),
    Input(Input),
    Frame(Box<Frame>),
    Hash { tick: u32, hash: [u8; 32] },
    End(String),
    Ping { id: u32 },
    Pong { id: u32 },
    /// Per machine, how long before the last input of a frame its input
    /// arrives at the host (ms, a moving average).
    Sync { early_ms: Vec<u16> },
}

/// This machine's view of the connection's timing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Timing {
    /// The round trip to the host (a client; smoothed, ms).
    pub rtt_ms: Option<f32>,
    /// How much earlier than the last machine's this machine's input
    /// arrives at the host (ms): above a tick it is running ahead.
    pub early_ms: Option<f32>,
}

// ---- encoding ----------------------------------------------------------

struct W(Vec<u8>);

impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.0.extend_from_slice(v);
    }
    fn pad(&mut self, p: &PadStatus) {
        self.u8(u8::from(p.connected) | u8::from(p.analog) << 1 | u8::from(p.has_stick) << 2);
        for e in &p.entries {
            self.0.extend_from_slice(e);
        }
    }
}

struct Rd<'a> {
    b: &'a [u8],
    at: usize,
}

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.b.len()).ok_or("message ends early")?;
        let s = &self.b[self.at..end];
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn bytes(&mut self) -> Result<Vec<u8>, String> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }
    fn text(&mut self) -> Result<String, String> {
        String::from_utf8(self.bytes()?).map_err(|_| "text is not UTF-8".to_string())
    }
    fn pad(&mut self) -> Result<PadStatus, String> {
        let f = self.u8()?;
        let mut p = PadStatus { connected: f & 1 != 0, analog: f & 2 != 0, has_stick: f & 4 != 0, ..Default::default() };
        for e in &mut p.entries {
            e.copy_from_slice(self.take(2)?);
        }
        Ok(p)
    }
}

fn encode(m: &Msg) -> Vec<u8> {
    let mut w = W(Vec::new());
    match m {
        Msg::Hello { protocol, version, data } => {
            w.u8(1);
            w.0.extend_from_slice(b"GDLNET");
            w.u16(*protocol);
            w.bytes(version.as_bytes());
            w.0.extend_from_slice(data);
        }
        Msg::Welcome => w.u8(2),
        Msg::Refuse(why) => {
            w.u8(3);
            w.bytes(why.as_bytes());
        }
        Msg::Lobby { machines } => {
            w.u8(4);
            w.u8(*machines);
        }
        Msg::Start(s) => {
            w.u8(5);
            w.u8(s.machines);
            w.u8(s.you);
            w.u8(s.delay);
            for p in s.prefs {
                w.u32(p as u32);
            }
            w.u8(s.card.len() as u8);
            for f in &s.card {
                match f {
                    Some(b) => {
                        w.u8(1);
                        w.bytes(b);
                    }
                    None => w.u8(0),
                }
            }
        }
        Msg::Input(i) => {
            w.u8(6);
            w.u32(i.tick);
            w.u8(i.pads.len() as u8);
            for p in &i.pads {
                w.pad(p);
            }
            w.bytes(&i.events);
        }
        Msg::Frame(f) => {
            w.u8(7);
            w.u32(f.tick);
            for p in &f.pads {
                w.pad(p);
            }
            w.u8(f.events.len() as u8);
            for e in &f.events {
                w.bytes(e);
            }
            w.u8(f.flags);
            w.u8(f.gone);
        }
        Msg::Hash { tick, hash } => {
            w.u8(8);
            w.u32(*tick);
            w.0.extend_from_slice(hash);
        }
        Msg::End(why) => {
            w.u8(9);
            w.bytes(why.as_bytes());
        }
        Msg::Ping { id } => {
            w.u8(10);
            w.u32(*id);
        }
        Msg::Pong { id } => {
            w.u8(11);
            w.u32(*id);
        }
        Msg::Sync { early_ms } => {
            w.u8(12);
            w.u8(early_ms.len() as u8);
            for &e in early_ms {
                w.u16(e);
            }
        }
    }
    w.0
}

fn decode(b: &[u8]) -> Result<Msg, String> {
    let mut r = Rd { b, at: 0 };
    let m = match r.u8()? {
        1 => {
            if r.take(6)? != b"GDLNET" {
                return Err("not a Gauntlet: Dark Legacy network game".into());
            }
            let protocol = r.u16()?;
            let version = r.text()?;
            let data = r.take(32)?.try_into().unwrap();
            Msg::Hello { protocol, version, data }
        }
        2 => Msg::Welcome,
        3 => Msg::Refuse(r.text()?),
        4 => Msg::Lobby { machines: r.u8()? },
        5 => {
            let (machines, you, delay) = (r.u8()?, r.u8()?, r.u8()?);
            let mut prefs = [0; 8];
            for p in &mut prefs {
                *p = r.u32()? as i32;
            }
            let n = r.u8()?;
            let card = (0..n).map(|_| Ok(if r.u8()? != 0 { Some(r.bytes()?) } else { None })).collect::<Result<_, String>>()?;
            Msg::Start(Start { machines, you, delay, prefs, card })
        }
        6 => {
            let tick = r.u32()?;
            let n = r.u8()? as usize;
            let pads = (0..n.min(PORTS)).map(|_| r.pad()).collect::<Result<_, _>>()?;
            Msg::Input(Input { tick, pads, events: r.bytes()? })
        }
        7 => {
            let tick = r.u32()?;
            let mut pads = [PadStatus::default(); PORTS];
            for p in &mut pads {
                *p = r.pad()?;
            }
            let n = r.u8()?;
            let events = (0..n).map(|_| r.bytes()).collect::<Result<_, _>>()?;
            Msg::Frame(Box::new(Frame { tick, pads, events, flags: r.u8()?, gone: r.u8()? }))
        }
        8 => Msg::Hash { tick: r.u32()?, hash: r.take(32)?.try_into().unwrap() },
        9 => Msg::End(r.text()?),
        10 => Msg::Ping { id: r.u32()? },
        11 => Msg::Pong { id: r.u32()? },
        12 => {
            let n = r.u8()?;
            Msg::Sync { early_ms: (0..n).map(|_| r.u16()).collect::<Result<_, _>>()? }
        }
        t => return Err(format!("unknown message type {t}")),
    };
    Ok(m)
}

// ---- connections ---------------------------------------------------------

/// One TCP connection: a reader thread reporting messages (tagged with
/// `id`) and a writer thread, so the game never blocks on the network.
/// Closing ends the writer's queue: it sends what is queued, then shuts the
/// socket.
struct Conn {
    /// Each message with the time it was queued (the lag simulator's start).
    tx: Option<Sender<(Instant, Vec<u8>)>>,
}

impl Conn {
    fn open(stream: TcpStream, id: u8, events: Sender<(u8, Result<Msg, String>)>) -> std::io::Result<Self> {
        stream.set_nodelay(true)?;
        let mut reader = stream.try_clone()?;
        let mut writer = stream;
        std::thread::spawn(move || {
            loop {
                let mut len = [0u8; 4];
                let r = reader.read_exact(&mut len).map_err(|e| e.to_string()).and_then(|_| {
                    let n = u32::from_le_bytes(len) as usize;
                    if MAX_MESSAGE < n {
                        return Err(format!("message of {n} bytes"));
                    }
                    let mut b = vec![0; n];
                    reader.read_exact(&mut b).map_err(|e| e.to_string())?;
                    decode(&b)
                });
                let failed = r.is_err();
                if events.send((id, r)).is_err() || failed {
                    return;
                }
            }
        });
        let (tx, rx) = channel::<(Instant, Vec<u8>)>();
        let mut lag = Lag::from_env();
        std::thread::spawn(move || {
            for (queued, b) in rx {
                if let Some(l) = lag.as_mut() {
                    l.wait(queued);
                }
                let mut out = (b.len() as u32).to_le_bytes().to_vec();
                out.extend_from_slice(&b);
                if writer.write_all(&out).is_err() {
                    break;
                }
            }
            let _ = writer.flush();
            let _ = writer.shutdown(Shutdown::Both);
        });
        Ok(Conn { tx: Some(tx) })
    }

    fn send(&self, m: &Msg) {
        if let Some(tx) = self.tx.as_ref() {
            let _ = tx.send((Instant::now(), encode(m)));
        }
    }

    fn close(&mut self) {
        self.tx = None;
    }
}

/// TEST: GDL_NET_LAG_MS (one way) and GDL_NET_JITTER_MS delay every message
/// a connection sends — a distant network on one machine. Each message
/// leaves `lag` + a uniform 0..`jitter` ms after it was queued, never before
/// the one queued ahead of it (TCP keeps the order).
struct Lag {
    lag: f64,
    jitter: f64,
    seed: u64,
    last: Instant,
}

impl Lag {
    fn from_env() -> Option<Lag> {
        let ms = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<f64>().ok()).filter(|v| 0.0 < *v);
        let (lag, jitter) = (ms("GDL_NET_LAG_MS"), ms("GDL_NET_JITTER_MS"));
        (lag.is_some() || jitter.is_some()).then(|| Lag {
            lag: lag.unwrap_or(0.0),
            jitter: jitter.unwrap_or(0.0),
            seed: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1,
            last: Instant::now(),
        })
    }

    /// Blocks the writer until the message queued at `queued` may leave.
    fn wait(&mut self, queued: Instant) {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        let u = (self.seed >> 11) as f64 / (1u64 << 53) as f64;
        let at = (queued + Duration::from_secs_f64((self.lag + u * self.jitter) / 1000.0)).max(self.last);
        self.last = at;
        let now = Instant::now();
        if now < at {
            std::thread::sleep(at - now);
        }
    }
}

// ---- host ---------------------------------------------------------------

struct Peer {
    conn: Conn,
    /// The handshake passed.
    ready: bool,
    /// The peer's id on its reader thread's messages.
    id: u8,
    heard: Instant,
}

struct HostGame {
    machines: u8,
    /// Machine m's peer index (machine 0 is the host itself).
    peers: Vec<usize>,
    /// Per step, each machine's input and when it arrived.
    inputs: BTreeMap<u32, Vec<Option<(Input, Instant)>>>,
    next: u32,
    ready: std::collections::VecDeque<Frame>,
    gone: u8,
    hashes: BTreeMap<u32, Vec<Option<[u8; 32]>>>,
    ended: bool,
    /// Per machine, how long before a frame's last input its own arrives
    /// (ms, a moving average).
    early_ms: Vec<f32>,
}

pub struct Host {
    port: u16,
    version: String,
    data: [u8; 32],
    /// Non-blocking, accepted from in poll: dropping the host frees the port.
    listener: TcpListener,
    events_tx: Sender<(u8, Result<Msg, String>)>,
    // (Mutex: a Bevy resource is Sync; only `get_mut` is used.)
    events: Mutex<Receiver<(u8, Result<Msg, String>)>>,
    peers: Vec<Peer>,
    next_id: u8,
    game: Option<HostGame>,
    /// FLAG_* for the next frame built.
    pub flags: u8,
    lobby_reported: u8,
    /// Events from building frames outside poll (submit).
    pending: Vec<Event>,
}

impl Host {
    fn listen(port: u16, version: &str, data: [u8; 32]) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("0.0.0.0", port))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let (events_tx, events) = channel();
        Ok(Host { port, version: version.into(), data, listener, events_tx, events: Mutex::new(events), peers: Vec::new(), next_id: 1, game: None, flags: 0, lobby_reported: 0, pending: Vec::new() })
    }

    fn machines(&self) -> u8 {
        1 + self.peers.iter().filter(|p| p.ready).count() as u8
    }

    fn refuse(stream: TcpStream, why: &str) {
        let mut s = stream;
        let b = encode(&Msg::Refuse(why.into()));
        let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
        let mut out = (b.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(&b);
        let _ = s.write_all(&out);
        let _ = s.shutdown(Shutdown::Both);
    }

    fn poll(&mut self, out: &mut Vec<Event>) {
        while let Ok((s, _)) = self.listener.accept() {
            // (Windows: an accepted socket inherits non-blocking.)
            if s.set_nonblocking(false).is_err() {
                continue;
            }
            if self.game.is_some() {
                Self::refuse(s, "the game has already started");
            } else if MAX_MACHINES <= 1 + self.peers.len() {
                Self::refuse(s, "the game is full");
            } else {
                let id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1).max(1);
                if let Ok(conn) = Conn::open(s, id, self.events_tx.clone()) {
                    self.peers.push(Peer { conn, ready: false, id, heard: Instant::now() });
                }
            }
        }
        while let Ok((id, m)) = self.events.get_mut().unwrap().try_recv() {
            let Some(k) = self.peers.iter().position(|p| p.id == id) else { continue };
            self.peers[k].heard = Instant::now();
            match m {
                Ok(Msg::Hello { protocol, version, data }) if !self.peers[k].ready => {
                    let why = if protocol != PROTOCOL || version != self.version {
                        Some(format!("the host runs version {} (protocol {PROTOCOL}), this machine {version} (protocol {protocol})", self.version))
                    } else if data != self.data {
                        Some("the two machines' game data differ (another disc or import)".into())
                    } else {
                        None
                    };
                    match why {
                        Some(why) => {
                            self.peers[k].conn.send(&Msg::Refuse(why));
                            self.drop_peer(k, out);
                        }
                        None => {
                            self.peers[k].ready = true;
                            self.peers[k].conn.send(&Msg::Welcome);
                        }
                    }
                }
                Ok(Msg::Input(i)) => {
                    if let Some(g) = self.game.as_mut()
                        && let Some(m) = g.peers.iter().position(|&p| p == k)
                        && g.next <= i.tick
                        && i.tick < g.next + 1024
                    {
                        let (n, t) = (g.machines as usize, i.tick);
                        g.inputs.entry(t).or_insert_with(|| vec![None; n])[m] = Some((i, Instant::now()));
                    }
                }
                Ok(Msg::Ping { id }) => self.peers[k].conn.send(&Msg::Pong { id }),
                Ok(Msg::Hash { tick, hash }) => {
                    if let Some(g) = self.game.as_mut()
                        && let Some(m) = g.peers.iter().position(|&p| p == k)
                    {
                        let n = g.machines as usize;
                        g.hashes.entry(tick).or_insert_with(|| vec![None; n])[m] = Some(hash);
                    }
                }
                Ok(_) => {}
                Err(_) => self.drop_peer(k, out),
            }
        }
        // A machine silent while the game waits on it is dropped.
        if let Some(g) = self.game.as_ref() {
            let waiting: Vec<usize> = (1..g.machines as usize)
                .filter(|&m| g.gone & 1 << m == 0 && g.inputs.get(&g.next).is_none_or(|v| v[m].is_none()))
                .map(|m| g.peers[m])
                .filter(|&k| SILENCE < self.peers[k].heard.elapsed())
                .collect();
            for k in waiting {
                self.drop_peer(k, out);
            }
        }
        let machines = self.machines();
        if self.game.is_none() && machines != self.lobby_reported {
            self.lobby_reported = machines;
            for p in self.peers.iter().filter(|p| p.ready) {
                p.conn.send(&Msg::Lobby { machines });
            }
            out.push(Event::Lobby { machines });
        }
        self.build();
        out.append(&mut self.pending);
    }

    /// A peer's connection ends: removed in the lobby, marked gone in a
    /// game (its ports read as unplugged from the next frame built on).
    fn drop_peer(&mut self, k: usize, out: &mut Vec<Event>) {
        self.peers[k].conn.close();
        match self.game.as_mut() {
            None => {
                self.peers.remove(k);
            }
            Some(g) => {
                if let Some(m) = g.peers.iter().position(|&p| p == k)
                    && g.gone & 1 << m == 0
                {
                    g.gone |= 1 << m;
                    out.push(Event::Left { machine: m as u8 });
                }
            }
        }
    }

    fn start(&mut self, delay: u8, prefs: [i32; 8], card: Vec<Option<Vec<u8>>>) -> Start {
        // Machines still in their handshake are left out.
        let (ready, waiting): (Vec<Peer>, Vec<Peer>) = std::mem::take(&mut self.peers).into_iter().partition(|p| p.ready);
        for mut p in waiting {
            p.conn.send(&Msg::Refuse("the host started without this machine".into()));
            p.conn.close();
        }
        self.peers = ready;
        let machines = 1 + self.peers.len() as u8;
        let start = |you: u8| Start { machines, you, delay, prefs, card: card.clone() };
        for (k, p) in self.peers.iter().enumerate() {
            p.conn.send(&Msg::Start(start(k as u8 + 1)));
        }
        self.game = Some(HostGame {
            machines,
            peers: std::iter::once(usize::MAX).chain(0..self.peers.len()).collect(),
            inputs: BTreeMap::new(),
            next: 0,
            ready: Default::default(),
            gone: 0,
            hashes: BTreeMap::new(),
            ended: false,
            early_ms: vec![0.0; machines as usize],
        });
        start(0)
    }

    /// Frames for every step all present machines have sent.
    fn build(&mut self) {
        let Some(g) = self.game.as_mut() else { return };
        let n = g.machines as usize;
        while let Some(v) = g.inputs.get(&g.next)
            && (0..n).all(|m| g.gone & 1 << m != 0 || v[m].is_some())
        {
            let v = g.inputs.remove(&g.next).unwrap();
            let mut f = Frame { tick: g.next, pads: [PadStatus::default(); PORTS], events: vec![Vec::new(); n], flags: self.flags, gone: g.gone };
            // The machines' arrival times: how early each was before the last.
            let last = v.iter().flatten().map(|(_, at)| *at).max();
            for (m, i) in v.into_iter().enumerate() {
                let Some((i, at)) = i.filter(|_| g.gone & 1 << m == 0) else { continue };
                if let Some(last) = last {
                    let early = (last - at).as_secs_f32() * 1000.0;
                    g.early_ms[m] += (early - g.early_ms[m]) * 0.1;
                }
                for (port, pad) in ports_of(m, n).into_iter().zip(i.pads) {
                    f.pads[port] = pad;
                }
                f.events[m] = i.events;
            }
            for &k in g.peers.iter().skip(1) {
                self.peers[k].conn.send(&Msg::Frame(Box::new(f.clone())));
            }
            g.ready.push_back(f);
            g.next += 1;
            self.flags = 0;
            if g.next.is_multiple_of(SYNC_EVERY) {
                let early_ms: Vec<u16> = g.early_ms.iter().map(|&e| e.clamp(0.0, 60000.0) as u16).collect();
                for &k in g.peers.iter().skip(1) {
                    self.peers[k].conn.send(&Msg::Sync { early_ms: early_ms.clone() });
                }
            }
        }
        // Hashes every machine still in the game has reported.
        let done: Vec<u32> = g.hashes.iter().filter(|(_, h)| (0..n).all(|m| g.gone & 1 << m != 0 || h[m].is_some())).map(|(&t, _)| t).collect();
        for t in done {
            let h = g.hashes.remove(&t).unwrap();
            let mut seen = h.iter().flatten();
            let first = seen.next().copied();
            if !g.ended && seen.any(|x| Some(*x) != first) {
                g.ended = true;
                let why = format!("the machines' games differ at step {t} (desynchronised)");
                for &k in g.peers.iter().skip(1) {
                    self.peers[k].conn.send(&Msg::End(why.clone()));
                }
                self.pending.push(Event::Ended(why));
            }
        }
    }
}

// ---- client -------------------------------------------------------------

pub struct Client {
    address: String,
    version: String,
    data: [u8; 32],
    connecting: Option<Mutex<Receiver<Result<TcpStream, String>>>>,
    conn: Option<Conn>,
    events_tx: Sender<(u8, Result<Msg, String>)>,
    events: Mutex<Receiver<(u8, Result<Msg, String>)>>,
    frames: BTreeMap<u32, Frame>,
    ended: bool,
    welcomed: bool,
    /// Machines the frames so far report gone.
    gone: u8,
    /// This machine's number once the game started.
    you: Option<u8>,
    /// Pings in flight (id → sent) and the smoothed round trip and its
    /// deviation (RFC 6298's estimator, ms).
    pings: BTreeMap<u32, Instant>,
    next_ping: u32,
    pinged: Instant,
    srtt: Option<f32>,
    rttvar: f32,
    early_ms: Option<f32>,
}

impl Client {
    fn join(address: &str, version: &str, data: [u8; 32]) -> Self {
        let (tx, rx) = channel();
        let target = address.to_string();
        std::thread::spawn(move || {
            let r = (|| {
                let addrs: Vec<_> = target.to_socket_addrs().map_err(|e| format!("{target}: {e}"))?.collect();
                let mut last = format!("{target}: no address");
                for a in addrs {
                    match TcpStream::connect_timeout(&a, CONNECT_TIMEOUT) {
                        Ok(s) => return Ok(s),
                        Err(e) => last = format!("{a}: {e}"),
                    }
                }
                Err(last)
            })();
            let _ = tx.send(r);
        });
        let (events_tx, events) = channel();
        Client { address: address.into(), version: version.into(), data, connecting: Some(Mutex::new(rx)), conn: None, events_tx, events: Mutex::new(events), frames: BTreeMap::new(), ended: false, welcomed: false, gone: 0, you: None, pings: BTreeMap::new(), next_ping: 0, pinged: Instant::now(), srtt: None, rttvar: 0.0, early_ms: None }
    }

    fn end(&mut self, why: String, out: &mut Vec<Event>) {
        if !self.ended {
            self.ended = true;
            if let Some(mut c) = self.conn.take() {
                c.close();
            }
            out.push(Event::Ended(why));
        }
    }

    fn poll(&mut self, out: &mut Vec<Event>) {
        if let Some(rx) = self.connecting.as_mut() {
            match rx.get_mut().unwrap().try_recv() {
                Ok(Ok(s)) => {
                    self.connecting = None;
                    match Conn::open(s, 0, self.events_tx.clone()) {
                        Ok(c) => {
                            c.send(&Msg::Hello { protocol: PROTOCOL, version: self.version.clone(), data: self.data });
                            self.conn = Some(c);
                        }
                        Err(e) => self.end(format!("could not connect: {e}"), out),
                    }
                }
                Ok(Err(e)) => {
                    self.connecting = None;
                    self.end(format!("could not connect to {}", e), out);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.connecting = None;
                    self.end("could not connect".into(), out);
                }
            }
        }
        if self.conn.is_some() && self.welcomed && PING_EVERY <= self.pinged.elapsed() {
            self.pinged = Instant::now();
            self.pings.insert(self.next_ping, self.pinged);
            // (Pings lost with a dropped connection are forgotten.)
            self.pings.retain(|_, at| at.elapsed() < Duration::from_secs(10));
            let id = self.next_ping;
            self.next_ping = self.next_ping.wrapping_add(1);
            self.send(&Msg::Ping { id });
        }
        while let Ok((_, m)) = self.events.get_mut().unwrap().try_recv() {
            match m {
                Ok(Msg::Welcome) => self.welcomed = true,
                Ok(Msg::Lobby { machines }) => out.push(Event::Lobby { machines }),
                Ok(Msg::Start(s)) => {
                    self.you = Some(s.you);
                    out.push(Event::Started(s));
                }
                Ok(Msg::Pong { id }) => {
                    if let Some(sent) = self.pings.remove(&id) {
                        let r = sent.elapsed().as_secs_f32() * 1000.0;
                        match self.srtt {
                            None => {
                                self.srtt = Some(r);
                                self.rttvar = r / 2.0;
                            }
                            Some(s) => {
                                self.rttvar += ((s - r).abs() - self.rttvar) * 0.25;
                                self.srtt = Some(s + (r - s) * 0.125);
                            }
                        }
                    }
                }
                Ok(Msg::Sync { early_ms }) => self.early_ms = self.you.and_then(|m| early_ms.get(m as usize)).map(|&e| f32::from(e)),
                Ok(Msg::Frame(f)) => {
                    for m in 0..8 {
                        if f.gone & !self.gone & 1 << m != 0 {
                            out.push(Event::Left { machine: m });
                        }
                    }
                    self.gone |= f.gone;
                    self.frames.insert(f.tick, *f);
                }
                Ok(Msg::Refuse(why)) => self.end(format!("the host refused: {why}"), out),
                Ok(Msg::End(why)) => self.end(why, out),
                Ok(_) => {}
                Err(_) => self.end("the connection to the host was lost".into(), out),
            }
        }
    }

    fn send(&self, m: &Msg) {
        if let Some(c) = self.conn.as_ref() {
            c.send(m);
        }
    }
}

// ---- link ---------------------------------------------------------------

/// This machine's side of a network game: the host's or a client's.
pub enum Link {
    Host(Host),
    Client(Client),
}

impl Link {
    /// Host a game on `port` (0: any free port). `version` and `data`
    /// (the game data's identity) must match every joining machine's.
    pub fn host(port: u16, version: &str, data: [u8; 32]) -> std::io::Result<Link> {
        Host::listen(port, version, data).map(Link::Host)
    }

    /// Join the game at `address` ("host:port").
    pub fn join(address: &str, version: &str, data: [u8; 32]) -> Link {
        Link::Client(Client::join(address, version, data))
    }

    pub fn is_host(&self) -> bool {
        matches!(self, Link::Host(_))
    }

    /// The listening port (host) or the address joined.
    pub fn describe(&self) -> String {
        match self {
            Link::Host(h) => format!("port {}", h.port),
            Link::Client(c) => c.address.clone(),
        }
    }

    /// A client: the host accepted this machine.
    pub fn joined(&self) -> bool {
        matches!(self, Link::Client(c) if c.welcomed)
    }

    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        match self {
            Link::Host(h) => h.poll(&mut out),
            Link::Client(c) => c.poll(&mut out),
        }
        out
    }

    /// The host starts the game with every machine that has joined; its
    /// own [`Start`] (machine 0). None for a client.
    pub fn start(&mut self, delay: u8, prefs: [i32; 8], card: Vec<Option<Vec<u8>>>) -> Option<Start> {
        match self {
            Link::Host(h) if h.game.is_none() => Some(h.start(delay.clamp(1, 30), prefs, card)),
            _ => None,
        }
    }

    /// This machine's input for step `input.tick`.
    pub fn submit(&mut self, input: Input) {
        match self {
            Link::Host(h) => {
                if let Some(g) = h.game.as_mut()
                    && g.next <= input.tick
                {
                    let (n, t) = (g.machines as usize, input.tick);
                    g.inputs.entry(t).or_insert_with(|| vec![None; n])[0] = Some((input, Instant::now()));
                }
                h.build();
            }
            Link::Client(c) => c.send(&Msg::Input(input)),
        }
    }

    /// The merged frame of step `tick`, once every machine's input is in.
    pub fn frame(&mut self, tick: u32) -> Option<Frame> {
        match self {
            Link::Host(h) => {
                let g = h.game.as_mut()?;
                while g.ready.front().is_some_and(|f| f.tick < tick) {
                    g.ready.pop_front();
                }
                g.ready.front().is_some_and(|f| f.tick == tick).then(|| g.ready.pop_front()).flatten()
            }
            Link::Client(c) => c.frames.remove(&tick),
        }
    }

    /// This machine's game state hash after step `tick` (compared by the
    /// host; a difference ends the game on every machine).
    pub fn report_hash(&mut self, tick: u32, hash: [u8; 32]) {
        match self {
            Link::Host(h) => {
                if let Some(g) = h.game.as_mut() {
                    let n = g.machines as usize;
                    g.hashes.entry(tick).or_insert_with(|| vec![None; n])[0] = Some(hash);
                }
            }
            Link::Client(c) => c.send(&Msg::Hash { tick, hash }),
        }
    }

    /// The round trip to the host and how early this machine's input
    /// arrives there (both None until measured).
    pub fn timing(&self) -> Timing {
        match self {
            Link::Host(h) => Timing { rtt_ms: Some(0.0), early_ms: h.game.as_ref().map(|g| g.early_ms[0]) },
            Link::Client(c) => Timing { rtt_ms: c.srtt, early_ms: c.early_ms },
        }
    }

    /// The host raises FLAG_* for the next frame built (clients: ignored).
    pub fn raise(&mut self, flags: u8) {
        if let Link::Host(h) = self {
            h.flags |= flags;
        }
    }

    /// Leave: a host tells everyone the game is over.
    pub fn close(&mut self, why: &str) {
        match self {
            Link::Host(h) => {
                for p in &mut h.peers {
                    p.conn.send(&Msg::End(why.into()));
                    p.conn.close();
                }
            }
            Link::Client(c) => {
                if let Some(mut conn) = c.conn.take() {
                    conn.close();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(i: usize) -> PadStatus {
        let mut p = PadStatus { connected: true, ..Default::default() };
        p.press(i);
        p
    }

    #[test]
    fn messages_round_trip() {
        let msgs = [
            Msg::Hello { protocol: PROTOCOL, version: "1.2.0".into(), data: [7; 32] },
            Msg::Welcome,
            Msg::Refuse("full".into()),
            Msg::Lobby { machines: 3 },
            Msg::Start(Start { machines: 2, you: 1, delay: 4, prefs: [1, 2, 3, 4, 5, -1, 6, 7], card: vec![None, Some(vec![1, 2, 3]), None] }),
            Msg::Input(Input { tick: 9, pads: vec![pad(0xd), PadStatus::default()], events: vec![1, 2, 0x81] }),
            Msg::Frame(Box::new(Frame { tick: 70000, pads: std::array::from_fn(pad), events: vec![vec![], vec![5]], flags: FLAG_MOVIE_DONE, gone: 2 })),
            Msg::Hash { tick: 30, hash: [9; 32] },
            Msg::End("bye".into()),
            Msg::Ping { id: 7 },
            Msg::Pong { id: 7 },
            Msg::Sync { early_ms: vec![0, 33, 65535] },
        ];
        for m in msgs {
            assert_eq!(decode(&encode(&m)).unwrap(), m);
        }
        assert!(decode(&[6, 1, 0]).is_err());
        assert!(decode(&[200]).is_err());
    }

    /// A joined client measures its round trip (pings) and learns how early
    /// its inputs arrive (the host's Sync every SYNC_EVERY frames).
    #[test]
    fn timing() {
        let mut host = Link::host(0, "t", [1; 32]).unwrap();
        let addr = format!("127.0.0.1:{}", host.describe().trim_start_matches("port "));
        let mut c = Link::join(&addr, "t", [1; 32]);
        wait(|| {
            c.poll();
            host.poll().into_iter().any(|e| e == Event::Lobby { machines: 2 }).then_some(())
        });
        host.start(2, [0; 8], Vec::new()).unwrap();
        wait(|| c.poll().into_iter().any(|e| matches!(e, Event::Started(_))).then_some(()));
        for t in 0..SYNC_EVERY {
            host.submit(Input { tick: t, pads: vec![pad(0)], events: vec![] });
            c.submit(Input { tick: t, pads: vec![pad(1)], events: vec![] });
        }
        let t = wait(|| {
            host.poll();
            c.poll();
            let t = c.timing();
            (t.rtt_ms.is_some() && t.early_ms.is_some()).then_some(t)
        });
        assert!(t.rtt_ms.unwrap() < 1000.0);
        assert!(host.timing().early_ms.is_some());
    }

    /// Leaving a lobby frees the port: hosting again on it works.
    #[test]
    fn host_again() {
        let h = Link::host(0, "t", [1; 32]).unwrap();
        let port: u16 = h.describe().trim_start_matches("port ").parse().unwrap();
        drop(h);
        let mut again = Link::host(port, "t", [1; 32]).unwrap();
        again.close("");
    }

    #[test]
    fn ports() {
        assert_eq!(ports_of(0, 1), [0, 1, 2, 3]);
        assert_eq!((ports_of(0, 2), ports_of(1, 2)), (vec![0, 2], vec![1, 3]));
        assert_eq!((ports_of(0, 3), ports_of(1, 3), ports_of(2, 3)), (vec![0, 3], vec![1], vec![2]));
        assert_eq!(ports_of(3, 4), [3]);
    }

    fn wait<T>(mut f: impl FnMut() -> Option<T>) -> T {
        let t = Instant::now();
        loop {
            if let Some(v) = f() {
                return v;
            }
            assert!(t.elapsed() < Duration::from_secs(10), "timed out");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A host and two clients on this machine: the lobby, the start, merged
    /// frames, a hash difference and a client leaving.
    #[test]
    fn lockstep_on_localhost() {
        let mut host = Link::host(0, "t", [1; 32]).unwrap();
        let addr = format!("127.0.0.1:{}", host.describe().trim_start_matches("port "));
        let mut a = Link::join(&addr, "t", [1; 32]);
        let mut b = Link::join(&addr, "t", [1; 32]);
        let mut bad = Link::join(&addr, "other", [1; 32]);
        wait(|| {
            let ev = host.poll();
            a.poll();
            b.poll();
            ev.contains(&Event::Lobby { machines: 3 }).then_some(())
        });
        let refused = wait(|| {
            host.poll();
            bad.poll().into_iter().find(|e| matches!(e, Event::Ended(_)))
        });
        assert!(matches!(refused, Event::Ended(w) if w.contains("version")));
        let card = vec![Some(vec![4; 16]), None];
        let mine = host.start(2, [0; 8], card.clone()).unwrap();
        assert_eq!((mine.machines, mine.you), (3, 0));
        let mut sa = wait(|| a.poll().into_iter().find_map(|e| if let Event::Started(s) = e { Some(s) } else { None }));
        let mut sb = wait(|| b.poll().into_iter().find_map(|e| if let Event::Started(s) = e { Some(s) } else { None }));
        // Machines are numbered in the order they connected.
        if sa.you == 2 {
            std::mem::swap(&mut a, &mut b);
            std::mem::swap(&mut sa, &mut sb);
        }
        assert_eq!((sa.you, sb.you, sa.card.clone()), (1, 2, card));
        host.raise(FLAG_MOVIE_DONE);
        for t in 0..3u32 {
            host.submit(Input { tick: t, pads: vec![pad(0); 2], events: vec![] });
            a.submit(Input { tick: t, pads: vec![pad(1); 2], events: vec![t as u8] });
            b.submit(Input { tick: t, pads: vec![pad(2); 2], events: vec![] });
        }
        for t in 0..3u32 {
            let fh = wait(|| {
                host.poll();
                host.frame(t)
            });
            let fa = wait(|| {
                a.poll();
                a.frame(t)
            });
            let fb = wait(|| {
                b.poll();
                b.frame(t)
            });
            assert_eq!(fh, fa);
            assert_eq!(fh, fb);
            // Three machines: the host has ports 0 and 3, the others 1 and 2.
            assert_eq!((fh.pads[0], fh.pads[1], fh.pads[2], fh.pads[3], fh.pads[4].connected), (pad(0), pad(1), pad(2), pad(0), false));
            assert_eq!(fh.events, vec![vec![], vec![t as u8], vec![]]);
            assert_eq!(fh.flags, if t == 0 { FLAG_MOVIE_DONE } else { 0 });
        }
        // A client leaving: its ports read as unplugged from then on.
        b.close("");
        wait(|| host.poll().into_iter().find(|e| *e == Event::Left { machine: 2 }));
        host.submit(Input { tick: 3, pads: vec![pad(0); 2], events: vec![] });
        a.submit(Input { tick: 3, pads: vec![pad(1); 2], events: vec![] });
        let f = wait(|| {
            host.poll();
            host.frame(3)
        });
        assert_eq!((f.gone, f.pads[1], f.pads[2].connected), (4, pad(1), false));
        let fa = wait(|| a.poll().into_iter().find(|e| *e == Event::Left { machine: 2 }).and_then(|_| a.frame(3)).or_else(|| a.frame(3)));
        assert_eq!(fa, f);
        // Equal hashes pass; a different one ends the game everywhere.
        host.report_hash(30, [1; 32]);
        a.report_hash(30, [1; 32]);
        host.report_hash(60, [1; 32]);
        a.report_hash(60, [2; 32]);
        let ended = wait(|| host.poll().into_iter().find(|e| matches!(e, Event::Ended(_))));
        assert!(matches!(ended, Event::Ended(w) if w.contains("step 60")));
        wait(|| a.poll().into_iter().find(|e| matches!(e, Event::Ended(w) if w.contains("step 60"))));
    }
}
