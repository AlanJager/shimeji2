use std::io::{Cursor, Read};

use serde_json::{Value, json};

use super::{Budget, MAX_EDGE, MAX_FILE_BYTES, MAX_FRAMES, duration_ms, integer, label};
use crate::error::{Result, err};
use crate::imagebuf::{Image, pack_rgba};

pub fn decode_raster(data: &[u8], name: &str, budget: &mut Budget) -> Result<Value> {
    let kind = signature(data, name)?;
    let mut notices = Vec::new();
    if kind == "JPEG" {
        notices.push("JPEG 保留原有不透明背景，不会自动抠图。".to_string());
    }
    let decoded = match kind {
        "PNG" => decode_png_animation(data)?,
        "GIF" => decode_gif(data)?,
        "WEBP" => decode_webp(data)?,
        "JPEG" => decode_jpeg(data)?,
        _ => return Err(err("只支持签名有效的 PNG/APNG、GIF、WebP 或 JPEG。")),
    };
    if decoded.poster {
        notices.push("APNG 默认静态封面已跳过，仅导入动画帧。".to_string());
    }
    if decoded.frames.is_empty() {
        return Err(err("图片中没有可导入的动画帧。"));
    }
    let (width, height) = (decoded.frames[0].0.width, decoded.frames[0].0.height);
    let mut frames = Vec::new();
    for (image, duration) in &decoded.frames {
        if (image.width, image.height) != (width, height) {
            return Err(err("动画帧改变了逻辑画布尺寸。"));
        }
        budget.reserve(image.width, image.height, true)?;
        let title = label(name)?;
        frames.push(budget.frame(
            image,
            &format!("{title}-{:03}.png", frames.len() + 1),
            *duration,
        )?);
    }
    let animated = decoded.animated || frames.len() > 1;
    let mut clips = Vec::new();
    if animated {
        let looped = animation_loop(decoded.plays, kind, &mut notices);
        clips.push(json!({"name": label(name)?, "indices": (0..frames.len()).collect::<Vec<_>>(), "loop": looped}));
    }
    let lower = kind.to_ascii_lowercase();
    let format = if decoded.apng { "apng" } else { lower.as_str() };
    Ok(
        json!({"format": format, "name": label(name)?, "frames": frames, "clips": clips, "warnings": notices}),
    )
}

struct Decoded {
    frames: Vec<(Image, f64)>,
    plays: Option<u32>,
    apng: bool,
    poster: bool,
    animated: bool,
}

fn animation_loop(plays: Option<u32>, kind: &str, notices: &mut Vec<String>) -> bool {
    let Some(count) = plays else {
        return false;
    };
    if count == 0 {
        return true;
    }
    if kind == "GIF" || count > 1 {
        notices
            .push("源动画指定有限重复次数；桌宠只支持循环或单次，本次按单个周期导入。".to_string());
    }
    false
}

pub(super) fn signature_public(data: &[u8], name: &str) -> Result<&'static str> {
    signature(data, name)
}

fn signature(data: &[u8], name: &str) -> Result<&'static str> {
    if data.is_empty() || data.len() > MAX_FILE_BYTES {
        return Err(err("单个图片需要在 8 MiB 以内。"));
    }
    let suffix = std::path::Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let (kind, extensions): (&str, &[&str]) = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        png_chunks(data)?;
        ("PNG", &["png", "apng"])
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        if !data.ends_with(b";") {
            return Err(err("GIF 缺少结束标记。"));
        }
        gif_header(data)?;
        ("GIF", &["gif"])
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        ("JPEG", &["jpg", "jpeg"])
    } else if data.len() >= 12 && data.starts_with(b"RIFF") && &data[8..12] == b"WEBP" {
        if u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize + 8 != data.len() {
            return Err(err("WebP RIFF 长度无效。"));
        }
        webp_header(data)?;
        ("WEBP", &["webp"])
    } else {
        return Err(err("只支持签名有效的 PNG/APNG、GIF、WebP 或 JPEG。"));
    };
    if !extensions.contains(&suffix.as_str()) {
        return Err(err("图片扩展名与实际格式不一致。"));
    }
    Ok(kind)
}

