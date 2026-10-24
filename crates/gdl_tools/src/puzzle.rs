//! Trigger planner for puzzle levels (M14/M15 traversal tooling; DEBUG, not
//! game logic and not a fidelity input). Levels such as G2 and G4 open
//! their exits through bridge pads whose bound objects swing or rise; the
//! bot visits triggers in distance order and gives up on a goal when its
//! graph says it is reachable but the bridge is still up.
//!
//! The planner searches the level's own simulation, breadth first over
//! sequences of actions — a reachable, touchable trigger, or a closed gate
//! while a key is held (gate doors are collision objects until opened; G4's
//! way on passes one): from a copy of the state it first collects every
//! reachable key, then places the player on the action's item (DEBUG
//! place_player, as game-run's teleport), lets the world run SETTLE updates
//! with the pad released, and regrows the bot's graph (bot.rs Nav) from the
//! player's start and that point. A trigger that brings an exit, key, gate, trigger or treasure
//! (rune stone, legend item) within reach opens a branch. The first
//! sequence after which a regular exit and every treasure lie within reach
//! is the plan; the bot then plays it as route hints (it walks to each
//! trigger itself — the probes' placements never reach the game run).
//! Limits: MAX_DEPTH triggers, MAX_PROBES probes. Not modelled: triggers
//! that act only while touched once the player leaves them, and timers.
use crate::bot::{Hint, item_nodes};
use crate::explore::{Pad, build_nav, step};
use gdl_core::game::Game;
use std::collections::{BTreeSet, VecDeque};

const SETTLE: usize = 900;
const MAX_DEPTH: usize = 5;
const MAX_PROBES: usize = 160;
const HEALTH: usize = 0x1a10;
const KEYS: usize = 0x1a14;

fn item_kind(g: &Game, slot: usize) -> (i32, i32) {
    let d = &g.items.definitions[g.items.records[slot].definition()];
    (d.kind(), d.subtype())
}

/// Items that matter for the route: exits (regular and secret), keys, the
/// level's rune stone and the realm's legend item, triggers and gates.
fn interesting(g: &Game, s: usize) -> bool {
    let r = &g.items.records[s];
    r.active() && r.u8_(0xe1) == 0 && matches!(item_kind(g, s), (9, _) | (1, 2) | (1, 10) | (1, 0xd) | (5, _) | (7, _))
}

/// A trigger the player can fire: not switch-off only (bit 0), not
/// untouchable (0x200, ItemCollSub skips it), not fired yet.
fn fresh_trigger(g: &Game, s: usize) -> bool {
    let r = &g.items.records[s];
    item_kind(g, s).0 == 5 && r.u16_(0xf4) & 0x201 == 0 && r.i8_(0xde) == 0 && r.u8_(0xe2) == 0
}

/// A gate a held key would open (state +0xdc 0: closed).
fn closed_gate(g: &Game, s: usize) -> bool {
    let r = &g.items.records[s];
    item_kind(g, s).0 == 7 && r.active() && r.i8_(0xdc) == 0
}

/// Stand on `slot` (DEBUG placement) for `n` updates with the pad released.
fn visit(g: &mut Game, slot: usize, n: usize, health: f32) -> bool {
    let q = g.items.records[slot].v3(0x40);
    let yaw = g.players[0].f32_(0x86c);
    if g.place_player(0, [q[0], q[1] + 1.5, q[2]], yaw).is_err() {
        return false;
    }
    let mut assists = 0;
    for _ in 0..n {
        step(g, Pad::digital(0), health, &mut assists);
    }
    true
}

/// The interesting items within reach of the player (and `extra` seeds).
/// A gate counts within its own radius (+0xe8, 10 in G4): its door's
/// collision keeps the graph that far off.
fn reach(g: &Game, extra: &[[f32; 3]]) -> BTreeSet<usize> {
    let nav = build_nav(g, extra);
    let slots: Vec<usize> = (0..g.items.records.len()).filter(|&s| interesting(g, s)).collect();
    let mut out: BTreeSet<usize> = item_nodes(&nav, g, &slots).into_iter().map(|(s, _)| s).collect();
    for &s in slots.iter().filter(|&&s| item_kind(g, s).0 == 7) {
        let r = &g.items.records[s];
        let (q, rad) = (r.v3(0x40), r.f32_(0xe8).max(2.5) + 1.5);
        if (0..nav.pts.len()).any(|n| !nav.adj[n].is_empty() && ((nav.pts[n][0] - q[0]).powi(2) + (nav.pts[n][2] - q[2]).powi(2)).sqrt() < rad && (nav.pts[n][1] - q[1]).abs() < 4.0) {
            out.insert(s);
        }
    }
    out
}

