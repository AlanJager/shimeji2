use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};

use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;
use zip::ZipArchive;

use super::{
    Budget, MAX_EDGE, MAX_FILE_BYTES, MAX_FRAMES, MAX_JSON_BYTES, MAX_PIXELS, MAX_ZIP_BYTES,
    duration_ms, integer, label,
};
use crate::error::{Result, err};
use crate::imagebuf::Image;
use crate::media::raster::decode_raster;

pub fn decode_zip(data: &[u8], name: &str) -> Result<Value> {
    if data.is_empty() || data.len() > MAX_ZIP_BYTES {
        return Err(err("ZIP 需要在 16 MiB 以内。"));
    }
    zip_directory(data)?;
    let mut archive = ZipArchive::new(Cursor::new(data))
        .map_err(|error| err(format!("ZIP 数据无效：{error}")))?;
    if !(1..=512).contains(&archive.len()) {
        return Err(err("ZIP 最多包含 512 个文件和目录条目。"));
    }
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    let mut all_paths = HashSet::new();
    let mut offsets = HashSet::new();
    let mut total = 0usize;
    let mut file_count = 0usize;
    let mut ignored = 0usize;
    let start = archive.central_directory_start();
    for index in 0..archive.len() {
        let entry = archive
            .by_index_raw(index)
            .map_err(|error| err(format!("ZIP 数据无效：{error}")))?;
        let directory = entry.is_dir();
        let path = zip_path(entry.name(), directory)?;
        let identity = path.nfc().collect::<String>().to_lowercase();
        if !seen.insert(identity.clone()) {
            return Err(err("ZIP 包含重复或大小写冲突的路径。"));
        }
        let mode = entry.unix_mode().unwrap_or(0);
        let kind = mode & 0o170000;
        if !matches!(kind, 0 | 0o100000 | 0o040000) {
            return Err(err("ZIP 不允许符号链接或特殊文件。"));
        }
        if directory {
            if entry.size() != 0 {
                return Err(err("ZIP 目录包含数据。"));
            }
            if os_metadata(&path) {
                ignored += 1;
            }
            all_paths.insert(identity);
            continue;
        }
        if entry.encrypted()
            || !matches!(
                entry.compression(),
                zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
            )
        {
            return Err(err("ZIP 不支持加密或此压缩方式。"));
        }
        let header = entry.header_start();
        if !offsets.insert(header) || header >= start {
            return Err(err("ZIP 文件头位置重叠或越界。"));
        }
        let suffix = std::path::Path::new(&path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let metadata_file = os_metadata(&path);
        if !metadata_file
            && !matches!(
                suffix.as_str(),
                "png" | "apng" | "gif" | "webp" | "jpg" | "jpeg" | "json"
            )
        {
            return Err(err("ZIP 只能包含图片序列，或一份图集 JSON 和对应 PNG。"));
        }
        let limit = if suffix == "json" && !metadata_file {
            MAX_JSON_BYTES
        } else {
            MAX_FILE_BYTES
        };
        let size = entry.size() as usize;
        let compressed = entry.compressed_size().max(1) as usize;
        if size == 0 || size > limit || size > compressed * 100 {
            return Err(err("ZIP 单文件大小或解压比例超过上限。"));
        }
        total += size;
        file_count += 1;
        all_paths.insert(identity);
        if total > 32 * 1024 * 1024 || file_count > 257 {
            return Err(err("ZIP 解压总量或文件数量超过上限。"));
        }
        if metadata_file {
            ignored += 1;
            continue;
        }
        files.push((path, index, size));
    }
    if files.is_empty() {
        return Err(err("ZIP 中没有图片。"));
    }
    let warning = if ignored > 0 {
        vec![format!(
            "已忽略 {ignored} 个 macOS 元数据条目（.DS_Store、__MACOSX 或 ._*）。"
        )]
    } else {
        Vec::new()
    };
    let metadata: Vec<_> = files
        .iter()
        .filter(|(path, _, _)| path.to_ascii_lowercase().ends_with(".json"))
        .cloned()
        .collect();
    let mut budget = Budget {
        frames: 0,
        pixels: 0,
        png_bytes: 0,
    };
    if !metadata.is_empty() {
        if metadata.len() != 1 || files.len() != 2 {
            return Err(err("图集 ZIP 必须只有一份 JSON 与其对应 PNG。"));
        }
        let raw = read_index(&mut archive, metadata[0].1, metadata[0].2)?;
        let document: Value =
            serde_json::from_slice(&raw).map_err(|_| err("图集 JSON 数据损坏。"))?;
        let image_name = basename(super::object(document.get("meta"), "meta")?.get("image"))?;
        let expected = {
            let parent = std::path::Path::new(&metadata[0].0)
                .parent()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            if parent.is_empty() || parent == "." {
                image_name.clone()
            } else {
                format!("{parent}/{image_name}")
            }
        };
        let sheet = files
            .iter()
            .find(|(path, _, _)| path == &expected)
            .ok_or_else(|| err("ZIP 中没有图集 JSON 引用的同目录图片。"))?;
        let sheet_bytes = read_index(&mut archive, sheet.1, sheet.2)?;
        let mut result = decode_atlas(
            &document,
            &sheet_bytes,
            &image_name,
            std::path::Path::new(&metadata[0].0)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&metadata[0].0),
            &mut budget,
        )?;
        if let Some(warnings) = result.get_mut("warnings").and_then(Value::as_array_mut) {
            warnings.extend(warning.into_iter().map(|item| json!(item)));
        }
        return Ok(result);
    }
    files.sort_by(|left, right| {
        natural(&left.0)
            .cmp(&natural(&right.0))
            .then(left.0.cmp(&right.0))
    });
    let mut frames = Vec::new();
    let mut clips = Vec::new();
    let mut notices = warning;
    let mut directories: Vec<(String, Vec<usize>)> = Vec::new();
    for (path, index, size) in files {
        let bytes = read_index(&mut archive, index, size)?;
        let decoded = decode_raster(&bytes, &path, &mut budget)?;
        let first = frames.len();
        let decoded_frames = decoded["frames"].as_array().cloned().unwrap_or_default();
        frames.extend(decoded_frames);
        if let Some(items) = decoded["warnings"].as_array() {
            for item in items {
                if let Some(text) = item.as_str() {
                    notices.push(text.to_string());
                }
            }
        }
        if let Some(items) = decoded
            .get("clips")
            .and_then(Value::as_array)
            .filter(|items| !items.is_empty())
        {
            let clip_name = std::path::Path::new(&path)
                .with_extension("")
                .to_string_lossy()
                .replace('\\', "/");
            for clip in items {
                let mut clip = clip.clone();
                clip["name"] = json!(clip_name);
                if let Some(indices) = clip.get_mut("indices").and_then(Value::as_array_mut) {
                    for index in indices {
                        if let Some(value) = index.as_u64() {
                            *index = json!(first as u64 + value);
                        }
                    }
                }
                clips.push(clip);
            }
        } else {
            let directory = std::path::Path::new(&path)
                .parent()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_else(|| ".".into());
            let directory = if directory.is_empty() {
                ".".into()
            } else {
                directory
            };
            let range = (first..frames.len()).collect::<Vec<_>>();
            if let Some((_, indices)) = directories.iter_mut().find(|(name, _)| name == &directory)
            {
                indices.extend(range);
            } else {
                directories.push((directory, range));
            }
        }
    }
    let mut directory_clips = Vec::new();
    for (directory, indices) in directories {
        let clip_name = if directory == "." {
            label(name)?
        } else {
            directory
        };
        directory_clips.push(json!({"name": clip_name, "indices": indices, "loop": true}));
    }
    directory_clips.extend(clips);
    let mut unique = Vec::new();
    for notice in notices {
        if !unique.contains(&notice) {
            unique.push(notice);
        }
    }
    Ok(
        json!({"format": "zip", "name": label(name)?, "frames": frames, "clips": directory_clips, "warnings": unique}),
    )
}

