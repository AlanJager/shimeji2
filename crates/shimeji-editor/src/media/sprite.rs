use std::collections::{HashMap, HashSet};
use std::io::Cursor;

use flate2::read::ZlibDecoder;
use serde_json::{Value, json};
use std::io::Read;

use super::{
    Budget, MAX_EDGE, MAX_FILE_BYTES, MAX_FRAMES, MAX_JSON_BYTES, MAX_PIXELS, MAX_ZIP_BYTES,
    duration_ms, integer, label,
};
use crate::error::{Result, err};
use crate::imagebuf::Image;
use crate::media::archive::{read_zip_bytes, zip_directory};

pub fn decode_aseprite(data: &[u8], name: &str) -> Result<Value> {
    if data.len() < 128 || data.len() > MAX_FILE_BYTES {
        return Err(ase("文件被截断或超过 8 MiB。"));
    }
    let declared = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let magic = u16::from_le_bytes(data[4..6].try_into().unwrap());
    let frame_count = u16::from_le_bytes(data[6..8].try_into().unwrap()) as usize;
    let width = u16::from_le_bytes(data[8..10].try_into().unwrap()) as u32;
    let height = u16::from_le_bytes(data[10..12].try_into().unwrap()) as u32;
    let depth = u16::from_le_bytes(data[12..14].try_into().unwrap());
    let header_flags = u32::from_le_bytes(data[14..18].try_into().unwrap());
    let speed = u16::from_le_bytes(data[18..20].try_into().unwrap());
    if magic != 0xA5E0 || declared != data.len() {
        return Err(ase("文件签名或长度无效。"));
    }
    if !(1..=MAX_FRAMES).contains(&frame_count) || !matches!(depth, 8 | 16 | 32) {
        return Err(ase("帧数或颜色深度无效。"));
    }
    integer(width as i64, 1, MAX_EDGE as i64, "图片宽度")?;
    integer(height as i64, 1, MAX_EDGE as i64, "图片高度")?;
    if width as usize * height as usize * frame_count > MAX_PIXELS {
        return Err(ase("解码帧合计超过 16M 像素上限。"));
    }
    let transparent = data[28];
    let mut palette = vec![(0u8, 0u8, 0u8, 255u8); 256];
    for (index, entry) in palette.iter_mut().enumerate() {
        if index as u8 == transparent {
            entry.3 = 0;
        }
    }
    let mut layers = Vec::new();
    let mut notices = Vec::new();
    let mut frame_cels = Vec::new();
    let mut durations = Vec::new();
    let mut tags = Vec::new();
    let mut slices = Vec::new();
    let mut warned_tiles = false;
    let mut warned_blend = false;
    let mut warned_patch = false;
    let mut at = 128usize;
    for _frame in 0..frame_count {
        at = take(data, at, 16, data.len(), "帧头")?;
        let frame_bytes = u32::from_le_bytes(data[at - 16..at - 12].try_into().unwrap()) as usize;
        let frame_magic = u16::from_le_bytes(data[at - 12..at - 10].try_into().unwrap());
        let old_chunks = u16::from_le_bytes(data[at - 10..at - 8].try_into().unwrap());
        let duration = u16::from_le_bytes(data[at - 8..at - 6].try_into().unwrap());
        let new_chunks = u32::from_le_bytes(data[at - 4..at].try_into().unwrap());
        if frame_magic != 0xF1FA || frame_bytes < 16 || at - 16 + frame_bytes > data.len() {
            return Err(ase("帧边界无效。"));
        }
        let chunk_count = if new_chunks != 0 {
            new_chunks as usize
        } else if old_chunks == 0xFFFF {
            0
        } else {
            old_chunks as usize
        };
        let frame_end = at - 16 + frame_bytes;
        let mut cels = HashMap::<usize, Cel>::new();
        for _chunk in 0..chunk_count {
            at = take(data, at, 6, frame_end, "块")?;
            let chunk_size = u32::from_le_bytes(data[at - 6..at - 2].try_into().unwrap()) as usize;
            let chunk_type = u16::from_le_bytes(data[at - 2..at].try_into().unwrap());
            if chunk_size < 6 || at - 6 + chunk_size > frame_end {
                return Err(ase("块边界无效。"));
            }
            let chunk_end = at - 6 + chunk_size;
            match chunk_type {
                0x2019 => palette_chunk(data, at, chunk_end, &mut palette)?,
                0x2004 => {
                    let layer = layer_chunk(data, at, chunk_end, header_flags)?;
                    if layer.tilemap && !warned_tiles {
                        notices.push("已跳过瓦片图层，瓦片地图留在 Aseprite 中。".into());
                        warned_tiles = true;
                    }
                    if layer.visible && layer.blend != 0 && !warned_blend {
                        notices.push("非普通混合模式按正常透明度叠加。".into());
                        warned_blend = true;
                    }
                    layers.push(layer);
                }
                0x2005 => {
                    let layer_index = if at + 2 <= chunk_end {
                        Some(u16::from_le_bytes(data[at..at + 2].try_into().unwrap()) as usize)
                    } else {
                        None
                    };
                    let layer = layer_index.and_then(|index| layers.get(index));
                    let (index, cel) =
                        cel_chunk(data, at, chunk_end, depth, &palette, layer, transparent)?;
                    if let Some(cel) = cel {
                        if cels.contains_key(&index) {
                            return Err(ase("同一图层在同一帧出现了两个单元格。"));
                        }
                        cels.insert(index, cel);
                    }
                }
                0x2018 => tags.extend(tag_chunk(data, at, chunk_end, frame_count)?),
                0x2022 => {
                    let (slice_name, flags, keys) = slice_chunk(data, at, chunk_end)?;
                    if flags & 1 != 0 && !warned_patch {
                        notices.push("九宫格只导入外框，中心缩放留在 Aseprite 中。".into());
                        warned_patch = true;
                    }
                    if let Some(chosen) = keys.iter().rev().find(|key| key.frame <= 0) {
                        if chosen.width > 0 && chosen.height > 0 && slices.len() < 32 {
                            let mut record = json!({"name": label(&slice_name)?, "x": chosen.x, "y": chosen.y, "width": chosen.width, "height": chosen.height});
                            if let Some((px, py)) = chosen.pivot {
                                record["pivotX"] = json!(px);
                                record["pivotY"] = json!(py);
                            }
                            slices.push(record);
                        }
                    }
                }
                _ => {}
            }
            at = chunk_end;
        }
        if at != frame_end {
            return Err(ase("帧内块数量与边界不一致。"));
        }
        durations.push(if duration == 0 && (1..=60_000).contains(&speed) {
            speed
        } else if duration == 0 {
            100
        } else {
            duration
        });
        frame_cels.push(cels);
    }
    if at != data.len() {
        return Err(ase("文件尾部存在未声明的数据。"));
    }
    for frame_index in 0..frame_cels.len() {
        let keys: Vec<usize> = frame_cels[frame_index].keys().copied().collect();
        for layer_index in keys {
            let mut seen = HashSet::new();
            loop {
                let Some(cel) = frame_cels[frame_index].get(&layer_index) else {
                    break;
                };
                let Some(link) = cel.link else { break };
                if !seen.insert(link)
                    || link >= frame_cels.len()
                    || !frame_cels[link].contains_key(&layer_index)
                {
                    frame_cels[frame_index].remove(&layer_index);
                    break;
                }
                let opacity = cel.opacity;
                let z = cel.z;
                if frame_cels[link][&layer_index].link.is_some() {
                    let next = frame_cels[link][&layer_index].link;
                    frame_cels[frame_index].get_mut(&layer_index).unwrap().link = next;
                    continue;
                }
                let source = frame_cels[link][&layer_index].clone();
                let mut resolved = source;
                resolved.opacity = opacity;
                resolved.z = z;
                resolved.link = None;
                frame_cels[frame_index].insert(layer_index, resolved);
                break;
            }
        }
    }
    let mut budget = Budget {
        frames: 0,
        pixels: 0,
        png_bytes: 0,
    };
    let mut frames = Vec::new();
    for (index, cels) in frame_cels.iter().enumerate() {
        budget.reserve(width, height, true)?;
        let mut canvas = Image::new(width, height);
        let mut ordered = Vec::new();
        for (layer_index, cel) in cels {
            if cel.image.is_none() || *layer_index >= layers.len() {
                continue;
            }
            let layer = &layers[*layer_index];
            ordered.push((
                (*layer_index as i32) + cel.z,
                cel.z,
                *layer_index,
                cel,
                layer,
            ));
        }
        ordered.sort_by_key(|item| (item.0, item.1, item.2));
        for (_, _, _, cel, layer) in ordered {
            let combined = (layer.opacity as i32 * cel.opacity as i32 + 127) / 255;
            blit(
                &mut canvas,
                cel.image.as_ref().unwrap(),
                cel.x,
                cel.y,
                combined,
            );
        }
        frames.push(budget.frame(
            &canvas,
            &format!("frame-{}.png", index + 1),
            duration_ms(Some(durations[index] as f64), 100.0)?,
        )?);
    }
    let mut clips = Vec::new();
    let mut names = HashSet::new();
    for (tag_name, start, stop, direction, repeat) in tags {
        let tag_label = label(&tag_name)?;
        if tag_label.is_empty() || !names.insert(tag_label.clone()) {
            return Err(ase("动作标签名称为空或重复。"));
        }
        if repeat > 1 {
            notices.push(format!(
                "动作“{tag_label}”指定有限重复次数，本次按单个周期导入。"
            ));
        }
        clips.push(json!({"name": tag_label, "indices": directions(start, stop, direction), "loop": repeat == 0}));
    }
    if clips.is_empty() {
        clips.push(json!({"name": label(name)?, "indices": (0..frame_count).collect::<Vec<_>>(), "loop": true}));
    }
    Ok(
        json!({"format": "aseprite", "name": label(name)?, "frames": frames, "clips": clips, "slices": slices, "warnings": notices}),
    )
}

