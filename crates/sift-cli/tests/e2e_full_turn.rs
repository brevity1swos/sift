//! Full-turn integration test: session-start → user-prompt → pre-tool × 2
//! → post-tool × 2 → accept all → stop.

use assert_cmd::Command;
use std::fs;
use tempfile::TempDir;

fn hook(_td: &TempDir, cmd: &str, event: &serde_json::Value) {
    Command::cargo_bin("sift-hook")
        .unwrap()
        .arg(cmd)
        .write_stdin(event.to_string())
        .assert()
        .success();
}

#[test]
fn full_turn_with_two_writes_then_accept_all() {
    let td = TempDir::new().unwrap();
    let init = serde_json::json!({ "cwd": td.path() });

    hook(&td, "session-start", &init);
    hook(&td, "user-prompt", &init);

    let f1 = td.path().join("a.txt");
    let f2 = td.path().join("sub/b.txt");
    fs::create_dir_all(td.path().join("sub")).unwrap();

    let e1 = serde_json::json!({
        "cwd": td.path(),
        "tool_name": "Write",
        "tool_input": { "file_path": f1.to_str().unwrap() },
        "tool_use_id": "tu1"
    });
    let e2 = serde_json::json!({
        "cwd": td.path(),
        "tool_name": "Write",
        "tool_input": { "file_path": f2.to_str().unwrap() },
        "tool_use_id": "tu2"
    });

    hook(&td, "pre-tool", &e1);
    fs::write(&f1, b"one").unwrap();
    hook(&td, "post-tool", &e1);

    hook(&td, "pre-tool", &e2);
    fs::write(&f2, b"two").unwrap();
    hook(&td, "post-tool", &e2);

    // Two pending entries visible via sift CLI.
    Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["list", "--pending", "--json"])
        .assert()
        .success()
        .stdout(predicates::str::contains("a.txt"))
        .stdout(predicates::str::contains("b.txt"));

    // Accept all.
    Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["accept", "all"])
        .assert()
        .success()
        .stdout(predicates::str::contains("accepted 2"));

    // Stop hook writes summary to stderr.
    let out = Command::cargo_bin("sift-hook")
        .unwrap()
        .arg("stop")
        .write_stdin(init.to_string())
        .assert()
        .success();
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(stderr.contains("accepted"), "summary should mention accepted, got: {stderr}");

    // Files still present after accept.
    assert!(f1.exists());
    assert!(f2.exists());
}
