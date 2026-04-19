//! SessionStart hook handler: mint a fresh session directory.

use anyhow::Result;
use sift_core::{paths::Paths, session::Session};
use std::path::PathBuf;

use crate::payload::HookEvent;

pub fn run(event: HookEvent) -> Result<()> {
    let project_root = event.cwd.unwrap_or_else(|| PathBuf::from("."));
    let paths = Paths::new(project_root);
    // Record the host agent's transcript path so `sift review` can hand off
    // to agx on the same session via the `t` keybind (suite-conventions §5).
    // Absent transcript_path is legal — degrades the `t` keybind, nothing else.
    let _session = Session::create_with_transcript(paths, event.transcript_path)?;
    Ok(())
}
