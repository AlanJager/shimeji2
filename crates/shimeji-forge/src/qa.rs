//! Blocking checks from research note 4 §4.4.
//!
//! Export stays closed while any check fails, unless the caller passes an
//! override. The override is recorded; it is not a silent skip.

use serde::Serialize;

use crate::raster::{self, RgbaImage};
use crate::templates::{Edge, TemplateSet};

/// Height spread inside a family, relative to the median. 15% fails; under 8% passes.
pub const SIZE_LIMIT: f32 = 0.08;
pub const FRINGE_LIMIT: f32 = 0.01;
pub const RESIDUE_LIMIT: f32 = 0.01;
pub const BASELINE_LIMIT: i32 = 2;

/// Walk frames that must not be copies, and the seated head-turn that must keep moving.
const WALK_PAIR: (u16, u16) = (2, 3);
const HEAD_SPIN: [u16; 8] = [11, 26, 15, 27, 16, 28, 17, 29];

#[derive(Debug, Clone)]
pub struct QaFrame {
    pub slot: usize,
    pub index: Option<u16>,
    pub image: RgbaImage,
    pub column_x0: u32,
    pub column_y0: u32,
    pub column_x1: u32,
    pub column_y1: u32,
    pub origin_x: u32,
    pub origin_y: u32,
    /// Canvas origin shift. A frame that grew past the cell is padded, and the
    /// contact line in that image is `template.contact_at` plus this pad.
    pub pad_x: i32,
    pub pad_y: i32,
}

