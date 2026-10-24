//! The player-select screen (game mode 0x400b): init_player_select
//! 0x1851b0, do_player_select 0x188228 and their helpers — setup_sel_menu
//! 0x1862f8, do_sel_menu 0x186af0, sel_set_choice 0x1861f8,
//! sel_set_inactive 0x186278, sel_activate_loadsave 0x186068, do_optmenu
//! 0x10c4f8, check_active_players 0x185f10, new_player 0x131470,
//! change_player 0x1314d8, init_player_change 0x188178, player_selected
//! 0x188050, set_hidden_player 0x12f500, LimitSeltype
//! 0x188100, useable_class 0x184828, InitGetName 0x174188 / DoGetName
//! 0x1745d0 / get_initials 0x174230, setup_file_entries 0x186910,
//! verify_vmu_file_ok 0x186898, PlayerLoadSaveFile 0x130b88 and
//! PlayerWriteSaveFile 0x130b28, and the column blits (setup_tex 0x184eb8,
//! init_blit_fx 0x184858, serve_blits 0x184918, hide_select_blits
//! 0x184fc8, set_selected_blit 0x1855b0, update_class_spec 0x185620,
//! update_class_attr 0x1859f8).
//!
//! Each player's column runs its own state (+0x2ad4, the previous one at
//! +0x2ad8, a sub-state / timer at +0x2ae4): 0 New/Load, 1 Save / Change /
//! Load / Quit / Done, 2 the quit question, 3 the name, 4 class and colour,
//! 5/10 the memory card, 6/0xb/0xc card checks, 7 "no save data", 8/0xd
//! the file list, 9 loading, 0xe writing. A player pressing Start on an
//! unassigned controller joins (check_active_players → new_player).
//!
//! NATIVE (docs/decisions: memory-card operations become versioned host
//! saves): the host's save store appears as one formatted PS2 card in slot
//! 1 with eight files ([`SaveCard`]); mounting, formatting and directory
//! creation succeed at once. LoadGauntWorldDone's background-load waits
//! complete immediately.
use super::hud::Blit;
use super::optmenu::{MenuEntry, MenuState, OptMenu, OptStyle};
use super::profile::{self, Pdats};
use super::record::Record;
use super::{Game, R};
use crate::player_class::exp_to_level;

/// Player record words the select screen uses (+0x2ac0.. are the player's
/// psel block).
pub mod o {
    pub const COLOUR: usize = 0x04;
    pub const CLASS8: usize = 0x08;
    pub const CLASS: usize = 0x0c;
    pub const SELTYPE: usize = 0x10;
    pub const STATE: usize = 0xfc;
    pub const W100: usize = 0x100;
    /// +0x104: the player descriptor (0, a Hidden entry's +0x10, or
    /// player_sumner_desc), stored as the guest address it points at.
    pub const DESC: usize = 0x104;
    pub const NAME_TIMER: usize = 0x1d0;
    pub const NAME: usize = 0x99c;
    pub const SAVED: usize = 0x9a7;
    pub const UNLOCKS: usize = 0x9a8;
    pub const LEVEL_SUM: usize = 0x9aa;
    pub const EXP: usize = 0x1a1c;
    pub const BACKUP: usize = 0x1a28;
    pub const LEVEL: usize = 0x2ac0;
    pub const CHANGING: usize = 0x2ac4;
    pub const NAME_DONE: usize = 0x2ac8;
    pub const NAME_LEN: usize = 0x2acc;
    pub const NAME_CHAR: usize = 0x2ad0;
    pub const SEL: usize = 0x2ad4;
    pub const SEL_PREV: usize = 0x2ad8;
    pub const SUB: usize = 0x2ae4;
    pub const PORT: usize = 0x2ae8;
    pub const SLOT: usize = 0x2aec;
    pub const FILE: usize = 0x2af0;
    pub const FILE2: usize = 0x2af4;
}

/// Hidden 0x221028 (entry +0x10 is its model code string) and the value
/// of player_sumner_desc 0x220ed0 (a pointer to "sum" at 0x2a6c00): the
/// descriptors +0x104 holds are these guest string addresses.
pub const HIDDEN: u32 = 0x22_1028;
pub const SUMNER_DESC: u32 = 0x2a_6c00;
const SAVE_LEN: usize = 0x1074;

/// One file of the native card (all_dirs: +0 level sum, +4 class, +8 name).
#[derive(Clone, Debug, PartialEq)]
pub struct CardFile {
    pub name: String,
    pub levels: i32,
    pub class: i32,
}

/// The host save store as the select screen's memory card.
pub trait SaveCard {
    /// The card's eight files (None = "Empty").
    fn directory(&self) -> [Option<CardFile>; 8];
    /// saveLoad: the 0x1074-byte save region of file `k`.
    fn load(&mut self, k: usize) -> Option<Vec<u8>>;
    /// saveSave.
    fn save(&mut self, k: usize, region: &[u8]) -> bool;
}

/// A select-screen blit slot (sel_bg[player][slot], 0xc bytes, and its fx
/// word pair at 0x2c358c).
#[derive(Clone, Debug, Default)]
pub struct SelBlit {
    pub tex: Option<String>,
    pub hidden: bool,
    pub x: i32,
    pub y: i32,
    pub z: f32,
    /// −1: the texture's size.
    pub w: i32,
    pub h: i32,
    /// MBBlitSetAlpha (0 opaque … 0xff clear).
    pub alpha: i32,
    pub flags: u32,
    pub fx: i32,
    pub fx_t: i32,
}

/// A select column's menu (sel_menus[player], from sel_def_menu).
#[derive(Clone, Debug)]
pub struct SelMenu {
    pub menu: OptMenu,
    pub state: MenuState,
    /// +0x84: the entry count (0 = recount).
    pub count: usize,
    /// +0x6c: shown (start_optmenu_nostack; remove_optmenu and
    /// reset_sel_menu clear it — the record itself stays).
    pub active: bool,
}

/// The select screen's tables from the ELF.
#[derive(Clone, Debug)]
pub struct SelTables {
    pub style: OptStyle,
    pub def_menu: OptMenu,
    pub new_entries: Vec<MenuEntry>,
    pub active_entries: Vec<MenuEntry>,
    pub yesno_entries: Vec<MenuEntry>,
    /// Hidden: (colour, class, name, flag) × 27.
    pub hidden: Vec<(i32, i32, String, i32)>,
    /// Cheats: (name, kind, value, extra) × 27.
    pub cheats: Vec<(String, i32, f32, i32)>,
    pub default_names: Vec<String>,
}

fn entries_at(elf: &gdl_formats::elf::Elf, at: u32) -> Result<Vec<MenuEntry>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for k in 0..16 {
        let e = elf.read(at + 0x24 * k, 0x24)?;
        let w = |o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap());
        if w(0) == 0 {
            break;
        }
        let text = |p: u32| (p != 0).then(|| elf.read(p, 32).ok().map(|b| String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned())).flatten();
        out.push(MenuEntry { label: text(w(0)).unwrap_or_default(), action: w(4) as i32, gap: w(8) as i32, value: text(w(0xc)), colour: w(0x10), mark: w(0x1c) as i32, state: w(0x20) as i32 });
    }
    Ok(out)
}

impl SelTables {
    pub fn from_elf(elf: &gdl_formats::elf::Elf) -> Result<Self, Box<dyn std::error::Error>> {
        let cstr = |b: &[u8]| String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned();
        let mut hidden = Vec::new();
        for k in 0..0x1b {
            let b = elf.read(HIDDEN + 0x24 * k, 0x24)?;
            let w = |o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            hidden.push((w(0), w(4), cstr(&b[8..0x10]), w(0x20)));
        }
        let mut cheats = Vec::new();
        for k in 0..0x1b {
            let b = elf.read(0x22_13f8 + 0x14 * k, 0x14)?;
            let w = |o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
            cheats.push((cstr(&b[0..8]), w(8), f32::from_bits(w(0xc) as u32), w(0x10)));
        }
        let mut default_names = Vec::new();
        for k in 0..16 {
            let p = u32::from_le_bytes(elf.read(0x23_f3b8 + 4 * k, 4)?.try_into().unwrap());
            default_names.push(cstr(elf.read(p, 8)?));
        }
        Ok(Self {
            style: OptStyle::from_elf(elf)?,
            def_menu: OptMenu::from_elf(elf, 0x24_a350)?,
            new_entries: entries_at(elf, 0x24_9df8)?,
            active_entries: entries_at(elf, 0x24_9e68)?,
            yesno_entries: entries_at(elf, 0x24_9f40)?,
            hidden,
            cheats,
            default_names,
        })
    }
}

/// The select screen's globals.
#[derive(Clone, Debug, Default)]
pub struct PSelect {
    pub blits: [[SelBlit; 11]; 4],
    pub menus: [Option<SelMenu>; 4],
    /// sel_file_entries per player (built by setup_file_entries).
    pub files: [Vec<MenuEntry>; 4],
    pub done: i32,
    pub abort_req: bool,
    pub save_delay: i32,
    pub load_audio: i32,
    /// init_player_select's argument (0 title, 1 Tower menu, 2 after a realm).
    pub mode: i32,
    /// The card directory as last read (all_dirs for slot 1) and whether
    /// MemCardCreateGaunt made the directory this session.
    pub dir: Vec<Option<CardFile>>,
    pub dir_created: bool,
    /// The frame's texts (DrawTextScaled / DrawGlowText) and temp blits.
    pub draws: Vec<Blit>,
}

/// do_player_select's result for the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectEnd {
    Running,
    /// psel_done passed 8: init_next_level.
    Done,
    /// psel_abort_req: init_titlescreen.
    Title,
    /// After the final level (LastFinished 8/3): init_attract_mode.
    Attract,
}

/// hs_name_column 0x23f3f8, hs_name_cx 0x23f400, flash_pal 0x23f408.
const HS_NAME_COLUMN: [i32; 4] = [42, 172, 300, 425];
const HS_NAME_CX: [i32; 4] = [64, 192, 320, 448];
const FLASH_PAL: [u32; 4] = [0xffff00, 0x87ceeb, 0xff0000, 0xff00];

impl Game {
    fn sel_x(&self, p: usize) -> i32 {
        self.hud.tables.as_ref().and_then(|t| t.sel_xpos.get(p).copied()).unwrap_or(0)
    }
    fn sel_pos(&self, slot: usize) -> [i32; 3] {
        self.hud.tables.as_ref().and_then(|t| t.sel_bg_pos.get(slot).copied()).unwrap_or([0, 0, 0])
    }
    fn sel_ctl(&self, p: usize, o: usize) -> u32 {
        self.controls.control[p].u32_(o)
    }
    fn pr(&self, p: usize) -> &Record {
        &self.players[p]
    }

    /// new_ctrl(mask, player): the player's edge (+8) has one of `mask`.
    fn new_ctrl(&self, mask: u32, p: usize) -> bool {
        mask & self.sel_ctl(p, 8) != 0
    }

    /// vmu_exists: the native card is a formatted PS2 card (2).
    fn vmu_exists(&self) -> i32 {
        2
    }

    // ---- blits -------------------------------------------------------

    /// setup_tex(p, slot, flags, hide, name): the slot's texture by name,
    /// its own size, alpha 0, flag 0x4000 colour 0x80808080.
    fn setup_tex(&self, ps: &mut PSelect, p: usize, slot: usize, flags: u32, hide: bool, name: String) {
        let b = &mut ps.blits[p][slot];
        b.tex = Some(name);
        b.hidden = hide;
        b.w = -1;
        b.h = -1;
        b.flags = flags;
        b.alpha = 0;
    }

