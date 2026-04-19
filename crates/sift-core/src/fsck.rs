//! `sift fsck`: byte-granular validation and repair for ledger JSONL files.
//!
//! The existing `store::read_jsonl` uses `BufReader::lines()`, which swallows
//! trailing non-newline fragments — a hook crash mid-write merges the partial
//! record with the next valid one, costing one entry silently. fsck parses
//! byte-for-byte with `read_until(b'\n')` so the truncated tail is reported
//! explicitly, then optionally writes a cleaned file and archives the original.
//!
//! Four issue classes are surfaced:
//!
//! - **TruncatedTail** — last record has no trailing `\n`, indicating a hook
//!   crash mid-write. The partial bytes are preserved in the `.bad.<ulid>`
//!   archive on repair.
//! - **InvalidJson** — record parses as a line but not as the expected JSON
//!   type. Rare in practice; usually filesystem corruption.
//! - **DuplicateId** — same `id` appears more than once. The
//!   `append_ledger` → `append_change` sequence in `store::finalize` has a
//!   documented crash window that can produce this. Repair keeps the first
//!   occurrence.
//! - **OrphanTombstone** — a `pending_changes.jsonl` or `ledger_changes.jsonl`
//!   row references an `id` that does not exist in the paired entries file.
//!   Repair drops the orphan from the cleaned changes file.

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use crate::entry::{LedgerEntry, StatusChange};
use crate::paths::Paths;
use crate::session::SessionMeta;

/// Which of sift's four JSONL files a record came from. Stringly-typed with a
/// stable `Display` impl so reports and test assertions read cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileKind {
    Pending,
    Ledger,
    PendingChanges,
    LedgerChanges,
}

impl FileKind {
    pub fn filename(self) -> &'static str {
        match self {
            FileKind::Pending => "pending.jsonl",
            FileKind::Ledger => "ledger.jsonl",
            FileKind::PendingChanges => "pending_changes.jsonl",
            FileKind::LedgerChanges => "ledger_changes.jsonl",
        }
    }

    /// All four JSONL files sift manages per session. Ordered so entry
    /// files come first and their paired change files follow — the order
    /// `check_session` walks and the order tests assert on.
    pub fn all() -> [FileKind; 4] {
        [
            FileKind::Pending,
            FileKind::Ledger,
            FileKind::PendingChanges,
            FileKind::LedgerChanges,
        ]
    }
}

impl std::fmt::Display for FileKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.filename())
    }
}

/// A single issue found during validation. Offsets are byte offsets into the
/// file from 0, pointing at the start of the affected record.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Issue {
    /// Last record in the file had no trailing `\n`. The partial bytes
    /// from `offset` to EOF are recoverable only from the `.bad.<ulid>`
    /// archive post-repair.
    TruncatedTail {
        file: FileKind,
        offset: u64,
        byte_length: u64,
    },
    /// Record parsed as a UTF-8 line but not as the expected JSON type.
    InvalidJson {
        file: FileKind,
        offset: u64,
        err: String,
    },
    /// Same `id` appears multiple times in `pending.jsonl` / `ledger.jsonl`.
    /// Offsets of all occurrences are preserved so forensic inspection is
    /// possible from the report alone.
    DuplicateId {
        file: FileKind,
        id: String,
        offsets: Vec<u64>,
    },
    /// A status-change row references an id that has no matching entry
    /// in the paired `*.jsonl` file.
    OrphanTombstone {
        file: FileKind,
        id: String,
        offset: u64,
    },
}

/// Report of all issues found across a session's JSONL files.
#[derive(Debug, Clone, Serialize, Default)]
pub struct FsckReport {
    pub session_id: String,
    pub issues: Vec<Issue>,
}

