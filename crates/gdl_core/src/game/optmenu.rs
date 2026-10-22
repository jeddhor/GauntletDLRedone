//! Option menus (start_optmenu_nostack 0x10acc8, show_optmenu 0x10b5d8)
//! as HUD blits: the 0xe8-byte menu records (title_menu, tower_menu,
//! game_menu, the options menus, hint_menu) with their 0x24-byte entries,
//! drawn in font32 (OPTMENU_FONT) over the SCROLL_A background with the
//! LOGO_BURN emblem, FONT32_PARCH glyphs, the glowing selection, the
//! FONT32GAR intro burn and the button row. Menu logic stays with the
//! caller; the ICON_ARROW selector is a powerup animation tree drawn in 3D
//! by the client at [`Hud::optmenu_icon_target`].
use super::hud::{Arg, Blit, Hud};
use gdl_formats::elf::Elf;

/// A menu entry (0x24 bytes): label, action id (+4), extra gap (+8), value
/// string (+0xc), colour (+0x10, flag 0x200), +0x1c and +0x20 (< 0:
/// disabled, drawn at least half transparent).
#[derive(Clone, Debug)]
pub struct MenuEntry {
    pub label: String,
    pub action: i32,
    pub gap: i32,
    pub value: Option<String>,
    pub colour: u32,
    pub mark: i32,
    pub state: i32,
}

/// A menu record (0xe8 bytes; offsets in the field docs).
#[derive(Clone, Debug)]
pub struct OptMenu {
    /// +0 optmenu_active value, +4 title scale, +8 title.
    pub kind: i32,
    pub title_scale: f32,
    pub title: Option<String>,
    /// +0xc item x (< 0: centred on −x), +0x10 item y (−1: centred), +0x14
    /// width, +0x18 height (< 1: measured).
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub entries: Vec<MenuEntry>,
    /// +0x20 flags: 1 Back, 2 Select/Accept, 4 Change, 8 Center, 0x10
    /// "Player n", 0x20 fade, 0x40 FONT32_PARCH glyphs, 0x80 FONT32GAR intro,
    /// 0x100 "Accept", 0x200 entry colours. +0x24 button-row y.
    pub flags: u32,
    pub msg_y: i32,
    /// +0x28/+0x2c/+0x30 colour triples: off, on, highlight.
    pub rgb: [[i32; 3]; 3],
    /// +0x34 item scale, +0x38 icon tree name, +0x3c icon scale, +0x40
    /// icon x offset.
    pub scale: f32,
    pub icon: Option<String>,
    pub icon_scale: f32,
    pub icon_x: i32,
    /// +0x44 background texture and rectangle +0x48..+0x54 (< 0: derived).
    pub back: Option<String>,
    pub back_rect: [i32; 4],
    /// +0x58 burn texture and rectangle +0x5c..+0x68.
    pub burn: Option<String>,
    pub burn_rect: [i32; 4],
}

/// The OPTMENU_* tuning words and colours.
#[derive(Clone, Debug)]
pub struct OptStyle {
    pub fade: i32,
    pub fade_hold: i32,
    pub font: usize,
    pub margin: [i32; 2],
    pub margin_title: i32,
    pub margin_player: i32,
    pub scroll_z: f32,
    pub finish_frames: i32,
    pub choice_twice: bool,
    pub rgb_text: u32,
    pub msg_scale: f32,
    pub player_scale: f32,
    pub hilite_scale: f32,
    /// OPTMENU_ICON_TIME (vblanks of the icon's glide and half turn) and
    /// optmenu_icon_z (MBScreenToWorld depth).
    pub icon_time: i32,
    pub icon_z: f32,
}

fn cstr(b: &[u8]) -> String {
    b.iter().take_while(|c| **c != 0).map(|c| char::from(*c)).collect()
}

