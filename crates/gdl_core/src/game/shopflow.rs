//! The shop between a realm and the Tower (game mode 0x4012): init_shop
//! 0x18b4a0, shop_setup 0x18b648 and do_shop 0x18d9f8 with the per-player
//! steps at player +0x980 — the level's gold/kills/experience piles
//! (init_piles 0x18bcd0, show_piles 0x18bff0), the level-up panel
//! (shop_show_lv 0x18caf8), the shop (do_shopping 0x18a898, show_gold
//! 0x18c3a0), the stats after shopping, the inventory panel and, after
//! Battle 3, the final stats (shop_show_final_stats 0x18d468).
//!
//! Player words: +0x980 step, +0x984 cursor, +0x988 step timer (bit 16:
//! the health line was highlighted), +0x98c/+0x990/+0x994/+0x998 the
//! strength/armor/magic/speed bought here. NATIVE: the shop is drawn
//! straight from this state each frame (blits via the HUD's temp list);
//! SelectLoadDone/LoadTowerDone are immediate; the item list does not
//! scroll (write_shop_menu's scroll_flag stays 0).
use super::hud::Blit;
use super::powerups::{Purchase, ShopItem};
use super::{Game, R};

/// PILE_BOTY 0x24ade4, PILE_TOPY 0x24ade0, PILE_MINY 0x24ade8.
pub const PILE_BOTY: i32 = 320;
pub const PILE_TOPY: i32 = 112;
pub const PILE_MINY: i32 = 20;
/// pile_ypos 0x24adc0 (type 0..2, then the Continue line).
const PILE_YPOS: [i32; 4] = [32, 52, 72, 92];
/// pile_descs 0x24adb0.
const PILE_DESCS: [&str; 3] = ["Gold", "Kills", "Exp."];
/// shop_misctab 0x24ad10 entries 0..2 (x 0, y 320, z −10).
const PILE_TEX: [&str; 3] = ["SHP_GOLD", "SHP_BONES", "SHP_EXP"];
/// The scroll's title colour 0 (DrawTextScaled(0.45, −shop_cx, 8, 6, 0)).
const TITLE_SCALE: f32 = f32::from_bits(0x3ee6_6666);

#[derive(Clone, Debug, Default)]
pub struct ShopFlow {
    /// shop_from_menu: 0 from a realm, 1 the Tower menu, 2 shop skipped.
    pub from_menu: i32,
    pub skip: bool,
    pub players: [ShopPlayer; 4],
    /// vb counter for the glow pulses.
    pub frames: u32,
}

/// Per player: pile_type/pile_cury/pile_maxy/pile_val (by display slot),
/// top_y, afford, bought_flag, the inventory panel timer and whether the
/// pile blits are hidden.
#[derive(Clone, Copy, Debug, Default)]
pub struct ShopPlayer {
    pub pile_type: [usize; 3],
    pub cury: [i32; 3],
    pub maxy: [i32; 3],
    pub val: [i32; 3],
    pub piles_hidden: bool,
    pub gold_shown: bool,
    pub top_y: i32,
    pub afford: i32,
    pub bought: i32,
    pub inv_timer: i32,
    pub inv_closing: bool,
}

