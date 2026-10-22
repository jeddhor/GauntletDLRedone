//! The attract sequence (game modes 0x8000..0x8009): init_attract_mode
//! 0x182708 walking attract_tab 0x249568 (13 × {mode, table}), the
//! Screen2D table-0 memory-card check (do_screen2d 0x183458) and the credits
//! scroll (scroll_credits 0x183d20). The host runs the movies (init_movie /
//! do_movie, MPEG2), the screens' blits and the title screen.
use super::hud::{Hud, TexRef};
use gdl_formats::elf::Elf;
use gdl_formats::fnt::Font;

/// What init_attract_mode starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttractEntry {
    /// 0x8000 init_credits.
    Credits,
    /// 0x8001/0x8002 init_movie: "/MPEG2/<name>.pss".
    Movie { name: String },
    /// 0x8003/0x8006/0x8008 init_flyby (a level under the old camera).
    Flyby,
    /// 0x8004 init_screen2d(table): table 0 the SONY screen and the card
    /// check, else the group shot while the next flyby level loads.
    Screen2D { table: i32 },
    /// 0x8009 init_titlescreen.
    Titlescreen,
}

/// init_attract_mode's state (BSS words at boot).
#[derive(Debug, Clone)]
pub struct Attract {
    /// attract_tab 0x249568: (mode, table); a negative mode ends the list.
    pub tab: Vec<(i32, i32)>,
    /// attract_movies 0x2495d0: 5 tables × 4 names (null → entry 0).
    pub movies: Vec<[Option<String>; 4]>,
    pub next_idx: usize,
    pub count: i32,
    /// attract_movie_idx (−1 = _heap_size after a forced mode).
    pub movie_idx: i32,
    /// did_mc_check: 1 while the card check runs, 2 once done.
    pub did_mc_check: i32,
    pub fail_movie_show: bool,
    pub mode: i32,
}

fn cstr_at(elf: &Elf, p: u32) -> Option<String> {
    if p == 0 {
        return None;
    }
    // Strings near a section end: shorten the read until it fits.
    let b = [128u32, 64, 32, 16].iter().find_map(|&n| elf.read(p, n).ok())?;
    Some(String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned())
}

impl Attract {
    pub fn from_elf(elf: &Elf) -> Result<Self, String> {
        let w = |b: &[u8], k: usize| i32::from_le_bytes(b[4 * k..4 * k + 4].try_into().unwrap());
        let t = elf.object("attract_tab").map_err(|e| e.to_string())?;
        let tab = (0..t.len() / 8).map(|k| (w(t, 2 * k), w(t, 2 * k + 1))).collect();
        let m = elf.object("attract_movies").map_err(|e| e.to_string())?;
        let movies = (0..m.len() / 16).map(|k| std::array::from_fn(|j| cstr_at(elf, w(m, 4 * k + j) as u32))).collect();
        Ok(Self { tab, movies, next_idx: 0, count: 0, movie_idx: 0, did_mc_check: 0, fail_movie_show: false, mode: 0 })
    }

    fn mode_at(&self, k: usize) -> i32 {
        self.tab.get(k).map_or(-1, |e| e.0)
    }

    /// init_attract_mode(param) (exit_attract 2 is the host's: it goes to the
    /// title). `both_shoulders`: any_level(0x400000) && any_level(0x800000)
    /// forces the credits. NoMovies is 0 in this build.
    pub fn init_attract_mode(&mut self, mut param: i32, both_shoulders: bool) -> AttractEntry {
        if both_shoulders {
            param = 0x8000;
        }
        if 0 <= param {
            let found = (0..0xd).find(|&k| self.mode_at(k) == param && (param != 0x8002 || !self.fail_movie_show || self.tab[k].1 == 4));
            if let Some(k) = found {
                self.next_idx = k;
            }
            self.movie_idx = -1;
            self.count = 0;
        }
        // SetMaxFPS(30): the attract modes run at 30 Hz (host pacing).
        let mut idx;
        loop {
            idx = self.next_idx;
            self.mode = self.mode_at(idx);
            let table = self.tab.get(idx).map_or(0, |e| e.1);
            let skip = match self.mode {
                0x8000 => param < 0 && self.count & 0xf != 0xf,
                0x8001 | 0x8002 => table == 4 && !self.fail_movie_show,
                0x8004 => table == 0 && self.did_mc_check != 0,
                0x8009 => 0 < self.count,
                _ => false,
            };
            self.next_idx = idx + 1;
            if self.mode_at(self.next_idx) < 0 {
                self.count += 1;
                self.next_idx = 0;
                self.movie_idx += 1;
                if 3 < self.movie_idx {
                    self.movie_idx = 0;
                }
            }
            if self.movie_idx < 0 {
                self.movie_idx = 0;
            }
            if !skip {
                break;
            }
        }
        let table = self.tab.get(idx).map_or(0, |e| e.1);
        match self.mode {
            0x8001 | 0x8002 => {
                // init_movie: attract_movies[table][movie_idx], else entry 0.
                let row = self.movies.get(table.max(0) as usize);
                let name = row.and_then(|r| r[self.movie_idx.clamp(0, 3) as usize].clone().or_else(|| r[0].clone())).unwrap_or_default();
                AttractEntry::Movie { name }
            }
            0x8003 | 0x8006 | 0x8008 => AttractEntry::Flyby,
            0x8004 => {
                if table == 0 {
                    self.did_mc_check = 1;
                }
                AttractEntry::Screen2D { table }
            }
            0x8009 => AttractEntry::Titlescreen,
            _ => AttractEntry::Credits,
        }
    }
}

