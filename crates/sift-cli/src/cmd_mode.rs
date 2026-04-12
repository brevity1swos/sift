use anyhow::{bail, Result};
use sift_core::{
    config::{Config, Mode},
    paths::Paths,
};
use std::path::Path;

pub fn run(cwd: &Path, mode_str: String) -> Result<()> {
    let paths = Paths::new(cwd);
    let mut config = Config::load(&paths.config_file())?;
    config.mode = match mode_str.as_str() {
        "strict" => Mode::Strict,
        "loose" => Mode::Loose,
        other => bail!("unknown mode: {other} (expected 'strict' or 'loose')"),
    };
    config.save(&paths.config_file())?;
    println!("sift: mode set to {mode_str}");
    Ok(())
}
