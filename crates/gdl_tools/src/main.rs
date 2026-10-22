use gdl_formats::texdef::TextureTable;
use gdl_formats::wad::{Entry, Limits, Wad};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize)]
struct Outcome {
    #[serde(flatten)]
    entry: Entry,
    identity: String,
    stored_sha256: String,
    decoded_sha256: Option<String>,
    output_file: Option<String>,
    error: Option<String>,
}
#[derive(Serialize)]
struct Manifest {
    schema_version: u32,
    converter_version: &'static str,
    source_sha256: String,
    declared_decoded_bytes: u64,
    duplicate_hashes: std::collections::BTreeMap<u32, Vec<u32>>,
    entries: Vec<Outcome>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AnimationSampleRequest {
    tree: usize,
    node: u32,
    sequence: u32,
    frame: f32,
    transition: Option<TransitionSample>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransitionSample {
    from: [f32; 9],
    fraction: f32,
    mirror: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClockRequest {
    state: gdl_core::animation_clock::ClockState,
    now: f32,
    frames: i16,
    anim_time_scale: f32,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ClockStartRequest {
    state: gdl_core::animation_clock::ClockState,
    input: gdl_core::animation_clock::ClockInit,
    evaluate: bool,
    sampled: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AnimationSelectionRequest {
    state: gdl_core::animation_clock::ClockState,
    current: gdl_core::animation_clock::ClipTiming,
    requested: gdl_core::animation_clock::ClipTiming,
    input: gdl_core::animation_clock::SelectionInput,
    sampled: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlayerAnimationRequest {
    state: gdl_core::animation_clock::ClockState,
    context: gdl_core::player_animation::ActionContext,
    action_map: Vec<i32>,
    now: f32,
    anim_time_scale: f32,
    sampled: bool,
    events: Option<gdl_core::player_animation::ActionEventInput>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorldMatrixRequest {
    leaf: gdl_core::world_transform::MatrixBox,
    parents: Vec<gdl_core::world_transform::MatrixBox>,
    offset: Option<[f32; 3]>,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn bounded_read(path: &Path, limit: u64) -> Result<Vec<u8>, Box<dyn Error>> {
    use std::io::Read;
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > limit {
        return Err("input exceeds size policy".into());
    }
    let mut data = Vec::new();
    file.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        return Err("input grew beyond size policy".into());
    }
    Ok(data)
}

/// The reference Items array accumulated from reference_enemies.py's
/// delta-coded `items_delta` (the first capture holds every record).
#[derive(Default)]
struct ReferenceItems(Vec<Vec<u8>>);

impl ReferenceItems {
    fn apply(&mut self, sample: &serde_json::Value) -> Result<bool, Box<dyn Error>> {
        let Some(delta) = sample["enemies"]["items_delta"].as_object() else { return Ok(false) };
        let count = sample["enemies"]["NumItems"].as_u64().ok_or("NumItems")? as usize;
        self.0.resize(count, vec![0; 0x110]);
        for (slot, hex) in delta {
            let slot: usize = slot.parse()?;
            let s = hex.as_str().ok_or("item hex")?;
            let raw = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16)).collect::<Result<Vec<u8>, _>>()?;
            *self.0.get_mut(slot).ok_or("item slot beyond NumItems")? = raw;
        }
        Ok(true)
    }

    /// A reference record in the export_live_items.py convention: +0x00
    /// definition pointer → index (WorldInfo +0x68, 0x50 bytes), kinds 5/12
    /// +0xf0 → WORLDS object index + 1 (WorldInfo +0x04, 0x3c bytes), kinds
    /// 5/11 +0xf8 → item slot + 1, kinds 2/13 +0xf8 cleared.
    fn native(raw: &[u8], worldinfo: &[u8], items_base: u32, kind_of: &dyn Fn(usize) -> i32) -> [u8; 0x110] {
        let word = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let (objects, defs) = (word(worldinfo, 4), word(worldinfo, 0x68));
        let mut new = [0u8; 0x110];
        new.copy_from_slice(&raw[..0x110]);
        let ptr = word(raw, 0);
        let def = if ptr == 0 { 0 } else { ptr.wrapping_sub(defs) / 0x50 };
        new[0..4].copy_from_slice(&def.to_le_bytes());
        let kind = kind_of(def as usize);
        let index = |v: u32, base: u32, size: u32| if v == 0 { 0 } else { v.wrapping_sub(base) / size + 1 };
        if matches!(kind, 5 | 12) {
            new[0xf0..0xf4].copy_from_slice(&index(word(raw, 0xf0), objects, 0x3c).to_le_bytes());
        }
        if matches!(kind, 5 | 11) {
            new[0xf8..0xfc].copy_from_slice(&index(word(raw, 0xf8), items_base, 0x110).to_le_bytes());
        }
        if matches!(kind, 2 | 13) {
            new[0xf8..0xfc].fill(0);
        }
        new
    }

    /// Guest pointers in an item record (render node, atree and AnimInfo
    /// links); the +0xe6 dyngrid link is a halfword handled separately.
    const POINTERS: [usize; 7] = [0x70, 0x80, 0x84, 0x88, 0x8c, 0xc0, 0xc4];

    /// The +0x84 AnimInfo (the enemy layout: the player's +0x94 less 0x10).
    fn clock_of(r: &[u8]) -> gdl_core::animation_clock::ClockState {
        let h = |o: usize| u16::from_le_bytes([r[o], r[o + 1]]);
        let f = |o: usize| f32::from_le_bytes(r[o..o + 4].try_into().unwrap());
        gdl_core::animation_clock::ClockState {
            sequence: h(0x92),
            previous_sequence: h(0xa0),
            reset_byte: r[0x96],
            option_bits: r[0x97],
            enabled_word: h(0xa2),
            loop_word: h(0xb8),
            mode_bits: h(0xba),
            frame: f(0x9c),
            blend_fraction: f(0x98),
            start_time: f(0xa4),
            blend_end_time: f(0xa8),
            period_scale: f(0xb0),
            last_time: f(0xb4),
        }
    }

    fn write_clock(r: &mut [u8], c: &gdl_core::animation_clock::ClockState) {
        let mut h = |o: usize, v: u16| r[o..o + 2].copy_from_slice(&v.to_le_bytes());
        h(0x92, c.sequence);
        h(0xa0, c.previous_sequence);
        h(0xa2, c.enabled_word);
        h(0xb8, c.loop_word);
        h(0xba, c.mode_bits);
        r[0x96] = c.reset_byte;
        r[0x97] = c.option_bits;
        for (o, v) in [(0x9c, c.frame), (0x98, c.blend_fraction), (0xa4, c.start_time), (0xa8, c.blend_end_time), (0xb0, c.period_scale), (0xb4, c.last_time)] {
            r[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
    }

    /// Seed the native items (see `native`); the guest pointer words and the
    /// +0xe6 dyngrid link stay native, item animation clocks come from the
    /// +0x84 AnimInfo. `visible` supplies the +0xd8 0x4000 bit do_items
    /// writes before do_enemies.
    fn seed(&self, game: &mut gdl_core::game::Game, worldinfo: &[u8], items_base: u32, visible: Option<&ReferenceItems>) -> Result<(), Box<dyn Error>> {
        if self.0.len() != game.items.records.len() {
            return Err(format!("reference NumItems {} differs from native {}", self.0.len(), game.items.records.len()).into());
        }
        for (slot, raw) in self.0.iter().enumerate() {
            let defs = &game.items.definitions;
            let mut new = Self::native(raw, worldinfo, items_base, &|d| defs.get(d).map_or(-1, |d| d.kind()));
            if let Some(v) = visible.and_then(|v| v.0.get(slot)) {
                let d8 = (u16::from_le_bytes([new[0xd8], new[0xd9]]) & !0x4000) | (u16::from_le_bytes([v[0xd8], v[0xd9]]) & 0x4000);
                new[0xd8..0xda].copy_from_slice(&d8.to_le_bytes());
            }
            let rec = &mut game.items.records[slot];
            let reused = rec.definition() != u32::from_le_bytes(new[0..4].try_into().unwrap()) as usize;
            for o in Self::POINTERS {
                new[o..o + 4].copy_from_slice(&rec.0[o..o + 4]);
            }
            new[0xe6..0xe8].copy_from_slice(&rec.0[0xe6..0xe8]);
            rec.0 = new;
            // SetItemGeo ran when the traced atree (+0x80) is set; a slot
            // holding another definition natively, or with another geometry
            // decision, takes its tree's clock.
            let geo = u32::from_le_bytes(raw[0x80..0x84].try_into().unwrap()) != 0;
            if game.items.geo.len() <= slot {
                game.items.geo.resize(slot + 1, false);
            }
            if reused || game.items.geo[slot] != geo {
                game.items.geo[slot] = geo;
                game.attach_item_anim(slot).map_err(|e| e.0)?;
            }
            if let Some(Some(an)) = game.item_anims.get_mut(slot) {
                an.clock = Self::clock_of(&new);
                an.current = an.clock.sequence;
            }
        }
        Ok(())
    }

    /// Differing u32 words of the native Items array (native clocks written
    /// back to the AnimInfo) against these records; guest pointers and the
    /// +0xe6 link skipped.
    fn compare(&self, game: &gdl_core::game::Game, worldinfo: &[u8], items_base: u32) -> Vec<serde_json::Value> {
        let defs = &game.items.definitions;
        let mut out = Vec::new();
        for (slot, raw) in self.0.iter().enumerate() {
            let Some(rec) = game.items.records.get(slot) else {
                out.push(serde_json::json!({"item":slot,"offset":"missing"}));
                continue;
            };
            let mut n = rec.0;
            if let Some(Some(an)) = game.item_anims.get(slot) {
                Self::write_clock(&mut n, &an.clock);
            }
            let mut r = Self::native(raw, worldinfo, items_base, &|d| defs.get(d).map_or(-1, |d| d.kind()));
            // Kind 13's +0xfe is the audio driver's (AudioLOOP queue id,
            // negated while pending, or AudioFindTID's track mask; native 1
            // while playing): compared by sign.
            if defs.get(u32::from_le_bytes(r[0..4].try_into().unwrap()) as usize).is_some_and(|d| d.kind() == 13) {
                for b in [&mut n, &mut r] {
                    let v = i16::from_le_bytes([b[0xfe], b[0xff]]).signum();
                    b[0xfe..0x100].copy_from_slice(&v.to_le_bytes());
                }
            }
            for o in (0..0x110).step_by(4) {
                if Self::POINTERS.contains(&o) {
                    continue;
                }
                let mask = if o == 0xe4 { 0xffff } else { u32::MAX };
                let w = |b: &[u8]| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) & mask;
                if w(&n) != w(&r) {
                    out.push(serde_json::json!({"item":slot,"offset":format!("{o:#x}"),"native":format!("{:08x}", w(&n)),"reference":format!("{:08x}", w(&r))}));
                }
            }
        }
        out
    }
}

/// The reference WORLDS object records (60 bytes) accumulated from
/// reference_enemies.py's `objects_delta`.
#[derive(Default, Clone)]
struct ReferenceObjects(Vec<Vec<u8>>);

impl ReferenceObjects {
    fn apply(&mut self, sample: &serde_json::Value) -> Result<bool, Box<dyn Error>> {
        let Some(delta) = sample["enemies"]["objects_delta"].as_object() else { return Ok(false) };
        for (k, hex) in delta {
            let k: usize = k.parse()?;
            let s = hex.as_str().ok_or("object hex")?;
            let raw = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16)).collect::<Result<Vec<u8>, _>>()?;
            if self.0.len() <= k {
                self.0.resize(k + 1, vec![0; 60]);
            }
            self.0[k] = raw;
        }
        Ok(true)
    }

    /// The runtime words (+0x10 flags, +0x14, +0x16, +0x17, +0x35 exclusion).
    fn seed(&self, game: &mut gdl_core::game::Game) {
        for (k, r) in self.0.iter().enumerate() {
            let Some(o) = game.items.objects.get_mut(k) else { break };
            o.flags_10 = u32::from_le_bytes(r[0x10..0x14].try_into().unwrap());
            o.word_14 = u16::from_le_bytes([r[0x14], r[0x15]]);
            o.byte_16 = r[0x16];
            o.byte_17 = r[0x17];
            if let Some(x) = game.items.object_exclusion.get_mut(k) {
                *x = r[0x35] as i8;
            }
        }
    }
}

/// The reference Effects array (64 × 0xf0) accumulated from
/// reference_enemies.py's `effects_delta`, with the live slots' render nodes.
#[derive(Default, Clone)]
struct ReferenceEffects {
    slots: Vec<Vec<u8>>,
    nodes: std::collections::BTreeMap<usize, Vec<u8>>,
    num: usize,
    sfx_id: i32,
}

impl ReferenceEffects {
    /// Effect words that are guest pointers or presentation: +0x14 node
    /// (native: 1 while a node exists), +0x18..+0x27 tree and AnimInfo links,
    /// +0x58/+0x5c, +0xd4 streak, +0xe4 hit-target node, +0xec carried item
    /// (native: slot + 1).
    const POINTERS: [usize; 9] = [0x14, 0x18, 0x1c, 0x20, 0x24, 0x58, 0x5c, 0xd4, 0xe4];

    fn apply(&mut self, sample: &serde_json::Value) -> Result<bool, Box<dyn Error>> {
        let e = &sample["enemies"];
        let Some(delta) = e["effects_delta"].as_object() else { return Ok(false) };
        let hexv = |v: &serde_json::Value| -> Result<Vec<u8>, Box<dyn Error>> {
            let s = v.as_str().ok_or("effect hex")?;
            Ok((0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16)).collect::<Result<Vec<u8>, _>>()?)
        };
        self.slots.resize(64, vec![0; 0xf0]);
        for (slot, hex) in delta {
            let k: usize = slot.parse()?;
            *self.slots.get_mut(k).ok_or("effect slot")? = hexv(hex)?;
        }
        self.nodes.clear();
        if let Some(nodes) = e["effect_nodes"].as_object() {
            for (slot, n) in nodes {
                self.nodes.insert(slot.parse()?, hexv(&n["node"])?);
            }
        }
        self.num = e["NumEffects"].as_u64().ok_or("NumEffects")? as usize;
        self.sfx_id = e["SfxId"].as_i64().ok_or("SfxId")? as i32;
        Ok(true)
    }

    fn native(raw: &[u8], items_base: u32) -> Vec<u8> {
        let word = |o: usize| u32::from_le_bytes(raw[o..o + 4].try_into().unwrap());
        let mut r = raw[..0xf0].to_vec();
        let node = u32::from(word(0x14) != 0);
        for o in Self::POINTERS {
            r[o..o + 4].fill(0);
        }
        r[0x14..0x18].copy_from_slice(&node.to_le_bytes());
        let it = word(0xec);
        let slot = if it == 0 || it < items_base { 0 } else { (it - items_base) / 0x110 + 1 };
        r[0xec..0xf0].copy_from_slice(&slot.to_le_bytes());
        r
    }

    /// EffectNode from the MB node: rows +0x00/+0x10/+0x20, translation
    /// +0x30, scale vector +0x40 (uniform = y, the rest as stretch).
    /// Presentation-only fields take neutral values (HYPOTHESIS: the parent
    /// is the effect root; homing targets are not mapped).
    fn node(n: &[u8], atime: f32) -> gdl_core::game::effects::EffectNode {
        let f = |o: usize| f32::from_le_bytes(n[o..o + 4].try_into().unwrap());
        let v = |o: usize| [f(o), f(o + 4), f(o + 8)];
        let s = f(0x44);
        let stretch = if s != 0.0 { [f(0x40) / s, 1.0, f(0x48) / s] } else { [1.0; 3] };
        gdl_core::game::effects::EffectNode {
            matrix: [v(0), v(0x10), v(0x20)],
            position: v(0x30),
            scale: s,
            stretch,
            visual: gdl_core::game::effects::Visual::Header(0),
            start: atime,
            radius: 0.0,
            parent: None,
            target: None,
            tint: None,
            alpha: None,
            ambient: None,
            follow: None,
        }
    }

    /// A node parented to player 0's tree (node +0x74 within 0x400 of the
    /// player's +0x80 node, e.g. its child): the native EffectParent.
    fn player_parent(n: &[u8], player_node: u32) -> Option<gdl_core::game::effects::EffectParent> {
        let par = u32::from_le_bytes(n[0x74..0x78].try_into().unwrap());
        if player_node == 0 || par < player_node || player_node + 0x400 <= par {
            return None;
        }
        let f = |o: usize| f32::from_le_bytes(n[o..o + 4].try_into().unwrap());
        let v = |o: usize| [f(o), f(o + 4), f(o + 8)];
        Some(gdl_core::game::effects::EffectParent { player: 0, local: v(0x30), local_matrix: [v(0), v(0x10), v(0x20)] })
    }

    fn seed(&self, game: &mut gdl_core::game::Game, items_base: u32, atime: f32, player_node: u32) {
        for (k, raw) in self.slots.iter().enumerate().take(game.effects.slots.len()) {
            game.effects.slots[k] = gdl_core::game::record::Record(Self::native(raw, items_base));
            game.effects.nodes[k] = self.nodes.get(&k).map(|n| {
                let mut node = Self::node(n, atime);
                node.parent = Self::player_parent(n, player_node);
                node
            });
        }
        game.effects.num = self.num;
        game.effects.sfx_id = self.sfx_id;
    }

    /// Differing words of the native effects against these: records (the
    /// AnimInfo clock +0x28..+0x4f is the tree's presentation clock and is
    /// skipped) and the nodes' rows and translation.
    fn compare(&self, game: &gdl_core::game::Game, items_base: u32) -> Vec<serde_json::Value> {
        let mut out = Vec::new();
        if game.effects.num != self.num {
            out.push(serde_json::json!({"field":"NumEffects","native":game.effects.num,"reference":self.num}));
        }
        for (k, raw) in self.slots.iter().enumerate().take(game.effects.slots.len()) {
            let r = Self::native(raw, items_base);
            let n = &game.effects.slots[k].0;
            for o in (0..0xf0).step_by(4) {
                // The tree's AnimInfo clock (+0x1c..+0x57) is presentation
                // except the loop word +0x50 (native: looping).
                if (0x28..0x50).contains(&o) || o == 0x54 {
                    continue;
                }
                let mask = if o == 0x50 { 0xffff } else { u32::MAX };
                let w = |b: &[u8]| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) & mask;
                if w(n) != w(&r) {
                    out.push(serde_json::json!({"effect":k,"offset":format!("{o:#x}"),"native":format!("{:08x}", w(n)),"reference":format!("{:08x}", w(&r))}));
                }
            }
            match (game.effects.nodes[k].as_ref(), self.nodes.get(&k)) {
                (Some(nn), Some(rn)) => {
                    let rn = Self::node(rn, 0.0);
                    // A parented node: the traced matrix is the local one.
                    let (pos, m) = match nn.parent {
                        Some(par) => (par.local, par.local_matrix),
                        None => (nn.position, nn.matrix),
                    };
                    let pairs = [("position", pos, rn.position), ("row0", m[0], rn.matrix[0]), ("row1", m[1], rn.matrix[1]), ("row2", m[2], rn.matrix[2])];
                    for (name, a, b) in pairs {
                        if a.map(f32::to_bits) != b.map(f32::to_bits) {
                            out.push(serde_json::json!({"effect":k,"node":name,"native":a,"reference":b}));
                        }
                    }
                }
                (None, None) => {}
                (a, b) => out.push(serde_json::json!({"effect":k,"node":"presence","native":a.is_some(),"reference":b.is_some()})),
            }
        }
        out
    }
}

/// The reference CritterInsts (16 × 0xb70) accumulated from
/// reference_enemies.py's `critters_delta`, with NumCritterInsts,
/// BossCritInst and `critter_globals` (traces from trace-k2-newgame-v3).
#[derive(Default, Clone)]
struct ReferenceCritters {
    slots: Vec<Vec<u8>>,
    num: usize,
    boss: u32,
    globals: serde_json::Map<String, serde_json::Value>,
    /// Live instances' +0x70 nodes: local matrix (0x40 bytes), parent link
    /// (traces from trace-k2-newgame-v4) and CritterGetNode's tree node
    /// pointers by index (from trace-k2-newgame-v5).
    nodes: std::collections::BTreeMap<usize, (Vec<u8>, u32, Vec<u32>)>,
}

impl ReferenceCritters {
    /// CritterInsts (CritterEmptyInst 0x193ae4..0x193ae8).
    const BASE: u32 = 0x251750;
    /// The waypoint records FindClosestWaypoint walks (0x14fddc..0x14fde0,
    /// stride 0x80); native +0xb5c holds the index + 1.
    const WAYPOINTS: u32 = 0x23bd20;
    fn apply(&mut self, sample: &serde_json::Value) -> Result<bool, Box<dyn Error>> {
        let e = &sample["enemies"];
        let Some(delta) = e["critters_delta"].as_object() else { return Ok(false) };
        let hexv = |v: &serde_json::Value| -> Result<Vec<u8>, Box<dyn Error>> {
            let s = v.as_str().ok_or("critter hex")?;
            Ok((0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16)).collect::<Result<Vec<u8>, _>>()?)
        };
        self.slots.resize(16, vec![0; 0xb70]);
        for (slot, hex) in delta {
            let k: usize = slot.parse()?;
            *self.slots.get_mut(k).ok_or("critter slot")? = hexv(hex)?;
        }
        self.num = e["NumCritterInsts"].as_u64().ok_or("NumCritterInsts")? as usize;
        let b = hexv(&e["BossCritInst"])?;
        self.boss = u32::from_le_bytes(b[..4].try_into()?);
        self.globals = e["critter_globals"].as_object().cloned().unwrap_or_default();
        self.nodes.clear();
        if let Some(nodes) = e["critter_nodes"].as_object() {
            for (k, n) in nodes {
                let p = hexv(&n["parent"])?;
                let tree = match n.get("tree_nodes") {
                    Some(t) => hexv(t)?.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect(),
                    None => Vec::new(),
                };
                self.nodes.insert(k.parse()?, (hexv(&n["node"])?, u32::from_le_bytes(p[..4].try_into()?), tree));
            }
        }
        Ok(true)
    }

    fn global(&self, name: &str) -> Result<Vec<u8>, Box<dyn Error>> {
        let s = self.globals.get(name).and_then(|v| v.as_str()).ok_or_else(|| format!("critter global {name}"))?;
        Ok((0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16)).collect::<Result<Vec<u8>, _>>()?)
    }

    fn word(r: &[u8], o: usize) -> u32 {
        u32::from_le_bytes(r[o..o + 4].try_into().unwrap())
    }

    /// Stack residue: CritterGetSingleTargetPlayer copies its 0x30-byte
    /// target struct into +0x140, and CritterCalcTarget never writes the
    /// struct's +0x14/+0x18/+0x1c/+0x2c (no critter code reads them back;
    /// the 0x15c loads in CritterDamage/CritterDamagePlayer/CritterGolemAI
    /// are the global 0x22015c).
    const RESIDUE: [usize; 4] = [0x154, 0x158, 0x15c, 0x16c];

    fn live(&self, k: usize) -> bool {
        k < self.num && self.slots.get(k).is_some_and(|r| Self::word(r, 4) != 0)
    }