impl Game {
    /// init_shop(from_menu): game mode 0x4012, pause/auto exit cleared; from
    /// the Tower menu PlayersRestoreHealth (TowerSetStartPos is the
    /// caller's); skipped for skip_wave > 1 (not 0xc) or when no player is
    /// in state 1 or 5; then shop_setup's per-player resets.
    pub fn init_shop(&mut self, from_menu: i32, items: &[ShopItem]) -> R<ShopFlow> {
        let g = &mut self.globals;
        g.game_mode = 0x4012;
        g.paused = 0;
        g.auto_exit_flag = 0;
        self.auto_exit_timer = 0;
        let mut flow = ShopFlow { from_menu, ..Default::default() };
        if from_menu != 0 {
            self.players_restore_health();
        }
        let sw = self.globals.skip_wave;
        if 1 < sw && sw != 0xc {
            flow.skip = true;
            return Ok(flow);
        }
        if !(0..4).any(|k| matches!(self.players[k].i32_(0xfc), 1 | 5)) {
            flow.skip = true;
        }
        self.globals.skip_wave = 0;
        if !flow.skip {
            self.shop_music_start();
        }
        // ResetModels and setup_player_display for every box.
        self.hud.reset_models();
        self.hud_reset_players();
        for k in 0..4 {
            self.setup_player_display(k);
        }
        // shop_setup.
        for k in 0..4 {
            self.players[k].set_i32(0x980, 0);
            self.players[k].set_i32(0x984, 0);
            self.players[k].set_i32(0x988, 0);
            if !flow.skip && matches!(self.players[k].i32_(0xfc), 1 | 5) {
                let gold = self.players[k].i32_(0x1a20);
                flow.players[k].afford = items.iter().filter(|it| it.price <= gold).count() as i32;
            }
        }
        Ok(flow)
    }

    /// PlayersRestoreHealth 0x130a60: dying players (8) back to 1, health
    /// from the class entry (+0x9b0 + class × 0x18).
    pub fn players_restore_health(&mut self) {
        for k in 0..4 {
            let r = &mut self.players[k];
            if r.i32_(0xfc) == 8 {
                r.set_i32(0xfc, 1);
            }
            let c = r.u32_(0xc) as usize;
            let v = r.u32_(0x9b0 + 0x18 * c);
            r.set_u32(0x1a10, v);
        }
    }

    /// do_shop: 0 while running; 2 once every shopping player is done (from
    /// a realm: init_player_select(2) follows), 1 from the Tower menu
    /// (init_next_level). The frame's blits go to the HUD's temp list.
    pub fn do_shop(&mut self, flow: &mut ShopFlow, items: &[ShopItem]) -> R<i32> {
        flow.frames = flow.frames.wrapping_add(self.globals.vb_elapsed as u32);
        let _ = self.write_player_info(-1);
        let mut done = true;
        let mut tally = false;
        if !flow.skip {
            if flow.from_menu != 2 {
                let panels = self.hud.shop_panel_blits(&self.players);
                self.hud.push_temp(panels);
            }
            for k in 0..4 {
                if !matches!(self.players[k].i32_(0xfc), 1 | 5) {
                    continue;
                }
                let mut finished = false;
                let step = self.players[k].i32_(0x980);
                let start = self.controls.control[k].u32_(8) & 0x200_0000 != 0;
                match step {
                    0 => {
                        flow.players[k].top_y = 0;
                        flow.players[k].bought = 0;
                        self.init_piles(flow, k);
                        for o in [0x988, 0x98c, 0x990, 0x994, 0x998] {
                            self.players[k].set_i32(o, 0);
                        }
                        if flow.from_menu == 0 {
                            self.players[k].set_i32(0x980, 1);
                        } else {
                            flow.players[k].piles_hidden = true;
                            self.init_show_gold(flow, k);
                            self.players[k].set_i32(0x980, 6);
                        }
                    }
                    1..=3 => {
                        if self.show_piles(flow, k) {
                            tally = true;
                        } else if start {
                            self.audio_cursor_select();
                            flow.players[k].piles_hidden = true;
                            self.players[k].set_i32(0x980, 4);
                            self.players[k].set_i32(0x988, 0);
                        }
                    }
                    4 => {
                        if self.shop_show_lv(flow, k, false)? {
                            let g = &self.globals;
                            let next = if g.last_world_played == 8 && g.last_level_played == 3 { 0x14 } else { 5 };
                            self.players[k].set_i32(0x980, next);
                        }
                    }
                    5 => {
                        self.init_show_gold(flow, k);
                        self.players[k].set_i32(0x980, 6);
                    }
                    6 | 7 => {
                        if step == 6 {
                            if flow.from_menu == 2 {
                                self.players[k].set_i32(0x980, 9);
                                continue;
                            }
                            self.players[k].set_i32(0x980, 7);
                        }
                        self.show_gold(flow, k);
                        if self.do_shopping(flow, k, items)? {
                            self.players[k].set_i32(0x980, 8);
                        }
                    }
                    8 => {
                        if self.shop_show_lv(flow, k, true)? {
                            self.players[k].set_i32(0x980, 9);
                        }
                    }
                    9 => {
                        if flow.from_menu == 1 {
                            self.players[k].set_i32(0x980, 100);
                        } else {
                            flow.players[k].inv_timer = 0;
                            flow.players[k].inv_closing = false;
                            self.audio_tower_fx(2);
                            self.players[k].set_i32(0x980, 10);
                        }
                    }
                    10 | 11 => {
                        if self.draw_inventory_panel(flow, k) && step == 11 {
                            self.players[k].set_i32(0x980, 12);
                        } else if step == 10 && start {
                            // end_inventory_panel: panel_fx 2, timer 0.
                            self.audio_cursor_select();
                            flow.players[k].inv_closing = true;
                            flow.players[k].inv_timer = 0;
                            self.players[k].set_i32(0x980, 11);
                        }
                    }
                    0x14 => {
                        if self.shop_show_final_stats(flow, k) {
                            self.players[k].set_i32(0x980, 0x15);
                        }
                    }
                    100 => finished = true,
                    _ => self.players[k].set_i32(0x980, 100),
                }
                if finished {
                    self.players[k].set_i32(0xfc, 5);
                } else {
                    done = false;
                }
            }
            // DoAudioTallySFX: the world's tally loop while a pile grows.
            // PrevGameWorld: the realm level just left (still loaded here).
            let pw = self.globals.cur_world;
            self.do_audio_tally_sfx(tally, pw);
        }
        // AudioStillPlaying(1) every frame: the shop waits for its lines.
        if self.audio_still_playing() || !done {
            return Ok(0);
        }
        self.bank_music_end();
        Ok(if flow.from_menu == 0 { 2 } else { 1 })
    }

