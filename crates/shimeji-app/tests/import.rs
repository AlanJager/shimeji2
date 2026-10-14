//! Import outcomes: success, degraded, duplicate, not a pack, hostile zip.

#[path = "support.rs"]
mod support;

use std::fs;

use shimeji_app::import::{self, Choice, Outcome};
use shimeji_app::library::Library;

#[test]
fn the_minimal_ee_fixture_succeeds_with_two_characters() {
    let root = support::scratch("ee");
    let outcome = import::import(&support::fixture("minimal-ee"), &root, None);
    let Outcome::Success(installed) = outcome else {
        panic!("expected success");
    };
    assert_eq!(installed.id, "minimal-ee");
    assert_eq!(installed.characters, 2);
    assert!(installed.warnings.is_empty());
    assert!(installed.path.join("img/A/a.png").is_file());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn an_unknown_class_is_degraded_and_names_the_file_and_line() {
    let root = support::scratch("class");
    let source = root.join("weird");
    support::write_unknown_class(&source);
    let outcome = import::import(&source, &root, None);
    let Outcome::Degraded(installed) = outcome else {
        panic!("expected degraded");
    };
    assert_eq!(installed.characters, 1);
    assert!(
        installed.warnings.iter().any(|warning| {
            warning.file.ends_with("actions.xml")
                && warning.line.is_some()
                && warning.message.contains("com.example.Foo")
        }),
        "{:?}",
        installed.warnings
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn reimporting_reports_a_duplicate_then_replace_and_keep_both() {
    let root = support::scratch("dup");
    let first = root.join("sources/minimal-ee");
    let second = root.join("other/minimal-ee");
    copy_dir(&support::fixture("minimal-ee"), &first);
    copy_dir(&support::fixture("minimal-ee"), &second);
    fs::write(first.join("from-first.txt"), b"first").unwrap();
    fs::write(second.join("from-second.txt"), b"second").unwrap();

    let outcome = import::import(&first, &root, None);
    assert!(matches!(outcome, Outcome::Success(_)), "{outcome:?}");

    let again = import::import(&second, &root, None);
    let Outcome::Duplicate { id } = again else {
        panic!("expected duplicate");
    };
    assert_eq!(id, "minimal-ee");
    assert!(
        root.join("packs/minimal-ee/from-first.txt").is_file(),
        "a duplicate must not replace the files"
    );
    assert!(!root.join("packs/minimal-ee/from-second.txt").exists());

    let replaced = import::import(&second, &root, Some(Choice::Replace));
    let Outcome::Success(installed) = replaced else {
        panic!("expected replace to succeed");
    };
    assert_eq!(installed.id, "minimal-ee");
    assert!(root.join("packs/minimal-ee/from-second.txt").is_file());
    assert!(
        !root.join("packs/minimal-ee/from-first.txt").exists(),
        "replace should swap the whole directory"
    );
    assert!(
        support::packs_entries(&root)
            .iter()
            .all(|name| !name.starts_with('.'))
    );

    let kept = import::import(&first, &root, Some(Choice::KeepBoth));
    let Outcome::Success(installed) = kept else {
        panic!("expected keep-both to succeed");
    };
    assert_eq!(installed.id, "minimal-ee-2");
    assert!(root.join("packs/minimal-ee-2").is_dir());
    assert!(root.join("packs/minimal-ee/from-second.txt").is_file());
    assert!(root.join("packs/minimal-ee-2/from-first.txt").is_file());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_file_that_is_not_a_pack_errors_and_leaves_the_library_unchanged() {
    let root = support::scratch("nota");
    let installed = import::import(&support::fixture("minimal-ee"), &root, None);
    assert!(matches!(installed, Outcome::Success(_)));
    let before = Library::open(&root);
    let before_ids: Vec<_> = before.packs().iter().map(|pack| pack.id.clone()).collect();

    let dropped = root.join("notes.txt");
    fs::write(&dropped, b"hello").unwrap();
    let outcome = import::import(&dropped, &root, None);
    let Outcome::Error { message } = outcome else {
        panic!("expected an error");
    };
    assert!(!message.is_empty());
    let after = Library::open(&root);
    let after_ids: Vec<_> = after.packs().iter().map(|pack| pack.id.clone()).collect();
    assert_eq!(before_ids, after_ids);
    assert_eq!(
        support::packs_entries(&root),
        vec!["minimal-ee".to_string()]
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_hostile_zip_errors_and_writes_nothing_under_packs() {
    let root = support::scratch("slip");
    let zip_path = root.join("slip.zip");
    fs::write(&zip_path, support::zip_slip("../../evil", b"pwned")).unwrap();
    let outcome = import::import(&zip_path, &root, None);
    let Outcome::Error { message } = outcome else {
        panic!(
            "expected an error, packs={:?}",
            support::packs_entries(&root)
        );
    };
    assert!(
        message.contains("../../evil") || message.contains("escapes") || message.contains("zip"),
        "{message}"
    );
    assert!(
        support::packs_entries(&root).is_empty(),
        "packs/ should stay empty, got {:?}",
        support::packs_entries(&root)
    );
    assert!(!root.join("staging").exists() || staging_empty(&root));
    assert!(
        !root.join("evil").exists(),
        "zip-slip must not write outside packs/"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn replace_with_a_non_pack_errors_and_keeps_the_installed_pack() {
    let root = support::scratch("replace-junk");
    let good = root.join("src/minimal-ee");
    copy_dir(&support::fixture("minimal-ee"), &good);
    assert!(matches!(
        import::import(&good, &root, None),
        Outcome::Success(_)
    ));
    let image = root.join("packs/minimal-ee/img/A/a.png");
    let before = fs::read(&image).expect("installed image");

    let junk = root.join("junk/minimal-ee");
    fs::create_dir_all(&junk).unwrap();
    fs::write(junk.join("notes.txt"), b"not a pack").unwrap();

    // A drop with no characters errors before the duplicate question.
    let first = import::import(&junk, &root, None);
    assert!(
        matches!(first, Outcome::Error { .. }),
        "{}",
        outcome_label(&first)
    );
    assert_eq!(fs::read(&image).unwrap(), before);

    let replaced = import::import(&junk, &root, Some(Choice::Replace));
    let Outcome::Error { message } = replaced else {
        panic!("expected error, got {}", outcome_label(&replaced));
    };
    assert!(!message.is_empty());
    assert_eq!(fs::read(&image).unwrap(), before);
    assert!(!root.join("packs/minimal-ee/notes.txt").exists());
    assert_eq!(
        support::packs_entries(&root),
        vec!["minimal-ee".to_string()]
    );
    assert_staging_cleaned(&root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn keep_both_with_a_non_pack_errors_and_adds_nothing() {
    let root = support::scratch("keep-junk");
    let good = root.join("src/minimal-ee");
    copy_dir(&support::fixture("minimal-ee"), &good);
    assert!(matches!(
        import::import(&good, &root, None),
        Outcome::Success(_)
    ));
    let image = root.join("packs/minimal-ee/img/A/a.png");
    let before = fs::read(&image).expect("installed image");

    let junk = root.join("junk/minimal-ee");
    fs::create_dir_all(&junk).unwrap();
    fs::write(junk.join("notes.txt"), b"not a pack").unwrap();

    let outcome = import::import(&junk, &root, Some(Choice::KeepBoth));
    let Outcome::Error { message } = outcome else {
        panic!("expected error, got {}", outcome_label(&outcome));
    };
    assert!(!message.is_empty());
    assert_eq!(fs::read(&image).unwrap(), before);
    assert!(!root.join("packs/minimal-ee-2").exists());
    assert_eq!(
        support::packs_entries(&root),
        vec!["minimal-ee".to_string()]
    );
    assert_staging_cleaned(&root);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn import_never_reports_zero_characters_as_installed() {
    let choices = [None, Some(Choice::Replace), Some(Choice::KeepBoth)];
    for choice in choices {
        let root = support::scratch("zero-fresh");
        let source = root.join("src/parsed-empty");
        write_parsed_empty(&source);
        let outcome = import::import(&source, &root, choice);
        assert!(
            matches!(outcome, Outcome::Error { .. }),
            "{choice:?} -> {}",
            outcome_label(&outcome)
        );
        assert!(
            outcome.installed().is_none(),
            "{choice:?} reported an install"
        );
        assert!(
            support::packs_entries(&root).is_empty(),
            "{choice:?} left {:?}",
            support::packs_entries(&root)
        );
        assert_staging_cleaned(&root);
        let _ = fs::remove_dir_all(&root);

        let root = support::scratch("zero-kept");
        let good = root.join("src/minimal-ee");
        copy_dir(&support::fixture("minimal-ee"), &good);
        assert!(matches!(
            import::import(&good, &root, None),
            Outcome::Success(_)
        ));
        let image = root.join("packs/minimal-ee/img/A/a.png");
        let before = fs::read(&image).expect("installed image");
        let source = root.join("bad/minimal-ee");
        write_parsed_empty(&source);
        let outcome = import::import(&source, &root, choice);
        assert!(
            matches!(outcome, Outcome::Error { .. }),
            "{choice:?}: {}",
            outcome_label(&outcome)
        );
        assert!(outcome.installed().is_none());
        assert_eq!(fs::read(&image).unwrap(), before);
        assert!(!root.join("packs/minimal-ee-2").exists());
        assert_eq!(
            support::packs_entries(&root),
            vec!["minimal-ee".to_string()]
        );
        assert_staging_cleaned(&root);
        let _ = fs::remove_dir_all(&root);
    }
}

/// `actions.xml` and `behaviors.xml` are present, but neither document yields a character.
fn write_parsed_empty(dir: &std::path::Path) {
    fs::create_dir_all(dir).unwrap();
    let xml = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<NotAMascot/>\n";
    fs::write(dir.join("actions.xml"), xml).unwrap();
    fs::write(dir.join("behaviors.xml"), xml).unwrap();
}

fn outcome_label(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Success(installed) => format!("Success({})", installed.characters),
        Outcome::Degraded(installed) => format!("Degraded({})", installed.characters),
        Outcome::Error { message } => format!("Error({message})"),
        Outcome::Duplicate { id } => format!("Duplicate({id})"),
    }
}

fn assert_staging_cleaned(root: &std::path::Path) {
    let leftovers: Vec<String> = fs::read_dir(root)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|entry| entry.starts_with(".import-"))
        .collect();
    assert!(leftovers.is_empty(), "staging left behind: {leftovers:?}");
}

fn staging_empty(root: &std::path::Path) -> bool {
    let staging = root.join("staging");
    if !staging.exists() {
        return true;
    }
    fs::read_dir(&staging).unwrap().next().is_none()
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(entry.file_name());
        let file_type = entry.file_type().unwrap();
        if file_type.is_dir() {
            copy_dir(&entry.path(), &dest);
        } else if file_type.is_file() {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}
