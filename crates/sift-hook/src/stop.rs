//! Stop hook handler: close the session and print a one-line summary.

use anyhow::Result;
use sift_core::{entry::Status, paths::Paths, session::Session, store::Store};
use std::path::PathBuf;

use crate::payload::HookEvent;

pub fn run(event: HookEvent) -> Result<()> {
    let project_root = event.cwd.unwrap_or_else(|| PathBuf::from("."));
    let paths = Paths::new(project_root);
    let session = match Session::open_current(paths) {
        Ok(s) => s,
        Err(_) => return Ok(()), // no active session, nothing to close
    };
    session.close()?;
    let store = Store::new(&session.dir);
    let pending = store.list_pending().unwrap_or_default().len();
    let ledger = store.list_ledger().unwrap_or_default();
    let accepted = ledger.iter().filter(|e| e.status == Status::Accepted).count();
    let reverted = ledger.iter().filter(|e| e.status == Status::Reverted).count();
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