#[derive(Clone)]
struct Cel {
    image: Option<Image>,
    x: i32,
    y: i32,
    opacity: u8,
    z: i32,
    link: Option<usize>,
}

struct Layer {
    visible: bool,
    tilemap: bool,
    background: bool,
    blend: u16,
    opacity: u8,
}

struct SliceKey {
    frame: i32,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    pivot: Option<(i32, i32)>,
}

fn directions(start: usize, end: usize, direction: u8) -> Vec<usize> {
    let mut indices: Vec<usize> = (start..=end).collect();
    if direction == 1 || direction == 3 {
        indices.reverse();
    }
    if (direction == 2 || direction == 3) && indices.len() >= 2 {
        let extra: Vec<usize> = indices
            .iter()
            .rev()
            .skip(1)
            .take(indices.len() - 2)
            .cloned()
            .collect();
        indices.extend(extra);
    }
    indices
}

fn palette_chunk(
    data: &[u8],
    mut at: usize,
    end: usize,
    palette: &mut [(u8, u8, u8, u8)],
) -> Result<()> {
    at = take(data, at, 20, end, "调色板")?;
    let first = u32::from_le_bytes(data[at - 16..at - 12].try_into().unwrap());
    let last = u32::from_le_bytes(data[at - 12..at - 8].try_into().unwrap());
    if first > last || last >= 256 {
        return Err(ase("调色板范围无效。"));
    }
    for index in first..=last {
        at = take(data, at, 6, end, "调色板")?;
        let flags = u16::from_le_bytes(data[at - 6..at - 4].try_into().unwrap());
        palette[index as usize] = (data[at - 4], data[at - 3], data[at - 2], data[at - 1]);
        if flags & 1 != 0 {
            let (_, next) = text(data, at, end)?;
            at = next;
        }
    }
    Ok(())
}

