//! Junk-detection heuristics for `sift sweep`.
//!
//! v0.1 rules:
//! 1. Exact-content duplicates: two paths whose snapshot_after hashes match.
//! 2. Slop-pattern globs: *_v[0-9]*, *_new, *_old, *_final, *_backup, *_copy, scratch_*, tmp_*.
//! 3. Orphan markdown: new .md files not referenced by any other file in the project.

use anyhow::Result;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

use crate::entry::{LedgerEntry, Op};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepReason {
    ExactDuplicateOf(PathBuf),
    FuzzyDuplicate {
        similar_to: PathBuf,
        similarity: u8, // 0-100 percent
    },
    SlopPattern(String),
    OrphanMarkdown,
}

#[derive(Debug, Clone)]
pub struct SweepCandidate {
    pub entry_id: String,
    pub path: PathBuf,
    pub reason: SweepReason,
}

/// Return a reference to the shared slop-pattern `GlobSet`.
///
/// Built once at first call via `OnceLock`; subsequent calls return the
/// cached value. All patterns are compile-time constants so the `expect`
/// calls are infallible in practice.
pub fn slop_globs() -> &'static GlobSet {
    use std::sync::OnceLock;
    static GLOBS: OnceLock<GlobSet> = OnceLock::new();
    GLOBS.get_or_init(|| {
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
    })
}

/// Scan pending entries and return sweep candidates.
/// - `project_root` is used for orphan-markdown reference scanning.
/// - Only entries with status=Pending are considered by callers; this function
///   does not filter by status — callers are expected to pre-filter.
///
/// Rules 2 and 3 both skip `Op::Delete` entries: a delete for a file that is
/// already being removed should not be doubly flagged as junk.
pub fn detect(pending: &[LedgerEntry], project_root: &Path) -> Result<Vec<SweepCandidate>> {
    let mut out = Vec::new();
    let globs = slop_globs();
    detect_exact_dups(pending, globs, &mut out);
    detect_fuzzy_dups(pending, project_root, &mut out);
    detect_slop(pending, globs, &mut out);
    detect_orphan_markdown(pending, project_root, &mut out)?;
    Ok(out)
}

/// Rule 1b: fuzzy duplicate detection. For pending entries not already flagged,
/// compare file contents pairwise. If two files are >80% similar (by line),
/// flag the later one as a near-duplicate.
fn detect_fuzzy_dups(
    pending: &[LedgerEntry],
    project_root: &Path,
    out: &mut Vec<SweepCandidate>,
) {
    use similar::TextDiff;

    // Collect non-flagged, non-delete entries with readable content.
    let candidates: Vec<(usize, &LedgerEntry, String)> = pending
        .iter()
        .enumerate()
        .filter(|(_, e)| e.op != Op::Delete)
        .filter(|(_, e)| !out.iter().any(|c| c.entry_id == e.id))
        .filter_map(|(i, e)| {
            let path = project_root.join(&e.path);
            std::fs::read_to_string(&path).ok().map(|content| (i, e, content))
        })
        .collect();

    // Pairwise comparison — O(n²) but n is typically < 50 per session.
    for i in 0..candidates.len() {
        if out.iter().any(|c| c.entry_id == candidates[i].1.id) {
            continue;
        }
        for j in (i + 1)..candidates.len() {
            if out.iter().any(|c| c.entry_id == candidates[j].1.id) {
                continue;
            }
            // Skip if same hash (already caught by exact dup rule).
            if candidates[i].1.snapshot_after == candidates[j].1.snapshot_after {
                continue;
            }
            let ratio = TextDiff::from_lines(&candidates[i].2, &candidates[j].2).ratio();
            if ratio >= 0.8 {
                out.push(SweepCandidate {
                    entry_id: candidates[j].1.id.clone(),
                    path: candidates[j].1.path.clone(),
                    reason: SweepReason::FuzzyDuplicate {
                        similar_to: candidates[i].1.path.clone(),
                        similarity: (ratio * 100.0) as u8,
                    },
                });
            }
        }
    }
}

