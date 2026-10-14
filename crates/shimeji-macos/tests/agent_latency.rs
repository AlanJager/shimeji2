//! Ignored: one GUI process and twenty hook calls. `cargo test --workspace`
//! does not run this. The measurement is hook start to the flushed
//! `agent_event` line on the app's stdout.

#![cfg(target_os = "macos")]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use shimeji_hook::{RunInput, run};

#[test]
#[ignore]
fn hook_to_main_thread_p95_is_under_300ms() {
    let pid = std::process::id();
    let profile = PathBuf::from(format!("/tmp/u14b-on-{pid}"));
    let home = PathBuf::from(format!("/tmp/b{pid}9"));
    let _ = std::fs::remove_dir_all(&profile);
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&profile).unwrap();
    // prepare creates Library/... under home. It does not create home itself.
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        profile.join("settings.json"),
        br#"{"version":1,"agents":true}"#,
    )
    .unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_shimeji-macos"))
        .args([
            "--profile",
            profile.to_str().unwrap(),
            "--agent-home",
            home.to_str().unwrap(),
            "--seconds",
            "50",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let _ = tx.send((Instant::now(), line));
        }
    });

    let ready = Instant::now();
    let mut listening = false;
    while ready.elapsed() < Duration::from_secs(25) {
        match rx.recv_timeout(Duration::from_secs(25)) {
            Ok((_, line)) if line.contains("agent_listener=listening") => {
                listening = true;
                break;
            }
            Ok((_, line)) if line.starts_with("agent_listener=") => {
                panic!("listener did not start: {line}");
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }
    assert!(listening, "timed out waiting for the listener");
    // The launch drop starts above the screen in 落下する. Interject is
    // skipped for that whole fall (about three seconds from this display).
    // The gate is still the main-thread line; the extra wait is so the pose
    // can land once and `agent_pose` is observable.
    std::thread::sleep(Duration::from_secs(8));

    let mut samples = Vec::with_capacity(20);
    let mut applied = 0u32;
    let mut poses = 0u32;
    for index in 0..20 {
        while let Ok((_, line)) = rx.try_recv() {
            if line.contains("agent_pose") {
                poses += 1;
            }
        }
        let waiting = index % 2 == 1;
        let payload = if waiting {
            r#"{"hook_event_name":"PermissionRequest","session_id":"lat","tool_name":"Bash"}"#
        } else {
            r#"{"hook_event_name":"PreToolUse","session_id":"lat","tool_name":"Read","tool_input":{"file_path":"/tmp/x"}}"#
        };
        let started = Instant::now();
        let sent = run(&RunInput {
            stdin: payload,
            args: &[],
            bundle_id: None,
            term_program: None,
            home: &home,
        });
        assert!(sent.sent.is_some(), "hook did not send run {index}");
        let want = if waiting {
            "state=waiting"
        } else {
            "state=working"
        };
        let mut saw = None;
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let wait = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(wait) {
                Ok((at, line)) if line.contains("agent_event") && line.contains(want) => {
                    if samples.is_empty() {
                        println!("first_event={line}");
                    }
                    if line.contains("applied=1") {
                        applied += 1;
                    }
                    saw = Some(at.saturating_duration_since(started));
                    break;
                }
                Ok((_, line)) => {
                    if line.contains("agent_pose") {
                        poses += 1;
                    }
                }
                Err(_) => break,
            }
        }
        let elapsed = saw.unwrap_or_else(|| panic!("no agent_event for run {index}"));
        samples.push(elapsed.as_millis());
    }

    let trail = Instant::now() + Duration::from_secs(1);
    while Instant::now() < trail {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok((_, line)) if line.contains("agent_pose") => poses += 1,
            Ok(_) => {}
            Err(_) => {}
        }
    }
    samples.sort_unstable();
    let p50 = percentile(&samples, 50.0);
    let p95 = percentile(&samples, 95.0);
    println!("latency_ms={samples:?}");
    println!("latency_p50_ms={p50}");
    println!("latency_p95_ms={p95}");
    println!("applied_ones={applied}");
    println!("agent_pose_lines={poses}");
    let _ = child.kill();
    let _ = child.wait();
    assert!(p95 < 300, "p95 {p95} ms");
    let _ = std::fs::remove_dir_all(&profile);
    let _ = std::fs::remove_dir_all(&home);
}

fn percentile(sorted: &[u128], percent: f64) -> u128 {
    let rank = ((percent / 100.0) * sorted.len() as f64).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}
