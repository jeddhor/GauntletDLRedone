//! Critters: bosses and statue enemies (research/critters.md). Transcribed
//! from SLUS_200.47 0x191850–0x1a1fa8; no reference comparison yet.
//!
//! Instances keep the original 0xb70-byte layout in a [`Record`]; pointer
//! fields are native (type, parent/child, tree nodes). The animated tree is
//! evaluated in the simulation (crate::tree_pose) because collision nodes,
//! attack origins and transitions read node matrices. Presentation-only work
//! (texture mods, skin FX, shadows, health-meter blits, debug DmgFx,
//! MBTree flags) is not simulated.
use super::effects::EffectHeader;
use super::record::Record;
use super::{unsupported, Game, Unsupported, R};
use crate::animation_clock::{ClipTiming, ClockState};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul};
use crate::tree_pose::{node_world, node_world_chain, Mat, IDENTITY};
use crate::world_transform::MatrixBox;
use gdl_formats::animation::Animations;
use gdl_formats::critter::{label, CritterWad};
use std::sync::Arc;

type V3 = [f32; 3];

pub const INST_SIZE: usize = 0xb70;
pub const MAX_INSTS: usize = 0x10;

/// Instance offsets used across the port.
pub mod c {
    pub const ID: usize = 0x002;
    pub const STATE: usize = 0x008;
    pub const MATRIX: usize = 0x010;
    pub const POS: usize = 0x040;
    pub const ATTN: usize = 0x050;
    pub const COL: usize = 0x060;
    pub const ANIM_SEQ: usize = 0x092;
    pub const ANIM_FRAMES: usize = 0x094;
    pub const ANIM_FRAME: usize = 0x09c;
    pub const ANIM_START: usize = 0x0a4;
    pub const YAW_TARGET: usize = 0x104;
    pub const YAW: usize = 0x108;
    pub const DIFFICULTY: usize = 0x11c;
    pub const INV_DIFFICULTY: usize = 0x120;
    pub const MOVE: usize = 0x124;
    pub const NEXT: usize = 0x126;
    pub const PATTERN: usize = 0x128;
    pub const PATTERN_CAND: usize = 0x12a;
    pub const PATTERN_STEP: usize = 0x12c;
    pub const MOVE_DONE: usize = 0x12e;
    pub const TARGET: usize = 0x130;
    pub const TARGET_CAND: usize = 0x132;
    pub const GRABBED: usize = 0x134;
    pub const NTARGETS: usize = 0x136;
    pub const TARGETS: usize = 0x140;
    pub const ANGER: usize = 0x200;
    pub const TARGET_POS: usize = 0x240;
    pub const ACTION_FLAGS: usize = 0x250;
    pub const SFX_FLAGS: usize = 0x252;
    pub const HOLD_UNTIL: usize = 0x258;
    pub const MOVE_TIMES: usize = 0x25c;
    pub const PATTERN_TIMES: usize = 0x35c;
    pub const NODE_MATRIX: usize = 0x3e0;
    pub const NODE_POS: usize = 0x410;
    pub const HOME_MATRIX: usize = 0x420;
    pub const HOME_POS: usize = 0x460;
    pub const PREV_NODE_POS: usize = 0x470;
    pub const NCHILDREN: usize = 0x486;
    pub const NLIVE_CHILDREN: usize = 0x487;
    pub const LEASH_CENTRE: usize = 0x4e0;
    pub const LEASH_DIST: usize = 0x4f0;
    pub const HEALTH: usize = 0x4f4;
    pub const DAMAGE_TAKEN: usize = 0x4f8;
    pub const DAMAGE_FLAGS: usize = 0x4fc;
    pub const KNOCK: usize = 0x500;
    pub const VELOCITY: usize = 0x510;
    pub const HIT_TIME: usize = 0x520;
    pub const COLNODES: usize = 0x540;
    pub const COLNODE_HIT: usize = 0xb40;
    pub const MOVE_FX: usize = 0xb42;
    pub const PAUSE: usize = 0xb4c;
    pub const SLOW: usize = 0xb4e;
    pub const FX_HOLD: usize = 0xb50;
    pub const WAYPOINT: usize = 0xb5c;
}

/// One loaded CRITTER/*.WAD with its runtime-written records.
#[derive(Clone, Debug)]
pub struct CritterFile {
    pub name: String,
    pub wad: CritterWad,
    /// TYPE +0x138: tree index in `model` per type (None: no tree).
    pub trees: Vec<Option<usize>>,
    /// The DESC's model package ANIM.PS2 (GetCritterDesc).
    pub model: Option<Arc<(Animations, Vec<u8>)>>,
}

