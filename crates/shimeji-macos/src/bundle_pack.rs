//! Where this process reads the bundled shime.
//!
//! A real app's `NSBundle` resource path is `Something.app/Contents/Resources`.
//! `cargo run` is not that shape, so it keeps the workspace copy.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// Pack directory for this process.
///
/// `resource_path` is `NSBundle.mainBundle.resourcePath` when the system has
/// one. A bundle uses `packs/shime` under that directory. Anything else uses
/// `manifest_fallback` (the workspace pack, for `cargo run`).
pub fn bundled_pack_dir(resource_path: Option<&Path>, manifest_fallback: &Path) -> PathBuf {
    if let Some(resources) = resource_path.filter(|path| is_bundle_resources(path)) {
        return resources.join("packs").join("shime");
    }
    manifest_fallback.to_path_buf()
}

fn is_bundle_resources(path: &Path) -> bool {
    let mut rev = path.components().rev();
    if !component_is(&mut rev, "Resources") {
        return false;
    }
    if !component_is(&mut rev, "Contents") {
        return false;
    }
    match rev.next() {
        Some(Component::Normal(name)) => is_app_name(name),
        _ => false,
    }
}

fn component_is(rev: &mut std::iter::Rev<std::path::Components<'_>>, want: &str) -> bool {
    matches!(rev.next(), Some(Component::Normal(name)) if name == want)
}

fn is_app_name(name: &OsStr) -> bool {
    Path::new(name).extension().is_some_and(|ext| ext == "app")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::bundled_pack_dir;

    #[test]
    fn a_bundle_resource_directory_points_at_the_shime() {
        let fallback = Path::new("/workspace/assets/packs/shime");
        let resources = Path::new("/Applications/shimeji2.app/Contents/Resources");
        assert_eq!(
            bundled_pack_dir(Some(resources), fallback),
            Path::new("/Applications/shimeji2.app/Contents/Resources/packs/shime")
        );
        let trailed = Path::new("/tmp/Shimeji Dev.app/Contents/Resources/");
        assert_eq!(
            bundled_pack_dir(Some(trailed), fallback),
            Path::new("/tmp/Shimeji Dev.app/Contents/Resources/packs/shime")
        );
    }

    #[test]
    fn cargo_run_falls_back_to_the_manifest_pack() {
        let fallback = Path::new("/workspace/assets/packs/shime");
        assert_eq!(bundled_pack_dir(None, fallback), fallback);
        assert_eq!(
            bundled_pack_dir(Some(Path::new("/workspace/target/debug")), fallback),
            fallback
        );
        assert_eq!(
            bundled_pack_dir(
                Some(Path::new("/workspace/shimeji2.app/Contents/MacOS")),
                fallback
            ),
            fallback
        );
        assert_eq!(
            bundled_pack_dir(Some(Path::new("/tmp/Resources")), fallback),
            fallback
        );
    }
}
