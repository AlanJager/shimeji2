//! Pose size and anchor lookup from the character plus decoded images.

use std::collections::HashMap;
use std::path::Path;

use shimeji_core::env::Point;
use shimeji_core::model::Character;
use shimeji_packs::DecodedImage;

/// Columns that contain a pixel with alpha > 0. `right` is exclusive.
///
/// An image with no such pixel uses `0..width`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpaqueSpan {
    pub left: i32,
    pub right: i32,
}

impl OpaqueSpan {
    pub fn full(width: i32) -> Self {
        Self {
            left: 0,
            right: width.max(0),
        }
    }

    /// A mirrored pose flips the image, so the span becomes `width - right .. width - left`.
    pub fn mirrored(self, width: i32) -> Self {
        Self {
            left: width.wrapping_sub(self.right),
            right: width.wrapping_sub(self.left),
        }
    }

    /// An empty span, or one that misses the image, keeps the full width.
    pub fn clipped(self, width: i32) -> Self {
        let width = width.max(0);
        if self.right <= self.left {
            return Self::full(width);
        }
        let left = self.left.clamp(0, width);
        let right = self.right.clamp(0, width);
        if right <= left {
            Self::full(width)
        } else {
            Self { left, right }
        }
    }
}

/// Leftmost and rightmost columns with alpha > 0.
///
/// Scanned once, when the pose table is built. A short buffer treats the missing
/// bytes as clear. An empty image keeps the full width.
pub fn opaque_columns(rgba: &[u8], width: u32, height: u32) -> OpaqueSpan {
    let width_i = i32::try_from(width).unwrap_or(0);
    let full = OpaqueSpan::full(width_i);
    if width == 0 || height == 0 {
        return full;
    }
    let width_us = width as usize;
    let mut left: Option<usize> = None;
    let mut right: Option<usize> = None;
    for y in 0..height as usize {
        for x in 0..width_us {
            let Some(index) = y
                .checked_mul(width_us)
                .and_then(|pixel| pixel.checked_add(x))
                .and_then(|pixel| pixel.checked_mul(4))
                .and_then(|byte| byte.checked_add(3))
            else {
                continue;
            };
            if rgba.get(index).copied().unwrap_or(0) > 0 {
                left = Some(left.map_or(x, |value| value.min(x)));
                right = Some(right.map_or(x, |value| value.max(x)));
            }
        }
    }
    match (left, right) {
        (Some(left), Some(right)) => OpaqueSpan {
            left: i32::try_from(left).unwrap_or(0),
            right: i32::try_from(right).unwrap_or(width_i).saturating_add(1),
        },
        _ => full,
    }
}

/// What the shell needs to place one pose on a panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoseLook {
    pub anchor: Point,
    pub width: i32,
    pub height: i32,
    /// Opaque columns of the image this pose draws, before a mirror flip.
    pub opaque: OpaqueSpan,
    /// Pack-relative path of explicit right-facing art, when the pack has it.
    pub right_path: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct ImageGeom {
    width: i32,
    height: i32,
    opaque: OpaqueSpan,
}

#[derive(Debug, Clone)]
pub struct PoseTable {
    by_key: HashMap<String, PoseLook>,
    by_path: HashMap<String, PoseLook>,
    images: HashMap<String, ImageGeom>,
    fallback: PoseLook,
}

