use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

mod cmd_list;
mod cmd_log;

#[derive(Parser)]
#[command(name = "sift", version, about = "git status for AI-generated writes")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List entries in the current (or specified) session.
    List {
        #[arg(long)]
        pending: bool,
        #[arg(long)]
        turn: Option<u32>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show the historical ledger for a session.
    Log {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;
    match cli.command {
        Commands::List { pending, turn, session, json } => {
            cmd_list::run(&cwd, pending, turn, session, json)?;
        }
        Commands::Log { session, json } => {
            cmd_log::run(&cwd, session, json)?;
        }
    }
    Ok(ExitCode::from(0))
}

/// Resolve the session directory from a session id string or the `current` symlink.
pub fn resolve_session_dir(
    cwd: &std::path::Path,
    session: Option<String>,
) -> anyhow::Result<PathBuf> {
    use sift_core::paths::Paths;
    let paths = Paths::new(cwd);
    match session {
        Some(id) => Ok(paths.session_dir(&id)),
        None => {
            let link = paths.current_symlink();
            let target = std::fs::read_link(&link)
                .map_err(|e| anyhow::anyhow!("no current session ({link:?}): {e}"))?;
            Ok(if target.is_absolute() {
                target
            } else {
                paths.sift_dir().join(target)
            })
        }
    }
}
