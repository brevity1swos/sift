//! UserPromptSubmit hook handler.
//!
//! Behavior:
//! - Load current session + config.
//! - If mode=Strict and pending.jsonl has entries, write a blocking message
//!   to stderr and exit with code 2 (Claude Code's documented block-prompt code).
//! - Otherwise bump the session turn counter and exit 0.

use anyhow::Result;
use sift_core::{
    config::{Config, Mode},
    paths::Paths,
    session::Session,
    state::SessionState,
    store::Store,
};
use std::path::PathBuf;
use std::process::ExitCode;

use crate::payload::HookEvent;

pub fn run(event: HookEvent) -> Result<ExitCode> {
    let project_root = event.cwd.unwrap_or_else(|| PathBuf::from("."));
    let paths = Paths::new(project_root);

    // No current session → nothing to gate. Let the prompt through.
    if paths.current_symlink().symlink_metadata().is_err() {
        return Ok(ExitCode::from(0));
    }
    let session = Session::open_current(paths.clone())?;
    let config = Config::load(&paths.config_file())?;
    let store = Store::new(&session.dir);
    let pending = store.list_pending()?;

    if config.mode == Mode::Strict && !pending.is_empty() {
        eprintln!(
            "sift: {} pending write(s) from the previous turn must be cleared before continuing.\n\
             Review in the sidecar (`sift review`), or run `sift accept all` / `sift revert <id>`.\n\
             To proceed anyway, switch mode with `sift mode loose`.",
            pending.len()
        );
        return Ok(ExitCode::from(2));
    }

    let mut state = SessionState::load(&session.state_path())?;
    state.bump_turn();
    state.save(&session.state_path())?;
    Ok(ExitCode::from(0))
}
