//! PNG files, identified by magic bytes and decoded with the pure-Rust `png` crate.
//!
//! # What rejects the pack, and what is only a diagnostic
//!
//! A bad individual image does not reject the pack. Wrong magic (a JPEG renamed
//! to `.png`), a truncated file, or a PNG the decoder cannot finish becomes a
//! warning diagnostic. The pose keeps whatever [`crate::load_dir`] already
//! recorded, and that one buffer is omitted.
//!
//! Two checks reject the whole pack, before the bytes are decoded:
//!
//! - the IHDR width or height is above [`MAX_IMAGE_EDGE`] (4096)
//! - the expanded RGBA size (or a WAV PCM payload) would push the pack past
//!   [`MAX_DECODED_BYTES`] (512 MiB)
//!
//! The budget is checked before each decode, so a pack of thousands of 4096²
//! PNGs stops when the next image would not fit.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use shimeji_core::diagnostics::{Diagnostic, DiagnosticKind, Severity};
use unicode_normalization::UnicodeNormalization;

use crate::import::error::{ImportError, ImportFailure};
use crate::import::fs::open_nofollow;
use crate::import::limits::{MAX_DECODED_BYTES, MAX_DEPTH, MAX_IMAGE_EDGE};
use crate::sounds::{self, DecodedWav};

pub(crate) const MAX_DECODED: u64 = MAX_DECODED_BYTES;

const PNG_SIG: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Decoded straight RGBA, 8 bits per channel, rows top to bottom.
/// The macOS shell wraps `rgba` as a `CGImage`. Pack bytes never reach ImageIO.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedImage {
    /// Path relative to the pack root, `/`-separated, NFC like resolved poses.
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub(crate) struct DecodedAssets {
    pub images: Vec<DecodedImage>,
    pub sounds: Vec<DecodedWav>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Budget {
    pub used: u64,
    pub images: usize,
}

pub(crate) fn decode_tree(root: &Path, pack: &str) -> Result<DecodedAssets, ImportError> {
    let mut files = Vec::new();
    collect(root, root, 0, pack, &mut files)?;
    files.sort();
    let mut assets = DecodedAssets {
        images: Vec::new(),
        sounds: Vec::new(),
        diagnostics: Vec::new(),
    };
    let mut budget = Budget { used: 0, images: 0 };
    for path in files {
        let relative = match path.strip_prefix(root) {
            Ok(relative) => slash(relative),
            Err(_) => continue,
        };
        decode_file(&path, &relative, pack, &mut budget, &mut assets)?;
    }
    Ok(assets)
}

fn collect(
    root: &Path,
    dir: &Path,
    depth: usize,
    pack: &str,
    out: &mut Vec<std::path::PathBuf>,
) -> Result<(), ImportError> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        Err(error) => return Err(ImportError::io(pack, error)),
    };
    for entry in read {
        let entry = entry.map_err(|error| ImportError::io(pack, error))?;
        let path = entry.path();
        let meta =
            std::fs::symlink_metadata(&path).map_err(|error| ImportError::io(pack, error))?;
        let ft = meta.file_type();
        if ft.is_symlink() {
            return Err(
                ImportError::symlink(slash(path.strip_prefix(root).unwrap_or(&path)))
                    .with_pack(pack),
            );
        }
        if ft.is_dir() {
            collect(root, &path, depth + 1, pack, out)?;
        } else if ft.is_file() {
            out.push(path);
        } else {
            return Err(
                ImportError::special(path.display().to_string(), "special file").with_pack(pack),
            );
        }
    }
    Ok(())
}

fn decode_file(
    path: &Path,
    relative: &str,
    pack: &str,
    budget: &mut Budget,
    assets: &mut DecodedAssets,
) -> Result<(), ImportError> {
    let mut file = match open_nofollow(path) {
        Ok(file) => file,
        Err(error) => return Err(ImportError::io(pack, error)),
    };
    let mut header = [0u8; 16];
    let n = file
        .read_at_start(&mut header)
        .map_err(|error| ImportError::io(pack, error))?;
    let ext_png = ext_is(path, "png");
    let ext_wav = ext_is(path, "wav");
    if n >= 8 && &header[..8] == PNG_SIG {
        return decode_png(file, relative, pack, budget, assets);
    }
    if sounds::is_wav(&header[..n]) {
        return decode_wav(file, relative, pack, budget, assets);
    }
    if ext_png {
        assets
            .diagnostics
            .push(warn(pack, relative, format!("{relative} is not a PNG")));
    } else if ext_wav {
        assets
            .diagnostics
            .push(warn(pack, relative, format!("{relative} is not a WAV")));
    }
    Ok(())
}

