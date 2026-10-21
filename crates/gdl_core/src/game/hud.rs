//! Player panels (the in-game HUD) as MB blits and font messages.
//!
//! Ported from reset_players 0x12ed50 / reset_players_sub 0x12e6e0 (blit
//! creation), setup_player_display 0x134d90, get_display_mode 0x134cc8,
//! SetPlayerWindows 0x12b868, WritePlayerInfo 0x1368c0,
//! write_health_and_items 0x135d38, write_gold 0x135878, write_health
//! 0x1359a0, draw_power_meter 0x135390 / power_bar_state 0x135278 /
//! hide_power_meter 0x135320, show_crystals 0x136690 and ShowRuneStones
//! 0x136418, with the MB blit layer (MBNewBlit 0x1c2b00, MBNewTempBlit,
//! MBBlitSet{Tex,Pos,Size,Crop,Color,Alpha,Hide,Z,Flags}) and the font layer
//! (DrawText/DrawTextScaled → DrawTextSub 0x1010b8, MBDrawText 0x1c4af8,
//! MBFontStringWidth 0x1c5460, MBRenderText 0x1c4f18, MBNewFont 0x1c47b8).
//!
//! Coordinates are the 512×384 blit screen (MBBlitSetPos scales by pbg +0x40
//! to the 640×448 frame); colours are GS vertex colours (0x80 = 1.0) packed
//! as MBBlitSetColor does: (rgb >> 1 & 0x7f7f7f) + 0x010101. The output is
//! data: [`Hud::draw_list`] gives the frame's blits (glyphs included) in GS
//! draw order with their z (larger = further; ZTST GEQUAL).
//!
//! The tables (box_lx, box_cx, player_rgb, player_inactive_rgb, player_pal,
//! tb_info, rune_color, key_color, player_desc_uc, potionicon_tab,
//! font_desc/font_space, CrystalDesc, GargItemDesc, MAX_CRYSTALS,
//! MAX_GARG_ITEMS) are read from the executable's symbols.
use super::{Game, R};
use gdl_formats::{elf::Elf, fnt::Font};
use std::collections::{BTreeSet, HashMap};

/// Player record offsets read by the panels (players 0x221550, 0x2b00 each).
mod o {
    pub const COLOUR: usize = 0x4;
    pub const CLASS: usize = 0xc;
    pub const CLASS_ALT: usize = 0x10;
    pub const STATE: usize = 0xfc;
    pub const POWER: usize = 0x800;
    pub const POWER_SHOWN: usize = 0x804;
    pub const QUEST: usize = 0x80c;
    pub const SHOW_ITEM: usize = 0x900;
    pub const SHOW_TIME: usize = 0x904;
    pub const SHOW_COUNT: usize = 0x908;
    pub const WRITTEN: usize = 0x93e;
    pub const DISPLAY_MODE: usize = 0x97c;
    pub const NAME: usize = 0x99c;
    pub const KEYS_CLASS: usize = 0xcf0;
    pub const GARG_ITEMS: usize = 0xd04;
    pub const CRYSTALS: usize = 0xd0a;
    pub const HEALTH: usize = 0x1a10;
    pub const KEYS: usize = 0x1a14;
    pub const POTIONS: usize = 0x1a18;
    pub const XP: usize = 0x1a1c;
    pub const GOLD: usize = 0x1a20;
    pub const RUNES: usize = 0x1a26;
    /// help_displayed bytes (0x222e60 − players), one per help id.
    pub const HELP_SEEN: usize = 0x1910;
    pub const POTION_SLOTS: usize = 0x2a9c;
    pub const LEVEL: usize = 0x2ac0;
    pub const SUBSTATE: usize = 0x2ad4;
    pub const METER_ANIM: usize = 0x2adc;
    pub const METER_TIME: usize = 0x2ae0;
}

/// A texture as MBOX_FindTexture returns it: a set (package, in search
/// order) and its OBJECTS bitmap record; MBBlitSetTex(b, −1, n) adds n.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexRef {
    pub set: u16,
    pub index: u16,
}

/// A bitmap record's MBRomTexPtr fields: +0x16/+0x18 size, +8 flags (bit 0
/// doubles blit sizes), and the TEXDEF definition size (+0x20/+0x22 of the
/// MBOX_FindTexture_Err record) fonts crop against.
#[derive(Clone, Debug)]
pub struct TexInfo {
    pub label: String,
    pub width: u16,
    pub height: u16,
    pub flags: u16,
    pub def_width: u16,
    pub def_height: u16,
}

/// One package's bitmap records (OBJECTS order) and the OBJECTS bitmap
/// name table MBOX_FindTexture searches (label → record).
#[derive(Clone, Debug)]
pub struct TexSet {
    pub name: String,
    pub records: Vec<TexInfo>,
    pub names: Vec<(String, u16)>,
}

/// An MB blit (word 0 flags: 1 hidden, 0x10 per-corner colours, 0x40
/// absolute units, 0x400/0x800 width/height from the texture, 0x4000 corner
/// highlight, 0x1000000 no size doubling).
#[derive(Clone, Debug, PartialEq)]
pub struct Blit {
    pub flags: u32,
    pub tex: Option<TexRef>,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// u0, u1, v0, v1 as texture fractions (MBBlitSetCrop).
    pub crop: [f32; 4],
    /// GS RGBA (word 7; 0x80 = 1.0).
    pub rgba: [u8; 4],
    /// MBBlitSetZ value (GS z = base − 32·z).
    pub z: f32,
    /// A coincident z-only blit (flag 0x10000: FBMSK all, TEST ATST EQUAL
    /// AREF 0) in front: this blit is hidden where that texture's alpha is 0
    /// over the screen rectangle (x, y, w, h). The fire scroll's masks.
    pub mask: Option<(TexRef, [f32; 4])>,
}

