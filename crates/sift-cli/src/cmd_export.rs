//! `sift export --format json` — emit the session ledger as the
//! schema-stable JSON contract that downstream consumers (agx, eval
//! scripts, third-party tooling) read.
//!
//! Other formats (md, patch, bundle) are reserved for Phase 4.3; only
//! `json` is implemented in v0.5.

use anyhow::{Context, Result};
use sift_core::export;
use sift_core::session::SessionMeta;
use sift_core::store::Store;
use std::fs;
use std::path::Path;

pub fn run(cwd: &Path, session: Option<String>, format: &str) -> Result<()> {
    let dir = crate::resolve_session_dir(cwd, session)?;

    match format {
        "json" => emit_json(&dir),
        "md" | "patch" | "bundle" => {
            anyhow::bail!(
                "format '{format}' is reserved for Phase 4.3; only 'json' is supported in v0.5"
            )
        }
        other => anyhow::bail!("unknown format '{other}'; expected 'json'"),
    }
}

fn emit_json(session_dir: &Path) -> Result<()> {
    let meta_path = session_dir.join("meta.json");
    let meta_text = fs::read_to_string(&meta_path)
        .with_context(|| format!("reading {}", meta_path.display()))?;
    let meta: SessionMeta = serde_json::from_str(&meta_text)
        .with_context(|| format!("parsing {}", meta_path.display()))?;

    let store = Store::new(session_dir);
    let mut entries = store.list_pending()?;
    entries.extend(store.list_ledger()?);

    let export = export::build(&meta, entries);
    println!("{}", serde_json::to_string_pretty(&export)?);
    Ok(())
}