fn decode_png(
    mut file: File,
    relative: &str,
    pack: &str,
    budget: &mut Budget,
    assets: &mut DecodedAssets,
) -> Result<(), ImportError> {
    let header = match read_ihdr(&mut file) {
        Ok(header) => header,
        Err(message) => {
            assets
                .diagnostics
                .push(warn(pack, relative, format!("{relative}: {message}")));
            return Ok(());
        }
    };
    if header.width > MAX_IMAGE_EDGE || header.height > MAX_IMAGE_EDGE {
        return Err(ImportError::new(
            pack,
            ImportFailure::ImageDimensions {
                path: relative.to_string(),
                width: header.width,
                height: header.height,
            },
        ));
    }
    let pixels = (header.width as u64)
        .saturating_mul(header.height as u64)
        .saturating_mul(4);
    if budget.used.saturating_add(pixels) > MAX_DECODED_BYTES {
        return Err(ImportError::new(
            pack,
            ImportFailure::DecodeBudget {
                path: relative.to_string(),
                decoded_images: budget.images,
                used_bytes: budget.used,
                next_bytes: pixels,
            },
        ));
    }
    let _ = file.seek(SeekFrom::Start(0));
    match decode_rgba(BufReader::new(file), pixels) {
        Ok((width, height, rgba)) => {
            budget.used = budget.used.saturating_add(rgba.len() as u64);
            budget.images += 1;
            assets.images.push(DecodedImage {
                path: relative.to_string(),
                width,
                height,
                rgba,
            });
            Ok(())
        }
        Err(message) => {
            assets
                .diagnostics
                .push(warn(pack, relative, format!("{relative}: {message}")));
            Ok(())
        }
    }
}

fn decode_wav(
    mut file: File,
    relative: &str,
    pack: &str,
    budget: &mut Budget,
    assets: &mut DecodedAssets,
) -> Result<(), ImportError> {
    let _ = file.seek(SeekFrom::Start(0));
    match sounds::read_wav(relative, &mut file, budget) {
        sounds::WavRead::Sound(sound) => {
            budget.used = budget.used.saturating_add(sound.pcm.len() as u64);
            assets.sounds.push(sound);
            Ok(())
        }
        sounds::WavRead::Bad(message) => {
            assets.diagnostics.push(warn(pack, relative, message));
            Ok(())
        }
        sounds::WavRead::OverBudget { next_bytes } => Err(ImportError::new(
            pack,
            ImportFailure::DecodeBudget {
                path: relative.to_string(),
                decoded_images: budget.images,
                used_bytes: budget.used,
                next_bytes,
            },
        )),
    }
}

struct Ihdr {
    width: u32,
    height: u32,
}

/// IHDR width and height. Header only: 24 bytes, no pixel decode.
/// An edge above [`MAX_IMAGE_EDGE`] is an error, same as the import path.
pub(crate) fn png_pixel_size(path: &Path) -> Result<(u32, u32), String> {
    let mut file = crate::import::fs::open_nofollow(path).map_err(|error| error.to_string())?;
    let header = read_ihdr(&mut file)?;
    if header.width > MAX_IMAGE_EDGE || header.height > MAX_IMAGE_EDGE {
        return Err(format!(
            "PNG is {}x{}, over the {MAX_IMAGE_EDGE}x{MAX_IMAGE_EDGE} cap",
            header.width, header.height
        ));
    }
    Ok((header.width, header.height))
}

fn read_ihdr(file: &mut File) -> Result<Ihdr, String> {
    let _ = file.seek(SeekFrom::Start(0));
    let mut buf = [0u8; 24];
    file.read_exact(&mut buf)
        .map_err(|_| "truncated PNG header".to_string())?;
    if &buf[0..8] != PNG_SIG {
        return Err("not a PNG".to_string());
    }
    if &buf[12..16] != b"IHDR" {
        return Err("PNG is missing IHDR".to_string());
    }
    let width = u32::from_be_bytes([buf[16], buf[17], buf[18], buf[19]]);
    let height = u32::from_be_bytes([buf[20], buf[21], buf[22], buf[23]]);
    if width == 0 || height == 0 {
        return Err("PNG has a zero dimension".to_string());
    }
    Ok(Ihdr { width, height })
}

