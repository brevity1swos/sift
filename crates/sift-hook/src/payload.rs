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

/// Parse a hook event from a JSON string. Exposed for tests and for
/// `read_from_stdin` to share the double-parse logic.
pub fn parse(input: &str) -> Result<HookEvent> {
    let mut event: HookEvent = serde_json::from_str(input)
        .with_context(|| format!("parsing hook event (typed): {input}"))?;
    event.raw = serde_json::from_str(input)
        .with_context(|| format!("parsing hook event (raw): {input}"))?;
    Ok(event)
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
        assert_eq!(e.cwd.as_deref(), Some(std::path::Path::new("/home/me/project")));
        // raw must still carry tool_use_id / tool_name / tool_input for
        // correlation::derive_key to find them.
        assert_eq!(e.raw.get("tool_use_id").and_then(|v| v.as_str()), Some("toolu_abc"));
        assert_eq!(e.raw.get("tool_name").and_then(|v| v.as_str()), Some("Write"));
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
        assert_eq!(e.raw.get("prompt").and_then(|v| v.as_str()), Some("do the thing"));
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
}