pub fn decode_atlas(
    metadata: &Value,
    sheet_bytes: &[u8],
    sheet_name: &str,
    atlas_name: &str,
    budget: &mut Budget,
) -> Result<Value> {
    let metadata = json_value(metadata)?;
    let sheet_name = basename(Some(&Value::String(sheet_name.to_string())))?;
    let meta = super::object(metadata.get("meta"), "atlas.meta")?;
    if basename(meta.get("image"))? != sheet_name {
        return Err(err("图集 image 与所选图片文件名不一致。"));
    }
    if signature_kind(sheet_bytes, &sheet_name)? != "PNG" {
        return Err(err("Aseprite 图集需要单张静态 PNG。"));
    }
    let entries = match metadata.get("frames") {
        Some(Value::Object(map)) => map
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect::<Vec<_>>(),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let name = entry
                    .get("filename")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let name = if name.is_empty() {
                    format!("frame-{}", index + 1)
                } else {
                    name
                };
                (name, entry.clone())
            })
            .collect(),
        _ => return Err(err("图集 frames 需要是数组或对象。")),
    };
    if !(1..=MAX_FRAMES).contains(&entries.len()) {
        return Err(err("图集需要包含 1–256 帧。"));
    }
    let sheet = crate::imagebuf::decode_png(sheet_bytes)
        .map_err(|error| err(format!("无法完整解码图集：{error}")))?;
    integer(sheet.width as i64, 1, MAX_EDGE as i64, "图集宽度")?;
    integer(sheet.height as i64, 1, MAX_EDGE as i64, "图集高度")?;
    if sheet.width as usize * sheet.height as usize > MAX_PIXELS {
        return Err(err("图集需要是不超过 16M 像素的单张静态 PNG。"));
    }
    if let Some(size) = meta.get("size") {
        let size = super::object(Some(size), "meta.size")?;
        let declared = (
            integer(
                size.get("w").and_then(Value::as_i64).unwrap_or(-1),
                1,
                MAX_EDGE as i64,
                "meta.size.w",
            )? as u32,
            integer(
                size.get("h").and_then(Value::as_i64).unwrap_or(-1),
                1,
                MAX_EDGE as i64,
                "meta.size.h",
            )? as u32,
        );
        if declared != (sheet.width, sheet.height) {
            return Err(err("图集 meta.size 与实际图片尺寸不一致。"));
        }
    }
    let scale = meta
        .get("scale")
        .map(|value| value.to_string())
        .unwrap_or_else(|| "1".into());
    let scale = scale.trim_matches('"');
    if scale != "1" && scale != "1.0" {
        return Err(err("暂不支持缩放导出的图集，请用 scale=1 导出。"));
    }
    let mut frames = Vec::new();
    let mut notices = Vec::new();
    for (filename, entry) in entries {
        label(&filename)?;
        let entry = super::object(Some(&entry), "frame")?;
        let (x, y, width, height) = rect(entry.get("frame"), "frame", false)?;
        if x + width > sheet.width || y + height > sheet.height {
            return Err(err("图集裁切区域超出图片边界。"));
        }
        let rotated = entry
            .get("rotated")
            .and_then(Value::as_bool)
            .ok_or_else(|| err("rotated/trimmed 需要是布尔值。"))?;
        let trimmed = entry
            .get("trimmed")
            .and_then(Value::as_bool)
            .ok_or_else(|| err("rotated/trimmed 需要是布尔值。"))?;
        let unrotated = if rotated {
            (height, width)
        } else {
            (width, height)
        };
        let source_size = entry.get("sourceSize");
        let (canvas_w, canvas_h) = if let Some(size) = source_size {
            let size = super::object(Some(size), "sourceSize")?;
            (
                integer(
                    size.get("w").and_then(Value::as_i64).unwrap_or(-1),
                    1,
                    MAX_EDGE as i64,
                    "sourceSize.w",
                )? as u32,
                integer(
                    size.get("h").and_then(Value::as_i64).unwrap_or(-1),
                    1,
                    MAX_EDGE as i64,
                    "sourceSize.h",
                )? as u32,
            )
        } else {
            unrotated
        };
        let placement = entry.get("spriteSourceSize");
        let (px, py, pw, ph) = if let Some(placement) = placement {
            rect(Some(placement), "spriteSourceSize", true)?
        } else {
            (0, 0, unrotated.0, unrotated.1)
        };
        let empty = (pw, ph) == (0, 0);
        if (empty && (!trimmed || unrotated != (1, 1)))
            || (!empty && (pw, ph) != unrotated)
            || px + pw > canvas_w
            || py + ph > canvas_h
        {
            return Err(err("spriteSourceSize 与裁切尺寸或源画布不一致。"));
        }
        if trimmed && (source_size.is_none() || entry.get("spriteSourceSize").is_none()) {
            return Err(err("裁切图集需要 sourceSize 与 spriteSourceSize。"));
        }
        if !trimmed && ((px, py) != (0, 0) || (canvas_w, canvas_h) != unrotated) {
            return Err(err("未裁切的帧与 sourceSize 不一致。"));
        }
        budget.reserve(canvas_w, canvas_h, true)?;
        let mut cropped = sheet.crop(x, y, width, height);
        if rotated {
            cropped = cropped.rotate_90_ccw();
        }
        let mut canvas = Image::new(canvas_w, canvas_h);
        if empty {
            if cropped.pixels.chunks_exact(4).any(|pixel| pixel[3] != 0) {
                return Err(err("空帧占位区域包含非透明像素。"));
            }
        } else {
            canvas.paste(&cropped, (px, py));
        }
        let duration = entry.get("duration").and_then(Value::as_f64);
        frames.push(budget.frame(
            &canvas,
            &format!("{}.png", label(&filename)?),
            duration_ms(duration, 100.0)?,
        )?);
    }
    let tags = meta
        .get("frameTags")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    if tags.len() > 64 {
        return Err(err("图集最多支持 64 个动作标签。"));
    }
    let mut clips = Vec::new();
    let mut names = HashSet::new();
    for tag in tags {
        let tag = super::object(Some(tag), "frameTag")?;
        let name = tag.get("name").and_then(Value::as_str).unwrap_or("");
        if name.trim().is_empty() || name.chars().count() > 80 || !names.insert(name.to_string()) {
            return Err(err("图集动作标签名称为空、重复或过长。"));
        }
        let start = integer(
            tag.get("from").and_then(Value::as_i64).unwrap_or(-1),
            0,
            frames.len() as i64 - 1,
            "tag.from",
        )? as usize;
        let end = integer(
            tag.get("to").and_then(Value::as_i64).unwrap_or(-1),
            start as i64,
            frames.len() as i64 - 1,
            "tag.to",
        )? as usize;
        let mut indices: Vec<usize> = (start..=end).collect();
        let direction = tag
            .get("direction")
            .and_then(Value::as_str)
            .unwrap_or("forward");
        if direction == "reverse" || direction == "pingpong_reverse" {
            indices.reverse();
        }
        if direction == "pingpong" || direction == "pingpong_reverse" {
            if indices.len() >= 2 {
                let extra: Vec<usize> = indices
                    .iter()
                    .rev()
                    .skip(1)
                    .take(indices.len() - 2)
                    .cloned()
                    .collect();
                indices.extend(extra);
            }
        } else if direction != "forward" && direction != "reverse" {
            return Err(err("图集动作标签方向不支持。"));
        }
        let repeat = match tag.get("repeat") {
            None => 0,
            Some(Value::String(text))
                if text.chars().all(|char| char.is_ascii_digit()) && text.len() <= 5 =>
            {
                text.parse().unwrap_or(-1)
            }
            Some(value) => value.as_i64().unwrap_or(-1),
        };
        let repeat = integer(repeat, 0, 65535, "tag.repeat")?;
        if repeat > 1 {
            notices.push(format!(
                "动作“{name}”指定有限重复次数，本次按单个周期导入。"
            ));
        }
        clips.push(json!({"name": name, "indices": indices, "loop": repeat == 0}));
    }
    if clips.is_empty() {
        clips.push(json!({"name": label(atlas_name)?, "indices": (0..frames.len()).collect::<Vec<_>>(), "loop": true}));
    }
    let slices = atlas_slices(meta, &mut notices)?;
    Ok(
        json!({"format": "aseprite", "name": label(atlas_name)?, "frames": frames, "clips": clips, "slices": slices, "warnings": notices}),
    )
}

