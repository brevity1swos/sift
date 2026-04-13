//! sift-tui: ratatui sidecar for `sift review`.

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use sift_core::{
    entry::Status,
    paths::Paths,
    snapshot::SnapshotStore,
    store::Store,
};
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub mod app;
pub mod events;
pub mod ui;

pub fn run(session_dir: &Path) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = app::App::new(session_dir)?;
    let tick_rate = Duration::from_millis(200);

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                events::handle_key(&mut app, key)?;
            }
        }

        // Handle edit request: suspend TUI → spawn $EDITOR → resume.
        if let Some(entry_id) = app.edit_request.take() {
            handle_edit(&mut terminal, &mut app, &entry_id)?;
        }

        app.reload()?;
        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Suspend the TUI, open the entry's post-state in $EDITOR, process the
/// result, and resume the TUI.
fn handle_edit<B: ratatui::backend::Backend + Write>(
    terminal: &mut Terminal<B>,
    app: &mut app::App,
    entry_id: &str,
) -> Result<()> {
    let store = Store::new(&app.session_dir);
    let project_root = app.project_root();
    let session_id = app.session_id();
    let paths = Paths::new(&project_root);

    // Find the entry in pending.
    let entry = app
        .entries
        .iter()
        .find(|e| e.id == entry_id)
        .cloned();
    let Some(entry) = entry else { return Ok(()) };
    let Some(ref after_hash) = entry.snapshot_after else {
        return Ok(()); // nothing to edit (delete op)
    };

    // Read the post-state blob.
    let snap = SnapshotStore::new(&paths, &session_id);
    let content = snap.get(after_hash)?;

    // Determine file extension for editor syntax highlighting.
    let ext = entry
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("txt");
    let tmp_path = std::env::temp_dir().join(format!("sift-edit-{}.{ext}", &entry_id[..8.min(entry_id.len())]));
    std::fs::write(&tmp_path, &content)
        .with_context(|| format!("writing temp file {}", tmp_path.display()))?;

    // Suspend TUI.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Spawn $EDITOR.
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = Command::new(&editor)
        .arg(&tmp_path)
        .status()
        .with_context(|| format!("launching editor '{editor}'"))?;

    // Resume TUI.
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.hide_cursor()?;
    terminal.clear()?;

    if !status.success() {
        // Editor exited with error — skip, don't modify anything.
        let _ = std::fs::remove_file(&tmp_path);
        return Ok(());
    }

    // Read edited content.
    let edited = std::fs::read(&tmp_path)
        .with_context(|| format!("reading edited file {}", tmp_path.display()))?;
    let _ = std::fs::remove_file(&tmp_path);

    // If unchanged, just accept as-is.
    if edited == content {
        store.finalize(&entry.id, Status::Accepted)?;
        app.reload()?;
        return Ok(());
    }

    // Content changed — write edited version to the real project file.
    let target = project_root.join(&entry.path);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, &edited)
        .with_context(|| format!("writing edited content to {}", target.display()))?;

    // Store the new snapshot and update the entry.
    let new_hash = snap.put(&edited)?;

    // Finalize as "edited" — we need to update snapshot_after before finalizing.
    // Read pending, find entry, update hash, rewrite, then finalize.
    let mut pending = store.list_pending()?;
    if let Some(e) = pending.iter_mut().find(|e| e.id == entry.id) {
        e.snapshot_after = Some(new_hash);
    }
    // Rewrite pending with updated hash, then finalize.
    store.rewrite_pending_entries(&pending)?;
    store.finalize(&entry.id, Status::Edited)?;
    app.reload()?;
    Ok(())
}
