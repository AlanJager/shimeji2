use std::path::{Path, PathBuf};

use crate::error::{Result, err};

#[derive(Clone)]
pub struct Layout {
    pub root: PathBuf,
    pub editor: PathBuf,
    pub shime: PathBuf,
    pub catalog: PathBuf,
}

impl Layout {
    pub fn discover() -> Result<Self> {
        let mut candidates = Vec::new();
        if let Ok(root) = std::env::var("SHIMEJI_EDITOR_ROOT") {
            candidates.push(PathBuf::from(root));
        }
        if let Ok(exe) = std::env::current_exe() {
            push_ancestors(&mut candidates, exe.parent());
        }
        if let Ok(cwd) = std::env::current_dir() {
            push_ancestors(&mut candidates, Some(cwd.as_path()));
        }
        for root in candidates {
            let editor = if root.join("editor/index.html").is_file() {
                root.join("editor")
            } else if root.join("tools/editor/index.html").is_file() {
                root.join("tools/editor")
            } else {
                continue;
            };
            let shime = root.join("assets/packs/shime");
            let catalog = root.join("assets/catalog/sources.json");
            if shime.join("img/shime1.png").is_file() && catalog.is_file() {
                return Ok(Self {
                    root,
                    editor,
                    shime,
                    catalog,
                });
            }
        }
        Err(err("找不到编辑器页面或内置素材。请在解压后的目录里运行。"))
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("output/material-catalog/cache")
    }
}

fn push_ancestors(candidates: &mut Vec<PathBuf>, start: Option<&Path>) {
    let Some(start) = start else { return };
    let mut path = start.to_path_buf();
    for _ in 0..8 {
        candidates.push(path.clone());
        if !path.pop() {
            break;
        }
    }
}