    /// init_blit_fx(p, slot, kind): kind 5 shows the slot opaque first.
    fn init_blit_fx(ps: &mut PSelect, p: usize, slot: usize, kind: i32) {
        let b = &mut ps.blits[p][slot];
        b.fx = kind;
        b.fx_t = 0;
        if kind == 5 {
            b.hidden = false;
            b.alpha = 0xff;
        }
    }

    /// hide_select_blits(p (−1 all), all): slots 2..8 always hide; the
    /// others too when `all`.
    fn hide_select_blits(ps: &mut PSelect, p: i32, all: bool) {
        let range = if p < 0 { 0..4 } else { p as usize..p as usize + 1 };
        for q in range {
            for (k, b) in ps.blits[q].iter_mut().enumerate() {
                b.hidden = all || (2..=8).contains(&k);
                b.fx = 0;
            }
        }
    }

    /// set_selected_blit(p, class): slot 2 "S12_WEAP_<class & 7>" at height
    /// 0x140.
    fn set_selected_blit(&self, ps: &mut PSelect, p: usize, class: i32) {
        let c = crate::player_class::CLASS_TAGS.get((class & 7) as usize).copied().unwrap_or("war").to_uppercase();
        self.setup_tex(ps, p, 2, 0, false, format!("S12_WEAP_{c}"));
        ps.blits[p][2].h = 0x140;
    }

    /// serve_blits(p): advance the slots' effects; the count of running ones.
    fn serve_blits(&self, ps: &mut PSelect, p: usize) -> i32 {
        let vb = self.globals.vb_elapsed;
        let xp = self.sel_x(p);
        let mut n = 0;
        for k in 0..11 {
            let pos = self.sel_pos(k);
            let dims = |g: &Game, b: &SelBlit| -> (i32, i32) {
                b.tex.as_ref().and_then(|t| g.hud.lookup(t)).and_then(|t| g.hud.tex_info(t)).map_or((0, 0), |i| (i32::from(i.width), i32::from(i.height)))
            };
            let b = &mut ps.blits[p][k];
            match b.fx {
                1 => {
                    b.fx = 0;
                    b.fx_t = 0;
                    b.hidden = true;
                }
                2 => {
                    b.fx_t += vb;
                    let t = b.fx_t;
                    let sq = t * t;
                    let half = if sq < 0 { sq + 7 } else { sq } >> 3;
                    b.x = pos[0] + xp + half;
                    b.y = sq + t * 3;
                    b.z = pos[2] as f32;
                    b.alpha = sq;
                    b.w = 0x80;
                    b.h = 0x100 - sq;
                    if sq < 0x100 {
                        b.hidden = false;
                    } else {
                        b.fx = 0;
                        b.fx_t = 0;
                        b.hidden = true;
                    }
                    n += 1;
                }
                3 => {
                    let (w0, h0) = dims(self, &ps.blits[p][k]);
                    let b = &mut ps.blits[p][k];
                    let t = (b.fx_t + 1) & 0x3f;
                    b.fx_t = t;
                    let u = if 0x1f < t { 0x20 - (t & 0x1f) } else { t };
                    // (double)u × DAT_002acb80 (0.025) + 1.0.
                    let f = (f64::from(u) * 0.025 + 1.0) as f32;
                    let (w, h) = ((w0 as f32 * f) as i32, (h0 as f32 * f) as i32);
                    b.x = pos[0] - w / 2 + xp;
                    b.y = pos[1] - h / 2;
                    b.z = pos[2] as f32;
                    b.w = w;
                    b.h = h;
                }
                4 => {
                    let (w0, h0) = dims(self, &ps.blits[p][k]);
                    let b = &mut ps.blits[p][k];
                    b.fx_t += vb;
                    let t = b.fx_t >> 1;
                    let sq = t * t;
                    let f = (0x100 - sq) as f32 * 0.003_906_25;
                    let (w, h) = ((w0 as f32 * f) as i32, (h0 as f32 * f) as i32);
                    b.x = w0 / 2 + pos[0] - w / 2 + xp;
                    b.y = h0 / 2 + pos[1] - h / 2;
                    b.z = pos[2] as f32;
                    b.w = w;
                    b.h = h;
                    if 0x100 <= sq {
                        b.fx = 0;
                        b.fx_t = 0;
                        b.hidden = true;
                    }
                    n += 1;
                }
                5 => {
                    b.fx_t += vb;
                    let t = b.fx_t >> 1;
                    let sq = (t * t).min(0x100);
                    b.alpha = 0x100 - sq;
                    if 0x100 <= sq {
                        b.fx = 0;
                        b.fx_t = 0;
                    }
                    n += 1;
                }
                6 | 7 => {
                    b.fx_t += vb;
                    let mut t = b.fx_t >> 1;
                    if b.fx == 7 {
                        t = (t - 0x10).max(0);
                    }
                    let sq = t * t;
                    b.alpha = sq;
                    if 0x100 <= sq {
                        b.fx = 0;
                        b.fx_t = 0;
                        b.hidden = true;
                    }
                    n += 1;
                }
                _ => {}
            }
        }
        n
    }

    // ---- menus -------------------------------------------------------

    /// setup_sel_menu(p, kind): sel_def_menu copied, x relative to the
    /// column, the kind's entries (0 New/Load, 1 the active menu, 5/10 the
    /// cards, 8/0xd the files, 0xf Yes/No) and start_optmenu_nostack.
    fn setup_sel_menu(&mut self, ps: &mut PSelect, t: &SelTables, p: usize, kind: i32) {
        self.controls.clear_player_control(p, 2);
        let mut menu = t.def_menu.clone();
        menu.x -= self.sel_x(p);
        menu.kind = kind;
        let mut selected = 0usize;
        match kind {
            0 | 1 => {
                menu.entries = if kind == 0 { t.new_entries.clone() } else { t.active_entries.clone() };
                let first = self.sel_activate_loadsave(&mut menu.entries, p);
                selected = usize::try_from(first).unwrap_or(0);
            }
            5 | 10 => {
                // sel_vmu_entries (setup_vmu_entries): " SLOT  1" (port 0
                // slot 0, action 1000; gap 4).
                menu.entries = vec![MenuEntry { label: " SLOT  1".into(), action: 1000, gap: 4, value: None, colour: 0, mark: 0, state: 0 }];
                menu.x = self.sel_x(p) + 4;
                menu.y = 0x46;
                menu.scale = f32::from_bits(0x3f19_999a);
            }
            8 | 0xd => {
                menu.entries = ps.files[p].clone();
                menu.x = self.sel_x(p) + 8;
                menu.y = 0x46;
                menu.scale = f32::from_bits(0x3f19_999a);
                selected = usize::try_from(self.pr(p).i32_(o::FILE2)).unwrap_or(0);
            }
            0xf => {
                menu.entries = t.yesno_entries.clone();
                selected = 1;
                menu.y = 100 + 0x40;
            }
            _ => {}
        }
        let count = menu.entries.len();
        ps.menus[p] = Some(SelMenu { menu, state: MenuState { selected, player: Some(p), ..Default::default() }, count, active: true });
    }

    /// sel_activate_loadsave: entries' +0x20 enabled (0) or not (−1); the
    /// first enabled one.
    fn sel_activate_loadsave(&self, entries: &mut [MenuEntry], p: usize) -> i32 {
        let mut first = -1;
        let demo = self.globals.demo_mode != 0;
        let last_83 = self.globals.last_finished_world == 8 && self.globals.last_finished_level == 3;
        let desc = self.pr(p).u32_(o::DESC);
        for (k, e) in entries.iter_mut().enumerate() {
            e.state = match e.action {
                1000 | 0x3ed => 0,
                0x3e9 => {
                    if demo || last_83 || self.vmu_exists() < 1 { -1 } else { 0 }
                }
                0x3ea => {
                    if demo || self.vmu_exists() < 1 { -1 } else { 0 }
                }
                0x3eb | 0x3ec => {
                    if !last_83 && (desc == 0 || desc == SUMNER_DESC) { 0 } else { -1 }
                }
                _ => e.state,
            };
            if 0 <= e.state && first < 0 {
                first = k as i32;
            }
        }
        first
    }

    /// sel_set_choice(p, action).
    fn sel_set_choice(ps: &mut PSelect, p: usize, action: i32) {
        if let Some(m) = ps.menus[p].as_mut() {
            let mut hit = None;
            for (k, e) in m.menu.entries.iter().enumerate() {
                if e.action == action {
                    hit = Some(k);
                    if 0 <= e.state {
                        break;
                    }
                }
            }
            if let Some(k) = hit {
                m.state.selected = k;
            }
        }
    }

    /// sel_set_inactive(p, action).
    fn sel_set_inactive(ps: &mut PSelect, p: usize, action: i32) {
        if let Some(m) = ps.menus[p].as_mut() {
            for e in m.menu.entries.iter_mut().filter(|e| e.action == action && 0 <= e.state) {
                e.state = -1;
            }
        }
    }

    /// do_optmenu(menu, vertical) for player p's menu: −2 abort, −1 back,
    /// an entry's action on accept, 2/3/4/7/8 the other edges, 1 a cursor
    /// move, 5/6/9/10 held directions, 0 nothing.
    fn do_optmenu(&mut self, ps: &mut PSelect, p: usize, vertical: bool) -> i32 {
        let edge = self.sel_ctl(p, 8);
        let level = self.sel_ctl(p, 4);
        let vb = self.globals.vb_elapsed;
        let active = self.globals.active_players;
        let Some(m) = ps.menus[p].as_mut() else { return 0 };
        m.state.frames += vb;
        if m.count == 0 {
            m.count = m.menu.entries.len();
        }
        let n = m.count;
        while m.state.selected < n && m.menu.entries[m.state.selected].state < 0 {
            m.state.selected += 1;
        }
        if n <= m.state.selected {
            m.state.selected = 0;
        }
        let mut edge = edge;
        let abort = if (1 << p) & active == 0 {
            true
        } else if m.menu.kind == 2 {
            if edge & 0x40000 != 0 {
                edge |= 0x200_0000;
            }
            false
        } else {
            m.menu.title.is_some() && edge & 0x40000 != 0
        };
        if abort {
            return -2;
        }
        if edge & 0x200_0000 != 0 {
            let e = m.menu.entries.get(m.state.selected).cloned();
            return match e {
                Some(e) if 0 <= e.state => {
                    if m.menu.flags & 0x400 != 0 {
                        self.audio_cursor_select();
                    }
                    e.action
                }
                _ => {
                    self.audio_buzzer();
                    0
                }
            };
        }
        if edge & 0x800_0000 != 0 {
            if m.menu.flags & 0x800 != 0 {
                self.audio_menu_exit();
            }
            return -1;
        }
        if edge & 0x100_0000 != 0 {
            return 2;
        }
        if edge & 3 != 0 {
            return 3;
        }
        if edge & 0xc != 0 {
            return 4;
        }
        if !vertical || m.menu.entries.is_empty() {
            if edge & 0x30 != 0 {
                return 7;
            }
            if edge & 0xc0 != 0 {
                return 8;
            }
            return if level & 3 != 0 {
                5
            } else if level & 0xc != 0 {
                6
            } else if level & 0x30 != 0 {
                9
            } else if level & 0xc0 != 0 {
                10
            } else {
                0
            };
        }
        if edge & 0xc0 != 0 {
            let start = m.state.selected;
            loop {
                m.state.selected = if n <= m.state.selected + 1 { 0 } else { m.state.selected + 1 };
                if m.state.selected == start || 0 <= m.menu.entries[m.state.selected].state {
                    break;
                }
            }
        } else if edge & 0x30 != 0 {
            let start = m.state.selected;
            loop {
                m.state.selected = if m.state.selected == 0 { n - 1 } else { m.state.selected - 1 };
                if m.state.selected == start || 0 <= m.menu.entries[m.state.selected].state {
                    break;
                }
            }
        } else {
            return if level & 3 != 0 {
                5
            } else if level & 0xc != 0 {
                6
            } else if level & 0x30 != 0 {
                9
            } else if level & 0xc0 != 0 {
                10
            } else {
                0
            };
        }
        self.audio_cursor_v();
        1
    }
}