impl Blit {
    fn init() -> Self {
        // MBInitBlit: flags 0, no texture, colours 0x80808080.
        Self { flags: 0, tex: None, x: 0.0, y: 0.0, w: 0.0, h: 0.0, crop: [0.0, 1.0, 0.0, 1.0], rgba: [0x80; 4], z: 0.0, mask: None }
    }
    pub fn hidden(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// A font message (MBDrawText's 0x2c-byte record).
#[derive(Clone, Debug)]
pub struct Message {
    pub x: i32,
    pub y: i32,
    pub font: usize,
    pub rgba: [u8; 4],
    pub scale_x: f32,
    pub scale_y: f32,
    pub space_x: f32,
    pub z: f32,
    pub flags: u32,
    pub text: Vec<u8>,
    /// Message +0x26: a texture replacing the font's (DrawGlowText's
    /// hilite_tex); the glyph crop is kept.
    pub tex: Option<TexRef>,
}

/// A loaded font (font_init_desc + MBNewFont).
#[derive(Clone, Debug)]
pub struct HudFont {
    pub font: Font,
    pub tex: Option<TexRef>,
    pub space: i32,
    /// Glyphs per texture page (MBNewFont's divisor: 0xd2 for fonts 10/11,
    /// 100 for 12, else 0x80).
    pub per_page: i32,
}

/// An inventory panel piece: name and the eight words after it.
pub type Piece = (Option<String>, [i32; 8]);

#[derive(Clone, Debug)]
pub struct HudTables {
    pub box_lx: [i32; 4],
    pub box_cx: [i32; 4],
    pub player_rgb: Vec<u32>,
    pub player_inactive_rgb: Vec<u32>,
    pub player_pal: Vec<u32>,
    /// tb_info: (texture name, x, y, z) of the seven turbo blits.
    pub tb_info: Vec<(String, i32, i32, i32)>,
    pub rune_color: Vec<String>,
    pub key_color: Vec<String>,
    pub player_desc_uc: Vec<String>,
    pub potion_icons: Vec<String>,
    pub font_desc: Vec<String>,
    pub font_space: Vec<i32>,
    pub crystal_desc: Vec<String>,
    pub garg_item_desc: Vec<String>,
    pub max_crystals: Vec<i32>,
    pub max_garg_items: Vec<i32>,
    /// helptab 0x24bf08 (0x1c each: +4 pause kind, +8 priority, +0xc
    /// display rule, +0x10 ENGLISH.ROM entry, +0x14 line, +0x18 audio),
    /// border_names, text_colors (per player, 4 = none) and
    /// announcer_interval_tab.
    pub helptab: Vec<[i32; 7]>,
    pub border_names: Vec<String>,
    pub text_colors: Vec<u32>,
    pub announcer_interval: Vec<i32>,
    /// Player select: sel_xpos (column x), sel_bg_pos ({x, y, z} per slot),
    /// player_color_uc.
    pub sel_xpos: Vec<i32>,
    pub sel_bg_pos: Vec<[i32; 3]>,
    pub player_color_uc: Vec<String>,
    /// Inventory panel pieces (0x24 each: name, x, y, kind, text scale
    /// (f32 bits), text dx, text dy, …): Panel_Pieces, I_Pieces,
    /// Quest_Pieces, Shard_Pieces; cos_table/sin_table (60 steps),
    /// swirl_mult, SWIRL_RATE, IFONT_FONT.
    pub panel_pieces: Vec<Piece>,
    pub i_pieces: Vec<Piece>,
    pub quest_pieces: Vec<Piece>,
    pub shard_pieces: Vec<Piece>,
    pub cos_table: Vec<f32>,
    pub sin_table: Vec<f32>,
    pub swirl_mult: i32,
    pub swirl_rate: i32,
    pub ifont_font: usize,
    /// Shop layout: shop_lx/rx/cx (per column), shop_topy, shop_boty,
    /// shop_dim_dy, pcol (colour suffixes), shop_misctab (name, x, y, z).
    pub shop_lx: Vec<i32>,
    pub shop_rx: Vec<i32>,
    pub shop_cx: Vec<i32>,
    pub shop_y: [i32; 3],
    pub pcol: Vec<String>,
    pub shop_misc: Vec<(String, i32, i32, i32)>,
}

fn cstr(b: &[u8]) -> String {
    b.iter().take_while(|c| **c != 0).map(|c| char::from(*c)).collect()
}

impl HudTables {
    pub fn from_elf(elf: &Elf) -> Result<Self, Box<dyn std::error::Error>> {
        let words = |n: &str| -> Result<Vec<u32>, Box<dyn std::error::Error>> {
            Ok(elf.object(n)?.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect())
        };
        let halves = |n: &str| -> Result<[i32; 4], Box<dyn std::error::Error>> {
            let b = elf.object(n)?;
            let v: Vec<i32> = b.chunks_exact(2).map(|c| i32::from(u16::from_le_bytes([c[0], c[1]]))).collect();
            v.try_into().map_err(|_| format!("{n}: 4 entries expected").into())
        };
        let strings = |n: &str, stride: usize| -> Result<Vec<String>, Box<dyn std::error::Error>> {
            Ok(elf.object(n)?.chunks_exact(stride).map(cstr).collect())
        };
        let pointed = |n: &str| -> Result<Vec<String>, Box<dyn std::error::Error>> {
            words(n)?.into_iter().map(|p| Ok(cstr(elf.read(p, 32)?))).collect()
        };
        let pieces = |n: &str| -> Result<Vec<Piece>, Box<dyn std::error::Error>> {
            let mut v = Vec::new();
            for c in elf.object(n)?.chunks_exact(0x24) {
                let w = |k: usize| i32::from_le_bytes(c[4 * k..4 * k + 4].try_into().unwrap());
                let name = if w(0) == 0 { None } else { Some(cstr(elf.read(w(0) as u32, 32)?)) };
                v.push((name, [w(1), w(2), w(3), w(4), w(5), w(6), w(7), w(8)]));
            }
            Ok(v)
        };
        let tb = elf.object("tb_info")?;
        let mut tb_info = Vec::new();
        for c in tb.chunks_exact(0x14) {
            let w = |k: usize| i32::from_le_bytes(c[k..k + 4].try_into().unwrap());
            tb_info.push((cstr(elf.read(w(4) as u32, 32)?), w(8), w(12), w(16)));
        }
        let ints = |n: &str| -> Result<Vec<i32>, Box<dyn std::error::Error>> { Ok(words(n)?.into_iter().map(|v| v as i32).collect()) };
        Ok(Self {
            box_lx: halves("box_lx")?,
            box_cx: halves("box_cx")?,
            player_rgb: words("player_rgb")?,
            player_inactive_rgb: words("player_inactive_rgb")?,
            player_pal: words("player_pal")?,
            tb_info,
            rune_color: strings("rune_color", 4)?,
            key_color: strings("key_color", 4)?,
            player_desc_uc: strings("player_desc_uc", 4)?,
            potion_icons: pointed("potionicon_tab")?,
            font_desc: pointed("font_desc")?,
            font_space: ints("font_space")?,
            crystal_desc: strings("CrystalDesc", 8)?,
            garg_item_desc: strings("GargItemDesc", 0xc)?,
            max_crystals: ints("MAX_CRYSTALS")?,
            max_garg_items: ints("MAX_GARG_ITEMS")?,
            helptab: ints("helptab")?.chunks_exact(7).map(|c| c.try_into().unwrap()).collect(),
            border_names: pointed("border_names")?,
            text_colors: words("text_colors")?,
            announcer_interval: ints("announcer_interval_tab")?,
            sel_xpos: ints("sel_xpos")?,
            sel_bg_pos: ints("sel_bg_pos")?.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
            player_color_uc: pointed("player_color_uc")?,
            panel_pieces: pieces("Panel_Pieces")?,
            i_pieces: pieces("I_Pieces")?,
            quest_pieces: pieces("Quest_Pieces")?,
            shard_pieces: pieces("Shard_Pieces")?,
            cos_table: words("cos_table")?.into_iter().map(f32::from_bits).collect(),
            sin_table: words("sin_table")?.into_iter().map(f32::from_bits).collect(),
            swirl_mult: ints("swirl_mult")?[0],
            swirl_rate: ints("SWIRL_RATE")?[0],
            ifont_font: ints("IFONT_FONT")?[0] as usize,
            shop_lx: ints("shop_lx")?,
            shop_rx: ints("shop_rx")?,
            shop_cx: ints("shop_cx")?,
            shop_y: [ints("shop_topy")?[0], ints("shop_boty")?[0], ints("shop_dim_dy")?[0]],
            pcol: pointed("pcol")?,
            shop_misc: {
                let mut v = Vec::new();
                for c in elf.object("shop_misctab")?.chunks_exact(16) {
                    let w = |k: usize| i32::from_le_bytes(c[4 * k..4 * k + 4].try_into().unwrap());
                    v.push((cstr(elf.read(w(0) as u32, 32)?), w(1), w(2), w(3)));
                }
                v
            },
        })
    }
}

#[derive(Clone, Debug, Default)]
struct PlayerBlits {
    bkg: [usize; 6],
    rune: [usize; 12],
    key: [usize; 8],
    ability: [usize; 4],
    turbo: [usize; 7],
    level: usize,
    rune13: usize,
    quest: usize,
}

/// String literals the panel code names by address.
const S3: &str = "S3"; // 0x2a6c50
const S4: &str = "S4"; // 0x2a7340
const S4_FRAME: &str = "S4_FRAME"; // 0x2a7348
const COIN: &str = "coin"; // 0x2a7358
const HEART: &str = "heart"; // 0x2a7360
const QUEST_ICON: &str = "QUEST_ICON"; // 0x2a7390
const BTMBK_LEVL: &str = "BTMBK_LEVL"; // 0x2a70a8
const RUNE13: &str = "RUNE13"; // 0x2a70b8
const KEY_ICON: &str = "KEY_ICON"; // 0x2a70c0
const IT: &str = "IT"; // 0x2a74b0
const WAIT_IN_TOWER: &str = "Wait In Tower"; // 0x2a7410
const QUIT_GAME: &str = "Quit Game"; // 0x2a7420
const IN_TOWER: &str = "IN TOWER"; // 0x2a7430
/// MBSetFontZ(63990.0) and the panel blits' MBBlitSetZ(64000.0).
const FONT_Z: f32 = 63990.0;
const PANEL_Z: f32 = 64000.0;

#[derive(Clone, Debug, Default)]
pub struct Hud {
    pub tables: Option<HudTables>,
    /// Texture sets in MBOX_FindTexture search order.
    pub textures: Vec<TexSet>,
    index: HashMap<String, TexRef>,
    pub fonts: Vec<Option<HudFont>>,
    blits: Vec<Option<Blit>>,
    temp: Vec<Blit>,
    pub messages: Vec<Message>,
    players: Vec<PlayerBlits>,
    it_blit: Option<usize>,
    tb_tex: Vec<Option<TexRef>>,
    key_tex: Option<TexRef>,
    potion_tex: Vec<Option<TexRef>>,
    font_z: f32,
    /// PlayerWindowsHidden, menu_on, 0x20f968 (debug readout), optmenu_active,
    /// full_screen_inventory_up and NumLivePlayers (front-end state).
    pub windows_hidden: i32,
    pub menu_on: i32,
    pub debug_readout: i32,
    pub optmenu_active: i32,
    pub full_screen_inventory: i32,
    pub num_live_players: i32,
    /// button_x_tex / button_tri_tex (set by ResetModels; not ported: None).
    pub button_x: Option<TexRef>,
    pub button_tri: Option<TexRef>,
    /// Texture names MBOX_FindTexture did not resolve.
    pub missing: BTreeSet<String>,
    /// GameText (TEXT/ENGLISH.ROM) for help messages.
    pub text: Option<gdl_formats::textrom::TextRom>,
    /// ScrollText[0] (SCROLL_E.ROM) for CaptionText.
    pub scrolls: Option<gdl_formats::textrom::TextRom>,
    pub help: HelpState,
    /// got_it 0x22c500: 0x18 pickup notices (state, player, kind, amount,
    /// timer, bar blit, picture blit).
    got_it: Vec<GotIt>,
}

#[derive(Clone, Debug, Default)]
struct GotIt {
    state: i32,
    player: usize,
    kind: i32,
    amount: i32,
    timer: i32,
    bar: Option<usize>,
    picture: Option<usize>,
}

/// NewHelp/DoHelp state (InitHelp 0x18e0b0 zeroes it per level).
#[derive(Clone, Debug, Default)]
pub struct HelpState {
    pub flag: bool,
    pub num: i32,
    pub player: usize,
    pub time: i32,
    pub interval: i32,
    pub interval_idx: usize,
    pub lv: i32,
    pub x: i32,
    pub y: i32,
    pub testflag: bool,
    pub colour: u32,
    border: Option<usize>,
}

impl Hud {
    pub fn new(tables: HudTables) -> Self {
        // MBResetFontData: fontZ 0.25.
        Self { tables: Some(tables), font_z: 0.25, ..Default::default() }
    }

    /// The client's texture sets (search order) and fonts (font_desc order).
    /// A texture set searched after the others (LoadModel of a screen's
    /// package, e.g. the realm map): its set number.
    pub fn add_texture_set(&mut self, set: TexSet) -> u16 {
        let s = self.textures.len() as u16;
        for (label, i) in &set.names {
            self.index.entry(label.to_uppercase()).or_insert(TexRef { set: s, index: *i });
        }
        self.textures.push(set);
        s
    }

    pub fn set_resources(&mut self, textures: Vec<TexSet>, fonts: Vec<Option<Font>>) {
        self.index.clear();
        for (s, set) in textures.iter().enumerate() {
            for (label, i) in &set.names {
                self.index.entry(label.to_uppercase()).or_insert(TexRef { set: s as u16, index: *i });
            }
        }
        self.textures = textures;
        let names = self.tables.as_ref().map(|t| t.font_desc.clone()).unwrap_or_default();
        let space = self.tables.as_ref().map(|t| t.font_space.clone()).unwrap_or_default();
        self.fonts = fonts
            .into_iter()
            .enumerate()
            .map(|(k, f)| {
                f.map(|font| HudFont {
                    font,
                    tex: names.get(k).and_then(|n| self.index.get(&n.to_uppercase()).copied()),
                    space: space.get(k).copied().unwrap_or(0),
                    per_page: match k {
                        10 | 11 => 0xd2,
                        12 => 100,
                        _ => 0x80,
                    },
                })
            })
            .collect();
    }

    pub fn tex_info(&self, t: TexRef) -> Option<&TexInfo> {
        self.textures.get(t.set as usize)?.records.get(t.index as usize)
    }

    /// MBOX_FindTexture by name (labels compare case-insensitively).
    pub fn find(&mut self, name: &str) -> Option<TexRef> {
        let t = self.index.get(&name.to_uppercase()).copied();
        if t.is_none() {
            self.missing.insert(name.to_string());
        }
        t
    }

    pub(crate) fn make_pub(&self, tex: Option<TexRef>, x: i32, y: i32, w: i32, h: i32) -> Blit {
        self.make(tex, x, y, w, h)
    }
    pub(crate) fn offset_pub(&self, t: TexRef, n: i32) -> Option<TexRef> {
        self.offset(t, n)
    }
    pub(crate) fn font_z_pub(&self) -> f32 {
        self.font_z
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn message_pub(&self, scale: f32, space: f32, x: i32, y: i32, font: usize, colour: u32, alpha: u8, z: f32, flags: u32, text: &str) -> Option<Message> {
        self.message(scale, space, x, y, font, pack_rgb(colour, alpha), z, flags, text)
    }

    /// MBOX_FindTexture without recording a miss.
    pub fn lookup(&self, name: &str) -> Option<TexRef> {
        self.index.get(&name.to_uppercase()).copied()
    }

    fn offset(&self, t: TexRef, n: i32) -> Option<TexRef> {
        let i = i32::from(t.index) + n;
        let set = self.textures.get(t.set as usize)?;
        (0 <= i && (i as usize) < set.records.len()).then_some(TexRef { set: t.set, index: i as u16 })
    }

    fn tex_size(&self, b: &Blit) -> (f32, f32) {
        match b.tex.and_then(|t| self.tex_info(t)) {
            Some(i) => {
                let k = if i.flags & 1 != 0 && b.flags & 0x100_0000 == 0 { 2.0 } else { 1.0 };
                (f32::from(i.width) * k, f32::from(i.height) * k)
            }
            None => (0.0, 0.0),
        }
    }

    fn get(&mut self, id: usize) -> Option<&mut Blit> {
        self.blits.get_mut(id).and_then(|b| b.as_mut())
    }

    /// MBBlitSetSize: a negative size takes the texture's (and flags it so a
    /// texture change resizes); 0 leaves it.
    fn size_blit(&self, b: &mut Blit, w: i32, h: i32) {
        let (tw, th) = self.tex_size(b);
        let mut from = 0;
        let (w, h) = (
            if w < 0 {
                from |= 0x400;
                tw
            } else {
                w as f32
            },
            if h < 0 {
                from |= 0x800;
                th
            } else {
                h as f32
            },
        );
        if w != 0.0 || from & 0x400 != 0 {
            b.w = w;
            b.flags &= !0x400;
        }
        if h != 0.0 || from & 0x800 != 0 {
            b.h = h;
            b.flags &= !0x800;
        }
        b.flags |= from;
    }

    fn make(&self, tex: Option<TexRef>, x: i32, y: i32, w: i32, h: i32) -> Blit {
        let mut b = Blit::init();
        b.tex = tex;
        b.x = x as f32;
        b.y = y as f32;
        // MBBlitSetPos(1.0, …): z 1.0.
        b.z = 1.0;
        self.size_blit(&mut b, w, h);
        b
    }

    /// MBNewBlit(0, tex, x, y, w, h) on the default node (appended).
    pub fn new_blit(&mut self, tex: Option<TexRef>, x: i32, y: i32, w: i32, h: i32) -> usize {
        let b = self.make(tex, x, y, w, h);
        self.blits.push(Some(b));
        self.blits.len() - 1
    }

    /// MBNewTempBlit: drawn after the node's blits this frame only.
    pub fn temp_blit(&mut self, tex: Option<TexRef>, x: i32, y: i32, w: i32, h: i32) -> usize {
        let mut b = self.make(tex, x, y, w, h);
        b.flags = 0;
        self.temp.push(b);
        self.temp.len() - 1
    }

    fn remove(&mut self, id: usize) {
        if let Some(b) = self.blits.get_mut(id) {
            *b = None;
        }
    }

    /// MBBlitSetTex(b, tex, n): tex None keeps the current texture; sizes
    /// flagged as texture-derived follow the new texture (crop fractions are
    /// kept: the original rescales its texel crop by the size ratio).
    pub fn set_tex(&mut self, id: usize, tex: Option<TexRef>, n: i32) {
        let Some(cur) = self.blits.get(id).and_then(|b| b.as_ref()).map(|b| b.tex) else { return };
        let base = tex.or(cur);
        let new = base.and_then(|t| self.offset(t, n));
        let Some(mut b) = self.blits[id].clone() else { return };
        if new != b.tex {
            b.tex = new;
            let (w, h) = (if b.flags & 0x400 != 0 { -1 } else { 0 }, if b.flags & 0x800 != 0 { -1 } else { 0 });
            if w < 0 || h < 0 {
                self.size_blit(&mut b, w, h);
            }
        }
        self.blits[id] = Some(b);
    }

    pub fn set_hide(&mut self, id: usize, hide: bool) {
        if let Some(b) = self.get(id) {
            b.flags = if hide { b.flags | 1 } else { b.flags & !1 };
        }
    }

    /// MBBlitSetPos(z, b, x, y): z only when ≥ 0.
    pub fn set_pos(&mut self, id: usize, z: f32, x: i32, y: i32) {
        if let Some(b) = self.get(id) {
            b.x = x as f32;
            b.y = y as f32;
            if 0.0 <= z {
                b.z = z;
            }
        }
    }

    pub fn set_size(&mut self, id: usize, w: i32, h: i32) {
        if let Some(mut b) = self.blits.get(id).cloned().flatten() {
            self.size_blit(&mut b, w, h);
            self.blits[id] = Some(b);
        }
    }

    pub fn set_z(&mut self, id: usize, z: f32) {
        if let Some(b) = self.get(id) {
            b.z = blit_z(z);
        }
    }

    pub fn set_colour(&mut self, id: usize, c: u32) {
        if let Some(b) = self.get(id) {
            b.flags &= !0x10;
            b.rgba = pack_rgb(c, b.rgba[3]);
        }
    }

    /// MBBlitSetAlpha: 0 opaque (0x80) … 0xff nearly transparent (1).
    pub fn set_alpha(&mut self, id: usize, a: u32) {
        if let Some(b) = self.get(id) {
            b.flags &= !0x10;
            b.rgba[3] = (0x80 - (a >> 1)) as u8;
        }
    }

    pub fn set_flags(&mut self, id: usize, and: u32, or: u32) {
        if let Some(b) = self.get(id) {
            b.flags = (b.flags & and) | or;
        }
    }

    /// MBFontStringWidth with the space scale `space`.
    pub fn string_width(&self, font: usize, space: f32, text: &[u8]) -> i32 {
        let Some(Some(f)) = self.fonts.get(font) else { return 0 };
        text.iter()
            .map(|&c| match glyph(f, c) {
                Some(g) if g.width != 0 => (g.width as f32 * space) as i32,
                _ if c == 0x20 => (f.space as f32 * space) as i32,
                _ => 0,
            })
            .sum()
    }

    /// DrawTextSub(scale, space, x, y, font, colour, text) (no shadow flag
    /// 0x100 or vertical-centre bit 0x1000 here: the panels use neither).
    pub fn draw_text(&mut self, scale: f32, x: i32, y: i32, font: u32, colour: u32, text: &str) {
        if let Some(m) = self.message(scale, scale, x, y, (font & 0xff) as usize, pack_rgb(colour, 0x80), self.font_z, 0, text) {
            self.messages.push(m);
        }
    }

    /// The FONT entry of a text-ROM entry resolved against font_desc
    /// (StringInitSub), its scale (× DrawStringScale 1.0) and space scale.
    fn entry_style(&self, e: usize) -> Option<(usize, f32, f32)> {
        let rom = self.text.as_ref()?;
        let entry = rom.entries.get(e)?;
        let name = rom.fonts.get(entry.font as usize)?;
        let font = self.tables.as_ref()?.font_desc.iter().position(|n| n == name).unwrap_or(0);
        Some((font, entry.scale, entry.space))
    }

    fn entry_lines(&self, e: usize) -> Vec<String> {
        self.text.as_ref().and_then(|r| r.entries.get(e)).map(|x| x.lines.iter().map(|l| l.replace('\r', "")).collect()).unwrap_or_default()
    }

    /// GetStringText(entry, line).
    pub fn string_text(&self, e: i32, line: i32) -> Option<String> {
        let l = usize::try_from(line).ok()?;
        self.entry_lines(usize::try_from(e).ok()?).get(l).cloned()
    }

    /// StringTextHeightSub(scale, entry, line, dy): (FontHeight + dy) per
    /// sub-line of the line (or of every line for line < 0).
    pub fn string_text_height(&self, scale: f32, e: i32, line: i32, dy: i32) -> i32 {
        let Ok(eu) = usize::try_from(e) else { return 0 };
        let Some((font, es, _)) = self.entry_style(eu) else { return 0 };
        let fh = self.font_height(font, scale * es);
        let lines = self.entry_lines(eu);
        let sub = |l: &String| l.matches('\n').count().min(15) as i32 + 1;
        match usize::try_from(line) {
            Ok(l) => lines.get(l).map_or(0, |x| (fh + dy) * sub(x)),
            Err(_) => lines.iter().map(|x| (fh + dy) * sub(x)).sum(),
        }
    }

    /// StringTextWidthSub(scale, entry, line): the widest sub-line.
    pub fn string_text_width(&self, scale: f32, e: i32, line: i32) -> i32 {
        let Ok(eu) = usize::try_from(e) else { return 0 };
        let Some((font, es, _)) = self.entry_style(eu) else { return 0 };
        let lines = self.entry_lines(eu);
        let w = |l: &String| l.split('\n').take(16).map(|s| self.string_width(font, scale * es, s.as_bytes())).max().unwrap_or(0);
        match usize::try_from(line) {
            Ok(l) => lines.get(l).map_or(0, w),
            Err(_) => lines.iter().map(w).max().unwrap_or(0),
        }
    }

    /// DrawTextSub with its vertical-centre bit (y & 0x1000, y > 0).
    #[allow(clippy::too_many_arguments)]
    fn draw_text_sub(&mut self, scale: f32, space: f32, x: i32, y: i32, font: usize, colour: u32, text: &str) {
        let space = if 0.0 < space {
            space * scale
        } else if space < 0.0 {
            -space
        } else {
            1.0
        };
        let mut y = y;
        if 0 < y && y & 0x1000 != 0 {
            y = (y & !0x1000) - self.font_height(font, scale) / 2;
        }
        if let Some(m) = self.message(scale, space, x, y, font, pack_rgb(colour, 0x80), self.font_z, 0, text) {
            self.messages.push(m);
        }
    }

    /// DrawStringText(x, y, font, colour, entry, line, args…): the line's
    /// sub-lines, each through DrawTextSub, stepping FontHeight (dy 0).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_string_text(&mut self, x: i32, y: i32, font: i32, colour: u32, e: i32, line: i32, args: &[Arg]) {
        let Some(text) = self.string_text(e, line) else { return };
        let Some((efont, scale, space)) = self.entry_style(e as usize) else { return };
        let font = if (0..10).contains(&font) && efont < 10 { font as usize } else { efont };
        let text = format_c(&text, args);
        let fh = self.font_height(font, scale);
        let mut y = y;
        for l in text.split('\n').take(16) {
            self.draw_text_sub(scale, space, x, y, font, colour, l);
            y += fh;
        }
    }

    /// DrawStringTextMulti(x, y, dy, font, colour, entry): every line of the
    /// entry; y & 0x1000 centres the block.
    pub fn draw_string_text_multi(&mut self, x: i32, y: i32, dy: i32, colour: u32, e: i32) {
        let Some((font, scale, space)) = self.entry_style(e as usize) else { return };
        let lines = self.entry_lines(e as usize);
        let fh = self.font_height(font, scale);
        let mut y = y;
        if y & 0x1000 != 0 {
            y = (y & !0x1000) - ((dy + fh) * (lines.len() as i32 - 1) + fh) / 2;
        }
        for l in &lines {
            self.draw_text_sub(scale, space, x, y, font, colour, l);
            y += dy + fh;
        }
    }

    /// DrawStringTextMLines: the entry's lines joined, formatted, split.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_string_text_mlines(&mut self, x: i32, y: i32, dy: i32, colour: u32, e: i32, args: &[Arg]) {
        let Some((font, scale, space)) = self.entry_style(e as usize) else { return };
        let lines = self.entry_lines(e as usize);
        let n = lines.len().min(16);
        let fh = self.font_height(font, scale);
        let mut y = y;
        if y & 0x1000 != 0 {
            y = (y & !0x1000) - ((dy + fh) * (n as i32 - 1) + fh) / 2;
        }
        let text = format_c(&lines.iter().take(16).map(|l| format!("{l}\n")).collect::<String>(), args);
        for l in text.split('\n').take(n) {
            self.draw_text_sub(scale, space, x, y, font, colour, l);
            y += dy + fh;
        }
    }

    /// DrawGlowText 0x102ab0: glow_font (6) at `scale` twice — the
    /// FONT32_GLOW layer (flag 0x4000, hilite_color 0x8200ea, alpha pulsing
    /// over hilite_fade 40 / hold 5 by vb_counter), then white; font z 0.25.
    pub fn glow_text(&self, scale: f32, x: i32, y: i32, text: &str, vb: u32) -> Vec<Blit> {
        const GLOW_FONT: usize = 6;
        let (fade, hold) = (40, 5);
        let mut i = (vb % (fade * 2 + hold) as u32) as i32;
        if fade * 2 < i {
            i = 0;
        } else if fade < i {
            i = fade * 2 - i;
        }
        let a = 0x7f - ((i * 0xff + fade - 1) / fade) / 2;
        let glow_alpha = (0x80 - (a as u32 >> 1)) as u8;
        let mut out = Vec::new();
        if let Some(mut m) = self.message(scale, scale, x, y, GLOW_FONT, pack_rgb(0x8200ea, glow_alpha), 0.25, 0x4000, text) {
            m.tex = self.lookup("FONT32_GLOW");
            out.extend(self.glyphs(&m));
        }
        if let Some(m) = self.message(scale, scale, x, y, GLOW_FONT, pack_rgb(0xffffff, 0x80), 0.25, 0, text) {
            out.extend(self.glyphs(&m));
        }
        out
    }

    /// CaptionTextSub 0x165290: `text` revealed by `frame` (1.75 per
    /// character, '.'/',' 2 more, '\t' 5, '\r' 30 and a new page), each line
    /// centred on x 256 from `y` stepping scale × 32, white over the font's
    /// shadow (font flag 0x100: shadow colour 0 at −2/−2). Returns whether
    /// the whole text has been revealed.
    pub fn caption(&mut self, scale: f32, text: &str, font: usize, frame: i32, y: i32) -> bool {
        let mut f = frame.max(0) as f32;
        let mut y = y as f32;
        let mut line = String::new();
        let mut page_lines: Vec<(String, f32)> = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        let mut done = true;
        while i < chars.len() {
            if f <= 0.0 {
                done = false;
                break;
            }
            let c = chars[i];
            i += 1;
            match c {
                '\n' => {
                    if !line.is_empty() {
                        page_lines.push((std::mem::take(&mut line), y));
                    }
                    y += scale * 32.0;
                }
                '\t' => f -= 5.0,
                '\r' => {
                    f -= 30.0;
                    if 0.0 <= f {
                        // A new page: what was shown so far goes.
                        line.clear();
                        page_lines.clear();
                        y -= 0.0;
                        if chars.get(i) == Some(&'\n') {
                            i += 1;
                        }
                    }
                }
                _ => {
                    if c == ',' || c == '.' {
                        f -= 2.0;
                    }
                    f -= 1.75;
                    line.push(c);
                }
            }
        }
        if !line.is_empty() {
            page_lines.push((line, y));
        }
        for (l, ly) in page_lines {
            let w = self.string_width(font, scale, l.as_bytes());
            let x = 0x100 - (w >> 1);
            for (dx, colour) in [(-2, 0), (0, 0xffffff)] {
                if let Some(m) = self.message(scale, scale, x + dx, ly as i32 + dx, font, pack_rgb(colour, 0x80), self.font_z, 0, &l) {
                    self.messages.push(m);
                }
            }
        }
        done
    }

    /// Lines of text centred on x 256 as a block around `y_mid` (font,
    /// scale, GS colour 0xRRGGBB, line step FontHeight + dy; font z 0.25).
    #[allow(clippy::too_many_arguments)]
    pub fn text_block_blits(&self, scale: f32, y_mid: i32, font: usize, colour: u32, lines: &[&str], dy: i32) -> Vec<Blit> {
        let fh = self.font_height(font, scale);
        let n = lines.len() as i32;
        let mut y = y_mid - ((fh + dy) * (n - 1) + fh) / 2;
        let mut out = Vec::new();
        for l in lines {
            if let Some(m) = self.message(scale, scale, -0x100, y, font, pack_rgb(colour, 0x80), 0.25, 0, l) {
                out.extend(self.glyphs(&m));
            }
            y += fh + dy;
        }
        out
    }

    /// TriggerCamUpdate's letterbox temp blits: TextureWhite (AAAWHITE)
    /// with MBBlitSetColor 0, z 0.5: rows 0..BORDER_HT (0x30) and
    /// 0x130..0x180.
    pub fn letterbox_blits(&self) -> Vec<Blit> {
        [(0, 0x30), (0x130, 0x50)]
            .into_iter()
            .map(|(y, h)| {
                let mut b = self.make(self.lookup("AAAWHITE"), 0, y, 0x200, h);
                b.rgba = pack_rgb(0, 0x80);
                b.z = 0.5;
                b
            })
            .collect()
    }

    /// The frame's blits in draw order: node blits, temp blits, then the font
    /// messages' glyphs (MBDrawBlits then MBRenderText in pbRenderNode).
    pub fn draw_list(&self) -> Vec<Blit> {
        let mut out: Vec<Blit> = self.blits.iter().flatten().filter(|b| !b.hidden()).cloned().collect();
        out.extend(self.temp.iter().filter(|b| !b.hidden()).cloned());
        for m in &self.messages {
            out.extend(self.glyphs(m));
        }
        out
    }

    /// MBRenderText for one message: a blit per glyph (flag 0x4000 grows the
    /// glyph by two GS pixels' worth of screen units on every side).
    pub fn glyphs(&self, m: &Message) -> Vec<Blit> {
        let mut out = Vec::new();
        let Some(Some(f)) = self.fonts.get(m.font) else { return out };
        let Some(tex) = f.tex else { return out };
        let Some(info) = self.tex_info(tex) else { return out };
        let (dw, dh) = (f32::from(info.def_width.max(1)), f32::from(info.def_height.max(1)));
        // iStack_d4/d0: (int)(pbg +0x40 scale × 2 + 0.5) GS units (40, 37).
        let (gx, gy) = (40.0 / 20.0, 37.0 / (16.0 * 448.0 / 384.0));
        let mut x = m.x;
        for &c in &m.text {
            let found = f.font.glyphs.iter().enumerate().find(|(_, g)| g.code == i32::from(c) && g.width != 0);
            let Some((k, g)) = found else {
                if c == 0x20 {
                    x += (f.space as f32 * m.space_x) as i32;
                }
                continue;
            };
            let t = match m.tex {
                Some(t) => Some(t),
                None => self.offset(tex, k as i32 / f.per_page),
            };
            let Some(t) = t else { continue };
            let mut b = Blit {
                flags: m.flags,
                tex: Some(t),
                x: x as f32,
                y: m.y as f32,
                w: g.width as f32 * m.scale_x,
                h: f.font.height as f32 * m.scale_y,
                crop: [g.x as f32 / dw, (g.x + g.width) as f32 / dw, g.y as f32 / dh, (g.y + f.font.height) as f32 / dh],
                rgba: m.rgba,
                z: m.z,
                mask: None,
            };
            if m.flags & 0x4000 != 0 {
                b.x -= gx;
                b.y -= gy;
                b.w += 2.0 * gx;
                b.h += 2.0 * gy;
            }
            out.push(b);
            x += (g.width as f32 * m.space_x) as i32;
        }
        out
    }

    /// A message as DrawTextSub/MBDrawText build it (x < 0 centres on −x).
    #[allow(clippy::too_many_arguments)]
    fn message(&self, scale: f32, space: f32, x: i32, y: i32, font: usize, rgba: [u8; 4], z: f32, flags: u32, text: &str) -> Option<Message> {
        let text = text.as_bytes().to_vec();
        let x = if x < 0 { -(self.string_width(font, space, &text) >> 1) - x } else { x };
        (y as u32 <= 0x17f).then_some(Message { x, y, font, rgba, scale_x: scale, scale_y: scale, space_x: space, z, flags, text, tex: None })
    }

    /// FontHeight(scale, font): (int)(height × scale).
    pub fn font_height(&self, font: usize, scale: f32) -> i32 {
        match self.fonts.get(font) {
            Some(Some(f)) => (f.font.height as f32 * scale) as i32,
            _ => 0,
        }
    }

    /// One frame of ScrollMessageBox 0x128158 / the fire scroll
    /// (StartFireScroll 0x128b78, ServeFireScroll 0x129008): the Scroll_A
    /// blit sized to the page text, the page lines (DrawScrollText: font and
    /// scale from the SCROLL_E entry, colour 0x160c03, 4-unit line gap,
    /// centred on x 256, font z 0.25), the triangle button and DrawGlowText's
    /// "Press     when done."; after the last page the 21 burn frames
    /// (greenCircTrans/greenCircTransM handles + 1 + frame).
    pub fn scroll_blits(&self, sb: &ScrollBox) -> Vec<Blit> {
        const DONE: &str = "Press     when done."; // donemsg.9 → 0x2a67d0
        const DONE_SCALE: f32 = 0.5; // done_scale 0x220420
        const GLOW_FONT: usize = 6; // glow_font 0x20ec00
        const TEXT_Z: f32 = 0.25; // MBResetFontData fontZ
        let page = sb.page.min(sb.pages.len().saturating_sub(1));
        let Some(text) = sb.pages.get(page) else { return Vec::new() };
        let lines: Vec<&str> = text.split('\n').take(16).collect();
        let width = lines.iter().map(|l| self.string_width(sb.font, sb.scale, l.as_bytes())).max().unwrap_or(0) + 0x60;
        let fh = self.font_height(sb.font, sb.scale);
        let height = (fh + 4) * lines.len() as i32 + 0x60;
        let done = self.string_width(GLOW_FONT, DONE_SCALE, DONE.as_bytes());
        let w = if done + 0x20 <= width { width.min(0x200) } else { done + 0x20 };
        let (x0, y0, ty) = (0x100 - w / 2, 0xa0 - height / 2, 0xc0 - height / 2);
        let scroll = self.lookup("Scroll_A");
        let mut out = Vec::new();
        let mut back = self.make(scroll, x0, y0, w, height);
        // Text: DrawScrollText → DrawStringTextSub, one DrawTextSub per line.
        let mut glyphs = Vec::new();
        let mut y = ty;
        for l in &lines {
            if let Some(m) = self.message(sb.scale, sb.space * sb.scale, -0x100, y, sb.font, pack_rgb(0x160c03, 0x80), TEXT_Z, 0, l) {
                glyphs.extend(self.glyphs(&m));
            }
            y += 4 + fh;
        }
        match sb.burn {
            None => {
                out.push(back);
                out.extend(glyphs);
                let tri = self.lookup("BUTTON_TRI");
                out.push(self.make(tri, 0xdf, y + 8, 0x14, 0x14));
                out.extend(self.glow_text(DONE_SCALE, -0x100, y + 8, DONE, sb.vb_counter));
            }
            Some(vb) => {
                let frame = (vb >> 1).clamp(0, 0x14);
                let (fy, fh2) = if y0 < 0 { (0, height + y0 + 0x20) } else { (y0, height) };
                let rect = [x0 as f32, fy as f32, w as f32, fh2 as f32];
                let mask = self.lookup("greenCircTransM").and_then(|t| self.offset(t, 1 + frame));
                let fire = self.lookup("greenCircTrans").and_then(|t| self.offset(t, 1 + frame));
                if let Some(m) = mask {
                    back.mask = Some((m, rect));
                    out.push(back);
                    for mut g in glyphs {
                        g.mask = Some((m, rect));
                        out.push(g);
                    }
                }
                let mut f = self.make(fire, x0, fy, w, fh2);
                f.z = 0.125;
                f.mask = scroll.map(|s| (s, [x0 as f32, y0 as f32, w as f32, height as f32]));
                out.push(f);
            }
        }
        out
    }
    /// add_got_it 0x12b060: the first free of 0x18 notices, for the kinds
    /// do_got_it has a picture for.
    pub fn add_got_it(&mut self, player: usize, kind: i32, amount: i32) {
        if self.got_it.len() < 0x18 {
            self.got_it.resize(0x18, GotIt::default());
        }
        if let Some(g) = self.got_it.iter_mut().find(|g| g.state == 0)
            && matches!(kind, 1..=10 | 0xd | 0xf | 0x10)
        {
            *g = GotIt { state: 1, player, kind, amount, timer: 0, bar: g.bar, picture: g.picture };
        }
    }

    /// ResetModels' part for the HUD: every node blit, the help box and the
    /// pickup notices go (the caller rebuilds the panels).
    pub fn reset_models(&mut self) {
        self.help = HelpState::default();
        self.got_it.clear();
        self.blits.clear();
    }

    /// Blits drawn this frame only (appended to the temp list).
    pub fn push_temp(&mut self, blits: impl IntoIterator<Item = Blit>) {
        self.temp.extend(blits);
    }

    /// MBResetBlits at the frame boundary: temp blits and messages go.
    pub fn begin_frame(&mut self) {
        self.temp.clear();
        self.messages.clear();
    }
}

/// ScrollMessageBox's modal state: pages (the entry's lines), font/scale/
/// space from the SCROLL_E entry, the 15-vblank input delay per page and
/// the fire scroll's vblank count once the last page is closed.
#[derive(Clone, Debug)]
pub struct ScrollBox {
    pub pages: Vec<String>,
    pub font: usize,
    pub scale: f32,
    pub space: f32,
    pub page: usize,
    pub wait: i32,
    pub burn: Option<i32>,
    pub vb_counter: u32,
}

impl ScrollBox {
    pub fn new(pages: Vec<String>, font: usize, scale: f32, space: f32) -> Self {
        Self { pages, font, scale, space, page: 0, wait: 15, burn: None, vb_counter: 0 }
    }

