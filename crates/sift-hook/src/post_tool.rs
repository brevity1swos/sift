//! PostToolUse hook handler.
//!
//! Reads the staging record written by PreToolUse, captures the post-state
//! of the target file, computes a diff, and appends a pending ledger entry.

use anyhow::{Context, Result};
use chrono::Utc;
use sift_core::{
    correlation::derive_key,
    diff::stats,
    entry::{new_entry_id, LedgerEntry, Op, Status, Tool},
    paths::Paths,
    session::Session,
    snapshot::SnapshotStore,
    state::SessionState,
    store::Store,
};
use std::fs;
use std::path::PathBuf;

use crate::payload::HookEvent;
use crate::pre_tool::StagingRecord;

pub fn run(event: HookEvent) -> Result<()> {
    let project_root = event.cwd.unwrap_or_else(|| PathBuf::from("."));
    let paths = Paths::new(&project_root);

    if paths.current_symlink().symlink_metadata().is_err() {
        return Ok(());
    }
    let session = Session::open_current(Paths::new(&project_root))?;

    // Only Write/Edit/MultiEdit are captured.
    let tool = match event.tool_name.as_deref() {
        Some("Write") => Tool::Write,
        Some("Edit") => Tool::Edit,
        Some("MultiEdit") => Tool::MultiEdit,
        _ => return Ok(()),
    };

    // Locate the matching staging record via the correlation key.
    let key = derive_key(&event.raw);
    let staging_path = paths.staging_path(&session.id, &key);
    let staging: StagingRecord = match fs::read_to_string(&staging_path) {
        Ok(s) => serde_json::from_str(&s)
            .with_context(|| format!("parsing staging {}", staging_path.display()))?,
        Err(_) => return Ok(()), // no matching pre-tool record; skip silently
    };

    // Capture post-state.
    let abs_target = project_root.join(&staging.path);
    let snap = SnapshotStore::new(&paths, &session.id);
    let post_exists = abs_target.exists();
    let post_hash = if post_exists {
        let bytes = fs::read(&abs_target)
            .with_context(|| format!("reading post-state {}", abs_target.display()))?;
        Some(snap.put(&bytes)?)
    } else {
        None
    };

    // Determine op from the (pre, post) state.
    let op = match (&staging.pre_hash, &post_hash) {
        (None, Some(_)) => Op::Create,
        (Some(_), Some(_)) => Op::Modify,
        (Some(_), None) => Op::Delete,
        (None, None) => {
            // Nothing happened (pre didn't exist, post doesn't exist).
            let _ = fs::remove_file(&staging_path);
            return Ok(());
        }
    };

    // Compute diff stats from the before/after text (lossy UTF-8 is fine for
    // stats; binary diffs will report 0 added/0 removed because from_lines
    // sees no line structure). Propagate snapshot corruption errors rather
    // than silently defaulting to empty bytes — a bad diff is worse than a
    // visible hook failure the user can investigate.
    let before_text = match &staging.pre_hash {
        Some(h) => String::from_utf8_lossy(&snap.get(h)?).into_owned(),
        None => String::new(),
    };
    let after_text = match &post_hash {
        Some(h) => String::from_utf8_lossy(&snap.get(h)?).into_owned(),
        None => String::new(),
    };
    let diff_stats = stats(&before_text, &after_text);

    let turn = SessionState::load(&session.state_path())
        .map(|s| s.turn)
        .unwrap_or(0);

    let entry = LedgerEntry {
        id: new_entry_id(),
        turn,
        tool,
        path: staging.path,
        op,
        rationale: String::new(),
        diff_stats,
        snapshot_before: staging.pre_hash,
        snapshot_after: post_hash,
        status: Status::Pending,
        timestamp: Utc::now(),
    };
    Store::new(&session.dir).append_pending(&entry)?;

    // Cleanup staging record (ignore errors — the entry is what matters).
    let _ = fs::remove_file(&staging_path);
    Ok(())
}
