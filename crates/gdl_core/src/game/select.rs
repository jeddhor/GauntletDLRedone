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
    Menu { title: Vec<String>, entries: Vec<String>, selected: usize },
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
                Column::Menu { title, entries, selected } => {
                    let mut y = 0x30;
                    for t in title {
                        text(self, &mut out, 0.8, 6, cx, y, t);
                        y += 0x1a;
                    }
                    let mut menu = def_menu.clone();
                    // setup_sel_menu case 8 (the file list): x = sel_xpos + 8,
                    // y 0x46, scale 0.6.
                    menu.x = xp + 8;
                    menu.y = 0x46;
                    menu.scale = f32::from_bits(0x3f19_999a);
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
