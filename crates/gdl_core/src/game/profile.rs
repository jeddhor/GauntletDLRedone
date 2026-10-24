//! Character records and the original save region: init_players 0x12e668,
//! clear_player 0x131348, init_player_powerups 0x12c030,
//! set_player_default_atts 0x12fe10, PlayerUpdateAtts 0x12bab0,
//! check_player_atts 0x12fee0, load_player_atts 0x12ffe0, save_player_atts
//! 0x12ffb8, player_store_in_save 0x130098 and player_get_from_save 0x130400
//! (both without the Sumner branch: player_sumner_desc is a runtime
//! pointer and the secret character is not supported), plus the per-level
//! fields load_player_geo 0x130d40 takes from the class record.
//!
//! The save region is player +0x99c..+0x1a10 (0x1074 bytes), the bytes
//! PlayerWriteSaveFile 0x130b28 hands to saveSave and PlayerLoadSaveFile
//! 0x130b88 reads back. Within it: name +0x99c (8), class +0x9a4 (i16),
//! colour +0x9a6 (i8), level sum +0x9aa (i16), per-class {exp, health,
//! 4 bonus attributes} at +0x9ac + class × 0x18, per-class records at
//! +0xcec + class × 0xb4 (potions, keys, shards, runes, wave bits,
//! gargoyle items, crystals, gold at +0x30, powerups at +0x34..+0xb4,
//! powerup flags at +0xa), the wave bytes +0x182c + class × 0xe + world
//! and the controller options +0x190c..+0x190f. Unknown bytes are kept.
use super::record::{Record, PLAYER};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, UnsupportedNumericDomain};
use crate::player_class::{default_class, exp_to_level, ClassTables};

pub const SAVE_START: usize = 0x99c;
pub const SAVE_LEN: usize = 0x1074;
/// PlayerSaveState's copy of the region (restored by PlayerRestoreState).
pub const SAVE_BACKUP: usize = 0x1a28;
/// Class record offset within the player record.
pub const CLASS_RECORD: usize = 0xcec;
pub const CLASS_STRIDE: usize = 0xb4;
/// Per-class attribute entry {exp, health, bonus × 4}.
pub const CLASS_ATTS: usize = 0x9ac;

/// PDAT class records by class index (PDATA/<tag>.WAD, 0x180 bytes each).
#[derive(Clone, Debug, Default)]
pub struct Pdats(pub Vec<Vec<u8>>);

impl Pdats {
    fn f(&self, class: u32, o: usize) -> Result<f32, String> {
        let b = self.0.get(class as usize).filter(|b| o + 4 <= b.len()).ok_or_else(|| format!("no PDAT for class {class}"))?;
        Ok(f32::from_le_bytes(b[o..o + 4].try_into().unwrap()))
    }
    /// A PDAT float of a class (PlayerData +o).
    pub fn value(&self, class: u32, o: usize) -> Result<f32, String> {
        self.f(class, o)
    }
    pub fn get(&self, class: u32) -> Option<&[u8]> {
        self.0.get(class as usize).map(Vec::as_slice)
    }
}

/// The PlayerControl option words a save carries (PlayerControl + k ×
/// 0x3c + 0x2c, +0x30, +0x38, +0x34 ↔ save +0x190c..+0x190f).
pub type ControlOptions = [u32; 4];

#[derive(Debug)]
pub enum ProfileError {
    Numeric(UnsupportedNumericDomain),
    Data(String),
}

impl From<UnsupportedNumericDomain> for ProfileError {
    fn from(e: UnsupportedNumericDomain) -> Self {
        Self::Numeric(e)
    }
}
impl From<ProfileError> for super::Unsupported {
    fn from(e: ProfileError) -> Self {
        super::Unsupported(match e {
            ProfileError::Numeric(n) => format!("profile numeric: {n:?}"),
            ProfileError::Data(d) => format!("profile: {d}"),
        })
    }
}

impl From<String> for ProfileError {
    fn from(e: String) -> Self {
        Self::Data(e)
    }
}
impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Numeric(e) => write!(f, "{e:?}"),
            Self::Data(s) => f.write_str(s),
        }
    }
}

