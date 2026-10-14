//! Placement templates measured from the bundled shime frames.
//!
//! The bounding box and the contact edge come from the PNG alpha. The anchor
//! is shime's primary anchor (`standard_atlas`): the image does not store it,
//! and the classic XML is what the engine places against.

use std::fs;
use std::path::{Path, PathBuf};

use shimeji_packs::standard_atlas::{self, CELL};

use crate::raster::{self, BBox, OPAQUE, RgbaImage};

/// Which side of the cell the pose is built against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameTemplate {
    pub index: u16,
    pub name: String,
    pub bbox: BBox,
    pub edge: Edge,
    /// Contact line in cell pixels. Bottom is exclusive (feet at 128). Top is the first ink row.
    pub contact_at: i32,
    pub anchor_x: i32,
    pub anchor_y: i32,
    /// Pixels from the crown to the eye line. Zero when the frame has no readable head.
    pub head_to_eye: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateSet {
    pub frames: Vec<FrameTemplate>,
}

impl TemplateSet {
    pub fn get(&self, index: u16) -> Option<&FrameTemplate> {
        self.frames.iter().find(|frame| frame.index == index)
    }
}

pub fn bundled_shime_img() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/packs/shime/img")
}

pub fn assets_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets")
}

pub fn templates_path() -> PathBuf {
    assets_dir().join("templates.toml")
}

pub fn pose_guides_dir() -> PathBuf {
    assets_dir().join("pose-guides")
}

/// Measure all 46 frames. The result is deterministic for a given PNG set.
pub fn measure_dir(dir: &Path) -> Result<TemplateSet, String> {
    let mut frames = Vec::with_capacity(46);
    for index in 1u16..=46 {
        let path = dir.join(format!("shime{index}.png"));
        let image = raster::decode_png(&path)?;
        let pose = standard_atlas::by_index(index)
            .ok_or_else(|| format!("no atlas pose for shime{index}"))?;
        let box_ = image
            .bbox()
            .ok_or_else(|| format!("shime{index} has no opaque pixels"))?;
        let (edge, contact_at) = contact_edge(&box_, image.width, image.height);
        let head_to_eye = raster::head_to_eye(&image).unwrap_or(0);
        frames.push(FrameTemplate {
            index,
            name: pose.name.to_string(),
            bbox: box_,
            edge,
            contact_at,
            anchor_x: pose.anchor_x,
            anchor_y: pose.anchor_y,
            head_to_eye,
        });
    }
    Ok(TemplateSet { frames })
}

/// Ceiling frames sit just under the top of the cell, reach the left edge,
/// and do not stand on the floor. Every other classic frame is classified
/// from whichever edge its ink actually touches.
pub fn contact_edge(box_: &BBox, width: u32, height: u32) -> (Edge, i32) {
    let top = box_.y as i32;
    let bottom_gap = height as i32 - (box_.y + box_.h) as i32;
    let left = box_.x as i32;
    let right_gap = width as i32 - (box_.x + box_.w) as i32;
    let ceiling = (15..=30).contains(&top) && bottom_gap >= 14 && left <= 2 && box_.w >= 100;
    if ceiling {
        return (Edge::Top, top);
    }
    if bottom_gap <= 2 {
        return (Edge::Bottom, (box_.y + box_.h) as i32);
    }
    if left <= 2 && bottom_gap > 8 && right_gap > 2 {
        return (Edge::Left, left);
    }
    if right_gap <= 2 && bottom_gap > 8 && left > 2 {
        return (Edge::Right, (width as i32) - right_gap);
    }
    (Edge::Bottom, (box_.y + box_.h) as i32)
}

pub fn render_toml(set: &TemplateSet) -> String {
    let mut out = String::from(
        "# Measured from assets/packs/shime/img by shimeji-forge.\n\
         # Bounding box and contact edge are the PNG alpha. Anchors are shime's primary anchors.\n\
         # cell = 128\n\n",
    );
    for frame in &set.frames {
        out.push_str("[[frame]]\n");
        out.push_str(&format!("index = {}\n", frame.index));
        out.push_str(&format!("name = \"{}\"\n", frame.name));
        out.push_str(&format!("x = {}\n", frame.bbox.x));
        out.push_str(&format!("y = {}\n", frame.bbox.y));
        out.push_str(&format!("w = {}\n", frame.bbox.w));
        out.push_str(&format!("h = {}\n", frame.bbox.h));
        out.push_str(&format!("contact = \"{}\"\n", frame.edge.as_str()));
        out.push_str(&format!("contact_at = {}\n", frame.contact_at));
        out.push_str(&format!("anchor_x = {}\n", frame.anchor_x));
        out.push_str(&format!("anchor_y = {}\n", frame.anchor_y));
        out.push_str(&format!("head_to_eye = {}\n\n", frame.head_to_eye));
    }
    let _ = CELL;
    out
}

