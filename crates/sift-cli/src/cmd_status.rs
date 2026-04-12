use anyhow::Result;
use sift_core::{
    config::{Config, Mode},
    paths::Paths,
    session::{Session, SessionMeta},
    state::SessionState,
    store::Store,
};
use std::fs;
use std::path::Path;

pub fn run(cwd: &Path) -> Result<()> {
    let paths = Paths::new(cwd);

    // No session?
    if paths.current_symlink().symlink_metadata().is_err() {
        println!("sift: no active session");
        println!();
        println!("  Start one by opening a Claude Code session in a project");
        println!("  with sift hooks configured in .claude/settings.json.");
        return Ok(());
    }

    let session = Session::open_current(paths.clone())?;
    let config = Config::load(&paths.config_file()).unwrap_or_default();
    let state = SessionState::load(&session.state_path()).unwrap_or_default();

    let mode_str = match config.mode {
        Mode::Loose => "loose",
        Mode::Strict => "strict",
    };

    // Read meta for the session id
    let meta: Option<SessionMeta> = fs::read_to_string(session.meta_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let session_id = meta
        .as_ref()
        .map(|m| m.id.as_str())
        .unwrap_or(&session.id);

    println!(
        "sift: session {} · turn {} · {} mode",
        session_id, state.turn, mode_str
    );

    let store = Store::new(&session.dir);
    let pending = store.list_pending().unwrap_or_default();
    let ledger = store.list_ledger().unwrap_or_default();

    if pending.is_empty() && ledger.is_empty() {
        println!();
        println!("  No writes captured yet this session.");
    } else {
        if !pending.is_empty() {
            println!();
            println!("Pending ({}):", pending.len());
            for e in &pending {
                println!(
                    "  {:?}  {}   +{} -{}",
                    e.op,
                    e.path.display(),
                    e.diff_stats.added,
                    e.diff_stats.removed,
                );
            }
        }

        if !ledger.is_empty() {
            let accepted = ledger
                .iter()
                .filter(|e| e.status == sift_core::Status::Accepted)
                .count();
            let reverted = ledger
                .iter()
                .filter(|e| e.status == sift_core::Status::Reverted)
                .count();
            println!();
            println!(
                "Ledger: {} accepted, {} reverted",
                accepted, reverted
            );
        }
    }

    if !pending.is_empty() {
        println!();
        println!("  sift ok           accept all pending");
        println!("  sift undo         revert all pending");
        println!("  sift d <id>       show diff for an entry");
        println!("  sift ls           list pending entries");
    }

    Ok(())
}
