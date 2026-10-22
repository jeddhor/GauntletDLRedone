//! MPEG/*.PSS and MPEG2/*.PSS movies: an MPEG-2 program stream (pack 0xba,
//! system header 0xbb, MPEG-2 video PES 0xe0, padding 0xbe) whose audio is
//! private stream 1 (0xbd): each payload starts with a 4-byte substream
//! header (ff a1 xx xx on this disc) and the concatenated rest is an
//! SShd/SSbd PS-ADPCM stream (the STREAMS/*.ADS layout). InitMovie's
//! initAll/readMpeg feed both to the IPU and the audio decoder; the video is
//! decoded natively by the host (docs/decisions/0003-movies.md).
use crate::audio::{Ads, AudioError};

type R<T> = Result<T, AudioError>;

fn err<T>(s: impl Into<String>) -> R<T> {
    Err(AudioError(s.into()))
}

/// Byte counts of each stream id seen while walking the program stream.
#[derive(Debug, Default, Clone)]
pub struct PssLayout {
    pub packs: usize,
    pub video_bytes: usize,
    pub audio_bytes: usize,
    pub padding_bytes: usize,
    /// Bytes after the last complete packet (end code 0xb9 included).
    pub trailing: usize,
}

/// Walk the program stream; returns the private-stream-1 payloads (substream
/// headers removed) and the layout. Stops at the program end code 0xb9.
pub fn demux_audio(b: &[u8]) -> R<(Vec<u8>, PssLayout)> {
    let mut at = 0usize;
    let mut audio = Vec::new();
    let mut layout = PssLayout::default();
    while at + 4 <= b.len() {
        if b[at..at + 3] != [0, 0, 1] {
            return err(format!("PSS: no start code at {at:#x}"));
        }
        let id = b[at + 3];
        match id {
            0xb9 => {
                layout.trailing = b.len() - at;
                break;
            }
            0xba => {
                // MPEG-2 pack header: 14 bytes + stuffing (low 3 bits of byte 13).
                let Some(&s) = b.get(at + 13) else { return err("PSS: short pack header") };
                if b[at + 4] >> 6 != 1 {
                    return err(format!("PSS: MPEG-1 pack at {at:#x}"));
                }
                at += 14 + usize::from(s & 7);
                layout.packs += 1;
            }
            _ => {
                let Some(len) = b.get(at + 4..at + 6).map(|l| usize::from(u16::from_be_bytes([l[0], l[1]]))) else {
                    return err("PSS: short packet header");
                };
                let end = at + 6 + len;
                if b.len() < end {
                    return err(format!("PSS: packet {id:#x} at {at:#x} past the end"));
                }
                match id {
                    0xbd => {
                        // MPEG-2 PES header: flags (2), header length (1).
                        let hl = usize::from(*b.get(at + 8).ok_or_else(|| AudioError("PSS: short PES".into()))?);
                        let start = at + 9 + hl;
                        if end < start + 4 {
                            return err(format!("PSS: private packet at {at:#x} without a substream header"));
                        }
                        audio.extend_from_slice(&b[start + 4..end]);
                        layout.audio_bytes += end - start - 4;
                    }
                    0xe0..=0xef => layout.video_bytes += len,
                    0xbe => layout.padding_bytes += len,
                    _ => {}
                }
                at = end;
            }
        }
    }
    Ok((audio, layout))
}

/// The movie's audio as interleaved PCM (frames × channels) with its rate
/// and channel count; None when the stream has no private audio.
pub fn decode_audio(b: &[u8]) -> R<Option<(Vec<i16>, u32, u32)>> {
    let (audio, _) = demux_audio(b)?;
    if audio.is_empty() {
        return Ok(None);
    }
    let ads = Ads::parse(&audio)?;
    Ok(Some((ads.decode(&audio), ads.rate, ads.channels)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> Vec<u8> {
        // MPEG-2 pack header with no stuffing.
        vec![0, 0, 1, 0xba, 0x44, 0, 4, 0, 4, 1, 0, 0, 3, 0xf8]
    }

    fn pes(id: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0, 0, 1, id];
        let len = (3 + payload.len()) as u16;
        v.extend(len.to_be_bytes());
        v.extend([0x81, 0x00, 0x00]);
        v.extend(payload);
        v
    }

    #[test]
    fn private_stream_payloads_form_an_ads_stream() {
        let mut head = vec![0xff, 0xa1, 0, 0];
        head.extend(b"SShd");
        for w in [0x18u32, 0x10, 24000, 1, 0x10, u32::MAX, u32::MAX] {
            head.extend(w.to_le_bytes());
        }
        head.extend(b"SSbd");
        head.extend(32u32.to_le_bytes());
        let mut s = pack();
        s.extend(pes(0xbd, &head));
        s.extend(pes(0xe0, &[1, 2, 3]));
        let mut body = vec![0xff, 0xa1, 0, 1];
        body.extend([0u8; 32]);
        s.extend(pes(0xbd, &body));
        s.extend([0, 0, 1, 0xb9]);
        let (audio, layout) = demux_audio(&s).unwrap();
        assert_eq!(&audio[..4], b"SShd");
        assert_eq!(layout.packs, 1);
        assert_eq!(layout.trailing, 4);
        let (pcm, rate, ch) = decode_audio(&s).unwrap().unwrap();
        assert_eq!((rate, ch), (24000, 1));
        // Two zero ADPCM frames decode to 56 silent samples.
        assert_eq!(pcm.len(), 56);
        assert!(pcm.iter().all(|&v| v == 0));
    }

    #[test]
    fn a_missing_start_code_is_refused() {
        assert!(demux_audio(&[1, 2, 3, 4, 5]).is_err());
    }
}
