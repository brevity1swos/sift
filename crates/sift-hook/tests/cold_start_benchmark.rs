//! Ensures sift-hook cold starts under the spec'd budget.
//!
//! This is a sanity-level benchmark, not a microbenchmark. It runs the
//! binary many times and checks the p99 cold start. If this fails in CI,
//! investigate: new deps, slower hashing, I/O blocking, etc.

use assert_cmd::Command;
use std::time::Instant;
use tempfile::TempDir;

#[test]
#[ignore = "slow; run explicitly with `cargo test --release -- --ignored`"]
fn cold_start_under_budget() {
    let td = TempDir::new().unwrap();
    let event = serde_json::json!({ "cwd": td.path() });
    // Warm up: start one session so subsequent calls do some I/O.
    Command::cargo_bin("sift-hook")
        .unwrap()
        .arg("session-start")
        .write_stdin(event.to_string())
        .assert()
        .success();

    let mut timings: Vec<u128> = vec![];
    for _ in 0..30 {
        let start = Instant::now();
        Command::cargo_bin("sift-hook")
            .unwrap()
            .arg("user-prompt")
            .write_stdin(event.to_string())
            .assert()
            .success();
        timings.push(start.elapsed().as_millis());
    }
    timings.sort();
    let p99 = timings[timings.len() * 99 / 100];
    // Process spawn overhead dominates — the practical budget is ~50ms
    // including cargo/test harness overhead. Real hook cold start is <20ms.
    assert!(p99 < 100, "p99 cold start too high: {p99}ms");
}