/// Rule 1: flag entries whose `snapshot_after` hash matches an earlier entry.
///
/// Keeps the earliest *non-slop* path as the canonical copy. If the first-seen
/// path is itself a slop match and a later one is not, the direction flips so
/// the slop file is flagged as the duplicate.
fn detect_exact_dups(
    pending: &[LedgerEntry],
    globs: &GlobSet,
    out: &mut Vec<SweepCandidate>,
) {
    let mut seen_hashes: HashMap<String, PathBuf> = HashMap::new();
    for e in pending {
        if e.op == Op::Delete {
            continue;
        }
        let Some(h) = &e.snapshot_after else { continue };
        match seen_hashes.get(h).cloned() {
            None => {
                seen_hashes.insert(h.clone(), e.path.clone());
            }
            Some(first) if first == e.path => {
                // Same path written twice — not a dup in the "two files with
                // identical content" sense. Skip.
            }
            Some(first) => {
                let first_is_slop = globs.is_match(&first);
                let curr_is_slop = globs.is_match(&e.path);
                if first_is_slop && !curr_is_slop {
                    // Promote the current (cleaner) name to canonical.
                    let flagged_id = pending
                        .iter()
                        .find(|p| p.path == first && p.snapshot_after.as_ref() == Some(h))
                        .map(|p| p.id.clone())
                        .unwrap_or_default();
                    if !flagged_id.is_empty() {
                        out.push(SweepCandidate {
                            entry_id: flagged_id,
                            path: first.clone(),
                            reason: SweepReason::ExactDuplicateOf(e.path.clone()),
                        });
                    }
                    seen_hashes.insert(h.clone(), e.path.clone());
                } else {
                    out.push(SweepCandidate {
                        entry_id: e.id.clone(),
                        path: e.path.clone(),
                        reason: SweepReason::ExactDuplicateOf(first.clone()),
                    });
                }
            }
        }
    }
}

