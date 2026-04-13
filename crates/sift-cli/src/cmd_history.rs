//! `sift history` — list all past sessions with summary stats.

use anyhow::Result;
use sift_core::{paths::Paths, session::SessionMeta, store::Store};
use std::fs;
use std::path::Path;

pub fn run(cwd: &Path, json: bool) -> Result<()> {
    let paths = Paths::new(cwd);
    let sessions_dir = paths.sessions_dir();

    if !sessions_dir.exists() {
        println!("sift: no sessions found");
        return Ok(());
    }

    let mut sessions: Vec<SessionInfo> = Vec::new();

    for entry in fs::read_dir(&sessions_dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let dir = entry.path();
        let meta_path = dir.join("meta.json");
        let meta: Option<SessionMeta> = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok());

        let store = Store::new(&dir);
        let pending = store.list_pending().unwrap_or_default().len();
        let ledger = store.list_ledger().unwrap_or_default();
        let accepted = ledger
            .iter()
            .filter(|e| e.status == sift_core::Status::Accepted)
            .count();
        let reverted = ledger
            .iter()
            .filter(|e| e.status == sift_core::Status::Reverted)
            .count();

        let id = meta
            .as_ref()
            .map(|m| m.id.clone())
            .unwrap_or_else(|| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            });
        let started = meta
            .as_ref()
            .map(|m| m.started_at.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_else(|| "unknown".into());
        let ended = meta
            .as_ref()
            .and_then(|m| m.ended_at)
            .map(|t| t.format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "active".into());

        // Check if this is the current session.
        let is_current = paths
            .current_symlink()
            .read_link()
            .ok()
            .map(|target| {
                let target = if target.is_absolute() {
                    target
                } else {
                    paths.sift_dir().join(target)
                };
                target == dir
            })
            .unwrap_or(false);

        sessions.push(SessionInfo {
            id,
            started,
            ended,
            total: ledger.len() + pending,
            accepted,
            reverted,
            pending,
            is_current,
        });
    }

    // Sort by id (which is a timestamp string) — newest first.
    sessions.sort_by(|a, b| b.id.cmp(&a.id));

    if json {
        println!("{}", serde_json::to_string_pretty(&sessions)?);
    } else {
        if sessions.is_empty() {
            println!("sift: no sessions found");
            return Ok(());
        }
        for s in &sessions {
            let marker = if s.is_current { " *" } else { "" };
            println!(
                "  {}{}  {}→{}  {} writes ({} ok, {} reverted, {} pending)",
                s.id, marker, s.started, s.ended, s.total, s.accepted, s.reverted, s.pending,
            );
        }
        println!();
        println!("  * = current session");
        println!("  Use `sift ls --session <id>` to inspect a specific session.");
    }
    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct SessionInfo {
    id: String,
    started: String,
    ended: String,
    total: usize,
    accepted: usize,
    reverted: usize,
    pending: usize,
    is_current: bool,
}
