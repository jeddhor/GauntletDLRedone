//! NATIVE ENHANCEMENT (HD textures): which textures keep their originals.
//! The user's choice at Checkpoint B (2026-10-31): Real-ESRGAN for every
//! category except glows, fire and magic effects and skies and backdrops.
//! Decided from the draws that use a texture (hd::census), not from its
//! name (research/enhancements.md "HD textures"):
//!
//! effect — any use that is
//!   - a particle system's sprite texture;
//!   - an additive draw (MB flag 0x800000);
//!   - a blended texture (alpha class 2) drawn without depth writes (MB
//!     flag 0x80) or camera-facing (MB flags 0xf000000);
//!   - a node of an effect tree: EffectInfo's names, the effect trees the
//!     game binds by name (EFFECT_TREES), trees whose label holds "FX"
//!     (the bosses' attack effects) and the players' SFX packages' power
//!     and combo trees (not their thrown weapons or familiars);
//!   - a texture modifier frame of an effect texture (by the target's
//!     label: frames are drawn wherever their target is), or a blended
//!     frame whose target no traced draw uses (found by name elsewhere:
//!     the animated fire, spit, steam and glow sets — a few animated items
//!     go with them);
//!
//! sky — not an effect, not animated, drawn only by world nodes, never with
//! a lightmap and never camera-facing, on a node of radius at least 60, or
//! at least 40 on at most two nodes (skies, horizons, backdrop panoramas,
//! fog and haze layers).
use super::census::{Entry, UseKind};
use std::collections::{HashMap, HashSet};

