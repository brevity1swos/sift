use anyhow::Result;
use sift_core::{entry::Status, paths::Paths, session::Session, store::Store, LedgerEntry};
use std::path::Path;

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