    /// init_piles: the level's gold (+0x1a20 − the class backup's +0x30),
    /// kills (the kill counters +0xb2c/+0xb3c − their backups) and
    /// experience (+0x1a1c − backup) as piles scaled against LEVL
    /// +0xe0/+0xe4/+0xe8 + 1 into 0x40..(BOTY − TOPY), ordered for display.
    fn init_piles(&mut self, flow: &mut ShopFlow, p: usize) {
        let span = PILE_BOTY - PILE_TOPY;
        let r = &self.players[p];
        let c = r.u32_(0xc) as usize;
        let gold = r.i32_(0x1a20) - r.i32_(0x1da8 + 0xb4 * c);
        let kills = r.i32_(0xb2c + 0x1c * c) + r.i32_(0xb3c + 0x1c * c) - (r.i32_(0x1bb8 + 0x1c * c) + r.i32_(0x1bc8 + 0x1c * c));
        let exp = r.i32_(0x1a1c) - r.i32_(0x1a38 + 0x18 * c);
        let lv = |o: usize| -> i32 { self.level.get(o..o + 4).map_or(0, |b| i32::from_le_bytes(b.try_into().unwrap())) + 1 };
        let h = |v: i32, m: i32| -> i32 {
            let x = if m == 0 { 0 } else { v * span / m };
            if x < 0x40 {
                0x40
            } else {
                x.min(span)
            }
        };
        let (hg, hk, he) = (h(gold, lv(0xe0)), h(kills, lv(0xe4)), h(exp, lv(0xe8)));
        // Slots: gold s_g, exp s_e, kills s_k (the comparisons of init_piles).
        let (sg, se, sk) = if hg < he || hg < hk {
            if he < hg || he < hk {
                if he <= hg { (1, 2, 0) } else { (2, 1, 0) }
            } else if hk <= hg {
                (1, 0, 2)
            } else {
                (2, 0, 1)
            }
        } else if he < hk {
            (0, 2, 1)
        } else {
            (0, 1, 2)
        };
        let f = &mut flow.players[p];
        f.pile_type[sg] = 0;
        f.pile_type[se] = 2;
        f.pile_type[sk] = 1;
        f.cury = [PILE_MINY; 3];
        f.maxy[sg] = hg;
        f.maxy[se] = he;
        f.maxy[sk] = hk;
        f.val[sg] = gold;
        f.val[se] = exp;
        f.val[sk] = kills;
        f.piles_hidden = false;
    }

