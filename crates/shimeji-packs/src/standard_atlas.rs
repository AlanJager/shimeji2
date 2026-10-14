//! Shimeji Standard Atlas v1 (`shimeji-standard-v1`).
//!
//! An 8×6 grid of 128×128 cells. Cell `i` (row-major, 0-based) is classic
//! `shime{i+1}.png`. Cells 47 and 48 are reserved and have no pose. The pose
//! names and the default anchor of each pose come from research note 3 and
//! from the bundled shime: feet sit at `(64, 128)` except where shime's own
//! primary anchor differs (ceiling, dangle, pull-out). A native manifest
//! anchor is a delta from that default, so a per-use anchor in the classic
//! XML keeps its offset.
//!
//! Frames 23–25 are the ceiling poses. Their contact edge is the top of the
//! cell (shime's anchor is `(64, 48)`, the hand-hold, not the feet).

/// Columns in the grid.
pub const COLUMNS: u32 = 8;
/// Rows in the grid.
pub const ROWS: u32 = 6;
/// Logical cell edge, in pixels. `@2x` art is double this and still one cell.
pub const CELL: u32 = 128;
/// Poses that have a name. Cells 47 and 48 are reserved.
pub const POSE_COUNT: u16 = 46;

/// Which edge of the cell the pose is authored against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Contact {
    /// Feet, at the bottom of the cell.
    Bottom,
    /// Hands on a ceiling. Frames 23–25.
    Top,
}

/// One named cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasPose {
    /// 1-based classic index (`shime{index}.png`).
    pub index: u16,
    pub name: &'static str,
    /// Extra names for the same cell (`lie` for crawl-b, and so on).
    pub aliases: &'static [&'static str],
    /// Shime's primary anchor. Per-use anchors in the XML are deltas from this.
    pub anchor_x: i32,
    pub anchor_y: i32,
    pub contact: Contact,
}

const NONE: &[&str] = &[];

const fn pose(
    index: u16,
    name: &'static str,
    aliases: &'static [&'static str],
    anchor_x: i32,
    anchor_y: i32,
    contact: Contact,
) -> AtlasPose {
    AtlasPose {
        index,
        name,
        aliases,
        anchor_x,
        anchor_y,
        contact,
    }
}

const POSES: &[AtlasPose] = &[
    pose(1, "stand", NONE, 64, 128, Contact::Bottom),
    pose(2, "walk-a", NONE, 64, 128, Contact::Bottom),
    pose(3, "walk-b", NONE, 64, 128, Contact::Bottom),
    pose(4, "fall", NONE, 64, 128, Contact::Bottom),
    pose(5, "resist-a", NONE, 64, 128, Contact::Bottom),
    pose(6, "resist-b", NONE, 64, 128, Contact::Bottom),
    pose(7, "pinched-a", NONE, 64, 128, Contact::Bottom),
    pose(8, "pinched-b", NONE, 64, 128, Contact::Bottom),
    pose(9, "pinched-c", &["pullout-alt"], 64, 128, Contact::Bottom),
    pose(10, "pinched-d", NONE, 64, 128, Contact::Bottom),
    pose(11, "sit", NONE, 64, 128, Contact::Bottom),
    pose(12, "climb-a", NONE, 64, 128, Contact::Bottom),
    pose(13, "wall-grab", NONE, 64, 128, Contact::Bottom),
    pose(14, "climb-b", NONE, 64, 128, Contact::Bottom),
    pose(15, "spin-a", NONE, 64, 128, Contact::Bottom),
    pose(16, "spin-b", NONE, 64, 128, Contact::Bottom),
    pose(17, "spin-c", NONE, 64, 128, Contact::Bottom),
    pose(18, "bounce-a", NONE, 64, 128, Contact::Bottom),
    pose(19, "bounce-b", NONE, 64, 128, Contact::Bottom),
    pose(20, "crawl-a", NONE, 64, 128, Contact::Bottom),
    pose(21, "crawl-b", &["lie"], 64, 128, Contact::Bottom),
    pose(22, "jump", NONE, 64, 128, Contact::Bottom),
    pose(23, "ceiling-grab", NONE, 64, 48, Contact::Top),
    pose(24, "ceiling-a", NONE, 64, 48, Contact::Top),
    pose(25, "ceiling-b", NONE, 64, 48, Contact::Top),
    pose(26, "sit-look-up", NONE, 64, 128, Contact::Bottom),
    pose(27, "spin-up-a", NONE, 64, 128, Contact::Bottom),
    pose(28, "spin-up-b", NONE, 64, 128, Contact::Bottom),
    pose(29, "spin-up-c", NONE, 64, 128, Contact::Bottom),
    pose(30, "sit-relaxed", NONE, 64, 112, Contact::Bottom),
    pose(31, "dangle-a", NONE, 64, 112, Contact::Bottom),
    pose(32, "dangle-b", NONE, 64, 112, Contact::Bottom),
    pose(33, "dangle-c", NONE, 64, 112, Contact::Bottom),
    pose(34, "carry-a", NONE, 64, 128, Contact::Bottom),
    pose(35, "carry-b", NONE, 64, 128, Contact::Bottom),
    pose(36, "carry-c", &["carry-fall"], 64, 128, Contact::Bottom),
    pose(37, "throw", NONE, 64, 128, Contact::Bottom),
    pose(38, "pullout-1", NONE, 96, 128, Contact::Bottom),
    pose(39, "pullout-2", NONE, 96, 128, Contact::Bottom),
    pose(40, "pullout-3", NONE, 96, 128, Contact::Bottom),
    pose(41, "pullout-4", NONE, 96, 128, Contact::Bottom),
    pose(42, "split-1", NONE, 64, 128, Contact::Bottom),
    pose(43, "split-2", NONE, 64, 128, Contact::Bottom),
    pose(44, "split-3", NONE, 64, 128, Contact::Bottom),
    pose(45, "split-4", NONE, 64, 128, Contact::Bottom),
    pose(46, "split-5", NONE, 64, 128, Contact::Bottom),
];