/// Done: a regular exit and every treasure left are within reach.
fn solved(g: &Game, r: &BTreeSet<usize>) -> bool {
    let exit = r.iter().any(|&s| matches!(item_kind(g, s), (9, t) if t != 0x32));
    let treasures = (0..g.items.records.len()).filter(|&s| interesting(g, s) && matches!(item_kind(g, s), (1, 10) | (1, 0xd)));
    exit && treasures.into_iter().all(|t| r.contains(&t))
}

struct Node {
    game: Box<Game>,
    seq: Vec<usize>,
    reach: BTreeSet<usize>,
}

/// The trigger sequence (route hints) and a report.
pub fn plan(start: &Game) -> (Vec<Hint>, serde_json::Value) {
    let health = start.players[0].f32_(HEALTH);
    let home = start.players[0].v3(0x50);
    let r0 = reach(start, &[]);
    let mut log = Vec::new();
    if solved(start, &r0) {
        return (Vec::new(), serde_json::json!({"scope": "DEBUG trigger planner (puzzle.rs)", "solved_at_start": true}));
    }
    let start_reach: Vec<serde_json::Value> = r0.iter().map(|&s| serde_json::json!([s, item_kind(start, s)])).collect();
    let mut queue = VecDeque::from([Node { game: Box::new(start.clone()), seq: Vec::new(), reach: r0 }]);
    let mut seen: BTreeSet<Vec<usize>> = BTreeSet::new();
    let mut probes = 0;
    let mut best: Option<(usize, Vec<usize>)> = None;
    while let Some(node) = queue.pop_front() {
        if MAX_DEPTH <= node.seq.len() {
            continue;
        }
        // Keys within reach first (the player picks them up on contact).
        let mut node = node;
        let keys: Vec<usize> = node.reach.iter().copied().filter(|&s| item_kind(&node.game, s) == (1, 2) && node.game.items.records[s].active()).collect();
        for k in keys {
            visit(&mut node.game, k, 30, health);
        }
        let held = node.game.players[0].i32_(KEYS);
        let candidates: Vec<usize> = node
            .reach
            .iter()
            .copied()
            .filter(|&s| (fresh_trigger(&node.game, s) || (0 < held && closed_gate(&node.game, s))) && !node.seq.contains(&s))
            .collect();
        for t in candidates {
            if MAX_PROBES <= probes {
                break;
            }
            let mut key = node.seq.clone();
            key.push(t);
            key.sort_unstable();
            if !seen.insert(key) {
                continue;
            }
            probes += 1;
            let mut g = node.game.clone();
            if !visit(&mut g, t, SETTLE, health) {
                continue;
            }
            let fired = !fresh_trigger(&g, t) && !closed_gate(&g, t);
            let pad = g.players[0].v3(0x50);
            let r = reach(&g, &[home]);
            let gained: Vec<usize> = r.difference(&node.reach).copied().collect();
            let mut seq = node.seq.clone();
            seq.push(t);
            log.push(serde_json::json!({"seq": seq, "fired": fired, "at": pad, "gained": gained.iter().map(|&s| serde_json::json!([s, item_kind(&g, s)])).collect::<Vec<_>>()}));
            if solved(&g, &r) {
                let hints = seq.iter().map(|&s| Hint { item: Some(s), goto: None, radius: None, wait: None }).collect();
                return (hints, serde_json::json!({"scope": "DEBUG trigger planner (puzzle.rs)", "plan": seq, "probes": probes, "start_reach": start_reach, "log": log}));
            }
            if !gained.is_empty() {
                if best.as_ref().is_none_or(|b| b.0 < gained.len()) {
                    best = Some((gained.len(), seq.clone()));
                }
                let mut union = node.reach.clone();
                union.extend(r);
                queue.push_back(Node { game: g, seq, reach: union });
            }
        }
    }
    (Vec::new(), serde_json::json!({"scope": "DEBUG trigger planner (puzzle.rs)", "plan": null, "probes": probes, "best_partial": best.map(|b| b.1), "start_reach": start_reach, "log": log}))
}
