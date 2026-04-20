//! `sift state --at-turn N` — return the file world at a chosen turn
//! as a `path → SHA-1-hex` JSON map. The Phase 1.7 diff primitive: any
//! two turns A and B can be diffed by composing this twice and comparing
//! the resulting maps (e.g.
//! `diff <(sift state --at-turn 5) <(sift state --at-turn 8)`).

use anyhow::Result;
use sift_core::store::Store;
use sift_core::world::{
    reconstruct_baseline, reconstruct_state_at_turn_with_options, IncludeReverted,
};
use std::path::Path;

pub fn run(
    cwd: &Path,
    session: Option<String>,
    at_turn: u32,
    include_reverted: bool,
    baseline: bool,
    format: &str,
) -> Result<()> {
    anyhow::ensure!(
        format == "json",
        "only --format json is supported (got '{format}')"
    );

    let dir = crate::resolve_session_dir(cwd, session)?;
    let store = Store::new(&dir);

    // Fold over the union of pending + finalized: a "live" view that
    // accounts for entries the user hasn't decided on yet.
    let mut ledger = store.list_pending()?;
    ledger.extend(store.list_ledger()?);

    if baseline {
        let map = reconstruct_baseline(&ledger);
        println!("{}", serde_json::to_string_pretty(&map)?);
    } else {
        let opts = if include_reverted {
            IncludeReverted::Yes
        } else {
            IncludeReverted::No
        };
        let map = reconstruct_state_at_turn_with_options(&ledger, at_turn, opts);
        println!("{}", serde_json::to_string_pretty(&map)?);
    }

    Ok(())
}
