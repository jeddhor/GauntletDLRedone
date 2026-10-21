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
    Save,
    QuitLevel,
    QuitGame,
    Exit,
    Volumes { music: u8, sfx: u8 },
    /// Tower menu → Shop: the host fills `shop` and opens the page.
    OpenShop,
    /// Buy shop entry `idx` (0 = leave).
    Buy { idx: usize },
    /// Tower menu → Inventory: the host fills `inventory` and opens the page.
    OpenInventory,
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
    Manage { at: usize },
    ConfirmQuit { at: usize },
    Shop { at: usize },
    Inventory,
    /// hint_menu 0x21ea28 (Sumner's pad): category list, then a category's
    /// hints one at a time.
    Hints { at: usize },
    HintText { list: usize, at: usize },
}

pub const CLASS_NAMES: [&str; 8] = ["WARRIOR", "VALKYRIE", "WIZARD", "ARCHER", "DWARF", "KNIGHT", "SORCERESS", "JESTER"];
pub const COLOUR_NAMES: [&str; 4] = ["YELLOW", "BLUE", "RED", "GREEN"];
const TITLE_MENU: [&str; 2] = ["Start", "Options"];
const TITLE_OPTIONS: [&str; 5] = ["Audio", "Game Options", "Compass", "Controls", "Screen"];
const TOWER_OPTIONS: [&str; 4] = ["Audio", "Compass", "Controls", "Screen"];
/// game_options 0x21d620 (a realm's Settings).
const GAME_OPTIONS: [&str; 3] = ["Audio", "Controls", "Screen"];

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
        Page::ConfirmQuit { at } => ("quitgame_question", *at),
        Page::Hints { at } => ("hint_menu", *at),
        Page::Audio { at, .. } => ("audio_options", *at),
        _ => return None,
    })
}
const TOWER_MENU: [&str; 5] = ["Settings", "Manage Character", "Shop", "Inventory", "Quit Game"];
const GAME_MENU: [&str; 2] = ["Settings", "Quit Level"];
const HINT_MENU: [&str; 4] = ["General Hints", "Guardians", "Items of Legend", "Runestones"];

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
        Self { page: Page::PressStart, saves: Vec::new(), import: None, music, sfx, status: String::new(), shop: Vec::new(), gold: 0, inventory: String::new(), hints: Vec::new() }
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
        matches!(self.page, Page::Paused { .. } | Page::Manage { .. } | Page::ConfirmQuit { .. } | Page::Options { .. } | Page::Audio { .. } | Page::NotPorted { .. } | Page::Shop { .. } | Page::Inventory | Page::Hints { .. } | Page::HintText { .. })
    }

    pub fn input(&mut self, key: Key) -> Option<Action> {
        let page = self.page.clone();
        match page {
            Page::PressStart => {
                if key == Key::Accept {
                    self.page = Page::Title { at: 0 };
                }
                None
            }
            Page::Title { at } => match key {
                Key::Up | Key::Down => {
                    self.page = Page::Title { at: wrap(at, 2, key == Key::Up) };
                    None
                }
                Key::Accept if at == 0 => {
                    self.page = Page::Select { at: 0 };
                    None
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
                    Key::Accept => self.page = Page::NotPorted { what: names[at].to_string(), back: Box::new(Page::Options { at, back }) },
                    Key::Back => self.page = *back,
                    _ => {}
                }
                None
            }
            Page::Audio { at, back } => match key {
                Key::Up | Key::Down => {
                    self.page = Page::Audio { at: wrap(at, 2, key == Key::Up), back };
                    None
                }
                Key::Left | Key::Right => {
                    let v = if at == 0 { &mut self.music } else { &mut self.sfx };
                    *v = if key == Key::Left { v.saturating_sub(16) } else { v.saturating_add(16) };
                    Some(Action::Volumes { music: self.music, sfx: self.sfx })
                }
                Key::Back | Key::Accept => {
                    self.page = *back;
                    None
                }
                _ => None,
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
                        "Manage Character" => self.page = Page::Manage { at: 0 },
                        "Quit Game" => self.page = Page::ConfirmQuit { at: 0 },
                        "Quit Level" => return Some(Action::QuitLevel),
                        "Shop" => return Some(Action::OpenShop),
                        "Inventory" => return Some(Action::OpenInventory),
                        other => self.page = Page::NotPorted { what: other.to_string(), back: Box::new(page) },
                    },
                    _ => {}
                }
                None
            }
            Page::Manage { at } => match key {
                Key::Up | Key::Down => {
                    self.page = Page::Manage { at: wrap(at, 2, key == Key::Up) };
                    None
                }
                Key::Accept if at == 0 => Some(Action::Save),
                Key::Accept | Key::Back => {
                    self.page = Page::Paused { tower: true, at: 1 };
                    None
                }
                _ => None,
            },
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
                    Key::Accept => self.page = Page::HintText { list: at, at: 0 },
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
                    self.page = Page::Paused { tower: true, at: 3 };
                }
                None
            }
            Page::ConfirmQuit { at } => match key {
                Key::Up | Key::Down => {
                    self.page = Page::ConfirmQuit { at: wrap(at, 2, key == Key::Up) };
                    None
                }
                Key::Accept if at == 1 => Some(Action::QuitGame),
                Key::Accept | Key::Back => {
                    self.page = Page::Paused { tower: true, at: 4 };
                    None
                }
                _ => None,
            },
        }
    }

    pub fn text(&self) -> String {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let body = match &self.page {
            Page::PressStart => "Press Start\n\n(Enter / Space / gamepad South)".to_string(),
            Page::Title { at } => list("", &s(&TITLE_MENU), *at),
            Page::Options { at, back } => {
                list("OPTIONS", &s(option_names(back)), *at)
            }
            Page::Audio { at, .. } => list("AUDIO\n(left/right to change)", &[format!("Music  {:>3}", self.music), format!("Sound  {:>3}", self.sfx)], *at),
            Page::NotPorted { what, .. } => format!("{what}\n\nnot ported yet\n\n(Enter / Esc to go back)"),
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
            Page::Manage { at } => list("MANAGE CHARACTER", &s(&["Save Character", "Back"]), *at),
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
            Page::Inventory => format!("INVENTORY\n\n{}\n\n(Enter / Esc to go back)", self.inventory),
            Page::ConfirmQuit { at } => list("QUIT GAME?\nprogress since your last save is lost", &s(&["No", "Yes"]), *at),
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
        assert_eq!(m.input(Key::Accept), None);
        assert!(matches!(m.page, Page::Select { at: 0 }));
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
        m.input(Key::Accept);
        assert_eq!(m.input(Key::Accept), Some(Action::Save));
        m.pause(true);
        m.input(Key::Up);
        m.input(Key::Accept);
        m.input(Key::Down);
        assert_eq!(m.input(Key::Accept), Some(Action::QuitGame));
        m.pause(false);
        m.input(Key::Down);
        assert_eq!(m.input(Key::Accept), Some(Action::QuitLevel));
        m.pause(false);
        m.input(Key::Accept);
        m.input(Key::Accept);
        assert_eq!(m.input(Key::Right), Some(Action::Volumes { music: 144, sfx: 128 }));
    }

    #[test]
    fn hint_pages() {
        let mut m = Menu::new(128, 128);
        m.hints = vec![vec!["a".into(), "b".into()], vec![], vec![], vec![]];
        m.page = Page::Hints { at: 0 };
        m.input(Key::Accept);
        assert!(m.text().contains("1/2"));
        m.input(Key::Accept);
        assert!(m.text().contains("2/2"));
        m.input(Key::Accept);
        assert_eq!(m.page, Page::Hints { at: 0 });
        assert_eq!(m.input(Key::Back), Some(Action::Resume));
    }
}