    /// show_piles: the first unfinished pile grows by 1.5 × vb_elapsed per
    /// frame (later ones hidden); each pile's "Gold: n" line (glowing while
    /// it grows); when all are up the Continue prompt. True while growing.
    fn show_piles(&mut self, flow: &mut ShopFlow, p: usize) -> bool {
        let Some(t) = self.hud.tables.as_ref() else { return false };
        let lx = t.shop_lx.get(p).copied().unwrap_or(0);
        let cx = t.shop_cx.get(p).copied().unwrap_or(0);
        let step = (self.globals.vb_elapsed >> 1) + self.globals.vb_elapsed;
        let mut growing = 0;
        let mut out = Vec::new();
        for k in 0..3 {
            let f = &mut flow.players[p];
            let ty = f.pile_type[k];
            let mut grew = false;
            if growing == 0 {
                let mut h = f.maxy[k];
                if f.cury[k] < f.maxy[k] {
                    h = (f.cury[k] + step).min(f.maxy[k]);
                    growing = k + 1;
                    grew = true;
                }
                f.cury[k] = h;
                let mut b = self.hud.make_pub(self.hud.lookup(PILE_TEX[ty]), lx, PILE_BOTY - h, -1, -1);
                b.h = h as f32;
                b.crop = [0.0, 1.0, 0.0, h as f32 / 256.0];
                b.z = 64000.0 - 10.0;
                out.push(b);
            }
            let text = format!("{}: {}", PILE_DESCS[ty], flow.players[p].val[k]);
            if grew {
                out.extend(self.hud.glow_text(0.5, lx + 0x10, PILE_YPOS[ty], &text, flow.frames));
            } else {
                self.hud.draw_text(0.5, lx + 0x10, PILE_YPOS[ty], 6, 0xffffff, &text);
            }
        }
        if growing == 0 {
            out.extend(self.hud.glow_text(0.5, lx + 0x20, PILE_YPOS[3], " Continue", flow.frames));
            if let Some(x) = self.hud.button_x {
                out.push(self.hud.make_pub(Some(x), lx + 0x10, PILE_YPOS[3] - 3, 0x14, 0x14));
            }
        }
        self.hud.draw_text(TITLE_SCALE, -cx, 8, 6, 0, "Stats");
        self.hud.push_temp(out);
        growing != 0
    }

    /// init_show_gold: the gold pile's slot keeps its height and targets the
    /// gold now held.
    fn init_show_gold(&mut self, flow: &mut ShopFlow, p: usize) {
        let f = &mut flow.players[p];
        let k = f.pile_type.iter().position(|&t| t == 0).unwrap_or(0);
        f.cury[k] = f.maxy[k];
        f.maxy[k] = self.players[p].i32_(0x1a20);
        f.gold_shown = true;
    }

    /// show_gold: the gold pile shrinks (1.5 × vb_elapsed per frame) towards
    /// (BOTY − TOPY − MINY) × gold / (maxy + 1) + MINY; hidden at 0 gold.
    fn show_gold(&mut self, flow: &mut ShopFlow, p: usize) {
        let Some(t) = self.hud.tables.as_ref() else { return };
        let lx = t.shop_lx.get(p).copied().unwrap_or(0);
        let step = (self.globals.vb_elapsed >> 1) + self.globals.vb_elapsed;
        let gold = self.players[p].i32_(0x1a20);
        let f = &mut flow.players[p];
        let k = f.pile_type.iter().position(|&t| t == 0).unwrap_or(0);
        if gold == 0 {
            return;
        }
        let target = (PILE_BOTY - PILE_TOPY - PILE_MINY) * gold / (f.maxy[k] + 1) + PILE_MINY;
        let mut h = target;
        if target < f.cury[k] {
            h = (f.cury[k] - step).max(target);
        }
        f.cury[k] = h;
        let mut b = self.hud.make_pub(self.hud.lookup(PILE_TEX[0]), lx, PILE_BOTY - h, -1, -1);
        b.h = h as f32;
        b.crop = [0.0, 1.0, 0.0, h as f32 / 256.0];
        b.z = 64000.0 - 10.0;
        self.hud.push_temp([b]);
    }

