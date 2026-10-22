//! Team moves (two players in play; combo_player 0x144688 needs turbo ≥ 50
//! and the initiator's +0x6b4 node). GetPlayerDMotion's code 0x16 sets
//! +0x694 to the partner; PlayerMotion 0x14c448..0x14c814 links the pair
//! when the action reaches 0x58/0x59 and runs the class's switch (jump table
//! 0x2a7c20): the warrior (0) carries the partner on its "<prefix>DUMMY"
//! node until frame 30 of 0x58 and throws it (status 0x40 for 0xf0
//! vblanks); the valkyrie and archer (1, 3) turn the partner away
//! (PlayerFacePlayer(π)) and ride its DUMMY node; the dwarf (4) does the
//! same with status 0x80 and the throw timer; the wizard, knight, sorceress
//! and jester (2, 5, 6, 7) carry the partner during 0x58/0x59 (the jester
//! lets go at once). Classes 8 and above have no case. PlayerSetParent
//! 0x136c58 / PlayerUnsetParent 0x136d70 move the player's node under the
//! other player's tree node and back to PlayerRootNode; GetObjWorldMat then
//! takes the world matrix from that node.
//!
//! The tree node's world matrix is the other player's record matrix (its
//! node's world) under the tree pose at its clock's sequence and frame
//! (HYPOTHESIS: the original reads the matrix the last MBEndFrame left; the
//! blend fraction is not applied, as in tree_pose).
use super::doplayers::record_matrix_of;
use super::pcontact::ptr;
use super::{unsupported, Game, Unsupported, R};
use crate::collision::yaw_mat3;
use crate::libm::atan2f;
use crate::numeric::{add_guarded_rz as add, sub_guarded_rz as sub};
use crate::tree_pose::{local_pose_where, node_world, Mat, IDENTITY};
use gdl_formats::animation::Animations;

type V3 = [f32; 3];
const PI: f32 = f32::from_bits(0x4049_0fdb);
const TWO_PI: f32 = f32::from_bits(0x40c9_0fdb);
const M4_IDENTITY: [[f32; 4]; 4] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];

/// A player's animation tree for posing its nodes in the simulation: tree 0
/// of the character's ANIM package (the tree the client draws) and the
/// index of its "DUMMY" node (load_player's AtreeFindMbidxNode of
/// "<+0x698>DUMMY"; None when the model has no such object).
#[derive(Debug)]
pub struct PlayerRig {
    pub anim: Animations,
    pub bytes: Vec<u8>,
    pub dummy: Option<usize>,
}

fn wrap(v: f32) -> R<f32> {
    Ok(if PI < v {
        sub(v, TWO_PI)?
    } else if v <= -PI {
        add(v, TWO_PI)?
    } else {
        v
    })
}

impl Game {
    /// World matrix of player `k`'s tree node `n` (see the module note).
    pub(crate) fn player_tree_node_world(&self, k: usize, n: usize) -> R<[[f32; 4]; 4]> {
        let rig = self.player_rigs[k].as_ref().ok_or_else(|| Unsupported("player tree pose without the player's rig".into()))?;
        let tree = rig.anim.trees.first().ok_or_else(|| Unsupported("player rig without a tree".into()))?;
        let parents: Vec<i32> = tree.nodes.iter().map(|x| x.parent).collect();
        let mut chain = Vec::new();
        let mut c = Some(n);
        while let Some(i) = c {
            if parents.len() <= i || 128 < chain.len() {
                return unsupported("player tree node chain");
            }
            chain.push(i);
            c = usize::try_from(parents[i]).ok();
        }
        let r = &self.players[k];
        let (seq, frame) = (u32::from(r.u16_(0xa2)), r.f32_(0xac));
        let locals = local_pose_where(&rig.anim, &rig.bytes, 0, seq, frame, &|i| chain.contains(&i)).map_err(|e| Unsupported(format!("player pose: {}", e.0)))?;
        let locals: Vec<Mat> = locals.into_iter().map(|m| m.unwrap_or(IDENTITY)).collect();
        let m = record_matrix_of(r);
        let root: Mat = std::array::from_fn(|i| m[i / 4][i % 4]);
        let w = node_world(&parents, &locals, root, n, None).map_err(|e| Unsupported(format!("player pose: {}", e.0)))?;
        Ok(std::array::from_fn(|i| std::array::from_fn(|j| w[i * 4 + j])))
    }