    /// One loop pass with `vb` vblanks elapsed and the close button state;
    /// false once the fire scroll has finished (ServeFireScroll: 21 frames,
    /// one per two vblanks).
    pub fn step(&mut self, vb: i32, pressed: bool) -> bool {
        self.vb_counter = self.vb_counter.wrapping_add(vb as u32);
        if let Some(b) = self.burn.as_mut() {
            *b += vb;
            return (*b >> 1) < 0x15;
        }
        if self.wait < 1 {
            if pressed {
                self.page += 1;
                self.wait = 15;
                if self.pages.len() <= self.page {
                    self.burn = Some(0);
                }
            }
        } else {
            self.wait -= vb;
        }
        true
    }
}

fn glyph(f: &HudFont, c: u8) -> Option<gdl_formats::fnt::Glyph> {
    f.font.glyphs.iter().find(|g| g.code == i32::from(c)).copied()
}

/// A vsprintf argument.
#[derive(Clone, Debug)]
pub enum Arg {
    S(String),
    D(i32),
}

/// vsprintf for the help strings' %s / %d (and %%).
pub fn format_c(fmt: &str, args: &[Arg]) -> String {
    let mut out = String::new();
    let mut it = args.iter();
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('s') => {
                if let Some(Arg::S(v)) = it.next() {
                    out.push_str(v);
                }
            }
            Some('d') => match it.next() {
                Some(Arg::D(v)) => out.push_str(&v.to_string()),
                Some(Arg::S(v)) => out.push_str(v),
                None => {}
            },
            Some(o) => {
                out.push('%');
                out.push(o);
            }
            None => out.push('%'),
        }
    }
    out
}