type R<T> = Result<T, ProfileError>;

fn class_of(r: &Record) -> u32 {
    r.u32_(0xc)
}

/// init_players: a zeroed record with +0 (index) and +4 (colour) = k.
pub fn init_player(k: usize) -> Record {
    let mut r = Record::zeroed(PLAYER);
    r.set_u32(0, k as u32);
    r.set_u32(4, k as u32);
    r
}

/// The record plays Sumner (change_player(k, 0x10)): class 2 (+0xc) with
/// player_sumner_desc in +0x104 (its value: the "sum" string 0x2a6c00).
pub fn is_sumner(r: &Record) -> bool {
    r.u32_(0xc) == 2 && r.u32_(0x104) == super::pselect::SUMNER_DESC
}

/// init_player_powerups: 8 × 16 powerup bytes at +0x144, +0x1c4 and +0x138.
pub fn init_player_powerups(r: &mut Record) {
    r.0[0x144..0x1c4].fill(0);
    r.set_u32(0x1c4, 0);
    r.set_u32(0x138, 0);
}

/// PlayerUpdateAtts(player, class, NULL): each attribute = min(base +
/// (level − 1) × 5, cap) + bonus, at most 999 (PDAT pairs +0x28/+0x2c fight,
/// +0x38/+0x3c armor, +0x40/+0x44 magic, +0x30/+0x34 speed).
pub fn player_update_atts(r: &mut Record, class: u32, pdats: &Pdats) -> R<()> {
    // Sumner: every attribute 999.0.
    if is_sumner(r) {
        for o in [0x108, 0x10c, 0x110, 0x114] {
            r.set_f32(o, 999.0);
        }
        return Ok(());
    }
    let bonus = CLASS_ATTS + 0x18 * class as usize;
    let step = ((r.i32_(0x2ac0) - 1) * 5) as f32;
    for (k, (base, cap)) in [(0x28, 0x2c), (0x38, 0x3c), (0x40, 0x44), (0x30, 0x34)].into_iter().enumerate() {
        let mut v = add(step, pdats.f(class, base)?)?;
        let c = pdats.f(class, cap)?;
        if v >= c {
            v = c;
        }
        let v = add(v, r.f32_(bonus + 8 + 4 * k))?;
        r.set_f32(0x108 + 4 * k, if v < 999.0 { v } else { 999.0 });
    }
    Ok(())
}

/// check_player_atts: PlayerUpdateAtts for the record's class, then the
/// derived words +0x118..+0x12c (missiles from magic for base classes 2, 6).
pub fn check_player_atts(r: &mut Record, pdats: &Pdats, t: &ClassTables) -> R<()> {
    player_update_atts(r, class_of(r), pdats)?;
    let atts = [r.f32_(0x108), r.f32_(0x10c), r.f32_(0x110), r.f32_(0x114)];
    let w = t.derived_words(atts, r.u32_(8))?;
    for (k, v) in w.iter().enumerate() {
        r.set_f32(0x118 + 4 * k, *v);
    }
    Ok(())
}

/// load_player_atts: derived words (with the level still unchanged), then
/// experience +0x1a1c, level +0x2ac0 and health +0x1a10 of the class.
pub fn load_player_atts(r: &mut Record, pdats: &Pdats, t: &ClassTables) -> R<()> {
    let e = CLASS_ATTS + 0x18 * class_of(r) as usize;
    check_player_atts(r, pdats, t)?;
    let exp = r.i32_(e);
    r.set_i32(0x1a1c, exp);
    r.set_i32(0x2ac0, exp_to_level(exp));
    r.set_u32(0x1a10, r.u32_(e + 4));
    Ok(())
}

/// save_player_atts: experience and health back into the class entry.
pub fn save_player_atts(r: &mut Record) {
    let e = CLASS_ATTS + 0x18 * class_of(r) as usize;
    r.set_u32(e, r.u32_(0x1a1c));
    r.set_u32(e + 4, r.u32_(0x1a10));
}