impl Game {
    /// game_main in the Tower (no option menu, no stage start):
    /// check_active_players; true when a controller joined (the caller
    /// runs init_player_select(1)).
    pub fn join_check(&mut self, pdats: &Pdats) -> R<bool> {
        let mut ps = PSelect::default();
        Ok(0 < self.check_active_players(&mut ps, pdats)?)
    }

    // ---- players -----------------------------------------------------

    /// assign_controller(port) with allow_assignments: the player a free
    /// port claims (no multitap: port & 3 == 0 → port / 4; else port & 3,
    /// else the first free player).
    fn assign_controller(&mut self, port: usize) -> Option<usize> {
        if self.controls.controller_lost != 0 || 0 <= self.controls.joy_assigned[port] {
            return None;
        }
        let claim = |g: &mut Game, k: usize| {
            g.controls.joy_remap[k] = port as i32;
            g.controls.joy_assigned[port] = k as i32;
            g.globals.active_players |= 1 << k;
            Some(k)
        };
        if port & 3 == 0 && self.controls.joy_remap[port >> 2] < 0 {
            return claim(self, port >> 2);
        }
        let k = port & 3;
        if self.controls.joy_remap[k] == -1 {
            return claim(self, k);
        }
        let free = (0..4).find(|&k| self.controls.joy_remap[k] == -1)?;
        claim(self, free)
    }

    /// check_active_players: new_menu_accept(−1, 1) (and_edges with
    /// allow_assignments: an unassigned port's Start edge joins), then
    /// new_player for every active player still in state 0.
    fn check_active_players(&mut self, ps: &mut PSelect, pdats: &Pdats) -> R<i32> {
        for port in 0..8 {
            if self.controls.joy_assigned[port] < 0 && self.controls.plr_edges[port] & 0x40000 != 0 && self.assign_controller(port).is_some() {
                break;
            }
        }
        let mut n = 0;
        for k in 0..4 {
            if self.pr(k).i32_(o::STATE) == 0 && (1 << k) & self.globals.active_players != 0 {
                self.new_player(ps, k, pdats)?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// new_player(p): a cleared record in state 2 at menu 0, unsaved, no file.
    fn new_player(&mut self, ps: &mut PSelect, p: usize, pdats: &Pdats) -> R<()> {
        let demo = self.globals.demo_mode != 0;
        profile::clear_player(&mut self.players[p], true, demo, pdats)?;
        let r = &mut self.players[p];
        r.set_i32(o::STATE, 2);
        r.set_i32(o::SEL, 0);
        r.set_u8(o::SAVED, 0xff);
        r.set_i32(o::FILE2, -1);
        // reset_sel_menu.
        if let Some(m) = ps.menus[p].as_mut() {
            m.active = false;
        }
        Ok(())
    }

    /// inactivate_player's select part: the slot leaves play.
    fn sel_inactivate(&mut self, p: usize, pdats: &Pdats) -> R<()> {
        let r = &mut self.players[p];
        r.set_i32(o::CHANGING, 0);
        r.set_i32(o::STATE, 0);
        r.set_i32(o::SEL, 0);
        r.set_i32(o::SEL_PREV, 0);
        self.controls.remove_active_player(p);
        self.globals.active_players &= !(1 << p);
        let demo = self.globals.demo_mode != 0;
        Ok(profile::clear_player(&mut self.players[p], true, demo, pdats)?)
    }

    /// change_player(p, type): the current class saved, type 0x10 =
    /// Sumner (class 2 with player_sumner_desc), the class's PDAT and save
    /// entry loaded; the model swap is the host's (Event::PlayerModel).
    fn change_player(&mut self, p: usize, ty: i32, pdats: &Pdats) -> R<()> {
        let controls = self.control_options[p];
        let t = self.class_tables.clone();
        if let Some(c) = profile::player_store_in_save(&mut self.players[p], &controls, pdats, &t)? {
            self.control_options[p] = c;
        }
        let (class, desc) = if ty == 0x10 { (2, SUMNER_DESC) } else { (ty, 0) };
        let r = &mut self.players[p];
        r.set_u32(o::DESC, desc);
        r.set_i32(o::CLASS, class);
        r.set_i32(o::CLASS8, if 7 < class { class - 8 } else { class });
        profile::player_get_from_save(&mut self.players[p], Some(class as u32), pdats, &t)?;
        self.events.push(super::Event::PlayerModel { player: p });
        Ok(())
    }

    /// set_hidden_player: a typed Hidden name (codes_enabled 0: entries
    /// without the +0x20 flag) becomes that colour/class with the entry's
    /// descriptor; Cheats names add gold (1), keys (2), potions (4) or a
    /// powerup. Sumner's descriptor keeps class 2. True for a descriptor.
    fn set_hidden_player(&mut self, p: usize, t: &SelTables, class: Option<&super::pickups::ClassData>) -> R<bool> {
        let desc = self.pr(p).u32_(o::DESC);
        if desc != 0 {
            if desc != SUMNER_DESC {
                return Ok(true);
            }
            self.players[p].set_i32(o::CLASS8, 2);
            self.players[p].set_i32(o::CLASS, 2);
            return Ok(false);
        }
        let name: Vec<u8> = self.pr(p).0[o::NAME..o::NAME + 6].to_vec();
        let same = |n: &str| -> bool { let b = n.as_bytes(); (0..6).all(|i| b.get(i).copied().unwrap_or(0) == name[i] || (name[i] == 0 && b.get(i).is_none_or(|&c| c == 0))) && !n.is_empty() };
        for (k, (colour, class, n, flag)) in t.hidden.iter().enumerate() {
            if same(n) && (*flag == 0 || 1 < self.globals.codes_enabled) {
                let r = &mut self.players[p];
                r.set_i32(o::COLOUR, *colour);
                r.set_i32(o::CLASS, *class);
                r.set_u32(o::DESC, HIDDEN + 0x24 * k as u32 + 0x10);
                return Ok(true);
            }
        }
        for (n, kind, v, extra) in &t.cheats {
            if !same(n) {
                continue;
            }
            match kind {
                1 => self.players[p].set_i32(0x1a20, *v as i32),
                2 => self.players[p].set_i32(0x1a14, *v as i32),
                4 => self.players[p].set_i32(0x1a18, *v as i32),
                _ => {
                    if let Some(c) = class {
                        self.add_powerup(*v, -1.0, p, *kind, *extra as u32, c)?;
                    }
                }
            }
        }
        Ok(false)
    }

    /// LimitSeltype(r, type, step): 0..=0x10 wrapping; Sumner (0x10) only
    /// with +0x9a8 bit 8 (else step on).
    fn limit_seltype(&self, p: usize, mut ty: i32, step: i32) -> i32 {
        if self.globals.demo_mode != 0 {
            return if ty < 4 { 7 } else if 7 < ty { 4 } else { ty };
        }
        loop {
            if 0x10 < ty {
                ty = 0;
            }
            if ty < 0 {
                ty = 0x10;
            }
            if ty == 0x10 && self.pr(p).u16_(o::UNLOCKS) & 0x100 == 0 {
                ty = step + 0x10;
                continue;
            }
            return ty;
        }
    }

    /// useable_class: classes 8.. need +0x9a8 bit (class − 8).
    pub fn useable_class(&self, p: usize) -> bool {
        let ty = self.pr(p).i32_(o::SELTYPE);
        ty <= 7 || (self.pr(p).u16_(o::UNLOCKS) >> ((ty - 8) & 0x1f)) & 1 != 0
    }

    /// player_selected(p, type): state 3, the destination of the others,
    /// change_player (the colour kept), the column's class art, the
    /// welcome line (back for a player with experience).
    fn player_selected(&mut self, ps: &mut PSelect, p: usize, ty: i32, pdats: &Pdats) -> R<()> {
        self.players[p].set_i32(o::STATE, 3);
        let dest = self.other_players_next_level(p);
        self.players[p].set_i32(0x808, dest);
        let colour = self.pr(p).i32_(o::COLOUR);
        self.change_player(p, ty, pdats)?;
        self.players[p].set_i32(o::COLOUR, colour);
        self.set_selected_blit(ps, p, ty);
        if self.pr(p).i32_(o::EXP) == 0 {
            self.audio_welcome(p, false, false);
        } else {
            self.audio_welcome(p, true, self.pr(p).u32_(o::DESC) == 0);
        }
        Ok(())
    }

    /// other_players_next_level(p): the +0x808 of the first other player
    /// in state 1, 4 or 5, else p's own.
    pub fn other_players_next_level(&self, p: usize) -> i32 {
        (0..4).find(|&k| k != p && matches!(self.pr(k).i32_(o::STATE), 1 | 4 | 5)).map_or(self.pr(p).i32_(0x808), |k| self.pr(k).i32_(0x808))
    }

    /// AudioWelcome / AudioWelcomeBack(p, with name): 0xc0083 / 0xc0084.
    fn audio_welcome(&mut self, p: usize, back: bool, named: bool) {
        let id = if back { 0xc_0084 } else { 0xc_0083 };
        if named {
            self.audio_with_name(10.0, id, p, -1, -1);
        } else {
            let pan = self.sounds.pantab.get(p).copied().unwrap_or(0x7f);
            self.vque_add(10.0, super::audio_queue::QueEntry::new(id, 0xe0, pan, 2));
        }
    }

    /// init_player_change(r): a hidden name selects at once; otherwise
    /// the class choice (state 4) from the current class.
    fn init_player_change(&mut self, ps: &mut PSelect, p: usize, t: &SelTables, cx: &mut SelectCtx) -> R<()> {
        let pdats = cx.pdats;
        let class = self.classes[p].clone();
        if self.set_hidden_player(p, t, Some(&class))? {
            let class = self.pr(p).i32_(o::CLASS);
            self.player_selected(ps, p, class, pdats)?;
            Self::init_blit_fx(ps, p, 2, 5);
            Self::init_blit_fx(ps, p, 3, 7);
            Self::init_blit_fx(ps, p, 6, 1);
            Self::init_blit_fx(ps, p, 5, 1);
            return Ok(());
        }
        let class = self.pr(p).i32_(o::CLASS);
        let ty = self.limit_seltype(p, class, 0);
        self.players[p].set_i32(o::SELTYPE, ty);
        self.players[p].set_i32(o::SEL, 4);
        Ok(())
    }

    // ---- name --------------------------------------------------------

    /// InitGetName(p): the typed length (up to 7 bytes; at 5+ the sixth
    /// becomes the cursor letter), the cursor '@' otherwise.
    fn init_get_name(&mut self, p: usize) {
        let r = &mut self.players[p];
        r.set_i32(o::NAME_DONE, 0);
        let len = r.0[o::NAME..o::NAME + 7].iter().filter(|&&c| c != 0).count() as i32;
        if len < 5 {
            r.set_i32(o::NAME_LEN, len);
            r.set_i32(o::NAME_CHAR, 0x40);
        } else {
            r.set_i32(o::NAME_LEN, 5);
            let c = r.0[o::NAME + 5] as i8;
            r.set_i32(o::NAME_CHAR, i32::from(c));
            r.0[o::NAME + 5] = 0;
        }
    }

    /// get_initials(p, typed): up/down (repeat edges 0x30 / 0xc0) cycle
    /// '0'..'9', '@' (end), 'A'..'Z', '_'; left / Back erases; right or
    /// accept enters the letter ('@' finishes on accept, '<' erases).
    /// NATIVE: `typed` (keyboard letters, labelled) enters a letter as if
    /// cycled to and accepted. True once six letters are in.
    fn get_initials(&mut self, ps: &mut PSelect, p: usize, typed: Option<char>) -> bool {
        if let Some(c) = typed {
            // NATIVE: the letter goes in as if cycled to and accepted; the
            // frame's pad edges (WASD share the keyboard) are ignored.
            self.players[p].set_i32(o::NAME_CHAR, c.to_ascii_uppercase() as i32);
            let len = self.pr(p).i32_(o::NAME_LEN);
            if len + 1 < 7 {
                let at = o::NAME + len as usize;
                self.players[p].0[at] = c.to_ascii_uppercase() as u8;
                self.players[p].0[at + 1] = 0;
            }
            self.players[p].set_i32(o::NAME_LEN, len + 1);
            self.players[p].set_i32(o::NAME_CHAR, 0x40);
            self.audio_click(p, 1);
            self.draw_initials(ps, p);
            return 5 < self.pr(p).i32_(o::NAME_LEN);
        }
        let accept = self.new_ctrl(0x200_0000, p);
        let rep = self.sel_ctl(p, 0xc);
        if !accept {
            let c = self.pr(p).i32_(o::NAME_CHAR);
            if rep & 0x30 != 0 {
                let n = match c {
                    0x40 => 0x41,
                    0x39 => 0x40,
                    0x5a => 0x5f,
                    0x5f => 0x30,
                    c => c + 1,
                };
                self.players[p].set_i32(o::NAME_CHAR, n);
                self.audio_cursor_v();
            }
            let c = self.pr(p).i32_(o::NAME_CHAR);
            if rep & 0xc0 != 0 {
                let n = match c {
                    0x40 => 0x39,
                    0x30 => 0x5f,
                    0x41 => 0x40,
                    0x5f => 0x5a,
                    c => c - 1,
                };
                self.players[p].set_i32(o::NAME_CHAR, n);
                self.audio_cursor_v();
            }
        }
        if self.new_ctrl(3, p) || self.new_ctrl(0x800_0000, p) {
            let len = self.pr(p).i32_(o::NAME_LEN);
            if 0 < len {
                let at = o::NAME + (len - 1) as usize;
                let c = self.players[p].0[at];
                self.players[p].set_i32(o::NAME_LEN, len - 1);
                self.players[p].set_i32(o::NAME_CHAR, i32::from(c as i8));
                self.players[p].0[at] = 0;
                self.audio_cursor_h();
            }
        }
        if accept || self.new_ctrl(0xc, p) {
            let c = self.pr(p).i32_(o::NAME_CHAR);
            let mut kept = false;
            if c == 0x40 {
                if accept {
                    self.players[p].set_i32(o::NAME_LEN, 6);
                } else {
                    kept = true;
                }
            } else if c != 0x3c {
                let len = self.pr(p).i32_(o::NAME_LEN);
                if len + 1 < 7 {
                    let at = o::NAME + len as usize;
                    self.players[p].0[at] = c as u8;
                    self.players[p].0[at + 1] = 0;
                }
                self.players[p].set_i32(o::NAME_LEN, len + 1);
            } else {
                let len = self.pr(p).i32_(o::NAME_LEN);
                if 0 < len {
                    self.players[p].set_i32(o::NAME_LEN, len - 1);
                    self.players[p].0[o::NAME + (len - 1) as usize] = 0;
                }
            }
            if !kept {
                self.players[p].set_i32(o::NAME_CHAR, 0x40);
                self.audio_click(p, 1);
            }
        }
        self.draw_initials(ps, p);
        5 < self.pr(p).i32_(o::NAME_LEN)
    }

    /// get_initials' drawing: the letters at hs_name_column[p] − 0x1e, 0x12
    /// apart, y 0x154, scale 0.9 font 7 in flash_pal[p]; the cursor letter
    /// blinking (vb_counter & 0x10) white / 0x404040; '_' fills to six.
    fn draw_initials(&mut self, ps: &mut PSelect, p: usize) {
        let mut x = HS_NAME_COLUMN[p] - 0x1e;
        let len = self.pr(p).i32_(o::NAME_LEN).clamp(0, 7) as usize;
        let scale = f32::from_bits(0x3f66_6666);
        for k in 0..len {
            let c = (self.pr(p).0[o::NAME + k] as char).to_string();
            self.sel_text(ps, scale, x - 4, 0x154, 7, FLASH_PAL[p], &c);
            x += 0x12;
        }
        let mut k = len;
        if k < 6 {
            let c = ((self.pr(p).i32_(o::NAME_CHAR) as u8) as char).to_string();
            let col = if self.vb_counter & 0x10 != 0 { 0xffffff } else { 0x404040 };
            self.sel_text(ps, scale, x - 4, 0x154, 7, col, &c);
            x += 0x12;
        }
        while {
            k += 1;
            k < 6
        } {
            self.sel_text(ps, scale, x - 4, 0x154, 7, 0xffffff, "_");
            x += 0x12;
        }
    }

    /// DoGetName(p): get_initials until six letters, then a 0x3c-vblank
    /// flash of the name; an empty name takes default_names[RandInt(16)].
    /// 1 when done.
    fn do_get_name(&mut self, ps: &mut PSelect, p: usize, t: &SelTables, typed: Option<char>) -> R<i32> {
        if self.pr(p).i32_(o::NAME_DONE) == 0 {
            if self.get_initials(ps, p, typed) {
                self.players[p].set_i32(o::NAME_DONE, 1);
                self.players[p].set_u16(o::NAME_TIMER, 0x3c);
                if self.pr(p).0[o::NAME] == 0 {
                    let k = self.rng.rand_int(16).ok_or_else(|| super::Unsupported("RandInt(16)".into()))? as usize;
                    let n = t.default_names.get(k).cloned().unwrap_or_default();
                    let b = n.as_bytes();
                    for (i, c) in self.players[p].0[o::NAME..o::NAME + 8].iter_mut().enumerate() {
                        *c = b.get(i).copied().unwrap_or(0);
                    }
                }
            }
            return Ok(0);
        }
        let t0 = self.pr(p).u16_(o::NAME_TIMER);
        let left = i32::from(t0) - (self.globals.vb_elapsed & 0xffff);
        self.players[p].set_u16(o::NAME_TIMER, left as u16);
        if (left << 16) < 0 {
            self.players[p].set_u16(o::NAME_TIMER, 0);
            return Ok(1);
        }
        if self.pr(p).u16_(o::NAME_TIMER) & 0x10 != 0 {
            let name = self.player_name(p);
            self.sel_text(ps, 0.75, -HS_NAME_CX[p], 0x154, 7, FLASH_PAL[p], &name);
        }
        Ok(0)
    }

    fn player_name(&self, p: usize) -> String {
        let b = &self.pr(p).0[o::NAME..o::NAME + 8];
        String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(8)]).into_owned()
    }

    /// DrawTextScaled (the frame's HUD messages).
    #[allow(clippy::too_many_arguments)]
    fn sel_text(&mut self, _ps: &mut PSelect, scale: f32, x: i32, y: i32, font: u32, colour: u32, text: &str) {
        self.hud.draw_text(scale, x, y, font, colour, text);
    }
}

/// do_player_select's inputs from the host.
pub struct SelectCtx<'a> {
    pub pdats: &'a Pdats,
    pub card: &'a mut dyn SaveCard,
    /// NATIVE: keyboard letters typed for each player's name entry.
    pub typed: [Option<char>; 4],
}

