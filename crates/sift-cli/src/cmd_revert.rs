use anyhow::Result;
use sift_core::{entry::Status, paths::Paths, session::Session, store::Store};
use std::path::Path;

pub fn run(cwd: &Path, target: String) -> Result<()> {
    let paths = Paths::new(cwd);
    let session = Session::open_current(Paths::new(cwd))?;
    let store = Store::new(&session.dir);

    // First try pending entries.
    let pending = store.list_pending()?;
    let pending_ids = crate::cmd_accept::resolve_target_ids(&pending, &target);

    // Then check ledger for accepted entries (revert after accept).
    let ledger = store.list_ledger()?;
    let accepted: Vec<_> = ledger
        .iter()
        .filter(|e| e.status == Status::Accepted)
        .cloned()
        .collect();
    let ledger_ids = crate::cmd_accept::resolve_target_ids(&accepted, &target);

    if pending_ids.is_empty() && ledger_ids.is_empty() {
        println!("sift: no entries match '{target}'");
        return Ok(());
    }

    let mut count = 0;

    // Revert pending entries (finalize + restore).
    for id in &pending_ids {
        let entry = store.finalize(id, Status::Reverted)?;
        store.restore_snapshot(&entry, paths.project_root(), &paths, &session.id)?;
        count += 1;
    }

    // Revert accepted entries (update ledger status + restore).
    for id in &ledger_ids {
        let entry = store.update_ledger_status(id, Status::Reverted)?;
        store.restore_snapshot(&entry, paths.project_root(), &paths, &session.id)?;
        count += 1;
    }

    println!("sift: reverted {count} entries");
    Ok(())
}
