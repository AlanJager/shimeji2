//! Zip import. The central directory is read before any entry is extracted.
//! Declared uncompressed sizes are summed first; a cap failure returns without
//! writing. Extraction still counts real output, because those sizes can lie.
//!
//! Only stored and deflate entries are accepted. Deflate is inflated with
//! `flate2`'s pure-Rust backend (`miniz_oxide`), not the zip crate's writer
//! and not a C zlib. Entry names are decoded in [`super::paths`].
//!
//! Two names that are the same path after NFC and case-folding are rejected
//! in that pass, before anything is written. A case-sensitive disk and a
//! case-insensitive one then refuse the same archive.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use flate2::read::DeflateDecoder;
use zip::CompressionMethod;
use zip::read::ZipArchive;

use super::error::{ImportError, ImportFailure};
use super::fs::create_nofollow;
use super::limits::{CopyBudget, OutputBudget};
use super::paths::{decode_entry_name, decode_entry_text, join_under, safe_parts};

pub(crate) fn extract_zip(zip_path: &Path, dest: &Path, pack: &str) -> Result<(), ImportError> {
    let file = File::open(zip_path).map_err(|error| ImportError::io(pack, error))?;
    let mut archive = ZipArchive::new(file).map_err(|error| zip_error(pack, error))?;
    let mut probe = File::open(zip_path).map_err(|error| ImportError::io(pack, error))?;

    let mut declared = CopyBudget::default();
    let mut plan = Vec::with_capacity(archive.len());
    let mut seen: HashMap<String, SeenEntry> = HashMap::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|error| zip_error(pack, error))?;
        if entry.encrypted() {
            let name = lossy_raw(entry.name_raw());
            return Err(ImportError::new(
                pack,
                ImportFailure::Encrypted { entry: name },
            ));
        }
        let utf8 = marked_utf8(&mut probe, entry.central_header_start())
            .map_err(|error| ImportError::io(pack, error))?;
        let raw_name = entry.name_raw().to_vec();
        let symlink = entry.is_symlink();
        let directory_mode = entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o040000);
        let method = entry.compression();
        let size = entry.size();
        drop(entry);

        let text = decode_entry_text(&raw_name, utf8).map_err(|error| error.with_pack(pack))?;
        let decoded = decode_entry_name(&raw_name, utf8).map_err(|error| error.with_pack(pack))?;
        let safe = safe_parts(&decoded).map_err(|error| error.with_pack(pack))?;
        if symlink {
            return Err(ImportError::symlink(decoded).with_pack(pack));
        }
        let directory = safe.directory || directory_mode;
        let key = safe.parts.join("/").to_lowercase();
        if let Some(previous) = seen.get(&key) {
            // A repeated directory entry is how some zippers list a folder.
            // Two files, or a file and a directory, are not the same pack on
            // every filesystem.
            if !(previous.directory && directory) {
                return Err(ImportError::new(
                    pack,
                    ImportFailure::DuplicateEntry {
                        first: previous.label.clone(),
                        second: text,
                    },
                ));
            }
        } else {
            seen.insert(
                key,
                SeenEntry {
                    label: text,
                    directory,
                },
            );
        }
        let stored = match method {
            CompressionMethod::Stored => true,
            CompressionMethod::Deflated => false,
            other => {
                return Err(ImportError::new(
                    pack,
                    ImportFailure::Compression {
                        entry: decoded,
                        method: other.to_string(),
                    },
                ));
            }
        };
        declared
            .add_entry()
            .map_err(|error| error.with_pack(pack))?;
        if !safe.directory {
            declared
                .add_declared(size)
                .map_err(|error| error.with_pack(pack))?;
        }
        plan.push(Planned {
            index,
            parts: safe.parts,
            directory,
            stored,
        });
    }

    let mut output = OutputBudget::default();
    for item in plan {
        extract_planned(&mut archive, &item, dest, pack, &mut output)?;
    }
    Ok(())
}

struct Planned {
    index: usize,
    parts: Vec<String>,
    directory: bool,
    stored: bool,
}

struct SeenEntry {
    /// Decoded name, before NFC, so the error can name both originals.
    label: String,
    directory: bool,
}

fn extract_planned<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    item: &Planned,
    dest_root: &Path,
    pack: &str,
    output: &mut OutputBudget,
) -> Result<(), ImportError> {
    let dest = join_under(dest_root, &item.parts);
    if item.directory {
        std::fs::create_dir_all(&dest).map_err(|error| ImportError::io(pack, error))?;
        return Ok(());
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|error| ImportError::io(pack, error))?;
    }
    let entry = archive
        .by_index_raw(item.index)
        .map_err(|error| zip_error(pack, error))?;
    let mut file = create_nofollow(&dest).map_err(|error| ImportError::io(pack, error))?;
    if item.stored {
        copy_limited(entry, &mut file, output).map_err(|error| error.with_pack(pack))?;
    } else {
        let decoder = DeflateDecoder::new(entry);
        copy_limited(decoder, &mut file, output).map_err(|error| error.with_pack(pack))?;
    }
    Ok(())
}

fn copy_limited<R: Read>(
    mut reader: R,
    output: &mut File,
    budget: &mut OutputBudget,
) -> Result<(), ImportError> {
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|error| ImportError::io("", error))?;
        if n == 0 {
            return Ok(());
        }
        budget.add(n as u64)?;
        output
            .write_all(&buf[..n])
            .map_err(|error| ImportError::io("", error))?;
    }
}

/// Bit 11 of the general-purpose flag, or an Info-ZIP Unicode Path extra
/// field (`0x7075`). Both are how a zip marks a name as UTF-8. Anything else
/// is CP932.
fn marked_utf8(probe: &mut File, central_start: u64) -> io::Result<bool> {
    probe.seek(SeekFrom::Start(central_start))?;
    let mut fixed = [0u8; 46];
    probe.read_exact(&mut fixed)?;
    if &fixed[0..4] != b"PK\x01\x02" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "central directory signature",
        ));
    }
    let flags = u16::from_le_bytes([fixed[8], fixed[9]]);
    if flags & (1 << 11) != 0 {
        return Ok(true);
    }
    let name_len = u16::from_le_bytes([fixed[28], fixed[29]]) as i64;
    let extra_len = u16::from_le_bytes([fixed[30], fixed[31]]) as usize;
    probe.seek(SeekFrom::Current(name_len))?;
    let mut extra = vec![0u8; extra_len];
    probe.read_exact(&mut extra)?;
    Ok(extra_has_unicode_path(&extra))
}

fn extra_has_unicode_path(mut extra: &[u8]) -> bool {
    while extra.len() >= 4 {
        let id = u16::from_le_bytes([extra[0], extra[1]]);
        let len = u16::from_le_bytes([extra[2], extra[3]]) as usize;
        extra = &extra[4..];
        if extra.len() < len {
            return false;
        }
        if id == 0x7075 {
            return true;
        }
        extra = &extra[len..];
    }
    false
}

fn lossy_raw(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn zip_error(pack: &str, error: zip::result::ZipError) -> ImportError {
    ImportError::new(pack, ImportFailure::Zip(error.to_string()))
}
