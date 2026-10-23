//! Traversal bot for game-run (M14/M15/M16 evidence tooling; not game logic
//! and not a fidelity input). It drives port 0 through the ordinary pad path
//! (ReadControls → PlayerControls → do_players) with the left stick, so every
//! step the player takes is the native PlayerMotion's.
//!
//! Navigation: a graph sampled from a copy of the level's collision as the
//! game holds it (runtime flags and dynamic matrices included) — columns of
//! CELL × CELL, floors found by FloorCollide sweeps from the top of the world
//! down, neighbours joined when the step up is at most STEP_UP, the drop at
//! most DROP, a floor lies under the midpoint and an EnemyWallCollide-style
//! sweep (wall normals, object mask 0x13a) at a metre above the higher floor
//! hits nothing. The graph is grown breadth-first from the player, so it is
//! the statically reachable area; items (gates, barrels) are not in it.
//!
//! Goals, re-chosen whenever a goal is reached or the bot is stuck: the
//! nearest reachable key (kind 1 subtype 2), then the nearest exit (kind 9),
//! then unvisited triggers (kind 5); a gate (kind 7) opens when touched with
//! a key. Stuck (less than half a unit in STUCK updates): the edge in use is
//! blocked and the bot throws for a while (barrels, enemies). Enemies within
//! ATTACK_RANGE are attacked with throws (cross) on alternate updates.
//! Health assist (labelled DEBUG in the report): health is restored to its
//! start value when it falls below 40 % of it.
use gdl_core::collision::{CollisionWorld, Query};
use gdl_core::game::Game;
use gdl_core::game::pad::PadStatus;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};

type V3 = [f32; 3];
type Place = (i32, i32, i32);

const CELL: f32 = 1.5;
const STEP_UP: f32 = 2.0;
const DROP: f32 = 12.0;
const STUCK: usize = 45;
const ATTACK_RANGE: f32 = 5.0;
const NODE_LIMIT: usize = 400_000;
const HEALTH: usize = 0x1a10;
const KEYS: usize = 0x1a14;

