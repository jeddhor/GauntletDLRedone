//! Skin effects: SetSkinFX 0x1800c8, ProcessSkinFX 0x180010, DoProcessSkinFX
//! 0x17ff18 and PlayerProcessSkinFX 0x132fd0. A skin is six floats in the
//! owner's record (enemy +0x1f8, player +0x7b4): end, current, rate,
//! texture, repeats, ambient add. While end > 0 the tree draws its
//! alternate texture (MBTreeSetAltTex −4 replace, −3 for the chrome
//! textures) at frame (int)current and ambient add (int)ambient
//! (MBTreeSetAmbientAdd). The texture word holds a [`SkinTex`] code instead
//! of the original MB texture handle (a pointer-class word); presentation
//! resolves it (InitEffects 0x179e20 names).
use super::record::Record;
use super::{Game, R};
use crate::numeric::{add_guarded_rz as add, mul_rz as mul};

/// InitEffects' skin textures: TextureWhite AAAWHITE (POWERUPS),
/// TextureSilver CHROMESILVER / TextureGold CHROMEGOLD (WEAPONS model),
/// TextureExit and TextureDeath[0..5] = WEAPONS texmods DEATHBLOOD
/// (gore_level 0; DEATHMAGIC otherwise), DEATHFIRE, DEATHELEC, DEATHLIGHT,
/// DEATHACID; AltTexDeath DEATHALT (enemy package), GolemTexDeath
/// DEATHGOLEM (golem package).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkinTex {
    White = 1,
    Silver = 2,
    Gold = 3,
    Exit = 4,
    DeathBlood = 10,
    DeathFire = 11,
    DeathElec = 12,
    DeathLight = 13,
    DeathAcid = 14,
    AltDeath = 20,
    GolemDeath = 21,
}

impl SkinTex {
    pub fn from_code(c: i32) -> Option<Self> {
        use SkinTex::*;
        [White, Silver, Gold, Exit, DeathBlood, DeathFire, DeathElec, DeathLight, DeathAcid, AltDeath, GolemDeath].into_iter().find(|t| *t as i32 == c)
    }
    /// TextureDeath[k] (0x244e90; index 5 reads TextureExit, DEATHLIGHT).
    pub fn death(k: u32) -> Self {
        use SkinTex::*;
        match k {
            0 => DeathBlood,
            1 => DeathFire,
            2 => DeathElec,
            3 => DeathLight,
            4 => DeathAcid,
            _ => Exit,
        }
    }
    /// (package kind, name): 0 = WEAPONS texmod frame base, 1 = POWERUPS
    /// texture, 2 = WEAPONS model texture, 3 = enemy package texmod.
    pub fn source(self) -> (u8, &'static str) {
        use SkinTex::*;
        match self {
            White => (1, "AAAWHITE"),
            Silver => (2, "CHROMESILVER"),
            Gold => (2, "CHROMEGOLD"),
            Exit | DeathLight => (0, "DEATHLIGHT"),
            DeathBlood => (0, "DEATHBLOOD"),
            DeathFire => (0, "DEATHFIRE"),
            DeathElec => (0, "DEATHELEC"),
            DeathAcid => (0, "DEATHACID"),
            AltDeath => (3, "DEATHALT"),
            GolemDeath => (3, "DEATHGOLEM"),
        }
    }
}

/// A tree's MBTreeSetAltTex / MBTreeSetAmbientAdd state (presentation).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkinView {
    /// −1 none, −4 replace every texture, −3 chrome replace.
    pub mode: i16,
    pub tex: i32,
    pub frame: i32,
    pub ambient: i32,
}

impl Default for SkinView {
    fn default() -> Self {
        Self { mode: -1, tex: 0, frame: 0, ambient: 0 }
    }
}

/// SetSkinFX(rate, skin, texture, end, repeats).
pub fn set_skin_fx(r: &mut Record, o: usize, rate: f32, tex: SkinTex, end: i32, repeats: f32) {
    r.set_f32(o, end as f32);
    r.set_f32(o + 4, -rate);
    r.set_f32(o + 8, rate);
    r.set_f32(o + 12, tex as i32 as f32);
    r.set_f32(o + 16, repeats);
    r.set_f32(o + 20, 999.0);
}

/// ProcessSkinFX + DoProcessSkinFX: true while the skin ran this call.
pub fn process_skin_fx(r: &mut Record, o: usize, vb: i32, view: &mut SkinView) -> R<bool> {
    if r.f32_(o) <= 0.0 {
        return Ok(false);
    }
    let cur = add(mul((vb >> 1) as f32, r.f32_(o + 8))?, r.f32_(o + 4))?;
    r.set_f32(o + 4, cur);
    if r.f32_(o) <= cur {
        if r.f32_(o + 16) == 0.0 {
            r.set_f32(o, 0.0);
        } else {
            r.set_f32(o + 4, 0.0);
            r.set_f32(o + 16, (r.f32_(o + 16) as i32 - 1) as f32);
        }
    }
    if 0.0 < r.f32_(o) {
        let tex = r.f32_(o + 12) as i32;
        let chrome = tex == SkinTex::Silver as i32 || tex == SkinTex::Gold as i32;
        *view = SkinView { mode: if chrome { -3 } else { -4 }, tex, frame: r.f32_(o + 4) as i32, ambient: r.f32_(o + 20) as i32 };
    } else {
        *view = SkinView::default();
    }
    Ok(true)
}

impl Game {
    /// PlayerProcessSkinFX: +0x7d4 (the special's flash, × 0.9 per call,
    /// 0 below 0.05) gives the ambient add (× 255); +0x932 = skin active;
    /// an ended alternate skin is cleared.
    pub(crate) fn player_process_skin_fx(&mut self, p: usize) -> R<()> {
        let vb = self.globals.vb_elapsed;
        let r = &mut self.players[p];
        let mut f = r.f32_(0x7d4);
        let view = &mut self.skin_views.players[p];
        if f == 0.0 {
            f = r.f32_(0x7b4);
        } else {
            let d = mul(f, 0.9)?;
            r.set_f32(0x7d4, d);
            if d.abs() < 0.05 {
                r.set_f32(0x7d4, 0.0);
                f = 0.0;
            }
            view.ambient = mul(f, 255.0)? as i32;
            f = r.f32_(0x7b4);
        }
        if 0.0 < f {
            r.set_u16(0x932, 1);
        } else if r.u16_(0x932) != 0 {
            r.set_u16(0x932, 0);
        }
        let mut v = *view;
        let ran = process_skin_fx(r, 0x7b4, vb, &mut v)?;
        if ran {
            *view = v;
        } else if view.mode < -1 {
            *view = SkinView::default();
        }
        Ok(())
    }

    /// ProcessSkinFX on enemy `i` (do_enemies): true while the skin runs.
    pub(crate) fn enemy_process_skin_fx(&mut self, i: usize) -> R<bool> {
        let vb = self.globals.vb_elapsed;
        let mut v = self.skin_views.enemies.get(i).copied().unwrap_or_default();
        let ran = process_skin_fx(&mut self.enemies[i], 0x1f8, vb, &mut v)?;
        if ran && let Some(s) = self.skin_views.enemies.get_mut(i) {
            *s = v;
        }
        Ok(ran)
    }
}

/// Per-tree skin views of the enemies (25 slots) and players.
#[derive(Clone, Debug)]
pub struct SkinViews {
    pub enemies: Vec<SkinView>,
    pub players: [SkinView; 4],
}

impl Default for SkinViews {
    fn default() -> Self {
        Self { enemies: vec![SkinView::default(); 25], players: [SkinView::default(); 4] }
    }
}
