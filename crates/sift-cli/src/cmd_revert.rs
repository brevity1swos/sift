use anyhow::Result;
use sift_core::{entry::Status, paths::Paths, session::Session, store::Store};
use std::path::Path;

pub fn run(cwd: &Path, target: String) -> Result<()> {
    let paths = Paths::new(cwd);
    let session = Session::open_current(paths.clone())?;
    let store = Store::new(&session.dir);
    let pending = store.list_pending()?;
    let ids = crate::cmd_accept::resolve_target_ids(&pending, &target);
    if ids.is_empty() {
        println!("sift: no pending entries match '{target}'");
        return Ok(());
    }
    for id in &ids {
        let entry = store.finalize(id, Status::Reverted)?;
        store.restore_snapshot(&entry, paths.project_root(), &paths, &session.id)?;
    }
    println!("sift: reverted {} entries", ids.len());
    Ok(())
}
