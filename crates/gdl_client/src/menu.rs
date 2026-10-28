//! Front-end menu flow (M17 interim presentation, no Bevy types).
//!
//! Structure and labels follow the original menus where they are known:
//! title "Press Start" (0x2abed0) → title_menu 0x21ce30 {Start, Options};
//! title_options 0x21cff0 {Audio, Game Options, Compass, Controls, Screen};
//! in the Tower tower_menu 0x21d1b0 {Settings, Manage Character, Shop,
//! Inventory, Quit Game}; in a realm game_menu 0x21d4a8 {Settings, Quit
//! Level}; Settings = tower_options 0x21d350 {Audio, Compass, Controls,
//! Screen}. Text from TEXT/ENGLISH.ROM: class and colour names, "SELECT A
//! CHARACTER", "TO BUILD A CHARACTER", "ENTER INITIALS".
//!
//! NATIVE (labelled): the player-select layout (the original SELECT scene,
//! carousel and file browser are not ported), the save list, the import
//! entry, "Save Character" inside Manage Character (the original goes to
//! the player-select screen, whose state 0xe writes the file), the quit
//! confirmation and the volume sliders' 0..=255 range.

/// Menu input, already debounced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Back,
    /// The circle button's "Center" (screen_options' button row).
    Center,
    /// A controller's START (do_titlescreen's new_start: the Start edge
    /// opens the title menu). Other pages ignore it.
    Start,
    Char(char),
    Erase,
}

/// What the host must do after an input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Play the character in save slot `slot`.
    Play { slot: u32 },
    /// Create a character (then play it).
    Create { class: u32, colour: u8, name: String },
    /// Import the PS2 capture character into a new slot.
    Import,
    Resume,
    QuitLevel,
    QuitGame,
    Exit,
    /// The audio sliders (AudioSetVolMusic / AudioSetVolSfx); `store`
    /// writes GamePrefs (the audio menu's end).
    Volumes { music: u8, sfx: u8, store: bool },
    /// The audio menu's Mono / Stereo entry (GamePrefs word 2,
    /// AudioSetStereo).
    Stereo { on: bool },
    /// The title's Start: the select screen (init_player_select(0)).
    StartSelect,
    /// Tower menu → Manage Character (the select screen).
    ManageCharacter,
    /// Tower menu → Shop: the host fills `shop` and opens the page.
    OpenShop,
    /// Buy shop entry `idx` (0 = leave).
    Buy { idx: usize },
    /// Tower menu → Inventory: the host fills `inventory` and opens the page.
    OpenInventory,
    /// A choice menu's entry `idx` accepted (DoOptions cases 0xa..0x11).
    Choose { key: &'static str, idx: usize },
    /// Screen Adjust (DoOptions 0x12): pbScreenSetAdjust(dx, dy); `keep`
    /// stores them in GamePrefs words 3/4 (Accept).
    Screen { dx: i32, dy: i32, keep: bool },
    /// hint_menu entry `list` accepted (DoOptions case 0x16): the host picks
    /// the category's next hint, fills `hints[list]` and `hint_title` and
    /// opens the page.
    ShowHint { list: usize },
    /// NATIVE: an options list's "Display" entry — the host opens the
    /// settings overlay over the menu, which stays on the list.
    Display,
}

/// The option menus' sound cues (do_optmenu 0x10c4f8 and DoOptions
/// 0x10d4e0), for the host to play (AudioFX).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// AudioCursorV 0x160698: up/down in a menu with entries.
    CursorV,
    /// AudioCursorH 0x1606a8: left/right on Control Style (or Mono/Stereo).
    CursorH,
    /// AudioCursorSelect 0x1606c8: an entry accepted in a menu with flag
    /// 0x400 (and the title's Options, DoOptions case 2 0xc).
    Select,
    /// AudioMenuExit 0x1606d8: DoOptions' pop after −1 / −2 (Back).
    Exit,
    /// AudioBuzzer 0x160688: a disabled entry (+0x20 < 0) accepted.
    Buzzer,
    /// AudioPlayerStart 0x160700: a choice stored (DoOptions 0x0010e340),
    /// the audio menu ended, Control Style or Screen Adjust accepted.
    PlayerStart,
    /// AudioVolSfx 0x15f078(1): the SFX slider held at its end
    /// (AudioSFX2(0x14, 0x7f, 0x7f, 1)).
    VolSfxEnd,
    /// AudioVolSfx(0): the SFX slider's sample, snd.286[k] (k alternates).
    VolSfxSample(usize),
}