pub struct Nav {
    pub pts: Vec<V3>,
    pub adj: Vec<Vec<usize>>,
    /// Edges the player-like wall test rejects (kept at a higher cost).
    hard: HashSet<(usize, usize)>,
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

fn dist_xz(a: V3, b: V3) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl Nav {
    /// The graph reachable from `start` in `c` (a copy of the game's collision).
    /// The graph reachable from `seeds` (the player, plus the places the
    /// previous graph reached, so an isolated spot on a lift does not lose
    /// the known level).
    pub fn build(c: &mut CollisionWorld, seeds: &[V3], bottom: f32, top: f32, radius: f32) -> Nav {
        let mut nav = Nav { pts: Vec::new(), adj: Vec::new(), hard: HashSet::new(), cols: HashMap::new(), top, bottom, radius, reached: 0 };
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
                for m in nav.column(c, k2) {
                    if seen.len() <= m {
                        seen.resize(nav.pts.len(), false);
                    }
                    if let Some(strict) = Self::edge_class(c, p, nav.pts[m], nav.radius) {
                        nav.adj[n].push(m);
                        if !strict {
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
        for _ in 0..16 {
            let depth = y - self.bottom + 2.0;
            if depth <= 0.0 {
                break;
            }
            let Ok((hit, _)) = c.floor_collide(0.3, 0.0, -depth, [x, y, z], true, 0) else { break };
            let Some(h) = hit.solid else { break };
            let fy = h.position[1];
            if out.last().is_none_or(|&l: &usize| 2.0 <= self.pts[l][1] - fy) {
                self.pts.push([x, fy, z]);
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
        if STEP_UP < dy || dy < -DROP {
            return None;
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
        if !matches!(loose, Ok((ref w, _)) if w.solid.is_none()) {
            return None;
        }
        let mid = [(a[0] + b[0]) * 0.5, a[1].max(b[1]) + 1.0, (a[2] + b[2]) * 0.5];
        let depth = mid[1] - a[1].min(b[1]) + 1.0;
        if !matches!(c.floor_collide(0.3, 0.0, -depth, mid, true, 0), Ok((f, _)) if f.solid.is_some()) {
            return None;
        }
        Some(Self::edge_ok(c, a, b, radius))
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
        matches!(c.floor_collide(0.3, 0.0, -depth, mid, true, 0), Ok((f, _)) if f.solid.is_some())
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
        (k.0, p[1].round() as i32, k.1)
    }

    /// Why each edge from the node at `p` to its neighbour columns passes or fails (diagnostics).
    pub fn explain(c: &mut CollisionWorld, p: V3, radius: f32, top: f32, bottom: f32) -> Vec<serde_json::Value> {
        let mut nav = Nav { pts: Vec::new(), adj: Vec::new(), hard: HashSet::new(), cols: HashMap::new(), top, bottom, radius, reached: 0 };
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
                let floor = matches!(c.floor_collide(0.3, 0.0, -depth, mid, true, 0), Ok((f, _)) if f.solid.is_some());
                out.push(serde_json::json!({"from": a, "to": b, "dy": dy, "wall": hit, "mid_floor": floor, "class": Self::edge_class(c, a, b, radius)}));
            }
        }
        out
    }

    /// Dijkstra from `from` to the first node `goal` accepts, avoiding `blocked` edges.
    pub fn path(&self, from: usize, goal: &dyn Fn(usize) -> bool, blocked: &HashSet<(Place, Place)>, avoid: &HashSet<usize>) -> Option<Vec<usize>> {
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
                let k = if self.hard.contains(&(n, m)) { 4.0 } else { 1.0 };
                // Drops lose height that may have taken lifts to gain.
                let drop = if self.pts[m][1] < self.pts[n][1] - 2.0 { 15.0 } else { 0.0 };
                let drop = drop + if avoid.contains(&m) { 25.0 } else { 0.0 };
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Goal {
    Item(usize),
    /// A reachable graph node in a column the player has not stood in.
    Explore(usize),
}

pub struct Bot {
    pub nav: Nav,
    collision_at_build: u64,
    path: Vec<usize>,
    at: usize,
    goal: Option<Goal>,
    blocked: HashSet<(Place, Place)>,
    /// Update of the last rebuild for want of a goal (rate limit).
    idle_rebuild_at: usize,
    given_up: HashSet<usize>,
    visited_triggers: HashSet<usize>,
    /// Columns the player has stood in (exploration).
    visited_cols: HashSet<(i32, i32)>,
    progress: (usize, V3),
    attack_until: usize,
    start_health: f32,
    /// Standing on a reached trigger: (since, y then, last y change).
    wait: Option<(usize, f32, usize)>,
    pub assists: u32,
    pub log: Vec<serde_json::Value>,
    pub exit_update: Option<usize>,
    pub exit_wave: Option<i32>,
    pub static_exit_reachable: bool,
    pub start: V3,
    replans: usize,
    rebuilds: usize,
    force_rebuild: bool,
    last_y: f32,
    /// Followers' (waypoint index, path) by player.
    followers: [(usize, Vec<usize>); 4],
    backoff: Option<(usize, V3)>,
    attack_at: Option<V3>,
    bad_nodes: HashSet<Place>,
    last_floor: Option<usize>,
    riding: usize,
}

fn item_kind(g: &Game, slot: usize) -> (i32, i32) {
    let r = &g.items.records[slot];
    let d = &g.items.definitions[r.definition()];
    (i32::from(d.kind()), i32::from(d.subtype()))
}

impl Bot {
    pub fn new(g: &Game) -> Bot {
        let start = g.players[0].v3(0x50);
        let mut b = Bot {
            nav: Nav { pts: Vec::new(), adj: Vec::new(), hard: HashSet::new(), cols: HashMap::new(), top: 0.0, bottom: 0.0, radius: 0.0, reached: 0 },
            collision_at_build: 0,
            path: Vec::new(),
            at: 0,
            goal: None,
            blocked: HashSet::new(),
            idle_rebuild_at: 0,
            given_up: HashSet::new(),
            visited_triggers: HashSet::new(),
            visited_cols: HashSet::new(),
            progress: (0, start),
            attack_until: 0,
            start_health: g.players[0].f32_(HEALTH),
            wait: None,
            assists: 0,
            log: Vec::new(),
            exit_update: None,
            exit_wave: None,
            static_exit_reachable: false,
            start,
            replans: 0,
            rebuilds: 0,
            force_rebuild: false,
            last_y: f32::NAN,
            followers: Default::default(),
            backoff: None,
            attack_at: None,
            bad_nodes: HashSet::new(),
            last_floor: None,
            riding: 0,
        };
        b.rebuild(g);
        let exits: Vec<usize> = (0..g.items.records.len()).filter(|&s| g.items.records[s].active() && item_kind(g, s).0 == 9).collect();
        let targets = b.item_nodes(g, &exits);
        b.static_exit_reachable = !targets.is_empty();
        b
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
        for v in &c.node_rot {
            v.map(|m| m.map(|r| r.map(f32::to_bits))).hash(&mut h);
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
        self.nav = Nav::build(&mut c, &seeds, bottom, top, 0.3);
        self.collision_at_build = Self::collision_state(g);
    }

    /// Reachable graph nodes within 2 units of each item (slot, node).
    fn item_nodes(&self, g: &Game, slots: &[usize]) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for &s in slots {
            let q = g.items.records[s].v3(0x40);
            let k = key(q[0], q[2]);
            let mut best: Option<(f32, usize)> = None;
            for dx in -1..=1 {
                for dz in -1..=1 {
                    for &n in self.nav.cols.get(&(k.0 + dx, k.1 + dz)).map(Vec::as_slice).unwrap_or(&[]) {
                        let p = self.nav.pts[n];
                        if self.nav.adj[n].is_empty() || !(-3.0..=3.0).contains(&(q[1] - p[1])) {
                            continue;
                        }
                        let d = dist_xz(p, q);
                        if d < 2.5 && best.is_none_or(|b| d < b.0) {
                            best = Some((d, n));
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

    /// Picks the next goal. The graph is regrown from the player only when
    /// needed (BFS with collision sweeps is the expensive part): off the
    /// graph, after a trigger wait, or when no goal is reachable and the
    /// collision state changed since the last build.
    fn choose(&mut self, f: usize, g: &mut Game) {
        self.replans += 1;
        let p = g.players[0].v3(0x50);
        if self.force_rebuild || self.nav.locate_cached(p, &HashSet::new()).is_none() {
            self.force_rebuild = false;
            self.rebuild(g);
        }
        if self.plan(f, g) {
            return;
        }
        if Self::collision_state(g) != self.collision_at_build && (self.idle_rebuild_at == 0 || 300 <= f - self.idle_rebuild_at) {
            self.idle_rebuild_at = f.max(1);
            self.rebuild(g);
            if self.plan(f, g) {
                return;
            }
        }
        self.goal = None;
        self.path.clear();
    }

    /// Graph nodes beside items that block the way (chests, barrels and
    /// obstacles; gates while no key is held): routes avoid them when they can.
    fn obstacle_nodes(&self, g: &Game) -> HashSet<usize> {
        let keys = g.players[0].i32_(KEYS);
        let mut out = HashSet::new();
        for s in 0..g.items.records.len() {
            let r = &g.items.records[s];
            let (k, _) = item_kind(g, s);
            if !r.active() || !(k == 2 || k == 10 || (k == 7 && keys < 1)) {
                continue;
            }
            let q = r.v3(0x40);
            let reach = r.f32_(0xe8).clamp(0.5, 4.0) + 1.0;
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
        let avoid = self.obstacle_nodes(g);
        let p = g.players[0].v3(0x50);
        let Some(here) = self.nav.locate_cached(p, &self.bad_nodes).or_else(|| self.nav.locate_cached(p, &HashSet::new())) else {
            return false;
        };
        let active = |g: &Game, want: &dyn Fn(i32, i32) -> bool| -> Vec<usize> {
            // +0xe1: items for more players than are in the game (ProcessItems skips them).
            (0..g.items.records.len()).filter(|&s| g.items.records[s].active() && g.items.records[s].u8_(0xe1) == 0 && { let (k, t) = item_kind(g, s); want(k, t) }).collect()
        };
        let keys = active(g, &|k, t| k == 1 && t == 2);
        // Regular exits first; secret exits (subtype 0x32, to the treasure
        // rooms) only when no regular exit is reachable.
        let exits = active(g, &|k, t| k == 9 && t != 0x32);
        let secret = active(g, &|k, t| k == 9 && t == 0x32);
        // Triggers that switch their bound object on (+0xf4 bit 0 only
        // switches it off) and were not used yet.
        let triggers: Vec<usize> = active(g, &|k, _| k == 5)
            .into_iter()
            .filter(|&s| !self.visited_triggers.contains(&s) && g.items.records[s].u16_(0xf4) & 1 == 0)
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
        let order = if tower_crystals { [near, Vec::new(), Vec::new()] } else { [exits, near, secret] };
        for list in order {
            let list: Vec<usize> = list.into_iter().filter(|s| !self.given_up.contains(s)).collect();
            let targets = self.item_nodes(g, &list);
            if targets.is_empty() {
                continue;
            }
            let goal_nodes: HashMap<usize, usize> = targets.iter().map(|&(s, n)| (n, s)).collect();
            if let Some(path) = self.nav.path(here, &|n| goal_nodes.contains_key(&n), &self.blocked, &avoid) {
                let slot = goal_nodes[path.last().unwrap()];
                self.goal = Some(Goal::Item(slot));
                self.path = path;
                self.at = 0;
                self.log.push(serde_json::json!({"update": f, "goal": slot, "kind": item_kind(g, slot), "nodes": self.path.len(), "from": p}));
                return true;
            }
        }
        // A boss or other critter alive: go to it (the attack logic fights it).
        let foes: Vec<V3> = g.critters.iter().flatten().filter(|c| 0.0 < c.rec.f32_(0x4f4)).map(|c| c.rec.v3(0x40)).collect();
        if !foes.is_empty() {
            // Within 4 units of a foe, else the reachable node nearest to one.
            let close = (0..self.nav.pts.len())
                .filter(|&n| !self.nav.adj[n].is_empty())
                .min_by(|&a, &b| {
                    let d = |n: usize| foes.iter().map(|q| dist_xz(self.nav.pts[n], *q)).fold(f32::MAX, f32::min);
                    d(a).total_cmp(&d(b))
                });
            let near = |n: usize| foes.iter().any(|q| dist_xz(self.nav.pts[n], *q) < 4.0) || Some(n) == close;
            if let Some(path) = self.nav.path(here, &near, &self.blocked, &avoid) {
                let n = *path.last().unwrap();
                self.goal = Some(Goal::Explore(n));
                self.path = path;
                self.at = 0;
                self.log.push(serde_json::json!({"update": f, "foe": self.nav.pts[n], "nodes": self.path.len()}));
                return true;
            }
        }
        // Nothing to collect or switch: explore the nearest column not stood in.
        let unvisited = |n: usize| {
            let q = self.nav.pts[n];
            !self.nav.adj[n].is_empty() && !self.visited_cols.contains(&key(q[0], q[2])) && 3.0 < dist_xz(q, p)
        };
        if let Some(path) = self.nav.path(here, &unvisited, &self.blocked, &avoid) {
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
        }
        if self.exit_update.is_some() || g.players[0].i32_(0xfc) != 1 {
            return pad;
        }
        let health = g.players[0].f32_(HEALTH);
        if health < self.start_health * 0.4 {
            g.players[0].set_f32(HEALTH, self.start_health);
            self.assists += 1;
        }
        // The stage-start sequence and camera waits hold the player: no
        // input and no stuck test until they end.
        if g.globals.start_of_stage != 0 || g.globals.cam_wait != 0 {
            self.progress = (f, g.players[0].v3(0x50));
            return pad;
        }
        // Goal bookkeeping.
        match self.goal {
            Some(Goal::Item(s)) => {
                let (k, _) = item_kind(g, s);
                // A trigger counts once the bot stands within 2.5 of it or the
                // trigger reports a player on it (+0xe2).
                let r = &g.items.records[s];
                let reached = !r.active() || (k == 5 && (dist_xz(g.players[0].v3(0x50), r.v3(0x40)) < 2.5 || r.u8_(0xe2) & 1 != 0));
                if reached {
                    self.log.push(serde_json::json!({"update": f, "reached": s, "keys": g.players[0].i32_(KEYS)}));
                    self.bad_nodes.clear();
                    if k == 5 {
                        // Stay on the pad while its platform moves (lifts,
                        // bridges): until the height has been still for 30
                        // updates after moving, 90 updates without moving,
                        // or 600 updates.
                        self.visited_triggers.insert(s);
                        let y = g.players[0].v3(0x50)[1];
                        self.wait = Some((f, y, f));
                        self.goal = None;
                        self.path.clear();
                    } else {
                        self.choose(f, g);
                    }
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
        let p = g.players[0].v3(0x50);
        self.visited_cols.insert(key(p[0], p[2]));
        if let Some((since, y0, moved)) = self.wait {
            let moved = if 0.05 < (p[1] - y0).abs() { f } else { moved };
            let done = if moved == since { 90 <= f - since } else { 30 <= f - moved } || 600 <= f - since;
            if done {
                self.wait = None;
                self.force_rebuild = true;
                self.log.push(serde_json::json!({"update": f, "waited": f - since, "rose": p[1] - self.start[1]}));
                self.progress = (f, p);
                self.choose(f, g);
            } else {
                self.wait = Some((since, p[1], moved));
                return pad;
            }
        }
        // Stuck detection.
        if STUCK <= f - self.progress.0 {
            if dist_xz(p, self.progress.1) < 0.5 {
                if let (Some(&a), Some(&b)) = (self.path.get(self.at.saturating_sub(1)), self.path.get(self.at)) {
                    let (pa, pb) = (self.nav.place(a), self.nav.place(b));
                    self.blocked.insert((pa, pb));
                    self.blocked.insert((pb, pa));
                    // The same obstacle along a ledge or wall: every penalised
                    // edge within 3 units heading the same way.
                    let (qa, qb) = (self.nav.pts[a], self.nav.pts[b]);
                    let dir = [qb[0] - qa[0], qb[2] - qa[2]];
                    let near: Vec<(usize, usize)> = self.nav.hard.iter().copied().filter(|&(n, m)| {
                        let (u, v) = (self.nav.pts[n], self.nav.pts[m]);
                        dist_xz(u, p) < 3.0 && (u[1] - p[1]).abs() < 1.5 && 0.5 < ((v[0] - u[0]) * dir[0] + (v[2] - u[2]) * dir[1]) / (dist_xz(u, v) * (dir[0].hypot(dir[1])).max(1e-3))
                    }).collect();
                    for (n, m) in near {
                        let (pn, pm) = (self.nav.place(n), self.nav.place(m));
                        self.blocked.insert((pn, pm));
                    }
                }
                self.attack_until = f + 40;
                // Back off briefly (away from the blocked waypoint) before the new route.
                if let Some(&b) = self.path.get(self.at) {
                    let q = self.nav.pts[b];
                    self.backoff = Some((f + 10, [2.0 * p[0] - q[0], p[1], 2.0 * p[2] - q[2]]));
                }
                // A container or obstacle item in the way (barrels, chests:
                // kinds 2 and 10): face it while attacking.
                self.attack_at = (0..g.items.records.len())
                    .filter(|&s| g.items.records[s].active() && matches!(item_kind(g, s).0, 2 | 10) && dist_xz(g.items.records[s].v3(0x40), p) < 3.5)
                    .map(|s| g.items.records[s].v3(0x40))
                    .min_by(|a, b| dist_xz(*a, p).total_cmp(&dist_xz(*b, p)));
                // No step taken from the located node: the node itself is out
                // of reach from here (an item between); locate elsewhere.
                if self.at == 0
                    && let Some(&n) = self.path.first()
                {
                    self.bad_nodes.insert(self.nav.place(n));
                }
                if let Some(Goal::Item(s)) = self.goal
                    && 40 < self.log.iter().filter(|e| e["stuck_goal"] == s).count()
                {
                    self.given_up.insert(s);
                }
                let enemies: Vec<_> = g.enemies.iter().enumerate().filter(|(_, e)| e.i32_(0xc8) != 0 && dist_xz(e.v3(0x40), p) < 4.0).map(|(k, e)| serde_json::json!([k, e.i32_(0), e.f32_(0x214)])).collect();
                let items: Vec<_> = (0..g.items.records.len()).filter(|&s| g.items.records[s].active() && dist_xz(g.items.records[s].v3(0x40), p) < 4.0).map(|s| serde_json::json!([s, item_kind(g, s), String::from_utf8_lossy(g.items.definitions[g.items.records[s].definition()].name())])).collect();
                let next = self.path.get(self.at).map(|&n| self.nav.pts[n]);
                self.log.push(serde_json::json!({"update": f, "stuck": p, "next": next, "enemies": enemies, "items": items, "stuck_goal": match self.goal { Some(Goal::Item(s)) => s, _ => usize::MAX }}));
                self.choose(f, g);
            }
            self.progress = (f, p);
        }
        // Riding a lift or falling: no stick until the height settles.
        // The floor object (+0x89c) moving: its node height changed.
        let floor = g.players[0].u32_(0x89c).checked_sub(1).map(|o| o as usize);
        let fy = floor.and_then(|o| g.collision.object_matrix(o).ok()).map_or(f32::NAN, |m| m[3][1]);
        let riding = floor == self.last_floor && 0.01 < (fy - self.last_y).abs();
        self.last_y = fy;
        self.last_floor = floor;
        self.riding = if riding { self.riding + 1 } else { 0 };
        if riding && self.riding < 300 {
            self.progress = (f, p);
            return pad;
        }
        if let Some((until, away)) = self.backoff {
            if f < until {
                return Self::steer(pad, g, p, away);
            }
            self.backoff = None;
        }
        // Waypoint following.
        while let Some(&n) = self.path.get(self.at) {
            let w = self.nav.pts[n];
            if dist_xz(p, w) < 0.9 && (p[1] - w[1]).abs() < 2.5 {
                self.at += 1;
            } else {
                break;
            }
        }
        let target = match (self.path.get(self.at), self.goal) {
            (Some(&n), _) => Some(self.nav.pts[n]),
            (None, Some(Goal::Item(s))) => Some(g.items.records[s].v3(0x40)),
            (None, Some(Goal::Explore(n))) => self.nav.pts.get(n).copied(),
            _ => None,
        };
        // The nearest live enemy in range: face it and hold attack (cross,
        // DMotion 0xf, acts on the level); stuck: hold attack along the path.
        // A boss (critter kind 4) in range: fight from a distance — hold
        // attack facing it, step back inside 6 units.
        let boss = usize::try_from(g.boss_crit_inst - 1).ok().and_then(|b| g.critters.get(b)).and_then(Option::as_ref).filter(|c| 0.0 < c.rec.f32_(0x4f4)).map(|c| c.rec.v3(0x40));
        if let Some(bp) = boss.filter(|bp| dist_xz(*bp, p) < 18.0) {
            pad.press(0xd);
            if dist_xz(bp, p) < 6.0 && (f / 20).is_multiple_of(2) {
                return Self::steer(pad, g, p, [2.0 * p[0] - bp[0], p[1], 2.0 * p[2] - bp[2]]);
            }
            return Self::steer(pad, g, p, bp);
        }
        let enemy = Self::nearest_foe(g, p, ATTACK_RANGE);
        // Attack in windows (12 updates fighting, 12 moving): holding attack
        // keeps the player in its attack actions, and generators refill.
        let fighting = (f / 12).is_multiple_of(2);
        let target = match enemy.filter(|_| fighting) {
            Some((_, at)) => {
                pad.press(0xd);
                Some(at)
            }
            None => {
                if f < self.attack_until {
                    pad.press(0xd);
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

    /// The nearest live enemy, or critter (bosses, statues' creatures:
    /// +0x4f4 health), within `range` and 3 units of height.
    fn nearest_foe(g: &Game, p: V3, range: f32) -> Option<(f32, V3)> {
        let enemies = g
            .enemies
            .iter()
            .filter(|e| e.i32_(0xc8) != 0 && e.f32_(0x214) > 0.0 && (e.v3(0x40)[1] - p[1]).abs() < 3.0)
            .map(|e| (dist_xz(e.v3(0x40), p), e.v3(0x40)));
        let critters = g.critters.iter().flatten().filter(|c| 0.0 < c.rec.f32_(0x4f4)).map(|c| (dist_xz(c.rec.v3(0x40), p), c.rec.v3(0x40)));
        enemies.chain(critters).filter(|(d, _)| *d < range).min_by(|a, b| a.0.total_cmp(&b.0))
    }

    /// The left stick toward `t`: PlayerMotion's heading is the camera yaw
    /// (atan2(camera_90, camera_98) + π) plus the stick angle
    /// (calc_analog_stick: atan2(left − right, down − up)).
    fn steer(mut pad: PadStatus, g: &Game, p: V3, t: V3) -> PadStatus {
        let (dx, dz) = (t[0] - p[0], t[2] - p[2]);
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
        let health = g.players[k].f32_(HEALTH);
        if health < self.start_health * 0.4 {
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
        if dist_xz(p, lead) <= 5.0 {
            return pad;
        }
        // A path to the leader on the leader's graph, renewed every 30 updates.
        let st = &mut self.followers[k];
        if st.1.is_empty() || f.is_multiple_of(30) {
            let none = HashSet::new();
            let to = self.nav.locate_cached(lead, &none);
            st.1 = match (self.nav.locate_cached(p, &none), to) {
                (Some(a), Some(b)) => self.nav.path(a, &|n| n == b, &self.blocked, &HashSet::new()).unwrap_or_default(),
                _ => Vec::new(),
            };
            st.0 = 0;
        }
        while let Some(&n) = st.1.get(st.0) {
            if dist_xz(p, self.nav.pts[n]) < 1.2 {
                st.0 += 1;
            } else {
                break;
            }
        }
        let t = st.1.get(st.0).map(|&n| self.nav.pts[n]).unwrap_or(lead);
        Self::steer(pad, g, p, t)
    }

    /// All floors of the level at the current collision state (bot=2).
    pub fn floors(&mut self, g: &Game, min: V3, max: V3) -> Vec<V3> {
        let mut c = g.collision.clone();
        let mut nav = Nav { pts: Vec::new(), adj: Vec::new(), hard: HashSet::new(), cols: HashMap::new(), top: max[1] + 5.0, bottom: min[1] - 5.0, radius: 0.0, reached: 0 };
        nav.sample_all(&mut c, min, max).into_iter().map(|p| p.map(|v| (v * 10.0).round() / 10.0)).collect()
    }

    pub fn report(&self, g: &Game) -> serde_json::Value {
        let marks: Vec<_> = (0..g.items.records.len())
            .filter(|&s| matches!(item_kind(g, s).0, 1 | 5 | 7 | 9 | 10) && (item_kind(g, s).0 != 1 || item_kind(g, s).1 == 2))
            .map(|s| serde_json::json!([s, item_kind(g, s), g.items.records[s].v3(0x40), g.items.records[s].active()]))
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
            "nav_edges": edges, "items": marks, "triggers": triggers,
            "critters": g.critters.iter().flatten().map(|c| serde_json::json!({"pos": c.rec.v3(0x40), "state": c.rec.i32_(8), "health": c.rec.f32_(0x4f4)})).collect::<Vec<_>>(),
            "scope": "TRAVERSAL BOT (gdl_tools bot.rs): port 0 driven through the ordinary pad path; health assist is a DEBUG shortcut",
            "start": self.start, "player_radius": g.players[0].f32_(0x828), "nav_nodes": self.nav.pts.len(), "reachable_nodes": self.nav.reached,
            "static_exit_reachable": self.static_exit_reachable, "exit_update": self.exit_update, "exit_wave": self.exit_wave.map(|w| format!("{w:#x}")),
            "health_assists": self.assists, "replans": self.replans, "rebuilds": self.rebuilds, "given_up": self.given_up.iter().collect::<Vec<_>>(),
            "log": self.log,
        })
    }
}