pub fn parse_toml(text: &str) -> Result<TemplateSet, String> {
    let mut frames = Vec::new();
    let mut cur: Option<FrameTemplate> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[frame]]" {
            if let Some(frame) = cur.take() {
                frames.push(frame);
            }
            cur = Some(FrameTemplate {
                index: 0,
                name: String::new(),
                bbox: BBox {
                    x: 0,
                    y: 0,
                    w: 0,
                    h: 0,
                },
                edge: Edge::Bottom,
                contact_at: 0,
                anchor_x: 0,
                anchor_y: 0,
                head_to_eye: 0,
            });
            continue;
        }
        let frame = cur
            .as_mut()
            .ok_or_else(|| format!("key outside a frame: {line}"))?;
        let (key, value) = split_kv(line)?;
        match key {
            "index" => frame.index = parse_u16(value)?,
            "name" => frame.name = parse_string(value)?,
            "x" => frame.bbox.x = parse_u32(value)?,
            "y" => frame.bbox.y = parse_u32(value)?,
            "w" => frame.bbox.w = parse_u32(value)?,
            "h" => frame.bbox.h = parse_u32(value)?,
            "contact" => {
                frame.edge = Edge::parse(&parse_string(value)?)
                    .ok_or_else(|| format!("unknown contact {value}"))?;
            }
            "contact_at" => frame.contact_at = parse_i32(value)?,
            "anchor_x" => frame.anchor_x = parse_i32(value)?,
            "anchor_y" => frame.anchor_y = parse_i32(value)?,
            "head_to_eye" => frame.head_to_eye = parse_u32(value)?,
            other => return Err(format!("unknown template key {other}")),
        }
    }
    if let Some(frame) = cur {
        frames.push(frame);
    }
    if frames.len() != 46 {
        return Err(format!("expected 46 frames, found {}", frames.len()));
    }
    Ok(TemplateSet { frames })
}

pub fn load_templates(path: &Path) -> Result<TemplateSet, String> {
    let text = fs::read_to_string(path).map_err(|err| err.to_string())?;
    parse_toml(&text)
}

fn split_kv(line: &str) -> Result<(&str, &str), String> {
    let (key, value) = line
        .split_once('=')
        .ok_or_else(|| format!("expected key = value, got {line}"))?;
    Ok((key.trim(), value.trim()))
}

fn parse_string(value: &str) -> Result<String, String> {
    let value = value.trim();
    if let Some(inner) = value.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Ok(inner.to_string())
    } else {
        Err(format!("expected a quoted string, got {value}"))
    }
}

fn parse_u32(value: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|_| format!("expected an integer, got {value}"))
}

fn parse_u16(value: &str) -> Result<u16, String> {
    value
        .parse()
        .map_err(|_| format!("expected an integer, got {value}"))
}

fn parse_i32(value: &str) -> Result<i32, String> {
    value
        .parse()
        .map_err(|_| format!("expected an integer, got {value}"))
}

/// Flat mannequin: the shime silhouette filled with one grey, outlined a step darker.
pub fn mannequin(src: &RgbaImage) -> RgbaImage {
    const FILL: [u8; 4] = [186, 186, 190, 255];
    const OUTLINE: [u8; 4] = [90, 90, 96, 255];
    let mut out = RgbaImage::new(src.width, src.height);
    for y in 0..src.height {
        for x in 0..src.width {
            if src.get(x, y)[3] < OPAQUE {
                continue;
            }
            let edge = neighbor_clear(src, x, y);
            out.set(x, y, if edge { OUTLINE } else { FILL });
        }
    }
    out
}

fn neighbor_clear(src: &RgbaImage, x: u32, y: u32) -> bool {
    for (dx, dy) in [(0i32, -1), (0, 1), (-1, 0), (1, 0)] {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= src.width as i32 || ny >= src.height as i32 {
            return true;
        }
        if src.get(nx as u32, ny as u32)[3] < OPAQUE {
            return true;
        }
    }
    false
}

pub fn render_poses_toml() -> String {
    let mut out = String::from(
        "# One line per classic frame. Slot contact rules for the group prompts live with these.\n\n",
    );
    for (offset, text) in POSE_TEXT.iter().enumerate() {
        let index = (offset as u16) + 1;
        let name = standard_atlas::by_index(index)
            .map(|p| p.name)
            .unwrap_or("pose");
        out.push_str("[[pose]]\n");
        out.push_str(&format!("index = {index}\n"));
        out.push_str(&format!("name = \"{name}\"\n"));
        out.push_str(&format!("text = \"{text}\"\n\n"));
    }
    out
}