fn atlas_slices(
    meta: &serde_json::Map<String, Value>,
    notices: &mut Vec<String>,
) -> Result<Vec<Value>> {
    let raw = meta
        .get("slices")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if raw.len() > 32 {
        return Err(err("图集切片最多 32 个。"));
    }
    let mut slices = Vec::new();
    let mut patch = false;
    for item in raw {
        let item = super::object(Some(&item), "slice")?;
        let keys = item
            .get("keys")
            .and_then(Value::as_array)
            .filter(|keys| !keys.is_empty())
            .ok_or_else(|| err("切片需要至少一个关键帧。"))?;
        let mut chosen = None;
        for key in keys {
            let key = super::object(Some(key), "slice key")?;
            if key.get("frame").and_then(Value::as_i64).unwrap_or(0) <= 0 {
                chosen = Some(key);
            }
        }
        let Some(chosen) = chosen else { continue };
        let bounds = super::object(chosen.get("bounds"), "slice.bounds")?;
        let x = integer(
            bounds.get("x").and_then(Value::as_i64).unwrap_or(i64::MAX),
            -(MAX_EDGE as i64),
            MAX_EDGE as i64,
            "slice.x",
        )?;
        let y = integer(
            bounds.get("y").and_then(Value::as_i64).unwrap_or(i64::MAX),
            -(MAX_EDGE as i64),
            MAX_EDGE as i64,
            "slice.y",
        )?;
        let width = integer(
            bounds.get("w").and_then(Value::as_i64).unwrap_or(-1),
            0,
            MAX_EDGE as i64,
            "slice.w",
        )?;
        let height = integer(
            bounds.get("h").and_then(Value::as_i64).unwrap_or(-1),
            0,
            MAX_EDGE as i64,
            "slice.h",
        )?;
        if width == 0 || height == 0 {
            continue;
        }
        let mut record = json!({"name": label(item.get("name").and_then(Value::as_str).unwrap_or(""))?, "x": x, "y": y, "width": width, "height": height});
        if let Some(pivot) = chosen.get("pivot") {
            let pivot = super::object(Some(pivot), "pivot")?;
            record["pivotX"] = json!(integer(
                pivot.get("x").and_then(Value::as_i64).unwrap_or(i64::MAX),
                -(MAX_EDGE as i64),
                MAX_EDGE as i64,
                "pivot.x"
            )?);
            record["pivotY"] = json!(integer(
                pivot.get("y").and_then(Value::as_i64).unwrap_or(i64::MAX),
                -(MAX_EDGE as i64),
                MAX_EDGE as i64,
                "pivot.y"
            )?);
        }
        patch |= chosen.get("center").is_some();
        slices.push(record);
    }
    if patch {
        notices.push("九宫格只导入外框，中心缩放留在 Aseprite 中。".into());
    }
    Ok(slices)
}

