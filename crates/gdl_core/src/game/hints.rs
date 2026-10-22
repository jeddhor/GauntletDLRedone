//! Sumner's hint pad (HintMenu 0x10e800, DoOptions case 0x16): the hint each
//! category shows, by progress — next_general_hint 0x10a610,
//! next_boss_hint 0x10a708, next_legend_hint 0x10a8f8, next_rune_hint
//! 0x10aae8. The texts are TEXT/HINTS_E.ROM's lists (DrawScrollListText
//! file 1): 0 BOSS_HINTS, 1 RUNE_HINTS, 2 LEGEND_HINTS, 3 GENERAL_HINTS.
use super::Game;

/// rune_idx_table 0x21ec20 (13 words, rune 1-based).
const RUNE_IDX: [i32; 13] = [8, 9, 4, 1, 2, 3, 10, 7, 5, 6, 11, 12, 13];

/// The hint globals (general_hint_index/num, boss/legend/rune index, pass,
/// num, hints_inited).
#[derive(Debug, Clone)]
pub struct Hints {
    pub inited: bool,
    pub general_index: i32,
    pub general_num: i32,
    pub boss_index: i32,
    pub boss_pass: i32,
    pub boss_num: i32,
    pub legend_index: i32,
    pub legend_pass: i32,
    pub legend_num: i32,
    pub rune_index: i32,
    pub rune_pass: i32,
    pub rune_num: i32,
}

impl Default for Hints {
    fn default() -> Self {
        Self { inited: false, general_index: -1, general_num: -1, boss_index: 0, boss_pass: 0, boss_num: 0, legend_index: 0, legend_pass: 0, legend_num: 0, rune_index: -1, rune_pass: 0, rune_num: 0 }
    }
}

/// What DoOptions draws for a category: the list, its entry, the lines and
/// the title (an entry of HINTS_E.ROM and its line; None: "A Hint for You").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintShown {
    pub list: usize,
    pub entry: usize,
    pub lines: Vec<usize>,
    pub title: Option<(usize, usize)>,
}

impl Game {
    /// HintMenu(player) opening the pad: next_general_hint(0) once
    /// (hints_inited), the other three reset every time.
    pub fn hint_menu_open(&self, h: &mut Hints) {
        if !h.inited {
            h.inited = true;
            h.general_index = -1;
            h.general_num = -1;
        }
        h.boss_pass = 0;
        h.boss_index = 0;
        h.legend_pass = 0;
        h.legend_index = 0;
        h.rune_index = -1;
        h.rune_pass = 0;
    }

    /// The class-record hint words of all four players (+0xcf8..+0xd02 at
    /// class × 0xb4): 1 + (any has bit `b` in `seen`) + (any in `done`, → 2).
    fn hint_lines(&self, seen: usize, done: usize, b: i32) -> i32 {
        let mut n = 0;
        for k in 0..4 {
            let r = &self.players[k];
            let cls = r.u32_(0xc) as usize * 0xb4;
            if n == 0 {
                n = 1;
                if (r.u16_(cls + seen) >> (b & 0x1f)) & 1 == 0 {
                    n = 0;
                }
            }
            if (r.u16_(cls + done) >> (b & 0x1f)) & 1 != 0 {
                n = 2;
            }
        }
        n + 1
    }

    fn world_at(&self, k: i32) -> i32 {
        usize::try_from(k).ok().and_then(|k| self.world_order.get(k)).copied().unwrap_or(0)
    }

    /// One category's next hint (DoOptions case 0x16, submenu 0x27..0x2a as
    /// `cat` 0..3). `general_lines(i)`: ScrollTextListNum(1, 3, i).
    pub fn next_hint(&self, h: &mut Hints, cat: usize, general_lines: &dyn Fn(i32) -> i32) -> HintShown {
        match cat {
            0 => {
                // next_general_hint(1).
                loop {
                    if h.general_index < 0 {
                        h.general_index = 3;
                    }
                    while 0 < h.general_index && !self.any_player_has_all_garg_items(h.general_index - 1) {
                        h.general_index -= 1;
                        h.general_num = -1;
                    }
                    h.general_num += 1;
                    if h.general_num < general_lines(h.general_index) {
                        break;
                    }
                    h.general_index -= 1;
                    h.general_num = -1;
                }
                HintShown { list: 3, entry: h.general_index.max(0) as usize, lines: vec![h.general_num.max(0) as usize], title: None }
            }
            1 => {
                let (idx, pass) = self.next_world_hint(h.boss_index, h.boss_pass, 11, |g, w| g.any_player_has_shard_world(w));
                h.boss_index = idx;
                h.boss_pass = pass;
                h.boss_num = self.hint_lines(0xd00, 0xd02, self.world_at(idx));
                HintShown { list: 0, entry: idx as usize, lines: (0..h.boss_num as usize).collect(), title: Some((12, idx as usize)) }
            }
            2 => {
                let (idx, pass) = self.next_world_hint(h.legend_index, h.legend_pass, 10, |g, w| (0..4).any(|k| g.player_has_legend_item(k, w)));
                h.legend_index = idx;
                h.legend_pass = pass;
                h.legend_num = self.hint_lines(0xcfc, 0xcfe, self.world_at(idx));
                HintShown { list: 2, entry: idx as usize, lines: (0..h.legend_num as usize).collect(), title: Some((40, idx as usize)) }
            }
            _ => {
                // next_rune_hint(1).
                let mut idx = h.rune_index + 1;
                let found = loop {
                    if 0xc < idx {
                        idx %= 0xd;
                        h.rune_pass = 1;
                    }
                    while idx < 0xd && h.rune_pass == 0 && self.player_has_rune(-1, RUNE_IDX[idx.clamp(0, 12) as usize] - 1) {
                        idx += 1;
                    }
                    if idx != 0xd {
                        break Some(idx);
                    }
                    if 1 < h.rune_pass {
                        break None;
                    }
                    h.rune_pass += 1;
                    idx = 1;
                };
                h.rune_index = match found {
                    Some(i) => i,
                    None => {
                        h.rune_pass = 1;
                        0
                    }
                };
                let rune = RUNE_IDX[h.rune_index.clamp(0, 12) as usize];
                h.rune_num = self.hint_lines(0xcf8, 0xcfa, rune - 1);
                HintShown { list: 1, entry: (rune - 1).max(0) as usize, lines: (0..h.rune_num as usize).collect(), title: Some((27, (rune - 1).max(0) as usize)) }
            }
        }
    }

    /// next_boss_hint / next_legend_hint's walk over WorldOrder indices
    /// 1..end−1: open worlds whose shard/legend item nobody holds first, a
    /// second pass takes every open world; three empty passes give index 1.
    fn next_world_hint(&self, index: i32, mut pass: i32, end: i32, held: impl Fn(&Game, i32) -> bool) -> (i32, i32) {
        let mut i = index + 1;
        loop {
            if end - 1 < i {
                i %= end;
                pass = 1;
            }
            let mut j = if i != 0 { i } else { 1 };
            while j < end {
                let w = self.world_at(j);
                if !self.world_open(w) {
                    j += 1;
                } else if pass != 0 || !held(self, w) {
                    break;
                } else {
                    j += 1;
                }
            }
            if j != end {
                return (j, pass);
            }
            if 1 < pass {
                return (1, 1);
            }
            pass += 1;
            i = 1;
        }
    }

    /// PlayerHasShard(-1, n) for one bit: any present player.
    fn any_player_has_shard_world(&self, n: i32) -> bool {
        (0..4).any(|k| self.player_has_shard(k, n.max(0) as u32))
    }
}