/// The cues do_optmenu / DoOptions play for `key` on `page`.
fn cues(page: &Page, key: Key) -> &'static [Cue] {
    use Cue::*;
    let vertical = matches!(key, Key::Up | Key::Down);
    match page {
        // title_menu (flags 0): Options plays AudioCursorSelect itself.
        Page::Title { at } => match key {
            _ if vertical => &[CursorV],
            Key::Accept if *at == 1 => &[Select],
            _ => &[],
        },
        // Flag 0x400 menus: select on Accept, the pop's exit on Back.
        Page::Options { .. } | Page::Submenu { .. } | Page::Paused { .. } | Page::Hints { .. } => match key {
            _ if vertical => &[CursorV],
            Key::Accept => &[Select],
            Key::Back => &[Exit],
            _ => &[],
        },
        Page::Choice { .. } => match key {
            _ if vertical => &[CursorV],
            Key::Accept => &[Select, PlayerStart],
            Key::Back => &[Exit],
            _ => &[],
        },
        // The questions (flags 0x60): No is action −1 (a pop).
        Page::ConfirmQuit { at, .. } => match key {
            _ if vertical => &[CursorV],
            Key::Accept if *at == 0 => &[Exit],
            Key::Back => &[Exit],
            _ => &[],
        },
        // end_audiomenu + AudioPlayerStart; bVar4 skips AudioMenuExit.
        Page::Audio { .. } => match key {
            _ if vertical => &[CursorV],
            Key::Back => &[PlayerStart],
            _ => &[],
        },
        Page::ControlStyle { .. } => match key {
            _ if vertical => &[CursorV],
            Key::Left | Key::Right => &[CursorH],
            Key::Accept => &[PlayerStart],
            _ => &[],
        },
        // screen_options has no entries (no cursor sound).
        Page::Screen { .. } => match key {
            Key::Accept => &[PlayerStart],
            Key::Back => &[Exit],
            _ => &[],
        },
        Page::HintText { .. } => match key {
            Key::Back => &[Exit],
            _ => &[],
        },
        _ => &[],
    }
}

/// A save shown in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub slot: u32,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    PressStart,
    Title { at: usize },
    Options { at: usize, back: Box<Page> },
    Audio { at: usize, back: Box<Page> },
    NotPorted { what: String, back: Box<Page> },
    Select { at: usize },
    Class { at: usize },
    Colour { class: u32, at: usize },
    Name { class: u32, colour: u8, name: String },
    Paused { tower: bool, at: usize },
    /// quitgame_question (the Tower's Quit Game) or quitlevel_question (a
    /// realm's Quit Level, DoOptions case 4 action 0x26): No / Yes.
    ConfirmQuit { at: usize, tower: bool },
    Shop { at: usize },
    Inventory,
    /// end_fullscreen_inventory: the exit swirl, then the Tower menu.
    InventoryClose,
    /// hint_menu 0x21ea28 (Sumner's pad): category list, then a category's
    /// hints one at a time.
    Hints { at: usize },
    HintText { list: usize, at: usize },
    /// A list of further option menus (gameoption_options,
    /// controls_options) or a choice menu (compass_options,
    /// difficulty_options, multiplayer_options, control_rumble_options,
    /// control_autoaim_options, control_autoattack_options).
    Submenu { key: &'static str, at: usize, back: Box<Page> },
    Choice { key: &'static str, at: usize, back: Box<Page> },
    /// Control Style (controlstyle_options, DoOptions 0xd/0xe case 0x21):
    /// Left/Right cycle the four styles; Select or Back stores the style
    /// in PlayerControl +0x2c.
    ControlStyle { style: usize, back: Box<Page> },
    /// Screen Adjust (screen_options, DoOptions 0x12): the d-pad moves the
    /// picture (dx ±4 to ±0x80, dy ±1 to ±0x28), Center resets, Accept
    /// keeps, Back restores `saved`.
    Screen { saved: (i32, i32), back: Box<Page> },
}

/// The entries of the submenus and choice menus (ELF records).
fn submenu_entries(key: &str) -> &'static [&'static str] {
    match key {
        "gameoption_options" => &["Difficulty", "Multiplayer Mode"],
        "controls_options" => &["Style ", "Vibration ", "Auto Aim ", "Auto Attack "],
        "compass_options" => &["Hide", "Show"],
        "difficulty_options" => &["Easy", "Medium", "Hard"],
        "multiplayer_options" => &["Normal", "Shots Stun Other Players", "Shots Hurt Other Players"],
        "control_rumble_options" => &["None", "Light", "Medium ", "Maximum "],
        "control_autoaim_options" | "control_autoattack_options" => &["On", "Off"],
        _ => &[],
    }
}

pub const CLASS_NAMES: [&str; 8] = ["WARRIOR", "VALKYRIE", "WIZARD", "ARCHER", "DWARF", "KNIGHT", "SORCERESS", "JESTER"];
pub const COLOUR_NAMES: [&str; 4] = ["YELLOW", "BLUE", "RED", "GREEN"];
const TITLE_MENU: [&str; 2] = ["Start", "Options"];
/// The original options lists plus a NATIVE last entry, "Display" (the
/// settings overlay: fullscreen, widescreen, VSync, …; gdl_play adds it to
/// the drawn menu records too).
const TITLE_OPTIONS: [&str; 6] = ["Audio", "Game Options", "Compass", "Controls", "Screen", DISPLAY];
const TOWER_OPTIONS: [&str; 5] = ["Audio", "Compass", "Controls", "Screen", DISPLAY];
/// game_options 0x21d620 (a realm's Settings).
const GAME_OPTIONS: [&str; 4] = ["Audio", "Controls", "Screen", DISPLAY];
/// NATIVE: the options lists' added entry.
pub const DISPLAY: &str = "Display";
/// The records that get the [`DISPLAY`] entry.
pub const DISPLAY_RECORDS: [&str; 3] = ["title_options", "tower_options", "game_options"];

/// The options list an Options page shows (by the page it returns to).
fn option_names(back: &Page) -> &'static [&'static str] {
    match back {
        Page::Title { .. } => &TITLE_OPTIONS,
        Page::Paused { tower: false, .. } => &GAME_OPTIONS,
        _ => &TOWER_OPTIONS,
    }
}

