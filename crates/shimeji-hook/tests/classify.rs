//! Classification and the silent hook process.
//!
//! A valid payload is sent through [`shimeji_hook::run`] with a temporary
//! home. The binary is spawned only for empty or malformed stdin, which
//! returns before any socket directory is touched.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use shimeji_hook::{RunInput, classify, host_field, run};
use shimeji_proto::{Agent, AgentState};

const SECRET: &str = "sk-test-SECRET-shimeji-u14";

fn scratch(_label: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    // `sun_path` is 104 bytes. The socket suffix is long, so the home stays short.
    let dir = PathBuf::from(format!("/tmp/u14h-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn claude(event: &str, extra: &str) -> String {
    format!(
        r#"{{"hook_event_name":"{event}","session_id":"sess-1","cwd":"/secret/{SECRET}","transcript_path":"/tmp/{SECRET}.jsonl","prompt":"{SECRET}","tool_input":{{"command":"{SECRET}"}},"last_assistant_message":"{SECRET}"{extra}}}"#
    )
}

#[test]
fn pretooluse_notification_and_stop_classify() {
    let pre = classify(&claude("PreToolUse", ""), &[]).unwrap();
    assert_eq!(pre.agent, Agent::ClaudeCode);
    assert_eq!(pre.session, "sess-1");
    assert_eq!(pre.state, AgentState::Working);

    let waiting = classify(
        &claude(
            "Notification",
            r#","notification_type":"permission_prompt","message":"allow?""#,
        ),
        &[],
    )
    .unwrap();
    assert_eq!(waiting.state, AgentState::Waiting);

    let done = classify(&claude("Stop", ""), &[]).unwrap();
    assert_eq!(done.state, AgentState::Done);
}

#[test]
fn codex_agent_turn_complete_is_read_from_argv() {
    let payload = format!(
        r#"{{"type":"agent-turn-complete","thread-id":"thr-9","turn-id":"t","cwd":"/tmp/{SECRET}","input-messages":["{SECRET}"],"last-assistant-message":"{SECRET}"}}"#
    );
    let stdin = claude("PreToolUse", "");
    let classified = classify(&stdin, &["--shimeji2".to_string(), payload]).unwrap();
    assert_eq!(classified.agent, Agent::Codex);
    assert_eq!(classified.session, "thr-9");
    assert_eq!(classified.state, AgentState::Done);
}

#[test]
fn malformed_or_empty_stdin_exits_0_with_no_output() {
    for stdin in ["", "{", "not json", "[]"] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_shimeji-hook"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "stdin {stdin:?} status {}",
            output.status
        );
        assert!(
            output.stdout.is_empty(),
            "stdout {}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(
            output.stderr.is_empty(),
            "stderr {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    assert!(classify("", &[]).is_none());
    assert!(classify("{", &[]).is_none());
}

#[test]
fn a_missing_socket_returns_within_150ms() {
    let home = scratch("missing");
    let started = Instant::now();
    let result = run(&RunInput {
        stdin: &claude("PreToolUse", ""),
        args: &[],
        bundle_id: None,
        term_program: None,
        home: &home,
    });
    assert!(result.sent.is_none());
    assert!(
        started.elapsed() < Duration::from_millis(150),
        "elapsed {:?}",
        started.elapsed()
    );
}

#[cfg(unix)]
fn accept_one(path: &Path) -> std::sync::mpsc::Receiver<String> {
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::thread;
    let listener = UnixListener::bind(path).unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = String::new();
        let _ = stream.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

#[cfg(unix)]
fn serve(home: &Path) -> (PathBuf, std::sync::mpsc::Receiver<String>) {
    let sock = shimeji_proto::prepare(home, shimeji_proto::SocketRole::Listener).unwrap();
    let rx = accept_one(&sock);
    (sock, rx)
}

#[test]
#[cfg(unix)]
fn the_sent_line_omits_a_seeded_secret() {
    let home = scratch("secret");
    let (_sock, rx) = serve(&home);
    let result = run(&RunInput {
        stdin: &claude("PreToolUse", r#","tool_name":"Bash""#),
        args: &[],
        bundle_id: Some("com.apple.Terminal"),
        term_program: Some("not a bundle"),
        home: &home,
    });
    let line = result.sent.expect("line");
    let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    for body in [&line, &received] {
        assert!(!body.contains(SECRET), "{body}");
        assert!(!body.contains("tool_input"), "{body}");
        assert!(!body.contains("transcript_path"), "{body}");
        assert!(body.contains(r#""state":"working""#), "{body}");
        assert!(body.contains(r#""agent":"claude-code""#), "{body}");
        assert!(body.contains(r#""session":"sess-1""#), "{body}");
        assert!(body.contains(r#""host":"com.apple.Terminal""#), "{body}");
    }
}

#[test]
#[cfg(unix)]
fn host_comes_from_the_bundle_id_and_a_bad_value_is_dropped() {
    let home = scratch("host");
    let (_sock, rx) = serve(&home);
    let bad = run(&RunInput {
        stdin: &claude("Stop", ""),
        args: &[],
        bundle_id: Some("not a bundle"),
        term_program: Some("has spaces"),
        home: &home,
    });
    let line = bad.sent.expect("line");
    assert!(!line.contains("host"), "{line}");
    assert!(!line.contains("not a bundle"), "{line}");
    assert!(line.contains(r#""state":"done""#), "{line}");
    let _ = rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let home = scratch("host-term");
    let (_sock, rx) = serve(&home);
    let term = run(&RunInput {
        stdin: &claude("Stop", ""),
        args: &[],
        bundle_id: Some("not a bundle"),
        term_program: Some("iTerm.app"),
        home: &home,
    });
    let line = term.sent.expect("line");
    assert!(line.contains(r#""host":"iTerm.app""#), "{line}");
    assert!(!line.contains("not a bundle"), "{line}");
    let _ = rx.recv_timeout(Duration::from_secs(2));
}

#[test]
#[cfg(unix)]
fn a_codex_argv_payload_is_what_gets_sent() {
    let home = scratch("codex");
    let (_sock, rx) = serve(&home);
    let payload = format!(
        r#"{{"type":"agent-turn-complete","thread-id":"thr-9","input-messages":["{SECRET}"],"last-assistant-message":"{SECRET}"}}"#
    );
    let result = run(&RunInput {
        stdin: &claude("PreToolUse", ""),
        args: &["--shimeji2".to_string(), payload],
        bundle_id: None,
        term_program: None,
        home: &home,
    });
    let line = result.sent.expect("line");
    let received = rx.recv_timeout(Duration::from_secs(2)).unwrap();
    for body in [&line, &received] {
        assert!(!body.contains(SECRET), "{body}");
        assert!(body.contains(r#""agent":"codex""#), "{body}");
        assert!(body.contains(r#""state":"done""#), "{body}");
        assert!(body.contains(r#""session":"thr-9""#), "{body}");
        assert!(!body.contains("input-messages"), "{body}");
    }
}

#[test]
fn a_bad_host_is_dropped_before_it_is_encoded() {
    assert_eq!(
        host_field(Some("com.apple.Terminal"), Some("ignored")).as_deref(),
        Some("com.apple.Terminal")
    );
    assert_eq!(host_field(Some("not a bundle"), None), None);
    assert_eq!(
        host_field(Some("../etc/passwd"), Some("iTerm.app")).as_deref(),
        Some("iTerm.app")
    );
}
