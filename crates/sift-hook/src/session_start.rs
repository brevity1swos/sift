//! SessionStart hook handler: mint a fresh session directory.

use anyhow::Result;
use sift_core::{paths::Paths, session::Session};
use std::path::PathBuf;

use crate::payload::HookEvent;

pub fn run(event: HookEvent) -> Result<()> {
    let project_root = event.cwd.unwrap_or_else(|| PathBuf::from("."));
    let paths = Paths::new(project_root);
    let _session = Session::create(paths)?;
    Ok(())
}