pub fn read_zip_bytes(data: &[u8], limit: usize, label: &str) -> Result<HashMap<String, Vec<u8>>> {
    let mut archive =
        ZipArchive::new(Cursor::new(data)).map_err(|_| err(format!("{label} 文件无法读取。")))?;
    if !(1..=limit).contains(&archive.len()) {
        return Err(err(format!("{label} 文件条目过多。")));
    }
    let mut files = HashMap::new();
    let mut seen = HashSet::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|_| err(format!("{label} 文件无法读取。")))?;
        if entry.is_dir() {
            continue;
        }
        let path = zip_path(entry.name(), false)?;
        let identity = path.nfc().collect::<String>().to_lowercase();
        if !seen.insert(identity) {
            return Err(err(format!("{label} 文件包含重复路径。")));
        }
        let mode = entry.unix_mode().unwrap_or(0) & 0o170000;
        if !matches!(mode, 0 | 0o100000) {
            return Err(err(format!("{label} 文件包含不受支持的条目。")));
        }
        if entry.encrypted()
            || !matches!(
                entry.compression(),
                zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
            )
        {
            return Err(err(format!("{label} 文件使用了不支持的压缩方式。")));
        }
        let size = entry.size();
        if size == 0
            || size as usize > MAX_FILE_BYTES
            || size > entry.compressed_size().max(1) * 100
        {
            return Err(err(format!("{label} 单个条目超过大小上限。")));
        }
        total += size;
        if total > 32 * 1024 * 1024 {
            return Err(err(format!("{label} 解压总量超过上限。")));
        }
        let mut raw = Vec::new();
        let mut reader = entry.take(size + 1);
        reader
            .read_to_end(&mut raw)
            .map_err(|_| err(format!("{label} 解压大小与声明不一致。")))?;
        if raw.len() as u64 != size {
            return Err(err(format!("{label} 解压大小与声明不一致。")));
        }
        files.insert(path, raw);
    }
    Ok(files)
}

