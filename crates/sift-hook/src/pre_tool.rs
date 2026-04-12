//! PreToolUse hook handler.
//!
//! For Write/Edit/MultiEdit tool calls, snapshot the prior file state (if
//! the file exists) into a content-addressed blob and write a staging
//! record keyed by `correlation::derive_key(&event.raw)`. PostToolUse will
//! read this staging record, capture the post-tool state, and finalize a
//! pending ledger entry.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sift_core::{correlation::derive_key, paths::Paths, session::Session, snapshot::SnapshotStore};
use std::fs;
use std::path::PathBuf;

use crate::payload::HookEvent;

#[derive(Debug, Serialize, Deserialize)]
pub struct StagingRecord {
    pub path: PathBuf,
    pub pre_hash: Option<String>, // None means the file did not exist before
    pub tool_name: String,
}

pub fn run(event: HookEvent) -> Result<()> {
    let project_root = event.cwd.clone().unwrap_or_else(|| PathBuf::from("."));
    let paths = Paths::new(project_root.clone());

    // No current session → nothing to record.
    if paths.current_symlink().symlink_metadata().is_err() {
        return Ok(());
    }
    let session = Session::open_current(paths.clone())?;

    // Only Write/Edit/MultiEdit are captured in v0.1.
    let Some(tool_name) = event.tool_name.clone() else {
        return Ok(());
    };
    if !matches!(tool_name.as_str(), "Write" | "Edit" | "MultiEdit") {
        return Ok(());
    }

    let Some(tool_input) = event.tool_input.clone() else {
        return Ok(());
    };
    let Some(target_path) = tool_input.get("file_path").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    let target_path = PathBuf::from(target_path);

    // Snapshot the pre-state, if the file exists.
    let snap = SnapshotStore::new(&paths, &session.id);
    let pre_hash = match fs::read(&target_path) {
        Ok(bytes) => Some(snap.put(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {}", target_path.display())),
    };

    // Compute a correlation key from the event's raw payload.
    let key = derive_key(&event.raw);
    let staging_path = paths.staging_path(&session.id, &key);
    if let Some(p) = staging_path.parent() {
        fs::create_dir_all(p).with_context(|| format!("creating staging dir {}", p.display()))?;
    }

    // Store the target path RELATIVE to the project root so the ledger is
    // portable. Fall back to the absolute path if the target is outside
    // project_root (defensive; real Claude Code tool calls use absolute
    // paths inside the project).
    let rel_path = target_path
        .strip_prefix(&project_root)
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|_| target_path.clone());

    let record = StagingRecord {
        path: rel_path,
        pre_hash,
        tool_name,
    };
    fs::write(&staging_path, serde_json::to_string(&record)?)
        .with_context(|| format!("writing staging {}", staging_path.display()))?;
    Ok(())
}