impl OptStyle {
    pub fn from_elf(elf: &Elf) -> Result<Self, Box<dyn std::error::Error>> {
        let w = |n: &str| -> Result<u32, Box<dyn std::error::Error>> { Ok(u32::from_le_bytes(elf.object(n)?[..4].try_into()?)) };
        let rgb = |n: &str| -> Result<u32, Box<dyn std::error::Error>> {
            let b = elf.object(n)?;
            let c = |k: usize| i32::from_le_bytes(b[4 * k..4 * k + 4].try_into().unwrap()).clamp(0, 255) as u32;
            Ok(c(0) << 16 | c(1) << 8 | c(2))
        };
        Ok(Self {
            fade: w("OPTMENU_FADE")? as i32,
            fade_hold: w("OPTMENU_FADE_HOLD")? as i32,
            font: w("OPTMENU_FONT")? as usize,
            margin: [w("OPTMENU_MARGIN_X")? as i32, w("OPTMENU_MARGIN_Y")? as i32],
            margin_title: w("OPTMENU_MARGIN_TITLE")? as i32,
            margin_player: w("OPTMENU_MARGIN_PLAYER")? as i32,
            scroll_z: f32::from_bits(w("OPTMENU_SCROLLZ")?),
            finish_frames: w("OPTMENU_FINISH_FRAMES")? as i32,
            choice_twice: w("optmenu_choice_twice")? != 0,
            rgb_text: rgb("optmenu_rgb_text")?,
            msg_scale: f32::from_bits(w("msg_scale")?),
            player_scale: f32::from_bits(w("optplyr_scale")?),
            hilite_scale: f32::from_bits(w("hilite_scale")?),
            icon_time: w("OPTMENU_ICON_TIME")? as i32,
            icon_z: f32::from_bits(w("optmenu_icon_z")?),
        })
    }
}

/// The Control Style screen (DoOptions 0xd/0xe case 0x21):
/// start_controlsmenu's three blits (opt_ctlblit, 0x18 each: texture name,
/// x, y, w, h), do_controlsmenu's sixteen labels (ctl_label_pos, 0xc each:
/// alignment 0 left / 1 centre / 2 right of 256 + x, y centre) of
/// GetStringListMsg(3, control_style) (ENGLISH.ROM CONTROLS_DESC) in
/// OPTCTL_FONT at OPTCTL_SCALE with stringtext_dy OPTCTL_DY and colour
/// optmenu_rgb_ctls; the entry label is opt_controls_desc[style].
#[derive(Clone, Debug)]
pub struct ControlsMenu {
    pub blits: Vec<(String, i32, i32, i32, i32)>,
    pub labels: Vec<(i32, i32, i32)>,
    pub font: i32,
    pub scale: f32,
    pub dy: i32,
    pub rgb: u32,
    pub styles: Vec<String>,
}

impl ControlsMenu {
    pub fn from_elf(elf: &Elf) -> Result<Self, Box<dyn std::error::Error>> {
        let i = |b: &[u8], o: usize| i32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let tb = elf.object("opt_ctlblit")?;
        let mut blits = Vec::new();
        for k in 0..3 {
            let r = &tb[0x18 * k..0x18 * k + 0x18];
            let name = cstr(elf.read(i(r, 0) as u32, 32)?);
            blits.push((name, i(r, 4), i(r, 8), i(r, 0xc), i(r, 0x10)));
        }
        let lb = elf.object("ctl_label_pos")?;
        let labels = (0..16).map(|k| (i(lb, 0xc * k), i(lb, 0xc * k + 4), i(lb, 0xc * k + 8))).collect();
        let w = |n: &str| -> Result<u32, Box<dyn std::error::Error>> { Ok(u32::from_le_bytes(elf.object(n)?[..4].try_into()?)) };
        let c = elf.object("optmenu_rgb_ctls")?;
        let k = |j: usize| i(c, 4 * j).clamp(0, 255) as u32;
        let d = elf.object("opt_controls_desc")?;
        let styles = (0..4).map(|j| cstr(&d[0x10 * j..0x10 * j + 0x10])).collect();
        Ok(Self { blits, labels, font: w("OPTCTL_FONT")? as i32, scale: f32::from_bits(w("OPTCTL_SCALE")?), dy: w("OPTCTL_DY")? as i32, rgb: k(0) << 16 | k(1) << 8 | k(2), styles })
    }
}

