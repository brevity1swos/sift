use anyhow::Result;
use sift_core::{
    entry::Status,
    paths::Paths,
    session::Session,
    store::Store,
    sweep::{detect, SweepReason},
};
use std::path::Path;

pub fn run(cwd: &Path, apply: bool) -> Result<()> {
    let paths = Paths::new(cwd);
    let session = Session::open_current(paths.clone())?;
    let store = Store::new(&session.dir);
    let pending = store.list_pending()?;
    let candidates = detect(&pending, paths.project_root())?;

    if candidates.is_empty() {
        println!("sift: no sweep candidates");
        return Ok(());
    }

    println!(
        "sift: {} sweep candidate(s){}:",
        candidates.len(),
        if apply { "" } else { " (dry run — pass --apply to revert)" }
    );
    for c in &candidates {
        let reason = match &c.reason {
            SweepReason::ExactDuplicateOf(p) => format!("duplicate of {}", p.display()),
            SweepReason::SlopPattern(p) => format!("slop pattern: {p}"),
            SweepReason::OrphanMarkdown => "orphan markdown".to_string(),
        };
        println!(
            "  {} {} — {reason}",
            &c.entry_id[..8.min(c.entry_id.len())],
            c.path.display()
        );
    }

    if apply {
        for c in &candidates {
            let entry = store.finalize(&c.entry_id, Status::Reverted)?;
            store.restore_snapshot(&entry, paths.project_root(), &paths, &session.id)?;
        }
        println!("sift: reverted {} entries", candidates.len());
    }
    Ok(())
}