impl FsckReport {
    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Validate one session's JSONL files. Never mutates; safe to run on a
/// currently-open session even though repair is not.
pub fn check_session(paths: &Paths, session_id: &str) -> Result<FsckReport> {
    let session_dir = paths.session_dir(session_id);
    let mut report = FsckReport {
        session_id: session_id.to_string(),
        issues: Vec::new(),
    };

    // Entries files: check for truncated tail, invalid JSON, duplicate ids.
    let mut entry_ids: HashMap<FileKind, HashSet<String>> = HashMap::new();
    for kind in [FileKind::Pending, FileKind::Ledger] {
        let path = session_dir.join(kind.filename());
        let records = parse_records::<LedgerEntry>(&path, kind, &mut report.issues)?;
        let mut seen: HashMap<String, Vec<u64>> = HashMap::new();
        let mut ids = HashSet::new();
        for (offset, entry) in &records {
            seen.entry(entry.id.clone()).or_default().push(*offset);
            ids.insert(entry.id.clone());
        }
        for (id, offsets) in seen {
            if offsets.len() > 1 {
                report.issues.push(Issue::DuplicateId {
                    file: kind,
                    id,
                    offsets,
                });
            }
        }
        entry_ids.insert(kind, ids);
    }

    // Changes files: truncated tail, invalid JSON, orphan tombstones.
    for (changes_kind, paired) in [
        (FileKind::PendingChanges, FileKind::Pending),
        (FileKind::LedgerChanges, FileKind::Ledger),
    ] {
        let path = session_dir.join(changes_kind.filename());
        let records = parse_records::<StatusChange>(&path, changes_kind, &mut report.issues)?;
        let empty = HashSet::new();
        let known = entry_ids.get(&paired).unwrap_or(&empty);
        for (offset, change) in records {
            if !known.contains(&change.id) {
                report.issues.push(Issue::OrphanTombstone {
                    file: changes_kind,
                    id: change.id,
                    offset,
                });
            }
        }
    }

    Ok(report)
}

/// Result of a repair pass: which issues were fixed, which files were
/// rewritten, and where the original bytes were archived.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RepairReport {
    pub session_id: String,
    /// Issues that existed before repair. Present so callers can display
    /// the full pre-repair state without running `check_session` twice.
    pub issues: Vec<Issue>,
    /// One entry per file that was rewritten during repair.
    pub rewrites: Vec<Rewrite>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Rewrite {
    pub file: FileKind,
    pub bad_archive: PathBuf,
    pub records_kept: usize,
    pub records_dropped: usize,
}

/// Repair a session's JSONL files. Refuses to touch an open session — an
/// active hook could race with our rewrite and leave inconsistent state.
///
/// Strategy per file:
/// 1. Parse byte-granular, collect valid records.
/// 2. For entry files (pending/ledger): dedupe by id, keep first.
/// 3. For change files: drop rows referencing unknown ids.
/// 4. Rename original to `<name>.bad.<ulid>`.
/// 5. Write cleaned records atomically via temp-file + rename.
pub fn repair_session(paths: &Paths, session_id: &str) -> Result<RepairReport> {
    let session_dir = paths.session_dir(session_id);
    ensure_session_closed(&session_dir)
        .context("fsck repair refuses to touch an open session")?;

    let report = check_session(paths, session_id)?;
    let mut out = RepairReport {
        session_id: session_id.to_string(),
        issues: report.issues,
        rewrites: Vec::new(),
    };

    // Collect the valid id set from entries files first so we can filter
    // orphan tombstones out of the change files in the second pass.
    let mut entry_ids: HashMap<FileKind, HashSet<String>> = HashMap::new();
    for kind in [FileKind::Pending, FileKind::Ledger] {
        let path = session_dir.join(kind.filename());
        let mut discard = Vec::new();
        let records = parse_records::<LedgerEntry>(&path, kind, &mut discard)?;
        if !path.exists() {
            entry_ids.insert(kind, HashSet::new());
            continue;
        }

        let parsed_count = records.len();
        let mut seen = HashSet::new();
        let mut deduped: Vec<LedgerEntry> = Vec::new();
        for (_, entry) in records {
            if seen.insert(entry.id.clone()) {
                deduped.push(entry);
            }
        }
        let ids: HashSet<String> = deduped.iter().map(|e| e.id.clone()).collect();

        // Rewrite only if (a) the file had issues or (b) we dropped duplicates.
        let had_issues = out.issues.iter().any(|i| issue_file(i) == Some(kind));
        if had_issues {
            let dropped = parsed_count.saturating_sub(deduped.len());
            let rewrite = rewrite_file(&path, kind, &deduped, dropped)?;
            out.rewrites.push(rewrite);
        }
        entry_ids.insert(kind, ids);
    }

    for (changes_kind, paired) in [
        (FileKind::PendingChanges, FileKind::Pending),
        (FileKind::LedgerChanges, FileKind::Ledger),
    ] {
        let path = session_dir.join(changes_kind.filename());
        if !path.exists() {
            continue;
        }
        let mut discard = Vec::new();
        let records = parse_records::<StatusChange>(&path, changes_kind, &mut discard)?;
        let parsed_count = records.len();
        let empty = HashSet::new();
        let known = entry_ids.get(&paired).unwrap_or(&empty);
        let kept: Vec<StatusChange> = records
            .into_iter()
            .map(|(_, c)| c)
            .filter(|c| known.contains(&c.id))
            .collect();

        let had_issues = out.issues.iter().any(|i| issue_file(i) == Some(changes_kind));
        if had_issues {
            let dropped = parsed_count.saturating_sub(kept.len());
            let rewrite = rewrite_file(&path, changes_kind, &kept, dropped)?;
            out.rewrites.push(rewrite);
        }
    }

    Ok(out)
}

fn issue_file(issue: &Issue) -> Option<FileKind> {
    Some(match issue {
        Issue::TruncatedTail { file, .. }
        | Issue::InvalidJson { file, .. }
        | Issue::DuplicateId { file, .. }
        | Issue::OrphanTombstone { file, .. } => *file,
    })
}

/// Confirm that `session/meta.json` has an `ended_at`. Refuses repair if the
/// session is still active — a concurrent hook fire would race us.
fn ensure_session_closed(session_dir: &Path) -> Result<()> {
    let meta_path = session_dir.join("meta.json");
    let text = fs::read_to_string(&meta_path)
        .with_context(|| format!("reading {}", meta_path.display()))?;
    let meta: SessionMeta =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", meta_path.display()))?;
    anyhow::ensure!(
        meta.ended_at.is_some(),
        "session {} is still open — close it (hit Stop) before running repair",
        meta.id
    );
    Ok(())
}

/// Atomically replace `path` with a serialization of `records`, archiving
/// the original to `<filename>.bad.<ulid>` first. `dropped` is the count
/// the caller computed (parsed - kept) and gets surfaced in the
/// user-facing report so "lost N records" is visible at a glance instead
/// of buried in the .bad archive.
fn rewrite_file<T: serde::Serialize>(
    path: &Path,
    kind: FileKind,
    records: &[T],
    dropped: usize,
) -> Result<Rewrite> {
    use std::io::Write;

    // Archive the original first so forensic inspection is always possible
    // even if the write below fails.
    let archive = path.with_file_name(format!(
        "{}.bad.{}",
        kind.filename(),
        ulid::Ulid::new()
    ));
    fs::rename(path, &archive).with_context(|| {
        format!(
            "archiving {} to {}",
            path.display(),
            archive.display()
        )
    })?;

    // Atomic write: temp file in the same dir, then rename over the final path.
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("no parent for {}", path.display()))?;
    let tmp = parent.join(format!(".{}.tmp.{}", kind.filename(), ulid::Ulid::new()));
    {
        let mut f = File::create(&tmp)
            .with_context(|| format!("creating temp file {}", tmp.display()))?;
        let mut kept = 0usize;
        for record in records {
            let line = serde_json::to_string(record)
                .with_context(|| "re-serializing record during repair")?;
            f.write_all(line.as_bytes())
                .with_context(|| format!("writing to {}", tmp.display()))?;
            f.write_all(b"\n")?;
            kept += 1;
        }
        f.sync_all().ok(); // best-effort durability
        drop(f);

        fs::rename(&tmp, path)
            .with_context(|| format!("renaming {} to {}", tmp.display(), path.display()))?;

        Ok(Rewrite {
            file: kind,
            bad_archive: archive,
            records_kept: kept,
            records_dropped: dropped,
        })
    }
}

