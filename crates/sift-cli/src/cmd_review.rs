use anyhow::Result;
use sift_core::{paths::Paths, session::Session};
use std::path::Path;

pub fn run(cwd: &Path) -> Result<()> {
    let paths = Paths::new(cwd);
    let session = Session::open_current(paths)?;
    sift_tui::run(&session.dir)
}
