//! Pure sprite lookup and top-down premultiplied BGRA for Win32 layered windows.

use std::collections::HashMap;
use std::path::Path;

use shimeji_core::MascotFrame;
use shimeji_core::env::Point;
use shimeji_core::model::Character;
use shimeji_packs::DecodedImage;

#[derive(Clone)]
struct Pose {
    image: usize,
    right: Option<usize>,
    anchor: Point,
}

pub struct Atlas {
    images: Vec<DecodedImage>,
    paths: HashMap<String, usize>,
    poses: HashMap<String, Pose>,
    scale: f64,
}

pub struct Drawing {
    pub origin: Point,
    pub width: i32,
    pub height: i32,
    pub bgra: Vec<u8>,
}

impl Atlas {
    pub fn new(character: &Character, images: Vec<DecodedImage>, scale: f64) -> Self {
        let mut paths = HashMap::new();
        // Exact pack paths take precedence over filename aliases.
        for (index, image) in images.iter().enumerate() {
            paths.insert(normalize(&image.path), index);
        }
        for (index, image) in images.iter().enumerate() {
            if let Some(name) = Path::new(&image.path).file_name() {
                paths
                    .entry(name.to_string_lossy().into_owned())
                    .or_insert(index);
            }
        }
        let root = &character.image_root;
        let find = |path: &str| {
            paths
                .get(&normalize(&format!("{root}/{path}")))
                .or_else(|| paths.get(&normalize(path)))
                .copied()
        };
        let mut poses = HashMap::new();
        character.visit_actions(&mut |action| {
            for animation in &action.animations {
                for pose in &animation.poses {
                    let image = pose
                        .image
                        .as_ref()
                        .and_then(|image| find(&image.relative))
                        .or_else(|| pose.image_source.as_deref().and_then(find));
                    let Some(image) = image else { continue };
                    let right = pose
                        .image_right
                        .as_ref()
                        .or(pose.right_sibling.as_ref())
                        .and_then(|image| find(&image.relative));
                    let entry = Pose {
                        image,
                        right,
                        anchor: Point::new(pose.anchor_x, pose.anchor_y),
                    };
                    if let Some(source) = &pose.image_source {
                        poses.insert(normalize(source), entry.clone());
                    }
                    if let Some(image) = &pose.image {
                        poses.insert(normalize(&image.source), entry.clone());
                        poses.insert(normalize(&image.relative), entry);
                    }
                }
            }
        });
        Self {
            images,
            paths,
            poses,
            scale,
        }
    }

    pub fn draw(&self, frame: &MascotFrame) -> Result<Option<Drawing>, String> {
        let Some(key) = frame.image.as_deref().map(normalize) else {
            return Ok(None);
        };
        let fallback;
        let pose = if let Some(pose) = self.poses.get(&key) {
            pose
        } else if let Some(&index) = self.paths.get(&key) {
            let image = &self.images[index];
            fallback = Pose {
                image: index,
                right: None,
                anchor: Point::new(image.width as i32 / 2, image.height as i32),
            };
            &fallback
        } else {
            return Err(format!("The current pose's image was not decoded: {key}"));
        };
        let mirror = frame.look_right && pose.right.is_none();
        let image = &self.images[if frame.look_right {
            pose.right.unwrap_or(pose.image)
        } else {
            pose.image
        }];
        let width = (f64::from(image.width) * self.scale).round().max(1.0) as i32;
        let height = (f64::from(image.height) * self.scale).round().max(1.0) as i32;
        if width > 4096 || height > 4096 || !self.scale.is_finite() || self.scale <= 0.0 {
            return Err("Scaled image exceeds the Windows preview's 4096-pixel limit".to_string());
        }
        let anchor_x = if mirror {
            image.width as i32 - pose.anchor.x
        } else {
            pose.anchor.x
        };
        let anchor = frame
            .image_anchor
            .unwrap_or(Point::new(anchor_x, pose.anchor.y));
        let origin = Point::new(
            frame
                .anchor
                .x
                .saturating_sub((f64::from(anchor.x) * self.scale).round() as i32),
            frame
                .anchor
                .y
                .saturating_sub((f64::from(anchor.y) * self.scale).round() as i32),
        );
        Ok(Some(Drawing {
            origin,
            width,
            height,
            bgra: scaled_bgra(image, width as usize, height as usize, mirror),
        }))
    }
}

fn normalize(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_start_matches('/')
        .to_string()
}

fn scaled_bgra(image: &DecodedImage, width: usize, height: usize, mirror: bool) -> Vec<u8> {
    let mut pixels = vec![0; width * height * 4];
    if image.width == 0 || image.height == 0 {
        return pixels;
    }
    for y in 0..height {
        for x in 0..width {
            let source_x = x * image.width as usize / width;
            let source_x = if mirror {
                image.width as usize - 1 - source_x
            } else {
                source_x
            };
            let source_y = y * image.height as usize / height;
            let index = (source_y * image.width as usize + source_x) * 4;
            let Some(rgba) = image.rgba.get(index..index + 4) else {
                continue;
            };
            let alpha = u16::from(rgba[3]);
            let premultiply = |value: u8| ((u16::from(value) * alpha + 127) / 255) as u8;
            pixels[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(&[
                premultiply(rgba[2]),
                premultiply(rgba[1]),
                premultiply(rgba[0]),
                rgba[3],
            ]);
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_is_premultiplied_top_down_and_mirrored_before_scaling() {
        let image = DecodedImage {
            path: "test.png".into(),
            width: 2,
            height: 1,
            rgba: vec![200, 100, 50, 128, 90, 80, 70, 0],
        };
        assert_eq!(
            scaled_bgra(&image, 2, 1, false),
            [25, 50, 100, 128, 0, 0, 0, 0]
        );
        assert_eq!(
            scaled_bgra(&image, 4, 1, true),
            [0, 0, 0, 0, 0, 0, 0, 0, 25, 50, 100, 128, 25, 50, 100, 128]
        );
    }
}
