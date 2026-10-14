//! Listener limits from the U14a open-issues list.
//!
//! Homes stay short so `{home}/Library/Application Support/shimeji2/run/sock`
//! fits in `sun_path` (104 bytes). No second uid is available here: the foreign
//! uid case calls `admit` with an injected id. The same-uid path is the real
//! `getpeereid` check inside the other three tests.

#![cfg(target_os = "macos")]

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use shimeji_macos::agent::listener::{self, admit, current_uid, peer_allowed};
use shimeji_proto::socket_path;

fn scratch(label: &str) -> PathBuf {
    let path = PathBuf::from(format!("/tmp/b{}{label}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn connect(path: &Path) -> UnixStream {
    let started = Instant::now();
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => return stream,
            Err(error) if started.elapsed() < Duration::from_secs(2) => {
                std::thread::sleep(Duration::from_millis(10));
                let _ = error;
            }
            Err(error) => panic!("connect {}: {error}", path.display()),
        }
    }
}

fn event_line(session: &str, state: &str) -> String {
    format!(r#"{{"v":1,"agent":"claude-code","session":"{session}","state":"{state}"}}"#,) + "\n"
}

#[test]
fn an_8kb_line_is_dropped() {
    let home = scratch("1");
    let (tx, rx) = mpsc::channel();
    let listener = listener::start(&home, false, move |inbound| {
        if let listener::Inbound::Event(event) = inbound {
            let _ = tx.send(event.session);
        }
    })
    .unwrap_or_else(|error| panic!("start: {error}"));
    let sock = socket_path(&home);
    assert!(sock.as_os_str().len() < 104, "{}", sock.display());

    let mut big = connect(&sock);
    let mut body = vec![b'x'; 8192];
    body.push(b'\n');
    let _ = big.write_all(&body);
    drop(big);

    let mut good = connect(&sock);
    good.write_all(event_line("kept", "working").as_bytes())
        .unwrap();
    let session = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(session, "kept");
    assert!(rx.try_recv().is_err());
    drop(listener);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn a_silent_client_does_not_delay_the_next_event() {
    let home = scratch("2");
    let (tx, rx) = mpsc::channel();
    let listener = listener::start(&home, false, move |inbound| {
        if let listener::Inbound::Event(event) = inbound {
            let _ = tx.send(event.session);
        }
    })
    .unwrap_or_else(|error| panic!("start: {error}"));
    let sock = socket_path(&home);

    let _silent = connect(&sock);
    std::thread::sleep(Duration::from_millis(30));
    let started = Instant::now();
    let mut second = connect(&sock);
    second
        .write_all(event_line("second", "working").as_bytes())
        .unwrap();
    let session = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(session, "second");
    assert!(
        started.elapsed() < Duration::from_millis(300),
        "silent client held the next event for {:?}",
        started.elapsed()
    );
    drop(listener);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn control_characters_are_cleaned_and_truncated() {
    let home = scratch("3");
    let (tx, rx) = mpsc::channel();
    let listener = listener::start(&home, false, move |inbound| {
        if let listener::Inbound::Event(event) = inbound {
            let _ = tx.send(event.session);
        }
    })
    .unwrap_or_else(|error| panic!("start: {error}"));
    let sock = socket_path(&home);

    let line = format!(
        "{{\"v\":1,\"agent\":\"claude-code\",\"session\":\"a\\u0001b{}\",\"state\":\"working\"}}\n",
        "x".repeat(200)
    );
    assert!(line.len() <= 4096, "fixture line is {} bytes", line.len());
    let mut client = connect(&sock);
    client.write_all(line.as_bytes()).unwrap();
    let session = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(session.chars().count(), 128);
    assert!(session.starts_with("ab"));
    assert!(!session.chars().any(|ch| ch.is_control()));
    drop(listener);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn an_install_line_is_delivered_and_not_imported() {
    let home = scratch("4");
    let (tx, rx) = mpsc::channel();
    let listener = listener::start(&home, false, move |inbound| {
        let _ = tx.send(inbound);
    })
    .unwrap_or_else(|error| panic!("start: {error}"));
    let sock = socket_path(&home);
    let mut client = connect(&sock);
    client
        .write_all(
            b"{\"v\":1,\"type\":\"install\",\"name\":\"demo\",\"path\":\"/tmp/demo-pack\"}\n",
        )
        .unwrap();
    let inbound = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    match inbound {
        listener::Inbound::Install(request) => {
            assert_eq!(request.name, "demo");
            assert_eq!(request.path, "/tmp/demo-pack");
        }
        other => panic!("expected an install request, got {other:?}"),
    }
    assert!(rx.try_recv().is_err());
    assert!(!home.join("packs").exists());
    drop(listener);
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn another_uid_is_rejected() {
    // This machine has no second uid to connect from. `admit` is the check
    // `handle_one` uses after `getpeereid`. The other tests accept this uid.
    let ours = current_uid();
    let other = ours.wrapping_add(1);
    assert!(peer_allowed(ours, ours));
    assert!(!peer_allowed(other, ours));
    assert!(admit(Some(ours), ours));
    assert!(!admit(Some(other), ours));
    assert!(!admit(None, ours));
}