impl Hud {
    /// do_controlsmenu's blits for `style` (0..4).
    pub fn controls_blits(&self, ctl: &ControlsMenu, style: usize) -> Vec<Blit> {
        let mut out = Vec::new();
        for (name, x, y, w, h) in &ctl.blits {
            out.push(self.make_pub(self.lookup(name), *x, *y, *w, *h));
        }
        // GetStringListMsg(3, style): list 3's entry `style`.
        let Some(e) = self.text.as_ref().and_then(|t| t.lists.get(3)).and_then(|(_, v)| v.get(style)).copied() else { return out };
        let e = e as i32;
        for (k, &(mode, lx, ly)) in ctl.labels.iter().enumerate() {
            let k = k as i32;
            let w = self.string_text_width(ctl.scale, e, k);
            let h = self.string_text_height(ctl.scale, e, k, ctl.dy);
            let x = match mode {
                1 => -(lx + 0x100),
                2 => (w / 2 - 0x100) - lx,
                _ => -(w / 2 + 0x100 + lx),
            };
            out.extend(self.string_text_blits(x, ly - h / 2, ctl.font, ctl.rgb, e, k, ctl.dy, ctl.scale));
        }
        out
    }
}

impl OptMenu {
    /// The menu record at a symbol (title_menu, tower_menu, …) or address.
    pub fn from_elf(elf: &Elf, at: u32) -> Result<Self, Box<dyn std::error::Error>> {
        let b = elf.read(at, 0xe8)?;
        let w = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let i = |o: usize| w(o) as i32;
        let s = |o: usize| -> Option<String> { (w(o) != 0).then(|| elf.read(w(o), 48).ok().map(cstr)).flatten() };
        let triple = |o: usize| -> [i32; 3] {
            elf.read(w(o), 12).map(|t| [0, 1, 2].map(|k| i32::from_le_bytes(t[4 * k..4 * k + 4].try_into().unwrap()))).unwrap_or([255; 3])
        };
        let mut entries = Vec::new();
        let base = w(0x1c);
        for k in 0..64 {
            let e = elf.read(base + 0x24 * k, 0x24)?;
            let ew = |o: usize| u32::from_le_bytes(e[o..o + 4].try_into().unwrap());
            if ew(0) == 0 {
                break;
            }
            let text = |p: u32| (p != 0).then(|| elf.read(p, 48).ok().map(cstr)).flatten();
            entries.push(MenuEntry {
                label: text(ew(0)).unwrap_or_default(),
                action: ew(4) as i32,
                gap: ew(8) as i32,
                value: text(ew(0xc)),
                colour: ew(0x10),
                mark: ew(0x1c) as i32,
                state: ew(0x20) as i32,
            });
        }
        Ok(Self {
            kind: i(0),
            title_scale: f32::from_bits(w(4)),
            title: s(8),
            x: i(0xc),
            y: i(0x10),
            width: i(0x14),
            height: i(0x18),
            entries,
            flags: w(0x20),
            msg_y: i(0x24),
            rgb: [triple(0x28), triple(0x2c), triple(0x30)],
            scale: f32::from_bits(w(0x34)),
            icon: s(0x38),
            icon_scale: f32::from_bits(w(0x3c)),
            icon_x: i(0x40),
            back: s(0x44),
            back_rect: [i(0x48), i(0x4c), i(0x50), i(0x54)],
            burn: s(0x58),
            burn_rect: [i(0x5c), i(0x60), i(0x64), i(0x68)],
        })
    }
}

/// A shown menu: the selected entry (+0x74), the frame count since it
/// opened (+0x7c), the closing count (+0x70) and the player (+0x78).
#[derive(Clone, Debug, Default)]
pub struct MenuState {
    pub selected: usize,
    pub frames: i32,
    pub finishing: i32,
    pub player: Option<usize>,
    /// do_audiomenu's volume sliders: (entry, value 0..=255).
    pub sliders: Vec<(usize, u8)>,
}

fn rgb(c: [i32; 3]) -> u32 {
    let k = |v: i32| v.clamp(0, 255) as u32;
    k(c[0]) << 16 | k(c[1]) << 8 | k(c[2])
}

