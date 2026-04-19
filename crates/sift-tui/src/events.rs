//! Keybinding dispatch.
//!
//! Keymap is aligned with `docs/suite-conventions.md` §1 as of v0.4. The
//! v0.3 compatibility alias (`a` still accepts) has been removed; `a` now
//! opens the annotate prompt, matching agx.

use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{App, InputMode};

pub fn handle_key(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match app.input_mode {
        InputMode::Annotating => handle_annotating(app, key),
        InputMode::Searching => handle_searching(app, key),
        InputMode::Normal => handle_normal(app, key),
    }
}

fn handle_normal(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    // Any keypress dismisses a stale one-shot status message. Branches
    // below may re-set it for this same keypress.
    app.status_msg = None;

    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.cursor_down(),
        KeyCode::Char('k') | KeyCode::Up => app.cursor_up(),
        // Accept: suite-conventions §1 primary. `a` no longer accepts as
        // of v0.4 (the v0.3 deprecation window is done).
        KeyCode::Enter | KeyCode::Char(' ') => {
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
        // Annotate. Moved from `n` to `a` in v0.4 per suite-conventions
        // §1 (aligns with agx's annotation key).
        KeyCode::Char('a') => {
            if let Some(e) = app.current() {
                let id = e.id.clone();
                let rationale = e.rationale.clone();
                app.annotating_id = Some(id);
                app.input_buf = rationale;
                app.input_mode = InputMode::Annotating;
            }
        }
        // Search: `/` prompts, `n`/`N` cycle the last query's matches.
        // Adds the conventions §1 search-verbs row.
        KeyCode::Char('/') => {
            app.input_buf.clear();
            app.input_mode = InputMode::Searching;
        }
        // Match guards intentionally call `cycle_search` for its side
        // effect: the cursor moves when the call returns true; when false
        // (no active search), the arm body fires the hint. Behaves
        // identically to a wrapping `if !cycle` inside the arm body and
        // keeps clippy's `collapsible_if` happy.
        KeyCode::Char('n') if !app.cycle_search(1) => {
            app.status_msg =
                Some("no active search — press `/` to search entries".into());
        }
        KeyCode::Char('N') if !app.cycle_search(-1) => {
            app.status_msg =
                Some("no active search — press `/` to search entries".into());
        }
        // Suite-conventions §1 cross-tool key: `t` hands off to agx on the
        // current session's transcript. Feature-detected per §6 rule 2.
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

fn handle_searching(app: &mut App, key: KeyEvent) -> anyhow::Result<()> {
    match key.code {
        KeyCode::Enter => {
            // Commit the query and jump to first match (or surface
            // "no matches" if the query found nothing).
            let q = std::mem::take(&mut app.input_buf);
            if !app.commit_search(&q) && !q.is_empty() {
                app.status_msg = Some(format!("no matches for /{q}"));
            }
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Esc => {
            app.input_buf.clear();
            app.input_mode = InputMode::Normal;
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

    fn seed_entry(session_dir: &std::path::Path, id: &str, path: &str) {
        let entry = LedgerEntry {
            id: id.to_string(),
            turn: 1,
            tool: Tool::Write,
            path: path.into(),
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

    fn assert_accept_key_works(key_code: KeyCode) {
        let td = TempDir::new().unwrap();
        let id = "01HVXK5QZ9G7B2A00000ACCEPT";
        seed_entry(td.path(), id, "src/x.rs");
        let mut app = App::new(td.path()).unwrap();
        assert_eq!(app.entries.len(), 1);

        handle_key(&mut app, key(key_code)).unwrap();

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
    fn a_no_longer_accepts_in_v04() {
        // Verifies the v0.3 deprecation window is closed: pressing `a`
        // now opens the annotation prompt, not the accept path.
        let td = TempDir::new().unwrap();
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000NOTACC", "src/x.rs");
        let mut app = App::new(td.path()).unwrap();
        handle_key(&mut app, key(KeyCode::Char('a'))).unwrap();
        // Still pending (not accepted), and we're now in Annotating mode.
        assert_eq!(app.entries.len(), 1);
        assert_eq!(app.input_mode, InputMode::Annotating);
    }

    #[test]
    fn a_opens_annotation_prompt() {
        let td = TempDir::new().unwrap();
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000ANNOTT", "src/x.rs");
        let mut app = App::new(td.path()).unwrap();
        handle_key(&mut app, key(KeyCode::Char('a'))).unwrap();
        assert_eq!(app.input_mode, InputMode::Annotating);
        assert!(app.annotating_id.is_some());
    }

    #[test]
    fn revert_still_works() {
        let td = TempDir::new().unwrap();
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000REVERT", "src/x.rs");
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
    fn slash_enters_search_mode() {
        let td = TempDir::new().unwrap();
        let mut app = App::new(td.path()).unwrap();
        handle_key(&mut app, key(KeyCode::Char('/'))).unwrap();
        assert_eq!(app.input_mode, InputMode::Searching);
        assert!(app.input_buf.is_empty());
    }

    #[test]
    fn search_commit_jumps_to_first_match() {
        let td = TempDir::new().unwrap();
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000000001", "src/a.rs");
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000000002", "tests/b.rs");
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000000003", "src/c.rs");
        let mut app = App::new(td.path()).unwrap();
        assert_eq!(app.cursor, 0);

        // Simulate "/tests<Enter>".
        handle_key(&mut app, key(KeyCode::Char('/'))).unwrap();
        for c in "tests".chars() {
            handle_key(&mut app, key(KeyCode::Char(c))).unwrap();
        }
        handle_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.cursor, 1, "cursor should jump to tests/b.rs at index 1");
        assert_eq!(app.search_matches, vec![1]);
    }

    #[test]
    fn n_and_shift_n_cycle_matches_after_search() {
        let td = TempDir::new().unwrap();
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000000001", "src/one.rs");
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000000002", "src/two.rs");
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000000003", "docs/three.md");
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000000004", "src/four.rs");
        let mut app = App::new(td.path()).unwrap();

        // Search for "src" → matches at indices 0, 1, 3.
        handle_key(&mut app, key(KeyCode::Char('/'))).unwrap();
        for c in "src".chars() {
            handle_key(&mut app, key(KeyCode::Char(c))).unwrap();
        }
        handle_key(&mut app, key(KeyCode::Enter)).unwrap();
        assert_eq!(app.cursor, 0);
        assert_eq!(app.search_matches, vec![0, 1, 3]);

        handle_key(&mut app, key(KeyCode::Char('n'))).unwrap();
        assert_eq!(app.cursor, 1);
        handle_key(&mut app, key(KeyCode::Char('n'))).unwrap();
        assert_eq!(app.cursor, 3);
        handle_key(&mut app, key(KeyCode::Char('n'))).unwrap();
        assert_eq!(app.cursor, 0, "wraps around to first match");

        handle_key(&mut app, key(KeyCode::Char('N'))).unwrap();
        assert_eq!(app.cursor, 3, "shift-N wraps backward");
    }

    #[test]
    fn n_without_active_search_surfaces_hint() {
        let td = TempDir::new().unwrap();
        let mut app = App::new(td.path()).unwrap();
        handle_key(&mut app, key(KeyCode::Char('n'))).unwrap();
        assert!(app
            .status_msg
            .as_deref()
            .map(|m| m.contains("no active search"))
            .unwrap_or(false));
    }

    #[test]
    fn search_escape_cancels_without_jumping() {
        let td = TempDir::new().unwrap();
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000000001", "src/a.rs");
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000000002", "src/b.rs");
        let mut app = App::new(td.path()).unwrap();
        app.cursor = 1; // start pointing at b.rs

        handle_key(&mut app, key(KeyCode::Char('/'))).unwrap();
        for c in "a.rs".chars() {
            handle_key(&mut app, key(KeyCode::Char(c))).unwrap();
        }
        handle_key(&mut app, key(KeyCode::Esc)).unwrap();

        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.cursor, 1, "Esc must not move cursor");
        assert!(app.search_matches.is_empty());
    }

    #[test]
    fn no_match_query_surfaces_status_msg() {
        let td = TempDir::new().unwrap();
        seed_entry(td.path(), "01HVXK5QZ9G7B2A00000000001", "src/a.rs");
        let mut app = App::new(td.path()).unwrap();

        handle_key(&mut app, key(KeyCode::Char('/'))).unwrap();
        for c in "zzzz".chars() {
            handle_key(&mut app, key(KeyCode::Char(c))).unwrap();
        }
        handle_key(&mut app, key(KeyCode::Enter)).unwrap();

        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.cursor, 0);
        assert!(app
            .status_msg
            .as_deref()
            .map(|m| m.contains("no matches"))
            .unwrap_or(false));
    }

    #[test]
    fn t_key_surfaces_missing_transcript_when_agx_present() {
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
        handle_key(&mut app, key(KeyCode::Char('j'))).unwrap();
        assert!(app.status_msg.is_none());
    }
}
