use anyhow::Result;
use sift_core::{entry::LedgerEntry, store::Store};
use std::path::Path;

pub fn run(
    cwd: &Path,
    pending_only: bool,
    turn: Option<u32>,
    session: Option<String>,
    path: Option<String>,
    json: bool,
) -> Result<()> {
    let dir = crate::resolve_session_dir(cwd, session)?;
    let store = Store::new(&dir);
    let mut entries: Vec<LedgerEntry> = if pending_only {
        store.list_pending()?
    } else {
        let mut p = store.list_pending()?;
        p.extend(store.list_ledger()?);
        p
    };
    if let Some(t) = turn {
        entries.retain(|e| e.turn == t);
    }
    if let Some(needle) = path.as_deref() {
        let needle = needle.to_lowercase();
        entries.retain(|e| e.path.to_string_lossy().to_lowercase().contains(&needle));
    }
    entries.sort_by_key(|e| e.timestamp);
    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        for e in &entries {
            println!(
                "{} turn{} [{}] {} {} +{} -{}",
                &e.id[..8.min(e.id.len())],
                e.turn,
                e.status,
                e.op,
                e.path.display(),
                e.diff_stats.added,
                e.diff_stats.removed,
            );
        }
    }
    Ok(())
}
