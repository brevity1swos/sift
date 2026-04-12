//! End-to-end integration tests for the sift CLI binary.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

/// Start a session in a temp dir by invoking the hook binary.
fn start_session(td: &TempDir) {
    let event = serde_json::json!({ "cwd": td.path() });
    Command::cargo_bin("sift-hook")
        .unwrap()
        .arg("session-start")
        .write_stdin(event.to_string())
        .assert()
        .success();
}

/// Simulate a Write via the hook pre/post pipeline. Returns the target path.
fn write_via_hook(td: &TempDir, filename: &str, content: &[u8]) -> std::path::PathBuf {
    let target = td.path().join(filename);
    let evt = serde_json::json!({
        "cwd": td.path(),
        "tool_name": "Write",
        "tool_input": { "file_path": target.to_str().unwrap() },
        "tool_use_id": format!("tid_{filename}")
    });
    Command::cargo_bin("sift-hook")
        .unwrap()
        .arg("pre-tool")
        .write_stdin(evt.to_string())
        .assert()
        .success();
    fs::write(&target, content).unwrap();
    Command::cargo_bin("sift-hook")
        .unwrap()
        .arg("post-tool")
        .write_stdin(evt.to_string())
        .assert()
        .success();
    target
}

#[test]
fn list_empty_pending() {
    let td = TempDir::new().unwrap();
    start_session(&td);
    Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["list", "--pending", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("[]"));
}

#[test]
fn list_after_write_via_hook() {
    let td = TempDir::new().unwrap();
    start_session(&td);
    write_via_hook(&td, "x.txt", b"contents");

    Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["list", "--pending"])
        .assert()
        .success()
        .stdout(predicates::str::contains("x.txt"));
}