/// The pose table, in classic index order. The slice is a process constant.
pub fn poses() -> &'static [AtlasPose] {
    POSES
}

/// Pose `index`, 1-based, or `None` for 0 and for the reserved cells.
pub fn by_index(index: u16) -> Option<&'static AtlasPose> {
    POSES.get(usize::from(index.checked_sub(1)?))
}

/// Primary name or alias.
pub fn by_name(name: &str) -> Option<&'static AtlasPose> {
    POSES
        .iter()
        .find(|pose| pose.name == name || pose.aliases.contains(&name))
}

/// Top-left of cell `index` (1-based, including the reserved cells 47 and 48).
pub fn cell_origin(index: u16) -> Option<(u32, u32)> {
    if index == 0 || u32::from(index) > COLUMNS * ROWS {
        return None;
    }
    let zero = u32::from(index - 1);
    Some(((zero % COLUMNS) * CELL, (zero / COLUMNS) * CELL))
}

/// Pixel size of one atlas. `@2x` is twice this on each edge.
pub fn atlas_size(scale: u32) -> (u32, u32) {
    (COLUMNS * CELL * scale, ROWS * CELL * scale)
}

/// Every name a non-empty cell contributes to capability detection.
pub fn names_of(pose: &AtlasPose) -> impl Iterator<Item = &'static str> {
    std::iter::once(pose.name).chain(pose.aliases.iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_stable_and_frames_23_to_25_contact_the_top() {
        let first = poses();
        let second = poses();
        assert_eq!(first.len(), usize::from(POSE_COUNT));
        assert_eq!(first, second);
        assert_eq!(atlas_size(1), (1024, 768));
        assert_eq!(atlas_size(2), (2048, 1536));

        for (offset, pose) in first.iter().enumerate() {
            let index = (offset as u16) + 1;
            assert_eq!(pose.index, index);
            assert_eq!(by_index(index).map(|item| item.name), Some(pose.name));
            assert_eq!(by_name(pose.name).map(|item| item.index), Some(index));
            for alias in pose.aliases {
                assert_eq!(by_name(alias).map(|item| item.index), Some(index));
            }
            let (x, y) = cell_origin(index).unwrap();
            assert_eq!(x, (u32::from(index - 1) % COLUMNS) * CELL);
            assert_eq!(y, (u32::from(index - 1) / COLUMNS) * CELL);
        }

        for index in 23..=25 {
            let pose = by_index(index).unwrap();
            assert_eq!(pose.contact, Contact::Top);
            assert_eq!((pose.anchor_x, pose.anchor_y), (64, 48));
        }
        for pose in first {
            if (23..=25).contains(&pose.index) {
                continue;
            }
            assert_eq!(pose.contact, Contact::Bottom, "{}", pose.name);
        }

        assert_eq!(by_name("stand").unwrap().anchor_y, 128);
        assert_eq!(by_name("lie").unwrap().name, "crawl-b");
        assert_eq!(by_name("pullout-alt").unwrap().index, 9);
        assert_eq!(by_name("carry-fall").unwrap().index, 36);
        assert!(by_index(47).is_none());
        assert!(by_index(48).is_none());
        assert_eq!(cell_origin(47), Some((6 * CELL, 5 * CELL)));
        assert_eq!(cell_origin(48), Some((7 * CELL, 5 * CELL)));
        assert!(by_name("not-a-pose").is_none());
    }
}
