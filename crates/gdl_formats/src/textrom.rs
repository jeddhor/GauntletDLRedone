//! Text ROMs (TEXT/ENGLISH.ROM, SCROLL_E.ROM, HINTS_E.ROM): MB containers
//! with FONT (font names), TEXT (NUL-terminated lines), TOFF (line offsets
//! into TEXT), STRS (per named entry: line count, first line, font index,
//! scale), DEFS/SDEF (entry names: SDEF holds each entry's offset into
//! DEFS) and LIST/LOFF/LDEF (named lists of entries).
//!
//! Layout established from the files themselves (counts and offsets agree
//! across all three ROMs); the consumers (StringTextNum, NewHelp 0x18e710,
//! GetScrollText 0x1019f8) index entries by number.
use crate::{binary::Reader, wad::ParseError};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TextEntry {
    pub name: String,
    pub first_line: u32,
    pub lines: Vec<String>,
    pub font: u32,
    pub scale: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextRom {
    pub fonts: Vec<String>,
    pub entries: Vec<TextEntry>,
    /// LIST {count, start into LOFF} named by LDEF: entry indices per list.
    pub lists: Vec<(String, Vec<usize>)>,
}

impl TextRom {
    pub fn parse(bytes: &[u8]) -> Result<Self, ParseError> {
        let r = Reader { bytes, family: "TEXTROM" };
        r.range(0, 8)?;
        if 1024 * 1024 < bytes.len() {
            return Err(r.error(0, "text ROM size policy exceeded"));
        }
        let (dir, n) = (r.u32(0) as usize, r.u32(4) as usize);
        if 16 < n {
            return Err(r.error(4, "chunk count policy exceeded"));
        }
        r.range(dir, 16 * n)?;
        let chunk = |tag: &[u8; 4]| -> Option<(usize, usize)> {
            (0..n).map(|i| dir + 16 * i).find(|&p| &r.u32(p).to_be_bytes() == tag).map(|p| (r.u32(p + 4) as usize, r.u32(p + 8) as usize))
        };
        let need = |c: Option<(usize, usize)>, t: &str| c.ok_or_else(|| r.error(dir, format!("no {t} chunk")));
        let (text, _) = need(chunk(b"TEXT"), "TEXT")?;
        let (toff, _) = need(chunk(b"TOFF"), "TOFF")?;
        let (strs, count) = need(chunk(b"STRS"), "STRS")?;
        let (defs, _) = need(chunk(b"DEFS"), "DEFS")?;
        let (sdef, _) = need(chunk(b"SDEF"), "SDEF")?;
        let font = chunk(b"FONT");
        if 4096 < count {
            return Err(r.error(strs, "entry count policy exceeded"));
        }
        // TEXT ends at TOFF; TOFF at STRS (chunks are laid out in order).
        let text_end = toff;
        let lines_in_toff = (strs.saturating_sub(toff)) / 4;
        r.range(text, text_end.saturating_sub(text))?;
        let cstr = |at: usize, end: usize| -> Result<String, ParseError> {
            if end <= at {
                return Err(r.error(at, "string outside its chunk"));
            }
            let s = &bytes[at..end];
            let len = s.iter().position(|&c| c == 0).ok_or_else(|| r.error(at, "unterminated string"))?;
            Ok(String::from_utf8_lossy(&s[..len]).into_owned())
        };
        let line = |k: usize| -> Result<String, ParseError> {
            if lines_in_toff <= k {
                return Err(r.error(toff, format!("line {k} beyond TOFF")));
            }
            let o = r.u32(toff + 4 * k) as usize;
            cstr(text + o, text_end)
        };
        r.range(strs, 20 * count)?;
        r.range(sdef, 4 * count)?;
        let mut entries = Vec::with_capacity(count);
        for k in 0..count {
            let e = strs + 20 * k;
            let (n_lines, first) = (r.u32(e) as usize, r.u32(e + 4) as usize);
            if 256 < n_lines {
                return Err(r.error(e, "line count policy exceeded"));
            }
            let lines = (first..first + n_lines).map(line).collect::<Result<Vec<_>, _>>()?;
            let name = cstr(defs + r.u32(sdef + 4 * k) as usize, sdef)?;
            entries.push(TextEntry { name, first_line: first as u32, lines, font: r.u32(e + 8), scale: f32::from_bits(r.u32(e + 12)) });
        }
        let mut lists = Vec::new();
        if let (Some((list, n)), Some((loff, _)), Some((ldef, _))) = (chunk(b"LIST"), chunk(b"LOFF"), chunk(b"LDEF")) {
            if 64 < n {
                return Err(r.error(list, "list count policy exceeded"));
            }
            r.range(list, 8 * n)?;
            r.range(ldef, 4 * n)?;
            for k in 0..n {
                let (c, start) = (r.u32(list + 8 * k) as usize, r.u32(list + 8 * k + 4) as usize);
                r.range(loff + 4 * start, 4 * c)?;
                let idx: Vec<usize> = (0..c).map(|i| r.u32(loff + 4 * (start + i)) as usize).filter(|&i| i < entries.len()).collect();
                let name = cstr(defs + r.u32(ldef + 4 * k) as usize, sdef)?;
                lists.push((name, idx));
            }
        }
        let mut fonts = Vec::new();
        if let Some((f, c)) = font {
            for k in 0..c.min(16) {
                fonts.push(cstr(f + 20 * k, text)?);
            }
        }
        Ok(Self { fonts, entries, lists })
    }

    pub fn list(&self, name: &str) -> Option<&[usize]> {
        self.lists.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_slice())
    }

    pub fn entry(&self, name: &str) -> Option<&TextEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_a_synthetic_rom() {
        // TEXT "A\0B\0C\0" at 0x10, TOFF at 0x18 (3 lines), STRS at 0x24
        // (two entries: 1 line from 0, 2 lines from 1), DEFS "ONE\0TWO\0" at
        // 0x4c, SDEF at 0x54, directory at 0x5c.
        let mut b = vec![0u8; 0x5c + 5 * 16];
        b[0..4].copy_from_slice(&0x5cu32.to_le_bytes());
        b[4..8].copy_from_slice(&5u32.to_le_bytes());
        b[0x10..0x16].copy_from_slice(b"A\0B\0C\0");
        for (k, o) in [0u32, 2, 4].iter().enumerate() {
            b[0x18 + 4 * k..0x1c + 4 * k].copy_from_slice(&o.to_le_bytes());
        }
        for (k, (n, f)) in [(1u32, 0u32), (2, 1)].iter().enumerate() {
            let e = 0x24 + 20 * k;
            b[e..e + 4].copy_from_slice(&n.to_le_bytes());
            b[e + 4..e + 8].copy_from_slice(&f.to_le_bytes());
            b[e + 12..e + 16].copy_from_slice(&1.0f32.to_le_bytes());
        }
        b[0x4c..0x54].copy_from_slice(b"ONE\0TWO\0");
        b[0x54..0x58].copy_from_slice(&0u32.to_le_bytes());
        b[0x58..0x5c].copy_from_slice(&4u32.to_le_bytes());
        for (k, (tag, off, n)) in [(b"TEXT", 0x10u32, 1u32), (b"TOFF", 0x18, 1), (b"STRS", 0x24, 2), (b"DEFS", 0x4c, 1), (b"SDEF", 0x54, 2)].iter().enumerate() {
            let p = 0x5c + 16 * k;
            b[p..p + 4].copy_from_slice(&u32::from_be_bytes(**tag).to_le_bytes());
            b[p + 4..p + 8].copy_from_slice(&off.to_le_bytes());
            b[p + 8..p + 12].copy_from_slice(&n.to_le_bytes());
        }
        let rom = super::TextRom::parse(&b).unwrap();
        assert_eq!(rom.entries.len(), 2);
        assert_eq!(rom.entry("TWO").unwrap().lines, vec!["B", "C"]);
        assert_eq!(rom.entries[0].lines, vec!["A"]);
    }
}