/// Parse records from a JSONL file byte-granular, populating `issues` with
/// every problem encountered. Returns `(offset, record)` tuples for records
/// that parsed cleanly.
///
/// Returns an empty vec if the file does not exist (caller does not need to
/// pre-check — a missing pending.jsonl is a legal state for a fresh session).
fn parse_records<T>(path: &Path, kind: FileKind, issues: &mut Vec<Issue>) -> Result<Vec<(u64, T)>>
where
    T: serde::de::DeserializeOwned,
{
    let f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(e).with_context(|| format!("opening {}", path.display()));
        }
    };
    let mut reader = BufReader::new(f);
    let mut buf = Vec::new();
    let mut offset: u64 = 0;
    let mut out = Vec::new();

    loop {
        buf.clear();
        let bytes = read_until_newline(&mut reader, &mut buf)?;
        if bytes == 0 {
            break;
        }

        // Distinguish a clean-terminated record from a truncated tail. A clean
        // record ends with '\n'; a truncated tail reaches EOF without one.
        let has_newline = buf.last() == Some(&b'\n');
        let record_bytes = if has_newline {
            &buf[..buf.len() - 1]
        } else {
            &buf[..]
        };

        // Empty-line tolerance: skip blank lines silently.
        if record_bytes.iter().all(|b| b.is_ascii_whitespace()) {
            offset += bytes as u64;
            continue;
        }

        if !has_newline {
            issues.push(Issue::TruncatedTail {
                file: kind,
                offset,
                byte_length: bytes as u64,
            });
            // Truncated tail is always the last record in the file — stop.
            break;
        }

        // Try to parse as T.
        match serde_json::from_slice::<T>(record_bytes) {
            Ok(record) => out.push((offset, record)),
            Err(err) => issues.push(Issue::InvalidJson {
                file: kind,
                offset,
                err: err.to_string(),
            }),
        }

        offset += bytes as u64;
    }

    Ok(out)
}

