//! The ledger store: append-only pending.jsonl + finalized ledger.jsonl.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::entry::{LedgerEntry, Op, Status, StatusChange};
use crate::paths::{validate_relative_path, Paths};
use crate::snapshot::SnapshotStore;

pub struct Store {
    session_dir: PathBuf,
}

/// Result of reading a JSONL ledger file, including the count of lines that
/// failed to parse (for example after a hook crash left a partial write).
///
/// Callers that only need the successfully-parsed entries can use
/// `list_pending` / `list_ledger` and discard the count; callers that want
/// to surface "ledger is partially broken" diagnostics should use the
/// `_with_stats` variants.
#[derive(Debug, Clone)]
pub struct ReadStats {
    pub entries: Vec<LedgerEntry>,
    pub skipped: usize,
}

impl Store {
    pub fn new(session_dir: impl Into<PathBuf>) -> Self {
        Self {
            session_dir: session_dir.into(),
        }
    }

    pub fn pending_path(&self) -> PathBuf {
        self.session_dir.join("pending.jsonl")
    }
    pub fn ledger_path(&self) -> PathBuf {
        self.session_dir.join("ledger.jsonl")
    }
    /// Side-file of status changes (tombstones) for pending entries. Reading
    /// pending involves folding these over the bare entries in `pending.jsonl`.
    pub(crate) fn pending_changes_path(&self) -> PathBuf {
        self.session_dir.join("pending_changes.jsonl")
    }
    /// Side-file of status changes for ledger entries.
    pub(crate) fn ledger_changes_path(&self) -> PathBuf {
        self.session_dir.join("ledger_changes.jsonl")
    }

    /// Append a pending entry to `pending.jsonl`.
    pub fn append_pending(&self, entry: &LedgerEntry) -> Result<()> {
        fs::create_dir_all(&self.session_dir)
            .with_context(|| format!("creating session dir {}", self.session_dir.display()))?;
        let path = self.pending_path();
        let line = serde_json::to_string(entry)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening pending {}", path.display()))?;
        writeln!(f, "{line}")
            .with_context(|| format!("writing pending line to {}", path.display()))?;
        Ok(())
    }

    /// Read all pending entries, silently skipping malformed lines.
    /// Only entries whose current status is `Pending` are returned — entries
    /// finalized via `finalize()` have a tombstone in `pending_changes.jsonl`
    /// and are filtered out here.
    pub fn list_pending(&self) -> Result<Vec<LedgerEntry>> {
        Ok(self.list_pending_with_stats()?.entries)
    }

    /// Read all finalized entries (with status changes folded in), silently
    /// skipping malformed lines.
    pub fn list_ledger(&self) -> Result<Vec<LedgerEntry>> {
        Ok(self.list_ledger_with_stats()?.entries)
    }

    /// Read pending entries and return a `ReadStats` including the count
    /// of lines that failed to parse across both the entries file and the
    /// status-changes side-file. Useful for `sift fsck` and for tests
    /// that need to assert on the skip count.
    ///
    /// Status changes in `pending_changes.jsonl` are folded over the bare
    /// entries, and the result is filtered to `Status::Pending` so that
    /// callers only see entries that have not yet been finalized.
    pub fn list_pending_with_stats(&self) -> Result<ReadStats> {
        let raw = Self::read_jsonl(&self.pending_path())?;
        let (changes, changes_skipped) = Self::read_changes(&self.pending_changes_path())?;
        let folded = Self::apply_changes(raw.entries, &changes);
        let entries = folded
            .into_iter()
            .filter(|e| e.status == Status::Pending)
            .collect();
        Ok(ReadStats {
            entries,
            skipped: raw.skipped + changes_skipped,
        })
    }

    /// Read finalized entries and return a `ReadStats`. Status changes in
    /// `ledger_changes.jsonl` are folded over the bare entries.
    pub fn list_ledger_with_stats(&self) -> Result<ReadStats> {
        let raw = Self::read_jsonl(&self.ledger_path())?;
        let (changes, changes_skipped) = Self::read_changes(&self.ledger_changes_path())?;
        let entries = Self::apply_changes(raw.entries, &changes);
        Ok(ReadStats {
            entries,
            skipped: raw.skipped + changes_skipped,
        })
    }

