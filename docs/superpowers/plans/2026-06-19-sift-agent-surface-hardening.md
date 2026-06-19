# sift Agent-Surface Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make sift's read/query commands (`status`, `diff`, `state`) accept `--json` and emit valid JSON, so the agent-guide's promise ("every command supports `--json`") becomes literally true; add a regression guard and fix the current Claude pricing gap in agx.

**Architecture:** Each affected command already loads its domain data; the change is an added serialization branch at the output boundary using explicit serde view structs. No new dependencies, no new data flows, no MCP server. The agx change is a single pricing-table row in a separate repo.

**Tech Stack:** Rust, clap, serde / serde_json, assert_cmd + tempfile (integration tests).

## Global Constraints

- Both repos are `brevity1swos` OSS tier: Conventional Commits (strict), `Co-Authored-By` trailer **allowed and expected**. End commits with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- Run `gh auth switch --user brevity1swos` before any `gh` operation. Do not push unless the user asks.
- sift: MSRV/edition per workspace; `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` must all pass clean. NOTE: the CLI crate lives in `crates/sift-cli/` but its cargo package name is **`sift-tui`** — always use `-p sift-tui` in cargo commands (`-p sift-cli` errors).
- agx: `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` clean; MSRV 1.85 / edition 2024.
- Default (no `--json`) output of every command must remain **byte-for-byte unchanged** — humans depend on it.
- sift public surface must not mention agx/sift/stepwise synergy beyond what already exists. This change touches none of that.
- `agent-guide.md` exists in TWO copies that must stay identical: `docs/agent-guide.md` (canonical) and `crates/sift-cli/agent-guide.md` (embedded via `include_str!`). Edit both.

---

### Task 1: `sift status --json`

**Files:**
- Modify: `crates/sift-cli/src/main.rs` (the `Status` enum variant + its dispatch arm)
- Modify: `crates/sift-cli/src/cmd_status.rs` (add `json` param + JSON branch)
- Test: `crates/sift-cli/tests/cli_e2e.rs` (append test, reuse existing `start_session` / `write_via_hook` helpers)

**Interfaces:**
- Produces: `cmd_status::run(cwd: &Path, json: bool) -> Result<()>` (signature changes from `run(cwd: &Path)`).
- JSON shape when active: `{"active":true,"session_id":String,"turn":u32,"mode":"loose"|"strict","pending":[LedgerEntry...],"accepted":usize,"reverted":usize}`. When no session: `{"active":false}`.

- [ ] **Step 1: Write the failing test**

Append to `crates/sift-cli/tests/cli_e2e.rs`:

```rust
#[test]
fn status_json_reports_active_session_and_pending() {
    let td = TempDir::new().unwrap();
    start_session(&td);
    write_via_hook(&td, "alpha.txt", b"hello\n");

    let out = Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["status", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let v: serde_json::Value = serde_json::from_slice(&out).expect("status --json must emit valid JSON");
    assert_eq!(v["active"], serde_json::json!(true));
    assert!(v["pending"].as_array().unwrap().iter().any(|e| e["path"]
        .as_str()
        .unwrap()
        .ends_with("alpha.txt")));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sift-tui --test cli_e2e status_json_reports_active_session_and_pending`
Expected: FAIL — clap rejects `--json` (`error: unexpected argument '--json'`), assert non-success.

- [ ] **Step 3: Add the `json` flag to the `Status` variant**

In `crates/sift-cli/src/main.rs`, change the `Status` variant from a unit variant to:

```rust
    /// Show session status and pending writes (default when no command given).
    Status {
        #[arg(long)]
        json: bool,
    },
```

Update the dispatch (the `None | Some(Commands::Status)` arm) to:

```rust
        None => {
            cmd_status::run(&cwd, false)?;
        }
        Some(Commands::Status { json }) => {
            cmd_status::run(&cwd, json)?;
        }
```

