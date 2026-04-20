use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

mod cmd_accept;
mod cmd_diff;
mod cmd_doctor;
mod cmd_export;
mod cmd_fsck;
mod cmd_gc;
mod cmd_history;
mod cmd_init;
mod cmd_list;
mod cmd_log;
mod cmd_mode;
mod cmd_revert;
mod cmd_review;
mod cmd_state;
mod cmd_status;
mod cmd_sweep;

#[derive(Parser)]
#[command(name = "sift", version, about = "git status for AI-generated writes")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Show session status and pending writes (default when no command given).
    Status,
    /// List entries in the current (or specified) session.
    #[command(visible_alias = "ls")]
    List {
        #[arg(long)]
        pending: bool,
        #[arg(long)]
        turn: Option<u32>,
        #[arg(long)]
        session: Option<String>,
        /// Filter entries whose path contains this substring (case-insensitive).
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show the historical ledger for a session.
    Log {
        #[arg(long)]
        session: Option<String>,
        /// Filter entries whose path contains this substring (case-insensitive).
        /// Useful for "what happened to this file across all turns?"
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show a unified diff for a specific entry.
    #[command(visible_alias = "d")]
    Diff { id: String },
    /// Accept pending entries (by id prefix, turn-N, or "all").
    #[command(visible_alias = "ok")]
    Accept { target: String },
    /// Revert pending entries (restores previous file state).
    #[command(visible_alias = "undo")]
    Revert { target: String },
    /// Auto-detect and optionally revert junk entries (dry-run by default).
    Sweep {
        #[arg(long)]
        apply: bool,
    },
    /// Garbage-collect old closed sessions (dry-run by default).
    Gc {
        /// Retention period in days.
        #[arg(long, default_value_t = 7)]
        days: u16,
        /// Actually delete sessions (default is dry-run).
        #[arg(long)]
        apply: bool,
        /// Compact the current session's JSONL files instead of deleting old sessions.
        #[arg(long, conflicts_with_all = ["days", "apply"])]
        compact: bool,
    },
    /// Set the session mode (strict or loose).
    Mode { mode: String },
    /// Launch the TUI sidecar for interactive review.
    Review,
    /// List all past sessions with summary stats.
    History {
        #[arg(long)]
        json: bool,
    },
    /// Open sift review in a new tmux pane (requires tmux).
    Watch,
    /// Wire sift hooks into the current project (or globally).
    Init {
        /// Write hooks to user-level config instead of project-level.
        #[arg(long)]
        global: bool,
        /// Target tool: claude (default), gemini, or cline.
        #[arg(long, default_value = "claude")]
        tool: String,
    },
    /// Report sift version, sibling-tool detection (agx, rgx), and stepwise suite integration status.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Emit the session ledger as schema-stable JSON for downstream consumers
    /// (agx overlay, eval harnesses, third-party tools). Versioned via
    /// `sift_export_version` integer.
    Export {
        #[arg(long)]
        session: Option<String>,
        /// Output format. Currently `json` only; `md` / `patch` / `bundle`
        /// are reserved for Phase 4.3.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Reconstruct the file world at a chosen turn as a path → SHA-1 JSON map.
    /// Compose twice (turns A and B) to diff the file world between any two turns.
    State {
        #[arg(long)]
        session: Option<String>,
        /// Turn boundary (inclusive). Use a large number for "all turns".
        #[arg(long)]
        at_turn: u32,
        /// Include reverted writes when reconstructing state.
        #[arg(long)]
        include_reverted: bool,
        /// Return the baseline (pre-state) map instead of state-at-turn.
        /// Conflicts with --include-reverted (baseline is by definition pre-anything).
        #[arg(long, conflicts_with = "include_reverted")]
        baseline: bool,
        /// Output format. Currently json only; reserved for future formats.
        #[arg(long, default_value = "json")]
        format: String,
    },
    /// Check (or repair) ledger JSONL integrity: truncated tails, duplicate ids, orphan tombstones.
    Fsck {
        /// Session id to check. Defaults to the current session.
        #[arg(long)]
        session: Option<String>,
        /// Archive corrupted files to `.bad.<ulid>` and write cleaned replacements.
        /// Refuses to run on open sessions.
        #[arg(long)]
        repair: bool,
        /// Emit machine-readable JSON instead of the default text report.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<ExitCode> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;
    match cli.command {
        None | Some(Commands::Status) => {
            cmd_status::run(&cwd)?;
        }
        Some(Commands::List {
            pending,
            turn,
            session,
            path,
            json,
        }) => {
            cmd_list::run(&cwd, pending, turn, session, path, json)?;
        }
        Some(Commands::Log {
            session,
            path,
            json,
        }) => {
            cmd_log::run(&cwd, session, path, json)?;
        }
        Some(Commands::Diff { id }) => {
            cmd_diff::run(&cwd, id)?;
        }
        Some(Commands::Accept { target }) => {
            cmd_accept::run(&cwd, target)?;
        }
        Some(Commands::Revert { target }) => {
            cmd_revert::run(&cwd, target)?;
        }
        Some(Commands::Sweep { apply }) => {
            cmd_sweep::run(&cwd, apply)?;
        }
        Some(Commands::Gc {
            days,
            apply,
            compact,
        }) => {
            cmd_gc::run(&cwd, days, apply, compact)?;
        }
        Some(Commands::Mode { mode }) => {
            cmd_mode::run(&cwd, mode)?;
        }
        Some(Commands::Review) => {
            cmd_review::run(&cwd)?;
        }
        Some(Commands::History { json }) => {
            cmd_history::run(&cwd, json)?;
        }
        Some(Commands::Watch) => {
            // Launch `sift review` in a new tmux pane.
            let status = std::process::Command::new("tmux")
                .args(["split-window", "-h", "sift review"])
                .status();
            match status {
                Ok(s) if s.success() => {
                    println!("sift: opened review sidecar in tmux pane");
                }
                Ok(_) => {
                    eprintln!("sift: tmux split-window failed — are you inside a tmux session?");
                }
                Err(_) => {
                    eprintln!("sift: tmux not found — run `sift review` manually in another terminal");
                }
            }
        }
        Some(Commands::Init { global, tool }) => {
            cmd_init::run(&cwd, global, &tool)?;
        }
        Some(Commands::Doctor { json }) => {
            cmd_doctor::run(&cwd, json)?;
        }
        Some(Commands::Export { session, format }) => {
            cmd_export::run(&cwd, session, &format)?;
        }
        Some(Commands::State {
            session,
            at_turn,
            include_reverted,
            baseline,
            format,
        }) => {
            cmd_state::run(&cwd, session, at_turn, include_reverted, baseline, &format)?;
        }
        Some(Commands::Fsck {
            session,
            repair,
            json,
        }) => {
            let code = cmd_fsck::run(&cwd, session, repair, json)?;
            return Ok(ExitCode::from(code));
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