/// clear_player(k, wipe). The class it assigns is def_char_type(9) — the
/// loop counter's final value, so always 6 (Sorceress) — as the original
/// does. `demo` is DemoMode (2500 gold).
pub fn clear_player(r: &mut Record, wipe: bool, demo: bool, pdats: &Pdats) -> R<()> {
    r.set_u32(0x1a18, 0);
    for k in 0..9 {
        r.set_u32(0x2a9c + 4 * k, (k & 3) as u32);
    }
    r.set_u32(0x1a20, if demo { 0x9c4 } else { 0 });
    r.set_f32(0x1a10, 500.0);
    r.set_u16(0x1a24, 0);
    r.set_u16(0x1a26, 0);
    for o in [0x2ae8, 0x2aec, 0x2af0] {
        r.set_u32(o, 0);
    }
    r.set_i32(0x2af4, -1);
    r.set_u32(0x104, 0);
    init_player_powerups(r);
    r.set_i32(0x2ac0, 1);
    r.set_u32(0x1a1c, 0);
    let c = default_class(9);
    r.set_u32(8, c);
    r.set_u32(0xc, c);
    for o in [0x1a14, 0x800, 0x804, 0x2acc, 0x2ac4] {
        r.set_u32(o, 0);
    }
    if wipe {
        r.0[SAVE_START..SAVE_START + SAVE_LEN].fill(0);
        r.0[SAVE_BACKUP..SAVE_BACKUP + SAVE_LEN].fill(0);
        for o in [0xfc, 0x2ad4, 0x2ad8] {
            r.set_u32(o, 0);
        }
    }
    // set_player_default_atts: every class's bonus attributes cleared.
    for class in 0..16 {
        let b = CLASS_ATTS + 0x18 * class + 8;
        r.0[b..b + 16].fill(0);
    }
    player_update_atts(r, class_of(r), pdats)
}

/// player_store_in_save: the record's live values into the save region.
/// Sumner: the backup (the region as loaded, +0x1a28) goes back over the
/// region, the saved character reloads with the descriptor cleared
/// (player_get_from_save(−1), which also sets PlayerControl) and the
/// descriptor returns; the store then runs with the class read on entry
/// (2), so class entry 2 takes the reloaded character's gold, potions,
/// keys, shards, runes and powerups. Sumner's own play is never kept.
/// Returns the reloaded controller options (Sumner only).
pub fn player_store_in_save(r: &mut Record, controls: &ControlOptions, pdats: &Pdats, t: &ClassTables) -> R<Option<ControlOptions>> {
    let class = class_of(r);
    let mut reloaded = None;
    if is_sumner(r) {
        r.0.copy_within(SAVE_BACKUP..SAVE_BACKUP + SAVE_LEN, SAVE_START);
        r.set_u32(0x104, 0);
        reloaded = Some(player_get_from_save(r, None, pdats, t)?);
        r.set_u32(0x104, super::pselect::SUMNER_DESC);
    }
    store_live(r, class, reloaded.as_ref().unwrap_or(controls));
    Ok(reloaded)
}

/// player_store_in_save's store for class entry `class`.
fn store_live(r: &mut Record, class: u32, controls: &ControlOptions) {
    save_player_atts(r);
    let c = CLASS_RECORD + CLASS_STRIDE * class as usize;
    r.set_u32(c + 0x30, r.u32_(0x1a20));
    r.set_u16(c, r.u16_(0x1a18));
    r.set_u16(c + 2, r.u16_(0x1a14));
    let v = r.u16_(c + 4) | r.u16_(0x1a24);
    r.set_u16(c + 4, v);
    let v = r.u16_(c + 6) | r.u16_(0x1a26);
    r.set_u16(c + 6, v);
    r.set_u16(0x9a4, r.u16_(0xc));
    r.set_u8(0x9a6, r.u8_(4));
    let sum: i32 = (0..16).map(|k| exp_to_level(r.i32_(CLASS_ATTS + 0x18 * k))).sum();
    r.set_i16(0x9aa, sum as i16);
    r.0.copy_within(0x144..0x1c4, c + 0x34);
    r.set_u16(c + 0xa, r.u16_(0x1c4));
    for (k, v) in controls.iter().enumerate() {
        r.set_u8(0x190c + k, *v as u8);
    }
}

