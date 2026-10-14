//! Deterministic half of pack generation: templates, extraction, placement, QA.
//!
//! Live calls to paid image APIs are stubs. Agent mode is files in, files out.

pub mod derive;
pub mod extract;
pub mod install;
pub mod jobs;
pub mod place;
pub mod provenance;
pub mod provider;
pub mod qa;
pub mod raster;
pub mod review;
pub mod templates;

pub use derive::{
    CLIP_FRAMES, ClipLine, DeriveReport, SPLIT_OFFSETS, SplitOffset, clip_frame, clip_lines,
    derive_pack, scaled_offset, split_frame,
};
pub use extract::{
    Cell, Extracted, ExtractedFrame, KeyKind, apply_key, detect_key, extract, extract_grid,
    frame_contains_color, frames_inside_cells,
};
pub use install::{InstallReport, install_with_home};
pub use jobs::{
    CHARACTER_SHEETS, CharacterSheet, GROUPS, Group, character_sheet, group, prompts_dir,
};
pub use place::{PlacedFrame, Placement, ScaleMethod, SheetFrame, place};
pub use provenance::{
    NOTICE, Provenance, date_from_unix_days, provenance_value, today_utc, write_provenance,
};
pub use provider::{
    AgentMode, Background, Gemini, GenerateRequest, ImageProvider, OpenAi, ProviderError,
};
pub use qa::{
    ExportDecision, QaFrame, QaInput, QaReport, decide_export, family_name, qa, same_sprite,
};
pub use raster::{
    BBox, OPAQUE, RgbaImage, decode_png, despeckle, encode_png, face_x, fringe_ratio, head_to_eye,
    residue_ratio, save_png,
};
pub use review::{ReviewOutput, delay_cs, read_all_labels, read_label, review};
pub use templates::{
    Edge, FrameTemplate, POSE_TEXT, TemplateSet, bundled_shime_img, contact_edge, load_templates,
    mannequin, measure_dir, parse_toml, pose_guides_dir, pose_text, render_poses_toml, render_toml,
    templates_path, write_pose_guides,
};

/// Names of the two workspace crates forge is allowed to depend on.
pub fn linked_crates() -> (&'static str, &'static str) {
    (shimeji_packs::crate_name(), shimeji_proto::crate_name())
}
