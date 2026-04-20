//! `sift export --format json` — schema-stable session export for agx,
//! eval harnesses, and other downstream consumers. The "agent-as-user"
//! reframe (Phase 1.7) makes this the primary contract sift publishes:
//! the agent parses this JSON to answer "what did you do this session?"
//! questions, and external tools build dashboards on top of it.
//!
//! Stability commitment: anything inside `sift_export_version: 1`
//! shapes is frozen. New fields may be added (consumers must tolerate
//! unknown fields per `#[serde(default)]`). Removing a field, changing
//! a field's type, or changing a field's semantics requires bumping the
//! version integer. Documented in `docs/export-schema.md`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::entry::LedgerEntry;
use crate::session::SessionMeta;

/// Current export schema version. Consumers refuse unknown major
/// versions. Bump this integer (and update `docs/export-schema.md`)
/// for any breaking change to the shapes below.
pub const EXPORT_SCHEMA_VERSION: u32 = 1;

/// The full session-export envelope. Top-level shape consumers parse
/// against. Reserved field-name space is the union of fields here plus
/// any field added under the same `sift_export_version`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiftExport {
    /// Schema version. Always equal to `EXPORT_SCHEMA_VERSION` at write
    /// time; preserved on round-trip for forward-compatibility checks.
    pub sift_export_version: u32,
    pub session_id: String,
    pub project: String,
    pub cwd: PathBuf,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Path to the host agent's transcript file (Phase 1 plumbing).
    /// `None` for sessions created before transcript_path was added.
    #[serde(default)]
    pub transcript_path: Option<PathBuf>,
    /// Total number of distinct turns observed in the session. Useful
    /// for consumers that want to walk turn-by-turn without iterating
    /// the entries vector.
    pub turn_count: u32,
    /// Total number of ledger entries (pending + finalized). Cheap
    /// summary so consumers don't have to sum the per-turn arrays.
    pub entry_count: u32,
    /// Entries grouped by turn, ascending. Each turn's entries are in
    /// the order sift recorded them (insertion order).
    pub turns: Vec<TurnExport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnExport {
    pub turn: u32,
    pub entries: Vec<LedgerEntry>,
}

