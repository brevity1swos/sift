//! `sift ai-help` — emit the agent-facing command cookbook to stdout.
//!
//! The content is a crate-local copy of the agent guide embedded at
//! build time via `include_str!`, so the guide ships inside the binary
//! with no file dependency at runtime. An agent discovering sift in a
//! new project can run `sift ai-help` to learn the command cookbook
//! without having to resolve a path to the repo.
//!
//! `agent-guide.md` must live inside this crate (not the repo-root
//! `docs/`) so `cargo publish` bundles it into the package tarball. The
//! repo-root `docs/agent-guide.md` stays canonical for web links; keep
//! the two in sync.

use anyhow::Result;

const AGENT_GUIDE: &str = include_str!("../agent-guide.md");

pub fn run() -> Result<()> {
    // Raw dump — no pager, no paging logic. Agents parse this;
    // human users who want paging can `sift ai-help | less`.
    print!("{AGENT_GUIDE}");
    Ok(())
}
