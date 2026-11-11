use std::io::Cursor;

use sha2::{Digest, Sha256};

use crate::error::{Result, err};

#[derive(Clone)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width as usize * height as usize * 4],
        }
    }

    pub fn get(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels[i..i + 4].try_into().unwrap()
    }

    pub fn set(&mut self, x: u32, y: u32, pixel: [u8; 4]) {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        self.pixels[i..i + 4].copy_from_slice(&pixel);
    }

    pub fn sha256_prefix(&self) -> String {
        hex(&Sha256::digest(&self.pixels))[..24].to_string()
    }

    /// Pillow `Image.transform(AFFINE, NEAREST)`: `(int)(a*x + b*y + c + 0.5)`.
    pub fn affine_nearest(&self, output: (u32, u32), inverse: [f64; 6]) -> Image {
        let [a, b, c, d, e, f] = inverse;
        let mut out = Image::new(output.0, output.1);
        for y in 0..output.1 {
            for x in 0..output.0 {
                let sx = (a * x as f64 + b * y as f64 + c + 0.5) as i32;
                let sy = (d * x as f64 + e * y as f64 + f + 0.5) as i32;
                if sx >= 0 && sy >= 0 && (sx as u32) < self.width && (sy as u32) < self.height {
                    out.set(x, y, self.get(sx as u32, sy as u32));
                }
            }
        }
        out
    }

    pub fn scale_alpha(&mut self, scale: f64) {
        for pixel in self.pixels.chunks_exact_mut(4) {
            pixel[3] = (pixel[3] as f64 * scale + 0.5) as u8;
        }
    }

    pub fn alpha_composite(&mut self, overlay: &Image, at: (i32, i32)) {
        for y in 0..overlay.height {
            for x in 0..overlay.width {
                let dx = at.0 + x as i32;
                let dy = at.1 + y as i32;
                if dx < 0 || dy < 0 || dx as u32 >= self.width || dy as u32 >= self.height {
                    continue;
                }
                let dst = self.get(dx as u32, dy as u32);
                self.set(dx as u32, dy as u32, over(dst, overlay.get(x, y)));
            }
        }
    }

    /// Replace pixels. Transparent source pixels still replace the destination.
    pub fn blit_source(&mut self, overlay: &Image, at: (i32, i32)) {
        for y in 0..overlay.height {
            for x in 0..overlay.width {
                let dx = at.0 + x as i32;
                let dy = at.1 + y as i32;
                if dx < 0 || dy < 0 || dx as u32 >= self.width || dy as u32 >= self.height {
                    continue;
                }
                self.set(dx as u32, dy as u32, overlay.get(x, y));
            }
        }
    }

    pub fn clear_rect(&mut self, x: u32, y: u32, width: u32, height: u32) {
        for row in y..y.saturating_add(height).min(self.height) {
            for col in x..x.saturating_add(width).min(self.width) {
                self.set(col, row, [0, 0, 0, 0]);
            }
        }
    }

    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Image {
        let mut out = Image::new(width, height);
        for row in 0..height {
            for col in 0..width {
                out.set(col, row, self.get(x + col, y + row));
            }
        }
        out
    }

    pub fn paste(&mut self, source: &Image, at: (u32, u32)) {
        for y in 0..source.height {
            for x in 0..source.width {
                let dx = at.0 + x;
                let dy = at.1 + y;
                if dx < self.width && dy < self.height {
                    self.set(dx, dy, source.get(x, y));
                }
            }
        }
    }

    /// 90° counter-clockwise, matching Pillow `ROTATE_90`.
    pub fn rotate_90_ccw(&self) -> Image {
        let mut out = Image::new(self.height, self.width);
        for y in 0..self.height {
            for x in 0..self.width {
                out.set(y, self.width - 1 - x, self.get(x, y));
            }
        }
        out
    }

    pub fn resize_nearest(&self, width: u32, height: u32) -> Image {
        if (width, height) == (self.width, self.height) {
            return self.clone();
        }
        let mut out = Image::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let sx = ((x as f64 + 0.5) * self.width as f64 / width as f64).floor() as u32;
                let sy = ((y as f64 + 0.5) * self.height as f64 / height as f64).floor() as u32;
                out.set(
                    x,
                    y,
                    self.get(sx.min(self.width - 1), sy.min(self.height - 1)),
                );
            }
        }
        out
    }

    pub fn encode_png(&self) -> Result<Vec<u8>> {
        let mut encoded = Vec::new();
        let mut encoder = png::Encoder::new(&mut encoded, self.width, self.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| err(format!("无法写出 PNG：{error}")))?;
        writer
            .write_image_data(&self.pixels)
            .map_err(|error| err(format!("无法写出 PNG：{error}")))?;
        drop(writer);
        Ok(encoded)
    }
}

