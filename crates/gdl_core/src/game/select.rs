//! Player-select screen blits (init_player_select 0x1851b0, setup_tex,
//! update_class_spec, set_selected_blit, setup_sel_menu, do_sel_menu): four
//! columns at sel_xpos (0, 128, 256, 384) with the sel_bg slots of
//! sel_bg_pos {x, y, z}: 0 S1_plyrN, 1 S2_plyrN (z 9), 2 S12_WEAP_<class>
//! (z 8, height 0x140), 3 S12_<CLASS>_<COL> (z 7), 7 selscrn_questmark,
//! 8 <CLASS>_NAME, 9/10 S1/S2_border (z 3). Menus use sel_def_menu's style.
//! NATIVE (labelled): the memory-card states, the join/ready flow and the
//! blit effects (serve_blits) are not ported; the caller supplies what the
//! first column shows.
use super::hud::{Blit, Hud};
use super::optmenu::{MenuEntry, MenuState, OptMenu, OptStyle};


/// What a column shows.
#[derive(Clone, Debug)]
pub enum Column {
    /// A player slot not in play: the column frame only.
    Empty,
    /// A menu (sel_def_menu style): entries and the selection.
    /// `file_list`: setup_sel_menu case 8's placement (x + 8, y 0x46,
    /// scale 0.6); otherwise sel_def_menu's (centred on the column).
    Menu { title: Vec<String>, entries: Vec<String>, selected: usize, file_list: bool },
    /// Class / colour choice: the S12 art, weapon, name and the
    /// L/R "Change" hint (do_sel_menu case 3).
    Class { class: usize, colour: usize },
    /// Name entry (do_sel_menu case 2): "Enter Your Name", the name and
    /// the button rows.
    Name { class: usize, colour: usize, name: String },
}

impl Hud {
    fn sel_x(&self, p: usize) -> i32 {
        self.tables.as_ref().and_then(|t| t.sel_xpos.get(p).copied()).unwrap_or(0)
    }

    /// setup_tex(p, slot, …): the slot's sel_bg_pos {x, y, z}, size from
    /// the texture (height `h` when ≥ 0).
    fn sel_tex(&self, out: &mut Vec<Blit>, p: usize, slot: usize, name: &str, h: i32) {
        let Some([x, y, z]) = self.tables.as_ref().and_then(|t| t.sel_bg_pos.get(slot).copied()) else { return };
        let mut b = self.make_pub(self.lookup(name), self.sel_x(p) + x, y, -1, h);
        b.z = z as f32;
        out.push(b);
    }

