//! Stop hook handler: close the session and print a one-line summary.

use anyhow::Result;
use sift_core::{paths::Paths, session::Session, store::Store};

use crate::payload::HookEvent;

pub fn run(event: HookEvent) -> Result<()> {
    let paths = Paths::new(event.project_root());
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
    let (accepted, reverted) = sift_core::entry::tally(&ledger);
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
