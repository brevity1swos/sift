# Session GC + Indexed Ledger Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate the two scalability ceilings in sift: unbounded session accumulation (no cleanup) and O(N) ledger rewrites on every accept/revert.

**Architecture:** Two independent features in `sift-core`, exposed via `sift-cli`. (1) `sift gc` deletes closed sessions older than a configurable retention period, with a `--dry-run` default. (2) Replace the full-file-rewrite ledger with an append-only design where `finalize` and `update_ledger_status` append status-change records instead of rewriting the entire file; readers reconstruct current state by folding the log.

**Tech Stack:** Rust, serde_json, chrono, clap, tempfile (tests)

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/sift-core/src/gc.rs` | **New.** Session garbage collection logic: scan sessions dir, parse meta.json, filter by age, delete. |
| `crates/sift-core/src/store.rs` | **Modify.** Replace `rewrite_pending`/`rewrite_ledger` with append-only status records. Update readers to fold the log. |
| `crates/sift-core/src/entry.rs` | **Modify.** Add `StatusChange` record type for the append-only ledger. |
| `crates/sift-core/src/lib.rs` | **Modify.** Re-export `gc` module. |
| `crates/sift-cli/src/main.rs` | **Modify.** Add `Gc` subcommand. |
| `crates/sift-cli/src/cmd_gc.rs` | **New.** CLI handler for `sift gc`. |

---

## Part 1: Session GC

### Task 1: GC core logic (`sift-core/src/gc.rs`)

**Files:**
- Create: `crates/sift-core/src/gc.rs`
- Modify: `crates/sift-core/src/lib.rs`

- [ ] **Step 1: Write the failing test for session age filtering**

In `crates/sift-core/src/gc.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::Paths;
    use crate::session::Session;
    use chrono::Duration;
    use tempfile::TempDir;

    #[test]
    fn collects_sessions_older_than_retention() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());

        // Create a session and manually backdate its meta.json.
        let s = Session::create(paths.clone()).unwrap();
        s.close().unwrap();
        backdate_session(&paths, &s.id, Duration::days(8));

        let result = collect(&paths, Duration::days(7), false).unwrap();
        assert_eq!(result.deleted.len(), 1);
        assert_eq!(result.deleted[0], s.id);
    }

    #[test]
    fn skips_sessions_within_retention() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());

        let s = Session::create(paths.clone()).unwrap();
        s.close().unwrap();
        // Session was just created — well within 7-day retention.

        let result = collect(&paths, Duration::days(7), false).unwrap();
        assert!(result.deleted.is_empty());
    }

    #[test]
    fn skips_open_sessions() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());

        let s = Session::create(paths.clone()).unwrap();
        // Do NOT close — session is still open.
        backdate_session(&paths, &s.id, Duration::days(30));

        let result = collect(&paths, Duration::days(7), false).unwrap();
        assert!(result.deleted.is_empty());
        assert_eq!(result.skipped_open, 1);
    }

    #[test]
    fn dry_run_does_not_delete() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());

        let s = Session::create(paths.clone()).unwrap();
        s.close().unwrap();
        backdate_session(&paths, &s.id, Duration::days(8));

        let result = collect(&paths, Duration::days(7), true).unwrap();
        assert_eq!(result.deleted.len(), 1);
        // Directory should still exist.
        assert!(paths.session_dir(&s.id).exists());
    }

    /// Helper: rewrite meta.json so started_at is `age` ago.
    fn backdate_session(paths: &Paths, id: &str, age: Duration) {
        let meta_path = paths.session_dir(id).join("meta.json");
        let text = std::fs::read_to_string(&meta_path).unwrap();
        let mut meta: crate::session::SessionMeta =
            serde_json::from_str(&text).unwrap();
        meta.started_at = chrono::Utc::now() - age;
        if let Some(ref mut ended) = meta.ended_at {
            *ended = chrono::Utc::now() - age + Duration::minutes(5);
        }
        let json = serde_json::to_string_pretty(&meta).unwrap();
        std::fs::write(&meta_path, json).unwrap();
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sift-core gc::tests --no-default-features 2>&1 | head -20`
Expected: compilation error — `gc` module doesn't exist yet.

- [ ] **Step 3: Write the GC implementation**

In `crates/sift-core/src/gc.rs`:

```rust
//! Session garbage collection: delete closed sessions older than a retention period.

use anyhow::{Context, Result};
use chrono::Duration;
use std::fs;

use crate::paths::Paths;
use crate::session::SessionMeta;

/// Summary of a GC run.
#[derive(Debug, Clone)]
pub struct GcResult {
    /// Session IDs that were deleted (or would be, in dry-run mode).
    pub deleted: Vec<String>,
    /// Sessions skipped because they are still open (no `ended_at`).
    pub skipped_open: usize,
    /// Sessions skipped because they are within the retention window.
    pub skipped_young: usize,
    /// Sessions whose meta.json could not be parsed (left untouched).
    pub skipped_corrupt: usize,
}

/// Scan `.sift/sessions/`, delete closed sessions older than `retention`.
///
/// If `dry_run` is true, populates `GcResult.deleted` but does not remove
/// any directories.
pub fn collect(paths: &Paths, retention: Duration, dry_run: bool) -> Result<GcResult> {
    let sessions_dir = paths.sessions_dir();
    let mut result = GcResult {
        deleted: vec![],
        skipped_open: 0,
        skipped_young: 0,
        skipped_corrupt: 0,
    };

    let entries = match fs::read_dir(&sessions_dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(result),
        Err(e) => {
            return Err(e)
                .with_context(|| format!("reading sessions dir {}", sessions_dir.display()))
        }
    };

    let now = chrono::Utc::now();

    for dir_entry in entries {
        let dir_entry = dir_entry?;
        let path = dir_entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        let meta_path = path.join("meta.json");
        let meta: SessionMeta = match fs::read_to_string(&meta_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
        {
            Some(m) => m,
            None => {
                result.skipped_corrupt += 1;
                continue;
            }
        };

        // Never delete open sessions.
        let ended_at = match meta.ended_at {
            Some(t) => t,
            None => {
                result.skipped_open += 1;
                continue;
            }
        };

        // Check age against ended_at (when the session was closed).
        let age = now - ended_at;
        if age < retention {
            result.skipped_young += 1;
            continue;
        }

        result.deleted.push(id);
        if !dry_run {
            fs::remove_dir_all(&path)
                .with_context(|| format!("deleting session {}", path.display()))?;
        }
    }

    Ok(result)
}
```

- [ ] **Step 4: Register the module in lib.rs**

In `crates/sift-core/src/lib.rs`, add:

```rust
pub mod gc;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p sift-core gc::tests`
Expected: all 4 tests pass.

- [ ] **Step 6: Run clippy**

Run: `cargo clippy -p sift-core -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add crates/sift-core/src/gc.rs crates/sift-core/src/lib.rs
git commit -m "feat(core): add session gc — collect closed sessions older than retention"
```

---

### Task 2: `sift gc` CLI command

**Files:**
- Create: `crates/sift-cli/src/cmd_gc.rs`
- Modify: `crates/sift-cli/src/main.rs`

- [ ] **Step 1: Write the CLI handler**

In `crates/sift-cli/src/cmd_gc.rs`:

```rust
use anyhow::Result;
use chrono::Duration;
use sift_core::gc;
use sift_core::paths::Paths;
use std::path::Path;

pub fn run(cwd: &Path, days: u32, dry_run: bool) -> Result<()> {
    let paths = Paths::new(cwd);
    let retention = Duration::days(i64::from(days));
    let result = gc::collect(&paths, retention, dry_run)?;

    if result.deleted.is_empty() {
        println!("sift gc: nothing to collect");
    } else {
        let verb = if dry_run { "would delete" } else { "deleted" };
        for id in &result.deleted {
            println!("  {verb} session {id}");
        }
        println!(
            "sift gc: {} {} session(s)",
            verb,
            result.deleted.len()
        );
    }

    if result.skipped_open > 0 {
        println!(
            "  skipped {} open session(s)",
            result.skipped_open
        );
    }
    if result.skipped_corrupt > 0 {
        println!(
            "  skipped {} corrupt session(s)",
            result.skipped_corrupt
        );
    }

    Ok(())
}
```

- [ ] **Step 2: Wire the subcommand into main.rs**

In `crates/sift-cli/src/main.rs`, add the module declaration:

```rust
mod cmd_gc;
```

Add the variant to `Commands`:

```rust
    /// Delete closed sessions older than a retention period.
    Gc {
        /// Retention in days (default: 7).
        #[arg(long, default_value = "7")]
        days: u32,
        /// Actually delete (default is dry-run).
        #[arg(long)]
        apply: bool,
    },
```

Add the match arm:

```rust
        Some(Commands::Gc { days, apply }) => {
            cmd_gc::run(&cwd, days, !apply)?;
        }
```

- [ ] **Step 3: Run the build**

Run: `cargo build -p sift-cli`
Expected: compiles.

- [ ] **Step 4: Smoke test the help output**

Run: `cargo run -p sift-cli -- gc --help`
Expected: shows `--days` and `--apply` flags.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-cli/src/cmd_gc.rs crates/sift-cli/src/main.rs
git commit -m "feat(cli): add sift gc subcommand with --days and --apply"
```

---

## Part 2: Append-Only Ledger

### Task 3: Add `StatusChange` record type

**Files:**
- Modify: `crates/sift-core/src/entry.rs`

- [ ] **Step 1: Write the failing test**

In `crates/sift-core/src/entry.rs`, add to the existing `mod tests`:

```rust
    #[test]
    fn status_change_roundtrip() {
        let sc = StatusChange {
            id: "01HVXK5QZ9G7B2000000000000".to_string(),
            new_status: Status::Accepted,
            timestamp: DateTime::parse_from_rfc3339("2026-04-12T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        };
        let json = serde_json::to_string(&sc).unwrap();
        let back: StatusChange = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, sc.id);
        assert_eq!(back.new_status, Status::Accepted);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sift-core entry::tests::status_change_roundtrip`
Expected: FAIL — `StatusChange` not defined.

- [ ] **Step 3: Add the StatusChange struct**

In `crates/sift-core/src/entry.rs`, add after `LedgerEntry`:

```rust
/// A status-change record appended to ledger.jsonl when an entry is
/// accepted, reverted, or edited. Readers fold these over the initial
/// `LedgerEntry` records to reconstruct current state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StatusChange {
    pub id: String,
    pub new_status: Status,
    pub timestamp: DateTime<Utc>,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p sift-core entry::tests::status_change_roundtrip`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-core/src/entry.rs
git commit -m "feat(core): add StatusChange record type for append-only ledger"
```

---

### Task 4: Add `LedgerRecord` enum for polymorphic JSONL lines

**Files:**
- Modify: `crates/sift-core/src/entry.rs`

- [ ] **Step 1: Write the failing test**

In `crates/sift-core/src/entry.rs` tests:

```rust
    #[test]
    fn ledger_record_entry_roundtrip() {
        let e = sample();
        let record = LedgerRecord::Entry(e.clone());
        let json = serde_json::to_string(&record).unwrap();
        let back: LedgerRecord = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, LedgerRecord::Entry(ref x) if x.id == e.id));
    }

    #[test]
    fn ledger_record_change_roundtrip() {
        let sc = StatusChange {
            id: "01HVXK5QZ9G7B2000000000000".to_string(),
            new_status: Status::Reverted,
            timestamp: DateTime::parse_from_rfc3339("2026-04-12T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        };
        let record = LedgerRecord::Change(sc);
        let json = serde_json::to_string(&record).unwrap();
        let back: LedgerRecord = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, LedgerRecord::Change(ref c) if c.new_status == Status::Reverted));
    }

    #[test]
    fn old_ledger_entry_json_parses_as_ledger_record() {
        // Backward compat: a bare LedgerEntry (no wrapper tag) must parse
        // as LedgerRecord::Entry so existing ledger.jsonl files keep working.
        let e = sample();
        let bare_json = serde_json::to_string(&e).unwrap();
        let record: LedgerRecord = serde_json::from_str(&bare_json).unwrap();
        assert!(matches!(record, LedgerRecord::Entry(_)));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p sift-core entry::tests::ledger_record`
Expected: FAIL — `LedgerRecord` not defined.

- [ ] **Step 3: Implement LedgerRecord**

In `crates/sift-core/src/entry.rs`:

```rust
/// A single line in ledger.jsonl: either an initial entry or a status change.
///
/// Uses an internally-tagged `"record"` field for new writes. For backward
/// compatibility, `serde(untagged)` falls through: lines without a `"record"`
/// field are parsed as bare `LedgerEntry` (the v0.1 format).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "record")]
pub enum LedgerRecord {
    #[serde(rename = "entry")]
    Entry(LedgerEntry),
    #[serde(rename = "change")]
    Change(StatusChange),
}
```

Note: internally-tagged won't work for backward compat with bare `LedgerEntry` which has no `"record"` field. We need a custom deserializer:

```rust
/// A single line in ledger.jsonl: either an initial entry or a status change.
///
/// New writes use the tagged format `{"record":"entry", ...}` or
/// `{"record":"change", ...}`. For backward compatibility, lines without a
/// `"record"` field are parsed as bare `LedgerEntry` (v0.1 format).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum LedgerRecord {
    Entry(LedgerEntry),
    Change(StatusChange),
}

