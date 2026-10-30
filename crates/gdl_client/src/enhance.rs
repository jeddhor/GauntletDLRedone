//! The native settings overlay (M19, NATIVE: not an original menu; F10):
//! the enhancement settings and player 1's keys, changed in place and
//! stored in settings.json. The game waits while it is open. This module
//! is the overlay's state and text; gdl_play draws it and feeds it keys.
use crate::controls::{ACTIONS, Bindings};
use crate::settings::{Graphics, Prompts, Settings, TextureFilter};
use bevy::input::keyboard::KeyCode;

/// The overlay's input, already decoded from the keyboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Input {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Back,
    /// Backspace/Delete: an action's default keys.
    Reset,
    /// Any key, while a binding waits for one.
    Key(KeyCode),
}

/// What an input did.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    None,
    /// Settings or bindings changed (store them).
    Changed,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Widescreen,
    Blend,
    Vsync,
    FpsCap,
    FrameQueue,
    Fullscreen,
    Msaa,
    ReduceFlashes,
    ShowFps,
    Prompts,
    Graphics,
    Controls,
    Original,
    Close,
}

const MAIN: [Row; 14] = [Row::Widescreen, Row::Blend, Row::Vsync, Row::FpsCap, Row::FrameQueue, Row::Fullscreen, Row::Msaa, Row::ReduceFlashes, Row::ShowFps, Row::Prompts, Row::Graphics, Row::Controls, Row::Original, Row::Close];
const FPS_CAPS: [u32; 7] = [0, 30, 60, 75, 120, 144, 240];

/// The graphics page's rows (post-1.0 enhancements).
#[derive(Clone, Copy, Debug, PartialEq)]
enum GRow {
    Preset,
    TextureFilter,
    Sharpen,
    FullColour,
    SmoothLightmaps,
    Back,
}

const GRAPHICS: [GRow; 6] = [GRow::Preset, GRow::TextureFilter, GRow::Sharpen, GRow::FullColour, GRow::SmoothLightmaps, GRow::Back];
const SHARPEN: [u32; 6] = [0, 20, 40, 60, 80, 100];

#[derive(Default, bevy::prelude::Resource)]
pub struct Overlay {
    pub open: bool,
    /// Closed with keys still down: the game ignores the keyboard until
    /// every key is released (Enter would otherwise press Start).
    pub held: bool,
    controls: bool,
    /// The graphics page.
    graphics: bool,
    cursor: usize,
    /// The action waiting for a key.
    capture: Option<usize>,
}

fn on(v: bool) -> &'static str {
    if v { "On" } else { "Off" }
}

impl Overlay {
    pub fn toggle(&mut self) -> Effect {
        if self.open {
            *self = Self { held: true, ..Self::default() };
            Effect::Closed
        } else {
            *self = Self { open: true, ..Self::default() };
            Effect::None
        }
    }

    /// Whether the next key goes to a binding.
    pub fn capturing(&self) -> bool {
        self.capture.is_some()
    }

