use anyhow::{Context, Result};
use sift_core::snapshot::sha1_of_file;
use sift_core::{entry::Status, paths::Paths, session::Session, store::Store, LedgerEntry};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run the target-based accept flow (pending-only, by id prefix / turn / "all").
pub fn run(cwd: &Path, target: String) -> Result<()> {
    let paths = Paths::new(cwd);
    let session = Session::open_current(paths)?;
    let store = Store::new(&session.dir);
    let pending = store.list_pending()?;
    let ids = resolve_target_ids(&pending, &target);
    if ids.is_empty() {
        if target == "all" {
            println!("sift: nothing to accept");
        } else if let Some(n) = parse_turn(&target) {
            println!("sift: no pending entries on turn {n}");
        } else {
            println!("sift: no pending entries match '{target}'");
        }
        return Ok(());
    }
    for id in &ids {
        store.finalize(id, Status::Accepted)?;
    }
    println!("sift: accepted {} entries", ids.len());
    Ok(())
}

/// Run the git-commit-driven accept flow. Accepts every pending entry
/// whose path is in the commit AND whose post-state hash matches the
/// file's current content. Diverged entries stay pending with a hint.
///
/// The workflow this closes: the user runs `git commit`, which settles
/// a set of paths; sift's pending ledger (which recorded each write
/// per-turn) stays consistent by auto-accepting the writes git
/// endorsed. Without this, the user would have to approve the same
/// change twice (once per `sift accept`, once per `git commit`), and
/// sift's pending list would drift out of sync with reality.
pub fn run_by_commit(cwd: &Path, git_ref: &str, apply: bool, quiet: bool) -> Result<()> {
    let paths_obj = Paths::new(cwd);
    let session = Session::open_current(paths_obj.clone())?;
    let store = Store::new(&session.dir);
    let pending = store.list_pending()?;

    let commit_paths = paths_in_commit(cwd, git_ref)
        .with_context(|| format!("resolving paths in commit {git_ref}"))?;
    let commit_set: HashSet<PathBuf> = commit_paths.iter().cloned().collect();

    let mut to_accept: Vec<&LedgerEntry> = Vec::new();
    let mut diverged: Vec<(PathBuf, String, String)> = Vec::new();
    let project_root = paths_obj.project_root();

    for entry in &pending {
        if !commit_set.contains(&entry.path) {
            continue;
        }
        let abs = project_root.join(&entry.path);
        let current_hash = match sha1_of_file(&abs) {
            Ok(h) => h,
            Err(_) => {
                // File vanished between the agent's write and the
                // commit — treat as divergence, not an error.
                diverged.push((
                    entry.path.clone(),
                    entry.snapshot_after.clone().unwrap_or_default(),
                    "<file not found>".into(),
                ));
                continue;
            }
        };
        if entry.snapshot_after.as_deref() == Some(&current_hash) {
            to_accept.push(entry);
        } else {
            diverged.push((
                entry.path.clone(),
                entry.snapshot_after.clone().unwrap_or_default(),
                current_hash,
            ));
        }
    }

    // Commit paths sift knew nothing about (user's own edits) — count
    // for the summary only; not an error.
    let commit_paths_without_match = commit_set
        .iter()
        .filter(|p| !pending.iter().any(|e| &e.path == *p))
        .count();

    if !quiet {
        println!("sift accept --by-commit {git_ref}:");
        println!(
            "  {} entries match committed content",
            to_accept.len()
        );
        if !diverged.is_empty() {
            println!(
                "  {} entries diverged (file changed since the agent wrote it — review manually)",
                diverged.len()
            );
            for (path, expected, actual) in &diverged {
                println!(
                    "      {} (recorded={}, current={})",
                    path.display(),
                    &expected[..8.min(expected.len())],
                    if actual == "<file not found>" {
                        actual.as_str()
                    } else {
                        &actual[..8.min(actual.len())]
                    }
                );
            }
        }
        if commit_paths_without_match > 0 {
            println!(
                "  {} committed paths had no matching pending entry (user edits, ignored)",
                commit_paths_without_match
            );
        }
        if !apply {
            println!("  (dry-run — pass --apply to finalize)");
        }
    }

    if apply {
        let ids: Vec<String> = to_accept.iter().map(|e| e.id.clone()).collect();
        for id in &ids {
            store.finalize(id, Status::Accepted)?;
        }
        if !quiet {
            println!("  accepted {}", ids.len());
        }
    }

    Ok(())
}

/// List the paths changed in a git commit. Uses `git show --name-only
/// --pretty=` which works for both normal and root commits (no parent
/// edge case to handle). Returns an empty vec for a merge commit with
/// no path-level changes, which is the semantically right answer.
fn paths_in_commit(cwd: &Path, git_ref: &str) -> Result<Vec<PathBuf>> {
    let output = Command::new("git")
        .args(["show", "--name-only", "--pretty=", git_ref])
        .current_dir(cwd)
        .output()
        .with_context(|| "running `git show` — is git installed and on PATH?")?;
    anyhow::ensure!(
        output.status.success(),
        "git show {} exited with {}: {}",
        git_ref,
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let paths = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .collect();
    Ok(paths)
}

/// Parse a turn number from "turn-1", "turn1", "turn-12", "turn12", etc.
pub(crate) fn parse_turn(t: &str) -> Option<u32> {
    t.strip_prefix("turn-")
        .or_else(|| t.strip_prefix("turn"))
        .and_then(|n| n.parse::<u32>().ok())
}

pub(crate) fn is_bulk_target(target: &str) -> bool {
    target == "all" || parse_turn(target).is_some()
}

pub(crate) fn resolve_target_ids(entries: &[LedgerEntry], target: &str) -> Vec<String> {
    if target == "all" {
        return entries.iter().map(|e| e.id.clone()).collect();
    }
    if let Some(n) = parse_turn(target) {
        return entries
            .iter()
            .filter(|e| e.turn == n)
            .map(|e| e.id.clone())
            .collect();
    }
    // Treat anything else as an id prefix.
    entries
        .iter()
        .filter(|e| e.id.starts_with(target))
        .map(|e| e.id.clone())
        .collect()
}