    /// Read a `StatusChange` JSONL file. Returns an empty vec if the file does
    /// not exist. Malformed lines are counted and skipped.
    fn read_changes(path: &Path) -> Result<(Vec<StatusChange>, usize)> {
        let f = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Vec::new(), 0));
            }
            Err(e) => {
                return Err(e).with_context(|| format!("opening {}", path.display()));
            }
        };
        let reader = BufReader::new(f);
        let mut changes = Vec::new();
        let mut skipped = 0usize;
        for line in reader.lines() {
            let line = line.with_context(|| format!("reading line from {}", path.display()))?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<StatusChange>(&line) {
                Ok(c) => changes.push(c),
                Err(_) => skipped += 1,
            }
        }
        Ok((changes, skipped))
    }

    /// Fold a list of status changes over entries. Later changes override
    /// earlier ones for the same id (last-write-wins). Changes with no
    /// matching entry id are silently ignored — they may appear transiently
    /// during concurrent reads/writes and are harmless.
    fn apply_changes(
        mut entries: Vec<LedgerEntry>,
        changes: &[StatusChange],
    ) -> Vec<LedgerEntry> {
        if changes.is_empty() {
            return entries;
        }
        let mut map: HashMap<&str, Status> = HashMap::new();
        for c in changes {
            map.insert(c.id.as_str(), c.new_status);
        }
        for e in entries.iter_mut() {
            if let Some(&s) = map.get(e.id.as_str()) {
                e.status = s;
            }
        }
        entries
    }

    /// Append one status-change record to the given changes file.
    fn append_change(&self, path: &Path, change: &StatusChange) -> Result<()> {
        fs::create_dir_all(&self.session_dir)
            .with_context(|| format!("creating session dir {}", self.session_dir.display()))?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("opening changes {}", path.display()))?;
        writeln!(f, "{}", serde_json::to_string(change)?)
            .with_context(|| format!("writing changes line to {}", path.display()))?;
        Ok(())
    }

    fn read_jsonl(path: &Path) -> Result<ReadStats> {
        // Match on `open` directly instead of a prior `exists()` check so a
        // concurrent delete between the two calls doesn't turn a missing file
        // into an I/O error.
        let f = match File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ReadStats {
                    entries: vec![],
                    skipped: 0,
                });
            }
            Err(e) => {
                return Err(e).with_context(|| format!("opening {}", path.display()));
            }
        };
        let reader = BufReader::new(f);
        let mut entries = Vec::new();
        let mut skipped = 0usize;
        for line in reader.lines() {
            let line = line.with_context(|| format!("reading line from {}", path.display()))?;
            if line.trim().is_empty() {
                continue;
            }
            // Note: `BufReader::lines()` splits on '\n'. A hook crash that
            // leaves a partial write with NO trailing newline will merge the
            // partial record with the first valid record that follows it,
            // causing BOTH to be counted as one skipped malformed line.
            // The worst-case data loss for this design is "one corrupted
            // write costs the next one valid entry." A future `sift fsck`
            // command should check for this by parsing byte-for-byte.
            match serde_json::from_str::<LedgerEntry>(&line) {
                Ok(e) => entries.push(e),
                Err(_) => skipped += 1,
            }
        }
        Ok(ReadStats { entries, skipped })
    }

    /// Move an entry from pending to ledger with the given final status.
    /// The physical `pending.jsonl` file is not rewritten; instead, a
    /// tombstone is appended to `pending_changes.jsonl` so subsequent
    /// `list_pending()` calls filter the entry out. The full entry (with
    /// its new status) is appended to `ledger.jsonl`.
    ///
    /// Returns the entry as written, or Err if the id is not in pending.
    ///
    /// **Durability:** `append_ledger` runs before `append_change` so a crash
    /// between the two leaves the entry duplicated (present in both files)
    /// rather than lost. A future fsck/compaction pass can resolve duplicates;
    /// a vanished entry cannot be recovered.
    ///
    /// **Concurrency & crash duplicates:** under two concurrent writers, or a
    /// crash after `append_ledger` but before `append_change`, the ledger may
    /// contain multiple rows for the same id (with potentially different
    /// statuses). Readers must tolerate this: `list_ledger` returns both rows
    /// and `apply_changes` folds last-write-wins across all matching rows, so
    /// they all converge to the same status — but the physical file keeps
    /// both. A future `sift fsck` / `sift gc --compact` command will dedupe.
    pub fn finalize(&self, id: &str, new_status: Status) -> Result<LedgerEntry> {
        let pending = self.list_pending()?;
        let entry = pending
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| anyhow::anyhow!("entry {id} not in pending"))?;
        let mut finalized = entry.clone();
        finalized.status = new_status;
        // Append to ledger FIRST so a subsequent change-file append failure
        // leaves the entry duplicated (present in both) rather than lost.
        self.append_ledger(&finalized)?;
        let change = StatusChange {
            id: id.to_string(),
            new_status,
            timestamp: chrono::Utc::now(),
        };
        self.append_change(&self.pending_changes_path(), &change)?;
        Ok(finalized)
    }

    /// Restore a reverted entry's `snapshot_before` to its path in the project root.
    /// Must be called AFTER `finalize(id, Status::Reverted)`.
    ///
    /// # Security
    /// `entry.path` must be a relative path that descends into `project_root`.
    /// An absolute path component (e.g. from a malformed ledger entry) would
    /// cause `Path::join` to silently replace the base, creating a traversal.
    /// We validate this before any filesystem operation.
    pub fn restore_snapshot(
        &self,
        entry: &LedgerEntry,
        project_root: &Path,
        paths: &Paths,
        session_id: &str,
    ) -> Result<()> {
        // Guard: a poisoned ledger entry must not direct writes outside the
        // project root. `validate_relative_path` rejects absolute paths and
        // `..` components.
        validate_relative_path(&entry.path)?;
        let target = project_root.join(&entry.path);
        match (&entry.op, &entry.snapshot_before) {
            (Op::Create, _) => {
                // Revert of a Create deletes the file.
                if target.exists() {
                    fs::remove_file(&target)
                        .with_context(|| format!("removing {}", target.display()))?;
                }
            }
            (_, Some(before)) => {
                let snap_store = SnapshotStore::new(paths, session_id);
                let bytes = snap_store.get(before)?;
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("creating parent {}", parent.display()))?;
                }
                fs::write(&target, bytes)
                    .with_context(|| format!("writing restored {}", target.display()))?;
            }
            (Op::Delete, None) => {
                // Silently skip — a Delete with no before snapshot is nonsense
                // (the revert has nothing to restore). This should never happen
                // for entries produced by post_tool; if it does, it is safe to
                // ignore.
            }
            (Op::Modify, None) => {
                anyhow::bail!("Modify entry {} has no snapshot_before", entry.id);
            }
        }
        Ok(())
    }

    /// Update an entry's status in the ledger (for reverting accepted entries).
    /// The physical `ledger.jsonl` file is not rewritten; instead a
    /// `StatusChange` is appended to `ledger_changes.jsonl` and folded over
    /// the ledger on subsequent reads. `id` may be a prefix of the full id.
    /// Returns the entry with its new status, or Err if not found.
    ///
    /// **Concurrency & bloat:** two concurrent updates for the same id append
    /// two `StatusChange` rows to `ledger_changes.jsonl`; the fold converges
    /// to the last-write-wins value but the change file grows. `sift gc
    /// --compact` collapses the history.
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
        self.append_change(&self.ledger_changes_path(), &change)?;
        Ok(updated)
    }

    /// Rewrite pending.jsonl with the given entries (public for TUI edit flow).
    ///
    /// Also truncates `pending_changes.jsonl`, since the provided entries are
    /// assumed to be a post-fold view — any tombstones would become orphans.
    pub fn rewrite_pending_entries(&self, entries: &[LedgerEntry]) -> Result<()> {
        self.rewrite_pending(entries)
    }

    fn rewrite_pending(&self, entries: &[LedgerEntry]) -> Result<()> {
        let tmp = self.session_dir.join("pending.jsonl.tmp");
        {
            let mut f =
                File::create(&tmp).with_context(|| format!("creating tmp {}", tmp.display()))?;
            for e in entries {
                writeln!(f, "{}", serde_json::to_string(e)?)
                    .with_context(|| format!("writing tmp {}", tmp.display()))?;
            }
        }
        fs::rename(&tmp, self.pending_path()).with_context(|| {
            format!("renaming tmp -> pending {}", self.pending_path().display())
        })?;
        // rewrite_pending writes the folded view — tombstones in pending_changes.jsonl
        // are now logically empty. Remove the file so orphan tombstones don't accumulate.
        let changes = self.pending_changes_path();
        if changes.exists() {
            fs::remove_file(&changes)
                .with_context(|| format!("removing {}", changes.display()))?;
        }
        Ok(())
    }

    /// Rewrite `pending.jsonl` from the folded-and-filtered view and truncate
    /// `pending_changes.jsonl`. After this runs, `pending.jsonl` contains only
    /// the currently-pending entries, and the change-file is empty.
    pub fn compact_pending(&self) -> Result<()> {
        let entries = self.list_pending()?; // already folded and filtered to Pending
        self.rewrite_pending(&entries)?;
        // Truncate the changes file by removing it.
        let changes = self.pending_changes_path();
        if changes.exists() {
            fs::remove_file(&changes)
                .with_context(|| format!("removing {}", changes.display()))?;
        }
        Ok(())
    }

    /// Rewrite `ledger.jsonl` from the folded view and truncate
    /// `ledger_changes.jsonl`.
    pub fn compact_ledger(&self) -> Result<()> {
        let entries = self.list_ledger()?; // folded, no status filter
        let tmp = self.session_dir.join("ledger.jsonl.tmp");
        {
            let mut f =
                File::create(&tmp).with_context(|| format!("creating tmp {}", tmp.display()))?;
            for e in &entries {
                writeln!(f, "{}", serde_json::to_string(e)?)
                    .with_context(|| format!("writing tmp {}", tmp.display()))?;
            }
        }
        fs::rename(&tmp, self.ledger_path()).with_context(|| {
            format!("renaming tmp -> ledger {}", self.ledger_path().display())
        })?;
        let changes = self.ledger_changes_path();
        if changes.exists() {
            fs::remove_file(&changes)
                .with_context(|| format!("removing {}", changes.display()))?;
        }
        Ok(())
    }

    fn append_ledger(&self, entry: &LedgerEntry) -> Result<()> {
        let path = self.ledger_path();
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening ledger {}", path.display()))?;
        writeln!(f, "{}", serde_json::to_string(entry)?)
            .with_context(|| format!("writing ledger line to {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{DiffStats, Op, Tool};
    use chrono::Utc;
    use tempfile::TempDir;

    fn make_entry(id: &str, turn: u32) -> LedgerEntry {
        LedgerEntry {
            id: id.to_string(),
            turn,
            tool: Tool::Write,
            path: PathBuf::from("foo.txt"),
            op: Op::Create,
            rationale: String::new(),
            diff_stats: DiffStats {
                added: 1,
                removed: 0,
            },
            snapshot_before: None,
            snapshot_after: Some("a".repeat(40)),
            status: Status::Pending,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn append_and_list_roundtrip() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.append_pending(&make_entry("02", 2)).unwrap();
        let all = store.list_pending().unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "01");
        assert_eq!(all[1].id, "02");
    }

    #[test]
    fn malformed_lines_are_skipped_and_counted() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        // Write a bad line directly.
        let mut f = OpenOptions::new()
            .append(true)
            .open(store.pending_path())
            .unwrap();
        writeln!(f, "not valid json").unwrap();
        store.append_pending(&make_entry("02", 2)).unwrap();
        let stats = store.list_pending_with_stats().unwrap();
        assert_eq!(stats.entries.len(), 2);
        assert_eq!(stats.skipped, 1, "one malformed line should be counted");
    }

    #[test]
    fn empty_file_returns_empty_vec() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        let stats = store.list_pending_with_stats().unwrap();
        assert!(stats.entries.is_empty());
        assert_eq!(stats.skipped, 0);
    }

    #[test]
    fn list_ledger_reads_ledger_file_not_pending() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        // Write a valid entry directly to ledger.jsonl (bypassing Store).
        fs::create_dir_all(td.path()).unwrap();
        let entry = make_entry("99", 3);
        let line = serde_json::to_string(&entry).unwrap();
        fs::write(store.ledger_path(), format!("{line}\n")).unwrap();
        // Pending is empty; ledger has one entry.
        assert!(store.list_pending().unwrap().is_empty());
        let ledger = store.list_ledger().unwrap();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].id, "99");
    }

    #[test]
    fn finalize_moves_entry_from_pending_to_ledger() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.append_pending(&make_entry("02", 2)).unwrap();
        let finalized = store.finalize("01", Status::Accepted).unwrap();
        assert_eq!(finalized.status, Status::Accepted);
        let remaining = store.list_pending().unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "02");
        let ledger = store.list_ledger().unwrap();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].id, "01");
    }

    #[test]
    fn finalize_unknown_id_errors_include_id() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        let err = store
            .finalize("nonexistent-xyz", Status::Accepted)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not in pending"), "got: {msg}");
        assert!(
            msg.contains("nonexistent-xyz"),
            "error should name the id: {msg}"
        );
    }

    fn setup_revert_scenario(td: &TempDir) -> (Paths, String, PathBuf) {
        let project_root = td.path().to_path_buf();
        let paths = Paths::new(&project_root);
        let session_id = "sess-1".to_string();
        let session_dir = paths.session_dir(&session_id);
        fs::create_dir_all(session_dir.join("snapshots")).unwrap();
        (paths, session_id, project_root)
    }

    #[test]
    fn restore_snapshot_deletes_file_on_create_revert() {
        let td = TempDir::new().unwrap();
        let (paths, session_id, project_root) = setup_revert_scenario(&td);
        let session_dir = paths.session_dir(&session_id);
        let store = Store::new(&session_dir);

        let target = project_root.join("new.txt");
        fs::write(&target, b"content").unwrap();
        let mut entry = make_entry("01", 1);
        entry.op = Op::Create;
        entry.path = PathBuf::from("new.txt");
        entry.snapshot_before = None;

        store
            .restore_snapshot(&entry, &project_root, &paths, &session_id)
            .unwrap();
        assert!(!target.exists());
    }

    #[test]
    fn restore_snapshot_rewrites_file_on_modify_revert() {
        let td = TempDir::new().unwrap();
        let (paths, session_id, project_root) = setup_revert_scenario(&td);
        let session_dir = paths.session_dir(&session_id);
        let store = Store::new(&session_dir);

        // Stash the pre-modify content in the snapshot store.
        let snap_store = SnapshotStore::new(&paths, &session_id);
        let before_hash = snap_store.put(b"original contents").unwrap();

        // Claude modified the file to something else.
        let target = project_root.join("src/lib.rs");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"claude's rewrite").unwrap();

        let mut entry = make_entry("m1", 1);
        entry.op = Op::Modify;
        entry.path = PathBuf::from("src/lib.rs");
        entry.snapshot_before = Some(before_hash);

        store
            .restore_snapshot(&entry, &project_root, &paths, &session_id)
            .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"original contents");
    }

    #[test]
    fn restore_snapshot_recreates_file_on_delete_revert() {
        let td = TempDir::new().unwrap();
        let (paths, session_id, project_root) = setup_revert_scenario(&td);
        let session_dir = paths.session_dir(&session_id);
        let store = Store::new(&session_dir);

        // Pre-delete content stashed in snapshots.
        let snap_store = SnapshotStore::new(&paths, &session_id);
        let before_hash = snap_store.put(b"the file that was deleted").unwrap();

        // File does not currently exist on disk (Claude deleted it).
        let target = project_root.join("removed.txt");
        assert!(!target.exists());

        let mut entry = make_entry("d1", 1);
        entry.op = Op::Delete;
        entry.path = PathBuf::from("removed.txt");
        entry.snapshot_before = Some(before_hash);
        entry.snapshot_after = None;

        store
            .restore_snapshot(&entry, &project_root, &paths, &session_id)
            .unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"the file that was deleted");
    }

    #[test]
    fn restore_snapshot_errors_on_modify_with_no_before() {
        let td = TempDir::new().unwrap();
        let (paths, session_id, project_root) = setup_revert_scenario(&td);
        let session_dir = paths.session_dir(&session_id);
        let store = Store::new(&session_dir);

        let mut entry = make_entry("bad", 1);
        entry.op = Op::Modify;
        entry.snapshot_before = None;

        let err = store
            .restore_snapshot(&entry, &project_root, &paths, &session_id)
            .unwrap_err();
        assert!(err.to_string().contains("has no snapshot_before"));
    }

    #[test]
    fn finalize_appends_instead_of_rewriting() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.append_pending(&make_entry("02", 2)).unwrap();
        store.append_pending(&make_entry("03", 3)).unwrap();

        store.finalize("02", Status::Accepted).unwrap();

        // pending.jsonl still has 3 lines (bare entries, unchanged).
        let raw = fs::read_to_string(store.pending_path()).unwrap();
        assert_eq!(raw.lines().count(), 3);

        // pending_changes.jsonl has 1 line (the tombstone).
        let changes_raw = fs::read_to_string(store.pending_changes_path()).unwrap();
        assert_eq!(changes_raw.lines().count(), 1);

        // list_pending filters out finalized entries.
        let pending = store.list_pending().unwrap();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|e| e.id != "02"));
    }

    #[test]
    fn update_ledger_status_appends_to_changes() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.finalize("01", Status::Accepted).unwrap();
        // ledger now has the entry with Accepted; change by prefix to Reverted.
        store.update_ledger_status("01", Status::Reverted).unwrap();

        // ledger.jsonl unchanged shape — just one entry.
        let raw = fs::read_to_string(store.ledger_path()).unwrap();
        assert_eq!(raw.lines().count(), 1);

        // ledger_changes.jsonl has one change.
        let changes_raw = fs::read_to_string(store.ledger_changes_path()).unwrap();
        assert_eq!(changes_raw.lines().count(), 1);

        let ledger = store.list_ledger().unwrap();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].status, Status::Reverted);
    }

    #[test]
    fn multiple_status_changes_last_write_wins() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.finalize("01", Status::Accepted).unwrap();
        store.update_ledger_status("01", Status::Reverted).unwrap();
        store.update_ledger_status("01", Status::Edited).unwrap();

        let ledger = store.list_ledger().unwrap();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].status, Status::Edited);
    }

    #[test]
    fn orphan_change_for_nonexistent_id_is_ignored() {
        // A change file that references an id not in the entries file must
        // be silently tolerated — this happens transiently during concurrent
        // reads/writes and after certain crash scenarios.
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();

        // Manually inject an orphan change for an unknown id.
        let orphan = StatusChange {
            id: "nonexistent-99".to_string(),
            new_status: Status::Accepted,
            timestamp: Utc::now(),
        };
        fs::create_dir_all(td.path()).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(store.pending_changes_path())
            .unwrap();
        writeln!(f, "{}", serde_json::to_string(&orphan).unwrap()).unwrap();

        // list_pending still returns the real entry untouched.
        let pending = store.list_pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "01");
        assert_eq!(pending[0].status, Status::Pending);
    }

    #[test]
    fn edit_flow_preserves_existing_tombstones() {
        // Regression: the TUI edit flow calls `rewrite_pending_entries` with
        // a post-fold view. Verify that after a rewrite on a DIFFERENT entry,
        // a previously-finalized entry stays excluded from `list_pending`.
        //
        // Note: rewrite_pending_entries also truncates pending_changes.jsonl,
        // because the post-fold view has no tombstones in it — so the finalize
        // tombstone is gone after the rewrite, but the entry is also not in
        // pending.jsonl anymore, so `list_pending` still excludes it.
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.append_pending(&make_entry("02", 1)).unwrap();

        // Finalize 01.
        store.finalize("01", Status::Accepted).unwrap();

        // Simulate TUI edit flow on 02: read, modify, rewrite.
        let mut pending = store.list_pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "02");
        if let Some(e) = pending.iter_mut().find(|e| e.id == "02") {
            e.snapshot_after = Some("b".repeat(40));
        }
        store.rewrite_pending_entries(&pending).unwrap();

        // After the rewrite, list_pending should still correctly exclude 01.
        let pending = store.list_pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "02");
    }

    #[test]
    fn rewrite_pending_truncates_change_file() {
        // rewrite_pending_entries must also clear pending_changes.jsonl, otherwise
        // orphan tombstones accumulate after every TUI edit flow.
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.append_pending(&make_entry("02", 2)).unwrap();
        store.finalize("01", Status::Accepted).unwrap();

        // Sanity: pending_changes.jsonl exists.
        assert!(store.pending_changes_path().exists());

        // Simulate a rewrite from the folded view (as TUI edit flow does).
        let pending = store.list_pending().unwrap();
        store.rewrite_pending_entries(&pending).unwrap();

        // The change file must be gone.
        assert!(
            !store.pending_changes_path().exists(),
            "rewrite_pending must truncate pending_changes.jsonl"
        );

        // And list_pending still returns only the pending entry.
        let pending = store.list_pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, "02");
    }

    #[test]
    fn compact_pending_removes_tombstones_and_finalized_entries() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.append_pending(&make_entry("02", 2)).unwrap();
        store.append_pending(&make_entry("03", 3)).unwrap();
        store.finalize("02", Status::Accepted).unwrap();

        // Before compact: 3 entries + 1 tombstone.
        assert_eq!(
            fs::read_to_string(store.pending_path())
                .unwrap()
                .lines()
                .count(),
            3
        );
        assert_eq!(
            fs::read_to_string(store.pending_changes_path())
                .unwrap()
                .lines()
                .count(),
            1
        );

        store.compact_pending().unwrap();

        // After compact: 2 entries, no changes file.
        assert_eq!(
            fs::read_to_string(store.pending_path())
                .unwrap()
                .lines()
                .count(),
            2
        );
        assert!(!store.pending_changes_path().exists());

        // list_pending still returns the same 2 entries.
        let pending = store.list_pending().unwrap();
        assert_eq!(pending.len(), 2);
        let ids: Vec<&str> = pending.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"01"));
        assert!(ids.contains(&"03"));
    }

    #[test]
    fn compact_ledger_folds_status_changes() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.finalize("01", Status::Accepted).unwrap();
        store.update_ledger_status("01", Status::Reverted).unwrap();

        // Before compact: 1 entry + 1 change.
        assert_eq!(
            fs::read_to_string(store.ledger_path())
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert_eq!(
            fs::read_to_string(store.ledger_changes_path())
                .unwrap()
                .lines()
                .count(),
            1
        );

        store.compact_ledger().unwrap();

        // After compact: 1 entry with the folded status, no changes file.
        assert_eq!(
            fs::read_to_string(store.ledger_path())
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert!(!store.ledger_changes_path().exists());

        let ledger = store.list_ledger().unwrap();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].status, Status::Reverted);
    }

    #[test]
    fn compact_idempotent_on_empty_session() {
        // Compact when there are no pending/ledger entries at all. Must not error.
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        fs::create_dir_all(td.path()).unwrap();
        store.compact_pending().unwrap();
        store.compact_ledger().unwrap();
    }

    #[test]
    fn finalize_preserves_order_of_remaining_entries() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        store.append_pending(&make_entry("02", 2)).unwrap();
        store.append_pending(&make_entry("03", 3)).unwrap();
        // Finalize the middle one.
        store.finalize("02", Status::Accepted).unwrap();
        let remaining = store.list_pending().unwrap();
        assert_eq!(remaining.len(), 2);
        assert_eq!(remaining[0].id, "01");
        assert_eq!(remaining[1].id, "03");
    }
}