impl Hud {
    /// position_audioslider 0x10ca38 (OPTAUDIO_VOL_WIDTH 264, slider_dxl −52,
    /// slider_dxr −24): markers, the empty and filled bars and the knob;
    /// the unselected slider is translucent.
    fn slider_blits(&self, x: i32, y: i32, v: u8, dim: bool) -> Vec<Blit> {
        let (w, dxl, dxr) = (264, -52, -24);
        let fill = (w * i32::from(v) / 0xff).max(1);
        let d = i32::from(dim);
        let alpha = |a: i32| (0x80 - (a as u32 >> 1)) as u8;
        let mut out = Vec::new();
        for (name, bx, by, bw, bh, a) in [
            ("empty_bar", x, y + 0xb, w, 0x20, d * 0x96),
            ("pink_bar", x, y + 0xf, fill, 0x20, d * 0xa0),
            ("marker_left", x + dxl, y, -1, -1, d * 100),
            ("marker_right", x + w + dxr, y, -1, -1, d * 100),
            ("slider", x + fill - 0x14, y + 2, -1, -1, d * 100),
        ] {
            let mut b = self.make_pub(self.lookup(name), bx, by, bw, bh);
            b.rgba[3] = alpha(a);
            b.z = 1.0;
            out.push(b);
        }
        out
    }

    /// show_optmenu's icon point: x = the item x (centred menus: −width/2 −
    /// x) + the icon x offset (+0x40), y = the selected entry's drawn y
    /// (entry +0x14) + FontHeight / 2; None without an icon tree (+0x38).
    pub fn optmenu_icon_target(&self, style: &OptStyle, menu: &OptMenu, state: &MenuState) -> Option<(i32, i32)> {
        menu.icon.as_ref()?;
        let fh = self.font_height(style.font, menu.scale);
        let height = if menu.height < 1 { menu.entries.iter().map(|e| fh + e.gap).sum() } else { menu.height };
        let width = if menu.width < 1 { menu.entries.iter().map(|e| self.string_width(style.font, menu.scale, e.label.as_bytes())).max().unwrap_or(0) } else { menu.width };
        let mut y = match menu.y {
            -1 => 0xc0 - height / 2,
            y if y < 0 => -(height / 2) - y,
            y => y,
        };
        for e in menu.entries.iter().take(state.selected) {
            y += fh + e.gap;
        }
        let x = if menu.x < 0 { -(width / 2) - menu.x } else { menu.x };
        Some((x + menu.icon_x, y + fh / 2))
    }