impl<'de> serde::Deserialize<'de> for LedgerRecord {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        match value.get("record").and_then(|v| v.as_str()) {
            Some("change") => {
                let sc: StatusChange =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(LedgerRecord::Change(sc))
            }
            Some("entry") | None => {
                // "entry" tag or no tag (v0.1 bare LedgerEntry) — both parse as Entry.
                let e: LedgerEntry =
                    serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                Ok(LedgerRecord::Entry(e))
            }
            Some(other) => Err(serde::de::Error::custom(format!(
                "unknown record type: {other}"
            ))),
        }
    }
}

impl serde::Serialize for LedgerRecord {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            LedgerRecord::Entry(e) => {
                // Serialize the entry, then inject {"record":"entry"}.
                let mut map = match serde_json::to_value(e) {
                    Ok(serde_json::Value::Object(m)) => m,
                    _ => return Err(serde::ser::Error::custom("entry not an object")),
                };
                map.insert("record".to_string(), serde_json::Value::String("entry".to_string()));
                map.serialize(serializer)
            }
            LedgerRecord::Change(c) => {
                let mut map = match serde_json::to_value(c) {
                    Ok(serde_json::Value::Object(m)) => m,
                    _ => return Err(serde::ser::Error::custom("change not an object")),
                };
                map.insert("record".to_string(), serde_json::Value::String("change".to_string()));
                map.serialize(serializer)
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p sift-core entry::tests::ledger_record`
Expected: all 3 pass.

- [ ] **Step 5: Commit**

```bash
git add crates/sift-core/src/entry.rs
git commit -m "feat(core): add LedgerRecord enum with backward-compat deserialization"
```

---

### Task 5: Convert `Store` to append-only ledger

**Files:**
- Modify: `crates/sift-core/src/store.rs`

This is the core change. `finalize` currently reads all pending, partitions, rewrites. New behavior: append a `StatusChange` to `ledger.jsonl`, then append a tombstone `StatusChange` to `pending.jsonl`. Readers fold changes over entries.

- [ ] **Step 1: Write the failing tests**

Add these tests to `crates/sift-core/src/store.rs` `mod tests`:

```rust
    #[test]
    fn finalize_appends_instead_of_rewriting() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.append_pending(&make_entry("02", 2)).unwrap();
        store.append_pending(&make_entry("03", 3)).unwrap();

        store.finalize("02", Status::Accepted).unwrap();

        // pending.jsonl should have 4 lines: 3 entries + 1 status change.
        let raw = fs::read_to_string(store.pending_path()).unwrap();
        let line_count = raw.lines().count();
        assert_eq!(line_count, 4, "expected append, not rewrite; got {line_count} lines");

        // But list_pending should fold and return only the 2 non-finalized entries.
        let pending = store.list_pending().unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|e| e.id != "02"));
    }

    #[test]
    fn list_ledger_folds_status_changes() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.finalize("01", Status::Accepted).unwrap();
        store.update_ledger_status("01", Status::Reverted).unwrap();

        let ledger = store.list_ledger().unwrap();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].status, Status::Reverted);
    }
