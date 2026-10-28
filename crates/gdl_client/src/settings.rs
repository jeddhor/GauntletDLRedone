//! M19 enhancement settings (NATIVE: not part of the original game). They
//! change presentation only — the simulation keeps the original 30 Hz
//! update and reads none of them — and `baseline()` is the original
//! presentation for fidelity comparisons. Stored as JSON beside the saves
//! (`settings.json`); missing fields take their defaults.
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Resource)]
#[serde(default)]
pub struct Settings {
    /// Blend the 3D scene between the last two simulation updates at the
    /// display rate (off: each update shown as it is, the original 30 Hz).
    pub interpolate: bool,
    /// Wait for the display's refresh (off: present as soon as ready).
    pub vsync: bool,
    /// Frames per second at most (0: no cap beyond vsync).
    pub fps_cap: u32,
    /// Fill the window's aspect ratio with a wider view (off: the original
    /// 4:3 picture, bordered).
    pub widescreen: bool,
    /// 3D render resolution relative to the window (supersampling above 1).
    pub render_scale: f32,
    /// Multisample anti-aliasing samples (1, 2 or 4).
    pub msaa: u32,
    pub fullscreen: bool,
    /// HUD size relative to the original (accessibility).
    pub hud_scale: f32,
    /// Soften full-screen flashes (lightning, ambient flashes): accessibility.
    pub reduce_flashes: bool,
    /// A texture replacement directory (PNG files named by texture hash).
    pub texture_pack: Option<PathBuf>,
    /// Frame-time overlay (diagnostics).
    pub show_fps: bool,
    /// Keyboard bindings for player 1 (action name → key names); empty: the
    /// built-in defaults.
    pub keys: std::collections::BTreeMap<String, Vec<String>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            interpolate: true,
            vsync: true,
            fps_cap: 0,
            widescreen: false,
            render_scale: 1.0,
            msaa: 1,
            fullscreen: false,
            hud_scale: 1.0,
            reduce_flashes: false,
            texture_pack: None,
            show_fps: false,
            keys: Default::default(),
        }
    }
}

impl Settings {
    /// The original presentation: no blending, 4:3, native resolution, no
    /// replacements (fidelity comparisons; GDL_BASELINE=1).
    pub fn baseline() -> Self {
        Self { interpolate: false, widescreen: false, render_scale: 1.0, msaa: 1, hud_scale: 1.0, reduce_flashes: false, texture_pack: None, ..Self::default() }
    }

    /// The settings file beside a save directory.
    pub fn path_for(saves: &Path) -> PathBuf {
        saves.join("settings.json")
    }

    /// The stored settings (defaults when absent or unreadable), then the
    /// environment: GDL_BASELINE=1 the original presentation, GDL_SETTINGS
    /// another file.
    pub fn load(saves: &Path) -> Self {
        if std::env::var_os("GDL_BASELINE").is_some_and(|v| v == "1") {
            return Self::baseline();
        }
        let path = std::env::var_os("GDL_SETTINGS").map(PathBuf::from).unwrap_or_else(|| Self::path_for(saves));
        let mut s: Self = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        s.sanitise();
        s
    }

    /// Values in range (a hand-edited file).
    pub fn sanitise(&mut self) {
        self.render_scale = if self.render_scale.is_finite() { self.render_scale.clamp(0.5, 3.0) } else { 1.0 };
        self.hud_scale = if self.hud_scale.is_finite() { self.hud_scale.clamp(0.75, 2.0) } else { 1.0 };
        self.msaa = match self.msaa {
            0 | 1 => 1,
            2 | 3 => 2,
            _ => 4,
        };
        if 0 < self.fps_cap {
            self.fps_cap = self.fps_cap.clamp(30, 1000);
        }
    }

    /// Write the settings (a new file then rename: an interrupted write
    /// keeps the previous one).
    pub fn save(&self, saves: &Path) -> std::io::Result<()> {
        let path = Self::path_for(saves);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_take_defaults_and_values_are_clamped() {
        let mut s: Settings = serde_json::from_str(r#"{"render_scale": 9.0, "msaa": 3, "interpolate": false}"#).unwrap();
        s.sanitise();
        assert_eq!((s.render_scale, s.msaa, s.interpolate, s.vsync), (3.0, 2, false, true));
    }

    #[test]
    fn baseline_is_the_original_presentation() {
        let b = Settings::baseline();
        assert!(!b.interpolate && !b.widescreen && b.render_scale == 1.0 && b.texture_pack.is_none());
    }
}
