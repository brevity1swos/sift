use anyhow::{bail, Result};

mod payload;
mod session_start;
mod stop;

fn main() -> Result<()> {
    let arg = std::env::args().nth(1).unwrap_or_default();
    // Read stdin even if the subcommand doesn't need it, so Claude Code's
    // hook runner doesn't block on an unread pipe. A missing/empty stdin
    // falls back to an empty event.
    let event = payload::read_from_stdin().unwrap_or_default();
    match arg.as_str() {
        "session-start" => session_start::run(event),
        "stop" => stop::run(event),
        other => bail!("unknown subcommand: {other}"),
    }
}
