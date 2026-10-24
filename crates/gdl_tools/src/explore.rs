//! State-search traversal for game-run (M14/M15 evidence tooling; not game
//! logic and not a fidelity input). A Go-Explore search over native `Game`
//! clones, driving port 0 through the ordinary pad path (ReadControls →
//! PlayerControls → do_players), so a result is a pad script that replays
//! from a fresh load (in process at the end of the run, and through
//! game-run's pads=/padfile=).
//!
//! The archive holds one state per cell — player 0's position in boxes of
//! BOX × BOX_Y × BOX, the keys held and a progress count (keys picked up,
//! gates opened, triggers set, legend items taken: kind 1 subtype 2 or 0xd
//! records gone, kind 7 records gone or with their state +0xdc set, kind 5
//! records with +0xde or +0xe2 set) — the
//! first state that reached it in the fewest updates. Each iteration restores a chosen cell and plays one
//! macro:
//! - random: one of the eight d-pad directions or none for MIN_LEN..=MAX_LEN
//!   updates, a quarter of them with cross on alternate updates;
//! - guided (nav=1): a bot.rs graph path to an item of interest or a random
//!   reachable node within EXPLORE_RADIUS, walked with the d-pad, edges it
//!   gets stuck on learnt as blocked;
//! - bot (nav=2): the traversal bot itself (bot.rs, on the shared graph) for
//!   BOT_LEN updates, with no hint (the bot's own goals from the restored
//!   state) or a random first route hint (an item of interest or a reachable
//!   node), its states filed every BOT_FILE updates.
//!
//! A cell at the highest progress is chosen PROGRESS_BIAS times as often as
//! one a step below, and less the more it has been chosen. On an exit portal
//! (contact item +0x884, kind 9) the pad is released for up to PORTAL_WAIT
//! updates (DoExit counts only then). The search ends with the level
//! (do_exit's destination wave, a treasure room's timer, a boss's auto exit)
//! or at the update budget. Health assist (labelled DEBUG, as the bot's):
//! health is restored to its start value when it falls below 40 % of it,
//! before the update.
use crate::bot::{Bot, Hint, Nav};
use gdl_core::game::Game;
use gdl_core::game::pad::PadStatus;
use std::collections::{BTreeMap, HashMap, HashSet};

const HEALTH: usize = 0x1a10;
const KEYS: usize = 0x1a14;
const BOX: f32 = 4.0;
const BOX_Y: f32 = 3.0;
const MIN_LEN: u32 = 6;
const MAX_LEN: u32 = 40;
const PORTAL_WAIT: u32 = 120;
const PROGRESS_BIAS: f64 = 4.0;
/// joyGetStatus entries: d-pad left 0, right 1, up 2, down 3; cross 0xd.
const DIRS: [u32; 9] = [0, 1 << 2, 1 << 2 | 1 << 1, 1 << 1, 1 << 3 | 1 << 1, 1 << 3, 1 << 3 | 1, 1, 1 << 2 | 1];
const CROSS: u32 = 1 << 0xd;
/// Share of guided (or bot) macros when the graph is in use.
const NAV_SHARE: f64 = 0.8;
/// A guided walk's update limit, and its stuck test (less than STUCK_GAIN
/// closer to the path's node over STUCK_UPDATES).
const WALK_LIMIT: u32 = 360;
const STUCK_UPDATES: u32 = 45;
const STUCK_GAIN: f32 = 0.5;
/// Random walk goals lie within this distance.
const EXPLORE_RADIUS: f32 = 40.0;
/// Bot macros: length and filing interval.
const BOT_LEN: u32 = 3000;
const BOT_FILE: u32 = 150;

type V3 = [f32; 3];
/// Cell: position box, keys held, progress.
type Key = (i32, i32, i32, i32, u32);
/// Edges walks got stuck on, per (keys held, items of interest left).
type Blocked = HashMap<(i32, usize), HashSet<(crate::bot::Place, crate::bot::Place)>>;

/// One update's port-0 input: the digital entries pressed and, for an
/// analog pad, the stick entries 4..7.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pad {
    pub mask: u32,
    pub analog: Option<[u8; 4]>,
}

impl Pad {
    pub fn digital(mask: u32) -> Pad {
        Pad { mask, analog: None }
    }

