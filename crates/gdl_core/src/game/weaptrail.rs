//! PlayerDoWeapTrail 0x14f428 (do_players, after every PlayerProcessSkinFX):
//! the weapon trail. While DoPlayerAction's trail actions hold +0x7dc
//! (cases 0x21, 0x25, 0x3c), each update copies the held weapon (+0x6b8,
//! load_player's MBNewObject of WEAP_HOLD under the right-hand node) into
//! one of eight slots (+0x7e0..+0x7fc) at the weapon's world matrix
//! (MBNewObject(object | +0x7cc << 16, matrix, no parent, 0x800)); every
//! copy fades by weaptrail_dalpha × vb_elapsed per update (MBTreeSetAlpha:
//! node +0x53 = ~alpha, flag 0x200 while alpha ≥ 1) and is removed at 0xff.
//! +0x7d8 is the highest live slot (−1 none). The new copy goes to the most
//! faded slot (an empty one counts as 0xff), or the slot after the top when
//! that is not above the top; none is made while the newest copy lies within
//! 0.1 of the weapon (squared distance < 0.01).
//!
//! The slots' node pointers are presentation: the copies live here with
//! their fade and world matrix (`Game::weap_trail`), drawn by the host.
//! The weapon's world matrix is the right-hand tree node's (the weapon node
//! has no local matrix) under the tree pose (teammove::player_tree_node_world;
//! HYPOTHESIS there: the blend fraction is not applied). Without a rig the
//! record matrix stands in (HYPOTHESIS).
use super::doplayers::record_matrix_of;
use super::{Game, R};

/// One weapon copy: its fade (MBTreeSetAlpha's value, 0 = opaque) and the
/// world matrix it was made at (PS2 rows, translation in row 3).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrailCopy {
    pub fade: i32,
    /// Node flag 0x200 (an alpha was set).
    pub alpha_set: bool,
    pub matrix: [[f32; 4]; 4],
}

impl Game {
    /// The held weapon's world matrix (GetWorldMat of +0x6b8).
    fn weapon_world(&self, p: usize) -> [[f32; 4]; 4] {
        let rhand = self.player_rigs[p].as_ref().and_then(|r| r.rhand);
        rhand.and_then(|n| self.player_tree_node_world(p, n).ok()).unwrap_or_else(|| record_matrix_of(&self.players[p]))
    }

    /// PlayerDoWeapTrail 0x14f428 for player `p`.
    pub(crate) fn player_do_weap_trail(&mut self, p: usize) -> R<()> {
        let mut top = self.players[p].i32_(0x7d8);
        if self.players[p].u32_(0x80) == 0 || self.globals.vb_elapsed == 0 {
            return Ok(());
        }
        let active = self.players[p].i32_(0x7dc) != 0;
        if top < 0 && !active {
            return Ok(());
        }
        let step = self.weaptrail_dalpha.wrapping_mul(self.globals.vb_elapsed);
        // The most faded slot (iVar12/iVar11) and the least faded (iStack_b0).
        let (mut most_i, mut most) = (0usize, 0i32);
        let (mut least_i, mut least) = (0usize, 0xffi32);
        if 0 <= top {
            for i in 0..=(top.min(7) as usize) {
                let fade = match &mut self.weap_trail[p][i] {
                    None => 0xff,
                    Some(c) => {
                        let mut f = if c.alpha_set { c.fade } else { 0 };
                        f = step.wrapping_add(f);
                        if 0xfe < f {
                            f = 0xff;
                            self.weap_trail[p][i] = None;
                        } else {
                            c.fade = f;
                            c.alpha_set = 1 <= f;
                        }
                        f
                    }
                };
                if most < fade {
                    most_i = i;
                    most = fade;
                }
                if fade < least {
                    least_i = i;
                    least = fade;
                }
            }
        }
        if 0 <= top {
            while 0 <= top && self.weap_trail[p][top as usize].is_none() {
                top -= 1;
            }
        }
        self.players[p].set_i32(0x7d8, top);
        if !active {
            return Ok(());
        }
        let mut slot = most_i as i32;
        if slot <= top && top < 7 {
            slot = top + 1;
        }
        let m = self.weapon_world(p);
        if least_i as i32 <= top
            && let Some(newest) = self.weap_trail[p][least_i]
        {
            let d: f32 = (0..3).map(|k| (newest.matrix[3][k] - m[3][k]) * (newest.matrix[3][k] - m[3][k])).sum();
            if d < 0.01 {
                return Ok(());
            }
        }
        self.weap_trail[p][slot as usize] = Some(TrailCopy { fade: 0, alpha_set: false, matrix: m });
        if self.players[p].i32_(0x7d8) < slot {
            self.players[p].set_i32(0x7d8, slot);
        }
        Ok(())
    }
}
