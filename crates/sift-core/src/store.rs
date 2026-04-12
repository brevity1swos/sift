//! The ledger store: append-only pending.jsonl + finalized ledger.jsonl.

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::entry::{LedgerEntry, Status};

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
        Self { session_dir: session_dir.into() }
    }

    pub fn pending_path(&self) -> PathBuf { self.session_dir.join("pending.jsonl") }
    pub fn ledger_path(&self) -> PathBuf { self.session_dir.join("ledger.jsonl") }

    /// Append a pending entry to `pending.jsonl`.
    pub fn append_pending(&self, entry: &LedgerEntry) -> Result<()> {
        fs::create_dir_all(&self.session_dir)
            .with_context(|| format!("creating session dir {}", self.session_dir.display()))?;
        let line = serde_json::to_string(entry)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.pending_path())
            .with_context(|| format!("opening pending {}", self.pending_path().display()))?;
        writeln!(f, "{line}")
            .with_context(|| format!("writing pending line to {}", self.pending_path().display()))?;
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
                return Ok(ReadStats { entries: vec![], skipped: 0 });
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
        Ok(self.list_pending()?.into_iter().filter(|e| e.status == status).collect())
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
            diff_stats: DiffStats { added: 1, removed: 0 },
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
        let mut f = OpenOptions::new().append(true).open(store.pending_path()).unwrap();
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
}