    /// do_shopping: up/down move the cursor (+0x984) over affordable,
    /// unskipped items (wrapping at the afford count), accept buys, back
    /// returns to entry 0; the list is drawn around the cursor. True when
    /// entry 0 (leave) was accepted.
    fn do_shopping(&mut self, flow: &mut ShopFlow, p: usize, items: &[ShopItem]) -> R<bool> {
        if items.is_empty() {
            return Ok(true);
        }
        let edges = self.controls.control[p].u32_(8);
        let (down, up) = (edges & 0xc0 != 0, edges & 0x30 != 0);
        let gold = self.players[p].i32_(0x1a20);
        let skip = self.shop_skiplist(p, items)?;
        let valid = |k: usize| items.get(k).is_some_and(|it| it.price <= gold) && !skip.get(k).copied().unwrap_or(false);
        let mut cur = self.players[p].i32_(0x984).max(0) as usize;
        if down || up {
            self.audio_click2(p, i32::from(up));
            let afford = flow.players[p].afford.max(1) as usize;
            for _ in 0..=items.len() {
                if down {
                    cur += 1;
                    if afford <= cur {
                        cur = 0;
                    }
                } else if cur == 0 {
                    // recalc_afford_idx: the last affordable entry.
                    cur = items.iter().rposition(|it| it.price <= gold).unwrap_or(0);
                    flow.players[p].afford = cur as i32 + 1;
                } else {
                    cur -= 1;
                }
                if valid(cur) {
                    break;
                }
            }
            self.players[p].set_i32(0x984, cur as i32);
        }
        let mut leave = false;
        if edges & 0x200_0000 != 0 {
            match self.shop_buy(p, items, cur)? {
                Purchase::Leave => leave = true,
                Purchase::Bought => {
                    flow.players[p].bought += 1;
                    self.audio_pickup(p);
                    let gold = self.players[p].i32_(0x1a20);
                    // The cursor steps back to an affordable, unskipped entry.
                    let skip = self.shop_skiplist(p, items)?;
                    while 0 < cur && (gold < items[cur].price || skip.get(cur).copied().unwrap_or(false)) {
                        cur -= 1;
                    }
                    self.players[p].set_i32(0x984, cur as i32);
                }
                Purchase::Refused => self.audio_buzzer(),
            }
        } else if edges & 0x800_0000 != 0 && cur != 0 {
            cur = 0;
            self.players[p].set_i32(0x984, 0);
        }
        let list = self.hud.shop_list_blits(items, p, cur, flow.frames as i32);
        self.hud.push_temp(list);
        let cx = self.hud.tables.as_ref().and_then(|t| t.shop_cx.get(p).copied()).unwrap_or(0);
        self.hud.draw_text(TITLE_SCALE, -cx, 8, 6, 0, "Shop");
        Ok(leave)
    }

