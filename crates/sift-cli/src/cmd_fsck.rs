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
        validate_session_id(&id)?;
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

/// Reject session ids that could traverse outside `.sift/sessions/`. The
/// ID becomes a path component via `Paths::session_dir(id).join(id)`, and
/// `PathBuf::join` does not block `..`. A user running
/// `sift fsck --session "../../etc" --repair` could otherwise rename /etc
/// files (still bounded by their own permissions, but well outside the
/// sift sandbox). Allow only the charset Session::create produces:
/// `[A-Za-z0-9-]` (timestamp ids look like `2026-04-19-144125`).
fn validate_session_id(id: &str) -> Result<()> {
    if id.is_empty() {
        anyhow::bail!("session id is empty");
    }
    if id.contains('/') || id.contains('\\') || id == "." || id == ".." {
        anyhow::bail!("session id {id:?} contains a path separator or traversal");
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        anyhow::bail!(
            "session id {id:?} contains characters outside [A-Za-z0-9_-]"
        );
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::validate_session_id;

    #[test]
    fn accepts_canonical_timestamp_id() {
        assert!(validate_session_id("2026-04-19-144125").is_ok());
    }

    #[test]
    fn accepts_id_with_dash_suffix_for_collision() {
        // Session::create appends `-1`, `-2` etc. on collisions.
        assert!(validate_session_id("2026-04-19-144125-3").is_ok());
    }

    #[test]
    fn rejects_traversal_attempts() {
        assert!(validate_session_id("..").is_err());
        assert!(validate_session_id("../etc").is_err());
        assert!(validate_session_id("../../foo").is_err());
        assert!(validate_session_id("foo/bar").is_err());
        assert!(validate_session_id("foo\\bar").is_err());
    }

    #[test]
    fn rejects_empty_id() {
        assert!(validate_session_id("").is_err());
    }

    #[test]
    fn rejects_special_characters() {
        assert!(validate_session_id("foo bar").is_err()); // space
        assert!(validate_session_id("foo:bar").is_err()); // colon (Windows alt-stream marker)
        assert!(validate_session_id("foo$(date)").is_err());
    }
}