pub fn decode_png(data: &[u8]) -> Result<Image> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let limits = png::Limits {
        bytes: 64 * 1024 * 1024,
    };
    decoder.set_limits(limits);
    let mut reader = decoder
        .read_info()
        .map_err(|error| err(format!("无法完整解码图片：{error}")))?;
    let mut buffer = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| err("无法完整解码图片：图像过大"))?
    ];
    let info = reader
        .next_frame(&mut buffer)
        .map_err(|error| err(format!("无法完整解码图片：{error}")))?;
    let pixels = pack_rgba(reader.output_color_type().0, &buffer[..info.buffer_size()])?;
    Ok(Image {
        width: info.width,
        height: info.height,
        pixels,
    })
}

pub fn pack_rgba(color: png::ColorType, data: &[u8]) -> Result<Vec<u8>> {
    match color {
        png::ColorType::Rgba => Ok(data.to_vec()),
        png::ColorType::Rgb => Ok(data
            .chunks_exact(3)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2], 255])
            .collect()),
        png::ColorType::Grayscale => Ok(data
            .iter()
            .flat_map(|pixel| [*pixel, *pixel, *pixel, 255])
            .collect()),
        png::ColorType::GrayscaleAlpha => Ok(data
            .chunks_exact(2)
            .flat_map(|pixel| [pixel[0], pixel[0], pixel[0], pixel[1]])
            .collect()),
        _ => Err(err("无法完整解码图片：不支持的颜色类型")),
    }
}

/// Porter-Duff source-over with the integer division Pillow uses for 8-bit pixels.
pub fn over(dst: [u8; 4], src: [u8; 4]) -> [u8; 4] {
    let sa = src[3] as u32;
    if sa == 0 {
        return dst;
    }
    if sa == 255 {
        return src;
    }
    let da = dst[3] as u32;
    let inv = 255 - sa;
    let out_a = sa + da * inv / 255;
    if out_a == 0 {
        return [0, 0, 0, 0];
    }
    let mut out = [0; 4];
    for channel in 0..3 {
        out[channel] =
            ((src[channel] as u32 * sa + dst[channel] as u32 * da * inv / 255) / out_a) as u8;
    }
    out[3] = out_a as u8;
    out
}

pub fn multiply(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    let [aa, ab, ac, ad, ae, af] = a;
    let [ba, bb, bc, bd, be, bf] = b;
    [
        aa * ba + ac * bb,
        ab * ba + ad * bb,
        aa * bc + ac * bd,
        ab * bc + ad * bd,
        aa * be + ac * bf + ae,
        ab * be + ad * bf + af,
    ]
}

pub fn invert_affine(matrix: [f64; 6]) -> Result<[f64; 6]> {
    let [a, b, c, d, e, f] = matrix;
    let det = a * d - b * c;
    if !det.is_finite() || det.abs() < 1e-12 {
        return Err(err("部件变换无法求逆"));
    }
    Ok([
        d / det,
        -c / det,
        (c * f - d * e) / det,
        -b / det,
        a / det,
        (b * e - a * f) / det,
    ])
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0xf) as usize] as char);
    }
    out
}

pub fn data_url(png: &[u8]) -> String {
    format!(
        "data:image/png;base64,{}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, png)
    )
}

pub fn decode_data_url(value: &str) -> Result<Vec<u8>> {
    let Some(payload) = value.strip_prefix("data:image/png;base64,") else {
        return Err(err("PNG 数据无效"));
    };
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, payload)
        .map_err(|_| err("PNG 数据无效"))
}
