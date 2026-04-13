use anyhow::{bail, Result};
use std::process::ExitCode;

mod payload;
mod post_tool;
mod pre_tool;
mod session_start;
mod stop;
mod user_prompt;

fn main() -> Result<ExitCode> {
    let arg = std::env::args().nth(1).unwrap_or_default();
    let event = payload::read_from_stdin().unwrap_or_default();
    match arg.as_str() {
        "session-start" => {
            session_start::run(event)?;
            Ok(ExitCode::from(0))
        }
        "stop" => {
            stop::run(event)?;
            Ok(ExitCode::from(0))
        }
        "user-prompt" => user_prompt::run(event),
        "pre-tool" => pre_tool::run(event),
        "post-tool" => {
            post_tool::run(event)?;
            Ok(ExitCode::from(0))
        }
        other => bail!("unknown subcommand: {other}"),
    }
}
