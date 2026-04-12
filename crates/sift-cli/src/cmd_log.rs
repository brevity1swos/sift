use anyhow::Result;
use sift_core::store::Store;
use std::path::Path;

pub fn run(cwd: &Path, session: Option<String>, json: bool) -> Result<()> {
    let dir = crate::resolve_session_dir(cwd, session)?;
    let store = Store::new(&dir);
    let ledger = store.list_ledger()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&ledger)?);
    } else {
        for e in &ledger {
            println!(
                "{} {} {:?} {} +{} -{}",
                &e.id[..8.min(e.id.len())],
                e.timestamp,
                e.status,
                e.path.display(),
                e.diff_stats.added,
                e.diff_stats.removed,
            );
        }
    }
    Ok(())
}