impl QaFrame {
    /// A frame that already fills its own cell, so the boundary check passes.
    pub fn lone(slot: usize, index: Option<u16>, image: RgbaImage) -> Self {
        let width = image.width;
        let height = image.height;
        Self {
            slot,
            index,
            image,
            column_x0: 0,
            column_y0: 0,
            column_x1: width.max(1),
            column_y1: height.max(1),
            origin_x: 0,
            origin_y: 0,
            pad_x: 0,
            pad_y: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QaInput {
    pub key: [u8; 3],
    pub expected_slots: usize,
    pub frames: Vec<QaFrame>,
    pub templates: Option<TemplateSet>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SizeOutlier {
    pub slot: usize,
    pub index: Option<u16>,
    pub family: String,
    pub height: u32,
    pub median: f32,
    pub ratio: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaselineError {
    pub slot: usize,
    pub index: u16,
    pub edge: String,
    pub error_px: i32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicatePair {
    pub a: u16,
    pub b: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct FrameStat {
    pub slot: usize,
    pub index: Option<u16>,
    pub width: u32,
    pub height: u32,
    pub opaque: usize,
    pub empty: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct QaReport {
    pub passed: bool,
    pub expected_slots: usize,
    pub nonempty: usize,
    pub empty_slots: Vec<usize>,
    pub slot_count_pass: bool,
    pub boundary_pass: bool,
    pub size_pass: bool,
    pub size_outliers: Vec<SizeOutlier>,
    pub baseline_pass: bool,
    pub baseline_errors: Vec<BaselineError>,
    pub residue_ratio: f32,
    pub residue_pass: bool,
    pub fringe_ratio: f32,
    pub fringe_pass: bool,
    pub duplicate_pass: bool,
    pub duplicate_pairs: Vec<DuplicatePair>,
    pub frames: Vec<FrameStat>,
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverrideRecord {
    pub failed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportDecision {
    pub allowed: bool,
    pub override_recorded: Option<OverrideRecord>,
}

pub fn qa(input: &QaInput) -> QaReport {
    let stats: Vec<FrameStat> = input
        .frames
        .iter()
        .map(|frame| {
            let box_ = frame.image.bbox();
            let empty = frame.image.opaque_count() == 0;
            FrameStat {
                slot: frame.slot,
                index: frame.index,
                width: box_.map(|b| b.w).unwrap_or(0),
                height: box_.map(|b| b.h).unwrap_or(0),
                opaque: frame.image.opaque_count(),
                empty,
            }
        })
        .collect();
    let empty_slots: Vec<usize> = stats
        .iter()
        .filter(|stat| stat.empty)
        .map(|stat| stat.slot)
        .collect();
    let nonempty = stats.len() - empty_slots.len();
    let slot_count_pass = input.frames.len() == input.expected_slots && empty_slots.is_empty();
    let boundary_pass = input.frames.iter().all(inside_column);

    let mut size_outliers = Vec::new();
    let mut families: Vec<(String, Vec<usize>)> = Vec::new();
    for (offset, frame) in input.frames.iter().enumerate() {
        if stats[offset].empty {
            continue;
        }
        let name = family_name(frame.index);
        if let Some((_, slots)) = families.iter_mut().find(|(n, _)| *n == name) {
            slots.push(offset);
        } else {
            families.push((name, vec![offset]));
        }
    }
    for (name, members) in &families {
        if members.len() < 2 {
            continue;
        }
        let heights: Vec<u32> = members.iter().map(|i| stats[*i].height).collect();
        let median = median(&heights);
        if median <= 0.0 {
            continue;
        }
        for index in members {
            let height = stats[*index].height;
            let ratio = (height as f32 - median).abs() / median;
            if ratio > SIZE_LIMIT {
                size_outliers.push(SizeOutlier {
                    slot: stats[*index].slot,
                    index: stats[*index].index,
                    family: name.clone(),
                    height,
                    median,
                    ratio,
                });
            }
        }
    }
    let size_pass = size_outliers.is_empty();

    let mut baseline_errors = Vec::new();
    if let Some(templates) = &input.templates {
        for frame in &input.frames {
            let Some(index) = frame.index else {
                continue;
            };
            let Some(template) = templates.get(index) else {
                continue;
            };
            let Some(mut actual) = contact_line(&frame.image, template.edge) else {
                continue;
            };
            actual -= match template.edge {
                Edge::Left | Edge::Right => frame.pad_x,
                Edge::Top | Edge::Bottom => frame.pad_y,
            };
            let error_px = (actual - template.contact_at).abs();
            if error_px > BASELINE_LIMIT {
                baseline_errors.push(BaselineError {
                    slot: frame.slot,
                    index,
                    edge: template.edge.as_str().to_string(),
                    error_px,
                });
            }
        }
    }
    let baseline_pass = baseline_errors.is_empty();

    let mut residue_weighted = 0.0f32;
    let mut residue_opaque = 0.0f32;
    let mut fringe_weighted = 0.0f32;
    let mut fringe_edges = 0.0f32;
    for frame in &input.frames {
        let opaque = frame.image.opaque_count() as f32;
        if opaque == 0.0 {
            continue;
        }
        residue_weighted += raster::residue_ratio(&frame.image, input.key) * opaque;
        residue_opaque += opaque;
        let edges = opaque.max(1.0);
        fringe_weighted += raster::fringe_ratio(&frame.image, input.key) * edges;
        fringe_edges += edges;
    }
    let residue_ratio = if residue_opaque == 0.0 {
        0.0
    } else {
        residue_weighted / residue_opaque
    };
    let fringe_ratio = if fringe_edges == 0.0 {
        0.0
    } else {
        fringe_weighted / fringe_edges
    };
    let residue_pass = residue_ratio < RESIDUE_LIMIT;
    let fringe_pass = fringe_ratio < FRINGE_LIMIT;

    let duplicate_pairs = duplicate_pairs(&input.frames);
    let duplicate_pass = duplicate_pairs.is_empty();

    let mut failed = Vec::new();
    if !slot_count_pass {
        failed.push("slot-count".to_string());
    }
    if !boundary_pass {
        failed.push("boundary".to_string());
    }
    if !size_pass {
        failed.push("size".to_string());
    }
    if !baseline_pass {
        failed.push("baseline".to_string());
    }
    if !residue_pass {
        failed.push("residue".to_string());
    }
    if !fringe_pass {
        failed.push("fringe".to_string());
    }
    if !duplicate_pass {
        failed.push("duplicates".to_string());
    }
    let passed = failed.is_empty();
    QaReport {
        passed,
        expected_slots: input.expected_slots,
        nonempty,
        empty_slots,
        slot_count_pass,
        boundary_pass,
        size_pass,
        size_outliers,
        baseline_pass,
        baseline_errors,
        residue_ratio,
        residue_pass,
        fringe_ratio,
        fringe_pass,
        duplicate_pass,
        duplicate_pairs,
        frames: stats,
        failed,
    }
}

fn inside_column(frame: &QaFrame) -> bool {
    if frame.image.opaque_count() == 0 {
        return true;
    }
    let right = frame.origin_x + frame.image.width;
    let bottom = frame.origin_y + frame.image.height;
    frame.origin_x >= frame.column_x0
        && frame.origin_y >= frame.column_y0
        && right <= frame.column_x1
        && bottom <= frame.column_y1
}

fn contact_line(image: &RgbaImage, edge: Edge) -> Option<i32> {
    let box_ = image.bbox()?;
    Some(match edge {
        Edge::Top => box_.y as i32,
        Edge::Bottom => (box_.y + box_.h) as i32,
        Edge::Left => box_.x as i32,
        Edge::Right => (box_.x + box_.w) as i32,
    })
}

pub fn family_name(index: Option<u16>) -> String {
    let Some(index) = index else {
        return "sheet".to_string();
    };
    let name = match index {
        1..=3 => "walk",
        4 | 22 => "air",
        5 | 6 => "resist",
        7..=10 => "pinched",
        11 | 15..=17 | 26..=29 => "sit",
        12..=14 => "wall",
        18 | 19 => "bounce",
        20 | 21 => "crawl",
        23..=25 => "ceiling",
        30..=33 => "ledge",
        34..=37 => "carry",
        38..=41 => "pull",
        42..=46 => "split",
        _ => "sheet",
    };
    name.to_string()
}

fn median(values: &[u32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2] as f32
    } else {
        (sorted[n / 2 - 1] as f32 + sorted[n / 2] as f32) / 2.0
    }
}

fn duplicate_pairs(frames: &[QaFrame]) -> Vec<DuplicatePair> {
    let mut pairs = Vec::new();
    if let (Some(a), Some(b)) = (
        find_index(frames, WALK_PAIR.0),
        find_index(frames, WALK_PAIR.1),
    ) {
        if same_sprite(&frames[a].image, &frames[b].image) {
            pairs.push(DuplicatePair {
                a: WALK_PAIR.0,
                b: WALK_PAIR.1,
            });
        }
    }
    let spin: Vec<usize> = HEAD_SPIN
        .iter()
        .filter_map(|index| find_index(frames, *index))
        .collect();
    for i in 0..spin.len() {
        for j in (i + 1)..spin.len() {
            if same_sprite(&frames[spin[i]].image, &frames[spin[j]].image) {
                let a = frames[spin[i]].index.unwrap_or(0);
                let b = frames[spin[j]].index.unwrap_or(0);
                pairs.push(DuplicatePair { a, b });
            }
        }
    }
    pairs
}

fn find_index(frames: &[QaFrame], index: u16) -> Option<usize> {
    frames.iter().position(|frame| frame.index == Some(index))
}

pub fn same_sprite(a: &RgbaImage, b: &RgbaImage) -> bool {
    match (a.bbox(), b.bbox()) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            let ca = a.crop(left);
            let cb = b.crop(right);
            ca.width == cb.width && ca.height == cb.height && ca.rgba == cb.rgba
        }
        _ => false,
    }
}

/// A failing report blocks export. An override allows it and is recorded.
pub fn decide_export(report: &QaReport, override_flag: bool) -> ExportDecision {
    if report.passed {
        return ExportDecision {
            allowed: true,
            override_recorded: None,
        };
    }
    if override_flag {
        ExportDecision {
            allowed: true,
            override_recorded: Some(OverrideRecord {
                failed: report.failed.clone(),
            }),
        }
    } else {
        ExportDecision {
            allowed: false,
            override_recorded: None,
        }
    }
}