    /// shop_show_lv(after): the level panel. Before shopping it shows only
    /// after a level-up (the backup experience's level differs) with the
    /// old-level attributes (backup bonuses) revealed one changed line per
    /// 0x3c frames from 0x5a; after shopping (0x1e) the bought amounts.
    /// Continue (X) once every line is shown. True when dismissed (or no
    /// level-up).
    fn shop_show_lv(&mut self, flow: &mut ShopFlow, p: usize, after: bool) -> R<bool> {
        let Some(t) = self.hud.tables.as_ref() else { return Ok(true) };
        let lx = t.shop_lx.get(p).copied().unwrap_or(0);
        let cx = t.shop_cx.get(p).copied().unwrap_or(0);
        let (x0, x1) = (lx + 8, lx + 0x58);
        let c = self.players[p].u32_(0xc) as usize;
        let lv = self.players[p].i32_(0x2ac0);
        let old_lv = if after { lv } else { crate::player_class::exp_to_level(self.players[p].i32_(0x1a38 + 0x18 * c)) };
        if !after && old_lv == lv {
            return Ok(true);
        }
        let timer = self.players[p].u32_(0x988);
        let shown = (timer & 0xffff) as u16;
        if timer == 0 {
            if !after {
                self.audio_gained_level(p, 1);
            }
            self.players[p].set_u32(0x988, 1);
        }
        if shown < 0xf000 {
            let v = self.players[p].u32_(0x988).wrapping_add(self.globals.vb_elapsed as u32);
            self.players[p].set_u32(0x988, v);
        }
        let mut out = Vec::new();
        let level_text = format!("Level {lv}");
        if !after {
            out.extend(self.hud.glow_text(0.75, -cx, 0x20, &level_text, flow.frames));
        } else {
            self.hud.draw_text(0.75, -cx, 0x20, 6, 0xffffff, &level_text);
        }
        let rank = if lv == 99 {
            self.hud.string_text(0x15, 0)
        } else if 9 < lv {
            let list = self.hud.text.as_ref().and_then(|r| r.lists.first()).and_then(|(_, v)| v.get(self.players[p].i32_(0xc) as usize).copied());
            list.and_then(|e| self.hud.string_text(e as i32, (lv / 10) >> 1))
        } else {
            self.hud.string_text(0x17, self.players[p].i32_(0xc))
        };
        self.hud.draw_text(f32::from_bits(0x3f19_999a), -cx, 0x40, 6, 0xffffff, &rank.unwrap_or_default());
        // The attributes at the old level with the backup's bonuses, minus
        // what was bought here (PlayerUpdateAtts(player, class, backup)).
        let pd = super::profile::Pdats({
            let mut v = vec![Vec::new(); 16];
            if let Some(s) = v.get_mut(c) {
                *s = self.classes[p].raw.clone();
            }
            v
        });
        let cur = [self.players[p].f32_(0x108), self.players[p].f32_(0x10c), self.players[p].f32_(0x110), self.players[p].f32_(0x114)];
        let old = if after {
            cur
        } else {
            let mut r = self.players[p].clone();
            r.set_i32(0x2ac0, old_lv);
            for k in 0..4 {
                let v = r.f32_(0x1a38 + 0x18 * c + 8 + 4 * k);
                r.set_f32(super::profile::CLASS_ATTS + 0x18 * c + 8 + 4 * k, v);
            }
            super::profile::player_update_atts(&mut r, c as u32, &pd).map_err(|e| super::Unsupported(e.to_string()))?;
            [r.f32_(0x108), r.f32_(0x10c), r.f32_(0x110), r.f32_(0x114)]
        };
        let bought = [self.players[p].f32_(0x98c), self.players[p].f32_(0x990), self.players[p].f32_(0x994), self.players[p].f32_(0x998)];
        let mut gate: u16 = if after { 0x1e } else { 0x5a };
        let label_scale = f32::from_bits(0x3ef5_c28f);
        for (k, (name, y)) in [("Strength", 0x60), ("Armor", 0x74), ("Magic", 0x88), ("Speed", 0x9c)].into_iter().enumerate() {
            let before = old[k] - bought[k];
            let same = before == cur[k];
            if same || shown <= gate || gate + 0x3c <= shown {
                self.hud.draw_text(label_scale, x0, y, 6, 0xffffff, name);
            } else {
                out.extend(self.hud.glow_text(label_scale, x0, y, name, flow.frames));
            }
            if same || shown <= gate {
                self.hud.draw_text(label_scale, x1, y, 6, 0xffffff, &(before as i32).to_string());
            } else {
                out.extend(self.hud.glow_text(label_scale, x1, y, &(cur[k] as i32).to_string(), flow.frames));
            }
            if !same {
                gate += 0x3c;
            }
        }
        let max_health = crate::player_class::max_health(lv);
        if after || shown <= gate || gate + 0x3c <= shown {
            self.hud.draw_text(label_scale, x0, 0xbc, 6, 0xffffff, "Max");
            self.hud.draw_text(label_scale, x0, 0xcc, 6, 0xffffff, "Health");
        } else {
            out.extend(self.hud.glow_text(label_scale, x0, 0xbc, "Max", flow.frames));
            out.extend(self.hud.glow_text(label_scale, x0, 0xcc, "Health", flow.frames));
        }
        if after || shown <= gate {
            let mut v = max_health as i32;
            if !after {
                v = (max_health - (lv - old_lv) as f32 * 100.0) as i32;
            }
            self.hud.draw_text(label_scale, x1, 0xc4, 6, 0xffffff, &v.to_string());
        } else {
            out.extend(self.hud.glow_text(label_scale, x1, 0xc4, &(max_health as i32).to_string(), flow.frames));
            let v = self.players[p].u32_(0x988) | 0x1_0000;
            self.players[p].set_u32(0x988, v);
        }
        if !after {
            gate += 0x3c;
            // Levels 25 and 50: the class's new move (GameText 0xb8 / 0xb9).
            let e = match lv {
                0x19 => Some(0xb8),
                0x32 => Some(0xb9),
                _ => None,
            };
            if let Some(e) = e {
                let class = self.players[p].i32_(8);
                self.hud.draw_string_text(-cx, 0xe0, 6, 0xff80c0, e, class, &[]);
            }
        }
        self.hud.draw_text(TITLE_SCALE, -cx, 8, 6, 0, "Stats");
        let ready = gate <= shown;
        let mut dismissed = false;
        if ready {
            if self.controls.control[p].u32_(8) & 0x200_0000 != 0 {
                self.audio_cursor_select();
                self.players[p].set_u32(0x988, 0);
                dismissed = true;
            } else {
                if let Some(x) = self.hud.button_x {
                    out.push(self.hud.make_pub(Some(x), lx + 0x10, 0x118, 0x10, 0x10));
                }
                out.extend(self.hud.glow_text(0.5, lx + 0x28, 0x118, "Continue", flow.frames));
            }
        }
        self.hud.push_temp(out);
        Ok(dismissed)
    }