/// MBBlitSetZ: z ≥ 0 is stored × 32; a negative z is stored as raw GS
/// units 0x1000000 + z (far behind everything), here in the same × 32 scale.
pub fn blit_z(z: f32) -> f32 {
    if 0.0 <= z { z } else { (0x100_0000 as f32 + z) / 32.0 }
}

/// MBBlitSetColor / MBSetFontColor packing of 0xRRGGBB.
pub(crate) fn pack_rgb_pub(c: u32, a: u8) -> [u8; 4] {
    pack_rgb(c, a)
}
fn pack_rgb(c: u32, a: u8) -> [u8; 4] {
    let ch = |s: u32| (((c >> s) & 0xff) >> 1) as u8 + 1;
    [ch(16), ch(8), ch(0), a]
}

/// power_bar_state 0x135278: (state, fraction) of the shown power +0x804.
fn power_bar_state(shown: f32) -> (i32, f32) {
    let f = shown * 0.01;
    if f < 0.4 {
        (1, f / 0.4)
    } else if 0.99 <= f {
        (3, 1.0)
    } else {
        (2, (f - 0.4) / 0.6)
    }
}

impl Game {
    /// reset_players' panel part: reset_players_sub for the four boxes,
    /// PlayerWindowsHidden = 0, key/potion icon textures (It and the player
    /// record fields it resets belong to the session start state).
    pub fn hud_reset_players(&mut self) {
        let Some(t) = self.hud.tables.clone() else { return };
        self.hud.it_blit = None;
        self.hud.players.clear();
        self.hud.blits.clear();
        for p in 0..4 {
            self.reset_players_sub(p, &t);
        }
        self.hud.windows_hidden = 0;
        self.hud.key_tex = self.hud.find(KEY_ICON);
        self.hud.potion_tex = t.potion_icons.iter().map(|n| self.hud.find(n)).collect();
    }