    fn of(s: &PadStatus) -> Pad {
        let mut mask = 0;
        for (i, e) in s.entries.iter().enumerate() {
            if !(4..8).contains(&i) && e[0] & 1 != 0 && e[1] != 0 {
                mask |= 1 << i;
            }
        }
        let analog = s.analog.then(|| [s.entries[4][1], s.entries[5][1], s.entries[6][1], s.entries[7][1]]);
        Pad { mask, analog }
    }

    /// The PadStatus game-run's pads= path (digital) or the bot (analog) gives.
    pub fn status(&self) -> PadStatus {
        let mut s = PadStatus { connected: true, ..Default::default() };
        if let Some(a) = self.analog {
            s.analog = true;
            s.has_stick = true;
            for (k, v) in a.iter().enumerate() {
                s.entries[4 + k] = [3, *v];
            }
        }
        for i in 0..0x16 {
            if self.mask >> i & 1 != 0 {
                s.press(i);
            }
        }
        s
    }
}

/// One macro's pads as runs (pad, updates).
type Runs = Vec<(Pad, u32)>;

fn push_run(runs: &mut Runs, p: Pad) {
    match runs.last_mut() {
        Some(l) if l.0 == p => l.1 += 1,
        _ => runs.push((p, 1)),
    }
}

struct Node {
    parent: Option<usize>,
    runs: Runs,
}

struct Cell {
    state: Box<Game>,
    node: Option<usize>,
    updates: usize,
    chosen: u32,
    progress: u32,
}

/// xorshift64* (deterministic per seed).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: u32) -> u32 {
        (self.next() >> 33) as u32 % n
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn item_kind(g: &Game, slot: usize) -> (i32, i32) {
    let d = &g.items.definitions[g.items.records[slot].definition()];
    (d.kind(), d.subtype())
}

/// The level ended: do_exit's destination wave, or game_main's exit test
/// after a boss's auto exit (players done, exit ready).
pub fn level_ended(g: &Game) -> bool {
    g.level_done.is_some() || (g.players_done && g.exit_ready)
}

/// The labelled DEBUG health assist (as bot.rs): applied before an update.
pub fn assist(g: &mut Game, start_health: f32) -> bool {
    if g.players[0].i32_(0xfc) == 1 && g.players[0].f32_(HEALTH) < start_health * 0.4 {
        g.players[0].set_f32(HEALTH, start_health);
        return true;
    }
    false
}

/// One update with port 0's `pad` (game-run's pads= path).
fn step(g: &mut Game, pad: Pad, start_health: f32, assists: &mut usize) {
    if assist(g, start_health) {
        *assists += 1;
    }
    let mut pads = [PadStatus::default(); 8];
    pads[0] = pad.status();
    for (k, pd) in pads.iter_mut().enumerate().take(4).skip(1) {
        pd.connected = g.globals.active_players >> k & 1 != 0;
    }
    g.step_with(None, &mut |g| {
        g.update_controls(&pads)?;
        g.do_players().map(|_| ())
    });
    // Presentation output only; the search keeps no history of it.
    g.events.clear();
    g.rumble.clear();
}

struct Progress {
    /// Records active at the start (keys and gates count when they go).
    start_active: Vec<bool>,
}

impl Progress {
    fn count(&self, g: &Game) -> u32 {
        let mut n = 0;
        for s in 0..g.items.records.len().min(self.start_active.len()) {
            let r = &g.items.records[s];
            match item_kind(g, s) {
                (5, _) => n += u32::from(self.start_active[s] && (!r.active() || r.i8_(0xde) != 0 || r.u8_(0xe2) != 0)),
                // A gate stays an active record once opened: its state +0xdc
                // leaves 0 (1 opening, 2 open; ItemCollSub stops testing it).
                (7, _) => n += u32::from(self.start_active[s] && (!r.active() || r.i8_(0xdc) != 0)),
                // Keys and the realm's legend item (kind 1 subtype 0xd,
                // PlayerGiveLegendItem: the bosses need it).
                (1, 2) | (1, 0xd) => n += u32::from(self.start_active[s] && !r.active()),
                _ => {}
            }
        }
        n
    }
}

fn key(g: &Game, p: &Progress) -> Key {
    let v = g.players[0].v3(0x50);
    ((v[0] / BOX).floor() as i32, (v[1] / BOX_Y).floor() as i32, (v[2] / BOX).floor() as i32, g.players[0].i32_(KEYS), p.count(g))
}