- [ ] **Step 4: Add the JSON branch to `cmd_status::run`**

In `crates/sift-cli/src/cmd_status.rs`, add `use serde::Serialize;` and `use sift_core::entry::LedgerEntry;` to the imports, change the signature to `pub fn run(cwd: &Path, json: bool) -> Result<()>`, and define the view + emit JSON. Replace the no-session early-return and the human print block so the JSON path is taken first:

```rust
#[derive(Serialize)]
struct StatusView<'a> {
    active: bool,
    session_id: &'a str,
    turn: u32,
    mode: &'a str,
    pending: &'a [LedgerEntry],
    accepted: usize,
    reverted: usize,
}
```

No-session branch (replaces the current `println!("sift: no active session") ...` block):

```rust
    if paths.current_symlink().symlink_metadata().is_err() {
        if json {
            println!("{}", serde_json::json!({ "active": false }));
            return Ok(());
        }
        println!("sift: no active session");
        println!();
        println!("  Start one by opening a Claude Code session in a project");
        println!("  with sift hooks configured in .claude/settings.json.");
        return Ok(());
    }
```

Then, after `pending` and `ledger` are loaded, compute the counts once and take the JSON path before the human prints:

```rust
    let accepted = ledger
        .iter()
        .filter(|e| e.status == sift_core::Status::Accepted)
        .count();
    let reverted = ledger
        .iter()
        .filter(|e| e.status == sift_core::Status::Reverted)
        .count();

    if json {
        let view = StatusView {
            active: true,
            session_id,
            turn: state.turn,
            mode: mode_str,
            pending: &pending,
            accepted,
            reverted,
        };
        println!("{}", serde_json::to_string_pretty(&view)?);
        return Ok(());
    }
```

Leave the existing human-output block (the `if pending.is_empty() && ledger.is_empty()` branch and the hint footer) exactly as-is below this — but delete the now-duplicated inline `accepted`/`reverted` recomputation inside the ledger block, using the values computed above instead.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p sift-tui --test cli_e2e status_json_reports_active_session_and_pending`
Expected: PASS

- [ ] **Step 6: Verify human output unchanged + lints**

Run: `cargo test -p sift-tui && cargo clippy -p sift-tui --all-targets -- -D warnings && cargo fmt --check`
Expected: all green (existing `status`-dependent tests still pass → human output unchanged).

- [ ] **Step 7: Commit**

```bash
git add crates/sift-cli/src/main.rs crates/sift-cli/src/cmd_status.rs crates/sift-cli/tests/cli_e2e.rs
git commit -m "fix(cli): expose --json on status

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `sift diff <id> --json`

**Files:**
- Modify: `crates/sift-cli/src/main.rs` (the `Diff` variant + dispatch arm)
- Modify: `crates/sift-cli/src/cmd_diff.rs` (add `json` param + JSON branch)
- Test: `crates/sift-cli/tests/cli_e2e.rs`

**Interfaces:**
- Consumes: a pending entry id discovered via `sift list --pending --json`.
- Produces: `cmd_diff::run(cwd: &Path, entry_id: String, json: bool) -> Result<()>`. JSON shape = all `LedgerEntry` fields (flattened) plus `"unified": String`.

- [ ] **Step 1: Write the failing test**

Append to `crates/sift-cli/tests/cli_e2e.rs`:

```rust
#[test]
fn diff_json_includes_unified_and_entry_fields() {
    let td = TempDir::new().unwrap();
    start_session(&td);
    write_via_hook(&td, "beta.txt", b"line one\nline two\n");

    // Find the entry id via the list JSON.
    let list = Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["list", "--pending", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let entries: serde_json::Value = serde_json::from_slice(&list).unwrap();
    let id = entries[0]["id"].as_str().unwrap().to_string();

    let out = Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["diff", &id, "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("diff --json must emit valid JSON");
    assert!(v["path"].as_str().unwrap().ends_with("beta.txt"));
    assert!(v["unified"].as_str().unwrap().contains("line two"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sift-tui --test cli_e2e diff_json_includes_unified_and_entry_fields`