impl CritterFile {
    pub fn i16_pub(r: &[u8], o: usize) -> i16 {
        Self::i16_(r, o)
    }
    pub fn i32_pub(r: &[u8], o: usize) -> i32 {
        Self::i32_(r, o)
    }
    pub fn f32_pub(r: &[u8], o: usize) -> f32 {
        Self::f32_(r, o)
    }
    fn i16_(r: &[u8], o: usize) -> i16 {
        i16::from_le_bytes([r[o], r[o + 1]])
    }
    fn set_i16(r: &mut [u8], o: usize, v: i16) {
        r[o..o + 2].copy_from_slice(&v.to_le_bytes());
    }
    fn i32_(r: &[u8], o: usize) -> i32 {
        i32::from_le_bytes(r[o..o + 4].try_into().unwrap())
    }
    fn set_i32(r: &mut [u8], o: usize, v: i32) {
        r[o..o + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn f32_(r: &[u8], o: usize) -> f32 {
        f32::from_le_bytes(r[o..o + 4].try_into().unwrap())
    }
}

/// A critter file for the level: (name, parsed WAD, model ANIM.PS2 + bytes).
pub type CritterInput = (String, CritterWad, Option<Arc<(Animations, Vec<u8>)>>);

/// A grabbed player's parent: (critter, tree node, local offset).
pub type Grab = (usize, Option<usize>, [f32; 3]);

/// (file, type) of a critter type.
pub type TypeRef = (usize, usize);

#[derive(Clone, Debug, Default)]
pub struct CritterData {
    pub files: Vec<CritterFile>,
    /// CritterETypes[desc kind][slot].
    pub etypes: [[Option<TypeRef>; 6]; 9],
    /// SafeRockHide (a DAMG of kind 6 was initialised).
    pub safe_rock_hide: bool,
}

/// The +0x84 AnimInfo clock of an instance record (the enemy layout).
pub fn anim_info_clock(r: &Record) -> ClockState {
    ClockState {
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
    }
}

/// Write a clock into an instance record's +0x84 AnimInfo (the enemy layout).
pub fn set_anim_info_clock(r: &mut Record, c: &ClockState) {
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

/// One instance (CritterInsts entry).
#[derive(Clone, Debug)]
pub struct Critter {
    pub rec: Record,
    pub ty: TypeRef,
    /// +0xb60 next child in the root's chain, +0xb64 the root (children).
    pub child: Option<usize>,
    pub parent: Option<usize>,
    /// +0x80 animation root node: 0 for a root; a child's subtree node
    /// (CritterInitChild detaches it from the root's animation tree but
    /// keeps the matrix hierarchy, so children share the root's pose).
    pub anim_root: usize,
    pub clock: ClockState,
    pub clip: u16,
    pub clips: Vec<ClipTiming>,
    /// Local matrices of every tree node (current pose).
    pub locals: Vec<Mat>,
    /// Parent indices of the tree nodes.
    pub parents: Vec<i32>,
    /// +0xdc transition node, +0xd4/+0xd8 look-at nodes, +0xe8 third node.
    pub node_dc: Option<usize>,
    pub node_d4: Option<usize>,
    pub node_d8: Option<usize>,
    pub node_e8: Option<usize>,
    /// Collision node tree nodes (+0x540 + 0x60k: +4 node, +8 reference node).
    pub colnode_nodes: Vec<(Option<usize>, Option<usize>)>,
    /// CritterAddShadow (type +0x5c bit 0): SHADOW<n>L1 of the critter's
    /// model (n = slot + 1, slots ≥ 3 → 2) and its position (the critter's
    /// x/z at the floor + 0.1 after ProcessCritter's floor test;
    /// presentation, world-aligned, z mod −32).
    pub shadow: Option<(usize, [f32; 3])>,
    /// The +0x70 MB node's local matrix (rows), as UnparentMatrix stored the
    /// instance matrix at the end of CritterInitGeo and ProcessCritter, and
    /// the WORLDS object it is parented to (CritterCollideWorld's flag-0x1000
    /// floor; None: EnemyRootNode). ProcessCritter starts from its
    /// GetWorldMat, so critters ride moving floors (trace-k2-newgame-v4).
    pub node: Option<[[f32; 4]; 4]>,
    pub floor: Option<usize>,
}

impl Game {
    pub(crate) fn crit(&self, i: usize) -> &Critter {
        self.critters[i].as_ref().expect("live critter")
    }
    pub(crate) fn crit_mut(&mut self, i: usize) -> &mut Critter {
        self.critters[i].as_mut().expect("live critter")
    }
    fn ctype(&self, t: TypeRef) -> &[u8] {
        &self.critter_data.files[t.0].wad.types[t.1]
    }
    fn cdesc(&self, t: TypeRef) -> &[u8] {
        let f = &self.critter_data.files[t.0];
        let d = CritterFile::i16_(&f.wad.types[t.1], 0x50) as usize;
        &f.wad.descs[d]
    }
    /// DESC +0x20: 4 boss, 3/7/8 golem-type, others animate only.
    pub fn critter_kind(&self, i: usize) -> i16 {
        CritterFile::i16_(self.cdesc(self.crit(i).ty), 0x20)
    }
    fn type_i16(&self, i: usize, o: usize) -> i16 {
        CritterFile::i16_(self.ctype(self.crit(i).ty), o)
    }
    fn type_f32(&self, i: usize, o: usize) -> f32 {
        CritterFile::f32_(self.ctype(self.crit(i).ty), o)
    }
    fn type_u32(&self, i: usize, o: usize) -> u32 {
        CritterFile::i32_(self.ctype(self.crit(i).ty), o) as u32
    }
    /// MOVE record `m` (relative to the type's first move).
    fn cmove(&self, i: usize, m: usize) -> &[u8] {
        let t = self.crit(i).ty;
        let first = CritterFile::i16_(&self.critter_data.files[t.0].wad.types[t.1], 0x112) as usize;
        &self.critter_data.files[t.0].wad.moves[first + m]
    }
    fn move_count(&self, i: usize) -> i16 {
        self.type_i16(i, 0x110)
    }
    fn cpattern(&self, i: usize, p: usize) -> &[u8] {
        let t = self.crit(i).ty;
        let first = CritterFile::i16_(&self.critter_data.files[t.0].wad.types[t.1], 0x116) as usize;
        &self.critter_data.files[t.0].wad.patterns[first + p]
    }
    fn cdamage(&self, i: usize, d: usize) -> &[u8] {
        &self.critter_data.files[self.crit(i).ty.0].wad.damages[d]
    }
    fn csfx(&self, i: usize, s: usize) -> &[u8] {
        &self.critter_data.files[self.crit(i).ty.0].wad.sfx[s]
    }
    fn mv_i32(&self, i: usize, m: usize, o: usize) -> i32 {
        CritterFile::i32_(self.cmove(i, m), o)
    }
    fn mv_i16(&self, i: usize, m: usize, o: usize) -> i16 {
        CritterFile::i16_(self.cmove(i, m), o)
    }
    fn mv_f32(&self, i: usize, m: usize, o: usize) -> f32 {
        CritterFile::f32_(self.cmove(i, m), o)
    }
    fn cr(&self, i: usize) -> &Record {
        &self.crit(i).rec
    }
    fn crm(&mut self, i: usize) -> &mut Record {
        &mut self.crit_mut(i).rec
    }

    /// TYPE +0xe4 × level_data +0xac: a critter type's starting health.
    pub fn critter_type_rec_pub(&self, t: TypeRef) -> Option<f32> {
        let ty = self.critter_data.files.get(t.0)?.wad.types.get(t.1)?;
        mul(CritterFile::f32_(ty, 0xe4), self.level_f32(0xac)).ok()
    }
    pub(crate) fn crm_pub(&mut self, i: usize) -> &mut Record {
        self.crm(i)
    }
    pub(crate) fn critter_type_f32(&self, i: usize, o: usize) -> f32 {
        self.type_f32(i, o)
    }
    pub(crate) fn critter_type_u32(&self, i: usize, o: usize) -> u32 {
        self.type_u32(i, o)
    }
    pub(crate) fn critter_type_i16(&self, i: usize, o: usize) -> i16 {
        self.type_i16(i, o)
    }
    pub(crate) fn critter_type_rec(&self, i: usize) -> &[u8] {
        self.ctype(self.crit(i).ty)
    }
    pub(crate) fn critter_type_rec_t(&self, t: TypeRef) -> &[u8] {
        self.ctype(t)
    }
    pub(crate) fn critter_move_i32(&self, i: usize, m: usize, o: usize) -> i32 {
        self.mv_i32(i, m, o)
    }
    pub(crate) fn critter_move_i16(&self, i: usize, m: usize, o: usize) -> i16 {
        self.mv_i16(i, m, o)
    }
    pub(crate) fn critter_move_f32(&self, i: usize, m: usize, o: usize) -> f32 {
        self.mv_f32(i, m, o)
    }
    pub(crate) fn critter_move_rec(&self, i: usize, m: usize) -> &[u8] {
        self.cmove(i, m)
    }
    /// MOVE field at a signed move index: the original indexes TYPE +0x124
    /// with +0x124 = -1 in some paths (CritterChildCriticalMove), reading the
    /// preceding record of the shared MOVE chunk.
    pub(crate) fn critter_move_i16_signed(&self, i: usize, m: i16, o: usize) -> R<i16> {
        let t = self.crit(i).ty;
        let first = CritterFile::i16_(&self.critter_data.files[t.0].wad.types[t.1], 0x112) as i32;
        let k = usize::try_from(first + i32::from(m)).map_err(|_| Unsupported("critter MOVE before the chunk".into()))?;
        let rec = self.critter_data.files[t.0].wad.moves.get(k).ok_or_else(|| Unsupported("critter MOVE past the chunk".into()))?;
        Ok(CritterFile::i16_(rec, o))
    }
    /// The children of root `i` in chain order (+0xb60 links).
    pub(crate) fn critter_children(&self, i: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut k = self.crit(i).child;
        while let Some(n) = k {
            if MAX_INSTS < out.len() {
                break;
            }
            out.push(n);
            k = self.crit(n).child;
        }
        out
    }
    pub(crate) fn move_count_pub(&self, i: usize) -> i16 {
        self.move_count(i)
    }
    pub(crate) fn critter_pattern_rec(&self, i: usize, p: usize) -> &[u8] {
        self.cpattern(i, p)
    }
    pub(crate) fn critter_pattern_i16(&self, i: usize, p: usize, o: usize) -> i16 {
        CritterFile::i16_(self.cpattern(i, p), o)
    }
    pub(crate) fn critter_pattern_f32(&self, i: usize, p: usize, o: usize) -> f32 {
        CritterFile::f32_(self.cpattern(i, p), o)
    }
    pub(crate) fn critter_damage_rec(&self, i: usize, d: usize) -> Vec<u8> {
        self.cdamage(i, d).to_vec()
    }
    pub(crate) fn csfx_pub(&self, i: usize, s: usize) -> Vec<u8> {
        self.csfx(i, s).to_vec()
    }
    pub(crate) fn critter_attention_pub(&mut self, i: usize) -> R<()> {
        self.critter_attention(i)
    }
    pub(crate) fn critter_update_colnodes_pub(&mut self, i: usize) -> R<()> {
        self.critter_update_colnodes(i)
    }
    pub(crate) fn critter_node_matrix_pub(&mut self, i: usize, node: Option<usize>, at: usize, offset: Option<V3>) -> R<Mat> {
        self.critter_node_matrix(i, node, at, offset)
    }
    pub(crate) fn critter_set_yaw_matrix_pub(&mut self, i: usize) -> R<()> {
        self.critter_set_yaw_matrix(i)
    }
    pub(crate) fn critter_node_world_pub(&self, i: usize, n: usize) -> R<Mat> {
        self.critter_node_world(i, n, None)
    }
    pub(crate) fn critter_root_pub(&self, i: usize) -> Mat {
        self.critter_root(i)
    }

    /// CritterLoadAllTypes / CritterAllocType / CritterLoadFinish /
    /// CritterInitAllMoves for the level's critter files.
    pub(crate) fn critter_load_types(&mut self) -> R<()> {
        let nfiles = self.critter_data.files.len();
        for f in 0..nfiles {
            let ntypes = self.critter_data.files[f].wad.types.len();
            for t in 0..ntypes {
                // CritterLoadFinish: "<DESC+0x10><TYPE name>" or the alias type's tree.
                let (alias, name) = {
                    let file = &self.critter_data.files[f];
                    let ty = &file.wad.types[t];
                    let d = CritterFile::i16_(ty, 0x50) as usize;
                    let desc = file.wad.descs.get(d).ok_or_else(|| Unsupported(format!("critter DESC {d}")))?;
                    (CritterFile::i16_(ty, 0x11e), format!("{}{}", label(desc, 0x10, 16), label(ty, 0, 16)))
                };
                let tree = if alias < 0 {
                    let file = &self.critter_data.files[f];
                    file.model.as_ref().and_then(|m| m.0.trees.iter().position(|tr| tr.label == name))
                } else {
                    self.critter_data.files[f].trees.get(alias as usize).copied().flatten()
                };
                self.critter_data.files[f].trees[t] = tree;
                if tree.is_some() {
                    for (o, name_at) in [(0x56, 0x20), (0x58, 0x30), (0x5a, 0x40)] {
                        let n = self.critter_node_idx((f, t), name_at, None);
                        CritterFile::set_i16(&mut self.critter_data.files[f].wad.types[t], o, n);
                    }
                }
                let file = &self.critter_data.files[f];
                let ty = &file.wad.types[t];
                let slot = CritterFile::i16_(ty, 0x52);
                let kind = CritterFile::i16_(&file.wad.descs[CritterFile::i16_(ty, 0x50) as usize], 0x20);
                if 0 <= slot {
                    let (k, s) = (kind as usize, slot as usize);
                    if k < 9 && s < 6 {
                        self.critter_data.etypes[k][s] = Some((f, t));
                    }
                }
            }
        }
        for f in 0..nfiles {
            for t in 0..self.critter_data.files[f].wad.types.len() {
                self.critter_init_moves((f, t))?;
            }
        }
        Ok(())
    }

    /// CritterGetNodeIdx: AtreeFindNodeIdx (strncmp 15) of the name at
    /// `name_at` in the TYPE record (or `name` when given).
    fn critter_node_idx(&self, t: TypeRef, name_at: usize, name: Option<&str>) -> i16 {
        let file = &self.critter_data.files[t.0];
        let Some(tree) = file.trees[t.1] else { return -1 };
        let n = match name {
            Some(n) => n.to_string(),
            None => label(&file.wad.types[t.1], name_at, 16),
        };
        let b = n.as_bytes();
        if b.len() < 2 {
            return -1;
        }
        let Some(m) = file.model.as_ref() else { return -1 };
        let key: Vec<u8> = b.iter().copied().take(15).collect();
        m.0.trees[tree]
            .nodes
            .iter()
            .position(|nd| nd.label.as_bytes().iter().copied().take(15).eq(key.iter().copied()))
            .map_or(-1, |p| p as i16)
    }

    /// CritterInitMoves 0x191e48 (one type).
    fn critter_init_moves(&mut self, t: TypeRef) -> R<()> {
        let Some(tree) = self.critter_data.files[t.0].trees[t.1] else { return Ok(()) };
        let ty = self.critter_data.files[t.0].wad.types[t.1].clone();
        let (count, first) = (CritterFile::i16_(&ty, 0x110), CritterFile::i16_(&ty, 0x112));
        let (ncol, firstcol) = (CritterFile::i16_(&ty, 0x118), CritterFile::i16_(&ty, 0x11a));
        for k in 0..count.max(0) as usize {
            let m = first as usize + k;
            let rec = self.critter_data.files[t.0].wad.moves.get(m).cloned().ok_or_else(|| Unsupported("critter MOVE index".into()))?;
            if (CritterFile::i32_(&rec, 0) as u32).wrapping_sub(0x30) < 10 {
                let f = CritterFile::i32_(&ty, 0x5c) | 0x10000;
                CritterFile::set_i32(&mut self.critter_data.files[t.0].wad.types[t.1], 0x5c, f);
            }
            if CritterFile::i16_(&rec, 0xc) < 0 {
                let seq = label(&rec, 0x20, 16);
                if !seq.is_empty() {
                    let model = self.critter_data.files[t.0].model.clone().ok_or_else(|| Unsupported("critter model".into()))?;
                    let idx = model.0.trees[tree].sequences.iter().position(|s| s.label == seq);
                    // Errorf + 0 when the sequence is missing.
                    CritterFile::set_i16(&mut self.critter_data.files[t.0].wad.moves[m], 0xc, idx.map_or(0, |v| v as i16));
                }
                let node = self.critter_node_idx(t, 0, Some(&label(&rec, 0x30, 16)));
                CritterFile::set_i16(&mut self.critter_data.files[t.0].wad.moves[m], 0xe, node);
                self.critter_init_damage(t.0, CritterFile::i16_(&rec, 0x48))?;
                self.critter_init_damage(t.0, CritterFile::i16_(&rec, 0x4a))?;
                self.critter_init_sfx(t.0, CritterFile::i16_(&rec, 0x58) as i32, 0)?;
                self.critter_init_sfx(t.0, CritterFile::i16_(&rec, 0x5c) as i32, 0)?;
            }
        }
        for k in 0..ncol.max(0) as usize {
            let n = firstcol as usize + k;
            let name = label(self.critter_data.files[t.0].wad.nodes.get(n).ok_or_else(|| Unsupported("critter NODE index".into()))?, 0, 16);
            let idx = self.critter_node_idx(t, 0, Some(&name));
            CritterFile::set_i16(&mut self.critter_data.files[t.0].wad.nodes[n], 0x14, idx);
        }
        self.critter_init_sfx(t.0, CritterFile::i16_(&ty, 0xf4) as i32, 0)?;
        self.critter_init_sfx(t.0, CritterFile::i16_(&ty, 0xf6) as i32, 0)?;
        Ok(())
    }

    /// CritterInitDamage 0x191d20.
    fn critter_init_damage(&mut self, f: usize, d: i16) -> R<()> {
        let Ok(d) = usize::try_from(d) else { return Ok(()) };
        let rec = self.critter_data.files[f].wad.damages.get(d).cloned().ok_or_else(|| Unsupported("critter DAMG index".into()))?;
        for o in [0x40, 0x44, 0x46, 0x42] {
            self.critter_init_sfx(f, CritterFile::i16_(&rec, o) as i32, 0)?;
        }
        if CritterFile::i16_(&rec, 0) == 6 {
            self.critter_data.safe_rock_hide = true;
        }
        Ok(())
    }

    /// CritterInitSfx 0x191bd0: custom effect (SFXX +8) and sound (+0xc)
    /// resolution, then the chained SFXX (+4).
    fn critter_init_sfx(&mut self, f: usize, s: i32, depth: usize) -> R<()> {
        let Ok(s) = usize::try_from(s) else { return Ok(()) };
        if 64 < depth {
            return unsupported("critter SFXX chain cycle");
        }
        let rec = self.critter_data.files[f].wad.sfx.get(s).cloned().ok_or_else(|| Unsupported("critter SFXX index".into()))?;
        let flags = CritterFile::i32_(&rec, 0) as u32;
        if CritterFile::i32_(&rec, 8) < 0 {
            let v = if flags & 0x0f00_0100 == 0 {
                self.init_custom_effect(Some(f), &label(&rec, 0x10, 16), CritterFile::i16_(&rec, 0x44), CritterFile::i16_(&rec, 0x46))
            } else {
                // Texture mods / particles: presentation (FindTexMod/MBOX_FindTexture).
                -1
            };
            CritterFile::set_i32(&mut self.critter_data.files[f].wad.sfx[s], 8, v);
        }
        if CritterFile::i32_(&self.critter_data.files[f].wad.sfx[s], 0xc) < 0 {
            let fmt = label(&rec, 0x20, 32);
            let id = if fmt.is_empty() {
                -1
            } else {
                let code = self.level.get(8).copied().unwrap_or(b'G') as char;
                let name = if fmt.contains("%c") { fmt.replacen("%c", &code.to_string(), 1) } else { fmt.clone() };
                if fmt.contains('%') && !fmt.contains("%c") {
                    return unsupported(format!("critter sound format {fmt}"));
                }
                self.sounds.rom.as_ref().and_then(|r| r.find(&name)).unwrap_or(-1)
            };
            CritterFile::set_i32(&mut self.critter_data.files[f].wad.sfx[s], 0xc, id);
        }
        self.critter_init_sfx(f, CritterFile::i32_(&rec, 4), depth + 1)
    }

    /// InitCustomEffectSub 0x179aa8 (critter file f's model when given, then
    /// weapon_anim, powerup_anim, item_anim and item_anim2):
    /// EffectHeaders[0x61 + n].
    pub(crate) fn init_custom_effect(&mut self, f: Option<usize>, name: &str, _zmod: i16, _alpha: i16) -> i32 {
        if name.is_empty() {
            return -1;
        }
        let found = f.and_then(|f| self.critter_data.files[f].model.as_ref().map(|m| (f, m))).and_then(|(f, m)| {
            m.0.trees.iter().position(|t| t.label == name).map(|k| {
                let s = m.0.trees[k].sequences.first();
                EffectHeader { source: 2 + f as u8, tree: k, frames: s.map_or(0, |s| s.frames), timing: s.map_or(0, |s| s.timing_factor) }
            })
        });
        self.register_custom_effect(found, name)
    }

    /// InitCustomEffectSub's search after the caller's package (`own`):
    /// weapon_anim, powerup_anim and the item packages; the next custom
    /// EffectHeaders entry (0x61 + n, at most 0x79 of them).
    pub(crate) fn register_custom_effect(&mut self, own: Option<EffectHeader>, name: &str) -> i32 {
        if name.is_empty() {
            return -1;
        }
        let Some(h) = own.or_else(|| self.effects.tables.shared_header(name)) else { return -1 };
        let n = self.effects.custom_effects;
        if 0x78 < n {
            return -1;
        }
        self.effects.custom_effects += 1;
        let idx = 0x61 + n;
        if let Some(slot) = self.effects.tables.headers.get_mut(idx) {
            *slot = Some(h);
        }
        idx as i32
    }

    /// AddBoss 0x19da88 for a boss locator (kind 6) matrix.
    pub(crate) fn add_boss(&mut self, m: Mat) -> R<()> {
        self.boss_start_pos = [m[12], m[13], m[14]];
        self.boss_crit_inst = 0;
        if self.critter_data.etypes[4][0].is_some()
            && let Some(i) = self.critter_new_inst(4, 0, m)?
        {
            self.boss_crit_inst = i as i32 + 1;
        }
        Ok(())
    }

    /// CritterEmptyInst 0x193ab0.
    fn critter_empty_inst(&mut self) -> Option<usize> {
        let mut k = 0;
        if self.critters.first().is_some_and(Option::is_some) {
            k = 1;
            while k < self.critters.len() && self.critters[k].is_some() {
                k += 1;
            }
        }
        if MAX_INSTS <= k {
            return None;
        }
        if k == self.critters.len() {
            self.critters.push(None);
        }
        Some(k)
    }

    /// CritterNewInst 0x193be0.
    pub(crate) fn critter_new_inst(&mut self, kind: usize, slot: usize, m: Mat) -> R<Option<usize>> {
        let Some(t) = self.critter_data.etypes.get(kind).and_then(|e| e.get(slot)).copied().flatten() else {
            return Ok(None);
        };
        if self.critter_data.files[t.0].trees[t.1].is_none() {
            return Ok(None);
        }
        let Some(i) = self.critter_empty_inst() else { return Ok(None) };
        self.critter_init_inst(i, t)?;
        self.critter_init_geo(i, m)?;
        if self.type_u32(i, 0x5c) & 1 != 0 {
            let n = if slot < 3 { slot } else { 1 };
            let at = self.crit(i).rec.v3(c::POS);
            self.crit_mut(i).shadow = Some((n + 1, at));
        }
        self.critter_init_colnodes(i);
        self.critter_add_health_meter(i);
        self.crm(i).set_u8(c::NCHILDREN, 0);
        let mut child_ty = CritterFile::i16_(self.ctype(t), 0x11c);
        let mut prev = i;
        while 0 <= child_ty {
            let ct = (t.0, child_ty as usize);
            if self.critter_data.files[t.0].wad.types.get(ct.1).is_none() {
                return unsupported("critter child TYPE index");
            }
            // The original does not test CritterEmptyInst's result here.
            let Some(k) = self.critter_empty_inst() else { return unsupported("no free critter instance for a child") };
            self.critter_init_inst(k, ct)?;
            self.critter_init_child(k, i)?;
            self.critter_init_colnodes(k);
            self.critter_add_health_meter(k);
            self.crit_mut(prev).child = Some(k);
            self.crit_mut(k).parent = Some(i);
            child_ty = CritterFile::i16_(self.ctype(ct), 0x11c);
            let n = self.cr(i).u8_(c::NCHILDREN).wrapping_add(1);
            self.crm(i).set_u8(c::NCHILDREN, n);
            prev = k;
        }
        if self.critter_kind(i) == 8 {
            // +0xb5c: FindClosestWaypoint(10, position, 0) (native lookout + 1).
            let p = self.cr(i).v3(c::POS);
            let w = self.find_closest_waypoint(10.0, p, false)?.map_or(0, |k| k as u32 + 1);
            self.crm(i).set_u32(c::WAYPOINT, w);
        }
        Ok(Some(i))
    }

    /// CritterInitInst 0x193298.
    fn critter_init_inst(&mut self, i: usize, t: TypeRef) -> R<()> {
        let mut rec = Record::zeroed(INST_SIZE);
        rec.set_i16(0, i as i16);
        rec.set_u16(2, self.critter_new_id);
        self.critter_new_id = self.critter_new_id.wrapping_add(1);
        if 0xfff < self.critter_new_id {
            self.critter_new_id = 1;
        }
        for o in [c::MOVE, c::PATTERN, c::PATTERN_CAND, c::PATTERN_STEP, c::TARGET, c::TARGET_CAND, c::GRABBED, c::MOVE_FX, 0xb44] {
            rec.set_i16(o, -1);
        }
        rec.set_i16(c::NEXT, 0);
        rec.set_i32(0xb48, -1);
        let health = mul(CritterFile::f32_(&self.critter_data.files[t.0].wad.types[t.1], 0xe4), self.level_f32(0xac))?;
        rec.set_f32(c::HEALTH, health);
        for k in 0..4 {
            rec.set_i16(0x524 + 2 * k, -1);
        }
        let file = &self.critter_data.files[t.0];
        let tree = file.trees[t.1].ok_or_else(|| Unsupported("critter tree".into()))?;
        let model = file.model.clone().ok_or_else(|| Unsupported("critter model".into()))?;
        let tr = &model.0.trees[tree];
        let clips = super::setup::clips_of(&model.0, tree);
        let parents: Vec<i32> = tr.nodes.iter().map(|n| n.parent).collect();
        let locals = tr.nodes.iter().map(|n| {
            let mut m = IDENTITY;
            m[12..15].copy_from_slice(&n.initial_position);
            m
        });
        let ncol = CritterFile::i16_(&file.wad.types[t.1], 0x118).max(0) as usize;
        let crit = Critter {
            rec,
            ty: t,
            child: None,
            parent: None,
            anim_root: 0,
            clock: super::setup::fresh_clock(),
            clip: 0,
            clips,
            locals: locals.collect(),
            parents,
            node_dc: None,
            node_d4: None,
            node_d8: None,
            node_e8: None,
            colnode_nodes: vec![(None, None); ncol],
            shadow: None,
            node: None,
            floor: None,
        };
        self.critters[i] = Some(crit);
        self.skin_views.critters.remove(&i);
        Ok(())
    }

    /// CritterInitChild 0x193900: the root's animation instance (+0x80..+0xc4)
    /// is copied, then +0x80 becomes the child's TYPE +0x10 node of the root's
    /// tree (AtreeFindNodeIdx, 16 characters) detached from the root's
    /// animation tree (AtreeNodeSetParent(node, 0, 0, 0): the MB parent is
    /// kept). The look-at nodes follow CritterInitGeo's rules; the child's
    /// matrix stays zero until CritterUpdateChild.
    fn critter_init_child(&mut self, k: usize, root: usize) -> R<()> {
        let bytes: Vec<u8> = (0x80..0xc4).map(|o| self.cr(root).u8_(o)).collect();
        for (n, b) in bytes.iter().enumerate() {
            self.crm(k).set_u8(0x80 + n, *b);
        }
        let (clock, clip) = (self.crit(root).clock, self.crit(root).clip);
        let t = self.crit(k).ty;
        let rt = self.crit(root).ty;
        let name = label(self.ctype(t), 0x10, 16);
        let node = {
            let file = &self.critter_data.files[rt.0];
            let tree = file.trees[rt.1].ok_or_else(|| Unsupported("critter tree".into()))?;
            let model = file.model.as_ref().ok_or_else(|| Unsupported("critter model".into()))?;
            let key: Vec<u8> = name.bytes().take(16).collect();
            model.0.trees[tree].nodes.iter().position(|nd| nd.label.bytes().take(16).eq(key.iter().copied()))
        };
        let Some(node) = node else { return unsupported(format!("critter child node {name} not in the root tree")) };
        let cr = self.crit_mut(k);
        cr.clock = clock;
        cr.clip = clip;
        cr.anim_root = node;
        let n56 = self.type_i16(k, 0x56);
        let mut d4 = self.critter_node(k, n56, None);
        if self.type_u32(k, 0x5c) & 0x10 != 0
            && let Some(n) = d4
            && let Ok(p) = usize::try_from(self.crit(k).parents[n])
        {
            d4 = Some(p);
        }
        let (n58, n5a) = (self.type_i16(k, 0x58), self.type_i16(k, 0x5a));
        let (d8, e8) = (self.critter_node(k, n58, None), self.critter_node(k, n5a, None));
        let cr = self.crit_mut(k);
        cr.node_d4 = d4;
        cr.node_d8 = d8;
        cr.node_e8 = e8;
        Ok(())
    }

    /// The root instance whose tree pose instance `i` uses.
    pub(crate) fn critter_pose_owner(&self, i: usize) -> usize {
        self.crit(i).parent.unwrap_or(i)
    }

    /// GetWorldMat of tree node `n` for instance `i` (children compose
    /// through the root's tree and matrix), with an optional local offset.
    /// With a stored +0x70 node the tree composes through it (its local as
    /// UnparentMatrix last stored it), the floor object chain, the identity
    /// root and the translation-only root, as collision::chain_world does.
    pub(crate) fn critter_node_world(&self, i: usize, n: usize, offset: Option<V3>) -> R<Mat> {
        let o = self.critter_pose_owner(i);
        let cr = self.crit(o);
        let flat = |m: [[f32; 4]; 4]| -> Mat { std::array::from_fn(|k| m[k / 4][k % 4]) };
        let w = match cr.node {
            Some(local) => {
                let mut roots = vec![MatrixBox { matrix: flat(local), flags: 0 }];
                let mut k = cr.floor;
                while let Some(p) = k {
                    if 128 < roots.len() {
                        return unsupported("critter floor chain");
                    }
                    roots.push(MatrixBox { matrix: flat(self.collision.node_matrix(p)), flags: 0 });
                    k = self.collision.node_parent(p);
                }
                roots.push(MatrixBox { matrix: IDENTITY, flags: 0 });
                let mut zero = IDENTITY;
                zero[12..15].fill(0.0);
                roots.push(MatrixBox { matrix: zero, flags: 4 });
                node_world_chain(&cr.parents, &cr.locals, &roots, n, offset)
            }
            None => node_world(&cr.parents, &cr.locals, self.critter_root(o), n, offset),
        };
        w.map_err(|e| Unsupported(format!("critter pose: {}", e.0)))
    }

    /// CritterGetNode: tree node `idx` or the default.
    fn critter_node(&self, i: usize, idx: i16, default: Option<usize>) -> Option<usize> {
        usize::try_from(idx).ok().filter(|&k| k < self.crit(i).locals.len()).or(default)
    }

    /// CritterInitGeo 0x1935f8.
    fn critter_init_geo(&mut self, i: usize, m: Mat) -> R<()> {
        let yaw = crate::libm::atan2f(m[8], m[10]).map_err(|_| Unsupported("atan2f".into()))?;
        let pos = [m[12], m[13], m[14]];
        {
            let r = self.crm(i);
            r.set_f32(c::YAW_TARGET, yaw);
            r.set_f32(c::YAW, yaw);
            r.set_v3(c::POS, pos);
        }
        self.critter_set_yaw_matrix(i)?;
        let t = self.crit(i).ty;
        // AtreeInit: the tree root is node 0, parented to the critter node.
        self.critter_geo_nodes(i);
        let floor_off = CritterFile::f32_(self.ctype(t), 0xb0);
        let (col, _) = self.collision.floor_collide(5.0, 4.0, -1000.0, pos, false, 2)?;
        let y = match col.solid {
            Some(h) => add(h.position[1], floor_off)?,
            None => add(pos[1], floor_off)?,
        };
        self.crm(i).set_f32(c::POS + 4, y);
        // CopyMat4(instance matrix, node), UnparentMatrix under EnemyRootNode.
        self.critter_store_node(i)?;
        // CopyMat3 of the placed matrix and the home position.
        let mrow: [V3; 3] = std::array::from_fn(|r| self.cr(i).v3(c::MATRIX + 16 * r));
        for (r, row) in mrow.iter().enumerate() {
            self.crm(i).set_v3(c::HOME_MATRIX + 16 * r, *row);
        }
        let p = self.cr(i).v3(c::POS);
        self.crm(i).set_v3(c::HOME_POS, p);
        self.critter_attention(i)?;
        self.crit_mut(i).node_dc = Some(0);
        self.critter_node_matrix(i, Some(0), c::NODE_MATRIX, None)?;
        let ty = self.ctype(t).to_vec();
        let centre = if CritterFile::f32_(&ty, 0xa4) < 999.0 {
            [CritterFile::f32_(&ty, 0xa0), CritterFile::f32_(&ty, 0xa4), CritterFile::f32_(&ty, 0xa8)]
        } else {
            self.cr(i).v3(c::HOME_POS)
        };
        self.crm(i).set_v3(c::LEASH_CENTRE, centre);
        Ok(())
    }

    /// CritterInitGeo's look-at nodes: +0xd4 (TYPE +0x56, its parent with
    /// TYPE +0x5c bit 0x10), +0xd8 (+0x58) and +0xe8 (+0x5a).
    fn critter_geo_nodes(&mut self, i: usize) {
        let n56 = self.type_i16(i, 0x56);
        let d4 = self.critter_node(i, n56, None);
        let flags = self.type_u32(i, 0x5c);
        let d4 = if flags & 0x10 != 0 { d4.map(|n| usize::try_from(self.crit(i).parents[n]).ok().unwrap_or(n)) } else { d4 };
        let n58 = self.type_i16(i, 0x58);
        let n5a = self.type_i16(i, 0x5a);
        let d8 = self.critter_node(i, n58, None);
        let e8 = self.critter_node(i, n5a, None);
        let cr = self.crit_mut(i);
        cr.node_d4 = d4;
        cr.node_d8 = d8;
        cr.node_e8 = e8;
    }

    /// Replay seeding (gdl_tools' frame replay; research only): root
    /// instance `i` of TYPE `t` (CritterETypes slot `slot`) from a reference
    /// CritterInsts record whose guest pointer words the caller cleared. The
    /// native tree state follows CritterInitInst, CritterInitGeo's nodes and
    /// CritterInitColnodes; `node_dc` is the +0xdc transition node (tree
    /// index), `node`/`floor` the +0x70 node's local matrix and parent
    /// object, the clock comes from the +0x84 AnimInfo and the pose is
    /// evaluated at it (the previous update's CritterAnimate). The health
    /// meter (+0x484) and children are not seeded.
    #[allow(clippy::too_many_arguments)]
    pub fn seed_critter(&mut self, i: usize, t: TypeRef, slot: usize, rec: &[u8], node_dc: Option<usize>, node: Option<[[f32; 4]; 4]>, floor: Option<usize>) -> R<()> {
        if rec.len() != INST_SIZE || MAX_INSTS <= i {
            return unsupported("critter seed record");
        }
        if 0 <= CritterFile::i16_(self.ctype(t), 0x11c) {
            return unsupported("critter seed with children");
        }
        while self.critters.len() <= i {
            self.critters.push(None);
        }
        let id = self.critter_new_id;
        self.critter_init_inst(i, t)?;
        self.critter_new_id = id;
        self.critter_geo_nodes(i);
        self.critter_init_colnodes(i);
        let rec = Record(rec.to_vec());
        let clock = anim_info_clock(&rec);
        let shadow = (self.type_u32(i, 0x5c) & 1 != 0).then(|| (if slot < 3 { slot } else { 1 } + 1, rec.v3(c::POS)));
        let cr = self.crit_mut(i);
        cr.clock = clock;
        cr.clip = clock.sequence;
        cr.rec = rec;
        cr.node_dc = node_dc;
        cr.shadow = shadow;
        cr.node = node;
        cr.floor = floor;
        self.critter_update_pose(i)?;
        self.critter_seed_look_at(i)
    }

    /// The instance matrix +0x10..+0x4c (4 × 4; translation = +0x40).
    fn critter_mat4(&self, i: usize) -> [[f32; 4]; 4] {
        let r = self.cr(i);
        std::array::from_fn(|a| std::array::from_fn(|b| r.f32_(c::MATRIX + 0x10 * a + 4 * b)))
    }

    /// CopyMat4(+0x10, node) then UnparentMatrix(node, its parent): the
    /// floor object's chain (collision::unparent_chain) or EnemyRootNode.
    pub(crate) fn critter_store_node(&mut self, i: usize) -> R<()> {
        let m = self.critter_mat4(i);
        let local = match self.crit(i).floor {
            Some(o) => self.collision.unparent_chain(m, Some(o)).map_err(|e| Unsupported(e.0.to_string()))?,
            None => super::doplayers::unparent_matrix(m, None)?,
        };
        self.crit_mut(i).node = Some(local);
        Ok(())
    }

    /// ProcessCritter's GetWorldMat(+0x70 node, +0x10): the node's local
    /// matrix through its parent chain into the instance matrix.
    pub(crate) fn critter_load_node(&mut self, i: usize) -> R<()> {
        let Some(local) = self.crit(i).node else { return Ok(()) };
        let floor = self.crit(i).floor;
        let w = self.collision.chain_world(local, floor).map_err(|e| Unsupported(e.0.to_string()))?;
        let r = self.crm(i);
        for (a, row) in w.iter().enumerate() {
            for (b, v) in row.iter().enumerate() {
                r.set_f32(c::MATRIX + 0x10 * a + 4 * b, *v);
            }
        }
        Ok(())
    }

    /// CopyMat3(identity) + YawMat3(+0x108) into the instance matrix.
    fn critter_set_yaw_matrix(&mut self, i: usize) -> R<()> {
        let mut m: [V3; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let yaw = self.cr(i).f32_(c::YAW);
        crate::collision::yaw_mat3(&mut m, yaw).map_err(|e| Unsupported(format!("YawMat3: {e:?}")))?;
        for (r, row) in m.iter().enumerate() {
            self.crm(i).set_v3(c::MATRIX + 16 * r, *row);
        }
        Ok(())
    }

    /// +0x50 attention point and +0x60 collision centre (ProcessCritter /
    /// CritterInitGeo): MulVec4Mat3(TYPE +0xc0, matrix) + position.
    fn critter_attention(&mut self, i: usize) -> R<()> {
        let t = self.crit(i).ty;
        let ty = self.ctype(t).to_vec();
        let p = self.cr(i).v3(c::POS);
        let att = add(p[1], CritterFile::f32_(&ty, 0xb4))?;
        let off = [CritterFile::f32_(&ty, 0xc0), CritterFile::f32_(&ty, 0xc4), CritterFile::f32_(&ty, 0xc8)];
        let m: [V3; 3] = std::array::from_fn(|r| self.cr(i).v3(c::MATRIX + 16 * r));
        let v = mul_vec_mat3(off, &m)?;
        let r = self.crm(i);
        r.set_v3(c::ATTN, [p[0], att, p[2]]);
        r.set_v3(c::COL, [add(p[0], v[0])?, add(p[1], v[1])?, add(p[2], v[2])?]);
        Ok(())
    }

    /// The instance matrix as a tree root (rows from +0x10, translation +0x40).
    fn critter_root(&self, i: usize) -> Mat {
        let r = self.cr(i);
        let mut m = IDENTITY;
        for row in 0..3 {
            m[4 * row..4 * row + 3].copy_from_slice(&r.v3(c::MATRIX + 16 * row));
        }
        m[12..15].copy_from_slice(&r.v3(c::POS));
        m
    }

    /// GetWorldMat(node, out, offset) into the instance record at `at`
    /// (16 floats); `None` node copies the instance matrix.
    fn critter_node_matrix(&mut self, i: usize, node: Option<usize>, at: usize, offset: Option<V3>) -> R<Mat> {
        let m = match node {
            Some(n) => self.critter_node_world(i, n, offset)?,
            None => self.critter_root(i),
        };
        for (k, v) in m.iter().enumerate() {
            self.crm(i).set_f32(at + 4 * k, *v);
        }
        Ok(m)
    }

    /// CritterInitColnodes 0x192ab0 (without debug DmgFx).
    fn critter_init_colnodes(&mut self, i: usize) {
        let t = self.crit(i).ty;
        let (n, first) = (self.type_i16(i, 0x118), self.type_i16(i, 0x11a));
        if n <= 0 {
            return;
        }
        self.crm(i).set_i16(c::COLNODE_HIT, -1);
        let health = self.cr(i).f32_(c::HEALTH);
        for k in 0..n as usize {
            let desc = self.critter_data.files[t.0].wad.nodes[first as usize + k].clone();
            let node = self.critter_node(i, CritterFile::i16_(&desc, 0x14), None);
            let mut node2 = node;
            if let Some(nd) = node {
                match desc[0x30] {
                    0 => {}
                    b'-' => {
                        for _ in 0..(desc[0x31] as i32 - 0x30).max(0) {
                            node2 = node2.and_then(|x| usize::try_from(self.crit(i).parents[x]).ok());
                        }
                    }
                    b'+' => {
                        // +0x78: first child node (presentation tree link).
                        for _ in 0..(desc[0x31] as i32 - 0x30).max(0) {
                            node2 = node2.and_then(|x| self.crit(i).parents.iter().position(|&p| p == x as i32));
                        }
                    }
                    _ => {
                        let idx = self.critter_node_idx(t, 0x30, Some(&label(&desc, 0x30, 16)));
                        node2 = self.critter_node(i, idx, Some(nd));
                    }
                }
            }
            self.crit_mut(i).colnode_nodes[k] = (node, node2);
            let at = c::COLNODES + 0x60 * k;
            let hp = mul(CritterFile::f32_(&desc, 0x44), health).unwrap_or(0.0);
            let r = self.crm(i);
            r.set_i32(at + 0x54, -1);
            r.set_f32(at + 0x5c, 0.0);
            r.set_f32(at + 0x58, hp);
        }
    }

    /// CritterUpdateColnodes 0x192cd8: colnode world matrices at +0x10.
    fn critter_update_colnodes(&mut self, i: usize) -> R<()> {
        let t = self.crit(i).ty;
        let first = self.type_i16(i, 0x11a) as usize;
        for k in 0..self.crit(i).colnode_nodes.len() {
            let at = c::COLNODES + 0x60 * k + 0x10;
            let (node, _) = self.crit(i).colnode_nodes[k];
            let desc = &self.critter_data.files[t.0].wad.nodes[first + k];
            let off = [CritterFile::f32_(desc, 0x20), CritterFile::f32_(desc, 0x24), CritterFile::f32_(desc, 0x28)];
            match node {
                Some(n) => {
                    self.critter_node_matrix(i, Some(n), at, Some(off))?;
                }
                None => {
                    let root = self.critter_root(i);
                    for (j, v) in root.iter().enumerate() {
                        self.crm(i).set_f32(at + 4 * j, *v);
                    }
                }
            }
        }
        Ok(())
    }
}

/// MulVec4Mat3 / WorldVector: v × rows 0–2 (translation ignored).
pub fn mul_vec_mat3(v: V3, m: &[V3; 3]) -> R<V3> {
    let mut out = [0.0; 3];
    for (col, o) in out.iter_mut().enumerate() {
        *o = add(add(mul(v[0], m[0][col])?, mul(v[1], m[1][col])?)?, mul(v[2], m[2][col])?)?;
    }
    Ok(out)
}