    /// draw_inventory_panel 0x129668 in the shop: the panel swirls in over
    /// 120 timer units (panel_fx 0) and, after end_inventory_panel, out over
    /// 15 (panel_fx 2, animate_panel_piece mode 2); the timer advances by
    /// SWIRL_RATE × vb_elapsed_ctl after the draw. True once closed (timer
    /// > 14).
    fn draw_inventory_panel(&mut self, flow: &mut ShopFlow, p: usize) -> bool {
        let rate = self.hud.tables.as_ref().map_or(1, |t| t.swirl_rate);
        let vb = self.globals.vb_elapsed;
        let f = &mut flow.players[p];
        let timer = f.inv_timer;
        let closing = f.inv_closing;
        f.inv_timer = (f.inv_timer + rate * vb).min(if closing { i32::MAX } else { 120 });
        let closed = closing && 0xe < f.inv_timer;
        let max_crystals = self.items.tables.max_crystals.clone();
        let mut players: [super::record::Record; 4] = std::array::from_fn(|k| self.players[k].clone());
        for (k, r) in players.iter_mut().enumerate() {
            if k != p {
                r.set_i32(0xfc, 0);
            }
        }
        let list = self.hud.inventory_blits(&players, &max_crystals, timer, closing.then_some(timer));
        self.hud.push_temp(list);
        closed
    }