impl Game {
    /// init_player_select(mode): 0 from the title, 1 from the Tower menu
    /// (TowerSetStartPos, PlayersRestoreHealth), 2 after a realm (finished
    /// players go to state 2 at menu 1). Inactive slots are cleared; players
    /// in play are stored in their saves and wait in state 3.
    pub fn init_player_select(&mut self, mode: i32, pdats: &Pdats) -> R<PSelect> {
        self.bg_music_stop();
        self.globals.game_mode = 0x400b;
        self.globals.paused = 0;
        self.globals.skip_wave = 0;
        self.auto_exit_timer = 0;
        if let Some(s) = self.stage_save.as_mut() {
            s.recorded_wave = -1;
        }
        let mut ps = PSelect { mode, ..Default::default() };
        if mode == 1 {
            self.players_restore_health();
        }
        for k in 0..4 {
            if (1 << k) & self.globals.active_players == 0 {
                self.sel_inactivate(k, pdats)?;
            }
            self.players[k].set_i32(0, k as i32);
            if self.players[k].i32_(o::STATE) == 4 {
                self.players[k].set_i32(o::STATE, 1);
            }
            let st = self.players[k].i32_(o::STATE);
            if mode == 2 && st == 5 {
                self.players[k].set_i32(o::STATE, 2);
                self.players[k].set_i32(o::SEL, 1);
            }
            if self.players[k].i32_(o::STATE) == 1 {
                self.players[k].set_i32(o::STATE, 3);
                let controls = self.control_options[k];
                let t = self.class_tables.clone();
                if let Some(c) = profile::player_store_in_save(&mut self.players[k], &controls, pdats, &t)? {
                    self.control_options[k] = c;
                }
            }
        }
        if mode != 0 {
            self.audio_stop_all();
            ps.load_audio = 4;
        }
        for k in 0..4 {
            let xp = self.sel_x(k);
            for slot in 0..11 {
                let pos = self.sel_pos(slot);
                ps.blits[k][slot] = SelBlit { hidden: true, x: pos[0] + xp, y: pos[1], z: pos[2] as f32, w: -1, h: -1, ..Default::default() };
            }
            let dest = self.globals.cur_world << 8 | (self.globals.cur_level & 0xff);
            self.players[k].set_i32(0x808, dest);
            if self.globals.aux_levels & 1 == 0 {
                self.setup_tex(&mut ps, k, 0, 0, false, format!("S1_plyr{}", k + 1));
                self.setup_tex(&mut ps, k, 1, 0, false, format!("S2_plyr{}", k + 1));
                self.setup_tex(&mut ps, k, 9, 0x4000, false, "S1_border".into());
                self.setup_tex(&mut ps, k, 10, 0x4000, false, "S2_border".into());
                self.update_class_spec(&mut ps, k);
            }
            // The borders' positions (sel_bg_pos +0x6c..+0x80).
            for slot in [9, 10] {
                let pos = self.sel_pos(slot);
                let b = &mut ps.blits[k][slot];
                b.x = pos[0] + xp;
                b.y = pos[1];
                b.z = pos[2] as f32;
            }
        }
        Ok(ps)
    }