    fn reset_players_sub(&mut self, p: usize, t: &HudTables) {
        let h = &mut self.hud;
        let lx = t.box_lx[p];
        let mut pb = PlayerBlits::default();
        for (k, (x, y, w, hh)) in [(lx, 0x130, 0x80, -1), (lx, 0x140, 0x80, -1), (lx, 0x140, 0x80, -1), (lx, 0x158, 0x94, -1), (lx + 8, 0x148, 0x14, 0x14), (lx, 0x148, 0x14, 0x14)].into_iter().enumerate() {
            pb.bkg[k] = h.new_blit(None, x, y, w, hh);
        }
        for &b in &pb.bkg {
            h.set_hide(b, true);
            h.set_z(b, PANEL_Z);
        }
        for i in 0..12 {
            let colour = t.rune_color.get(i / 3).cloned().unwrap_or_default();
            let tex = h.find(&format!("SM_RUNE_{colour}_{:02}", i % 3 + 1));
            let b = h.new_blit(tex, lx + i as i32 * 8 + 0xf + i as i32 / 3, 0x132, -1, -1);
            pb.rune[i] = b;
            h.set_hide(b, true);
            h.set_z(b, PANEL_Z);
        }
        for k in 0..8 {
            let colour = t.key_color.get(k).cloned().unwrap_or_default();
            let tex = h.find(&format!("SM_KEY_{colour}"));
            let b = h.new_blit(tex, lx + k as i32 * 0xc + 0xc, 300, -1, -1);
            pb.key[k] = b;
            h.set_hide(b, true);
            h.set_z(b, PANEL_Z);
        }
        for k in 0..4 {
            let b = h.new_blit(None, lx + 0x1a, k as i32 * 3 + 0x142, -1, -1);
            pb.ability[k] = b;
            h.set_hide(b, true);
            h.set_z(b, PANEL_Z);
        }
        h.tb_tex.resize(t.tb_info.len().max(h.tb_tex.len()), None);
        for (k, (name, x, y, z)) in t.tb_info.iter().enumerate().take(7) {
            let tex = h.find(name);
            let b = h.new_blit(tex, p as i32 * 0x80 + x, *y, -1, -1);
            pb.turbo[k] = b;
            h.tb_tex[k] = tex;
            h.set_hide(b, true);
            h.set_z(b, *z as f32);
        }
        self.players[p].set_i32(o::METER_ANIM, 0);
        let h = &mut self.hud;
        let cx = t.box_cx[p];
        pb.level = h.new_blit(None, cx - 0xe, -0x143, -1, -1);
        let tex = h.find(BTMBK_LEVL);
        h.set_tex(pb.level, tex, 0);
        h.set_hide(pb.level, true);
        h.set_z(pb.level, PANEL_Z);
        // low_res (MBBlitSetFlags 0x100) is not modelled.
        pb.rune13 = h.new_blit(None, lx + 8, 0x154, 0x10, 0x10);
        let tex = h.find(RUNE13);
        h.set_tex(pb.rune13, tex, 0);
        h.set_hide(pb.rune13, true);
        h.set_z(pb.rune13, PANEL_Z);
        pb.quest = h.new_blit(None, lx + 0x68, 0x152, 0x10, 0x10);
        h.set_hide(pb.quest, true);
        h.set_z(pb.quest, PANEL_Z);
        h.players.push(pb);
    }

    /// get_display_mode 0x134cc8.
    pub fn hud_display_mode(&self, p: usize) -> i32 {
        let r = &self.players[p];
        match r.i32_(o::STATE) {
            1 | 4 | 5 => {
                if self.globals.game_mode == 0x4012 {
                    5
                } else {
                    1
                }
            }
            2 => match r.i32_(o::SUBSTATE) {
                1 | 2 | 10..=14 => 1,
                3 => 3,
                4 => 6,
                _ => 0,
            },
            3 => 2,
            0xb => 10,
            _ => 0,
        }
    }

    /// setup_player_display 0x134d90.
    pub fn setup_player_display(&mut self, p: usize) {
        let Some(t) = self.hud.tables.clone() else { return };
        if self.hud.players.len() <= p {
            return;
        }
        let pb = self.hud.players[p].clone();
        let lx = t.box_lx[p];
        let mode = self.hud_display_mode(p);
        let colour = self.players[p].u32_(o::COLOUR) as usize;
        // del_player_blits is not exported; the panel blits are re-set below.
        let mut class = self.players[p].u32_(o::CLASS) as usize;
        self.players[p].set_i32(o::DISPLAY_MODE, mode);
        let state = self.players[p].i32_(o::STATE);
        let h = &mut self.hud;
        let tex = h.find(S3);
        h.set_tex(pb.bkg[0], tex, 0);
        h.set_hide(pb.bkg[0], false);
        let tex = h.find(S4);
        h.set_tex(pb.bkg[1], tex, 0);
        h.set_hide(pb.bkg[1], false);
        let tint = if state == 0 { &t.player_inactive_rgb } else { &t.player_rgb };
        h.set_colour(pb.bkg[1], tint.get(colour).copied().unwrap_or(0));
        let tex = h.find(S4_FRAME);
        h.set_tex(pb.bkg[2], tex, 0);
        h.set_hide(pb.bkg[2], false);
        let tex = h.find(COIN);
        h.set_tex(pb.bkg[4], tex, 0);
        h.set_hide(pb.bkg[4], false);
        h.set_pos(pb.bkg[4], 63990.0, lx + 6, 0x165);
        let tex = h.find(HEART);
        h.set_tex(pb.bkg[5], tex, 0);
        h.set_hide(pb.bkg[5], false);
        h.set_pos(pb.bkg[5], 63990.0, lx + 0x3d, 0x165);
        match mode {
            0 | 3 => {
                h.set_hide(pb.bkg[4], true);
                h.set_hide(pb.bkg[5], true);
            }
            1 | 2 | 5 | 6 => {
                if mode == 6 {
                    class = self.players[p].u32_(o::CLASS_ALT) as usize;
                }
                h.set_hide(pb.bkg[4], false);
                h.set_hide(pb.bkg[5], false);
                let tex = h.find("BK_RUNE_STONE_02");
                h.set_tex(pb.bkg[0], tex, 0);
                let tex = h.find(&format!("S4_{}", t.player_desc_uc.get(class).cloned().unwrap_or_default()));
                h.set_tex(pb.bkg[1], tex, 0);
                if self.globals.game_mode & 0x8000 == 0 {
                    let runes = self.players[p].u16_(o::RUNES);
                    for (i, &b) in pb.rune.iter().enumerate() {
                        h.set_hide(b, (runes >> i) & 1 == 0);
                    }
                }
                if (1 << p) & self.globals.active_players != 0 {
                    h.set_hide(pb.level, false);
                }
                let tex = h.find(QUEST_ICON);
                h.set_tex(pb.quest, tex, 0);
            }
            _ => {}
        }
        if matches!(self.globals.game_mode, 0x400b | 0x4012) {
            h.set_flags(pb.bkg[0], u32::MAX, 0x4010);
            h.set_flags(pb.bkg[2], u32::MAX, 0x4010);
        } else {
            h.set_flags(pb.bkg[0], !0x4010, 0);
            h.set_flags(pb.bkg[2], !0x4010, 0);
        }
    }

    /// SetPlayerWindows 0x12b868: alpha 0xff (hidden) or 0 on every panel blit.
    pub fn set_player_windows(&mut self, hide: bool) {
        let a = if hide { 0xff } else { 0 };
        for pb in self.hud.players.clone() {
            for b in pb.bkg.iter().chain(&pb.rune).chain(&pb.key).chain(&pb.ability).chain([&pb.level, &pb.rune13, &pb.quest]).chain(&pb.turbo) {
                self.hud.set_alpha(*b, a);
            }
        }
        self.hud.windows_hidden = i32::from(hide);
    }

