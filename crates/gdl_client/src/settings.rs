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
    /// latency, default 2). 3 may absorb a late frame at about one frame
    /// more latency; the M20 30-minute soaks showed no benefit (2 present
    /// intervals over 33.4 ms with 2, 8 with 3; performance.md).
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
    /// The button prompts' style (gdl_client::glyphs): "auto" follows the
    /// last controller used (Sony pads and no controller: PlayStation;
    /// others: Xbox letters), or "playstation" / "xbox".
    pub button_prompts: Prompts,
    /// Keyboard bindings for player 1 (action name → key names); empty: the
    /// built-in defaults.
    pub keys: std::collections::BTreeMap<String, Vec<String>>,
    /// Graphics (post-1.0 enhancements; research/enhancements.md
    /// "Graphics"). "sharp": every base texture at the GPU's level of
    /// detail with 16× anisotropic filtering over a full mip chain
    /// (texfilter); "original": the GS's filtering.
    pub texture_filter: TextureFilter,
    /// Contrast-adaptive sharpening of the 3D view, percent (0: off).
    pub sharpen: u32,
    /// Full colour precision: no 16-bit frame with its dither, no integer
    /// truncation at the GS multiplies.
    pub full_colour: bool,
    /// Lightmaps sampled with a cubic B-spline instead of bilinearly.
    pub smooth_lightmaps: bool,
    /// Up to 32 point lights, chosen among those reaching the view (the
    /// original presentation: the 8 nearest the camera).
    pub more_lights: bool,
    /// Flickering point lights at the levels' fires (world particle
    /// emitters with a flame texture).
    pub torch_lights: bool,
    /// Shadow-mapped character shadows along the level's light (players,
    /// enemies, items and critters cast; the blob shadows are hidden).
    pub shadows: bool,
    /// Screen-space ambient occlusion on the opaque parts. It needs the
    /// 3D view without MSAA: anti-aliasing is then SMAA (`msaa_in_use`).
    pub ambient_occlusion: bool,
    /// Bloom around glows brighter than full white (stacked additive
    /// layers).
    pub bloom: bool,
    /// Anti-aliasing by SMAA (a post-process) instead of MSAA.
    pub smaa: bool,
    /// The generated HD textures (hd::generate: 4× Real-ESRGAN; effects
    /// and skies keep their originals). Turning it on generates the pack
    /// once; not part of the presets (Original turns it off).
    pub hd_textures: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            interpolate: true,
            vsync: true,
            fps_cap: 0,
            frame_queue: 2,
            widescreen: false,
            msaa: 4,
            fullscreen: false,
            reduce_flashes: false,
            texture_pack: None,
            show_fps: false,
            button_prompts: Prompts::Auto,
            keys: Default::default(),
            // The Enhanced preset (the user's choice after the checkpoint A
            // playtest, 2026-10-30).
            texture_filter: TextureFilter::Sharp,
            sharpen: 40,
            full_colour: true,
            smooth_lightmaps: true,
            // Checkpoint C (lighting): part of Enhanced until the user's
            // playtest decides.
            more_lights: true,
            torch_lights: true,
            shadows: true,
            ambient_occlusion: true,
            bloom: true,
            smaa: false,
            hd_textures: false,
        }
    }
}

/// Settings::texture_filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TextureFilter {
    #[default]
    Original,
    Sharp,
}

/// The graphics fields taken together (the overlay's preset row).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Graphics {
    /// The original presentation's graphics: every enhancement off.
    Original,
    /// Every enhancement at its recommended value.
    Enhanced,
    /// Anything else.
    Custom,
}

/// Settings::button_prompts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Prompts {
    #[default]
    Auto,
    PlayStation,
    Xbox,
}

impl Settings {
    /// The original presentation: no blending, the frame's aspect, no
    /// anti-aliasing, no flash softening, no replacements, the PlayStation
    /// prompts (fidelity comparisons; GDL_BASELINE=1).
    pub fn baseline() -> Self {
        Self { button_prompts: Prompts::PlayStation, ..Self::default().with_baseline_presentation() }
    }

    /// These settings with the original presentation (see `baseline`);
    /// window, sync, controls and the button prompts are kept.
    pub fn with_baseline_presentation(&self) -> Self {
        Self { interpolate: false, widescreen: false, msaa: 1, smaa: false, reduce_flashes: false, texture_pack: None, ..self.with_graphics(Graphics::Original) }
    }

    /// The MSAA sample count the window cameras use: 1 with SMAA or with
    /// ambient occlusion (Bevy's SSAO reads a single-sampled prepass).
    pub fn msaa_in_use(&self) -> u32 {
        if self.smaa || self.ambient_occlusion { 1 } else { self.msaa }
    }

