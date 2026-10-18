//! Lookouts (waypoints): AddLocatorInstList 0x150ac8 builds one 0x80-byte
//! record per kind-8/10 locator (at most 0x14): +0x30 position, +0x70 next
//! lookout index (locator +2), +0x72 parameter (locator +1). The arrow
//! matrix (+0x00) and copies (+0x40/+0x50) are presentation. Consumers:
//! FindClosestWaypoint 0x14fda8, NextWaypoint, FindLookoutParam.
use super::Game;
use crate::numeric::{add_guarded_rz as add, mul_rz as mul, sqrt_rn, sub_guarded_rz as sub};
use gdl_formats::world::Locator;

pub const MAX_LOOKOUTS: usize = 0x14;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lookout {
    pub position: [f32; 3],
    /// Locator +0x10..+0x18 (the record's matrix is built from it).
    pub rotation: [f32; 3],
    pub next: i16,
    pub param: i16,
}

/// The kind-8/10 locators in list order (Errorf past 0x14).
pub fn build(locators: &[Locator]) -> Vec<Lookout> {
    locators
        .iter()
        .filter(|l| l.kind == 8 || l.kind == 10)
        .take(MAX_LOOKOUTS)
        .map(|l| Lookout { position: l.position, rotation: l.rotation, next: l.id, param: i16::from(l.byte_01) })
        .collect()
}

impl Game {
    /// FindClosestWaypoint(range, position, any): the nearest lookout within
    /// `range` (strictly closer), only lookouts with a next link unless `any`;
    /// a lookout whose next link is itself is skipped.
    pub(crate) fn find_closest_waypoint(&self, range: f32, p: [f32; 3], any: bool) -> super::R<Option<usize>> {
        let mut best = range;
        let mut pick = None;
        for (k, l) in self.lookouts.iter().enumerate() {
            if !any && (l.next < 0 || l.next as usize == k) {
                continue;
            }
            let d = [sub(l.position[0], p[0])?, sub(l.position[1], p[1])?, sub(l.position[2], p[2])?];
            let s = sqrt_rn(add(add(mul(d[0], d[0])?, mul(d[1], d[1])?)?, mul(d[2], d[2])?)?)?;
            if s < best {
                best = s;
                pick = Some(k);
            }
        }
        Ok(pick)
    }

    /// LevelEnemyDesc 0x1626c8: the ENMY +0x10 name of the first EnemyType
    /// slot of `kind` that has one, else world_data +4.
    pub(crate) fn level_enemy_desc_name(&self, kind: i32) -> super::R<String> {
        Ok(self.enemy_slot_names.iter().find(|(k, n)| *k == kind && !n.is_empty()).map_or_else(|| self.world_dir.clone(), |(_, n)| n.clone()))
    }

    /// NextWaypoint: the linked lookout, None for no link or a self-link.
    pub(crate) fn next_waypoint(&self, k: usize) -> Option<usize> {
        let n = usize::try_from(self.lookouts.get(k)?.next).ok()?;
        (n != k).then_some(n)
    }
}

/// add_arrow(2, 0, 1, rotation, position, record) 0x14ff70: the lookout record's
/// matrix — CreateYPRMatrix(FixAngle(−x), FixAngle(y + π), z) with the
/// locator position as translation (no FloorPos for arrow type 2).
pub fn lookout_matrix(l: &Lookout) -> Result<[[f32; 4]; 4], super::Unsupported> {
    let a = [
        super::enemy_ai::fix_angle(-l.rotation[0])?,
        super::enemy_ai::fix_angle(crate::numeric::add_guarded_rz(l.rotation[1], std::f32::consts::PI).map_err(|e| super::Unsupported(format!("{e:?}")))?)?,
        l.rotation[2],
    ];
    let mut m = super::enemy_ai::create_ypr_matrix(a)?;
    m[3] = [l.position[0], l.position[1], l.position[2], 1.0];
    Ok(m)
}
