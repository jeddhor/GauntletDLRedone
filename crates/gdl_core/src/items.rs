//! Item records transcribed from NewItemPtr 0x152e70 / NewItem 0x152f88 /
//! SetItem 0x151878 / AddItemWobj 0x151390 / PlaceItem 0x153068.
//!
//! A record keeps the original 0x110-byte layout so later item functions can
//! be ported field for field. Guest pointers become indices: +0x00 definition
//! index, +0xf0 of types 5/12 world-object index + 1 (0 = none). Presentation
//! (SetItemGeo, anim trees +0x80.., render node +0x70, z-sort, MB parenting)
//! is outside the simulation and those bytes stay zero here.
use crate::collision::{CollisionWorld, UnsupportedCollision};
use crate::libm::{atan2f, cosf, sinf};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sub_guarded_rz as sub, UnsupportedNumericDomain};
use crate::player_collision::collision_position;
use crate::rng::Rand;
use gdl_formats::elf::Elf;
use gdl_formats::world::{ItemInstance, World};

pub const RECORD: usize = 0x110;
pub const DEFINITION: usize = 0x50;

#[derive(Debug)]
pub struct UnsupportedItem(pub String);
impl std::fmt::Display for UnsupportedItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unsupported item path: {}", self.0)
    }
}
impl std::error::Error for UnsupportedItem {}
impl From<UnsupportedNumericDomain> for UnsupportedItem {
    fn from(e: UnsupportedNumericDomain) -> Self {
        Self(format!("numeric domain: {e:?}"))
    }
}
impl From<UnsupportedCollision> for UnsupportedItem {
    fn from(e: UnsupportedCollision) -> Self {
        Self(format!("collision: {e}"))
    }
}
type R<T> = Result<T, UnsupportedItem>;
fn err<T>(s: impl Into<String>) -> R<T> {
    Err(UnsupportedItem(s.into()))
}

macro_rules! accessors {
    ($($get:ident, $set:ident, $t:ty;)*) => {$(
        pub fn $get(&self, o: usize) -> $t {
            <$t>::from_le_bytes(self.0[o..o + std::mem::size_of::<$t>()].try_into().unwrap())
        }
        pub fn $set(&mut self, o: usize, v: $t) {
            self.0[o..o + std::mem::size_of::<$t>()].copy_from_slice(&v.to_le_bytes());
        }
    )*};
}

/// One item in the original layout.
#[derive(Clone, PartialEq, Eq)]
pub struct ItemRecord(pub [u8; RECORD]);
impl ItemRecord {
    accessors! {
        u8_, set_u8, u8; i8_, set_i8, i8; u16_, set_u16, u16; i16_, set_i16, i16;
        u32_, set_u32, u32; i32_, set_i32, i32; f32_, set_f32, f32;
    }
    pub fn definition(&self) -> usize {
        self.u32_(0) as usize
    }
    pub fn position(&self) -> [f32; 3] {
        [self.f32_(0x40), self.f32_(0x44), self.f32_(0x48)]
    }
    pub fn v3(&self, o: usize) -> [f32; 3] {
        [self.f32_(o), self.f32_(o + 4), self.f32_(o + 8)]
    }
    pub fn or_u16(&mut self, o: usize, v: u16) {
        self.set_u16(o, self.u16_(o) | v);
    }
    pub fn and_u16(&mut self, o: usize, v: u16) {
        self.set_u16(o, self.u16_(o) & v);
    }
    pub fn matrix(&self) -> [[f32; 3]; 3] {
        std::array::from_fn(|r| std::array::from_fn(|c| self.f32_(0x10 + 16 * r + 4 * c)))
    }
    /// +0xd8 == -1 marks a free or disabled slot (NewItemPtr).
    pub fn active(&self) -> bool {
        self.i16_(0xd8) != -1
    }
}

/// Mutable 80-byte item definition (SetItem writes +0x28 and +0x48).
#[derive(Clone)]
pub struct Definition(pub [u8; DEFINITION]);
impl Definition {
    accessors! { i16_, set_i16, i16; i32_, set_i32, i32; f32_, set_f32, f32; }
    pub fn kind(&self) -> i32 {
        self.i32_(0)
    }
    pub fn subtype(&self) -> i32 {
        self.i32_(4)
    }
    pub fn name(&self) -> &[u8] {
        let n = &self.0[0x28..0x3c];
        &n[..n.iter().position(|&b| b == 0).unwrap_or(n.len())]
    }
}

/// item_wobjs/item_wobj_{inity,y,starty,endy,snd} (EE 0x23c8f0, 0x2c1b48, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct ItemWobj {
    pub object: usize,
    pub inity: f32,
    pub y: f32,
    pub starty: f32,
    pub endy: f32,
    pub snd: f32,
}

/// World-object words items modify (+0x10 flags, +0x14 trigger, +0x16/+0x17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectWords {
    pub flags_10: u32,
    pub word_14: u16,
    pub byte_16: u8,
    pub byte_17: u8,
}

/// Constant data SetItem reads from the executable.
#[derive(Clone)]
pub struct ItemTables {
    /// enemy_table EE 0x22c7a0: 44 × 36 bytes, type at +0, name at +4.
    pub enemy_table: Vec<(i32, Vec<u8>)>,
    pub enemy_logic_num: Vec<i32>,
    pub gen_maxenemies: [i32; 3],
    pub gen_mininterval: [i32; 3],
    pub crystal_order: Vec<i32>,
    /// worlddatas EE 0x23deb0: 14 × 44 bytes, number at +0, letter at +0xf.
    pub worlds: Vec<(i32, u8)>,
    /// 4-byte generator names written by the temple/hell overrides (0x2a81d0/8).
    pub generator_name_5: [u8; 4],
    pub generator_name_6: [u8; 4],
    /// rand_rot.216 EE 0x234380 (static local, 8 floats): rock spin rates.
    pub rand_rot_216: [f32; 8],
    /// MAX_CRYSTALS 0x23f420 (9) and max_garg_items 0x23f598 (3).
    pub max_crystals: Vec<i32>,
    pub max_garg_items: Vec<i32>,
}

fn words(bytes: &[u8]) -> Vec<i32> {
    bytes.chunks_exact(4).map(|c| i32::from_le_bytes(c.try_into().unwrap())).collect()
}
fn cstr(bytes: &[u8]) -> Vec<u8> {
    bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())].to_vec()
}

type DynResult<T> = Result<T, Box<dyn std::error::Error>>;

