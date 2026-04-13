//! Keybinding dispatch.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, InputMode};

pub fn handle_key(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match app.input_mode {
        InputMode::Annotating => handle_annotating(app, key),
        InputMode::Normal => handle_normal(app, key),
    }
}

fn handle_normal(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
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
            if let Some(e) = app.current() {
                app.edit_request = Some(e.id.clone());
            }
        }
        KeyCode::Char('n') => {
            // Enter annotation mode for the current entry.
            // Clone values before mutating app to satisfy the borrow checker.
            if let Some(e) = app.current() {
                let id = e.id.clone();
                let rationale = e.rationale.clone();
                app.annotating_id = Some(id);
                app.input_buf = rationale;
                app.input_mode = InputMode::Annotating;
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_annotating(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        KeyCode::Enter => {
            // Save the annotation to pending.jsonl.
            if let Some(ref id) = app.annotating_id {
                let store = sift_core::store::Store::new(&app.session_dir);
                let mut pending = store.list_pending()?;
                if let Some(entry) = pending.iter_mut().find(|e| e.id == *id) {
                    entry.rationale = app.input_buf.clone();
                }
                store.rewrite_pending_entries(&pending)?;
            }
            app.input_mode = InputMode::Normal;
            app.input_buf.clear();
            app.annotating_id = None;
            app.reload()?;
        }
        KeyCode::Esc => {
            // Cancel — discard input.
            app.input_mode = InputMode::Normal;
            app.input_buf.clear();
            app.annotating_id = None;
        }
        KeyCode::Backspace => {
            app.input_buf.pop();
        }
        KeyCode::Char(c) => {
            app.input_buf.push(c);
        }
        _ => {}
    }
    Ok(())
}