fn png_chunks(data: &[u8]) -> Result<()> {
    let mut at = 8usize;
    let mut chunks = 0usize;
    let mut canvas = None;
    let mut declared = None;
    let mut controls = 0usize;
    let mut frame_data = false;
    while at < data.len() {
        chunks += 1;
        if chunks > 4096 || at + 12 > data.len() {
            return Err(err("PNG chunk 数量或边界无效。"));
        }
        let length = u32::from_be_bytes(data[at..at + 4].try_into().unwrap()) as usize;
        let end = at + 12 + length;
        if end > data.len() {
            return Err(err("PNG chunk 被截断。"));
        }
        let kind = &data[at + 4..at + 8];
        let expected = u32::from_be_bytes(data[end - 4..end].try_into().unwrap());
        if crc32fast::hash(&data[at + 4..end - 4]) != expected {
            return Err(err("PNG chunk 校验失败。"));
        }
        if chunks == 1 {
            if kind != b"IHDR" || length != 13 {
                return Err(err("PNG 缺少首个 IHDR。"));
            }
            let width = u32::from_be_bytes(data[at + 8..at + 12].try_into().unwrap());
            let height = u32::from_be_bytes(data[at + 12..at + 16].try_into().unwrap());
            integer(width as i64, 1, MAX_EDGE as i64, "PNG 尺寸")?;
            integer(height as i64, 1, MAX_EDGE as i64, "PNG 尺寸")?;
            canvas = Some((width, height));
        } else if kind == b"IHDR" {
            return Err(err("PNG 包含重复 IHDR。"));
        }
        if kind == b"acTL" {
            if length != 8 || declared.is_some() {
                return Err(err("APNG acTL 长度无效。"));
            }
            declared = Some(integer(
                u32::from_be_bytes(data[at + 8..at + 12].try_into().unwrap()) as i64,
                1,
                MAX_FRAMES as i64,
                "APNG 帧数",
            )? as u32);
        }
        if kind == b"fcTL" {
            if controls > 0 && !frame_data {
                return Err(err("APNG 帧控制缺少对应位图数据。"));
            }
            controls += 1;
            frame_data = false;
            if length != 26 || declared.is_none() || controls > MAX_FRAMES {
                return Err(err("APNG 帧控制数量或长度无效。"));
            }
            let width = u32::from_be_bytes(data[at + 12..at + 16].try_into().unwrap());
            let height = u32::from_be_bytes(data[at + 16..at + 20].try_into().unwrap());
            let x = u32::from_be_bytes(data[at + 20..at + 24].try_into().unwrap());
            let y = u32::from_be_bytes(data[at + 24..at + 28].try_into().unwrap());
            let (canvas_w, canvas_h) = canvas.unwrap_or((0, 0));
            if width == 0
                || height == 0
                || x.saturating_add(width) > canvas_w
                || y.saturating_add(height) > canvas_h
            {
                return Err(err("APNG 帧区域超出逻辑画布。"));
            }
        }
        if kind == b"IDAT" && controls > 0 {
            if controls > 1 {
                return Err(err("APNG 后续帧必须使用 fdAT。"));
            }
            frame_data |= length > 0;
        }
        if kind == b"fdAT" {
            if controls == 0 || length < 4 {
                return Err(err("APNG fdAT 没有对应帧控制或数据被截断。"));
            }
            frame_data |= length > 4;
        }
        if kind == b"IEND" {
            if length != 0 || end != data.len() {
                return Err(err("PNG IEND 或尾部数据无效。"));
            }
            if declared.is_some_and(|declared| controls != declared as usize) {
                return Err(err("APNG 声明帧数与完整帧控制数量不一致。"));
            }
            if declared.is_some() && !frame_data {
                return Err(err("APNG 最后一帧缺少位图数据。"));
            }
            return Ok(());
        }
        at = end;
    }
    Err(err("PNG 缺少 IEND。"))
}

