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
        println!("sift: no pending entries match '{target}'");
        return Ok(());
    }
    for id in &ids {
        store.finalize(id, Status::Accepted)?;
    }
    println!("sift: accepted {} entries", ids.len());
    Ok(())
}

pub(crate) fn resolve_target_ids(entries: &[LedgerEntry], target: &str) -> Vec<String> {
    match target {
        "all" => entries.iter().map(|e| e.id.clone()).collect(),
        t if t.starts_with("turn-") => {
            if let Ok(n) = t["turn-".len()..].parse::<u32>() {
                entries
                    .iter()
                    .filter(|e| e.turn == n)
                    .map(|e| e.id.clone())
                    .collect()
            } else {
                vec![]
            }
        }
        prefix => entries
            .iter()
            .filter(|e| e.id.starts_with(prefix))
            .map(|e| e.id.clone())
            .collect(),
    }
}
