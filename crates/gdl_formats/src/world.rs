//! Observed v2 world node, collision-triangle and collision-grid records.
//! Field meanings come from WorldCollide/CTriListCollide/BTriLineCol consumers
//! (EE 0x11e160/0x11d600/0x11af80); the grid and triangles load unchanged.
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
    /// Signed y range ×64 compared with the query's truncated y ×64.
    pub min_y64: i16,
    pub max_y64: i16,
    /// Normal-xz scale used by BodyVectorNorm/WorldVectorNorm/CreateMat3Norm.
    /// Stored as bits: flat triangles hold +inf, which the original never reads.
    pub scale_bits: u32,
    pub normal: [f32; 3],
    pub origin: [f32; 3],
    /// Local-plane vertices (u1, v1, u2, v2) ×64; vertex zero is the origin.
    pub vertices64: [i16; 4],
    pub raw: Vec<u8>,
}
#[derive(Debug, Serialize, Clone, Copy)]
pub struct GridRow {
    pub min_x: u16,
    pub max_x: u16,
    pub cell_offset: u32,
}
/// Static collision grid. Cell words are `count << 22 | content byte offset`;
/// content entries are i16 object index, i16 triangle count, then the counts'
/// i16 triangle offsets relative to the object's first triangle.
#[derive(Debug, Serialize, Clone)]
pub struct CollisionGrid {
    pub cell_size: f32,
    pub dim_x: u32,
    pub dim_z: u32,
    pub rows: Vec<GridRow>,
    pub cells: Vec<u32>,
    pub contents: Vec<u8>,
    /// Header words +16 and +24; meanings unresolved.
    pub header_16: u32,
    pub header_24: u32,
}
/// Item definition (80 bytes; WorldInfo +0x68/+0x74 consumers InitItemInfoData,
/// NewItem). Type/subtype words and name are consumer-qualified; the remaining
/// fields are retained raw until their consumers are recovered.
#[derive(Debug, Serialize, Clone)]
pub struct ItemDefinition {
    pub index: u32,
    pub kind: i32,
    pub subtype: i32,
    pub word_08: i32,
    pub scale: f32,
    pub name: String,
    pub raw: Vec<u8>,
}
/// Item instance (60 bytes; AddItemInstList → NewItem(record, +0x18, +0x24)).
#[derive(Debug, Serialize, Clone)]
pub struct ItemInstance {
    pub index: u32,
    pub definition: u16,
    pub flags_02: u16,
    /// +0x04..+0x18: parameters, e.g. a referenced world-object name.
    pub parameters: Vec<u8>,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub raw: Vec<u8>,
}