/// do_screen2d's table-0 card check, one state per frame over ports 0–1 ×
/// slots 0–3. `card(port, slot)`: get_card_state (0 none, 1 unformatted, 2
/// formatted) and whether a save fits. The native card (decision: host
/// saves) is port 0 slot 0, formatted with room, so the messages (Yes/No
/// format prompt, "not enough space", "no memory card") are not shown.
#[derive(Debug, Clone)]
pub struct CardCheck {
    state: i32,
    port: i32,
    slot: i32,
    ok: i32,
}

impl Default for CardCheck {
    fn default() -> Self {
        Self { state: -1, port: 0, slot: 0, ok: 0 }
    }
}

impl CardCheck {
    /// One do_screen2d frame; true once did_mc_check becomes 2.
    pub fn step(&mut self, card: &dyn Fn(i32, i32) -> (i32, bool)) -> Result<bool, String> {
        if self.state < 0 {
            self.state = card(self.port, self.slot).0;
        }
        match self.state {
            0 => self.state = 0xb,
            2 => self.state = if card(self.port, self.slot).1 { 10 } else { 3 },
            10 | 0xb => {
                if self.state == 10 {
                    self.ok += 1;
                }
                self.state = -1;
                self.slot += 1;
                if 3 < self.slot {
                    self.slot = 0;
                    self.port += 1;
                    if 1 < self.port {
                        self.state = 0x14;
                    }
                }
            }
            0x14 => self.state = if self.ok == 0 { 0x15 } else { 99 },
            99 => return Ok(true),
            s => return Err(format!("card check state {s:#x} (an unformatted or full card, or none) is not ported")),
        }
        Ok(false)
    }
}

/// credit_text 0x249a30 (35 lines), credit_text2 0x249ac0 (16) and
/// credit_text3 0x249b00 (82).
#[derive(Debug, Clone, Default)]
pub struct Credits {
    pub text: [Vec<String>; 3],
    /// credits_scroll and timer1.
    pub scroll: i32,
    pub timer: i32,
}

impl Credits {
    pub fn from_elf(elf: &Elf) -> Result<Self, String> {
        let list = |n: &str| -> Result<Vec<String>, String> {
            let b = elf.object(n).map_err(|e| e.to_string())?;
            Ok(b.chunks_exact(4).map(|c| cstr_at(elf, u32::from_le_bytes(c.try_into().unwrap())).unwrap_or_default()).collect())
        };
        Ok(Self { text: [list("credit_text")?, list("credit_text2")?, list("credit_text3")?], scroll: 0, timer: 0 })
    }

    /// scroll_credits: the three lists from y 0x17f − scroll upwards (21,
    /// 15, 15 pixels apart; the second and third at scale 0.833) in font
    /// 0xd, white, fading over the top and bottom 16 lines; the scroll
    /// advances by 1 + the finished sections each frame. Returns true once
    /// nothing is drawn any more.
    pub fn scroll_credits(&mut self, hud: &mut Hud, vb_elapsed: i32) -> bool {
        let scale2 = f32::from_bits(0x3f55_3f7d);
        let mut y = self.scroll;
        let mut nothing = true;
        let mut speed = 0;
        let draw = |hud: &mut Hud, y: i32, scale: f32, text: &str| -> bool {
            if !(0..0x180).contains(&y) {
                return false;
            }
            let n = hud.messages.len();
            hud.draw_text(scale, 0x20, 0x17f - y, 0xd, 0xffffff, text);
            if hud.messages.len() == n {
                return false;
            }
            let fade = if 0x180 - y < 0x10 {
                Some(((0x180 - y) * -0x10 + 0xff) as u32)
            } else if y < 0x10 {
                Some(((0x10 - y) * 0x10) as u32)
            } else {
                None
            };
            if let (Some(v), Some(m)) = (fade, hud.messages.last_mut()) {
                // MBFontMsgSetAlpha: alpha 0x80 − value / 2.
                m.rgba[3] = 0x80u32.saturating_sub(v >> 1) as u8;
            }
            true
        };
        let sections: [(usize, f32, i32, Option<usize>); 3] = [(0, 1.0, 0x15, Some(0x22)), (1, scale2, 0xf, Some(0xf)), (2, scale2, 0xf, None)];
        for (s, scale, dy, last) in sections {
            let mut done_last = false;
            for (k, line) in self.text[s].clone().iter().enumerate() {
                if y < 0 {
                    break;
                }
                if draw(hud, y, scale, line) {
                    nothing = false;
                    if last == Some(k) && 0xc0 < y {
                        done_last = true;
                    }
                }
                y -= dy;
            }
            if last.is_some() && (done_last || nothing) {
                speed += 1;
            }
        }
        if vb_elapsed != 0 {
            self.scroll += speed + 1;
        }
        nothing
    }
}

