//! End-to-end integration tests for the sift-hook binary session lifecycle.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

#[test]
fn session_start_creates_dir_and_symlink() {
    let td = TempDir::new().unwrap();
    let event = serde_json::json!({
        "session_id": "sess-x",
        "cwd": td.path(),
        "hook_event_name": "SessionStart"
    });
    Command::cargo_bin("sift-hook")
        .unwrap()
        .arg("session-start")
        .write_stdin(event.to_string())
        .assert()
        .success();
    // A session dir should exist under .sift/sessions/.
    let sessions_dir = td.path().join(".sift/sessions");
    assert!(sessions_dir.exists(), "{sessions_dir:?} should exist");
    let first = sessions_dir.read_dir().unwrap().next();
    assert!(first.is_some(), "at least one session dir should be present");
    // The current symlink should point to the new session dir.
    let current = td.path().join(".sift/current");
    let meta = current.symlink_metadata().unwrap();
    assert!(meta.file_type().is_symlink(), "{current:?} should be a symlink");
}

#[test]
fn stop_writes_ended_at_and_summary() {
    let td = TempDir::new().unwrap();
    let event = serde_json::json!({ "cwd": td.path() });

    // First start a session.
    Command::cargo_bin("sift-hook")
        .unwrap()
        .arg("session-start")
        .write_stdin(event.to_string())
        .assert()
        .success();

    // Then stop it — should print a summary to stderr and succeed.
    let out = Command::cargo_bin("sift-hook")
        .unwrap()
        .arg("stop")
        .write_stdin(event.to_string())
        .assert()
        .success();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(stderr.starts_with("sift:"), "expected summary line, got: {stderr}");

    // meta.json should now have `ended_at` set.
    let current = td.path().join(".sift/current");
    let session_dir = fs::read_link(&current).unwrap();
    let meta_path = session_dir.join("meta.json");
    let meta: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
    assert!(meta["ended_at"].is_string(), "ended_at should be set");
}

#[test]
fn stop_on_no_session_is_a_noop() {
    // Calling stop without an existing session should succeed silently
    // (no session to close).
    let td = TempDir::new().unwrap();
    let event = serde_json::json!({ "cwd": td.path() });
    Command::cargo_bin("sift-hook")
        .unwrap()
        .arg("stop")
        .write_stdin(event.to_string())
        .assert()
        .success();
}