    /// WritePlayerInfo 0x1368c0 (`player` < 0: every player, then the rune
    /// stones and the "It" marker).
    pub fn write_player_info(&mut self, player: i32) -> R<()> {
        if self.hud.tables.is_none() || self.hud.players.len() < 4 {
            return Ok(());
        }
        let (from, to) = if player < 0 {
            (0, 4)
        } else {
            if (1 << player) & self.globals.active_players == 0 {
                return Ok(());
            }
            (player as usize, player as usize + 1)
        };
        for p in from..to {
            if (1 << p) & self.globals.active_players == 0 || self.players[p].u16_(o::WRITTEN) & 1 != 0 {
                continue;
            }
            self.players[p].or_u16(o::WRITTEN, 1);
            match self.players[p].i32_(o::DISPLAY_MODE) {
                0 | 3 => {}
                6 | 10 => self.write_health_and_items(p)?,
                _ => {
                    self.write_health_and_items(p)?;
                    self.show_crystals(p);
                }
            }
        }
        if player < 0 {
            self.show_rune_stones();
            if let Some(b) = self.hud.it_blit.take() {
                self.hud.remove(b);
            }
            let it = self.globals.it;
            if 0 <= it && (1 << it) & self.globals.active_players != 0 {
                let t = self.hud.tables.as_ref().map(|t| t.box_lx[it as usize & 3]).unwrap_or(0);
                let tex = self.hud.find(IT);
                let b = self.hud.new_blit(tex, t + 0x22, -0x15d, -1, -1);
                self.hud.set_size(b, 0x20, 0x20);
                self.hud.set_z(b, PANEL_Z);
                self.hud.it_blit = Some(b);
            }
        }
        Ok(())
    }

    /// write_health_and_items 0x135d38.
    fn write_health_and_items(&mut self, p: usize) -> R<()> {
        let t = self.hud.tables.clone().expect("tables");
        let pb = self.hud.players[p].clone();
        let pal = t.player_pal.get(self.players[p].u32_(o::COLOUR) as usize).copied().unwrap_or(0xffffff);
        let saved_z = std::mem::replace(&mut self.hud.font_z, FONT_Z);
        let gm = self.globals.game_mode;
        let mut drawn = self.hud.menu_on != 0 || self.hud.debug_readout != 0;
        let mut money = true;
        let lx = t.box_lx[p];
        let cx = t.box_cx[p];
        let state = self.players[p].i32_(o::STATE);
        if gm == 0x4010 && 0 < self.hud.num_live_players && state == 0xb && self.players[p].i32_(o::SUBSTATE) == 1 {
            drawn = true;
            if let Some(x) = self.hud.button_x {
                self.hud.temp_blit(Some(x), lx + 6, 0x14c, 0xe, 0xe);
            }
            if let Some(tri) = self.hud.button_tri {
                self.hud.temp_blit(Some(tri), lx + 6, 0x160, 0xe, 0xe);
            }
            let s = f32::from_bits(0x3f99_999a);
            self.hud.draw_text(s, lx + 0x14, 0x150, 1, 0xffffff, WAIT_IN_TOWER);
            self.hud.draw_text(s, lx + 0x14, 0x164, 1, 0xffffff, QUIT_GAME);
            money = false;
        }
        let state = self.players[p].i32_(o::STATE);
        if (state == 5 || state == 0xb) && !matches!(gm, 0x400d | 0x4013 | 0x4017) && !drawn {
            drawn = true;
            self.setup_player_display(p);
            if self.players[p].i32_(o::STATE) == 0xb {
                self.hud.draw_text(f32::from_bits(0x3f99_999a), -cx, 0x154, 1, pal, IN_TOWER);
            } else {
                drawn = false;
            }
        }
        let show = money && self.hud.windows_hidden == 0;
        self.write_gold(p, show);
        self.write_health(p, show);
        let mode = self.players[p].i32_(o::DISPLAY_MODE);
        let name = |g: &Game| -> String {
            let b = &g.players[p].0[o::NAME..o::NAME + 0x20];
            cstr(b)
        };
        let small = f32::from_bits(0x3f2a_c083);
        match mode {
            1 | 2 | 5 => {
                if matches!(mode, 1 | 2) && self.hud.debug_readout != 0 && self.players[p].i32_(o::STATE) == 1 {
                    drawn = true;
                    // debug_player_pos / "XP: %d" debug readouts (0x20f968).
                    if self.hud.debug_readout != 1 {
                        let xp = self.players[p].i32_(o::XP);
                        self.hud.draw_text(1.0, lx + 8, 0x154, 1, 0xffffff, &format!("XP: {xp}"));
                    }
                }
                if !drawn {
                    let n = name(self);
                    self.hud.draw_text(small, -cx, 0x153, 7, pal, &n);
                }
                if self.hud.menu_on != 0 || !drawn {
                    let lv = self.players[p].i32_(o::LEVEL);
                    self.hud.draw_text(1.0, -cx, 0x146, 1, 0xffffff, &format!("LV {lv}"));
                }
            }
            6 if !drawn => {
                let n = name(self);
                self.hud.draw_text(small, -cx, 0x153, 7, pal, &n);
            }
            _ => {}
        }
        // del_item_blits is empty in this build.
        let health = self.players[p].f32_(o::HEALTH);
        if self.players[p].i32_(o::STATE) != 2 && self.players[p].i32_(o::DISPLAY_MODE) != 0 && self.hud.windows_hidden == 0 {
            let keys = self.players[p].i32_(o::KEYS);
            if 0 < keys {
                let b = self.hud.temp_blit(self.hud.key_tex, lx + 8, 0x143, -1, -1);
                self.hud.temp[b].z = PANEL_Z;
                self.hud.draw_text(f32::from_bits(0x3f4c_cccd), lx + 0x1a, 0x147, 4, pal, &keys.to_string());
            }
            let potions = self.players[p].i32_(o::POTIONS);
            if 0 < potions {
                let kind = self.players[p].i32_(o::POTION_SLOTS + 4 * (potions as usize - 1).min(0x1f));
                let tex = usize::try_from(kind).ok().and_then(|k| self.hud.potion_tex.get(k).copied().flatten());
                let b = self.hud.temp_blit(tex, lx + 0x66, 0x143, -1, -1);
                self.hud.temp[b].z = PANEL_Z;
                self.hud.draw_text(f32::from_bits(0x3f4c_cccd), lx + 0x5c, 0x147, 4, pal, &potions.to_string());
            }
        }
        if health <= 0.0 || self.hud.full_screen_inventory != 0 {
            for b in pb.turbo {
                self.hud.set_hide(b, true);
            }
        } else {
            let quest = gm == 0x4010 && self.players[p].i32_(o::QUEST) != 0 && self.globals.cur_world != 0xd;
            self.hud.set_hide(pb.quest, !quest);
            let rune13 = gm.wrapping_sub(0x400f) < 2 && self.players[p].u16_(o::RUNES) & 0x1000 != 0;
            self.hud.set_hide(pb.rune13, !rune13);
            if gm == 0x4010 {
                self.draw_power_meter(p);
            }
        }
        self.hud.font_z = saved_z;
        Ok(())
    }

    /// write_gold 0x135878 (gold capped at 99999).
    fn write_gold(&mut self, p: usize, show: bool) {
        let t = self.hud.tables.clone().expect("tables");
        let b = self.hud.players[p].bkg[4];
        if self.players[p].i32_(o::GOLD) > 99999 {
            self.players[p].set_i32(o::GOLD, 99999);
        }
        if !show {
            self.hud.set_hide(b, true);
            return;
        }
        let s = self.players[p].i32_(o::GOLD).to_string();
        let w = self.hud.string_width(4, 1.0, s.as_bytes());
        let pal = t.player_pal.get(self.players[p].u32_(o::COLOUR) as usize).copied().unwrap_or(0xffffff);
        self.hud.draw_text(1.0, t.box_lx[p] - (w - 0x3c), 0x167, 4, pal, &s);
        self.hud.set_hide(b, false);
    }

    /// write_health 0x1359a0 (health capped at 9999.0, drawn truncated).
    fn write_health(&mut self, p: usize, show: bool) {
        let t = self.hud.tables.clone().expect("tables");
        let b = self.hud.players[p].bkg[5];
        if 9999.0 < self.players[p].f32_(o::HEALTH) {
            self.players[p].set_f32(o::HEALTH, 9999.0);
        }
        if !show {
            self.hud.set_hide(b, true);
            return;
        }
        let s = (self.players[p].f32_(o::HEALTH) as i32).to_string();
        let w = self.hud.string_width(4, 1.0, s.as_bytes());
        let pal = t.player_pal.get(self.players[p].u32_(o::COLOUR) as usize).copied().unwrap_or(0xffffff);
        self.hud.draw_text(1.0, t.box_lx[p] - (w - 0x74), 0x167, 4, pal, &s);
        self.hud.set_hide(b, false);
    }

    /// draw_power_meter 0x135390: the shown power (+0x804) follows +0x800
    /// (up vb_elapsed, down 2·vb_elapsed per frame); state changes start the
    /// gleam (1) or, full, the glow loop (2); otherwise the bar is coloured.
    fn draw_power_meter(&mut self, p: usize) {
        let t = self.hud.tables.clone().expect("tables");
        let tb = self.hud.players[p].turbo;
        for &b in &tb {
            self.hud.set_hide(b, true);
        }
        let r = &mut self.players[p];
        let (s0, _) = power_bar_state(r.f32_(o::POWER_SHOWN));
        if 100.0 < r.f32_(o::POWER) {
            r.set_f32(o::POWER, 100.0);
        }
        let vb = self.globals.vb_elapsed;
        let (target, shown) = (r.f32_(o::POWER), r.f32_(o::POWER_SHOWN));
        let next = if shown < target { (shown + vb as f32).min(target) } else { (shown - (vb * 2) as f32).max(target) };
        r.set_f32(o::POWER_SHOWN, next);
        let (s1, frac) = power_bar_state(next);
        let restart = if s0 == s1 { s1 == 3 && r.i32_(o::METER_ANIM) == 0 } else { true };
        if restart {
            r.set_i32(o::METER_ANIM, if s1 == 3 { 2 } else { 1 });
            r.set_i32(o::METER_TIME, 0);
        }
        for k in [1, 0, 2] {
            self.hud.set_hide(tb[k], false);
        }
        let time = self.players[p].i32_(o::METER_TIME);
        match self.players[p].i32_(o::METER_ANIM) {
            1 => {
                let mut i = time >> 2;
                if (i as u32).wrapping_sub(5) < 5 {
                    i = 9 - i;
                }
                if i < 5 {
                    self.hud.set_hide(tb[6], false);
                    let base = self.hud.tb_tex.get(6).copied().flatten();
                    self.hud.set_tex(tb[6], base, i);
                } else {
                    self.players[p].set_i32(o::METER_ANIM, 0);
                }
            }
            0 => {
                let v = (frac * 127.0 + 128.0) as u32 & 0xff;
                let (c0, c1) = match s1 {
                    1 => ((v << 16) | (v << 8), 0),
                    2 | 3 => (v << 16, 0xffff00),
                    _ => (0, 0),
                };
                let width = self.hud.blits[tb[0]].as_ref().and_then(|b| b.tex).and_then(|x| self.hud.tex_info(x)).map(|i| i32::from(i.width)).unwrap_or(0);
                let half = (((width as f32) * frac) as i32 >> 1).max(1);
                let (_, x0, y0, z0) = t.tb_info[0].clone();
                let (_, x1, y1, z1) = t.tb_info[1].clone();
                self.hud.set_pos(tb[0], z0 as f32, p as i32 * 0x80 + x0 + (width >> 1) - half, y0);
                self.hud.set_size(tb[0], half << 1, 0);
                self.hud.set_pos(tb[1], z1 as f32, p as i32 * 0x80 + x1, y1);
                self.hud.set_colour(tb[0], c0);
                self.hud.set_colour(tb[1], c1);
            }
            2 => {
                let mut i = (time << 9) / 0x78;
                if (i as u32).wrapping_sub(0x100) < 0x100 {
                    i = 0x1ff - i;
                }
                if i < 0x100 {
                    self.hud.set_hide(tb[3], false);
                    self.hud.set_alpha(tb[3], (0xff - i) as u32);
                } else {
                    self.players[p].set_i32(o::METER_TIME, 0);
                }
                self.hud.set_colour(tb[0], 0xff0000);
                self.hud.set_colour(tb[1], 0xff0000);
            }
            _ => {}
        }
        let time = self.players[p].i32_(o::METER_TIME);
        self.players[p].set_i32(o::METER_TIME, time + vb);
    }

