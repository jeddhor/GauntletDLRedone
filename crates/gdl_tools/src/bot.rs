//! Traversal bot for game-run (M14/M15/M16 evidence tooling; not game logic
//! and not a fidelity input). It drives port 0 through the ordinary pad path
//! (ReadControls → PlayerControls → do_players) with the left stick, so every
//! step the player takes is the native PlayerMotion's.
//!
//! Navigation: a graph sampled from a copy of the level's collision as the
//! game holds it (runtime flags and dynamic matrices included) — columns of
//! CELL × CELL, floors found by FloorCollide sweeps from the top of the world
//! down, neighbours joined when the step up is at most STEP_UP (steeper only
//! on a ramp: a floor under the midpoint near the mean height), the drop at
//! most DROP, a floor lies under the midpoint and an EnemyWallCollide-style
//! sweep (wall normals, object mask 0x13a) at a metre above the higher floor
//! hits nothing. The graph is grown breadth-first from the player, so it is
//! the statically reachable area; items (gates, barrels) are not in it.
//!
//! Goals, re-chosen whenever a goal is reached or the bot is stuck: the
//! nearest reachable key (kind 1 subtype 2), then the nearest exit (kind 9),
//! then unvisited triggers (kind 5); a gate (kind 7) opens when touched with
//! a key, so its nodes are closed while no key is held. Optional route hints
//! (tools/bot-routes/<W><L>.json: items, points, waits) come first. On an
//! exit portal the stick is released (DoExit counts only then). Stuck (less
//! than half a unit in STUCK updates, or within DWELL units for 4 × STUCK
//! without a new goal): the edge in use is blocked (not at a
//! gate; on a drop, every drop edge nearby heading the same way) and the
//! bot throws for a while (barrels, chests, generators, enemies). With no
//! goal for 900 updates it forgets given-up goals, gate blocks and explored
//! columns and regrows the graph. Enemies within
//! ATTACK_RANGE are attacked with throws (cross) in alternate 12-update
//! windows; without the health assist every update, and those within twice
//! that in windows. Below 60 % health, food first.
//! Health assist (labelled DEBUG in the report): health is restored to its
//! start value when it falls below 40 % of it.
use gdl_core::collision::{CollisionWorld, Query};
use gdl_core::player_collision::{DisplacementInput, PlayerFloorState, player_displacement};
use gdl_core::game::Game;
use gdl_core::game::pad::PadStatus;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

type V3 = [f32; 3];
pub type Place = (i32, i32, i32);

const CELL: f32 = 1.5;
const STEP_UP: f32 = 2.0;
/// Steps up to this height pass when the player-like wall test does.
const STEP_MAX: f32 = 3.0;
/// Player 0's collision radius (+0x828), height (+0x82c) and the world's
/// bottom, set when a bot is made: steps and drops of the graph are walked
/// with the game's own displacement (walk_reaches).
static PLAYER_DIMS: std::sync::OnceLock<(f32, f32, f32)> = std::sync::OnceLock::new();
/// Without the health assist, steering keeps off dormant statues and
/// live hazards (steer); set at each Bot::step.
static AVOID_CONTACT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
thread_local! {
    /// Foes no fight target until an update (note_fire): fired at for 600
    /// updates without their health dropping; and the current update.
    static FOE_SKIP: std::cell::RefCell<(HashMap<(u8, usize), usize>, usize)> = std::cell::RefCell::new((HashMap::new(), 0));
    /// Items keep_off no longer steers around: a dormant statue the bot got
    /// stuck by three times (its passage needs contact).
    static CONTACT_OK: std::cell::RefCell<HashSet<usize>> = std::cell::RefCell::new(HashSet::new());
}

/// Whether a player walking from floor point `a` towards `b` gets there:
/// PlayerMotion's static displacement (player_collision::player_displacement:
/// PlayerCollideWalls, PlayerCollideFloor with its 3-unit step limit, the
/// floor-height adjustment; 96/96 updates byte-equal with the reference in
/// G1, movement.md) in steps of 0.25, from a standing start on the floor
/// object under `a`. Used for the graph's steps and drops (over 1 unit),
/// where the column sweeps and the one-unit-up wall test pass edges the
/// player cannot take (D3's bridge pieces 71.9 → 73.4: 162 stuck events).
pub fn set_player_dims(g: &Game) {
    let _ = PLAYER_DIMS.set((g.players[0].f32_(0x828), g.players[0].f32_(0x82c), g.globals.world_bottom));
}

fn walk_reaches(c: &mut CollisionWorld, a: V3, b: V3) -> bool {
    let Some(&(radius, height, bottom)) = PLAYER_DIMS.get() else { return true };
    let floor = c.floor_collide(0.3, 0.0, -3.0, [a[0], a[1] + 1.0, a[2]], true, 0).ok().and_then(|(f, _)| f.solid).map(|h| h.object);
    let mut state = PlayerFloorState { floor_y_88c: a[1], floor_object_89c: floor, flags_8ac: 0, word_100: 0, y_54: a[1], wall_object_8a0: None, floor_contact: floor, floor_hit: None, water: None };
    let mut pos = a;
    let mut stalls = 0;
    for _ in 0..48 {
        let (dx, dz) = (b[0] - pos[0], b[2] - pos[2]);
        let dist = dx.hypot(dz);
        if dist < 0.35 {
            return (pos[1] - b[1]).abs() < 1.5;
        }
        let k = 0.25f32.min(dist) / dist;
        let input = DisplacementInput {
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            collision_offset_81c: [0.0; 3],
            offset_flags_84: 0,
            position: pos,
            request: [dx * k, 0.0, dz * k],
            speed: 0.25,
            radius_828: radius,
            height_82c: height,
            flags_8b0: 0,
            current_action: 0x13,
            spf: 1.0 / 30.0,
            world_bottom: bottom,
        };
        let Ok(out) = player_displacement(c, input, &mut state) else { return true };
        if dist_xz(out.position, pos) < 0.02 {
            stalls += 1;
            if 3 <= stalls {
                return false;
            }
        } else {
            stalls = 0;
        }
        pos = out.position;
        state.y_54 = pos[1];
    }
    false
}
const HEADROOM: f32 = 3.0;
const STRICT_RADIUS: f32 = 1.2;
const HARD_COST: f32 = 20.0;
/// Shot switches are fired at from within this distance, or along a clear
/// level line from within SHOT_FAR.
const SHOOT_RANGE: f32 = 9.0;
const SHOT_FAR: f32 = 30.0;
/// No-assist runs fire at generators up to this far along a clear line.
const GEN_RANGE: f32 = 15.0;
/// The largest drop walked off a ledge: PlayerCollideFloor 0x145218 refuses
/// a floor change of over 3 units (4 on the same object) and the player
/// stops at the edge (D4's pit rim, H3's ledge 8.7 above trigger 504, D1's
/// walkway); steeper descents only on a ramp, as climbs. (12 before: the
/// graph held drops no player can take.)
const DROP: f32 = 4.0;
/// Ride edges (Nav::ride): an object whose translation y has spanned at
/// least MOVER_MIN is a mover; the bot steps across once the far floor is
/// within RIDE_DROP below (PlayerCollideFloor's 3-unit limit) or RIDE_CLIMB
/// above (the graph's ordinary step), waiting at most RIDE_WAIT
/// updates. RIDE_COST weighs the wait in routes (tuning, not game data).
const MOVER_MIN: f32 = 1.0;
/// Place heights of mover nodes: this plus the object index.
const MOVER_PLACE: i32 = 1_000_000;
const RIDE_DROP: f32 = 3.0;
const RIDE_CLIMB: f32 = STEP_UP;
const RIDE_COST: f32 = 10.0;
const RIDE_WAIT: usize = 600;
const STUCK: usize = 45;
/// Updates spent striking at one trigger out of touch (strike_reach).
const STRIKE_FOR: usize = 900;
/// The strike's lure phase after STRIKE_FOR (see the strike routine).
const LURE_FOR: usize = 2400;
/// No-assist runs: updates a stuck bot may spend fighting foes in place.
const FIGHT_HOLD: usize = 600;
const DWELL: f32 = 2.0;
/// Floor probes for columns and edge midpoints.
const FLOOR_RADIUS: f32 = 0.3;
/// Column sweeps go down in pieces of this length: a long radius sweep
/// misses some dynamic floor objects that short ones find (E1's doorway
/// floor, object 825, under a sweep from 28 units up; the player's own
/// floor test is a short sweep).
const SWEEP: f32 = 8.0;
const ATTACK_RANGE: f32 = 5.0;
/// loot=1: pickups within this many units (and 5 of height) are goals.
const LOOT_RANGE: f32 = 25.0;
const NODE_LIMIT: usize = 400_000;
const HEALTH: usize = 0x1a10;
const KEYS: usize = 0x1a14;

#[derive(Clone)]
pub struct Nav {
    pub pts: Vec<V3>,
    /// The collision object each node's floor belongs to.
    pub obj: Vec<usize>,
    pub adj: Vec<Vec<usize>>,
    /// Edges the player-like wall test rejects (kept at a higher cost).
    hard: HashSet<(usize, usize)>,
    /// Edges between floors on different objects of which at least one
    /// moves vertically (a lift): walkable only while the mover holds the
    /// far floor within a step, so the bot waits at the near end (ride
    /// edges). I2's lift 47 (bridge pad 476) carries the player between
    /// the walkway at 13.1 and the ground at 3.5 by trigger 485, a 9.6-unit
    /// drop no walk takes (DROP).
    pub ride: HashSet<(usize, usize)>,
    /// Vertically moving objects at the build: object → (lowest, highest,
    /// current) translation y the bot has seen.
    pub movers: HashMap<usize, (f32, f32, f32)>,
    cols: HashMap<(i32, i32), Vec<usize>>,
    top: f32,
    bottom: f32,
    radius: f32,
    pub reached: usize,
}

fn key(x: f32, z: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (z / CELL).floor() as i32)
}

fn centre(k: (i32, i32)) -> (f32, f32) {
    ((k.0 as f32 + 0.5) * CELL, (k.1 as f32 + 0.5) * CELL)
}