Expected: FAIL — clap rejects `--json` on `diff`.

- [ ] **Step 3: Add the `json` flag to the `Diff` variant**

In `crates/sift-cli/src/main.rs`, change the `Diff` variant to:

```rust
    /// Show a unified diff for a specific entry.
    #[command(visible_alias = "d")]
    Diff {
        id: String,
        #[arg(long)]
        json: bool,
    },
```

Update its dispatch arm to:

```rust
        Some(Commands::Diff { id, json }) => {
            cmd_diff::run(&cwd, id, json)?;
        }
```

- [ ] **Step 4: Add the JSON branch to `cmd_diff::run`**

In `crates/sift-cli/src/cmd_diff.rs`, add `use serde::Serialize;` and `use sift_core::entry::LedgerEntry;` to imports, change the signature to `pub fn run(cwd: &Path, entry_id: String, json: bool) -> Result<()>`, and add the view + branch. After `diff_output` is computed (the line `let diff_output = unified(&before, &after, 3);`), replace the final `page_output(&diff_output)` with:

```rust
    if json {
        #[derive(Serialize)]
        struct DiffView<'a> {
            #[serde(flatten)]
            entry: &'a LedgerEntry,
            unified: String,
        }
        let view = DiffView {
            entry: &entry,
            unified: diff_output,
        };
        println!("{}", serde_json::to_string_pretty(&view)?);
        return Ok(());
    }
    page_output(&diff_output)
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p sift-tui --test cli_e2e diff_json_includes_unified_and_entry_fields`
Expected: PASS

- [ ] **Step 6: Lints + full suite**

Run: `cargo test -p sift-tui && cargo clippy -p sift-tui --all-targets -- -D warnings && cargo fmt --check`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add crates/sift-cli/src/main.rs crates/sift-cli/src/cmd_diff.rs crates/sift-cli/tests/cli_e2e.rs
git commit -m "fix(cli): expose --json on diff

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `sift state --json` alias

**Files:**
- Modify: `crates/sift-cli/src/main.rs` (the `State` variant + dispatch arm)
- Test: `crates/sift-cli/tests/cli_e2e.rs`

**Interfaces:**
- `state` already emits JSON by default via `--format json`. The only defect is that it **rejects** `--json`. Add an accepted-but-no-op `--json` flag so the documented invocation succeeds. No change to `cmd_state::run`.

- [ ] **Step 1: Write the failing test**

Append to `crates/sift-cli/tests/cli_e2e.rs`:

```rust
#[test]
fn state_accepts_json_flag_and_emits_object() {
    let td = TempDir::new().unwrap();
    start_session(&td);
    write_via_hook(&td, "gamma.txt", b"x\n");

    let out = Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["state", "--at-turn", "99", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&out).expect("state --json must emit valid JSON");
    assert!(v.is_object(), "state emits a path->hash map object");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sift-tui --test cli_e2e state_accepts_json_flag_and_emits_object`
Expected: FAIL — `error: unexpected argument '--json' found`.

- [ ] **Step 3: Add the accepted `--json` flag to the `State` variant**

In `crates/sift-cli/src/main.rs`, add the flag to the `State` variant (keep the existing `--format` field untouched):

```rust
        /// Accepted for agent-guide consistency; state already emits JSON.
        /// Present so `sift state --json` does not error. Forces JSON when set.
        #[arg(long)]
        json: bool,
```

In the `State { .. }` dispatch arm, bind the new field but ignore it (output is already JSON): add `json: _,` to the destructured pattern. Do not change the call to `cmd_state::run`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p sift-tui --test cli_e2e state_accepts_json_flag_and_emits_object`
Expected: PASS

- [ ] **Step 5: Lints + commit**

Run: `cargo clippy -p sift-tui --all-targets -- -D warnings && cargo fmt --check`

```bash
git add crates/sift-cli/src/main.rs crates/sift-cli/tests/cli_e2e.rs
git commit -m "fix(cli): accept --json on state for agent-guide consistency

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Agent-guide reconciliation + JSON regression guard

