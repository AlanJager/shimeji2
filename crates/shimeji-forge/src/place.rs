//! One scale per sheet, then each frame sits on its template's contact edge.
//!
//! A standing group takes its scale from the tallest upright frame (feet on
//! the floor, nothing treated as a jump or a fall). Every other group takes
//! it from the head-top-to-eye distance. Frames of one action then share a
//! head x, so a reaching limb does not drag the face sideways.

use shimeji_packs::standard_atlas::CELL;

use crate::raster::{self, RgbaImage};
use crate::templates::{Edge, FrameTemplate, TemplateSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleMethod {
    TallestUpright,
    HeadToEye,
    HeightRatio,
}

impl ScaleMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TallestUpright => "tallest-upright",
            Self::HeadToEye => "head-to-eye",
            Self::HeightRatio => "height-ratio",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SheetFrame {
    pub index: u16,
    pub image: RgbaImage,
}

#[derive(Debug, Clone)]
pub struct PlacedFrame {
    pub index: u16,
    pub image: RgbaImage,
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub content_height: u32,
}

#[derive(Debug, Clone)]
pub struct Placement {
    pub scale: f32,
    pub method: ScaleMethod,
    pub reference: u16,
    pub frames: Vec<PlacedFrame>,
    pub warnings: Vec<String>,
}

impl Placement {
    pub fn frame(&self, index: u16) -> Option<&PlacedFrame> {
        self.frames.iter().find(|frame| frame.index == index)
    }
}

struct Prepared {
    index: u16,
    scaled: RgbaImage,
    template: FrameTemplate,
    dest_x: i32,
    dest_y: i32,
}

pub fn place(frames: &[SheetFrame], templates: &TemplateSet) -> Result<Placement, String> {
    if frames.is_empty() {
        return Err("nothing to place".to_string());
    }
    let (scale, method, reference) = choose_scale(frames, templates)?;
    let mut prepared = Vec::with_capacity(frames.len());
    for frame in frames {
        let template = templates
            .get(frame.index)
            .ok_or_else(|| format!("no template for frame {}", frame.index))?
            .clone();
        let scaled = if frame.image.opaque_count() == 0 {
            RgbaImage::empty()
        } else {
            let grown = frame.image.scale_nn(scale);
            let cleaned = raster::despeckle(&grown);
            match cleaned.bbox() {
                Some(box_) => cleaned.crop(box_),
                None => RgbaImage::empty(),
            }
        };
        let (dest_x, dest_y) = dest_for(&scaled, &template);
        prepared.push(Prepared {
            index: frame.index,
            scaled,
            template,
            dest_x,
            dest_y,
        });
    }
    align_heads(&mut prepared, reference);
    let mut warnings = Vec::new();
    let mut placed = Vec::with_capacity(prepared.len());
    for item in prepared {
        let (image, anchor_x, anchor_y, grew) = paint(&item);
        if grew {
            warnings.push(format!(
                "frame {} extends past the {} cell",
                item.index, CELL
            ));
        }
        let content_height = image.bbox().map(|b| b.h).unwrap_or(0);
        placed.push(PlacedFrame {
            index: item.index,
            image,
            anchor_x,
            anchor_y,
            content_height,
        });
    }
    Ok(Placement {
        scale,
        method,
        reference,
        frames: placed,
        warnings,
    })
}

fn choose_scale(
    frames: &[SheetFrame],
    templates: &TemplateSet,
) -> Result<(f32, ScaleMethod, u16), String> {
    let mut upright: Option<(u16, u32, u32)> = None;
    for frame in frames {
        let Some(template) = templates.get(frame.index) else {
            continue;
        };
        let Some(box_) = frame.image.bbox() else {
            continue;
        };
        let stands = template.edge == Edge::Bottom
            && template.contact_at >= (CELL as i32) - 2
            && template.bbox.h >= 100
            && box_.h > box_.w
            && box_.h >= 16;
        if !stands {
            continue;
        }
        let replace = match upright {
            None => true,
            Some((_, _, height)) => box_.h > height,
        };
        if replace {
            upright = Some((frame.index, template.bbox.h, box_.h));
        }
    }
    if let Some((index, target, source)) = upright {
        return Ok((
            target as f32 / source as f32,
            ScaleMethod::TallestUpright,
            index,
        ));
    }

    let mut eye: Option<(u16, f32)> = None;
    for frame in frames {
        let Some(template) = templates.get(frame.index) else {
            continue;
        };
        if template.head_to_eye == 0 {
            continue;
        }
        let Some(dist) = raster::head_to_eye(&frame.image) else {
            continue;
        };
        if dist == 0 {
            continue;
        }
        let factor = template.head_to_eye as f32 / dist as f32;
        // The first readable head sets the sheet. Later frames share it.
        if eye.is_none() {
            eye = Some((frame.index, factor));
        }
    }
    if let Some((index, factor)) = eye {
        return Ok((factor, ScaleMethod::HeadToEye, index));
    }

    let mut best: Option<(u16, u32, u32)> = None;
    for frame in frames {
        let Some(template) = templates.get(frame.index) else {
            continue;
        };
        let Some(box_) = frame.image.bbox() else {
            continue;
        };
        let replace = match best {
            None => true,
            Some((_, height, _)) => template.bbox.h > height,
        };
        if replace {
            best = Some((frame.index, template.bbox.h, box_.h));
        }
    }
    let (index, target, source) =
        best.ok_or_else(|| "no opaque frame to scale from".to_string())?;
    if source == 0 {
        return Err("scale source has no height".to_string());
    }
    Ok((
        target as f32 / source as f32,
        ScaleMethod::HeightRatio,
        index,
    ))
}

fn dest_for(scaled: &RgbaImage, template: &FrameTemplate) -> (i32, i32) {
    if scaled.is_empty() {
        return (0, 0);
    }
    let dest_y = match template.edge {
        Edge::Top => template.contact_at,
        Edge::Bottom | Edge::Left | Edge::Right => template.contact_at - scaled.height as i32,
    };
    let template_cx = template.bbox.x as i32 + template.bbox.w as i32 / 2;
    let dest_x = template_cx - scaled.width as i32 / 2;
    let dest_x = match template.edge {
        Edge::Left => template.contact_at,
        Edge::Right => template.contact_at - scaled.width as i32,
        Edge::Top | Edge::Bottom => dest_x,
    };
    (dest_x, dest_y)
}

fn align_heads(frames: &mut [Prepared], reference: u16) {
    let Some(reference_at) = frames.iter().position(|frame| frame.index == reference) else {
        return;
    };
    let Some(reference_head) = head_in_cell(&frames[reference_at]) else {
        return;
    };
    for frame in frames.iter_mut() {
        if frame.index == reference || frame.scaled.is_empty() {
            continue;
        }
        let Some(head) = head_in_cell(frame) else {
            continue;
        };
        let delta = (reference_head - head).round() as i32;
        frame.dest_x += delta;
    }
}

fn head_in_cell(frame: &Prepared) -> Option<f32> {
    let local = raster::face_x(&frame.scaled)?;
    Some(frame.dest_x as f32 + local)
}

fn paint(item: &Prepared) -> (RgbaImage, i32, i32, bool) {
    if item.scaled.is_empty() {
        return (
            RgbaImage::new(CELL, CELL),
            item.template.anchor_x,
            item.template.anchor_y,
            false,
        );
    }
    let x0 = item.dest_x.min(0);
    let y0 = item.dest_y.min(0);
    let x1 = item
        .dest_x
        .saturating_add(item.scaled.width as i32)
        .max(CELL as i32);
    let y1 = item
        .dest_y
        .saturating_add(item.scaled.height as i32)
        .max(CELL as i32);
    let pad_x = -x0;
    let pad_y = -y0;
    let width = (x1 - x0).max(1) as u32;
    let height = (y1 - y0).max(1) as u32;
    let grew = width > CELL || height > CELL;
    let mut canvas = RgbaImage::new(width, height);
    canvas.blit(&item.scaled, item.dest_x + pad_x, item.dest_y + pad_y);
    let anchor_x = item.template.anchor_x + pad_x;
    let anchor_y = item.template.anchor_y + pad_y;
    (canvas, anchor_x, anchor_y, grew)
}