    /// show_crystals 0x136690: the item just collected (+0x900) for +0x904 s.
    fn show_crystals(&mut self, p: usize) {
        if 0 < self.globals.rune_disp_timer || self.hud.optmenu_active != 0 || self.globals.game_mode != 0x4010 {
            return;
        }
        let t = self.hud.tables.clone().expect("tables");
        let r = &mut self.players[p];
        if r.f32_(o::SHOW_TIME) <= 0.0 {
            return;
        }
        let spf = self.globals.spf;
        r.set_f32(o::SHOW_TIME, r.f32_(o::SHOW_TIME) - spf);
        let item = r.i32_(o::SHOW_ITEM);
        let cls = 0xb4 * r.u32_(o::CLASS) as usize;
        let (name, have, max) = if item < 0x100 {
            let n = item.max(0) as usize;
            let max = t.max_crystals.get(n).copied().unwrap_or(0);
            let c = i32::from(r.i16_(o::CRYSTALS + cls + 2 * n));
            let have = if c < 0 { max } else { c };
            (format!("SM_CRYSTAL_{}", t.crystal_desc.get(n).cloned().unwrap_or_default()), have, max)
        } else if item < 0x200 {
            let n = (item - 0x100) as usize;
            let have = i32::from(r.i16_(o::GARG_ITEMS + cls + 2 * n));
            (format!("SM_{}", t.garg_item_desc.get(n).cloned().unwrap_or_default()), have, t.max_garg_items.get(n).copied().unwrap_or(0))
        } else {
            let have = r.i32_(o::SHOW_COUNT);
            let n = (item - 0x200) as usize;
            let name = if 0xf < n { "16_SUM".to_string() } else { format!("16_{}COIN", t.player_desc_uc.get(n).cloned().unwrap_or_default()) };
            (name, have, self.globals.max_level_coins)
        };
        // tbuf[14] = 0 before MBOX_FindTexture: names are cut to 14 bytes.
        let name: String = name.chars().take(14).collect();
        let tex = self.hud.find(&name);
        let lx = t.box_lx[p];
        self.hud.temp_blit(tex, lx + 0x1c, 0x120, 0x10, 0x10);
        let shown = if -1 < have { have } else { max };
        self.hud.draw_text(1.5, lx + 0x30, 0x124, 1, 0xffffff, &format!("{shown}/{max}"));
    }

    /// ShowRuneStones 0x136418: while RuneDispTimer runs, the key icons (the
    /// class record's +0xcf0 bits) and rune stones (+0x1a26 bits).
    fn show_rune_stones(&mut self) {
        let players = self.hud.players.clone();
        if self.globals.rune_disp_timer < 1 || self.hud.optmenu_active != 0 {
            for pb in &players {
                for &b in &pb.key {
                    self.hud.set_hide(b, true);
                }
            }
            return;
        }
        self.globals.rune_disp_timer = (self.globals.rune_disp_timer - self.globals.vb_elapsed).max(0);
        for (k, pb) in players.iter().enumerate() {
            let state = self.players[k].i32_(o::STATE);
            if self.players[k].u16_(o::WRITTEN) & 2 != 0 {
                continue;
            }
            self.players[k].or_u16(o::WRITTEN, 2);
            if matches!(state, 1 | 2 | 4 | 5) {
                let cls = 0xb4 * self.players[k].u32_(o::CLASS) as usize;
                let bits = self.players[k].u16_(o::KEYS_CLASS + cls);
                for (i, &b) in pb.key.iter().enumerate() {
                    self.hud.set_hide(b, (bits >> i) & 1 == 0);
                }
            } else {
                for &b in &pb.key {
                    self.hud.set_hide(b, true);
                }
            }
            if self.globals.game_mode & 0x8000 == 0 && (state == 1 || state == 5) {
                let runes = self.players[k].u16_(o::RUNES);
                for (i, &b) in pb.rune.iter().enumerate() {
                    self.hud.set_hide(b, (runes >> i) & 1 == 0);
                }
            }
        }
    }

    /// NewHelp 0x18e710 (`player` None = −1; `at` the world point placing
    /// the box above it via MBWorldToScreen, else centred or over the box).
    pub fn new_help(&mut self, id: i32, player: Option<usize>, at: Option<[f32; 3]>) -> R<()> {
        let Some(t) = self.hud.tables.clone() else { return Ok(()) };
        if self.hud.text.is_none() || !(0..0x100).contains(&id) {
            return Ok(());
        }
        let Some(&h) = t.helptab.get(id as usize) else { return Ok(()) };
        if h[4] < 0 || self.globals.trig_cam_active != 0 {
            return Ok(());
        }
        // help_displayed: bit 0x100 while every active player has seen it.
        let seen = |g: &Game, p: Option<usize>| -> u32 {
            let mut v = 0x100u32;
            for k in p.map_or(0..4, |k| k..k + 1) {
                if g.players[k].i32_(o::STATE) != 0 {
                    let b = u32::from(g.players[k].u8_(o::HELP_SEEN + id as usize));
                    v = if b == 0 { v & 0xff } else { v | b };
                }
            }
            v
        };
        let blocked = match h[3] {
            0 => false,
            1 => seen(self, None) & 0xf != 0,
            2 => seen(self, player) != 0,
            _ => seen(self, None) & 0x100 != 0,
        };
        if blocked {
            return Ok(());
        }
        let hs = &self.hud.help;
        if hs.flag && h[2] <= t.helptab.get(hs.num as usize).map_or(0, |c| c[2]) {
            return Ok(());
        }
        let gm = self.globals.game_mode;
        if (id < 0x1d || matches!(id, 0x2c | 0x2d | 0x37 | 0x50)) && gm != 0x8006 && gm != 0x8003 && 0 < hs.interval {
            return Ok(());
        }
        let dy = 8; // low_res 0xc, Japanese 0xe
        let style = player.unwrap_or(4);
        let p = player.unwrap_or(0);
        let (mut cx, mut cy) = match at {
            None => match player {
                None => (0x100, 0xc0),
                Some(k) => (t.box_cx[k & 3], 0xfa),
            },
            Some(pos) => {
                let s = match self.std_camera.as_mut() {
                    Some(cam) => {
                        let w = cam.window.clone();
                        cam.pb.world_to_screen(&w, pos)?
                    }
                    None => [256.0, 192.0, 1.0],
                };
                (s[0] as i32, s[1] as i32 - 0x3e)
            }
        };
        let lines = if h[5] < 0 { self.hud.entry_lines(h[4] as usize).len() as i32 } else { 1 };
        let bh = self.hud.string_text_height(1.0, h[4], h[5], dy) + 0x10;
        let mut by = cy - (bh >> 1);
        if by < 2 {
            cy = cy + 2 - by;
            by = 2;
        } else if 0x130 < by + bh {
            cy = cy + 0x130 - (by + bh);
            by = 0x130 - bh;
        }
        // msg_width (with the previous help_player, as the original reads it).
        let prev = self.hud.help.player;
        let mut bw = self.hud.string_text_width(1.0, h[4], h[5]);
        if matches!(id, 0x32 | 0x59 | 0x5d) {
            let c = self.players[prev].i32_(o::CLASS);
            bw = bw.max(self.hud.string_text_width(1.0, 2, prev as i32) + self.hud.string_text_width(1.0, 3, c) + 0xc);
        }
        let bw = bw + 0x40;
        let mut bx = cx - (bw >> 1);
        if bx < 0 {
            cx -= bx;
            bx = 0;
        } else if 0x1ff < bx + bw {
            cx = cx + 0x1ff - (bx + bw);
            bx = 0x1ff - bw;
        }
        if let Some(b) = self.hud.help.border.take() {
            self.hud.remove(b);
        }
        let tex = t.border_names.get(style).and_then(|n| self.hud.find(n));
        let b = self.hud.new_blit(tex, bx, by, -1, -1);
        self.hud.set_size(b, bw, bh);
        self.hud.set_alpha(b, 0x40);
        self.hud.set_hide(b, false);
        let lv = self.players[p].i32_(o::LEVEL);
        let hs = &mut self.hud.help;
        hs.border = Some(b);
        hs.x = cx;
        hs.y = cy;
        hs.colour = t.text_colors.get(style).copied().unwrap_or(0xffffff);
        hs.lv = lv;
        hs.flag = true;
        hs.num = id;
        hs.player = p;
        self.write_help_text();
        self.hud.help.time = lines * 0x3c + 0x1e;
        if h[3] != 0 {
            // set_help_displayed: |= 0x11 for the player (rule 2) or all.
            let range = if h[3] == 2 { p..p + 1 } else { 0..4 };
            for k in range {
                if self.players[k].i32_(o::STATE) != 0 {
                    let v = self.players[k].u8_(o::HELP_SEEN + id as usize) | 0x11;
                    self.players[k].set_u8(o::HELP_SEEN + id as usize, v);
                }
            }
        }
        let kind = if gm == 0x8003 || self.hud.help.testflag { -1 } else { h[1] };
        self.globals.help = match kind {
            -1 => 0x3c,
            1 => 0x1e,
            _ => 0,
        };
        if h[6] != 0 {
            if id == 0x65 && 9 < self.hud.help.lv {
                self.audio("AudioExp", player, self.hud.help.lv.min(99));
            } else {
                self.audio("AudioHelp", player, h[6]);
            }
        }
        if self.globals.aux_levels & 0x10 == 0 {
            if gm == 0x8003 || gm == 0x8006 {
                self.hud.help.interval = 0x3c;
            } else {
                let i = self.hud.help.interval_idx;
                self.hud.help.interval = t.announcer_interval.get(i).copied().unwrap_or(0);
                if 0 <= t.announcer_interval.get(i + 1).copied().unwrap_or(-1) {
                    self.hud.help.interval_idx += 1;
                }
            }
        } else {
            self.hud.help.interval = 0;
        }
        Ok(())
    }