fn layer_chunk(data: &[u8], mut at: usize, end: usize, header_flags: u32) -> Result<Layer> {
    at = take(data, at, 16, end, "图层")?;
    let flags = u16::from_le_bytes(data[at - 16..at - 14].try_into().unwrap());
    let kind = u16::from_le_bytes(data[at - 14..at - 12].try_into().unwrap());
    let blend = u16::from_le_bytes(data[at - 6..at - 4].try_into().unwrap());
    let opacity = data[at - 4];
    let (_, mut next) = text(data, at, end)?;
    if kind == 2 && next + 4 <= end {
        next += 4;
    }
    if header_flags & 4 != 0 && next + 16 <= end {
        next += 16;
    }
    let _ = next;
    Ok(Layer {
        visible: flags & 1 != 0 && flags & 64 == 0 && kind == 0,
        tilemap: kind == 2,
        background: flags & 8 != 0,
        blend,
        opacity: if header_flags & 1 != 0 { opacity } else { 255 },
    })
}

fn cel_chunk(
    data: &[u8],
    mut at: usize,
    end: usize,
    depth: u16,
    palette: &[(u8, u8, u8, u8)],
    layer: Option<&Layer>,
    transparent: u8,
) -> Result<(usize, Option<Cel>)> {
    at = take(data, at, 16, end, "单元格")?;
    let layer_index = u16::from_le_bytes(data[at - 16..at - 14].try_into().unwrap()) as usize;
    let x = i16::from_le_bytes(data[at - 14..at - 12].try_into().unwrap()) as i32;
    let y = i16::from_le_bytes(data[at - 12..at - 10].try_into().unwrap()) as i32;
    let opacity = data[at - 10];
    let cel_type = u16::from_le_bytes(data[at - 9..at - 7].try_into().unwrap());
    let z_index = i16::from_le_bytes(data[at - 7..at - 5].try_into().unwrap()) as i32;
    if cel_type == 1 {
        at = take(data, at, 2, end, "链接单元格")?;
        let link = u16::from_le_bytes(data[at - 2..at].try_into().unwrap()) as usize;
        return Ok((
            layer_index,
            Some(Cel {
                image: None,
                x,
                y,
                opacity,
                z: z_index,
                link: Some(link),
            }),
        ));
    }
    if cel_type == 3 || layer.is_none_or(|layer| !layer.visible) {
        return Ok((layer_index, None));
    }
    if cel_type != 0 && cel_type != 2 {
        return Err(ase("单元格类型无效。"));
    }
    at = take(data, at, 4, end, "单元格")?;
    let cel_width = u16::from_le_bytes(data[at - 4..at - 2].try_into().unwrap()) as u32;
    let cel_height = u16::from_le_bytes(data[at - 2..at].try_into().unwrap()) as u32;
    let channels = match depth {
        32 => 4,
        16 => 2,
        _ => 1,
    };
    let expected = cel_width as usize * cel_height as usize * channels;
    if cel_width == 0
        || cel_height == 0
        || cel_width > MAX_EDGE
        || cel_height > MAX_EDGE
        || expected > MAX_PIXELS * channels
    {
        return Err(ase("单元格尺寸无效。"));
    }
    let raw = if cel_type == 0 {
        data[at..end].to_vec()
    } else {
        inflate(&data[at..end], expected)?
    };
    if raw.len() != expected {
        return Err(ase("单元格像素大小不一致。"));
    }
    let background = layer.is_some_and(|layer| layer.background);
    let image = rgba_image(
        &raw,
        cel_width,
        cel_height,
        depth,
        palette,
        transparent,
        background,
    )?;
    Ok((
        layer_index,
        Some(Cel {
            image: Some(image),
            x,
            y,
            opacity,
            z: z_index,
            link: None,
        }),
    ))
}