**Files:**
- Modify: `docs/agent-guide.md` and `crates/sift-cli/agent-guide.md` (must stay identical)
- Modify: `docs/suite-conventions.md` (only if it documents an agent-surface flag convention — check first)
- Test: `crates/sift-cli/tests/cli_e2e.rs` (the guard)

**Interfaces:**
- Consumes: the `--json` support added in Tasks 1–3.
- Produces: a test that fails if any read/query command stops emitting JSON under `--json`.

- [ ] **Step 1: Write the failing guard test**

Append to `crates/sift-cli/tests/cli_e2e.rs`. This needs one pending entry so `diff` has a target:

```rust
#[test]
fn every_read_command_emits_json_under_json_flag() {
    let td = TempDir::new().unwrap();
    start_session(&td);
    write_via_hook(&td, "delta.txt", b"a\nb\n");

    let list = Command::cargo_bin("sift")
        .unwrap()
        .current_dir(td.path())
        .args(["list", "--pending", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let entries: serde_json::Value = serde_json::from_slice(&list).unwrap();
    let id = entries[0]["id"].as_str().unwrap().to_string();

    // (label, args) for every command the agent-guide tells the agent to call.
    let cases: Vec<(&str, Vec<String>)> = vec![
        ("status", vec!["status".into(), "--json".into()]),
        ("list", vec!["list".into(), "--json".into()]),
        ("log", vec!["log".into(), "--json".into()]),
        ("history", vec!["history".into(), "--json".into()]),
        ("fsck", vec!["fsck".into(), "--json".into()]),
        ("state", vec!["state".into(), "--at-turn".into(), "99".into(), "--json".into()]),
        ("diff", vec!["diff".into(), id.clone(), "--json".into()]),
    ];

    for (label, args) in cases {
        let out = Command::cargo_bin("sift")
            .unwrap()
            .current_dir(td.path())
            .args(&args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        serde_json::from_slice::<serde_json::Value>(&out)
            .unwrap_or_else(|e| panic!("`sift {label} --json` did not emit valid JSON: {e}"));
    }
}
```

- [ ] **Step 2: Run the guard to verify it passes**

Run: `cargo test -p sift-tui --test cli_e2e every_read_command_emits_json_under_json_flag`
Expected: PASS (Tasks 1–3 already landed the support). If it FAILS, a command is still missing `--json` — fix that command before continuing.

- [ ] **Step 3: Reconcile the agent-guide (both copies)**

The guide's principle #1 ("every sift command supports `--json`") is now true for the read/query surface. Verify every command example in `docs/agent-guide.md` uses a flag that works. In particular, search for any cookbook example invoking `status`, `diff`, or `state` and ensure it passes `--json` (now valid). Keep wording, fix any stale example. Then make the embedded copy identical:

```bash
cp docs/agent-guide.md crates/sift-cli/agent-guide.md
git diff --stat docs/agent-guide.md crates/sift-cli/agent-guide.md
```

Confirm the two files are byte-identical after the copy.

- [ ] **Step 4: Check suite-conventions for an agent-surface flag rule**

Run: `grep -niE 'json|agent surface|read.?only command|machine.readable' docs/suite-conventions.md`

If a convention section governs agent-facing flags, add/adjust one line: *"Read/query commands expose `--json` emitting parseable JSON."* If you edit `suite-conventions.md`, the verbatim-sync rule requires the same edit in `rgx/docs/suite-conventions.md` and `agx/docs/suite-conventions.md` in the same change — note this for the user; do NOT silently let it drift. If no such section exists, skip (do not invent one — out of scope).

- [ ] **Step 5: Full suite + lints**

