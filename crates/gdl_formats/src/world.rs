//! Observed v2 world node and collision records; no gameplay collision solver.
use crate::{binary::Reader, wad::ParseError};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct WorldNode {
    pub index: u32,
    pub label: String,
    pub flags: u32,
    pub position: [f32; 3],
    pub radius: f32,
    pub next: i16,
    pub child: i16,
    pub collision_count: i16,
    pub collision_index: i32,
    pub raw: Vec<u8>,
}
#[derive(Debug, Serialize)]
pub struct CollisionRecord {
    pub index: u32,
    pub raw: Vec<u8>,
}
#[derive(Debug, Serialize)]
pub struct World {
    pub raw_header: Vec<u8>,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub nodes: Vec<WorldNode>,
    pub collisions: Vec<CollisionRecord>,
}
#[derive(Debug, Serialize)]
pub struct Placement {
    pub node_index: u32,
    pub parent: Option<u32>,
    pub summed_position: [f32; 3],
}
impl World {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let r = Reader {
            bytes,
            family: "WORLDS",
        };
        r.range(0, 120)?;
        if bytes.len() > 64 * 1024 * 1024 || r.u32(96) != 0xf00bab02 {
            return Err(r.error(96, "unsupported world version or file size policy"));
        }
        let nodes = r.table(r.u32(4), r.u32(0), 60, 120)?;
        let collisions = r.table(r.u32(12), r.u32(8), 40, 120)?;
        if !nodes.is_empty()
            && !collisions.is_empty()
            && nodes.start < collisions.end
            && collisions.start < nodes.end
        {
            return Err(r.error(4, "world nodes and collisions overlap"));
        }
        let min = r.xyz(36)?;
        let max = r.xyz(48)?;
        if (0..3).any(|a| min[a] > max[a]) {
            return Err(r.error(36, "reversed bounds"));
        }
        let mut results = Vec::new();
        for (index, p) in (nodes.start..nodes.end).step_by(60).enumerate() {
            let next = r.i16(p + 44);
            let child = r.i16(p + 46);
            for link in [next, child] {
                if link < -1 || (link >= 0 && link as u32 >= r.u32(0)) {
                    return Err(r.error(p + 44, "node graph link out of range"));
                }
            }
            let count = r.i16(p + 54);
            let first = r.i32(p + 56);
            if count < 0
                || (count > 0 && (first < 0 || first as u64 + count as u64 > r.u32(8) as u64))
            {
                return Err(r.error(p + 54, "node collision range out of bounds"));
            }
            results.push(WorldNode {
                index: index as u32,
                label: r.label(p, 16),
                flags: r.u32(p + 16),
                position: r.xyz(p + 28)?,
                radius: r.float(p + 48)?,
                next,
                child,
                collision_count: count,
                collision_index: first,
                raw: bytes[p..p + 60].to_vec(),
            });
        }
        let collisions = (collisions.start..collisions.end)
            .step_by(40)
            .enumerate()
            .map(|(index, p)| CollisionRecord {
                index: index as u32,
                raw: bytes[p..p + 40].to_vec(),
            })
            .collect();
        Ok(Self {
            raw_header: bytes[..120].to_vec(),
            min,
            max,
            nodes: results,
            collisions,
        })
    }
    /// Candidate translation-only interpretation of the next/child graph.
    /// Multiple parents and cycles are rejected, never flattened arbitrarily.
    pub fn placements(&self) -> Result<Vec<Placement>, ParseError> {
        let error = |reason: &str| ParseError {
            entry_index: None,
            byte_offset: 0,
            reason: format!("WORLDS graph: {reason}"),
        };
        let mut incoming = vec![0usize; self.nodes.len()];
        for node in &self.nodes {
            for link in [node.next, node.child] {
                if link >= 0 {
                    incoming[link as usize] += 1;
                }
            }
        }
        if incoming.iter().any(|n| *n > 1) {
            return Err(error("multiple incoming links"));
        }
        let mut stack: Vec<_> = incoming
            .iter()
            .enumerate()
            .filter(|(_, n)| **n == 0)
            .map(|(i, _)| (i, None, [0.0f32; 3]))
            .collect();
        let mut seen = vec![false; self.nodes.len()];
        let mut out = Vec::new();
        while let Some((i, parent, translation)) = stack.pop() {
            if seen[i] {
                return Err(error("cycle/repeated node"));
            }
            seen[i] = true;
            let node = &self.nodes[i];
            let sum = std::array::from_fn(|a| translation[a] + node.position[a]);
            if sum.iter().any(|v| !v.is_finite()) {
                return Err(error("non-finite summed transform"));
            }
            out.push(Placement {
                node_index: i as u32,
                parent,
                summed_position: sum,
            });
            if node.next >= 0 {
                stack.push((node.next as usize, parent, translation));
            }
            if node.child >= 0 {
                stack.push((node.child as usize, Some(i as u32), sum));
            }
        }
        if seen.iter().any(|b| !*b) {
            return Err(error("unrooted cycle"));
        }
        out.sort_by_key(|p| p.node_index);
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn world() -> Vec<u8> {
        let mut b = vec![0; 300];
        b[0..4].copy_from_slice(&3u32.to_le_bytes());
        b[4..8].copy_from_slice(&120u32.to_le_bytes());
        b[96..100].copy_from_slice(&0xf00bab02u32.to_le_bytes());
        for i in 0..3 {
            let p = 120 + i * 60;
            b[p + 28..p + 32].copy_from_slice(&((i + 1) as f32).to_le_bytes());
            b[p + 44..p + 48].copy_from_slice(&[255; 4]);
        }
        b[166..168].copy_from_slice(&1i16.to_le_bytes()); // 0.child=1
        b[224..226].copy_from_slice(&2i16.to_le_bytes()); // 1.next=2
        b
    }
    #[test]
    fn sibling_translation_uses_parent_and_cycles_fail() {
        let mut b = world();
        let w = World::parse(&b).unwrap();
        let p = w.placements().unwrap();
        assert_eq!(p[1].summed_position, [3.0, 0.0, 0.0]);
        assert_eq!(p[2].summed_position, [4.0, 0.0, 0.0]);
        assert_eq!(p[2].parent, Some(0));
        b[284..286].copy_from_slice(&0i16.to_le_bytes());
        assert!(World::parse(&b).unwrap().placements().is_err());
        b[284..286].copy_from_slice(&4i16.to_le_bytes());
        assert!(World::parse(&b).is_err());
    }
    #[test]
    fn world_tables_and_collision_references_are_bounded() {
        let b = world();
        for n in 0..300 {
            assert!(World::parse(&b[..n]).is_err());
        }
        let mut bad = b.clone();
        bad[8..12].copy_from_slice(&1u32.to_le_bytes());
        bad[12..16].copy_from_slice(&120u32.to_le_bytes());
        assert!(World::parse(&bad).is_err());
        let mut bad = b;
        bad[174..176].copy_from_slice(&1i16.to_le_bytes());
        assert!(World::parse(&bad).is_err());
    }
}
