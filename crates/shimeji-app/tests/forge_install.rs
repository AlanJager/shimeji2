//! Offering an install does nothing until confirm. A symlink source is rejected.

use std::fs;
use std::path::PathBuf;

use shimeji_app::agent::{Busy, ForgeInstall};
use shimeji_app::import::Outcome;
use shimeji_proto::InstallRequest;

fn scratch(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "shimeji-u16-gate-{label}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn request(name: &str, path: &str) -> InstallRequest {
    InstallRequest {
        v: 1,
        name: name.to_string(),
        path: path.to_string(),
    }
}

#[test]
fn an_unanswered_offer_installs_nothing() {
    let root = scratch("quiet");
    let pack = root.join("pack");
    fs::create_dir(&pack).unwrap();
    let packs = root.join("library");
    fs::create_dir(&packs).unwrap();
    let mut gate = ForgeInstall::new();
    let pending = request("pack", &pack.display().to_string());
    let prompt = ForgeInstall::prompt(&pending);
    assert_eq!(
        prompt,
        format!(
            "forge wants to install pack from {}: Install / Ignore",
            pack.display()
        )
    );
    gate.offer(pending).unwrap();
    assert!(gate.pending().is_some());
    assert!(fs::read_dir(&packs).unwrap().next().is_none());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_second_offer_is_busy_and_does_not_replace_the_first() {
    let mut gate = ForgeInstall::new();
    gate.offer(request("first", "/tmp/first")).unwrap();
    let err = gate.offer(request("second", "/tmp/second")).unwrap_err();
    assert_eq!(err, Busy);
    assert_eq!(gate.pending().unwrap().name, "first");
    assert_eq!(gate.pending().unwrap().path, "/tmp/first");
}

#[cfg(unix)]
#[test]
fn a_symlink_source_is_rejected_and_installs_nothing() {
    let root = scratch("link");
    let real = root.join("real");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("pack.json"), b"{}").unwrap();
    let link = root.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let packs = root.join("library");
    fs::create_dir(&packs).unwrap();
    let mut gate = ForgeInstall::new();
    gate.offer(request("link", &link.display().to_string()))
        .unwrap();
    let outcome = gate.confirm(&packs).unwrap();
    match outcome {
        Outcome::Error { message } => assert!(message.contains("symlink"), "{message}"),
        other => panic!("expected a rejected import, got {other:?}"),
    }
    assert!(gate.pending().is_none());
    assert!(fs::read_dir(&packs).unwrap().next().is_none());
    let _ = fs::remove_dir_all(&root);
}