fn dist3(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn dist_xz(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl Nav {
    /// The graph reachable from `start` in `c` (a copy of the game's collision).
    /// The graph reachable from `seeds` (the player, plus the places the
    /// previous graph reached, so an isolated spot on a lift does not lose
    /// the known level).
    pub fn build(c: &mut CollisionWorld, seeds: &[V3], bottom: f32, top: f32, radius: f32) -> Nav {
        Self::build_with_movers(c, seeds, bottom, top, radius, HashMap::new())
    }

    /// `build` with the vertically moving objects seen so far (object →
    /// lowest, highest, current translation y): ride edges join their
    /// floors to floors a step away at some point of their travel.
    pub fn build_with_movers(c: &mut CollisionWorld, seeds: &[V3], bottom: f32, top: f32, radius: f32, movers: HashMap<usize, (f32, f32, f32)>) -> Nav {
        let mut nav = Nav { pts: Vec::new(), obj: Vec::new(), adj: Vec::new(), hard: HashSet::new(), ride: HashSet::new(), movers, cols: HashMap::new(), top, bottom, radius, reached: 0 };
        let mut queue = VecDeque::new();
        let mut seen = Vec::new();
        for &p in seeds {
            if let Some(s) = nav.locate(c, p) {
                seen.resize(nav.pts.len(), false);
                if !seen[s] {
                    seen[s] = true;
                    queue.push_back(s);
                }
            }
        }
        while let Some(n) = queue.pop_front() {
            nav.reached += 1;
            if NODE_LIMIT < nav.reached {
                break;
            }
            let p = nav.pts[n];
            let k = key(p[0], p[2]);
            for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let k2 = (k.0 + dx, k.1 + dz);
                let col = nav.column(c, k2);
                for &m in &col {
                    if seen.len() <= m {
                        seen.resize(nav.pts.len(), false);
                    }
                    // A drop into a column whose other floor lies between the
                    // two heights (within a step of the source) lands on that
                    // floor instead: K1's ramp over the y −10 floor at (−53,
                    // 39), where the bot stood on the ramp aiming below it.
                    let q = nav.pts[m];
                    if q[1] < p[1] - STEP_UP && col.iter().any(|&o| o != m && q[1] + 1.0 < nav.pts[o][1] && nav.pts[o][1] < p[1] + STEP_UP) {
                        continue;
                    }
                    let class = Self::edge_class(c, p, nav.pts[m], nav.radius);
                    let ride = nav.ride_edge(c, n, m, class.is_some());
                    if let Some(strict) = class.or(ride.then_some(false)) {
                        nav.adj[n].push(m);
                        if ride {
                            nav.ride.insert((n, m));
                        } else if !strict {
                            nav.hard.insert((n, m));
                        }
                        if !seen[m] {
                            seen[m] = true;
                            queue.push_back(m);
                        }
                    }
                }
            }
        }
        nav
    }

    /// The node for `p`: its column's (and the neighbours') floor nearest in
    /// height within 2.5 below/1.5 above.
    pub fn locate(&mut self, c: &mut CollisionWorld, p: V3) -> Option<usize> {
        let k = key(p[0], p[2]);
        let mut best: Option<(f32, usize)> = None;
        for dx in -1..=1 {
            for dz in -1..=1 {
                for n in self.column(c, (k.0 + dx, k.1 + dz)) {
                    let q = self.pts[n];
                    let dy = p[1] - q[1];
                    if !(-1.5..=2.5).contains(&dy) {
                        continue;
                    }
                    let d = dist_xz(p, q) + dy.abs();
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, n));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// The edge fails the player-like wall test (kept at a higher cost).
    pub fn is_hard(&self, a: usize, b: usize) -> bool {
        self.hard.contains(&(a, b))
    }

    /// `locate` over the columns already sampled (reachable graph nodes only).
    pub fn locate_cached(&self, p: V3, bad: &HashSet<Place>) -> Option<usize> {
        let k = key(p[0], p[2]);
        let mut best: Option<(f32, usize)> = None;
        for dx in -2..=2 {
            for dz in -2..=2 {
                for &n in self.cols.get(&(k.0 + dx, k.1 + dz)).map(Vec::as_slice).unwrap_or(&[]) {
                    let q = self.pts[n];
                    let dy = p[1] - q[1];
                    if self.adj[n].is_empty() || !(-3.0..=3.0).contains(&dy) || (!bad.is_empty() && bad.contains(&self.place(n))) {
                        continue;
                    }
                    let d = dist_xz(p, q) + dy.abs();
                    if best.is_none_or(|b| d < b.0) {
                        best = Some((d, n));
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// Node ids of a column's floors (sampled once).
    fn column(&mut self, c: &mut CollisionWorld, k: (i32, i32)) -> Vec<usize> {
        if let Some(v) = self.cols.get(&k) {
            return v.clone();
        }
        let (x, z) = centre(k);
        let mut out = Vec::new();
        let mut y = self.top;
        let mut sweeps = 0;
        while out.len() < 16 && sweeps < 400 {
            sweeps += 1;
            let depth = y - self.bottom + 2.0;
            if depth <= 0.0 {
                break;
            }
            let seg = depth.min(SWEEP);
            let Ok((hit, _)) = c.floor_collide(FLOOR_RADIUS, 0.0, -seg, [x, y, z], true, 0) else { break };
            let Some(h) = hit.solid else {
                y -= seg - 0.1;
                continue;
            };
            let fy = h.position[1];
            // A floor under another one with less than HEADROOM between
            // their tops is not stood on: the player (collision height 2.5)
            // does not fit (G2's ramp at (−112, 3.3, −157) over the ground at
            // 0.9: the graph walked under it, the player walked up it).
            if out.last().is_none_or(|&l: &usize| HEADROOM <= self.pts[l][1] - fy) {
                self.pts.push([x, fy, z]);
                self.obj.push(h.object);
                self.adj.push(Vec::new());
                out.push(self.pts.len() - 1);
            }
            y = fy - 0.3;
        }
        self.cols.insert(k, out.clone());
        out
    }

    /// None: no edge; Some(strict): an edge, and whether the player-like
    /// wall test also passes. The permissive test sweeps 1.5 above the
    /// higher floor (lifts' lips, ramps the strict test rejects but the
    /// player walks); edges the bot cannot walk are blocked when stuck.
    fn edge_class(c: &mut CollisionWorld, a: V3, b: V3, radius: f32) -> Option<bool> {
        let dy = b[1] - a[1];
        // Climbs steeper than STEP_UP and descents deeper than DROP only on
        // a ramp: floors (normal y ≥ 0.5, at most 60°) and a floor under the
        // midpoint near the mean height.
        let span = dy.abs();
        // A step of up to STEP_MAX (PlayerCollideFloor's limit is 3, 4 on one
        // object) when the player-like wall test at one unit up passes: a
        // step face lower than that, or a step object (+0x10 & 0x38), which
        // PlayerMotion hands to the floor stage (the floor stage limits a
        // step to 3 units, 4 on one object). The walk check below decides.
        let step = STEP_UP < dy && dy <= STEP_MAX;
        let ramp = (STEP_UP < dy || dy < -DROP) && span <= dist_xz(a, b) * 1.7;
        if (STEP_UP < dy || dy < -DROP) && !ramp && !step {
            return None;
        }
        if ramp && !(step && Self::edge_ok(c, a, b, radius.max(STRICT_RADIUS))) {
            let mid = [(a[0] + b[0]) * 0.5, a[1].max(b[1]) + 1.0, (a[2] + b[2]) * 0.5];
            let mean = (a[1] + b[1]) * 0.5;
            if !matches!(c.floor_collide(0.3, 0.0, -(span + 2.0), mid, true, 0), Ok((f, _)) if f.solid.is_some_and(|h| (h.position[1] - mean).abs() < 0.4 * span)) {
                return None;
            }
        }
        let top = a[1].max(b[1]) + 1.5;
        let loose = c.world_collide(Query {
            radius,
            first: [a[0], top, a[2]],
            last: [b[0], top, b[2]],
            check_flags: 7,
            min_normal_y: f32::from_bits(0xbf5d_b22d),
            max_normal_y: f32::from_bits(0x3f5d_b22d),
            object_mask: 0x13a,
            exclusion_mask: 1,
        });
        // A hit on a step object (+0x10 & 0x38, handed to the floor stage)
        // passes when the player-like test does: C2's stairs to its exit
        // (risers of 1.25, object 1340 flags 0xa) — a column centre 0.16
        // short of a riser samples the lower step, and the level edge onto
        // it swept 0.297 from the riser's top corner, so the stairs had
        // edges down only (the regular exit out of the graph; every c90 win
        // took the secret exit; holding Up there climbs them, c2-step-m4).
        // Not on a drop: rim object 1196's drops of 1.9 onto C2's west pit
        // floor (object 1181 at 5.7) passed so and were not taken in play
        // (lg32/lg33's tries, c92–c94: stuck events at y 7–9 aiming at 5.7
        // for 10,000 updates; the 35,559 pit edges behind them).
        let stepped = match &loose {
            Ok((w, _)) => match w.solid {
                None => Some(false),
                Some(h) if c.runtime_flags[h.object] & 0x38 != 0 && a[1] - 0.5 <= b[1] => Some(true),
                Some(_) => None,
            },
            Err(_) => None,
        };
        match stepped {
            None => return None,
            Some(true) if !Self::edge_ok(c, a, b, radius.max(STRICT_RADIUS)) => return None,
            Some(_) => {}
        }
        let mid = [(a[0] + b[0]) * 0.5, a[1].max(b[1]) + 1.0, (a[2] + b[2]) * 0.5];
        let depth = mid[1] - a[1].min(b[1]) + 1.0;
        if !matches!(c.floor_collide(FLOOR_RADIUS, 0.0, -depth, mid, true, 0), Ok((f, _)) if f.solid.is_some()) {
            return None;
        }
        // The player-like test with nearly the player's radius (1.5): the
        // permissive 0.3 passes gaps the player cannot (G2's ground under
        // and beside its railed ramp).
        let ok = Self::edge_ok(c, a, b, radius.max(STRICT_RADIUS));
        if step && !ramp && !ok {
            return None;
        }
        // The walk check (walk_reaches) is not applied: with it A3 and K2,
        // which end otherwise, did not (b56: 1289 and 2652 stuck events) —
        // the static displacement from a standing start refuses steps the
        // moving player takes. Kept for diagnostics.
        let _ = walk_reaches;
        // A drop is walked off at the upper floor, where PlayerMotion tests
        // walls: a railing there stops the player whatever lies above it
        // (G2's walkway at y 9.7 over the spikes: the permissive sweep at
        // 11.2 passed it, and the bot tried its drops for 3,000 updates).
        if dy < -STEP_UP && !ok {
            return None;
        }
        Some(ok)
    }

    /// Whether the edge from node `n` to node `m` is a ride edge: the two
    /// floors lie on different objects, one of them a mover, and at some
    /// point of the movers' travel the far floor is within RIDE_DROP below
    /// or RIDE_CLIMB above the near one. An edge already walkable now
    /// (`walkable`) counts too: the mover will not wait for the bot.
    fn ride_edge(&self, c: &mut CollisionWorld, n: usize, m: usize, walkable: bool) -> bool {
        let (oa, ob) = (self.obj[n], self.obj[m]);
        if oa == ob {
            return false;
        }
        let travel = |o: usize| self.movers.get(&o).map_or((0.0, 0.0), |&(lo, hi, cur)| (cur - lo, hi - cur));
        let ((ad, au), (bd, bu)) = (travel(oa), travel(ob));
        if ad + au + bd + bu < MOVER_MIN {
            return false;
        }
        if walkable {
            return true;
        }
        let (a, b) = (self.pts[n], self.pts[m]);
        let (lo, hi) = ((b[1] - bd) - (a[1] + au), (b[1] + bu) - (a[1] - ad));
        if hi < -RIDE_DROP || RIDE_CLIMB < lo {
            return false;
        }
        // Nothing in the way above both floors at their highest.
        let top = (a[1] + au).max(b[1] + bu) + 1.5;
        let wall = c.world_collide(Query {
            radius: self.radius,
            first: [a[0], top, a[2]],
            last: [b[0], top, b[2]],
            check_flags: 7,
            min_normal_y: f32::from_bits(0xbf5d_b22d),
            max_normal_y: f32::from_bits(0x3f5d_b22d),
            object_mask: 0x13a,
            exclusion_mask: 1,
        });
        matches!(wall, Ok((ref w, _)) if w.solid.is_none())
    }

    /// Node `n` where its floor is now: moved with its object when that is
    /// a mover of the build.
    pub fn live(&self, g: &Game, n: usize) -> V3 {
        let mut q = self.pts[n];
        if let Some(&(_, _, built)) = self.movers.get(&self.obj[n])
            && let Ok(m) = g.collision.object_matrix(self.obj[n])
        {
            q[1] += m[3][1] - built;
        }
        q
    }

    fn edge_ok(c: &mut CollisionWorld, a: V3, b: V3, radius: f32) -> bool {
        // PlayerMotion tests walls one unit above the current floor; hits on
        // step objects (+0x10 & 0x38) go to the floor stage instead.
        let h = a[1] + 1.0;
        let wall = c.world_collide(Query {
            radius,
            first: [a[0], h, a[2]],
            last: [b[0], h, b[2]],
            check_flags: 7,
            min_normal_y: f32::from_bits(0xbf5d_b22d),
            max_normal_y: f32::from_bits(0x3f5d_b22d),
            object_mask: 0x13a,
            exclusion_mask: 1,
        });
        match wall {
            Ok((w, _)) => {
                if w.solid.is_some_and(|hit| c.runtime_flags[hit.object] & 0x38 == 0) {
                    return false;
                }
            }
            Err(_) => return false,
        }
        let mid = [(a[0] + b[0]) * 0.5, a[1].max(b[1]) + 1.0, (a[2] + b[2]) * 0.5];
        let h = mid[1];
        let depth = h - a[1].min(b[1]) + 1.0;
        matches!(c.floor_collide(FLOOR_RADIUS, 0.0, -depth, mid, true, 0), Ok((f, _)) if f.solid.is_some())
    }

    /// Every column's floors over the world's x/z bounds (diagnostic maps).
    pub fn sample_all(&mut self, c: &mut CollisionWorld, min: V3, max: V3) -> Vec<V3> {
        let (k0, k1) = (key(min[0], min[2]), key(max[0], max[2]));
        let mut out = Vec::new();
        for kx in k0.0..=k1.0 {
            for kz in k0.1..=k1.1 {
                for n in self.column(c, (kx, kz)) {
                    out.push(self.pts[n]);
                }
            }
        }
        out
    }

    /// A node's place (column, height to the nearest unit): blocked edges
    /// are kept by place so they survive rebuilds (node ids do not).
    pub fn place(&self, n: usize) -> Place {
        let p = self.pts[n];
        let k = key(p[0], p[2]);
        // A mover's floor by its object, whatever height the build caught it
        // at: blocks learnt there outlast rebuilds (no-assist G2, lg9: the
        // step off bridge 1054 by pad 407 failed 60 times, each rebuild
        // placing the bridge's nodes at another height).
        if self.movers.contains_key(&self.obj[n]) {
            return (k.0, MOVER_PLACE + self.obj[n] as i32, k.1);
        }
        (k.0, p[1].round() as i32, k.1)
    }

    /// Why each edge from the node at `p` to its neighbour columns passes or fails (diagnostics).
    pub fn explain(c: &mut CollisionWorld, p: V3, radius: f32, top: f32, bottom: f32) -> Vec<serde_json::Value> {
        let mut nav = Nav { pts: Vec::new(), obj: Vec::new(), adj: Vec::new(), hard: HashSet::new(), ride: HashSet::new(), movers: HashMap::new(), cols: HashMap::new(), top, bottom, radius, reached: 0 };
        let Some(n) = nav.locate(c, p) else { return Vec::new() };
        let a = nav.pts[n];
        let k = key(a[0], a[2]);
        let mut out = Vec::new();
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            for m in nav.column(c, (k.0 + dx, k.1 + dz)) {
                let b = nav.pts[m];
                let dy = b[1] - a[1];
                let h = a[1] + 1.0;
                let wall = c.world_collide(Query {
                    radius, first: [a[0], h, a[2]], last: [b[0], h, b[2]], check_flags: 7,
                    min_normal_y: f32::from_bits(0xbf5d_b22d), max_normal_y: f32::from_bits(0x3f5d_b22d), object_mask: 0x13a, exclusion_mask: 1,
                });
                let hit = match &wall { Ok((w, _)) => w.solid.map(|h| (h.object, format!("{:#x}", c.runtime_flags[h.object]), h.position)), Err(_) => None };
                let mid = [(a[0] + b[0]) * 0.5, h, (a[2] + b[2]) * 0.5];
                let depth = h - a[1].min(b[1]) + 1.0;
                let floor = matches!(c.floor_collide(FLOOR_RADIUS, 0.0, -depth, mid, true, 0), Ok((f, _)) if f.solid.is_some());
                // edge_class's permissive sweep (1.5 above the higher floor).
                let top = a[1].max(b[1]) + 1.5;
                let loose = c.world_collide(Query {
                    radius, first: [a[0], top, a[2]], last: [b[0], top, b[2]], check_flags: 7,
                    min_normal_y: f32::from_bits(0xbf5d_b22d), max_normal_y: f32::from_bits(0x3f5d_b22d), object_mask: 0x13a, exclusion_mask: 1,
                });
                let loose = match &loose { Ok((w, _)) => w.solid.map(|h| (h.object, format!("{:#x}", c.runtime_flags[h.object]), h.position)), Err(_) => None };
                out.push(serde_json::json!({"from": a, "to": b, "dy": dy, "wall": hit, "loose": loose, "mid_floor": floor, "class": Self::edge_class(c, a, b, radius)}));
            }
        }
        out
    }

    /// Dijkstra from `from` to the first node `goal` accepts, avoiding `blocked` edges.
    pub fn path(&self, from: usize, goal: &dyn Fn(usize) -> bool, blocked: &HashSet<(Place, Place)>, avoid: &Avoid) -> Option<Vec<usize>> {
        #[derive(PartialEq)]
        struct S(f32, usize);
        impl Eq for S {}
        impl PartialOrd for S {
            fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(o))
            }
        }
        impl Ord for S {
            fn cmp(&self, o: &Self) -> std::cmp::Ordering {
                o.0.total_cmp(&self.0)
            }
        }
        let mut dist: HashMap<usize, f32> = HashMap::from([(from, 0.0)]);
        let mut prev: HashMap<usize, usize> = HashMap::new();
        let mut heap = BinaryHeap::from([S(0.0, from)]);
        while let Some(S(d, n)) = heap.pop() {
            if goal(n) {
                let mut out = vec![n];
                let mut k = n;
                while let Some(&p) = prev.get(&k) {
                    out.push(p);
                    k = p;
                }
                out.reverse();
                return Some(out);
            }
            if dist.get(&n).is_some_and(|&b| b < d) {
                continue;
            }
            for &m in &self.adj[n] {
                if !blocked.is_empty() && blocked.contains(&(self.place(n), self.place(m))) {
                    continue;
                }
                if avoid.closed.contains(&m) && !avoid.closed.contains(&n) && !goal(m) {
                    continue;
                }
                // Never entered unless it is the goal (exit portals: a start
                // node inside the zone must not route through it).
                if avoid.forbidden.contains(&m) && !goal(m) {
                    continue;
                }
                // Edges the player-like test rejects: nearly closed (with
                // the 1.2 radius they cross walls the player cannot, G2's
                // stair wall); kept for gaps the sweep misjudges.
                let k = if self.hard.contains(&(n, m)) {
                    HARD_COST
                } else if self.ride.contains(&(n, m)) {
                    RIDE_COST
                } else {
                    1.0
                };
                // Drops lose height that may have taken lifts to gain.
                let drop = if self.pts[m][1] < self.pts[n][1] - 2.0 { 15.0 } else { 0.0 };
                let drop = drop
                    + if avoid.soft.contains(&m) {
                        25.0
                    } else if avoid.hazard.contains(&m) {
                        avoid.hazard_cost
                    } else if avoid.cheap.contains(&m) {
                        4.0
                    } else {
                        0.0
                    };
                // Clearance: a node missing neighbours lies by a wall or a
                // ledge; the player (radius 1.5) slides there.
                let drop = drop + (8usize.saturating_sub(self.adj[m].len())) as f32 * 0.6;
                let nd = d + (dist_xz(self.pts[n], self.pts[m]) + (self.pts[m][1] - self.pts[n][1]).abs() * 0.5) * k + drop;
                if dist.get(&m).is_none_or(|&b| nd < b) {
                    dist.insert(m, nd);
                    prev.insert(m, n);
                    heap.push(S(nd, m));
                }
            }
        }
        None
    }
}

/// Nodes routes go around: `soft` ones cost extra (containers, obstacles),
/// `closed` ones are not entered (gates while no key is held).
#[derive(Default)]
pub struct Avoid {
    pub soft: HashSet<usize>,
    /// Breakable barrels (kind 10 subtypes 43..45: BAROBJ, BAREXP,
    /// BARPOI): a smaller cost, the bot breaks them on its way (G2's
    /// ground route runs through dozens; at the full cost the route took a
    /// raised walkway that ends above the ground).
    pub cheap: HashSet<usize>,
    pub closed: HashSet<usize>,
    /// Nodes no route enters except as its goal (exit portals).
    pub forbidden: HashSet<usize>,
    /// Hazards' nodes (kind 8: spikes, flames) and what entering one costs.
    pub hazard: HashSet<usize>,
    pub hazard_cost: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Goal {
    Item(usize),
    /// A route hint's point (within its radius).
    Point(usize),
    /// A reachable graph node in a column the player has not stood in.
    Explore(usize),
}

/// A moving object as the bot has seen it (sampled every 5 updates): the
/// lowest and highest translation y, the last y and direction, and how often
/// the direction reversed. Only objects that came back (a reversal) are
/// movers for ride edges: one that rose once and stays (G1's platform by
/// (62, 21, −157), b60: 183 ride timeouts) is level geometry again.
#[derive(Clone, Copy, Debug)]
struct MoverSeen {
    lo: f32,
    hi: f32,
    last: f32,
    dir: i8,
    reversals: u32,
    /// The update the object last moved.
    moved_at: usize,
}

/// One route hint (bot=1 hints=FILE): steps taken in order before the
/// ordinary goals — an item to collect or a trigger to touch (by slot), a
/// point to reach, or a pause. Hints are authored from map analysis; they
/// choose what to do next, the movement stays the player's own.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Hint {
    pub item: Option<usize>,
    pub goto: Option<[f32; 3]>,
    pub radius: Option<f32>,
    pub wait: Option<usize>,
}

/// The DEBUG health assist's baseline: the health at the level's start, or
/// 80 % of the class maximum for the player's level when that is more (a
/// chained campaign carries health between levels: B6 began at 86, below a
/// dragon breath, and the assist never fired).
pub fn assist_baseline(g: &Game) -> f32 {
    let r = &g.players[0];
    r.f32_(HEALTH).max(0.8 * gdl_core::player_class::max_health(r.i32_(0x2ac0)))
}

/// Where to shoot a switch at `q` from: a reachable node near it (within
/// SHOOT_RANGE, not above it: D2's 405 was hit from the floor 5 units below,
/// missed from 4 above), else one at about its height within SHOT_FAR whose
/// level line to it is clear for a missile (WeaponWallCollide's query, mask
/// 0x23e). A switch counts only while visible (+0xd8 0x4000, the camera's
/// view): D2's 406, 6 units under the walkway beside it, was not hit from
/// the corridor's far end 58 units away.
fn shot_node(nav: &Nav, g: &Game, q: V3) -> Option<usize> {
    let score = |n: usize| dist_xz(nav.pts[n], q) + if q[1] < nav.pts[n][1] { 20.0 } else { 0.0 };
    let near = (0..nav.pts.len())
        .filter(|&n| !nav.adj[n].is_empty() && dist_xz(nav.pts[n], q) < SHOOT_RANGE && (q[1] - nav.pts[n][1]).abs() < 8.0)
        .min_by(|&a, &c| score(a).total_cmp(&score(c)));
    if near.is_some_and(|n| nav.pts[n][1] <= q[1] + 1.0) {
        return near;
    }
    let mut far: Vec<usize> = (0..nav.pts.len())
        .filter(|&n| !nav.adj[n].is_empty() && dist_xz(nav.pts[n], q) < SHOT_FAR && 3.0 < dist_xz(nav.pts[n], q) && (nav.pts[n][1] + 1.5 - q[1]).abs() < 2.0)
        .collect();
    far.sort_by(|&a, &c| dist_xz(nav.pts[a], q).total_cmp(&dist_xz(nav.pts[c], q)));
    let mut c = g.collision.clone();
    for n in far.into_iter().take(40) {
        let from = [nav.pts[n][0], q[1], nav.pts[n][2]];
        let hit = c.world_collide(Query {
            radius: 0.3,
            first: from,
            last: q,
            check_flags: 7,
            min_normal_y: -2.0,
            max_normal_y: 2.0,
            object_mask: 0x23e,
            exclusion_mask: 4,
        });
        if matches!(hit, Ok((ref w, _)) if w.solid.is_none()) {
            return Some(n);
        }
    }
    near
}

/// No wall between floor node `p` and item `q` one unit above the node
/// (PlayerWallCollide's query, radius 0.3).
fn clear_line(coll: &mut Option<CollisionWorld>, g: &Game, p: V3, q: V3) -> bool {
    clear_line_skip(coll, g, p, q, None)
}

/// Why item `s` has or lacks a target node (DIAGNOSTIC): each node within
/// 2.5 units in the item's and neighbouring columns with its distance,
/// height difference, reachability and the line test's first hit.
pub fn explain_item(nav: &Nav, g: &Game, s: usize) -> serde_json::Value {
    let q = g.items.records[s].v3(0x40);
    let bound = g.items.records[s].u32_(0xf0).checked_sub(1).map(|o| o as usize);
    let mut c = g.collision.clone();
    let k = key(q[0], q[2]);
    let mut out = Vec::new();
    for dx in -1..=1 {
        for dz in -1..=1 {
            for &n in nav.cols.get(&(k.0 + dx, k.1 + dz)).map(Vec::as_slice).unwrap_or(&[]) {
                let p = nav.pts[n];
                let d = dist_xz(p, q);
                if 2.5 <= d {
                    continue;
                }
                let h = p[1] + 1.0;
                let hit = c.world_collide(Query { radius: 0.3, first: [p[0], h, p[2]], last: [q[0], h, q[2]], check_flags: 7, min_normal_y: f32::from_bits(0xbf5d_b22d), max_normal_y: f32::from_bits(0x3f5d_b22d), object_mask: 0x13a, exclusion_mask: 1 });
                let first = match hit { Ok((w, _)) => w.solid.map(|h| (h.object, format!("{:#x}", c.runtime_flags[h.object]))), Err(_) => None };
                // An unreachable node: whether the game's own displacement
                // walks into it from the nearest reachable node (walk_reaches).
                let walk = if nav.adj[n].is_empty() {
                    (0..nav.pts.len())
                        .filter(|&m| !nav.adj[m].is_empty() && dist_xz(nav.pts[m], p) < 2.2 && (nav.pts[m][1] - p[1]).abs() < 1.5)
                        .min_by(|&a, &b| dist_xz(nav.pts[a], p).total_cmp(&dist_xz(nav.pts[b], p)))
                        .map(|m| (nav.pts[m], walk_reaches(&mut c, nav.pts[m], p)))
                } else {
                    None
                };
                out.push(serde_json::json!({"node": p, "obj": nav.obj[n], "d": d, "dy": q[1] - p[1], "reachable": !nav.adj[n].is_empty(), "hit": first, "walk_from": walk}));
            }
        }
    }
    serde_json::json!({"slot": s, "pos": q, "bound": bound, "kind": item_kind(g, s), "flags_f4": g.items.records[s].u16_(0xf4), "nodes": item_nodes(nav, g, &[s]), "candidates": out})
}

/// Graph edges with over `min_dy` of height change inside an x/z box
/// (DIAGNOSTIC): whether the game's own displacement walks each one
/// (walk_reaches) and both floors' objects.
pub fn explain_walks(nav: &Nav, g: &Game, lo: [f32; 2], hi: [f32; 2], min_dy: f32) -> serde_json::Value {
    let mut c = g.collision.clone();
    let inside = |p: V3| lo[0] <= p[0] && p[0] <= hi[0] && lo[1] <= p[2] && p[2] <= hi[1];
    let mut out = Vec::new();
    for a in 0..nav.pts.len() {
        if !inside(nav.pts[a]) {
            continue;
        }
        for &b in &nav.adj[a] {
            let (pa, pb) = (nav.pts[a], nav.pts[b]);
            if (pb[1] - pa[1]).abs() < min_dy {
                continue;
            }
            out.push(serde_json::json!({"from": pa, "to": pb, "objs": [nav.obj[a], nav.obj[b]], "hard": nav.is_hard(a, b), "walks": walk_reaches(&mut c, pa, pb)}));
        }
    }
    serde_json::json!(out)
}

/// `clear_line`, a hit on object `skip` (an item's own bound object) not
/// counting.
fn clear_line_skip(coll: &mut Option<CollisionWorld>, g: &Game, p: V3, q: V3, skip: Option<usize>) -> bool {
    let c = coll.get_or_insert_with(|| g.collision.clone());
    let h = p[1] + 1.0;
    let hit = c.world_collide(Query {
        radius: 0.3,
        first: [p[0], h, p[2]],
        last: [q[0], h, q[2]],
        check_flags: 7,
        min_normal_y: f32::from_bits(0xbf5d_b22d),
        max_normal_y: f32::from_bits(0x3f5d_b22d),
        object_mask: 0x13a,
        exclusion_mask: 1,
    });
    !matches!(hit, Ok((ref w, _)) if w.solid.is_some_and(|h| c.runtime_flags[h.object] & 0x38 == 0 && Some(h.object) != skip))
}

/// Reachable graph nodes within 2 units of each item (slot, node).
pub fn item_nodes(nav: &Nav, g: &Game, slots: &[usize]) -> Vec<(usize, usize)> {
    item_nodes_ex(nav, g, slots, true)
}

/// `item_nodes`; `strike` admits strike nodes (strike_reach) for items with
/// no node within touch.
pub fn item_nodes_ex(nav: &Nav, g: &Game, slots: &[usize], strike: bool) -> Vec<(usize, usize)> {
    // A collision copy for the wall checks, made on first use.
    let mut coll: Option<CollisionWorld> = None;
    let mut out = Vec::new();
    for &s in slots {
        // A trigger that counts only players standing on its bound object
        // (+0xf4 0x100, e.g. lifts): the nearest node on that object (or a
        // child of it) within the trigger's reach.
        let r = &g.items.records[s];
        if item_kind(g, s).0 == 5
            && r.u16_(0xf4) & 0x100 != 0
            && let Some(b) = r.u32_(0xf0).checked_sub(1).map(|o| o as usize)
        {
            let q = r.v3(0x40);
            let reach = r.f32_(0xfc).max(r.f32_(0xe8)).max(2.5) + 1.5;
            let on = |o: usize| o == b || g.collision.objects.get(o).is_some_and(|c| c.parent == Some(b));
            let best = (0..nav.pts.len())
                .filter(|&n| !nav.adj[n].is_empty() && on(nav.obj[n]) && dist_xz(nav.pts[n], q) < reach)
                .min_by(|&a, &c| dist_xz(nav.pts[a], q).total_cmp(&dist_xz(nav.pts[c], q)));
            if let Some(n) = best {
                out.push((s, n));
            }
            continue;
        }
        let q = g.items.records[s].v3(0x40);
        // A shot switch (kind 5 subtype 0x1f: WeaponCheckItemCollision lets
        // missiles hit it; D2's elevator call 405 sits 5 units above the
        // floor): any reachable node within SHOOT_RANGE, nearest first.
        if item_kind(g, s) == (5, 0x1f) {
            if let Some(n) = shot_node(nav, g, q) {
                out.push((s, n));
            }
            continue;
        }
        // A trigger (kind 5) is touched by distance, walls or not
        // (item_coll_sub: within the player's radius + the definition's
        // reach +0xc, +0xfc when set, doubled for subtype 0x1b): a node
        // nearer than that, less 0.3, counts even when the line test fails. I2's
        // trigger 495 fires through wall 1192 from 1.1 units (b37 took it so);
        // J3's 502, 2.2 from the nearest node behind wall 1512, never did.
        let touch = (item_kind(g, s).0 == 5).then(|| trigger_touch(g, s) - 0.3);
        let k = key(q[0], q[2]);
        // The item's bound object (+0xf0) is no wall for its own line test:
        // I2's trigger 495 sits on door 262, which it opens (c38: no target
        // node, hint 11 and the rune's route after it skipped).
        let bound = g.items.records[s].u32_(0xf0).checked_sub(1).map(|o| o as usize);
        let mut best: Option<(f32, usize)> = None;
        for dx in -1..=1 {
            for dz in -1..=1 {
                for &n in nav.cols.get(&(k.0 + dx, k.1 + dz)).map(Vec::as_slice).unwrap_or(&[]) {
                    let p = nav.pts[n];
                    if nav.adj[n].is_empty() || !(-3.0..=3.0).contains(&(q[1] - p[1])) {
                        continue;
                    }
                    let d = dist_xz(p, q);
                    // Not through a wall: J3's trigger 502 stands 1.4 behind
                    // the static wall 1512 from the floor the bot reached; a
                    // node 2.2 away on that side was taken as within reach
                    // (2521 stuck events in b56).
                    let near = (d < 2.5 && (d <= 1.0 || clear_line_skip(&mut coll, g, p, q, bound))) || touch.is_some_and(|t| d < t);
                    if near && best.is_none_or(|b| d < b.0) {
                        best = Some((d, n));
                    }
                }
            }
        }
        // No node within touch: a trigger is struck instead (strike_reach),
        // from the nearest node in range.
        if best.is_none()
            && strike
            && let Some(sr) = strike_reach(g, s)
        {
            for dx in -3..=3 {
                for dz in -3..=3 {
                    for &n in nav.cols.get(&(k.0 + dx, k.1 + dz)).map(Vec::as_slice).unwrap_or(&[]) {
                        let p = nav.pts[n];
                        let d = dist_xz(p, q);
                        if !nav.adj[n].is_empty() && (-3.0..=3.0).contains(&(q[1] - p[1])) && d < sr && best.is_none_or(|b| d < b.0) {
                            best = Some((d, n));
                        }
                    }
                }
            }
        }
        if let Some((_, n)) = best {
            out.push((s, n));
        }
    }
    out
}

/// The player radius (+0x828; 1.5 when no bot set it).
fn player_radius() -> f32 {
    PLAYER_DIMS.get().map_or(1.5, |p| p.0)
}

/// A trigger's contact distance: item_coll_sub's player radius + reach
/// (+0xc of the definition, +0xfc when set, doubled for subtype 0x1b).
fn trigger_touch(g: &Game, s: usize) -> f32 {
    let r = &g.items.records[s];
    let d = &g.items.definitions[r.definition()];
    let reach = if item_kind(g, s).0 != 5 {
        d.f32_(0xc)
    } else if 0.0 < r.f32_(0xfc) {
        r.f32_(0xfc)
    } else if d.subtype() == 0x1b {
        2.0 * d.f32_(0xc)
    } else {
        d.f32_(0xc)
    };
    player_radius() + reach
}

/// How far a trigger can be struck from: an attack (action event & 0xfe)
/// with the stick released and no item in contact (+0x880 0) probes the
/// items at the player's position + 2 radii along the facing
/// (PlayerMotion, PlayerCollideItems at `probe`), walls not tested — so a
/// trigger behind a wall within 2 radii + its contact distance (less 0.5)
/// is touched by striking toward it (I4's trigger 534, three nodes behind
/// wall 2644; J3's 502 behind wall 1512). Stand-on triggers (+0xf4 0x100)
/// and shot switches (subtype 0x1f) excluded; rune stones and legend items
/// included (picked up by the probe).
fn strike_reach(g: &Game, s: usize) -> Option<f32> {
    let r = &g.items.records[s];
    let trigger = item_kind(g, s).0 == 5 && r.u16_(0xf4) & 0x100 == 0 && item_kind(g, s).1 != 0x1f;
    // Rune stones and legend items too: the probe's PlayerItemCollision picks
    // them up (K1's rune 192, 3.8 units across a gap from the floor the bot
    // reached, at its height; k1h-a-d149).
    let treasure = matches!(item_kind(g, s), (1, 10) | (1, 13));
    (trigger || treasure).then(|| 2.0 * player_radius() + trigger_touch(g, s) - 0.5)
}

/// Without the assist: the stick direction (dx, dz) turned by the least of
/// ±30°, ±60°, ±90°, ±120° that keeps the points 0.5, 1.0 and 1.5 units
/// ahead out of contact with every dormant statue (kind 4, shown, +0xf8
/// bit 0 clear: contact or a hit wakes it, and G2's KNI statues' creatures
/// hit for 29–39 from 6–20 units — 290–540 of the 850–960 health each
/// Easy G2 try lost, sg2e) and every live hazard (kind 8 in state 2 or 4
/// with +0xd8 bit 0, as PlayerSkipItem leaves them); contact = the player's
/// radius + the item's reach (+0xc) + 0.4. Unchanged when already in
/// contact or when no turn helps. Path costs keep the routes off them; this
/// covers stepping away from foes, back-offs and corner cutting (sg2e-d541:
/// statues 113 and 114 passed at 1.0 units before the first burst).
fn keep_off(g: &Game, p: V3, dx: f32, dz: f32) -> (f32, f32) {
    let l = dx.hypot(dz);
    if l < 1e-3 {
        return (dx, dz);
    }
    let rad = player_radius();
    let near: Vec<(V3, f32)> = (0..g.items.records.len())
        .filter_map(|s| {
            let r = &g.items.records[s];
            if !r.active() || r.u8_(0xe1) != 0 || CONTACT_OK.with(|c| c.borrow().contains(&s)) {
                return None;
            }
            let (k, _) = item_kind(g, s);
            let live = match k {
                4 => r.u32_(0xf8) & 1 == 0 && r.u16_(0xd8) & 1 == 0,
                8 => matches!(r.i8_(0xdc), 2 | 4) && r.u16_(0xd8) & 1 != 0,
                _ => false,
            };
            let q = r.v3(0x40);
            let c = rad + g.items.definitions[r.definition()].f32_(0xc).clamp(0.0, 4.0) + 0.4;
            (live && dist_xz(q, p) < c + 2.0 && (q[1] - p[1]).abs() < 3.0).then_some((q, c))
        })
        .collect();
    if near.is_empty() || near.iter().any(|&(q, c)| dist_xz(q, p) < c) {
        return (dx, dz);
    }
    let safe = |a: f32| {
        let (u, v) = (dx / l, dz / l);
        let (s, c0) = a.sin_cos();
        let (ux, uz) = (u * c0 + v * s, -u * s + v * c0);
        [0.5, 1.0, 1.5].iter().all(|&k| near.iter().all(|&(q, c)| c <= dist_xz([p[0] + ux * k, p[1], p[2] + uz * k], q)))
            .then_some((ux * l, uz * l))
    };
    for deg in [0.0f32, 30.0, -30.0, 60.0, -60.0, 90.0, -90.0, 120.0, -120.0] {
        if let Some(d) = safe(deg.to_radians()) {
            return d;
        }
    }
    (dx, dz)
}

/// The absolute difference of two angles (radians, at most π).
fn angle_diff(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(2.0 * std::f32::consts::PI);
    d.min(2.0 * std::f32::consts::PI - d)
}

/// Whether the attack probe facing `w` (atan2 of x over z) from `p` touches
/// item `s`: PlayerMotion probes PlayerCollideItems at the position + 2
/// radii along the facing; item_coll_sub takes the probe within radius +
/// reach of the item's +0x60 in x/z and, for box shape 3, within radius +
/// the definition's +0x14/+0x18 along the item's rows +0x10/+0x30 (J3's 502:
/// reach 0.7, half extents 0.5 — the probe within 2.0 of it on both axes).
/// Each bound less `margin`; the other shapes as the circle (approximate).
fn probe_touches(g: &Game, s: usize, p: V3, w: f32, margin: f32) -> bool {
    let r = &g.items.records[s];
    let d = &g.items.definitions[r.definition()];
    let rad = player_radius();
    let c = r.v3(0x60);
    let e = [p[0] + 2.0 * rad * w.sin() - c[0], p[2] + 2.0 * rad * w.cos() - c[2]];
    if trigger_touch(g, s) - margin < e[0].hypot(e[1]) {
        return false;
    }
    let circle = d.i16_(8) != 3 || (item_kind(g, s).0 == 5 && (0.0 < r.f32_(0xfc) || d.subtype() == 0x1b));
    let (m0, m2) = (r.v3(0x10), r.v3(0x30));
    circle || ((e[0] * m0[0] + e[1] * m0[2]).abs() <= rad + d.f32_(0x14) - margin && (e[0] * m2[0] + e[1] * m2[2]).abs() <= rad + d.f32_(0x18) - margin)
}

/// The nearest enemy within melee reach with attack held, by PlayerMotion's
/// measure (PlayerGetTarget/closest_enemy: +0x70 to the enemy's +0x60 less
/// its width +0x24c, state 1 or 6; melee while that is under radius + 1 +
/// the held-attack extra 1), less a 0.2 margin: its +0x60.
fn melee_foe(g: &Game, p: V3) -> Option<V3> {
    use gdl_core::game::enemies::e;
    let r = &g.players[0];
    let o = r.v3(0x70);
    let reach = r.f32_(0x828) + 2.0 - 0.2;
    g.enemies
        .iter()
        .filter(|en| matches!(en.i32_(e::STATE), 1 | 6) && 0.0 < en.f32_(e::HEALTH) && !matches!(en.i32_(e::TYPE), 0x1e | 0x1f))
        .map(|en| {
            let c = en.v3(e::COLLISION);
            let v = [c[0] - o[0], c[1] - o[1], c[2] - o[2]];
            ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - en.f32_(e::WIDTH), v[1], c)
        })
        .filter(|&(d, dy, c)| d < reach && dy.abs() <= 10.0 && 0.01 < dist_xz(c, p))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, _, c)| c)
}

/// A facing from which an attack swings in melee and its probe touches item
/// `s`, the one nearest `cur` (2° steps), or None. PlayerMotion's attack
/// (code 0xf) swings in melee only with a target within the player's radius
/// + 1 (closest_enemy: the distance from +0x70 to the enemy's +0x60 less its
/// width +0x24c, the enemy in state 1 or 6, inside the cone 0.707 + the
/// distance × 0.293/30 of the facing; target_cone_mode 0: 0.5), else it
/// throws; the swing does not turn the player (+0x8dc picks a side or back
/// swing), so the probe goes along the facing, not toward the foe. Margins:
/// 0.1 off the reach, 0.05 on the cone, 0.25 off the probe's bounds.
fn melee_facing(g: &Game, s: usize, p: V3, cur: f32) -> Option<f32> {
    use gdl_core::game::enemies::e;
    let origin = g.players[0].v3(0x70);
    let reach = player_radius() + 1.0 - 0.1;
    let cone = if 0 < g.target_cone_mode { f32::from_bits(0x3f34_fdf4) } else { 0.5 };
    let foes: Vec<([f32; 2], f32)> = g
        .enemies
        .iter()
        .filter(|en| matches!(en.i32_(e::STATE), 1 | 6) && !matches!(en.i32_(e::TYPE), 0x1e | 0x1f) && 0.0 < en.f32_(e::HEALTH))
        .filter_map(|en| {
            let c = en.v3(e::COLLISION);
            let v = [c[0] - origin[0], c[1] - origin[1], c[2] - origin[2]];
            let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            let d = l - en.f32_(e::WIDTH);
            (v[1].abs() <= 10.0 && d < reach && 0.0 < l).then(|| {
                let h = v[0].hypot(v[2]).max(1e-6);
                ([v[0] / h, v[2] / h], cone + d.max(0.0) * (1.0 - cone) / 30.0 + 0.05)
            })
        })
        .collect();
    if foes.is_empty() {
        return None;
    }
    (0..180)
        .map(|k| (k as f32 * 2.0 - 180.0).to_radians())
        .filter(|&w| foes.iter().any(|(u, c)| u[0] * w.sin() + u[1] * w.cos() >= *c) && probe_touches(g, s, p, w, 0.25))
        .min_by(|&a, &b| angle_diff(a, cur).total_cmp(&angle_diff(b, cur)))
}

pub struct Bot {
    pub nav: Nav,
    collision_at_build: u64,
    path: Vec<usize>,
    at: usize,
    goal: Option<Goal>,
    blocked: HashSet<(Place, Place)>,
    /// Edges into a gate the bot pressed against without a key: blocked
    /// until a key is held (a node inside a gate's closed area may route
    /// through it otherwise).
    gate_blocked: HashSet<(Place, Place)>,
    /// Update of the last rebuild for want of a goal (rate limit).
    idle_rebuild_at: usize,
    given_up: HashSet<usize>,
    /// Stuck events per item goal since it was last (re)opened.
    stuck_goal_counts: HashMap<usize, usize>,
    /// Optional pickups (gold, food, potions, chests) the bot got stuck on
    /// three times: never goals again (no-assist G2: 328 tries at one food
    /// item out of reach while health drained from 600 to 0).
    optional_skip: HashSet<usize>,
    /// The collision object each treasure rests on (a floor query on a
    /// copy of the collision state, once per treasure).
    support_cache: HashMap<usize, Option<usize>>,
    /// The graph region around a treasure the bot cannot reach (built once
    /// from the treasure's position): triggers whose bound objects lie by
    /// it may be the way in (H3's rune island; trigger 503's object 350 at
    /// its edge).
    islands: HashMap<usize, Vec<V3>>,
    /// When a given-up goal was last reopened for being near.
    reopened_at: HashMap<usize, usize>,
    /// Steering at a closed gate's centre until the update (key held).
    gate_push: Option<(usize, V3)>,
    /// Stuck events per next waypoint, and the waypoints routes no longer
    /// enter (five or more: a thin wall the graph does not see).
    stuck_waypoints: HashMap<Place, usize>,
    dead_nodes: HashSet<Place>,
    /// The last potion used (circle).
    potion_at: Option<usize>,
    visited_triggers: HashSet<usize>,
    /// Contacts per trigger (revisits are bounded) and the trigger now
    /// sought again (reached only on contact: its state stays set).
    trigger_visits: HashMap<usize, usize>,
    revisiting: Option<usize>,
    /// Columns the player has stood in (exploration).
    visited_cols: HashSet<(i32, i32)>,
    progress: (usize, V3),
    /// Where the player has stayed within DWELL units since the last goal
    /// choice (an oscillation the half-unit test misses).
    dwell: (usize, V3),
    attack_until: usize,
    start_health: f32,
    /// Player 0's level (+0x2ac0) when the bot started (pexp precondition).
    start_level: i32,
    /// Standing on a reached trigger: (since, position then, last move).
    wait: Option<(usize, V3, usize)>,
    pub assists: u32,
    pub log: Vec<serde_json::Value>,
    pub exit_update: Option<usize>,
    pub exit_wave: Option<i32>,
    pub exit_state: Option<i32>,
    pub static_exit_reachable: bool,
    pub start: V3,
    replans: usize,
    rebuilds: usize,
    force_rebuild: bool,
    last_y: V3,
    hints: Vec<Hint>,
    hint_at: usize,
    hint_stuck: usize,
    hint_wait: Option<usize>,
    /// Slots reached since the current hint began, and the current trigger
    /// hint's state when it began: a trigger hint is done when the bot
    /// reached it or its state changed since (K1: hints whose triggers a
    /// chain had already set counted as done, and the bot never went west).
    hint_reached: HashSet<usize>,
    hint_base: Option<(usize, i8, u8)>,
    /// Followers' (waypoint index, path) by player.
    followers: [(usize, Vec<usize>); 4],
    /// Followers' path to the exit portal while the leader waits on one
    /// (path, next waypoint, next plan update).
    follow_nav: [(Vec<usize>, usize, usize); 4],
    /// Followers' blocked test on the way to the portal (since, position
    /// then, step aside until).
    follow_stuck: [(usize, V3, usize); 4],
    /// The leader's positions every 10 updates (walkable by construction):
    /// followers walk this trail.
    trail: Vec<V3>,
    /// The player's way (update, position) every 2 units for the report
    /// (route_hints.py takes a winning run's approach to the exit from it).
    route: Vec<(usize, V3)>,
    backoff: Option<(usize, V3)>,
    attack_at: Option<V3>,
    /// Updates held without input: stage start/camera wait, exit portal,
    /// riding (reported as `holds`).
    holds: [usize; 3],
    /// The boss attack range: 18, or the foe goal node's distance to the
    /// foe + 4 when no reachable node is closer (C5's ring around the
    /// djinn's pit, 17.4 units at the nearest).
    boss_range: f32,
    bad_nodes: HashSet<Place>,
    last_floor: Option<usize>,
    riding: usize,
    still: usize,
    ride_from: V3,
    /// Updates held riding while pursuing this goal (a platform that never
    /// stops — G1's ferry near (16, 41.4, −320) — held the bot 26,000).
    ride_holds: (Option<Goal>, usize),
    /// The first update of the current stretch without a goal.
    no_goal_since: Option<usize>,
    /// The DEBUG health assist (explore.rs applies its own).
    pub assist_enabled: bool,
    /// loot=1: gold, gems, food (below the maximum health), potions (below
    /// the cap), powerups and chests (with keys to spare) within LOOT_RANGE come
    /// before the other goals — the
    /// economy a campaign without the DEBUG assist lives on (the Tower
    /// shop's healing costs 2.5 gold a point).
    pub loot: bool,
    /// seek_crystals=1: every reachable crystal (kind 1 subtype 0xf) of a set
    /// the player has not filled is a goal before the others. The Tower's
    /// realm doors open on full sets (WorldOpen: class record +0xd0a + 2n
    /// at MAX_CRYSTALS[n], or −1 once delivered); one play of every level
    /// holds 125 of B's 100, 140 of A's 125, 160 of K's 150, 180 of D's
    /// 175, 203 of C's 200, 225 of I's 225 and 250 of J's 250.
    pub crystals: bool,
    /// no_secret=1: secret exits (kind 9 subtype 0x32) are never goals. The
    /// treasure room they lead to returns to the level (SaveStage, then
    /// LoadStage on the timer's skip_wave 0xd), whose regular exit is still
    /// to be reached; a chain that does not play the room and the return
    /// cannot count the secret exit as the level's end (C2: every c90 win
    /// left by SECRET_ICON 516 for S1).
    pub no_secret: bool,
    /// require=1: a level counts only with its rune stone and legend item,
    /// so exits are not goals while one is left (E1: the bot took the exit
    /// at update 113,924 without the soul, which needs pads 134/135 first).
    pub require: bool,
    /// Updates to stand idle after the stage start (bot_delay=N: a
    /// variation of the same start for retries; the simulation is
    /// deterministic, so a retry needs a different input).
    pub delay: usize,
    /// Long attacks spent on each blocking item (slot) and the update of
    /// the last.
    break_tries: HashMap<usize, (u32, f32)>,
    /// No fighting before this update: just carried by a platform.
    disembark_until: usize,
    /// Generators under fire: (update of the last health drop, health).
    gen_fire: HashMap<usize, (usize, u16)>,
    /// Generators the bot no longer fires at (no damage in 300 updates).
    gen_skip: HashSet<usize>,
    /// Objects whose node can move (a run-time y, rotation or animation
    /// matrix on it or an ancestor; rescanned every 300 updates), the
    /// lowest and highest translation y seen of each (sampled every 5
    /// updates) and when the bot began waiting at a ride edge.
    mover_candidates: Vec<usize>,
    mover_scan_at: usize,
    movers: HashMap<usize, MoverSeen>,
    ride_wait: Option<((usize, usize), usize)>,
    /// Updates spent fighting in place while stuck (no assist), since the
    /// last progress.
    fight_hold: usize,
    /// The collision copy of the last graph build (line tests) and each
    /// generator's last line test (update, clear).
    coll: Option<CollisionWorld>,
    gen_seen: HashMap<usize, (usize, bool)>,
    last_health: f32,
    /// The trigger being struck at and since when.
    strike: Option<(usize, usize)>,
    /// Run-ups at each step-up edge (by place) before it is blocked.
    run_ups: HashMap<(Place, Place), u32>,
    /// The strike's melee swing: the stick released until this update.
    melee_until: usize,
    /// Melee swings started by the strike (the first 40 logged).
    melee_swings: usize,
    /// Strike foe diagnostics logged (at most 200).
    strike_logs: usize,
    /// Attack-branch diagnostics logged (tag).
    attack_tags: usize,
    /// Stuck events beside each dormant statue (keep_off's CONTACT_OK).
    statue_stuck: HashMap<usize, u32>,
    /// Fire at each foe: (first, last update, health then) (note_fire).
    foe_fire: HashMap<(u8, usize), (usize, usize, f32)>,
    /// A fight's exchange per foe: (first update, last update, the foe's
    /// health and the player's at the first).
    foe_trade: HashMap<(u8, usize), (usize, usize, f32, f32)>,
    /// The last update the player lost 3 or more health.
    hurt_at: Option<usize>,
    /// An optional goal item the player stands beside, since that update.
    beside: Option<(usize, usize)>,
    /// (update, health, position) every 60 updates (the danger rule).
    trail_hp: std::collections::VecDeque<(usize, f32, V3)>,
    /// Nest mode until this update (the danger rule): generators first
    /// whatever foes are near, potions against the crowd.
    nest_until: usize,
}

fn item_kind(g: &Game, slot: usize) -> (i32, i32) {
    let r = &g.items.records[slot];
    let d = &g.items.definitions[r.definition()];
    (d.kind(), d.subtype())
}

/// Whether item `slot` (at height `qy`) lies within an obstacle's height
/// window of a player or node at `py`: 3 units, and for breakable walls
/// (kind 10 subtype 0x2a, the origin raised in the wall) up to 5 above
/// (H3's wall 8 stands 3.76 above the floor that leads to the rune RUNEE1;
/// b43 never attacked it: 1699 stuck events).
fn in_height(g: &Game, slot: usize, qy: f32, py: f32) -> bool {
    let dy = qy - py;
    dy.abs() < 3.0 || (item_kind(g, slot) == (10, 0x2a) && (0.0..5.0).contains(&dy))
}

impl Bot {
    /// Route hints to follow first (hints=FILE).
    pub fn with_hints(mut self, hints: Vec<Hint>) -> Bot {
        self.hints = hints;
        self
    }

    pub fn new(g: &Game) -> Bot {
        set_player_dims(g);
        CONTACT_OK.with(|c| c.borrow_mut().clear());
        FOE_SKIP.with(|s| s.borrow_mut().0.clear());
        let mut b = Self::blank(g);
        b.rebuild(g);
        let exits: Vec<usize> = (0..g.items.records.len()).filter(|&s| g.items.records[s].active() && item_kind(g, s).0 == 9).collect();
        let targets = b.item_nodes(g, &exits);
        b.static_exit_reachable = !targets.is_empty();
        b
    }

    /// A bot on an already built graph (explore.rs: the state search
    /// shares one graph between its bot macros); it rebuilds as usual when
    /// the collision state has changed since.
    pub fn with_nav(g: &Game, nav: Nav, built_on: u64) -> Bot {
        let mut b = Self::blank(g);
        b.nav = nav;
        b.collision_at_build = built_on;
        b
    }

    /// The graph and the collision state it was built on.
    pub fn nav_state(&self) -> (&Nav, u64) {
        (&self.nav, self.collision_at_build)
    }

    /// The bot's own graph (DIAGNOSTIC navdump while a bot runs): nodes,
    /// the reachable ones, edges, ride edges, movers at the build (object,
    /// lowest, highest, current y) and the blocked edges by place.
    pub fn nav_dump(&self, f: usize) -> serde_json::Value {
        let nav = &self.nav;
        let r1 = |v: f32| (v * 10.0).round() / 10.0;
        let reach: Vec<usize> = (0..nav.pts.len()).filter(|&k| !nav.adj[k].is_empty()).collect();
        let edges: Vec<[usize; 2]> = reach.iter().flat_map(|&a| nav.adj[a].iter().map(move |&b| [a, b])).collect();
        let ride: Vec<[usize; 2]> = nav.ride.iter().map(|&(a, b)| [a, b]).collect();
        let movers: Vec<(usize, [f32; 3])> = nav.movers.iter().map(|(&o, &(lo, hi, cur))| (o, [r1(lo), r1(hi), r1(cur)])).collect();
        serde_json::json!({"scope": "DEBUG bot graph (the running bot's own Nav, ride edges included); not game data", "update": f,
            "pts": nav.pts.iter().map(|q| q.map(r1)).collect::<Vec<_>>(), "obj": nav.obj, "reachable": reach, "edges": edges, "ride": ride, "movers": movers,
            "blocked": self.blocked.iter().collect::<Vec<_>>(), "rebuilds": self.rebuilds,
            "seen": self.movers.iter().filter(|(_, m)| 0.5 <= m.hi - m.lo).map(|(&o, m)| serde_json::json!([o, r1(m.lo), r1(m.hi), r1(m.last), m.reversals, m.moved_at])).collect::<Vec<_>>(),
            "candidates": self.mover_candidates.len()})
    }

    pub fn collision_fingerprint(g: &Game) -> u64 {
        Self::collision_state(g)
    }

    fn blank(g: &Game) -> Bot {
        let start = g.players[0].v3(0x50);
        Bot {
            nav: Nav { pts: Vec::new(), obj: Vec::new(), adj: Vec::new(), hard: HashSet::new(), ride: HashSet::new(), movers: HashMap::new(), cols: HashMap::new(), top: 0.0, bottom: 0.0, radius: 0.0, reached: 0 },
            collision_at_build: 0,
            path: Vec::new(),
            at: 0,
            goal: None,
            blocked: HashSet::new(),
            gate_blocked: HashSet::new(),
            idle_rebuild_at: 0,
            given_up: HashSet::new(),
            stuck_goal_counts: HashMap::new(),
            optional_skip: HashSet::new(),
            support_cache: HashMap::new(),
            islands: HashMap::new(),
            reopened_at: HashMap::new(),
            gate_push: None,
            stuck_waypoints: HashMap::new(),
            dead_nodes: HashSet::new(),
            potion_at: None,
            visited_triggers: HashSet::new(),
            trigger_visits: HashMap::new(),
            revisiting: None,
            visited_cols: HashSet::new(),
            progress: (0, start),
            dwell: (0, start),
            attack_until: 0,
            start_health: assist_baseline(g),
            start_level: g.players[0].i32_(0x2ac0),
            wait: None,
            assists: 0,
            log: Vec::new(),
            exit_update: None,
            exit_wave: None,
            exit_state: None,
            static_exit_reachable: false,
            start,
            replans: 0,
            rebuilds: 0,
            force_rebuild: false,
            last_y: [f32::NAN; 3],
            hints: Vec::new(),
            hint_at: 0,
            hint_stuck: 0,
            hint_wait: None,
            hint_reached: HashSet::new(),
            hint_base: None,
            followers: Default::default(),
            follow_nav: Default::default(),
            follow_stuck: Default::default(),
            trail: Vec::new(),
            route: Vec::new(),
            backoff: None,
            attack_at: None,
            holds: [0; 3],
            boss_range: 18.0,
            bad_nodes: HashSet::new(),
            last_floor: None,
            riding: 0,
            still: 0,
            ride_from: [0.0; 3],
            ride_holds: (None, 0),
            no_goal_since: None,
            assist_enabled: true,
            loot: false,
            crystals: false,
            no_secret: false,
            require: false,
            break_tries: HashMap::new(),
            disembark_until: 0,
            gen_fire: HashMap::new(),
            gen_skip: HashSet::new(),
            delay: 0,
            mover_candidates: Vec::new(),
            mover_scan_at: 0,
            movers: HashMap::new(),
            ride_wait: None,
            fight_hold: 0,
            coll: None,
            gen_seen: HashMap::new(),
            last_health: f32::NAN,
            strike: None,
            run_ups: HashMap::new(),
            melee_until: 0,
            melee_swings: 0,
            strike_logs: 0,
            attack_tags: 0,
            statue_stuck: HashMap::new(),
            foe_fire: HashMap::new(),
            foe_trade: HashMap::new(),
            hurt_at: None,
            beside: None,
            trail_hp: std::collections::VecDeque::new(),
            nest_until: 0,
        }
    }

    /// A fingerprint of the collision state that moves floors: runtime
    /// flags, item-driven node y, rotations, world-animation locals.
    fn collision_state(g: &Game) -> u64 {
        use std::hash::{Hash, Hasher};
        let c = &g.collision;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        c.runtime_flags.hash(&mut h);
        for v in &c.node_y {
            v.map(f32::to_bits).hash(&mut h);
        }
        for (k, m) in c.node_rot.values() {
            (k, m.map(|r| r.map(f32::to_bits))).hash(&mut h);
        }
        // World animations' node matrices (doors, rotating platforms).
        for (k, m) in c.node_local.values() {
            (k, m.map(|r| r.map(f32::to_bits))).hash(&mut h);
        }
        h.finish()
    }

    fn rebuild(&mut self, g: &Game) {
        self.rebuilds += 1;
        let mut c = g.collision.clone();
        let p = g.players[0].v3(0x50);
        let (bottom, top) = (g.globals.world_bottom - 2.0, p[1] + 80.0);
        // A permissive sweep (radius 0.3, 1.5 above the higher floor): the
        // player's own wall test has step-object exceptions; edges it cannot
        // walk are blocked when the bot gets stuck on them.
        let mut seeds = vec![p];
        seeds.extend(self.nav.pts.iter().zip(&self.nav.adj).filter(|(_, a)| !a.is_empty()).map(|(q, _)| *q));
        let movers = self
            .movers
            .iter()
            .filter(|(_, m)| MOVER_MIN <= m.hi - m.lo && 1 <= m.reversals)
            .filter_map(|(&o, s)| g.collision.object_matrix(o).ok().map(|m| (o, (s.lo, s.hi, m[3][1]))))
            .collect();
        self.nav = Nav::build_with_movers(&mut c, &seeds, bottom, top, 0.3, movers);
        self.collision_at_build = Self::collision_state(g);
        self.coll = Some(c);
    }

    /// Movers' travel: the objects that can move (rescanned every 300
    /// updates) and their translation y range, every 5 updates.
    fn sample_movers(&mut self, f: usize, g: &Game) {
        if !f.is_multiple_of(5) {
            return;
        }
        let c = &g.collision;
        if self.mover_scan_at <= f {
            self.mover_scan_at = f + 300;
            let own = |o: usize| c.node_y.get(o).is_some_and(Option::is_some) || c.node_local[o].is_some() || c.node_rot[o].is_some();
            self.mover_candidates = (0..c.objects.len())
                .filter(|&o| {
                    let mut k = Some(o);
                    for _ in 0..32 {
                        let Some(x) = k else { break };
                        if own(x) {
                            return true;
                        }
                        k = c.objects[x].parent;
                    }
                    false
                })
                .collect();
        }
        for &o in &self.mover_candidates {
            if let Ok(m) = c.object_matrix(o) {
                let y = m[3][1];
                let e = self.movers.entry(o).or_insert(MoverSeen { lo: y, hi: y, last: y, dir: 0, reversals: 0, moved_at: 0 });
                e.lo = e.lo.min(y);
                e.hi = e.hi.max(y);
                let dy = y - e.last;
                if 0.01 < dy.abs() {
                    let d = if 0.0 < dy { 1 } else { -1 };
                    if e.dir != 0 && d != e.dir {
                        e.reversals += 1;
                    }
                    e.dir = d;
                    e.moved_at = f;
                }
                e.last = y;
            }
        }
    }

    /// Reachable graph nodes within 2 units of each item (slot, node).
    fn item_nodes(&self, g: &Game, slots: &[usize]) -> Vec<(usize, usize)> {
        item_nodes(&self.nav, g, slots)
    }

    /// Picks the next goal. The graph is regrown from the player only when
    /// needed (BFS with collision sweeps is the expensive part): off the
    /// graph, after a trigger wait, or when no goal is reachable and the
    /// collision state changed since the last build.
    fn choose(&mut self, f: usize, g: &mut Game) {
        self.replans += 1;
        let p = g.players[0].v3(0x50);
        self.dwell = (f, p);
        // Objects moved since the graph was built (doors swung open by a
        // trigger the player brushed, platforms): regrow it, at most every
        // 900 updates (rebuilds are the slow part in large levels).
        let moved = Self::collision_state(g) != self.collision_at_build && (self.idle_rebuild_at == 0 || 900 <= f - self.idle_rebuild_at);
        if moved {
            self.idle_rebuild_at = f.max(1);
        }
        if self.force_rebuild || moved || self.nav.locate_cached(p, &HashSet::new()).is_none() {
            self.force_rebuild = false;
            self.rebuild(g);
        }
        if self.plan(f, g) {
            self.no_goal_since = None;
            return;
        }
        // Nothing to do: regrow from here after objects moved (300 updates
        // apart), and every 900 updates regardless — the player may stand
        // where the last build could not reach (K4: the island its ferry
        // reached after the graph was built, b12).
        let since = if self.idle_rebuild_at == 0 { usize::MAX } else { f - self.idle_rebuild_at };
        if (Self::collision_state(g) != self.collision_at_build && 300 <= since) || 900 <= since {
            self.idle_rebuild_at = f.max(1);
            self.rebuild(g);
            if self.plan(f, g) {
                return;
            }
        }
        // Nothing reachable: the edges blocked after stuck events may have
        // cut the bot off (they are guesses); forget them and try again.
        if !self.blocked.is_empty() || !self.bad_nodes.is_empty() {
            self.log.push(serde_json::json!({"update": f, "unblocked": self.blocked.len()}));
            self.blocked.clear();
            self.bad_nodes.clear();
            if self.plan(f, g) {
                return;
            }
        }
        // Still nothing for 900 updates: forget the given-up goals, gate
        // blocks and explored columns and regrow the graph (H3 and D1 b10
        // idled for 40,000 updates with every goal given up; enemies, doors
        // and lifts have moved since).
        let since = *self.no_goal_since.get_or_insert(f);
        if 900 <= f - since {
            self.no_goal_since = Some(f);
            self.log.push(serde_json::json!({"update": f, "reset": self.given_up.len()}));
            self.given_up.clear();
            self.gate_blocked.clear();
            self.dead_nodes.clear();
            self.stuck_waypoints.clear();
            self.visited_cols.clear();
            self.visited_triggers.clear();
            self.rebuild(g);
            if self.plan(f, g) {
                self.no_goal_since = None;
                return;
            }
        }
        self.goal = None;
        self.path.clear();
    }

    /// Graph nodes beside items that block the way: chests, barrels and
    /// obstacles (routes avoid them when they can); closed gates while no key
    /// is held (not entered: use_key opens a gate on contact).
    fn obstacle_nodes(&self, g: &Game) -> Avoid {
        let keys = g.players[0].i32_(KEYS);
        // Hazards cost what other obstacles do with the DEBUG assist; without
        // it a detour of up to 150 units is cheaper (no-assist G2 in lg7: 36
        // to 146 health a try lost by its SPIKES, e.g. by (−64, 0, −5)).
        let mut out = Avoid { hazard_cost: if self.assist_enabled { 25.0 } else { 150.0 }, ..Avoid::default() };
        if !self.dead_nodes.is_empty() {
            for n in 0..self.nav.pts.len() {
                if self.dead_nodes.contains(&self.nav.place(n)) {
                    out.closed.insert(n);
                }
            }
        }
        for s in 0..g.items.records.len() {
            let r = &g.items.records[s];
            let (k, t) = item_kind(g, s);
            // Generators (kind 3) once three attacks did not damage them.
            let spent = k == 3 && self.break_tries.get(&s).is_some_and(|&(n, _)| 3 <= n);
            // Hazards (kind 8: spikes, traps) too, at the soft cost.
            // Without the assist statues (kind 4) cost what hazards do:
            // contact or a hit wakes them (+0xf8 bit 0), and their creatures
            // did 290 of the 790 health hurt-g2-easy-v2 lost (G2 holds 53
            // statues). Closed, with 2.5 units around, they shut lg11's try 2
            // in by (−89, 1, −26) for 155,000 updates.
            let statue = k == 4 && !self.assist_enabled;
            // no_secret=1: a secret exit is closed (contact takes it).
            let secret = k == 9 && self.no_secret && t == 0x32;
            if !r.active() || !(k == 2 || k == 8 || k == 10 || spent || (k == 7 && keys < 1) || statue || secret) {
                continue;
            }
            let q = r.v3(0x40);
            let reach = r.f32_(0xe8).clamp(0.5, 4.0) + if statue { 1.5 } else { 1.0 };
            let c = key(q[0], q[2]);
            let span = (reach / CELL).ceil() as i32;
            for dx in -span..=span {
                for dz in -span..=span {
                    for &n in self.nav.cols.get(&(c.0 + dx, c.1 + dz)).map(Vec::as_slice).unwrap_or(&[]) {
                        let pn = self.nav.pts[n];
                        if dist_xz(pn, q) < reach && in_height(g, s, q[1], pn[1]) {
                            if k == 7 || secret {
                                out.closed.insert(n);
                            } else if k == 8 || statue {
                                out.hazard.insert(n);
                            } else if k == 10 && (43..=45).contains(&item_kind(g, s).1) && (self.assist_enabled || item_kind(g, s).1 == 43) {
                                // Without the assist explosive and poison
                                // barrels (BAREXP 44, BARPOI 45) are not
                                // broken on the way: their blasts (12 and 6.5
                                // units, the poison cloud lingering) hurt.
                                out.cheap.insert(n);
                            } else {
                                out.soft.insert(n);
                            }
                        }
                    }
                }
            }
        }
        // Without the assist the nodes inside a live poison cloud (an effect
        // with damage flags 0x800: StartExplosion kind 0x19, a broken BARPOI,
        // blast 6.5) are closed (lg11 try 1: 230 health lost to 4.1-point
        // ticks every 31 updates).
        if !self.assist_enabled {
            for (i, node) in g.effects.nodes.iter().enumerate() {
                let (Some(node), Some(r)) = (node, g.effects.slots.get(i)) else { continue };
                if r.u32_(gdl_core::game::effects::fx::DAMAGE_FLAGS) & 0x800 == 0 || r.f32_(gdl_core::game::effects::fx::DAMAGE) <= 0.0 {
                    continue;
                }
                let q = node.position;
                let reach = r.f32_(gdl_core::game::effects::fx::BLAST).clamp(1.0, 12.0) + 1.5;
                let c = key(q[0], q[2]);
                let span = (reach / CELL).ceil() as i32;
                for dx in -span..=span {
                    for dz in -span..=span {
                        for &n in self.nav.cols.get(&(c.0 + dx, c.1 + dz)).map(Vec::as_slice).unwrap_or(&[]) {
                            let pn = self.nav.pts[n];
                            if dist_xz(pn, q) < reach && (pn[1] - q[1]).abs() < 4.0 {
                                out.closed.insert(n);
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// Nodes within an exit's reach (+0xe8 and the player's radius): routes
    /// to other goals go around them. E1's way to pad 159 (the legend
    /// item's pedestal) passed the exit portal, and the bot left the level
    /// without the item (update 245,365).
    fn exit_nodes(&self, g: &Game) -> HashSet<usize> {
        let mut out = HashSet::new();
        for s in 0..g.items.records.len() {
            let r = &g.items.records[s];
            if !r.active() || item_kind(g, s).0 != 9 {
                continue;
            }
            let q = r.v3(0x40);
            // The player's radius and a margin past the exit's reach (E1:
            // exit state 4 at 4.33 units from the centre while fighting).
            let reach = r.f32_(0xe8).max(1.0) + 3.0;
            let c = key(q[0], q[2]);
            let span = (reach / CELL).ceil() as i32;
            for dx in -span..=span {
                for dz in -span..=span {
                    for &n in self.nav.cols.get(&(c.0 + dx, c.1 + dz)).map(Vec::as_slice).unwrap_or(&[]) {
                        let pn = self.nav.pts[n];
                        if dist_xz(pn, q) < reach && (pn[1] - q[1]).abs() < 3.0 {
                            out.insert(n);
                        }
                    }
                }
            }
        }
        out
    }

    /// The next goal on the current graph; false when none is reachable.
    fn plan(&mut self, f: usize, g: &mut Game) -> bool {
        // A given-up goal the player has come near again gets another try
        // (at most every 2000 updates): G2's regular exit, given up behind
        // barrels at update 30,000, lay 15 units away at 42,500 and the bot
        // took the secret exit beside it.
        let p0 = g.players[0].v3(0x50);
        let near: Vec<usize> = self
            .given_up
            .iter()
            .copied()
            .filter(|&s| {
                let q = g.items.records[s].v3(0x40);
                dist_xz(q, p0) < 25.0 && (q[1] - p0[1]).abs() < 6.0 && self.reopened_at.get(&s).is_none_or(|&u| u + 2000 <= f)
            })
            .collect();
        for s in near {
            self.given_up.remove(&s);
            self.stuck_goal_counts.insert(s, 30);
            self.reopened_at.insert(s, f);
            self.log.push(serde_json::json!({"update": f, "reopened_near": s}));
        }
        let avoid = self.obstacle_nodes(g);
        // Routes to anything but an exit stay out of the exits' reach.
        let avoid_x = Avoid {
            soft: avoid.soft.clone(),
            cheap: avoid.cheap.clone(),
            closed: avoid.closed.clone(),
            forbidden: self.exit_nodes(g),
            hazard: avoid.hazard.clone(),
            hazard_cost: avoid.hazard_cost,
        };
        let is_exit = |s: usize| item_kind(g, s).0 == 9;
        if 0 < g.players[0].i32_(KEYS) {
            self.gate_blocked.clear();
        }
        let blocked: HashSet<(Place, Place)> = self.blocked.union(&self.gate_blocked).copied().collect();
        let p = g.players[0].v3(0x50);
        let Some(here) = self.nav.locate_cached(p, &self.bad_nodes).or_else(|| self.nav.locate_cached(p, &HashSet::new())) else {
            return false;
        };
        // Route hints first, in order.
        while let Some(h) = self.hints.get(self.hint_at).cloned() {
            if let Some(s) = h.item {
                // A gate stays an active record once opened (+0xdc leaves 0).
                let state = g.items.records.get(s).map(|r| (r.i8_(0xde), r.u8_(0xe2)));
                if self.hint_base.is_none_or(|b| b.0 != self.hint_at)
                    && let Some((de, e2)) = state
                {
                    self.hint_base = Some((self.hint_at, de, e2));
                }
                let changed = self.hint_base.zip(state).is_some_and(|(b, st)| (b.1, b.2) != st && st.0 != 0);
                let done = !g.items.records.get(s).is_some_and(|r| r.active())
                    || (item_kind(g, s).0 == 5 && (self.hint_reached.contains(&s) || changed))
                    || (item_kind(g, s).0 == 7 && g.items.records[s].i8_(0xdc) != 0);
                if done {
                    self.log.push(serde_json::json!({"update": f, "hint_done": self.hint_at}));
                    self.hint_at += 1;
                    self.hint_reached.clear();
                    // Each hint counts its own stuck events: K1's rune hint
                    // (11) was skipped 180 updates after hint 10 was done,
                    // the earlier hints' events carried over (lg29 K1 tries).
                    self.hint_stuck = 0;
                    continue;
                }
                let targets = self.item_nodes(g, &[s]);
                let path = targets.first().and_then(|&(_, n)| self.nav.path(here, &|m| m == n, &blocked, if is_exit(s) { &avoid } else { &avoid_x }));
                // Not reachable yet: the ordinary goals (keys open gates,
                // triggers move floors) until the next re-plan.
                let Some(path) = path else { break };
                self.goal = Some(Goal::Item(s));
                // A trigger whose state an earlier touch left set counts only
                // on contact (+0xe2): I3's hint 8 (trigger 533, which raises
                // the rune's platform 566 for a while) counted as reached from
                // 184 nodes away, and the rune stayed down (b65).
                self.revisiting = (item_kind(g, s).0 == 5 && g.items.records[s].i8_(0xde) != 0).then_some(s);
                self.path = path;
                self.at = 0;
                let outline: Vec<V3> = self.path.iter().step_by(8).map(|&n| self.nav.pts[n]).collect();
                self.log.push(serde_json::json!({"update": f, "hint": self.hint_at, "item": s, "nodes": self.path.len(), "outline": outline}));
                return true;
            }
            if let Some(q) = h.goto {
                let r = h.radius.unwrap_or(1.5);
                if dist_xz(p, q) < r && (p[1] - q[1]).abs() < 3.0 {
                    self.log.push(serde_json::json!({"update": f, "hint_done": self.hint_at}));
                    self.hint_at += 1;
                    continue;
                }
                let goal = self.nav.locate_cached(q, &HashSet::new());
                let path = goal.and_then(|n| self.nav.path(here, &|m| m == n, &blocked, &avoid_x));
                let Some(path) = path else { break };
                self.goal = Some(Goal::Point(self.hint_at));
                self.path = path;
                self.at = 0;
                self.log.push(serde_json::json!({"update": f, "hint": self.hint_at, "goto": q, "nodes": self.path.len()}));
                return true;
            }
            if let Some(w) = h.wait {
                if self.hint_wait.is_none() {
                    self.hint_wait = Some(f + w);
                }
                if self.hint_wait.is_some_and(|until| until <= f) {
                    self.hint_wait = None;
                    self.hint_at += 1;
                    continue;
                }
                self.goal = None;
                self.path.clear();
                return true;
            }
            self.hint_at += 1;
        }
        let active = |g: &Game, want: &dyn Fn(i32, i32) -> bool| -> Vec<usize> {
            // +0xe1: items for more players than are in the game (ProcessItems skips them).
            (0..g.items.records.len()).filter(|&s| g.items.records[s].active() && g.items.records[s].u8_(0xe1) == 0 && { let (k, t) = item_kind(g, s); want(k, t) }).collect()
        };
        // At max_keys_per_player a key is not picked up (G4 in the campaign:
        // nine keys carried, 566 stuck events beside keys).
        // One short of it a keyring gives a single key and stays (the pickup
        // takes max − keys and keeps the rest: K2 with eight carried keys
        // circled keyring 350).
        let held = g.players[0].i32_(KEYS);
        let max_keys = g.globals.max_keys_per_player;
        let keys: Vec<usize> = if max_keys <= held {
            Vec::new()
        } else {
            active(g, &|k, t| k == 1 && t == 2)
                .into_iter()
                .filter(|&s| held + 1 < max_keys || g.items.definitions[g.items.records[s].definition()].name() != b"KEYRING")
                .collect()
        };
        // Regular exits first; secret exits (subtype 0x32, to the treasure
        // rooms) only when no regular exit is reachable.
        let exits = active(g, &|k, t| k == 9 && t != 0x32);
        let secret = if self.no_secret { Vec::new() } else { active(g, &|k, t| k == 9 && t == 0x32) };
        // Triggers that switch their bound object on (+0xf4 bit 0 only
        // switches it off) and were not used yet. Bit 0x200: no player can
        // touch it (ItemCollSub skips it; another trigger's chain fires it —
        // A4's BRIDGEPAD 231, G2's 408).
        let touchable = |s: usize| g.items.records[s].u16_(0xf4) & 0x201 == 0;
        let triggers: Vec<usize> = active(g, &|k, _| k == 5)
            .into_iter()
            .filter(|&s| !self.visited_triggers.contains(&s) && touchable(s))
            .collect();
        let mut near: Vec<usize> = keys;
        near.extend(triggers);
        // The Tower (world 13): its crystals (kind 1 subtype 15) unlock the
        // realm doors (UnlockLevel), so they come before its exits.
        if g.globals.cur_world == 0xd {
            let crystals = active(g, &|k, t| k == 1 && t == 15);
            if !crystals.is_empty() {
                near = crystals;
            }
        }
        let tower_crystals = g.globals.cur_world == 0xd && !active(g, &|k, t| k == 1 && t == 15).is_empty();
        // The level's rune stone (kind 1 subtype 10) and the realm's legend
        // item (subtype 0xd) before its exits: the campaign needs every rune
        // (H4 opens with all thirteen) and the bosses their legend item.
        let treasures = active(g, &|k, t| k == 1 && (t == 10 || t == 0xd));
        // While one is left and out of reach, keys and triggers come before
        // the exits: a trigger may bring it within reach (A3's rune lies on
        // the elevator A3ELEV3 that trigger 249 raises 7 units to the floor).
        // Then, still out of reach, triggers already used, nearest the
        // treasure first and at most three contacts each: B5's javelin needs
        // lift 275 ridden again once trigger 279 has raised the bridge.
        // With no treasure left the same holds for the exits, after the
        // exits and fresh triggers (D2: the bot came back down for keys and
        // had to shoot the elevator call 405 again to climb to the exit).
        let mut revisit: Vec<usize> = if tower_crystals {
            Vec::new()
        } else {
            active(g, &|k, _| k == 5)
                .into_iter()
                .filter(|&s| self.visited_triggers.contains(&s) && touchable(s) && self.trigger_visits.get(&s).copied().unwrap_or(0) < 3)
                .collect()
        };
        // Last of all, switch-off triggers (+0xf4 bit 0) the player can touch,
        // two contacts each: combination pads (K1's 67 switches objects 880,
        // 881 and 1968 back off; 40, 41 and 66 alone left the way closed).
        let offs: Vec<usize> = if tower_crystals {
            Vec::new()
        } else {
            active(g, &|k, _| k == 5)
                .into_iter()
                .filter(|&s| g.items.records[s].u16_(0xf4) & 0x201 == 1 && self.trigger_visits.get(&s).copied().unwrap_or(0) < 2)
                .collect()
        };
        revisit.extend(offs);
        let goals_left: Vec<usize> = if treasures.is_empty() { exits.clone() } else { treasures.clone() };
        let to_goal = |s: usize| goals_left.iter().map(|&t| dist3(g.items.records[s].v3(0x40), g.items.records[t].v3(0x40))).fold(f32::MAX, f32::min);
        revisit.sort_by(|&a, &b| to_goal(a).total_cmp(&to_goal(b)));
        let (mut first_revisit, revisits) = (if treasures.is_empty() { 3 } else { 2 }, revisit.len());
        let mut order: Vec<Vec<usize>> = if tower_crystals {
            vec![near]
        } else if treasures.is_empty() {
            let mut o = vec![exits, near, secret];
            o.extend(revisit.into_iter().map(|s| vec![s]));
            o
        } else {
            // Fresh triggers bound to the object a left treasure rests on (or
            // one of its parents) first: A3's rune on
            // the elevator trigger 249 raises, E1's legend item on the
            // pedestal pad 159 moves (object 632), D4's sword on object 735
            // that trigger 355 raises (the bot stood on the ledge beside it
            // 686 times: the graph's drop into the pit fails in play).
            // E1's soul rests on object 636 (no item floor parent, no graph
            // node on its platform): a floor query on a collision copy.
            for &t in &treasures {
                if !self.support_cache.contains_key(&t) {
                    let q = g.items.records[t].v3(0x40);
                    let o = g.items.floor_parent.get(t).copied().flatten().map(|(o, _)| o).or_else(|| {
                        let mut c = g.collision.clone();
                        c.floor_pos(q[1] - 3.0, 0.3, [q[0], q[1] + 1.0, q[2]], 0).ok().and_then(|(_, h)| h).map(|h| h.object)
                    });
                    self.support_cache.insert(t, o);
                }
            }
            let mut support: HashSet<usize> = HashSet::new();
            for &t in &treasures {
                // The object and its ancestors (a pedestal under its lift).
                let mut o = self.support_cache.get(&t).copied().flatten();
                for _ in 0..4 {
                    let Some(x) = o else { break };
                    support.insert(x);
                    o = g.collision.objects.get(x).and_then(|c| c.parent);
                }
            }
            // A treasure out of reach: its island's edge objects too.
            for &t in &treasures {
                if self.islands.contains_key(&t) || !self.item_nodes(g, &[t]).is_empty() {
                    continue;
                }
                let q = g.items.records[t].v3(0x40);
                let mut c = g.collision.clone();
                let nav = Nav::build(&mut c, &[q], g.globals.world_bottom - 2.0, q[1] + 80.0, 0.3);
                let pts: Vec<V3> = (0..nav.pts.len()).filter(|&n| !nav.adj[n].is_empty()).map(|n| nav.pts[n]).collect();
                self.log.push(serde_json::json!({"update": f, "island": t, "nodes": pts.len()}));
                self.islands.insert(t, pts);
            }
            // Islands up to 3000 nodes (H3 695, J6 130, D4 35): from a
            // larger one the treasure reaches most of the level, the start is
            // the cut-off part (G1's rune: 14,451 nodes, every trigger "by the
            // island"; b45 pressed them in turn for 150,000 updates and never
            // went for the rune that b34 took after pad 418's lift).
            let by_island = |o: usize| {
                g.collision.object_matrix(o).ok().map(|m| m[3]).is_some_and(|w| {
                    self.islands.values().filter(|pts| pts.len() <= 3000).any(|pts| pts.iter().any(|p| dist_xz(*p, [w[0], w[1], w[2]]) < 12.0 && (p[1] - w[1]).abs() < 10.0))
                })
            };
            let bound: Vec<usize> = near
                .iter()
                .copied()
                .filter(|&s| {
                    item_kind(g, s).0 == 5
                        && g.items.records[s].u32_(0xf0).checked_sub(1).is_some_and(|o| support.contains(&(o as usize)) || by_island(o as usize))
                })
                .collect();
            // (The revisits start one list later.)
            first_revisit += 1;
            // The support's triggers before the treasure itself: the graph
            // may show the treasure within reach (D4: a drop into the sword's
            // pit) while play needs the pedestal raised first.
            let mut o = vec![bound, treasures, near];
            o.extend(revisit.into_iter().map(|s| vec![s]));
            if !self.require {
                o.push(exits);
                o.push(secret);
            }
            o
        };
        // loot=1: pickups near the player first, nearest by path.
        if self.loot {
            let hurt = g.players[0].f32_(HEALTH) < gdl_core::player_class::max_health(g.players[0].i32_(0x2ac0));
            let potions_left = g.players[0].i32_(0x1a18) < g.globals.max_potions_per_player;
            // Chests (kind 2 subtypes 0x2e..0x30) open on contact with a key
            // (use_key, OpenChest: gold or an item) while three keys are held
            // (two kept for gates); not the time-bomb chest (0x2c).
            let spare_keys = 3 <= g.players[0].i32_(KEYS);
            // A closed chest (+0xd8 & 0x11 == 0x10, +0xdc 0) takes a key; an
            // opened one (+0xdc 2) gives its gold or item on contact.
            let chest = |s: usize| {
                let r = &g.items.records[s];
                (spare_keys && r.u16_(0xd8) & 0x11 == 0x10 && r.u8_(0xdc) == 0) || r.u8_(0xdc) == 2
            };
            // Powerups (kind 1 subtypes 5..=9: weapons, shields and
            // invulnerability, boosts, turbo and the size potions) are always
            // taken (add_powerup) except a size potion (subtype 9, +0xf0 &
            // 0xf000) while one is active (+0x138 & 0xf000), which stays.
            // F1 holds 2 invulnerability icons, 5 shields, rapid fire and
            // multi-shot icons and 7 turbo pickups the bot passed (feas-F1).
            let sized = g.players[0].u32_(0x138) & 0xf000 != 0;
            let powerup = |s: usize| {
                let (k, t) = item_kind(g, s);
                k == 1 && (5..=9).contains(&t) && !(t == 9 && sized && g.items.records[s].u32_(0xf0) & 0xf000 != 0)
            };
            let loot: Vec<usize> = active(g, &|k, t| (k == 1 && (t == 1 || (t == 3 && hurt) || (t == 4 && potions_left) || (5..=9).contains(&t))) || (k == 2 && (0x2e..=0x30).contains(&t)))
                .into_iter()
                .filter(|&s| item_kind(g, s).0 != 1 || !(5..=9).contains(&item_kind(g, s).1) || powerup(s))
                .filter(|&s| item_kind(g, s).0 != 2 || chest(s))
                .filter(|s| !self.optional_skip.contains(s))
                .filter(|&s| {
                    let q = g.items.records[s].v3(0x40);
                    // Food with a negative amount (+0xf4) poisons too.
                    g.items.definitions[g.items.records[s].definition()].name() != b"BADMEAT" && !(item_kind(g, s) == (1, 3) && g.items.records[s].i32_(0xf4) < 0) && dist_xz(q, p) < LOOT_RANGE && (q[1] - p[1]).abs() < 5.0
                })
                .collect();
            if !loot.is_empty() {
                order.insert(0, loot);
                first_revisit += 1;
            }
        }
        // seek_crystals=1: the crystals of unfilled sets, nearest by path, before
        // the other lists (after three stuck events one is skipped like the
        // other optional pickups; a replay of the level makes up for it —
        // PlayerGiveCrystal counts every pickup up to the set's maximum).
        // Only with half the level-start health or more (a detour for a
        // crystal is not worth a death).
        if self.crystals && self.start_health * 0.5 <= g.players[0].f32_(HEALTH) {
            let cls = 0xb4 * g.players[0].u32_(0xc) as usize;
            let unfilled = |n: i32| {
                let c = usize::try_from(n).ok().map(|n| g.players[0].i16_(0xd0a + cls + 2 * n));
                c.is_some_and(|c| 0 <= c && g.items.tables.max_crystals.get(n as usize).is_some_and(|&m| i32::from(c) < m))
            };
            let want: Vec<usize> = active(g, &|k, t| k == 1 && t == 0xf)
                .into_iter()
                .filter(|&s| unfilled(g.items.records[s].i32_(0xf4)) && !self.optional_skip.contains(&s))
                .collect();
            if !want.is_empty() {
                order.insert(0, want);
                first_revisit += 1;
            }
        }
        // Hungry (under 60 % of the start health): food first (kind 1
        // subtype 3; BADMEAT poisons).
        if g.players[0].f32_(HEALTH) < self.start_health * 0.6 {
            let food = active(g, &|k, t| k == 1 && t == 3)
                .into_iter()
                .filter(|&s| g.items.definitions[g.items.records[s].definition()].name() != b"BADMEAT" && g.items.records[s].i32_(0xf4) >= 0)
                .filter(|s| !self.optional_skip.contains(s))
                .collect::<Vec<_>>();
            if !food.is_empty() {
                order.insert(0, food);
                first_revisit += 1;
            }
        }
        self.revisiting = None;
        // Strike nodes only after every ordinary list found no path: C1's
        // trigger 342, given a strike node (c47), led the bot into the water
        // pit by (−20, −12, 52), which it never left (c44 ended C1 at update
        // 134,752 without going for 342).
        let mut passes: Vec<(usize, Vec<usize>, bool)> = order.iter().cloned().enumerate().map(|(li, l)| (li, l, false)).collect();
        passes.extend(order.into_iter().enumerate().map(|(li, l)| (li, l, true)));
        for (li, list, strike) in passes {
            let list: Vec<usize> = list.into_iter().filter(|s| !self.given_up.contains(s)).collect();
            let targets = if strike {
                // The strike pass only for items that gained a strike node.
                let plain: HashSet<usize> = item_nodes_ex(&self.nav, g, &list, false).iter().map(|&(s, _)| s).collect();
                item_nodes_ex(&self.nav, g, &list, true).into_iter().filter(|(s, _)| !plain.contains(s)).collect()
            } else {
                item_nodes_ex(&self.nav, g, &list, false)
            };
            if targets.is_empty() {
                continue;
            }
            let goal_nodes: HashMap<usize, usize> = targets.iter().map(|&(s, n)| (n, s)).collect();
            let av = if list.iter().all(|&s| is_exit(s)) { &avoid } else { &avoid_x };
            if let Some(path) = self.nav.path(here, &|n| goal_nodes.contains_key(&n), &blocked, av) {
                let slot = goal_nodes[path.last().unwrap()];
                if (first_revisit..first_revisit + revisits).contains(&li) {
                    self.revisiting = Some(slot);
                }
                self.goal = Some(Goal::Item(slot));
                self.path = path;
                self.at = 0;
                self.log.push(serde_json::json!({"update": f, "goal": slot, "kind": item_kind(g, slot), "nodes": self.path.len(), "from": p}));
                return true;
            }
        }
        // A boss or other critter alive: go to it (the attack logic fights it).
        // Only on boss levels: elsewhere the critters are statues' creatures
        // (gargoyles, golems, generals), fought when met, not sought.
        let foes: Vec<V3> = if g.globals.boss_type < 0 { Vec::new() } else { g.critters.iter().flatten().filter(|c| 0.0 < c.rec.f32_(0x4f4)).map(|c| c.rec.v3(0x40)).collect() };
        if !foes.is_empty() {
            // Within 4 units of a foe, else the reachable node nearest to one.
            let close = (0..self.nav.pts.len())
                .filter(|&n| !self.nav.adj[n].is_empty())
                .min_by(|&a, &b| {
                    let d = |n: usize| foes.iter().map(|q| dist_xz(self.nav.pts[n], *q)).fold(f32::MAX, f32::min);
                    d(a).total_cmp(&d(b))
                });
            // Or any node within 3 units of the closest node's distance: E2's
            // Skorne stands in a pit 15 units under the floor, and after a
            // knockback and stuck events by the one closest node (9.75, −10,
            // 0.75) no path reached it again; the bot explored for 4,000
            // updates with Skorne at 2,171 of 6,000 (feas-E2).
            let foe_d = |n: usize| foes.iter().map(|q| dist_xz(self.nav.pts[n], *q)).fold(f32::MAX, f32::min);
            let close_d = close.map(foe_d).unwrap_or(f32::MAX);
            let near = |n: usize| foes.iter().any(|q| dist_xz(self.nav.pts[n], *q) < 4.0) || Some(n) == close || foe_d(n) < close_d + 3.0;
            if let Some(path) = self.nav.path(here, &near, &blocked, &avoid_x) {
                let n = *path.last().unwrap();
                self.goal = Some(Goal::Explore(n));
                self.path = path;
                self.at = 0;
                // The range to the boss itself (BossCritInst), not the nearest
                // of its children: A5's chimera body stood 18.1 from the node
                // nearest a head, so the boss branch never fought it (lg23).
                let boss = usize::try_from(g.boss_crit_inst - 1).ok().and_then(|b| g.critters.get(b)).and_then(Option::as_ref).map(|c| c.rec.v3(0x40));
                let d = match boss {
                    Some(b) => dist_xz(self.nav.pts[n], b),
                    None => foes.iter().map(|q| dist_xz(self.nav.pts[n], *q)).fold(f32::MAX, f32::min),
                };
                self.boss_range = (d + 4.0).max(18.0);
                self.log.push(serde_json::json!({"update": f, "foe": self.nav.pts[n], "nodes": self.path.len()}));
                return true;
            }
        }
        // Nothing to collect or switch: explore the nearest column not stood in.
        let unvisited = |n: usize| {
            let q = self.nav.pts[n];
            !self.nav.adj[n].is_empty() && !self.visited_cols.contains(&key(q[0], q[2])) && 3.0 < dist_xz(q, p)
        };
        if let Some(path) = self.nav.path(here, &unvisited, &blocked, &avoid_x) {
            let n = *path.last().unwrap();
            self.goal = Some(Goal::Explore(n));
            self.path = path;
            self.at = 0;
            self.log.push(serde_json::json!({"update": f, "explore": self.nav.pts[n], "nodes": self.path.len()}));
            return true;
        }
        false
    }

    /// Port 0 for update `f`; also applies the health assist.
    pub fn step(&mut self, f: usize, g: &mut Game) -> PadStatus {
        // Not along a goto route hint (Goal::Point): a hand-made route is
        // walked as written — H3's no-assist tries, turned off its waypoints
        // by the statues and hazards beside them, skipped every goto hint
        // after 60 stuck events each (h3n: one hint per ~10,000 updates;
        // the assisted win did them 15–25 updates apart).
        AVOID_CONTACT.store(!self.assist_enabled && !matches!(self.goal, Some(Goal::Point(_))), std::sync::atomic::Ordering::Relaxed);
        FOE_SKIP.with(|s| s.borrow_mut().1 = f);
        let mut pad = PadStatus { connected: true, analog: true, has_stick: true, ..Default::default() };
        for e in 4..8 {
            pad.entries[e] = [3, 0];
        }
        if self.exit_update.is_none() && (g.level_done.is_some() || (g.players_done && g.exit_ready)) {
            // The level ended: do_exit's destination wave (an exit portal, a
            // treasure room's timer), or game_main's exit test after a boss's
            // auto exit (do_players done, AudioStillPlaying clear → TowerWave).
            self.exit_update = Some(f);
            self.exit_wave = g.level_done.or(Some(gdl_core::game::boss::TOWER_WAVE));
            // 5 exited by a portal, 1 still in play at a boss level's auto
            // exit; dying (8) or continue (0xb) means the level ended by a
            // death (PlayersSetWaveDied → TowerWave).
            self.exit_state = Some(g.players[0].i32_(0xfc));
        }
        if self.exit_update.is_some() || g.players[0].i32_(0xfc) != 1 {
            return pad;
        }
        self.sample_movers(f, g);
        let health = g.players[0].f32_(HEALTH);
        // Health lost since the last update (diagnostics, report `hurt`):
        // the foes within 30 units (type, distance, height, health) and
        // hazards within 5, for losses of 3 or more.
        if health + 3.0 <= self.last_health {
            self.hurt_at = Some(f);
            let p = g.players[0].v3(0x50);
            let mut foes: Vec<(f32, i32, f32, f32)> = g
                .enemies
                .iter()
                .filter(|e| e.i32_(0xc8) != 0 && dist_xz(e.v3(0x40), p) < 30.0)
                .map(|e| (dist_xz(e.v3(0x40), p), e.i32_(0), e.v3(0x40)[1] - p[1], e.f32_(0x214)))
                .collect();
            // Critters (bosses, statues' creatures) as type −1, any distance
            // within 40.
            foes.extend(g.critters.iter().flatten().filter(|c| dist_xz(c.rec.v3(0x40), p) < 40.0).map(|c| (dist_xz(c.rec.v3(0x40), p), -1, c.rec.v3(0x40)[1] - p[1], c.rec.f32_(0x4f4))));
            foes.sort_by(|a, b| a.0.total_cmp(&b.0));
            foes.truncate(3);
            let hazards: Vec<usize> = (0..g.items.records.len()).filter(|&s| g.items.records[s].active() && item_kind(g, s).0 == 8 && dist_xz(g.items.records[s].v3(0x40), p) < 5.0).collect();
            let r1 = |v: f32| (v * 10.0).round() / 10.0;
            self.log.push(serde_json::json!({"update": f, "hurt": r1(self.last_health - health), "at": p.map(r1), "foes": foes.iter().map(|x| [r1(x.0), x.1 as f32, r1(x.2), r1(x.3)]).collect::<Vec<_>>(), "hazards": hazards}));
        }
        self.last_health = health;
        // Danger: without the assist, 8 % of the level-start health lost in
        // the last 300 updates within 6 units of where the player stood —
        // an optional goal there is dropped (logged `danger_skip`). C1's
        // try held crystal 389 in the GRU nest at (−36, −12, 23) for 1,750
        // updates, fighting in place, and lost 1,400 (c1j-d0).
        if !self.assist_enabled && f.is_multiple_of(60) {
            let p = g.players[0].v3(0x50);
            self.trail_hp.push_back((f, health, p));
            while self.trail_hp.front().is_some_and(|&(u, _, _)| u + 300 < f) {
                self.trail_hp.pop_front();
            }
            let danger = self.trail_hp.front().is_some_and(|&(u, h0, p0)| u + 240 <= f && self.start_health * 0.08 < h0 - health && dist_xz(p0, p) < 6.0);
            if danger && self.nest_until <= f {
                // Nest mode: the spawns' generators are the target (C1's
                // tries stood firing at type-7 spawns of GRU generators 148
                // and 234, the generator branch held off by the foes within
                // 4 units, c1k-d0).
                self.nest_until = f + 600;
                self.log.push(serde_json::json!({"update": f, "nest": p}));
            }
            if let (true, Some(&(_, h0, _)), Some(Goal::Item(s))) = (danger, self.trail_hp.front(), self.goal)
                && matches!(item_kind(g, s), (1, 1) | (1, 3) | (1, 4) | (1, 5..=9) | (1, 0xf) | (2, 0x2e..=0x30))
                && self.optional_skip.insert(s)
            {
                // And every optional item within 10 units (the nest's other
                // crystals and loot).
                let near: Vec<usize> = (0..g.items.records.len())
                    .filter(|&t| g.items.records[t].active() && dist_xz(g.items.records[t].v3(0x40), p) < 10.0 && matches!(item_kind(g, t), (1, 1) | (1, 3) | (1, 4) | (1, 5..=9) | (1, 0xf) | (2, 0x2e..=0x30)))
                    .collect();
                self.optional_skip.extend(near.iter().copied());
                self.log.push(serde_json::json!({"update": f, "danger_skip": s, "lost": h0 - health, "near": near}));
                self.trail_hp.clear();
                self.choose(f, g);
            }
        }
        // An optional pickup the player stands beside (1.5 units) for 300
        // updates without taking it is skipped and the goal chosen again:
        // C1's try stood 0.5 units from BOOSTICON 78 from update 21,163 to
        // 36,913 (c1h-d263).
        if let Some(Goal::Item(s)) = self.goal {
            let optional = matches!(item_kind(g, s), (1, 1) | (1, 3) | (1, 4) | (1, 5..=9) | (1, 0xf) | (2, 0x2e..=0x30));
            let q = g.items.records[s].v3(0x40);
            let p = g.players[0].v3(0x50);
            if optional && g.items.records[s].active() && dist_xz(q, p) < 1.5 && (q[1] - p[1]).abs() < 3.0 {
                match self.beside {
                    Some((b, since)) if b == s => {
                        if since + 300 <= f && self.optional_skip.insert(s) {
                            self.log.push(serde_json::json!({"update": f, "skip_unpicked": s}));
                            self.beside = None;
                            self.choose(f, g);
                        }
                    }
                    _ => self.beside = Some((s, f)),
                }
            } else {
                self.beside = None;
            }
        }
        // Without the assist, inside a poison cloud: walk straight out of it
        // (away from its centre) and re-plan with its nodes closed.
        if !self.assist_enabled && f.is_multiple_of(10) && self.backoff.is_none() {
            let p = g.players[0].v3(0x50);
            let cloud = g.effects.nodes.iter().zip(&g.effects.slots).filter_map(|(n, r)| {
                let n = n.as_ref()?;
                let blast = r.f32_(gdl_core::game::effects::fx::BLAST);
                (r.u32_(gdl_core::game::effects::fx::DAMAGE_FLAGS) & 0x800 != 0 && 0.0 < r.f32_(gdl_core::game::effects::fx::DAMAGE) && dist_xz(n.position, p) < blast + 0.5 && (n.position[1] - p[1]).abs() < 4.0).then_some(n.position)
            }).next();
            if let Some(q) = cloud {
                let d = dist_xz(p, q).max(0.1);
                let away = [p[0] + (p[0] - q[0]) / d * 8.0, p[1], p[2] + (p[2] - q[2]) / d * 8.0];
                self.backoff = Some((f + 30, away));
                self.force_rebuild = false;
                self.log.push(serde_json::json!({"update": f, "poison_cloud": q}));
                self.choose(f, g);
            }
        }
        if self.assist_enabled && health < self.start_health * 0.4 {
            g.players[0].set_f32(HEALTH, self.start_health);
            self.assists += 1;
        }
        // The stage-start sequence and camera waits hold the player: no
        // input and no stuck test until they end.
        if g.globals.start_of_stage != 0 || g.globals.cam_wait != 0 {
            self.progress = (f, g.players[0].v3(0x50));
            self.holds[0] += 1;
            return pad;
        }
        if 0 < self.delay {
            self.delay -= 1;
            let p = g.players[0].v3(0x50);
            self.progress = (f, p);
            // Standing still, not defenceless: a foe within reach is fought
            // in place (no-assist G2, lg4 try 6: health 600 → 477 while it
            // waited out its 541-update delay at the start).
            if !self.assist_enabled
                && let Some((_, at)) = Self::nearest_foe(g, p, ATTACK_RANGE)
            {
                pad.press(0xd);
                self.tag(f, "delay");
                return Self::steer(pad, g, p, at);
            }
            return pad;
        }
        // On an exit portal (contact item +0x884, kind 9): DoExit counts only
        // while the stick is released, so stand still.
        if let Some(it) = g.players[0].u32_(0x884).checked_sub(1).map(|i| i as usize)
            && it < g.items.records.len()
            && item_kind(g, it).0 == 9
        {
            self.progress = (f, g.players[0].v3(0x50));
            self.holds[1] += 1;
            return pad;
        }
        // Goal bookkeeping.
        match self.goal {
            Some(Goal::Item(s)) => {
                let (k, _) = item_kind(g, s);
                // A trigger counts once the game shows the contact: its state
                // (+0xde) or a player's touch bits (+0xe2); near it without
                // contact the bot steers onto the item's own position.
                let r = &g.items.records[s];
                // A shot switch is never touched: its state counts.
                let shot = item_kind(g, s) == (5, 0x1f);
                let reached = !r.active() || (k == 5 && (r.u8_(0xe2) != 0 || ((shot || self.revisiting != Some(s)) && r.i8_(0xde) != 0)));
                if reached {
                    self.hint_reached.insert(s);
                    self.log.push(serde_json::json!({"update": f, "reached": s, "keys": g.players[0].i32_(KEYS)}));
                    self.bad_nodes.clear();
                    if k == 5 {
                        // Stay on the pad while its platform moves (lifts,
                        // bridges, rafts): until the player has been still
                        // for 10 updates after moving, 90 updates without
                        // moving, or 600 updates.
                        self.visited_triggers.insert(s);
                        *self.trigger_visits.entry(s).or_insert(0) += 1;
                        self.revisiting = None;
                        // A trigger may bring a given-up rune or legend item
                        // within reach (B5's javelin: trigger 279 raises
                        // B5ELEV1 5 units to bridge the ditch before it).
                        // Any other given-up goal too, with half the stuck
                        // allowance: E1's doors open for a few seconds after
                        // its pads, and goals given up behind them closed
                        // stay out of reach until pressed again.
                        let reopened: Vec<usize> = self.given_up.iter().copied().collect();
                        for t in reopened {
                            self.given_up.remove(&t);
                            if matches!(item_kind(g, t), (1, 10) | (1, 0xd)) {
                                self.stuck_goal_counts.remove(&t);
                            } else {
                                self.stuck_goal_counts.insert(t, 20);
                            }
                            self.log.push(serde_json::json!({"update": f, "reopened": t, "after_trigger": s}));
                        }
                        // The trigger's object moves: edges learnt as blocked
                        // within 30 units of it or of the trigger were guesses
                        // about the old geometry (D4 b47: pad 55 raises the
                        // pit floor 333 beside trigger 355; the edges into the
                        // pit, blocked while it was down, kept 355 — the
                        // sword's pedestal — out of reach for 130,000 updates).
                        let mut at = vec![r.v3(0x40)];
                        if let Some(o) = r.u32_(0xf0).checked_sub(1)
                            && let Ok(m) = g.collision.object_matrix(o as usize)
                        {
                            at.push([m[3][0], m[3][1], m[3][2]]);
                        }
                        let near = |pl: &Place| {
                            let (x, z) = centre((pl.0, pl.2));
                            at.iter().any(|q| dist_xz([x, 0.0, z], *q) < 30.0)
                        };
                        let before = self.blocked.len();
                        self.blocked.retain(|(a, b)| !(near(a) || near(b)));
                        if self.blocked.len() < before {
                            self.log.push(serde_json::json!({"update": f, "unblocked": before - self.blocked.len(), "after_trigger": s}));
                        }
                        self.wait = Some((f, g.players[0].v3(0x50), f));
                        self.goal = None;
                        self.path.clear();
                    } else {
                        self.choose(f, g);
                    }
                }
            }
            Some(Goal::Point(h)) => {
                let q = self.hints.get(h).and_then(|x| x.goto).unwrap_or(self.start);
                let r = self.hints.get(h).and_then(|x| x.radius).unwrap_or(1.5);
                let pp = g.players[0].v3(0x50);
                if dist_xz(pp, q) < r && (pp[1] - q[1]).abs() < 3.0 {
                    self.choose(f, g);
                }
            }
            Some(Goal::Explore(n)) => {
                let q = self.nav.pts.get(n).copied().unwrap_or(self.start);
                if dist_xz(g.players[0].v3(0x50), q) < 1.5 || self.at >= self.path.len() {
                    self.visited_cols.insert(key(q[0], q[2]));
                    self.choose(f, g);
                }
            }
            None => {
                if f.is_multiple_of(60) && self.wait.is_none() {
                    self.choose(f, g);
                }
            }
        }
        if self.goal.is_some() {
            self.no_goal_since = None;
        }
        let p = g.players[0].v3(0x50);
        self.visited_cols.insert(key(p[0], p[2]));
        if f.is_multiple_of(10) && self.route.last().is_none_or(|(_, q)| 2.0 < dist_xz(*q, p) || 1.5 < (q[1] - p[1]).abs()) {
            self.route.push((f, p));
        }
        if f.is_multiple_of(10) && self.trail.last().is_none_or(|q| 1.0 < dist_xz(*q, p) || 1.0 < (q[1] - p[1]).abs()) {
            self.trail.push(p);
        }
        if let Some((since, p0, moved)) = self.wait {
            // Carried in any direction (I5's raft, object 111, travels
            // level to the arena).
            let moved = if 0.05 < dist3(p, p0) { f } else { moved };
            // Co-op: pads may need every player on them (+0xf4 0x400) and
            // lifts carry only those aboard — wait for the followers.
            let apart = (1..4).any(|k| g.globals.active_players >> k & 1 != 0 && g.players[k].i32_(0xfc) == 1 && 3.0 < dist_xz(g.players[k].v3(0x50), p));
            // Carried: off as soon as the platform has stopped for 10
            // updates (J6's lift 694 stays at the rune room's level for
            // about 30 updates; waiting 30 left the bot aboard going down).
            let done = (!apart && if moved == since { 90 <= f - since } else { 10 <= f - moved }) || 600 <= f - since;
            if done {
                self.wait = None;
                self.force_rebuild = true;
                if moved != since {
                    self.disembark_until = f + 120;
                }
                self.log.push(serde_json::json!({"update": f, "waited": f - since, "rose": p[1] - self.start[1]}));
                self.progress = (f, p);
                self.choose(f, g);
            } else {
                self.wait = Some((since, p, moved));
                // A foe within reach: fight it rather than stand still (no-
                // assist K2: the General statue's creature beat the player
                // down on lift pad 483 while the bot waited for the lift).
                if Self::nearest_foe(g, p, ATTACK_RANGE).is_none() {
                    return pad;
                }
            }
        }
        // Stuck detection: under half a unit in STUCK updates, or no goal
        // reached while within DWELL units for 4 × STUCK (an oscillation
        // between two points, A2 b13).
        if DWELL < dist_xz(p, self.dwell.1) {
            self.dwell = (f, p);
        }
        if STUCK <= f - self.progress.0 {
            let stuck = dist_xz(p, self.progress.1) < 0.5 || 4 * STUCK <= f - self.dwell.0;
            // Without the assist, foes in the way are fought where the bot
            // stands, not routed around (lg7's G2 tries took about two thirds
            // of their damage in stretches with stuck events, re-planning
            // with knights at their backs): up to FIGHT_HOLD updates, then
            // the ordinary stuck handling.
            let fight = stuck && !self.assist_enabled && self.fight_hold < FIGHT_HOLD && Self::nearest_foe(g, p, 2.0 * ATTACK_RANGE).is_some();
            if fight {
                self.fight_hold += f - self.progress.0;
                self.dwell = (f, p);
            } else if !stuck {
                self.fight_hold = 0;
            }
            if stuck && !fight && !self.assist_enabled {
                // A dormant statue within contact + 1.5 units at three stuck
                // events: keep_off lets the bot touch it (lg21's A4: tries
                // held for 150,000 updates beside the GENERAL statue 54, the
                // way to goal 262 passing within its contact).
                let rad = player_radius();
                for s2 in 0..g.items.records.len() {
                    let r = &g.items.records[s2];
                    if r.active() && item_kind(g, s2).0 == 4 && r.u32_(0xf8) & 1 == 0 {
                        let c = rad + g.items.definitions[r.definition()].f32_(0xc).clamp(0.0, 4.0) + 0.4;
                        if dist_xz(r.v3(0x40), p) < c + 1.5 && (r.v3(0x40)[1] - p[1]).abs() < 3.0 {
                            let n = self.statue_stuck.entry(s2).or_insert(0);
                            *n += 1;
                            if *n == 3 {
                                CONTACT_OK.with(|c| c.borrow_mut().insert(s2));
                                self.log.push(serde_json::json!({"update": f, "contact_ok": s2}));
                            }
                        }
                    }
                }
            }
            if stuck && !fight {
                // At a closed gate the key count decides (obstacle_nodes), not
                // a blocked edge that would outlast the next key.
                let at_gate = (0..g.items.records.len())
                    .any(|s| g.items.records[s].active() && item_kind(g, s).0 == 7 && dist_xz(g.items.records[s].v3(0x40), p) < 4.0 && (g.items.records[s].v3(0x40)[1] - p[1]).abs() < 3.0);
                if at_gate
                    && g.players[0].i32_(KEYS) < 1
                    && let (Some(&a), Some(&b)) = (self.path.get(self.at.saturating_sub(1)), self.path.get(self.at))
                {
                    self.gate_blocked.insert((self.nav.place(a), self.nav.place(b)));
                }
                // With a key: walk at the gate's centre for a while. A path
                // node beside its centre leaves the player against the door
                // frame short of the item's contact (D2's gate 116: 192 stuck
                // events with seven keys; approached head-on it opens).
                if at_gate && 0 < g.players[0].i32_(KEYS) {
                    let gate = (0..g.items.records.len())
                        .filter(|&s| g.items.records[s].active() && item_kind(g, s).0 == 7 && g.items.records[s].i8_(0xdc) == 0)
                        .map(|s| g.items.records[s].v3(0x40))
                        .min_by(|a, b| dist_xz(*a, p).total_cmp(&dist_xz(*b, p)));
                    if let Some(q) = gate.filter(|q| dist_xz(*q, p) < 6.0) {
                        self.gate_push = Some((f + 60, q));
                    }
                }
                // A container, generator or obstacle item in the way (chests,
                // generators, barrels and walls: kinds 2, 3 and 10): face it
                // while attacking (K3 b23: goals given up beside generators).
                // It goes when destroyed (C1's corridor generator 231, radius
                // 2, stopped the bot at its edge): attack longer and keep the
                // edge open instead of routing away.
                // Within the item's own radius (+0xe8, at most 6) + 2 and 3
                // units of height: K2's barrels 7 and 58 have radius 6 and
                // stopped the player 5 units off (2999 stuck events with the
                // 3.5 limit); its generator 240, 45 units below, has 40.
                let blocker = (0..g.items.records.len())
                    .filter(|&s| {
                        let r = &g.items.records[s];
                        r.active() && matches!(item_kind(g, s).0, 2 | 3 | 10) && dist_xz(r.v3(0x40), p) < (r.f32_(0xe8).min(6.0) + 2.0).max(3.5) && in_height(g, s, r.v3(0x40)[1], p[1])
                    })
                    .min_by(|&a, &b| dist_xz(g.items.records[a].v3(0x40), p).total_cmp(&dist_xz(g.items.records[b].v3(0x40), p)));
                self.attack_at = blocker.map(|s| g.items.records[s].v3(0x40));
                // At most three long attacks per item: one that does not
                // break (statues, rocks, a respawning generator) is then
                // routed around as before (b27: A3's 1285 stuck events).
                // Three long attacks per item. (+0xfc is no damage measure:
                // K2's generator 240 cycles it 0 → 1 as a spawn timer.)
                // Counted per visit: 3000 updates after the last try the
                // count starts again (J6 b44: barrel 567, health 5, spent
                // on passes from the floor above, then blocked the trench
                // for 255,000 updates; from a teleport it breaks at once).
                let breakable = blocker.is_some_and(|s| {
                    let (n, last) = self.break_tries.entry(s).or_insert((0, 0.0));
                    if 3000.0 < f as f32 - *last {
                        *n = 0;
                    }
                    *n += 1;
                    *last = f as f32;
                    *n <= 3
                });
                // A step up (over a unit) is run at again before it is
                // blocked: three times, backing off 15 updates first. D3's
                // bridge pieces (70.2 → 71.9 → 73.4 → 74.7) are climbed by a
                // moving player (d3dir2-6: one diagonal hold from 71.9 to
                // 76.3) and refused from a standstill (the bot's 162 stuck
                // events at 71.9 → 73.4).
                let run_up = match (self.path.get(self.at.saturating_sub(1)), self.path.get(self.at)) {
                    (Some(&a), Some(&b)) if 1.0 < self.nav.pts[b][1] - self.nav.pts[a][1] => {
                        let n = self.run_ups.entry((self.nav.place(a), self.nav.place(b))).or_insert(0);
                        *n += 1;
                        *n <= 3
                    }
                    _ => false,
                };
                if run_up && let Some(&b) = self.path.get(self.at) {
                    let q = self.nav.live(g, b);
                    let d = dist_xz(p, q).max(0.1);
                    self.backoff = Some((f + 15, [p[0] + (p[0] - q[0]) / d * 3.0, p[1], p[2] + (p[2] - q[2]) / d * 3.0]));
                    self.log.push(serde_json::json!({"update": f, "run_up": q}));
                }
                if !at_gate
                    && !breakable
                    && !run_up
                    && let (Some(&a), Some(&b)) = (self.path.get(self.at.saturating_sub(1)), self.path.get(self.at))
                {
                    let (pa, pb) = (self.nav.place(a), self.nav.place(b));
                    self.blocked.insert((pa, pb));
                    self.blocked.insert((pb, pa));
                    // The same obstacle along a ledge or wall: every penalised
                    // edge within 3 units heading the same way.
                    let (qa, qb) = (self.nav.pts[a], self.nav.pts[b]);
                    let dir = [qb[0] - qa[0], qb[2] - qa[2]];
                    let near: Vec<(usize, usize)> = self.nav.hard.iter().chain(self.nav.ride.iter()).copied().filter(|&(n, m)| {
                        let (u, v) = (self.nav.pts[n], self.nav.pts[m]);
                        dist_xz(u, p) < 3.0 && (u[1] - p[1]).abs() < 1.5 && 0.5 < ((v[0] - u[0]) * dir[0] + (v[2] - u[2]) * dir[1]) / (dist_xz(u, v) * (dir[0].hypot(dir[1])).max(1e-3))
                    }).collect();
                    for (n, m) in near {
                        let (pn, pm) = (self.nav.place(n), self.nav.place(m));
                        self.blocked.insert((pn, pm));
                    }
                    // A drop the player could not take (a railing or an edge
                    // wall the graph's sweep misses, D1's walkway 1114 above
                    // 1108): every drop edge within 3 units heading the same
                    // way, not only the one in use (b10: 1474 stuck events).
                    if qb[1] < qa[1] - STEP_UP {
                        let drops: Vec<(usize, usize)> = (0..self.nav.pts.len())
                            .filter(|&n| dist_xz(self.nav.pts[n], p) < 3.0 && (self.nav.pts[n][1] - p[1]).abs() < 1.5)
                            .flat_map(|n| self.nav.adj[n].iter().map(move |&m| (n, m)))
                            .filter(|&(n, m)| {
                                let (u, v) = (self.nav.pts[n], self.nav.pts[m]);
                                v[1] < u[1] - STEP_UP && 0.5 < ((v[0] - u[0]) * dir[0] + (v[2] - u[2]) * dir[1]) / (dist_xz(u, v) * (dir[0].hypot(dir[1])).max(1e-3))
                            })
                            .collect();
                        for (n, m) in drops {
                            let (pn, pm) = (self.nav.place(n), self.nav.place(m));
                            self.blocked.insert((pn, pm));
                        }
                        // Off the same object, any way, within 12 units: a
                        // railed ramp or walkway (G2's ramp object over the
                        // ground: one drop edge per column, each tried in turn).
                        let from = self.nav.obj[a];
                        let off: Vec<(usize, usize)> = (0..self.nav.pts.len())
                            .filter(|&n| self.nav.obj[n] == from && dist_xz(self.nav.pts[n], p) < 12.0)
                            .flat_map(|n| self.nav.adj[n].iter().map(move |&m| (n, m)))
                            .filter(|&(n, m)| self.nav.pts[m][1] < self.nav.pts[n][1] - STEP_UP && self.nav.obj[m] != from)
                            .collect();
                        for (n, m) in off {
                            let (pn, pm) = (self.nav.place(n), self.nav.place(m));
                            self.blocked.insert((pn, pm));
                        }
                    }
                    // A climb (over a unit) not taken after its run-ups:
                    // every climb from the same object onto the same object
                    // within 12 units. C2's west pit floor (object 1181 at
                    // 5.7) has 195 edges onto its rim (objects 1196 and 1201
                    // at 6.6–8.7) that the game's displacement does not walk;
                    // with one blocked a stuck event lg32's tries (c92) spent
                    // 10,000 updates there.
                    if qa[1] + 1.0 < qb[1] {
                        let (from, to) = (self.nav.obj[a], self.nav.obj[b]);
                        let climbs: Vec<(usize, usize)> = (0..self.nav.pts.len())
                            .filter(|&n| self.nav.obj[n] == from && dist_xz(self.nav.pts[n], p) < 12.0)
                            .flat_map(|n| self.nav.adj[n].iter().map(move |&m| (n, m)))
                            .filter(|&(n, m)| self.nav.obj[m] == to && self.nav.pts[n][1] + 1.0 < self.nav.pts[m][1])
                            .collect();
                        for (n, m) in climbs {
                            let (pn, pm) = (self.nav.place(n), self.nav.place(m));
                            self.blocked.insert((pn, pm));
                        }
                    }
                }
                self.attack_until = f + if breakable { 150 } else { 40 };
                // Back off briefly (away from the blocked waypoint) before the new route.
                if let (Some(&b), false, false) = (self.path.get(self.at), breakable, run_up) {
                    let q = self.nav.pts[b];
                    self.backoff = Some((f + 10, [2.0 * p[0] - q[0], p[1], 2.0 * p[2] - q[2]]));
                }
                // No step taken from the located node: the node itself is out
                // of reach from here (an item between); locate elsewhere.
                if self.at == 0
                    && let Some(&n) = self.path.first()
                {
                    self.bad_nodes.insert(self.nav.place(n));
                }
                if let Some(Goal::Item(s)) = self.goal {
                    let n = self.stuck_goal_counts.entry(s).or_insert(0);
                    *n += 1;
                    if 40 < *n {
                        self.given_up.insert(s);
                    }
                    let optional = matches!(item_kind(g, s), (1, 1) | (1, 3) | (1, 4) | (1, 5..=9) | (1, 0xf) | (2, 0x2e..=0x30));
                    // A crystal after one stuck event (replays make up for
                    // it): C1's tries were stuck three times at each of ten
                    // crystals in its pits, among GRU generators (c1g).
                    let limit = if item_kind(g, s) == (1, 0xf) { 1 } else { 3 };
                    if optional && limit <= *n && self.optional_skip.insert(s) {
                        self.log.push(serde_json::json!({"update": f, "skip_optional": s}));
                    }
                }
                // The same waypoint stuck at five times is closed to routes
                // (K2 in the campaign: 2727 stuck events at one waypoint
                // across a thin wall, each re-plan finding another edge to it).
                if let Some(&w) = self.path.get(self.at) {
                    let pl = self.nav.place(w);
                    let n = self.stuck_waypoints.entry(pl).or_insert(0);
                    *n += 1;
                    if 5 <= *n && self.dead_nodes.insert(pl) {
                        self.log.push(serde_json::json!({"update": f, "dead_node": self.nav.pts[w]}));
                    }
                }
                // An exploration target the bot is stuck short of counts as
                // explored, and so does the waypoint it was stuck at (K1:
                // 214 stuck events at one chest, the same column chosen
                // again each time).
                if let Some(Goal::Explore(n)) = self.goal {
                    let q = self.nav.pts[n];
                    self.visited_cols.insert(key(q[0], q[2]));
                    if let Some(&w) = self.path.get(self.at) {
                        let q = self.nav.pts[w];
                        self.visited_cols.insert(key(q[0], q[2]));
                    }
                }
                let enemies: Vec<_> = g.enemies.iter().enumerate().filter(|(_, e)| e.i32_(0xc8) != 0 && dist_xz(e.v3(0x40), p) < 4.0).map(|(k, e)| serde_json::json!([k, e.i32_(0), e.f32_(0x214)])).collect();
                let items: Vec<_> = (0..g.items.records.len()).filter(|&s| g.items.records[s].active() && dist_xz(g.items.records[s].v3(0x40), p) < 4.0).map(|s| serde_json::json!([s, item_kind(g, s), String::from_utf8_lossy(g.items.definitions[g.items.records[s].definition()].name())])).collect();
                let next = self.path.get(self.at).map(|&n| self.nav.pts[n]);
                self.log.push(serde_json::json!({"update": f, "stuck": p, "next": next, "enemies": enemies, "items": items, "keys": g.players[0].i32_(KEYS), "stuck_goal": match self.goal { Some(Goal::Item(s)) => s, _ => usize::MAX }}));
                if self.hint_at < self.hints.len() {
                    self.hint_stuck += 1;
                    // Sixty stuck events (K1: twenty went by on the way to
                    // pad 40, skipped two units short of it).
                    if 60 <= self.hint_stuck {
                        self.log.push(serde_json::json!({"update": f, "hint_skipped": self.hint_at}));
                        self.hint_at += 1;
                        self.hint_stuck = 0;
                    }
                }
                self.choose(f, g);
            }
            self.progress = (f, p);
        }
        // Riding a lift, raft or falling: no stick until the floor settles.
        // The floor object (+0x89c) moving: its node translation changed.
        let floor = g.players[0].u32_(0x89c).checked_sub(1).map(|o| o as usize);
        let fy = floor.and_then(|o| g.collision.object_matrix(o).ok()).map_or([f32::NAN; 3], |m| [m[3][0], m[3][1], m[3][2]]);
        let riding = floor == self.last_floor && 0.01 < dist3(fy, self.last_y);
        self.last_y = fy;
        self.last_floor = floor;
        // A ride holds input once the floor has moved 1.5 units from where
        // it began (lifts), at most 300 updates; it restarts after 8 still
        // updates. K4's raft (object 647) bobs 0.63 units every ~90 updates
        // and never holds the bot.
        self.still = if riding { 0 } else { self.still + 1 };
        if riding {
            if self.riding == 0 {
                self.ride_from = fy;
            }
            self.riding += 1;
        } else if 8 <= self.still {
            self.riding = 0;
        }
        if riding && 1.5 < dist3(fy, self.ride_from) && self.riding < 300 {
            if self.ride_holds.0 != self.goal {
                self.ride_holds = (self.goal, 0);
            }
            self.ride_holds.1 += 1;
            if self.ride_holds.1 <= 1800 {
                self.progress = (f, p);
                self.dwell = (f, p);
                self.holds[2] += 1;
                return pad;
            }
            // Ridden long enough for this goal: give it up and steer.
            self.log.push(serde_json::json!({"update": f, "ride_giveup": format!("{:?}", self.goal), "floor": floor}));
            match self.goal {
                Some(Goal::Explore(n)) => {
                    let q = self.nav.pts[n];
                    self.visited_cols.insert(key(q[0], q[2]));
                }
                Some(Goal::Item(s)) => *self.stuck_goal_counts.entry(s).or_insert(0) += 10,
                _ => {}
            }
            self.ride_holds = (None, 0);
            self.riding = 300;
            self.choose(f, g);
        }
        if let Some(Goal::Item(s)) = self.goal
            && item_kind(g, s) == (5, 0x1f)
            && dist_xz(g.items.records[s].v3(0x40), p) < SHOT_FAR
            && (self.at + 2 >= self.path.len())
        {
            if (f / 6).is_multiple_of(2) {
                pad.press(0xd);
                self.tag(f, "shot");
            }
            return Self::steer(pad, g, p, g.items.records[s].v3(0x40));
        }
        // An exit portal close by while the goal is something else: step
        // away from it first (fights pulled E1's bot into its exit).
        let to_exit = !matches!(self.goal, Some(Goal::Item(s)) if item_kind(g, s).0 == 9);
        if to_exit {
            let near_exit = (0..g.items.records.len())
                .filter(|&s| g.items.records[s].active() && item_kind(g, s).0 == 9)
                .map(|s| (g.items.records[s].v3(0x40), g.items.records[s].f32_(0xe8).max(1.0) + 2.5))
                .find(|&(q, reach)| dist_xz(q, p) < reach && (q[1] - p[1]).abs() < 3.0);
            if let Some((q, _)) = near_exit {
                return Self::steer(pad, g, p, [2.0 * p[0] - q[0], p[1], 2.0 * p[2] - q[2]]);
            }
        }
        if let Some((until, q)) = self.gate_push {
            if f < until {
                return Self::steer(pad, g, p, q);
            }
            self.gate_push = None;
        }
        if let Some((until, away)) = self.backoff {
            if f < until {
                return Self::steer(pad, g, p, away);
            }
            self.backoff = None;
        }
        // Waypoint following (nodes on movers where their floors are now).
        while let Some(&n) = self.path.get(self.at) {
            let w = self.nav.live(g, n);
            if dist_xz(p, w) < 0.9 && (p[1] - w[1]).abs() < 2.5 {
                self.at += 1;
            } else {
                break;
            }
        }
        // A ride edge next: wait at its near end until the mover brings the
        // far floor within a step (a foe within reach is fought meanwhile).
        if let (Some(&a), Some(&b)) = (self.at.checked_sub(1).and_then(|i| self.path.get(i)), self.path.get(self.at))
            && self.nav.ride.contains(&(a, b))
        {
            let (wa, wb) = (self.nav.live(g, a), self.nav.live(g, b));
            // Timed per edge, waiting and crossing alike: a mover bobbing in
            // and out of step range restarted a per-wait timer, and J6's bot
            // held at one edge from update 30,000 to 300,000 (b74).
            let since = match self.ride_wait {
                Some((e, s)) if e == (a, b) => s,
                _ => f,
            };
            self.ride_wait = Some(((a, b), since));
            let in_range = (-RIDE_DROP..=RIDE_CLIMB).contains(&(wb[1] - p[1]));
            if !in_range || 2 * RIDE_WAIT <= f - since {
                // A mover standing still for 300 updates is not coming (a
                // lift that moves only while stood on: K1's by (−71, 74),
                // 31 units below the ledge, waited for 16 times in
                // k1rune-c38-v1).
                // Standing on the mover itself: a stand-on lift starts once
                // stood on (K1's lift 122 by pad 7: 438 timeouts at once in
                // b71k), so 200 updates first.
                let floor = g.players[0].u32_(0x89c).checked_sub(1).map(|o| o as usize);
                let aboard = [a, b].iter().any(|&n| floor == Some(self.nav.obj[n]) && self.movers.contains_key(&self.nav.obj[n]));
                let still = !(aboard && f - since < 200) && [b, a].iter().filter_map(|&n| self.movers.get(&self.nav.obj[n])).all(|m| 300 < f - m.moved_at.min(f));
                if RIDE_WAIT <= f - since || still || 2 * RIDE_WAIT <= f - since {
                    self.log.push(serde_json::json!({"update": f, "ride_timeout": [wa, wb]}));
                    self.blocked.insert((self.nav.place(a), self.nav.place(b)));
                    // Every ride edge onto the same object within 4 units: the
                    // mover does not come (I2, i2r1: 228 timeouts, eight at
                    // each of a dozen edges onto one platform by (193, 28, 64)).
                    let ob = self.nav.obj[b];
                    let same: Vec<(usize, usize)> = self.nav.ride.iter().copied().filter(|&(n, m)| self.nav.obj[m] == ob && dist_xz(self.nav.pts[n], p) < 4.0).collect();
                    for (n, m) in same {
                        let (pn, pm) = (self.nav.place(n), self.nav.place(m));
                        self.blocked.insert((pn, pm));
                    }
                    self.ride_wait = None;
                    self.choose(f, g);
                    return pad;
                }
                self.progress = (f, p);
                self.dwell = (f, p);
                if Self::nearest_foe(g, p, ATTACK_RANGE).is_none() {
                    self.holds[2] += 1;
                    return if 0.6 < dist_xz(p, wa) { Self::steer(pad, g, p, wa) } else { pad };
                }
            }
        } else {
            self.ride_wait = None;
        }
        let target = match (self.path.get(self.at), self.goal) {
            // A step up next: aim past its top, so the player reaches it
            // moving (see run_ups).
            (Some(&n), _) => {
                let w = self.nav.live(g, n);
                match self.at.checked_sub(1).and_then(|i| self.path.get(i)) {
                    Some(&a) if 1.0 < w[1] - self.nav.live(g, a)[1] => {
                        let v = self.nav.live(g, a);
                        Some([2.0 * w[0] - v[0], w[1], 2.0 * w[2] - v[2]])
                    }
                    _ => Some(w),
                }
            }
            // On a stand-on trigger's object: stay at the path's end.
            (None, Some(Goal::Item(s))) if item_kind(g, s).0 == 5 && g.items.records[s].u16_(0xf4) & 0x100 != 0 => self.path.last().map(|&n| self.nav.pts[n]),
            (None, Some(Goal::Item(s))) => Some(g.items.records[s].v3(0x40)),
            (None, Some(Goal::Explore(n))) => self.nav.pts.get(n).copied(),
            (None, Some(Goal::Point(h))) => self.hints.get(h).and_then(|x| x.goto),
            _ => None,
        };
        // At the end of the path to a trigger out of touch (behind a wall):
        // strike toward it (strike_reach), at most STRIKE_FOR updates.
        if let Some(Goal::Item(s)) = self.goal
            && self.at >= self.path.len()
            && let Some(sr) = strike_reach(g, s)
        {
            let q = g.items.records[s].v3(0x40);
            let d = dist_xz(p, q);
            if trigger_touch(g, s) - 0.3 <= d && d < sr + 0.5 && (q[1] - p[1]).abs() < 3.0 {
                let since = match self.strike {
                    Some((t, since)) if t == s => since,
                    _ => {
                        self.log.push(serde_json::json!({"update": f, "strike": s, "from": p, "d": d}));
                        f
                    }
                };
                self.strike = Some((s, since));
                if f - since < STRIKE_FOR + LURE_FOR {
                    self.progress = (f, p);
                    self.dwell = (f, p);
                    // A melee swing under way: the stick stays released (the
                    // probe needs it at the swing's event), attack held for
                    // 20 of 30 updates.
                    if f < self.melee_until {
                        if f + 10 < self.melee_until {
                            pad.press(0xd);
                            self.tag(f, "strike");
                        }
                        return pad;
                    }
                    // A foe within melee reach: turn to a facing whose cone
                    // holds it and whose probe lands on the trigger, then
                    // attack with the stick released (melee_facing). The
                    // Sorceress swings — the only attack that probes — only
                    // so (J3's 502 from the chain's J2 end: 0 of 30 tries
                    // facing the trigger; tfj3e: no melee request in any strike
                    // window, the foes beside or behind). The facing is
                    // PlayerMotion's atan2 of matrix row +0x40/+0x48.
                    let r = &g.players[0];
                    let facing = r.f32_(0x40).atan2(r.f32_(0x48));
                    // DIAGNOSTIC: the foes within 4 units every 60 updates
                    // (closest_enemy's distance less the width, the angle
                    // from the trigger's direction in degrees, the type).
                    if (f - since).is_multiple_of(60) && self.strike_logs < 200 {
                        use gdl_core::game::enemies::e;
                        let o = r.v3(0x70);
                        let qa = (q[0] - p[0]).atan2(q[2] - p[2]);
                        let near: Vec<_> = g
                            .enemies
                            .iter()
                            .filter(|en| en.i32_(e::STATE) != 0 && 0.0 < en.f32_(e::HEALTH))
                            .filter_map(|en| {
                                let c = en.v3(e::COLLISION);
                                let v = [c[0] - o[0], c[1] - o[1], c[2] - o[2]];
                                let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                                let a = (v[0].atan2(v[2]) - qa + std::f32::consts::PI).rem_euclid(2.0 * std::f32::consts::PI) - std::f32::consts::PI;
                                (l < 4.0).then(|| serde_json::json!([(l * 100.0).round() / 100.0, en.f32_(e::WIDTH), a.to_degrees().round(), en.i32_(e::TYPE), en.i32_(e::STATE)]))
                            })
                            .collect();
                        if !near.is_empty() {
                            self.strike_logs += 1;
                            self.log.push(serde_json::json!({"update": f, "strike_foes": near, "facing_rel": ((facing - qa + std::f32::consts::PI).rem_euclid(2.0 * std::f32::consts::PI) - std::f32::consts::PI).to_degrees().round(), "d": dist_xz(p, q)}));
                        }
                    }
                    // A foe within melee reach at any angle while the facing's
                    // probe lands on the trigger: flick the stick at the foe
                    // for one update with attack pressed, then release it.
                    // PlayerMotion's targeting tests the cone along the
                    // stick's direction (want), and the swing does not turn
                    // the player (+0x8dc picks a side or back swing); one
                    // update turns the facing by at most spf × 5π × +0x968
                    // (about 30°), inside the probe's window (tfj3g: type-24
                    // foes 3.9 units off, width 2.0, at 112°–178° from the
                    // trigger's direction, held still for hundreds of
                    // updates; no single facing covered both).
                    if probe_touches(g, s, p, facing, 0.3)
                        && let Some(t) = melee_foe(g, p)
                    {
                        self.melee_until = f + 30;
                        self.melee_swings += 1;
                        if self.melee_swings <= 40 {
                            self.log.push(serde_json::json!({"update": f, "melee_at": s, "from": p, "facing": facing, "flick": t}));
                        }
                        pad.press(0xd);
                        self.tag(f, "strike");
                        return Self::steer(pad, g, p, t);
                    }
                    if let Some(w) = melee_facing(g, s, p, facing) {
                        if angle_diff(facing, w) <= 0.1 {
                            self.melee_until = f + 30;
                            self.melee_swings += 1;
                            if self.melee_swings <= 40 {
                                self.log.push(serde_json::json!({"update": f, "melee_at": s, "from": p, "facing": facing}));
                            }
                            pad.press(0xd);
                            self.tag(f, "strike");
                            return pad;
                        }
                        return Self::steer(pad, g, p, [p[0] + 3.0 * w.sin(), p[1], p[2] + 3.0 * w.cos()]);
                    }
                }
                // Lure phase after STRIKE_FOR: stand and face the trigger
                // (one-update taps when off by over 0.3 rad — any stick input
                // moves the player; c58/c59), waiting for a foe to come
                // within melee reach (above).
                if STRIKE_FOR <= f - since && f - since < STRIKE_FOR + LURE_FOR {
                    let lure_t = f - since - STRIKE_FOR;
                    let r = &g.players[0];
                    let facing = r.f32_(0x40).atan2(r.f32_(0x48));
                    let want = (q[0] - p[0]).atan2(q[2] - p[2]);
                    if 0.3 < angle_diff(facing, want) && lure_t % 10 == 0 {
                        return Self::steer(pad, g, p, q);
                    }
                    return pad;
                }
                if f - since < STRIKE_FOR {
                    self.progress = (f, p);
                    self.dwell = (f, p);
                    // Face it, then: with turbo +0x800 at 40 or more, the turbo
                    // attack (triangle edge: a melee with event bits 0xf0)
                    // with the stick released; otherwise attack with the stick
                    // released — a melee swing (event 2) only when a target
                    // lies within the player's radius + 1, else a throw (0x5c,
                    // event bit 1), which does not probe (J3's 502 from 2.86
                    // units: throws only, state unchanged). Turbo refills by
                    // 2 × spf an update.
                    let t = (f - since) % 40;
                    if t < 6 {
                        return Self::steer(pad, g, p, q);
                    }
                    if 40.0 <= g.players[0].f32_(0x800) {
                        if t == 6 {
                            pad.press(0xc);
                        }
                        return pad;
                    }
                    if t < 30 {
                        pad.press(0xd);
                        self.tag(f, "strike");
                    }
                    return pad;
                }
            }
        }
        // The nearest live enemy in range: face it and hold attack (cross,
        // DMotion 0xf, acts on the level); stuck: hold attack along the path.
        // A boss (critter kind 4) in range: fight from a distance — hold
        // attack facing it; inside 8 units step back with attack released
        // (held attack keeps the player in its attack actions, so it never
        // walked away; H4's Garm took no damage from 5.8 units for 48,000
        // updates in b12).
        // Just carried by a platform (a lift's top, a ferry's landing): walk
        // off before fighting. J6's lift 694 (pad 31) stays level with the
        // rune room for a few seconds; the bot fought the room's generator
        // from the lift and rode it back down (teleport check, f91).
        if f < self.disembark_until {
            return match target {
                Some(t) => Self::steer(pad, g, p, t),
                None => pad,
            };
        }
        let boss_inst = usize::try_from(g.boss_crit_inst - 1).ok();
        let boss = boss_inst.and_then(|b| g.critters.get(b)).and_then(Option::as_ref).filter(|c| 0.0 < c.rec.f32_(0x4f4)).map(|c| c.rec.v3(0x40));
        if let Some(bp) = boss.filter(|bp| dist_xz(*bp, p) < self.boss_range) {
            if dist_xz(bp, p) < 8.0 && (f / 20).is_multiple_of(2) {
                return Self::steer(pad, g, p, [2.0 * p[0] - bp[0], p[1], 2.0 * p[2] - bp[2]]);
            }
            // Knocked back past the fight node (the foe goal): walk back to it
            // in alternate windows. E2's tries took 8.6–10.6 a sample and dealt
            // 12.4–12.8 from 24–28 units of Skorne, but 17–20 against 2.3–3.7
            // from 32–36, where they spent a quarter of the fight (e2t).
            if let Some(Goal::Explore(n)) = self.goal
                && let Some(&q) = self.nav.pts.get(n)
                && dist_xz(bp, q) + 2.0 < dist_xz(bp, p)
                && (f / 20).is_multiple_of(2)
            {
                self.tag(f, "boss_close_in");
                return Self::steer(pad, g, p, q);
            }
            // A child (A5's chimera heads) low enough to be hit, the weakest
            // first: CritterDamage takes a child's damage from the root too,
            // and each dead child lifts the boss's active-critter scaling
            // (0.2 with the body and three heads up, then 0.3, 0.5, 1). The
            // heads idle 10–20 units above the player (lg23, a5L).
            let child = g
                .critters
                .iter()
                .flatten()
                // +0x8 state 2 or more: CritterDamage's damageable instances.
                .filter(|c| c.parent.is_some() && c.parent == boss_inst && 0.0 < c.rec.f32_(0x4f4) && 2 <= c.rec.u32_(0x8))
                .map(|c| (c.rec.f32_(0x4f4), c.rec.v3(0x40)))
                .filter(|(_, q)| (q[1] - p[1]).abs() < 6.0 && dist_xz(*q, p) < self.boss_range)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, q)| q);
            let aim = child.unwrap_or(bp);
            // Without the assist: a potion (circle, entry 0xb, the magic
            // blast) every 300 updates while one is held. The potion rule
            // below never ran at a boss (this branch returns first): E2's
            // tries held nine bought potions and died with Skorne at 119–347
            // of 6000 (e2t).
            if !self.assist_enabled && 0 < g.players[0].i32_(0x1a18) && self.potion_at.is_none_or(|u| u + 300 <= f) {
                self.potion_at = Some(f);
                let bh = boss_inst.and_then(|b| g.critters.get(b)).and_then(Option::as_ref).map(|c| c.rec.f32_(0x4f4));
                self.log.push(serde_json::json!({"update": f, "potion": g.players[0].i32_(0x1a18), "boss_health": bh}));
                pad.press(0xb);
                return pad;
            }
            // Turbo (triangle, entry 0xc) every 40 updates at +0x800 >= 40:
            // the meter otherwise sat full through the fight (lg23).
            if 40.0 <= g.players[0].f32_(0x800) && f.is_multiple_of(40) {
                pad.press(0xc);
            } else {
                pad.press(0xd);
            }
            self.tag(f, if child.is_some() { "boss_child" } else { "boss" });
            return Self::steer(pad, g, p, aim);
        }
        // Enemies within ATTACK_RANGE: attack in windows (12 updates
        // fighting, 12 moving; holding attack keeps the player in its attack
        // actions). Without the health assist the bot must survive: it fires
        // every update at those within ATTACK_RANGE and in windows at those
        // within twice that. Close-range windows alone let crowds reach the
        // player (no-assist G1 died in all three tries by update 11714; with
        // ranged fire two of three survived 60,000 updates, health 308 and
        // 403 of 483), but the fights cost assisted runs their progress
        // (b32: B5 and A3 no longer ended within 60,000 updates).
        // Also when stuck again and again (five stuck events within 2000
        // updates): enemies scale with the player's level, and in the
        // campaign (level 25 by K2) a crowd by K2's barrels held the bot.
        let recent = self.log.iter().rev().take_while(|e| e["update"].as_u64().is_some_and(|u| u + 2000 >= f as u64)).filter(|e| e.get("stuck").is_some()).count();
        let ranged = !self.assist_enabled || 5 <= recent;
        let melee_class = !self.assist_enabled && g.players[0].f32_(0x120) < g.players[0].f32_(0x118);
        // A potion (circle, entry 0xb: the magic blast) against a crowd that
        // holds the bot (K2 in the campaign: level-scaled LOW creatures
        // under the thrown weapons' height kept the corridor by generator 333
        // closed) or, without the assist, when health runs low. At most one
        // every 600 updates.
        // Death (enemy type 0x1e) takes no weapon damage and drains health
        // on contact (death_touch); a magic blast kills it. It is no fight
        // target (nearest_foe); within 4 units a potion if one is held (one
        // every 150 updates). Stepping away from it held A1's bot by the
        // fenced Death statue for 150,000 updates (b54).
        let death = g
            .enemies
            .iter()
            .filter(|e| e.i32_(0xc8) != 0 && e.i32_(0) == 0x1e && (e.v3(0x40)[1] - p[1]).abs() < 4.0)
            .map(|e| (dist_xz(e.v3(0x40), p), e.v3(0x40)))
            .filter(|&(d, _)| d < 4.0)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, at)) = death
            && 0 < g.players[0].i32_(0x1a18)
            && self.potion_at.is_none_or(|u| u + 150 <= f)
        {
            self.potion_at = Some(f);
            self.log.push(serde_json::json!({"update": f, "potion": g.players[0].i32_(0x1a18), "death": at}));
            pad.press(0xb);
            return pad;
        }
        // Without the assist, generators (kind 3) within SHOOT_RANGE come
        // first while no foe is within 2 units: they spawn the crowds (lg3's
        // G2 tries died in a nest of four by (−184, 0, −165), fighting the
        // spawns from health 383 to 0 in 1800 updates). One whose health
        // (+0xe4) has not dropped in 300 updates of fire, or with armour −1
        // (+0xe3), is left alone.
        // A melee class fights the foes first (wg1c: the Warrior throwing at
        // generator 123 while its knights struck from 4 units).
        // A ranged class leaves the generators for foes within 4 units, or
        // within ATTACK_RANGE after a hit in the last 30 updates: C1's tries
        // took most of their damage standing at generators while type-7
        // creatures struck from 2.3–3.7 units (c1n-d61-tag).
        // EXPERIMENT GDL_BOT_GEN_GUARD=N: the ranged class's guard distance.
        let guard_x = std::env::var("GDL_BOT_GEN_GUARD").ok().and_then(|v| v.parse::<f32>().ok());
        let gen_guard = if melee_class { ATTACK_RANGE } else if let Some(x) = guard_x { x } else if self.hurt_at.is_some_and(|h| f < h + 30) { ATTACK_RANGE } else { 4.0 };
        // Hurt within 30 updates with an enemy above or below within 12
        // units (8 of height): that fight first (the ranged branch's
        // retaliation; lg30's C1 try 2 shot generators under them).
        let threat_above = self.hurt_at.is_some_and(|h| f < h + 30)
            && g.enemies.iter().any(|e| e.i32_(0xc8) != 0 && e.f32_(0x214) > 0.0 && e.i32_(0) != 0x1e && (e.v3(0x40)[1] - p[1]).abs() < 8.0 && dist_xz(e.v3(0x40), p) < 12.0);
        let nest = f < self.nest_until;
        if nest && 0 < g.players[0].i32_(0x1a18) && Self::nearest_foe(g, p, 6.0).is_some() && self.potion_at.is_none_or(|u| u + 300 <= f) {
            self.potion_at = Some(f);
            self.log.push(serde_json::json!({"update": f, "potion": g.players[0].i32_(0x1a18), "nest": true}));
            pad.press(0xb);
            return pad;
        }
        if !self.assist_enabled && (nest || (!threat_above && Self::nearest_foe(g, p, gen_guard).is_none())) {
            // Also up to GEN_RANGE along a clear line (lg8's fastest G2
            // death fought the knights of generators 123 and 125, 11–23
            // units off, from health 700 to 370 by update 1800).
            let mut gens: Vec<usize> = (0..g.items.records.len())
                .filter(|&s| {
                    let r = &g.items.records[s];
                    // Shown only (+0xe1 0): ProcessItems, closest_generator and
                    // ItemCollSub skip the others — a generator in state 2
                    // (shown once off screen) stays hidden while the bot
                    // stands at it (wg1g: the Warrior beside generator 170,
                    // +0xe1 2, PlayerGetTarget finding nothing).
                    r.active() && r.u8_(0xe1) == 0 && item_kind(g, s).0 == 3 && 0 <= r.i8_(0xe3) && !self.gen_skip.contains(&s) && dist_xz(r.v3(0x40), p) < GEN_RANGE && (r.v3(0x40)[1] - p[1]).abs() < 3.0
                })
                .collect();
            gens.sort_by(|&a, &b| dist_xz(g.items.records[a].v3(0x40), p).total_cmp(&dist_xz(g.items.records[b].v3(0x40), p)));
            let gen_target = gens.into_iter().find(|&s| {
                let q = g.items.records[s].v3(0x40);
                if dist_xz(q, p) < SHOOT_RANGE {
                    return true;
                }
                if let Some(&(at, ok)) = self.gen_seen.get(&s)
                    && f < at + 60
                {
                    return ok;
                }
                let ok = clear_line(&mut self.coll, g, p, q);
                self.gen_seen.insert(s, (f, ok));
                ok
            });
            if let Some(s) = gen_target {
                let h = g.items.records[s].u16_(0xe4);
                let e = self.gen_fire.entry(s).or_insert((f, h));
                if h < e.1 {
                    *e = (f, h);
                } else if 300 <= f - e.0 {
                    self.gen_skip.insert(s);
                }
                // A melee class walks up to it first (its throws are weak).
                let gq = g.items.records[s].v3(0x40);
                if !melee_class || dist_xz(gq, p) < 3.0 {
                    pad.press(0xd);
                    self.tag(f, "generator");
                }
                return Self::steer(pad, g, p, gq);
            }
        }
        let crowd = Self::nearest_foe(g, p, 6.0).is_some();
        let low_health = !self.assist_enabled && g.players[0].f32_(HEALTH) < self.start_health * 0.35;
        // EXPERIMENT GDL_BOT_CROWD_POTION=1 (no assist): a potion when three
        // or more foes are within 6 units, at most every 600 updates.
        if !self.assist_enabled && std::env::var("GDL_BOT_CROWD_POTION").is_ok_and(|v| v == "1") && 0 < g.players[0].i32_(0x1a18) && self.potion_at.is_none_or(|u| u + 600 <= f) {
            let n = g.enemies.iter().filter(|e| e.i32_(0xc8) != 0 && e.f32_(0x214) > 0.0 && dist_xz(e.v3(0x40), p) < 6.0 && (e.v3(0x40)[1] - p[1]).abs() < 4.0).count();
            if 3 <= n {
                self.potion_at = Some(f);
                self.log.push(serde_json::json!({"update": f, "potion": g.players[0].i32_(0x1a18), "crowd": n}));
                pad.press(0xb);
                return pad;
            }
        }
        if crowd && (5 <= recent || low_health) && 0 < g.players[0].i32_(0x1a18) && self.potion_at.is_none_or(|u| u + 600 <= f) {
            self.potion_at = Some(f);
            self.log.push(serde_json::json!({"update": f, "potion": g.players[0].i32_(0x1a18)}));
            pad.press(0xb);
            return pad;
        }
        // Without the assist, a class whose melee damage (+0x118) exceeds
        // its throw's (+0x120: the Warrior 14 against 10.4, the Dwarf 14
        // against 10.4; the Sorceress 8 against 22.4) closes in and swings:
        // attack (a melee swing with a target within radius + 1 in front,
        // PlayerMotion's attack request) only within 2.6 units of the
        // nearest foe, walking at it otherwise, the turbo attack (its melee
        // triples the damage) every 40 updates at 40 or more; no stepping
        // away (lgw1: the Warrior kiting from knights threw 247 times and
        // swung 33, dead in G1 by update 8,085 in all six tries).
        if melee_class && let Some((_, at)) = Self::nearest_foe(g, p, ATTACK_RANGE) {
            // In reach by the game's measure (closest_enemy: +0x70 to the
            // enemy's +0x60 less its width +0x24c, under radius + 1; a
            // knight's body stops the player about 3 units off in x/z, so an
            // x/z test never fired: wg1b) or a critter within 3.5 units.
            use gdl_core::game::enemies::e;
            let r = &g.players[0];
            let o = r.v3(0x70);
            let reach = r.f32_(0x828) + 1.0 - 0.1;
            let in_reach = g
                .enemies
                .iter()
                .filter(|en| matches!(en.i32_(e::STATE), 1 | 6) && 0.0 < en.f32_(e::HEALTH) && !matches!(en.i32_(e::TYPE), 0x1e | 0x1f))
                .map(|en| {
                    let c = en.v3(e::COLLISION);
                    let v = [c[0] - o[0], c[1] - o[1], c[2] - o[2]];
                    ((v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt() - en.f32_(e::WIDTH), c)
                })
                .filter(|(dg, _)| *dg < reach)
                .min_by(|x, y| x.0.total_cmp(&y.0))
                .map(|(_, c)| c)
                .or_else(|| g.critters.iter().flatten().filter(|c| 0.0 < c.rec.f32_(0x4f4) && dist_xz(c.rec.v3(0x40), p) < 3.5).map(|c| c.rec.v3(0x40)).next());
            if let Some(t) = in_reach {
                self.tag(f, "melee");
                if 40.0 <= r.f32_(0x800) && f.is_multiple_of(40) {
                    pad.press(0xc);
                } else {
                    pad.press(0xd);
                }
                return Self::steer(pad, g, p, t);
            }
            return Self::steer(pad, g, p, at);
        }
        let near = Self::nearest_foe(g, p, ATTACK_RANGE);
        let mut enemy = if ranged { near.or_else(|| Self::nearest_foe(g, p, 2.0 * ATTACK_RANGE)) } else { near };
        // Without the assist, hurt within the last 60 updates and no foe
        // within twice ATTACK_RANGE: a live critter (an awake statue's
        // creature) within 20 units is the target — C1's tries were hit for
        // 8–43 from 10–19 units by one they never fired at (feas-C1).
        let retaliate = !self.assist_enabled && enemy.is_none() && self.hurt_at.is_some_and(|h| f < h + 60);
        if retaliate {
            let skip = |k: (u8, usize)| FOE_SKIP.with(|s| s.borrow().0.get(&k).is_some_and(|&until| s.borrow().1 < until));
            enemy = g
                .critters
                .iter()
                .enumerate()
                .filter_map(|(k, c)| c.as_ref().map(|c| (k, c)))
                .filter(|&(k, c)| 0.0 < c.rec.f32_(0x4f4) && 2 <= c.rec.u32_(0x8) && !skip((1, k)) && (c.rec.v3(0x40)[1] - p[1]).abs() < 6.0)
                .map(|(_, c)| (dist_xz(c.rec.v3(0x40), p), c.rec.v3(0x40)))
                .filter(|(d, _)| *d < 20.0)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            // Or an enemy above or below within 12 units and 8 of height
            // (nearest_foe takes 3; closest_enemy's auto-aim 10): C1's type-7
            // creatures struck from 2–6 units above the pit floor at (−32,
            // −12, 16) while the bot shot generators (lg30 C1 try 2: 2,600
            // health in 1,100 updates).
            enemy = enemy.or_else(|| {
                g.enemies
                    .iter()
                    .enumerate()
                    .filter(|&(k, e)| e.i32_(0xc8) != 0 && e.f32_(0x214) > 0.0 && e.i32_(0) != 0x1e && (e.v3(0x40)[1] - p[1]).abs() < 8.0 && !skip((0, k)))
                    .map(|(_, e)| (dist_xz(e.v3(0x40), p), e.v3(0x40)))
                    .filter(|(d, _)| *d < 12.0)
                    .min_by(|a, b| a.0.total_cmp(&b.0))
            });
        }
        // EXPERIMENT (GDL_BOT_RANGED_FAR=1, no assist): fire every update at
        // foes within twice ATTACK_RANGE too (F1's no-assist try took 8,100
        // damage from types 2/20/24/25 at 4.4–7.2 units, feas-F1).
        let far = !self.assist_enabled && std::env::var("GDL_BOT_RANGED_FAR").is_ok_and(|v| v == "1");
        let fighting = (f / 12).is_multiple_of(2) || (ranged && near.is_some()) || (far && enemy.is_some()) || (retaliate && enemy.is_some());
        // An enemy against the player (within 2 units): step away for a
        // moment, then fire — thrown weapons leave beyond it (K2 in the
        // campaign: a type-0x15 creature, never knocked back, held the player
        // for 100,000 updates at health 14 without a hit).
        // Without the assist from 3.2 units, every other 20-update window:
        // knights (type 13) strike from about 3 (hurt-g2-easy-v2: 382 of
        // 790 health lost to knights 3 to 3.5 units off).
        // Without the assist and not held by repeated stuck events: always
        // back away from foes within 4 units (kiting) — lg13's tries took
        // 173–278 of their knight damage throwing on the spot (Easy halves
        // nothing but enemy speed and aggression: × 0.667).
        let (close, every) = if self.assist_enabled {
            (2.0, 3)
        } else if recent < 5 {
            (4.0, 1)
        } else {
            (3.2, 2)
        };
        let window = if self.assist_enabled { 30 } else { 20 };
        // A weak foe (health at most twice the throw's damage +0x120: one or
        // two shots) is shot, not kited from: C2's type-7 creatures (20–30
        // health against a 28 throw) struck from 3–4 units while the bot
        // backed away without firing, 63 % of its damage taken standing
        // against walls (lg31 C2, twelve tries dead).
        let weak = near.is_some_and(|(_, at)| {
            g.enemies.iter().find(|e| e.i32_(0xc8) != 0 && dist_xz(e.v3(0x40), at) < 0.01).is_some_and(|e| e.f32_(0x214) <= 2.0 * g.players[0].f32_(0x120))
        });
        if let Some((d, at)) = near
            && d < close
            && !(weak && !self.assist_enabled)
            && (f / window).is_multiple_of(every)
        {
            return Self::steer(pad, g, p, [2.0 * p[0] - at[0], p[1], 2.0 * p[2] - at[2]]);
        }
        let target = match enemy.filter(|_| fighting) {
            // Turbo (triangle, entry 0xc: an edge gives code 0x15, the turbo
            // attack at +0x800 >= 40; its melee triples the damage) every 40
            // updates against a foe within reach.
            Some((_, at)) if near.is_some() && 40.0 <= g.players[0].f32_(0x800) && f.is_multiple_of(40) => {
                pad.press(0xc);
                Some(at)
            }
            Some((_, at)) => {
                pad.press(0xd);
                self.note_fire(f, g, at);
                self.tag(f, "ranged");
                if self.attack_tags < 600 && std::env::var("GDL_BOT_TAG_FROM").is_ok_and(|v| v.parse::<usize>().is_ok_and(|n| n <= f)) && f.is_multiple_of(25) {
                    // DIAGNOSTIC: the ranged target (enemy type, health, state).
                    let who: Vec<_> = g.enemies.iter().enumerate().filter(|(_, e)| e.i32_(0xc8) != 0 && dist_xz(e.v3(0x40), at) < 0.01).map(|(k, e)| serde_json::json!([k, e.i32_(0), e.f32_(0x214), e.i32_(0xc8), e.v3(0x40)])).collect();
                    self.log.push(serde_json::json!({"update": f, "ranged_at": at, "who": who}));
                }
                Some(at)
            }
            None => {
                if f < self.attack_until {
                    pad.press(0xd);
                    self.tag(f, "stuck");
                    if let Some(at) = self.attack_at {
                        return Self::steer(pad, g, p, at);
                    }
                }
                target
            }
        };
        match target {
            Some(t) => Self::steer(pad, g, p, t),
            None => pad,
        }
    }

    /// The nearest live enemy (not Death: no weapon damages it), or critter
    /// (bosses, statues' creatures: +0x4f4 health), within `range` and 3
    /// units of height.
    fn nearest_foe(g: &Game, p: V3, range: f32) -> Option<(f32, V3)> {
        let skip = |k: (u8, usize)| FOE_SKIP.with(|s| s.borrow().0.get(&k).is_some_and(|&until| s.borrow().1 < until));
        let enemies = g
            .enemies
            .iter()
            .enumerate()
            .filter(|&(k, e)| e.i32_(0xc8) != 0 && e.f32_(0x214) > 0.0 && e.i32_(0) != 0x1e && (e.v3(0x40)[1] - p[1]).abs() < 3.0 && !skip((0, k)))
            .map(|(_, e)| (dist_xz(e.v3(0x40), p), e.v3(0x40)));
        let critters = g.critters.iter().enumerate().filter_map(|(k, c)| c.as_ref().map(|c| (k, c))).filter(|&(k, c)| 0.0 < c.rec.f32_(0x4f4) && !skip((1, k))).map(|(_, c)| (dist_xz(c.rec.v3(0x40), p), c.rec.v3(0x40)));
        enemies.chain(critters).filter(|(d, _)| *d < range).min_by(|a, b| a.0.total_cmp(&b.0))
    }

    /// A fight target fired at (the foe at `at`): one whose health has not
    /// dropped through 600 updates of fire (gaps under 100) is no target for
    /// 3000 updates (a4dbg3: lg21's A4 tries threw at a critter 6.25 units
    /// off at (−18.8, 0, −8.9) from update 120,600 on without hurting it).
    fn note_fire(&mut self, f: usize, g: &Game, at: V3) {
        let key = g
            .enemies
            .iter()
            .enumerate()
            .find(|(_, e)| e.i32_(0xc8) != 0 && dist_xz(e.v3(0x40), at) < 0.01 && (e.v3(0x40)[1] - at[1]).abs() < 0.01)
            .map(|(k, e)| ((0u8, k), e.f32_(0x214)))
            .or_else(|| g.critters.iter().enumerate().find_map(|(k, c)| c.as_ref().filter(|c| dist_xz(c.rec.v3(0x40), at) < 0.01).map(|c| ((1u8, k), c.rec.f32_(0x4f4)))));
        let Some((key, health)) = key else { return };
        // A losing exchange (without the assist): after 900 updates of fire
        // (gaps under 100) the foe has lost under a fifth of its health
        // while the player lost 800 or more — no target for 6000 updates
        // (logged `foe_losing`). C1's no-assist try fought a critter from
        // 305 to 268 health for 6,000 updates and lost 3,400 (feas-C1).
        let me = g.players[0].f32_(HEALTH);
        let t = self.foe_trade.entry(key).or_insert((f, f, health, me));
        if 100 < f - t.1 {
            *t = (f, f, health, me);
        }
        t.1 = f;
        let (first, _, h0, me0) = *t;
        if !self.assist_enabled && 900 <= f - first && h0 * 0.8 < health && 800.0 <= me0 - me {
            FOE_SKIP.with(|s| s.borrow_mut().0.insert(key, f + 6000));
            self.foe_trade.remove(&key);
            self.log.push(serde_json::json!({"update": f, "foe_losing": [key.0, key.1], "health": [h0, health], "player": [me0, me]}));
            return;
        }
        let e = self.foe_fire.entry(key).or_insert((f, f, health));
        if 100 < f - e.1 || health < e.2 {
            *e = (f, f, health);
        }
        e.1 = f;
        // 300 updates without the assist (C1's try spent 5,400 updates on
        // nine foes behind its fence at 600 each, c1h-d263).
        if (if self.assist_enabled { 600 } else { 300 }) <= f - e.0 {
            FOE_SKIP.with(|s| s.borrow_mut().0.insert(key, f + 3000));
            self.foe_fire.remove(&key);
            self.log.push(serde_json::json!({"update": f, "foe_skip": [key.0, key.1], "health": health}));
        }
    }

    /// DIAGNOSTIC: which branch pressed attack (every 5th update, at most 600).
    /// GDL_BOT_TAG_FROM=N (DIAGNOSTIC): tags from update N on.
    fn tag(&mut self, f: usize, name: &str) {
        static FROM: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let from = *FROM.get_or_init(|| std::env::var("GDL_BOT_TAG_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
        if from <= f && f.is_multiple_of(5) && self.attack_tags < 600 {
            self.attack_tags += 1;
            self.log.push(serde_json::json!({"update": f, "atk": name}));
        }
    }

    /// The left stick toward `t`: PlayerMotion's heading is the camera yaw
    /// (atan2(camera_90, camera_98) + π) plus the stick angle
    /// (calc_analog_stick: atan2(left − right, down − up)).
    fn steer(mut pad: PadStatus, g: &Game, p: V3, t: V3) -> PadStatus {
        let (mut dx, mut dz) = (t[0] - p[0], t[2] - p[2]);
        if AVOID_CONTACT.load(std::sync::atomic::Ordering::Relaxed) {
            (dx, dz) = keep_off(g, p, dx, dz);
        }
        if 1e-3 < dx.abs() + dz.abs() {
            let cam = g.globals.camera_90.atan2(g.globals.camera_98) + std::f32::consts::PI;
            let a = dx.atan2(dz) - cam;
            let (h, v) = (a.sin(), a.cos());
            let to = |x: f32| (x.clamp(0.0, 1.0) * 255.0) as u8;
            pad.entries[4] = [3, to(h)];
            pad.entries[5] = [3, to(-h)];
            pad.entries[6] = [3, to(-v)];
            pad.entries[7] = [3, to(v)];
        }
        pad
    }

    /// Port `k` (players 2..4 in co-op runs): follow player 1 when more than
    /// 5 units away, attack the nearest enemy in range in the same windows
    /// as the leader, stand still otherwise.
    pub fn follower_step(&mut self, f: usize, k: usize, g: &mut Game) -> PadStatus {
        let mut pad = PadStatus { connected: true, analog: true, has_stick: true, ..Default::default() };
        for e in 4..8 {
            pad.entries[e] = [3, 0];
        }
        if g.players[k].i32_(0xfc) != 1 {
            return pad;
        }
        // The DEBUG health assist for followers only when the leader has it
        // (no-assist co-op is legitimate play).
        let health = g.players[k].f32_(HEALTH);
        if self.assist_enabled && health < self.start_health * 0.4 {
            g.players[k].set_f32(HEALTH, self.start_health);
            self.assists += 1;
        }
        let p = g.players[k].v3(0x50);
        let lead = g.players[0].v3(0x50);
        let enemy = Self::nearest_foe(g, p, ATTACK_RANGE);
        if let Some((_, at)) = enemy.filter(|_| (f / 12).is_multiple_of(2)) {
            pad.press(0xd);
            return Self::steer(pad, g, p, at);
        }
        // The leader waits on a trigger: stand beside it (pads for all
        // players, lifts).
        if self.wait.is_some() {
            return if dist_xz(p, lead) <= 1.5 { pad } else { Self::steer(pad, g, p, lead) };
        }
        // The leader waits on an exit portal (state 4): every player must
        // stand on one (B2 with four players: followers idled within 5 units
        // of the leader beside the portal). On an exit portal (+0x884 contact,
        // kind 9) release the stick, as the leader does; else walk the
        // leader's graph to the exit nearest the leader.
        // A key held by this follower while the leader, keyless, stands at a
        // closed gate (a follower walked over the key; use_key opens a gate
        // only for the player touching it — B2 with three players).
        let gate = (g.players[0].i32_(KEYS) < 1 && 0 < g.players[k].i32_(KEYS) && g.players[0].i32_(0xfc) == 1)
            .then(|| {
                (0..g.items.records.len())
                    .filter(|&s| g.items.records[s].active() && item_kind(g, s).0 == 7 && dist_xz(g.items.records[s].v3(0x40), lead) < 6.0)
                    .map(|s| g.items.records[s].v3(0x40))
                    .min_by(|a, b| dist_xz(*a, lead).total_cmp(&dist_xz(*b, lead)))
            })
            .flatten();
        if g.players[0].i32_(0xfc) != 1 || gate.is_some() {
            if gate.is_none()
                && let Some(it) = g.players[k].u32_(0x884).checked_sub(1).map(|i| i as usize)
                && it < g.items.records.len()
                && item_kind(g, it).0 == 9
            {
                return pad;
            }
            let exit = gate.or_else(|| {
                (0..g.items.records.len())
                    .filter(|&s| g.items.records[s].active() && item_kind(g, s).0 == 9)
                    .map(|s| g.items.records[s].v3(0x40))
                    .min_by(|a, b| dist_xz(*a, lead).total_cmp(&dist_xz(*b, lead)))
            });
            if let Some(t) = exit {
                let (path, idx, next) = &mut self.follow_nav[k];
                if *idx >= path.len() && *next <= f {
                    *next = f + 150;
                    let none = HashSet::new();
                    if let (Some(a), Some(b)) = (self.nav.locate_cached(p, &none), self.nav.locate_cached(t, &none))
                        && let Some(found) = self.nav.path(a, &|n| n == b, &HashSet::new(), &Avoid::default())
                    {
                        *path = found;
                        *idx = 0;
                    }
                }
                while *idx < path.len() && dist_xz(p, self.nav.pts[path[*idx]]) < 0.9 && (p[1] - self.nav.pts[path[*idx]][1]).abs() < 2.5 {
                    *idx += 1;
                }
                let goal = path.get(*idx).map_or(t, |&n| self.nav.pts[n]);
                // Blocked (another player stands in the way: the leader on the
                // portal's edge): under 0.3 units in 30 updates → step aside
                // for 20 updates, left or right by player.
                let (since, at, aside) = &mut self.follow_stuck[k];
                if 0.3 < dist_xz(p, *at) {
                    *at = p;
                    *since = f;
                }
                if f < *aside {
                    let (dx, dz) = (goal[0] - p[0], goal[2] - p[2]);
                    let side = if k.is_multiple_of(2) { 1.0 } else { -1.0 };
                    return Self::steer(pad, g, p, [p[0] - dz * side, p[1], p[2] + dx * side]);
                }
                if 30 <= f - *since {
                    *aside = f + 20;
                    *since = f;
                }
                return Self::steer(pad, g, p, goal);
            }
        }
        if dist_xz(p, lead) <= 5.0 {
            return pad;
        }
        // The leader's trail: from the trail point nearest the follower (in
        // 3D) onward, each point passed within 1.5 units.
        let st = &mut self.followers[k];
        if self.trail.is_empty() {
            return Self::steer(pad, g, p, lead);
        }
        if st.1.is_empty() || f.is_multiple_of(150) {
            let near = (0..self.trail.len())
                .min_by(|&a, &b| {
                    let d = |i: usize| dist_xz(self.trail[i], p) + (self.trail[i][1] - p[1]).abs() * 2.0;
                    d(a).total_cmp(&d(b))
                })
                .unwrap_or(0);
            st.1 = vec![near];
            st.0 = near;
        }
        while st.0 + 1 < self.trail.len() && dist_xz(p, self.trail[st.0]) < 1.5 && (p[1] - self.trail[st.0][1]).abs() < 2.5 {
            st.0 += 1;
        }
        let t = self.trail.get(st.0).copied().unwrap_or(lead);
        Self::steer(pad, g, p, t)
    }

    /// All floors of the level at the current collision state (bot=2).
    pub fn floors(&mut self, g: &Game, min: V3, max: V3) -> Vec<V3> {
        let mut c = g.collision.clone();
        let mut nav = Nav { pts: Vec::new(), obj: Vec::new(), adj: Vec::new(), hard: HashSet::new(), ride: HashSet::new(), movers: HashMap::new(), cols: HashMap::new(), top: max[1] + 5.0, bottom: min[1] - 5.0, radius: 0.0, reached: 0 };
        nav.sample_all(&mut c, min, max).into_iter().map(|p| p.map(|v| (v * 10.0).round() / 10.0)).collect()
    }

    pub fn report(&self, g: &Game) -> serde_json::Value {
        let marks: Vec<_> = (0..g.items.records.len())
            .filter(|&s| matches!(item_kind(g, s).0, 1 | 5 | 7 | 9 | 10) && (item_kind(g, s).0 != 1 || item_kind(g, s).1 == 2))
            .map(|s| serde_json::json!([s, item_kind(g, s), g.items.records[s].v3(0x40), g.items.records[s].active()]))
            .collect();
        let all_items: Vec<_> = (0..g.items.records.len())
            .map(|s| serde_json::json!([s, item_kind(g, s), String::from_utf8_lossy(g.items.definitions[g.items.records[s].definition()].name()), g.items.records[s].v3(0x40), g.items.records[s].active()]))
            .collect();
        let edges: usize = self.nav.adj.iter().map(Vec::len).sum();
        let triggers: Vec<_> = (0..g.items.records.len())
            .filter(|&s| item_kind(g, s).0 == 5)
            .map(|s| {
                let r = &g.items.records[s];
                let bound = r.u32_(0xf0).checked_sub(1).map(|o| o as usize);
                serde_json::json!({"slot": s, "sub": item_kind(g, s).1, "name": String::from_utf8_lossy(g.items.definitions[r.definition()].name()),
                    "flags": format!("{:#x}", r.u16_(0xf4)), "bound": bound, "chain": r.u32_(0xf8).checked_sub(1), "state": r.i8_(0xde), "e1": r.u8_(0xe1),
                    "f6": r.i8_(0xf6), "pos": r.v3(0x40), "bound_byte16": bound.and_then(|o| g.items.objects.get(o)).map(|w| format!("{:#x}", w.byte_16)),
                    "bound_object": bound.map(|o| { let c = &g.collision.objects[o]; serde_json::json!({"position": c.position, "flags": format!("{:#x}", g.collision.runtime_flags[o]), "parent": c.parent, "triangles": c.triangle_count, "first": c.first_triangle, "node_y": g.collision.node_y[o], "moved": g.collision.object_moved(o), "matrix": g.collision.object_matrix(o).ok().map(|m| m[3])}) })})
            })
            .collect();
        let mut c = g.collision.clone();
        let explain = Nav::explain(&mut c, g.players[0].v3(0x50), 0.3, g.players[0].v3(0x50)[1] + 80.0, g.globals.world_bottom - 2.0);
        serde_json::json!({
            "explain": explain,
            "nav": self.nav.pts.iter().zip(&self.nav.adj).filter(|(_, a)| !a.is_empty()).map(|(p, _)| [(p[0] * 10.0).round() / 10.0, (p[1] * 10.0).round() / 10.0, (p[2] * 10.0).round() / 10.0]).collect::<Vec<_>>(),
            "nav_edges": edges, "items": marks, "all_items": all_items, "triggers": triggers,
            // Each treasure's support object and its parent chain.
            "treasure_support": self.support_cache.iter().map(|(&t, &o)| {
                let mut chain = Vec::new();
                let mut x = o;
                for _ in 0..6 {
                    let Some(v) = x else { break };
                    chain.push(v);
                    x = g.collision.objects.get(v).and_then(|c| c.parent);
                }
                serde_json::json!([t, chain])
            }).collect::<Vec<_>>(),
            "critters": g.critters.iter().flatten().map(|c| serde_json::json!({"pos": c.rec.v3(0x40), "state": c.rec.i32_(8), "health": c.rec.f32_(0x4f4)})).collect::<Vec<_>>(),
            "scope": "TRAVERSAL BOT (gdl_tools bot.rs): port 0 driven through the ordinary pad path; health assist is a DEBUG shortcut",
            "start": self.start, "start_level": self.start_level, "player_radius": g.players[0].f32_(0x828), "collision_offset": g.players[0].f32_(0x820), "collision_height": g.players[0].f32_(0x82c), "nav_nodes": self.nav.pts.len(), "reachable_nodes": self.nav.reached,
            "static_exit_reachable": self.static_exit_reachable, "exit_update": self.exit_update, "exit_wave": self.exit_wave.map(|w| format!("{w:#x}")), "exit_state": self.exit_state,
            "health_assists": self.assists, "hints": self.hints.len(), "hints_done": self.hint_at, "replans": self.replans, "rebuilds": self.rebuilds, "holds": {"stage_or_camera": self.holds[0], "exit_portal": self.holds[1], "riding": self.holds[2], "cam_wait_now": g.globals.cam_wait, "start_of_stage_now": g.globals.start_of_stage}, "given_up": self.given_up.iter().collect::<Vec<_>>(),
            "log": self.log,
            "route": self.route.iter().map(|&(f, q)| [f as f32, (q[0] * 10.0).round() / 10.0, (q[1] * 10.0).round() / 10.0, (q[2] * 10.0).round() / 10.0]).collect::<Vec<_>>(),
        })
    }
}