    /// GetWorldMat's parent for a team-parented player: the other player's
    /// +0x6b4 node.
    pub(crate) fn team_parent_world(&self, k: usize) -> R<[[f32; 4]; 4]> {
        let n = ptr(self.players[k].u32_(0x6b4)).ok_or_else(|| Unsupported("team parent without a +0x6b4 node".into()))?;
        self.player_tree_node_world(k, n)
    }

    /// PlayerSetParent 0x136c58(p, player k's +0x6b4 node, no offset): the
    /// position is kept at +0xf0, the node becomes an identity matrix under
    /// that node, GetObjWorldMat, status +0x93c |= 0x20.
    fn player_set_team_parent(&mut self, p: usize, k: usize) -> R<()> {
        if ptr(self.players[k].u32_(0x6b4)).is_none() {
            return unsupported("PlayerSetParent under a player without a +0x6b4 node");
        }
        let pos = self.players[p].v3(0x50);
        self.players[p].set_v3(0xf0, pos);
        self.player_floor_parent[p] = None;
        self.player_team_parent[p] = Some(k);
        self.player_node[p] = Some(M4_IDENTITY);
        self.get_obj_world_mat(p)?;
        self.players[p].or_u16(0x93c, 0x20);
        Ok(())
    }

    /// PlayerUnsetParent 0x136d70(p, restore): with `restore` the kept
    /// position returns; the node takes the position under PlayerRootNode
    /// (its rotation stays identity) and +0x93c loses 0x20.
    fn player_unset_parent(&mut self, p: usize, restore: bool) {
        if restore {
            let kept = self.players[p].v3(0xf0);
            self.players[p].set_v3(0x50, kept);
        }
        let pos = self.players[p].v3(0x50);
        let mut m = self.player_node[p].unwrap_or(M4_IDENTITY);
        m[3] = [pos[0], pos[1], pos[2], m[3][3]];
        self.player_node[p] = Some(m);
        self.player_team_parent[p] = None;
        self.players[p].and_u16(0x93c, 0xffdf);
    }

    /// PlayerFacePlayer 0x144548(angle, a, b): a turns to face b plus
    /// `angle` (YawMat3 of the difference to its forward row) and keeps the
    /// facing at +0x86c.
    fn player_face_player(&mut self, angle: f32, a: usize, b: usize) -> R<()> {
        let (pa, pb) = (self.players[a].v3(0x50), self.players[b].v3(0x50));
        let mut d: V3 = [sub(pb[0], pa[0])?, sub(pb[1], pa[1])?, sub(pb[2], pa[2])?];
        super::critter_ai::slow_normal_vector(&mut d)?;
        let atan = |x: f32, z: f32| atan2f(x, z).map_err(|_| Unsupported("atan2f".into()));
        let yaw = wrap(add(atan(d[0], d[2])?, angle)?)?;
        let f = self.players[a].v3(0x40);
        let rel = wrap(sub(yaw, atan(f[0], f[2])?)?)?;
        let r = &self.players[a];
        let mut m = [r.v3(0x20), r.v3(0x30), r.v3(0x40)];
        yaw_mat3(&mut m, rel).map_err(|e| Unsupported(format!("{e:?}")))?;
        for (i, row) in m.iter().enumerate() {
            self.players[a].set_v3(0x20 + 0x10 * i, *row);
        }
        self.players[a].set_f32(0x86c, yaw);
        Ok(())
    }

    /// Both links of a pair cleared (+0x690).
    fn team_unlink(&mut self, p: usize, k: usize) {
        self.players[k].set_u32(0x690, 0);
        self.players[p].set_u32(0x690, 0);
    }