pub fn zip_directory(data: &[u8]) -> Result<()> {
    let lower = data.len().saturating_sub(65557);
    let mut at = rfind_bytes(data, b"PK\x05\x06").filter(|at| *at >= lower);
    while let Some(found) = at {
        if found + 22 <= data.len()
            && found
                + 22
                + u16::from_le_bytes(data[found + 20..found + 22].try_into().unwrap()) as usize
                == data.len()
        {
            break;
        }
        at = rfind_bytes(&data[..found], b"PK\x05\x06").filter(|at| *at >= lower);
    }
    let Some(at) = at.filter(|at| *at >= lower) else {
        return Err(err("ZIP 缺少有效结束目录。"));
    };
    let disk = u16::from_le_bytes(data[at + 4..at + 6].try_into().unwrap());
    let directory_disk = u16::from_le_bytes(data[at + 6..at + 8].try_into().unwrap());
    let disk_count = u16::from_le_bytes(data[at + 8..at + 10].try_into().unwrap());
    let count = u16::from_le_bytes(data[at + 10..at + 12].try_into().unwrap());
    let size = u32::from_le_bytes(data[at + 12..at + 16].try_into().unwrap());
    let offset = u32::from_le_bytes(data[at + 16..at + 20].try_into().unwrap());
    if disk != 0
        || directory_disk != 0
        || count != disk_count
        || !(1..=512).contains(&count)
        || size == 0xffffffff
        || offset == 0xffffffff
    {
        return Err(err("ZIP 不支持分卷、ZIP64 目录或超量条目。"));
    }
    if offset as usize + size as usize != at {
        return Err(err("ZIP 目录边界无效。"));
    }
    let mut position = offset as usize;
    let mut actual = 0usize;
    while position < at {
        actual += 1;
        if actual > 512 || position + 46 > at || &data[position..position + 4] != b"PK\x01\x02" {
            return Err(err("ZIP 目录记录数量或边界无效。"));
        }
        let name =
            u16::from_le_bytes(data[position + 28..position + 30].try_into().unwrap()) as usize;
        let extra =
            u16::from_le_bytes(data[position + 30..position + 32].try_into().unwrap()) as usize;
        let comment =
            u16::from_le_bytes(data[position + 32..position + 34].try_into().unwrap()) as usize;
        position += 46 + name + extra + comment;
        if position > at {
            return Err(err("ZIP 目录记录被截断。"));
        }
    }
    if position != at || actual != count as usize {
        return Err(err("ZIP 目录声明数量不一致。"));
    }
    Ok(())
}