/// Build an export from session metadata and the union of pending +
/// finalized ledger entries. Entries are grouped by turn ascending;
/// within a turn, insertion order is preserved.
pub fn build(meta: &SessionMeta, entries: Vec<LedgerEntry>) -> SiftExport {
    let entry_count = entries.len() as u32;

    // BTreeMap groups by turn ascending automatically.
    let mut by_turn: BTreeMap<u32, Vec<LedgerEntry>> = BTreeMap::new();
    for e in entries {
        by_turn.entry(e.turn).or_default().push(e);
    }
    let turn_count = by_turn.len() as u32;

    let turns = by_turn
        .into_iter()
        .map(|(turn, entries)| TurnExport { turn, entries })
        .collect();

    SiftExport {
        sift_export_version: EXPORT_SCHEMA_VERSION,
        session_id: meta.id.clone(),
        project: meta.project.clone(),
        cwd: meta.cwd.clone(),
        started_at: meta.started_at,
        ended_at: meta.ended_at,
        transcript_path: meta.transcript_path.clone(),
        turn_count,
        entry_count,
        turns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry::{DiffStats, LedgerEntry, Op, Status, Tool};
    use chrono::TimeZone;

    fn entry(turn: u32, path: &str, id_suffix: u32) -> LedgerEntry {
        LedgerEntry {
            id: format!("01HVXK5QZ9G7B2A0000000{id_suffix:04}"),
            turn,
            tool: Tool::Write,
            path: path.into(),
            op: Op::Create,
            rationale: String::new(),
            diff_stats: DiffStats {
                added: 1,
                removed: 0,
            },
            snapshot_before: None,
            snapshot_after: Some("a".repeat(40)),
            status: Status::Pending,
            timestamp: Utc.with_ymd_and_hms(2026, 4, 19, 12, 0, 0).unwrap(),
        }
    }

    fn meta() -> SessionMeta {
        SessionMeta {
            id: "2026-04-19-120000".into(),
            project: "test".into(),
            cwd: "/tmp/test".into(),
            started_at: Utc.with_ymd_and_hms(2026, 4, 19, 12, 0, 0).unwrap(),
            ended_at: Some(Utc.with_ymd_and_hms(2026, 4, 19, 12, 30, 0).unwrap()),
            transcript_path: Some("/tmp/test/.claude/sessions/abc.jsonl".into()),
        }
    }

    #[test]
    fn empty_ledger_yields_zero_turns_and_entries() {
        let export = build(&meta(), vec![]);
        assert_eq!(export.sift_export_version, EXPORT_SCHEMA_VERSION);
        assert_eq!(export.entry_count, 0);
        assert_eq!(export.turn_count, 0);
        assert!(export.turns.is_empty());
    }

    #[test]
    fn entries_group_by_turn_ascending() {
        // Insert entries out of order to verify the BTreeMap sort.
        let entries = vec![
            entry(3, "src/c.rs", 3),
            entry(1, "src/a.rs", 1),
            entry(2, "src/b.rs", 2),
        ];
        let export = build(&meta(), entries);
        assert_eq!(export.turn_count, 3);
        assert_eq!(export.entry_count, 3);
        let turns: Vec<u32> = export.turns.iter().map(|t| t.turn).collect();
        assert_eq!(turns, vec![1, 2, 3], "turns must be sorted ascending");
    }

    #[test]
    fn multiple_entries_per_turn_preserve_insertion_order() {
        let entries = vec![
            entry(1, "src/a.rs", 1),
            entry(1, "src/b.rs", 2),
            entry(1, "src/c.rs", 3),
        ];
        let export = build(&meta(), entries);
        assert_eq!(export.turn_count, 1);
        assert_eq!(export.entry_count, 3);
        let paths: Vec<String> = export.turns[0]
            .entries
            .iter()
            .map(|e| e.path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(paths, vec!["src/a.rs", "src/b.rs", "src/c.rs"]);
    }

    #[test]
    fn round_trip_preserves_all_fields() {
        let entries = vec![entry(1, "src/a.rs", 1), entry(2, "src/b.rs", 2)];
        let export = build(&meta(), entries);
        let json = serde_json::to_string(&export).unwrap();
        let back: SiftExport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.sift_export_version, export.sift_export_version);
        assert_eq!(back.session_id, export.session_id);
        assert_eq!(back.entry_count, export.entry_count);
        assert_eq!(back.turn_count, export.turn_count);
        assert_eq!(back.turns.len(), export.turns.len());
        assert_eq!(back.transcript_path, export.transcript_path);
    }

    #[test]
    fn unknown_top_level_fields_tolerated_on_parse() {
        // Forward-compat: a future v1 export adds a field; current
        // parser must not reject it.
        let json = r#"{
            "sift_export_version": 1,
            "session_id": "s",
            "project": "p",
            "cwd": "/tmp",
            "started_at": "2026-04-19T12:00:00Z",
            "ended_at": null,
            "turn_count": 0,
            "entry_count": 0,
            "turns": [],
            "future_field_that_does_not_exist_yet": "hello"
        }"#;
        let parsed: SiftExport = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.sift_export_version, 1);
    }

    #[test]
    fn schema_version_is_one() {
        // Regression guard: bumping this constant requires a parallel
        // doc update in docs/export-schema.md and a versioning note in
        // ROADMAP. If you're seeing this assertion fail, that's the
        // checklist.
        assert_eq!(EXPORT_SCHEMA_VERSION, 1);
    }

    #[test]
    fn missing_transcript_path_round_trips_as_none() {
        // Pre-Phase-1 sessions have no transcript_path. Their meta
        // defaults the field to None; export must serialize None
        // cleanly and round-trip back to None.
        let mut m = meta();
        m.transcript_path = None;
        let export = build(&m, vec![]);
        let json = serde_json::to_string(&export).unwrap();
        let back: SiftExport = serde_json::from_str(&json).unwrap();
        assert!(back.transcript_path.is_none());
    }
}
