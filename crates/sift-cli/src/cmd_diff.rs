use anyhow::{anyhow, Result};
use sift_core::{
    diff::unified,
    paths::Paths,
    session::Session,
    snapshot::SnapshotStore,
    store::Store,
};
use std::path::Path;

pub fn run(cwd: &Path, entry_id: String) -> Result<()> {
    let paths = Paths::new(cwd);
    let session = Session::open_current(paths.clone())?;
    let store = Store::new(&session.dir);
    let mut all = store.list_pending()?;
    all.extend(store.list_ledger()?);
    let entry = all
        .into_iter()
        .find(|e| e.id.starts_with(&entry_id))
        .ok_or_else(|| anyhow!("no entry matches id prefix {entry_id}"))?;

    let snap = SnapshotStore::new(&paths, &session.id);
    let before = match &entry.snapshot_before {
        Some(h) => String::from_utf8_lossy(&snap.get(h)?).into_owned(),
        None => String::new(),
    };
    let after = match &entry.snapshot_after {
        Some(h) => String::from_utf8_lossy(&snap.get(h)?).into_owned(),
        None => String::new(),
    };
    if before.is_empty() && after.is_empty() {
        anyhow::bail!("entry has no snapshots to diff");
    }
    print!("{}", unified(&before, &after, 3));
    Ok(())
}