    /// do_player_select (see the module doc). Call once per frame after
    /// update_controls; the frame's blits are [`Game::select_blits`].
    pub fn do_player_select(&mut self, ps: &mut PSelect, t: &SelTables, cx: &mut SelectCtx) -> R<SelectEnd> {
        let mut all_ready = true;
        let mut any = false;
        let mut busy = 0u32;
        if 0 < ps.load_audio {
            ps.load_audio -= 1;
            if ps.load_audio == 0 {
                self.audio_select(1);
            } else {
                self.sel_glow(ps, 1.0, 0x154, 0x104, "Loading...");
            }
        }
        self.check_active_players(ps, cx.pdats)?;
        ps.dir = cx.card.directory().to_vec();
        for k in 0..4 {
            match self.pr(k).i32_(o::STATE) {
                2 => {
                    all_ready = false;
                    any = true;
                }
                3 => any = true,
                _ => {}
            }
        }
        if self.globals.active_players == 0 {
            ps.abort_req = true;
        }
        if any {
            for k in 0..4 {
                if self.serve_blits(ps, k) != 0 {
                    busy |= 1 << k;
                }
            }
            if busy != 0 {
                all_ready = false;
            }
        } else {
            Self::hide_select_blits(ps, -1, true);
        }
        if 0 < ps.save_delay {
            ps.save_delay = (ps.save_delay - self.globals.vb_elapsed).max(0);
        }
        for p in 0..4 {
            if ps.load_audio != 0 {
                all_ready = false;
                continue;
            }
            if ps.abort_req {
                continue;
            }
            let st = self.pr(p).i32_(o::STATE);
            if st != 2 {
                if st == 3 {
                    let class = self.pr(p).i32_(o::CLASS);
                    if !all_ready && (busy >> p) & 1 == 0 {
                        let x = -(self.sel_x(p) + 0x40);
                        let s = f32::from_bits(0x3f99_999a);
                        for (y, line) in [(0x8e, "Press START"), (0x9a, "to Save/Load"), (0xa6, "or Change"), (0xb2, "Character")] {
                            self.sel_text(ps, s, x, y, 0, 0xffffff, line);
                        }
                        if self.sel_ctl(p, 8) & 0x40000 != 0 {
                            self.players[p].set_i32(o::STATE, 2);
                            self.players[p].set_i32(o::SEL, 1);
                            self.players[p].set_i32(o::SEL_PREV, 1);
                        }
                    }
                    let name = if class == 2 && self.pr(p).u32_(o::DESC) == SUMNER_DESC { "SUM_NAME".to_string() } else { format!("{}_NAME", class_code(class)) };
                    self.setup_tex(ps, p, 8, 0, false, name);
                } else if st != 1 {
                    Self::init_blit_fx(ps, p, 2, 1);
                }
                continue;
            }
            self.select_column(ps, t, cx, p, &mut busy)?;
            if self.pr(p).i32_(o::STATE) == 2 {
                self.update_class_spec(ps, p);
                self.update_class_attr(ps, p, cx.pdats);
            }
        }
        let mut end = SelectEnd::Running;
        if self.globals.game_mode == 0x400b {
            let last_83 = self.globals.last_finished_world == 8 && self.globals.last_finished_level == 3;
            if last_83 {
                if all_ready || ps.abort_req {
                    for k in 0..4 {
                        self.sel_inactivate(k, cx.pdats)?;
                    }
                    return Ok(SelectEnd::Attract);
                }
            } else if ps.abort_req {
                end = SelectEnd::Title;
            }
        }
        ps.done = if all_ready { ps.done + 1 } else { 0 };
        if 8 < ps.done {
            return Ok(SelectEnd::Done);
        }
        Ok(end)
    }

    fn sel_glow(&mut self, _ps: &mut PSelect, scale: f32, x: i32, y: i32, text: &str) {
        let vb = self.vb_counter;
        let g = self.hud.glow_text(scale, x, y, text, vb);
        self.hud.push_temp(g);
    }

    /// remove_optmenu: the menu hides (+0x6c = 0); its record stays.
    fn remove_menu(ps: &mut PSelect, p: usize) {
        if let Some(m) = ps.menus[p].as_mut() {
            m.active = false;
            m.state.finishing = 0;
        }
    }

    /// The menu is up (+0x6c != 0).
    fn menu_up(ps: &PSelect, p: usize) -> bool {
        ps.menus[p].as_ref().is_some_and(|m| m.active)
    }

    /// One player's column in state 2 (do_player_select's switch on
    /// +0x2ad4).
    fn select_column(&mut self, ps: &mut PSelect, t: &SelTables, cx: &mut SelectCtx, p: usize, busy: &mut u32) -> R<()> {
        let vb = self.globals.vb_elapsed;
        let sel = self.pr(p).i32_(o::SEL);
        let set = |g: &mut Game, off: usize, v: i32| g.players[p].set_i32(off, v);
        match sel {
            0 => {
                set(self, o::SEL_PREV, 0);
                if self.globals.aux_levels & 4 != 0 {
                    // AuxLevels 4: straight in as "JOE" (0x2ad1d0).
                    set(self, o::STATE, 3);
                    self.players[p].0[o::NAME..o::NAME + 4].copy_from_slice(b"JOE\0");
                    return Ok(());
                }
                if !Self::menu_up(ps, p) {
                    self.setup_sel_menu(ps, t, p, 0);
                    if 1 < self.vmu_exists() && self.vmu_directory_exists(ps) {
                        Self::sel_set_choice(ps, p, 0x3e9);
                    }
                }
                self.sel_show_menu(ps, p);
                let r = self.do_optmenu(ps, p, true);
                self.do_sel_menu(ps, p, 0);
                match r {
                    1000 => {
                        Self::remove_menu(ps, p);
                        set(self, o::SUB, 1);
                        set(self, o::SEL, 3);
                        self.audio_cursor_select();
                    }
                    0x3e9 => {
                        Self::remove_menu(ps, p);
                        set(self, o::SUB, 1);
                        set(self, o::SEL, 5);
                        self.audio_cursor_select();
                    }
                    -1 => {
                        Self::remove_menu(ps, p);
                        self.sel_leave(ps, p, cx.pdats)?;
                    }
                    _ => {}
                }
            }
            1 => {
                set(self, o::SEL_PREV, 1);
                if !Self::menu_up(ps, p) {
                    self.setup_sel_menu(ps, t, p, 1);
                    if self.vmu_exists() < 2 || !self.vmu_directory_exists(ps) {
                        Self::sel_set_choice(ps, p, 0x3ed);
                    }
                    if self.globals.last_finished_world == 8 && self.globals.last_finished_level == 3 {
                        for a in [0x3eb, 0x3e9, 0x3ec] {
                            Self::sel_set_inactive(ps, p, a);
                        }
                    }
                }
                self.sel_show_menu(ps, p);
                let r = self.do_optmenu(ps, p, true);
                self.do_sel_menu(ps, p, 1);
                let saved = i32::from(self.pr(p).u8_(o::SAVED) != 0);
                match r {
                    0x3ea => {
                        Self::remove_menu(ps, p);
                        set(self, o::SEL, 10);
                    }
                    0x3e9 => {
                        Self::remove_menu(ps, p);
                        set(self, o::SUB, saved);
                        set(self, o::SEL, 5);
                    }
                    0x3eb => {
                        Self::remove_menu(ps, p);
                        self.init_player_change(ps, p, t, cx)?;
                    }
                    0x3ec => {
                        Self::remove_menu(ps, p);
                        set(self, o::SUB, saved);
                        set(self, o::SEL, 2);
                    }
                    0x3ed => {
                        self.audio_cursor_select();
                        Self::remove_menu(ps, p);
                        set(self, o::STATE, 3);
                    }
                    _ => {}
                }
            }
            2 => {
                if self.pr(p).i32_(o::SUB) == 0 {
                    if !Self::menu_up(ps, p) {
                        self.setup_sel_menu(ps, t, p, 0xf);
                    }
                    self.sel_show_menu(ps, p);
                    let r = self.do_optmenu(ps, p, true);
                    self.do_sel_menu(ps, p, 4);
                    match r {
                        0x3ee => {
                            Self::remove_menu(ps, p);
                            set(self, o::SUB, 1);
                        }
                        -1 | 0x3ef => {
                            Self::remove_menu(ps, p);
                            set(self, o::SEL, 1);
                        }
                        _ => {}
                    }
                } else {
                    self.sel_leave(ps, p, cx.pdats)?;
                }
            }
            3 => {
                let sub = self.pr(p).i32_(o::SUB);
                if (0..2).contains(&sub) {
                    self.init_get_name(p);
                    set(self, o::SUB, 2);
                }
                if self.do_get_name(ps, p, t, cx.typed[p])? != 0 {
                    self.init_player_change(ps, p, t, cx)?;
                    let region = self.players[p].0[o::NAME..o::NAME + SAVE_LEN].to_vec();
                    self.players[p].0[o::BACKUP..o::BACKUP + SAVE_LEN].copy_from_slice(&region);
                }
                let r = self.do_optmenu(ps, p, false);
                self.do_sel_menu(ps, p, 2);
                if r == -1 {
                    let prev = self.pr(p).i32_(o::SEL_PREV);
                    set(self, o::SEL, prev);
                }
            }
            4 => self.select_class(ps, t, cx, p, busy)?,
            5 => {
                if self.pr(p).i32_(o::SUB) == 0 {
                    if !Self::menu_up(ps, p) {
                        self.setup_sel_menu(ps, t, p, 0xf);
                    }
                    self.sel_show_menu(ps, p);
                    let r = self.do_optmenu(ps, p, true);
                    match r {
                        0x3ee => {
                            Self::remove_menu(ps, p);
                            set(self, o::SUB, 1);
                        }
                        -1 | 0x3ef => {
                            Self::remove_menu(ps, p);
                            let prev = self.pr(p).i32_(o::SEL_PREV);
                            set(self, o::SEL, prev);
                        }
                        _ => {}
                    }
                    self.do_sel_menu(ps, p, 5);
                    if self.pr(p).i32_(o::SUB) == 0 {
                        return Ok(());
                    }
                }
                self.choose_card(ps, t, p, 5)?;
            }
            6 | 0xb | 0xc => {
                // Mount (6) / format (0xb) / create the save file (0xc):
                // sub 0 asks Yes/No, 1..2 wait for LoadGauntWorldDone, 3
                // runs saveMount / MemCardCreateGaunt (native: succeed,
                // the directory now exists), then the result shows for
                // 0x78 vblanks (≥ 1000: "Insufficient Free Space", 0x460).
                let sub = self.pr(p).i32_(o::SUB);
                if sub == 0 {
                    if !Self::menu_up(ps, p) {
                        self.setup_sel_menu(ps, t, p, 0xf);
                    }
                    self.sel_show_menu(ps, p);
                    match self.do_optmenu(ps, p, true) {
                        0x3ee => {
                            Self::remove_menu(ps, p);
                            set(self, o::SUB, 1);
                        }
                        -1 | 0x3ef => {
                            Self::remove_menu(ps, p);
                            if sel == 6 {
                                set(self, o::SEL, 5);
                                set(self, o::SUB, 1);
                            } else {
                                set(self, o::SEL, 10);
                            }
                        }
                        _ => {}
                    }
                } else if (1..3).contains(&sub) {
                    set(self, o::SUB, sub + 1);
                } else if sub == 3 {
                    ps.dir_created = true;
                    set(self, o::SUB, 4);
                } else if sub < 1000 {
                    if sub < 0 {
                        let v = sub - vb;
                        set(self, o::SUB, v);
                        if v < -0x77 {
                            let prev = self.pr(p).i32_(o::SEL_PREV);
                            set(self, o::SEL, prev);
                            set(self, o::SUB, 0);
                        }
                    } else {
                        let v = sub + vb;
                        set(self, o::SUB, v);
                        if 0x77 < v {
                            if sel != 6 {
                                set(self, o::SEL, 0xd);
                                self.setup_file_entries(ps, p, true);
                                set(self, o::SUB, 0);
                            } else {
                                set(self, o::SEL, 5);
                                set(self, o::SUB, 1);
                            }
                        }
                    }
                } else {
                    let v = sub + vb;
                    set(self, o::SUB, v);
                    if 0x45f < v {
                        set(self, o::SUB, 0);
                        let prev = self.pr(p).i32_(o::SEL_PREV);
                        set(self, o::SEL, prev);
                    }
                }
                self.do_sel_menu(ps, p, if sel == 0xb { 0xb } else if sel == 6 { 6 } else { 0xc });
            }
            7 => {
                self.do_sel_menu(ps, p, 7);
                let v = self.pr(p).i32_(o::SUB) + vb;
                set(self, o::SUB, v);
                if 0x77 < v {
                    let prev = self.pr(p).i32_(o::SEL_PREV);
                    set(self, o::SEL, prev);
                    set(self, o::SUB, 0);
                }
            }
            8 => {
                if !Self::menu_up(ps, p) {
                    self.setup_sel_menu(ps, t, p, 8);
                }
                if let Some(m) = ps.menus[p].as_mut() {
                    m.count = 0;
                }
                self.sel_show_menu(ps, p);
                let r = self.do_optmenu(ps, p, true);
                self.setup_file_entries(ps, p, false);
                self.do_sel_menu(ps, p, 8);
                if r == -1 {
                    Self::remove_menu(ps, p);
                    set(self, o::SEL, 5);
                } else if 1000 <= r {
                    let k = r - 1000;
                    if self.verify_vmu_file_ok(p, k) {
                        Self::remove_menu(ps, p);
                        set(self, o::SEL, 9);
                        set(self, o::FILE, k);
                        set(self, o::FILE2, k);
                        set(self, o::SUB, 0);
                    }
                }
            }
            9 => {
                let sub = self.pr(p).i32_(o::SUB);
                if sub < 0 {
                    let v = sub - vb;
                    set(self, o::SUB, v);
                    if v < -0x77 {
                        let prev = self.pr(p).i32_(o::SEL_PREV);
                        set(self, o::SEL, prev);
                        set(self, o::SUB, -1);
                    }
                } else if sub < 3 {
                    set(self, o::SUB, sub + 1);
                } else if sub == 3 {
                    let k = self.pr(p).i32_(o::FILE);
                    let ok = self.player_load_save_file(p, k, cx)?;
                    set(self, o::SUB, if ok { 4 } else { -1 });
                } else {
                    let v = sub + vb;
                    set(self, o::SUB, v);
                    if 0x77 < v {
                        self.init_player_change(ps, p, t, cx)?;
                        set(self, o::CHANGING, 1);
                        set(self, o::SUB, -1);
                    }
                }
                self.do_sel_menu(ps, p, 9);
            }
            10 => {
                self.choose_card(ps, t, p, 10)?;
            }
            0xd => {
                if !Self::menu_up(ps, p) {
                    self.setup_sel_menu(ps, t, p, 0xd);
                }
                self.sel_show_menu(ps, p);
                self.setup_file_entries(ps, p, true);
                let r = self.do_optmenu(ps, p, true);
                self.do_sel_menu(ps, p, 0xd);
                if r == -1 {
                    Self::remove_menu(ps, p);
                    set(self, o::SEL, 10);
                } else if 1000 <= r && ps.save_delay == 0 {
                    Self::remove_menu(ps, p);
                    set(self, o::SEL, 0xe);
                    set(self, o::FILE, r - 1000);
                    set(self, o::FILE2, r - 1000);
                    set(self, o::SUB, 0);
                    ps.save_delay = 0x1e;
                }
            }
            0xe => {
                let sub = self.pr(p).i32_(o::SUB);
                if sub == 0 {
                    let k = self.pr(p).i32_(o::FILE);
                    let used = usize::try_from(k).ok().and_then(|k| ps.dir.get(k)).is_some_and(|f| f.is_some());
                    if used {
                        // Overwrite? (Yes / No).
                        if !Self::menu_up(ps, p) {
                            self.setup_sel_menu(ps, t, p, 0xf);
                        }
                        self.sel_show_menu(ps, p);
                        match self.do_optmenu(ps, p, true) {
                            0x3ee => {
                                Self::remove_menu(ps, p);
                                set(self, o::SUB, 1);
                            }
                            -1 | 0x3ef => {
                                Self::remove_menu(ps, p);
                                set(self, o::SEL, 0xd);
                                set(self, o::FILE2, -1);
                            }
                            _ => {}
                        }
                    } else {
                        set(self, o::SUB, 1);
                    }
                } else if sub < 0 {
                    let v = sub - vb;
                    set(self, o::SUB, v);
                    if v < -0x77 {
                        let prev = self.pr(p).i32_(o::SEL_PREV);
                        set(self, o::SEL, prev);
                        set(self, o::SUB, 0);
                    }
                } else if sub < 3 {
                    set(self, o::SUB, sub + 1);
                } else if sub == 3 {
                    let k = self.pr(p).i32_(o::FILE);
                    let ok = self.player_write_save_file(p, k, cx)?;
                    set(self, o::SUB, if ok { 4 } else { -1 });
                    if ok {
                        ps.dir = cx.card.directory().to_vec();
                    }
                } else {
                    let v = sub + vb;
                    set(self, o::SUB, v);
                    if 0x77 < v {
                        set(self, o::SUB, 0);
                        let prev = self.pr(p).i32_(o::SEL_PREV);
                        set(self, o::SEL, prev);
                        self.setup_sel_menu(ps, t, p, prev);
                        Self::sel_set_choice(ps, p, 0x3ed);
                    }
                }
                self.do_sel_menu(ps, p, 0xe);
            }
            _ => {}
        }
        Ok(())
    }

