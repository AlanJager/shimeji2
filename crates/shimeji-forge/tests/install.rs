//! No listener: print the path and exit 0. A listener receives the request.
//! Forge itself never copies a pack into place.

use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use shimeji_forge::install_with_home;
use shimeji_proto::parse_line;

fn scratch(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "shimeji-u16-install-{label}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn no_app_prints_the_path_and_exits_0_without_creating_the_support_dir() {
    let home = scratch("home");
    let pack = home.join("mypack");
    fs::create_dir(&pack).unwrap();
    let report = install_with_home(&pack, &home);
    assert_eq!(report.exit, 0);
    assert!(!report.sent);
    let shown = pack.canonicalize().unwrap();
    assert!(
        report.message.contains(&shown.display().to_string()),
        "{}",
        report.message
    );
    assert!(report.message.contains("Drag this folder in to install"));
    assert!(!home.join("Library").exists());
    let _ = fs::remove_dir_all(&home);
}

#[cfg(unix)]
#[test]
fn a_listener_that_does_not_respond_installs_nothing() {
    use std::os::unix::net::UnixListener;

    // `{home}/Library/Application Support/shimeji2/run/sock` must fit in sun_path.
    let home = PathBuf::from(format!("/tmp/u16i{}", std::process::id()));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    let pack = home.join("mypack");
    fs::create_dir(&pack).unwrap();
    fs::write(pack.join("note.txt"), b"not copied").unwrap();
    let run = home.join("Library/Application Support/shimeji2/run");
    fs::create_dir_all(&run).unwrap();
    let listener = UnixListener::bind(run.join("sock")).unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = String::new();
        let _ = stream.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });
    let report = install_with_home(&pack, &home);
    assert_eq!(report.exit, 0);
    assert!(report.sent, "{}", report.message);
    let line = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let parsed = parse_line(&line).unwrap();
    let shimeji_proto::ParseOutcome::Message(shimeji_proto::Message::Install(request)) = parsed
    else {
        panic!("not an install line: {line}");
    };
    assert_eq!(request.name, "mypack");
    assert_eq!(
        request.path,
        pack.canonicalize().unwrap().display().to_string()
    );
    assert!(!home.join("packs").exists());
    let _ = fs::remove_dir_all(&home);
}