/// player_get_from_save(player, class) (`class` None = the saved class
/// +0x9a4). Returns the restored controller options. Sumner: the region is
/// kept as the backup (+0x1a28), colour 0, attributes 999, level 99 with
/// its experience, full health, 5000 gold, nine keys and potions, every
/// shard (0x7fe) and rune (0x1fff), no powerups.
pub fn player_get_from_save(r: &mut Record, class: Option<u32>, pdats: &Pdats, t: &ClassTables) -> R<ControlOptions> {
    if is_sumner(r) {
        r.0.copy_within(SAVE_START..SAVE_START + SAVE_LEN, SAVE_BACKUP);
        r.set_u32(4, 0);
        for o in [0x108, 0x10c, 0x110, 0x114] {
            r.set_f32(o, 999.0);
        }
        check_player_atts(r, pdats, t)?;
        r.set_i32(0x2ac0, 99);
        r.set_i32(0x1a1c, crate::player_class::level_to_exp(99));
        r.set_f32(0x1a10, crate::player_class::max_health(99));
        r.set_i32(0x1a20, 5000);
        r.set_i32(0x1a18, 9);
        r.set_i32(0x1a14, 9);
        r.set_u16(0x1a24, 0x7fe);
        r.set_u16(0x1a26, 0x1fff);
        init_player_powerups(r);
        return Ok(std::array::from_fn(|k| u32::from(r.u8_(0x190c + k))));
    }
    let class = class.unwrap_or(r.i16_(0x9a4) as u32);
    r.set_u32(0xc, class);
    r.set_i32(4, i32::from(r.i8_(0x9a6)));
    load_player_atts(r, pdats, t)?;
    let c = CLASS_RECORD + CLASS_STRIDE * class as usize;
    r.set_u32(0x1a20, r.u32_(c + 0x30));
    r.set_i32(0x1a18, i32::from(r.i16_(c)));
    r.set_i32(0x1a14, i32::from(r.i16_(c + 2)));
    r.set_u16(0x1a24, r.u16_(c + 4));
    r.set_u16(0x1a26, r.u16_(c + 6));
    if r.f32_(CLASS_ATTS + 0x18 * class as usize + 4) == 0.0 {
        clear_player(r, false, false, pdats)?;
    }
    r.set_u32(0xc, class);
    r.set_u32(8, if class < 8 { class } else { class - 8 });
    player_update_atts(r, class, pdats)?;
    r.0.copy_within(c + 0x34..c + 0xb4, 0x144);
    r.set_i32(0x1c4, i32::from(r.i16_(c + 0xa)));
    Ok(std::array::from_fn(|k| u32::from(r.u8_(0x190c + k))))
}

/// change_player(k, class) (not Sumner): store the record, switch class,
/// restore that class from the region.
pub fn change_player(r: &mut Record, class: u32, controls: &ControlOptions, pdats: &Pdats, t: &ClassTables) -> R<ControlOptions> {
    player_store_in_save(r, controls, pdats, t)?;
    r.set_u32(0xc, class);
    r.set_u32(8, if class < 8 { class } else { class - 8 });
    player_get_from_save(r, Some(class), pdats, t)
}

/// new_player(k): clear_player(k, 1), +0xfc = 2, +0x2ad4 = 0, +0x9a7 =
/// 0xff, +0x2af4 = −1 (reset_sel_menu is presentation).
pub fn new_player(k: usize, pdats: &Pdats) -> R<Record> {
    let mut r = init_player(k);
    clear_player(&mut r, true, false, pdats)?;
    r.set_u32(0xfc, 2);
    r.set_u32(0x2ad4, 0);
    r.set_u8(0x9a7, 0xff);
    r.set_i32(0x2af4, -1);
    Ok(r)
}