    /// The `joined_r0x001888a8` exit: another player stays (this one
    /// leaves), else the select screen aborts to the title.
    fn sel_leave(&mut self, ps: &mut PSelect, p: usize, pdats: &Pdats) -> R<()> {
        if self.globals.active_players & !(1 << p) != 0 {
            self.sel_inactivate(p, pdats)
        } else {
            let demo = self.globals.demo_mode != 0;
            profile::clear_player(&mut self.players[p], true, demo, pdats)?;
            ps.abort_req = true;
            Ok(())
        }
    }

    /// States 5 / 10: the card menu (" SLOT  1"); the native card always
    /// holds the Gauntlet directory, so loading goes to the file list (8)
    /// or "no save data" (7), saving to the file list (0xd).
    fn choose_card(&mut self, ps: &mut PSelect, t: &SelTables, p: usize, state: i32) -> R<()> {
        if !Self::menu_up(ps, p) {
            self.setup_sel_menu(ps, t, p, state);
        }
        if let Some(m) = ps.menus[p].as_mut() {
            m.count = 0;
        }
        self.sel_show_menu(ps, p);
        let mut r = self.do_optmenu(ps, p, true);
        if self.vmu_exists() < 1 {
            r = -1;
        }
        self.do_sel_menu(ps, p, state);
        if r == -1 {
            Self::remove_menu(ps, p);
            let prev = self.pr(p).i32_(o::SEL_PREV);
            self.players[p].set_i32(o::SEL, prev);
            return Ok(());
        }
        if 1000 <= r {
            Self::remove_menu(ps, p);
            let k = r - 1000;
            self.players[p].set_i32(o::PORT, k >> 2);
            self.players[p].set_i32(o::SLOT, k & 3);
            let n = self.setup_file_entries(ps, p, true);
            let to = match (state, n) {
                (5, -1) => (6, 0),
                (5, n) if n < 1 => (7, 1),
                (5, _) => (8, -2),
                (_, -2) => (10, -2),
                (_, -1) => (0xb, 0),
                (_, 0) => (0xc, 0),
                _ => (0xd, -2),
            };
            self.players[p].set_i32(o::SEL, to.0);
            if 0 <= to.1 {
                self.players[p].set_i32(o::SUB, to.1);
            }
        }
        Ok(())
    }

    /// vmu_directory_exists (saveExists of the Gauntlet directory): the
    /// native card has it once any file is saved or after MemCardCreateGaunt.
    fn vmu_directory_exists(&self, ps: &PSelect) -> bool {
        ps.dir_created || ps.dir.iter().any(|f| f.is_some())
    }

    /// setup_file_entries(r, saving): 0 without the Gauntlet directory;
    /// otherwise the card's eight files as entries (action 1000 + k, gap
    /// 4); loading disables "Empty" files; files another player in play
    /// uses are disabled. 1 when saving or when a file exists.
    fn setup_file_entries(&mut self, ps: &mut PSelect, p: usize, saving: bool) -> i32 {
        if !self.vmu_directory_exists(ps) {
            return 0;
        }
        let mut some = false;
        let mut out = Vec::new();
        for k in 0..8 {
            let f = ps.dir.get(k).cloned().flatten();
            let name = f.as_ref().map_or_else(|| "Empty".to_string(), |f| f.name.clone());
            let mut state = 0;
            if !saving {
                if f.is_none() {
                    state = -1;
                } else {
                    some = true;
                }
            }
            if !self.verify_vmu_file_ok(p, k as i32) {
                state = -1;
            }
            out.push(MenuEntry { label: name, action: 1000 + k as i32, gap: 4, value: None, colour: 0, mark: 0, state });
        }
        ps.files[p] = out.clone();
        if let Some(m) = ps.menus[p].as_mut().filter(|m| m.menu.kind == 8 || m.menu.kind == 0xd) {
            m.menu.entries = out;
        }
        i32::from(saving || some)
    }

    /// verify_vmu_file_ok(r, k): no other player in play uses file k.
    fn verify_vmu_file_ok(&self, p: usize, k: i32) -> bool {
        !(0..4).any(|q| q != p && self.pr(q).i32_(o::STATE) != 0 && self.pr(q).i32_(o::FILE2) == k)
    }

    /// PlayerLoadSaveFile(p, k): the region into +0x99c, player_get_from_save
    /// (the saved class), marked saved, the backup copy, +0x1910..+0x1a10
    /// &= 0xf0, change_player to the class.
    fn player_load_save_file(&mut self, p: usize, k: i32, cx: &mut SelectCtx) -> R<bool> {
        self.players[p].set_i32(o::FILE2, k);
        let Some(region) = usize::try_from(k).ok().and_then(|k| cx.card.load(k)) else { return Ok(false) };
        if region.len() != SAVE_LEN {
            return Ok(false);
        }
        self.players[p].0[o::NAME..o::NAME + SAVE_LEN].copy_from_slice(&region);
        let t = self.class_tables.clone();
        self.control_options[p] = profile::player_get_from_save(&mut self.players[p], None, cx.pdats, &t)?;
        self.players[p].set_u8(o::SAVED, 1);
        self.players[p].0[o::BACKUP..o::BACKUP + SAVE_LEN].copy_from_slice(&region);
        for b in &mut self.players[p].0[0x1910..0x1a10] {
            *b &= 0xf0;
        }
        let class = self.pr(p).i32_(o::CLASS);
        self.change_player(p, class, cx.pdats)?;
        Ok(true)
    }