#[derive(Debug, Serialize)]
pub struct World {
    pub raw_header: Vec<u8>,
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub nodes: Vec<WorldNode>,
    pub collisions: Vec<CollisionRecord>,
    pub grid: CollisionGrid,
    pub item_definitions: Vec<ItemDefinition>,
    pub items: Vec<ItemInstance>,
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
        let mut records = Vec::new();
        for (index, p) in (collisions.start..collisions.end).step_by(40).enumerate() {
            let scale_bits = r.u32(p + 4);
            if f32::from_bits(scale_bits).is_nan() {
                return Err(r.error(p + 4, "NaN collision normal scale"));
            }
            records.push(CollisionRecord {
                index: index as u32,
                min_y64: r.i16(p),
                max_y64: r.i16(p + 2),
                scale_bits,
                normal: r.xyz(p + 8)?,
                origin: r.xyz(p + 20)?,
                vertices64: std::array::from_fn(|i| r.i16(p + 32 + 2 * i)),
                raw: bytes[p..p + 40].to_vec(),
            });
        }
        let grid = Self::grid(&r)?;
        let definitions = r.table(r.u32(76), r.u32(72), 80, 120)?;
        let mut item_definitions = Vec::new();
        for (index, p) in definitions.step_by(80).enumerate() {
            item_definitions.push(ItemDefinition {
                index: index as u32,
                kind: r.i32(p),
                subtype: r.i32(p + 4),
                word_08: r.i32(p + 8),
                scale: r.float(p + 12)?,
                name: r.label(p + 0x28, 20),
                raw: bytes[p..p + 80].to_vec(),
            });
        }
        let instances = r.table(r.u32(84), r.u32(80), 60, 120)?;
        let mut items = Vec::new();
        for (index, p) in instances.step_by(60).enumerate() {
            let definition = r.u16(p);
            if usize::from(definition) >= item_definitions.len() {
                return Err(r.error(p, "item instance definition out of range"));
            }
            items.push(ItemInstance {
                index: index as u32,
                definition,
                flags_02: r.u16(p + 2),
                parameters: bytes[p + 4..p + 0x18].to_vec(),
                position: r.xyz(p + 0x18)?,
                rotation: r.xyz(p + 0x24)?,
                raw: bytes[p..p + 60].to_vec(),
            });
        }
        Ok(Self {
            raw_header: bytes[..120].to_vec(),
            min,
            max,
            nodes: results,
            collisions: records,
            grid,
            item_definitions,
            items,
        })
    }
    fn grid(r: &Reader) -> Result<CollisionGrid, ParseError> {
        let (dim_x, dim_z) = (r.u32(64), r.u32(68));
        let cell_size = r.float(60)?;
        if cell_size <= 0.0 || dim_x == 0 || dim_z == 0 || dim_x > 4096 || dim_z > 4096 {
            return Err(r.error(60, "collision grid dimensions outside policy"));
        }
        let rows_range = r.table(r.u32(32), dim_z, 8, 120)?;
        let rows: Vec<_> = rows_range
            .step_by(8)
            .map(|p| GridRow {
                min_x: r.u16(p),
                max_x: r.u16(p + 2),
                cell_offset: r.u32(p + 4),
            })
            .collect();
        let mut cell_count = 0u64;
        for (z, row) in rows.iter().enumerate() {
            if row.min_x > row.max_x || u32::from(row.max_x) >= dim_x {
                return Err(r.error(r.u32(32) as usize + 8 * z, "grid row x range invalid"));
            }
            cell_count = cell_count.max(u64::from(row.cell_offset) + u64::from(row.max_x - row.min_x) + 1);
        }
        if cell_count > 1 << 22 {
            return Err(r.error(r.u32(32) as usize, "grid cell count policy exceeded"));
        }
        let cells_range = r.table(r.u32(20), cell_count as u32, 4, 120)?;
        let cells: Vec<u32> = cells_range.step_by(4).map(|p| r.u32(p)).collect();
        let content_start = r.u32(28) as usize;
        let mut content_end = content_start;
        for cell in &cells {
            let mut p = content_start + (cell & 0x3f_ffff) as usize;
            for _ in 0..cell >> 22 {
                r.range(p, 4)?;
                let count = r.i16(p + 2);
                if count < 0 {
                    return Err(r.error(p + 2, "negative grid triangle count"));
                }
                // A zero count does not advance in WorldCollide; it is retained as-is.
                if count > 0 {
                    r.range(p + 4, 2 * count as usize)?;
                    if (0..count as usize).any(|k| r.i16(p + 4 + 2 * k) < 0) {
                        return Err(r.error(p + 4, "negative grid triangle offset"));
                    }
                    p += 4 + 2 * count as usize;
                } else {
                    p += 4;
                }
            }
            content_end = content_end.max(p);
        }
        Ok(CollisionGrid {
            cell_size,
            dim_x,
            dim_z,
            rows,
            cells,
            contents: r.bytes[content_start..content_end].to_vec(),
            header_16: r.u32(16),
            header_24: r.u32(24),
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
        b.resize(308, 0);
        b[60..64].copy_from_slice(&3.0f32.to_le_bytes());
        b[64..68].copy_from_slice(&1u32.to_le_bytes());
        b[68..72].copy_from_slice(&1u32.to_le_bytes());
        b[32..36].copy_from_slice(&300u32.to_le_bytes()); // rows; row 0 = x 0..0, cell 0
        b[20..24].copy_from_slice(&296u32.to_le_bytes()); // one empty cell word
        b[28..32].copy_from_slice(&296u32.to_le_bytes());
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
        let w = World::parse(&b).unwrap();
        assert_eq!((w.grid.dim_x, w.grid.rows.len(), w.grid.cells.len()), (1, 1, 1));
        for n in 0..308 {
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
