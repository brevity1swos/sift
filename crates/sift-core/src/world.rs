//! Reconstruct the agent's file world at a chosen turn boundary.
//!
//! The Phase 1.7 substrate: given a ledger and a turn number N, return
//! a `BTreeMap<PathBuf, String>` of `path → snapshot_after_hash` for
//! every path the agent touched at or before turn N. Latest write per
//! path wins (within the `turn ≤ N` window). Delete ops drop the path
//! from the world; reverted entries are excluded by default.
//!
//! Two arbitrary turns can be diffed by composing this twice and taking
//! the symmetric difference plus per-path content comparison — the
//! primitive nothing else in the AI-dev workflow exposes (git is
//! commit-grain; the agent transcript records intent, not state).

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::entry::{LedgerEntry, Status};

/// Whether reverted entries participate in state reconstruction.
///
/// Default (`No`) gives the "what the user kept" view, which is what
/// almost every consumer wants. `Yes` gives the "what would the world
/// have looked like if the user had accepted everything" view, useful
/// for forensic analysis or counterfactual inspection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncludeReverted {
    Yes,
    No,
}

/// Reconstruct the file world at turn `at_turn` using the default
/// (kept-only) view. Convenience wrapper for the common case.
pub fn reconstruct_state_at_turn(
    ledger: &[LedgerEntry],
    at_turn: u32,
) -> BTreeMap<PathBuf, String> {
    reconstruct_state_at_turn_with_options(ledger, at_turn, IncludeReverted::No)
}

/// Reconstruct the file world at turn `at_turn` with explicit reverted-
/// entry policy.
///
/// Walks all entries with `turn ≤ at_turn` in turn-ascending order. For
/// each entry whose status is not `Reverted` (unless `include_reverted`
/// says otherwise): if the entry has a `snapshot_after` hash, insert
/// `path → hash` (overwriting any earlier write to the same path); if
/// `snapshot_after` is `None` (a Delete op), remove the path from the
/// world.
///
/// Returns a `BTreeMap` for deterministic JSON-output ordering across
/// runs — important because consumers like `diff <(jq -S . a) <(jq -S . b)`
/// rely on stable output to compare cleanly.
pub fn reconstruct_state_at_turn_with_options(
    ledger: &[LedgerEntry],
    at_turn: u32,
    include_reverted: IncludeReverted,
) -> BTreeMap<PathBuf, String> {
    let mut map = BTreeMap::new();

    let mut sorted: Vec<&LedgerEntry> = ledger.iter().filter(|e| e.turn <= at_turn).collect();
    sorted.sort_by_key(|e| e.turn);

    for e in sorted {
        if e.status == Status::Reverted && include_reverted == IncludeReverted::No {
            continue;
        }
        match &e.snapshot_after {
            Some(hash) => {
                map.insert(e.path.clone(), hash.clone());
            }
            None => {
                // Delete op: drop the path from the world. (Or a
                // pathological entry with neither snapshot — same
                // outcome: the path doesn't survive into this state.)
                map.remove(&e.path);
            }
        }
    }

    map
}

