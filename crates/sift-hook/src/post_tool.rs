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
use crate::pre_tool::{BashStaging, StagingRecord};

pub fn run(event: HookEvent) -> Result<()> {
    let project_root = event
        .cwd
        .clone()
        .unwrap_or_else(|| PathBuf::from("."));
    let paths = Paths::new(&project_root);

    if paths.current_symlink().symlink_metadata().is_err() {
        return Ok(());
    }
    let session = Session::open_current(paths.clone())?;

    // Bash: detect files modified since the pre-tool timestamp.
    if event.tool_name.as_deref() == Some("Bash") {
        return handle_bash_post(&event, &paths, &session, &project_root);
    }

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

    // Best-effort rationale from the session transcript. If the transcript
    // is missing or unparseable, fall back to empty (user can annotate later
    // via `n` in the TUI).
    let rationale = event
        .transcript_path
        .as_ref()
        .and_then(|p| extract_rationale_from_transcript(p))
        .unwrap_or_default();

    let entry = LedgerEntry {
        id: new_entry_id(),
        turn,
        tool,
        path: staging.path,
        op,
        rationale,
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

/// Handle Bash PostToolUse: walk the project, find files modified after the
/// pre-tool timestamp, and create ledger entries for each.
fn handle_bash_post(
    event: &HookEvent,
    paths: &Paths,
    session: &Session,
    project_root: &std::path::Path,
) -> Result<()> {
    use walkdir::WalkDir;

    let key = derive_key(&event.raw);
    let staging_path = paths.staging_path(&session.id, &key);
    let bash: BashStaging = match fs::read_to_string(&staging_path) {
        Ok(s) => serde_json::from_str(&s)
            .with_context(|| format!("parsing bash staging {}", staging_path.display()))?,
        Err(_) => return Ok(()), // no pre-tool record
    };

    let pre_time = std::time::UNIX_EPOCH
        + std::time::Duration::from_millis(bash.timestamp_ms as u64);

    let snap = SnapshotStore::new(paths, &session.id);
    let turn = SessionState::load(&session.state_path())
        .map(|s| s.turn)
        .unwrap_or(0);
    let store = Store::new(&session.dir);

    let rationale = if bash.command.is_empty() {
        String::new()
    } else {
        format!("bash: {}", truncate_to_sentence(&bash.command, 100))
    };

    // Walk the project and find files modified after the pre-tool timestamp.
    for entry in WalkDir::new(project_root).into_iter().filter_entry(|e| {
        let name = e.file_name().to_string_lossy();
        !matches!(
            name.as_ref(),
            ".git" | "target" | "node_modules" | ".sift" | ".omc"
        )
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let modified = match meta.modified() {
            Ok(t) => t,
            Err(_) => continue,
        };
        // Only files modified AFTER the pre-tool timestamp.
        if modified <= pre_time {
            continue;
        }
        let abs_path = entry.path();
        let rel_path = match abs_path.strip_prefix(project_root) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue,
        };

        // Skip paths that look like build artifacts or hidden files.
        let rel_str = rel_path.to_string_lossy();
        if rel_str.starts_with('.') || rel_str.contains("/.") {
            continue;
        }

        // Snapshot the current content as post-state.
        let bytes = match fs::read(abs_path) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let post_hash = snap.put(&bytes)?;
        let diff_stats = stats(
            "",
            &String::from_utf8_lossy(&bytes),
        );

        let ledger_entry = LedgerEntry {
            id: new_entry_id(),
            turn,
            tool: Tool::Write, // Attribute to Write since we can't distinguish
            path: rel_path,
            op: Op::Modify, // Conservative — could be create or modify
            rationale: rationale.clone(),
            diff_stats,
            snapshot_before: None, // No pre-state for Bash
            snapshot_after: Some(post_hash),
            status: Status::Pending,
            timestamp: Utc::now(),
        };
        store.append_pending(&ledger_entry)?;
    }

    let _ = fs::remove_file(&staging_path);
    Ok(())
}

/// Scan the Claude Code session transcript (JSONL) backward and extract the
/// most recent assistant text as a one-line rationale. The transcript format
/// is undocumented and may change; this is best-effort.
///
/// Strategy: read the file, parse each line as a JSON object, collect all
/// objects with `"role": "assistant"` and a `"content"` field containing text,
/// take the last one, and extract the first sentence (up to 120 chars).
fn extract_rationale_from_transcript(path: &std::path::Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let mut last_assistant_text: Option<String> = None;

    for line in text.lines() {
        let obj: serde_json::Value = serde_json::from_str(line).ok()?;

        // Claude Code transcripts use various shapes. Try common ones:
        // 1. {"role": "assistant", "content": "text..."}
        // 2. {"role": "assistant", "content": [{"type": "text", "text": "..."}]}
        // 3. {"type": "assistant", "message": {"content": [...]}}
        if obj.get("role").and_then(|v| v.as_str()) == Some("assistant") {
            if let Some(content) = obj.get("content") {
                let text = extract_text_from_content(content);
                if !text.is_empty() {
                    last_assistant_text = Some(text);
                }
            }
        } else if obj.get("type").and_then(|v| v.as_str()) == Some("assistant") {
            if let Some(msg) = obj.get("message") {
                if let Some(content) = msg.get("content") {
                    let text = extract_text_from_content(content);
                    if !text.is_empty() {
                        last_assistant_text = Some(text);
                    }
                }
            }
        }
    }

    last_assistant_text.map(|t| truncate_to_sentence(&t, 120))
}

fn extract_text_from_content(content: &serde_json::Value) -> String {
    // String content: "content": "some text"
    if let Some(s) = content.as_str() {
        return s.trim().to_string();
    }
    // Array content: "content": [{"type": "text", "text": "..."}]
    if let Some(arr) = content.as_array() {
        for item in arr {
            if item.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                    let trimmed = t.trim();
                    if !trimmed.is_empty() {
                        return trimmed.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

/// Take the first sentence (ending in '.', '!', '?') or truncate to `max` chars.
fn truncate_to_sentence(text: &str, max: usize) -> String {
    // Take only the first line to avoid multi-paragraph rationale.
    let first_line = text.lines().next().unwrap_or(text);
    // Find the first sentence end.
    if let Some(pos) = first_line.find(['.', '!', '?']) {
        let end = pos + 1;
        if end <= max {
            return first_line[..end].to_string();
        }
    }
    // No sentence end found or too long — truncate.
    if first_line.len() <= max {
        first_line.to_string()
    } else {
        format!("{}...", &first_line[..max.saturating_sub(3)])
    }
}
