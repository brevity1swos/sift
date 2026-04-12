//! The ledger store: append-only pending.jsonl + finalized ledger.jsonl.

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::entry::{LedgerEntry, Status};

pub struct Store {
    session_dir: PathBuf,
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
            .with_context(|| format!("creating session dir {:?}", self.session_dir))?;
        let line = serde_json::to_string(entry)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.pending_path())
            .with_context(|| format!("opening pending {:?}", self.pending_path()))?;
        writeln!(f, "{line}")
            .with_context(|| format!("writing pending line {:?}", self.pending_path()))?;
        Ok(())
    }

    /// Read all pending entries, skipping malformed lines.
    pub fn list_pending(&self) -> Result<Vec<LedgerEntry>> {
        Self::read_jsonl(&self.pending_path())
    }

    /// Read all finalized entries.
    pub fn list_ledger(&self) -> Result<Vec<LedgerEntry>> {
        Self::read_jsonl(&self.ledger_path())
    }

    fn read_jsonl(path: &Path) -> Result<Vec<LedgerEntry>> {
        if !path.exists() {
            return Ok(vec![]);
        }
        let f = File::open(path)
            .with_context(|| format!("opening {:?}", path))?;
        let reader = BufReader::new(f);
        let mut out = Vec::new();
        for line in reader.lines() {
            let line = line.with_context(|| format!("reading line from {:?}", path))?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<LedgerEntry>(&line) {
                Ok(e) => out.push(e),
                Err(_) => {
                    // Silently skip malformed lines; hook crash may leave partial writes.
                    continue;
                }
            }
        }
        Ok(out)
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
    fn malformed_lines_are_skipped() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        store.append_pending(&make_entry("01", 1)).unwrap();
        // Write a bad line directly.
        let mut f = OpenOptions::new().append(true).open(store.pending_path()).unwrap();
        writeln!(f, "not valid json").unwrap();
        store.append_pending(&make_entry("02", 2)).unwrap();
        let all = store.list_pending().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn empty_file_returns_empty_vec() {
        let td = TempDir::new().unwrap();
        let store = Store::new(td.path());
        assert!(store.list_pending().unwrap().is_empty());
    }
}
