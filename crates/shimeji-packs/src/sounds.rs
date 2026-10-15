//! WAV files, identified by RIFF/WAVE magic and parsed here.
//!
//! Only integer PCM (format 1) is accepted. A chunk whose declared size is
//! larger than the bytes still in the file is a bad sound, decided before any
//! buffer is sized from that field. PCM is appended as it is read. The decode
//! budget is charged that byte count, and only when the file contains it.

use std::io::{Read, Seek, SeekFrom};

use super::images::{Budget, MAX_DECODED};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedWav {
    /// Path relative to the pack root, `/`-separated, NFC.
    pub path: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    /// Interleaved little-endian PCM from the `data` chunk.
    pub pcm: Vec<u8>,
}

pub(crate) fn is_wav(header: &[u8]) -> bool {
    header.len() >= 12 && &header[..4] == b"RIFF" && &header[8..12] == b"WAVE"
}

#[derive(Debug)]
pub(crate) enum WavRead {
    Sound(DecodedWav),
    /// Wrong magic, truncated, or not integer PCM. The pack still loads.
    Bad(String),
    /// The PCM payload would exceed the budget. The pack is rejected.
    OverBudget {
        next_bytes: u64,
    },
}

struct Located {
    at: u64,
    len: u64,
}

pub(crate) fn read_wav<R: Read + Seek>(path: &str, mut reader: R, budget: &Budget) -> WavRead {
    let file_len = match reader.seek(SeekFrom::End(0)) {
        Ok(len) => len,
        Err(_) => return bad(path, "is not a WAV"),
    };
    if reader.seek(SeekFrom::Start(0)).is_err() {
        return bad(path, "is not a WAV");
    }
    let mut prefix = [0u8; 12];
    if reader.read_exact(&mut prefix).is_err() || !is_wav(&prefix) {
        return bad(path, "is not a WAV");
    }

    let mut format = None;
    let mut located = None;
    loop {
        let header_at = match reader.stream_position() {
            Ok(pos) => pos,
            Err(_) => return bad(path, "is truncated"),
        };
        if file_len.saturating_sub(header_at) < 8 {
            break;
        }
        let mut chunk = [0u8; 8];
        if reader.read_exact(&mut chunk).is_err() {
            return bad(path, "has a truncated chunk header");
        }
        let id = [chunk[0], chunk[1], chunk[2], chunk[3]];
        let declared = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as u64;
        let payload_at = match reader.stream_position() {
            Ok(pos) => pos,
            Err(_) => return bad(path, "is truncated"),
        };
        let available = file_len.saturating_sub(payload_at);
        // The size field is not a buffer length. A short file that claims a
        // huge chunk is a bad sound, not an allocation.
        if declared > available {
            return WavRead::Bad(format!(
                "{path} chunk declares {declared} bytes but only {available} remain"
            ));
        }
        if &id == b"fmt " {
            if declared < 16 {
                return bad(path, "has a short fmt chunk");
            }
            if format.is_none() {
                let mut fmt = [0u8; 16];
                if reader.read_exact(&mut fmt).is_err() {
                    return bad(path, "has a truncated fmt chunk");
                }
                let audio_format = u16::from_le_bytes([fmt[0], fmt[1]]);
                let channels = u16::from_le_bytes([fmt[2], fmt[3]]);
                let sample_rate = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
                let bits = u16::from_le_bytes([fmt[14], fmt[15]]);
                if audio_format != 1 || channels == 0 || sample_rate == 0 || bits == 0 {
                    return WavRead::Bad(format!(
                        "{path} is not integer PCM (format {audio_format}, {channels} ch, {bits} bits)"
                    ));
                }
                format = Some((channels, sample_rate, bits));
                if !skip_payload(&mut reader, file_len, declared, 16) {
                    return bad(path, "has a truncated fmt chunk");
                }
            } else if !skip_payload(&mut reader, file_len, declared, 0) {
                return bad(path, "has a truncated fmt chunk");
            }
        } else if &id == b"data" && located.is_none() {
            located = Some(Located {
                at: payload_at,
                len: declared,
            });
            if !skip_payload(&mut reader, file_len, declared, 0) {
                return bad(path, "data chunk is truncated");
            }
        } else if !skip_payload(&mut reader, file_len, declared, 0) {
            return bad(path, "is truncated");
        }
    }

    let Some((channels, sample_rate, bits_per_sample)) = format else {
        return bad(path, "has no fmt chunk");
    };
    let Some(span) = located else {
        return bad(path, "has no data chunk");
    };
    if budget.used.saturating_add(span.len) > MAX_DECODED {
        return WavRead::OverBudget {
            next_bytes: span.len,
        };
    }
    if reader.seek(SeekFrom::Start(span.at)).is_err() {
        return bad(path, "data chunk is truncated");
    }
    let Some(pcm) = read_counted(&mut reader, span.len) else {
        return bad(path, "data chunk is truncated");
    };
    WavRead::Sound(DecodedWav {
        path: path.to_string(),
        channels,
        sample_rate,
        bits_per_sample,
        pcm,
    })
}

