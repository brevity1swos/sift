//! Ledger entry: the unit of accounting for a captured write.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use ulid::Ulid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Tool {
    Write,
    Edit,
    MultiEdit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Create,
    Modify,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Accepted,
    Reverted,
    Edited,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffStats {
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub id: String,              // ULID string
    pub turn: u32,
    pub tool: Tool,
    pub path: PathBuf,
    pub op: Op,
    #[serde(default)]
    pub rationale: String,
    pub diff_stats: DiffStats,
    pub snapshot_before: Option<String>, // sha1 hex, none if Create
    pub snapshot_after: Option<String>,  // sha1 hex, none if Delete
    pub status: Status,
    pub timestamp: DateTime<Utc>,
}

impl LedgerEntry {
    pub fn new_ulid() -> String {
        Ulid::new().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LedgerEntry {
        LedgerEntry {
            id: "01HVXK5QZ9G7B2000000000000".to_string(),
            turn: 7,
            tool: Tool::Edit,
            path: PathBuf::from("src/lib.rs"),
            op: Op::Modify,
            rationale: String::new(),
            diff_stats: DiffStats { added: 4, removed: 2 },
            snapshot_before: Some("abcd".repeat(10)),
            snapshot_after: Some("ef01".repeat(10)),
            status: Status::Pending,
            timestamp: DateTime::parse_from_rfc3339("2026-04-11T14:35:22Z")
                .unwrap()
                .with_timezone(&Utc),
        }
    }

    #[test]
    fn serializes_to_expected_shape() {
        let e = sample();
        let json = serde_json::to_value(&e).unwrap();
        assert_eq!(json["tool"], "Edit");
        assert_eq!(json["op"], "modify");
        assert_eq!(json["status"], "pending");
        assert_eq!(json["turn"], 7);
        assert_eq!(json["diff_stats"]["added"], 4);
    }

    #[test]
    fn roundtrip_preserves_all_fields() {
        let e = sample();
        let s = serde_json::to_string(&e).unwrap();
        let back: LedgerEntry = serde_json::from_str(&s).unwrap();
        assert_eq!(back.id, e.id);
        assert_eq!(back.turn, e.turn);
        assert_eq!(back.tool, e.tool);
        assert_eq!(back.op, e.op);
        assert_eq!(back.diff_stats, e.diff_stats);
        assert_eq!(back.snapshot_before, e.snapshot_before);
        assert_eq!(back.snapshot_after, e.snapshot_after);
    }

    #[test]
    fn missing_rationale_defaults_to_empty() {
        let raw = serde_json::json!({
            "id": "01HVXK5QZ9G7B2000000000000",
            "turn": 1,
            "tool": "Write",
            "path": "foo.txt",
            "op": "create",
            "diff_stats": { "added": 1, "removed": 0 },
            "snapshot_before": null,
            "snapshot_after": "a".repeat(40),
            "status": "pending",
            "timestamp": "2026-04-11T14:35:22Z"
        });
        let e: LedgerEntry = serde_json::from_value(raw).unwrap();
        assert_eq!(e.rationale, "");
    }

    #[test]
    fn new_ulid_returns_26_char_string() {
        let u = LedgerEntry::new_ulid();
        assert_eq!(u.len(), 26);
    }
}