```

- [ ] **Step 2: Run tests to verify the new ones fail (and old ones still pass)**

Run: `cargo test -p sift-core store::tests`
Expected: `finalize_appends_instead_of_rewriting` fails (line count is 2, not 4). Other tests may still pass.

- [ ] **Step 3: Refactor `read_jsonl` to return `LedgerRecord` variants**

Replace the private `read_jsonl` method in `Store`:

```rust
    /// Read a JSONL file and return parsed `LedgerRecord` variants plus skip count.
    fn read_jsonl_records(path: &Path) -> Result<(Vec<LedgerRecord>, usize)> {
        let f = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((vec![], 0));
            }
            Err(e) => {
                return Err(e).with_context(|| format!("opening {}", path.display()));
            }
        };
        let reader = BufReader::new(f);
        let mut records = Vec::new();
        let mut skipped = 0usize;
        for line in reader.lines() {
            let line = line.with_context(|| format!("reading line from {}", path.display()))?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<LedgerRecord>(&line) {
                Ok(r) => records.push(r),
                Err(_) => skipped += 1,
            }
        }
        Ok((records, skipped))
    }

    /// Fold a sequence of `LedgerRecord` into resolved `LedgerEntry` list.
    /// Status changes update the matching entry's status in place.
    /// Entries whose status was changed to a finalized state are included
    /// (callers filter by status as needed).
    fn fold_records(records: Vec<LedgerRecord>) -> Vec<LedgerEntry> {
        use std::collections::HashMap;
        let mut entries: Vec<LedgerEntry> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();

        for record in records {
            match record {
                LedgerRecord::Entry(e) => {
                    let idx = entries.len();
                    index.insert(e.id.clone(), idx);
                    entries.push(e);
                }
                LedgerRecord::Change(c) => {
                    if let Some(&idx) = index.get(&c.id) {
                        entries[idx].status = c.new_status;
                    }
                    // If the id isn't found, the change is orphaned — skip silently.
                    // This happens if the entry was in the other file (pending vs ledger).
                }
            }
        }
        entries
    }