fn gif_header(data: &[u8]) -> Result<()> {
    if data.len() < 14 {
        return Err(err("GIF 文件头被截断。"));
    }
    let width = u16::from_le_bytes(data[6..8].try_into().unwrap()) as u32;
    let height = u16::from_le_bytes(data[8..10].try_into().unwrap()) as u32;
    integer(width as i64, 1, MAX_EDGE as i64, "GIF 尺寸")?;
    integer(height as i64, 1, MAX_EDGE as i64, "GIF 尺寸")?;
    let mut at = 13
        + if data[10] & 128 != 0 {
            3 * (1usize << ((data[10] & 7) + 1))
        } else {
            0
        };
    let mut frames = 0usize;
    let subblocks = |data: &[u8], mut offset: usize| -> Result<usize> {
        while offset < data.len() {
            let length = data[offset] as usize;
            offset += 1;
            if length == 0 {
                return Ok(offset);
            }
            offset += length;
            if offset > data.len() {
                break;
            }
        }
        Err(err("GIF 数据块被截断。"))
    };
    while at < data.len() {
        let marker = data[at];
        at += 1;
        if marker == 0x3b {
            if at != data.len() || frames == 0 {
                return Err(err("GIF 尾部或帧数无效。"));
            }
            return Ok(());
        }
        if marker == 0x21 {
            if at >= data.len() {
                break;
            }
            at = subblocks(data, at + 1)?;
        } else if marker == 0x2c {
            if at + 9 > data.len() {
                break;
            }
            let x = u16::from_le_bytes(data[at..at + 2].try_into().unwrap()) as u32;
            let y = u16::from_le_bytes(data[at + 2..at + 4].try_into().unwrap()) as u32;
            let fw = u16::from_le_bytes(data[at + 4..at + 6].try_into().unwrap()) as u32;
            let fh = u16::from_le_bytes(data[at + 6..at + 8].try_into().unwrap()) as u32;
            let packed = data[at + 8];
            frames += 1;
            if fw == 0 || fh == 0 || x + fw > width || y + fh > height || frames > MAX_FRAMES {
                return Err(err("GIF 帧区域或帧数超出逻辑画布上限。"));
            }
            at += 9 + if packed & 128 != 0 {
                3 * (1usize << ((packed & 7) + 1))
            } else {
                0
            };
            if at >= data.len() {
                break;
            }
            at = subblocks(data, at + 1)?;
        } else {
            return Err(err("GIF 数据块标记无效。"));
        }
    }
    Err(err("GIF 文件被截断。"))
}

fn webp_dimensions(data: &[u8], start: usize, end: usize, kind: &[u8]) -> Result<(u32, u32)> {
    if kind == b"VP8 " {
        if end - start < 10 || &data[start + 3..start + 6] != b"\x9d\x01\x2a" {
            return Err(err("WebP VP8 文件头无效。"));
        }
        let width =
            u16::from_le_bytes(data[start + 6..start + 8].try_into().unwrap()) as u32 & 0x3fff;
        let height =
            u16::from_le_bytes(data[start + 8..start + 10].try_into().unwrap()) as u32 & 0x3fff;
        return Ok((width, height));
    }
    if end - start < 5 || data[start] != 0x2f {
        return Err(err("WebP VP8L 文件头无效。"));
    }
    let bits = u32::from_le_bytes(data[start + 1..start + 5].try_into().unwrap());
    Ok((1 + (bits & 0x3fff), 1 + ((bits >> 14) & 0x3fff)))
}