/// Maximum bytes per ledger record. Real entries are well under 8 KB
/// (ULID + a few path strings + small JSON metadata). 1 MB is generous
/// for any plausible legitimate record, while still bounding memory if a
/// corrupt or pathological file omits newlines for gigabytes.
const MAX_RECORD_BYTES: usize = 1024 * 1024;

/// Read bytes up to and including the next `\n` into `buf`. On EOF with
/// partial data, returns what was read — caller checks the last byte to
/// distinguish "clean line" from "truncated tail."
///
/// Returns an error if a single record exceeds `MAX_RECORD_BYTES`,
/// preventing OOM on a corrupt file with no newlines.
fn read_until_newline<R: Read>(reader: &mut R, buf: &mut Vec<u8>) -> Result<usize> {
    let mut byte = [0u8; 1];
    let mut count = 0;
    loop {
        match reader.read(&mut byte) {
            Ok(0) => return Ok(count),
            Ok(_) => {
                buf.push(byte[0]);
                count += 1;
                if byte[0] == b'\n' {
                    return Ok(count);
                }
                if count > MAX_RECORD_BYTES {
                    anyhow::bail!(
                        "ledger record exceeded {} bytes without a newline — refusing to read further (file may be corrupt or adversarial)",
                        MAX_RECORD_BYTES
                    );
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{DiffStats, LedgerEntry, Op, Status, StatusChange, Tool};
    use crate::session::SessionMeta;
    use chrono::{TimeZone, Utc};
    use std::io::Write;
    use tempfile::TempDir;

    fn sample_entry(id: &str) -> LedgerEntry {
        LedgerEntry {
            id: id.to_string(),
            turn: 1,
            tool: Tool::Write,
            path: "src/a.rs".into(),
            op: Op::Create,
            rationale: String::new(),
            diff_stats: DiffStats {
                added: 1,
                removed: 0,
            },
            snapshot_before: None,
            snapshot_after: Some("a".repeat(40)),
            status: Status::Pending,
            timestamp: Utc.with_ymd_and_hms(2026, 4, 19, 12, 0, 0).unwrap(),
        }
    }

    fn sample_change(id: &str, status: Status) -> StatusChange {
        StatusChange {
            id: id.to_string(),
            new_status: status,
            timestamp: Utc.with_ymd_and_hms(2026, 4, 19, 12, 1, 0).unwrap(),
        }
    }

    fn make_session(td: &TempDir, id: &str, open: bool) -> (Paths, std::path::PathBuf) {
        let paths = Paths::new(td.path());
        let dir = paths.session_dir(id);
        fs::create_dir_all(dir.join("snapshots")).unwrap();
        let meta = SessionMeta {
            id: id.to_string(),
            project: "test".into(),
            cwd: "/tmp".into(),
            started_at: Utc.with_ymd_and_hms(2026, 4, 19, 12, 0, 0).unwrap(),
            ended_at: if open {
                None
            } else {
                Some(Utc.with_ymd_and_hms(2026, 4, 19, 12, 30, 0).unwrap())
            },
            transcript_path: None,
        };
        fs::write(dir.join("meta.json"), serde_json::to_string(&meta).unwrap()).unwrap();
        (paths, dir)
    }

    fn write_jsonl<T: serde::Serialize>(path: &Path, records: &[T]) {
        let mut f = File::create(path).unwrap();
        for r in records {
            f.write_all(serde_json::to_string(r).unwrap().as_bytes())
                .unwrap();
            f.write_all(b"\n").unwrap();
        }
    }

    #[test]
    fn clean_session_reports_no_issues() {
        let td = TempDir::new().unwrap();
        let (paths, dir) = make_session(&td, "clean-1", false);
        write_jsonl(
            &dir.join("pending.jsonl"),
            &[sample_entry("01HVXK5QZ9G7B2A00000000001")],
        );
        let report = check_session(&paths, "clean-1").unwrap();
        assert!(report.is_clean(), "expected clean, got {:?}", report.issues);
    }

    #[test]
    fn detects_truncated_trailing_record() {
        let td = TempDir::new().unwrap();
        let (paths, dir) = make_session(&td, "trunc-1", false);
        let pending = dir.join("pending.jsonl");
        {
            let mut f = File::create(&pending).unwrap();
            // First record — complete (ends in \n).
            let line1 = serde_json::to_string(&sample_entry("01HVXK5QZ9G7B2A00000000001")).unwrap();
            f.write_all(line1.as_bytes()).unwrap();
            f.write_all(b"\n").unwrap();
            // Second record — truncated (no \n). Simulates a hook crash
            // mid-append.
            f.write_all(b"{\"id\":\"partial\"").unwrap();
        }
        let report = check_session(&paths, "trunc-1").unwrap();
        assert_eq!(report.issues.len(), 1);
        match &report.issues[0] {
            Issue::TruncatedTail {
                file, byte_length, ..
            } => {
                assert_eq!(*file, FileKind::Pending);
                assert_eq!(*byte_length, 15); // b"{\"id\":\"partial\"" = 15 bytes
            }
            other => panic!("expected TruncatedTail, got {other:?}"),
        }
    }

    #[test]
    fn detects_duplicate_ids() {
        let td = TempDir::new().unwrap();
        let (paths, dir) = make_session(&td, "dup-1", false);
        let dup_id = "01HVXK5QZ9G7B2A00000000001";
        write_jsonl(
            &dir.join("pending.jsonl"),
            &[sample_entry(dup_id), sample_entry(dup_id)],
        );
        let report = check_session(&paths, "dup-1").unwrap();
        let dups: Vec<_> = report
            .issues
            .iter()
            .filter(|i| matches!(i, Issue::DuplicateId { .. }))
            .collect();
        assert_eq!(dups.len(), 1);
        if let Issue::DuplicateId { id, offsets, .. } = &dups[0] {
            assert_eq!(id, dup_id);
            assert_eq!(offsets.len(), 2);
        }
    }

    #[test]
    fn detects_orphan_tombstone_in_changes_jsonl() {
        let td = TempDir::new().unwrap();
        let (paths, dir) = make_session(&td, "orphan-1", false);
        write_jsonl(
            &dir.join("pending.jsonl"),
            &[sample_entry("01HVXK5QZ9G7B2A00000000001")],
        );
        // A change for an id that doesn't exist in pending.jsonl.
        write_jsonl(
            &dir.join("pending_changes.jsonl"),
            &[sample_change("01HVXK5QZ9G7B2A00000000999", Status::Accepted)],
        );
        let report = check_session(&paths, "orphan-1").unwrap();
        let orphans: Vec<_> = report
            .issues
            .iter()
            .filter(|i| matches!(i, Issue::OrphanTombstone { .. }))
            .collect();
        assert_eq!(orphans.len(), 1);
    }

    #[test]
    fn detects_invalid_json_mid_file() {
        let td = TempDir::new().unwrap();
        let (paths, dir) = make_session(&td, "invalid-1", false);
        let pending = dir.join("pending.jsonl");
        {
            let mut f = File::create(&pending).unwrap();
            // Good record.
            let good = serde_json::to_string(&sample_entry("01HVXK5QZ9G7B2A00000000001")).unwrap();
            f.write_all(good.as_bytes()).unwrap();
            f.write_all(b"\n").unwrap();
            // Junk line — terminated by \n so not truncated, but not valid JSON.
            f.write_all(b"NOT JSON\n").unwrap();
            // Another good record.
            let good2 =
                serde_json::to_string(&sample_entry("01HVXK5QZ9G7B2A00000000002")).unwrap();
            f.write_all(good2.as_bytes()).unwrap();
            f.write_all(b"\n").unwrap();
        }
        let report = check_session(&paths, "invalid-1").unwrap();
        let invalids: Vec<_> = report
            .issues
            .iter()
            .filter(|i| matches!(i, Issue::InvalidJson { .. }))
            .collect();
        assert_eq!(
            invalids.len(),
            1,
            "expected one invalid-json issue, got {:?}",
            report.issues
        );
    }

    #[test]
    fn repair_refuses_to_touch_open_session() {
        let td = TempDir::new().unwrap();
        let (paths, dir) = make_session(&td, "open-1", true);
        write_jsonl(
            &dir.join("pending.jsonl"),
            &[sample_entry("01HVXK5QZ9G7B2A00000000001")],
        );
        let err = repair_session(&paths, "open-1").unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("still open") || msg.contains("open session"),
            "expected open-session error, got: {msg}"
        );
    }

    #[test]
    fn repair_archives_bad_file_and_rewrites_clean() {
        let td = TempDir::new().unwrap();
        let (paths, dir) = make_session(&td, "repair-1", false);
        let pending = dir.join("pending.jsonl");
        let dup_id = "01HVXK5QZ9G7B2A00000000001";
        {
            let mut f = File::create(&pending).unwrap();
            // Two records with the same id (duplicate).
            let rec = serde_json::to_string(&sample_entry(dup_id)).unwrap();
            f.write_all(rec.as_bytes()).unwrap();
            f.write_all(b"\n").unwrap();
            f.write_all(rec.as_bytes()).unwrap();
            f.write_all(b"\n").unwrap();
            // And a truncated tail.
            f.write_all(b"{partial").unwrap();
        }

        let rep = repair_session(&paths, "repair-1").unwrap();
        assert_eq!(rep.rewrites.len(), 1);
        assert_eq!(rep.rewrites[0].file, FileKind::Pending);
        assert_eq!(rep.rewrites[0].records_kept, 1); // deduped to one
        assert_eq!(
            rep.rewrites[0].records_dropped, 1,
            "duplicate id collapsed → 1 record dropped"
        );

        // Archive file exists and starts with the original filename.
        let archive = &rep.rewrites[0].bad_archive;
        assert!(archive.exists(), "archive {archive:?} should exist");
        let archive_name = archive.file_name().unwrap().to_string_lossy();
        assert!(archive_name.starts_with("pending.jsonl.bad."));

        // Repaired pending.jsonl has exactly one record + trailing newline.
        let repaired = fs::read_to_string(&pending).unwrap();
        assert_eq!(
            repaired.lines().count(),
            1,
            "repaired file should have one line, got: {repaired:?}"
        );
        assert!(repaired.ends_with('\n'));

        // A second check is clean.
        let report = check_session(&paths, "repair-1").unwrap();
        assert!(
            report.is_clean(),
            "post-repair session should be clean, got: {:?}",
            report.issues
        );
    }

    #[test]
    fn file_kind_all_covers_four_filenames() {
        let names: Vec<&'static str> = FileKind::all().iter().map(|k| k.filename()).collect();
        assert_eq!(
            names,
            vec![
                "pending.jsonl",
                "ledger.jsonl",
                "pending_changes.jsonl",
                "ledger_changes.jsonl"
            ]
        );
    }
}