/// Effect trees InitEffects / InitCustomEffect bind by name besides
/// EffectInfo (gdl_core::game::effects).
pub const EFFECT_TREES: [&str; 20] = [
    "SUICIDEEXP", "GENFX1", "GENFX2", "GENFX3", "TREEHIT", "TREEDIE", "ATK1FX", "ATK2FX", "DEATHFX1", "DEATHFX2", "DEATH_ARC", "DEATH_EXP",
    "GENHIT", "GENDIE", "LEGENDFX", "LEGENDFX2", "LEGENDPRJ", "LEGENDHLD", "SAFEREXP", "WORLD_EXP",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Keep {
    Particle,
    Additive,
    SoftNoDepth,
    SoftFacing,
    EffectTree,
    EffectFrame,
    Sky,
}

/// Textures that keep their originals, by census name.
pub fn classify(entries: &[Entry], effect_info: &[String]) -> HashMap<String, Keep> {
    let effect_trees: HashSet<String> = effect_info.iter().map(|s| s.to_uppercase()).chain(EFFECT_TREES.iter().map(|s| s.to_string())).collect();
    let is_effect_tree = |package: &str, who: &str| {
        let tree = who.split(':').next().unwrap_or("").to_uppercase();
        effect_trees.contains(&tree)
            || tree.contains("FX")
            || (package.starts_with("PLAYERS/") && package.rsplit('/').next().is_some_and(|p| p.starts_with("SFX")) && !tree.contains("THROW") && !tree.starts_with("FAMILIAR"))
    };
    let mut keep = HashMap::new();
    for e in entries {
        let mut why = None;
        for u in &e.uses {
            let drawn = matches!(u.kind, UseKind::World | UseKind::Tree) && u.flags & 3 == 0;
            let w = match u.kind {
                UseKind::Particle => Some(Keep::Particle),
                _ if drawn && u.flags & 0x80_0000 != 0 => Some(Keep::Additive),
                _ if drawn && e.class == 2 && u.flags & 0x80 != 0 => Some(Keep::SoftNoDepth),
                _ if drawn && e.class == 2 && u.flags & 0xf00_0000 != 0 => Some(Keep::SoftFacing),
                UseKind::Tree if drawn && is_effect_tree(&u.package, &u.who) => Some(Keep::EffectTree),
                _ => None,
            };
            if w.is_some() {
                why = w;
                break;
            }
        }
        if let Some(w) = why {
            keep.insert(e.name.clone(), w);
        }
    }
    // Frames of effect textures, by the target's label, to a fixed point.
    loop {
        let labels: HashSet<String> = entries.iter().filter(|e| keep.contains_key(&e.name)).map(|e| e.label.to_uppercase()).collect();
        let mut added = false;
        for e in entries {
            if !keep.contains_key(&e.name) && e.uses.iter().any(|u| u.kind == UseKind::Frame && labels.contains(&u.who)) {
                keep.insert(e.name.clone(), Keep::EffectFrame);
                added = true;
            }
        }
        if !added {
            break;
        }
    }
    for e in entries {
        if !keep.contains_key(&e.name) && e.class == 2 && !e.uses.is_empty() && e.uses.iter().all(|u| u.kind == UseKind::Frame) {
            keep.insert(e.name.clone(), Keep::EffectFrame);
        }
    }
    for e in entries {
        if keep.contains_key(&e.name) || e.uses.iter().any(|u| u.kind == UseKind::Frame) {
            continue;
        }
        let uses: Vec<_> = e.uses.iter().filter(|u| !(matches!(u.kind, UseKind::World | UseKind::Tree) && u.flags & 3 != 0)).collect();
        if uses.is_empty() || uses.iter().any(|u| u.kind != UseKind::World || u.lightmapped || u.flags & 0xf00_0000 != 0) {
            continue;
        }
        let radius = uses.iter().map(|u| u.radius).max().unwrap_or(0);
        let nodes: HashSet<(&str, &str)> = uses.iter().map(|u| (u.package.as_str(), u.who.as_str())).collect();
        if 60 <= radius || (40 <= radius && nodes.len() <= 2) {
            keep.insert(e.name.clone(), Keep::Sky);
        }
    }
    keep
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hd::census::Use;

    fn entry(name: &str, label: &str, class: u8, uses: Vec<Use>) -> Entry {
        Entry { name: name.into(), label: label.into(), width: 32, height: 32, class, uses: uses.into_iter().collect(), ..Default::default() }
    }

    fn u(kind: UseKind, flags: u32, package: &str, who: &str, lightmapped: bool, radius: i32) -> Use {
        Use { kind, flags, package: package.into(), who: who.into(), lightmapped, radius }
    }

    #[test]
    fn effects_and_skies_from_their_draws() {
        let w = |flags, lm, r| u(UseKind::World, flags, "LEVELS/LEVELX1", "X1#1", lm, r);
        let entries = vec![
            entry("add", "GLOWY", 0, vec![w(0x800000, false, 5)]),
            entry("soft", "SOFT", 2, vec![w(0x80, false, 5)]),
            entry("facing", "FLARE", 2, vec![w(0x1000000, false, 5)]),
            // An opaque camera-facing part (a chain sprite) is upscaled.
            entry("chain", "CHAIN", 1, vec![w(0x1000000, false, 5)]),
            entry("fx", "SPARKTEX", 0, vec![u(UseKind::Tree, 0, "WEAPONS", "SPARKS:NODE", false, 0)]),
            entry("boss", "BOSSTEX", 0, vec![u(UseKind::Tree, 0, "MONSTERS/SKORNE1", "BOSSATK1FX:NODE", false, 0)]),
            entry("power", "POWTEX", 0, vec![u(UseKind::Tree, 0, "PLAYERS/WIZ/SFXBLU", "WIZ_POWERA1:NODE", false, 0)]),
            entry("throw", "AXE", 0, vec![u(UseKind::Tree, 0, "PLAYERS/WIZ/SFXBLU", "BOW_THROW1:NODE", false, 0)]),
            entry("psys", "P_TORCH", 0, vec![u(UseKind::Particle, 0, "LEVELS/LEVELX1", "PSYSA", false, 0)]),
            entry("frame", "GLOWY01", 0, vec![u(UseKind::Frame, 0, "LEVELS/LEVELX1", "GLOWY", false, 0)]),
            entry("untraced", "FLAME03", 2, vec![u(UseKind::Frame, 0, "LEVELS/LEVELX1", "FLAME", false, 0)]),
            entry("water", "WATER03", 2, vec![u(UseKind::Frame, 0, "LEVELS/LEVELX1", "WATER", false, 0), w(0, false, 90)]),
            entry("sky", "SKYTEX", 0, vec![w(0x1000, false, 300)]),
            entry("backdrop", "PANEL", 0, vec![w(0x1000, false, 45)]),
            // Lightmapped, many small nodes, or also drawn by a tree: not a sky.
            entry("floor", "GRASS", 0, vec![w(0, true, 300)]),
            entry("posts", "POST", 0, vec![w(0, false, 45), u(UseKind::World, 0, "LEVELS/LEVELX1", "X1#2", false, 45), u(UseKind::World, 0, "LEVELS/LEVELX1", "X1#3", false, 45)]),
            entry("prop", "BARREL", 0, vec![w(0, false, 300), u(UseKind::Tree, 0, "ITEMS/LEVELX", "BAROBJ:NODE", false, 0)]),
        ];
        let k = classify(&entries, &["SPARKS".to_string()]);
        assert_eq!(k.get("add"), Some(&Keep::Additive));
        assert_eq!(k.get("soft"), Some(&Keep::SoftNoDepth));
        assert_eq!(k.get("facing"), Some(&Keep::SoftFacing));
        assert_eq!(k.get("chain"), None);
        assert_eq!(k.get("fx"), Some(&Keep::EffectTree));
        assert_eq!(k.get("boss"), Some(&Keep::EffectTree));
        assert_eq!(k.get("power"), Some(&Keep::EffectTree));
        assert_eq!(k.get("throw"), None);
        assert_eq!(k.get("psys"), Some(&Keep::Particle));
        assert_eq!(k.get("frame"), Some(&Keep::EffectFrame));
        assert_eq!(k.get("untraced"), Some(&Keep::EffectFrame));
        assert_eq!(k.get("water"), None, "animated surfaces are upscaled");
        assert_eq!(k.get("sky"), Some(&Keep::Sky));
        assert_eq!(k.get("backdrop"), Some(&Keep::Sky));
        assert_eq!(k.get("floor"), None);
        assert_eq!(k.get("posts"), None);
        assert_eq!(k.get("prop"), None);
    }
}