    /// The select screen for four columns; `frames` drives menu fades.
    pub fn select_blits(&self, style: &OptStyle, def_menu: &OptMenu, cols: &[Column; 4], frames: i32) -> Vec<Blit> {
        let mut out = Vec::new();
        for (p, col) in cols.iter().enumerate() {
            self.sel_tex(&mut out, p, 0, &format!("S1_plyr{}", p + 1), -1);
            self.sel_tex(&mut out, p, 1, &format!("S2_plyr{}", p + 1), -1);
            self.sel_tex(&mut out, p, 9, "S1_border", -1);
            self.sel_tex(&mut out, p, 10, "S2_border", -1);
            let xp = self.sel_x(p);
            let cx = -(xp + 0x40);
            let text = |hud: &Hud, out: &mut Vec<Blit>, scale: f32, font: usize, x: i32, y: i32, s: &str| {
                if let Some(m) = hud.message_pub(scale, scale, x, y, font, 0xffffff, 0x80, 0.25, 0, s) {
                    out.extend(hud.glyphs(&m));
                }
            };
            let art = |hud: &Hud, out: &mut Vec<Blit>, class: usize, colour: usize| {
                let Some(t) = hud.tables.as_ref() else { return };
                let c = t.player_desc_uc.get(class).cloned().unwrap_or_default();
                let k = t.player_color_uc.get(colour).cloned().unwrap_or_default();
                // set_selected_blit (slot 2, height 0x140), update_class_spec
                // (slot 3 S12_%s_%s, slot 8 %s_NAME).
                hud.sel_tex(out, p, 2, &format!("S12_WEAP_{c}"), 0x140);
                hud.sel_tex(out, p, 3, &format!("S12_{c}_{k}"), -1);
                hud.sel_tex(out, p, 8, &format!("{c}_NAME"), -1);
            };
            // do_sel_menu's button hints: sel_msg_scale 1.2 × 16 buttons.
            let bs = (1.2f32 * 16.0) as i32;
            let bx = bs + 10 + xp;
            let button = |hud: &Hud, out: &mut Vec<Blit>, name: &str, x: i32, y: i32| {
                let mut b = hud.make_pub(hud.lookup(name), x, y, bs, bs);
                b.z = 1.0;
                out.push(b);
            };
            match col {
                Column::Empty => {}
                Column::Menu { title, entries, selected, file_list } => {
                    let mut y = 0x30;
                    for t in title {
                        text(self, &mut out, 0.8, 6, cx, y, t);
                        y += 0x1a;
                    }
                    let mut menu = def_menu.clone();
                    if *file_list {
                        // setup_sel_menu case 8 (the file list): x = sel_xpos
                        // + 8, y 0x46, scale 0.6.
                        menu.x = xp + 8;
                        menu.y = 0x46;
                        menu.scale = f32::from_bits(0x3f19_999a);
                    } else {
                        // setup_sel_menu: x −= sel_xpos.
                        menu.x -= xp;
                    }
                    menu.entries = entries.iter().map(|e| MenuEntry { label: e.clone(), action: 0, gap: 0, value: None, colour: 0, mark: 0, state: 0 }).collect();
                    let state = MenuState { selected: *selected, frames, ..Default::default() };
                    out.extend(self.optmenu_blits(style, &menu, &state));
                }
                Column::Class { class, colour } => {
                    art(self, &mut out, *class, *colour);
                    let y = 180 + 0x34; // sel_namemsg_y + 0x34
                    button(self, &mut out, "BUTTON_L", bx - bs, y);
                    button(self, &mut out, "BUTTON_R", bx, y);
                    text(self, &mut out, 1.2, 0, bx + bs + 8, 180 + 0x38, "Change");
                }
                Column::Name { class, colour, name } => {
                    art(self, &mut out, *class, *colour);
                    for (k, w) in ["Enter", "Your", "Name"].iter().enumerate() {
                        text(self, &mut out, 0.8, 6, cx, 0x40 + 0x1a * k as i32, w);
                    }
                    text(self, &mut out, 0.8, 6, cx, 0x92, &format!("{name}_"));
                    let y = 180;
                    for (row, (buttons, label)) in [(&["BUTTON_U", "BUTTON_D"][..], "Change"), (&["BUTTON_L", "BUTTON_R"][..], "Edit"), (&["BUTTON_X"][..], "Accept"), (&["BUTTON_TRI"][..], "Cancel")].iter().enumerate() {
                        let ry = y + 0x14 * row as i32;
                        if buttons.len() == 2 {
                            button(self, &mut out, buttons[0], bx - bs, ry);
                            button(self, &mut out, buttons[1], bx, ry);
                        } else {
                            button(self, &mut out, buttons[0], bx, ry);
                        }
                        text(self, &mut out, 1.2, 0, bx + bs + 8, ry + 4, label);
                    }
                }
            }
        }
        out
    }
}