fn on_exit_portal(g: &Game) -> bool {
    g.players[0].u32_(0x884).checked_sub(1).map(|i| i as usize).is_some_and(|it| it < g.items.records.len() && item_kind(g, it).0 == 9)
}

fn vm_rss_kib() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines().find(|l| l.starts_with("VmRSS:"))?.split_whitespace().nth(1)?.parse().ok()
}

fn dist_xz(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// bot.rs's graph from the player (and `seeds`), on a copy of the state's collision.
fn build_nav(g: &Game, seeds: &[V3]) -> Nav {
    let mut c = g.collision.clone();
    let p = g.players[0].v3(0x50);
    let mut all = vec![p];
    all.extend_from_slice(seeds);
    Nav::build(&mut c, &all, g.globals.world_bottom - 2.0, p[1] + 80.0, 0.3)
}

/// Port 0's d-pad toward `t`: the stick angle relative to the camera yaw
/// (bot.rs steer: atan2(camera_90, camera_98) + π), quantised to the eight
/// directions (entries 0..3 for the stick's 4..7).
fn dpad_toward(g: &Game, p: V3, t: V3) -> u32 {
    let (dx, dz) = (t[0] - p[0], t[2] - p[2]);
    if dx.abs() + dz.abs() < 1e-3 {
        return 0;
    }
    let cam = g.globals.camera_90.atan2(g.globals.camera_98) + std::f32::consts::PI;
    let a = dx.atan2(dz) - cam;
    let (h, v) = (a.sin(), a.cos());
    const T: f32 = 0.383;
    let mut m = 0;
    if T < h {
        m |= 1;
    }
    if h < -T {
        m |= 1 << 1;
    }
    if v < -T {
        m |= 1 << 2;
    }
    if T < v {
        m |= 1 << 3;
    }
    m
}

/// Items worth going to (slot): keys (kind 1 subtype 2), chests and barrels
/// (2), unset triggers (5), gates (7) while a key is held, exits (9) and the
/// realm's legend item (kind 1 subtype 0xd) listed three times.
fn interest_items(g: &Game) -> Vec<usize> {
    let keys = g.players[0].i32_(KEYS);
    let mut out = Vec::new();
    for s in 0..g.items.records.len() {
        let r = &g.items.records[s];
        if !r.active() || r.u8_(0xe1) != 0 {
            continue;
        }
        let n = match item_kind(g, s) {
            (1, 2) | (2, _) => 1,
            (5, _) if r.i8_(0xde) == 0 && r.u8_(0xe2) == 0 => 1,
            (7, _) if 0 < keys && r.i8_(0xdc) == 0 => 1,
            (9, _) | (1, 0xd) => 3,
            _ => 0,
        };
        for _ in 0..n {
            out.push(s);
        }
    }
    out
}

/// One macro's output: segments of pads, each ending in a state to file
/// (None: the last segment, whose state is the game the macro ran on).
struct Segment {
    runs: Runs,
    updates: u32,
    state: Option<Box<Game>>,
}

/// A guided macro (see the module note); true when the goal was reached.
#[allow(clippy::too_many_arguments)]
fn nav_walk(g: &mut Game, nav: &Nav, blocked: &mut Blocked, rng: &mut Rng, start_health: f32, assists: &mut usize, runs: &mut Runs, n: &mut u32) -> bool {
    let none = HashSet::new();
    let p = g.players[0].v3(0x50);
    let Some(from) = nav.locate_cached(p, &none) else { return false };
    let items = interest_items(g);
    let goal_pt = if !items.is_empty() && rng.unit() < 0.5 {
        g.items.records[items[rng.below(items.len() as u32) as usize]].v3(0x40)
    } else {
        let live: Vec<usize> = (0..nav.pts.len()).filter(|&k| !nav.adj[k].is_empty() && dist_xz(nav.pts[k], p) < EXPLORE_RADIUS).collect();
        if live.is_empty() {
            return false;
        }
        nav.pts[live[rng.below(live.len() as u32) as usize]]
    };
    let Some(target) = nav.locate_cached(goal_pt, &none) else { return false };
    // The progress signature's learnt blocks (a gate opened later is not blocked there).
    let sig = (g.players[0].i32_(KEYS), items.len());
    let empty = HashSet::new();
    let bl = blocked.get(&sig).unwrap_or(&empty);
    let Some(path) = nav.path(from, &|k| k == target, bl, &crate::bot::Avoid::default()) else { return false };
    let goal = nav.pts[target];
    let mut idx = 0;
    // Progress: the path index reached, then the distance to its node.
    let mut best = (0usize, f32::INFINITY);
    let mut best_at = 0;
    let mut t = 0u32;
    while t < WALK_LIMIT && !level_ended(g) && g.players[0].i32_(0xfc) == 1 {
        let p = g.players[0].v3(0x50);
        while idx + 1 < path.len() && dist_xz(p, nav.pts[path[idx]]) < 1.2 {
            idx += 1;
        }
        let d = dist_xz(p, goal);
        if d < 1.5 && (p[1] - goal[1]).abs() < 2.5 {
            return true;
        }
        let w = dist_xz(p, nav.pts[path[idx]]);
        if best.0 < idx || w + STUCK_GAIN < best.1 {
            best = (idx, w);
            best_at = t;
        } else if STUCK_UPDATES < t - best_at {
            // Learn the edge in use as blocked (as bot.rs does).
            if 0 < idx {
                blocked.entry(sig).or_default().insert((nav.place(path[idx - 1]), nav.place(path[idx])));
            }
            return false;
        }
        let mut m = dpad_toward(g, p, nav.pts[path[idx]]);
        let foe = g.enemies.iter().any(|e| e.i32_(0xc8) == 1 && dist_xz(e.v3(0x40), p) < 5.0);
        if foe && t.is_multiple_of(2) {
            m |= CROSS;
        }
        let pad = Pad::digital(m);
        step(g, pad, start_health, assists);
        push_run(runs, pad);
        *n += 1;
        t += 1;
    }
    false
}

/// A bot macro (see the module note): the segments filed on the way.
fn bot_walk(g: &mut Game, shared: &mut (Nav, u64), rng: &mut Rng, start_health: f32, assists: &mut usize) -> Vec<Segment> {
    let items = interest_items(g);
    let x = rng.unit();
    let hints = if x < 0.4 {
        Vec::new()
    } else if !items.is_empty() && x < 0.75 {
        vec![Hint { item: Some(items[rng.below(items.len() as u32) as usize]), goto: None, radius: None, wait: None }]
    } else {
        let p = g.players[0].v3(0x50);
        let nav = &shared.0;
        let live: Vec<usize> = (0..nav.pts.len()).filter(|&k| !nav.adj[k].is_empty() && dist_xz(nav.pts[k], p) < EXPLORE_RADIUS).collect();
        let at = if live.is_empty() { p } else { nav.pts[live[rng.below(live.len() as u32) as usize]] };
        vec![Hint { item: None, goto: Some(at), radius: Some(2.0), wait: None }]
    };
    let mut bot = Bot::with_nav(g, shared.0.clone(), shared.1).with_hints(hints);
    bot.assist_enabled = false;
    let mut out = Vec::new();
    let mut runs: Runs = Vec::new();
    let mut seg_updates = 0u32;
    let mut t = 0u32;
    while t < BOT_LEN && !level_ended(g) && g.players[0].i32_(0xfc) == 1 {
        let pad = Pad::of(&bot.step(t as usize, g));
        step(g, pad, start_health, assists);
        push_run(&mut runs, pad);
        seg_updates += 1;
        t += 1;
        if seg_updates == BOT_FILE && t < BOT_LEN {
            out.push(Segment { runs: std::mem::take(&mut runs), updates: seg_updates, state: Some(Box::new(g.clone())) });
            seg_updates = 0;
        }
    }
    // A grown graph replaces the shared one.
    let (nav, built_on) = bot.nav_state();
    if shared.0.pts.len() < nav.pts.len() {
        *shared = (nav.clone(), built_on);
    }
    out.push(Segment { runs, updates: seg_updates, state: None });
    out
}

pub struct Outcome {
    /// Port 0's pad per update from the start state.
    pub script: Vec<Pad>,
    pub report: serde_json::Value,
}

/// Search from `start` for at most `budget` updates (`max_cells` archive
/// cells; past it new cells replace the most chosen one of the lowest
/// progress). `mode`: 0 random macros, 1 guided, 2 bot macros. Without
/// `assist` no health is given (a branch whose player dies is dropped).
pub fn explore(start: &Game, budget: usize, seed: u64, max_cells: usize, mode: u32, assist: bool) -> Outcome {
    let mut rng = Rng(seed.max(1).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    // The assist rule's reference health; 0 disables it.
    let start_health = if assist { start.players[0].f32_(HEALTH) } else { 0.0 };
    let prog = Progress { start_active: start.items.records.iter().map(|r| r.active()).collect() };
    let rss0 = vm_rss_kib();
    let mut assists = 0usize;
    // The stage-start sequence and camera waits hold the player: the search
    // starts after them (an idle prefix of the script).
    let mut first = start.clone();
    let mut prefix = 0u32;
    while prefix < 1800
        && !level_ended(&first)
        && (first.globals.start_of_stage != 0 || first.globals.cam_wait != 0 || first.players[0].i32_(0xfc) != 1)
    {
        step(&mut first, Pad::digital(0), start_health, &mut assists);
        prefix += 1;
    }
    let mut nodes: Vec<Node> = vec![Node { parent: None, runs: vec![(Pad::digital(0), prefix)] }];
    let mut archive: BTreeMap<Key, Cell> = BTreeMap::new();
    let k0 = key(&first, &prog);
    let mut used = prefix as usize;
    let mut iterations = 0usize;
    // The navigation graph (bot.rs's Nav), grown when a filed state stands
    // off it (guided macros) or by the bots' own rebuilds.
    let mut nav: Option<(Nav, u64)> = None;
    let mut nav_builds = 0usize;
    let mut regrow_at: Option<V3> = None;
    let (mut nav_macros, mut nav_reached, mut bot_macros) = (0usize, 0usize, 0usize);
    let mut blocked = Blocked::new();
    if 0 < mode {
        nav = Some((build_nav(&first, &[]), Bot::collision_fingerprint(&first)));
        nav_builds += 1;
    }
    archive.insert(k0, Cell { state: Box::new(first), node: Some(0), updates: prefix as usize, chosen: 0, progress: k0.4 });
    let mut best_progress = k0.4;
    let mut timeline = vec![serde_json::json!({"updates": 0, "cells": 1, "progress": best_progress})];
    let mut found: Option<(Option<usize>, usize)> = None;
    let mut evicted = 0usize;
    while used < budget && found.is_none() {
        iterations += 1;
        // Choose a cell: weight PROGRESS_BIAS^(progress − best) / √(1 + chosen).
        let keys: Vec<Key> = archive.keys().copied().collect();
        let weights: Vec<f64> = keys
            .iter()
            .map(|k| {
                let c = &archive[k];
                PROGRESS_BIAS.powi(c.progress as i32 - best_progress as i32) / (1.0 + f64::from(c.chosen)).sqrt()
            })
            .collect();
        let total: f64 = weights.iter().sum();
        let mut x = rng.unit() * total;
        let mut pick = keys[keys.len() - 1];
        for (k, w) in keys.iter().zip(&weights) {
            if x < *w {
                pick = *k;
                break;
            }
            x -= w;
        }
        let (mut g, base_node, base_updates) = {
            let c = archive.get_mut(&pick).expect("chosen cell");
            c.chosen += 1;
            ((*c.state).clone(), c.node, c.updates)
        };
        let mut segments: Vec<Segment> = Vec::new();
        let guided = nav.is_some() && rng.unit() < NAV_SHARE;
        if guided && mode == 2 {
            bot_macros += 1;
            let shared = nav.as_mut().expect("graph");
            segments = bot_walk(&mut g, shared, &mut rng, start_health, &mut assists);
        } else {
            let mut runs: Runs = Vec::new();
            let mut n = 0u32;
            if guided {
                nav_macros += 1;
                if nav_walk(&mut g, &nav.as_ref().expect("graph").0, &mut blocked, &mut rng, start_health, &mut assists, &mut runs, &mut n) {
                    nav_reached += 1;
                }
            } else {
                let dir = DIRS[rng.below(DIRS.len() as u32) as usize];
                let attack = rng.below(4) == 0;
                let len = MIN_LEN + rng.below(MAX_LEN - MIN_LEN + 1);
                while n < len && !level_ended(&g) {
                    let pad = Pad::digital(if n.is_multiple_of(2) && attack { dir | CROSS } else { dir });
                    step(&mut g, pad, start_health, &mut assists);
                    push_run(&mut runs, pad);
                    n += 1;
                }
            }
            segments.push(Segment { runs, updates: n, state: None });
        }
        if !level_ended(&g) && on_exit_portal(&g) {
            let last = segments.last_mut().expect("segment");
            let mut w = 0;
            while w < PORTAL_WAIT && !level_ended(&g) {
                step(&mut g, Pad::digital(0), start_health, &mut assists);
                push_run(&mut last.runs, Pad::digital(0));
                w += 1;
            }
            last.updates += w;
        }
        // File the segments in order (the last one's state is `g`).
        let mut parent = base_node;
        let mut updates = base_updates;
        let count = segments.len();
        let mut g = Some(g);
        for (i, seg) in segments.into_iter().enumerate() {
            nodes.push(Node { parent, runs: seg.runs });
            let node = nodes.len() - 1;
            parent = Some(node);
            updates += seg.updates as usize;
            used += seg.updates as usize;
            let state: Box<Game> = match seg.state {
                Some(s) => s,
                None if i + 1 == count => Box::new(g.take().expect("final state")),
                None => continue,
            };
            if level_ended(&state) {
                found = Some((Some(node), updates));
                break;
            }
            if state.players[0].i32_(0xfc) != 1 {
                continue;
            }
            let k = key(&state, &prog);
            if let Some((nv, _)) = nav.as_ref()
                && mode == 1
                && regrow_at.is_none()
                && nv.locate_cached(state.players[0].v3(0x50), &HashSet::new()).is_none()
            {
                regrow_at = Some(state.players[0].v3(0x50));
            }
            if best_progress < k.4 {
                best_progress = k.4;
                timeline.push(serde_json::json!({"updates": used, "cells": archive.len(), "progress": best_progress, "keys": k.3, "pos": state.players[0].v3(0x50)}));
            }
            match archive.get_mut(&k) {
                Some(c) => {
                    if updates < c.updates {
                        c.state = state;
                        c.node = Some(node);
                        c.updates = updates;
                    }
                }
                None => {
                    if max_cells <= archive.len() {
                        // Evict the most chosen cell of the lowest progress
                        // that has one besides the start.
                        let victim = archive
                            .iter()
                            .filter(|(kk, _)| **kk != k0)
                            .min_by(|(_, a), (_, b)| a.progress.cmp(&b.progress).then(b.chosen.cmp(&a.chosen)))
                            .map(|(kk, _)| *kk);
                        if let Some(v) = victim {
                            archive.remove(&v);
                            evicted += 1;
                        }
                    }
                    archive.insert(k, Cell { state, node: Some(node), updates, chosen: 0, progress: k.4 });
                }
            }
        }
        if let (Some(at), true) = (regrow_at, iterations.is_multiple_of(25)) {
            // Regrow the graph from every known node and the new place, on
            // the start state's collision.
            let seeds: Vec<V3> = nav.as_ref().map(|(nv, _)| nv.pts.iter().zip(&nv.adj).filter(|(_, a)| !a.is_empty()).map(|(q, _)| *q).chain(std::iter::once(at)).collect()).unwrap_or_default();
            let base = &archive[&k0].state;
            nav = Some((build_nav(base, &seeds), Bot::collision_fingerprint(base)));
            nav_builds += 1;
            regrow_at = None;
        }
        if iterations.is_multiple_of(500) {
            timeline.push(serde_json::json!({"updates": used, "cells": archive.len(), "progress": best_progress, "rss_kib": vm_rss_kib()}));
        }
    }
    // The script: the chosen path's macros from the start.
    let (end_node, ended_at) = match found {
        Some((n, u)) => (n, Some(u.saturating_sub(1))),
        None => {
            // No end: the path to the best cell (highest progress, then fewest updates).
            let c = archive.values().max_by(|a, b| a.progress.cmp(&b.progress).then(b.updates.cmp(&a.updates))).expect("archive");
            (c.node, None)
        }
    };
    let mut chain = Vec::new();
    let mut at = end_node;
    while let Some(i) = at {
        chain.push(i);
        at = nodes[i].parent;
    }
    chain.reverse();
    let mut script = Vec::new();
    for &i in &chain {
        for &(p, k) in &nodes[i].runs {
            script.extend(std::iter::repeat_n(p, k as usize));
        }
    }
    let max_progress = archive.values().map(|c| c.progress).max().unwrap_or(0);
    // The frontier (diagnosis of a level not ended): the best cell's place
    // and the progress items it has and has not done, and the box the cells
    // at the highest progress span.
    let frontier = archive.values().max_by(|a, b| a.progress.cmp(&b.progress).then(b.updates.cmp(&a.updates))).map(|c| {
        let g = &c.state;
        let mut done = Vec::new();
        let mut left = Vec::new();
        for s in 0..g.items.records.len().min(prog.start_active.len()) {
            let r = &g.items.records[s];
            let (kind, sub) = item_kind(g, s);
            if !matches!((kind, sub), (5, _) | (7, _) | (1, 2) | (1, 0xd) | (9, _)) || !prog.start_active[s] {
                continue;
            }
            let gone = !r.active() || (kind == 5 && (r.i8_(0xde) != 0 || r.u8_(0xe2) != 0)) || (kind == 7 && r.i8_(0xdc) != 0);
            let row = serde_json::json!([s, kind, sub, r.v3(0x40).map(|v| (v * 10.0).round() / 10.0)]);
            if gone && kind != 9 { done.push(row) } else { left.push(row) }
        }
        let at_max: Vec<V3> = archive.values().filter(|x| x.progress == max_progress).map(|x| x.state.players[0].v3(0x50)).collect();
        let lo = (0..3).map(|i| at_max.iter().map(|p| p[i]).fold(f32::INFINITY, f32::min)).collect::<Vec<_>>();
        let hi = (0..3).map(|i| at_max.iter().map(|p| p[i]).fold(f32::NEG_INFINITY, f32::max)).collect::<Vec<_>>();
        serde_json::json!({"pos": g.players[0].v3(0x50), "keys": g.players[0].i32_(KEYS), "progress": c.progress, "updates": c.updates,
            "done": done, "left": left, "cells_at_max": at_max.len(), "box_at_max": [lo, hi]})
    });
    let report = serde_json::json!({
        "scope": "DEBUG state search (explore.rs): Go-Explore over native Game clones, pad macros (random, graph-guided or bot), health assist; tooling, not a fidelity input",
        "mode": mode, "health_assist": assist, "budget": budget, "seed": seed, "updates_used": used, "iterations": iterations, "cells": archive.len(), "evicted": evicted,
        "ended": found.is_some(), "ended_at": ended_at, "start_prefix": prefix, "script_updates": script.len(), "macros": chain.len(),
        "max_progress": max_progress, "start_progress": k0.4, "assists_in_search": assists,
        "nav": {"builds": nav_builds, "macros": nav_macros, "reached": nav_reached, "bot_macros": bot_macros, "nodes": nav.as_ref().map(|n| n.0.pts.len()), "blocked_edges": blocked.values().map(|b| b.len()).sum::<usize>()},
        "rss_kib": [rss0, vm_rss_kib()], "timeline": timeline, "frontier": frontier,
    });
    Outcome { script, report }
}

/// The script as runs: `MASK_HEX:UPDATES,...` (game-run's pads= format) when
/// every pad is digital, else a JSON list of [mask, a4, a5, a6, a7, updates]
/// (padfile=; −1 analog bytes for a digital pad).
pub fn script_text(script: &[Pad]) -> (bool, String) {
    let mut runs: Vec<(Pad, usize)> = Vec::new();
    for &p in script {
        match runs.last_mut() {
            Some(l) if l.0 == p => l.1 += 1,
            _ => runs.push((p, 1)),
        }
    }
    if script.iter().all(|p| p.analog.is_none()) {
        (true, runs.iter().map(|(p, n)| format!("{:x}:{n}", p.mask)).collect::<Vec<_>>().join(","))
    } else {
        let v: Vec<serde_json::Value> = runs
            .iter()
            .map(|(p, n)| {
                let a = p.analog.map_or([-1i32; 4], |a| a.map(i32::from));
                serde_json::json!([p.mask, a[0], a[1], a[2], a[3], n])
            })
            .collect();
        (false, serde_json::Value::Array(v).to_string())
    }
}

/// A padfile= script (script_text's JSON) as pads per update.
pub fn parse_padfile(text: &str) -> Result<Vec<Pad>, Box<dyn std::error::Error>> {
    let v: Vec<[i64; 6]> = serde_json::from_str(text)?;
    let mut out = Vec::new();
    for r in v {
        let analog = (0 <= r[1]).then(|| [r[1] as u8, r[2] as u8, r[3] as u8, r[4] as u8]);
        out.extend(std::iter::repeat_n(Pad { mask: r[0] as u32, analog }, r[5] as usize));
    }
    Ok(out)
}