    /// The overlay's lines, the cursor's marked with "> ".
    pub fn lines(&self, s: &Settings, b: &Bindings) -> Vec<String> {
        let mark = |k: usize| if k == self.cursor { "> " } else { "  " };
        let mut out = Vec::new();
        if self.controls {
            out.push("PLAYER 1 KEYS (native)".to_string());
            out.push(String::new());
            for (k, (a, keys)) in ACTIONS.iter().zip(&b.keys).enumerate() {
                let names = if self.capture == Some(k) {
                    "press a key... (Escape / pad B: cancel)".to_string()
                } else {
                    keys.iter().map(|&k| crate::controls::key_name(k)).collect::<Vec<_>>().join(", ")
                };
                out.push(format!("{}{:<18} {}", mark(k), a.label, names));
            }
            out.push(format!("{}Back", mark(ACTIONS.len())));
            out.push(String::new());
            out.push("Enter/pad A: bind  Backspace: default  Escape/pad B: back".to_string());
            return out;
        }
        if self.graphics {
            out.push("GRAPHICS (native enhancements)".to_string());
            out.push(String::new());
            for (k, row) in GRAPHICS.iter().enumerate() {
                let text = match row {
                    GRow::Preset => format!(
                        "Preset               {}",
                        match s.graphics() {
                            Graphics::Original => "Original",
                            Graphics::Enhanced => "Enhanced",
                            Graphics::Custom => "Custom",
                        }
                    ),
                    GRow::TextureFilter => format!(
                        "Texture filtering    {}",
                        match s.texture_filter {
                            TextureFilter::Original => "Original",
                            TextureFilter::Sharp => "Sharp",
                        }
                    ),
                    GRow::Sharpen => format!("Sharpening           {}", if s.sharpen == 0 { "Off".to_string() } else { format!("{} %", s.sharpen) }),
                    GRow::FullColour => format!("Colour precision     {}", if s.full_colour { "Full" } else { "Original 16-bit" }),
                    GRow::SmoothLightmaps => format!("Smooth lightmaps     {}", on(s.smooth_lightmaps)),
                    GRow::Back => "Back".to_string(),
                };
                out.push(format!("{}{text}", mark(k)));
            }
            out.push(String::new());
            out.push("Arrows / d-pad: choose   Enter / pad A / Left / Right: change".to_string());
            out.push("Escape / pad B: back".to_string());
            return out;
        }
        out.push("SETTINGS (native enhancements)".to_string());
        out.push(String::new());
        for (k, row) in MAIN.iter().enumerate() {
            let text = match row {
                Row::Widescreen => format!("Widescreen view      {}", on(s.widescreen)),
                Row::Blend => format!("Motion blending      {}", on(s.interpolate)),
                Row::Vsync => format!("VSync                {}", on(s.vsync)),
                Row::FpsCap => format!("Frame cap            {}", if s.fps_cap == 0 { "Off".to_string() } else { s.fps_cap.to_string() }),
                Row::FrameQueue => format!(
                    "Frames queued        {} ({})",
                    s.frame_queue,
                    match s.frame_queue {
                        1 => "lowest latency",
                        2 => "default",
                        _ => "more buffering",
                    }
                ),
                Row::Fullscreen => format!("Fullscreen (F11)     {}", on(s.fullscreen)),
                Row::Msaa => format!("Anti-aliasing        {}", if s.msaa <= 1 { "Off".to_string() } else { format!("{}x MSAA", s.msaa) }),
                Row::ReduceFlashes => format!("Reduce flashes       {}", on(s.reduce_flashes)),
                Row::ShowFps => format!("Frame rate (F3)      {}", on(s.show_fps)),
                Row::Prompts => format!(
                    "Button prompts       {}",
                    match s.button_prompts {
                        Prompts::Auto => "Automatic",
                        Prompts::PlayStation => "PlayStation",
                        Prompts::Xbox => "Xbox",
                    }
                ),
                Row::Graphics => "Graphics...".to_string(),
                Row::Controls => "Player 1 keys...".to_string(),
                Row::Original => "Original presentation".to_string(),
                Row::Close => "Close".to_string(),
            };
            out.push(format!("{}{text}", mark(k)));
        }
        out.push(String::new());
        out.push(format!("Texture pack: {}", s.texture_pack.as_ref().map_or("none".to_string(), |p| p.display().to_string())));
        out.push("Arrows / d-pad: choose   Enter / pad A / Left / Right: change".to_string());
        out.push("Escape / pad B / F10: close".to_string());
        out
    }

