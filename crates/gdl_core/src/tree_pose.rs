//! Simulation-side pose of an animation tree: DoAnimation local matrices at a
//! sequence frame (exact keys or sampled channels) composed into world
//! matrices with the source GetWorldMat parent order. Critter collision nodes,
//! attack origins and transitions read these matrices.
//!
//! Not modelled (labelled): sequence blending (the blend fraction is ignored),
//! node scale (returned separately by DoAnimation, not applied by
//! GetWorldMat), texture-mod and object-animation nodes (types other than 1
//! keep their initial position with an identity basis).
use crate::numeric::UnsupportedNumericDomain;
use crate::world_transform::{compose, MatrixBox};
use gdl_formats::animation::Animations;

/// Row-major MB matrix (rows 0–2 basis, row 3 translation).
pub type Mat = [f32; 16];

pub const IDENTITY: Mat = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

#[derive(Debug)]
pub struct PoseError(pub String);

impl From<UnsupportedNumericDomain> for PoseError {
    fn from(_: UnsupportedNumericDomain) -> Self {
        PoseError("numeric domain".into())
    }
}

/// Local matrices of every node of `tree` at `frame` of `sequence`.
pub fn local_pose(anim: &Animations, bytes: &[u8], tree: usize, sequence: u32, frame: f32) -> Result<Vec<Mat>, PoseError> {
    Ok(local_pose_where(anim, bytes, tree, sequence, frame, &|_| true)?.into_iter().map(|m| m.unwrap_or(IDENTITY)).collect())
}

/// [`local_pose`] for the nodes accepted by `keep` (others are None): an
/// animation instance whose root is a subtree node animates only that subtree.
pub fn local_pose_where(anim: &Animations, bytes: &[u8], tree: usize, sequence: u32, frame: f32, keep: &dyn Fn(usize) -> bool) -> Result<Vec<Option<Mat>>, PoseError> {
    let t = anim.trees.get(tree).ok_or_else(|| PoseError("tree index".into()))?;
    let mut out = Vec::with_capacity(t.nodes.len());
    for (k, node) in t.nodes.iter().enumerate() {
        if !keep(k) {
            out.push(None);
            continue;
        }
        if node.node_type != 1 {
            let mut m = IDENTITY;
            m[12..15].copy_from_slice(&node.initial_position);
            out.push(Some(m));
            continue;
        }
        let track = t.key_track(bytes, node.index, sequence).map_err(|e| PoseError(format!("key track: {e}")))?;
        let channels = crate::animation::sample_channels_long_gaps(&track, frame).map_err(|e| PoseError(format!("sample: {e}")))?;
        let (m, _scale) = crate::transform::local_transform(channels, node.initial_position, track.transform_flags)?;
        out.push(Some(m));
    }
    Ok(out)
}

/// GetWorldMat for node `i`: the local matrix multiplied by each ancestor in
/// turn (immediate parent first), then by `root` (the owner's matrix), with an
/// optional local offset added to the leaf translation first.
pub fn node_world(tree_parents: &[i32], locals: &[Mat], root: Mat, i: usize, offset: Option<[f32; 3]>) -> Result<Mat, PoseError> {
    let mut parents = Vec::new();
    let mut k = usize::try_from(tree_parents[i]).ok();
    while let Some(p) = k {
        if 128 < parents.len() {
            return Err(PoseError("parent chain".into()));
        }
        parents.push(MatrixBox { matrix: locals[p], flags: 0 });
        k = usize::try_from(tree_parents[p]).ok();
    }
    parents.push(MatrixBox { matrix: root, flags: 0 });
    Ok(compose(MatrixBox { matrix: locals[i], flags: 0 }, &mut parents, offset)?.matrix)
}

/// [`node_world`] with an explicit chain after the tree's ancestors
/// (immediate first; flag 4 boxes add their translation), for trees whose
/// root hangs under further MB nodes.
pub fn node_world_chain(tree_parents: &[i32], locals: &[Mat], roots: &[MatrixBox], i: usize, offset: Option<[f32; 3]>) -> Result<Mat, PoseError> {
    let mut parents = Vec::new();
    let mut k = usize::try_from(tree_parents[i]).ok();
    while let Some(p) = k {
        if 128 < parents.len() {
            return Err(PoseError("parent chain".into()));
        }
        parents.push(MatrixBox { matrix: locals[p], flags: 0 });
        k = usize::try_from(tree_parents[p]).ok();
    }
    parents.extend_from_slice(roots);
    Ok(compose(MatrixBox { matrix: locals[i], flags: 0 }, &mut parents, offset)?.matrix)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_follows_parent_chain_and_root() {
        // Synthetic: root translation (10,0,0), node 0 at (0,1,0), node 1 child
        // of 0 at (0,0,2); node 1 world = (10,1,2).
        let mut a = IDENTITY;
        a[13] = 1.0;
        let mut b = IDENTITY;
        b[14] = 2.0;
        let mut root = IDENTITY;
        root[12] = 10.0;
        let w = node_world(&[-1, 0], &[a, b], root, 1, None).unwrap();
        assert_eq!(&w[12..15], &[10.0, 1.0, 2.0]);
        let w = node_world(&[-1, 0], &[a, b], root, 1, Some([0.5, 0.0, 0.0])).unwrap();
        assert_eq!(w[12], 10.5);
    }
}