impl ItemTables {
    pub fn from_elf(elf: &Elf) -> DynResult<Self> {
        let table = elf.object("enemy_table")?;
        if table.len() != 44 * 36 {
            return Err("enemy_table size".into());
        }
        let three = |name: &str| -> DynResult<[i32; 3]> {
            let w = words(elf.object(name)?);
            Ok(w.try_into().map_err(|_| format!("{name} size"))?)
        };
        let worlds = elf.object("worlddatas")?;
        if worlds.len() != 14 * 44 {
            return Err("worlddatas size".into());
        }
        let name4 = |a: u32| -> DynResult<[u8; 4]> { Ok(elf.read(a, 4)?.try_into()?) };
        Ok(Self {
            enemy_table: table
                .chunks_exact(36)
                .map(|e| (i32::from_le_bytes(e[0..4].try_into().unwrap()), cstr(&e[4..20])))
                .collect(),
            enemy_logic_num: words(elf.object("enemy_logic_num")?),
            gen_maxenemies: three("gen_maxenemies")?,
            gen_mininterval: three("gen_mininterval")?,
            crystal_order: words(elf.object("crystal_order")?),
            worlds: worlds
                .chunks_exact(44)
                .map(|w| (i32::from_le_bytes(w[0..4].try_into().unwrap()), w[0xf]))
                .collect(),
            generator_name_5: name4(0x2a_81d0)?,
            generator_name_6: name4(0x2a_81d8)?,
            rand_rot_216: {
                let b = elf.read(0x23_4380, 32)?;
                std::array::from_fn(|k| f32::from_le_bytes(b[4 * k..4 * k + 4].try_into().unwrap()))
            },
            max_crystals: words(elf.object("MAX_CRYSTALS")?),
            max_garg_items: words(elf.object("max_garg_items")?),
        })
    }
}

/// Level/session state SetItem reads.
#[derive(Clone, Debug)]
pub struct ItemSession {
    /// LEVL +0xcc..+0xdc.
    pub multipliers: [f32; 5],
    /// EnemyType[8] EE 0x23e1b0 and the generic table at 0x23e1d0.
    pub enemy_kind: [i32; 8],
    pub gen_kind: [i32; 8],
    pub boss_type: i32,
    /// worlddatas number (1 castle … 7 town … 0xd tower).
    pub cur_world: i32,
    pub num_virtual_players: i32,
    pub game_mode: u32,
    /// Globals EE 0x20f958 / 0x20f970.
    pub global_958: i32,
    pub global_970: i32,
    /// RandomItemNum EE 0x23c76c (vb_counter at load, supplied by the caller).
    pub random_item_num: u32,
    /// Load-time presence of statue animations/critters (enemy_anim +116/+120,
    /// CritterTypeLoaded(3) / (7)) consulted by type-4 statue generators.
    pub golem_statue_anim: bool,
    pub death_statue_anim: bool,
    pub critter_3_loaded: bool,
    pub critter_7_loaded: bool,
    /// SetItem's Tower crystal test, evaluated by the caller from the
    /// players at load: DemoMode != 0 || AnyPlayerHasAllCrystals(1).
    pub tower_crystals_taken: bool,
    /// PlayerWaves(player, 8) & 1 over the present players at load (the
    /// Tower's 'h'/199 triggers start opened).
    pub battle_wave_1_done: bool,
    /// TowerInit's TowerNewPlayer (SetItem hides the Tower crystals).
    pub tower_new_player: bool,
    /// AudioFindSound(name, 0x10, 1): the audio data's sound names (upper
    /// case) with their IDs, for type-13 items; empty without audio data.
    pub sound_names: Vec<(Vec<u8>, i32)>,
}

pub struct ItemWorld {
    pub tables: ItemTables,
    pub session: ItemSession,
    pub definitions: Vec<Definition>,
    pub records: Vec<ItemRecord>,
    pub first_empty: usize,
    pub wobjs: Vec<ItemWobj>,
    pub objects: Vec<ObjectWords>,
    /// Loader parent (+0x18) and position (+0x1c) of each world object.
    pub object_parent: Vec<Option<usize>>,
    pub object_position: Vec<[f32; 3]>,
    /// WORLDS node child (+0x2e) and next-sibling (+0x2c) links.
    pub object_child: Vec<Option<usize>>,
    pub object_next: Vec<Option<usize>>,
    /// Render-node translation y of each object (item_wobj_inity + y once moved).
    pub object_node_y: Vec<f32>,
    /// Runtime +0x35 exclusion byte (ProcessItemWobjs rewrites it for wobjs).
    pub object_exclusion: Vec<i8>,
    /// MaxItems = instances + 500 (AddItemInstList).
    pub max_items: usize,
    pub world_has_gates: i32,
    /// Objects whose world animation SetItem put at its last frame.
    pub wanim_to_end: Vec<usize>,
    /// Item slots whose tree SetItem set to alpha 0xff (MBTreeSetAlpha).
    pub alpha_full: Vec<usize>,
    pub rune_item: Option<usize>,
    pub rune_pos: [f32; 3],
    pub quest_item: Option<usize>,
    pub rng: Rand,
    /// PlaceItem's MBNodeSetParent onto a moving (flag 0x1000) floor object:
    /// per slot the object and the node matrix relative to it
    /// (UpdateObjWorldMat); ProcessItems' GetObjWorldMat rebuilds the record
    /// matrix from them every update.
    pub floor_parent: Vec<Option<(usize, [[f32; 4]; 4])>>,
    /// Kind 13's +0xf8, SetItem's FindWorldAnimNode(10.0, position): the
    /// world-animated object whose node the item follows (ProcessItems case
    /// 0xd copies its GetWorldMat into the record matrix).
    pub sound_node: Vec<Option<usize>>,
    /// The render node (+0x70) matrix of a sound item without a floor
    /// parent (SetItem's, then PlaceItem's UpdateObjWorldMat): case 0xd moves
    /// only the record, so GetObjWorldMat keeps rebuilding the record (and
    /// +0x50/+0x60) from this anchor.
    pub sound_anchor: Vec<Option<[[f32; 4]; 4]>>,
    /// SetItem's SetItemGeo decision per slot (the item gets its definition's
    /// tree, +0x4c): not for kind 4 (placed enemies; statues take enemy or
    /// critter trees), 13, 12 other than subtype 2, nor an instance with flag
    /// byte +3 bit 2 (fixture: the Tower's BRIDGEPAD triggers have no atree).
    pub geo: Vec<bool>,
}

/// WPitchMat3/WYawMat3/WRollMat3 0x1d5228/0x1d5178/0x1d52d8 on rows 0..2.
fn rotate(m: &mut [[f32; 3]; 3], angle: f32, a: usize, b: usize) -> R<()> {
    if angle.abs() < f32::from_bits(0x38d1_b717) {
        return Ok(());
    }
    let (s, c) = (sinf(angle)?, cosf(angle)?);
    for row in m.iter_mut() {
        let (x, y) = (row[a], row[b]);
        let na = sub(mul(c, x)?, mul(s, y)?)?;
        let nb = add(mul(c, y)?, mul(s, x)?)?;
        row[b] = nb;
        row[a] = na;
    }
    Ok(())
}