    /// show_optmenu for `menu` in `state` (entries may be relabelled by the
    /// caller, e.g. for value columns).
    pub fn optmenu_blits(&self, style: &OptStyle, menu: &OptMenu, state: &MenuState) -> Vec<Blit> {
        let mut out = Vec::new();
        let font = style.font;
        let fh = self.font_height(font, menu.scale);
        // start_optmenu_nostack: measured height/width, item y, background.
        let height = if menu.height < 1 { menu.entries.iter().map(|e| fh + e.gap).sum() } else { menu.height };
        let width = if menu.width < 1 { menu.entries.iter().map(|e| self.string_width(font, menu.scale, e.label.as_bytes())).max().unwrap_or(0) } else { menu.width };
        let mut y = match menu.y {
            -1 => 0xc0 - height / 2,
            y if y < 0 => -(height / 2) - y,
            y => y,
        };
        let [mut bx, mut by, mut bw, mut bh] = menu.back_rect;
        if bw < 0 {
            bw = style.margin[0] * 2 + width;
        }
        if bh < 0 {
            bh = style.margin[1] * 2 + height;
        }
        if bx < 0 {
            bx = 0x100 - bw / 2;
        }
        if by < 0 {
            by = 0xc0 - bh / 2;
        }
        let back = menu.back.as_deref().and_then(|n| self.lookup(n));
        if back.is_some() {
            let mut b = self.make_pub(back, bx, by, bw, bh);
            b.z = style.scroll_z - 1.0;
            out.push(b);
        }
        if let Some(t) = menu.burn.as_deref().and_then(|n| self.lookup(n)) {
            let [x, yy, w, h] = menu.burn_rect;
            let t = self.offset_pub(t, (state.frames >> 3) % 5).unwrap_or(t);
            // MBNewBlit's z 1.0 (start_optmenu_nostack's second MBBlitSetZ
            // re-targets the background, +0xdc).
            out.push(self.make_pub(Some(t), x, yy, w, h));
        }
        // Fade (flag 0x20): font alpha 0..0xff (0 opaque).
        let ff = style.finish_frames.max(1);
        let fade = if menu.flags & 0x20 == 0 {
            0
        } else if state.finishing != 0 {
            state.finishing * 0xff / ff
        } else if state.frames < ff {
            (ff - state.frames) * 0xff / ff
        } else {
            0
        };
        let parch = if menu.flags & 0x40 != 0 { self.lookup("FONT32_PARCH") } else { None };
        // Flag 0x80: FONT32GAR0..5 burn the text in (and out).
        let gar = if menu.flags & 0x80 != 0 { self.lookup("FONT32GAR0") } else { None };
        let gar = gar.and_then(|g| {
            let i = if state.finishing == 0 { (state.frames - 10) / 2 } else { 6 - state.finishing * 6 / ff };
            if i < 0 {
                Some(g)
            } else if i <= 5 {
                self.offset_pub(g, i)
            } else {
                None
            }
        });
        let text_rgb = style.rgb_text;
        let font_z = self.font_z_pub();
        let push = |hud: &Hud, out: &mut Vec<Blit>, scale: f32, x: i32, yy: i32, colour: u32, alpha: i32, flags: u32, tex: Option<super::hud::TexRef>, text: &str| {
            if let Some(mut m) = hud.message_pub(scale, scale, x, yy, font, colour, (0x80 - (alpha.clamp(0, 0xff) as u32 >> 1)) as u8, font_z, flags, text) {
                m.tex = tex;
                out.extend(hud.glyphs(&m));
            }
        };
        if let (Some(title), true) = (&menu.title, back.is_some()) {
            let ty = by + style.margin_title;
            let tx = -(bw / 2 + bx);
            push(self, &mut out, menu.title_scale, tx, ty, text_rgb, 0, 0, parch, title);
        }
        let parch_items = if menu.flags & 0x200 != 0 { None } else { parch };
        for (k, e) in menu.entries.iter().enumerate() {
            let alpha = if e.state < 0 && fade < 0x80 { 0x80 } else { fade };
            let x = menu.x;
            let selected = k == state.selected && 0 <= e.state;
            if selected {
                // The pulse over OPTMENU_FADE / FADE_HOLD.
                let period = style.fade * 2 + style.fade_hold;
                let mut i = state.frames % period.max(1);
                if style.fade * 2 < i {
                    i = 0;
                } else if style.fade < i {
                    i = style.fade * 2 - i;
                }
                if style.choice_twice {
                    let a = (fade + 0x7f - ((i * 0xff + style.fade - 1) / style.fade.max(1)) / 2).min(0xff);
                    let glow = gar.or(self.lookup("FONT32_GLOW"));
                    push(self, &mut out, menu.scale, x, y, rgb(menu.rgb[2]), a, 0x4000, glow, &e.label);
                    push(self, &mut out, menu.scale, x, y, rgb(menu.rgb[1]), fade, 0, gar, &e.label);
                } else {
                    let c: [i32; 3] = [0, 1, 2].map(|j| (menu.rgb[2][j] + (i * (menu.rgb[1][j] - menu.rgb[2][j]) + style.fade - 1) / style.fade.max(1)).clamp(0, 255));
                    let s = menu.scale * ((i as f32 / style.fade as f32) * style.hilite_scale + 1.0);
                    push(self, &mut out, s, x, y, rgb(c), alpha, 0, gar.or(parch_items), &e.label);
                }
            } else {
                let colour = if menu.flags & 0x200 != 0 && (parch_items.is_none() || gar.is_some()) {
                    e.colour
                } else if parch_items.is_none() || gar.is_some() {
                    rgb(menu.rgb[0])
                } else {
                    0xffffff
                };
                push(self, &mut out, menu.scale, x, y, colour, alpha, 0, gar.or(parch_items), &e.label);
            }
            if e.mark == 1 {
                // show_optmenu: a marked entry (+0x1c = pass + 1) gets " ~"
                // (the font's check mark) after its label.
                let lx = if x < 0 { -x - self.string_width(font, menu.scale, e.label.as_bytes()) / 2 } else { x };
                let mx = lx + self.string_width(font, menu.scale, e.label.as_bytes());
                let c = if selected { rgb(menu.rgb[1]) } else if parch_items.is_none() || gar.is_some() { rgb(menu.rgb[0]) } else { 0xffffff };
                push(self, &mut out, menu.scale, mx, y, c, alpha, 0, gar.or(parch_items), " ~");
            }
            if let Some(v) = &e.value {
                // The value column after " ~ ".
                let lx = if x < 0 { -x - self.string_width(font, menu.scale, e.label.as_bytes()) / 2 } else { x };
                let vx = lx + self.string_width(font, menu.scale, e.label.as_bytes()) + self.string_width(font, menu.scale, b" ~ ");
                let c = if selected { rgb(menu.rgb[1]) } else { rgb(menu.rgb[0]) };
                push(self, &mut out, menu.scale, vx, y, c, fade, 0, gar.or(parch_items), v);
            }
            if let Some(&(_, v)) = state.sliders.iter().find(|(i, _)| *i == k) {
                out.extend(self.slider_blits(menu.x, y + fh, v, k != state.selected));
            }
            y += fh + e.gap;
        }
        if menu.flags & 0x10 != 0
            && let Some(p) = state.player
        {
            let txt = super::hud::format_c("Player %d", &[Arg::D(p as i32 + 1)]);
            push(self, &mut out, style.player_scale, -(p as i32 * 100 + 0x6a), by + style.margin_player, text_rgb, 0, 0, parch, &txt);
        }
        if menu.flags & 0xf != 0 && state.finishing == 0 {
            let n = (0..4).filter(|b| menu.flags >> b & 1 != 0).count() as i32;
            let step = 0x200 / (n + 1);
            let my = menu.msg_y;
            let bs = (style.msg_scale * 32.0) as i32;
            let mut x = 0;
            let row = |hud: &Hud, out: &mut Vec<Blit>, x: i32, label: &str, buttons: &[&str]| {
                push(hud, out, style.msg_scale, -x, my, text_rgb, 0, 0, parch, label);
                let w = hud.string_width(font, style.msg_scale, label.as_bytes());
                let mut bxx = x - w / 2 - bs - 4;
                for b in buttons.iter().rev() {
                    let mut bl = hud.make_pub(hud.lookup(b), bxx, my, bs, bs);
                    bl.z = 1.0;
                    out.push(bl);
                    bxx -= bs;
                }
            };
            if menu.flags & 1 != 0 {
                x = step;
                row(self, &mut out, x, "Back", &["BUTTON_TRI"]);
            }
            if menu.flags & 4 != 0 {
                x += step;
                row(self, &mut out, x, "Change", &["BUTTON_L", "BUTTON_R"]);
            }
            if menu.flags & 2 != 0 {
                x += step;
                row(self, &mut out, x, if menu.flags & 0x102 == 0x102 { "Accept" } else { "Select" }, &["BUTTON_X"]);
            }
            if menu.flags & 8 != 0 {
                x += step;
                row(self, &mut out, x, "Center", &["BUTTON_O"]);
            }
        }
        out
    }
}