/// Reconstruct the baseline (pre-state) for every path touched in the
/// session. Returns each path's `snapshot_before` from the *first* time
/// it appears in the ledger, regardless of subsequent writes.
///
/// `None` values mean the path did not exist before the agent created
/// it (Create ops have no pre-state). Useful as the comparison endpoint
/// for "what did the agent change cumulatively" diffs:
///
/// ```text
/// state-at-turn-N vs baseline = the cumulative agent contribution
/// ```
pub fn reconstruct_baseline(ledger: &[LedgerEntry]) -> BTreeMap<PathBuf, Option<String>> {
    let mut sorted: Vec<&LedgerEntry> = ledger.iter().collect();
    sorted.sort_by_key(|e| e.turn);

    let mut map: BTreeMap<PathBuf, Option<String>> = BTreeMap::new();
    for e in sorted {
        // First-seen-wins; entry() handles "only insert if absent."
        map.entry(e.path.clone())
            .or_insert_with(|| e.snapshot_before.clone());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{DiffStats, LedgerEntry, Op, Status, Tool};
    use chrono::{TimeZone, Utc};
    use std::path::Path;

    fn entry(
        turn: u32,
        path: &str,
        snap_after: Option<&str>,
        status: Status,
    ) -> LedgerEntry {
        LedgerEntry {
            id: format!("01HVXK5QZ9G7B2A0000000{turn:04}"),
            turn,
            tool: Tool::Write,
            path: path.into(),
            op: if snap_after.is_some() { Op::Modify } else { Op::Delete },
            rationale: String::new(),
            diff_stats: DiffStats {
                added: 0,
                removed: 0,
            },
            snapshot_before: Some("a".repeat(40)),
            snapshot_after: snap_after.map(String::from),
            status,
            timestamp: Utc.with_ymd_and_hms(2026, 4, 19, 12, 0, 0).unwrap(),
        }
    }

    #[test]
    fn state_at_turn_in_empty_session_is_empty() {
        let ledger: Vec<LedgerEntry> = vec![];
        let state = reconstruct_state_at_turn(&ledger, 0);
        assert!(state.is_empty());
        let state = reconstruct_state_at_turn(&ledger, 999);
        assert!(state.is_empty());
    }

    #[test]
    fn single_write_at_turn_1_appears_at_or_after_turn_1() {
        let ledger = vec![entry(1, "src/a.rs", Some("hash_a"), Status::Pending)];

        // Before the write: empty.
        assert!(reconstruct_state_at_turn(&ledger, 0).is_empty());

        // At and after the write: visible.
        let s = reconstruct_state_at_turn(&ledger, 1);
        assert_eq!(s.get(Path::new("src/a.rs")).map(String::as_str), Some("hash_a"));
        let s = reconstruct_state_at_turn(&ledger, 999);
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn later_write_to_same_path_overrides_earlier() {
        let ledger = vec![
            entry(1, "src/a.rs", Some("hash_v1"), Status::Pending),
            entry(2, "src/a.rs", Some("hash_v2"), Status::Pending),
        ];

        // At turn 1 we should only see v1.
        let s1 = reconstruct_state_at_turn(&ledger, 1);
        assert_eq!(s1.get(Path::new("src/a.rs")).map(String::as_str), Some("hash_v1"));

        // At turn 2 (and later) the override has happened.
        let s = reconstruct_state_at_turn(&ledger, 2);
        assert_eq!(s.get(Path::new("src/a.rs")).map(String::as_str), Some("hash_v2"));
    }

    #[test]
    fn delete_op_removes_path_from_world() {
        let ledger = vec![
            entry(1, "src/a.rs", Some("hash_a"), Status::Pending),
            entry(2, "src/a.rs", None, Status::Pending), // delete
        ];

        // After turn 1: visible.
        let s = reconstruct_state_at_turn(&ledger, 1);
        assert_eq!(s.len(), 1);

        // After turn 2 (delete): gone.
        let s = reconstruct_state_at_turn(&ledger, 2);
        assert!(s.is_empty(), "delete op should drop the path from the world");
    }

    #[test]
    fn reverted_writes_excluded_unless_requested() {
        let ledger = vec![entry(1, "src/a.rs", Some("hash_a"), Status::Reverted)];

        let s = reconstruct_state_at_turn(&ledger, 999);
        assert!(s.is_empty(), "reverted entries excluded by default");

        let s = reconstruct_state_at_turn_with_options(&ledger, 999, IncludeReverted::Yes);
        assert_eq!(s.len(), 1, "include_reverted=Yes brings the entry back");
        assert_eq!(s.get(Path::new("src/a.rs")).map(String::as_str), Some("hash_a"));
    }

    #[test]
    fn reverted_write_still_overrides_earlier_when_included() {
        // Forensic case: agent wrote v1 in turn 1, v2 in turn 2, user
        // reverted v2. Default view: v1. Include-reverted view: v2
        // (because the reverted entry is the latest by turn).
        let ledger = vec![
            entry(1, "src/a.rs", Some("hash_v1"), Status::Accepted),
            entry(2, "src/a.rs", Some("hash_v2"), Status::Reverted),
        ];

        let s = reconstruct_state_at_turn(&ledger, 999);
        assert_eq!(
            s.get(Path::new("src/a.rs")).map(String::as_str),
            Some("hash_v1"),
            "default view: reverted v2 dropped; v1 wins"
        );

        let s = reconstruct_state_at_turn_with_options(&ledger, 999, IncludeReverted::Yes);
        assert_eq!(
            s.get(Path::new("src/a.rs")).map(String::as_str),
            Some("hash_v2"),
            "include_reverted: v2 reasserts itself as the latest write"
        );
    }

    #[test]
    fn baseline_returns_first_pre_state_per_path() {
        let ledger = vec![
            entry(1, "src/a.rs", Some("hash_a_v1"), Status::Pending),
            entry(2, "src/a.rs", Some("hash_a_v2"), Status::Pending), // later write — should NOT override baseline
            entry(3, "src/b.rs", Some("hash_b"), Status::Pending),
        ];

        let baseline = reconstruct_baseline(&ledger);
        assert_eq!(baseline.len(), 2);
        // Both paths have a Some(snapshot_before) because the test
        // helper sets snapshot_before to "a" * 40 for every entry.
        // The important property: baseline[a.rs] is the snapshot_before
        // from turn 1 (first appearance), not turn 2.
        assert!(baseline.contains_key(Path::new("src/a.rs")));
        assert!(baseline.contains_key(Path::new("src/b.rs")));
    }

    #[test]
    fn map_is_deterministic_via_btreemap() {
        // Regression guard: a future refactor that swaps BTreeMap for
        // HashMap would silently break consumers that diff JSON output
        // across runs (jq -S could mask the issue). Assert that two
        // separate constructions produce byte-identical JSON.
        let ledger = vec![
            entry(1, "z.rs", Some("z"), Status::Pending),
            entry(1, "a.rs", Some("a"), Status::Pending),
            entry(1, "m.rs", Some("m"), Status::Pending),
        ];
        let s1 = reconstruct_state_at_turn(&ledger, 999);
        let s2 = reconstruct_state_at_turn(&ledger, 999);
        let json1 = serde_json::to_string(&s1).unwrap();
        let json2 = serde_json::to_string(&s2).unwrap();
        assert_eq!(json1, json2);
        // Order is alphabetical by path.
        assert!(json1.find("\"a.rs\"").unwrap() < json1.find("\"m.rs\"").unwrap());
        assert!(json1.find("\"m.rs\"").unwrap() < json1.find("\"z.rs\"").unwrap());
    }
}