    /// Whether the 3D view is anti-aliased by SMAA: chosen, or standing in
    /// for MSAA under ambient occlusion.
    pub fn smaa_in_use(&self) -> bool {
        self.smaa || (self.ambient_occlusion && 1 < self.msaa)
    }

    /// These settings with the graphics fields of a preset (Custom: unchanged).
    pub fn with_graphics(&self, g: Graphics) -> Self {
        match g {
            Graphics::Original => Self {
                texture_filter: TextureFilter::Original,
                sharpen: 0,
                full_colour: false,
                smooth_lightmaps: false,
                more_lights: false,
                torch_lights: false,
                shadows: false,
                ambient_occlusion: false,
                bloom: false,
                hd_textures: false,
                ..self.clone()
            },
            Graphics::Enhanced => Self {
                texture_filter: TextureFilter::Sharp,
                sharpen: 40,
                full_colour: true,
                smooth_lightmaps: true,
                more_lights: true,
                torch_lights: true,
                shadows: true,
                ambient_occlusion: true,
                bloom: true,
                ..self.clone()
            },
            Graphics::Custom => self.clone(),
        }
    }

    /// Which preset the graphics fields match.
    pub fn graphics(&self) -> Graphics {
        for g in [Graphics::Original, Graphics::Enhanced] {
            if self.with_graphics(g) == *self {
                return g;
            }
        }
        Graphics::Custom
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
        self.sharpen = self.sharpen.min(100);
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
        assert!(!b.interpolate && !b.widescreen && b.msaa == 1 && b.texture_pack.is_none() && b.button_prompts == Prompts::PlayStation);
    }

    #[test]
    fn graphics_presets() {
        let s = Settings::default();
        assert_eq!(s.graphics(), Graphics::Enhanced);
        // A settings file from before the graphics fields gets them too.
        assert_eq!(serde_json::from_str::<Settings>(r#"{"msaa": 2}"#).unwrap().graphics(), Graphics::Enhanced);
        let e = s.with_graphics(Graphics::Original).with_graphics(Graphics::Enhanced);
        assert_eq!(e.graphics(), Graphics::Enhanced);
        assert!(e.texture_filter == TextureFilter::Sharp && e.full_colour && e.smooth_lightmaps && 0 < e.sharpen && e.more_lights && e.torch_lights && e.shadows && e.ambient_occlusion && e.bloom);
        let c = Settings { sharpen: 80, ..e.clone() };
        assert_eq!(c.graphics(), Graphics::Custom);
        // The original presentation turns every graphics enhancement off.
        assert_eq!(c.with_baseline_presentation().graphics(), Graphics::Original);
        assert_eq!(Settings::baseline().graphics(), Graphics::Original);
        let b = Settings::baseline();
        assert!(!b.more_lights && !b.torch_lights && !b.shadows && !b.ambient_occlusion && !b.bloom && !b.smaa && !b.hd_textures);
        // HD textures: off by default, outside the presets (Enhanced keeps
        // them as they are), off with Original.
        assert!(!Settings::default().hd_textures);
        let hd = Settings { hd_textures: true, ..Settings::default() };
        assert_eq!(hd.graphics(), Graphics::Enhanced);
        assert!(hd.with_graphics(Graphics::Enhanced).hd_textures && !hd.with_graphics(Graphics::Original).hd_textures);
        let mut h: Settings = serde_json::from_str(r#"{"texture_filter": "sharp", "sharpen": 250}"#).unwrap();
        h.sanitise();
        assert_eq!((h.texture_filter, h.sharpen), (TextureFilter::Sharp, 100));
    }

    #[test]
    fn ambient_occlusion_swaps_msaa_for_smaa() {
        let s = Settings { msaa: 4, ambient_occlusion: false, smaa: false, ..Settings::default() };
        assert_eq!((s.msaa_in_use(), s.smaa_in_use()), (4, false));
        let ao = Settings { ambient_occlusion: true, ..s.clone() };
        assert_eq!((ao.msaa_in_use(), ao.smaa_in_use()), (1, true));
        // No anti-aliasing chosen: none under ambient occlusion either.
        let off = Settings { msaa: 1, ..ao.clone() };
        assert_eq!((off.msaa_in_use(), off.smaa_in_use()), (1, false));
        let smaa = Settings { smaa: true, ..s };
        assert_eq!((smaa.msaa_in_use(), smaa.smaa_in_use()), (1, true));
        assert_eq!((Settings::baseline().msaa_in_use(), Settings::baseline().smaa_in_use()), (1, false));
    }

    #[test]
    fn button_prompts_are_stored_by_name() {
        let s: Settings = serde_json::from_str(r#"{"button_prompts": "xbox"}"#).unwrap();
        assert_eq!(s.button_prompts, Prompts::Xbox);
        assert_eq!(Settings::default().button_prompts, Prompts::Auto);
        assert!(serde_json::to_string(&Settings::default()).unwrap().contains(r#""button_prompts":"auto""#));
    }
}
