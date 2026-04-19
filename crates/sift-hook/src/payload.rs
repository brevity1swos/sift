//! Hook event JSON parsing. Reads from stdin and extracts the fields we need.
//!
//! The `HookEvent` struct has two layers:
//!   1. Typed `Option<...>` fields for the common hook payload keys.
//!   2. A `raw: serde_json::Value` holding the full original payload, so
//!      correlation::derive_key can read `tool_use_id` / `tool_name` /
//!      `tool_input` off of the same parse output without losing fields
//!      to serde's typed extraction.
//!
//! `raw` is `#[serde(skip)]` so serde does not try to fill it during typed
//! deserialization; `parse()` and `read_from_stdin()` populate it by
//! parsing the input string a second time into a `Value`.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::io::Read;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Default)]
#[allow(dead_code)] // Fields consumed by later hook subcommand tasks.
pub struct HookEvent {
    pub session_id: Option<String>,
    pub transcript_path: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub hook_event_name: Option<String>,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
    pub tool_response: Option<Value>,
    pub tool_use_id: Option<String>,
    #[serde(skip)]
    pub raw: Value,
}

pub fn read_from_stdin() -> Result<HookEvent> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading hook event JSON from stdin")?;
    parse(&buf)
}

/// Top-level payload keys sift has structured handling for. Anything a
/// hook payload carries that isn't in this set is silently ignored by
/// serde (we don't set `deny_unknown_fields`), which is the right
/// default for format drift — but the drift is invisible without the
/// `SIFT_DEBUG_UNKNOWNS` escape hatch below.
///
/// Ordered alphabetically; update when a handler starts consuming a
/// field off of `raw`.
const KNOWN_HOOK_KEYS: &[&str] = &[
    "cwd",
    "hook_event_name",
    "prompt",
    "session_id",
    "stop_hook_active",
    "tool_input",
    "tool_name",
    "tool_response",
    "tool_use_id",
    "transcript_path",
];

/// Parse a hook event from a JSON string. Exposed for tests and for
/// `read_from_stdin` to share the single-parse logic.
///
/// We parse the input string once into a `serde_json::Value`, then
/// deserialize the typed fields from that value. This avoids running the
/// JSON parser twice on potentially large payloads.
pub fn parse(input: &str) -> Result<HookEvent> {
    let raw: Value = serde_json::from_str(input)
        .with_context(|| format!("parsing hook event: {input}"))?;
    let mut event: HookEvent = serde_json::from_value(raw.clone())
        .with_context(|| format!("deserializing hook event fields: {input}"))?;

    // Drift detection: when `SIFT_DEBUG_UNKNOWNS` is set, report any
    // top-level keys in the payload that aren't in KNOWN_HOOK_KEYS.
    // Off by default so the hot path is untouched; enabling it in one
    // shell is enough for a dogfood sweep without rebuilding anything.
    if std::env::var_os("SIFT_DEBUG_UNKNOWNS").is_some() {
        if let Some(unknowns) = unknown_keys(&raw) {
            eprintln!("sift-hook: unknown payload keys: {}", unknowns.join(", "));
        }
    }

    event.raw = raw;
    Ok(event)
}

/// Return the top-level keys of `raw` that are not in `KNOWN_HOOK_KEYS`,
/// sorted for deterministic output. Returns `None` when everything is
/// recognized or when `raw` is not an object.
fn unknown_keys(raw: &Value) -> Option<Vec<String>> {
    let obj = raw.as_object()?;
    let mut unknowns: Vec<String> = obj
        .keys()
        .filter(|k| !KNOWN_HOOK_KEYS.contains(&k.as_str()))
        .cloned()
        .collect();
    if unknowns.is_empty() {
        return None;
    }
    unknowns.sort();
    Some(unknowns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pretooluse_write() {
        let raw = r#"{
            "session_id": "sess-1",
            "cwd": "/home/me/project",
            "hook_event_name": "PreToolUse",
            "tool_name": "Write",
            "tool_input": { "file_path": "/home/me/project/foo.txt", "content": "hi" },
            "tool_use_id": "toolu_abc"
        }"#;
        let e = parse(raw).unwrap();
        assert_eq!(e.tool_name.as_deref(), Some("Write"));
        assert_eq!(e.tool_use_id.as_deref(), Some("toolu_abc"));
        assert_eq!(
            e.cwd.as_deref(),
            Some(std::path::Path::new("/home/me/project"))
        );
        // raw must still carry tool_use_id / tool_name / tool_input for
        // correlation::derive_key to find them.
        assert_eq!(
            e.raw.get("tool_use_id").and_then(|v| v.as_str()),
            Some("toolu_abc")
        );
        assert_eq!(
            e.raw.get("tool_name").and_then(|v| v.as_str()),
            Some("Write")
        );
        assert!(e.raw.get("tool_input").is_some());
    }

    #[test]
    fn parse_user_prompt_submit() {
        let raw = r#"{
            "session_id": "sess-1",
            "cwd": "/home/me/project",
            "hook_event_name": "UserPromptSubmit",
            "prompt": "do the thing"
        }"#;
        let e = parse(raw).unwrap();
        assert!(e.tool_name.is_none());
        assert!(e.tool_use_id.is_none());
        assert_eq!(e.hook_event_name.as_deref(), Some("UserPromptSubmit"));
        // Unknown fields like "prompt" survive in raw.
        assert_eq!(
            e.raw.get("prompt").and_then(|v| v.as_str()),
            Some("do the thing")
        );
    }

    #[test]
    fn parse_errors_on_invalid_json() {
        let err = parse("{ not valid").unwrap_err();
        let rendered = format!("{err:#}");
        assert!(rendered.contains("parsing hook event"), "got: {rendered}");
    }

    #[test]
    fn default_hook_event_has_empty_raw() {
        // Sanity check: the Default impl (needed because `raw` is #[serde(skip)])
        // produces an empty event whose `raw` is Value::Null.
        let e = HookEvent::default();
        assert!(e.tool_name.is_none());
        assert!(e.raw.is_null());
    }

    #[test]
    fn unknown_keys_returns_none_when_all_known() {
        let raw: Value = serde_json::from_str(
            r#"{
                "session_id": "s",
                "cwd": "/tmp",
                "hook_event_name": "PreToolUse",
                "tool_name": "Write",
                "tool_input": {"file_path": "/tmp/a.rs"},
                "tool_use_id": "toolu_1"
            }"#,
        )
        .unwrap();
        assert!(unknown_keys(&raw).is_none());
    }

    #[test]
    fn unknown_keys_reports_drift_sorted() {
        let raw: Value = serde_json::from_str(
            r#"{
                "session_id": "s",
                "cwd": "/tmp",
                "hook_event_name": "PreToolUse",
                "reasoning_effort": "high",
                "permission_mode": "plan",
                "tool_name": "Write"
            }"#,
        )
        .unwrap();
        let unknowns = unknown_keys(&raw).expect("should flag the two drift keys");
        // Sorted alphabetically so assertions are stable across JSON
        // object key-order quirks (serde_json uses IndexMap but we
        // shouldn't rely on that).
        assert_eq!(unknowns, vec!["permission_mode", "reasoning_effort"]);
    }

    #[test]
    fn unknown_keys_returns_none_for_non_object_input() {
        let raw: Value = serde_json::from_str("[1,2,3]").unwrap();
        assert!(unknown_keys(&raw).is_none());
    }
}