fn webp_header(data: &[u8]) -> Result<()> {
    let mut at = 12usize;
    let mut frames = 0usize;
    let mut canvas = None;
    let mut chunks = 0usize;
    while at < data.len() {
        chunks += 1;
        if at + 8 > data.len() || chunks > 4096 {
            return Err(err("WebP chunk 边界或数量无效。"));
        }
        let kind = &data[at..at + 4];
        let length = u32::from_le_bytes(data[at + 4..at + 8].try_into().unwrap()) as usize;
        let start = at + 8;
        let end = start + length;
        if end + length % 2 > data.len() {
            return Err(err("WebP chunk 被截断。"));
        }
        if kind == b"VP8X" {
            if length != 10 {
                return Err(err("WebP VP8X 长度无效。"));
            }
            let width =
                1 + u32::from_le_bytes([data[start + 4], data[start + 5], data[start + 6], 0]);
            let height =
                1 + u32::from_le_bytes([data[start + 7], data[start + 8], data[start + 9], 0]);
            canvas = Some((width, height));
        } else if kind == b"VP8 " || kind == b"VP8L" {
            let actual = webp_dimensions(data, start, end, kind)?;
            if canvas.is_some_and(|canvas| canvas != actual) {
                return Err(err("WebP 位图尺寸与逻辑画布不一致。"));
            }
            canvas = Some(actual);
        } else if kind == b"ANMF" {
            frames += 1;
            if length < 16 || canvas.is_none() || frames > MAX_FRAMES {
                return Err(err("WebP 动画帧控制无效或超过 256 帧。"));
            }
            let read3 = |offset: usize| {
                u32::from_le_bytes([
                    data[start + offset],
                    data[start + offset + 1],
                    data[start + offset + 2],
                    0,
                ])
            };
            let x = read3(0) * 2;
            let y = read3(3) * 2;
            let width = read3(6) + 1;
            let height = read3(9) + 1;
            let (canvas_w, canvas_h) = canvas.unwrap();
            if x + width > canvas_w || y + height > canvas_h {
                return Err(err("WebP 动画帧超出逻辑画布。"));
            }
        }
        if let Some((width, height)) = canvas {
            integer(width as i64, 1, MAX_EDGE as i64, "WebP 尺寸")?;
            integer(height as i64, 1, MAX_EDGE as i64, "WebP 尺寸")?;
        }
        at = end + length % 2;
    }
    if canvas.is_none() {
        return Err(err("WebP 缺少画布尺寸。"));
    }
    Ok(())
}

fn decode_png_animation(data: &[u8]) -> Result<Decoded> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let limits = png::Limits {
        bytes: 64 * 1024 * 1024,
    };
    decoder.set_limits(limits);
    let mut reader = decoder
        .read_info()
        .map_err(|error| err(format!("无法完整解码图片：{error}")))?;
    let width = reader.info().width;
    let height = reader.info().height;
    let poster = reader.info().animation_control.is_some() && reader.info().frame_control.is_none();
    let plays = reader
        .info()
        .animation_control()
        .map(|control| control.num_plays);
    let apng = plays.is_some();
    let mut canvas = Image::new(width, height);
    let mut frames = Vec::new();
    let mut skip_poster = poster;
    loop {
        if reader.info().animation_control.is_some()
            && frames.len() + usize::from(skip_poster)
                >= reader
                    .info()
                    .animation_control()
                    .map(|c| c.num_frames as usize)
                    .unwrap_or(0)
                    + usize::from(poster)
        {
            break;
        }
        let size = reader
            .output_buffer_size()
            .ok_or_else(|| err("无法完整解码图片：图像过大"))?;
        let mut buffer = vec![0; size];
        let output = match reader.next_frame(&mut buffer) {
            Ok(output) => output,
            Err(png::DecodingError::Parameter(_)) if !frames.is_empty() || skip_poster => break,
            Err(error) => return Err(err(format!("无法完整解码图片：{error}"))),
        };
        let control = reader.info().frame_control;
        if skip_poster {
            skip_poster = false;
            continue;
        }
        let (frame_w, frame_h, x, y, delay_num, delay_den, dispose, blend) = match control {
            Some(control) => (
                control.width,
                control.height,
                control.x_offset,
                control.y_offset,
                control.delay_num,
                control.delay_den,
                control.dispose_op,
                control.blend_op,
            ),
            None => (
                output.width,
                output.height,
                0,
                0,
                0,
                0,
                png::DisposeOp::None,
                png::BlendOp::Source,
            ),
        };
        let pixels = pack_rgba(
            reader.output_color_type().0,
            &buffer[..output.buffer_size()],
        )?;
        let subframe = Image {
            width: frame_w,
            height: frame_h,
            pixels,
        };
        let snapshot = if dispose == png::DisposeOp::Previous {
            Some(canvas.clone())
        } else {
            None
        };
        if blend == png::BlendOp::Source {
            canvas.blit_source(&subframe, (x as i32, y as i32));
        } else {
            canvas.alpha_composite(&subframe, (x as i32, y as i32));
        }
        let den = if delay_den == 0 {
            100
        } else {
            delay_den as u32
        };
        let duration = if apng {
            delay_num as f64 * 1000.0 / den as f64
        } else {
            240.0
        };
        frames.push((canvas.clone(), duration_ms(Some(duration), 100.0)?));
        match dispose {
            png::DisposeOp::Background => canvas.clear_rect(x, y, frame_w, frame_h),
            png::DisposeOp::Previous => {
                canvas = snapshot.unwrap_or_else(|| Image::new(width, height))
            }
            png::DisposeOp::None => {}
        }
        if !apng {
            break;
        }
    }
    Ok(Decoded {
        animated: apng,
        apng,
        poster,
        plays,
        frames,
    })
}