impl Hud {
    /// draw_fullscreen_inventory / draw_inventory_panel 0x129668 for the
    /// four columns (x = 0x80·p): Panel_Pieces (s1/s2_plyrN, borders), the
    /// player's I_Pieces (window, garg items, crystals) with print_n_of_m
    /// counts, Quest_Pieces (legend items, "_empty" when not held) and the
    /// shards held, swirling in over 120 timer units (SWIRL_RATE per vblank;
    /// animate_panel_piece mode 0), then "Continue" (X) and "Inv".
    /// `timer` is panel_fx_timer; players in state 0 show the panel only.
    /// `exit`: end_fullscreen_inventory's panel_fx 2 timer (the exit swirl).
    pub fn inventory_blits(&self, players: &[super::record::Record; 4], max_crystals: &[i32], timer: i32, exit: Option<i32>) -> Vec<Blit> {
        let mut out = Vec::new();
        let Some(t) = self.tables.as_ref() else { return out };
        let (fx, f) = match exit {
            Some(e) => (2, e as f32 / 15.0),
            None if timer < 120 => (0, timer as f32 / 120.0),
            None => (1, 1.0),
        };
        let piece = |hud: &Hud, out: &mut Vec<Blit>, name: &str, base: &[i32; 8], x0: i32| {
            let Some(tex) = hud.lookup(name) else { return };
            let (w, h) = hud.tex_info(tex).map_or((0, 0), |i| (i32::from(i.width), i32::from(i.height)));
            let (bx, by) = (x0 + base[0], base[1]);
            let mut b = hud.make_pub(Some(tex), bx, by, w, h);
            b.z = 3.0;
            if fx == 0 {
                // animate_panel_piece mode 0: swirl, grow, fade.
                let f2 = f * f;
                let k = ((base[0] + base[1] + (f2 * f * 3.0 * 60.0) as i32).rem_euclid(60)) as usize;
                let m = t.swirl_mult as f32 * (1.0 - f);
                b.x = bx as f32 + m * t.cos_table.get(k).copied().unwrap_or(1.0);
                b.y = by as f32 + m * t.sin_table.get(k).copied().unwrap_or(0.0);
                let g = 1.0 - f2;
                b.w = (w as f32 + w as f32 * g) as i32 as f32;
                b.h = (h as f32 + h as f32 * g) as i32 as f32;
                b.rgba[3] = (0x80 - (((g * 255.0) as i32).clamp(0, 255) as u32 >> 1)) as u8;
            } else if fx == 2 {
                // animate_panel_piece mode 2: out from (0x40, 0xb4), ×(1 +
                // 5f) larger, fading.
                b.x = (bx as f32 + (base[0] - 0x40) as f32 * f * 10.0) as i32 as f32;
                b.y = (by as f32 + (base[1] - 0xb4) as f32 * f * 10.0) as i32 as f32;
                b.w = (w as f32 + w as f32 * f * 5.0) as i32 as f32;
                b.h = (h as f32 + h as f32 * f * 5.0) as i32 as f32;
                b.rgba[3] = (0x80 - (((f * 255.0) as i32).clamp(0, 255) as u32 >> 1)) as u8;
            }
            out.push(b);
        };
        for (p, r) in players.iter().enumerate() {
            let x0 = 0x80 * p as i32;
            // init_panel_blits: entries 0/1 get the player number appended.
            for (k, (name, base)) in t.panel_pieces.iter().enumerate() {
                let Some(name) = name else { continue };
                let name = if k < 2 { format!("{name}{}", p + 1) } else { name.clone() };
                let mut b = self.make_pub(self.lookup(&name), x0 + base[0], base[1], -1, -1);
                b.z = 3.0;
                out.push(b);
            }
            if r.i32_(0xfc) == 0 {
                continue;
            }
            let cls = 0xb4 * r.u32_(0xc) as usize;
            for (name, base) in &t.i_pieces {
                if let Some(n) = name {
                    piece(self, &mut out, n, base, x0);
                }
            }
            let legend = r.u16_(0xcf4 + cls);
            for (k, (name, base)) in t.quest_pieces.iter().enumerate() {
                if let Some(n) = name {
                    let n = if legend >> k & 1 == 0 { format!("{n}_empty") } else { n.clone() };
                    piece(self, &mut out, &n, base, x0);
                }
            }
            let shards = r.u16_(0xcf0 + cls) | r.u16_(0x1a24);
            for (k, (name, base)) in t.shard_pieces.iter().enumerate() {
                if let Some(n) = name
                    && shards >> k & 1 != 0
                {
                    piece(self, &mut out, n, base, x0);
                }
            }
            let alpha = match fx {
                0 => ((1.0 - f) * 255.0) as i32,
                2 => (f * 255.0) as i32,
                _ => 0,
            };
            let font = t.ifont_font;
            let n_of_m = |hud: &Hud, out: &mut Vec<Blit>, i: usize, n: i32, m: i32| {
                let Some((_, b)) = t.i_pieces.get(i) else { return };
                let scale = f32::from_bits(b[3] as u32);
                if scale <= 0.0 {
                    return;
                }
                let n = if n < 0 || m < n { m } else { n };
                let x = x0 + b[0] + b[4];
                let y = b[1] + b[5];
                let a = (0x80 - (alpha.clamp(0, 255) as u32 >> 1)) as u8;
                let s = n.to_string();
                let w = hud.string_width(font, scale, s.as_bytes());
                for (tx, txt) in [(x - w, s), (x, format!("/{m}"))] {
                    if let Some(msg) = hud.message_pub(scale, scale, tx, y, font, 0xffffff, a, 0.25, 0, &txt) {
                        out.extend(hud.glyphs(&msg));
                    }
                }
            };
            let c16 = |o: usize| i32::from(r.i16_(o + cls));
            n_of_m(self, &mut out, 1, c16(0xd04), 0xc);
            n_of_m(self, &mut out, 2, c16(0xd06), 0x14);
            n_of_m(self, &mut out, 3, c16(0xd08), 0x1c);
            for i in 1..9 {
                n_of_m(self, &mut out, i + 3, c16(0xd0a + 2 * i), max_crystals.get(i).copied().unwrap_or(0));
            }
            if fx != 2 {
                let mut b = self.make_pub(self.lookup("BUTTON_X"), x0 + 0x10, 0x118, 0x10, 0x10);
                b.z = 1.0;
                out.push(b);
                out.extend(self.glow_text(0.5, x0 + 0x28, 0x118, "Continue", timer as u32));
            }
            if let Some(msg) = self.message_pub(f32::from_bits(0x3ee6_6666), f32::from_bits(0x3ee6_6666), -(x0 + 0x40), 8, 6, 0, 0x80, 0.25, 0, "Inv") {
                out.extend(self.glyphs(&msg));
            }
        }
        out
    }
}