/// WPitchMat3 0x1d5228: each row's (y, z) pair rotated by `angle`.
pub fn pitch_mat3(m: &mut [[f32; 3]; 3], angle: f32) -> R<()> {
    rotate(m, angle, 1, 2)
}

/// NewItem's matrix: identity, then pitch (rows' y/z), yaw (x/z), roll (x/y).
pub fn item_matrix(rotation: [f32; 3]) -> R<[[f32; 3]; 3]> {
    let mut m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    rotate(&mut m, rotation[0], 1, 2)?;
    rotate(&mut m, rotation[1], 0, 2)?;
    rotate(&mut m, rotation[2], 0, 1)?;
    Ok(m)
}

/// float→int conversion of the original (truncating CVT.W.S).
fn cvt(v: f32) -> i32 {
    v as i32
}

const TENTH: u32 = 0x3dcc_cccd;

impl ItemWorld {
    pub fn new(world: &World, collision: &CollisionWorld, tables: ItemTables, session: ItemSession) -> R<Self> {
        let definitions = world
            .item_definitions
            .iter()
            .map(|d| Ok(Definition(d.raw.as_slice().try_into().map_err(|_| UnsupportedItem("definition size".into()))?)))
            .collect::<R<Vec<_>>>()?;
        let objects = world
            .nodes
            .iter()
            .map(|n| ObjectWords {
                flags_10: crate::collision::new_world_object_flags(&n.raw),
                word_14: u16::from_le_bytes([n.raw[0x14], n.raw[0x15]]),
                byte_16: n.raw[0x16],
                byte_17: n.raw[0x17],
            })
            .collect();
        Ok(Self {
            tables,
            session,
            definitions,
            records: Vec::new(),
            first_empty: 0,
            wobjs: Vec::new(),
            objects,
            object_parent: collision.objects.iter().map(|o| o.parent).collect(),
            object_position: collision.objects.iter().map(|o| o.position).collect(),
            object_child: world.nodes.iter().map(|n| usize::try_from(n.child).ok()).collect(),
            object_next: world.nodes.iter().map(|n| usize::try_from(n.next).ok()).collect(),
            max_items: world.items.len() + 500,
            wanim_to_end: Vec::new(),
            alpha_full: Vec::new(),
            object_node_y: collision.objects.iter().map(|o| o.position[1]).collect(),
            object_exclusion: collision.objects.iter().map(|o| o.exclusion).collect(),
            world_has_gates: 0,
            rune_item: None,
            rune_pos: [0.0; 3],
            quest_item: None,
            rng: Rand::new(0),
            floor_parent: Vec::new(),
            sound_node: Vec::new(),
            sound_anchor: Vec::new(),
            geo: Vec::new(),
        })
    }

    /// NewItemPtr: first slot with +0xd8 == -1 or a new one; keeps +0xe6.
    pub fn new_item_ptr(&mut self, max_items: usize) -> R<usize> {
        let mut i = self.first_empty;
        while i < self.records.len() && self.records[i].active() {
            i += 1;
        }
        if i >= max_items {
            return err("MaxItems exceeded (FatalErrorC)");
        }
        if i == self.records.len() {
            self.records.push(ItemRecord([0; RECORD]));
        }
        self.first_empty = i + 1;
        let keep = self.records[i].u16_(0xe6);
        self.records[i] = ItemRecord([0; RECORD]);
        self.records[i].set_u16(0xd4, 0xffff);
        self.records[i].set_u16(0xe6, keep);
        Ok(i)
    }

    /// NewItem for a placed instance; returns the slot.
    pub fn new_item(&mut self, instance: &ItemInstance, max_items: usize) -> R<usize> {
        let slot = self.new_item_ptr(max_items)?;
        if (instance.definition as i16) < 0 {
            return err("negative instance definition (FatalErrorC)");
        }
        let m = item_matrix(instance.rotation)?;
        self.set_item(slot, Some(&instance.raw), instance.definition as usize, m, instance.position)?;
        Ok(slot)
    }

    pub fn enemy_desc_type(&self, name: &[u8]) -> i32 {
        if name.eq_ignore_ascii_case(b"BOSSGEN") {
            let t = self.session.enemy_kind[0];
            return if t != self.session.boss_type { t } else { -1 };
        }
        self.tables.enemy_table.iter().find(|(_, n)| n.eq_ignore_ascii_case(name)).map_or(-1, |(t, _)| *t)
    }

    /// GetEnemyType 0x138050 (an unresolved -1 is reported by the original and returned).
    pub fn get_enemy_type(&self, kind: i32, level: i8) -> i32 {
        let g = &self.session.gen_kind;
        let mut s = if kind == 3 { g[1] } else { kind };
        if (kind - 4) as u32 <= 1 {
            if level >= 4 && g[4] >= 0 {
                s = g[4];
            } else if g[2] >= 0 {
                s = g[2];
            } else if g[3] >= 0 {
                s = g[3];
            }
        }
        s
    }

    fn logic_num(&self, t: i16) -> R<u8> {
        usize::try_from(t)
            .ok()
            .and_then(|t| self.tables.enemy_logic_num.get(t))
            .map(|&v| v as u8)
            .ok_or_else(|| UnsupportedItem(format!("enemy_logic_num[{t}] outside the table")))
    }

    /// show_item 0x151620.
    fn show_item(&self, record: &ItemRecord) -> bool {
        let mut need = i32::from(record.i8_(0xe0));
        let mut players = if self.session.game_mode & 0x8000 != 0 { 2 } else { self.session.num_virtual_players };
        let exact = 10 < need;
        if exact {
            need -= 10;
        }
        if 0 < self.session.global_958 {
            players = self.session.global_958;
        }
        if self.session.global_958 <= 0 && !exact {
            return need <= players;
        }
        players == need
    }

    /// FindInfoIndex 0x1537d0 (strcmp on the name, then kind/subtype).
    pub fn find_info_index(&self, kind: i32, subtype: i32, name: &[u8]) -> Option<usize> {
        self.definitions
            .iter()
            .position(|d| d.name() == name && d.kind() == kind && (subtype == 0 || subtype == d.subtype()))
    }

    /// RandItemIdx 0x14fa10 with advance = 1.
    pub(crate) fn rand_item_idx(&mut self, index: usize, count: u32) -> u32 {
        let r = if count == 0 { 0 } else { ((self.session.random_item_num >> 5).wrapping_add(index as u32)) % count };
        self.session.random_item_num = self.session.random_item_num.wrapping_add(0x1b7);
        r
    }

    /// FindWave 0x162620: world letter + level digit.
    fn find_wave(&self, s: &[u8]) -> i32 {
        let letter = s[0].to_ascii_uppercase() as i8;
        match self.tables.worlds.iter().find(|(_, l)| *l as i8 == letter) {
            Some(&(n, _)) if n >= 0 => (n << 8) | ((i32::from(s[1] as i8) - 0x31) & 0xff),
            _ => -1,
        }
    }

