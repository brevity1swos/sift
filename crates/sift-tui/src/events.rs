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
    // Any keypress dismisses a stale one-shot status message. The `t`
    // branch below may re-set it for this same keypress; all other
    // branches leave it cleared.
    app.status_msg = None;

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.cursor_down(),
        KeyCode::Char('k') | KeyCode::Up => app.cursor_up(),
        // Accept. `Enter` and `Space` are the suite-conventions §1
        // primaries; `a` is retained for one release as a non-breaking
        // compatibility alias. In v0.4 the full keymap migration moves
        // `a` to annotate — see docs/superpowers/plans/
        // 2026-04-19-phase1-agx-synergy.md Task C1.
        KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('a') => {
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
        // Suite-conventions §1 cross-tool key: `t` hands off to agx on the
        // current session's transcript. Feature-detected per §6 rule 2 —
        // if agx is missing or the transcript wasn't recorded (pre-v0.3
        // session), set a status message instead of crashing.
        KeyCode::Char('t') => {
            if sift_core::agx::detect().is_none() {
                app.status_msg = Some(
                    "agx not on PATH — install from https://github.com/brevity1swos/agx"
                        .to_string(),
                );
            } else if app.transcript_path().is_none() {
                app.status_msg = Some(
                    "no agent transcript recorded — start a new session in v0.3+ to enable `t`"
                        .to_string(),
                );
            } else {
                // Main loop picks this up and suspends the TUI to run agx.
                app.jump_to_agx_request = true;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use chrono::Utc;
    use crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};
    use sift_core::entry::{DiffStats, LedgerEntry, Op, Status, Tool};
    use sift_core::store::Store;
    use tempfile::TempDir;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn seed_pending(session_dir: &std::path::Path, id: &str) {
        let entry = LedgerEntry {
            id: id.to_string(),
            turn: 1,
            tool: Tool::Write,
            path: "src/x.rs".into(),
            op: Op::Create,
            rationale: String::new(),
            diff_stats: DiffStats {
                added: 1,
                removed: 0,
            },
            snapshot_before: None,
            snapshot_after: Some("a".repeat(40)),
            status: Status::Pending,
            timestamp: Utc::now(),
        };
        Store::new(session_dir).append_pending(&entry).unwrap();
    }

    /// Verify that pressing `key_code` on a pending entry finalizes it to
    /// Accepted. Used to exercise each of the three accept-equivalent keys
    /// (Enter, Space, 'a') per suite-conventions §1 + §10 retrofit.
    fn assert_accept_key_works(key_code: KeyCode) {
        let td = TempDir::new().unwrap();
        let id = "01HVXK5QZ9G7B2A00000ACCEPT";
        seed_pending(td.path(), id);
        let mut app = App::new(td.path()).unwrap();
        assert_eq!(app.entries.len(), 1);

        handle_key(&mut app, key(key_code)).unwrap();

        // After accept, pending is empty and the entry is in the ledger.
        assert_eq!(app.entries.len(), 0, "pending should be empty after accept");
        let ledger = Store::new(td.path()).list_ledger().unwrap();
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0].status, Status::Accepted);
    }

    #[test]
    fn enter_accepts_current_entry() {
        assert_accept_key_works(KeyCode::Enter);
    }

    #[test]
    fn space_accepts_current_entry() {
        assert_accept_key_works(KeyCode::Char(' '));
    }

    #[test]
    fn legacy_a_still_accepts_current_entry() {
        // During the v0.3 migration window, `a` remains bound to accept for
        // compatibility. Remove this test when v0.4 flips `a` to annotate.
        assert_accept_key_works(KeyCode::Char('a'));
    }

    #[test]
    fn revert_still_works() {
        let td = TempDir::new().unwrap();
        seed_pending(td.path(), "01HVXK5QZ9G7B2A00000REVERT");
        let mut app = App::new(td.path()).unwrap();
        handle_key(&mut app, key(KeyCode::Char('r'))).unwrap();
        assert_eq!(app.entries.len(), 0);
        let ledger = Store::new(td.path()).list_ledger().unwrap();
        assert_eq!(ledger[0].status, Status::Reverted);
    }

    #[test]
    fn quit_sets_should_quit_flag() {
        let td = TempDir::new().unwrap();
        let mut app = App::new(td.path()).unwrap();
        assert!(!app.should_quit);
        handle_key(&mut app, key(KeyCode::Char('q'))).unwrap();
        assert!(app.should_quit);
    }

    #[test]
    fn t_key_surfaces_missing_transcript_when_agx_present() {
        // If agx is on PATH but meta.json has no transcript_path, the `t`
        // key should set a helpful status message, not crash or blindly
        // spawn. We can only exercise this branch on a machine where
        // agx::detect() returns Some — otherwise the earlier "agx not on
        // PATH" branch wins. Accept either outcome; both are correct
        // graceful-degrade paths per suite-conventions §6 rule 2.
        let td = TempDir::new().unwrap();
        let mut app = App::new(td.path()).unwrap();
        handle_key(&mut app, key(KeyCode::Char('t'))).unwrap();

        assert!(!app.jump_to_agx_request, "no transcript => no jump");
        let msg = app
            .status_msg
            .as_ref()
            .expect("t keypress without agx + transcript must surface a status message");
        assert!(
            msg.contains("agx not on PATH") || msg.contains("no agent transcript"),
            "got unexpected status msg: {msg}"
        );
    }

    #[test]
    fn next_keypress_clears_stale_status_msg() {
        let td = TempDir::new().unwrap();
        let mut app = App::new(td.path()).unwrap();
        app.status_msg = Some("old hint".into());
        // `j` (cursor-down) is not `t` and not anything that sets a new
        // message; it should reach the catch-all arm and clear the hint.
        handle_key(&mut app, key(KeyCode::Char('j'))).unwrap();
        assert!(app.status_msg.is_none());
    }
}