impl Hud {
    /// shop_setup 0x18b648 / draw_shop_screen / write_shop_menu 0x189f80
    /// for `player`'s column (the others show their frames): S1/S2_PLYRn
    /// and borders (z 64000 / 63900), SHOP_TOP_<colour> (z 63800), the
    /// scroll pieces of shop_misctab 3/4, then the item list placed by
    /// calc_shop_ypos around the selection (icon +0x18, description lines
    /// at item scale × 0.5 + 0x10), faded within shop_dim_dy of
    /// shop_topy/shop_boty, prices at shop_rx − 0x40 (font 6 × 0.5),
    /// descriptions centred on shop_cx, the selection drawn with
    /// DrawGlowText, and MORE_UP/MORE_DOWN when items are cut off.
    /// NATIVE: no scroll animation (items sit at their target y), no
    /// highlight timers or class skip list.
    pub fn shop_blits(&self, items: &[super::powerups::ShopItem], players: &[super::record::Record; 4], player: usize, selected: usize, frames: i32) -> Vec<Blit> {
        let mut out = self.shop_panel_blits(players);
        out.extend(self.shop_list_blits(items, player, selected, frames));
        out
    }

    /// shop_setup's column frames: S1/S2_PLYRn, the borders and
    /// SHOP_TOP_<colour> for present players.
    pub fn shop_panel_blits(&self, players: &[super::record::Record; 4]) -> Vec<Blit> {
        let mut out = Vec::new();
        let Some(t) = self.tables.as_ref() else { return out };
        for (p, r) in players.iter().enumerate() {
            let lx = t.shop_lx.get(p).copied().unwrap_or(0);
            for (name, y, z) in [(format!("S1_PLYR{}", p + 1), 0, 64000.0), (format!("S2_PLYR{}", p + 1), 0x100, 64000.0), ("S1_BORDER".into(), 0, 63900.0), ("S2_BORDER".into(), 0x100, 63900.0)] {
                let mut b = self.make_pub(self.lookup(&name), lx, y, 0x80, -1);
                b.z = z;
                // The borders: MBBlitSetFlags(0x4000) and MBBlitSetColor4
                // with 0x80808080 at every corner (the corner sweep).
                if z == 63900.0 {
                    b.flags |= 0x4000;
                    b.set_colour4([0x8080_8080; 4]);
                }
                out.push(b);
            }
            if r.i32_(0xfc) != 0 {
                let col = t.pcol.get(r.u32_(4) as usize).cloned().unwrap_or_default();
                let mut b = self.make_pub(self.lookup(&format!("SHOP_TOP_{col}")), lx + 0x20, 0, -1, -1);
                b.z = 63800.0;
                out.push(b);
            }
        }
        out
    }