    /// shop_show_final_stats (after Battle 3): enemies killed, generators
    /// destroyed, gold found and total playtime (days/hours/minutes of the
    /// class's play counter) revealed in turn; Continue (X).
    fn shop_show_final_stats(&mut self, flow: &mut ShopFlow, p: usize) -> bool {
        let Some(t) = self.hud.tables.as_ref() else { return true };
        let lx = t.shop_lx.get(p).copied().unwrap_or(0) + 8;
        let cx = t.shop_cx.get(p).copied().unwrap_or(0);
        let r = &self.players[p];
        let c = r.u32_(0xc) as usize;
        let stats = 0xb2c + 0x1c * c;
        let (kills, gens, gold, play) = (r.i32_(stats), r.i32_(stats + 0x10), r.i32_(stats + 0x14), r.i32_(stats + 0x18));
        let timer = r.u32_(0x988);
        let shown = (timer & 0xffff) as u16;
        if shown < 0xf000 {
            let v = timer.wrapping_add(self.globals.vb_elapsed as u32);
            self.players[p].set_u32(0x988, v);
        }
        let s = f32::from_bits(0x3ef5_c28f);
        let mut out = self.hud.glow_text(f32::from_bits(0x3f0f_5c29), -cx, 0x20, "Final Stats", flow.frames);
        let line = |hud: &mut super::hud::Hud, out: &mut Vec<Blit>, text: &str, y: i32, lo: u16, hi: u16| {
            if shown < lo || hi < shown {
                hud.draw_text(s, -cx, y, 6, 0xffffff, text);
            } else {
                out.extend(hud.glow_text(s, -cx, y, text, flow.frames));
            }
        };
        line(&mut self.hud, &mut out, "Enemies Killed", 0x3c, 0x5b, 0x95);
        if 0x5a < shown {
            out.extend(self.hud.glow_text(s, -cx, 0x4e, &kills.to_string(), flow.frames));
        }
        line(&mut self.hud, &mut out, "Generators", 0x62, 0x97, 0xd1);
        line(&mut self.hud, &mut out, "Destroyed", 0x74, 0x97, 0xd1);
        if 0x96 < shown {
            out.extend(self.hud.glow_text(s, -cx, 0x86, &gens.to_string(), flow.frames));
        }
        line(&mut self.hud, &mut out, "Gold Found", 0x9a, 0xd3, 0x10d);
        if 0xd2 < shown {
            out.extend(self.hud.glow_text(s, -cx, 0xac, &gold.to_string(), flow.frames));
        }
        line(&mut self.hud, &mut out, "Total Playtime", 0xc0, 0x10f, 0x149);
        // The counter in vblanks: days of 5184000, hours of 216000,
        // minutes of 3600 (float arithmetic as the original).
        let pt = play as f32;
        let days = (pt / 5.184e6) as i32;
        let rem = pt - days as f32 * 5.184e6;
        let hours = (rem / 216000.0) as i32;
        let mins = ((rem - hours as f32 * 216000.0) / 3600.0) as i32;
        if 0x10e < shown {
            out.extend(self.hud.glow_text(s, -cx, 0xd2, &format!("{days} Days"), flow.frames));
            out.extend(self.hud.glow_text(s, -cx, 0xe4, &format!("{hours} Hours"), flow.frames));
            out.extend(self.hud.glow_text(s, -cx, 0xf6, &format!("{mins} Minutes"), flow.frames));
        }
        let mut dismissed = false;
        if 0x149 < shown {
            if self.controls.control[p].u32_(8) & 0x200_0000 != 0 {
                self.audio_cursor_select();
                self.players[p].set_u32(0x988, 0);
                dismissed = true;
            } else {
                if let Some(x) = self.hud.button_x {
                    out.push(self.hud.make_pub(Some(x), lx + 8, 0x118, 0x10, 0x10));
                }
                out.extend(self.hud.glow_text(0.5, lx + 0x20, 0x118, "Continue", flow.frames));
            }
        }
        self.hud.draw_text(TITLE_SCALE, -cx, 8, 6, 0, "Stats");
        self.hud.push_temp(out);
        dismissed
    }
}