impl Hud {
    /// MBOX_NewBlit("<prefix>00".."03") at screenblitxy (0,0), (256,0),
    /// (0,256), (256,256): a full-screen picture of four tiles (Screen2D's
    /// sony_screen / Group_shot_midway, the credits' CRED_ALL), behind the
    /// text.
    pub fn screen_blits(&self, prefix: &str) -> Vec<super::hud::Blit> {
        [(0, 0), (256, 0), (0, 256), (256, 256)]
            .into_iter()
            .enumerate()
            .map(|(k, (x, y))| {
                let mut b = self.make_pub(self.lookup(&format!("{prefix}{k:02}")), x, y, -1, -1);
                b.z = super::hud::blit_z(-1000.0);
                b
            })
            .collect()
    }

    /// do_credits' frame: the CRED_ALL picture and scroll_credits' text as
    /// glyph blits. True once the scroll drew nothing.
    pub fn credits_frame(&mut self, credits: &mut Credits, vb_elapsed: i32) -> (Vec<super::hud::Blit>, bool) {
        let mut out = self.screen_blits("CRED_ALL");
        let n = self.messages.len();
        let done = credits.scroll_credits(self, vb_elapsed);
        let msgs: Vec<_> = self.messages.drain(n..).collect();
        for m in &msgs {
            out.extend(self.glyphs(m));
        }
        (out, done)
    }

    /// FontInitSpecial(name, 8): font 0xd = fonts/<name>.fnt with space 8;
    /// its texture is the loaded model's texture of the same name.
    pub fn font_init_special(&mut self, font: Font, tex: Option<TexRef>) {
        if self.fonts.len() <= 0xd {
            self.fonts.resize(0xe, None);
        }
        self.fonts[0xd] = Some(super::hud::HudFont { font, tex, space: 8, per_page: 0x80 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// attract_tab / attract_movies as the ELF holds them (synthetic copy).
    fn attract() -> Attract {
        let tab = vec![(0x8004, 0), (0x8001, 0), (0x8002, 1), (0x8002, 2), (0x8009, 1), (0x8004, 1), (0x8008, 0), (0x8002, 3), (0x8004, 2), (0x8008, 0), (0x8000, 0), (0x8002, 4), (-1, 0)];
        let row = |n: [&str; 4]| n.map(|s| Some(s.to_string()));
        let movies = vec![row(["midway"; 4]), row(["opening"; 4]), row(["title2"; 4]), row(["war_dwa", "val_kni", "wiz_sor", "jes_arc"]), row(["fail"; 4])];
        Attract { tab, movies, next_idx: 0, count: 0, movie_idx: 0, did_mc_check: 0, fail_movie_show: false, mode: 0 }
    }

    fn movie(n: &str) -> AttractEntry {
        AttractEntry::Movie { name: n.into() }
    }

    #[test]
    fn boot_cycle_then_later_cycles() {
        let mut a = attract();
        let first: Vec<_> = (0..10).map(|_| a.init_attract_mode(-1, false)).collect();
        assert_eq!(
            first,
            vec![
                AttractEntry::Screen2D { table: 0 },
                movie("midway"),
                movie("opening"),
                movie("title2"),
                AttractEntry::Titlescreen,
                AttractEntry::Screen2D { table: 1 },
                AttractEntry::Flyby,
                movie("war_dwa"),
                AttractEntry::Screen2D { table: 2 },
                AttractEntry::Flyby,
            ]
        );
        // Credits (count & 0xf != 0xf) and fail (no failed game) are skipped;
        // the card check and the title only run in the first cycle.
        let second: Vec<_> = (0..8).map(|_| a.init_attract_mode(-1, false)).collect();
        assert_eq!(second[0], movie("midway"));
        assert_eq!(second[3], AttractEntry::Screen2D { table: 1 });
        assert_eq!(second[5], movie("val_kni"));
    }

    #[test]
    fn both_shoulders_force_the_credits() {
        let mut a = attract();
        assert_eq!(a.init_attract_mode(-1, true), AttractEntry::Credits);
    }

    #[test]
    fn the_native_card_check_takes_eighteen_frames() {
        let mut c = CardCheck::default();
        let card = |p: i32, s: i32| if (p, s) == (0, 0) { (2, true) } else { (0, false) };
        let mut n = 0;
        while !c.step(&card).unwrap() {
            n += 1;
        }
        assert_eq!(n + 1, 18);
    }
}
