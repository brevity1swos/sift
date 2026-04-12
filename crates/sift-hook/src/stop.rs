//! Stop hook handler: close the session and print a one-line summary.

use anyhow::Result;
use sift_core::{entry::Status, paths::Paths, session::Session, store::Store};
use std::path::PathBuf;

use crate::payload::HookEvent;

pub fn run(event: HookEvent) -> Result<()> {
    let project_root = event.cwd.unwrap_or_else(|| PathBuf::from("."));
    let paths = Paths::new(project_root);
    // Distinguish "no active session" (common: Claude started outside a
    // sift-enabled project) from "corrupted session state" (unusual but
    // worth surfacing). Only suppress the error when the symlink is
    // genuinely absent; propagate all other failures.
    if paths.current_symlink().symlink_metadata().is_err() {
        return Ok(());
    }
    let session = Session::open_current(paths)?;
    session.close()?;
    let store = Store::new(&session.dir);
    let pending = store.list_pending()?.len();
    let ledger = store.list_ledger()?;
    let accepted = ledger
        .iter()
        .filter(|e| e.status == Status::Accepted)
        .count();
    let reverted = ledger
        .iter()
        .filter(|e| e.status == Status::Reverted)
        .count();
    let total = ledger.len() + pending;
    eprintln!(
        "sift: {} writes · {} accepted · {} reverted · {} pending · {}",
        total,
        accepted,
        reverted,
        pending,
        session.dir.display()
    );
    Ok(())
}