    /// AddItemWobj 0x151390. Returns the wobj slot or -1.
    fn add_item_wobj(&mut self, object: usize, trigger: i16, start: i16, end: i16, snd: u8) -> R<i32> {
        let kind = (trigger as i32 as u32) & 0xff;
        let start = mul(f32::from(start), f32::from_bits(TENTH))?;
        if let Some(i) = self.wobjs.iter().position(|w| w.object == object) {
            let o = &mut self.objects[object];
            let current = u32::from(o.word_14 & 0xff);
            if current != kind && current.wrapping_sub(0x1b) < 3 && kind.wrapping_sub(0x1b) < 3 {
                o.word_14 = (o.word_14 & 0xff00) | 0x1b;
            } // other conflicts: Errorf only
            let w = &mut self.wobjs[i];
            if w.starty == 0.0 {
                w.starty = start;
                w.y = start;
            }
            if w.endy == 0.0 {
                w.endy = mul(f32::from(end), f32::from_bits(TENTH))?;
            }
            if 0.0 < w.snd {
                return Ok(-1);
            }
            w.snd = f32::from(snd as i8);
            return Ok(-1);
        }
        if self.wobjs.len() + 1 > 0x95 {
            return err("too many item wobjs (FatalErrorC)");
        }
        self.wobjs.push(ItemWobj {
            object,
            // MB node +0x34 (translation y) of the object; equal to the
            // loader position for the verified level (see the comparator).
            inity: self.object_position[object][1],
            y: start,
            starty: start,
            endy: mul(f32::from(end), f32::from_bits(TENTH))?,
            snd: f32::from(snd as i8),
        });
        self.objects[object].word_14 = trigger as u16;
        Ok(self.wobjs.len() as i32 - 1)
    }