fn decode_rgba<R: BufRead + Seek>(file: R, rgba_bytes: u64) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = png::Decoder::new(file);
    // The decoder's own buffers are a bit larger than the RGBA output. The
    // header check above is what rejects a huge image; this only stops the
    // crate allocating past a small multiple of the size we already accepted.
    let limits = png::Limits {
        bytes: usize::try_from(rgba_bytes.saturating_mul(2).saturating_add(1 << 20))
            .unwrap_or(usize::MAX),
    };
    decoder.set_limits(limits);
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|error| error.to_string())?;
    let info = reader.info();
    let width = info.width;
    let height = info.height;
    if width > MAX_IMAGE_EDGE || height > MAX_IMAGE_EDGE {
        return Err(format!("PNG is {width}x{height}"));
    }
    let Some(need) = reader.output_buffer_size() else {
        return Err("PNG has no output size".to_string());
    };
    if need as u64 > rgba_bytes.saturating_mul(2) {
        return Err("PNG decoder asked for more memory than the header allows".to_string());
    }
    let mut buffer = vec![0u8; need];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|error| error.to_string())?;
    buffer.truncate(frame.buffer_size());
    let rgba = to_rgba(&buffer, frame.color_type, width, height)?;
    Ok((width, height, rgba))
}

fn to_rgba(data: &[u8], color: png::ColorType, width: u32, height: u32) -> Result<Vec<u8>, String> {
    let pixels = (width as usize).saturating_mul(height as usize);
    let mut rgba = Vec::with_capacity(pixels.saturating_mul(4));
    match color {
        png::ColorType::Rgba => {
            if data.len() < pixels.saturating_mul(4) {
                return Err("PNG RGBA buffer is short".to_string());
            }
            rgba.extend_from_slice(&data[..pixels * 4]);
        }
        png::ColorType::Rgb => {
            if data.len() < pixels.saturating_mul(3) {
                return Err("PNG RGB buffer is short".to_string());
            }
            for chunk in data[..pixels * 3].chunks_exact(3) {
                rgba.extend_from_slice(&[chunk[0], chunk[1], chunk[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            if data.len() < pixels {
                return Err("PNG gray buffer is short".to_string());
            }
            for byte in &data[..pixels] {
                rgba.extend_from_slice(&[*byte, *byte, *byte, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            if data.len() < pixels.saturating_mul(2) {
                return Err("PNG gray-alpha buffer is short".to_string());
            }
            for chunk in data[..pixels * 2].chunks_exact(2) {
                rgba.extend_from_slice(&[chunk[0], chunk[0], chunk[0], chunk[1]]);
            }
        }
        other => return Err(format!("PNG color type {other:?} did not expand")),
    }
    Ok(rgba)
}

fn warn(pack: &str, file: &str, message: String) -> Diagnostic {
    Diagnostic::new(
        Severity::Warning,
        DiagnosticKind::Other,
        pack,
        file,
        None,
        message,
    )
}

fn ext_is(path: &Path, want: &str) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(want))
}

/// Decode one PNG to straight RGBA. Edges above the import cap are an error.
pub(crate) fn decode_png_file(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let mut file = open_nofollow(path).map_err(|error| error.to_string())?;
    let header = read_ihdr(&mut file)?;
    if header.width > MAX_IMAGE_EDGE || header.height > MAX_IMAGE_EDGE {
        return Err(format!(
            "PNG is {}x{}, over the {MAX_IMAGE_EDGE} edge cap",
            header.width, header.height
        ));
    }
    let pixels = u64::from(header.width) * u64::from(header.height);
    let rgba_bytes = pixels.saturating_mul(4);
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let reader = BufReader::new(file);
    decode_rgba(reader, rgba_bytes)
}

/// Encode straight RGBA as an 8-bit PNG.
pub(crate) fn encode_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let expect = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    if rgba.len() < expect || width == 0 || height == 0 {
        return Err("RGBA buffer does not match the image size".to_string());
    }
    let mut encoded = Vec::new();
    let mut encoder = png::Encoder::new(&mut encoded, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    writer
        .write_image_data(&rgba[..expect])
        .map_err(|error| error.to_string())?;
    drop(writer);
    Ok(encoded)
}

/// NFC, whatever the stored form. The shell keys decoded assets by this path
/// and looks them up by NFC pose names.
fn slash(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => {
                Some(part.to_string_lossy().nfc().collect::<String>())
            }
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

trait ReadAtStart {
    fn read_at_start(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
}

impl ReadAtStart for File {
    fn read_at_start(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.seek(SeekFrom::Start(0))?;
        self.read(buf)
    }
}