/// The original menu record a page is drawn with (title_menu, …), its
/// selected entry; None for pages without one (native presentation).
pub fn page_record(page: &Page) -> Option<(&'static str, usize)> {
    Some(match page {
        Page::Title { at } => ("title_menu", *at),
        Page::Options { at, back } => (
            match **back {
                Page::Title { .. } => "title_options",
                Page::Paused { tower: false, .. } => "game_options",
                _ => "tower_options",
            },
            *at,
        ),
        Page::Paused { tower, at } => (if *tower { "tower_menu" } else { "game_menu" }, *at),
        Page::ConfirmQuit { at, tower } => (if *tower { "quitgame_question" } else { "quitlevel_question" }, *at),
        Page::Hints { at } => ("hint_menu", *at),
        Page::Audio { at, .. } => ("audio_options", *at),
        Page::HintText { .. } => ("hint_general_menu", 0),
        Page::Submenu { key, at, .. } | Page::Choice { key, at, .. } => (*key, *at),
        Page::ControlStyle { .. } => ("controlstyle_options", 0),
        Page::Screen { .. } => ("screen_options", 0),
        _ => return None,
    })
}
/// The option-menu stack level a page is drawn at (options_level: 0 for
/// the first menu, +1 per start_optmenu push); None for pages without a
/// menu record.
pub fn page_depth(page: &Page) -> Option<usize> {
    Some(match page {
        Page::Title { .. } | Page::Paused { .. } | Page::Hints { .. } => 0,
        Page::ConfirmQuit { .. } | Page::HintText { .. } => 1,
        Page::Options { back, .. } | Page::Audio { back, .. } | Page::Submenu { back, .. } | Page::Choice { back, .. } | Page::ControlStyle { back, .. } | Page::Screen { back, .. } => page_depth(back)? + 1,
        _ => return None,
    })
}

const TOWER_MENU: [&str; 5] = ["Settings", "Manage Character", "Shop", "Inventory", "Quit Game"];
const GAME_MENU: [&str; 2] = ["Settings", "Quit Level"];
const HINT_MENU: [&str; 4] = ["General Hints", "Guardians", "Items of Legend", "Runestones"];

impl Menu {
    /// The entry a choice menu marks and starts on (start_optmenu's +0x74):
    /// the GamePrefs word or control option; auto aim/attack mark "Off"
    /// (1) for 0.
    pub fn choice_value(&self, key: &str) -> usize {
        let v = match key {
            "compass_options" => self.prefs[6] as i64,
            "difficulty_options" => self.prefs[5] as i64,
            "multiplayer_options" => self.prefs[7] as i64,
            "control_rumble_options" => i64::from(self.controls[1]),
            "control_autoaim_options" => i64::from(self.controls[3] == 0),
            "control_autoattack_options" => i64::from(self.controls[2] == 0),
            _ => 0,
        };
        usize::try_from(v).unwrap_or(0).min(submenu_entries(key).len().saturating_sub(1))
    }
}

pub struct Menu {
    pub page: Page,
    pub saves: Vec<Entry>,
    /// Label of an importable PS2 character, if any.
    pub import: Option<String>,
    pub music: u8,
    pub sfx: u8,
    /// One-line status (save result, errors).
    pub status: String,
    /// Shop lines (entry 0 = leave) and the player's gold.
    pub shop: Vec<String>,
    pub gold: i32,
    /// Inventory text (native summary of the character record).
    pub inventory: String,
    /// Hint categories (hint_menu order) with their hint texts.
    pub hints: Vec<Vec<String>>,
    /// The shown hint's title (a *HINTDESC line; None: "A Hint for You")
    /// and its ROM entry's scale (StringInitSub: × DrawStringScale).
    pub hint_title: Option<String>,
    pub hint_scale: f32,
    /// GamePrefs words (0 sfx, 1 music, … 5 difficulty +0x14, 6 compass
    /// +0x18, 7 multiplayer +0x1c) and player 1's control options
    /// (style +0x2c, vibration +0x30, auto attack +0x38, auto aim +0x34).
    pub prefs: [i32; 8],
    /// pbScreenSetAdjust's current dx/dy (screen_dx/screen_dy).
    pub screen: (i32, i32),
    pub controls: [u32; 4],
    /// OptionsStart: game_menu's Quit Level (action 0x26) gets +0x20 = −1
    /// (disabled) when CurWorld is 0xc.
    pub quit_level_disabled: bool,
    /// Sound cues of the inputs so far (drained by the host).
    pub cues: Vec<Cue>,
    /// do_audiomenu's sfx_sound_count (vblanks since the last SFX slider
    /// sound) and AudioVolSfx's idx.285.
    pub sfx_sound_count: i32,
    pub sfx_sample: usize,
}

fn wrap(at: usize, n: usize, up: bool) -> usize {
    if n == 0 {
        0
    } else if up {
        (at + n - 1) % n
    } else {
        (at + 1) % n
    }
}

fn list(title: &str, items: &[String], at: usize) -> String {
    let mut s = format!("{title}\n\n");
    for (k, i) in items.iter().enumerate() {
        s.push_str(&format!("{} {i}\n", if k == at { ">" } else { " " }));
    }
    s
}