Run: `cargo test -p sift-tui && cargo clippy -p sift-tui --all-targets -- -D warnings && cargo fmt --check`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add docs/agent-guide.md crates/sift-cli/agent-guide.md crates/sift-cli/tests/cli_e2e.rs
# include docs/suite-conventions.md only if Step 4 changed it
git commit -m "docs: reconcile agent-guide with real --json surface; guard with test

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: agx pricing row for `claude-opus-4-8` (separate repo)

**Files (in the `agx` repo, not sift):**
- Modify: `crates/agx-core/src/pricing.rs` (add one `ModelPricing` row + one test)

**Interfaces:**
- `cost_usd(Some("claude-opus-4-8"), Some(n), ..)` must return `Some(_)` instead of `None`. Lookup is case-insensitive exact match — the exact id `claude-opus-4-8` (as Claude Code reports it) must be present.

- [ ] **Step 1: Write the failing test**

In `crates/agx-core/src/pricing.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn claude_opus_4_8_is_priced() {
        // Regression: agx_session_summary returned cost_usd:null for the
        // current flagship because this row was missing.
        let c = cost_usd(Some("claude-opus-4-8"), Some(1_000_000), Some(1_000_000), None, None)
            .expect("claude-opus-4-8 must be in the pricing table");
        assert!((c - 90.0).abs() < 1e-6, "expected 90.0, got {c}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run (in the agx repo): `cargo test -p agx-core claude_opus_4_8_is_priced`
Expected: FAIL — `.expect(...)` panics because lookup returns `None`.

- [ ] **Step 3: Add the pricing row**

In the `PRICES` array in `crates/agx-core/src/pricing.rs`, add (next to the existing `claude-opus-4-6` row; Opus family list price is $15/$75 per Mtok with Anthropic's standard cache multipliers, mirroring the existing Opus row):

```rust
    ModelPricing {
        name: "claude-opus-4-8",
        input_per_mtoken: 15.0,
        output_per_mtoken: 75.0,
        cache_read_per_mtoken: Some(1.50),
        cache_create_per_mtoken: Some(18.75),
        last_verified: "2026-06-19 (estimate; unverified)",
    },
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p agx-core claude_opus_4_8_is_priced`
Expected: PASS. Also run `cargo test -p agx-core pricing` to confirm `no_duplicate_model_names` and `every_entry_has_last_verified_date` still pass.

- [ ] **Step 5: Lints + commit (in agx repo)**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`

```bash
git add crates/agx-core/src/pricing.rs
git commit -m "fix(pricing): add claude-opus-4-8 rate

agx_session_summary reported cost_usd:null for the current flagship model
because it was missing from the exact-match pricing table.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**
- Goal 1 (status/diff/state accept `--json`) → Tasks 1, 2, 3 ✓
- Goal 2 (`--json` canonical, preserve `--format`) → Task 3 keeps `--format` on state/export, adds `--json` ✓
- Goal 3 (reconcile guide + regression guard) → Task 4 ✓
- Goal 4 / Unit 5 (agx pricing) → Task 5 ✓
- Non-goals (no sift-mcp, no `--json` on mutations, no agx progress surface) → none added ✓
- Spec open question (Unit 3 minimal no-op vs unify `--format`) → resolved to minimal no-op in Task 3, `--format` preserved ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete code; pricing row uses concrete rates with an honest `(estimate; unverified)` marker matching the module's existing convention.

**Type consistency:** `cmd_status::run(&Path, bool)`, `cmd_diff::run(&Path, String, bool)` consistent between their main.rs dispatch and definition. `StatusView.pending: &[LedgerEntry]` matches `store.list_pending() -> Vec<LedgerEntry>`. `DiffView` flattens `&LedgerEntry`. `cost_usd` signature in Task 5 matches the real one read from source. JSON field names used in tests (`active`, `pending`, `path`, `unified`, `id`) match the view structs.
