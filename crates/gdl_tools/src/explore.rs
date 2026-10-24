//! State-search traversal for game-run (M14/M15 evidence tooling; not game
//! logic and not a fidelity input). A Go-Explore search over native `Game`
//! clones, driving port 0 through the ordinary pad path (ReadControls →
//! PlayerControls → do_players) with digital buttons only, so a result is a
//! plain `pads=` script that replays from a fresh load.
//!
//! The archive holds one state per cell — player 0's position in boxes of
//! BOX × BOX_Y × BOX, the keys held and a progress count (keys picked up,
//! gates opened, triggers set: kind 1 subtype 2 and kind 7 records gone,
//! kind 5 records with +0xde or +0xe2 set) — the first state that reached it
//! in the fewest updates. Each iteration restores a chosen cell and plays one
//! macro: one of the eight d-pad directions or none for MIN_LEN..=MAX_LEN
//! updates, a quarter of them with cross on alternate updates (throws;
//! barrels, chests, generators, enemies). A cell at the highest progress is
//! chosen PROGRESS_BIAS times as often as one a step below, and less the
//! more it has been chosen. On an exit portal (contact item +0x884, kind 9)
//! the pad is released for up to PORTAL_WAIT updates (DoExit counts only
//! then). The search ends with the level (do_exit's destination wave, a
//! treasure room's timer, a boss's auto exit) or at the update budget.
//! Health assist as the bot (labelled DEBUG): health is restored to its start
//! value when it falls below 40 % of it, before the update.
use gdl_core::game::Game;
use gdl_core::game::pad::PadStatus;
use std::collections::BTreeMap;

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

/// Cell: position box, keys held, progress.
type Key = (i32, i32, i32, i32, u32);

/// One macro's pad masks as runs (mask, updates).
type Runs = Vec<(u32, u32)>;

struct Node {
    parent: Option<usize>,
    runs: Runs,
}

