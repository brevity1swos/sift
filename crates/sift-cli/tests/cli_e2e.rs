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

#[test]
fn accept_all_moves_pending_to_ledger() {
    let td = TempDir::new().unwrap();
    start_session(&td);
    write_via_hook(&td, "y.txt", b"a");
    Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["accept", "all"])
        .assert()
        .success()
        .stdout(predicates::str::contains("accepted 1"));
    // Pending should now be empty.
    Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["list", "--pending", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("[]"));
}

#[test]
fn revert_all_on_create_deletes_file() {
    let td = TempDir::new().unwrap();
    start_session(&td);
    let target = write_via_hook(&td, "z.txt", b"z");
    assert!(target.exists());
    Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["revert", "all"])
        .assert()
        .success();
    assert!(!target.exists());
}

#[test]
fn gc_dry_run_reports_old_sessions() {
    let td = TempDir::new().unwrap();
    let paths = sift_core::paths::Paths::new(td.path());
    let s = sift_core::session::Session::create(paths).unwrap();
    s.close().unwrap();

    // Backdate meta.json.
    let meta_path = s.dir.join("meta.json");
    let text = fs::read_to_string(&meta_path).unwrap();
    let mut meta: sift_core::session::SessionMeta = serde_json::from_str(&text).unwrap();
    meta.started_at = chrono::Utc::now() - chrono::Duration::days(30);
    meta.ended_at = Some(chrono::Utc::now() - chrono::Duration::days(30));
    fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap()).unwrap();

    let output = Command::cargo_bin("sift")
        .unwrap()
        .args(["gc", "--days", "7"])
        .current_dir(td.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("would delete"),
        "expected 'would delete' in: {stdout}"
    );
    assert!(
        s.dir.exists(),
        "session dir should still exist in dry-run mode"
    );
}

#[test]
fn gc_apply_deletes_old_sessions() {
    let td = TempDir::new().unwrap();
    let paths = sift_core::paths::Paths::new(td.path());
    let s = sift_core::session::Session::create(paths).unwrap();
    s.close().unwrap();

    let meta_path = s.dir.join("meta.json");
    let text = fs::read_to_string(&meta_path).unwrap();
    let mut meta: sift_core::session::SessionMeta = serde_json::from_str(&text).unwrap();
    meta.started_at = chrono::Utc::now() - chrono::Duration::days(30);
    meta.ended_at = Some(chrono::Utc::now() - chrono::Duration::days(30));
    fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap()).unwrap();

    Command::cargo_bin("sift")
        .unwrap()
        .args(["gc", "--days", "7", "--apply"])
        .current_dir(td.path())
        .assert()
        .success();
    assert!(
        !s.dir.exists(),
        "session dir should be deleted after --apply"
    );
}

#[test]
fn gc_compact_current_session() {
    let td = TempDir::new().unwrap();
    start_session(&td);

    // Create three pending entries.
    write_via_hook(&td, "a.txt", b"aaa");
    write_via_hook(&td, "b.txt", b"bbb");
    write_via_hook(&td, "c.txt", b"ccc");

    // Accept all three to produce tombstones in pending_changes.jsonl.
    Command::cargo_bin("sift")
        .unwrap()
        .args(["accept", "all"])
        .current_dir(td.path())
        .assert()
        .success();

    // Resolve current session dir.
    let current_link = td.path().join(".sift").join("current");
    let session_dir = fs::read_link(&current_link).unwrap();
    let session_dir = if session_dir.is_absolute() {
        session_dir
    } else {
        td.path().join(".sift").join(session_dir)
    };

    let pending_changes = session_dir.join("pending_changes.jsonl");
    let pending = session_dir.join("pending.jsonl");

    // Before compact: pending_changes.jsonl exists (tombstones).
    assert!(
        pending_changes.exists(),
        "pending_changes.jsonl must exist after accept"
    );

    // Run compact.
    let output = Command::cargo_bin("sift")
        .unwrap()
        .args(["gc", "--compact"])
        .current_dir(td.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "gc --compact should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("compacted"),
        "expected 'compacted' in: {stdout}"
    );

    // After compact: pending_changes.jsonl is gone.
    assert!(
        !pending_changes.exists(),
        "pending_changes.jsonl should be removed after compact"
    );
    // All three entries were accepted, so pending.jsonl should be empty.
    let content_after = fs::read_to_string(&pending).unwrap_or_default();
    let lines_after = content_after.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(
        lines_after, 0,
        "after compact+accept all, pending.jsonl should be empty"
    );
}

#[test]
fn mode_strict_persists_in_config() {
    let td = TempDir::new().unwrap();
    start_session(&td);
    Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["mode", "strict"])
        .assert()
        .success();
    let content = fs::read_to_string(td.path().join(".sift/config.toml")).unwrap();
    assert!(
        content.contains("strict"),
        "config should contain 'strict', got: {content}"
    );
}

#[test]
fn list_path_filter_keeps_only_matching_entries() {
    let td = TempDir::new().unwrap();
    start_session(&td);

    // Pre-create the subdirectories the hook will write into. The hook
    // does not auto-create parent dirs because the host tool is the one
    // doing the actual write.
    fs::create_dir_all(td.path().join("src")).unwrap();
    fs::create_dir_all(td.path().join("tests")).unwrap();
    fs::create_dir_all(td.path().join("docs")).unwrap();

    write_via_hook(&td, "src/a.rs", b"a");
    write_via_hook(&td, "tests/b.rs", b"b");
    write_via_hook(&td, "docs/c.md", b"c");

    // Path filter `src` should match src/a.rs only — substring,
    // case-insensitive, applied after the pending list is loaded.
    let output = Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["list", "--pending", "--path", "src"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("src/a.rs"), "src/a.rs should appear: {stdout}");
    assert!(
        !stdout.contains("tests/b.rs"),
        "tests/b.rs should be filtered out: {stdout}"
    );
    assert!(
        !stdout.contains("docs/c.md"),
        "docs/c.md should be filtered out: {stdout}"
    );

    // Case-insensitivity sanity check.
    let output_upper = Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["list", "--pending", "--path", "DOCS"])
        .output()
        .unwrap();
    let stdout_upper = String::from_utf8_lossy(&output_upper.stdout);
    assert!(
        stdout_upper.contains("docs/c.md"),
        "uppercase --path DOCS should still match docs/: {stdout_upper}"
    );
}