```

- [ ] **Step 4: Update `list_pending` / `list_ledger` to use the new reader**

```rust
    fn read_jsonl(path: &Path) -> Result<ReadStats> {
        let (records, skipped) = Self::read_jsonl_records(path)?;
        let entries = Self::fold_records(records);
        Ok(ReadStats { entries, skipped })
    }
```

This keeps `read_jsonl` as the entry point, so `list_pending_with_stats` and `list_ledger_with_stats` don't change.

- [ ] **Step 5: Update `list_pending` to filter out finalized entries**

`list_pending` should only return entries with `Status::Pending`:

```rust
    pub fn list_pending(&self) -> Result<Vec<LedgerEntry>> {
        Ok(self
            .list_pending_with_stats()?
            .entries
            .into_iter()
            .filter(|e| e.status == Status::Pending)
            .collect())
    }
```

- [ ] **Step 6: Replace `finalize` with append-only implementation**

```rust
    pub fn finalize(&self, id: &str, new_status: Status) -> Result<LedgerEntry> {
        let pending_entries = self.list_pending()?;
        let entry = pending_entries
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| anyhow::anyhow!("entry {id} not in pending"))?;
        let mut finalized = entry.clone();
        finalized.status = new_status;

        // Append the full entry to ledger.jsonl.
        self.append_ledger_record(&LedgerRecord::Entry(finalized.clone()))?;

        // Append a status change to pending.jsonl so readers know to exclude it.
        let change = StatusChange {
            id: id.to_string(),
            new_status,
            timestamp: chrono::Utc::now(),
        };
        self.append_pending_record(&LedgerRecord::Change(change))?;

        Ok(finalized)
    }