impl Menu {
    pub fn new(music: u8, sfx: u8) -> Self {
        Self {
            page: Page::PressStart,
            saves: Vec::new(),
            import: None,
            music,
            sfx,
            status: String::new(),
            shop: Vec::new(),
            gold: 0,
            inventory: String::new(),
            hints: Vec::new(),
            hint_title: None,
            hint_scale: 1.0,
            prefs: [0; 8],
            screen: (0, 0),
            controls: [0; 4],
            quit_level_disabled: false,
            cues: Vec::new(),
            sfx_sound_count: 0,
            sfx_sample: 0,
        }
    }

    /// The Select page's entries (for the original-style column menu).
    pub fn select_entries(&self) -> Vec<String> {
        // Short labels for the 128-wide column: a save's name, New, Import.
        let mut v: Vec<String> = self.saves.iter().map(|e| e.label.split_whitespace().next().unwrap_or("?").to_string()).collect();
        v.push("New".into());
        if self.import.is_some() {
            v.push("Import".into());
        }
        v.push("Back".into());
        v
    }

    fn select_items(&self) -> Vec<String> {
        let mut v: Vec<String> = self.saves.iter().map(|e| e.label.clone()).collect();
        v.push("Create a new character".into());
        if let Some(i) = &self.import {
            v.push(format!("Import {i}"));
        }
        v.push("Back".into());
        v
    }

    /// Open the in-game pause menu (Tower or realm variant).
    pub fn pause(&mut self, tower: bool) {
        self.page = Page::Paused { tower, at: 0 };
    }

    pub fn is_paused(&self) -> bool {
        // Every page an in-game menu reaches (before 2026-10-28 Screen and
        // ControlStyle were missing: the game resumed under them and Screen
        // Adjust never showed in game).
        matches!(
            self.page,
            Page::Paused { .. }
                | Page::ConfirmQuit { .. }
                | Page::Options { .. }
                | Page::Audio { .. }
                | Page::NotPorted { .. }
                | Page::Shop { .. }
                | Page::Inventory
                | Page::InventoryClose
                | Page::Hints { .. }
                | Page::HintText { .. }
                | Page::Submenu { .. }
                | Page::Choice { .. }
                | Page::ControlStyle { .. }
                | Page::Screen { .. }
        )
    }

    pub fn input(&mut self, key: Key) -> Option<Action> {
        let disabled = self.disabled_entry(&self.page);
        // do_optmenu: Accept on a disabled entry buzzes and returns 0.
        if key == Key::Accept && disabled {
            self.cues.push(Cue::Buzzer);
            return None;
        }
        self.cues.extend_from_slice(cues(&self.page, key));
        let action = self.input_page(key);
        // do_optmenu's cursor skips disabled entries (+0x20 < 0).
        if matches!(key, Key::Up | Key::Down) {
            for _ in 0..8 {
                if !self.disabled_entry(&self.page) {
                    break;
                }
                self.input_page(key);
            }
        }
        action
    }

    /// One audio-menu frame of `vb` vblanks with left/right held (DoOptions
    /// case 8, do_optmenu's level results 5/6 after the edges 3/4; left
    /// first): the selected slider moves by vb (0..=255); the SFX slider at
    /// its end sounds AudioVolSfx(1) every 16 vblanks, resting on Sfx
    /// Volume AudioVolSfx(0) every 61.
    pub fn hold(&mut self, left: bool, right: bool, vb: i32) -> Option<Action> {
        let Page::Audio { at, .. } = self.page else { return None };
        self.sfx_sound_count += vb;
        let delta = if left { -vb } else if right { vb } else { 0 };
        let moved = |v: u8| (i32::from(v) + delta).clamp(0, 0xff) as u8;
        match at {
            0 if delta != 0 => {
                self.music = moved(self.music);
                Some(Action::Volumes { music: self.music, sfx: self.sfx, store: false })
            }
            1 if delta != 0 => {
                let v = moved(self.sfx);
                if v != self.sfx {
                    self.sfx = v;
                    return Some(Action::Volumes { music: self.music, sfx: self.sfx, store: false });
                }
                if 0x10 <= self.sfx_sound_count {
                    self.cues.push(Cue::VolSfxEnd);
                    self.sfx_sound_count = 0;
                }
                None
            }
            1 if 0x3d <= self.sfx_sound_count => {
                self.cues.push(Cue::VolSfxSample(self.sfx_sample));
                self.sfx_sample = (self.sfx_sample + 1) % 2;
                self.sfx_sound_count = 0;
                None
            }
            _ => None,
        }
    }

    /// The selected entry of `page` is disabled (+0x20 < 0).
    fn disabled_entry(&self, page: &Page) -> bool {
        matches!(page, Page::Paused { tower: false, at: 1 }) && self.quit_level_disabled
    }