fn push_run(runs: &mut Runs, m: u32) {
    match runs.last_mut() {
        Some(l) if l.0 == m => l.1 += 1,
        _ => runs.push((m, 1)),
    }
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

/// One update with port 0's digital `mask` (game-run's pads= path).
fn step(g: &mut Game, mask: u32, start_health: f32, assists: &mut usize) {
    if assist(g, start_health) {
        *assists += 1;
    }
    let mut pads = [PadStatus::default(); 8];
    pads[0].connected = true;
    for (k, pd) in pads.iter_mut().enumerate().take(4).skip(1) {
        pd.connected = g.globals.active_players >> k & 1 != 0;
    }
    for i in 0..0x16 {
        if mask >> i & 1 != 0 {
            pads[0].press(i);
        }
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
                (7, _) | (1, 2) => n += u32::from(self.start_active[s] && !r.active()),
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

pub struct Outcome {
    /// Port 0's mask per update from the start state.
    pub script: Vec<u32>,
    pub report: serde_json::Value,
}

/// Search from `start` for at most `budget` updates (`max_cells` archive
/// cells; past it new cells replace the most chosen one of the lowest
/// progress).
pub fn explore(start: &Game, budget: usize, seed: u64, max_cells: usize, use_nav: bool) -> Outcome {
    let mut rng = Rng(seed.max(1).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let start_health = start.players[0].f32_(HEALTH);
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
        step(&mut first, 0, start_health, &mut assists);
        prefix += 1;
    }
    let mut nodes: Vec<Node> = vec![Node { parent: None, runs: vec![(0, prefix)] }];
    let mut archive: BTreeMap<Key, Cell> = BTreeMap::new();
    let k0 = key(&first, &prog);
    archive.insert(k0, Cell { state: Box::new(first), node: Some(0), updates: prefix as usize, chosen: 0, progress: k0.4 });
    let mut used = prefix as usize;
    let mut iterations = 0usize;
    // The navigation graph (bot.rs's Nav) for guided macros, grown when a
    // filed state stands off it.
    let mut nav: Option<crate::bot::Nav> = None;
    let mut nav_builds = 0usize;
    let mut regrow_at: Option<V3> = None;
    let mut nav_macros = 0usize;
    let mut nav_reached = 0usize;
    if use_nav {
        nav = Some(build_nav(archive.values().next().map(|c| &*c.state).expect("start"), &[]));
        nav_builds += 1;
    }
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
        let mut runs: Runs = Vec::new();
        let mut n = 0u32;
        match nav.as_ref().filter(|_| rng.unit() < NAV_SHARE) {
            Some(nv) => {
                nav_macros += 1;
                if nav_walk(&mut g, nv, &mut rng, start_health, &mut assists, &mut runs, &mut n) {
                    nav_reached += 1;
                }
            }
            None => {
                let dir = DIRS[rng.below(DIRS.len() as u32) as usize];
                let attack = rng.below(4) == 0;
                let len = MIN_LEN + rng.below(MAX_LEN - MIN_LEN + 1);
                while n < len && !level_ended(&g) {
                    let m = if n.is_multiple_of(2) && attack { dir | CROSS } else { dir };
                    step(&mut g, m, start_health, &mut assists);
                    push_run(&mut runs, m);
                    n += 1;
                }
            }
        }
        if !level_ended(&g) && on_exit_portal(&g) {
            let mut w = 0;
            while w < PORTAL_WAIT && !level_ended(&g) {
                step(&mut g, 0, start_health, &mut assists);
                push_run(&mut runs, 0);
                w += 1;
            }
            n += w;
        }
        nodes.push(Node { parent: base_node, runs });
        let node = nodes.len() - 1;
        let updates = base_updates + n as usize;
        used += n as usize;
        if level_ended(&g) {
            found = Some((Some(node), updates));
            break;
        }
        if g.players[0].i32_(0xfc) != 1 {
            continue;
        }
        let k = key(&g, &prog);
        if let Some(nv) = nav.as_ref()
            && regrow_at.is_none()
            && nv.locate_cached(g.players[0].v3(0x50), &std::collections::HashSet::new()).is_none()
        {
            regrow_at = Some(g.players[0].v3(0x50));
        }
        if let (Some(at), true) = (regrow_at, iterations.is_multiple_of(25)) {
            // Regrow the graph from every known node and the new place, on
            // this state's collision.
            let seeds: Vec<V3> = nav.as_ref().map(|nv| nv.pts.iter().zip(&nv.adj).filter(|(_, a)| !a.is_empty()).map(|(q, _)| *q).collect()).unwrap_or_default();
            let mut s2 = seeds;
            s2.push(at);
            nav = Some(build_nav(&g, &s2));
            nav_builds += 1;
            regrow_at = None;
        }
        if best_progress < k.4 {
            best_progress = k.4;
            timeline.push(serde_json::json!({"updates": used, "cells": archive.len(), "progress": best_progress, "keys": k.3, "pos": g.players[0].v3(0x50)}));
        }
        match archive.get_mut(&k) {
            Some(c) => {
                if updates < c.updates {
                    *c.state = g;
                    c.node = Some(node);
                    c.updates = updates;
                }
            }
            None => {
                if max_cells <= archive.len() {
                    // Evict the most chosen cell of the lowest progress (not the start).
                    let low = archive.values().map(|c| c.progress).min().unwrap_or(0);
                    if let Some(v) = archive.iter().filter(|(kk, c)| c.progress == low && **kk != k0).max_by_key(|(_, c)| c.chosen).map(|(kk, _)| *kk) {
                        archive.remove(&v);
                        evicted += 1;
                    }
                }
                archive.insert(k, Cell { state: Box::new(g), node: Some(node), updates, chosen: 0, progress: k.4 });
            }
        }
        if iterations.is_multiple_of(2000) {
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
        for &(m, k) in &nodes[i].runs {
            script.extend(std::iter::repeat_n(m, k as usize));
        }
    }
    let max_progress = archive.values().map(|c| c.progress).max().unwrap_or(0);
    let report = serde_json::json!({
        "scope": "DEBUG state search (explore.rs): Go-Explore over native Game clones, digital pad macros, health assist; tooling, not a fidelity input",
        "budget": budget, "seed": seed, "updates_used": used, "iterations": iterations, "cells": archive.len(), "evicted": evicted,
        "ended": found.is_some(), "ended_at": ended_at, "start_prefix": prefix, "script_updates": script.len(), "macros": chain.len(),
        "max_progress": max_progress, "start_progress": k0.4, "assists_in_search": assists,
        "nav": {"builds": nav_builds, "macros": nav_macros, "reached": nav_reached, "nodes": nav.as_ref().map(|n| n.pts.len())},
        "rss_kib": [rss0, vm_rss_kib()], "timeline": timeline,
    });
    Outcome { script, report }
}

type V3 = [f32; 3];

/// Share of guided macros when the graph is in use.
const NAV_SHARE: f64 = 0.8;
/// A guided walk's update limit, and its stuck test (less than STUCK_GAIN
/// closer to the goal over STUCK_UPDATES).
const WALK_LIMIT: u32 = 360;
const STUCK_UPDATES: u32 = 45;
const STUCK_GAIN: f32 = 0.5;

/// bot.rs's graph from the player (and `seeds`), on a copy of the state's collision.
fn build_nav(g: &Game, seeds: &[V3]) -> crate::bot::Nav {
    let mut c = g.collision.clone();
    let p = g.players[0].v3(0x50);
    let mut all = vec![p];
    all.extend_from_slice(seeds);
    crate::bot::Nav::build(&mut c, &all, g.globals.world_bottom - 2.0, p[1] + 80.0, 0.3)
}

fn dist_xz(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
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

/// Items worth walking to: keys (kind 1 subtype 2), chests and barrels (2),
/// unset triggers (5), gates (7) while a key is held, exits (9, listed
/// three times).
fn interest_points(g: &Game) -> Vec<V3> {
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
            (7, _) if 0 < keys => 1,
            (9, _) => 3,
            _ => 0,
        };
        for _ in 0..n {
            out.push(r.v3(0x40));
        }
    }
    out
}

/// A guided macro: a graph path from the player to an item of interest or
/// a random reachable node, followed with the d-pad (throws on alternate
/// updates near enemies) until reached, stuck or WALK_LIMIT. True when the
/// goal was reached.
fn nav_walk(g: &mut Game, nav: &crate::bot::Nav, rng: &mut Rng, start_health: f32, assists: &mut usize, runs: &mut Runs, n: &mut u32) -> bool {
    let none = std::collections::HashSet::new();
    let p = g.players[0].v3(0x50);
    let Some(from) = nav.locate_cached(p, &none) else { return false };
    let pts = interest_points(g);
    let goal_pt = if !pts.is_empty() && rng.unit() < 0.5 {
        pts[rng.below(pts.len() as u32) as usize]
    } else {
        let live: Vec<usize> = (0..nav.pts.len()).filter(|&k| !nav.adj[k].is_empty()).collect();
        if live.is_empty() {
            return false;
        }
        nav.pts[live[rng.below(live.len() as u32) as usize]]
    };
    let Some(target) = nav.locate_cached(goal_pt, &none) else { return false };
    let Some(path) = nav.path(from, &|k| k == target, &std::collections::HashSet::new(), &crate::bot::Avoid::default()) else { return false };
    let goal = nav.pts[target];
    let mut idx = 0;
    let mut best = dist_xz(p, goal);
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
        if d + STUCK_GAIN < best {
            best = d;
            best_at = t;
        } else if STUCK_UPDATES < t - best_at {
            return false;
        }
        let mut m = dpad_toward(g, p, nav.pts[path[idx]]);
        let foe = g.enemies.iter().any(|e| e.i32_(0xc8) == 1 && dist_xz(e.v3(0x40), p) < 5.0);
        if foe && t.is_multiple_of(2) {
            m |= CROSS;
        }
        step(g, m, start_health, assists);
        push_run(runs, m);
        *n += 1;
        t += 1;
    }
    false
}

/// `MASK_HEX:UPDATES,...` runs of a script (game-run's pads= format).
pub fn script_string(script: &[u32]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < script.len() {
        let m = script[i];
        let mut j = i;
        while j < script.len() && script[j] == m {
            j += 1;
        }
        out.push(format!("{m:x}:{}", j - i));
        i = j;
    }
    out.join(",")
}