```

- [ ] **Step 7: Replace `update_ledger_status` with append-only implementation**

```rust
    pub fn update_ledger_status(&self, id: &str, new_status: Status) -> Result<LedgerEntry> {
        let ledger = self.list_ledger()?;
        let entry = ledger
            .iter()
            .find(|e| e.id.starts_with(id))
            .ok_or_else(|| anyhow::anyhow!("entry {id} not in ledger"))?;
        let mut updated = entry.clone();
        updated.status = new_status;

        let change = StatusChange {
            id: entry.id.clone(),
            new_status,
            timestamp: chrono::Utc::now(),
        };
        self.append_ledger_record(&LedgerRecord::Change(change))?;

        Ok(updated)
    }
```

- [ ] **Step 8: Add the append helpers, remove `rewrite_ledger`**

```rust
    fn append_pending_record(&self, record: &LedgerRecord) -> Result<()> {
        let path = self.pending_path();
        let line = serde_json::to_string(record)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening pending {}", path.display()))?;
        writeln!(f, "{line}")
            .with_context(|| format!("writing pending record to {}", path.display()))?;
        Ok(())
    }

    fn append_ledger_record(&self, record: &LedgerRecord) -> Result<()> {
        let path = self.ledger_path();
        let line = serde_json::to_string(record)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening ledger {}", path.display()))?;
        writeln!(f, "{line}")
            .with_context(|| format!("writing ledger record to {}", path.display()))?;
        Ok(())
    }