fn rfind_bytes(data: &[u8], needle: &[u8]) -> Option<usize> {
    data.windows(needle.len())
        .rposition(|window| window == needle)
}

fn read_index<R: Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    index: usize,
    size: usize,
) -> Result<Vec<u8>> {
    let entry = archive
        .by_index(index)
        .map_err(|error| err(format!("ZIP 数据无效：{error}")))?;
    let mut raw = Vec::new();
    entry
        .take((size + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|error| err(format!("ZIP 数据无效：{error}")))?;
    if raw.len() != size {
        return Err(err("ZIP 解压大小与声明不一致。"));
    }
    Ok(raw)
}

fn zip_path(name: &str, directory: bool) -> Result<String> {
    if name.is_empty()
        || name.chars().count() > 1024
        || name.contains('\\')
        || name.contains(':')
        || name.starts_with('/')
        || name.chars().any(|char| char.is_control())
    {
        return Err(err("ZIP 包含不安全路径。"));
    }
    let path = if directory && name.ends_with('/') {
        &name[..name.len() - 1]
    } else {
        name
    };
    let components: Vec<_> = path.split('/').collect();
    if components.len() > 32
        || components
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == ".." || part.len() > 255)
    {
        return Err(err("ZIP 包含不安全路径。"));
    }
    Ok(path.to_string())
}