fn decode_gif(data: &[u8]) -> Result<Decoded> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options
        .read_info(Cursor::new(data))
        .map_err(|error| err(format!("无法完整解码图片：{error}")))?;
    let width = decoder.width() as u32;
    let height = decoder.height() as u32;
    let plays = match decoder.repeat() {
        gif::Repeat::Infinite => Some(0),
        gif::Repeat::Finite(0) => None,
        gif::Repeat::Finite(count) => Some(count as u32),
    };
    let mut canvas = Image::new(width, height);
    let mut frames = Vec::new();
    let mut previous_dispose = gif::DisposalMethod::Any;
    let mut previous_rect = (0u32, 0, 0, 0);
    let mut snapshot = None;
    while let Some(frame) = decoder
        .read_next_frame()
        .map_err(|error| err(format!("无法完整解码图片：{error}")))?
    {
        match previous_dispose {
            gif::DisposalMethod::Background => canvas.clear_rect(
                previous_rect.0,
                previous_rect.1,
                previous_rect.2,
                previous_rect.3,
            ),
            gif::DisposalMethod::Previous => {
                if let Some(saved) = snapshot.take() {
                    canvas = saved;
                }
            }
            _ => {}
        }
        if frame.dispose == gif::DisposalMethod::Previous {
            snapshot = Some(canvas.clone());
        }
        let subframe = Image {
            width: frame.width as u32,
            height: frame.height as u32,
            pixels: frame.buffer.to_vec(),
        };
        canvas.alpha_composite(&subframe, (frame.left as i32, frame.top as i32));
        frames.push((
            canvas.clone(),
            duration_ms(Some(frame.delay as f64 * 10.0), 100.0)?,
        ));
        previous_dispose = frame.dispose;
        previous_rect = (
            frame.left as u32,
            frame.top as u32,
            frame.width as u32,
            frame.height as u32,
        );
    }
    Ok(Decoded {
        animated: true,
        apng: false,
        poster: false,
        plays,
        frames,
    })
}

fn decode_webp(data: &[u8]) -> Result<Decoded> {
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(data))
        .map_err(|error| err(format!("无法完整解码图片：{error}")))?;
    let (width, height) = decoder.dimensions();
    integer(width as i64, 1, MAX_EDGE as i64, "WebP 尺寸")?;
    integer(height as i64, 1, MAX_EDGE as i64, "WebP 尺寸")?;
    let plays = match decoder.loop_count() {
        image_webp::LoopCount::Forever => Some(0),
        image_webp::LoopCount::Times(count) => Some(count.get() as u32),
    };
    if !decoder.is_animated() {
        let size = decoder
            .output_buffer_size()
            .ok_or_else(|| err("无法完整解码图片：图像过大"))?;
        let mut buffer = vec![0; size];
        decoder
            .read_image(&mut buffer)
            .map_err(|error| err(format!("无法完整解码图片：{error}")))?;
        let pixels = if decoder.has_alpha() {
            buffer
        } else {
            buffer
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect()
        };
        return Ok(Decoded {
            frames: vec![(
                Image {
                    width,
                    height,
                    pixels,
                },
                240.0,
            )],
            plays: None,
            apng: false,
            poster: false,
            animated: false,
        });
    }
    let mut frames = Vec::new();
    let count = decoder.num_frames();
    for _ in 0..count {
        let size = decoder
            .output_buffer_size()
            .ok_or_else(|| err("无法完整解码图片：图像过大"))?;
        let mut buffer = vec![0; size];
        let duration = decoder
            .read_frame(&mut buffer)
            .map_err(|error| err(format!("无法完整解码图片：{error}")))?;
        let pixels = if decoder.has_alpha() {
            buffer
        } else {
            buffer
                .chunks_exact(3)
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect()
        };
        frames.push((
            Image {
                width,
                height,
                pixels,
            },
            duration_ms(Some(duration as f64), 100.0)?,
        ));
    }
    Ok(Decoded {
        animated: true,
        apng: false,
        poster: false,
        plays,
        frames,
    })
}