    fn input_page(&mut self, key: Key) -> Option<Action> {
        let page = self.page.clone();
        match page {
            Page::PressStart => match key {
                Key::Accept | Key::Start => {
                    self.page = Page::Title { at: 0 };
                    None
                }
                // NATIVE: Back on "Press Start" (Escape, Backspace, a
                // controller's triangle) quits the client — the PS2 has no
                // exit; the title's Back leads here.
                Key::Back => Some(Action::Exit),
                _ => None,
            },
            Page::Title { at } => match key {
                Key::Up | Key::Down => {
                    self.page = Page::Title { at: wrap(at, 2, key == Key::Up) };
                    None
                }
                // do_titlescreen → init_player_select(0); DEBUG: with
                // GDL_DEBUG_NATIVE_SELECT the native save list (and the
                // capture import) instead.
                Key::Accept if at == 0 => {
                    if std::env::var("GDL_DEBUG_NATIVE_SELECT").is_ok() {
                        self.page = Page::Select { at: 0 };
                        None
                    } else {
                        Some(Action::StartSelect)
                    }
                }
                Key::Accept => {
                    self.page = Page::Options { at: 0, back: Box::new(page) };
                    None
                }
                Key::Back => {
                    self.page = Page::PressStart;
                    None
                }
                _ => None,
            },
            Page::Options { at, back } => {
                let names = option_names(&back);
                let n = names.len();
                match key {
                    Key::Up | Key::Down => self.page = Page::Options { at: wrap(at, n, key == Key::Up), back },
                    Key::Accept if names[at] == "Audio" => self.page = Page::Audio { at: 0, back: Box::new(Page::Options { at, back }) },
                    // DoOptions 0x14: compass_options, selection = GamePrefs
                    // +0x18; 0x10 / 0x15: the Game Options and Controls lists.
                    Key::Accept if names[at] == "Compass" => {
                        let cur = self.choice_value("compass_options");
                        self.page = Page::Choice { key: "compass_options", at: cur, back: Box::new(Page::Options { at, back }) };
                    }
                    Key::Accept if names[at] == "Game Options" => self.page = Page::Submenu { key: "gameoption_options", at: 0, back: Box::new(Page::Options { at, back }) },
                    Key::Accept if names[at] == "Controls" => self.page = Page::Submenu { key: "controls_options", at: 0, back: Box::new(Page::Options { at, back }) },
                    Key::Accept if names[at] == "Screen" => self.page = Page::Screen { saved: (self.prefs[3], self.prefs[4]), back: Box::new(Page::Options { at, back }) },
                    Key::Accept if names[at] == DISPLAY => {
                        self.page = Page::Options { at, back };
                        return Some(Action::Display);
                    }
                    Key::Accept => self.page = Page::NotPorted { what: names[at].to_string(), back: Box::new(Page::Options { at, back }) },
                    Key::Back => self.page = *back,
                    _ => {}
                }
                None
            }
            Page::Submenu { key: k, at, back } => {
                let names = submenu_entries(k);
                match key {
                    Key::Up | Key::Down => self.page = Page::Submenu { key: k, at: wrap(at, names.len(), key == Key::Up), back },
                    Key::Back => self.page = *back,
                    Key::Accept => {
                        let choice = match (k, at) {
                            ("gameoption_options", 0) => Some("difficulty_options"),
                            ("gameoption_options", 1) => Some("multiplayer_options"),
                            ("controls_options", 1) => Some("control_rumble_options"),
                            ("controls_options", 2) => Some("control_autoaim_options"),
                            ("controls_options", 3) => Some("control_autoattack_options"),
                            _ => None,
                        };
                        let here = Page::Submenu { key: k, at, back };
                        if (k, at) == ("controls_options", 0) {
                            self.page = Page::ControlStyle { style: self.controls[0] as usize & 3, back: Box::new(here) };
                            return None;
                        }
                        self.page = match choice {
                            Some(c) => Page::Choice { key: c, at: self.choice_value(c), back: Box::new(here) },
                            None => Page::NotPorted { what: names[at].trim().to_string(), back: Box::new(here) },
                        };
                    }
                    _ => {}
                }
                None
            }
            Page::Screen { saved, back } => {
                let (mut dx, mut dy) = (self.screen.0, self.screen.1);
                let mut keep = false;
                match key {
                    Key::Left => dx = (dx - 4).clamp(-0x80, 0x80),
                    Key::Right => dx = (dx + 4).clamp(-0x80, 0x80),
                    Key::Up => dy = (dy - 1).clamp(-0x28, 0x28),
                    Key::Down => dy = (dy + 1).clamp(-0x28, 0x28),
                    Key::Center => (dx, dy) = (0, 0),
                    Key::Accept => {
                        keep = true;
                        self.prefs[3] = dx;
                        self.prefs[4] = dy;
                    }
                    Key::Back => (dx, dy) = saved,
                    _ => {}
                }
                self.screen = (dx, dy);
                self.page = if matches!(key, Key::Accept | Key::Back) { *back } else { Page::Screen { saved, back } };
                Some(Action::Screen { dx, dy, keep })
            }
            Page::ControlStyle { style, back } => match key {
                Key::Left | Key::Right => {
                    let style = if key == Key::Left { (style + 3) % 4 } else { (style + 1) % 4 };
                    self.page = Page::ControlStyle { style, back };
                    None
                }
                // DoOptions 0xe: Select (0x21) stores the style in
                // PlayerControl +0x2c and pops; triangle (−1) is ignored.
                Key::Accept => {
                    self.page = *back;
                    self.controls[0] = style as u32;
                    Some(Action::Choose { key: "controlstyle_options", idx: style })
                }
                _ => {
                    self.page = Page::ControlStyle { style, back };
                    None
                }
            },
            Page::Choice { key: k, at, back } => {
                let n = submenu_entries(k).len();
                match key {
                    Key::Up | Key::Down => {
                        self.page = Page::Choice { key: k, at: wrap(at, n, key == Key::Up), back };
                        None
                    }
                    Key::Back => {
                        self.page = *back;
                        None
                    }
                    // DoOptions 0xa..0x11: the choice is stored and marked;
                    // the menu stays until Back.
                    Key::Accept => {
                        self.page = Page::Choice { key: k, at, back };
                        Some(Action::Choose { key: k, idx: at })
                    }
                    _ => {
                        self.page = Page::Choice { key: k, at, back };
                        None
                    }
                }
            }
            // audio_options: Music Volume (0x17), Sfx Volume (0x18) and Mono
            // (0x19, value "Stereo"). The sliders move while left/right is
            // held (Menu::hold); the edge toggles Mono / Stereo
            // (AudioCursorH, entry +0x20 ^= 1, AudioSetStereo).
            Page::Audio { at, back } => match key {
                Key::Up | Key::Down => {
                    self.page = Page::Audio { at: wrap(at, 3, key == Key::Up), back };
                    None
                }
                Key::Left | Key::Right if at == 2 => {
                    self.cues.push(Cue::CursorH);
                    self.prefs[2] ^= 1;
                    self.page = Page::Audio { at, back };
                    Some(Action::Stereo { on: self.prefs[2] != 0 })
                }
                // DoOptions case 8: the entries' actions (0x17..0x19) do
                // nothing; −1 / −2 end the audio menu (GamePrefs kept).
                Key::Back => {
                    self.page = *back;
                    Some(Action::Volumes { music: self.music, sfx: self.sfx, store: true })
                }
                _ => {
                    self.page = Page::Audio { at, back };
                    None
                }
            },
            Page::NotPorted { back, .. } => {
                if matches!(key, Key::Back | Key::Accept) {
                    self.page = *back;
                }
                None
            }
            Page::Select { at } => {
                let items = self.select_items();
                let n = items.len();
                match key {
                    Key::Up | Key::Down => self.page = Page::Select { at: wrap(at, n, key == Key::Up) },
                    Key::Back => self.page = Page::Title { at: 0 },
                    Key::Accept => {
                        if at < self.saves.len() {
                            return Some(Action::Play { slot: self.saves[at].slot });
                        }
                        let rest = at - self.saves.len();
                        if rest == 0 {
                            self.page = Page::Class { at: 6 };
                        } else if rest == 1 && self.import.is_some() {
                            return Some(Action::Import);
                        } else {
                            self.page = Page::Title { at: 0 };
                        }
                    }
                    _ => {}
                }
                None
            }
            Page::Class { at } => {
                match key {
                    Key::Up | Key::Down | Key::Left | Key::Right => self.page = Page::Class { at: wrap(at, 8, matches!(key, Key::Up | Key::Left)) },
                    Key::Accept => self.page = Page::Colour { class: at as u32, at: 0 },
                    Key::Back => self.page = Page::Select { at: 0 },
                    _ => {}
                }
                None
            }
            Page::Colour { class, at } => {
                match key {
                    Key::Up | Key::Down | Key::Left | Key::Right => self.page = Page::Colour { class, at: wrap(at, 4, matches!(key, Key::Up | Key::Left)) },
                    Key::Accept => self.page = Page::Name { class, colour: at as u8, name: String::new() },
                    Key::Back => self.page = Page::Class { at: class as usize },
                    _ => {}
                }
                None
            }
            Page::Name { class, colour, mut name } => match key {
                Key::Char(c) if c.is_ascii_alphanumeric() && name.len() < 7 => {
                    name.push(c.to_ascii_uppercase());
                    self.page = Page::Name { class, colour, name };
                    None
                }
                Key::Erase => {
                    name.pop();
                    self.page = Page::Name { class, colour, name };
                    None
                }
                Key::Accept if !name.is_empty() => Some(Action::Create { class, colour, name }),
                Key::Back => {
                    self.page = Page::Colour { class, at: colour as usize };
                    None
                }
                _ => None,
            },
            Page::Paused { tower, at } => {
                let names: &[&str] = if tower { &TOWER_MENU } else { &GAME_MENU };
                match key {
                    Key::Up | Key::Down => self.page = Page::Paused { tower, at: wrap(at, names.len(), key == Key::Up) },
                    Key::Back => return Some(Action::Resume),
                    Key::Accept => match names[at] {
                        "Settings" => self.page = Page::Options { at: 0, back: Box::new(page) },
                        // DoOptions 0xd: init_player_select(1) with this
                        // player at the select screen's menu 1.
                        "Manage Character" => return Some(Action::ManageCharacter),
                        // start_optmenu(quitgame_question / quitlevel_question)
                        // with +0x74 = 0 ("No").
                        "Quit Game" => self.page = Page::ConfirmQuit { at: 0, tower: true },
                        "Quit Level" => self.page = Page::ConfirmQuit { at: 0, tower: false },
                        "Shop" => return Some(Action::OpenShop),
                        "Inventory" => return Some(Action::OpenInventory),
                        other => self.page = Page::NotPorted { what: other.to_string(), back: Box::new(page) },
                    },
                    _ => {}
                }
                None
            }
            Page::Shop { at } => match key {
                Key::Up | Key::Down => {
                    self.page = Page::Shop { at: wrap(at, self.shop.len().max(1), key == Key::Up) };
                    None
                }
                Key::Accept => Some(Action::Buy { idx: at }),
                Key::Back => {
                    self.page = Page::Paused { tower: true, at: 2 };
                    None
                }
                _ => None,
            },
            Page::Hints { at } => {
                match key {
                    Key::Up | Key::Down => self.page = Page::Hints { at: wrap(at, HINT_MENU.len(), key == Key::Up) },
                    Key::Accept => return Some(Action::ShowHint { list: at }),
                    Key::Back => return Some(Action::Resume),
                    _ => {}
                }
                None
            }
            Page::HintText { list, at } => {
                let n = self.hints.get(list).map_or(0, Vec::len);
                match key {
                    Key::Accept | Key::Right | Key::Down if at + 1 < n => self.page = Page::HintText { list, at: at + 1 },
                    Key::Left | Key::Up => self.page = Page::HintText { list, at: at.saturating_sub(1) },
                    Key::Accept | Key::Back => self.page = Page::Hints { at: list },
                    _ => {}
                }
                None
            }
            Page::Inventory => {
                if matches!(key, Key::Back | Key::Accept) {
                    self.page = Page::InventoryClose;
                }
                None
            }
            Page::InventoryClose => None,
            Page::ConfirmQuit { at, tower } => match key {
                Key::Up | Key::Down => {
                    self.page = Page::ConfirmQuit { at: wrap(at, 2, key == Key::Up), tower };
                    None
                }
                // DoOptions 0x13 / 0x14: quit_game / quit_level and
                // end_optmenu(−1, −1).
                Key::Accept if at == 1 => Some(if tower { Action::QuitGame } else { Action::QuitLevel }),
                Key::Accept | Key::Back => {
                    self.page = if tower { Page::Paused { tower, at: 4 } } else { Page::Paused { tower, at: 1 } };
                    None
                }
                _ => None,
            },
        }
    }