/// A new character from player select: new_player, the colour (+4), the
/// name (upper case, at most 7 characters, at +0x99c) and change_player to
/// the chosen class.
pub fn new_character(k: usize, name: &str, class: u32, colour: u8, pdats: &Pdats, t: &ClassTables) -> R<Record> {
    let mut r = new_player(k, pdats)?;
    r.set_u32(4, u32::from(colour));
    let n: Vec<u8> = name.bytes().filter(u8::is_ascii_graphic).map(|b| b.to_ascii_uppercase()).take(7).collect();
    r.0[SAVE_START..SAVE_START + 8].fill(0);
    r.0[SAVE_START..SAVE_START + n.len()].copy_from_slice(&n);
    change_player(&mut r, class, &[0; 4], pdats, t)?;
    Ok(r)
}

/// PlayerLoadSaveFile after new_player: the region, player_get_from_save,
/// the saved flag +0x9a7, the backup copy, +0x1910..+0x1a10 &= 0xf0, then
/// change_player to the loaded class.
pub fn load_character(k: usize, region: &[u8], pdats: &Pdats, t: &ClassTables) -> R<(Record, ControlOptions)> {
    if region.len() != SAVE_LEN {
        return Err(ProfileError::Data(format!("save region is {:#x} bytes, expected {SAVE_LEN:#x}", region.len())));
    }
    let mut r = new_player(k, pdats)?;
    r.0[SAVE_START..SAVE_START + SAVE_LEN].copy_from_slice(region);
    let controls = player_get_from_save(&mut r, None, pdats, t)?;
    r.set_u8(0x9a7, 1);
    r.0.copy_within(SAVE_START..SAVE_START + SAVE_LEN, SAVE_BACKUP);
    for b in &mut r.0[0x1910..0x1a10] {
        *b &= 0xf0;
    }
    let class = r.u32_(0xc);
    let controls = change_player(&mut r, class, &controls, pdats, t)?;
    Ok((r, controls))
}

/// The save region of a (not Sumner) record after player_store_in_save.
pub fn save_region(r: &Record, controls: &ControlOptions) -> Vec<u8> {
    let mut c = r.clone();
    store_live(&mut c, class_of(r), controls);
    c.0[SAVE_START..SAVE_START + SAVE_LEN].to_vec()
}