    /// PlayerWriteSaveFile(p, k): player_store_in_save, the region to file k.
    fn player_write_save_file(&mut self, p: usize, k: i32, cx: &mut SelectCtx) -> R<bool> {
        self.players[p].set_i32(o::FILE2, k);
        let controls = self.control_options[p];
        let t = self.class_tables.clone();
        if let Some(c) = profile::player_store_in_save(&mut self.players[p], &controls, cx.pdats, &t)? {
            self.control_options[p] = c;
        }
        let region = self.players[p].0[o::NAME..o::NAME + SAVE_LEN].to_vec();
        let ok = usize::try_from(k).is_ok_and(|k| cx.card.save(k, &region));
        if ok {
            self.players[p].set_u8(o::SAVED, 1);
        }
        Ok(ok)
    }

    /// State 4: class and colour (do_optmenu non-vertical: 3/4 step the
    /// type, 7/8 the colour, −1 back; accept selects a useable class).
    fn select_class(&mut self, ps: &mut PSelect, t: &SelTables, cx: &mut SelectCtx, p: usize, busy: &mut u32) -> R<()> {
        let _ = t;
        let mut colour = self.pr(p).i32_(o::COLOUR);
        let old = self.pr(p).i32_(o::SELTYPE);
        let r = self.do_optmenu(ps, p, false);
        self.do_sel_menu(ps, p, 3);
        let (mut step, mut changed) = (0, false);
        match r {
            3 => {
                step = -1;
                changed = true;
            }
            4 => {
                step = 1;
                changed = true;
            }
            7 => {
                colour = if 3 <= colour { 0 } else { colour + 1 };
                changed = true;
            }
            8 => {
                colour = if colour <= 0 { 3 } else { colour - 1 };
                changed = true;
            }
            -1 => {
                let prev = self.pr(p).i32_(o::SEL_PREV);
                self.players[p].set_i32(o::SEL, prev);
                if prev == 0 {
                    self.new_player(ps, p, cx.pdats)?;
                }
                ps.blits[p][3].hidden = true;
            }
            _ => {}
        }
        let mut ty = old + step;
        if changed || ty != old || self.pr(p).i32_(o::W100) != 2 {
            self.players[p].set_i32(o::W100, 2);
            ty = self.limit_seltype(p, ty, step);
            if changed || ty != old {
                self.players[p].set_i32(o::SELTYPE, ty);
                let useable = self.useable_class(p);
                let col = if useable { COLOUR_CODES[colour.clamp(0, 3) as usize].to_string() } else { "shadw".to_string() };
                self.audio_cursor_char();
                if ty == 0x10 {
                    self.setup_tex(ps, p, 4, 0, false, "S12_SUM".into());
                } else {
                    self.setup_tex(ps, p, 4, 0, false, format!("S12_{}_{}", class_code(ty), col));
                }
                Self::init_blit_fx(ps, p, 4, 2);
                if self.serve_blits(ps, p) != 0 {
                    *busy |= 1 << p;
                }
            }
            self.players[p].set_i32(o::SELTYPE, ty);
            self.players[p].set_i32(o::COLOUR, colour);
        }
        if self.new_ctrl(0x200_0000, p) {
            if !self.useable_class(p) {
                self.audio_buzzer();
                return Ok(());
            }
            self.audio_cursor_select();
            let ty = self.pr(p).i32_(o::SELTYPE);
            if self.pr(p).i32_(o::CHANGING) == 0 {
                self.player_selected(ps, p, ty, cx.pdats)?;
            } else {
                self.change_player(p, ty, cx.pdats)?;
                self.players[p].set_i32(o::CHANGING, 1);
                self.players[p].set_i32(o::SEL, 1);
                self.setup_sel_menu(ps, t, p, 1);
                Self::sel_set_choice(ps, p, 0x3ed);
                self.set_selected_blit(ps, p, ty);
                let named = !(ty == 0x10 || self.pr(p).u32_(o::DESC) != 0);
                let back = self.pr(p).i32_(o::EXP) != 0;
                self.audio_welcome(p, back, named);
            }
            Self::init_blit_fx(ps, p, 2, 5);
            Self::init_blit_fx(ps, p, 3, 7);
            Self::init_blit_fx(ps, p, 6, 1);
            Self::init_blit_fx(ps, p, 5, 1);
        }
        Ok(())
    }
}

/// player_desc_uc 0x220db8 (WAR … HYE, SUM) and player_color_uc 0x220e10.
const CLASS_CODES: [&str; 17] = ["WAR", "VAL", "WIZ", "ARC", "DWF", "KNI", "SOR", "JES", "MIN", "FAL", "JAC", "TIG", "OGR", "UNI", "MED", "HYE", "SUM"];
const COLOUR_CODES: [&str; 4] = ["YEL", "BLU", "RED", "GRE"];

fn class_code(c: i32) -> &'static str {
    CLASS_CODES.get(c.clamp(0, 16) as usize).copied().unwrap_or("WAR")
}

/// sel_msg_scale 0x24a448, sel_msg_font, sel_msg_dy, sel_confirm_y,
/// sel_namemsg_y, sel_msg_y, sel_vmumsg_dy; saveFileSize 0x47400.
const MSG_SCALE: f32 = 1.2;
const MSG_FONT: u32 = 0;
const MSG_DY: i32 = 10;
const CONFIRM_Y: i32 = 100;
const NAMEMSG_Y: i32 = 180;
const MSG_Y: i32 = 192;
const VMUMSG_DY: i32 = -8;
const SAVE_FILE_SIZE: i32 = 0x47400;
/// sel_attr_*: level y 292 / scale 1.2 / font 0, text scale 0.5 / font 7 /
/// dy 16, the value column +84, the first row y 162 (0x249da4).
const ATTR_LVL_Y: i32 = 292;
const ATTR_TEXT_SCALE: f32 = 0.5;
const ATTR_TEXT_FONT: u32 = 7;
const ATTR_TEXT_DY: i32 = 16;
const ATTR_POS: i32 = 84;
const ATTR_Y0: i32 = 162;

impl Game {
    /// show_optmenu for player p's menu (drawn into the frame).
    fn sel_show_menu(&mut self, ps: &mut PSelect, p: usize) {
        let _ = (ps, p);
    }

    /// The temp blit of a button texture (MBNewTempBlit).
    fn sel_button(&mut self, name: &str, x: i32, y: i32, size: i32) {
        let b = self.hud.make_pub(self.hud.lookup(name), x, y, size, size);
        self.hud.push_temp([b]);
    }

    /// do_sel_menu(p, kind): the column's messages and button rows.
    fn do_sel_menu(&mut self, ps: &mut PSelect, p: usize, kind: i32) {
        let xp = self.sel_x(p);
        let cx = -(xp + 0x40);
        let bs = (MSG_SCALE * 16.0) as i32;
        let mut select_row = true;
        let mut back_row = 0;
        let menu_y = ps.menus[p].as_ref().map_or(0, |m| m.menu.y);
        let sub = self.pr(p).i32_(o::SUB);
        let lines = |g: &mut Game, ps: &mut PSelect, y0: i32, step: i32, l: &[&str]| {
            for (k, t) in l.iter().enumerate() {
                g.sel_text(ps, MSG_SCALE, cx, y0 + step * k as i32, MSG_FONT, 0xffffff, t);
            }
        };
        const KEEP: [&str; 4] = ["Memory", "Card(PS2)", "or power", "off system!"];
        match kind {
            0 | 1 => {
                if kind == 0 {
                    back_row = 1;
                }
                if let Some(m) = ps.menus[p].as_mut() {
                    let mut e = std::mem::take(&mut m.menu.entries);
                    self.sel_activate_loadsave(&mut e, p);
                    if let Some(m) = ps.menus[p].as_mut() {
                        m.menu.entries = e;
                    }
                }
                if self.globals.demo_mode == 0 && self.vmu_exists() < 1 {
                    lines(self, ps, MSG_Y, MSG_DY, &["Insert a", "Memory", "Card(PS2) to", "Load or Save", "Characters."]);
                }
            }
            2 => {
                let s = f32::from_bits(0x3f4c_cccd);
                for (y, t) in [(0x40, "Enter"), (0x5a, "Your"), (0x74, "Name")] {
                    self.sel_text(ps, s, cx, y, 6, 0xffffff, t);
                }
                let x = bs + 10 + xp;
                let tx = x + bs + 8;
                self.sel_button("BUTTON_U", x - bs, NAMEMSG_Y, bs);
                self.sel_button("BUTTON_D", x, NAMEMSG_Y, bs);
                self.sel_text(ps, MSG_SCALE, tx, NAMEMSG_Y + 4, MSG_FONT, 0xffffff, "Change");
                self.sel_button("BUTTON_L", x - bs, NAMEMSG_Y + 0x14, bs);
                self.sel_button("BUTTON_R", x, NAMEMSG_Y + 0x14, bs);
                self.sel_text(ps, MSG_SCALE, tx, NAMEMSG_Y + 0x18, MSG_FONT, 0xffffff, "Edit");
                self.sel_button("BUTTON_X", x, NAMEMSG_Y + 0x28, bs);
                self.sel_text(ps, MSG_SCALE, tx, NAMEMSG_Y + 0x2c, MSG_FONT, 0xffffff, "Accept");
                self.sel_button("BUTTON_TRI", x, NAMEMSG_Y + 0x3c, bs);
                self.sel_text(ps, MSG_SCALE, tx, NAMEMSG_Y + 0x40, MSG_FONT, 0xffffff, "Cancel");
                select_row = false;
            }
            3 => {
                let x = bs + 10 + xp;
                self.sel_button("BUTTON_L", x - bs, NAMEMSG_Y + 0x34, bs);
                self.sel_button("BUTTON_R", x, NAMEMSG_Y + 0x34, bs);
                self.sel_text(ps, MSG_SCALE, x + bs + 8, NAMEMSG_Y + 0x38, MSG_FONT, 0xffffff, "Change");
                select_row = self.useable_class(p);
            }
            4 => {
                if self.globals.demo_mode == 0 {
                    lines(self, ps, CONFIRM_Y, MSG_DY, &["Character", "Not Saved.", "Quit Anyway?"]);
                } else {
                    lines(self, ps, CONFIRM_Y, MSG_DY, &["Are you sure", "you want", "to Quit?"]);
                }
            }
            5 if sub == 0 => {
                let y = menu_y + VMUMSG_DY - MSG_DY * 6;
                lines(self, ps, y, MSG_DY, &["Current", "Character", "Not Saved.", "Load new", "Character", "Anyway?"]);
            }
            5 | 10 => {
                lines(self, ps, menu_y + VMUMSG_DY - MSG_DY * 3, MSG_DY, &["Choose a", "Memory", "Card(PS2)"]);
                back_row = 1;
            }
            6 => {
                let y0 = CONFIRM_Y - MSG_DY * 5;
                if sub == 0 {
                    lines(self, ps, y0, MSG_DY, &["Unformatted", "Memory", "Card(PS2)."]);
                    lines(self, ps, y0 + MSG_DY * 4, MSG_DY, &["Would you", "like to", "format it?"]);
                } else if (0..4).contains(&sub) {
                    self.sel_text(ps, MSG_SCALE, cx, CONFIRM_Y, MSG_FONT, 0xffffff, "Formatting...");
                    self.sel_text(ps, MSG_SCALE, cx, CONFIRM_Y + MSG_DY * 2, MSG_FONT, 0xffffff, "Do not remove");
                    lines(self, ps, CONFIRM_Y + MSG_DY * 3, MSG_DY, &KEEP);
                    select_row = false;
                } else {
                    lines(self, ps, CONFIRM_Y, MSG_DY, &["Format", if sub < 0 { "Failed!" } else { "Complete." }]);
                    select_row = false;
                }
            }
            7 => lines(self, ps, CONFIRM_Y, MSG_DY, &["No Gauntlet", "save data on", "Memory", "Card(PS2)."]),
            8 => {
                lines(self, ps, menu_y + VMUMSG_DY - MSG_DY * 2, MSG_DY, &["Choose a", "File to Load"]);
                back_row = 1;
            }
            9 | 0xe => {
                let load = kind != 0xe;
                if sub == 0 && !load {
                    lines(self, ps, CONFIRM_Y, MSG_DY, &["Overwrite", "Existing", "File?"]);
                } else if sub != 0 && !(0..4).contains(&sub) {
                    let title = if load { "Load" } else { "Save" };
                    lines(self, ps, CONFIRM_Y, MSG_DY, &[title, if sub < 0 { "Failed!" } else { "Complete" }]);
                    select_row = false;
                } else {
                    lines(self, ps, CONFIRM_Y, MSG_DY, &[if load { "Loading from" } else { "Writing to" }, "Memory", "Card(PS2)..."]);
                    self.sel_text(ps, MSG_SCALE, cx, CONFIRM_Y + MSG_DY * 4, MSG_FONT, 0xffffff, "Do not remove");
                    lines(self, ps, CONFIRM_Y + MSG_DY * 5, MSG_DY, &KEEP);
                    select_row = false;
                }
            }
            0xb | 0xc => {
                let y0 = CONFIRM_Y - MSG_DY * 5;
                let kb = format!("{}KB", SAVE_FILE_SIZE >> 10);
                if sub == 0 {
                    if kind == 0xb {
                        lines(self, ps, y0, MSG_DY, &["Unformatted", "Memory", "Card(PS2)."]);
                        lines(self, ps, y0 + MSG_DY * 4, MSG_DY, &["Format Memory", "Card(PS2)", "And Create", &kb, "Save File?"]);
                    } else {
                        lines(self, ps, y0, MSG_DY, &["No Gauntlet", "save data on", "Memory", "Card(PS2)."]);
                        lines(self, ps, y0 + MSG_DY * 5, MSG_DY, &["Create", &kb, "Save File?"]);
                        // NATIVE: the native card reports 8 MB less its
                        // files (saveGetFreeBytes has no host equivalent).
                        let used = ps.dir.iter().filter(|f| f.is_some()).count() as i32;
                        let free = format!("{}KB Free", (0x80_0000 - used * SAVE_FILE_SIZE) >> 10);
                        self.sel_text(ps, MSG_SCALE, cx, y0 + MSG_DY * 10, MSG_FONT, 0xffffff, &free);
                    }
                } else if (0..4).contains(&sub) {
                    lines(self, ps, CONFIRM_Y, MSG_DY, &["Creating", "Gauntlet", "Save File..."]);
                    self.sel_text(ps, MSG_SCALE, cx, CONFIRM_Y + MSG_DY * 4, MSG_FONT, 0xffffff, "Do not remove");
                    lines(self, ps, CONFIRM_Y + MSG_DY * 5, MSG_DY, &KEEP);
                    select_row = false;
                } else if sub < 1000 {
                    if sub < 0 {
                        lines(self, ps, CONFIRM_Y, MSG_DY, &["Create", "Save File", "Failed!"]);
                    } else {
                        lines(self, ps, CONFIRM_Y, MSG_DY, &["Save File", "Created."]);
                    }
                    select_row = false;
                } else {
                    lines(self, ps, CONFIRM_Y, MSG_DY, &["Insufficient", "Free Space."]);
                    select_row = false;
                }
            }
            0xd => {
                lines(self, ps, menu_y + VMUMSG_DY - MSG_DY * 3, MSG_DY, &["Choose a", "Location to", "Write to"]);
                back_row = 1;
            }
            _ => {}
        }
        if select_row {
            self.sel_button("BUTTON_X", xp + 0x14, 0xfc, bs);
            self.sel_text(ps, MSG_SCALE, xp + 0x14 + bs + 8, 0x100, MSG_FONT, 0xffffff, "Select");
        }
        if back_row != 0 {
            let y = if back_row == 2 { 0xf2 - MSG_DY } else { MSG_DY + 0x106 };
            self.sel_button("BUTTON_TRI", xp + 0x14, y, bs);
            self.sel_text(ps, MSG_SCALE, xp + 0x14 + bs + 8, y + 4, MSG_FONT, 0xffffff, "Back");
        }
    }