impl PoseTable {
    pub fn new(character: &Character, images: &[DecodedImage]) -> Self {
        let mut stored = HashMap::new();
        for image in images {
            let opaque = opaque_columns(&image.rgba, image.width, image.height);
            remember(
                &mut stored,
                &image.path,
                ImageGeom {
                    width: image.width as i32,
                    height: image.height as i32,
                    opaque,
                },
            );
        }
        let mut by_key = HashMap::new();
        let mut by_path = HashMap::new();
        let image_root = character.image_root.trim_matches('/');
        character.visit_actions(&mut |action| {
            for animation in &action.animations {
                for pose in &animation.poses {
                    let geom = pose
                        .image
                        .as_ref()
                        .and_then(|image| geom_of(&stored, image_root, &image.relative))
                        .or_else(|| {
                            pose.image_source
                                .as_ref()
                                .and_then(|source| geom_of(&stored, image_root, source))
                        })
                        .unwrap_or(ImageGeom {
                            width: 128,
                            height: 128,
                            opaque: OpaqueSpan::full(128),
                        });
                    let right_path = pose
                        .image_right
                        .as_ref()
                        .map(|image| join_root(image_root, &image.relative))
                        .or_else(|| {
                            pose.right_sibling
                                .as_ref()
                                .map(|image| join_root(image_root, &image.relative))
                        });
                    let look = PoseLook {
                        anchor: Point::new(pose.anchor_x, pose.anchor_y),
                        width: geom.width,
                        height: geom.height,
                        opaque: geom.opaque,
                        right_path,
                    };
                    if let Some(source) = &pose.image_source {
                        by_key.insert(source.clone(), look.clone());
                        by_key.insert(normalize(source), look.clone());
                    }
                    if let Some(image) = &pose.image {
                        by_key.insert(image.source.clone(), look.clone());
                        let path = join_root(image_root, &image.relative);
                        by_path.insert(path, look.clone());
                    }
                }
            }
        });
        for image in images {
            let geom = stored
                .get(&normalize(&image.path))
                .copied()
                .unwrap_or(ImageGeom {
                    width: image.width as i32,
                    height: image.height as i32,
                    opaque: OpaqueSpan::full(image.width as i32),
                });
            by_path.entry(normalize(&image.path)).or_insert(PoseLook {
                anchor: Point::new(image.width as i32 / 2, image.height as i32),
                width: geom.width,
                height: geom.height,
                opaque: geom.opaque,
                right_path: None,
            });
        }
        Self {
            by_key,
            by_path,
            images: stored,
            fallback: PoseLook {
                anchor: Point::new(64, 128),
                width: 128,
                height: 128,
                opaque: OpaqueSpan::full(128),
                right_path: None,
            },
        }
    }

    /// Opaque columns of a decoded image, keyed the same way poses are.
    pub(crate) fn image_span(&self, key: &str) -> Option<OpaqueSpan> {
        self.images.get(&normalize(key)).map(|geom| geom.opaque)
    }

    pub fn lookup(&self, image_key: Option<&str>) -> &PoseLook {
        let Some(key) = image_key else {
            return &self.fallback;
        };
        self.by_key
            .get(key)
            .or_else(|| self.by_key.get(&normalize(key)))
            .or_else(|| self.by_path.get(&normalize(key)))
            .unwrap_or(&self.fallback)
    }

    /// Conservative room above a new pet's anchor, from the character's poses.
    pub fn drop_headroom(&self, scale: f64) -> i32 {
        // CALayer scales around its centre. Shrinking can safely retain the
        // unscaled allowance; enlargement adds half the extra image height.
        let above = |pose: &PoseLook| {
            (f64::from(pose.anchor.y.max(0))
                + f64::from(pose.height.max(0)) * (scale - 1.0).max(0.0) / 2.0)
                .ceil() as i32
        };
        self.by_key
            .values()
            .chain(self.by_path.values())
            .map(above)
            .max()
            .unwrap_or_else(|| above(&self.fallback))
    }
}

fn geom_of(
    images: &HashMap<String, ImageGeom>,
    image_root: &str,
    relative: &str,
) -> Option<ImageGeom> {
    let trimmed = relative.trim_start_matches('/');
    images
        .get(&normalize(relative))
        .copied()
        .or_else(|| images.get(&normalize(trimmed)).copied())
        .or_else(|| {
            images
                .get(&normalize(&join_root(image_root, trimmed)))
                .copied()
        })
}

fn remember(images: &mut HashMap<String, ImageGeom>, path: &str, geom: ImageGeom) {
    images.insert(normalize(path), geom);
    if let Some(name) = Path::new(path).file_name() {
        images.insert(normalize(&name.to_string_lossy()), geom);
    }
}

fn join_root(root: &str, relative: &str) -> String {
    let relative = relative.trim_start_matches('/');
    if root.is_empty() {
        relative.to_string()
    } else {
        format!("{root}/{relative}")
    }
}