/// The level-start part of init_next_level / load_player_geo 0x130d40 /
/// load_player 0x12f748 for an active player, without the model, tree and
/// display pointers (presentation) and without get_player_pos (the caller
/// places the player): +0x808 = game_wave, +0xfc = 1, the class-record
/// fields (+0x814 attention height PDAT +0x50, +0x820 collision-offset
/// height +0x54, +0x828 radius +0x4c, +0x82c PDAT +0x48 × 0.5), the four
/// 1.0 scales +0x964..+0x970, load_player_geo's constant stores, the
/// "<CLS>_<COLOUR>" name +0x698 and the InitActions map at +0x1e8 (149 ×
/// {sequence, frames}) and reset_player_powerups. setup_player_display is
/// presentation; +0x94c = heart_rate (ELF initial value passed in).
pub fn level_start(r: &mut Record, pdats: &Pdats, action_map: &[[i64; 2]], game_wave: i32, heart_rate: u8) -> R<()> {
    let class = class_of(r);
    r.set_i32(0x808, game_wave);
    // NATIVE markers for load_player's node pointers (presentation): +0x80
    // the player's node and +0x6b8 the held weapon (MBNewObject while
    // weapon_anim is set — 0xf90000 at runtime in the reference); code
    // tests them against 0 (PlayerDoWeapTrail, DoPlayerAction's weapon
    // tree flags).
    r.set_u32(0x80, 1);
    r.set_u32(0x90, 0);
    r.set_u32(0x6b8, 1);
    r.set_u32(0xfc, 1);
    r.set_u32(0x10, class);
    // load_player.
    for o in [0x71c, 0x704, 0x70c, 0x720, 0x768, 0x6bc, 0x714, 0x718, 0x7d4, 0x708] {
        r.set_u32(o, 0);
    }
    r.set_i32(0x710, -1);
    r.set_i32(0x7d0, -1);
    let colour = r.u32_(4) as usize;
    let tag = crate::player_class::CLASS_TAGS.get(class as usize).ok_or_else(|| ProfileError::Data("class tag".into()))?.to_ascii_uppercase();
    let name = format!("{tag}_{}", ["YEL", "BLU", "RED", "GRE"].get(colour).copied().unwrap_or("YEL"));
    r.0[0x698..0x6a0].fill(0);
    let n = name.as_bytes();
    r.0[0x698..0x698 + n.len().min(8)].copy_from_slice(&n[..n.len().min(8)]);
    if action_map.len() != 149 {
        return Err(ProfileError::Data(format!("action map has {} entries", action_map.len())));
    }
    for (k, a) in action_map.iter().enumerate() {
        r.set_i32(0x1e8 + 8 * k, a[0] as i32);
        r.set_i32(0x1ec + 8 * k, a[1] as i32);
    }
    // load_player_geo.
    for o in [0x7d8, 0x7dc, 0x1e0, 0x1e4, 0x1dc, 0x8e8, 0x690, 0x694, 0x810, 0x818, 0x81c, 0x824, 0x830, 0x834, 0x838, 0x848, 0x84c, 0x850, 0x860, 0x864, 0x868, 0x870, 0x874, 0x7b4, 0x8cc, 0x8d0, 0x8d8, 0x8d4, 0x8dc, 0x8e0, 0x8e4, 0x8ec, 0x8f0, 0x948, 0x880, 0x940, 0x944, 0x984, 0x914, 0x918, 0x888, 0x974, 0x978, 0x8fc, 0x908, 0x130, 0x134, 0x138, 0x13c, 0x140, 0x89c] {
        r.set_u32(o, 0);
    }
    // reset_player_powerups 0x12c090 (after the +0x130..+0x140 stores):
    // a slot of kind 9 (+0x148) with flag 8 (+0x150; powerup 0x24) loses
    // its time (+0x144).
    for k in 0..8 {
        let o = 0x144 + 0x10 * k;
        if r.i32_(o + 4) == 9 && r.u32_(o + 0xc) & 8 != 0 {
            r.set_u32(o, 0);
        }
    }
    for o in (0x7e0..0x800).step_by(4).chain((0x8a8..0x8c8).step_by(4)) {
        r.set_u32(o, 0);
    }
    for o in [0x932, 0x1c8, 0x1d2, 0x1d4, 0x1d6, 0x1d8, 0x1da, 0x930, 0x92c, 0x934, 0x928, 0x92a, 0x936, 0x938, 0x93a, 0x93c] {
        r.set_u16(o, 0);
    }
    r.set_f32(0x814, pdats.f(class, 0x50)?);
    r.set_f32(0x820, pdats.f(class, 0x54)?);
    r.set_f32(0x828, pdats.f(class, 0x4c)?);
    r.set_f32(0x82c, mul(pdats.f(class, 0x48)?, 0.5)?);
    r.set_f32(0x878, 256.0);
    r.set_f32(0x87c, 192.0);
    r.set_u16(0x1d0, 0xf0);
    for o in [0x964, 0x968, 0x96c, 0x970] {
        r.set_f32(o, 1.0);
    }
    r.set_u16(0x92e, 0x10);
    r.set_u32(0x94c, u32::from(heart_rate));
    r.set_u32(0x8f4, r.u32_(0x8f8));
    r.set_f32(0x904, -1.0);
    for k in 0..5 {
        r.set_i32(0x950 + 4 * k, -1);
    }
    Ok(())
}

/// PlayerSaveState(k, backup) (skipped for Sumner): player_store_in_save, the
/// backup copy unless (CurWorld, CurLevel) is (5, 1) or (6, 1), and the
/// saved flag +0x9a7 = 0.
pub fn player_save_state(r: &mut Record, backup: bool, cur_world: i32, cur_level: i32, controls: &ControlOptions) {
    // Sumner keeps no state.
    if is_sumner(r) {
        return;
    }
    store_live(r, class_of(r), controls);
    if backup && !(cur_world == 5 && cur_level == 1) && !(cur_world == 6 && cur_level == 1) {
        r.0.copy_within(SAVE_START..SAVE_START + SAVE_LEN, SAVE_BACKUP);
    }
    r.set_u8(0x9a7, 0);
}