pub fn write_pose_guides(shime_dir: &Path, out_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(out_dir).map_err(|err| err.to_string())?;
    for index in 1u16..=46 {
        let src = raster::decode_png(&shime_dir.join(format!("shime{index}.png")))?;
        let guide = mannequin(&src);
        raster::save_png(&out_dir.join(format!("shime{index}.png")), &guide)?;
    }
    fs::write(out_dir.join("poses.toml"), render_poses_toml()).map_err(|err| err.to_string())?;
    Ok(())
}

/// Pose-guide strip for one group, left to right, one cell per frame.
pub fn guide_strip(guide_dir: &Path, frames: &[u16]) -> Result<RgbaImage, String> {
    if frames.is_empty() {
        return Err("group has no frames".to_string());
    }
    let mut cells = Vec::new();
    for index in frames {
        let path = guide_dir.join(format!("shime{index}.png"));
        cells.push(raster::decode_png(&path)?);
    }
    let cell_w = cells.iter().map(|c| c.width).max().unwrap_or(CELL);
    let cell_h = cells.iter().map(|c| c.height).max().unwrap_or(CELL);
    let mut strip = RgbaImage::new(cell_w * frames.len() as u32, cell_h);
    for (i, cell) in cells.iter().enumerate() {
        strip.blit(cell, (i as u32 * cell_w) as i32, 0);
    }
    Ok(strip)
}

pub const POSE_TEXT: [&str; 46] = [
    "Standing still, relaxed. Feet on the bottom edge.",
    "Walking, left foot forward. Feet on the bottom edge. Not a copy of the other walk frame.",
    "Walking, right foot forward. Feet on the bottom edge. The opposite step from the other walk frame.",
    "Falling, body tilted, sleeves and hair up. Feet off the ground.",
    "Held and protesting, waving one way. Feet hang free.",
    "Held and protesting, waving the other way. Feet hang free.",
    "Hanging straight down from a pinch at the top of the head. The top of the head touches the top edge.",
    "Hanging and swinging left. The top of the head touches the top edge.",
    "Hanging and swinging right. The top of the head touches the top edge.",
    "Hanging and dangling. The top of the head touches the top edge.",
    "Sitting on the ground, looking ahead. Seat on the bottom edge. The body stays still through the head turn.",
    "Clinging to a wall on the right side of the cell. Hands and feet toward the right edge.",
    "Grabbing the wall, body close to the right edge.",
    "Climbing one step up the wall, still against the right edge.",
    "Seated, head turned up-left. Only the head moves. Seat on the bottom edge.",
    "Seated, head turned left. Only the head moves. Seat on the bottom edge.",
    "Seated, head turned down-left. Only the head moves. Seat on the bottom edge.",
    "Low bounce, body near the bottom edge.",
    "Rising out of the bounce.",
    "Crawling, arms reaching forward. Body on the bottom edge.",
    "Crawling, pulling the body forward. Body on the bottom edge.",
    "Jumping, one knee raised. Feet off the ground.",
    "Hanging from the ceiling. Hands on the top edge, body dangling.",
    "Swinging from the ceiling to one side. Hands stay on the top edge.",
    "Swinging from the ceiling to the other side. Hands stay on the top edge.",
    "Sitting, looking up. Seat on the bottom edge.",
    "Seated, head turned down. Only the head moves. Seat on the bottom edge.",
    "Seated, head turned down-right. Only the head moves. Seat on the bottom edge.",
    "Seated, head turned right. Only the head moves. Seat on the bottom edge.",
    "Sitting on a ledge, legs together. The seat is about an eighth of the cell up from the bottom; the legs hang below it.",
    "Sitting on a ledge, legs swinging forward. Same seat line.",
    "Sitting on a ledge, legs swinging back. Same seat line.",
    "Sitting on a ledge, legs swinging forward again. Same seat line.",
    "Walking while carrying something overhead. Feet on the bottom edge. The carried object is clipped in later; do not draw it.",
    "The other step of that walk. Feet on the bottom edge. Do not draw the carried object.",
    "Carrying while falling. Do not draw the carried object.",
    "Throwing, a forward flick. Feet on the bottom edge. Do not draw the object.",
    "Pulling a companion up: only their head and shoulders above the bottom edge. No ground mound.",
    "Pulling: the companion is up to the waist. No ground mound.",
    "Pulling: the companion is up to the knees. No ground mound.",
    "Pulling: the companion is out and finding their balance. No ground mound.",
    "Splitting apart, two copies of the standing pose just separating. Derived from the standing frame.",
    "Splitting, the copies further apart.",
    "Splitting, the copies near the sides of the cell.",
    "Splitting, the copies at the edges.",
    "Splitting, the copies fully apart.",
];

pub fn pose_text(index: u16) -> Option<&'static str> {
    POSE_TEXT.get(usize::from(index.wrapping_sub(1))).copied()
}
