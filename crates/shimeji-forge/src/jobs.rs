//! Generation groups G1–G8, and the sheet table for the one real character.

use std::path::PathBuf;

/// A generation job. Frame numbers are classic `shime{n}` indices, in slot order.
#[derive(Debug, Clone, Copy)]
pub struct Group {
    pub id: &'static str,
    pub title: &'static str,
    pub frames: &'static [u16],
    /// Standing groups scale from the tallest upright frame.
    pub standing: bool,
}

pub const GROUPS: &[Group] = &[
    Group {
        id: "G1",
        title: "core",
        frames: &[1, 2, 3, 22, 4],
        standing: true,
    },
    Group {
        id: "G2",
        title: "pinched",
        frames: &[7, 8, 9, 10, 5, 6],
        standing: false,
    },
    Group {
        id: "G3",
        title: "head-spin",
        frames: &[11, 26, 15, 27, 16, 28, 17, 29],
        standing: false,
    },
    Group {
        id: "G4",
        title: "edge-sit",
        frames: &[30, 31, 32, 33],
        standing: false,
    },
    Group {
        id: "G5",
        title: "wall-ceiling",
        frames: &[12, 13, 14, 23, 24, 25],
        standing: false,
    },
    Group {
        id: "G6",
        title: "floor",
        frames: &[18, 19, 20, 21],
        standing: false,
    },
    Group {
        id: "G7",
        title: "window",
        frames: &[34, 35, 36, 37],
        standing: true,
    },
    Group {
        id: "G8",
        title: "pull-out",
        frames: &[38, 39, 40, 41],
        standing: false,
    },
];

pub fn group(id: &str) -> Option<&'static Group> {
    GROUPS
        .iter()
        .find(|group| group.id.eq_ignore_ascii_case(id))
}

pub fn prompts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("prompts")
}

/// One raw sheet from the 2026-10-09 character. `frames` is classic indices in
/// slot order; numbers above 46 are that character's own poses and have no template.
#[derive(Debug, Clone, Copy)]
pub struct CharacterSheet {
    pub file: &'static str,
    pub cols: u16,
    pub rows: u16,
    pub frames: &'static [u16],
}

pub const CHARACTER_SHEETS: &[CharacterSheet] = &[
    CharacterSheet {
        file: "group-00-stand-walk.png",
        cols: 4,
        rows: 1,
        frames: &[1, 2, 3, 4],
    },
    CharacterSheet {
        file: "group-00b-walk.png",
        cols: 1,
        rows: 1,
        frames: &[2],
    },
    CharacterSheet {
        file: "group-00c-walk-6.png",
        cols: 6,
        rows: 1,
        frames: &[88, 89, 90, 91, 92, 93],
    },
    CharacterSheet {
        file: "group-00d-walk-16.png",
        cols: 4,
        rows: 4,
        frames: &[],
    },
    CharacterSheet {
        file: "group-01-held-struggle.png",
        cols: 6,
        rows: 1,
        frames: &[7, 8, 9, 10, 5, 6],
    },
    CharacterSheet {
        file: "group-02-fall-leap.png",
        cols: 2,
        rows: 1,
        frames: &[4, 22],
    },
    CharacterSheet {
        file: "group-03-sit-head-turn.png",
        cols: 8,
        rows: 1,
        frames: &[11, 26, 15, 27, 16, 28, 17, 29],
    },
    CharacterSheet {
        file: "group-04-ledge-leg-swing.png",
        cols: 4,
        rows: 1,
        frames: &[30, 31, 32, 33],
    },
    CharacterSheet {
        file: "group-05-wall-ceiling.png",
        cols: 6,
        rows: 1,
        frames: &[12, 13, 14, 23, 24, 25],
    },
    CharacterSheet {
        file: "group-05-wallrun-left-v2.png",
        cols: 3,
        rows: 1,
        frames: &[14, 12, 13],
    },
    CharacterSheet {
        file: "group-05b-wall-run.png",
        cols: 3,
        rows: 1,
        frames: &[14, 12, 13],
    },
    CharacterSheet {
        file: "group-05c-wall-run.png",
        cols: 3,
        rows: 1,
        frames: &[14, 12, 13],
    },
    CharacterSheet {
        file: "group-05d-wall-run-4.png",
        cols: 5,
        rows: 1,
        frames: &[14, 76, 12, 77, 13],
    },
    CharacterSheet {
        file: "group-05e-wall-run-ninja.png",
        cols: 5,
        rows: 1,
        frames: &[14, 76, 12, 77, 13],
    },
    CharacterSheet {
        file: "group-06-bounce-crawl.png",
        cols: 4,
        rows: 1,
        frames: &[18, 19, 20, 21],
    },
    CharacterSheet {
        file: "group-06-landing-crawl.png",
        cols: 4,
        rows: 1,
        frames: &[18, 19, 20, 21],
    },
    CharacterSheet {
        file: "group-06-qinggong-landing-v2.png",
        cols: 4,
        rows: 1,
        frames: &[18, 19, 20, 21],
    },
    CharacterSheet {
        file: "group-07-carry-throw.png",
        cols: 4,
        rows: 1,
        frames: &[34, 35, 36, 37],
    },
    CharacterSheet {
        file: "group-07-sash-carry.png",
        cols: 4,
        rows: 1,
        frames: &[34, 35, 36, 37],
    },
    CharacterSheet {
        file: "group-08-pulled-out.png",
        cols: 4,
        rows: 1,
        frames: &[38, 39, 40, 41],
    },
    CharacterSheet {
        file: "group-09-thinking.png",
        cols: 4,
        rows: 1,
        frames: &[47, 48, 49, 50],
    },
    CharacterSheet {
        file: "group-10-reciting.png",
        cols: 4,
        rows: 1,
        frames: &[51, 52, 53, 54],
    },
    CharacterSheet {
        file: "group-11-brush-spin.png",
        cols: 4,
        rows: 1,
        frames: &[55, 56, 57, 58],
    },
    CharacterSheet {
        file: "group-12-hand-tuning.png",
        cols: 4,
        rows: 1,
        frames: &[59, 60, 61, 62],
    },
    CharacterSheet {
        file: "group-13-scroll.png",
        cols: 4,
        rows: 1,
        frames: &[63, 64, 65, 66],
    },
    CharacterSheet {
        file: "group-14-sword-dance.png",
        cols: 4,
        rows: 1,
        frames: &[67, 68, 69, 70],
    },
    CharacterSheet {
        file: "group-15-umbrella.png",
        cols: 5,
        rows: 1,
        frames: &[71, 72, 73, 74, 75],
    },
    CharacterSheet {
        file: "group-16-vanish.png",
        cols: 5,
        rows: 1,
        frames: &[78, 79, 80, 81, 82],
    },
    CharacterSheet {
        file: "group-17-appear.png",
        cols: 5,
        rows: 1,
        frames: &[83, 84, 85, 86, 87],
    },
];

pub fn character_sheet(file: &str) -> Option<&'static CharacterSheet> {
    CHARACTER_SHEETS.iter().find(|sheet| sheet.file == file)
}