    /// Guest pointer words: TYPE +0x04, the critter node +0x70, the atree and
    /// AnimInfo links +0x80..+0x8c and +0xc0/+0xc4, CritterInitGeo's +0xcc,
    /// the shadow +0xd0, the nodes +0xd4/+0xd8/+0xdc/+0xe8, the move effect
    /// node +0xe0 (CritterDoTransition's MBRemoveNode), the chain
    /// +0xb60/+0xb64 and each collision node's +0/+4/+8.
    fn pointer_words(ncol: usize) -> Vec<usize> {
        let mut v = vec![0x04, 0x70, 0x80, 0x84, 0x88, 0x8c, 0xc0, 0xc4, 0xcc, 0xd0, 0xd4, 0xd8, 0xdc, 0xe0, 0xe8, 0xb60, 0xb64];
        for c in 0..ncol {
            v.extend([0x540 + 0x60 * c, 0x544 + 0x60 * c, 0x548 + 0x60 * c]);
        }
        v
    }

    /// The record in the native convention: pointer words cleared, the
    /// +0xb54 item as slot + 1 and the +0xb5c waypoint as index + 1.
    fn native(raw: &[u8], ncol: usize, items_base: u32) -> Result<Vec<u8>, Box<dyn Error>> {
        let mut r = raw[..0xb70].to_vec();
        for o in Self::pointer_words(ncol) {
            r[o..o + 4].fill(0);
        }
        let it = Self::word(raw, 0xb54);
        if it != 0 {
            if it < items_base || !(it - items_base).is_multiple_of(0x110) {
                return Err(format!("critter +0xb54 {it:#x} is not an Items record").into());
            }
            r[0xb54..0xb58].copy_from_slice(&((it - items_base) / 0x110 + 1).to_le_bytes());
        }
        let wp = Self::word(raw, 0xb5c);
        if wp != 0 {
            if wp < Self::WAYPOINTS || !(wp - Self::WAYPOINTS).is_multiple_of(0x80) {
                return Err(format!("critter +0xb5c {wp:#x} is not a waypoint record").into());
            }
            r[0xb5c..0xb60].copy_from_slice(&((wp - Self::WAYPOINTS) / 0x80 + 1).to_le_bytes());
        }
        Ok(r)
    }

    fn ncol(game: &gdl_core::game::Game, t: (usize, usize)) -> usize {
        let ty = &game.critter_data.files[t.0].wad.types[t.1];
        i16::from_le_bytes([ty[0x118], ty[0x119]]).max(0) as usize
    }

    /// Seed the native instances, CritterNewID/CritterCounter, the boss
    /// state and BossCritInst (index + 1). Roots only (children: error).
    fn seed(&self, game: &mut gdl_core::game::Game, items_base: u32, objects: &ReferenceObjects) -> Result<(), Box<dyn Error>> {
        let etypes = self.global("CritterETypes")?;
        let enemy_root = self.global("EnemyRootNode").ok().map(|b| Self::word(&b, 0));
        game.critters.clear();
        for k in 0..self.num.min(16) {
            game.critters.push(None);
            if !self.live(k) {
                continue;
            }
            let raw = &self.slots[k];
            let ty = Self::word(raw, 4);
            let at = (0..54).find(|&n| Self::word(&etypes, 4 * n) == ty).ok_or_else(|| format!("critter {k} TYPE {ty:#x} is not a CritterETypes entry"))?;
            let (kind, slot) = (at / 6, at % 6);
            let t = game.critter_data.etypes[kind][slot].ok_or_else(|| format!("no native critter type for kind {kind} slot {slot}"))?;
            let ncol = Self::ncol(game, t);
            // Tree node pointers → indices by CritterGetNode's table (+0xc0,
            // 0x28-byte entries); traces without it cannot seed +0xdc.
            let table = self.nodes.get(&k).map(|n| n.2.clone()).unwrap_or_default();
            let index = |p: u32| -> Option<usize> { if p == 0 { None } else { table.iter().position(|&q| q == p) } };
            let dc = Self::word(raw, 0xdc);
            let node_dc = if dc == 0 {
                None
            } else {
                Some(index(dc).ok_or_else(|| format!("critter {k} +0xdc {dc:#x} is not in its node table (trace before trace-k2-newgame-v5?)"))?)
            };
            // The +0x70 node: local matrix and parent (EnemyRootNode, or a
            // WORLDS object by its +0x28 node pointer).
            let (node, floor) = match self.nodes.get(&k) {
                Some((n, parent, _)) => {
                    let m: [[f32; 4]; 4] = std::array::from_fn(|a| std::array::from_fn(|b| f32::from_bits(Self::word(n, 0x10 * a + 4 * b))));
                    let floor = if Some(*parent) == enemy_root {
                        None
                    } else {
                        Some(objects.0.iter().position(|o| Self::word(o, 0x28) == *parent).ok_or_else(|| format!("critter {k} node parent {parent:#x} is not a WORLDS object node"))?)
                    };
                    (Some(m), floor)
                }
                None => (None, None),
            };
            game.seed_critter(k, t, slot, &Self::native(raw, ncol, items_base)?, node_dc, node, floor).map_err(|e| e.0)?;
            // The traced node pointers against the native look-at and
            // collision nodes (a reference node outside the tree is noted).
            let cr = game.critters[k].as_ref().ok_or("seeded critter")?;
            let mut checks = vec![(0xd4, cr.node_d4), (0xd8, cr.node_d8), (0xe8, cr.node_e8)];
            for (c, (n, n2)) in cr.colnode_nodes.iter().enumerate() {
                checks.push((0x544 + 0x60 * c, *n));
                checks.push((0x548 + 0x60 * c, *n2));
            }
            for (o, n) in checks {
                let p = Self::word(raw, o);
                if index(p) != n {
                    game.log.note("critter seed", &gdl_core::game::Unsupported(format!("critter {k} +{o:#x} {p:#x} ≠ native node {n:?}")));
                }
            }
        }
        let g = |n: &str| -> Result<u32, Box<dyn Error>> { Ok(Self::word(&self.global(n)?, 0)) };
        game.critter_new_id = g("CritterNewID")? as u16;
        game.critter_counter = g("CritterCounter")? as u16;
        let bp = self.global("BossStartPos")?;
        game.boss_start_pos = std::array::from_fn(|j| f32::from_bits(Self::word(&bp, 4 * j)));
        game.boss_activate_time = f32::from_bits(g("BossActivateTime")?);
        game.boss_cam_mode = g("BossCamMode")? as i32;
        game.boss_crit_inst = if self.boss == 0 { 0 } else { ((self.boss - Self::BASE) / 0xb70 + 1) as i32 };
        Ok(())
    }

    /// Differing words of the native instances against these (the native
    /// clock written back to the +0x84 AnimInfo).
    fn compare(&self, game: &gdl_core::game::Game, items_base: u32) -> Result<Vec<serde_json::Value>, Box<dyn Error>> {
        let mut out = Vec::new();
        if game.critters.len() != self.num {
            out.push(serde_json::json!({"field":"NumCritterInsts","native":game.critters.len(),"reference":self.num}));
        }
        let boss = if self.boss == 0 { 0 } else { ((self.boss - Self::BASE) / 0xb70 + 1) as i32 };
        if game.boss_crit_inst != boss {
            out.push(serde_json::json!({"field":"BossCritInst","native":game.boss_crit_inst,"reference":boss}));
        }
        for k in 0..16 {
            let n = game.critters.get(k).and_then(Option::as_ref);
            match (n, self.live(k)) {
                (None, false) => {}
                (Some(cr), true) => {
                    let ncol = Self::ncol(game, cr.ty);
                    let mut nr = cr.rec.0.clone();
                    ReferenceItems::write_clock(&mut nr, &cr.clock);
                    let r = Self::native(&self.slots[k], ncol, items_base)?;
                    let mut skip = Self::pointer_words(ncol);
                    skip.extend(Self::RESIDUE);
                    for o in (0..0xb70).step_by(4) {
                        if skip.contains(&o) {
                            continue;
                        }
                        let (a, b) = (Self::word(&nr, o), Self::word(&r, o));
                        if a != b {
                            out.push(serde_json::json!({"critter":k,"offset":format!("{o:#x}"),"native":format!("{a:08x}"),"reference":format!("{b:08x}")}));
                        }
                    }
                }
                (a, b) => out.push(serde_json::json!({"critter":k,"field":"live","native":a.is_some(),"reference":b})),
            }
        }
        Ok(out)
    }
}

fn texdef_export(
    source: &Path,
    textures: Option<&Path>,
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    if output.exists() {
        return Err("output already exists; choose a new path".into());
    }
    let bytes = bounded_read(source, 64 * 1024 * 1024)?;
    let table = TextureTable::parse(&bytes)?;
    let pixels = textures
        .map(|p| bounded_read(p, 1024 * 1024 * 1024))
        .transpose()?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = Staging(parent.join(format!(".gdl-staging-{}", std::process::id())));
    fs::create_dir(&stage.0)?;
    let mut outcomes = Vec::new();
    let mut decoded = 0;
    for bitmap in &table.bitmaps {
        let mut png_name = None;
        let mut error = None;
        if let Some(data) = &pixels {
            match bitmap.decode_base_rgba(data) {
                Ok(rgba) => {
                    let channel = if matches!(bitmap.format, 0 | 16 | 48) {
                        "binary-texa-selector"
                    } else {
                        "source-alpha"
                    };
                    let name = format!("{:06}-{channel}.png", bitmap.index);
                    let file = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(stage.0.join(&name))?;
                    let mut encoder = png::Encoder::new(
                        std::io::BufWriter::new(file),
                        bitmap.width as u32,
                        bitmap.height as u32,
                    );
                    encoder.set_color(png::ColorType::Rgba);
                    encoder.set_depth(png::BitDepth::Eight);
                    let mut writer = encoder.write_header()?;
                    writer.write_image_data(&rgba)?;
                    writer.finish()?;
                    fs::File::open(stage.0.join(&name))?.sync_all()?;
                    png_name = Some(name);
                    decoded += 1;
                }
                Err(e) => error = Some(e.to_string()),
            }
        }
        outcomes
            .push(serde_json::json!({"bitmap_index":bitmap.index,"png":png_name,"error":error}));
    }
    let manifest = serde_json::json!({"schema_version":1,"converter_version":env!("CARGO_PKG_VERSION"),
        "pixel_decoder_version":gdl_formats::texdef::BASE_RGBA_DECODER_VERSION,
        "texdef_sha256":hash(&bytes),"textures_sha256":pixels.as_ref().map(|p|hash(p)),
        "decode_scope":"candidate base pixels; 8-bit alpha preserved; CT16 alpha selector shown binary, TEXA/material blending/mipmaps unverified",
        "table":table,"outcomes":outcomes});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(stage.0.join("manifest.json"))?;
    serde_json::to_writer_pretty(&mut file, &manifest)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(&stage.0, output)?;
    println!(
        "{} bitmap records; {decoded} base PNGs; unsupported formats retained in manifest",
        table.bitmaps.len()
    );
    Ok(())
}