    /// Apply one input.
    pub fn input(&mut self, i: Input, s: &mut Settings, b: &mut Bindings) -> Effect {
        if !self.open {
            return Effect::None;
        }
        if let Some(a) = self.capture {
            return match i {
                Input::Back => {
                    self.capture = None;
                    Effect::None
                }
                Input::Key(k) if crate::controls::BINDABLE.contains(&k) => {
                    self.capture = None;
                    b.bind(a, k);
                    s.keys = b.to_settings();
                    Effect::Changed
                }
                _ => Effect::None,
            };
        }
        let rows = if self.controls {
            ACTIONS.len() + 1
        } else if self.graphics {
            GRAPHICS.len()
        } else {
            MAIN.len()
        };
        match i {
            Input::Up => {
                self.cursor = (self.cursor + rows - 1) % rows;
                Effect::None
            }
            Input::Down => {
                self.cursor = (self.cursor + 1) % rows;
                Effect::None
            }
            Input::Back if self.controls => {
                *self = Self { open: true, cursor: MAIN.iter().position(|r| *r == Row::Controls).unwrap_or(0), ..Self::default() };
                Effect::None
            }
            Input::Back if self.graphics => {
                *self = Self { open: true, cursor: MAIN.iter().position(|r| *r == Row::Graphics).unwrap_or(0), ..Self::default() };
                Effect::None
            }
            Input::Back => self.toggle(),
            Input::Accept | Input::Left | Input::Right if self.graphics => {
                let back = i == Input::Left;
                match GRAPHICS[self.cursor] {
                    GRow::Preset => {
                        let next = if s.graphics() == Graphics::Enhanced { Graphics::Original } else { Graphics::Enhanced };
                        *s = s.with_graphics(next);
                    }
                    GRow::TextureFilter => {
                        s.texture_filter = if s.texture_filter == TextureFilter::Sharp { TextureFilter::Original } else { TextureFilter::Sharp };
                    }
                    GRow::Sharpen => {
                        let at = SHARPEN.iter().position(|&v| v == s.sharpen).unwrap_or(0);
                        s.sharpen = SHARPEN[if back { (at + SHARPEN.len() - 1) % SHARPEN.len() } else { (at + 1) % SHARPEN.len() }];
                    }
                    GRow::FullColour => s.full_colour = !s.full_colour,
                    GRow::SmoothLightmaps => s.smooth_lightmaps = !s.smooth_lightmaps,
                    GRow::Back => {
                        if i != Input::Accept {
                            return Effect::None;
                        }
                        return self.input(Input::Back, s, b);
                    }
                }
                Effect::Changed
            }
            _ if self.controls => {
                if self.cursor == ACTIONS.len() {
                    if i == Input::Accept {
                        return self.input(Input::Back, s, b);
                    }
                    return Effect::None;
                }
                match i {
                    Input::Accept => {
                        self.capture = Some(self.cursor);
                        Effect::None
                    }
                    Input::Reset => {
                        b.keys[self.cursor] = ACTIONS[self.cursor].defaults.to_vec();
                        // Defaults may be taken by another action meanwhile.
                        for &k in ACTIONS[self.cursor].defaults {
                            for (j, keys) in b.keys.iter_mut().enumerate() {
                                if j != self.cursor {
                                    keys.retain(|&x| x != k);
                                }
                            }
                        }
                        s.keys = b.to_settings();
                        Effect::Changed
                    }
                    _ => Effect::None,
                }
            }
            Input::Accept | Input::Left | Input::Right => {
                let back = i == Input::Left;
                match MAIN[self.cursor] {
                    Row::Widescreen => s.widescreen = !s.widescreen,
                    Row::Blend => s.interpolate = !s.interpolate,
                    Row::Vsync => s.vsync = !s.vsync,
                    Row::Fullscreen => s.fullscreen = !s.fullscreen,
                    Row::ReduceFlashes => s.reduce_flashes = !s.reduce_flashes,
                    Row::ShowFps => s.show_fps = !s.show_fps,
                    Row::Prompts => {
                        let order = [Prompts::Auto, Prompts::PlayStation, Prompts::Xbox];
                        let at = order.iter().position(|&p| p == s.button_prompts).unwrap_or(0);
                        s.button_prompts = order[if back { (at + 2) % 3 } else { (at + 1) % 3 }];
                    }
                    Row::FpsCap => {
                        let at = FPS_CAPS.iter().position(|&c| c == s.fps_cap).unwrap_or(0);
                        s.fps_cap = FPS_CAPS[if back { (at + FPS_CAPS.len() - 1) % FPS_CAPS.len() } else { (at + 1) % FPS_CAPS.len() }];
                    }
                    Row::FrameQueue => {
                        s.frame_queue = if back { (s.frame_queue + 1) % 3 + 1 } else { s.frame_queue % 3 + 1 };
                    }
                    Row::Msaa => {
                        let order = [1, 2, 4];
                        let at = order.iter().position(|&m| m == s.msaa).unwrap_or(0);
                        s.msaa = order[if back { (at + 2) % 3 } else { (at + 1) % 3 }];
                    }
                    Row::Graphics => {
                        if i != Input::Accept {
                            return Effect::None;
                        }
                        *self = Self { open: true, graphics: true, ..Self::default() };
                        return Effect::None;
                    }
                    Row::Controls => {
                        if i != Input::Accept {
                            return Effect::None;
                        }
                        *self = Self { open: true, controls: true, ..Self::default() };
                        return Effect::None;
                    }
                    Row::Original => {
                        if i != Input::Accept {
                            return Effect::None;
                        }
                        *s = s.with_baseline_presentation();
                    }
                    Row::Close => {
                        if i != Input::Accept {
                            return Effect::None;
                        }
                        return self.toggle();
                    }
                }
                Effect::Changed
            }
            _ => Effect::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> (Overlay, Settings, Bindings) {
        let mut o = Overlay::default();
        o.toggle();
        (o, Settings::default(), Bindings::default())
    }

    #[test]
    fn rows_toggle_and_cycle() {
        let (mut o, mut s, mut b) = open();
        assert_eq!(o.input(Input::Accept, &mut s, &mut b), Effect::Changed);
        assert!(s.widescreen);
        for _ in 0..3 {
            o.input(Input::Down, &mut s, &mut b);
        }
        o.input(Input::Right, &mut s, &mut b);
        assert_eq!(s.fps_cap, 30);
        o.input(Input::Left, &mut s, &mut b);
        o.input(Input::Left, &mut s, &mut b);
        assert_eq!(s.fps_cap, 240);
        assert_eq!(o.input(Input::Back, &mut s, &mut b), Effect::Closed);
        assert!(!o.open);
    }

    #[test]
    fn binding_a_key_stores_it() {
        let (mut o, mut s, mut b) = open();
        while o.lines(&s, &b).iter().all(|l| !l.starts_with("> Player 1 keys")) {
            o.input(Input::Down, &mut s, &mut b);
        }
        o.input(Input::Accept, &mut s, &mut b);
        let cross = ACTIONS.iter().position(|a| a.name == "cross").unwrap();
        for _ in 0..cross {
            o.input(Input::Down, &mut s, &mut b);
        }
        o.input(Input::Accept, &mut s, &mut b);
        assert!(o.capturing());
        assert_eq!(o.input(Input::Key(KeyCode::KeyN), &mut s, &mut b), Effect::Changed);
        assert_eq!(b.of("cross"), &[KeyCode::KeyN]);
        assert_eq!(s.keys.get("cross"), Some(&vec!["KeyN".to_string()]));
        assert_eq!(o.input(Input::Reset, &mut s, &mut b), Effect::Changed);
        assert!(s.keys.is_empty());
    }

    #[test]
    fn button_prompts_cycle() {
        let (mut o, mut s, mut b) = open();
        while o.lines(&s, &b).iter().all(|l| !l.starts_with("> Button prompts")) {
            o.input(Input::Down, &mut s, &mut b);
        }
        assert_eq!(o.input(Input::Right, &mut s, &mut b), Effect::Changed);
        assert_eq!(s.button_prompts, Prompts::PlayStation);
        o.input(Input::Accept, &mut s, &mut b);
        assert_eq!(s.button_prompts, Prompts::Xbox);
        o.input(Input::Left, &mut s, &mut b);
        o.input(Input::Left, &mut s, &mut b);
        assert_eq!(s.button_prompts, Prompts::Auto);
        assert!(o.lines(&s, &b).iter().any(|l| l == "> Button prompts       Automatic"));
    }

    #[test]
    fn graphics_page_presets_and_rows() {
        let (mut o, mut s, mut b) = open();
        while o.lines(&s, &b).iter().all(|l| !l.starts_with("> Graphics...")) {
            o.input(Input::Down, &mut s, &mut b);
        }
        o.input(Input::Accept, &mut s, &mut b);
        assert!(o.lines(&s, &b).iter().any(|l| l == "> Preset               Original"));
        assert_eq!(o.input(Input::Accept, &mut s, &mut b), Effect::Changed);
        assert_eq!(s.graphics(), Graphics::Enhanced);
        // Sharpening steps, then the preset reads Custom.
        o.input(Input::Down, &mut s, &mut b);
        o.input(Input::Down, &mut s, &mut b);
        o.input(Input::Right, &mut s, &mut b);
        assert_eq!(s.sharpen, 60);
        assert_eq!(s.graphics(), Graphics::Custom);
        o.input(Input::Up, &mut s, &mut b);
        o.input(Input::Up, &mut s, &mut b);
        o.input(Input::Accept, &mut s, &mut b);
        assert_eq!(s.graphics(), Graphics::Enhanced);
        o.input(Input::Accept, &mut s, &mut b);
        assert_eq!(s.graphics(), Graphics::Original);
        // Back returns to the main page at "Graphics...".
        o.input(Input::Back, &mut s, &mut b);
        assert!(o.open && o.lines(&s, &b).iter().any(|l| l == "> Graphics..."));
    }

    #[test]
    fn original_presentation_keeps_window_and_keys() {
        let (mut o, mut s, mut b) = open();
        s.widescreen = true;
        s.fullscreen = true;
        s.msaa = 4;
        s.button_prompts = Prompts::Xbox;
        while o.lines(&s, &b).iter().all(|l| !l.starts_with("> Original")) {
            o.input(Input::Down, &mut s, &mut b);
        }
        o.input(Input::Accept, &mut s, &mut b);
        assert!(!s.widescreen && s.fullscreen && s.msaa == 1 && !s.interpolate && s.button_prompts == Prompts::Xbox);
    }
}
