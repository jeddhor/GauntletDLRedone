//! Bounded animation-tree metadata; sampling and animation events are separate.
use crate::{binary::Reader, wad::ParseError};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Sequence {
    pub index: u32,
    pub label: String,
    pub frames: i16,
    pub frame_rate: i16,
    pub repeat: u16,
    pub flags: u16,
    pub raw: Vec<u8>,
}
#[derive(Debug, Serialize)]
pub struct Node {
    pub index: u32,
    pub label: String,
    pub initial_position: [f32; 3],
    pub node_type: i16,
    pub flags: u16,
    pub model_flags: u32,
    pub parent: i32,
    pub sequence_info_offset: i32,
    pub raw: Vec<u8>,
}
#[derive(Debug, Serialize)]
pub struct SequenceInfo {
    pub node_index: u32,
    pub sequence_index: u32,
    pub transform_flags: u16,
    pub channels: u16,
    pub relative_data_offset: u32,
    pub raw: Vec<u8>,
}
#[derive(Debug, Serialize)]
pub struct Tree {
    pub index: u32,
    pub label: String,
    pub file_offset: u32,
    pub prefix: String,
    pub raw_info: Vec<u8>,
    pub raw_header: Vec<u8>,
    pub raw_animation_header: Vec<u8>,
    pub compressed_angles_offset: u32,
    pub compressed_positions_offset: u32,
    pub compressed_scales_offset: u32,
    pub blocks_offset: u32,
    pub sequences: Vec<Sequence>,
    pub nodes: Vec<Node>,
    pub sequence_infos: Vec<SequenceInfo>,
}
#[derive(Debug, Serialize)]
pub struct Animations {
    pub version: u16,
    pub raw_header: Vec<u8>,
    pub trees: Vec<Tree>,
}
#[derive(Debug, Serialize)]
pub struct InitialChannels {
    pub node_index: u32,
    pub sequence_index: u32,
    pub angles: [f32; 3],
    pub position_delta: [f32; 3],
    pub scale: [f32; 3],
    pub bitmap_file_offset: u32,
    pub bitmap_raw: Vec<u8>,
    pub initial_values_file_offset: u32,
    pub initial_values_raw: Vec<u8>,
}
impl Animations {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let r = Reader {
            bytes,
            family: "ANIM",
        };
        r.range(0, 16)?;
        let version = r.u16(2);
        let header = match version {
            0 => 16,
            8 => 24,
            _ => return Err(r.error(2, "unsupported version")),
        };
        r.range(0, header)?;
        if bytes.len() > 64 * 1024 * 1024 || r.u16(0) > 4096 {
            return Err(r.error(0, "input/tree count policy exceeded"));
        }
        let infos = r.table(r.u32(4), r.u16(0) as u32, 36, header)?;
        let mut trees = Vec::new();
        let mut total_infos = 0usize;
        let mut total_nodes = 0usize;
        let mut total_sequences = 0usize;
        for (index, i) in (infos.start..infos.end).step_by(36).enumerate() {
            let base = r.u32(i + 32) as usize;
            r.range(base, 56)?;
            if base < header || !base.is_multiple_of(4) {
                return Err(r.error(i + 32, "invalid tree header pointer"));
            }
            let relative = |offset: usize| -> Result<usize, ParseError> {
                let p = base
                    .checked_add(r.u32(base + offset) as usize)
                    .ok_or_else(|| r.error(base + offset, "tree-relative overflow"))?;
                r.range(p, 0)?;
                Ok(p)
            };
            let sequence_count = r.i32(base + 20);
            let node_count = r.i32(base + 16);
            if !(0..=4096).contains(&sequence_count) || !(0..=16384).contains(&node_count) {
                return Err(r.error(base + 16, "invalid sequence/node count"));
            }
            total_nodes += node_count as usize;
            total_sequences += sequence_count as usize;
            if total_nodes > 100_000 || total_sequences > 100_000 {
                return Err(r.error(
                    base + 16,
                    "aggregate node/sequence metadata budget exceeded",
                ));
            }
            let seqs = r.table(relative(0)? as u32, sequence_count as u32, 48, header)?;
            let node_range = r.table(relative(12)? as u32, node_count as u32, 60, header)?;
            if !seqs.is_empty()
                && !node_range.is_empty()
                && seqs.start < node_range.end
                && node_range.start < seqs.end
            {
                return Err(r.error(base, "sequences overlap nodes"));
            }
            let animation = relative(4)?;
            r.range(animation, 28)?;
            let pointer = |field: usize| -> Result<usize, ParseError> {
                let p = animation
                    .checked_add(r.u32(animation + field) as usize)
                    .ok_or_else(|| r.error(animation + field, "animation-relative overflow"))?;
                r.range(p, 0)?;
                Ok(p)
            };
            let blocks = pointer(12)?;
            let mut sequences = Vec::new();
            for (index, p) in (seqs.start..seqs.end).step_by(48).enumerate() {
                let frames = r.i16(p + 32);
                let rate = r.i16(p + 34);
                if frames < 0 || rate <= 0 {
                    return Err(r.error(p + 32, "invalid frame count/rate"));
                }
                sequences.push(Sequence {
                    index: index as u32,
                    label: r.label(p, 32),
                    frames,
                    frame_rate: rate,
                    repeat: r.u16(p + 36),
                    flags: r.u16(p + 42),
                    raw: bytes[p..p + 48].to_vec(),
                });
            }
            let mut nodes = Vec::new();
            let mut sequence_infos = Vec::new();
            for (index, p) in (node_range.start..node_range.end).step_by(60).enumerate() {
                let parent = r.i32(p + 56);
                if parent < -1 || parent >= node_count || parent == index as i32 {
                    return Err(r.error(p + 56, "invalid parent index"));
                }
                let node_type = r.i16(p + 44);
                let info_offset = r.i32(p + 52);
                if node_type == 1 && sequence_count > 0 {
                    if info_offset < 0 {
                        return Err(r.error(p + 52, "negative skeletal sequence-info offset"));
                    }
                    let info_start = animation
                        .checked_add(info_offset as usize)
                        .ok_or_else(|| r.error(p + 52, "sequence-info overflow"))?;
                    let range = r.range(info_start, sequence_count as usize * 8)?;
                    total_infos += sequence_count as usize;
                    if total_infos > 1_000_000 {
                        return Err(r.error(p + 52, "total channel metadata budget exceeded"));
                    }
                    for (seq, q) in (range.start..range.end).step_by(8).enumerate() {
                        let data_offset = r.u32(q + 4);
                        let flags = r.u16(q);
                        let channels = r.u16(q + 2);
                        if channels > 9 {
                            return Err(r.error(q + 2, "channel count exceeds transform axes"));
                        }
                        let data = blocks
                            .checked_add(data_offset as usize)
                            .ok_or_else(|| r.error(q + 4, "channel-data pointer overflow"))?;
                        // The future sampler must validate actual frame bitmaps/data sizes.
                        r.range(data, if channels > 0 { 1 } else { 0 })?;
                        sequence_infos.push(SequenceInfo {
                            node_index: index as u32,
                            sequence_index: seq as u32,
                            transform_flags: flags,
                            channels,
                            relative_data_offset: data_offset,
                            raw: bytes[q..q + 8].to_vec(),
                        });
                    }
                }
                nodes.push(Node {
                    index: index as u32,
                    label: r.label(p, 32),
                    initial_position: r.xyz(p + 32)?,
                    node_type,
                    flags: r.u16(p + 46),
                    model_flags: r.u32(p + 48),
                    parent,
                    sequence_info_offset: info_offset,
                    raw: bytes[p..p + 60].to_vec(),
                });
            }
            let tree = Tree {
                index: index as u32,
                label: r.label(i, 32),
                file_offset: base as u32,
                prefix: r.label(base + 24, 30),
                raw_info: bytes[i..i + 36].to_vec(),
                raw_header: bytes[base..base + 56].to_vec(),
                raw_animation_header: bytes[animation..animation + 28].to_vec(),
                compressed_angles_offset: pointer(0)? as u32,
                compressed_positions_offset: pointer(4)? as u32,
                compressed_scales_offset: pointer(8)? as u32,
                blocks_offset: blocks as u32,
                sequences,
                nodes,
                sequence_infos,
            };
            tree.initial_translations()?;
            trees.push(tree);
        }
        Ok(Self {
            version,
            raw_header: bytes[..header].to_vec(),
            trees,
        })
    }
}
impl Tree {
    /// Fresh frame-zero branch from the inspected GetAnimAngXYZVal consumer.
    /// This is not compressed delta accumulation, interpolation or playback.
    pub fn initial_channels(&self, bytes: &[u8]) -> Result<Vec<InitialChannels>, ParseError> {
        let r = Reader {
            bytes,
            family: "ANIM initial channels",
        };
        // Original TypeFlags array at EE 0x21edd8, checked in the frozen fixture.
        let flags = [1u16, 2, 4, 16, 32, 64, 256, 512, 1024];
        let mut result = Vec::new();
        let mut retained_bytes = 0usize;
        for info in &self.sequence_infos {
            if flags
                .iter()
                .filter(|bit| info.transform_flags & **bit != 0)
                .count()
                != info.channels as usize
            {
                return Err(r.error(
                    self.blocks_offset as usize,
                    "axis flags/channel count disagree",
                ));
            }
            let start = (self.blocks_offset as usize)
                .checked_add(info.relative_data_offset as usize)
                .ok_or_else(|| {
                    r.error(self.blocks_offset as usize, "initial data pointer overflow")
                })?;
            let frames = self.sequences[info.sequence_index as usize].frames as usize;
            let bitmap_size = if info.transform_flags & 0x4000 != 0 {
                0
            } else {
                frames.div_ceil(32) * 4
            };
            retained_bytes += bitmap_size + info.channels as usize * 4;
            if retained_bytes > 16 * 1024 * 1024 {
                return Err(r.error(start, "retained initial-channel byte policy exceeded"));
            }
            let bitmap = r.range(start, bitmap_size)?;
            let initial = r.range(bitmap.end, info.channels as usize * 4)?;
            let mut values = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
            let mut cursor = initial.start;
            for (index, bit) in flags.iter().enumerate() {
                if info.transform_flags & bit != 0 {
                    values[index] = r.float(cursor)?;
                    cursor += 4;
                }
            }
            result.push(InitialChannels {
                node_index: info.node_index,
                sequence_index: info.sequence_index,
                angles: values[..3].try_into().unwrap(),
                position_delta: values[3..6].try_into().unwrap(),
                scale: values[6..].try_into().unwrap(),
                bitmap_file_offset: start as u32,
                bitmap_raw: bytes[bitmap].to_vec(),
                initial_values_file_offset: initial.start as u32,
                initial_values_raw: bytes[initial].to_vec(),
            });
        }
        Ok(result)
    }
    /// Translation-only diagnostic rest positions, not an accepted bind pose.
    pub fn initial_translations(&self) -> Result<Vec<[f32; 3]>, ParseError> {
        let mut results = vec![None; self.nodes.len()];
        let mut visiting = vec![false; self.nodes.len()];
        for start in 0..self.nodes.len() {
            let mut path = Vec::new();
            let mut cursor = start;
            let mut translation = loop {
                if let Some(value) = results[cursor] {
                    break value;
                }
                if visiting[cursor] {
                    return Err(ParseError {
                        entry_index: None,
                        byte_offset: self.file_offset as usize,
                        reason: "ANIM: cyclic parent chain".into(),
                    });
                }
                visiting[cursor] = true;
                path.push(cursor);
                if self.nodes[cursor].parent < -1
                    || self.nodes[cursor].parent >= self.nodes.len() as i32
                {
                    return Err(ParseError {
                        entry_index: None,
                        byte_offset: self.file_offset as usize,
                        reason: "ANIM: invalid parent in hierarchy".into(),
                    });
                }
                if self.nodes[cursor].parent < 0 {
                    break [0.0; 3];
                }
                cursor = self.nodes[cursor].parent as usize;
            };
            for &i in path.iter().rev() {
                translation = std::array::from_fn(|axis| {
                    translation[axis] + self.nodes[i].initial_position[axis]
                });
                if translation.iter().any(|v| !v.is_finite()) {
                    return Err(ParseError {
                        entry_index: None,
                        byte_offset: self.file_offset as usize,
                        reason: "ANIM: non-finite summed translation".into(),
                    });
                }
                results[i] = Some(translation);
                visiting[i] = false;
            }
        }
        Ok(results.into_iter().map(Option::unwrap).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 256];
        for (p, v) in [
            (0, 1u32),
            (4, 16),
            (48, 52),
            (52, 56),
            (56, 164),
            (60, 164),
            (64, 104),
            (68, 1),
            (72, 1),
            (216, 28),
            (220, 28),
            (224, 28),
            (228, 36),
            (232, 28),
            (236, 1),
            (240, 1),
            (248, 0),
        ] {
            b[p..p + 4].copy_from_slice(&v.to_le_bytes());
        }
        b[16..20].copy_from_slice(b"TEST");
        b[108..113].copy_from_slice(b"READY");
        b[140..142].copy_from_slice(&30i16.to_le_bytes());
        b[142..144].copy_from_slice(&30i16.to_le_bytes());
        b[156..160].copy_from_slice(b"ROOT");
        b[192..196].copy_from_slice(&1.0f32.to_le_bytes());
        b[200..202].copy_from_slice(&1i16.to_le_bytes());
        b[208..212].copy_from_slice(&28i32.to_le_bytes());
        b[212..216].copy_from_slice(&(-1i32).to_le_bytes());
        b[244..246].copy_from_slice(&0x4001u16.to_le_bytes());
        b[246..248].copy_from_slice(&1u16.to_le_bytes());
        b
    }
    #[test]
    fn relative_metadata_and_static_hierarchy_are_explicit() {
        let a = Animations::parse(&fixture()).unwrap();
        let t = &a.trees[0];
        assert_eq!(t.sequences[0].label, "READY");
        assert_eq!(t.sequences[0].frame_rate, 30);
        assert_eq!(t.nodes[0].parent, -1);
        assert_eq!(t.initial_translations().unwrap(), [[0.0, 1.0, 0.0]]);
        assert_eq!(t.sequence_infos[0].transform_flags, 0x4001);
        assert_eq!(t.blocks_offset, 252);
    }
    #[test]
    fn metadata_truncation_overflow_invalid_parents_and_counts_fail() {
        let b = fixture();
        for n in 0..253 {
            assert!(Animations::parse(&b[..n]).is_err(), "prefix {n}");
        }
        let mut b = fixture();
        b[52..56].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Animations::parse(&b).is_err());
        let mut b = fixture();
        b[212..216].copy_from_slice(&0i32.to_le_bytes());
        assert!(Animations::parse(&b).is_err());
        let mut b = fixture();
        b[68..72].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Animations::parse(&b).is_err());
        let mut b = fixture();
        b[246..248].copy_from_slice(&10u16.to_le_bytes());
        assert!(Animations::parse(&b).is_err());
    }
    #[test]
    fn first_frame_bitmap_alignment_and_channels_follow_consumer() {
        let mut b = fixture();
        let a = Animations::parse(&b).unwrap();
        let s = a.trees[0].initial_channels(&b).unwrap();
        assert!(s[0].bitmap_raw.is_empty());
        assert_eq!(s[0].scale, [1.0; 3]);
        b.resize(264, 0);
        b[140..142].copy_from_slice(&33i16.to_le_bytes());
        b[244..246].copy_from_slice(&0x2001u16.to_le_bytes());
        b[260..264].copy_from_slice(&1.5f32.to_le_bytes());
        let a = Animations::parse(&b).unwrap();
        let s = a.trees[0].initial_channels(&b).unwrap();
        assert_eq!(s[0].bitmap_raw.len(), 8);
        assert_eq!(s[0].initial_values_file_offset, 260);
        assert_eq!(s[0].angles, [1.5, 0.0, 0.0]);
        assert!(a.trees[0].initial_channels(&b[..263]).is_err());
        b[260..264].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(a.trees[0].initial_channels(&b).is_err());
    }
    #[test]
    fn sparse_compressed_keys_cross_word_boundaries_and_retain_raw_rows() {
        let mut b = fixture();
        b.resize(1324, 0);
        b[140..142].copy_from_slice(&33i16.to_le_bytes());
        b[244..246].copy_from_slice(&0x2001u16.to_le_bytes());
        b[216..220].copy_from_slice(&84u32.to_le_bytes()); // angle table = 300
        b[252] = 5; // keys 0, 2
        b[256] = 1; // key 32, including padding to 64 bits
        b[260..264].copy_from_slice(&0.5f32.to_le_bytes());
        b[264..266].copy_from_slice(&[1, 2]);
        b[304..308].copy_from_slice(&0.125f32.to_le_bytes());
        b[308..312].copy_from_slice(&(-0.25f32).to_le_bytes());
        let a = Animations::parse(&b).unwrap();
        let t = a.trees[0].key_track(&b, 0, 0).unwrap();
        assert_eq!(t.key_frames, [0, 2, 32]);
        assert_eq!(t.bitmap_raw.len(), 8);
        assert_eq!(t.initial[0], 0.5);
        assert_eq!(t.rows_raw, [1, 2]);
        assert_eq!(t.row(0).unwrap()[0], 0.125);
        assert_eq!(t.row(1).unwrap()[0], -0.25);
        assert!(t.row(2).is_err());
        assert!(a.trees[0].key_track(&b[..265], 0, 0).is_err());
        b[264] = 255;
        assert!(
            a.trees[0]
                .key_track(&b[..1000], 0, 0)
                .unwrap()
                .row(0)
                .is_err()
        );
        b[304..308].copy_from_slice(&f32::NAN.to_le_bytes());
        b[264] = 1;
        assert!(a.trees[0].key_track(&b, 0, 0).unwrap().row(0).is_err());
    }
    #[test]
    fn absolute_key_rows_and_initial_only_paths_are_distinct() {
        let mut b = fixture();
        b.resize(264, 0);
        b[244..246].copy_from_slice(&1u16.to_le_bytes());
        b[252] = 9; // key 3
        b[256..260].copy_from_slice(&0.5f32.to_le_bytes());
        b[260..264].copy_from_slice(&2.0f32.to_le_bytes());
        let a = Animations::parse(&b).unwrap();
        let t = a.trees[0].key_track(&b, 0, 0).unwrap();
        assert_eq!(t.key_frames, [0, 3]);
        assert!(!t.compressed);
        assert_eq!(t.row(0).unwrap()[0], 2.0);
        assert_eq!(t.row(0).unwrap()[6..], [1.0; 3]);
        b[244..246].copy_from_slice(&0x4001u16.to_le_bytes());
        b[252..256].copy_from_slice(&0.75f32.to_le_bytes());
        let a = Animations::parse(&b).unwrap();
        let t = a.trees[0].key_track(&b, 0, 0).unwrap();
        assert_eq!(t.key_frames, [0]);
        assert_eq!(t.initial[0], 0.75);
        assert!(t.bitmap_raw.is_empty());
        assert!(t.rows_raw.is_empty());
        assert!(a.trees[0].key_track(&b, 1, 0).is_err());
        b[244..246].copy_from_slice(&0x1001u16.to_le_bytes());
        let a = Animations::parse(&b).unwrap();
        assert!(a.trees[0].key_track(&b, 0, 0).is_err());
    }
}