type AseTag = (String, usize, usize, u8, u16);

fn tag_chunk(data: &[u8], mut at: usize, end: usize, frame_count: usize) -> Result<Vec<AseTag>> {
    at = take(data, at, 10, end, "标签")?;
    let count = u16::from_le_bytes(data[at - 10..at - 8].try_into().unwrap());
    let mut found = Vec::new();
    for _ in 0..count {
        at = take(data, at, 17, end, "标签")?;
        let start = u16::from_le_bytes(data[at - 17..at - 15].try_into().unwrap()) as usize;
        let stop = u16::from_le_bytes(data[at - 15..at - 13].try_into().unwrap()) as usize;
        let direction = data[at - 13];
        let repeat = u16::from_le_bytes(data[at - 12..at - 10].try_into().unwrap());
        let (tag_name, next) = text(data, at, end)?;
        at = next;
        if direction > 3 || start > stop || stop >= frame_count {
            return Err(ase("动作标签范围或方向无效。"));
        }
        found.push((tag_name, start, stop, direction, repeat));
    }
    Ok(found)
}

fn slice_chunk(data: &[u8], mut at: usize, end: usize) -> Result<(String, u32, Vec<SliceKey>)> {
    at = take(data, at, 12, end, "切片")?;
    let count = u32::from_le_bytes(data[at - 12..at - 8].try_into().unwrap());
    let flags = u32::from_le_bytes(data[at - 8..at - 4].try_into().unwrap());
    let (slice_name, mut next) = text(data, at, end)?;
    at = next;
    let mut keys = Vec::new();
    for _ in 0..count {
        at = take(data, at, 20, end, "切片")?;
        let frame = i32::from_le_bytes(data[at - 16..at - 12].try_into().unwrap());
        let x = i32::from_le_bytes(data[at - 12..at - 8].try_into().unwrap());
        let y = i32::from_le_bytes(data[at - 8..at - 4].try_into().unwrap());
        let width = u32::from_le_bytes(data[at - 4..at].try_into().unwrap());
        // The python layout is `<IllII`: frame i32, x i32, y i32, w u32, h u32 = 4+4+4+4+4 = 20.
        // I consumed 16 bytes for frame/x/y and then width from the last 4, missing height.
        let _ = (frame, x, y, width);
        next = at;
        let frame = i32::from_le_bytes(data[at - 20..at - 16].try_into().unwrap());
        let x = i32::from_le_bytes(data[at - 16..at - 12].try_into().unwrap());
        let y = i32::from_le_bytes(data[at - 12..at - 8].try_into().unwrap());
        let slice_width = u32::from_le_bytes(data[at - 8..at - 4].try_into().unwrap());
        let slice_height = u32::from_le_bytes(data[at - 4..at].try_into().unwrap());
        let mut pivot = None;
        if flags & 1 != 0 {
            at = take(data, at, 16, end, "九宫格")?;
        }
        if flags & 2 != 0 {
            at = take(data, at, 8, end, "锚点")?;
            pivot = Some((
                i32::from_le_bytes(data[at - 8..at - 4].try_into().unwrap()),
                i32::from_le_bytes(data[at - 4..at].try_into().unwrap()),
            ));
        }
        keys.push(SliceKey {
            frame,
            x,
            y,
            width: slice_width,
            height: slice_height,
            pivot,
        });
        let _ = next;
    }
    Ok((slice_name, flags, keys))
}