fn os_metadata(path: &str) -> bool {
    let parts: Vec<_> = path.split('/').collect();
    parts
        .iter()
        .any(|part| part.eq_ignore_ascii_case("__macosx"))
        || parts
            .last()
            .is_some_and(|part| part.eq_ignore_ascii_case(".ds_store") || part.starts_with("._"))
}

fn natural(path: &str) -> Vec<(u8, String)> {
    let mut out = Vec::new();
    let mut rest = path;
    while !rest.is_empty() {
        let digits = rest
            .find(|char: char| char.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits > 0 {
            out.push((0, rest[..digits].to_lowercase()));
            rest = &rest[digits..];
        }
        if rest.is_empty() {
            break;
        }
        let end = rest
            .find(|char: char| !char.is_ascii_digit())
            .unwrap_or(rest.len());
        out.push((1, rest[..end].trim_start_matches('0').to_string()));
        rest = &rest[end..];
    }
    out
}

fn basename(value: Option<&Value>) -> Result<String> {
    let Some(value) = value.and_then(Value::as_str) else {
        return Err(err(
            "图集 image 必须是所选图片的准确文件名，不能包含 URL 或路径。",
        ));
    };
    if value.is_empty()
        || value.len() > 255
        || value.contains(['/', '\\', ':'])
        || value == "."
        || value == ".."
        || value.chars().any(|char| char.is_control())
    {
        return Err(err(
            "图集 image 必须是所选图片的准确文件名，不能包含 URL 或路径。",
        ));
    }
    Ok(value.to_string())
}

fn rect(value: Option<&Value>, name: &str, allow_zero: bool) -> Result<(u32, u32, u32, u32)> {
    let value = super::object(value, name)?;
    let low = if allow_zero { 0 } else { 1 };
    let x = integer(
        value.get("x").and_then(Value::as_i64).unwrap_or(-1),
        0,
        MAX_EDGE as i64,
        name,
    )? as u32;
    let y = integer(
        value.get("y").and_then(Value::as_i64).unwrap_or(-1),
        0,
        MAX_EDGE as i64,
        name,
    )? as u32;
    let w = integer(
        value.get("w").and_then(Value::as_i64).unwrap_or(-1),
        low,
        MAX_EDGE as i64,
        name,
    )? as u32;
    let h = integer(
        value.get("h").and_then(Value::as_i64).unwrap_or(-1),
        low,
        MAX_EDGE as i64,
        name,
    )? as u32;
    Ok((x, y, w, h))
}

fn json_value(metadata: &Value) -> Result<Value> {
    let encoded = serde_json::to_vec(metadata).map_err(|_| err("图集 JSON 数据无效。"))?;
    if encoded.len() > MAX_JSON_BYTES {
        return Err(err("图集 JSON 需要在 1 MiB 以内。"));
    }
    Ok(metadata.clone())
}

pub fn signature_kind(data: &[u8], name: &str) -> Result<&'static str> {
    super::raster::signature_public(data, name)
}