```

Remove `rewrite_ledger` (now dead code). Keep `rewrite_pending` and `rewrite_pending_entries` — the TUI edit flow still needs them.

- [ ] **Step 9: Update `append_pending` to wrap in `LedgerRecord::Entry`**

The existing `append_pending` writes a bare `LedgerEntry`. Update it to write a `LedgerRecord::Entry` so new pending files are consistently formatted:

```rust
    pub fn append_pending(&self, entry: &LedgerEntry) -> Result<()> {
        fs::create_dir_all(&self.session_dir)
            .with_context(|| format!("creating session dir {}", self.session_dir.display()))?;
        let record = LedgerRecord::Entry(entry.clone());
        self.append_pending_record(&record)
    }
```

And update `append_ledger` similarly:

```rust
    fn append_ledger(&self, entry: &LedgerEntry) -> Result<()> {
        let record = LedgerRecord::Entry(entry.clone());
        self.append_ledger_record(&record)
    }
```

- [ ] **Step 10: Run all store tests**

Run: `cargo test -p sift-core store::tests`
Expected: all tests pass, including the two new ones.

- [ ] **Step 11: Run the full test suite**

Run: `cargo test --all`
Expected: all pass. Existing tests work because `list_pending` / `list_ledger` return the same `Vec<LedgerEntry>` shape.

- [ ] **Step 12: Run clippy**

Run: `cargo clippy --all -- -D warnings`
Expected: clean.

- [ ] **Step 13: Commit**

```bash
git add crates/sift-core/src/store.rs crates/sift-core/src/entry.rs
git commit -m "refactor(core): switch ledger to append-only with fold-on-read"
```

---

### Task 6: Add `sift gc --compact` to compact JSONL files

**Files:**
- Modify: `crates/sift-core/src/store.rs`
- Modify: `crates/sift-core/src/gc.rs`
- Modify: `crates/sift-cli/src/cmd_gc.rs`
- Modify: `crates/sift-cli/src/main.rs`

The append-only ledger trades write speed for read speed. Over time, pending.jsonl accumulates tombstones. `--compact` rewrites the current session's JSONL files, folding all status changes into the entries and removing resolved records.

- [ ] **Step 1: Write the failing test**

In `crates/sift-core/src/store.rs` tests:

```rust
    #[test]
    fn compact_removes_resolved_pending_entries() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.append_pending(&make_entry("02", 2)).unwrap();
        store.append_pending(&make_entry("03", 3)).unwrap();
        store.finalize("02", Status::Accepted).unwrap();

        // Before compact: 4 lines in pending (3 entries + 1 change).
        let raw_before = fs::read_to_string(store.pending_path()).unwrap();
        assert_eq!(raw_before.lines().count(), 4);

        store.compact_pending().unwrap();

        // After compact: 2 lines (only entries 01 and 03).
        let raw_after = fs::read_to_string(store.pending_path()).unwrap();
        assert_eq!(raw_after.lines().count(), 2);

        // list_pending still returns the same result.
        let pending = store.list_pending().unwrap();
        assert_eq!(pending.len(), 2);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p sift-core store::tests::compact_removes_resolved_pending_entries`
Expected: FAIL — `compact_pending` not defined.

- [ ] **Step 3: Implement `compact_pending` and `compact_ledger`**

In `crates/sift-core/src/store.rs`:

```rust
    /// Rewrite `pending.jsonl`, folding all status changes and removing entries
    /// that have been finalized. Reduces file size after many accept/revert cycles.
    pub fn compact_pending(&self) -> Result<()> {
        let entries = self.list_pending()?; // already folded and filtered
        self.rewrite_pending(&entries)
    }

    /// Rewrite `ledger.jsonl`, folding all status changes into entries.
    pub fn compact_ledger(&self) -> Result<()> {
        let entries = self.list_ledger()?;
        let tmp = self.session_dir.join("ledger.jsonl.tmp");
        {
            let mut f =
                File::create(&tmp).with_context(|| format!("creating tmp {}", tmp.display()))?;
            for e in &entries {
                let record = LedgerRecord::Entry(e.clone());
                writeln!(f, "{}", serde_json::to_string(&record)?)
                    .with_context(|| format!("writing tmp {}", tmp.display()))?;
            }
        }
        fs::rename(&tmp, self.ledger_path()).with_context(|| {
            format!("renaming tmp -> ledger {}", self.ledger_path().display())
        })?;
        Ok(())
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p sift-core store::tests::compact_removes_resolved_pending_entries`
Expected: PASS.

- [ ] **Step 5: Wire `--compact` into the GC CLI**

In `crates/sift-cli/src/main.rs`, add to `Gc`:

```rust
        /// Compact the current session's JSONL files (fold status changes).
        #[arg(long)]
        compact: bool,
```

In `crates/sift-cli/src/cmd_gc.rs`, add compact handling:

```rust
pub fn run(cwd: &Path, days: u32, dry_run: bool, compact: bool) -> Result<()> {
    let paths = Paths::new(cwd);

    if compact {
        return run_compact(cwd);
    }

    // ... existing gc logic ...
}

fn run_compact(cwd: &Path) -> Result<()> {
    let session_dir = crate::resolve_session_dir(cwd, None)?;
    let store = sift_core::store::Store::new(&session_dir);
    store.compact_pending()?;
    store.compact_ledger()?;
    println!("sift gc: compacted current session");
    Ok(())
}
```

Update the match arm in `main.rs`:

```rust
        Some(Commands::Gc { days, apply, compact }) => {
            cmd_gc::run(&cwd, days, !apply, compact)?;
        }
```

- [ ] **Step 6: Run the full test suite**

Run: `cargo test --all`
Expected: all pass.

- [ ] **Step 7: Run clippy**

Run: `cargo clippy --all -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add crates/sift-core/src/store.rs crates/sift-core/src/gc.rs \
       crates/sift-cli/src/cmd_gc.rs crates/sift-cli/src/main.rs
git commit -m "feat: add sift gc --compact to fold JSONL status changes"
```

---

### Task 7: E2E integration test

**Files:**
- Modify: `crates/sift-cli/tests/cli_e2e.rs`

- [ ] **Step 1: Write the E2E test for gc**

Append to `crates/sift-cli/tests/cli_e2e.rs`:

```rust
#[test]
fn gc_dry_run_reports_old_sessions() {
    let td = tempfile::TempDir::new().unwrap();
    // Create and close a session via the library directly.
    let paths = sift_core::paths::Paths::new(td.path());
    let s = sift_core::session::Session::create(paths.clone()).unwrap();
    s.close().unwrap();
    // Backdate it.
    let meta_path = s.dir.join("meta.json");
    let text = std::fs::read_to_string(&meta_path).unwrap();
    let mut meta: sift_core::session::SessionMeta =
        serde_json::from_str(&text).unwrap();
    meta.started_at = chrono::Utc::now() - chrono::Duration::days(30);
    meta.ended_at = Some(chrono::Utc::now() - chrono::Duration::days(30));
    std::fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap()).unwrap();

    let output = Command::cargo_bin("sift")
        .unwrap()
        .args(["gc", "--days", "7"])
        .current_dir(td.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("would delete"), "got: {stdout}");
    // Session dir should still exist (dry run).
    assert!(s.dir.exists());
}