    pub fn text(&self) -> String {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let body = match &self.page {
            Page::PressStart => "Press Start\n\n(Enter / Space / gamepad Start or South)\nEscape / gamepad North: quit".to_string(),
            Page::Title { at } => list("", &s(&TITLE_MENU), *at),
            Page::Options { at, back } => {
                list("OPTIONS", &s(option_names(back)), *at)
            }
            Page::Audio { at, .. } => list(
                "AUDIO\n(hold left/right to change)",
                &[format!("Music  {:>3}", self.music), format!("Sound  {:>3}", self.sfx), (if self.prefs[2] != 0 { "Stereo" } else { "Mono" }).to_string()],
                *at,
            ),
            Page::NotPorted { what, .. } => format!("{what}\n\nnot ported yet\n\n(Enter / Esc to go back)"),
            Page::Submenu { key, at, .. } | Page::Choice { key, at, .. } => list(key, &s(submenu_entries(key)), *at),
            Page::Screen { .. } => format!("Screen Adjust\n\nd-pad: move the picture ({}, {})\nC: center   Enter: accept   Esc: back", self.screen.0, self.screen.1),
            Page::ControlStyle { style, .. } => list("Control Style", &s(&["Default", "Arcade", "One Handed", "Robotron"][*style..=*style]), 0),
            Page::Select { at } => list("SELECT A CHARACTER", &self.select_items(), *at),
            Page::Class { at } => list("TO BUILD A CHARACTER", &s(&CLASS_NAMES), *at),
            Page::Colour { class, at } => list(&format!("{}\nchoose a colour", CLASS_NAMES[*class as usize]), &s(&COLOUR_NAMES), *at),
            Page::Name { class, colour, name } => format!(
                "{} {}\n\nENTER INITIALS\n\n{}_\n\n(letters/digits, Backspace erases, Enter confirms, Esc goes back)",
                COLOUR_NAMES[*colour as usize],
                CLASS_NAMES[*class as usize],
                name
            ),
            Page::Paused { tower, at } => list("PAUSED", &s(if *tower { &TOWER_MENU } else { &GAME_MENU }), *at),
            Page::Shop { at } => {
                // A window of 12 lines around the cursor.
                let first = at.saturating_sub(6).min(self.shop.len().saturating_sub(12));
                let lines: Vec<String> = self.shop.iter().skip(first).take(12).cloned().collect();
                list(&format!("SHOP          gold {}\n(- : not available now)", self.gold), &lines, at - first)
            }
            Page::Hints { at } => list("HINTS", &s(&HINT_MENU), *at),
            Page::HintText { list: l, at } => {
                let v = self.hints.get(*l);
                format!(
                    "{}  {}/{}\n\n{}\n\n(Enter: next, Esc: back)",
                    HINT_MENU.get(*l).copied().unwrap_or(""),
                    at + 1,
                    v.map_or(0, Vec::len),
                    v.and_then(|v| v.get(*at)).map_or("", String::as_str)
                )
            }
            Page::Inventory | Page::InventoryClose => format!("INVENTORY\n\n{}\n\n(Enter / Esc to go back)", self.inventory),
            Page::ConfirmQuit { at, tower } => list(if *tower { "QUIT GAME?\nprogress since your last save is lost" } else { "ABORT LEVEL?" }, &s(&["No", "Yes"]), *at),
        };
        if self.status.is_empty() { body } else { format!("{body}\n\n{}", self.status) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_character_flow() {
        let mut m = Menu::new(128, 128);
        m.saves = vec![Entry { slot: 1, label: "PELE".into() }];
        assert_eq!(m.input(Key::Accept), None);
        // The title's Start: the select screen (init_player_select(0)).
        assert_eq!(m.input(Key::Accept), Some(Action::StartSelect));
        // DEBUG (GDL_DEBUG_NATIVE_SELECT): the native pages.
        m.page = Page::Select { at: 0 };
        m.input(Key::Down);
        m.input(Key::Accept);
        assert!(matches!(m.page, Page::Class { at: 6 }));
        m.input(Key::Left);
        m.input(Key::Accept);
        m.input(Key::Down);
        m.input(Key::Accept);
        for c in "ab1".chars() {
            m.input(Key::Char(c));
        }
        m.input(Key::Erase);
        assert_eq!(m.input(Key::Accept), Some(Action::Create { class: 5, colour: 1, name: "AB".into() }));
    }

    #[test]
    fn load_import_and_pause_menus() {
        let mut m = Menu::new(128, 128);
        m.saves = vec![Entry { slot: 3, label: "X".into() }];
        m.import = Some("PELE".into());
        m.page = Page::Select { at: 0 };
        assert_eq!(m.input(Key::Accept), Some(Action::Play { slot: 3 }));
        m.input(Key::Down);
        m.input(Key::Down);
        assert_eq!(m.input(Key::Accept), Some(Action::Import));
        m.pause(true);
        m.input(Key::Down);
        assert_eq!(m.input(Key::Accept), Some(Action::ManageCharacter));
        m.pause(true);
        m.input(Key::Up);
        m.input(Key::Accept);
        m.input(Key::Down);
        assert_eq!(m.input(Key::Accept), Some(Action::QuitGame));
        m.pause(false);
        m.input(Key::Down);
        m.input(Key::Accept);
        assert_eq!(page_record(&m.page), Some(("quitlevel_question", 0)));
        assert_eq!(m.input(Key::Accept), None);
        assert_eq!(m.page, Page::Paused { tower: false, at: 1 });
        m.input(Key::Accept);
        m.input(Key::Down);
        assert_eq!(m.input(Key::Accept), Some(Action::QuitLevel));
        m.pause(false);
        m.input(Key::Accept);
        m.input(Key::Accept);
        assert_eq!(m.input(Key::Right), None);
        assert_eq!(m.hold(false, true, 2), Some(Action::Volumes { music: 130, sfx: 128, store: false }));
        m.input(Key::Down);
        m.sfx = 255;
        m.sfx_sound_count = 0;
        assert_eq!(m.hold(false, true, 16), None);
        assert_eq!(m.cues.last(), Some(&Cue::VolSfxEnd));
        m.input(Key::Down);
        assert_eq!(m.input(Key::Left), Some(Action::Stereo { on: true }));
        assert_eq!(m.input(Key::Back), Some(Action::Volumes { music: 130, sfx: 255, store: true }));
    }

    #[test]
    fn in_game_settings_pages_keep_the_game_paused() {
        // Every entry of the Tower's and a realm's Settings, and Controls →
        // Style: the game stays paused (Screen and ControlStyle resumed it
        // before 2026-10-28, so Screen Adjust never showed in game).
        for tower in [true, false] {
            let names = option_names(&Page::Paused { tower, at: 0 });
            for (i, name) in names.iter().enumerate() {
                let mut m = Menu::new(128, 128);
                m.pause(tower);
                m.input(Key::Accept);
                for _ in 0..i {
                    m.input(Key::Down);
                }
                let action = m.input(Key::Accept);
                assert!(m.is_paused(), "tower {tower} {name}: {:?}", m.page);
                assert_eq!(action == Some(Action::Display), *name == DISPLAY);
                if *name == "Screen" {
                    assert!(matches!(m.page, Page::Screen { .. }));
                }
                if *name == "Controls" {
                    m.input(Key::Accept);
                    assert!(matches!(m.page, Page::ControlStyle { .. }));
                    assert!(m.is_paused());
                }
            }
        }
    }

    #[test]
    fn press_start_takes_start_and_back_quits() {
        let mut m = Menu::new(128, 128);
        assert_eq!(m.input(Key::Start), None);
        assert_eq!(m.page, Page::Title { at: 0 });
        assert_eq!(m.input(Key::Back), None);
        assert_eq!(m.page, Page::PressStart);
        assert_eq!(m.input(Key::Back), Some(Action::Exit));
        // The title's Options → Display: the overlay, the list stays.
        m.page = Page::Title { at: 1 };
        m.input(Key::Accept);
        for _ in 0..5 {
            m.input(Key::Down);
        }
        assert_eq!(m.input(Key::Accept), Some(Action::Display));
        assert!(matches!(m.page, Page::Options { at: 5, .. }));
    }

    #[test]
    fn hint_pages() {
        let mut m = Menu::new(128, 128);
        m.hints = vec![vec!["a".into(), "b".into()], vec![], vec![], vec![]];
        m.page = Page::Hints { at: 0 };
        // Accept asks the host for the category's next hint (ShowHint); the
        // host then opens the text page.
        assert_eq!(m.input(Key::Accept), Some(Action::ShowHint { list: 0 }));
        m.page = Page::HintText { list: 0, at: 0 };
        assert!(m.text().contains("1/2"));
        m.input(Key::Accept);
        assert!(m.text().contains("2/2"));
        m.input(Key::Accept);
        assert_eq!(m.page, Page::Hints { at: 0 });
        assert_eq!(m.input(Key::Back), Some(Action::Resume));
    }
}