/// Removing an unpublished staging directory cannot affect an existing cache.
struct Staging(PathBuf);
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn write_manifest(path: &Path, manifest: &Manifest) -> Result<(), Box<dyn Error>> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    serde_json::to_writer_pretty(&mut file, manifest)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 3 && args[0] == "critter-inspect" {
        // critter-inspect CRITTER.WAD NEW_REPORT.json: chunk table and record labels.
        use gdl_formats::critter::{label, CritterWad};
        let b = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let w = CritterWad::parse(&b)?;
        let i16_ = |r: &[u8], o: usize| i16::from_le_bytes([r[o], r[o + 1]]);
        let i32_ = |r: &[u8], o: usize| i32::from_le_bytes(r[o..o + 4].try_into().unwrap());
        let report = serde_json::json!({
            "source_sha256":hash(&b),"chunks":w.chunks,
            "types":w.types.iter().map(|t| serde_json::json!({"name":label(t,0,16),"nodes":[label(t,0x20,16),label(t,0x30,16),label(t,0x40,16)],
                "desc":i16_(t,0x50),"etype_slot":i16_(t,0x52),"flags_5c":format!("{:#x}",i32_(t,0x5c)),
                "moves":[i16_(t,0x110),i16_(t,0x112)],"patterns":[i16_(t,0x114),i16_(t,0x116)],"colnodes":[i16_(t,0x118),i16_(t,0x11a)],
                "child_type":i16_(t,0x11c),"alias_type":i16_(t,0x11e)})).collect::<Vec<_>>(),
            "descs":w.descs.iter().map(|d| serde_json::json!({"name":label(d,0,16),"prefix":label(d,0x10,16),"kind":i16_(d,0x20)})).collect::<Vec<_>>(),
            "addas":w.addas.iter().map(|a| serde_json::json!({"type":i16_(a,0),"flags":i16_(a,2),"tree":label(a,0x10,8),"node":label(a,0x18,8)})).collect::<Vec<_>>(),
            "nodes":w.nodes.iter().map(|n| label(n,0,16)).collect::<Vec<_>>(),
            "moves":w.moves.iter().map(|m| serde_json::json!({"type":i32_(m,0),"flags":format!("{:#x}",i32_(m,4)),"priority":i32_(m,8),
                "name":label(m,0x10,16),"seq":label(m,0x20,16),"node":label(m,0x30,16)})).collect::<Vec<_>>(),
            "patterns":w.patterns.iter().map(|p| (0..8).map(|k| i16_(p,0x20 + 2 * k)).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "damages":w.damages.iter().map(|d| i16_(d,0)).collect::<Vec<_>>(),
            "sfx":w.sfx.iter().map(|x| serde_json::json!({"flags":format!("{:#x}",i32_(x,0)),"name":label(x,0x10,16),"sound":label(x,0x20,32)})).collect::<Vec<_>>(),
            "scope":"structure of CritterInitHeader tables; labels only, semantics in research/critters.md"});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "anim-trees" {
        // anim-trees ANIM.PS2 NEW_REPORT.json: tree labels with the first
        // sequence's frames and timing factor.
        let b = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let a = gdl_formats::animation::Animations::parse(&b)?;
        let trees: Vec<_> = a
            .trees
            .iter()
            .enumerate()
            .map(|(k, t)| {
                let s = t.sequences.first();
                serde_json::json!({"index":k,"label":t.label,"nodes":t.nodes.len(),"node_list":t.nodes.iter().map(|n| {
                    // A type-4 node's particle record: tree +0 header offset +
                    // the node's sequence info offset (as gdl_play reads it).
                    let ptex = (n.node_type == 4).then(|| -> String {
                        let base = t.file_offset as usize;
                        let Some(h0) = b.get(base..base + 4).map(|v| u32::from_le_bytes(v.try_into().unwrap()) as usize) else { return "no header".into() };
                        let at = base + h0 + n.sequence_info_offset as usize;
                        let Some(rec) = b.get(at..at + gdl_core::psys::RECORD) else { return format!("record {at:#x} past {:#x}", b.len()) };
                        match gdl_core::psys::Psys::new_world(0, rec, &[], None, true) {
                            Ok(p) => format!("texture {:?}", p.texture),
                            Err(e) => format!("new_world: {e}"),
                        }
                    });
                    serde_json::json!({"label":n.label,"type":n.node_type,"flags":n.flags,"model_flags":n.model_flags,"parent":n.parent,"pos":n.initial_position,"psys_texture":ptex,"raw_len":n.raw.len()})
                }).collect::<Vec<_>>(),"prefix":t.prefix,"sequences":t.sequences.len(),"frames":s.map(|s| s.frames),"timing":s.map(|s| s.timing_factor)})
            })
            .collect();
        let mods: Vec<_> = gdl_formats::animation::texmods(&b)?
            .iter()
            .map(|m| serde_json::json!({"owner":m.owner,"target_name":m.target_name,"frame_name":m.frame_name,"target":m.target,"frame_base":m.frame_base,"count":m.count,"divisor":m.divisor}))
            .collect();
        let report = serde_json::json!({"source_sha256":hash(&b),"trees":trees,"texmods":mods});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "pss-inspect" {
        // pss-inspect DIR NEW_REPORT.json: every *.PSS's program-stream layout
        // and its private-stream audio (SShd header, decoded length).
        let mut entries = Vec::new();
        let mut names: Vec<_> = fs::read_dir(&args[1])?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pss"))).collect();
        names.sort();
        for path in names {
            let b = bounded_read(&path, 64 * 1024 * 1024)?;
            let e = match gdl_formats::pss::demux_audio(&b) {
                Ok((audio, l)) => {
                    let ads = gdl_formats::audio::Ads::parse(&audio).ok();
                    let pcm = ads.as_ref().map(|a| a.decode(&audio).len() / a.channels.max(1) as usize);
                    serde_json::json!({"file":path.file_name().map(|n| n.to_string_lossy().into_owned()),"sha256":hash(&b),"packs":l.packs,"video_bytes":l.video_bytes,
                        "audio_bytes":l.audio_bytes,"padding_bytes":l.padding_bytes,"trailing":l.trailing,"ads":ads,
                        "audio_seconds":pcm.zip(ads.as_ref()).map(|(n, a)| n as f64 / f64::from(a.rate.max(1)))})
                }
                Err(e) => serde_json::json!({"file":path.file_name().map(|n| n.to_string_lossy().into_owned()),"sha256":hash(&b),"error":e.to_string()}),
            };
            entries.push(e);
        }
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &serde_json::json!({"movies":entries}))?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 2 && args[0] == "levl-list" {
        // levl-list WDATA.WAD: each LEVL record's code, +0x34 movie name, +0 flags
        // (bit 3: do_players' dark-level player light), +4 flags
        // (bit 2: NextAttractWave's attract levels) and its CAMS record's +0x26
        // attract_cam_mode / +0x28 (InitCamera's flyby branch) (stdout).
        let w = gdl_formats::wdata::WorldData::parse(&bounded_read(Path::new(&args[1]), 16 * 1024 * 1024)?)?;
        for (k, l) in w.levels.iter().enumerate() {
            let m = &l.raw[0x34..0x44];
            let m = String::from_utf8_lossy(&m[..m.iter().position(|&b| b == 0).unwrap_or(m.len())]).into_owned();
            let cam = w.cameras.get(l.camera_index.max(0) as usize).map(|c| {
                (i16::from_le_bytes([c.raw[0x26], c.raw[0x27]]), f32::from_le_bytes(c.raw[0x28..0x2c].try_into().unwrap_or([0; 4])))
            });
            println!("{k} {} movie {:?} flags00 {:#x} flags04 {:#x} cams {} attract_cam {:?} boss {:#x}", l.code, m, l.flags_00, l.word_04, l.camera_index, cam, l.boss_type);
        }
        return Ok(());
    }
    if args.len() == 3 && args[0] == "audio-inspect" {
        // audio-inspect FILE(.VBK|.ADS|AUDATPS2.ROM) NEW_REPORT.json: tables + decoded lengths.
        let b = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let report = if b.starts_with(b"VBNK") {
            let v = gdl_formats::audio::Vbk::parse(&b)?;
            let lens: Vec<_> = (0..v.samples.len()).map(|k| v.decode(&b, k).map(|p| p.len())).collect::<Result<_, _>>()?;
            serde_json::json!({"bank":v,"decoded_samples":lens})
        } else if b.starts_with(b"SShd") {
            let a = gdl_formats::audio::Ads::parse(&b)?;
            let pcm = a.decode(&b);
            let peak = pcm.iter().map(|s| i32::from(*s).abs()).max().unwrap_or(0);
            serde_json::json!({"stream":a,"frames":pcm.len() / a.channels as usize,"peak":peak})
        } else {
            serde_json::json!({"rom":gdl_formats::audio::AudioRom::parse(&b)?})
        };
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &serde_json::json!({"source_sha256":hash(&b),"report":report}))?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "textrom-inspect" {
        // textrom-inspect ROM NEW_REPORT.json: fonts, entries and lists.
        let b = bounded_read(Path::new(&args[1]), 1024 * 1024)?;
        let rom = gdl_formats::textrom::TextRom::parse(&b)?;
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &serde_json::json!({"source_sha256":hash(&b),"rom":rom}))?;
        return Ok(());
    }
    if args.len() == 20 && args[0] == "profile-check" {
        // profile-check GAME_START.json SLUS_200.47 CLASS_ANIM.PS2 PDATA×16 (class order):
        // rebuild player 0 from its save region (PlayerLoadSaveFile path) and
        // list the record offsets that differ from the captured record (stdout).
        use gdl_core::game::profile;
        let start: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?)?;
        let elf_bytes = bounded_read(Path::new(&args[2]), 16 * 1024 * 1024)?;
        let elf = gdl_formats::elf::Elf::parse(&elf_bytes)?;
        let tables = gdl_core::player_class::ClassTables::from_elf(&elf)?;
        let mut pdats = Vec::new();
        for a in &args[4..20] {
            pdats.push(gdl_formats::pdata::PlayerData::parse(&bounded_read(Path::new(a), 1024 * 1024)?)?.pdat);
        }
        let pdats = profile::Pdats(pdats);
        let h = start["players"][0].as_str().ok_or("players[0]")?;
        let captured: Vec<u8> = (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16)).collect::<Result<_, _>>()?;
        let region = &captured[profile::SAVE_START..profile::SAVE_START + profile::SAVE_LEN];
        println!("summary {:?}", profile::summary(region));
        let (mut r, controls) = profile::load_character(0, region, &pdats, &tables).map_err(|e| e.to_string())?;
        let anim = gdl_formats::animation::Animations::parse(&bounded_read(Path::new(&args[3]), 64 * 1024 * 1024)?)?;
        let heart = elf.object("heart_rate")?.first().copied().unwrap_or(0);
        // init_next_level for G1 (game_wave 0x700): level start, then PlayerSaveState(k, 1).
        profile::level_start(&mut r, &pdats, &tables.action_map(&anim), 0x700, heart).map_err(|e| e.to_string())?;
        profile::player_save_state(&mut r, true, 7, 0, &controls);
        println!("controls {controls:?} heart_rate {heart}");
        let mut runs: Vec<(usize, usize)> = Vec::new();
        for o in (0..captured.len()).step_by(4) {
            if captured[o..o + 4] != r.0[o..o + 4] {
                match runs.last_mut() {
                    Some(l) if l.1 == o => l.1 = o + 4,
                    _ => runs.push((o, o + 4)),
                }
            }
        }
        for (a, b) in runs {
            let w = |v: &[u8], o: usize| u32::from_le_bytes(v[o..o + 4].try_into().unwrap());
            println!("{a:#06x}..{b:#06x} captured {:#010x} native {:#010x}", w(&captured, a), w(&r.0, a));
        }
        return Ok(());
    }
    if args.len() == 3 && args[0] == "objects-bitmaps" {
        // objects-bitmaps OBJECTS.PS2 TEXDEF.PS2: each object bitmap record with
        // its flags and the TEXDEF candidates (offset/format/size match) (stdout).
        let o = gdl_formats::objects::Objects::parse(&bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?)?;
        let t = gdl_formats::texdef::TextureTable::parse(&bounded_read(Path::new(&args[2]), 64 * 1024 * 1024)?)?;
        for b in &o.bitmap_records {
            let c: Vec<String> = t
                .bitmaps
                .iter()
                .filter(|x| x.texture_offset == b.texture_offset && x.format == b.format && x.width == b.width && x.height == b.height)
                .map(|x| x.label.clone())
                .collect();
            println!("{} fmt {} flags {:#x} {}x{} off {:#x} tex0 {:#018x} candidates {:?}", b.index, b.format, b.flags, b.width, b.height, b.texture_offset, b.tex0, c);
        }
        return Ok(());
    }
    if args.len() == 2 && args[0] == "text-rom" {
        // text-rom FILE.ROM: named entries with their lines (stdout).
        let rom = gdl_formats::textrom::TextRom::parse(&bounded_read(Path::new(&args[1]), 1024 * 1024)?)?;
        println!("fonts {:?}", rom.fonts);
        for (k, e) in rom.entries.iter().enumerate() {
            println!("{k} {} font {} scale {} {:?}", e.name, e.font, e.scale, e.lines);
        }
        for (k, (name, entries)) in rom.lists.iter().enumerate() {
            println!("list {k} {name} {entries:?}");
        }
        return Ok(());
    }
    if args.len() == 3 && args[0] == "world-anim-eval" {
        // world-anim-eval WORLDS.PS2 FRAMES.json: [[entry, frame], ...] → the
        // DoWorldAnimSub local matrix of each entry at that frame (stdout JSON).
        let world = gdl_formats::world::World::parse(&bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?)?;
        let frames: Vec<(usize, f32)> = serde_json::from_slice(&bounded_read(Path::new(&args[2]), 1024 * 1024)?)?;
        let anims = world.anims.as_ref().ok_or("world without animations")?;
        let set = gdl_core::game::world_anim::WorldAnimSet::new(world.bytes.clone(), anims);
        let positions: Vec<[f32; 3]> = world.nodes.iter().map(|n| n.position).collect();
        let col = gdl_core::collision::CollisionWorld::from_world(&world).map_err(|e| format!("{e:?}"))?;
        let mut out = Vec::new();
        for (k, frame) in frames {
            let e = set.entries.get(k).ok_or("entry index")?;
            let r = gdl_core::game::world_anim::eval_local(&set, e, frame, col.objects[e.object].position);
            out.push(serde_json::json!({"entry": k, "frame": frame, "object": e.object, "file_position": positions[e.object],
                "matrix": r.as_ref().ok(), "error": r.err().map(|e| e.0)}));
        }
        println!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }
    if args.len() == 2 && args[0] == "world-positions" {
        // world-positions WORLDS.PS2: loaded object +0x1c positions (stdout JSON hex).
        let world = gdl_formats::world::World::parse(&bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?)?;
        let col = gdl_core::collision::CollisionWorld::from_world(&world).map_err(|e| format!("{e:?}"))?;
        let v: Vec<String> = col.objects.iter().map(|o| o.position.iter().map(|f| format!("{:08x}", f.to_bits())).collect()).collect();
        println!("{}", serde_json::to_string(&v)?);
        return Ok(());
    }
    if args.len() == 7 && args[0] == "stdcam-replay" {
        // stdcam-replay TRACE.json WORLDS.PS2 WDATA.WAD ELF LEVEL_CODE CUR_WORLD: for each
        // traced camera update, start from the previous update's StdCamera record, rebuild
        // the pb view from it (MBEndFrame), compare with the traced pb block, then run one
        // UpdateCam with the traced player inputs and compare the record (stdout JSON).
        use gdl_core::game::camera::{build_transmitters, o, CamConsts, CamCtx, Camera};
        use gdl_core::game::record::Record;
        let trace: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(&args[1]), 256 * 1024 * 1024)?)?;
        let world = gdl_formats::world::World::parse(&bounded_read(Path::new(&args[2]), 64 * 1024 * 1024)?)?;
        let wdata = gdl_formats::wdata::WorldData::parse(&bounded_read(Path::new(&args[3]), 4 * 1024 * 1024)?)?;
        let elf_bytes = bounded_read(Path::new(&args[4]), 16 * 1024 * 1024)?;
        let elf = gdl_formats::elf::Elf::parse(&elf_bytes)?;
        let level = wdata.levels.iter().find(|l| std::ffi::OsStr::new(&l.code) == args[5]).ok_or("level code not in WDATA")?;
        let params = wdata.cameras.get(level.camera_index as usize).ok_or("CAMS index")?;
        let cur_world: i32 = args[6].to_str().ok_or("cur world")?.parse()?;
        let hexb = |v: &serde_json::Value| -> Result<Vec<u8>, Box<dyn Error>> {
            let s = v.as_str().ok_or("hex field")?;
            Ok((0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16)).collect::<Result<_, _>>()?)
        };
        let word = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let samples = trace["samples"].as_array().ok_or("samples")?;
        let mut cam = Camera::new(CamConsts::from_elf(&elf)?, build_transmitters(&world.locators, cur_world).map_err(|e| e.0)?, params, world.min, world.max).map_err(|e| e.0)?;
        let mut last: Option<Vec<u8>> = None;
        let (mut updates, mut pb_ok, mut std_ok) = (0, 0, 0);
        let mut first_pb = None;
        let mut first_std = None;
        let mut per_update = Vec::new();
        for s in samples {
            let c = &s["stdcam"];
            let std = hexb(&c["std"])?;
            let Some(prev) = last.clone() else {
                last = Some(std);
                continue;
            };
            if prev == std {
                continue;
            }
            last = Some(std.clone());
            updates += 1;
            cam.std = Record(prev);
            cam.cur3d = i32::from_le_bytes(hexb(&c["Cur3DTransmitter"])?[..4].try_into()?);
            // Cur3DTransmitter after the update is traced; the value before it is the
            // previous update's (kept below).
            if let Some(p) = per_update.last().and_then(|u: &serde_json::Value| u["cur3d_ref"].as_i64()) {
                cam.cur3d = p as i32;
            }
            cam.refresh_view().map_err(|e| e.0)?;
            let pb = hexb(&c["pb"])?;
            let mut pb_diff = Vec::new();
            for (name, base, m) in [("view", 0x210, cam.pb.view), ("inv", 0x250, cam.pb.inv), ("pv", 0x290, cam.pb.pv), ("wts", 0x2d0, cam.pb.wts), ("proj", 0x90, cam.pb.proj), ("vp", 0xd0, cam.pb.vp)] {
                for (i, row) in m.iter().enumerate() {
                    for (j, v) in row.iter().enumerate() {
                        let r = word(&pb, base + 16 * i + 4 * j);
                        if v.to_bits() != r {
                            pb_diff.push(format!("{name}[{i}][{j}] {:08x}/{r:08x}", v.to_bits()));
                        }
                    }
                }
            }
            if pb_diff.is_empty() {
                pb_ok += 1;
            } else if first_pb.is_none() {
                first_pb = Some(serde_json::json!({"step": s["step"], "diff": pb_diff}));
            }
            let mut players: [Record; 4] = std::array::from_fn(|_| Record::zeroed(0x2b00));
            for (k, pl) in players.iter_mut().enumerate() {
                let b50 = hexb(&c[format!("player{k}_50")])?;
                pl.0[0x50..0x80].copy_from_slice(&b50);
                pl.0[0xfc..0x100].copy_from_slice(&hexb(&c[format!("player{k}_fc")])?);
                pl.0[0x93c..0x940].copy_from_slice(&hexb(&c[format!("player{k}_93c")])?);
            }
            let g = |n: &str| -> Result<i32, Box<dyn Error>> { Ok(i32::from_le_bytes(hexb(&c[n])?[..4].try_into()?)) };
            let ctx = CamCtx {
                players: &players,
                num_in_game_players: g("NumInGamePlayers")?,
                spf: f32::from_bits(g("SPF")? as u32),
                vb_elapsed: g("vb_elapsed")?,
                paused: g("paused")?,
                help: g("help")?,
                game_mode: g("game_mode")? as u32,
            };
            let r = cam.update_once(&ctx);
            let mut diff = Vec::new();
            match r {
                Err(e) => diff.push(format!("unsupported: {}", e.0)),
                Ok(_) => {
                    for off in (0..o::SIZE).step_by(4) {
                        let (a, b) = (word(&cam.std.0, off), word(&std, off));
                        if a != b {
                            diff.push(format!("+{off:#05x} {:e}/{:e}", f32::from_bits(a), f32::from_bits(b)));
                        }
                    }
                }
            }
            if diff.is_empty() {
                std_ok += 1;
            } else if first_std.is_none() {
                first_std = Some(serde_json::json!({"step": s["step"], "diff": diff}));
            }
            per_update.push(serde_json::json!({"step": s["step"], "pb_diffs": pb_diff.len(), "std_diffs": diff.len(), "cur3d_ref": g("Cur3DTransmitter")?, "cur3d_native": cam.cur3d}));
        }
        let report = serde_json::json!({"updates": updates, "pb_exact": pb_ok, "std_exact": std_ok, "first_pb_divergence": first_pb, "first_std_divergence": first_std, "per_update": per_update});
        println!("{}", serde_json::to_string(&report)?);
        return Ok(());
    }
    if args.len() == 5 && args[0] == "camera-check" {
        // camera-check FIXTURE.json WORLDS.PS2 WDATA.WAD ELF (compare_camera.py).
        use gdl_core::game::camera::{build_transmitters, o, CamConsts, CamCtx, Camera};
        use gdl_core::game::record::Record;
        let fx: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(&args[1]), 16 * 1024 * 1024)?)?;
        let bytes = |k: &str| -> Result<Vec<u8>, Box<dyn Error>> {
            let s = fx[k].as_str().ok_or(format!("fixture {k}"))?;
            Ok((0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16)).collect::<Result<_, _>>()?)
        };
        let int = |k: &str| fx[k].as_i64().unwrap_or(0) as i32;
        let world = gdl_formats::world::World::parse(&bounded_read(Path::new(&args[2]), 64 * 1024 * 1024)?)?;
        let wdata = gdl_formats::wdata::WorldData::parse(&bounded_read(Path::new(&args[3]), 4 * 1024 * 1024)?)?;
        let elf_bytes = bounded_read(Path::new(&args[4]), 16 * 1024 * 1024)?;
        let elf = gdl_formats::elf::Elf::parse(&elf_bytes)?;
        let code = fx["level_code"].as_str().ok_or("level code")?;
        let level = wdata.levels.iter().find(|l| l.code == code).ok_or("level code not in WDATA")?;
        let params = wdata.cameras.get(level.camera_index as usize).ok_or("CAMS index")?;
        let set = build_transmitters(&world.locators, int("cur_world")).map_err(|e| e.0)?;
        let consts = CamConsts::from_elf(&elf)?;
        let mut cam = Camera::new(consts, set, params, world.min, world.max).map_err(|e| e.0)?;
        let ram_t = bytes("transmitters")?;
        let mut t_mismatches = Vec::new();
        if ram_t.len() / 0x40 != cam.transmitters.len() {
            t_mismatches.push(format!("count native {} reference {}", cam.transmitters.len(), ram_t.len() / 0x40));
        }
        for (k, t) in cam.transmitters.iter().enumerate().take(ram_t.len() / 0x40) {
            let r = &ram_t[0x40 * k..0x40 * k + 0x40];
            let f = |o: usize| u32::from_le_bytes(r[o..o + 4].try_into().unwrap());
            let native = [t.pos[0].to_bits(), t.pos[1].to_bits(), t.pos[2].to_bits(), t.pitch.to_bits(), t.yaw.to_bits(), t.roll.to_bits()];
            let reference = [f(0x10), f(0x14), f(0x18), f(0x20), f(0x24), f(0x28)];
            if r[0] != t.flag || r[1] != t.radius || u16::from_le_bytes([r[2], r[3]]) != t.word_02 || native != reference {
                t_mismatches.push(format!("transmitter {k}: native flag {} radius {} {:08x?} reference {:02x?} {:08x?}", t.flag, t.radius, native, &r[..4], reference));
            }
        }
        let cams = bytes("cams")?;
        let fbits = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let mut c_mismatches = Vec::new();
        for k in 0..3 {
            for (name, native, off) in [("min", cam.min[k], 0xc + 4 * k), ("max", cam.max[k], 0x18 + 4 * k)] {
                if native.to_bits() != fbits(&cams, off) {
                    c_mismatches.push(format!("CAMS {name}[{k}] native {native} reference {}", f32::from_bits(fbits(&cams, off))));
                }
            }
        }
        cam.std = Record(bytes("std")?);
        cam.cur3d = int("cur3d");
        cam.zoom_frames = int("zoom_frames");
        cam.refresh_view().map_err(|e| e.0)?;
        let pb = bytes("pb")?;
        let win = bytes("win")?;
        let mut pb_mismatches = Vec::new();
        let mats = [("view 0x210", 0x210, cam.pb.view), ("inv 0x250", 0x250, cam.pb.inv), ("proj 0x90", 0x90, cam.pb.proj), ("vp 0xd0", 0xd0, cam.pb.vp), ("pv 0x290", 0x290, cam.pb.pv), ("wts 0x2d0", 0x2d0, cam.pb.wts)];
        for (name, base, m) in mats {
            for (i, row) in m.iter().enumerate() {
                for (j, v) in row.iter().enumerate() {
                    let r = fbits(&pb, base + 16 * i + 4 * j);
                    if v.to_bits() != r {
                        pb_mismatches.push(format!("{name}[{i}][{j}] native {v:e} ({:08x}) reference {:e} ({r:08x})", v.to_bits(), f32::from_bits(r)));
                    }
                }
            }
        }
        for (name, native, off) in [("ang h", cam.pb.ang[0], 0x1c), ("ang v", cam.pb.ang[1], 0x20), ("forward x", cam.pb.cam_rows[2][0], 0x90), ("forward z", cam.pb.cam_rows[2][2], 0x98)] {
            if native.to_bits() != fbits(&win, off) {
                pb_mismatches.push(format!("MboxWinPtr {name} native {native:e} reference {:e}", f32::from_bits(fbits(&win, off))));
            }
        }
        let players: Vec<Record> = (0..4)
            .map(|k| -> Result<Record, Box<dyn Error>> {
                let s = fx["players"][k].as_str().ok_or("player")?;
                Ok(Record((0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16)).collect::<Result<_, _>>()?))
            })
            .collect::<Result<_, _>>()?;
        let players: [Record; 4] = players.try_into().map_err(|_| "players")?;
        let ctx = CamCtx {
            players: &players,
            num_in_game_players: int("num_in_game_players"),
            spf: fx["spf"].as_f64().ok_or("spf")? as f32,
            vb_elapsed: int("vb_elapsed"),
            paused: int("paused"),
            help: int("help"),
            game_mode: int("game_mode") as u32,
        };
        let before = cam.std.0.clone();
        let moved = cam.update_once(&ctx).map_err(|e| e.0)?;
        let mut s_mismatches = Vec::new();
        for off in (0..o::SIZE).step_by(4) {
            let (a, b) = (fbits(&cam.std.0, off), fbits(&before, off));
            if a != b {
                s_mismatches.push(format!("+{off:#05x} after {:e} ({a:08x}) captured {:e} ({b:08x})", f32::from_bits(a), f32::from_bits(b)));
            }
        }
        // LinkTriggerToCam tables and the last trigger matrix (trig_cam_mat).
        let (tc, _) = gdl_core::game::camera::tower_camera_links(&cam.transmitters, int("cur_world"));
        let mut trig_mismatches = Vec::new();
        let ref_index = |v: i64| -> Option<usize> { (v != 0).then(|| ((v - 0x234500) / 0x40) as usize) };
        let tcr = &fx["tower_cams"];
        // Outside the Tower the tables keep the Tower's stale entries (not reset).
        if !tcr.is_null() && int("cur_world") == 0xd {
            let mut cmp = |name: String, native: Option<usize>, r: &serde_json::Value| {
                let reference = r.as_i64().and_then(ref_index);
                if native != reference {
                    trig_mismatches.push(format!("{name}: native {native:?} reference {reference:?}"));
                }
            };
            cmp("crystal".into(), tc.crystal, &tcr["crystal"]);
            for k in 0..3 {
                cmp(format!("rune[{k}]"), tc.rune[k], &tcr["rune"][k]);
            }
            for k in 0..2 {
                cmp(format!("window[{k}]"), tc.window[k], &tcr["window"][k]);
            }
            for t in 0..3 {
                for k in 0..14 {
                    cmp(format!("sumner[{t}][{k}]"), tc.sumner[t][k], &tcr["sumner"][14 * t + k]);
                }
            }
            if let Some(k) = tc.crystal {
                let t = &cam.transmitters[k];
                let rows = gdl_core::game::camera::trigger_camera_rows([t.pitch, t.yaw]).map_err(|e| e.0)?;
                let m = bytes("trig_mat")?;
                for (i, row) in rows.iter().enumerate() {
                    for (j, v) in row.iter().enumerate() {
                        let r = fbits(&m, 16 * i + 4 * j);
                        if v.to_bits() != r {
                            trig_mismatches.push(format!("crystal trig_cam_mat[{i}][{j}] native {v:e} reference {:e}", f32::from_bits(r)));
                        }
                    }
                }
            }
        }
        let report = serde_json::json!({"level_code": code, "transmitters": cam.transmitters.len(), "transmitter_mismatches": t_mismatches,
            "trigger_mismatches": trig_mismatches,
            "cams_mismatches": c_mismatches, "pb_mismatches": pb_mismatches, "quiet": !moved, "update_mismatches": s_mismatches,
            "cur3d_after": cam.cur3d});
        println!("{}", serde_json::to_string(&report)?);
        return Ok(());
    }
    if args.len() == 2 && args[0] == "lookouts" {
        // lookouts WORLDS.PS2: lookout records with CreateYPRMatrix(rotation) + position (stdout JSON).
        let world = gdl_formats::world::World::parse(&bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?)?;
        let mut out = Vec::new();
        for l in gdl_core::game::lookouts::build(&world.locators) {
            let m = gdl_core::game::lookouts::lookout_matrix(&l).map_err(|e| e.0)?;
            out.push(serde_json::json!({"param": l.param, "next": l.next, "position": l.position, "rows": m}));
        }
        println!("{}", serde_json::to_string(&out)?);
        return Ok(());
    }
    if args.len() == 2 && args[0] == "memcard-list" {
        // memcard-list CARD.ps2: read-only directory listing with file hashes (stdout).
        let b = bounded_read(Path::new(&args[1]), 16 * 1024 * 1024)?;
        let card = gdl_formats::memcard::MemoryCard::parse(&b)?;
        println!("card sha256 {} stride {} flags {:#x}", hash(&b), card.superblock.stride, card.superblock.card_flags);
        for d in card.root()? {
            println!("{:#06x} {:>8} {}", d.mode, d.length, d.name);
            if d.is_dir() {
                for f in card.list(&d)? {
                    let h = if f.is_dir() { String::new() } else { hash(&card.read(&f)?) };
                    println!("    {:#06x} {:>8} {} {}", f.mode, f.length, f.name, h);
                }
            }
        }
        return Ok(());
    }
    if args.len() == 2 && args[0] == "anim-trees" {
        // anim-trees ANIM.PS2: tree labels with sequence label/frames/timing (stdout).
        let anim = gdl_formats::animation::Animations::parse(&bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?)?;
        for t in &anim.trees {
            let seqs: Vec<String> = t.sequences.iter().map(|s| format!("{}:{}x{}", s.label, s.frames, s.timing_factor)).collect();
            println!("{}\t{}\t{}\t{}", t.index, t.label, t.nodes.len(), seqs.join(" "));
        }
        return Ok(());
    }
    if args.len() == 6 && args[0] == "player-sim" {
        // player-sim WORLDS.PS2 SLUS_200.47 START.json INPUTS.json NEW_REPORT.json
        // INPUTS: [[levels, edges], ...] one per game update.
        let worlds = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let mut world = gdl_core::collision::CollisionWorld::from_world(&gdl_formats::world::World::parse(&worlds)?)?;
        let elf_bytes = bounded_read(Path::new(&args[2]), 64 * 1024 * 1024)?;
        let tables = gdl_core::controls::tables_from_elf(&gdl_formats::elf::Elf::parse(&elf_bytes)?)?;
        let start: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(&args[3]), 4 * 1024 * 1024)?)?;
        let mut sim: gdl_core::player_sim::PlayerSim = serde_json::from_value(start["sim"].clone())?;
        let inputs: Vec<[u32; 2]> = serde_json::from_slice(&bounded_read(Path::new(&args[4]), 4 * 1024 * 1024)?)?;
        if inputs.len() > 100_000 {
            return Err("simulation input policy exceeded".into());
        }
        let mut updates = Vec::new();
        for [levels, edges] in inputs {
            match sim.step(&mut world, &tables, levels, edges) {
                Ok(report) => updates.push(serde_json::json!({"report":report,
                    "position_bits":sim.position.map(f32::to_bits),"facing_bits":sim.facing.to_bits(),
                    "floor_y_bits":sim.floor_y_88c.to_bits(),"error":null})),
                Err(e) => {
                    updates.push(serde_json::json!({"error":e.to_string()}));
                    break;
                }
            }
        }
        let report = serde_json::json!({"updates":updates,"scope":"interim closed-loop PlayerSim; see player_sim.rs labels"});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[5])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 4 && args[0] == "level-enemies" {
        // level-enemies WDATA.wad LEVEL_INDEX NEW_REPORT.json
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let world = gdl_formats::wdata::WorldData::parse(&raw)?;
        let index: usize = args[2].to_str().ok_or("level index")?.parse()?;
        let level = world.levels.get(index).ok_or("level index out of range")?;
        let world_kind = 0; // world_data[0] is checked only against tower (0xd).
        let types = gdl_core::level::enemy_types(&world, level, world_kind, 3)?;
        let report = serde_json::json!({"wdata_sha256":hash(&raw),"level":level,"enemies":world.enemies,
            "chunks":world.chunks,"enemy_types":types});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[3])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 7 && args[0] == "item-records" {
        // item-records WORLDS.PS2 WDATA.wad LEVEL SLUS_200.47 SESSION.json NEW_REPORT.json
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let world = gdl_formats::world::World::parse(&raw)?;
        let wraw = bounded_read(Path::new(&args[2]), 4 * 1024 * 1024)?;
        let wdata = gdl_formats::wdata::WorldData::parse(&wraw)?;
        let index: usize = args[3].to_str().ok_or("level index")?.parse()?;
        let level = wdata.levels.get(index).ok_or("level index out of range")?;
        let elf_raw = bounded_read(Path::new(&args[4]), 64 * 1024 * 1024)?;
        let elf = gdl_formats::elf::Elf::parse(&elf_raw)?;
        let session: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(&args[5]), 1024 * 1024)?)?;
        let int = |k: &str| -> Result<i64, Box<dyn std::error::Error>> {
            session[k].as_i64().ok_or_else(|| format!("session field {k}").into())
        };
        let cur_world = int("cur_world")? as i32;
        let world_kind = if cur_world == 0xd { 0xd } else { 0 };
        let types = gdl_core::level::enemy_types(&wdata, level, world_kind, int("mode_950")? as i32)?;
        let level_raw = gdl_core::level::resolved_level(level, &elf, int("difficulty")? as i32)?;
        let session = gdl_core::items::ItemSession {
            multipliers: gdl_core::level::multipliers_cc(&level_raw),
            enemy_kind: types.kind,
            gen_kind: types.gen_kind,
            boss_type: int("boss_type")? as i32,
            cur_world,
            num_virtual_players: int("num_virtual_players")? as i32,
            game_mode: int("game_mode")? as u32,
            global_958: int("global_958")? as i32,
            global_970: int("global_970")? as i32,
            random_item_num: int("random_item_num")? as u32,
            golem_statue_anim: int("golem_statue_anim")? != 0,
            death_statue_anim: int("death_statue_anim")? != 0,
            critter_3_loaded: int("critter_3_loaded")? != 0,
            critter_7_loaded: int("critter_7_loaded")? != 0,
            tower_crystals_taken: session["tower_crystals_taken"].as_i64().unwrap_or(0) != 0,
            battle_wave_1_done: session["battle_wave_1_done"].as_i64().unwrap_or(0) != 0,
            tower_new_player: session["tower_new_player"].as_i64().unwrap_or(0) != 0,
            sound_names: Vec::new(),
        };
        let tables = gdl_core::items::ItemTables::from_elf(&elf)?;
        let mut collision = gdl_core::collision::CollisionWorld::from_world(&world)?;
        let seed = int("rng_seed")? as u32;
        // NATIVE: without the load-time world_update (no world animation state
        // here) and AddItemInstList's ProcessItemWobjs (no game globals);
        // items on moving floors and elevators are placed on the rest pose.
        let loaded = gdl_core::items::load_items(&world, &mut collision, tables, session, seed, &mut |_, _| Ok(()), &mut |_, _| Ok(()))?;
        let w = &loaded.world;
        let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
        let report = serde_json::json!({
            "worlds_sha256":hash(&raw),"wdata_sha256":hash(&wraw),"elf_sha256":hash(&elf_raw),
            "records":w.records.iter().map(|r| hex(&r.0)).collect::<Vec<_>>(),
            "outcome":loaded.outcome.iter().map(|o| match o { Ok(s) => serde_json::json!({"slot":s}), Err(e) => serde_json::json!({"error":e}) }).collect::<Vec<_>>(),
            "definitions":w.definitions.iter().map(|d| hex(&d.0)).collect::<Vec<_>>(),
            "wobjs":w.wobjs.iter().map(|o| serde_json::json!({"object":o.object,"inity":o.inity.to_bits(),"y":o.y.to_bits(),
                "starty":o.starty.to_bits(),"endy":o.endy.to_bits(),"snd":o.snd.to_bits()})).collect::<Vec<_>>(),
            "objects":w.objects.iter().map(|o| [o.flags_10, u32::from(o.word_14), u32::from(o.byte_16), u32::from(o.byte_17)]).collect::<Vec<_>>(),
            "world_has_gates":w.world_has_gates,"max_level_coins":loaded.max_level_coins,"random_calls":loaded.random_calls,
            "rune_item":w.rune_item,"quest_item":w.quest_item,
            "scope":"native AddItemInstList through PlaceItem; presentation bytes zero; ProcessItemWobjs not applied; rand seeded from rng_seed (values not comparable)"});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[6])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() >= 9 && args[0] == "game-run" {
        // game-run WORLDS.PS2 WDATA.wad LEVEL SLUS_200.47 START_V4.json ITEMS_ANIM.PS2 FRAMES NEW_REPORT.json [TYPE=MONSTER_ANIM.PS2 ...]
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let world = gdl_formats::world::World::parse(&raw)?;
        let wraw = bounded_read(Path::new(&args[2]), 4 * 1024 * 1024)?;
        let wdata = gdl_formats::wdata::WorldData::parse(&wraw)?;
        let level: usize = args[3].to_str().ok_or("level")?.parse()?;
        let elf_raw = bounded_read(Path::new(&args[4]), 64 * 1024 * 1024)?;
        let elf = gdl_formats::elf::Elf::parse(&elf_raw)?;
        let mut start: gdl_core::game::setup::StartState = serde_json::from_slice(&bounded_read(Path::new(&args[5]), 16 * 1024 * 1024)?)?;
        let iraw = bounded_read(Path::new(&args[6]), 64 * 1024 * 1024)?;
        let ianim = gdl_formats::animation::Animations::parse(&iraw)?;
        let frames: usize = args[7].to_str().ok_or("frames")?.parse()?;
        let mut item_anims = std::collections::HashMap::new();
        let mut item_anim_nodes = std::collections::HashMap::new();
        gdl_core::game::setup::add_item_trees(&mut item_anims, &mut item_anim_nodes, &ianim);
        let mut enemy_anims = gdl_core::game::enemy_ai::EnemyAnimData::default();
        let mut opts = std::collections::HashMap::new();
        let mut critters = Vec::new();
        for a in &args[9..] {
            let a = a.to_str().ok_or("monster arg")?;
            let (t, path) = a.split_once('=').ok_or("TYPE=PATH:PREFIX expected")?;
            if t == "critter" {
                // critter=CRITTER.WAD[:MODEL_ANIM.PS2]
                let (wad, model) = match path.split_once(':') {
                    Some((w, m)) => (w, Some(m)),
                    None => (path, None),
                };
                let w = gdl_formats::critter::CritterWad::parse(&bounded_read(Path::new(wad), 4 * 1024 * 1024)?)?;
                let m = match model {
                    Some(m) => {
                        let bytes = bounded_read(Path::new(m), 64 * 1024 * 1024)?;
                        Some(std::sync::Arc::new((gdl_formats::animation::Animations::parse(&bytes)?, bytes)))
                    }
                    None => None,
                };
                critters.push((wad.to_string(), w, m));
                continue;
            }
            if t.parse::<i32>().is_err() {
                opts.insert(t.to_string(), path.to_string());
                continue;
            }
            let (path, prefix) = path.rsplit_once(':').ok_or("TYPE=PATH:PREFIX expected")?;
            let m = gdl_formats::animation::Animations::parse(&bounded_read(Path::new(path), 64 * 1024 * 1024)?)?;
            gdl_core::game::setup::enemy_trees(&mut enemy_anims, t.parse()?, prefix, &m);
        }
        // AtreeMatchAnyHeader 0x14fa60 (an item definition's tree, +0x4c):
        // item_anim, powerup_anim, then item_anim2 (generators then try the
        // enemy packages; not modelled — no G1 generator needs it).
        if let Some(p) = opts.get("powerups") {
            let pw = gdl_formats::animation::Animations::parse(&bounded_read(Path::new(p), 64 * 1024 * 1024)?)?;
            gdl_core::game::setup::add_item_trees(&mut item_anims, &mut item_anim_nodes, &pw);
        }
        if let Some(extra) = opts.get("items2") {
            // item_anim2: the level's own package (after the realm's).
            let more = gdl_formats::animation::Animations::parse(&bounded_read(Path::new(extra), 64 * 1024 * 1024)?)?;
            gdl_core::game::setup::add_item_trees(&mut item_anims, &mut item_anim_nodes, &more);
        }
        if let Some(w) = opts.get("world") {
            // Other realms: CurWorld/CurLevel for the session tables (the
            // player record is still the one from the save).
            start.globals.cur_world = w.parse()?;
            start.globals.cur_level = level as i32;
        }
        let anim_of = |k: &str| -> Result<gdl_formats::animation::Animations, Box<dyn Error>> {
            let path = opts.get(k).ok_or_else(|| format!("{k}=PATH required"))?;
            Ok(gdl_formats::animation::Animations::parse(&bounded_read(Path::new(path), 64 * 1024 * 1024)?)?)
        };
        let effect_tables = gdl_core::game::effects::EffectTables::from_elf(&elf, &anim_of("weapons")?, &anim_of("powerups")?)?;
        let start_rune_disp = start.globals.rune_disp_timer;
        let mut game = gdl_core::game::Game::new_level(gdl_core::game::setup::LevelInputs {
            world: &world, wdata: &wdata, level, elf: &elf, start, enemy_anims, item_anims, item_anim_nodes, effect_tables,
            // load_players=N: the select-screen entry (the Tower of a new game: 0).
            load_in_game_players: match opts.get("load_players") {
                Some(n) => Some(n.parse()?),
                None => None,
            },
            audio_rom: match opts.get("rom") {
                Some(p) => Some(gdl_formats::audio::AudioRom::parse(&bounded_read(Path::new(p), 4 * 1024 * 1024)?)?),
                None => None,
            },
            mb_objects: match opts.get("objects") {
                Some(list) => {
                    let mut set = std::collections::HashSet::new();
                    for p in list.split(',') {
                        let o = gdl_formats::objects::Objects::parse(&bounded_read(Path::new(p), 64 * 1024 * 1024)?)?;
                        set.extend(o.definitions.iter().map(|d| d.label.clone()));
                    }
                    Some(set)
                }
                None => None,
            },
            critters })?;
        // Replays of traces that start mid-level keep the start state's
        // RuneDispTimer rather than init_next_level's 300 (traces since
        // trace-a1-newgame-v2 seed their own per update).
        if opts.contains_key("replay") || opts.contains_key("enemy_replay") {
            game.globals.rune_disp_timer = start_rune_disp;
        }
        // init_next_level's player displays (reset_players, setup_player_display,
        // SetPlayerWindows) as gdl_play's init_hud runs them, without textures:
        // WritePlayerInfo keeps its record words (+0x904 show timer, meter
        // time) in headless runs too.
        // text=TEXT/ENGLISH.ROM (GameText): NewHelp's logic (help pause,
        // seen marks, voice lines) runs with it; the layout is not drawn.
        let text = match opts.get("text") {
            Some(p) => Some(gdl_formats::textrom::TextRom::parse(&bounded_read(Path::new(p), 4 * 1024 * 1024)?)?),
            None => None,
        };
        game.hud_init(Vec::new(), Vec::new(), text);
        // Optional scripted player: sim=START_SIM.json clips=SOR_CLIPS.PS2 script=LEVELS_HEX:UPDATES,...
        let mut driver = match (opts.get("sim"), opts.get("script")) {
            (Some(sim_path), Some(_)) => {
                let st: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(sim_path), 4 * 1024 * 1024)?)?;
                let sim: gdl_core::player_sim::PlayerSim = serde_json::from_value(st["sim"].clone())?;
                let map: Vec<[i64; 2]> = serde_json::from_value(st["presentation"]["action_map"].clone())?;
                let clips = anim_of("clips")?;
                let tables = gdl_core::controls::tables_from_elf(&elf)?;
                Some(gdl_core::game::players::PlayerDriver::new(sim, tables, map, gdl_core::game::setup::clips_of(&clips, 0)))
            }
            _ => None,
        };
        if let (Some(d), true) = (driver.as_mut(), 0 < level || opts.contains_key("world")) {
            // Later levels: enter at the start locator (get_player_pos path).
            let loc = world.locators.iter().find(|l| l.kind == 7 && l.id == 0).ok_or("no start locator")?;
            game.place_player_start(0, &mut d.sim, loc.position, loc.rotation[1]).map_err(|e| e.to_string())?;
            d.sim.current_action = 0;
            d.sim.requested_action = 0;
        }
        if let (Some(t), Some(d)) = (opts.get("teleport"), driver.as_mut()) {
            // DEBUG: start player 0 at x,y,z through the start-locator path
            // (not a fidelity input; recorded in the report).
            let v: Vec<f32> = t.split(',').map(str::parse).collect::<Result<_, _>>()?;
            let p = [*v.first().ok_or("teleport x")?, *v.get(1).ok_or("teleport y")?, *v.get(2).ok_or("teleport z")?];
            game.place_player_start(0, &mut d.sim, p, 0.0).map_err(|e| e.to_string())?;
            d.sim.current_action = 0;
            d.sim.requested_action = 0;
        }
        // init_next_level's StdCamInit: a fresh level start runs the stage-start
        // sequence; fixture starts settle the camera (HYPOTHESIS, camera.md).
        if 0 < level || opts.contains_key("world") || opts.contains_key("teleport") {
            game.start_camera().map_err(|e| e.0)?;
        } else {
            game.start_camera_settled().map_err(|e| e.0)?;
        }
        if let Some(h) = opts.get("boss_health") {
            // DEBUG: start the boss with this health (not a fidelity input).
            let h: f32 = h.parse()?;
            for c in game.critters.iter_mut().flatten() {
                c.rec.set_f32(0x4f4, h);
            }
        }
        if let Some(path) = opts.get("pdata") {
            // PDATA/<CLS>.WAD of player 0's class: the SFXX/DAMG tables.
            let data = gdl_formats::pdata::PlayerData::parse(&bounded_read(Path::new(path), 1024 * 1024)?)?;
            game.classes[0].sfxx = data.chunk("SFXX").map(|c| c.bytes.clone()).unwrap_or_default();
            game.classes[0].damg = data.chunk("DAMG").map(|c| c.bytes.clone()).unwrap_or_default();
        }
        if opts.contains_key("sfx") {
            // PLAYERS/<CLS>/SFX<COL>/ANIM.PS2 of player 0: InitPlayerMissiles'
            // WeapThrowFx (load_player).
            let a = anim_of("sfx")?;
            game.classes[0].sfx_trees = a.trees.iter().map(|t| (t.label.clone(), t.sequences.first().map_or(0, |s| s.frames), t.sequences.first().map_or(0, |s| s.timing_factor), t.sequences.first().map_or(0, |s| s.repeat))).collect();
            game.init_player_missiles(0);
            game.resolve_plyr_data(0);
            eprintln!("WeapThrowFx[0] = {:?}", game.effects.weap_throw_fx[0]);
        }
        if let Some(v) = opts.get("weapon") {
            // DEBUG: a kind-5 weapon powerup "TYPE:SECONDS" for player 0.
            let (t, secs) = v.split_once(':').ok_or("weapon=TYPE:SECONDS")?;
            let class = game.classes[0].clone();
            game.add_powerup(0.0, secs.parse()?, 0, 5, t.parse()?, &class).map_err(|e| e.0)?;
        }
        if let Some(v) = opts.get("plevel") {
            // DEBUG: player 0's level (+0x2ac0).
            game.players[0].set_i32(0x2ac0, v.parse()?);
        }
        if opts.contains_key("legend") {
            // DEBUG: player 0 holds this world's legend item (class word +0xcf4);
            // GauntWorldInit's check then runs as at a level start.
            let o = 0xcf4 + 0xb4 * game.players[0].u32_(0xc) as usize;
            let w = game.globals.cur_world;
            let v = game.players[0].u16_(o) | (1u16 << (w & 0xf));
            game.players[0].set_u16(o, v);
            if game.globals.quest_item_state == 0 && game.globals.boss_type < 0x2b && 0 <= game.globals.boss_type {
                game.players[0].set_i32(0x80c, 1);
                game.globals.quest_item_state = 1;
            }
        }
        if let Some(n) = opts.get("potions") {
            // DEBUG override recorded in the report scope.
            game.players[0].set_i32(0x1a18, n.parse()?);
        }
        if let Some(v) = opts.get("crystals") {
            // DEBUG: player 0's crystal count for realm index N, "N:COUNT".
            let (n, c) = v.split_once(':').ok_or("crystals=N:COUNT")?;
            let o = 0xd0a + 0xb4 * game.players[0].u32_(0xc) as usize + 2 * n.parse::<usize>()?;
            game.players[0].set_i16(o, c.parse()?);
        }
        let mut script: Vec<u32> = Vec::new();
        if let Some(sc) = opts.get("script") {
            for part in sc.split(',') {
                let (lv, n) = part.split_once(':').ok_or("script LEVELS_HEX:UPDATES")?;
                let lv = u32::from_str_radix(lv, 16)?;
                script.extend(std::iter::repeat_n(lv, n.parse::<usize>()?));
            }
        }
        // Record-based players: pads=MASK_HEX:UPDATES,... where bit i presses
        // joyGetStatus entry i of port 0 (clips=CLASS_ANIM.PS2 required).
        let mut pad_script: Vec<u32> = Vec::new();
        if let Some(sc) = opts.get("pads") {
            for part in sc.split(',') {
                let (m, n) = part.split_once(':').ok_or("pads MASK_HEX:UPDATES")?;
                let m = u32::from_str_radix(m, 16)?;
                pad_script.extend(std::iter::repeat_n(m, n.parse::<usize>()?));
            }
            let clips = anim_of("clips")?;
            game.player_clips[0] = gdl_core::game::setup::clips_of(&clips, 0);
            // load_player's +0x6b4: tree 0's DUMMY node (the client also checks
            // the model package for "<prefix>DUMMY"; not loaded here).
            let clip_bytes = bounded_read(Path::new(opts.get("clips").ok_or("clips=PATH required")?), 64 * 1024 * 1024)?;
            let dummy = clips.trees.first().and_then(|t| t.nodes.iter().position(|n| n.label == "DUMMY"));
            game.player_rigs[0] = Some(std::sync::Arc::new(gdl_core::game::teammove::PlayerRig { anim: gdl_formats::animation::Animations::parse(&clip_bytes)?, bytes: clip_bytes, dummy }));
            game.players[0].set_u32(0x6b4, dummy.map_or(0, |n| n as u32 + 1));
            game.controls.assign(0, 0);
            if 0 < level || opts.contains_key("world") {
                let loc = world.locators.iter().find(|l| l.kind == 7 && l.id == 0).ok_or("no start locator")?;
                game.place_player(0, loc.position, loc.rotation[1]).map_err(|e| e.0)?;
                game.players[0].set_u32(0x1e0, 0);
                game.players[0].set_u32(0x1e4, 0);
            }
            game.init_player_anim(0).map_err(|e| e.0)?;
            // players=N (2..4): copies of player 0 (index +0 and colour +4 =
            // k, no input) placed as init_next_level does: load_player_geo →
            // get_player_pos(k, 1).
            let n: usize = opts.get("players").map_or(Ok(1), |v| v.parse())?;
            if !(1..=4).contains(&n) {
                return Err("players=1..4".into());
            }
            for k in 1..n {
                let mut r = game.players[0].clone();
                r.set_u32(0, k as u32);
                r.set_u32(4, k as u32);
                game.players[k] = r;
                game.classes[k] = game.classes[0].clone();
                game.init_player_missiles(k);
                game.resolve_plyr_data(k);
                game.player_clips[k] = game.player_clips[0].clone();
                game.player_rigs[k] = game.player_rigs[0].clone();
                let (start, yaw) = game.player_start;
                game.place_player(k, start, yaw).map_err(|e| e.0)?;
                game.load_player_geo_place(k).map_err(|e| e.0)?;
                game.players[k].set_u32(0x1e0, 0);
                game.players[k].set_u32(0x1e4, 0);
                game.init_player_anim(k).map_err(|e| e.0)?;
                game.controls.assign(k, k);
                game.globals.active_players |= 1 << k;
                eprintln!("player {k} placed at {:?} yaw {}", game.players[k].v3(0x50), game.players[k].f32_(0x86c));
            }
            // DEBUG turbo=V: player 0's turbo (+0x800). front=1: player 1
            // stands 2 ahead of player 0, facing it (team-move checks).
            if let Some(v) = opts.get("turbo") {
                game.players[0].set_f32(0x800, v.parse()?);
            }
            if opts.get("front").is_some_and(|v| v == "1") && 2 <= n {
                let (p0, yaw0) = (game.players[0].v3(0x50), game.players[0].f32_(0x86c));
                let at = [p0[0] + 2.0 * yaw0.sin(), p0[1], p0[2] + 2.0 * yaw0.cos()];
                let yaw = game.players[0].f32_(0x86c) + std::f32::consts::PI;
                game.place_player(1, at, yaw).map_err(|e| e.0)?;
                game.players[1].set_u32(0x1e0, 0);
                game.players[1].set_u32(0x1e4, 0);
                eprintln!("player 1 in front at {at:?}");
            }
        }
        // items_seed=FILE (tools/python/export_live_items.py): the fixture's
        // live item records and item world-object words in the native
        // convention replace the freshly loaded ones (replays).
        if let Some(ip) = opts.get("items_seed") {
            let hexb = |v: &serde_json::Value| -> Result<Vec<u8>, Box<dyn Error>> {
                let s = v.as_str().ok_or("hex field")?;
                (0..s.len()).step_by(2).map(|i| Ok(u8::from_str_radix(&s[i..i + 2], 16)?)).collect()
            };
            let v: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(ip), 64 * 1024 * 1024)?)?;
            for r in v["records"].as_array().ok_or("items_seed records")? {
                let slot = r["slot"].as_u64().ok_or("slot")? as usize;
                let bytes = hexb(&r["record"])?;
                let rec: [u8; 0x110] = bytes.try_into().map_err(|_| "item record size")?;
                while game.items.records.len() <= slot {
                    game.items.records.push(gdl_core::items::ItemRecord([0; 0x110]));
                }
                game.items.records[slot] = gdl_core::items::ItemRecord(rec);
            }
            for (k, o) in v["objects"].as_array().ok_or("items_seed objects")?.iter().enumerate() {
                if let Some(w) = game.items.objects.get_mut(k) {
                    let f = |n: usize| o[n].as_u64().unwrap_or(0);
                    w.flags_10 = f(0) as u32;
                    w.word_14 = f(1) as u16;
                    w.byte_16 = f(2) as u8;
                    w.byte_17 = f(3) as u8;
                }
            }
            game.sync_collision();
        }
        if let Some(tp) = opts.get("enemy_replay") {
            // Per-update replay of a reference_trace --player --enemies trace
            // through the native do_enemies: each update starts from the
            // reference's 25 enemy records (animation clocks from the +0x84
            // AnimInfo, the tree's level found by its InitActions table at
            // +0xe8), player 0, controls and globals, with SetupDynGrid rebuilt;
            // the visibility flags are the reference result's +0x2ee/+0x2f0.
            // main reseeds rand every loop (srand(pbGetCPUTime() >> 3)), so the
            // seed is solved from the reference's state after the update by
            // inverting newlib's LCG over the native call count.
            let trace: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(tp), 256 * 1024 * 1024)?)?;
            let samples = trace["samples"].as_array().ok_or("trace samples")?;
            let clips = anim_of("clips")?;
            game.player_clips[0] = gdl_core::game::setup::clips_of(&clips, 0);
            let hexb = |v: &serde_json::Value| -> Result<Vec<u8>, Box<dyn Error>> {
                let s = v.as_str().ok_or("hex field")?;
                (0..s.len()).step_by(2).map(|i| Ok(u8::from_str_radix(&s[i..i + 2], 16)?)).collect()
            };
            let word = |b: &[u8]| u32::from_le_bytes(b[..4].try_into().unwrap());
            let fl = |b: &[u8]| f32::from_le_bytes(b[..4].try_into().unwrap());
            const LCG_MUL: u32 = 0x41c6_4e6d;
            const LCG_INV: u32 = {
                // Inverse of LCG_MUL modulo 2^32 (Newton iteration).
                let mut x: u32 = LCG_MUL;
                let mut i = 0;
                while i < 5 {
                    x = x.wrapping_mul(2u32.wrapping_sub(LCG_MUL.wrapping_mul(x)));
                    i += 1;
                }
                x
            };
            let back = |mut s: u32, n: u32| {
                for _ in 0..n {
                    s = s.wrapping_sub(0x3039).wrapping_mul(LCG_INV);
                }
                s
            };
            let steps_between = |from: u32, to: u32| -> Option<u32> {
                let mut s = from;
                for n in 0..200_000u32 {
                    if s == to {
                        return Some(n);
                    }
                    s = s.wrapping_mul(LCG_MUL).wrapping_add(0x3039);
                }
                None
            };
            const ESIZE: usize = 0x3b0;
            // Guest pointers: presentation/tree/anim links stay raw (the native
            // only tests +0x80 for zero) and are not compared; the generator
            // (+0x2a4) and blocking-item (+0x2a0) links (Items records) and the
            // floor link (+0x2ac, a WorldBase object of 0x3c bytes) become the
            // native slot/object + 1.
            // +0x204: the skin texture word (a texture handle in the
            // original, a SkinTex code natively).
            let pointer_words: std::collections::HashSet<usize> = [0x70, 0x80, 0x84, 0x88, 0x8c, 0xc0, 0xc4, 0x1f0, 0x204].into_iter().collect();
            let s0 = samples.first().ok_or("empty trace")?;
            let items_base = word(&hexb(&s0["enemies"]["Items"])?);
            let world_base = word(&hexb(&s0["enemies"]["WorldBase"])?);
            let link = |v: u32, base: u32, size: u32| -> u32 {
                if v == 0 || v < base || !(v - base).is_multiple_of(size) {
                    return if v == 0 { 0 } else { u32::MAX };
                }
                (v - base) / size + 1
            };
            let native_words = |raw: &[u8]| -> Vec<u8> {
                let mut r = raw.to_vec();
                for k in 0..r.len() / ESIZE {
                    for (o, base, size) in [(0x2a4, items_base, 0x110), (0x2a0, items_base, 0x110), (0x2ac, world_base, 0x3c)] {
                        let at = k * ESIZE + o;
                        let v = link(u32::from_le_bytes(r[at..at + 4].try_into().unwrap()), base, size);
                        r[at..at + 4].copy_from_slice(&v.to_le_bytes());
                    }
                }
                r
            };
            let mut updates = Vec::new();
            let mut equal_updates = 0;
            let mut by_offset: std::collections::BTreeMap<usize, usize> = std::collections::BTreeMap::new();
            // Traces with items_delta seed every item from the before state
            // (the +0xd8 0x4000 visible bit from the after state).
            let mut ref_before = ReferenceItems::default();
            let mut have_items = ref_before.apply(s0)?;
            // items_replay: ProcessItems runs first (native visibility from
            // the seeded StdCam view) and the Items array is compared too.
            let items_mode = opts.contains_key("items_replay") || opts.contains_key("frame_replay");
            if items_mode && !have_items {
                return Err("items_replay needs a trace with items_delta".into());
            }
            // frame_replay: the whole game_main update — ProcessItems,
            // do_enemies, do_players, ProcessEffects — from the before state
            // (effects from the trace's effects_delta), compared after.
            let frame_mode = opts.contains_key("frame_replay");
            let mut fx_before = ReferenceEffects::default();
            let have_fx = fx_before.apply(s0)?;
            if frame_mode && !have_fx {
                return Err("frame_replay needs a trace with effects_delta".into());
            }
            let player_pointer_words: std::collections::HashSet<usize> = [0x80, 0x90, 0x94, 0x98, 0x9c, 0x6a0, 0x6a4, 0x6a8, 0x6ac, 0x6b0, 0x6b4, 0x6b8, 0x6bc, 0x704, 0x708, 0x7c0, 0x880, 0x884, 0x888, 0x89c, 0x8a0, 0x690, 0x694]
                .into_iter().collect();
            let player_hud_words: std::collections::HashSet<usize> = [0x878, 0x87c, 0x2ae0].into_iter().collect();
            let mut obj_before = ReferenceObjects::default();
            obj_before.apply(s0)?;
            // Critters (traces from trace-k2-newgame-v3): seeded before the
            // update and compared after.
            let mut crit_before = ReferenceCritters::default();
            let have_crit = frame_mode && crit_before.apply(s0)? && !crit_before.globals.is_empty();
            let mut critter_updates_equal = 0;
            let mut camera_updates_equal = 0;
            let mut world_anim_updates_equal = 0;
            let mut player_updates_equal = 0;
            let mut effect_updates_equal = 0;
            let mut item_updates_equal = 0;
            let mut by_item_offset: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
            // Per item slot over every update (item_diffs keeps 60 per update).
            let mut by_item_slot: std::collections::BTreeMap<u64, usize> = std::collections::BTreeMap::new();
            for w in samples.windows(2) {
                let (b, a) = (&w[0], &w[1]);
                let mut ref_after = ReferenceItems(ref_before.0.clone());
                have_items &= ref_after.apply(a)?;
                let ref_b = std::mem::replace(&mut ref_before, ref_after);
                let mut fx_after = fx_before.clone();
                if !fx_after.apply(a)? && frame_mode {
                    return Err("frame_replay: a sample without effects_delta".into());
                }
                let fx_b = std::mem::replace(&mut fx_before, fx_after);
                let mut obj_after = obj_before.clone();
                obj_after.apply(a)?;
                let obj_b = std::mem::replace(&mut obj_before, obj_after);
                let mut crit_after = crit_before.clone();
                if have_crit && !crit_after.apply(a)? {
                    return Err("frame_replay: a sample without critters_delta".into());
                }
                let crit_b = std::mem::replace(&mut crit_before, crit_after);
                if b["observation"]["scalars"]["AFrame"]["raw_u32"] == a["observation"]["scalars"]["AFrame"]["raw_u32"] {
                    continue;
                }
                let eb = native_words(&hexb(&b["enemies"]["enemies"])?);
                let ea = native_words(&hexb(&a["enemies"]["enemies"])?);
                let rand_after = word(&hexb(&a["enemies"]["rand_next"])?);
                let seed_all = |game: &mut gdl_core::game::Game| -> Result<(), Box<dyn Error>> {
                    let (bp, ap) = (&b["player"], &a["player"]);
                    game.players[0] = gdl_core::game::record::Record(hexb(&bp["player0"])?);
                    let ctl = hexb(&ap["control"])?;
                    for k in 0..4 {
                        game.controls.control[k] = gdl_core::game::record::Record(ctl[k * 0x3c..(k + 1) * 0x3c].to_vec());
                    }
                    let g = |n: &str| -> Result<Vec<u8>, Box<dyn Error>> { hexb(&ap[n]) };
                    let gl = &mut game.globals;
                    gl.time = fl(&g("Time")?);
                    gl.spf = fl(&g("SPF")?);
                    gl.atime = fl(&g("ATime")?);
                    gl.anim_time_scale = fl(&g("AnimTimeScale")?);
                    gl.vb_elapsed = word(&g("vb_elapsed")?) as i32;
                    gl.game_mode = word(&g("game_mode")?);
                    // The camera (main, after game_main) ends the stage start:
                    // the update ran with the before sample's values
                    // (trace-a1-newgame-v2 steps 78 → 79).
                    gl.cam_wait = word(&hexb(&bp["cam_wait"])?) as i32;
                    gl.start_of_stage = word(&hexb(&bp["start_of_stage"])?) as i32;
                    gl.enemy_shrink = fl(&g("EnemyShrink")?);
                    gl.quest_item_state = word(&g("QuestItemState")?);
                    gl.boss_type = word(&g("BossType")?) as i32;
                    gl.boss_active = word(&g("BossActive")?) as i32;
                    gl.world_bottom = fl(&g("WldBtm")?);
                    gl.paused = word(&g("paused")?) as i32;
                    gl.help = word(&g("help")?) as i32;
                    gl.trig_cam_active = word(&g("trig_cam_active")?) as i32;
                    gl.num_active_players = word(&g("NumActivePlayers")?) as i32;
                    gl.boss_index = word(&hexb(&b["enemies"]["BossIndex"])?) as i32;
                    game.stop_time = word(&g("StopTime")?) as i32;
                    // world_update's start_enemyinfo from the update's
                    // starting start_of_stage (the before sample; the globals
                    // above are the after sample's) and LEVL +0x48.
                    game.start_enemyinfo = game.globals.start_of_stage == 0 || game.level.get(0x48..0x4c).is_some_and(|v| v != [0; 4]);
                    game.suicide_bomber_index = word(&hexb(&b["enemies"]["SuicideBomberIndex"])?) as i32;
                    // find_closest_player's stagger (frame_main & 7); traces
                    // before trace-enemies-g1-v3 lack it.
                    if let Some(v) = a["enemies"].get("frame_main") {
                        game.frame_main = word(&hexb(v)?) as i32;
                    }
                    // The enemy nodes hold what UpdateObjWorldMat stored last
                    // update: derived from the seeded records.
                    game.enemy_node = vec![None; game.enemies.len()];
                    for k in 0..game.enemies.len().min(eb.len() / ESIZE) {
                        let rec = gdl_core::game::record::Record(eb[k * ESIZE..(k + 1) * ESIZE].to_vec());
                        let (t, st) = (rec.i32_(0), rec.i32_(0xc8));
                        game.enemies[k] = rec;
                        game.enemy_anims[k] = None;
                        if st == 0 {
                            continue;
                        }
                        let r = &game.enemies[k];
                        let level = (1..=3).find(|l| {
                            game.enemy_anim_data.trees.get(&(t, *l)).is_some_and(|(_, tab)| {
                                tab.iter().enumerate().all(|(i, (idx, v))| r.i32_(0xe8 + 8 * i) == *idx && r.i32_(0xec + 8 * i) == *v)
                            })
                        });
                        let Some(level) = level else { continue };
                        let clips = game.enemy_anim_data.trees[&(t, level)].0.clone();
                        // The +0x84 AnimInfo: the player's +0x94 layout less 0x10.
                        let clock = gdl_core::animation_clock::ClockState {
                            sequence: r.u16_(0x92),
                            previous_sequence: r.u16_(0xa0),
                            reset_byte: r.u8_(0x96),
                            option_bits: r.u8_(0x97),
                            enabled_word: r.u16_(0xa2),
                            loop_word: r.u16_(0xb8),
                            mode_bits: r.u16_(0xba),
                            frame: r.f32_(0x9c),
                            blend_fraction: r.f32_(0x98),
                            start_time: r.f32_(0xa4),
                            blend_end_time: r.f32_(0xa8),
                            period_scale: r.f32_(0xb0),
                            last_time: r.f32_(0xb4),
                        };
                        let current = clock.sequence;
                        game.enemy_anims[k] = Some(gdl_core::game::enemy_ai::EnemyAnim { clock, clips, current, tree: (t, level) });
                    }
                    // The generator item records the enemies link to (traces
                    // from trace-enemies-g1-v4): +0x00 definition pointer →
                    // index (WorldInfo +0x68, 0x50 bytes), presentation words
                    // (+0x70..+0xc7, +0xe6 dyngrid link) left native.
                    if have_items {
                        ref_b.seed(game, &hexb(&b["enemies"]["WorldInfo"])?, items_base, (!items_mode).then_some(&ref_before))?;
                    } else if let (Some(gens), Some(wi)) = (b["enemies"]["generator_items"].as_object(), b["enemies"].get("WorldInfo")) {
                        let wi = hexb(wi)?;
                        let defs = word(&wi[0x68..]);
                        for (slot, hex) in gens {
                            let slot: usize = slot.parse()?;
                            let raw = hexb(hex)?;
                            let Some(rec) = game.items.records.get_mut(slot) else { continue };
                            let keep = rec.0;
                            let mut new = [0u8; 0x110];
                            new.copy_from_slice(&raw[..0x110]);
                            let ptr = word(&raw[0..]);
                            let def = if ptr == 0 { 0 } else { ptr.wrapping_sub(defs) / 0x50 };
                            new[0..4].copy_from_slice(&def.to_le_bytes());
                            new[0x70..0xc8].copy_from_slice(&keep[0x70..0xc8]);
                            new[0xe6..0xe8].copy_from_slice(&keep[0xe6..0xe8]);
                            rec.0 = new;
                        }
                    }
                    if items_mode {
                        // The view of the previous update's camera step.
                        if let Some(c) = game.std_camera.as_mut() {
                            c.std = gdl_core::game::record::Record(hexb(&bp["stdcam"])?);
                        }
                        game.replay_sync_view();
                        // Traces whose window block reaches the eye (+0xa0,
                        // from trace-b6-boss-v2): the view the reference
                        // culled with, also under the boss camera.
                        let win = hexb(&bp["win"])?;
                        if 0xac <= win.len() {
                            let v = |o: usize| [fl(&win[o..]), fl(&win[o + 4..]), fl(&win[o + 8..])];
                            game.replay_set_view([v(0x70), v(0x80), v(0x90)], v(0xa0));
                        }
                        // The camera state main's dispatch reads (traces from
                        // trace-b6-boss-v4): the boss camera record, mode and
                        // BCAM record, the transmitter indices, DoShake's
                        // globals and the window eye BossCamGetCurrent reads.
                        if let Some(bc_raw) = bp.get("bosscam") {
                            let gw = |n: &str| -> Result<i32, Box<dyn Error>> { Ok(word(&hexb(&bp[n])?) as i32) };
                            let (cur, cur3d) = (gw("CurTransmitter")?, gw("Cur3DTransmitter")?);
                            if let Some(bc) = game.boss_camera.as_mut() {
                                bc.rec = gdl_core::game::record::Record(hexb(bc_raw)?);
                                bc.mode = gw("BossCameraMode")?;
                                if let Some(d) = bp.get("bcam") {
                                    bc.data = Some(hexb(d)?);
                                }
                                bc.cur_transmitter = cur;
                            }
                            let shake = gdl_core::game::camera::Shake {
                                shaking: gw("shaking")?,
                                kind: gw("shake_type")?,
                                priority: gw("shake_priority")?,
                                delay: gw("shake_delay")?,
                                count: gw("shake_count")?,
                                radius: f32::from_bits(gw("shake_rad")? as u32),
                            };
                            if let Some(c) = game.std_camera.as_mut() {
                                c.cur3d = cur3d;
                                c.shake = shake;
                                if 0xac <= win.len() {
                                    c.pb.eye = [fl(&win[0xa0..]), fl(&win[0xa4..]), fl(&win[0xa8..])];
                                }
                            }
                            game.globals.cur_transmitter = cur;
                            game.globals.cur3d_transmitter = cur3d;
                            game.boss_cam_mode = gw("BossCamMode")?;
                        }
                        // Cam[0]'s matrix (traces from trace-b6-boss-v3).
                        if let Some(c0) = bp.get("cam0") {
                            let m = hexb(c0)?;
                            let v = |o: usize| [fl(&m[o..]), fl(&m[o + 4..]), fl(&m[o + 8..])];
                            if let Some(c) = game.std_camera.as_mut() {
                                c.cam0_mat = Some([v(0), v(0x10), v(0x20), v(0x30)]);
                            }
                        }
                    }
                    if frame_mode {
                        // Player 0 as the player replay seeds it: the floor link
                        // by PlayerCheckFloor's query, the item link as a slot +
                        // 1, other links cleared; SetPlayerVars' +0x93e clear;
                        // the node matrix from the trace.
                        // The floor link: the traced WORLDS object (0x3c bytes
                        // from WorldBase) as index + 1.
                        let floor_obj = {
                            let v = game.players[0].u32_(0x89c);
                            (v != 0 && world_base <= v && (v - world_base).is_multiple_of(0x3c)).then(|| ((v - world_base) / 0x3c) as usize)
                        };
                        {
                            let floor = floor_obj.map_or(0, |o| o as u32 + 1);
                            let r = &mut game.players[0];
                            let item = r.u32_(0x880);
                            r.set_u32(0x89c, floor);
                            for o in [0x880, 0x884, 0x888, 0x8a0, 0x690, 0x694, 0x6b4, 0x6b8] {
                                r.set_u32(o, 0);
                            }
                            if item >= items_base && (item - items_base).is_multiple_of(0x110) {
                                r.set_u32(0x880, (item - items_base) / 0x110 + 1);
                            }
                            r.set_u16(0x93e, 0);
                        }
                        // PlayerCheckMovingFloor parented the node to a flag-
                        // 0x1000 floor last update: the traced node is local
                        // to it.
                        game.player_floor_parent[0] = floor_obj.filter(|&o| game.collision.runtime_flags.get(o).is_some_and(|f| f & 0x1000 != 0));
                        game.player_node[0] = match bp.get("node") {
                            Some(v) => {
                                let n = hexb(v)?;
                                Some(std::array::from_fn(|i| std::array::from_fn(|j| fl(&n[0x10 * i + 4 * j..]))))
                            }
                            None => None,
                        };
                        let gl = &mut game.globals;
                        gl.aux_levels = word(&g("AuxLevels")?);
                        gl.skip_wave = word(&g("skip_wave")?) as i32;
                        gl.quit_level = word(&g("quit_level")?) as i32;
                        gl.auto_exit_flag = word(&g("auto_exit_flag")?) as i32;
                        let opt = g("options")?;
                        gl.options = word(&opt[0..]) as i32;
                        gl.options_4 = word(&opt[4..]);
                        gl.options_18 = word(&opt[0x18..]);
                        let win = hexb(&bp["win"])?;
                        gl.camera_90 = fl(&win[0x90..]);
                        gl.camera_98 = fl(&win[0x98..]);
                        let atime = gl.atime;
                        let player_node = word(&hexb(&bp["player0"])?[0x80..]);
                        fx_b.seed(game, items_base, atime, player_node);
                        if have_crit {
                            crit_b.seed(game, items_base, &obj_b)?;
                        }
                        // AddEnemyHitCol's alternating blood (traces from
                        // trace-fight-g1-v5).
                        if let Some(v) = b["enemies"].get("sfx_ene_bloodidx") {
                            game.sfx_ene_bloodidx = word(&hexb(v)?);
                        }
                        // NewHelp/DoHelp's globals (traces from trace-varied-g1-v2).
                        if b["enemies"].get("help_flag").is_some() {
                            let gw = |n: &str| -> Result<i32, Box<dyn Error>> { Ok(word(&hexb(&b["enemies"][n])?) as i32) };
                            let h = &mut game.hud.help;
                            h.flag = gw("help_flag")? != 0;
                            h.num = gw("help_num")?;
                            h.player = gw("help_player")?.clamp(0, 3) as usize;
                            h.time = gw("help_time")?;
                            h.interval = gw("help_interval")?;
                            h.interval_idx = gw("help_interval_idx")?.max(0) as usize;
                            h.lv = gw("help_lv")?;
                            h.testflag = gw("help_testflag")? != 0;
                        }
                        // RuneDispTimer (traces from trace-a1-newgame-v2).
                        if let Some(v) = b["enemies"].get("RuneDispTimer") {
                            game.globals.rune_disp_timer = word(&hexb(v)?) as i32;
                        }
                        // world_update's DoWorldAnimation from the traced
                        // entries (+6 flags, +8 frame) and object words (traces
                        // from trace-fight-g1-v6): the moving floors' matrices.
                        if let Some(wa) = b["enemies"].get("world_anims") {
                            let raw = hexb(wa)?;
                            obj_b.seed(game);
                            for (k, e) in game.world_anims.entries.iter_mut().enumerate().take(raw.len() / 16) {
                                e.flags = u16::from_le_bytes([raw[16 * k + 6], raw[16 * k + 7]]);
                                e.frame = fl(&raw[16 * k + 8..]);
                            }
                            game.sync_collision();
                            game.do_world_animation().map_err(|e| e.0)?;
                            game.sync_collision();
                        }
                    }
                    game.setup_dyn_grid().map_err(|e| e.0)?;
                    Ok(())
                };
                let vis: Vec<(bool, bool)> = (0..25)
                    .map(|k| {
                        let o = k * ESIZE;
                        (i16::from_le_bytes([ea[o + 0x2ee], ea[o + 0x2ef]]) != 0, i16::from_le_bytes([ea[o + 0x2f0], ea[o + 0x2f1]]) != 0)
                    })
                    .collect();
                let visible = |_: &gdl_core::game::Game, k: usize| vis.get(k).copied().unwrap_or((false, false));
                // Solve the seed: run, count the calls, re-seed from LCG^-n.
                let mut seed = rand_after;
                let mut solved = false;
                let mut res = Ok(());
                // Traces without the help globals: every solve attempt starts
                // from the help box state before the update.
                let help_before = game.hud.help.clone();
                // The native item side state an attempt may clear (DeleteItem:
                // floor parent, sound node/anchor, tree, clock, first empty
                // slot): every attempt starts from the update's
                // (trace-b1-newgame-v2 step 366's poison barrel).
                let item_side = (
                    game.items.floor_parent.clone(),
                    game.items.sound_node.clone(),
                    game.items.sound_anchor.clone(),
                    game.items.first_empty,
                    game.item_trees.clone(),
                    game.item_anims.clone(),
                    game.items.geo.clone(),
                );
                let attempt = |game: &mut gdl_core::game::Game, seed: u32| -> Result<Result<(), gdl_core::game::Unsupported>, Box<dyn Error>> {
                    if b["enemies"].get("help_flag").is_none() {
                        game.hud.help = help_before.clone();
                    }
                    (
                        game.items.floor_parent,
                        game.items.sound_node,
                        game.items.sound_anchor,
                        game.items.first_empty,
                        game.item_trees,
                        game.item_anims,
                        game.items.geo,
                    ) = item_side.clone();
                    seed_all(game)?;
                    game.rng.state = seed;
                    let mut res = if items_mode { game.process_items() } else { Ok(()) };
                    if res.is_ok() {
                        res = game.do_enemies(&visible);
                    }
                    if frame_mode && res.is_ok() {
                        res = game.do_players().map(|_| ());
                        if res.is_ok() {
                            res = game.process_effects();
                        }
                    }
                    Ok(res)
                };
                // The native clock lives beside the record: write it back to
                // the +0x84 AnimInfo for the comparison.
                let write_back = |game: &mut gdl_core::game::Game| {
                    for k in 0..25 {
                        if let Some(Some(an)) = game.enemy_anims.get(k) {
                            let c = an.clock;
                            let r = &mut game.enemies[k];
                            r.set_u16(0x92, c.sequence);
                            r.set_u16(0xa0, c.previous_sequence);
                            r.set_u8(0x96, c.reset_byte);
                            r.set_u8(0x97, c.option_bits);
                            r.set_u16(0xa2, c.enabled_word);
                            r.set_u16(0xb8, c.loop_word);
                            r.set_u16(0xba, c.mode_bits);
                            r.set_f32(0x9c, c.frame);
                            r.set_f32(0x98, c.blend_fraction);
                            r.set_f32(0xa4, c.start_time);
                            r.set_f32(0xa8, c.blend_end_time);
                            r.set_f32(0xb0, c.period_scale);
                            r.set_f32(0xb4, c.last_time);
                        }
                    }
                };
                for _ in 0..4 {
                    res = attempt(&mut game, seed)?;
                    if game.rng.state == rand_after {
                        solved = true;
                        break;
                    }
                    let n = steps_between(seed, game.rng.state).unwrap_or(0);
                    seed = back(rand_after, n);
                }
                write_back(&mut game);
                // A solved seed fixes the call count, not the draw order: a
                // wrong draw can change later consumers so that another count
                // is self-consistent too (trace-g1-newgame-v1 step 364: a
                // spawn direction decided whether the new enemy saw the
                // player or wandered with one more rand). When the solution
                // still differs, neighbouring counts are tried and the one
                // matching the reference best is kept.
                let mut alt_seed = false;
                if solved {
                    let after_player = if frame_mode { Some(hexb(&a["player"]["player0"])?) } else { None };
                    let wi_after = if items_mode { Some(hexb(&a["enemies"]["WorldInfo"])?) } else { None };
                    let score = |game: &gdl_core::game::Game| -> usize {
                        let mut n = 0;
                        for k in 0..25 {
                            for o in (0..ESIZE).step_by(4) {
                                if !pointer_words.contains(&o) && game.enemies[k].u32_(o) != word(&ea[k * ESIZE + o..]) {
                                    n += 1;
                                }
                            }
                        }
                        if let Some(pa) = &after_player {
                            for o in (0..0x2b00).step_by(4) {
                                if !player_pointer_words.contains(&o) && !player_hud_words.contains(&o) && game.players[0].u32_(o) != word(&pa[o..]) {
                                    n += 1;
                                }
                            }
                            n += fx_before.compare(game, items_base).len();
                        }
                        if let Some(wi) = &wi_after {
                            n += ref_before.compare(game, wi, items_base).len();
                        }
                        n
                    };
                    let base = score(&game);
                    if 0 < base {
                        let n0 = steps_between(seed, rand_after).unwrap_or(0);
                        let mut best = (base, seed);
                        for n in [n0.checked_sub(1), Some(n0 + 1), n0.checked_sub(2), Some(n0 + 2)].into_iter().flatten() {
                            let s = back(rand_after, n);
                            let r = attempt(&mut game, s)?;
                            if r.is_err() || game.rng.state != rand_after {
                                continue;
                            }
                            write_back(&mut game);
                            let sc = score(&game);
                            if sc < best.0 {
                                best = (sc, s);
                            }
                        }
                        alt_seed = best.1 != seed;
                        seed = best.1;
                        res = attempt(&mut game, seed)?;
                        write_back(&mut game);
                    }
                }
                let mut diffs = Vec::new();
                for k in 0..25 {
                    for o in (0..ESIZE).step_by(4) {
                        if pointer_words.contains(&o) {
                            continue;
                        }
                        let n = game.enemies[k].u32_(o);
                        let r = word(&ea[k * ESIZE + o..]);
                        if n != r {
                            *by_offset.entry(o).or_default() += 1;
                            diffs.push(serde_json::json!({"enemy":k,"offset":format!("{o:#x}"),"native":format!("{n:08x}"),"reference":format!("{r:08x}"),"before":format!("{:08x}", word(&eb[k * ESIZE + o..]))}));
                        }
                    }
                }
                if diffs.is_empty() && res.is_ok() {
                    equal_updates += 1;
                }
                let item_diffs = if items_mode { ref_before.compare(&game, &hexb(&a["enemies"]["WorldInfo"])?, items_base) } else { Vec::new() };
                let (mut player_diffs, effect_diffs) = (Vec::new(), if frame_mode { fx_before.compare(&game, items_base) } else { Vec::new() });
                let mut critter_diffs = Vec::new();
                let mut camera_diffs: Vec<serde_json::Value> = Vec::new();
                if frame_mode {
                    let after = hexb(&a["player"]["player0"])?;
                    for o in (0..0x2b00).step_by(4) {
                        if player_pointer_words.contains(&o) || player_hud_words.contains(&o) {
                            continue;
                        }
                        let (n, r) = (game.players[0].u32_(o), word(&after[o..]));
                        if n != r {
                            player_diffs.push(serde_json::json!({"offset":format!("{o:#x}"),"native":format!("{n:08x}"),"reference":format!("{r:08x}")}));
                        }
                    }
                    if player_diffs.is_empty() && res.is_ok() {
                        player_updates_equal += 1;
                    }
                    if effect_diffs.is_empty() && res.is_ok() {
                        effect_updates_equal += 1;
                    }
                    // main's camera dispatch after game_main, compared with
                    // the after sample (traces with the boss camera state).
                    if let Some(abc) = a["player"].get("bosscam") {
                        let cr = game.camera_step_pub();
                        let mut d = Vec::new();
                        if let Err(e) = &cr {
                            d.push(serde_json::json!({"camera":"unsupported","detail":e.0}));
                        }
                        let rb = hexb(abc)?;
                        if let Some(bc) = &game.boss_camera {
                            for o in (0..0x1b0).step_by(4) {
                                if bc.rec.u32_(o) != word(&rb[o..]) {
                                    d.push(serde_json::json!({"camera":"bosscam","offset":format!("{o:#x}"),"native":format!("{:08x}", bc.rec.u32_(o)),"reference":format!("{:08x}", word(&rb[o..]))}));
                                }
                            }
                            let m = word(&hexb(&a["player"]["BossCameraMode"])?);
                            if bc.mode as u32 != m {
                                d.push(serde_json::json!({"camera":"BossCameraMode","native":bc.mode,"reference":m}));
                            }
                        }
                        let rs = hexb(&a["player"]["stdcam"])?;
                        let wa = hexb(&a["player"]["win"])?;
                        if let Some(c) = &game.std_camera {
                            for o in (0..0x1b0).step_by(4) {
                                if c.std.u32_(o) != word(&rs[o..]) {
                                    d.push(serde_json::json!({"camera":"stdcam","offset":format!("{o:#x}"),"native":format!("{:08x}", c.std.u32_(o)),"reference":format!("{:08x}", word(&rs[o..]))}));
                                }
                            }
                            let rows = c.pb.cam_rows;
                            for (k, row) in rows.iter().enumerate() {
                                for (j, v) in row.iter().enumerate() {
                                    let r = word(&wa[0x70 + 0x10 * k + 4 * j..]);
                                    if v.to_bits() != r {
                                        d.push(serde_json::json!({"camera":"window","row":k,"col":j,"native":format!("{:08x}", v.to_bits()),"reference":format!("{r:08x}")}));
                                    }
                                }
                            }
                            for (j, v) in c.pb.eye.iter().enumerate() {
                                let r = word(&wa[0xa0 + 4 * j..]);
                                if v.to_bits() != r {
                                    d.push(serde_json::json!({"camera":"window eye","col":j,"native":format!("{:08x}", v.to_bits()),"reference":format!("{r:08x}")}));
                                }
                            }
                        }
                        if d.is_empty() && res.is_ok() {
                            camera_updates_equal += 1;
                        }
                        camera_diffs = d;
                    }
                    if have_crit {
                        critter_diffs = crit_before.compare(&game, items_base)?;
                        if critter_diffs.is_empty() && res.is_ok() {
                            critter_updates_equal += 1;
                        }
                    }
                    if let Some(wa) = a["enemies"].get("world_anims") {
                        let raw = hexb(wa)?;
                        let same = game.world_anims.entries.iter().enumerate().take(raw.len() / 16).all(|(k, e)| {
                            e.flags == u16::from_le_bytes([raw[16 * k + 6], raw[16 * k + 7]]) && e.frame.to_bits() == word(&raw[16 * k + 8..])
                        });
                        if same {
                            world_anim_updates_equal += 1;
                        }
                    }
                }
                if items_mode && item_diffs.is_empty() && res.is_ok() {
                    item_updates_equal += 1;
                }
                for d in &item_diffs {
                    *by_item_offset.entry(d["offset"].as_str().unwrap_or("?").to_string()).or_default() += 1;
                    *by_item_slot.entry(d["item"].as_u64().unwrap_or(u64::MAX)).or_default() += 1;
                }
                updates.push(serde_json::json!({"step":a["step"],"seed_solved":solved,"alt_seed":alt_seed,"error":res.err().map(|e| e.0),
                    "words_differing":diffs.len(),"diffs":diffs.iter().take(60).collect::<Vec<_>>(),
                    "item_words_differing":item_diffs.len(),"item_diffs":item_diffs.iter().take(60).collect::<Vec<_>>(),
                    "player_words_differing":player_diffs.len(),"player_diffs":player_diffs.iter().take(40).collect::<Vec<_>>(),
                    "effect_words_differing":effect_diffs.len(),"effect_diffs":effect_diffs.iter().take(40).collect::<Vec<_>>(),
                    "critter_words_differing":critter_diffs.len(),"critter_diffs":critter_diffs.iter().take(60).collect::<Vec<_>>(),
                    "camera_words_differing":camera_diffs.len(),"camera_diffs":camera_diffs.iter().take(40).collect::<Vec<_>>()}));
            }
            let report = serde_json::json!({"scope":"per-update replay of a reference enemies trace through the native do_enemies (all 25 records, every u32 except presentation pointers 0x70 0x80 0x84 0x88 0x8c 0xc0 0xc4 0x1f0; generator/floor links mapped)",
                "items_mode":items_mode,"item_scope":"items_replay: ProcessItems then do_enemies; Items array words except +0x70..+0xc7 and the +0xe6 link, export_live_items convention",
                "item_updates_equal":item_updates_equal,"differing_item_offsets":by_item_offset,"differing_item_slots":by_item_slot,
                "differing_item_names":by_item_slot.keys().filter_map(|&k| game.items.records.get(k as usize).map(|r| (k.to_string(), String::from_utf8_lossy(game.items.definitions[r.definition()].name()).into_owned()))).collect::<std::collections::BTreeMap<_, _>>(),
                "frame_mode":frame_mode,"player_updates_equal":player_updates_equal,"effect_updates_equal":effect_updates_equal,
                "world_anim_updates_equal":world_anim_updates_equal,
                "critter_updates_equal":have_crit.then_some(critter_updates_equal),
                "camera_updates_equal":camera_updates_equal,
                "item_floor_parents":game.items.floor_parent.iter().enumerate().filter_map(|(k, p)| p.map(|(o, l)| serde_json::json!({"item":k,"object":o,"local":l}))).collect::<Vec<_>>(),
                "items_without_native_clock":(0..game.items.records.len()).filter(|&k| game.items.records[k].active() && game.item_anims.get(k).is_none_or(|a| a.is_none()))
                    .map(|k| String::from_utf8_lossy(game.items.definitions[game.items.records[k].definition()].name()).into_owned())
                    .fold(std::collections::BTreeMap::<String, usize>::new(), |mut m, n| { *m.entry(n).or_default() += 1; m }),
                "trace":tp.to_string(),"updates":updates.len(),"equal_updates":equal_updates,
                "differing_offsets":by_offset.iter().map(|(o, n)| (format!("{o:#x}"), *n)).collect::<std::collections::BTreeMap<_, _>>(),
                "results":updates,
                "unsupported":game.log.unsupported.iter().map(|(k, v)| serde_json::json!({"path":k,"count":v})).collect::<Vec<_>>()});
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[8])?;
            serde_json::to_writer_pretty(&mut file, &report)?;
            file.write_all(b"\n")?;
            return Ok(());
        }
        if let Some(tp) = opts.get("replay") {
            // Per-update replay of a reference_trace --player trace: each game
            // update starts from the reference's player-0 record, controls,
            // window/StdCam blocks and globals; the native do_players result is
            // compared with the reference's next record (u32 words).
            let trace: serde_json::Value = serde_json::from_slice(&bounded_read(Path::new(tp), 64 * 1024 * 1024)?)?;
            let samples = trace["samples"].as_array().ok_or("trace samples")?;
            let clips = anim_of("clips")?;
            game.player_clips[0] = gdl_core::game::setup::clips_of(&clips, 0);
            let hexb = |v: &serde_json::Value| -> Result<Vec<u8>, Box<dyn Error>> {
                let s = v.as_str().ok_or("hex field")?;
                (0..s.len()).step_by(2).map(|i| Ok(u8::from_str_radix(&s[i..i + 2], 16)?)).collect()
            };
            let word = |b: &[u8]| u32::from_le_bytes(b[..4].try_into().unwrap());
            let fl = |b: &[u8]| f32::from_le_bytes(b[..4].try_into().unwrap());
            // Guest pointers (trees, nodes, item/object/player links) differ by design.
            let pointer_words: std::collections::HashSet<usize> = [0x80, 0x90, 0x94, 0x98, 0x9c, 0x6a0, 0x6a4, 0x6a8, 0x6ac, 0x6b0, 0x6b4, 0x6b8, 0x6bc, 0x704, 0x708, 0x7c0, 0x880, 0x884, 0x888, 0x89c, 0x8a0, 0x690, 0x694]
                .into_iter().collect();
            // HUD words written by presentation code: get_screen_pos (+0x878,
            // +0x87c) and draw_power_meter (+0x2ae0); compared separately.
            let hud_words: std::collections::HashSet<usize> = [0x878, 0x87c, 0x2ae0].into_iter().collect();
            let mut updates = Vec::new();
            let mut equal_updates = 0;
            // Traces with items_delta: every item from the before state with
            // the after state's +0xd8 0x4000 visible bit (do_items runs first).
            let mut ref_before = ReferenceItems::default();
            let mut have_items = match samples.first() {
                Some(s0) => ref_before.apply(s0)?,
                None => false,
            };
            for w in samples.windows(2) {
                let (b, a) = (&w[0], &w[1]);
                let mut ref_after = ReferenceItems(ref_before.0.clone());
                have_items &= ref_after.apply(a)?;
                let ref_b = std::mem::replace(&mut ref_before, ref_after);
                if b["observation"]["scalars"]["AFrame"]["raw_u32"] == a["observation"]["scalars"]["AFrame"]["raw_u32"] {
                    continue;
                }
                let (bp, ap) = (&b["player"], &a["player"]);
                let before = hexb(&bp["player0"])?;
                let after = hexb(&ap["player0"])?;
                game.players[0] = gdl_core::game::record::Record(before.clone());
                // Guest pointers → native links: the floor object by
                // PlayerCheckFloor's query when the reference has one; item,
                // wall and partner links cleared (none are near in G1's start).
                {
                    let r = &game.players[0];
                    let (rad, h, pos) = (r.f32_(0x828), r.f32_(0x82c), r.v3(0x50));
                    let floor = if r.u32_(0x89c) != 0 {
                        game.collision.floor_collide(rad, 0.0, -(h + 3.0), pos, false, 1).map_err(|e| e.0)?.0.solid.map_or(0, |hit| hit.object as u32 + 1)
                    } else {
                        0
                    };
                    // The item link (+0x880, an Items record) becomes the slot
                    // + 1 when the trace records the Items base (--enemies).
                    let items_base = samples.first().and_then(|s| s["enemies"].get("Items")).map(&hexb).transpose()?.map(|b| word(&b));
                    let r = &mut game.players[0];
                    let item = r.u32_(0x880);
                    r.set_u32(0x89c, floor);
                    for o in [0x880, 0x884, 0x888, 0x8a0, 0x690, 0x694, 0x6b4, 0x6b8] {
                        r.set_u32(o, 0);
                    }
                    if let Some(base) = items_base
                        && item >= base
                        && (item - base).is_multiple_of(0x110)
                    {
                        r.set_u32(0x880, (item - base) / 0x110 + 1);
                    }
                }
                let ctl = hexb(&ap["control"])?;
                for k in 0..4 {
                    game.controls.control[k] = gdl_core::game::record::Record(ctl[k * 0x3c..(k + 1) * 0x3c].to_vec());
                }
                let g = |n: &str| -> Result<Vec<u8>, Box<dyn Error>> { hexb(&ap[n]) };
                let gl = &mut game.globals;
                gl.time = fl(&g("Time")?);
                gl.spf = fl(&g("SPF")?);
                gl.atime = fl(&g("ATime")?);
                gl.anim_time_scale = fl(&g("AnimTimeScale")?);
                gl.vb_elapsed = word(&g("vb_elapsed")?) as i32;
                gl.aux_levels = word(&g("AuxLevels")?);
                gl.game_mode = word(&g("game_mode")?);
                gl.cam_wait = word(&g("cam_wait")?) as i32;
                gl.start_of_stage = word(&g("start_of_stage")?) as i32;
                gl.num_visible_enemies = word(&g("NumVisibleEnemies")?) as i32;
                gl.enemy_shrink = fl(&g("EnemyShrink")?);
                gl.quest_item_state = word(&g("QuestItemState")?);
                gl.boss_type = word(&g("BossType")?) as i32;
                gl.boss_active = word(&g("BossActive")?) as i32;
                gl.world_bottom = fl(&g("WldBtm")?);
                gl.paused = word(&g("paused")?) as i32;
                gl.help = word(&g("help")?) as i32;
                gl.trig_cam_active = word(&g("trig_cam_active")?) as i32;
                gl.num_active_players = word(&g("NumActivePlayers")?) as i32;
                gl.skip_wave = word(&g("skip_wave")?) as i32;
                gl.quit_level = word(&g("quit_level")?) as i32;
                gl.auto_exit_flag = word(&g("auto_exit_flag")?) as i32;
                let opt = g("options")?;
                gl.options = word(&opt[0..]) as i32;
                gl.options_4 = word(&opt[4..]);
                gl.options_18 = word(&opt[0x18..]);
                game.stop_time = word(&g("StopTime")?) as i32;
                let win = hexb(&bp["win"])?;
                game.globals.camera_90 = fl(&win[0x90..]);
                game.globals.camera_98 = fl(&win[0x98..]);
                if let Some(c) = game.std_camera.as_mut() {
                    c.std = gdl_core::game::record::Record(hexb(&bp["stdcam"])?);
                }
                // Traces with --enemies: do_enemies runs before do_players,
                // so the enemy records after the update (generator/item/floor
                // links mapped) and
                // the player's +0x940/+0x944 (the enemies' targeting count and
                // crowd) come from the reference; the grid is rebuilt.
                if let (Some(eb), Some(e0)) = (a["enemies"].get("enemies"), samples.first().and_then(|s| s["enemies"].as_object())) {
                    let eb = hexb(eb)?;
                    let before_enemies = hexb(&b["enemies"]["enemies"])?;
                    let base_of = |n: &str| -> Result<u32, Box<dyn Error>> { Ok(word(&hexb(e0.get(n).ok_or("trace base")?)?)) };
                    let (items_base, world_base) = (base_of("Items")?, base_of("WorldBase")?);
                    game.enemy_node = vec![None; game.enemies.len()];
                    for k in 0..game.enemies.len().min(eb.len() / 0x3b0) {
                        // An enemy killed this update (state 1 → 8) died in
                        // ProcessEffects after do_players: the before record.
                        let state = |m: &[u8]| word(&m[k * 0x3b0 + 0xc8..]);
                        let src = if state(&eb) == 8 && state(&before_enemies) == 1 { &before_enemies } else { &eb };
                        let mut rec = gdl_core::game::record::Record(src[k * 0x3b0..(k + 1) * 0x3b0].to_vec());
                        for (o, base, size) in [(0x2a4, items_base, 0x110u32), (0x2a0, items_base, 0x110), (0x2ac, world_base, 0x3c)] {
                            let v = rec.u32_(o);
                            let n = if v == 0 || v < base || !(v - base).is_multiple_of(size) { if v == 0 { 0 } else { u32::MAX } } else { (v - base) / size + 1 };
                            rec.set_u32(o, n);
                        }
                        game.enemies[k] = rec;
                    }
                    if have_items {
                        let items_base = word(&hexb(&e0["Items"])?);
                        ref_b.seed(&mut game, &hexb(&b["enemies"]["WorldInfo"])?, items_base, Some(&ref_before))?;
                    }
                    // Otherwise the generator items as do_items left them
                    // (WorldInfo +0x68 maps the definition pointer;
                    // presentation words kept). The after state also holds
                    // ProcessEffects' damage, so full traces use the before.
                    if have_items {
                    } else if let (Some(gens), Some(wi)) = (a["enemies"]["generator_items"].as_object(), a["enemies"].get("WorldInfo")) {
                        let defs = word(&hexb(wi)?[0x68..]);
                        for (slot, hex) in gens {
                            let slot: usize = slot.parse()?;
                            let raw = hexb(hex)?;
                            let Some(rec) = game.items.records.get_mut(slot) else { continue };
                            let keep = rec.0;
                            let mut new = [0u8; 0x110];
                            new.copy_from_slice(&raw[..0x110]);
                            let ptr = word(&raw[0..]);
                            let def = if ptr == 0 { 0 } else { ptr.wrapping_sub(defs) / 0x50 };
                            new[0..4].copy_from_slice(&def.to_le_bytes());
                            new[0x70..0xc8].copy_from_slice(&keep[0x70..0xc8]);
                            new[0xe6..0xe8].copy_from_slice(&keep[0xe6..0xe8]);
                            rec.0 = new;
                        }
                    }
                    let after = hexb(&ap["player0"])?;
                    game.players[0].set_u32(0x940, word(&after[0x940..]));
                    game.players[0].set_u32(0x944, word(&after[0x944..]));
                    game.setup_dyn_grid().map_err(|e| e.0)?;
                }
                // game_main's SetPlayerVars clears the panel-written word
                // (+0x93e) before do_players.
                game.players[0].set_u16(0x93e, 0);
                game.player_floor_parent[0] = None;
                // The node matrix at the update start (trace field "node":
                // node +0x00..+0x40); without it, the record's own matrix.
                game.player_node[0] = match bp.get("node") {
                    Some(v) => {
                        let n = hexb(v)?;
                        Some(std::array::from_fn(|i| std::array::from_fn(|j| fl(&n[0x10 * i + 4 * j..]))))
                    }
                    None => None,
                };
                let res = game.do_players();
                let mut diffs = Vec::new();
                for o in (0..0x2b00).step_by(4) {
                    if pointer_words.contains(&o) || hud_words.contains(&o) {
                        continue;
                    }
                    let (n, r) = (game.players[0].u32_(o), word(&after[o..]));
                    if n != r {
                        diffs.push(serde_json::json!({"offset":format!("{o:#x}"),"native":format!("{n:08x}"),"reference":format!("{r:08x}"),"before":format!("{:08x}", word(&before[o..]))}));
                    }
                }
                if diffs.is_empty() && res.is_ok() {
                    equal_updates += 1;
                }
                updates.push(serde_json::json!({"step":a["step"],"error":res.err().map(|e| e.0),"words_differing":diffs.len(),
                    "action":[word(&before[0x1e0..]), game.players[0].u32_(0x1e0), word(&after[0x1e0..])],
                    "diffs":diffs.iter().take(40).collect::<Vec<_>>()}));
            }
            let report = serde_json::json!({"scope":"per-update replay of a reference player trace through the native do_players; pointer words excluded",
                "trace":tp.to_string(),"updates":updates.len(),"equal_updates":equal_updates,"results":updates,
                "unsupported":game.log.unsupported.iter().map(|(k, v)| serde_json::json!({"path":k,"count":v})).collect::<Vec<_>>()});
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[8])?;
            serde_json::to_writer_pretty(&mut file, &report)?;
            file.write_all(b"\n")?;
            return Ok(());
        }
        if opts.contains_key("flyby") {
            // flyby=1: init_flyby on this level (the attract's 30 Hz: vb 2,
            // SPF 1/30) and the flyby's updates; the camera path per update.
            game.globals.vb_elapsed = 2;
            game.globals.spf = 1.0 / 30.0;
            let mut f = game.init_flyby((0, 0), 0.0).map_err(|e| e.0)?;
            let mut path = Vec::new();
            let mut end = None;
            for k in 0..frames {
                game.globals.vb_elapsed = 2;
                if game.flyby_step(&mut f) {
                    end = Some(k);
                    break;
                }
                if k % 5 == 0 {
                    path.push(serde_json::json!({"update":k,"eye":f.cam.eye,"attention":f.cam.attention,"pyr":f.cam.pyr,
                        "t1":f.transmitter1,"t2":f.transmitter2,"current":f.current,"gravity":f.gravity,"seq_done":f.seq_done,"timer1":f.timer1,
                        "enemies":game.enemies.iter().filter(|e| e.i32_(0xc8) != 0).count()}));
                }
            }
            let report = serde_json::json!({"scope":"actual disc level, native flyby (init_flyby/do_flyby, old Cam[0] transmitter path); not compared with a reference trace",
                "end_update":end,"path":path,"attract_transmitters":game.std_camera.as_ref().map(|c| c.transmitters.iter().enumerate().filter(|(_, t)| t.flag == 1).map(|(k, t)| serde_json::json!([k, t.pos, t.word_02])).collect::<Vec<_>>()),
                "unsupported":game.log.unsupported.iter().map(|(k, v)| serde_json::json!({"path":k,"count":v})).collect::<Vec<_>>()});
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[8])?;
            serde_json::to_writer_pretty(&mut file, &report)?;
            file.write_all(b"\n")?;
            return Ok(());
        }
        let mut samples = Vec::new();
        // DEBUG spawn=TYPE:LEVEL:AI:FRAME[:DX:DZ[:DY]][;...]: generate_enemy beside
        // player 0 at that update; `spawned` traces the first one every 5 updates.
        let spawns: Vec<Vec<f32>> = match opts.get("spawn") {
            Some(v) => v.split(';').map(|one| one.split(':').map(|x| x.parse::<f32>()).collect::<Result<Vec<_>, _>>()).collect::<Result<_, _>>()?,
            None => Vec::new(),
        };
        let mut spawned_slot: Option<usize> = None;
        let mut spawned = Vec::new();
        // Actuator requests (do_vibe / vibe_off) by update.
        let mut rumble = Vec::new();
        // Team-move state per update while a link exists.
        let mut team = Vec::new();
        for f in 0..frames {
            if (0..2).any(|k| game.players[k].u32_(0x690) != 0 || game.players[k].u32_(0x694) != 0 || game.players[k].u16_(0x93c) & 0xf0 != 0) {
                team.push(serde_json::json!([f, (0..2).map(|k| { let r = &game.players[k]; serde_json::json!({"action":r.u32_(0x1e0),"frame":r.f32_(0xac),"link":[r.u32_(0x690), r.u32_(0x694)],"status":format!("{:#x}", r.u16_(0x93c)),"pos":r.v3(0x50),"turbo":r.f32_(0x800),"t1d2":r.i16_(0x1d2)}) }).collect::<Vec<_>>()]));
            }
            rumble.extend(game.rumble.drain(..).map(|(port, i)| serde_json::json!([f, port, i])));
            for (n, sp) in spawns.iter().enumerate().filter(|(_, sp)| 4 <= sp.len() && sp[3] as usize == f) {
                let p = game.players[0].v3(0x50);
                let (dx, dz) = (sp.get(4).copied().unwrap_or(0.0), sp.get(5).copied().unwrap_or(8.0));
                let k = game.generate_enemy(1.0, [p[0] + dx, p[1], p[2] + dz], sp[0] as i32, sp[1] as i32, [0.0, 0.0, -1.0], sp[2] as i32, 0, 1).map_err(|e| e.0)?;
                if let Ok(k) = usize::try_from(k) {
                    // Without a generator generate_enemy keeps the given point;
                    // drop it on the floor as check_enemy_pos does for generators.
                    let at = game.enemies[k].v3(0x40);
                    let dy = sp.get(6).copied().unwrap_or(2.0);
                    let r = game.check_enemy_pos_pub([at[0], p[1] + dy, at[2]], None, k).map_err(|e| e.0)?;
                    eprintln!("spawn {n} at {f}: generate_enemy -> {k}, check_enemy_pos {r}, at {:?}", game.enemies[k].v3(0x40));
                    if n == 0 {
                        spawned_slot = Some(k);
                    }
                }
            }
            if let Some(k) = spawned_slot.filter(|_| f % 5 == 0) {
                let e = &game.enemies[k];
                spawned.push(serde_json::json!({"frame":f,"state":e.i32_(0xc8),"logic":e.i16_(0x324),"ran":e.i16_(0x328),"stage":e.i16_(0x32a),"action":e.u32_(0xe0),
                    "pos":e.v3(0x40),"heading":e.f32_(0x260),"target":e.i16_(0x288),"dist":e.f32_(0x290),"health":e.f32_(0x214),"anim_enabled":e.u16_(0xa2)}));
            }
            if opts.contains_key("pads") {
                let mut pads = [gdl_core::game::pad::PadStatus::default(); 8];
                pads[0].connected = true;
                let m = pad_script.get(f).copied().unwrap_or(0);
                for i in 0..0x16 {
                    if m >> i & 1 != 0 {
                        pads[0].press(i);
                    }
                }
                if opts.get("quest").and_then(|v| v.parse::<usize>().ok()) == Some(f) {
                    // DEBUG: the boss reached the legend-item stage (QuestItemState 2).
                    game.globals.quest_item_state = 2;
                }
                if opts.get("fall").and_then(|v| v.parse::<usize>().ok()) == Some(f) {
                    // DEBUG: player 0 drops below the world bottom (PlayerMotion's
                    // get_player_pos(p, 0) respawn).
                    let wb = game.globals.world_bottom;
                    let r = &mut game.players[0];
                    r.set_f32(0x54, wb - 1.0);
                    let m = r.v3(0x50);
                    eprintln!("fall at {f}: {m:?}");
                    if let Some(n) = game.player_node[0].as_mut() {
                        n[3][1] = wb - 1.0;
                    }
                }
                if let Some(at) = opts.get("boss-kill").and_then(|v| v.parse::<usize>().ok())
                    && at <= f
                    && (f - at).is_multiple_of(60)
                    && game.globals.boss_is_dead == 0
                {
                    // DEBUG: lethal CritterDamage on the boss (Game::debug_damage_boss),
                    // once a second until it dies.
                    let r = game.debug_damage_boss().map_err(|e| e.0)?;
                    eprintln!("boss-kill at {f}: CritterDamage {r}");
                }
                game.step_with(None, &mut |g| {
                    g.update_controls(&pads)?;
                    g.do_players().map(|_| ())
                });
                if f % 15 == 0 || f + 1 == frames || (opts.contains_key("weapon") && f % 3 == 0) {
                    let r = &game.players[0];
                    samples.push(serde_json::json!({"frame":f,"pos":r.v3(0x50),"facing":r.f32_(0x86c),"action":r.u32_(0x1e0),"request":r.u32_(0x1e4),
                        "code":r.u32_(0x1dc),"seq":r.u16_(0xa2),"frame_anim":r.f32_(0xac),"health":r.f32_(0x1a10),"state":r.i32_(0xfc),"floor_y":r.f32_(0x88c),"water_y":r.f32_(0x8a4),
                        "turbo":r.f32_(0x800),"weapon_flags":r.u32_(0x130),"events":r.u32_(0x8d8),"levels":game.controls.control[0].u32_(4),"floor":r.u32_(0x89c),
                        "legend":r.i32_(0x80c),"quest":game.globals.quest_item_state,"boss_dead":game.globals.boss_is_dead,
                        "good_wizard":game.globals.good_wizard,"wiz":[game.good_wiz.alpha,game.good_wiz.speech_idx,game.good_wiz.speech_frame],"wiz_pos":game.good_wiz.matrix.map(|m| m[3]),
                        "auto_exit":[game.auto_exit_timer,game.globals.auto_exit_flag],"que":[game.audio_que.entries[0].len(),game.audio_que.entries[1].len()],
                        "exit":[game.players_done,game.exit_ready],"level_done":game.level_done,"shards":r.u16_(0x1a24),"boss_cam":game.boss_camera.as_ref().map(|c| [c.rec.v3(gdl_core::game::camera::o::EYE), c.rec.v3(gdl_core::game::camera::o::TARGET)]),
                        "boss_health":usize::try_from(game.boss_crit_inst - 1).ok().and_then(|b| game.critters.get(b)).and_then(|c| c.as_ref()).map(|c| c.rec.f32_(0x4f4)),
                        "effects":(0..game.effects.num).filter(|&k| 0.0 < game.effects.slots[k].f32_(0x68)).map(|k| serde_json::json!({"slot":k,"kind":game.effects.slots[k].u32_(0x60),"link":game.effects.slots[k].i16_(0xe8),"visual":game.effects.nodes[k].map(|n| format!("{:?}", n.visual)),"follow":game.effects.nodes[k].and_then(|n| n.follow)})).collect::<Vec<_>>(),
                        "others":(1..4).filter(|&k| game.globals.active_players >> k & 1 != 0).map(|k| serde_json::json!({"player":k,"pos":game.players[k].v3(0x50),"state":game.players[k].i32_(0xfc),"health":game.players[k].f32_(0x1a10)})).collect::<Vec<_>>()}));
                }
                continue;
            }
            let mut levels = script.get(f).copied().unwrap_or(0);
            if levels & 0x8000_0000 != 0 {
                // Tool-only token: magic button edge (interim use_potion).
                levels &= 0x7fff_ffff;
                if let Some(d) = driver.as_mut() {
                    d.magic_pressed = true;
                }
            }
            match driver.as_mut() {
                Some(d) => game.step_with(None, &mut |g| g.drive_player(0, d, levels)),
                None => game.step(None),
            }
            if f % 15 == 0 || f + 1 == frames {
                let enemies: Vec<_> = game.enemies.iter().enumerate().filter(|(_, e)| e.i32_(0xc8) != 0)
                    .map(|(k, e)| serde_json::json!({"slot":k,"type":e.i32_(0),"state":e.i32_(0xc8),"pos":e.v3(0x40),
                        "action":e.u32_(0xe0),"logic":e.i16_(0x324),"health":e.f32_(0x214),"target":e.i16_(0x288)})).collect();
                let effects: Vec<_> = (0..game.effects.num).filter(|&k| 0.0 < game.effects.slots[k].f32_(0x68))
                    .map(|k| { let r = &game.effects.slots[k]; serde_json::json!({"slot":k,"node":game.effects.nodes[k],"flags":r.u32_(0x64),
                        "kind":r.u32_(0x60),"end":r.f32_(0x68),"vel":r.v3(0x80),"grav":r.f32_(0xa0),"dflags":r.u32_(0xb4),"owner":r.i16_(0xbc),"link":r.i16_(0xe8)}) }).collect();
                let critters: Vec<_> = game.critters.iter().enumerate().filter_map(|(k, c)| c.as_ref().map(|c| serde_json::json!({
                    "slot":k,"state":c.rec.i32_(8),"health":c.rec.f32_(0x4f4),"move":c.rec.i16_(0x124),"next":c.rec.i16_(0x126),
                    "seq":c.clock.sequence,"frame":c.clock.frame,"pos":c.rec.v3(0x40),"target":c.rec.i16_(0x130),"ntargets":c.rec.i16_(0x136)}))).collect();
                samples.push(serde_json::json!({"frame":f,"enemies":enemies,"critters":critters,"player_health":game.players[0].f32_(0x1a10),
                    "level_done":game.level_done,"good_wizard":game.globals.good_wizard,"auto_exit":[game.auto_exit_timer,game.globals.auto_exit_flag],"player_state":game.players[0].i32_(0xfc),
                    "player_pos":game.players[0].v3(0x50),"cam_wait":game.globals.cam_wait,"start_of_stage":game.globals.start_of_stage,"camera_eye":game.std_camera.as_ref().map(|c| c.std.v3(gdl_core::game::camera::o::EYE)),"boss_camera_eye":game.boss_camera.as_ref().map(|c| c.rec.v3(gdl_core::game::camera::o::EYE)),"events":game.events.len(),"effects":effects,
                    "driver":driver.as_ref().map(|d| serde_json::json!({"anim":d.anim,"missiles":d.missiles_started,"event_error":d.event_error,"event_word":d.event_word}))}));
            }
        }
        let sounds: Vec<String> = game.events.iter().filter(|e| matches!(e, gdl_core::game::Event::Sound { .. })).take(200).map(|e| format!("{e:?}")).collect();
        let events: Vec<String> = game.events.iter().filter(|e| !matches!(e, gdl_core::game::Event::Sound { .. })).take(400).map(|e| format!("{e:?}")).collect();
        let generators: Vec<_> = game.items.records.iter().enumerate()
            .filter(|(_, r)| r.active() && game.items.definitions[r.definition()].kind() == 3)
            .map(|(k, r)| serde_json::json!({"slot":k,"name":String::from_utf8_lossy(game.items.definitions[r.definition()].name()),
                "type":r.i16_(0xf0),"count":r.i8_(0xf2),"max":r.i8_(0xf3),"level":r.i8_(0xf6),"logic":r.i8_(0xf7),
                "d8":r.u16_(0xd8),"pos":r.v3(0x60)})).collect();
        // Placed-enemy items (kind 4): slot, +0xf0 type, +0xf2 level, +0xf3 ai.
        let placed: Vec<_> = (0..game.items.records.len())
            .filter(|&k| game.items.definitions[game.items.records[k].definition()].kind() == 4)
            .map(|k| { let r = &game.items.records[k]; serde_json::json!([k, r.i16_(0xf0), r.i8_(0xf2), r.i8_(0xf3)]) })
            .collect();
        // Exit items (kind 9): slot, +0xf0 destination wave, +0xd8 flags.
        let exits: Vec<_> = (0..game.items.records.len())
            .filter(|&k| game.items.definitions[game.items.records[k].definition()].kind() == 9)
            .map(|k| { let r = &game.items.records[k]; serde_json::json!([k, r.i16_(0xf0), r.u16_(0xd8), r.position()]) })
            .collect();
        let report = serde_json::json!({"exits":exits,"frames":frames,"samples":samples,"events":events,"sounds":sounds,"generators":generators,"placed":placed,"spawned":spawned,"rumble":rumble,"team":team,
            "attract_transmitters":game.std_camera.as_ref().map(|c| serde_json::json!({"start_att":c.start_att,
                "list":c.transmitters.iter().enumerate().filter(|(_, t)| t.flag == 1).map(|(k, t)| serde_json::json!([k, t.pos, t.pitch, t.yaw, t.id])).collect::<Vec<_>>()})),
            "level_multipliers_cc":(0..5).map(|k| game.level_f32(0xcc + 4 * k)).collect::<Vec<_>>(),
            "critter_files":game.critter_data.files.iter().map(|f| serde_json::json!({"name":f.name,"trees":f.trees,"model":f.model.is_some()})).collect::<Vec<_>>(),
            "critter_etypes":game.critter_data.etypes.iter().map(|r| r.iter().map(|t| t.map(|t| [t.0, t.1])).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "lookouts":game.lookouts.iter().map(|l| serde_json::json!({"pos":l.position,"next":l.next,"param":l.param})).collect::<Vec<_>>(),
            "unsupported":game.log.unsupported.iter().map(|(k, v)| serde_json::json!({"path":k,"count":v})).collect::<Vec<_>>(),
            "scope":"INTERIM native game run: fresh items, no initial enemies, optional scripted interim player (0x80000000 = magic edge), optional potions= debug override; not a fidelity comparison",
            "potions_override":opts.get("potions"),"crystals_override":opts.get("crystals"),
            "crystal_counts":(0..9).map(|n| game.players[0].i16_(0xd0a + 0xb4 * game.players[0].u32_(0xc) as usize + 2 * n)).collect::<Vec<_>>(),"boss_health_override":opts.get("boss_health"),"teleport_override":opts.get("teleport")});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[8])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "place-items" {
        // place-items WORLDS.PS2 NEW_REPORT.json — PlaceItem 0x153068 heights.
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let world = gdl_formats::world::World::parse(&raw)?;
        let mut collision = gdl_core::collision::CollisionWorld::from_world(&world)?;
        let mut out = Vec::new();
        for item in &world.items {
            let def = &world.item_definitions[item.definition as usize];
            let flags_0a = u16::from_le_bytes([def.raw[10], def.raw[11]]);
            if flags_0a & 1 != 0 {
                out.push(serde_json::json!({"index":item.index,"skipped":"definition +0xa bit 0"}));
                continue;
            }
            let p = item.position;
            out.push(match collision.floor_pos(p[1], 1.0, p, 0) {
                Ok((y, hit)) => {
                    let placed = gdl_core::numeric::add_guarded_rz(y, f32::from_bits(0x3dcc_cccd))?;
                    serde_json::json!({"index":item.index,"y_bits":placed.to_bits(),
                        "hit_object":hit.map(|h| h.object),"error":null})
                }
                Err(e) => serde_json::json!({"index":item.index,"error":e.to_string()}),
            });
        }
        let report = serde_json::json!({"worlds_sha256":hash(&raw),"items":out,
            "scope":"PlaceItem FloorPos+0.1 with static WorldCollide; dynamic objects excluded"});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "joy-tables" {
        // joy-tables SLUS_200.47 NEW_REPORT.json
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let elf = gdl_formats::elf::Elf::parse(&raw)?;
        let tables = gdl_core::controls::tables_from_elf(&elf)?;
        let (angle, magnitude) = tables.tables();
        let report = serde_json::json!({"elf_sha256":hash(&raw),
            "angle_bits":angle.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),"magnitude_bits":magnitude.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            "scope":"native InitJoyAng from ELF magval/angidx"});
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 4 && args[0] == "player-displacement" {
        // player-displacement WORLDS.PS2 STEPS.json NEW_REPORT.json
        #[derive(serde::Deserialize, serde::Serialize, Clone, Copy)]
        #[serde(deny_unknown_fields)]
        struct Step {
            request: gdl_core::player_motion::MotionRequestInput,
            matrix: [[f32; 3]; 3],
            collision_offset_81c: [f32; 3],
            offset_flags_84: u32,
            position: [f32; 3],
            radius_828: f32,
            height_82c: f32,
            floor_y_88c: f32,
            floor_object_89c: Option<usize>,
            flags_8ac: u32,
            word_100: i32,
            y_54: f32,
            wall_object_8a0: Option<usize>,
        }
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let mut world = gdl_core::collision::CollisionWorld::from_world(&gdl_formats::world::World::parse(&raw)?)?;
        let raw_steps = bounded_read(Path::new(&args[2]), 4 * 1024 * 1024)?;
        let steps: Vec<Step> = serde_json::from_slice(&raw_steps)?;
        if steps.len() > 4096 {
            return Err("displacement request policy exceeded".into());
        }
        let mut outcomes = Vec::new();
        for step in steps {
            let result = (|| -> Result<serde_json::Value, Box<dyn Error>> {
                let request = gdl_core::player_motion::motion_request(step.request)?;
                let mut state = gdl_core::player_collision::PlayerFloorState {
                    floor_y_88c: step.floor_y_88c,
                    floor_object_89c: step.floor_object_89c,
                    flags_8ac: step.flags_8ac,
                    word_100: step.word_100,
                    y_54: step.y_54,
                    wall_object_8a0: step.wall_object_8a0,
                    floor_contact: None,
                    floor_hit: None,
                    water: None,
                };
                let out = gdl_core::player_collision::player_displacement(
                    &mut world,
                    gdl_core::player_collision::DisplacementInput {
                        matrix: step.matrix,
                        collision_offset_81c: step.collision_offset_81c,
                        offset_flags_84: step.offset_flags_84,
                        position: step.position,
                        request: request.request,
                        speed: request.speed,
                        radius_828: step.radius_828,
                        height_82c: step.height_82c,
                        flags_8b0: step.request.flags_8b0,
                        current_action: step.request.current_action_1e0,
                        spf: step.request.spf,
                        world_bottom: step.request.world_bottom,
                    },
                    &mut state,
                )?;
                Ok(serde_json::json!({"displacement_bits":out.displacement.map(f32::to_bits),
                    "position_bits":out.position.map(f32::to_bits),"wall_result":out.wall_result,
                    "floor_result":out.floor_result,"floor_y_bits":state.floor_y_88c.to_bits(),
                    "floor_object":state.floor_object_89c,"wall_object":state.wall_object_8a0}))
            })();
            outcomes.push(match result {
                Ok(v) => serde_json::json!({"input":step,"output":v,"error":null}),
                Err(e) => serde_json::json!({"input":step,"output":null,"error":e.to_string()}),
            });
        }
        let report = serde_json::json!({"worlds_sha256":hash(&raw),"steps_sha256":hash(&raw_steps),"outcomes":outcomes,
            "scope":"request + static walls/floor/floor-height/slope + position add; items, players, enemies, camera limit, moving floors and transporters excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[3])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if (args.len() == 3 || args.len() == 4) && args[0] == "collision-query" {
        // collision-query WORLDS.PS2 NEW_REPORT.json [QUERIES.json]
        #[derive(serde::Deserialize, serde::Serialize, Clone, Copy)]
        #[serde(deny_unknown_fields)]
        struct Request {
            radius: f32,
            first: [f32; 3],
            last: [f32; 3],
            check_flags: u32,
            min_normal_y: f32,
            max_normal_y: f32,
            object_mask: u32,
            exclusion_mask: i32,
        }
        let raw = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let world = gdl_formats::world::World::parse(&raw)?;
        let mut native = gdl_core::collision::CollisionWorld::from_world(&world)?;
        let objects: Vec<_> = native
            .objects
            .iter()
            .map(|o| serde_json::json!({"position_bits":o.position.map(f32::to_bits),"parent":o.parent}))
            .collect();
        let mut outcomes = Vec::new();
        let mut requests_hash = serde_json::Value::Null;
        if args.len() == 4 {
            let raw_requests = bounded_read(Path::new(&args[3]), 4 * 1024 * 1024)?;
            requests_hash = serde_json::json!(hash(&raw_requests));
            let requests: Vec<Request> = serde_json::from_slice(&raw_requests)?;
            if requests.len() > 4096 {
                return Err("collision request policy exceeded".into());
            }
            for r in requests {
                let query = gdl_core::collision::Query {
                    radius: r.radius,
                    first: r.first,
                    last: r.last,
                    check_flags: r.check_flags,
                    min_normal_y: r.min_normal_y,
                    max_normal_y: r.max_normal_y,
                    object_mask: r.object_mask,
                    exclusion_mask: r.exclusion_mask,
                };
                let hit_json = |h: &Option<gdl_core::collision::Hit>| {
                    h.map(|h| serde_json::json!({"object":h.object,"triangle":h.triangle,
                        "distance_bits":h.distance.to_bits(),"position_bits":h.position.map(f32::to_bits)}))
                };
                outcomes.push(match native.world_collide(query) {
                    Ok((c, state)) => {
                        let basis = match c.solid {
                            Some(h) => {
                                let t = native.triangles[h.triangle];
                                gdl_core::collision::create_mat3_norm(t.scale, t.normal)
                                    .map(|m| serde_json::json!(m.map(|row| row.map(f32::to_bits))))
                                    .unwrap_or(serde_json::Value::Null)
                            }
                            None => serde_json::Value::Null,
                        };
                        serde_json::json!({"request":r,"solid":hit_json(&c.solid),"water":hit_json(&c.water),
                            "basis_bits":basis,"direction_bits":state.direction.map(f32::to_bits),
                            "length_bits":state.length.to_bits(),"min_y_bits":state.min_y.to_bits(),
                            "max_y_bits":state.max_y.to_bits(),"check_num":state.check_num,"error":null})
                    }
                    Err(e) => serde_json::json!({"request":r,"error":e.to_string()}),
                });
            }
        }
        let report = serde_json::json!({"worlds_sha256":hash(&raw),"requests_sha256":requests_hash,
            "objects":objects,"outcomes":outcomes,
            "scope":"static WorldCollide subset; dynamic objects, matrix/offset objects and TriLineCol excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "vu0-fqdist" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<[f32; 2]> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("fqdist request policy exceeded".into());
        }
        let outcomes: Vec<_> = requests
            .into_iter()
            .map(|[x, z]| match gdl_core::vu0::fqdist(x, z) {
                Ok((q, zz)) => serde_json::json!({"x":x,"z":z,"x_lane":q,"y_lane":zz,"error":null}),
                Err(e) => serde_json::json!({"x":x,"z":z,"x_lane":null,"y_lane":null,"error":e.to_string()}),
            })
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"fqdist VU0 microprogram 0x44 subset: finite operands, toward-zero VU multiply/add/sqrt"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "motion-request" {
        #[derive(serde::Deserialize, serde::Serialize, Clone, Copy)]
        #[serde(deny_unknown_fields)]
        struct Step {
            request: gdl_core::player_motion::MotionRequestInput,
            forward_40: f32,
            forward_48: f32,
            turn_968: f32,
            position_50: [f32; 3],
        }
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let steps: Vec<Step> = serde_json::from_slice(&raw)?;
        if steps.len() > 4096 {
            return Err("motion request policy exceeded".into());
        }
        let outcomes: Vec<_> = steps
            .into_iter()
            .map(|step| {
                let result = gdl_core::player_motion::motion_request(step.request).and_then(|output| {
                    let facing = gdl_core::player_motion::facing_turn(gdl_core::player_motion::FacingInput {
                        forward_40: step.forward_40,
                        forward_48: step.forward_48,
                        target: output.direction,
                        turn_968: step.turn_968,
                        spf: step.request.spf,
                    })?;
                    // Diagnostic: EE 0x14c14c's scalar add if the request were the
                    // final displacement; collision stages are not evaluated.
                    let mut position = [0.0f32; 3];
                    for ((out, base), delta) in position.iter_mut().zip(step.position_50).zip(output.request) {
                        *out = gdl_core::numeric::add_guarded_rz(base, delta)?;
                    }
                    Ok(serde_json::json!({"request_output":output,"facing":facing,"position_if_request_final":position}))
                });
                match result {
                    Ok(output) => serde_json::json!({"input":step,"output":output,"error":null}),
                    Err(e) => serde_json::json!({"input":step,"output":null,"error":e.to_string()}),
                }
            })
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"ordinary PlayerMotion pre-collision displacement request (+0x860..+0x868) and facing turn (+0x86c); collision, pushes, camera limits, final displacement, pad polling and requested-action mapping excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "motion-code" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<gdl_core::player_input::MotionCodeInput> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("motion code request policy exceeded".into());
        }
        let outcomes: Vec<_> = requests
            .into_iter()
            .map(
                |request| match gdl_core::player_input::motion_code(request) {
                    Ok(output) => {
                        serde_json::json!({"request":request,"output":output,"error":null})
                    }
                    Err(e) => {
                        serde_json::json!({"request":request,"output":null,"error":e.to_string()})
                    }
                },
            )
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"ordinary logical motion-code and run-delay timer subset; pad polling, angle-dependent branches, collision/forced modes, requested-action mapping and displacement excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "world-matrix" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<WorldMatrixRequest> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096
            || requests
                .iter()
                .map(|r| (r.parents.len() + 1) * 64)
                .sum::<usize>()
                > 1_000_000
        {
            return Err("world-matrix request/work policy exceeded".into());
        }
        let outcomes: Vec<_> = requests.into_iter().map(|request| {
            let mut parents = request.parents.clone();
            match gdl_core::world_transform::compose(request.leaf,&mut parents,request.offset) {
                Ok(output) => serde_json::json!({"request":request,"output":output,"parents_after":parents,"error":null}),
                Err(e) => serde_json::json!({"request":request,"output":null,"error":e.to_string()}),
            }
        }).collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"source-derived finite GetWorldMat/MulMat4 experiment, configured VU RZ arithmetic; actual guest world outputs, scale consumers and render composition unaccepted"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 4 && args[0] == "player-anim-select" {
        let source = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let animations = gdl_formats::animation::Animations::parse(&source)?;
        let tree = animations.trees.first().ok_or("animation tree missing")?;
        let raw = bounded_read(Path::new(&args[2]), 4 * 1024 * 1024)?;
        let requests: Vec<PlayerAnimationRequest> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("player animation request policy exceeded".into());
        }
        let timing = |id: u16| -> Result<_, Box<dyn Error>> {
            let clip = tree
                .sequences
                .get(usize::from(id))
                .ok_or("sequence ID out of range")?;
            Ok(gdl_core::animation_clock::ClipTiming {
                frames: clip.frames,
                timing_factor: clip.timing_factor,
                repeat: clip.repeat,
                root_transfer_flags: clip.root_transfer_flags,
            })
        };
        let outcomes: Vec<_> = requests.into_iter().map(|request| {
            let result = (|| -> Result<_, Box<dyn Error>> {
                if request.action_map.len() != 150 || request.context.frame.to_bits() != request.state.frame.to_bits() {
                    return Err("action map/frame differs from supported request layout".into());
                }
                let chosen = gdl_core::player_animation::choose(request.context)?;
                let mapped = request.action_map[chosen.action as usize].max(0);
                let sequence = u16::try_from(mapped)?;
                let mut state = request.state;
                state.option_bits |= 2;
                state.loop_word = chosen.loop_word_before_selection;
                let mut output = gdl_core::animation_clock::select(state, timing(state.sequence)?, timing(sequence)?,
                    gdl_core::animation_clock::SelectionInput { sequence, mode: chosen.mode,
                        start_frame: chosen.start_frame, blend_duration: chosen.blend_duration,
                        playback_scale: chosen.playback_scale, now: request.now,
                        anim_time_scale: request.anim_time_scale })?;
                if request.sampled && output.state.reset_byte != 0 {
                    output.state.reset_byte = 2;
                }
                let current_action = if output.return_flags != 0 { chosen.action } else { request.context.current_action };
                let events = request.events.map(|input|
                    gdl_core::player_animation::consume_events(request.context, chosen, output.return_flags, input)
                ).transpose()?;
                Ok(serde_json::json!({"selection":chosen,"animation":output,"current_action":current_action,
                    "attack_type":gdl_core::player_animation::attack_type(current_action),"events":events}))
            })();
            match result {
                Ok(output) => serde_json::json!({"request":request,"output":output,"error":null}),
                Err(e) => serde_json::json!({"request":request,"output":null,"error":e.to_string()}),
            }
        }).collect();
        let report = serde_json::json!({"source_sha256":hash(&source),"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"bounded DoPlayerAction animation-input branches + valid AnimateTree/clock, optional sampling handshake and ordinary Sorceress event-word/missile-request timing; requested action producer, projectile creation/hits, other gameplay side effects and world/root transforms excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[3])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "anim-select" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<AnimationSelectionRequest> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("animation selection request policy exceeded".into());
        }
        let outcomes: Vec<_> = requests
            .into_iter()
            .map(|request| {
                match gdl_core::animation_clock::select(
                    request.state,
                    request.current,
                    request.requested,
                    request.input,
                ) {
                    Ok(mut output) => {
                        if request.sampled && output.state.reset_byte != 0 {
                            output.state.reset_byte = 2;
                        }
                        serde_json::json!({"request":request,"output":output,"error":null})
                    }
                    Err(e) => {
                        serde_json::json!({"request":request,"output":null,"error":e.to_string()})
                    }
                }
            })
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"valid-sequence AnimateTree selection with supplied action/clip/mode/blend inputs and optional tagged-node sampling reset handshake; cache setup, root transfer, player action choices and events excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "anim-clock-start" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<ClockStartRequest> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("clock start request policy exceeded".into());
        }
        let outcomes: Vec<_> = requests
            .into_iter()
            .map(|request| {
                let result = (|| {
                    let mut state =
                        gdl_core::animation_clock::initialize(request.state, request.input)?;
                    if request.evaluate {
                        state = gdl_core::animation_clock::advance(
                            state,
                            request.input.now,
                            request.input.frames,
                            request.input.anim_time_scale,
                        )?
                        .state;
                    }
                    // Source DoAnimation's reset handshake, only requested when the
                    // fixture proves tagged skeletal nodes were actually sampled.
                    if request.sampled && state.reset_byte != 0 {
                        state.reset_byte = 2;
                    }
                    Ok::<_, gdl_core::numeric::UnsupportedNumericDomain>(state)
                })();
                match result {
                    Ok(state) => serde_json::json!({"request":request,"state":state,"error":null}),
                    Err(e) => {
                        serde_json::json!({"request":request,"state":null,"error":e.to_string()})
                    }
                }
            })
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"valid-sequence InitAnim, optional CalcAnimInfo and source sampling reset handshake; requested action/sequence/blend inputs must be supplied, not predicted"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "anim-clock" {
        let raw = bounded_read(Path::new(&args[1]), 4 * 1024 * 1024)?;
        let requests: Vec<ClockRequest> = serde_json::from_slice(&raw)?;
        if requests.len() > 4096 {
            return Err("clock request policy exceeded".into());
        }
        let outcomes: Vec<_> = requests
            .into_iter()
            .map(|request| {
                let result = gdl_core::animation_clock::advance(
                    request.state,
                    request.now,
                    request.frames,
                    request.anim_time_scale,
                );
                match result {
                    Ok(output) => {
                        serde_json::json!({"request":request,"output":output,"error":null})
                    }
                    Err(e) => {
                        serde_json::json!({"request":request,"output":null,"error":e.to_string()})
                    }
                }
            })
            .collect();
        let report = serde_json::json!({"requests_sha256":hash(&raw),"outcomes":outcomes,
            "scope":"finite CalcAnimInfo clock state; input/action transitions, InitAnim, sampling, events and game loop excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if (args.len() == 4 && args[0] == "anim-samples")
        || (args.len() == 5 && args[0] == "anim-transforms")
    {
        let transforms = args[0] == "anim-transforms";
        let bytes = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let rig_bytes = if transforms {
            Some(bounded_read(Path::new(&args[2]), 64 * 1024 * 1024)?)
        } else {
            None
        };
        let rig = rig_bytes
            .as_ref()
            .map(|b| gdl_formats::animation::Animations::parse(b))
            .transpose()?;
        let requests_raw = bounded_read(
            Path::new(&args[if transforms { 3 } else { 2 }]),
            4 * 1024 * 1024,
        )?;
        let requests: Vec<AnimationSampleRequest> = serde_json::from_slice(&requests_raw)?;
        if requests.len() > 4096 {
            return Err("animation request count policy exceeded".into());
        }
        let animations = gdl_formats::animation::Animations::parse(&bytes)?;
        let mut outcomes = Vec::new();
        let mut work = 0;
        for request in requests {
            let tree = animations
                .trees
                .get(request.tree)
                .ok_or("tree index out of range")?;
            let track = tree.key_track(&bytes, request.node, request.sequence)?;
            let rig_node = if let Some(rig) = &rig {
                let rig_tree = rig
                    .trees
                    .get(request.tree)
                    .ok_or("rig tree index out of range")?;
                if rig_tree.nodes.len() != tree.nodes.len()
                    || !rig_tree.nodes.iter().zip(&tree.nodes).all(|(a, b)| {
                        a.label == b.label && a.parent == b.parent && a.node_type == b.node_type
                    })
                {
                    return Err("animation/rig node correspondence differs".into());
                }
                Some(
                    rig_tree
                        .nodes
                        .get(request.node as usize)
                        .ok_or("rig node index out of range")?,
                )
            } else {
                None
            };
            work += track.key_frames.len();
            if work > 1_000_000 {
                return Err("animation sample work policy exceeded".into());
            }
            let result: Result<[f32; 9], Box<dyn Error>> =
                gdl_core::animation::sample_channels(&track, request.frame)
                    .map_err(|e| Box::new(e) as Box<dyn Error>);
            let result = result.and_then(|values| {
                if let Some(blend) = &request.transition
                    && track.transform_flags & 0x0fff != 0
                {
                    let from = if blend.fraction == 0.0 {
                        values
                    } else {
                        blend.from
                    };
                    let fraction = if blend.fraction == 0.0 {
                        1.0
                    } else {
                        blend.fraction
                    };
                    return gdl_core::animation::blend_channels(
                        from,
                        values,
                        fraction,
                        blend.mirror,
                    )
                    .map_err(|e| Box::new(e) as Box<dyn Error>);
                }
                Ok(values)
            });
            let sample = match result {
                Ok(values) => {
                    if let Some(node) = rig_node {
                        match gdl_core::transform::local_transform(
                            values,
                            node.initial_position,
                            track.transform_flags,
                        ) {
                            Ok((matrix, scale)) => {
                                serde_json::json!({"values":values,"bits":values.map(f32::to_bits),"matrix":matrix,
                            "matrix_bits":matrix.map(f32::to_bits),"scale":scale,"scale_bits":scale.map(f32::to_bits),"error":null})
                            }
                            Err(e) => {
                                serde_json::json!({"values":null,"bits":null,"matrix":null,"matrix_bits":null,"error":e.to_string()})
                            }
                        }
                    } else {
                        serde_json::json!({"values":values,"bits":values.map(f32::to_bits),"error":null})
                    }
                }
                Err(e) => serde_json::json!({"values":null,"bits":null,"error":e.to_string()}),
            };
            outcomes.push(serde_json::json!({"request":request,"sample":sample}));
        }
        let report = serde_json::json!({"source_sha256":hash(&bytes),"requests_sha256":hash(&requests_raw),"rig_sha256":rig_bytes.as_ref().map(|b|hash(b)),
            "scope":"fresh channels and optional local matrices; finite guarded/RZ arithmetic; spacing1..255; clocks/blending/mirror/world transforms/events excluded",
            "outcomes":outcomes});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[if transforms { 4 } else { 3 }])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 5 && args[0] == "anim-keys" {
        let bytes = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let animations = gdl_formats::animation::Animations::parse(&bytes)?;
        let tree_index: usize = args[2].to_str().ok_or("tree index is not UTF-8")?.parse()?;
        let sequence_index: u32 = args[3]
            .to_str()
            .ok_or("sequence index is not UTF-8")?
            .parse()?;
        let tree = animations
            .trees
            .get(tree_index)
            .ok_or("tree index out of range")?;
        let sequence = tree
            .sequences
            .get(sequence_index as usize)
            .ok_or("sequence index out of range")?;
        let mut outcomes = Vec::new();
        let mut key_count = 0;
        for node in tree.nodes.iter().filter(|n| n.node_type == 1) {
            let track = tree.key_track(&bytes, node.index, sequence_index)?;
            key_count += track.key_frames.len();
            if key_count > 100_000 {
                return Err("aggregate decoded-key policy exceeded".into());
            }
            let mut keys = Vec::new();
            let result = gdl_core::animation::visit_keys(&track, |frame, values| {
                keys.push(serde_json::json!({"frame":frame,"values":values,"bits":values.map(f32::to_bits)}));
            });
            outcomes.push(serde_json::json!({"node_index":node.index,"node_label":node.label,
                "transform_flags":track.transform_flags,"initial_only":track.initial_only,"compressed":track.compressed,
                "bitmap_file_offset":track.bitmap_file_offset,"bitmap_raw":track.bitmap_raw,
                "initial_file_offset":track.initial_file_offset,"initial_raw":track.initial_raw,
                "rows_file_offset":track.rows_file_offset,"rows_raw":track.rows_raw,
                "keys":keys,"error":result.err().map(|e|e.to_string())}));
        }
        let report = serde_json::json!({"source_sha256":hash(&bytes),"tree_index":tree_index,
            "sequence_index":sequence_index,"sequence":sequence,"outcomes":outcomes,
            "numeric_contract":"normal finite/zero guarded addition toward zero; PCSX2 2.6.3 FPU.Roundmode=3 reference subset",
            "scope":"exact encoded keys; interpolation, wrap, matrices, transitions and events excluded"});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[4])?;
        serde_json::to_writer_pretty(&mut file, &report)?;
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3
        && (args[0] == "objects-inspect"
            || args[0] == "world-inspect"
            || args[0] == "anim-inspect"
            || args[0] == "anim-initial-frames")
    {
        let bytes = bounded_read(Path::new(&args[1]), 64 * 1024 * 1024)?;
        let report = if args[0] == "anim-initial-frames" {
            let animations = gdl_formats::animation::Animations::parse(&bytes)?;
            let mut samples = Vec::new();
            let mut retained = 0usize;
            for tree in &animations.trees {
                let tree_samples = tree.initial_channels(&bytes)?;
                retained += tree_samples
                    .iter()
                    .map(|s| s.bitmap_raw.len() + s.initial_values_raw.len())
                    .sum::<usize>();
                if retained > 64 * 1024 * 1024 {
                    return Err("aggregate initial-channel byte policy exceeded".into());
                }
                samples.push(tree_samples);
            }
            serde_json::json!({"source_sha256":hash(&bytes),"initial_channels":samples,
                "scope":"fresh frame-zero values only; compressed deltas/interpolation/matrix assembly/playback/events not implemented"})
        } else if args[0] == "anim-inspect" {
            let animations = gdl_formats::animation::Animations::parse(&bytes)?;
            serde_json::json!({"source_sha256":hash(&bytes),"animations":animations,
                "scope":"metadata and translation-only hierarchy; frame sampling/events not implemented"})
        } else if args[0] == "world-inspect" {
            let world = gdl_formats::world::World::parse(&bytes)?;
            let placements = world.placements()?;
            serde_json::json!({"source_sha256":hash(&bytes),"world":world,"candidate_placements":placements})
        } else {
            let objects = gdl_formats::objects::Objects::parse(&bytes)?;
            let mut outcomes = Vec::new();
            for object in &objects.objects {
                for part in &object.parts {
                    let result = part.decode_integer_geometry();
                    outcomes.push(match result {
                        Ok(geometry) => serde_json::json!({"object":object.index,"part":part.index,
                            "geometry":geometry,"error":null}),
                        Err(error) => serde_json::json!({"object":object.index,"part":part.index,
                            "geometry":null,"error":error.to_string()}),
                    });
                }
            }
            serde_json::json!({"source_sha256":hash(&bytes),"objects":objects,"outcomes":outcomes})
        };
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[2])?;
        let mut buffered = std::io::BufWriter::new(&mut file);
        serde_json::to_writer_pretty(&mut buffered, &report)?;
        buffered.flush()?;
        drop(buffered);
        file.write_all(b"\n")?;
        return Ok(());
    }
    if args.len() == 3 && args[0] == "texdef-inspect" {
        return texdef_export(Path::new(&args[1]), None, Path::new(&args[2]));
    }
    if args.len() == 4 && args[0] == "texdef-extract" {
        return texdef_export(
            Path::new(&args[1]),
            Some(Path::new(&args[2])),
            Path::new(&args[3]),
        );
    }
    if args.len() != 3 || (args[0] != "wad-inspect" && args[0] != "wad-extract") {
        return Err("Usage: gdl_tools wad-inspect WAD.BIN NEW_MANIFEST.json\n       gdl_tools wad-extract WAD.BIN NEW_DIRECTORY\n       gdl_tools texdef-inspect TEXDEF.PS2 NEW_DIRECTORY\n       gdl_tools texdef-extract TEXDEF.PS2 TEXTURES.PS2 NEW_DIRECTORY\n       gdl_tools objects-inspect OBJECTS.PS2 NEW_REPORT.json\n       gdl_tools world-inspect WORLDS.PS2 NEW_REPORT.json\n       gdl_tools anim-inspect ANIM.PS2 NEW_REPORT.json\n       gdl_tools anim-initial-frames ANIM.PS2 NEW_REPORT.json\n       gdl_tools anim-keys ANIM.PS2 TREE_INDEX SEQUENCE_INDEX NEW_REPORT.json\n       gdl_tools anim-samples ANIM.PS2 REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-transforms ANIM.PS2 INITIAL_POSITION_ANIM.PS2 REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-clock REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-clock-start START_REQUESTS.json NEW_REPORT.json\n       gdl_tools anim-select SELECTION_REQUESTS.json NEW_REPORT.json\n       gdl_tools player-anim-select ANIM.PS2 PLAYER_REQUESTS.json NEW_REPORT.json\n       gdl_tools world-matrix MATRIX_REQUESTS.json NEW_REPORT.json\n       gdl_tools motion-code MOTION_REQUESTS.json NEW_REPORT.json\n       gdl_tools motion-request MOTION_STEPS.json NEW_REPORT.json\n       gdl_tools vu0-fqdist FQDIST_REQUESTS.json NEW_REPORT.json\n       gdl_tools collision-query WORLDS.PS2 NEW_REPORT.json [QUERIES.json]\n       gdl_tools player-displacement WORLDS.PS2 STEPS.json NEW_REPORT.json\n       gdl_tools textrom-inspect ROM NEW_REPORT.json".into());
    }
    let extracting = args[0] == "wad-extract";
    let source = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    if output.exists() {
        return Err("output already exists; use a new path (existing data is preserved)".into());
    }
    let limits = Limits::default();
    let bytes = bounded_read(source, limits.archive_bytes as u64)?;
    let source_sha256 = hash(&bytes);
    let wad = Wad::parse(&bytes, limits)?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage_path = parent.join(format!(".gdl-staging-{}", std::process::id()));
    fs::create_dir(&stage_path)?;
    let stage = Staging(stage_path);
    let mut manifest = Manifest {
        schema_version: 1,
        converter_version: env!("CARGO_PKG_VERSION"),
        source_sha256: source_sha256.clone(),
        declared_decoded_bytes: wad.declared_decoded_bytes,
        duplicate_hashes: wad.duplicate_hashes.clone(),
        entries: Vec::with_capacity(wad.entries().len()),
    };
    let mut failures = 0;
    for entry in wad.entries() {
        let identity = format!("{source_sha256}:{}:{:08x}", entry.index, entry.path_hash);
        let mut row = Outcome {
            entry: entry.clone(),
            identity,
            stored_sha256: hash(wad.stored(entry.index as usize)?),
            decoded_sha256: None,
            output_file: None,
            error: None,
        };
        match wad.decode(entry.index as usize) {
            Ok(data) => {
                row.decoded_sha256 = Some(hash(&data));
                if extracting {
                    let name = entry.output_filename();
                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(stage.0.join(&name))?;
                    file.write_all(&data)?;
                    file.sync_all()?;
                    row.output_file = Some(name);
                }
            }
            Err(e) => {
                failures += 1;
                row.error = Some(e.to_string());
            }
        }
        manifest.entries.push(row);
    }
    write_manifest(&stage.0.join("manifest.json"), &manifest)?;
    if extracting && failures != 0 {
        return Err(format!(
            "{failures} decode failures; no cache published. Run wad-inspect to retain diagnostics."
        )
        .into());
    }
    if extracting {
        fs::rename(&stage.0, output)?;
    } else {
        fs::rename(stage.0.join("manifest.json"), output)?;
    }
    println!(
        "{} entries; {} successful; {failures} failures; {} declared decoded bytes",
        manifest.entries.len(),
        manifest.entries.len() - failures,
        manifest.declared_decoded_bytes
    );
    if failures != 0 {
        return Err("inspection recorded decode failures".into());
    }
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