    /// draw_shop_screen / write_shop_menu for `player`'s column.
    pub fn shop_list_blits(&self, items: &[super::powerups::ShopItem], player: usize, selected: usize, frames: i32) -> Vec<Blit> {
        let mut out = Vec::new();
        let Some(t) = self.tables.as_ref() else { return out };
        let font = 6;
        let p = player.min(3);
        let lx = t.shop_lx.get(p).copied().unwrap_or(0);
        let rx = t.shop_rx.get(p).copied().unwrap_or(0);
        let cx = t.shop_cx.get(p).copied().unwrap_or(0);
        let [top, bot, dim] = t.shop_y;
        for k in [3usize, 4] {
            if let Some((name, x, y, z)) = t.shop_misc.get(k) {
                let mut b = self.make_pub(self.lookup(name), lx + x, *y, -1, -1);
                b.z = 64000.0 + *z as f32;
                out.push(b);
            }
        }
        // calc_shop_ypos.
        let heights: Vec<i32> = items
            .iter()
            .map(|it| {
                let mut h = if self.lookup(&it.icon).is_some() && !it.icon.is_empty() { 0x18 } else { 0 };
                if !it.text.is_empty() {
                    let n = it.text.split('\n').count() as i32;
                    h += n * self.font_height(font, it.word_40 * 0.5) + 0x10;
                }
                h
            })
            .collect();
        let mut ypos = Vec::with_capacity(items.len());
        let mut acc = 0;
        for h in &heights {
            ypos.push(acc);
            acc += h;
        }
        let sel = selected.min(items.len().saturating_sub(1));
        let base_sel = ypos.get(sel).copied().unwrap_or(0);
        let ypos: Vec<i32> = ypos.iter().map(|y| (y - base_sel).min(0x400)).collect();
        let mut base = (bot - top) / 2 + top;
        if let Some(&first) = ypos.first()
            && top < base + first
        {
            base = top - first;
        }
        if let Some(&last) = ypos.last()
            && base + last < bot
        {
            base = bot - last;
        }
        let (mut above, mut below) = (false, false);
        for (k, it) in items.iter().enumerate() {
            let y0 = base + ypos[k];
            let fade = if y0 < top - dim {
                above = true;
                0x100
            } else if y0 < top {
                above = true;
                ((top - y0) * 0x1fe / dim.max(1)).min(0xff)
            } else if bot + dim < y0 {
                below = true;
                0x100
            } else if bot < y0 {
                below = true;
                ((y0 - bot) * 0x1fe / dim.max(1)).min(0xff)
            } else {
                0
            };
            if 0x100 <= fade {
                continue;
            }
            let a = (0x80 - (fade as u32 >> 1)) as u8;
            let icon = if it.icon.is_empty() { None } else { self.lookup(&it.icon) };
            if icon.is_some() {
                let mut b = self.make_pub(icon, lx + 0x14, y0, -1, -1);
                b.rgba[3] = a;
                out.push(b);
            }
            let chosen = k == sel;
            if 0 < it.price {
                let s = it.price.to_string();
                if chosen {
                    out.extend(self.glow_text(0.5, rx - 0x40, y0 + 0xc, &s, frames as u32));
                } else if let Some(m) = self.message_pub(0.5, 0.5, rx - 0x40, y0 + 0xc, font, 0, a, 0.25, 0, &s) {
                    out.extend(self.glyphs(&m));
                }
            }
            let mut y = if icon.is_some() { y0 + 0x20 } else { y0 + 0xc };
            let scale = it.word_40 * 0.5;
            let fh = self.font_height(font, scale);
            for line in it.text.split('\n').filter(|l| !l.is_empty()) {
                if chosen {
                    out.extend(self.glow_text(scale, -cx, y, line, frames as u32));
                } else if let Some(m) = self.message_pub(scale, scale, -cx, y, font, 0, a, 0.25, 0, line) {
                    out.extend(self.glyphs(&m));
                }
                y += fh;
            }
        }
        if above {
            out.push(self.make_pub(self.lookup("MORE_UP"), cx - 0x20, top - 0x28, -1, -1));
        }
        if below {
            out.push(self.make_pub(self.lookup("MORE_DOWN"), cx - 0x20, bot + 0x38, -1, -1));
        }
        out
    }
}
