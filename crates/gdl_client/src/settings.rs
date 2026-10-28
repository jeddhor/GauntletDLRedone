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
    /// Frames queued for presentation (1..3; wgpu's desired maximum frame
    /// latency). 3 absorbs a late frame (smoother, about one frame more
    /// latency); M20 soak: no present interval over 26 ms with 3, up to
    /// 44 ms with 2.
    pub frame_queue: u32,
    /// A window wider than the PS2 frame (640:448) shows a wider 3D view
    /// (Hor+; the HUD stays centred). Off: the original picture, bordered.
    pub widescreen: bool,
    /// Multisample anti-aliasing samples (1, 2 or 4; 4 is Bevy's camera
    /// default, which every native render used before 2026-10-28; the GS
    /// has none, so the baseline is 1).
    pub msaa: u32,
    pub fullscreen: bool,
    /// Soften lightning and other ambient flashes (accessibility).
    pub reduce_flashes: bool,
    /// A texture replacement directory (texture_pack: PNG files named by
    /// texture hash; absolute, or relative to the working directory).
    /// Read at startup.
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
            frame_queue: 3,
            widescreen: false,
            msaa: 4,
            fullscreen: false,
            reduce_flashes: false,
            texture_pack: None,
            show_fps: false,
            keys: Default::default(),
        }
    }
}

impl Settings {
    /// The original presentation: no blending, the frame's aspect, no
    /// anti-aliasing, no flash softening, no replacements (fidelity
    /// comparisons; GDL_BASELINE=1). Window, sync and controls are kept.
    pub fn baseline() -> Self {
        Self::default().with_baseline_presentation()
    }

    /// These settings with the original presentation (see `baseline`).
    pub fn with_baseline_presentation(&self) -> Self {
        Self { interpolate: false, widescreen: false, msaa: 1, reduce_flashes: false, texture_pack: None, ..self.clone() }
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
        self.msaa = match self.msaa {
            0 | 1 => 1,
            2 | 3 => 2,
            _ => 4,
        };
        self.frame_queue = self.frame_queue.clamp(1, 3);
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
        let mut s: Settings = serde_json::from_str(r#"{"fps_cap": 7, "msaa": 3, "interpolate": false}"#).unwrap();
        s.sanitise();
        assert_eq!((s.fps_cap, s.msaa, s.interpolate, s.vsync), (30, 2, false, true));
    }

    #[test]
    fn baseline_is_the_original_presentation() {
        let b = Settings::baseline();
        assert!(!b.interpolate && !b.widescreen && b.msaa == 1 && b.texture_pack.is_none());
    }
}
