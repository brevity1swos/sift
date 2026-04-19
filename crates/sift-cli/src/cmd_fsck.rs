//! `sift fsck` subcommand: report (and optionally repair) ledger corruption.

use anyhow::{anyhow, Result};
use sift_core::fsck::{self, FsckReport, Issue, RepairReport, Rewrite};
use sift_core::paths::Paths;
use std::path::Path;

pub fn run(cwd: &Path, session: Option<String>, repair: bool, json: bool) -> Result<u8> {
    let paths = Paths::new(cwd);
    let session_id = resolve_session(&paths, session)?;

    if repair {
        let report = fsck::repair_session(&paths, &session_id)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            render_repair_text(&report);
        }
        Ok(0)
    } else {
        let report = fsck::check_session(&paths, &session_id)?;
        if json {
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            render_check_text(&report);
        }
        // Non-zero exit when issues are present so CI / scripting can gate on it.
        Ok(if report.is_clean() { 0 } else { 1 })
    }
}

fn resolve_session(paths: &Paths, explicit: Option<String>) -> Result<String> {
    if let Some(id) = explicit {
        return Ok(id);
    }
    // Fall back to the `current` symlink — same resolution as `sift list` etc.
    let link = paths.current_symlink();
    let target = std::fs::read_link(&link)
        .map_err(|e| anyhow!("no session specified and no current session ({}): {e}", link.display()))?;
    let name = target
        .file_name()
        .ok_or_else(|| anyhow!("current symlink target has no file_name: {}", target.display()))?;
    Ok(name.to_string_lossy().into_owned())
}

fn render_check_text(report: &FsckReport) {
    println!("session {} — fsck", report.session_id);
    if report.issues.is_empty() {
        println!("  clean (no issues)");
        return;
    }
    println!("  {} issue(s) found:", report.issues.len());
    for issue in &report.issues {
        println!("    {}", describe(issue));
    }
    println!();
    println!("Run `sift fsck --repair` to fix (session must be closed first).");
}

fn render_repair_text(report: &RepairReport) {
    println!("session {} — fsck --repair", report.session_id);
    println!("  {} issue(s) fixed:", report.issues.len());
    for issue in &report.issues {
        println!("    {}", describe(issue));
    }
    println!();
    if report.rewrites.is_empty() {
        println!("  no files rewritten");
    } else {
        println!("  {} file(s) rewritten:", report.rewrites.len());
        for rw in &report.rewrites {
            println!("    {}", describe_rewrite(rw));
        }
    }
}

fn describe(issue: &Issue) -> String {
    match issue {
        Issue::TruncatedTail {
            file,
            offset,
            byte_length,
        } => format!(
            "truncated-tail {} at offset {offset} ({byte_length} bytes, no trailing newline)",
            file
        ),
        Issue::InvalidJson { file, offset, err } => {
            format!("invalid-json {} at offset {offset}: {err}", file)
        }
        Issue::DuplicateId {
            file,
            id,
            offsets,
        } => format!(
            "duplicate-id {} id={id} at offsets {:?}",
            file, offsets
        ),
        Issue::OrphanTombstone { file, id, offset } => format!(
            "orphan-tombstone {} id={id} at offset {offset}",
            file
        ),
    }
}

fn describe_rewrite(rw: &Rewrite) -> String {
    format!(
        "{}: kept {} record(s), archive = {}",
        rw.file,
        rw.records_kept,
        rw.bad_archive.display(),
    )
}
