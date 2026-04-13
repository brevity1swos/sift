//! PreToolUse hook handler.
//!
//! For Write/Edit/MultiEdit tool calls, snapshot the prior file state (if
//! the file exists) into a content-addressed blob and write a staging
//! record keyed by `correlation::derive_key(&event.raw)`. PostToolUse will
//! read this staging record, capture the post-tool state, and finalize a
//! pending ledger entry.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sift_core::{
    correlation::derive_key,
    paths::{validate_relative_path, Paths},
    policy::{Action, Policy},
    session::Session,
    snapshot::SnapshotStore,
};
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::payload::HookEvent;

#[derive(Debug, Serialize, Deserialize)]
pub struct StagingRecord {
    pub path: PathBuf,
    pub pre_hash: Option<String>, // None means the file did not exist before
    pub tool_name: String,
}

/// Staging record for Bash commands: just a timestamp so post-tool can
/// find files modified after this point.
#[derive(Debug, Serialize, Deserialize)]
pub struct BashStaging {
    pub timestamp_ms: u128,
    pub command: String,
}

pub fn run(event: HookEvent) -> Result<ExitCode> {
    let project_root = event.cwd.unwrap_or_else(|| PathBuf::from("."));
    let paths = Paths::new(&project_root);

    if paths.current_symlink().symlink_metadata().is_err() {
        return Ok(ExitCode::from(0));
    }
    let session = Session::open_current(Paths::new(&project_root))?;

    let Some(tool_name) = event.tool_name else {
        return Ok(ExitCode::from(0));
    };

    // Bash: save a timestamp marker so post-tool can detect modified files.
    if tool_name == "Bash" {
        handle_bash_pre(&paths, &session, &event.raw, &event.tool_input)?;
        return Ok(ExitCode::from(0));
    }

    if !matches!(tool_name.as_str(), "Write" | "Edit" | "MultiEdit") {
        return Ok(ExitCode::from(0));
    }

    let Some(tool_input) = event.tool_input else {
        return Ok(ExitCode::from(0));
    };
    let Some(target_path) = tool_input.get("file_path").and_then(|v| v.as_str()) else {
        return Ok(ExitCode::from(0));
    };
    let target_path = PathBuf::from(target_path);

    // Security: only snapshot files that live under the project root.
    // Claude Code always supplies absolute paths. We canonicalize both the
    // project root and the target to resolve OS-level symlinks (e.g. on
    // macOS, /var → /private/var) before checking containment.
    // For files that don't exist yet (Create ops), `canonicalize` will fail;
    // in that case we fall back to lexical prefix matching after cleaning
    // any `..` components.
    let canonical_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let rel_path = if target_path.is_absolute() {
        // Try to canonicalize the target (works when the file already exists).
        // Fall back to the raw path when the file doesn't exist yet.
        let canonical_target = target_path
            .canonicalize()
            .unwrap_or_else(|_| target_path.to_path_buf());
        match canonical_target.strip_prefix(&canonical_root) {
            Ok(rel) => rel.to_path_buf(),
            Err(_) => {
                // Also try stripping the non-canonical root, in case the
                // project root itself wasn't resolvable (unlikely but safe).
                match target_path.strip_prefix(&project_root) {
                    Ok(rel) => rel.to_path_buf(),
                    Err(_) => {
                        // File is outside the project root — skip silently.
                        return Ok(ExitCode::from(0));
                    }
                }
            }
        }
    } else {
        // Relative path: reject `..` components that could escape the root.
        if validate_relative_path(&target_path).is_err() {
            return Ok(ExitCode::from(0));
        }
        target_path.to_path_buf()
    };

    // Policy check: evaluate the relative path against .sift/policy.yml.
    let policy = Policy::load(&paths.policy_file())?;
    match policy.evaluate(&rel_path) {
        Action::Deny => {
            eprintln!(
                "sift: BLOCKED by policy — write to '{}' is denied by .sift/policy.yml",
                rel_path.display()
            );
            return Ok(ExitCode::from(2));
        }
        Action::Review => {
            eprintln!(
                "sift: note — write to '{}' is flagged for review by .sift/policy.yml",
                rel_path.display()
            );
        }
        Action::Allow => {}
    }

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

    let record = StagingRecord {
        path: rel_path,
        pre_hash,
        tool_name,
    };
    fs::write(&staging_path, serde_json::to_string(&record)?)
        .with_context(|| format!("writing staging {}", staging_path.display()))?;
    Ok(ExitCode::from(0))
}

fn handle_bash_pre(
    paths: &Paths,
    session: &Session,
    raw: &serde_json::Value,
    tool_input: &Option<serde_json::Value>,
) -> Result<()> {
    let command = tool_input
        .as_ref()
        .and_then(|ti| ti.get("command").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();

    let timestamp_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();

    let key = derive_key(raw);
    let staging_path = paths.staging_path(&session.id, &key);
    if let Some(p) = staging_path.parent() {
        fs::create_dir_all(p)
            .with_context(|| format!("creating staging dir {}", p.display()))?;
    }

    let record = BashStaging {
        timestamp_ms,
        command,
    };
    fs::write(&staging_path, serde_json::to_string(&record)?)
        .with_context(|| format!("writing bash staging {}", staging_path.display()))?;
    Ok(())
}
