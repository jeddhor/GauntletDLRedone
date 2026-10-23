//! The X-ray powerup's chest view: do_see_thru 0x12c6a0 / end_see_thru
//! 0x12c580 (PlayerProcessPowerups while the powerup flags +0x138 hold 2;
//! end_see_thru once they no longer do) with ClosestChest 0x12c408 and
//! PointVisible 0x1d0908 (MBWorldSphereVisible3).
//!
//! The closest chest within 10 units of the player (item records with +0xd8
//! ≠ −1, a kind-2 definition, +0xe1 and +0xdc clear, contents of kind −1
//! (random), 4 (Death) or 1; the 3D distance from record +0x40 to player
//! +0x50, a nearer one replacing the best) that the camera sees (sphere of
//! twice the definition's +0xc at record +0x50) shows its contents: the
//! definition at record +0xf0, a random one resolved through RandItemIdx
//! (chest, choice count +4, no advance) and the choice list at +8 until a
//! non-random definition; the icon is DEATH_ICON for Death, the key ring
//! (KEYRING) for a key with +0x100 ≥ 2, else the definition's own tree
//! (InitItemInfoData's AtreeMatchAnyHeader of its name, +0x4c). A chest
//! another player already shows, or contents without a tree, show
//! nothing. A new chest or new contents plays AudioPlayerXray 0x15ee48
//! (AudioSFX(0x51, player +0x60, 0x7f, 0x3c)).
//!
//! Presentation (gdl_play): InitItemInfoData's per-player "SEETHRU" object
//! (camera-facing, MB flags 0x4200000) and the icon tree at 0.65 scale on a
//! node at the chest's matrix; the chest keeps drawing in place
//! (CopyMat4 of the identity 0x2a1ed0 under that node).
use super::{Game, R};

type V3 = [f32; 3];

/// What a player's X-ray view shows inside the chest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeeThruIcon {
    Death,
    Keys,
    /// The contents definition's own tree (by its name).
    Tree(usize),
}

/// seethru_chest / seethru_contents / seethru_atree for one player.
#[derive(Debug, Clone, Default)]
pub struct SeeThru {
    pub chest: Option<usize>,
    /// The contents definition (−1: none; 0 after InitItemInfoData).
    pub contents: i32,
    pub icon: Option<SeeThruIcon>,
}

impl Game {
    /// ClosestChest 0x12c408(player record).
    fn closest_chest(&self, p: usize) -> Option<usize> {
        let at = self.players[p].v3(0x50);
        let mut best: Option<(f32, usize)> = None;
        for (s, r) in self.items.records.iter().enumerate() {
            if !r.active() || r.i16_(0xd8) == -1 || r.u8_(0xe1) != 0 || r.u8_(0xdc) != 0 {
                continue;
            }
            let d = &self.items.definitions[r.definition()];
            if d.kind() != 2 {
                continue;
            }
            let c = r.i16_(0xf0);
            let Some(cd) = usize::try_from(c).ok().and_then(|c| self.items.definitions.get(c)) else { continue };
            if !matches!(cd.kind(), -1 | 4 | 1) {
                continue;
            }
            let q = r.v3(0x40);
            let dist = ((q[0] - at[0]).powi(2) + (q[1] - at[1]).powi(2) + (q[2] - at[2]).powi(2)).sqrt();
            if dist < best.map_or(10.0, |b| b.0) {
                best = Some((dist, s));
            }
        }
        best.map(|b| b.1)
    }

    /// do_see_thru 0x12c6a0 for player `p`.
    pub(crate) fn do_see_thru(&mut self, p: usize) -> R<()> {
        if self.items.records.is_empty() {
            return Ok(());
        }
        let mut chest = self.closest_chest(p);
        if let Some(c) = chest {
            let r = &self.items.records[c];
            let radius = self.items.definitions[r.definition()].f32_(0xc);
            let visible = match &self.camera {
                Some(cam) => cam.sphere_visible(radius + radius, r.v3(0x50))?,
                None => true,
            };
            if !visible {
                chest = None;
            }
        }
        let shown = chest.and_then(|c| {
            // The contents: random definitions resolved without advancing.
            let mut def = usize::try_from(self.items.records[c].i16_(0xf0)).ok()?;
            for _ in 0..16 {
                let d = self.items.definitions.get(def)?;
                if d.kind() != -1 {
                    break;
                }
                let k = self.items.rand_item_idx_peek(c, d.subtype() as u32) as usize;
                def = usize::try_from(d.i16_(8 + 2 * k)).ok()?;
            }
            let d = self.items.definitions.get(def)?;
            let icon = match (d.kind(), d.subtype()) {
                (4, _) => SeeThruIcon::Death,
                (1, 2) if 2 <= self.items.records[c].i16_(0x100) => SeeThruIcon::Keys,
                _ => {
                    let name = String::from_utf8_lossy(d.name()).into_owned();
                    if !self.item_anim_lib.contains_key(&name) {
                        return None;
                    }
                    SeeThruIcon::Tree(def)
                }
            };
            // Another player already shows this chest.
            if (0..4).any(|j| j != p && self.see_thru[j].chest == Some(c)) {
                return None;
            }
            Some((c, def as i32, icon))
        });
        let Some((c, def, icon)) = shown else {
            self.see_thru[p].contents = -1;
            self.end_see_thru(p);
            return Ok(());
        };
        let s = &mut self.see_thru[p];
        let mut changed = s.chest != Some(c);
        if changed {
            s.chest = Some(c);
        }
        if (changed && s.icon.is_none()) || s.contents != def {
            s.contents = def;
            s.icon = Some(icon);
            changed = true;
        }
        if changed {
            let at: V3 = self.players[p].v3(0x60);
            self.sfx(0x51, Some(at), 0x7f, 0x3c)?;
        }
        Ok(())
    }

    /// end_see_thru 0x12c580: the view hidden and the chest released.
    pub(crate) fn end_see_thru(&mut self, p: usize) {
        let s = &mut self.see_thru[p];
        s.icon = None;
        s.chest = None;
    }
}