fn bad(path: &str, reason: &str) -> WavRead {
    WavRead::Bad(format!("{path} {reason}"))
}

/// Grow the buffer from bytes that actually arrive. `len` is the number of
/// bytes the file contained for this chunk, not a header field used on its own.
fn read_counted<R: Read>(reader: &mut R, len: u64) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut left = len;
    let mut buf = [0u8; 8 * 1024];
    while left > 0 {
        let want = usize::try_from(left.min(buf.len() as u64)).ok()?;
        let n = reader.read(&mut buf[..want]).ok()?;
        if n == 0 {
            return None;
        }
        out.extend_from_slice(&buf[..n]);
        left -= u64::try_from(n).ok()?;
    }
    Some(out)
}

fn skip_payload<R: Seek>(reader: &mut R, file_len: u64, declared: u64, already: u64) -> bool {
    if !seek_by(reader, declared.saturating_sub(already)) {
        return false;
    }
    if declared & 1 == 1 {
        let Ok(pos) = reader.stream_position() else {
            return false;
        };
        if pos < file_len && !seek_by(reader, 1) {
            return false;
        }
    }
    true
}

fn seek_by<R: Seek>(reader: &mut R, len: u64) -> bool {
    let Ok(delta) = i64::try_from(len) else {
        return false;
    };
    reader.seek(SeekFrom::Current(delta)).is_ok()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{WavRead, read_wav};
    use crate::images::{Budget, MAX_DECODED};

    fn fmt_pcm() -> [u8; 16] {
        let mut fmt = [0u8; 16];
        fmt[0..2].copy_from_slice(&1u16.to_le_bytes());
        fmt[2..4].copy_from_slice(&1u16.to_le_bytes());
        fmt[4..8].copy_from_slice(&8_000u32.to_le_bytes());
        fmt[8..12].copy_from_slice(&16_000u32.to_le_bytes());
        fmt[12..14].copy_from_slice(&2u16.to_le_bytes());
        fmt[14..16].copy_from_slice(&16u16.to_le_bytes());
        fmt
    }

    fn push_chunk(out: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
        out.extend_from_slice(id);
        out.extend_from_slice(&(u32::try_from(data.len()).unwrap()).to_le_bytes());
        out.extend_from_slice(data);
        if data.len() % 2 == 1 {
            out.push(0);
        }
    }

    fn riff(body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(u32::try_from(body.len()).unwrap()).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(body);
        out
    }

    fn wav_with_data(pcm: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        push_chunk(&mut body, b"JUNK", &[0xAB]);
        push_chunk(&mut body, b"fmt ", &fmt_pcm());
        push_chunk(&mut body, b"data", pcm);
        riff(&body)
    }

    /// Classic 44-byte header plus 4 payload bytes. The `data` size field lies.
    fn lying_wav(declared: u32) -> Vec<u8> {
        let mut body = Vec::new();
        push_chunk(&mut body, b"fmt ", &fmt_pcm());
        body.extend_from_slice(b"data");
        body.extend_from_slice(&declared.to_le_bytes());
        body.extend_from_slice(&[1, 2, 3, 4]);
        let wav = riff(&body);
        assert_eq!(wav.len(), 48);
        wav
    }

    #[test]
    fn reads_integer_pcm_after_an_odd_chunk() {
        let wav = wav_with_data(&[0x11, 0x22, 0x33, 0x44]);
        match read_wav("ok.wav", Cursor::new(wav), &Budget { used: 0, images: 0 }) {
            WavRead::Sound(sound) => {
                assert_eq!(sound.pcm, [0x11, 0x22, 0x33, 0x44]);
                assert_eq!(sound.channels, 1);
                assert_eq!(sound.sample_rate, 8_000);
                assert_eq!(sound.bits_per_sample, 16);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn declared_size_past_the_file_is_bad() {
        let wav = lying_wav(400 * 1024 * 1024);
        match read_wav("lie.wav", Cursor::new(wav), &Budget { used: 0, images: 0 }) {
            WavRead::Bad(message) => {
                assert!(message.contains("declares"), "{message}");
                assert!(message.contains("lie.wav"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn present_pcm_that_exceeds_the_budget_is_refused() {
        let wav = wav_with_data(&[9, 8, 7, 6]);
        let budget = Budget {
            used: MAX_DECODED - 3,
            images: 0,
        };
        match read_wav("tight.wav", Cursor::new(wav), &budget) {
            WavRead::OverBudget { next_bytes } => assert_eq!(next_bytes, 4),
            other => panic!("{other:?}"),
        }
    }
}