    /// update_class_spec(p): slot 2 the selected class's weapon art; in
    /// state 2 at menu 1 the class name, at menu 4 the type's S12 art
    /// (shadow for a locked class, its name, the "?" mark pulsing for a
    /// locked one); state 3 keeps the column; others hide it.
    fn update_class_spec(&mut self, ps: &mut PSelect, p: usize) {
        let class = self.pr(p).i32_(o::CLASS);
        self.set_selected_blit(ps, p, class);
        for k in [7, 6, 5, 8] {
            ps.blits[p][k].hidden = true;
        }
        if self.globals.game_mode != 0x400b {
            return;
        }
        match (self.pr(p).i32_(o::STATE), self.pr(p).i32_(o::SEL)) {
            (2, 1) => {
                let name = if class == 2 && self.pr(p).u32_(o::DESC) == SUMNER_DESC { "SUM_NAME".to_string() } else { format!("{}_NAME", class_code(class)) };
                self.setup_tex(ps, p, 8, 0, false, name);
            }
            (2, 0) | (2, 3) => Self::hide_select_blits(ps, p as i32, false),
            (2, 4) => {
                ps.blits[p][2].hidden = true;
                let ty = self.pr(p).i32_(o::SELTYPE);
                let useable = self.useable_class(p);
                let col = if useable { COLOUR_CODES[self.pr(p).i32_(o::COLOUR).clamp(0, 3) as usize].to_string() } else { "shadw".to_string() };
                if ty == 0x10 {
                    self.setup_tex(ps, p, 3, 0, false, "S12_SUM".into());
                } else {
                    self.setup_tex(ps, p, 3, 0, false, format!("S12_{}_{}", class_code(ty), col));
                }
                if useable {
                    self.setup_tex(ps, p, 8, 0, false, format!("{}_NAME", class_code(ty)));
                    ps.blits[p][7].hidden = true;
                } else {
                    for k in [6, 5, 8] {
                        ps.blits[p][k].hidden = true;
                    }
                    self.setup_tex(ps, p, 7, 0, false, "selscrn_questmark".into());
                    Self::init_blit_fx(ps, p, 7, 3);
                }
                ps.blits[p][2].hidden = true;
            }
            (3, _) => {}
            (2, _) => {}
            _ => Self::hide_select_blits(ps, p as i32, false),
        }
    }

    /// update_class_attr(p): in state 2 at menu 1 "Level n"; at menu 4 for
    /// a useable type the four attributes (PDAT +0x28/+0x30/+0x38/+0x40 +
    /// (level − 1) × 5 + the class bonus words, capped at 999; Sumner 999)
    /// with GameText 0xa7's names, the highest glowing over the HILITE
    /// mark, and "Level n" / "NEW".
    fn update_class_attr(&mut self, ps: &mut PSelect, p: usize, pdats: &Pdats) {
        if self.pr(p).i32_(o::LEVEL) < 1 {
            self.players[p].set_i32(o::LEVEL, 1);
        }
        if self.pr(p).i32_(o::STATE) != 2 {
            return;
        }
        let xp = self.sel_x(p);
        match self.pr(p).i32_(o::SEL) {
            1 => {
                let lv = exp_to_level(self.pr(p).i32_(o::EXP));
                self.sel_text(ps, MSG_SCALE, -(xp + 0x40), ATTR_LVL_Y, 0, 0xffffff, &format!("Level {lv}"));
            }
            4 if self.useable_class(p) => {
                let ty = self.pr(p).i32_(o::SELTYPE);
                let (vals, best, level, new) = if ty == 0x10 {
                    ([999; 4], -1, 99, true)
                } else {
                    let e = profile::CLASS_ATTS + 0x18 * ty as usize;
                    let exp = self.pr(p).i32_(e);
                    let lv = exp_to_level(exp);
                    let f = ((lv - 1) * 5) as f32;
                    let bonus = |k: usize| self.pr(p).i32_(e + 4 * k) as f32;
                    let base = |o2: usize| pdats.value(ty as u32, o2).unwrap_or(0.0);
                    let v = [
                        (f + base(0x28) + bonus(2)) as i32,
                        (f + base(0x30) + bonus(5)) as i32,
                        (f + base(0x38) + bonus(3)) as i32,
                        (f + base(0x40) + bonus(4)) as i32,
                    ]
                    .map(|x| x.min(999));
                    let mut best = 0;
                    let mut top = 0;
                    for (k, &x) in v.iter().enumerate() {
                        if top < x {
                            top = x;
                            best = k as i32;
                        }
                    }
                    (v, best, lv, exp < 1)
                };
                let mut y = ATTR_Y0;
                for (k, v) in vals.iter().enumerate() {
                    let name = self.hud.string_text(0xa7, k as i32).unwrap_or_default();
                    let w = self.hud.string_width(6, 1.0, name.as_bytes());
                    let w = (w as f32 * ATTR_TEXT_SCALE) as i32;
                    let x = xp + 0x51 - w;
                    if best == k as i32 {
                        let vb = self.vb_counter;
                        let g = self.hud.glow_text(ATTR_TEXT_SCALE, x, y - 2, &name, vb);
                        self.hud.push_temp(g);
                        let mark = self.hud.make_pub(self.hud.lookup("ATT_GLOW"), xp + ATTR_POS - 6, y - 6, 0x44, -1);
                        self.hud.push_temp([mark]);
                    } else {
                        self.sel_text(ps, ATTR_TEXT_SCALE, x, y - 2, 6, 0xffffff, &name);
                    }
                    self.sel_text(ps, ATTR_TEXT_SCALE, xp + ATTR_POS, y, ATTR_TEXT_FONT, 0xffffff, &format!("{v:03}"));
                    y += ATTR_TEXT_DY;
                }
                let text = if new { "NEW".to_string() } else { format!("Level {level}") };
                self.sel_text(ps, MSG_SCALE, -(xp + 0x40), ATTR_LVL_Y, 0, 0xffffff, &text);
            }
            _ => {}
        }
    }

    /// The frame's select-screen blits: the column slots (MB blits, alpha
    /// 0 opaque … 0xff clear) and the menus (show_optmenu).
    pub fn select_blits(&self, ps: &PSelect, t: &SelTables) -> Vec<Blit> {
        let mut out = Vec::new();
        for p in 0..4 {
            for b in &ps.blits[p] {
                if b.hidden {
                    continue;
                }
                let Some(name) = &b.tex else { continue };
                let mut bl = self.hud.make_pub(self.hud.lookup(name), b.x, b.y, b.w, b.h);
                bl.z = b.z;
                let a = (0x100 - b.alpha.clamp(0, 0x100)) >> 1;
                bl.rgba[3] = a.clamp(0, 0x80) as u8;
                if b.flags & 0x4000 != 0 {
                    bl.rgba[0] = 0x80;
                    bl.rgba[1] = 0x80;
                    bl.rgba[2] = 0x80;
                }
                out.push(bl);
            }
            if let Some(m) = ps.menus[p].as_ref().filter(|m| m.active) {
                out.extend(self.hud.optmenu_blits(&t.style, &m.menu, &m.state));
            }
        }
        out
    }
}
