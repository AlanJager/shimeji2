mod archive;
mod raster;
mod sprite;

use std::path::Path;

use serde_json::{Value, json};

use crate::error::{Result, err};
use crate::imagebuf::{Image, data_url};

const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ZIP_BYTES: usize = 16 * 1024 * 1024;
const MAX_JSON_BYTES: usize = 1024 * 1024;
const MAX_PIXELS: usize = 16 * 1024 * 1024;
const MAX_PNG_BYTES: usize = 16 * 1024 * 1024;
const MAX_FRAMES: usize = 256;
const MAX_EDGE: u32 = 4096;

struct Budget {
    frames: usize,
    pixels: usize,
    png_bytes: usize,
}

impl Budget {
    fn reserve(&mut self, width: u32, height: u32, output: bool) -> Result<()> {
        integer(width as i64, 1, MAX_EDGE as i64, "图片宽度")?;
        integer(height as i64, 1, MAX_EDGE as i64, "图片高度")?;
        self.pixels += width as usize * height as usize;
        if output {
            self.frames += 1;
        }
        if self.pixels > MAX_PIXELS {
            return Err(err("解码帧合计超过 16M 像素上限，请缩小图片或减少帧数。"));
        }
        if self.frames > MAX_FRAMES {
            return Err(err("一次最多导入 256 帧。"));
        }
        Ok(())
    }

    fn frame(&mut self, image: &Image, name: &str, duration: f64) -> Result<Value> {
        let encoded = image.encode_png()?;
        self.png_bytes += encoded.len();
        if self.png_bytes > MAX_PNG_BYTES {
            return Err(err("解码后的 PNG 合计超过 16 MiB 上限。"));
        }
        Ok(json!({
            "name": name,
            "src": data_url(&encoded),
            "width": image.width,
            "height": image.height,
            "durationMs": duration,
        }))
    }
}

pub fn decode_media(data: &[u8], name: &str) -> Result<Value> {
    label(name)?;
    let suffix = Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if suffix == "zip" {
        if !(data.starts_with(b"PK\x03\x04") || data.starts_with(b"PK\x05\x06")) {
            return Err(err("ZIP 文件签名无效。"));
        }
        return archive::decode_zip(data, name);
    }
    if suffix == "ase" || suffix == "aseprite" {
        return sprite::decode_aseprite(data, name);
    }
    if suffix == "piskel" {
        return sprite::decode_piskel(data, name);
    }
    if suffix == "pxo" {
        return sprite::decode_pixelorama(data, name);
    }
    raster::decode_raster(
        data,
        name,
        &mut Budget {
            frames: 0,
            pixels: 0,
            png_bytes: 0,
        },
    )
}

pub fn decode_atlas(
    metadata: &Value,
    sheet: &[u8],
    sheet_name: &str,
    atlas_name: &str,
) -> Result<Value> {
    archive::decode_atlas(
        metadata,
        sheet,
        sheet_name,
        atlas_name,
        &mut Budget {
            frames: 0,
            pixels: 0,
            png_bytes: 0,
        },
    )
}

fn label(value: &str) -> Result<String> {
    if value.is_empty()
        || value.chars().count() > 1024
        || value.chars().any(|char| char.is_control())
    {
        return Err(err("素材名称无效。"));
    }
    let normalized = value.replace('\\', "/");
    let stem = Path::new(&normalized)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("");
    let stem: String = stem.chars().take(255).collect();
    Ok(if stem.is_empty() {
        "素材".to_string()
    } else {
        stem
    })
}

fn integer(value: i64, low: i64, high: i64, name: &str) -> Result<i64> {
    if (low..=high).contains(&value) {
        Ok(value)
    } else {
        Err(err(format!("{name} 需要是 {low}–{high} 之间的整数。")))
    }
}

fn duration_ms(value: Option<f64>, default: f64) -> Result<f64> {
    let Some(value) = value else {
        return Ok(default);
    };
    if !value.is_finite() || !(0.0..=60_000.0).contains(&value) {
        return Err(err(
            "源帧时长需要在 0–60000 毫秒以内；请先缩短超过 60 秒的源帧。",
        ));
    }
    Ok(value)
}

fn object<'a>(value: Option<&'a Value>, name: &str) -> Result<&'a serde_json::Map<String, Value>> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| err(format!("{name} 格式无效。")))
}
