//! Borrowed, bounded skeletal key payloads. Arithmetic belongs to gdl_core.
use crate::{animation::Tree, binary::Reader, wad::ParseError};

pub const AXIS_FLAGS: [u16; 9] = [1, 2, 4, 16, 32, 64, 256, 512, 1024];
pub const DEFAULT_CHANNELS: [f32; 9] = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0];

/// Key zero is implicit. Later keys correspond to set bitmap bits in increasing
/// frame order. Float rows are absolute; byte rows index one of three delta tables.
pub struct KeyTrack<'a> {
    pub frames: u16,
    pub initial_only: bool,
    pub compressed: bool,
    pub transform_flags: u16,
    pub key_frames: Vec<u16>,
    pub bitmap_file_offset: usize,
    pub bitmap_raw: &'a [u8],
    pub initial_file_offset: usize,
    pub initial_raw: &'a [u8],
    pub initial: [f32; 9],
    pub rows_file_offset: usize,
    pub rows_raw: &'a [u8],
    channels: usize,
    tables: [usize; 3],
    bytes: &'a [u8],
}
impl Tree {
    pub fn key_track<'a>(
        &self,
        bytes: &'a [u8],
        node: u32,
        sequence: u32,
    ) -> Result<KeyTrack<'a>, ParseError> {
        let r = Reader {
            bytes,
            family: "ANIM keys",
        };
        let info = self
            .sequence_infos
            .iter()
            .find(|i| i.node_index == node && i.sequence_index == sequence)
            .ok_or_else(|| r.error(self.blocks_offset as usize, "missing skeletal track"))?;
        let seq = self
            .sequences
            .get(sequence as usize)
            .ok_or_else(|| r.error(self.file_offset as usize, "invalid sequence index"))?;
        let flags = info.transform_flags;
        let known_flags = AXIS_FLAGS.iter().fold(0xe000, |v, bit| v | bit);
        if flags & !known_flags != 0
            || AXIS_FLAGS.iter().filter(|bit| flags & **bit != 0).count() != info.channels as usize
        {
            return Err(r.error(
                self.blocks_offset as usize,
                "unknown flags or channel count mismatch",
            ));
        }
        let start = (self.blocks_offset as usize)
            .checked_add(info.relative_data_offset as usize)
            .ok_or_else(|| r.error(self.blocks_offset as usize, "track offset overflow"))?;
        let frames =
            u16::try_from(seq.frames).map_err(|_| r.error(start, "negative frame count"))?;
        let initial_only = flags & 0x4000 != 0;
        let compressed = flags & 0x2000 != 0;
        let bitmap = r.range(
            start,
            if initial_only {
                0
            } else {
                usize::from(frames).div_ceil(32) * 4
            },
        )?;
        let initial_range = r.range(bitmap.end, info.channels as usize * 4)?;
        let mut initial = DEFAULT_CHANNELS;
        let mut cursor = initial_range.start;
        for (axis, bit) in AXIS_FLAGS.iter().enumerate() {
            if flags & bit != 0 {
                initial[axis] = r.float(cursor)?;
                cursor += 4;
            }
        }
        let mut key_frames = vec![0];
        if !initial_only {
            for frame in 1..frames {
                if bytes[bitmap.start + usize::from(frame) / 8] & (1 << (frame % 8)) != 0 {
                    key_frames.push(frame);
                }
            }
        }
        let size = (key_frames.len() - 1) * info.channels as usize * if compressed { 1 } else { 4 };
        let rows = r.range(initial_range.end, size)?;
        Ok(KeyTrack {
            frames,
            initial_only,
            compressed,
            transform_flags: flags,
            key_frames,
            bitmap_file_offset: bitmap.start,
            bitmap_raw: &bytes[bitmap],
            initial_file_offset: initial_range.start,
            initial_raw: &bytes[initial_range],
            initial,
            rows_file_offset: rows.start,
            rows_raw: &bytes[rows],
            channels: info.channels as usize,
            tables: [
                self.compressed_angles_offset as usize,
                self.compressed_positions_offset as usize,
                self.compressed_scales_offset as usize,
            ],
            bytes,
        })
    }
}
impl<'a> KeyTrack<'a> {
    /// A track outside an atree: world animations (DoWorldAnimSub) give the
    /// key data start, transform flags, channel count, frame count and the
    /// three compressed delta tables directly.
    pub fn from_parts(bytes: &'a [u8], start: usize, flags: u16, channels: u16, frames: u16, tables: [usize; 3]) -> Result<Self, ParseError> {
        let r = Reader { bytes, family: "ANIM keys" };
        let known_flags = AXIS_FLAGS.iter().fold(0xe000, |v, bit| v | bit);
        if flags & !known_flags != 0 || AXIS_FLAGS.iter().filter(|bit| flags & **bit != 0).count() != usize::from(channels) {
            return Err(r.error(start, "unknown flags or channel count mismatch"));
        }
        let initial_only = flags & 0x4000 != 0;
        let compressed = flags & 0x2000 != 0;
        let bitmap = r.range(start, if initial_only { 0 } else { usize::from(frames).div_ceil(32) * 4 })?;
        let initial_range = r.range(bitmap.end, usize::from(channels) * 4)?;
        let mut initial = DEFAULT_CHANNELS;
        let mut cursor = initial_range.start;
        for (axis, bit) in AXIS_FLAGS.iter().enumerate() {
            if flags & bit != 0 {
                initial[axis] = r.float(cursor)?;
                cursor += 4;
            }
        }
        let mut key_frames = vec![0];
        if !initial_only {
            for frame in 1..frames {
                if bytes[bitmap.start + usize::from(frame) / 8] & (1 << (frame % 8)) != 0 {
                    key_frames.push(frame);
                }
            }
        }
        let size = (key_frames.len() - 1) * usize::from(channels) * if compressed { 1 } else { 4 };
        let rows = r.range(initial_range.end, size)?;
        Ok(KeyTrack {
            frames,
            initial_only,
            compressed,
            transform_flags: flags,
            key_frames,
            bitmap_file_offset: bitmap.start,
            bitmap_raw: &bytes[bitmap],
            initial_file_offset: initial_range.start,
            initial_raw: &bytes[initial_range],
            initial,
            rows_file_offset: rows.start,
            rows_raw: &bytes[rows],
            channels: usize::from(channels),
            tables,
            bytes,
        })
    }

    /// Row ordinal 0 describes key_frames[1]. Only present axes are returned;
    /// absent axes use zero deltas or the original absolute defaults.
    pub fn row(&self, ordinal: usize) -> Result<[f32; 9], ParseError> {
        let r = Reader {
            bytes: self.bytes,
            family: "ANIM key row",
        };
        if ordinal >= self.key_frames.len() - 1 {
            return Err(r.error(self.rows_file_offset, "row ordinal out of range"));
        }
        let mut result = if self.compressed {
            [0.0; 9]
        } else {
            DEFAULT_CHANNELS
        };
        let mut p =
            self.rows_file_offset + ordinal * self.channels * if self.compressed { 1 } else { 4 };
        for (axis, bit) in AXIS_FLAGS.iter().enumerate() {
            if self.transform_flags & bit != 0 {
                if self.compressed {
                    let table = self.tables[axis / 3];
                    let lookup = table
                        .checked_add(usize::from(self.bytes[p]) * 4)
                        .ok_or_else(|| r.error(table, "delta lookup overflow"))?;
                    r.range(lookup, 4)?;
                    result[axis] = r.float(lookup)?;
                    p += 1;
                } else {
                    result[axis] = r.float(p)?;
                    p += 4;
                }
            }
        }
        Ok(result)
    }
}
