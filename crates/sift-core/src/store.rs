//! The ledger store: append-only pending.jsonl + finalized ledger.jsonl.

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::entry::{LedgerEntry, Op, Status};
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
    pub fn list_pending(&self) -> Result<Vec<LedgerEntry>> {
        Ok(self.list_pending_with_stats()?.entries)
    }

    /// Read all finalized entries, silently skipping malformed lines.
    pub fn list_ledger(&self) -> Result<Vec<LedgerEntry>> {
        Ok(self.list_ledger_with_stats()?.entries)
    }

    /// Read pending entries and return a `ReadStats` including the count
    /// of lines that failed to parse. Useful for `sift fsck` and for tests
    /// that need to assert on the skip count.
    pub fn list_pending_with_stats(&self) -> Result<ReadStats> {
        Self::read_jsonl(&self.pending_path())
    }

    /// Read finalized entries and return a `ReadStats`.
    pub fn list_ledger_with_stats(&self) -> Result<ReadStats> {
        Self::read_jsonl(&self.ledger_path())
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

    /// Filter pending entries by status (convenience).
    pub fn pending_with_status(&self, status: Status) -> Result<Vec<LedgerEntry>> {
        Ok(self
            .list_pending()?
            .into_iter()
            .filter(|e| e.status == status)
            .collect())
    }

    /// Move an entry from pending.jsonl to ledger.jsonl with the given final status.
    /// Returns the entry as written, or Err if the id is not in pending.
    ///
    /// **Concurrency invariant:** only one writer per session dir at a time.
    /// Two concurrent `finalize` calls would both write `pending.jsonl.tmp` and
    /// race on rename, silently dropping one caller's mutation.
    pub fn finalize(&self, id: &str, new_status: Status) -> Result<LedgerEntry> {
        let pending = self.list_pending()?;
        let (keep, take): (Vec<_>, Vec<_>) = pending.into_iter().partition(|e| e.id != id);
        debug_assert!(
            take.len() <= 1,
            "duplicate entry id {id} in pending — ledger invariant violated",
        );
        let mut entry = take
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("entry {id} not in pending"))?;
        entry.status = new_status;
        // Append to ledger FIRST so a subsequent rewrite_pending failure leaves
        // the entry duplicated (present in both files) rather than lost. A
        // future fsck/dedup pass can resolve duplicates; a vanished entry
        // cannot be recovered.
        self.append_ledger(&entry)?;
        self.rewrite_pending(&keep)?;
        Ok(entry)
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

    /// Update an entry's status in ledger.jsonl (for reverting accepted entries).
    /// Returns the entry with its new status, or Err if not found.
    pub fn update_ledger_status(&self, id: &str, new_status: Status) -> Result<LedgerEntry> {
        let mut ledger = self.list_ledger()?;
        let entry = ledger
            .iter_mut()
            .find(|e| e.id.starts_with(id))
            .ok_or_else(|| anyhow::anyhow!("entry {id} not in ledger"))?;
        entry.status = new_status;
        let result = entry.clone();
        self.rewrite_ledger(&ledger)?;
        Ok(result)
    }

    fn rewrite_ledger(&self, entries: &[LedgerEntry]) -> Result<()> {
        let tmp = self.session_dir.join("ledger.jsonl.tmp");
        {
            let mut f =
                File::create(&tmp).with_context(|| format!("creating tmp {}", tmp.display()))?;
            for e in entries {
                writeln!(f, "{}", serde_json::to_string(e)?)
                    .with_context(|| format!("writing tmp {}", tmp.display()))?;
            }
        }
        fs::rename(&tmp, self.ledger_path()).with_context(|| {
            format!("renaming tmp -> ledger {}", self.ledger_path().display())
        })?;
        Ok(())
    }

    /// Rewrite pending.jsonl with the given entries (public for TUI edit flow).
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
    fn pending_with_status_filters_correctly() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        let pending_entry = make_entry("p1", 1);
        let mut accepted_entry = make_entry("a1", 2);
        accepted_entry.status = Status::Accepted;
        store.append_pending(&pending_entry).unwrap();
        store.append_pending(&accepted_entry).unwrap();
        let pending_only = store.pending_with_status(Status::Pending).unwrap();
        assert_eq!(pending_only.len(), 1);
        assert_eq!(pending_only[0].id, "p1");
        let accepted_only = store.pending_with_status(Status::Accepted).unwrap();
        assert_eq!(accepted_only.len(), 1);
        assert_eq!(accepted_only[0].id, "a1");
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
