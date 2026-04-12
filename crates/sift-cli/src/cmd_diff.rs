use anyhow::{anyhow, Context, Result};
use sift_core::{
    diff::unified, paths::Paths, session::Session, snapshot::SnapshotStore, store::Store,
};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn run(cwd: &Path, entry_id: String) -> Result<()> {
    let paths = Paths::new(cwd);
    let session = Session::open_current(Paths::new(cwd))?;
    let store = Store::new(&session.dir);
    let mut all = store.list_pending()?;
    all.extend(store.list_ledger()?);
    let entry = all
        .into_iter()
        .find(|e| e.id.starts_with(&entry_id))
        .ok_or_else(|| anyhow!("no entry matches id prefix {entry_id}"))?;

    let snap = SnapshotStore::new(&paths, &session.id);
    let before = match &entry.snapshot_before {
        Some(h) => String::from_utf8_lossy(&snap.get(h)?).into_owned(),
        None => String::new(),
    };
    let after = match &entry.snapshot_after {
        Some(h) => String::from_utf8_lossy(&snap.get(h)?).into_owned(),
        None => String::new(),
    };
    if before.is_empty() && after.is_empty() {
        anyhow::bail!("entry has no snapshots to diff");
    }

    let diff_output = unified(&before, &after, 3);
    page_output(&diff_output)
}

/// If stdout is a terminal and the output is taller than the terminal,
/// pipe through $PAGER (defaults to `less`). Otherwise print directly.
fn page_output(text: &str) -> Result<()> {
    use std::io::IsTerminal;

    if !std::io::stdout().is_terminal() {
        // Piped to another command — just print.
        print!("{text}");
        return Ok(());
    }

    // Check if output fits the terminal height.
    let term_height = terminal_height().unwrap_or(24);
    let line_count = text.lines().count();
    if line_count <= term_height {
        print!("{text}");
        return Ok(());
    }

    // Pipe through pager.
    let pager = std::env::var("PAGER").unwrap_or_else(|_| "less".to_string());
    let mut child = Command::new(&pager)
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("launching pager '{pager}'"))?;

    if let Some(mut stdin) = child.stdin.take() {
        // Ignore broken-pipe errors (user quit the pager early).
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
    Ok(())
}

fn terminal_height() -> Option<usize> {
    // Use crossterm to query terminal size (already a workspace dep).
    crossterm::terminal::size().ok().map(|(_, h)| h as usize)
}

