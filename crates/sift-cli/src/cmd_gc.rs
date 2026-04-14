use anyhow::Result;
use chrono::Duration;
use sift_core::store::Store;
use sift_core::{gc, paths::Paths};
use std::path::Path;

pub fn run(cwd: &Path, days: u16, apply: bool, compact: bool) -> Result<()> {
    if compact {
        return run_compact(cwd);
    }

    // `apply` means "actually delete"; `dry_run` is its inverse. We invert here
    // (rather than at the call site) so the CLI handler stays readable.
    let dry_run = !apply;
    let paths = Paths::new(cwd);
    let result = gc::collect(&paths, Duration::days(i64::from(days)), dry_run)?;

    if result.deleted.is_empty() {
        println!("sift gc: nothing to collect");
    } else {
        let verb = if dry_run { "would delete" } else { "deleted" };
        for id in &result.deleted {
            println!("  {verb} session {id}");
        }
        println!("sift gc: {verb} {} session(s)", result.deleted.len());
    }

    if result.skipped_open > 0 {
        println!("  skipped {} open session(s)", result.skipped_open);
    }
    if result.skipped_corrupt > 0 {
        println!("  skipped {} corrupt session(s)", result.skipped_corrupt);
    }

    Ok(())
}

fn run_compact(cwd: &Path) -> Result<()> {
    let session_dir = crate::resolve_session_dir(cwd, None)?;
    let store = Store::new(&session_dir);
    store.compact_pending()?;
    store.compact_ledger()?;
    println!("sift gc: compacted current session");
    Ok(())
}