fn rgba_image(
    raw: &[u8],
    width: u32,
    height: u32,
    depth: u16,
    palette: &[(u8, u8, u8, u8)],
    transparent: u8,
    background: bool,
) -> Result<Image> {
    let mut pixels = vec![0; width as usize * height as usize * 4];
    if depth == 32 {
        pixels.copy_from_slice(raw);
    } else if depth == 16 {
        for (index, pair) in raw.chunks_exact(2).enumerate() {
            pixels[index * 4..index * 4 + 4].copy_from_slice(&[pair[0], pair[0], pair[0], pair[1]]);
        }
    } else {
        for (index, entry) in raw.iter().enumerate() {
            let (red, green, blue, mut alpha) = palette[*entry as usize];
            if !background && *entry == transparent {
                alpha = 0;
            }
            pixels[index * 4..index * 4 + 4].copy_from_slice(&[red, green, blue, alpha]);
            let _ = red;
        }
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}

fn blit(canvas: &mut Image, image: &Image, x: i32, y: i32, opacity: i32) {
    if opacity <= 0 {
        return;
    }
    let mut image = image.clone();
    if opacity < 255 {
        image.scale_alpha(opacity as f64 / 255.0);
    }
    let left = x.max(0);
    let top = y.max(0);
    let right = (x + image.width as i32).min(canvas.width as i32);
    let bottom = (y + image.height as i32).min(canvas.height as i32);
    if left >= right || top >= bottom {
        return;
    }
    let cropped = image.crop(
        (left - x) as u32,
        (top - y) as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    );
    canvas.alpha_composite(&cropped, (left, top));
}

fn inflate(payload: &[u8], expected: usize) -> Result<Vec<u8>> {
    if expected == 0 || expected > MAX_PIXELS * 4 {
        return Err(ase("图像尺寸无效。"));
    }
    let mut decoder = ZlibDecoder::new(payload);
    let mut raw = Vec::new();
    std::io::Read::take(&mut decoder, (expected + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|_| ase("像素数据无法解压。"))?;
    if raw.len() != expected || decoder.total_in() as usize != payload.len() {
        return Err(ase("像素数据大小与图像不一致。"));
    }
    Ok(raw)
}

fn text(data: &[u8], mut at: usize, end: usize) -> Result<(String, usize)> {
    at = take(data, at, 2, end, "名称")?;
    let length = u16::from_le_bytes(data[at - 2..at].try_into().unwrap()) as usize;
    if length > 1024 {
        return Err(ase("名称过长。"));
    }
    at = take(data, at, length, end, "名称")?;
    let bytes = &data[at - length..at];
    let text = std::str::from_utf8(bytes).map_err(|_| ase("名称不是有效的 UTF-8。"))?;
    if text.chars().any(|char| char.is_control()) {
        return Err(ase("名称包含控制字符。"));
    }
    Ok((text.to_string(), at))
}

fn take(data: &[u8], at: usize, size: usize, end: usize, label: &str) -> Result<usize> {
    if at + size > end || at + size > data.len() {
        return Err(ase(&format!("{label}被截断。")));
    }
    Ok(at + size)
}

fn ase(message: &str) -> crate::error::Error {
    err(format!("Aseprite {message}"))
}

pub fn decode_piskel(data: &[u8], name: &str) -> Result<Value> {
    if data.is_empty() || data.len() > MAX_FILE_BYTES {
        return Err(err("Piskel 文件被截断或超过 8 MiB。"));
    }
    let document: Value = serde_json::from_slice(data).map_err(|_| err("Piskel 文件无法读取。"))?;
    if document.get("modelVersion") != Some(&json!(2)) {
        return Err(err("Piskel 只支持 modelVersion 2。"));
    }
    let body = super::object(document.get("piskel"), "Piskel")?;
    let width = integer(
        json_i64(body.get("width")).unwrap_or(-1),
        1,
        MAX_EDGE as i64,
        "Piskel 宽度",
    )? as u32;
    let height = integer(
        json_i64(body.get("height")).unwrap_or(-1),
        1,
        MAX_EDGE as i64,
        "Piskel 高度",
    )? as u32;
    let fps = match body.get("fps") {
        None => 12.0,
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    };
    if !fps.is_finite() || !(0.0..=120.0).contains(&fps) || fps == 0.0 {
        return Err(err("Piskel 帧率无效。"));
    }
    let duration = duration_ms(Some((1000.0 / fps).round()), 100.0)?;
    let raw_layers = body
        .get("layers")
        .and_then(Value::as_array)
        .filter(|layers| (1..=64).contains(&layers.len()))
        .ok_or_else(|| err("Piskel 图层无效。"))?;
    let empty_hidden = Vec::new();
    let hidden = match body.get("hiddenFrames") {
        None => &empty_hidden,
        Some(Value::Array(hidden)) => hidden,
        Some(_) => return Err(err("Piskel 隐藏帧无效。")),
    };
    let mut layers = Vec::new();
    for item in raw_layers {
        let Some(raw) = item.as_str() else {
            return Err(err("Piskel 图层无效。"));
        };
        layers.push(piskel_layer(raw, width, height)?);
    }
    let count = layers[0].len();
    if layers.iter().any(|layer| layer.len() != count) {
        return Err(err("Piskel 各图层帧数不一致。"));
    }
    let mut hidden_set = HashSet::new();
    for item in hidden {
        if let Some(index) = item.as_i64() {
            if (0..count as i64).contains(&index) {
                hidden_set.insert(index as usize);
            }
        }
    }
    let visible: Vec<usize> = (0..count)
        .filter(|index| !hidden_set.contains(index))
        .collect();
    if visible.is_empty() {
        return Err(err("Piskel 没有可见帧。"));
    }
    let mut budget = Budget {
        frames: 0,
        pixels: 0,
        png_bytes: 0,
    };
    let mut frames = Vec::new();
    for index in visible {
        budget.reserve(width, height, true)?;
        let mut canvas = Image::new(width, height);
        for layer in &layers {
            blit(&mut canvas, &layer[index].0, 0, 0, layer[index].1 as i32);
        }
        frames.push(budget.frame(
            &canvas,
            &format!("frame-{}.png", frames.len() + 1),
            duration,
        )?);
    }
    let title = body
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .map(label)
        .transpose()?
        .unwrap_or(label(name)?);
    Ok(
        json!({"format": "piskel", "name": title, "frames": frames, "clips": [{"name": title, "indices": (0..frames.len()).collect::<Vec<_>>(), "loop": true}], "slices": [], "warnings": []}),
    )
}

fn piskel_layer(raw: &str, width: u32, height: u32) -> Result<Vec<(Image, u8)>> {
    if raw.is_empty() || raw.len() > MAX_FILE_BYTES {
        return Err(err("Piskel 图层无效。"));
    }
    let layer: Value = serde_json::from_str(raw).map_err(|_| err("Piskel 图层无效。"))?;
    let layer = super::object(Some(&layer), "Piskel 图层")?;
    let mut cells: Vec<(usize, Image)> = Vec::new();
    if let Some(chunks) = layer
        .get("chunks")
        .and_then(Value::as_array)
        .filter(|chunks| !chunks.is_empty())
    {
        for chunk in chunks {
            let chunk = super::object(Some(chunk), "Piskel 图层")?;
            let sheet = png_data_url(chunk.get("base64PNG"))?;
            cells.extend(piskel_cells(&sheet, chunk.get("layout"), width, height)?);
        }
    } else {
        let count = integer(
            json_i64(layer.get("frameCount")).unwrap_or(-1),
            1,
            MAX_FRAMES as i64,
            "Piskel 帧数",
        )? as usize;
        let sheet = png_data_url(layer.get("base64PNG"))?;
        let layout = Value::Array((0..count).map(|index| json!([index])).collect());
        cells.extend(piskel_cells(&sheet, Some(&layout), width, height)?);
    }
    let mut frames = HashMap::new();
    for (index, cell) in cells {
        if frames.contains_key(&index) {
            return Err(err("Piskel 帧编号重复。"));
        }
        frames.insert(index, cell);
    }
    let count = integer(
        json_i64(layer.get("frameCount")).unwrap_or(frames.len() as i64),
        1,
        MAX_FRAMES as i64,
        "Piskel 帧数",
    )? as usize;
    if frames.len() != count || (0..count).any(|index| !frames.contains_key(&index)) {
        return Err(err("Piskel 帧不完整。"));
    }
    let opacity = unit_byte(
        layer.get("opacity").cloned().unwrap_or(json!(1)),
        "Piskel 图层透明度",
    )?;
    Ok((0..count)
        .map(|index| (frames.remove(&index).unwrap(), opacity))
        .collect())
}

fn piskel_cells(
    image: &Image,
    layout: Option<&Value>,
    width: u32,
    height: u32,
) -> Result<Vec<(usize, Image)>> {
    let Some(layout) = layout.and_then(Value::as_array) else {
        return Err(err("Piskel 图层排列无效。"));
    };
    if layout.is_empty()
        || layout
            .iter()
            .any(|row| row.as_array().is_none_or(|row| row.is_empty()))
    {
        return Err(err("Piskel 图层排列无效。"));
    }
    let columns = layout.len();
    let rows = layout[0].as_array().unwrap().len();
    if layout
        .iter()
        .any(|row| row.as_array().unwrap().len() != rows)
        || columns * rows > MAX_FRAMES
    {
        return Err(err("Piskel 图层排列无效。"));
    }
    if image.width != width * columns as u32 || image.height != height * rows as u32 {
        return Err(err("Piskel 图层尺寸与画布不一致。"));
    }
    let mut cells = Vec::new();
    for (column, row_indices) in layout.iter().enumerate() {
        for (row, index) in row_indices.as_array().unwrap().iter().enumerate() {
            let Some(index) = index
                .as_i64()
                .filter(|index| (0..MAX_FRAMES as i64).contains(index))
            else {
                return Err(err("Piskel 帧编号无效。"));
            };
            cells.push((
                index as usize,
                image.crop(column as u32 * width, row as u32 * height, width, height),
            ));
        }
    }
    Ok(cells)
}

fn png_data_url(value: Option<&Value>) -> Result<Image> {
    let Some(value) = value.and_then(Value::as_str) else {
        return Err(err("Piskel 图层图片无效。"));
    };
    if !value.starts_with("data:image/png;base64,") || value.len() > MAX_FILE_BYTES {
        return Err(err("Piskel 图层图片无效。"));
    }
    let raw = crate::imagebuf::decode_data_url(value).map_err(|_| err("Piskel 图层图片无效。"))?;
    if !raw.starts_with(b"\x89PNG\r\n\x1a\n") || raw.len() > MAX_FILE_BYTES {
        return Err(err("Piskel 图层图片无效。"));
    }
    crate::imagebuf::decode_png(&raw).map_err(|_| err("Piskel 图层图片无效。"))
}

pub fn decode_pixelorama(data: &[u8], name: &str) -> Result<Value> {
    if !(data.starts_with(b"PK\x03\x04") || data.starts_with(b"PK\x05\x06")) {
        return Err(err("Pixelorama 0.x 工程请用 1.x 重新保存后再导入。"));
    }
    if data.len() > MAX_ZIP_BYTES {
        return Err(err("Pixelorama 文件需要在 16 MiB 以内。"));
    }
    zip_directory(data)?;
    let files = read_zip_bytes(data, 2048, "Pixelorama")?;
    let raw = files
        .get("data.json")
        .ok_or_else(|| err("Pixelorama 工程缺少 data.json。"))?;
    if raw.len() > MAX_JSON_BYTES {
        return Err(err("Pixelorama 工程缺少 data.json。"));
    }
    let document: Value =
        serde_json::from_slice(raw).map_err(|_| err("Pixelorama 工程无法读取。"))?;
    let document = super::object(Some(&document), "Pixelorama")?;
    if document
        .get("color_mode")
        .and_then(Value::as_i64)
        .unwrap_or(5)
        != 5
    {
        return Err(err("Pixelorama 索引色或非 RGBA 工程请先导出 PNG。"));
    }
    let width = integer(
        json_i64(document.get("size_x")).unwrap_or(-1),
        1,
        MAX_EDGE as i64,
        "Pixelorama 宽度",
    )? as u32;
    let height = integer(
        json_i64(document.get("size_y")).unwrap_or(-1),
        1,
        MAX_EDGE as i64,
        "Pixelorama 高度",
    )? as u32;
    let fps = match document.get("fps") {
        None => 6.0,
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        _ => f64::NAN,
    };
    if !fps.is_finite() || !(0.0..=120.0).contains(&fps) || fps == 0.0 {
        return Err(err("Pixelorama 帧率无效。"));
    }
    let layers = document
        .get("layers")
        .and_then(Value::as_array)
        .filter(|l| (1..=64).contains(&l.len()))
        .ok_or_else(|| err("Pixelorama 图层或帧无效。"))?;
    let frames_in = document
        .get("frames")
        .and_then(Value::as_array)
        .filter(|f| !f.is_empty())
        .ok_or_else(|| err("Pixelorama 图层或帧无效。"))?;
    let mut blend_warned = false;
    let mut tile_warned = false;
    let mut other_warned = false;
    let mut budget = Budget {
        frames: 0,
        pixels: 0,
        png_bytes: 0,
    };
    let mut output = Vec::new();
    let mut notices: Vec<String> = Vec::new();
    for (frame_number, frame) in frames_in.iter().enumerate() {
        let frame = super::object(Some(frame), "Pixelorama 帧")?;
        let cels = frame
            .get("cels")
            .and_then(Value::as_array)
            .filter(|cels| cels.len() == layers.len())
            .ok_or_else(|| err("Pixelorama 单元格数量与图层不一致。"))?;
        let span = match frame.get("duration") {
            None => 1.0,
            Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
            _ => f64::NAN,
        };
        if !span.is_finite() || !(0.0..=600.0).contains(&span) || span == 0.0 {
            return Err(err("Pixelorama 帧时长无效。"));
        }
        budget.reserve(width, height, true)?;
        let mut canvas = Image::new(width, height);
        let mut painters = Vec::new();
        for (index, layer) in layers.iter().enumerate() {
            let layer = super::object(Some(layer), "Pixelorama 图层")?;
            let kind = match layer.get("type") {
                None => 0,
                Some(Value::Number(number)) => number
                    .as_i64()
                    .ok_or_else(|| err("Pixelorama 图层类型无效。"))?,
                _ => return Err(err("Pixelorama 图层类型无效。")),
            };
            if kind == 3 {
                if !tile_warned {
                    notices.push("已跳过瓦片图层。".into());
                    tile_warned = true;
                }
                continue;
            }
            if kind != 0 || layer.get("visible") == Some(&json!(false)) {
                if matches!(kind, 1 | 2 | 4) && !other_warned {
                    notices.push("已跳过分组、三维或音频图层。".into());
                    other_warned = true;
                }
                continue;
            }
            if !matches!(
                layer.get("blend_mode").and_then(Value::as_i64),
                None | Some(0)
            ) && !blend_warned
            {
                notices.push("非普通混合模式按正常透明度叠加。".into());
                blend_warned = true;
            }
            let cel = super::object(cels.get(index), "Pixelorama 单元格")?;
            let z_index = cel.get("z_index").and_then(Value::as_i64).unwrap_or(0);
            if !(-4096..=4096).contains(&z_index) {
                return Err(err("Pixelorama 单元格顺序无效。"));
            }
            painters.push((index as i64 + z_index, z_index, index, layer, cel));
        }
        painters.sort_by_key(|item| (item.0, item.1, item.2));
        for (_, _, index, layer, cel) in painters {
            let (image_width, image_height) = if cel.get("image_size").is_some() {
                vector2i(cel.get("image_size"))?
            } else {
                (width as i32, height as i32)
            };
            integer(
                image_width as i64,
                1,
                MAX_EDGE as i64,
                "Pixelorama 单元格宽度",
            )?;
            integer(
                image_height as i64,
                1,
                MAX_EDGE as i64,
                "Pixelorama 单元格高度",
            )?;
            let (origin_x, origin_y) = vector2i(cel.get("offset"))?;
            let path = format!("image_data/frames/{}/layer_{}", frame_number + 1, index + 1);
            let raw_image = files
                .get(&path)
                .ok_or_else(|| err("Pixelorama 单元格图片缺失或尺寸不符。"))?;
            if raw_image.len() != image_width as usize * image_height as usize * 4 {
                return Err(err("Pixelorama 单元格图片缺失或尺寸不符。"));
            }
            let image = Image {
                width: image_width as u32,
                height: image_height as u32,
                pixels: raw_image.clone(),
            };
            let opacity = (unit_byte(
                layer.get("opacity").cloned().unwrap_or(json!(1)),
                "Pixelorama 图层透明度",
            )? as i32
                * unit_byte(
                    cel.get("opacity").cloned().unwrap_or(json!(1)),
                    "Pixelorama 单元格透明度",
                )? as i32
                + 127)
                / 255;
            blit(&mut canvas, &image, origin_x, origin_y, opacity);
        }
        output.push(budget.frame(
            &canvas,
            &format!("frame-{}.png", frame_number + 1),
            duration_ms(Some((1000.0 * span / fps).round()), 100.0)?,
        )?);
    }
    let mut clips = Vec::new();
    let mut names = HashSet::new();
    let tags = document.get("tags").cloned().unwrap_or(json!([]));
    let tags = tags
        .as_array()
        .ok_or_else(|| err("Pixelorama 动作标签无效。"))?;
    for tag in tags {
        let tag = super::object(Some(tag), "Pixelorama 动作")?;
        let tag_label = label(tag.get("name").and_then(Value::as_str).unwrap_or(""))?;
        let start = tag.get("from").and_then(Value::as_i64);
        let stop = tag.get("to").and_then(Value::as_i64);
        if start.is_none_or(|start| {
            stop.is_none_or(|stop| {
                !(1..=output.len() as i64).contains(&start)
                    || stop < start
                    || stop > output.len() as i64
            })
        }) {
            return Err(err("Pixelorama 动作范围无效。"));
        }
        if !names.insert(tag_label.clone()) {
            return Err(err("Pixelorama 动作名称重复。"));
        }
        clips.push(json!({"name": tag_label, "indices": ((start.unwrap() - 1) as usize..stop.unwrap() as usize).collect::<Vec<_>>(), "loop": true}));
    }
    if clips.is_empty() {
        clips.push(json!({"name": label(name)?, "indices": (0..output.len()).collect::<Vec<_>>(), "loop": true}));
    }
    Ok(
        json!({"format": "pixelorama", "name": label(name)?, "frames": output, "clips": clips, "slices": [], "warnings": notices}),
    )
}

fn vector2i(value: Option<&Value>) -> Result<(i32, i32)> {
    let Some(value) = value else {
        return Ok((0, 0));
    };
    let Some(text) = value.as_str() else {
        return Err(err("Pixelorama 坐标无效。"));
    };
    let text = text.trim();
    let Some(body) = text
        .strip_prefix('(')
        .and_then(|text| text.strip_suffix(')'))
    else {
        return Err(err("Pixelorama 坐标无效。"));
    };
    let Some((x, y)) = body.split_once(", ") else {
        return Err(err("Pixelorama 坐标无效。"));
    };
    let x: i32 = x.parse().map_err(|_| err("Pixelorama 坐标无效。"))?;
    let y: i32 = y.parse().map_err(|_| err("Pixelorama 坐标无效。"))?;
    Ok((x, y))
}

fn unit_byte(value: Value, name: &str) -> Result<u8> {
    if value.is_boolean() {
        return Err(err(format!("{name}需要在 0–1 之间。")));
    }
    let Some(number) = value.as_f64() else {
        return Err(err(format!("{name}需要在 0–1 之间。")));
    };
    if !number.is_finite() || !(0.0..=1.0).contains(&number) {
        return Err(err(format!("{name}需要在 0–1 之间。")));
    }
    Ok((number * 255.0 + 0.5) as u8)
}

fn json_i64(value: Option<&Value>) -> Option<i64> {
    value?.as_i64()
}

#[allow(dead_code)]
fn _cursor(data: &[u8]) -> Cursor<&[u8]> {
    Cursor::new(data)
}