    /// SetItem. `instance` is the 60-byte placed record (None = param_2 NULL).
    pub fn set_item(
        &mut self,
        slot: usize,
        instance: Option<&[u8]>,
        definition: usize,
        matrix: [[f32; 3]; 3],
        position: [f32; 3],
    ) -> R<()> {
        let inst = instance;
        let iu8 = |o: usize| inst.map_or(0, |i| i[o]);
        let iu16 = |o: usize| inst.map_or(0, |i| u16::from_le_bytes([i[o], i[o + 1]]));
        let ii16 = |o: usize| iu16(o) as i16;
        let ii32 = |o: usize| inst.map_or(0, |i| i32::from_le_bytes(i[o..o + 4].try_into().unwrap()));
        let mut def = definition;
        if def >= self.definitions.len() {
            return err("definition index");
        }
        if self.definitions[def].kind() == -1 {
            loop {
                let count = self.definitions[def].i32_(4) as u32;
                let i = self.rand_item_idx(slot, count) as usize;
                let pick = self.definitions[def].i16_(8 + 2 * i);
                def = usize::try_from(pick)
                    .ok()
                    .filter(|&d| d < self.definitions.len())
                    .ok_or_else(|| UnsupportedItem("random pick".into()))?;
                if self.definitions[def].kind() != -1 {
                    break;
                }
            }
        }
        let (mut kind, mut subtype) = (self.definitions[def].kind(), self.definitions[def].subtype());
        if kind == 1 {
            match subtype {
                2 if inst.is_some() && ii16(0x30) > 1 => {
                    def = self
                        .find_info_index(1, subtype, b"KEYRING")
                        .ok_or_else(|| UnsupportedItem("KEYRING definition missing".into()))?;
                    kind = self.definitions[def].kind();
                    subtype = self.definitions[def].subtype();
                }
                0xc => {
                    // Errorf, then disabled: only +0xd8 is written.
                    self.records[slot].set_u16(0xd8, 0xffff);
                    return Ok(());
                }
                0xf if self.session.cur_world == 0xd && self.session.tower_crystals_taken => {
                    // Disabled: only +0xd8 is written.
                    self.records[slot].set_u16(0xd8, 0xffff);
                    return Ok(());
                }
                _ => {}
            }
        }
        let mut r = self.records[slot].clone();
        r.set_u32(0, def as u32);
        let d = self.definitions[def].clone();
        if self.geo.len() <= slot {
            self.geo.resize(slot + 1, false);
        }
        self.geo[slot] = !(matches!(kind, 4 | 13) || kind == 12 && subtype != 2) && iu8(3) & 2 == 0;
        r.set_f32(0xc8, d.f32_(0x1c));
        r.set_f32(0xcc, add(d.f32_(0x20), 1.0)?);
        r.set_f32(0xd0, d.f32_(0x24));
        let (s0c, s10) = (d.f32_(0x0c), d.f32_(0x10));
        r.set_f32(0xe8, if s10 < s0c { add(s0c, s0c)? } else { add(s10, s10)? });
        r.set_u32(0x74, 0);
        if add(r.f32_(0xc8).abs(), r.f32_(0xd0).abs())? < f32::from_bits(0x3c23_d70a) {
            r.set_u32(0x74, 2);
        }
        r.set_u16(0xd4, if inst.is_some() { iu16(4) } else { 0xffff });
        r.set_u16(0xd6, iu16(6));
        r.set_i16(0xd8, d.i16_(0x46));
        r.set_u16(0xda, 0);
        r.set_u8(0xdc, 0);
        r.set_u8(0xdd, 0);
        r.set_u8(0xde, 0);
        r.set_u8(0xe0, iu8(2));
        if iu8(3) & 1 != 0 {
            r.set_u16(0xd8, r.u16_(0xd8) | 0x40);
        }
        r.set_u8(0xe2, 0);
        r.set_u8(0xdf, 0xff);
        r.set_u8(0xe1, u8::from(!self.show_item(&r)));
        r.set_u8(0xe3, d.0[0x42]);
        r.set_i16(0xe4, d.i16_(0x44));
        let facing = || atan2f(matrix[2][0], matrix[2][2]);
        let m = self.session.multipliers;
        match kind {
            1 => {
                r.set_i32(0xf0, d.i32_(0x3c));
                r.set_i32(0xf4, i32::from(d.i16_(0x40)));
                r.set_f32(0xf8, d.i16_(0x4a) as f32);
                r.set_u32(0xfc, 0);
                r.set_u16(0x100, 0);
                match subtype {
                    2 => {
                        if inst.is_some() {
                            r.set_i32(0xf4, i32::from(ii16(0x30)));
                        }
                        if r.i32_(0xf4) < 1 {
                            r.set_i32(0xf4, 1);
                        }
                    }
                    10 => {
                        self.rune_pos = position;
                        self.rune_item = Some(slot);
                    }
                    0xd => self.quest_item = Some(slot),
                    0xe if inst.is_some() => r.set_i32(0xf4, i32::from(ii16(0x30))),
                    0xf => {
                        let i = r.i32_(0xf4);
                        let v = usize::try_from(i).ok().and_then(|i| self.tables.crystal_order.get(i));
                        r.set_i32(0xf4, *v.ok_or_else(|| UnsupportedItem(format!("crystal_order[{i}]")))?);
                    }
                    _ => {}
                }
            }
            2 => {
                r.set_u16(0xf0, if inst.is_some() { iu16(0x30) } else { 0xffff });
                r.set_u16(0xf2, 0);
                r.set_u32(0xf8, 0);
                r.set_u32(0xfc, 0);
                r.set_f32(0xf4, facing()?);
                r.set_u16(0x100, iu16(0x34));
            }
            3 => {
                r.set_f32(0xe8, mul(r.f32_(0xe8), 4.0)?);
                r.set_u8(0xf2, 0);
                r.set_u8(0xf3, iu8(0x34));
                r.set_u8(0xfb, iu8(0x36));
                r.set_u8(0xf4, 0);
                r.set_u8(0xf7, iu8(0x32));
                let t = self.enemy_desc_type(d.name());
                r.set_i16(0xf0, t as i16);
                // Only the record's definition changes; this call keeps
                // reading param_3 (`d`).
                if let Some(low) = (t as i16 == 3).then(|| self.find_info_index(kind, subtype, b"LOW")).flatten() {
                    r.set_u32(0, low as u32);
                }
                if self.session.global_970 != 0 || r.i8_(0xf7) < 0 {
                    r.set_u8(0xf7, self.logic_num(r.i16_(0xf0))?);
                }
                r.set_u8(0xf6, if inst.is_some() { iu8(0x30) } else { 1 });
                if r.i8_(0xf6) < 1 {
                    r.set_u8(0xf6, 1);
                }
                r.set_u8(0xf5, 0xff);
                r.set_u32(0xfc, 0);
                r.set_u16(0xf8, 0);
                r.set_u8(0xfa, 0);
                r.set_f32(0x100, facing()?);
                r.set_u16(0xda, 0x28);
                let level = (i32::from(r.i8_(0xf6)) - 1).clamp(0, 2) as usize;
                r.set_i16(0xe4, i16::from(r.i8_(0xf6)).wrapping_mul(r.i16_(0xe4)));
                if r.u8_(0xf3) == 0 {
                    r.set_u8(0xf3, self.tables.gen_maxenemies[level] as u8);
                }
                if r.u8_(0xfb) == 0 {
                    r.set_u8(0xfb, self.tables.gen_mininterval[level] as u8);
                }
                r.set_i8(0xf3, cvt(mul(f32::from(r.i8_(0xf3)), m[2])?) as i8);
                r.set_u8(0xfb, cvt(mul(f32::from(r.u8_(0xfb)), m[1])?) as u8);
                r.set_i16(0xe4, cvt(mul(f32::from(r.i16_(0xe4)), m[0])?) as i16);
                let world_override = match self.session.cur_world {
                    5 => Some((self.tables.generator_name_5, -2i16, 2u8)),
                    6 => Some((self.tables.generator_name_6, -3, 3)),
                    _ => None,
                };
                if let Some((name, t, level)) = world_override {
                    let target = r.definition();
                    self.definitions[target].0[0x28..0x2c].copy_from_slice(&name);
                    r.set_i16(0xf0, t);
                    r.set_u8(0xf6, level);
                }
                r.set_i16(0xf0, self.get_enemy_type(i32::from(r.i16_(0xf0)), r.i8_(0xf2)) as i16);
            }
            4 => {
                r.set_u8(0xf2, if inst.is_some() { iu8(0x30) } else { 1 });
                r.set_u8(0xf3, if inst.is_some() { iu8(0x32) } else { 0xff });
                r.set_i16(0xf0, self.enemy_desc_type(d.name()) as i16);
                r.set_f32(0xf4, facing()?);
                r.set_u32(0xf8, 0);
                r.set_i32(0xfc, ii32(0x34));
                r.set_u16(0x100, iu16(0x38));
                if self.session.global_970 != 0 || r.i8_(0xf3) < 0 {
                    r.set_u8(0xf3, self.logic_num(r.i16_(0xf0))?);
                }
                let t = self.get_enemy_type(i32::from(r.i16_(0xf0)), r.i8_(0xf2)) as i16;
                r.set_i16(0xf0, t);
                // Statues: model selection is presentation; the record effects are
                // +0xd8 bit 0 cleared, +0xf2 level and +0xf8 bit 0.
                let s = &self.session;
                let mut statue_level = None;
                match t {
                    0x20 => {
                        if s.critter_7_loaded {
                            r.set_u16(0xd8, r.u16_(0xd8) & 0xfffe);
                        }
                        statue_level = Some(1);
                    }
                    0x1d if s.golem_statue_anim => {
                        r.set_u16(0xd8, r.u16_(0xd8) & 0xfffe);
                        statue_level = Some(1);
                    }
                    0x1d => {
                        if s.critter_3_loaded {
                            r.set_u16(0xd8, r.u16_(0xd8) & 0xfffe);
                        }
                        statue_level = Some(1);
                    }
                    0x1e if s.death_statue_anim => {
                        r.set_u16(0xd8, r.u16_(0xd8) & 0xfffe);
                        match r.i8_(0xf2) {
                            0 => statue_level = Some(1),
                            1 => {
                                r.set_u32(0xf8, r.u32_(0xf8) | 1);
                                statue_level = Some(1);
                            }
                            2 => statue_level = Some(2),
                            3 => {
                                r.set_u32(0xf8, r.u32_(0xf8) | 1);
                                statue_level = Some(2);
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
                if let Some(l) = statue_level {
                    r.set_u8(0xf2, l);
                }
            }
            5 => {
                let mut bound = None;
                if inst.is_some() {
                    let flags: u32 = match subtype {
                        0x14 => 0x10,
                        0x15 => 8,
                        0x16 => 0x12,
                        0x17 | 0x1d => 10,
                        0x19 => 0x804,
                        0x1a => 2,
                        0x1b => 0x80c,
                        0x1c => 9,
                        _ => (i32::from(ii16(0x32)) | 8) as u32,
                    };
                    let trigger = (subtype as u32 | ((flags & 0xff) << 8)) as i16;
                    let object = ii16(0x30);
                    // Every loaded object of the verified level has a model
                    // node (+0x28), so the original's Errorf branch is not taken.
                    if object >= 0 && (object as usize) < self.objects.len() {
                        bound = Some(object as usize);
                    }
                    r.set_u32(0xf0, bound.map_or(0, |o| o as u32 + 1));
                    if let Some(o) = bound {
                        if self.objects[o].flags_10 & 0x800 != 0 {
                            self.objects[o].flags_10 |= 0x1000_0000;
                        }
                        self.add_item_wobj(o, trigger, ii16(0x38), ii16(0x3a), iu8(0x35))?;
                        let ow = &mut self.objects[o];
                        ow.byte_17 = 0;
                        ow.byte_16 = 0;
                        ow.flags_10 |= 0x10_0000;
                    }
                    r.set_u16(0xf4, (flags as u16) | (iu16(0x32) & 0xff00));
                    let wait = if iu8(0x34) == 0xff { f32::from_bits(0x3c23_d70a) } else { mul(f32::from(iu8(0x34)), 0.5)? };
                    r.set_f32(0xfc, wait);
                    r.set_u8(0xf6, iu8(0x36));
                    r.set_u8(0xf7, iu8(0x37));
                } else {
                    r.set_u32(0xf0, 0);
                    r.set_u16(0xf4, 0);
                    r.set_u32(0xfc, 0);
                    r.set_u8(0xf6, 0);
                    r.set_u8(0xf7, 0);
                }
                r.set_u32(0xf8, 0);
                r.set_u16(0x100, 0);
                r.set_u16(0x102, 0xffff);
                // SetItem (Tower): triggers 'h' (0x68) and 199 whose players have
                // finished wave 1 of world 8 open their object at once: flags
                // 0xa00000, bytes 0x2f, and the object's world animation at its
                // last frame (FindWobjWanim; applied by the game after load).
                if let Some(o) = bound
                    && self.session.cur_world == 0xd
                    && matches!(r.u8_(0xf6), 0x68 | 0xc7)
                    && self.session.battle_wave_1_done
                {
                    let ob = &mut self.objects[o];
                    ob.byte_17 = 0x2f;
                    ob.byte_16 = 0x2f;
                    ob.flags_10 |= 0xa0_0000;
                    self.wanim_to_end.push(o);
                }
            }
            7 => self.world_has_gates += 1,
            8 => {
                r.set_f32(0xf0, if inst.is_some() { ii16(0x30) as f32 } else { 0.0 });
                if r.f32_(0xf0) == 0.0 {
                    r.set_f32(0xf0, d.i16_(0x40) as f32);
                }
                r.set_u16(0xf4, iu16(0x32));
                if r.i16_(0xf4) != 0 {
                    let v = r.i16_(0xf4).wrapping_mul(-3);
                    self.definitions[def].set_i16(0x48, v);
                }
                r.set_f32(0xf0, mul(r.f32_(0xf0), m[4])?);
                r.set_i16(0xe4, cvt(mul(f32::from(r.i16_(0xe4)), m[0])?) as i16);
                // GetActiveTime(def+0x48 << 1, 0): 0 → 0, positive → itself,
                // negative → RandInt-based (0x1d6b50, not yet recovered).
                let time = self.get_active_time(i32::from(self.definitions[def].i16_(0x48)) << 1, 0)?;
                r.set_i16(0xda, cvt(mul(time as f32, m[3])?) as i16);
            }
            9 => {
                let wave = match inst {
                    Some(i) if ii32(0x30) == 0 => self.find_wave(&i[0x34..0x36]),
                    _ => -1,
                };
                r.set_u16(0xf0, wave as u16);
                r.set_u32(0xf4, 0);
                r.set_u16(0xd8, (r.u16_(0xd8) & 0xfffe) | 0x40);
            }
            10 => {
                r.set_i16(0xf0, ii16(0x30));
                r.set_u16(0xf2, if inst.is_some() { iu16(0x32) } else { 1 });
                if r.i16_(0xf0) < 1 {
                    r.set_i16(0xf0, d.i16_(4));
                }
                let s = r.i16_(0xf0);
                if s < 0x36 && (0x32 < s || s == 0x28 || s == 0x31) {
                    r.set_u16(0xd8, r.u16_(0xd8) | 0x40);
                }
                if s == 0x29 {
                    r.set_i16(0xe4, r.i16_(0xe4).wrapping_mul(r.i16_(0xf2)));
                }
                r.set_u16(0xf4, 0);
                r.set_u16(0xf6, 0);
                r.set_u32(0xf8, 0);
                r.set_u32(0xfc, 0);
                r.set_u32(0x100, 0);
            }
            11 => {
                r.set_i32(0xf0, ii32(0x30));
                r.set_i32(0xf4, ii32(0x34));
                r.set_u32(0xf8, 0);
            }
            12 => {
                let o = ii32(0x30);
                if inst.is_some() && o >= 0 {
                    if o as usize >= self.objects.len() {
                        return err("type-12 object index outside the world");
                    }
                    r.set_u32(0xf0, o as u32 + 1);
                    r.set_i32(0xf4, ii32(0x34));
                } else {
                    r.set_u32(0xf0, 0);
                    r.set_u32(0xf4, 0);
                }
                r.set_i32(0xf8, ii32(0x38));
                r.set_u32(0xfc, 0);
            }
            13 => {
                r.set_i32(0xf0, ii32(0x30));
                r.set_u16(0xfc, iu16(0x34));
                let mut name = [0u8; 16];
                if let Some(i) = inst {
                    let n = i[8..0x18].iter().position(|&b| b == 0).unwrap_or(16);
                    name[..n].copy_from_slice(&i[8..8 + n]);
                }
                if inst.is_none() || r.i16_(0xfc) != 0 {
                    r.set_i32(0xf4, -1);
                } else {
                    name.make_ascii_uppercase();
                    let n = name.iter().position(|&b| b == 0).unwrap_or(16);
                    // AudioFindSound: the first name equal over 15 bytes.
                    let key = &name[..n.min(15)];
                    let id = self.session.sound_names.iter().find(|(s, _)| s.iter().take(15).eq(key.iter())).map(|(_, id)| *id);
                    r.set_i32(0xf4, id.unwrap_or(-1));
                }
                r.set_u16(0xfe, 0);
                r.set_u16(0x100, iu16(0x38));
                r.set_u16(0x102, iu16(0x3a));
                if inst.is_some() {
                    self.definitions[def].0[0x28..0x38].copy_from_slice(&name);
                }
                // +0xf8 = FindWorldAnimNode(10.0, position): sound_node,
                // set by the caller (attach_sound_node: it needs the posed
                // world animation objects).
                r.set_u16(0xd8, r.u16_(0xd8) | 0x40);
            }
            _ => {}
        }
        for (row, values) in matrix.iter().enumerate() {
            for (col, v) in values.iter().enumerate() {
                r.set_f32(0x10 + 16 * row + 4 * col, *v);
            }
            r.set_u32(0x1c + 16 * row, 0);
        }
        r.set_f32(0x40, position[0]);
        r.set_f32(0x44, position[1]);
        r.set_f32(0x48, position[2]);
        // Observed 1.0 in every fixture record; CopyMat4 source word not recovered.
        r.set_f32(0x4c, 1.0);
        Self::update_offsets(&mut r)?;
        if kind == 8 {
            r.set_u8(0x97, r.u8_(0x97) | 4);
        } else if kind == 1 && subtype == 0xf && self.session.cur_world == 0xd && self.session.tower_new_player {
            // A new player's Tower crystals start hidden (0x800, tree alpha
            // 0xff) until the welcome fades them in (ProcessItems).
            r.or_u16(0xd8, 0x800);
            self.alpha_full.push(slot);
        }
        self.records[slot] = r;
        Ok(())
    }

    fn kind_of(&self, slot: usize) -> Option<i32> {
        let r = &self.records[slot];
        r.active().then(|| self.definitions[r.definition()].kind())
    }

    /// MatchTransporters 0x152a70: +0xf8 = first other active type-11 item
    /// whose +0xf0 equals this item's +0xf4 (stored as slot + 1).
    pub fn match_transporters(&mut self) -> Vec<usize> {
        let mut unmatched = Vec::new();
        for a in 0..self.records.len() {
            if self.kind_of(a) != Some(11) {
                continue;
            }
            let want = self.records[a].i32_(0xf4);
            let found = (0..self.records.len())
                .find(|&b| b != a && self.kind_of(b) == Some(11) && self.records[b].i32_(0xf0) == want);
            match found {
                Some(b) => self.records[a].set_u32(0xf8, b as u32 + 1),
                None => unmatched.push(a), // Errorf
            }
        }
        unmatched
    }

    /// LinkTriggers 0x152bb8: de-duplicate trigger ids (+0xf6), then chain
    /// each trigger with a target id (+0xf7) to the first matching
    /// non-0x40 trigger (+0xf8 = slot + 1, target +0xf4 |= 0x200), refusing cycles.
    pub fn link_triggers(&mut self) -> Vec<usize> {
        let n = self.records.len();
        let is5 = |w: &Self, i: usize| w.kind_of(i) == Some(5);
        for a in 0..n {
            if !is5(self, a) {
                continue;
            }
            for b in 0..n {
                let (ra, rb) = (&self.records[a], &self.records[b]);
                if b != a
                    && is5(self, b)
                    && rb.u16_(0xf4) & 0x40 == ra.u16_(0xf4) & 0x40
                    && 0 < ra.i8_(0xf6)
                    && rb.i8_(0xf6) == ra.i8_(0xf6)
                {
                    self.records[b].set_u8(0xf6, 0); // Errorf: duplicate trigger id
                }
            }
        }
        let mut problems = Vec::new();
        for a in 0..n {
            if !is5(self, a) || self.records[a].i8_(0xf7) == 0 {
                continue;
            }
            let target = self.records[a].i8_(0xf7);
            let found = (0..n).find(|&b| {
                b != a && is5(self, b) && self.records[b].u16_(0xf4) & 0x40 == 0 && self.records[b].i8_(0xf6) == target
            });
            let Some(b) = found else {
                problems.push(a); // Errorf: missing target
                continue;
            };
            let mut p = Some(b);
            let mut cycle = false;
            let mut guard = 0;
            while let Some(q) = p {
                let next = self.records[q].u32_(0xf8).checked_sub(1).map(|x| x as usize);
                if next == Some(a) {
                    cycle = true; // Errorf: trigger loop
                    break;
                }
                p = next;
                guard += 1;
                if guard > n {
                    break;
                }
            }
            if cycle {
                problems.push(a);
            } else {
                self.records[a].set_u32(0xf8, b as u32 + 1);
                let f = self.records[b].u16_(0xf4) | 0x200;
                self.records[b].set_u16(0xf4, f);
            }
        }
        problems
    }

    /// GetActiveTime 0x1512c0: 0 → default, positive → itself,
    /// negative → (-t >> 1) + RandInt(-t).
    /// GetActiveTime on the item world's own generator (load-time use only;
    /// in play ProcessItems draws from the game's rand).
    pub fn get_active_time(&mut self, t: i32, default: i32) -> R<i32> {
        if t == 0 {
            return Ok(default);
        }
        if t > 0 {
            return Ok(t);
        }
        let n = t.wrapping_neg();
        let r = self.rng.rand_int(n as u32).ok_or_else(|| UnsupportedItem("RandInt(0)".into()))?;
        Ok((n >> 1).wrapping_add(r as i32))
    }

    /// UpdateObjOffsets 0x103398 → UpdateObjColOffset (+0x60) and
    /// UpdateObjAttnOffset (+0x50), both with the offset at +0xc8.
    pub fn update_offsets(r: &mut ItemRecord) -> R<()> {
        let m = r.matrix();
        let p = collision_position(r.position(), [r.f32_(0xc8), r.f32_(0xcc), r.f32_(0xd0)], r.u32_(0x74), &m)?;
        for (i, v) in p.iter().enumerate() {
            r.set_f32(0x50 + 4 * i, *v);
            r.set_f32(0x60 + 4 * i, *v);
        }
        Ok(())
    }

    /// PlaceItem: drop onto the floor + 0.1 unless definition +0xa bit 0.
    /// Returns the floor object. Every SetItem path that returns normally
    /// creates the render node the original requires (+0x70).
    pub fn place_item(&mut self, slot: usize, collision: &mut CollisionWorld) -> R<Option<usize>> {
        let r = &self.records[slot];
        if !r.active() || self.definitions[r.definition()].i16_(10) & 1 != 0 {
            return Ok(None);
        }
        let p = r.position();
        let (y, hit) = collision.floor_pos(p[1], 1.0, p, 0)?;
        let placed = add(y, f32::from_bits(TENTH))?;
        let floor = hit.map(|h| h.object);
        let kind = self.definitions[r.definition()].kind();
        let r = &mut self.records[slot];
        r.set_f32(0x44, placed);
        // MBNodeSetParent(item node, floor node) when the floor object has a
        // node (+0x28: every object, all 1,360 in the G1 fixture) and flag
        // 0x1000, then UpdateObjWorldMat (the node relative to that parent).
        // A static floor leaves the node's parent as it was.
        if let Some(o) = floor.filter(|&o| collision.runtime_flags.get(o).is_some_and(|f| f & 0x1000 != 0)) {
            let m: [[f32; 4]; 4] = std::array::from_fn(|i| std::array::from_fn(|j| r.f32_(0x10 + 0x10 * i + 4 * j)));
            let local = collision.unparent_chain(m, Some(o)).map_err(|e| UnsupportedItem(format!("PlaceItem parent: {e:?}")))?;
            if self.floor_parent.len() <= slot {
                self.floor_parent.resize(slot + 1, None);
            }
            self.floor_parent[slot] = Some((o, local));
        } else if self.sound_node.get(slot).is_some_and(|n| n.is_some()) {
            if self.sound_anchor.len() <= slot {
                self.sound_anchor.resize(slot + 1, None);
            }
            self.sound_anchor[slot] = Some(std::array::from_fn(|i| std::array::from_fn(|j| r.f32_(0x10 + 0x10 * i + 4 * j))));
        }
        let r = &mut self.records[slot];
        if let (true, Some(bound), Some(f)) = (kind == 5 && r.u16_(0xf4) & 0x400 != 0, r.u32_(0xf0).checked_sub(1), floor) {
            let bound = bound as usize;
            if bound == f || Some(bound) == self.object_parent[f] {
                r.set_u16(0xf4, r.u16_(0xf4) | 0x100);
            }
        }
        Ok(floor)
    }
}

/// FindWorldAnimNode 0x11cd38(limit, position): over the world animation
/// entries with data (+0xc), the object whose node's world translation
/// (GetWorldMat) is strictly nearest below `limit` (FPU distance, SQRT.S).
pub fn find_world_anim_node(collision: &CollisionWorld, anim_objects: &[usize], limit: f32, p: [f32; 3]) -> R<Option<usize>> {
    let mut best = limit;
    let mut found = None;
    for &o in anim_objects {
        let m = collision.object_matrix(o).map_err(|e| UnsupportedItem(format!("FindWorldAnimNode: {e:?}")))?;
        let d = [sub(m[3][0], p[0])?, sub(m[3][1], p[1])?, sub(m[3][2], p[2])?];
        let s = add(add(mul(d[0], d[0])?, mul(d[1], d[1])?)?, mul(d[2], d[2])?)?;
        let dist = crate::numeric::sqrt_rn(s)?;
        if dist < best {
            best = dist;
            found = Some(o);
        }
    }
    Ok(found)
}

impl ItemWorld {
    /// SetItem's kind-13 FindWorldAnimNode(10.0, position) for `slot` at the
    /// record's current position (call before PlaceItem moves it).
    pub fn attach_sound_node(&mut self, slot: usize, collision: &CollisionWorld, anim_objects: &[usize]) -> R<()> {
        let r = &self.records[slot];
        if !r.active() || self.definitions[r.definition()].kind() != 13 {
            return Ok(());
        }
        let node = find_world_anim_node(collision, anim_objects, 10.0, r.position())?;
        let m: [[f32; 4]; 4] = std::array::from_fn(|i| std::array::from_fn(|j| r.f32_(0x10 + 0x10 * i + 4 * j)));
        if self.sound_node.len() <= slot {
            self.sound_node.resize(slot + 1, None);
        }
        if self.sound_anchor.len() <= slot {
            self.sound_anchor.resize(slot + 1, None);
        }
        self.sound_node[slot] = node;
        // SetItem's node starts at the record matrix; PlaceItem's
        // UpdateObjWorldMat moves it (not for definitions it skips, +10 bit 0).
        self.sound_anchor[slot] = node.map(|_| m);
        Ok(())
    }

    /// Copy the run-time object words (+0x10 flags, +0x35 exclusion, the
    /// wobj node y) into the collision world, which reads them as the
    /// original reads the objects.
    pub fn sync_collision(&self, collision: &mut CollisionWorld) {
        for o in 0..collision.objects.len() {
            collision.runtime_flags[o] = self.objects[o].flags_10;
            collision.runtime_exclusion[o] = self.object_exclusion[o];
        }
        for w in &self.wobjs {
            collision.node_y[w.object] = Some(self.object_node_y[w.object]);
        }
    }
}

/// Result of [`load_items`].
pub struct LoadedItems {
    pub world: ItemWorld,
    /// Per instance: the slot or why the port stopped (instances after the
    /// first unsupported one are still attempted; their slots may then differ).
    pub outcome: Vec<Result<usize, String>>,
    pub max_level_coins: i32,
    pub random_calls: u32,
}

/// AddItemInstList 0x1531d0 up to PlaceItem: RandomItemNum = vb_counter,
/// NewItem per instance, coin count, ProcessItemWobjs (`before_place`, the
/// caller's step: it needs the game's globals), then PlaceItem per slot.
pub fn load_items(
    world: &World,
    collision: &mut CollisionWorld,
    tables: ItemTables,
    session: ItemSession,
    seed: u32,
    before_instances: &mut dyn FnMut(&mut ItemWorld, &mut CollisionWorld) -> R<()>,
    before_place: &mut dyn FnMut(&mut ItemWorld, &mut CollisionWorld) -> R<()>,
) -> R<LoadedItems> {
    let start = session.random_item_num;
    let mut w = ItemWorld::new(world, collision, tables, session)?;
    w.rng = Rand::new(seed);
    // GauntWorldInit's world_update runs between the world's creation and
    // AddItemInstList (the caller's step).
    before_instances(&mut w, collision)?;
    let max_items = world.items.len() + 500;
    let mut outcome = Vec::new();
    let mut coins = 0;
    for instance in &world.items {
        match w.new_item(instance, max_items) {
            Ok(slot) => {
                let r = &w.records[slot];
                let d = &w.definitions[r.definition()];
                if r.active() && d.kind() == 1 && d.subtype() == 1 && w.show_item(r) {
                    coins += 1;
                }
                outcome.push(Ok(slot));
            }
            Err(e) => outcome.push(Err(e.0)),
        }
    }
    // ProcessItemWobjs moves the elevators to inity + y (and sets the object
    // words) before PlaceItem finds each item's floor.
    before_place(&mut w, collision)?;
    for slot in 0..w.records.len() {
        if w.records[slot].i32_(0) == 0 && !w.records[slot].active() {
            continue;
        }
        w.place_item(slot, collision)?;
        // The fixture's +0x50/+0x60 hold the placed position plus offset;
        // the recomputing caller after PlaceItem is not yet identified.
        let mut r = w.records[slot].clone();
        ItemWorld::update_offsets(&mut r)?;
        w.records[slot] = r;
    }
    w.match_transporters();
    w.link_triggers();
    let random_calls = w.session.random_item_num.wrapping_sub(start) / 0x1b7;
    Ok(LoadedItems { world: w, outcome, max_level_coins: coins, random_calls })
}

#[cfg(test)]
mod tests {
    #[test]
    fn pitch_mat3_rotates_each_row_y_z() {
        // Synthetic: a quarter turn maps row 1 (0,1,0) to (0,cos,sin) = (0,0,1).
        let mut m = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        super::pitch_mat3(&mut m, std::f32::consts::FRAC_PI_2).unwrap();
        assert_eq!(m[0], [1.0, 0.0, 0.0]);
        assert!(m[1][1].abs() < 1e-6 && (m[1][2] - 1.0).abs() < 1e-6);
        assert!((m[2][1] + 1.0).abs() < 1e-6 && m[2][2].abs() < 1e-6);
        let mut n = [[0.0, 1.0, 0.0]; 3];
        super::pitch_mat3(&mut n, 0.00005).unwrap();
        assert_eq!(n, [[0.0, 1.0, 0.0]; 3]);
    }
}