impl Hud {
    /// init_titlescreen / do_titlescreen: TITLE00–03 at screenblitxy (0,0),
    /// (256,0), (0,256), (256,256) with titleblit_z −1000 (far back), the
    /// titleglow (GLOWCROP_00 + (frame >> 2) % 10 at (0xc0, 0), 0x80 × 0x80,
    /// fading in over 60 vblanks) while no menu or the title menu is up,
    /// and DrawGlowText(1.0, "Press Start") at y 0x140 before the menu.
    pub fn title_blits(&self, frame: i32, glow: bool, press_start: bool) -> Vec<Blit> {
        let mut out = Vec::new();
        for (k, (x, y)) in [(0, 0), (256, 0), (0, 256), (256, 256)].into_iter().enumerate() {
            let mut b = self.make_pub(self.lookup(&format!("TITLE{k:02}")), x, y, -1, -1);
            b.z = super::hud::blit_z(-1000.0);
            out.push(b);
        }
        if glow && let Some(t) = self.lookup("GLOWCROP_00") {
            let t = self.offset_pub(t, (frame >> 2) % 10).unwrap_or(t);
            let mut b = self.make_pub(Some(t), 0xc0, 0, 0x80, 0x80);
            if frame < 0x3c {
                b.rgba[3] = (0x80 - ((0xff - frame * 0xff / 0x3c) as u32 >> 1)) as u8;
            }
            out.push(b);
        }
        if press_start {
            out.extend(self.glow_text(1.0, -0x100, 0x140, "Press Start", frame as u32));
        }
        out
    }
}