/// PlayerRestoreState(k): the backup region back, then
/// player_get_from_save (Sumner: health to the maximum only).
pub fn player_restore_state(r: &mut Record, pdats: &Pdats, t: &ClassTables) -> R<ControlOptions> {
    // Sumner: only the health back to its maximum.
    if is_sumner(r) {
        r.set_f32(0x1a10, crate::player_class::max_health(r.i32_(0x2ac0)));
        return Ok(std::array::from_fn(|k| u32::from(r.u8_(0x190c + k))));
    }
    r.0.copy_within(SAVE_BACKUP..SAVE_BACKUP + SAVE_LEN, SAVE_START);
    player_get_from_save(r, None, pdats, t)
}

/// Save-region summary for menus (name, class, colour, level of the
/// class, level sum, gold of the class).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Summary {
    pub name: String,
    pub class: i16,
    pub colour: i8,
    pub level: i32,
    pub level_sum: i16,
    pub gold: i32,
}

pub fn summary(region: &[u8]) -> Option<Summary> {
    if region.len() != SAVE_LEN {
        return None;
    }
    let at = |o: usize| o - SAVE_START;
    let i16_ = |o: usize| i16::from_le_bytes([region[at(o)], region[at(o) + 1]]);
    let i32_ = |o: usize| i32::from_le_bytes(region[at(o)..at(o) + 4].try_into().unwrap());
    let class = i16_(0x9a4);
    let c = usize::try_from(class).ok().filter(|&c| c < 16)?;
    let name = &region[..8];
    Some(Summary {
        name: String::from_utf8_lossy(&name[..name.iter().position(|&b| b == 0).unwrap_or(8)]).into_owned(),
        class,
        colour: region[at(0x9a6)] as i8,
        level: exp_to_level(i32_(CLASS_ATTS + 0x18 * c)),
        level_sum: i16_(0x9aa),
        gold: i32_(CLASS_RECORD + CLASS_STRIDE * c + 0x30),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic class records: fight 200/600, speed 400/800, armor
    /// 150/550, magic 600/999 (the SOR values), radius 1.5, height 5.0.
    fn pdats() -> Pdats {
        let mut b = vec![0u8; 0x180];
        for (o, v) in [(0x28, 200.0f32), (0x2c, 600.0), (0x30, 400.0), (0x34, 800.0), (0x38, 150.0), (0x3c, 550.0), (0x40, 600.0), (0x44, 999.0), (0x48, 5.0), (0x4c, 1.5), (0x50, 4.4), (0x54, 2.5)] {
            b[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        Pdats(vec![b; 16])
    }

    fn tables() -> ClassTables {
        ClassTables { ranges: [(5.0, 20.0), (0.0, 5.0), (8.0, 32.0), (5.0, 12.5), (5.0, 20.0), (20.0, 60.0)], action_names: Vec::new() }
    }

    #[test]
    fn new_character_round_trips_through_the_region() {
        let (p, t) = (pdats(), tables());
        let mut r = new_character(1, "pele", 6, 0, &p, &t).unwrap();
        assert_eq!(&r.0[SAVE_START..SAVE_START + 5], b"PELE\0");
        // A new class entry has health 0 → clear_player: level 1, 500 health.
        assert_eq!(r.i32_(0x2ac0), 1);
        assert_eq!(r.f32_(0x1a10), 500.0);
        assert_eq!(r.f32_(0x108), 200.0);
        assert_eq!(r.f32_(0x124), 8.0);
        // Play: gold, exp and health change, then save and reload.
        r.set_i32(0x1a20, 1234);
        r.set_i32(0x1a1c, 1060);
        r.set_f32(0x1a10, 321.0);
        r.set_u16(0x1a24, 0x2);
        let region = save_region(&r, &[1, 0, 1, 0]);
        let s = summary(&region).unwrap();
        assert_eq!((s.name.as_str(), s.class, s.level, s.level_sum, s.gold), ("PELE", 6, 2, 17, 1234));
        let (back, controls) = load_character(1, &region, &p, &t).unwrap();
        assert_eq!(controls, [1, 0, 1, 0]);
        assert_eq!(back.i32_(0x1a20), 1234);
        assert_eq!(back.i32_(0x2ac0), 2);
        assert_eq!(back.f32_(0x1a10), 321.0);
        assert_eq!(back.u16_(0x1a24), 0x2);
        // Level 2: fight 205 after PlayerUpdateAtts.
        assert_eq!(back.f32_(0x108), 205.0);
        // Only the saved flag +0x9a7 differs (new 0xff → loaded 1).
        let differ: Vec<usize> = (0..SAVE_LEN).filter(|&k| back.0[SAVE_START + k] != region[k]).map(|k| SAVE_START + k).collect();
        assert_eq!(differ, vec![0x9a7]);
        assert_eq!(back.u8_(0x9a7), 1);
    }

    #[test]
    fn sumner_plays_and_keeps_the_saved_character() {
        let (p, t) = (pdats(), tables());
        let mut r = new_character(1, "pele", 6, 0, &p, &t).unwrap();
        r.set_i32(0x1a20, 1234);
        // change_player(1, 0x10): store, class 2 with player_sumner_desc, load.
        assert_eq!(player_store_in_save(&mut r, &[1, 0, 1, 0], &p, &t).unwrap(), None);
        r.set_u32(0x104, super::super::pselect::SUMNER_DESC);
        r.set_u32(0xc, 2);
        r.set_u32(8, 2);
        player_get_from_save(&mut r, Some(2), &p, &t).unwrap();
        assert!(is_sumner(&r));
        assert_eq!((r.i32_(0x2ac0), r.i32_(0x1a20), r.i32_(0x1a18), r.i32_(0x1a14)), (99, 5000, 9, 9));
        assert_eq!((r.u16_(0x1a24), r.u16_(0x1a26), r.f32_(0x1a10), r.f32_(0x108), r.f32_(0x114)), (0x7fe, 0x1fff, 9999.0, 999.0, 999.0));
        // Play changes nothing: the store reloads the saved character
        // (class 6) and writes its values into class entry 2 as well.
        r.set_i32(0x1a20, 7);
        let controls = player_store_in_save(&mut r, &[0, 0, 0, 0], &p, &t).unwrap();
        assert_eq!(controls, Some([1, 0, 1, 0]));
        assert_eq!((r.u32_(0xc), r.i32_(0x1a20), r.u32_(0x104)), (6, 1234, super::super::pselect::SUMNER_DESC));
        assert!(!is_sumner(&r));
        let s = summary(&r.0[SAVE_START..SAVE_START + SAVE_LEN]).unwrap();
        assert_eq!((s.name.as_str(), s.class, s.gold), ("PELE", 6, 1234));
        assert_eq!(r.i32_(CLASS_RECORD + CLASS_STRIDE * 2 + 0x30), 1234);
        // PlayerSaveState keeps nothing for Sumner; PlayerRestoreState only
        // refills the health.
        let mut q = r.clone();
        q.set_u32(0xc, 2);
        q.set_f32(0x1a10, 1.0);
        let before = q.clone();
        player_save_state(&mut q, true, 1, 0, &[0; 4]);
        assert_eq!(q.0, before.0);
        player_restore_state(&mut q, &p, &t).unwrap();
        assert_eq!(q.f32_(0x1a10), crate::player_class::max_health(q.i32_(0x2ac0)));
    }

    #[test]
    fn attributes_cap_and_bonus() {
        let p = pdats();
        let mut r = init_player(0);
        r.set_u32(0xc, 6);
        r.set_i32(0x2ac0, 99);
        r.set_f32(CLASS_ATTS + 0x18 * 6 + 8, 50.0);
        player_update_atts(&mut r, 6, &p).unwrap();
        // fight: min(200 + 490, 600) + 50; magic: min(600 + 490, 999) → 999 cap.
        assert_eq!(r.f32_(0x108), 650.0);
        assert_eq!(r.f32_(0x110), 999.0);
    }
}