fn normalize(path: &str) -> String {
    path.trim().trim_start_matches('/').replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use shimeji_core::model::{
        Action, ActionKind, Animation, Character, Information, OrderedMap, Pose, ResolvedImage,
        Schema,
    };

    fn rgba_column(width: usize, column: usize, alpha: u8) -> Vec<u8> {
        let mut rgba = vec![0u8; width * 4];
        let index = column * 4 + 3;
        rgba[index] = alpha;
        rgba
    }

    #[test]
    fn opaque_columns_keep_the_outer_pixels_and_a_blank_image() {
        let mut rgba = vec![0u8; 8 * 4];
        rgba[4 + 3] = 1;
        rgba[6 * 4 + 3] = 255;
        assert_eq!(
            opaque_columns(&rgba, 8, 1),
            OpaqueSpan { left: 1, right: 7 }
        );
        assert_eq!(opaque_columns(&[0u8; 8 * 4], 8, 1), OpaqueSpan::full(8));
        assert_eq!(opaque_columns(&[255u8; 4 * 4], 4, 1), OpaqueSpan::full(4));
        assert_eq!(opaque_columns(&[], 0, 0), OpaqueSpan::full(0));
        assert_eq!(
            OpaqueSpan {
                left: 31,
                right: 128
            }
            .mirrored(128),
            OpaqueSpan { left: 0, right: 97 }
        );
    }

    #[test]
    fn a_pose_records_the_image_span_once() {
        let mut actions = OrderedMap::new();
        actions.insert(
            "Sit",
            Action {
                name: Some("Sit".to_string()),
                schema: Schema::English,
                kind: ActionKind::Animate,
                degraded_from: None,
                params: OrderedMap::new(),
                animations: vec![Animation {
                    condition: None,
                    is_turn: false,
                    poses: vec![Pose {
                        image_source: Some("/pet.png".to_string()),
                        image: Some(ResolvedImage {
                            source: "/pet.png".to_string(),
                            relative: "img/pet.png".to_string(),
                            width: 4,
                            height: 1,
                        }),
                        image_right_source: None,
                        image_right: Some(ResolvedImage {
                            source: "/pet-r.png".to_string(),
                            relative: "img/pet-r.png".to_string(),
                            width: 4,
                            height: 1,
                        }),
                        right_sibling: None,
                        anchor_x: 2,
                        anchor_y: 1,
                        velocity_x: 0,
                        velocity_y: 0,
                        duration_ticks: 1,
                        sound: None,
                        volume: None,
                        line: 1,
                    }],
                    hotspots: Vec::new(),
                    line: 1,
                }],
                children: Vec::new(),
                line: 1,
            },
        );
        let character = Character {
            name: "pet".to_string(),
            schema: Schema::English,
            behavior_schema: Schema::English,
            image_root: String::new(),
            actions_file: "actions.xml".to_string(),
            behaviors_file: "behaviors.xml".to_string(),
            info_file: None,
            constants: OrderedMap::new(),
            actions,
            behaviors: OrderedMap::new(),
            behavior_tree: Vec::new(),
            info: Information::default(),
        };
        let images = vec![
            DecodedImage {
                path: "img/pet.png".to_string(),
                width: 4,
                height: 1,
                rgba: rgba_column(4, 2, 255),
            },
            DecodedImage {
                path: "img/pet-r.png".to_string(),
                width: 4,
                height: 1,
                rgba: rgba_column(4, 0, 200),
            },
            DecodedImage {
                path: "img/blank.png".to_string(),
                width: 4,
                height: 1,
                rgba: vec![0u8; 16],
            },
        ];
        let table = PoseTable::new(&character, &images);
        assert_eq!(table.drop_headroom(1.0), 1);
        assert_eq!(table.drop_headroom(0.5), 1);
        assert_eq!(table.drop_headroom(2.0), 2);
        assert_eq!(
            table.lookup(Some("/pet.png")).opaque,
            OpaqueSpan { left: 2, right: 3 }
        );
        assert_eq!(
            table.image_span("img/pet-r.png"),
            Some(OpaqueSpan { left: 0, right: 1 })
        );
        assert_eq!(
            table.lookup(Some("img/blank.png")).opaque,
            OpaqueSpan::full(4)
        );
    }
}