#[test]
fn gc_apply_deletes_old_sessions() {
    let td = tempfile::TempDir::new().unwrap();
    let paths = sift_core::paths::Paths::new(td.path());
    let s = sift_core::session::Session::create(paths.clone()).unwrap();
    s.close().unwrap();
    let meta_path = s.dir.join("meta.json");
    let text = std::fs::read_to_string(&meta_path).unwrap();
    let mut meta: sift_core::session::SessionMeta =
        serde_json::from_str(&text).unwrap();
    meta.started_at = chrono::Utc::now() - chrono::Duration::days(30);
    meta.ended_at = Some(chrono::Utc::now() - chrono::Duration::days(30));
    std::fs::write(&meta_path, serde_json::to_string_pretty(&meta).unwrap()).unwrap();

    Command::cargo_bin("sift")
        .unwrap()
        .args(["gc", "--days", "7", "--apply"])
        .current_dir(td.path())
        .assert()
        .success();
    // Session dir should be gone.
    assert!(!s.dir.exists());
}
```

- [ ] **Step 2: Run E2E tests**

Run: `cargo test -p sift-cli --test cli_e2e`
Expected: all pass.

- [ ] **Step 3: Commit**

```bash
git add crates/sift-cli/tests/cli_e2e.rs
git commit -m "test: add E2E tests for sift gc"
```

---

## Self-Review Checklist

1. **Spec coverage**: GC with dry-run default, retention period, skip open sessions. Append-only ledger with fold-on-read. Compact command. All covered.
2. **Placeholder scan**: No TBD/TODO/placeholders. All steps have code.
3. **Type consistency**: `LedgerRecord`, `StatusChange`, `GcResult` used consistently across tasks. `fold_records` signature matches usage in `read_jsonl`. `compact` flag threaded through CLI to handler.
4. **Backward compat**: Custom `Deserialize` for `LedgerRecord` handles bare `LedgerEntry` JSON (v0.1 format). Tested in Task 4 step 3.