fn decode_jpeg(data: &[u8]) -> Result<Decoded> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(data));
    let pixels = decoder
        .decode()
        .map_err(|error| err(format!("无法完整解码图片：{error}")))?;
    let info = decoder
        .info()
        .ok_or_else(|| err("无法完整解码图片：缺少图像信息"))?;
    integer(info.width as i64, 1, MAX_EDGE as i64, "图片宽度")?;
    integer(info.height as i64, 1, MAX_EDGE as i64, "图片高度")?;
    let rgba = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|p| [*p, *p, *p, 255]).collect(),
        jpeg_decoder::PixelFormat::L16 => pixels
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], 255])
            .collect(),
        jpeg_decoder::PixelFormat::CMYK32 => return Err(err("无法完整解码图片：不支持 CMYK JPEG")),
    };
    let image = orient(
        Image {
            width: info.width as u32,
            height: info.height as u32,
            pixels: rgba,
        },
        exif_orientation(decoder.exif_data()),
    );
    Ok(Decoded {
        frames: vec![(image, 240.0)],
        plays: None,
        apng: false,
        poster: false,
        animated: false,
    })
}

fn exif_orientation(exif: Option<&[u8]>) -> u16 {
    let Some(data) = exif else { return 1 };
    if data.len() < 8 {
        return 1;
    }
    let le = &data[..2] == b"II";
    if !le && &data[..2] != b"MM" {
        return 1;
    }
    let u16_at = |offset: usize| {
        let bytes: [u8; 2] = data.get(offset..offset + 2)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(bytes)
        } else {
            u16::from_be_bytes(bytes)
        })
    };
    let u32_at = |offset: usize| {
        let bytes: [u8; 4] = data.get(offset..offset + 4)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        })
    };
    let mut offset = u32_at(4).unwrap_or(0) as usize;
    if offset + 2 > data.len() {
        return 1;
    }
    let count = u16_at(offset).unwrap_or(0) as usize;
    offset += 2;
    for _ in 0..count {
        if offset + 12 > data.len() {
            break;
        }
        let tag = u16_at(offset).unwrap_or(0);
        if tag == 0x0112 {
            return u16_at(offset + 8).unwrap_or(1);
        }
        offset += 12;
    }
    1
}

fn orient(image: Image, orientation: u16) -> Image {
    match orientation {
        2 => flip(&image, true, false),
        3 => rotate_180(&image),
        4 => flip(&image, false, true),
        5 => rotate_90_cw(&flip(&image, true, false)),
        6 => rotate_90_cw(&image),
        7 => rotate_90_cw(&flip(&image, false, true)),
        8 => image.rotate_90_ccw(),
        _ => image,
    }
}

fn rotate_90_cw(image: &Image) -> Image {
    let mut out = Image::new(image.height, image.width);
    for y in 0..image.height {
        for x in 0..image.width {
            out.set(image.height - 1 - y, x, image.get(x, y));
        }
    }
    out
}

fn rotate_180(image: &Image) -> Image {
    let mut out = Image::new(image.width, image.height);
    for y in 0..image.height {
        for x in 0..image.width {
            out.set(image.width - 1 - x, image.height - 1 - y, image.get(x, y));
        }
    }
    out
}

fn flip(image: &Image, horizontal: bool, vertical: bool) -> Image {
    let mut out = Image::new(image.width, image.height);
    for y in 0..image.height {
        for x in 0..image.width {
            let dx = if horizontal { image.width - 1 - x } else { x };
            let dy = if vertical { image.height - 1 - y } else { y };
            out.set(dx, dy, image.get(x, y));
        }
    }
    out
}

#[allow(dead_code)]
fn _read_exact(data: &[u8]) -> usize {
    let mut cursor = Cursor::new(data);
    let mut buf = [0; 1];
    cursor.read(&mut buf).unwrap_or(0)
}