    /// PlayerMotion 0x14c448..0x14c814 (see the module note).
    pub(crate) fn team_move_parenting(&mut self, p: usize) -> R<()> {
        let r = &self.players[p];
        let class = if r.u32_(0x138) & 0x400 != 0 && r.u32_(0x690) == 0 { -1 } else { r.u32_(8) as i32 };
        let cur = r.u32_(0x1e0);
        let frame = r.f32_(0xac);
        let in_move = (0x58..0x5a).contains(&cur);
        if in_move {
            let pending = self.players[p].u32_(0x694);
            if let Some(k) = ptr(pending) {
                self.players[p].set_u32(0x690, pending);
                self.players[k].or_u16(0x93c, 0x10);
                self.players[k].set_u32(0x690, p as u32 + 1);
                self.players[p].set_u32(0x694, 0);
                let t = sub(self.players[p].f32_(0x800), self.players[p].f32_(0x8e8))?;
                self.players[p].set_f32(0x800, t);
                self.players[p].set_u32(0x8e8, 0);
            }
        } else {
            self.players[p].set_u32(0x694, 0);
        }
        let partner = ptr(self.players[p].u32_(0x690));
        let time = self.globals.time;
        match class {
            0 => {
                if cur == 0x58 && frame < 30.0 {
                    if let Some(k) = partner
                        && self.players[k].u16_(0x93c) & 0x20 == 0
                    {
                        self.player_set_team_parent(k, p)?;
                        self.players[k].set_f32(0x8d4, time);
                    }
                    return Ok(());
                }
                let Some(k) = partner else { return Ok(()) };
                let s = self.players[k].u16_(0x93c);
                if s & 0x10 != 0 {
                    if s & 0x20 != 0 {
                        self.player_unset_parent(k, false);
                    }
                    self.players[k].and_u16(0x93c, 0xffef);
                    self.players[k].or_u16(0x93c, 0x40);
                    self.players[p].set_i16(0x1d2, 0xf0);
                }
                let s = self.players[k].u16_(0x93c);
                if s & 0x40 != 0 && self.players[p].i16_(0x1d2) < 1 {
                    self.players[k].set_u16(0x93c, s & 0xffbf);
                    self.team_unlink(p, k);
                    self.players[p].and_u16(0x93c, 0xff7f);
                }
            }
            1 | 3 => {
                let Some(k) = partner else { return Ok(()) };
                if in_move {
                    if self.players[p].u16_(0x93c) & 0x20 == 0 {
                        self.player_face_player(PI, k, p)?;
                        self.player_set_team_parent(p, k)?;
                    }
                    return Ok(());
                }
                if self.players[k].u16_(0x93c) & 0x10 == 0 {
                    return Ok(());
                }
                if self.players[p].u16_(0x93c) & 0x20 != 0 {
                    self.player_unset_parent(p, true);
                }
                self.players[k].and_u16(0x93c, 0xffef);
                self.team_unlink(p, k);
            }
            4 => {
                if cur == 0x58 {
                    if let Some(k) = partner
                        && self.players[p].u16_(0x93c) & 0x20 == 0
                    {
                        self.player_face_player(PI, k, p)?;
                        self.player_set_team_parent(p, k)?;
                        self.players[p].or_u16(0x93c, 0x80);
                        self.players[k].set_f32(0x8d4, time);
                    }
                    self.players[p].set_i16(0x1d2, 0xf0);
                    return Ok(());
                }
                let Some(k) = partner else { return Ok(()) };
                let s = self.players[k].u16_(0x93c);
                if s & 0x10 != 0 {
                    self.players[k].set_u16(0x93c, s & 0xffef);
                    self.players[k].or_u16(0x93c, 0x40);
                    return Ok(());
                }
                if s & 0x40 == 0 || 0 < self.players[p].i16_(0x1d2) {
                    return Ok(());
                }
                if self.players[p].u16_(0x93c) & 0x20 != 0 {
                    self.player_unset_parent(p, false);
                }
                self.players[k].and_u16(0x93c, 0xffbf);
                self.players[p].and_u16(0x93c, 0xff7f);
                self.team_unlink(p, k);
            }
            2 | 5 | 6 | 7 => {
                if in_move {
                    if let Some(k) = partner
                        && self.players[k].u16_(0x93c) & 0x20 == 0
                    {
                        self.player_set_team_parent(k, p)?;
                        return Ok(());
                    }
                    if class != 7 {
                        return Ok(());
                    }
                }
                let Some(k) = partner else { return Ok(()) };
                let s = self.players[k].u16_(0x93c);
                if s & 0x10 == 0 {
                    return Ok(());
                }
                if s & 0x20 != 0 {
                    self.player_unset_parent(k, true);
                }
                self.players[k].and_u16(0x93c, 0xffef);
                self.team_unlink(p, k);
            }
            _ => {}
        }
        Ok(())
    }
}
