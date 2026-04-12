//! End-to-end integration tests for the sift-hook binary session lifecycle.

use assert_cmd::Command;
use std::fs;
use std::io::Write;
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

#[test]
fn user_prompt_loose_mode_always_allows() {
    let td = TempDir::new().unwrap();
    let event = serde_json::json!({ "cwd": td.path() });
    Command::cargo_bin("sift-hook").unwrap()
        .arg("session-start")
        .write_stdin(event.to_string())
        .assert()
        .success();
    Command::cargo_bin("sift-hook").unwrap()
        .arg("user-prompt")
        .write_stdin(event.to_string())
        .assert()
        .success();
}

#[test]
fn user_prompt_strict_mode_blocks_when_pending() {
    let td = TempDir::new().unwrap();
    let event = serde_json::json!({ "cwd": td.path() });

    // Start a session.
    Command::cargo_bin("sift-hook").unwrap()
        .arg("session-start")
        .write_stdin(event.to_string())
        .assert()
        .success();

    // Set strict mode in .sift/config.toml.
    fs::write(
        td.path().join(".sift/config.toml"),
        "mode = \"strict\"\nignore_globs = []\n",
    )
    .unwrap();

    // Manually append a pending entry so we simulate "writes happened".
    let current = td.path().join(".sift/current");
    let session_dir = fs::read_link(&current).unwrap();
    let mut pending = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(session_dir.join("pending.jsonl"))
        .unwrap();
    let sample = r#"{"id":"01","turn":1,"tool":"Write","path":"x","op":"create","diff_stats":{"added":1,"removed":0},"snapshot_before":null,"snapshot_after":"aaaa","status":"pending","timestamp":"2026-04-11T00:00:00Z"}"#;
    writeln!(pending, "{sample}").unwrap();

    // user-prompt should now exit 2 with the block message on stderr.
    let out = Command::cargo_bin("sift-hook").unwrap()
        .arg("user-prompt")
        .write_stdin(event.to_string())
        .assert()
        .failure()
        .code(2);
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("pending"), "expected block message, got: {stderr}");
}

#[test]
fn user_prompt_strict_mode_allows_when_pending_empty() {
    let td = TempDir::new().unwrap();
    let event = serde_json::json!({ "cwd": td.path() });

    // Start session.
    Command::cargo_bin("sift-hook").unwrap()
        .arg("session-start")
        .write_stdin(event.to_string())
        .assert()
        .success();

    // Strict mode but no pending entries — prompt should pass.
    fs::write(
        td.path().join(".sift/config.toml"),
        "mode = \"strict\"\nignore_globs = []\n",
    )
    .unwrap();

    Command::cargo_bin("sift-hook").unwrap()
        .arg("user-prompt")
        .write_stdin(event.to_string())
        .assert()
        .success();
}

#[test]
fn user_prompt_bumps_turn_counter() {
    let td = TempDir::new().unwrap();
    let event = serde_json::json!({ "cwd": td.path() });

    Command::cargo_bin("sift-hook").unwrap()
        .arg("session-start")
        .write_stdin(event.to_string())
        .assert()
        .success();

    Command::cargo_bin("sift-hook").unwrap()
        .arg("user-prompt")
        .write_stdin(event.to_string())
        .assert()
        .success();
    Command::cargo_bin("sift-hook").unwrap()
        .arg("user-prompt")
        .write_stdin(event.to_string())
        .assert()
        .success();

    // state.json should now show turn >= 2
    let current = td.path().join(".sift/current");
    let session_dir = fs::read_link(&current).unwrap();
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(session_dir.join("state.json")).unwrap())
            .unwrap();
    assert_eq!(state["turn"], 2, "expected turn=2, got {}", state["turn"]);
}

#[test]
fn user_prompt_no_session_allows() {
    // user-prompt before any session exists should just pass (no gate).
    let td = TempDir::new().unwrap();
    let event = serde_json::json!({ "cwd": td.path() });
    Command::cargo_bin("sift-hook").unwrap()
        .arg("user-prompt")
        .write_stdin(event.to_string())
        .assert()
        .success();
}

#[test]
fn pre_post_creates_pending_entry() {
    let td = TempDir::new().unwrap();
    let target = td.path().join("foo.txt");

    // Start the session.
    let init_event = serde_json::json!({ "cwd": td.path() });
    Command::cargo_bin("sift-hook").unwrap()
        .arg("session-start")
        .write_stdin(init_event.to_string())
        .assert()
        .success();

    // Pre-tool: file doesn't exist yet (pre_hash will be None).
    let pre_event = serde_json::json!({
        "cwd": td.path(),
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": target.to_str().unwrap() },
        "tool_use_id": "toolu_1"
    });
    Command::cargo_bin("sift-hook").unwrap()
        .arg("pre-tool")
        .write_stdin(pre_event.to_string())
        .assert()
        .success();

    // Simulate Claude writing the file between pre and post.
    fs::write(&target, b"hello").unwrap();

    // Post-tool: same event → same correlation key → staging record found.
    let post_event = pre_event.clone();
    Command::cargo_bin("sift-hook").unwrap()
        .arg("post-tool")
        .write_stdin(post_event.to_string())
        .assert()
        .success();

    // pending.jsonl should now contain one Create entry for foo.txt.
    let current = td.path().join(".sift/current");
    let session_dir = fs::read_link(&current).unwrap();
    let pending = fs::read_to_string(session_dir.join("pending.jsonl")).unwrap();
    assert!(pending.contains("\"op\":\"create\""), "expected Create op, got: {pending}");
    assert!(pending.contains("\"tool\":\"Write\""), "expected Tool::Write, got: {pending}");
    assert!(pending.contains("foo.txt"), "expected path foo.txt, got: {pending}");

    // Staging record should have been cleaned up.
    let staging_dir = session_dir.join("staging");
    let staging_count = staging_dir.read_dir().map(|d| d.count()).unwrap_or(0);
    assert_eq!(staging_count, 0, "staging dir should be empty after post-tool");
}
