//! Junk-detection heuristics for `sift sweep`.
//!
//! v0.1 rules:
//! 1. Exact-content duplicates: two paths whose snapshot_after hashes match.
//! 2. Slop-pattern globs: *_v[0-9]*, *_new, *_old, *_final, *_backup, *_copy, scratch_*, tmp_*.
//! 3. Orphan markdown: new .md files not referenced by any other file in the project.

use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::entry::{LedgerEntry, Op};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepReason {
    ExactDuplicateOf(PathBuf),
    SlopPattern(String),
    OrphanMarkdown,
}

#[derive(Debug, Clone)]
pub struct SweepCandidate {
    pub entry_id: String,
    pub path: PathBuf,
    pub reason: SweepReason,
}

pub fn slop_globs() -> GlobSet {
    let patterns = [
        "**/*_v[0-9]*",
        "**/*_new",
        "**/*_new.*",
        "**/*_old",
        "**/*_old.*",
        "**/*_final",
        "**/*_final.*",
        "**/*_backup",
        "**/*_backup.*",
        "**/*_copy",
        "**/*_copy.*",
        "**/scratch_*",
        "**/tmp_*",
    ];
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(Glob::new(p).expect("slop glob pattern is valid"));
    }
    b.build().expect("slop glob set builds")
}

/// Scan pending entries and return sweep candidates.
/// - `project_root` is used for orphan-markdown reference scanning.
/// - Only entries with status=Pending are considered by callers; this function
///   does not filter by status — callers are expected to pre-filter.
pub fn detect(pending: &[LedgerEntry], project_root: &Path) -> Result<Vec<SweepCandidate>> {
    let mut out = Vec::new();
    let globs = slop_globs();

    // Rule 1: exact dup by snapshot_after hash. Keep the earliest entry per hash.
    let mut seen_hashes: HashMap<String, PathBuf> = HashMap::new();
    for e in pending {
        if e.op == Op::Delete {
            continue;
        }
        if let Some(h) = &e.snapshot_after {
            if let Some(first) = seen_hashes.get(h) {
                if first != &e.path {
                    out.push(SweepCandidate {
                        entry_id: e.id.clone(),
                        path: e.path.clone(),
                        reason: SweepReason::ExactDuplicateOf(first.clone()),
                    });
                    continue;
                }
            } else {
                seen_hashes.insert(h.clone(), e.path.clone());
            }
        }
    }

    // Rule 2: slop-pattern globs
    for e in pending {
        if out.iter().any(|c| c.entry_id == e.id) {
            continue;
        }
        if globs.is_match(&e.path) {
            let pat_name = e
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(SweepCandidate {
                entry_id: e.id.clone(),
                path: e.path.clone(),
                reason: SweepReason::SlopPattern(pat_name),
            });
        }
    }

    // Rule 3: orphan markdown. Only considers Create entries with .md extension.
    let md_created: Vec<&LedgerEntry> = pending
        .iter()
        .filter(|e| e.op == Op::Create)
        .filter(|e| e.path.extension().and_then(|s| s.to_str()) == Some("md"))
        .collect();
    for md in md_created {
        if out.iter().any(|c| c.entry_id == md.id) {
            continue;
        }
        let basename = md.path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if basename.is_empty() {
            continue;
        }
        if !is_referenced(project_root, &md.path, basename)? {
            out.push(SweepCandidate {
                entry_id: md.id.clone(),
                path: md.path.clone(),
                reason: SweepReason::OrphanMarkdown,
            });
        }
    }

    Ok(out)
}

/// Scan the project root for any text file mentioning `basename`. Skips
/// `target/`, `.git/`, `.sift/`, and `node_modules/`. Skips `exclude` (the
/// markdown file itself) so it cannot reference itself.
fn is_referenced(project_root: &Path, exclude: &Path, basename: &str) -> Result<bool> {
    let exclude_abs = project_root.join(exclude);
    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !matches!(
            name.as_ref(),
            "target" | ".git" | ".sift" | "node_modules"
        )
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if entry.path() == exclude_abs {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        if content.contains(basename) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{DiffStats, Status, Tool};
    use chrono::Utc;
    use tempfile::TempDir;

    fn e(id: &str, path: &str, op: Op, hash_after: Option<&str>) -> LedgerEntry {
        LedgerEntry {
            id: id.into(),
            turn: 1,
            tool: Tool::Write,
            path: PathBuf::from(path),
            op,
            rationale: String::new(),
            diff_stats: DiffStats { added: 0, removed: 0 },
            snapshot_before: None,
            snapshot_after: hash_after.map(|s| s.into()),
            status: Status::Pending,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn detects_exact_duplicates() {
        let td = TempDir::new().unwrap();
        let pending = vec![
            e("01", "foo.py", Op::Create, Some("abc")),
            e("02", "foo_v2.py", Op::Create, Some("abc")),
        ];
        let c = detect(&pending, td.path()).unwrap();
        // foo_v2.py is both a duplicate AND slop-glob; duplicate rule wins (first).
        assert!(c.iter().any(|x| matches!(x.reason, SweepReason::ExactDuplicateOf(_))));
    }

    #[test]
    fn detects_slop_pattern_globs() {
        let td = TempDir::new().unwrap();
        let pending = vec![
            e("01", "scratch_thing.py", Op::Create, Some("h1")),
            e("02", "notes_final.md", Op::Create, Some("h2")),
            e("03", "tmp_output.txt", Op::Create, Some("h3")),
        ];
        let c = detect(&pending, td.path()).unwrap();
        assert_eq!(c.len(), 3);
        assert!(c.iter().all(|x| matches!(x.reason, SweepReason::SlopPattern(_))));
    }

    #[test]
    fn detects_orphan_markdown() {
        let td = TempDir::new().unwrap();
        // Create a markdown file with a basename that isn't referenced anywhere.
        std::fs::write(td.path().join("lonely.md"), "content").unwrap();
        // A referenced md file should NOT be flagged.
        std::fs::write(td.path().join("referenced.md"), "content").unwrap();
        std::fs::write(td.path().join("code.rs"), "// see referenced for details").unwrap();

        let pending = vec![
            e("01", "lonely.md", Op::Create, Some("h1")),
            e("02", "referenced.md", Op::Create, Some("h2")),
        ];
        let c = detect(&pending, td.path()).unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].entry_id, "01");
    }
}
