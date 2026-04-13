//! Keybinding dispatch.
//!
//! v0.1 limitation: `r` (revert) marks the entry as Reverted in the ledger
//! but does NOT restore the file on disk. File restoration requires `Paths`
//! and `session_id`, which the TUI does not currently carry. Use `sift revert
//! <id>` from the CLI for a full on-disk revert.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::App;

pub fn handle_key(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.cursor_down(),
        KeyCode::Char('k') | KeyCode::Up => app.cursor_up(),
        KeyCode::Char('a') => {
            if let Some(e) = app.current() {
                let id = e.id.clone();
                let store = sift_core::store::Store::new(&app.session_dir);
                store.finalize(&id, sift_core::Status::Accepted)?;
                app.reload()?;
            }
        }
        KeyCode::Char('r') => {
            if let Some(e) = app.current() {
                let id = e.id.clone();
                let store = sift_core::store::Store::new(&app.session_dir);
                store.finalize(&id, sift_core::Status::Reverted)?;
                app.reload()?;
            }
        }
        KeyCode::Char('e') => {
            // Request edit — the main loop will suspend the TUI, spawn
            // $EDITOR on the post-state snapshot, and resume after.
            if let Some(e) = app.current() {
                app.edit_request = Some(e.id.clone());
            }
        }
        _ => {}
    }
    Ok(())
}
