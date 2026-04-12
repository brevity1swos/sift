use anyhow::Result;
use sift_core::{entry::Status, paths::Paths, session::Session, store::Store};
use std::path::Path;

pub fn run(cwd: &Path, target: String) -> Result<()> {
    let paths = Paths::new(cwd);
    let session = Session::open_current(Paths::new(cwd))?;
    let store = Store::new(&session.dir);

    let pending = store.list_pending()?;
    let is_bulk = crate::cmd_accept::is_bulk_target(&target);

    // Bulk targets (all, turn-N) only touch pending — never silently revert
    // accepted entries. Use a specific ID prefix to revert an accepted entry.
    let pending_ids = crate::cmd_accept::resolve_target_ids(&pending, &target);

    let mut ledger_ids: Vec<String> = vec![];
    if !is_bulk && pending_ids.is_empty() {
        // Specific ID prefix didn't match pending — check ledger for accepted.
        let ledger = store.list_ledger()?;
        let accepted: Vec<_> = ledger
            .iter()
            .filter(|e| e.status == Status::Accepted)
            .cloned()
            .collect();
        ledger_ids = crate::cmd_accept::resolve_target_ids(&accepted, &target);
    }

    if pending_ids.is_empty() && ledger_ids.is_empty() {
        if is_bulk {
            println!("sift: no pending entries match '{target}'");
        } else {
            println!("sift: no pending or accepted entries match '{target}'");
        }
        return Ok(());
    }

    let mut count = 0;

    for id in &pending_ids {
        let entry = store.finalize(id, Status::Reverted)?;
        store.restore_snapshot(&entry, paths.project_root(), &paths, &session.id)?;
        count += 1;
    }

    for id in &ledger_ids {
        let entry = store.update_ledger_status(id, Status::Reverted)?;
        store.restore_snapshot(&entry, paths.project_root(), &paths, &session.id)?;
        count += 1;
    }

    println!("sift: reverted {count} entries");
    Ok(())
}