/// Rule 2: flag entries whose paths match the slop-pattern glob set.
/// Skips entries already flagged by rule 1 and `Op::Delete` entries.
fn detect_slop(pending: &[LedgerEntry], globs: &GlobSet, out: &mut Vec<SweepCandidate>) {
    for e in pending {
        if e.op == Op::Delete {
            continue;
        }
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
}

/// Rule 3: flag `Op::Create` `.md` entries not referenced by any project file.
/// Skips entries already flagged by rules 1 or 2.
fn detect_orphan_markdown(
    pending: &[LedgerEntry],
    project_root: &Path,
    out: &mut Vec<SweepCandidate>,
) -> Result<()> {
    for md in pending
        .iter()
        .filter(|e| e.op == Op::Create)
        .filter(|e| e.path.extension().and_then(|s| s.to_str()) == Some("md"))
    {
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
    Ok(())
}

/// Lexical-only path cleanup: strip `Component::CurDir` (`.`) components so
/// `foo/./bar` becomes `foo/bar`. Does not resolve symlinks or `..` — this
/// is only for comparing paths built via `join` against WalkDir output, which
/// never contains `.` components itself.
fn lexical_clean(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        if !matches!(c, Component::CurDir) {
            out.push(c.as_os_str());
        }
    }
    out
}

/// Scan the project root for any file whose bytes contain `basename`. Skips
/// `target/`, `.git/`, `.sift/`, and `node_modules/`. Skips `exclude` (the
/// markdown file itself) so it cannot reference itself.
///
/// Uses byte-level search over `fs::read` rather than `read_to_string` so
/// binary files that happen to be valid UTF-8 don't silently allocate their
/// full content, and so we stop scanning as soon as the basename is found.
fn is_referenced(project_root: &Path, exclude: &Path, basename: &str) -> Result<bool> {
    // Lexical clean handles `./foo.md`-style inputs; WalkDir output never
    // contains `.` components so only exclude_abs needs cleaning.
    let exclude_abs = lexical_clean(&project_root.join(exclude));
    let needle = basename.as_bytes();
    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !matches!(name.as_ref(), "target" | ".git" | ".sift" | "node_modules")
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
        let Ok(bytes) = std::fs::read(entry.path()) else {
            continue;
        };
        if bytes.windows(needle.len()).any(|w| w == needle) {
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
            diff_stats: DiffStats {
                added: 0,
                removed: 0,
            },
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
        assert!(c
            .iter()
            .any(|x| matches!(x.reason, SweepReason::ExactDuplicateOf(_))));
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
        assert!(c
            .iter()
            .all(|x| matches!(x.reason, SweepReason::SlopPattern(_))));
    }

    #[test]
    fn detects_orphan_markdown() {
        let td = TempDir::new().unwrap();
        // Create a markdown file with a basename that isn't referenced anywhere.
        std::fs::write(td.path().join("lonely.md"), "this one stands alone").unwrap();
        // A referenced md file should NOT be flagged.
        std::fs::write(td.path().join("referenced.md"), "documented behavior").unwrap();
        std::fs::write(td.path().join("code.rs"), "// see referenced for details").unwrap();

        // NOTE: file contents are deliberately distinct to keep fuzzy-dup
        // detection from firing on identical-content fixtures — that would
        // flag referenced.md as a fuzzy duplicate and suppress the orphan
        // check we are trying to exercise here.
        let pending = vec![
            e("01", "lonely.md", Op::Create, Some("h1")),
            e("02", "referenced.md", Op::Create, Some("h2")),
        ];
        let c = detect(&pending, td.path()).unwrap();
        assert_eq!(c.len(), 1, "got {} candidates: {:?}", c.len(), c);
        assert_eq!(c[0].entry_id, "01");
        assert!(matches!(c[0].reason, SweepReason::OrphanMarkdown));
    }

    #[test]
    fn empty_pending_returns_empty_output() {
        let td = TempDir::new().unwrap();
        let c = detect(&[], td.path()).unwrap();
        assert!(c.is_empty());
    }

    #[test]
    fn deletes_are_not_flagged_as_slop_or_dup() {
        let td = TempDir::new().unwrap();
        let pending = vec![
            // A Delete of a scratch file — should NOT be flagged even though
            // the name matches the slop glob.
            e("d1", "scratch_thing.py", Op::Delete, None),
            // A Create of a real file with the same hash as another —
            // non-delete dup still flagged.
            e("c1", "a.txt", Op::Create, Some("h1")),
            e("c2", "b.txt", Op::Create, Some("h1")),
        ];
        let c = detect(&pending, td.path()).unwrap();
        // Delete not in output; only c2 flagged as dup of c1.
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].entry_id, "c2");
    }

    #[test]
    fn orphan_markdown_handles_dot_slash_paths() {
        // Regression guard for the lexical_clean fix: an `exclude` path with
        // a `./` prefix previously compared unequal to WalkDir output and the
        // md file would scan itself, falsely discovering its own basename.
        let td = TempDir::new().unwrap();
        std::fs::write(td.path().join("lonely.md"), "lonely is in this file too").unwrap();
        let pending = vec![e("01", "./lonely.md", Op::Create, Some("h1"))];
        let c = detect(&pending, td.path()).unwrap();
        assert_eq!(c.len(), 1, "lonely.md should be flagged as orphan");
        assert_eq!(c[0].entry_id, "01");
    }

    #[test]
    fn multiple_orphan_md_files_each_flagged() {
        let td = TempDir::new().unwrap();
        // Use multi-char basenames so the needle for file A can't coincidentally
        // be a substring of file B's content (and vice versa), AND content that
        // is distinct so fuzzy-dup doesn't claim one of them and mask the
        // orphan-markdown flag we are asserting on.
        std::fs::write(td.path().join("alpha.md"), "one").unwrap();
        std::fs::write(td.path().join("omega.md"), "two").unwrap();
        let pending = vec![
            e("01", "alpha.md", Op::Create, Some("h1")),
            e("02", "omega.md", Op::Create, Some("h2")),
        ];
        let c = detect(&pending, td.path()).unwrap();
        assert_eq!(c.len(), 2, "got {} candidates: {:?}", c.len(), c);
        assert!(c
            .iter()
            .all(|x| matches!(x.reason, SweepReason::OrphanMarkdown)));
        assert!(c.iter().any(|x| x.entry_id == "01"));
        assert!(c.iter().any(|x| x.entry_id == "02"));
    }

    #[test]
    fn dup_direction_flips_when_first_seen_is_slop() {
        // foo_v2.py arrives FIRST with hash abc. Then foo.py arrives with
        // the same hash. The canonical-swap logic should flag foo_v2.py as
        // the dup of foo.py, not the other way around.
        let td = TempDir::new().unwrap();
        let pending = vec![
            e("01", "foo_v2.py", Op::Create, Some("abc")),
            e("02", "foo.py", Op::Create, Some("abc")),
        ];
        let c = detect(&pending, td.path()).unwrap();
        // Exactly one dup flag, pointing at foo_v2.py as duplicate of foo.py.
        assert_eq!(c.len(), 1, "got {} candidates: {:?}", c.len(), c);
        assert_eq!(c[0].entry_id, "01");
        match &c[0].reason {
            SweepReason::ExactDuplicateOf(p) => assert_eq!(p, &PathBuf::from("foo.py")),
            other => panic!("expected ExactDuplicateOf, got {other:?}"),
        }
    }
}