    /// write_text 0x18e120: the current help message (centred on help_x,
    /// help_y | 0x1000).
    fn write_help_text(&mut self) {
        let Some(t) = self.hud.tables.clone() else { return };
        let hs = self.hud.help.clone();
        let Some(&h) = t.helptab.get(hs.num as usize) else { return };
        if h[4] < 0 {
            return;
        }
        let (x, y, c) = (-hs.x, hs.y | 0x1000, hs.colour);
        let class = self.players[hs.player].i32_(o::CLASS);
        let colour_idx = self.players[hs.player].i32_(o::COLOUR);
        let name = |hud: &Hud, e: i32, l: i32| Arg::S(hud.string_text(e, l).unwrap_or_default());
        if hs.num == 0x65 && 9 < hs.lv {
            let rank = if hs.lv == 99 {
                self.hud.string_text(0x15, 0)
            } else {
                let list = self.hud.text.as_ref().and_then(|r| r.lists.first()).and_then(|(_, v)| v.get(class as usize).copied());
                list.and_then(|e| self.hud.string_text(e as i32, (hs.lv / 10) >> 1))
            }
            .unwrap_or_default();
            let step = self.hud.string_text_height(1.0, 0x18, 0, 2) + 2;
            let (cn, kn) = (name(&self.hud, 2, colour_idx), name(&self.hud, 3, class));
            self.hud.draw_string_text(x, y - step, -1, c, 0x18, 0, &[cn, kn]);
            self.hud.draw_string_text(x, y, -1, c, 0x18, 1, &[]);
            self.hud.draw_string_text(x, y + step, -1, c, 0x18, 2, &[Arg::D(hs.lv), Arg::S(rank)]);
        } else if hs.num == 0x22 {
            self.hud.draw_string_text_mlines(x, y, 2, c, h[4], &[Arg::D(hs.lv)]);
        } else if matches!(hs.num, 0x32 | 0x59 | 0x5d) {
            let (cn, kn) = (name(&self.hud, 2, colour_idx), name(&self.hud, 3, class));
            let step = (self.hud.string_text_height(1.0, h[4], 0, 2) + 2) >> 1;
            if self.players[hs.player].u32_(0x138) & 0x400 == 0 || hs.num == 0x5d {
                self.hud.draw_string_text(x, y - step, 1, c, h[4], 0, &[cn, kn]);
            } else {
                self.hud.draw_string_text(x, y - step, -1, c, 4, 0, &[]);
            }
            self.hud.draw_string_text(x, y + step, 1, c, h[4], 1, &[]);
        } else if h[5] < 0 {
            self.hud.draw_string_text_multi(x, y, 2, c, h[4]);
        } else {
            self.hud.draw_string_text(x, y, -1, c, h[4], h[5], &[]);
        }
    }

    /// DoHelp 0x18ed58 (do_players, before the player loop): the interval,
    /// the help pause (`help`, which stops do_players) and the message's
    /// lifetime (lines × 60 + 30 vblanks).
    pub fn do_help(&mut self) {
        let vb = self.globals.vb_elapsed;
        let paused = self.globals.paused != 0;
        if !paused {
            self.hud.help.interval = (self.hud.help.interval - vb).max(0);
        }
        if !(0..4).any(|k| matches!(self.players[k].i32_(o::STATE), 1 | 2 | 3 | 5)) {
            self.hud.help.time = 0;
        }
        if !self.hud.help.flag {
            return;
        }
        let border = self.hud.help.border;
        if self.hud.optmenu_active != 0 || self.globals.trig_cam_active != 0 {
            if let Some(b) = border {
                self.hud.set_hide(b, true);
            }
            return;
        }
        if !paused {
            self.globals.help = (self.globals.help - vb).max(0);
            self.hud.help.time -= vb;
            if self.hud.help.time < 1 {
                if let Some(b) = self.hud.help.border.take() {
                    self.hud.remove(b);
                }
                self.hud.help.flag = false;
                self.globals.help = 0;
                self.hud.help.testflag = false;
                return;
            }
        } else if self.hud.help.time < 1 {
            return;
        }
        self.write_help_text();
        if let Some(b) = border {
            self.hud.set_hide(b, false);
        }
    }

    /// do_got_it 0x12b228: a notice's S3 bar and picture (by kind and
    /// amount) slide up from y 0x180 to 0x130 over the player's box, stay 90
    /// vblanks and slide back below 399.
    pub fn do_got_it(&mut self) {
        let Some(t) = self.hud.tables.clone() else { return };
        let vb = self.globals.vb_elapsed;
        for i in 0..self.hud.got_it.len() {
            let g = self.hud.got_it[i].clone();
            match g.state {
                1 => {
                    let x = t.box_lx[g.player & 3];
                    let name = match g.kind {
                        1 if self.globals.cur_world == 0xc => "COINHUD",
                        1 if g.amount < 0xb => "JUNK",
                        1 => "GOLD",
                        2 if 1 < g.amount => "KEY_RING",
                        2 => "KEY",
                        3 if 100 <= g.amount => "MEAT",
                        3 if g.amount < -99 => "BADMEAT",
                        3 if g.amount < 0 => "BADFRUIT",
                        3 => "FRUIT",
                        4 => "MAGIC",
                        5..=9 => "SPECIALS",
                        10 => "RUNESTONE",
                        0xd => "LEGEND",
                        0xf => "CRYSTAL",
                        0x10 => "GOLDNICON",
                        _ => {
                            // The original returns from do_got_it here.
                            self.hud.got_it[i].state = 0;
                            return;
                        }
                    };
                    let s3 = self.hud.find(S3);
                    let bar = self.hud.new_blit(s3, x, 0, -1, -1);
                    let tex = self.hud.find(name);
                    let pic = self.hud.new_blit(tex, x, 0, -1, -1);
                    self.hud.set_size(bar, 0x80, 0);
                    self.hud.set_pos(bar, 63980.0, x, 0x180);
                    self.hud.set_size(pic, 0x80, 0);
                    self.hud.set_pos(pic, 63979.0, x, 400);
                    let n = &mut self.hud.got_it[i];
                    n.bar = Some(bar);
                    n.picture = Some(pic);
                    n.state = 2;
                }
                2 | 4 => {
                    let Some(bar) = g.bar else { continue };
                    let y0 = self.hud.blits.get(bar).and_then(|b| b.as_ref()).map_or(0x180, |b| b.y as i32);
                    let mut y = if g.state == 2 { y0 - vb } else { y0 + vb };
                    if g.state == 2 && y < 0x131 {
                        y = 0x130;
                        self.hud.got_it[i].state = 3;
                        self.hud.got_it[i].timer = 0x5a;
                    } else if g.state == 4 && 399 < y {
                        self.hud.got_it[i].state = -1;
                    }
                    for (b, dy) in [(Some(bar), 0), (g.picture, 0x10)] {
                        if let Some(b) = b
                            && let Some(bl) = self.hud.get(b)
                        {
                            bl.y = (y + dy) as f32;
                        }
                    }
                }
                3 => {
                    self.hud.got_it[i].timer -= vb;
                    if self.hud.got_it[i].timer < 1 {
                        self.hud.got_it[i].state = 4;
                    }
                }
                -1 => {
                    for b in [g.bar, g.picture].into_iter().flatten() {
                        self.hud.remove(b);
                    }
                    let n = &mut self.hud.got_it[i];
                    n.state = 0;
                    n.bar = None;
                    n.picture = None;
                }
                _ => {}
            }
        }
    }

    /// The client's textures and fonts, then reset_players' panel part and
    /// init_next_level's (setup_player_display for every box,
    /// SetPlayerWindows(0)).
    pub fn hud_init(&mut self, textures: Vec<TexSet>, fonts: Vec<Option<Font>>, text: Option<gdl_formats::textrom::TextRom>) {
        self.hud.missing.clear();
        self.hud.text = text;
        self.hud.set_resources(textures, fonts);
        self.hud_reset_players();
        for p in 0..4 {
            self.setup_player_display(p);
        }
        self.set_player_windows(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_packing_and_power_states() {
        assert_eq!(pack_rgb(0x787800, 0x80), [61, 61, 1, 0x80]);
        assert_eq!(pack_rgb(0x1e1e69, 0x80), [16, 16, 53, 0x80]);
        assert_eq!(pack_rgb(0xffff80, 0x80), [128, 128, 65, 0x80]);
        let (s, f) = power_bar_state(20.0);
        assert_eq!(s, 1);
        assert!((f - 0.5).abs() < 1e-6);
        assert_eq!(power_bar_state(100.0), (3, 1.0));
        let (s, f) = power_bar_state(70.0);
        assert_eq!(s, 2);
        assert!((f - 0.5).abs() < 1e-6);
    }

    fn font() -> Font {
        Font {
            height: 16,
            glyphs: vec![
                gdl_formats::fnt::Glyph { code: 48, width: 8, x: 0, y: 0 },
                gdl_formats::fnt::Glyph { code: 53, width: 8, x: 40, y: 0 },
            ],
        }
    }

    #[test]
    fn right_aligned_text_and_glyph_blits() {
        let mut h = Hud { font_z: FONT_Z, ..Default::default() };
        let info = |l: &str| TexInfo { label: l.into(), width: 128, height: 16, flags: 0x8c, def_width: 128, def_height: 16 };
        h.set_resources(vec![TexSet { name: "STATIC".into(), records: vec![info("SCORE")], names: vec![("SCORE".into(), 0)] }], vec![]);
        h.fonts = vec![None, None, None, None, Some(HudFont { font: font(), tex: h.index.get("SCORE").copied(), space: 9, per_page: 0x80 })];
        // write_health: "500" right-aligned at box_lx + 0x74.
        let w = h.string_width(4, 1.0, b"500");
        assert_eq!(w, 24);
        h.draw_text(1.0, 0x74 - w, 0x167, 4, 0xffff80, "500");
        let list = h.draw_list();
        assert_eq!(list.len(), 3);
        assert_eq!((list[0].x, list[0].y, list[0].w, list[0].h), (92.0, 359.0, 8.0, 16.0));
        assert_eq!(list[0].crop, [40.0 / 128.0, 48.0 / 128.0, 0.0, 1.0]);
        assert_eq!(list[1].x, 100.0);
        assert_eq!(list[0].rgba, [128, 128, 65, 0x80]);
        assert_eq!(list[0].z, FONT_Z);
        // Centred text: x < 0 centres on −x.
        h.begin_frame();
        h.draw_text(1.0, -0x40, 0x146, 4, 0xffffff, "00");
        assert_eq!(h.messages[0].x, 0x40 - 8);
    }

    #[test]
    fn texture_sized_blits_follow_texture_changes() {
        let mut h = Hud::default();
        let info = |l: &str, w, hh| TexInfo { label: l.into(), width: w, height: hh, flags: 0x8c, def_width: w, def_height: hh };
        let records = vec![info("S3", 128, 16), info("S4", 128, 64), info("G1", 16, 16), info("G2", 16, 16)];
        let names = records.iter().enumerate().map(|(i, r)| (r.label.clone(), i as u16)).collect();
        h.set_resources(vec![TexSet { name: "S".into(), records, names }], vec![]);
        let b = h.new_blit(None, 0, 0x140, 0x80, -1);
        assert_eq!(h.blits[b].as_ref().unwrap().h, 0.0);
        let s4 = h.find("s4");
        h.set_tex(b, s4, 0);
        let bl = h.blits[b].as_ref().unwrap();
        assert_eq!((bl.w, bl.h), (128.0, 64.0));
        let g = h.find("G1");
        let t = h.new_blit(g, 0, 0, -1, -1);
        h.set_tex(t, g, 1);
        let g2 = h.find("G2");
        assert_eq!(h.blits[t].as_ref().unwrap().tex, g2);
        h.set_alpha(t, 0xff);
        assert_eq!(h.blits[t].as_ref().unwrap().rgba[3], 1);
    }

    #[test]
    fn scroll_box_pages_then_burns() {
        let mut sb = ScrollBox::new(vec!["a".into(), "b".into()], 6, 1.0, 1.0);
        // The 15-vblank delay ignores the button.
        assert!(sb.step(2, true));
        assert_eq!(sb.page, 0);
        for _ in 0..7 {
            sb.step(2, false);
        }
        assert!(sb.step(2, true));
        assert_eq!((sb.page, sb.burn), (1, None));
        for _ in 0..8 {
            sb.step(2, false);
        }
        sb.step(2, true);
        assert_eq!(sb.burn, Some(0));
        // 21 frames, one per two vblanks.
        let mut n = 0;
        while sb.step(2, false) {
            n += 1;
        }
        assert_eq!(n, 20);
    }
}
