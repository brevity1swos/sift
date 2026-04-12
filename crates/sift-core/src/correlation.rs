//! Correlation key for pairing PreToolUse and PostToolUse hook invocations.
//!
//! Primary: `tool_use_id` from the hook payload (documented but currently
//! unreliable in PostToolUse due to anthropics/claude-code#13241).
//! Fallback: sha1 hex of `tool_name + canonical_json(tool_input)`.

use serde_json::{Map, Value};
use sha1::{Digest, Sha1};

/// Given the hook payload, return the correlation key we should use.
/// If `tool_use_id` is present and non-empty, use it (prefixed with `"id:"`).
/// Otherwise compute a content hash (prefixed with `"h:"`).
pub fn derive_key(payload: &Value) -> String {
    if let Some(id) = payload.get("tool_use_id").and_then(|v| v.as_str()) {
        if !id.is_empty() {
            return format!("id:{id}");
        }
    }
    let tool_name = payload.get("tool_name").and_then(|v| v.as_str()).unwrap_or("");
    let tool_input = payload.get("tool_input").cloned().unwrap_or(Value::Null);
    let canonical = canonical_json(&tool_input);
    let mut h = Sha1::new();
    h.update(tool_name.as_bytes());
    h.update(b"|");
    h.update(canonical.as_bytes());
    format!("h:{}", hex::encode(h.finalize()))
}

/// Deterministic JSON serialization: keys of every object sorted alphabetically.
pub fn canonical_json(v: &Value) -> String {
    let canonical = canonicalize(v);
    serde_json::to_string(&canonical).unwrap_or_default()
}

fn canonicalize(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut sorted = Map::new();
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            for k in keys {
                sorted.insert(k.clone(), canonicalize(&m[k]));
            }
            Value::Object(sorted)
        }
        Value::Array(a) => Value::Array(a.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn uses_tool_use_id_when_present() {
        let payload = json!({
            "tool_use_id": "toolu_abc123",
            "tool_name": "Edit",
            "tool_input": { "file_path": "/x/y.rs" }
        });
        assert_eq!(derive_key(&payload), "id:toolu_abc123");
    }

    #[test]
    fn falls_back_to_hash_when_id_missing() {
        let payload = json!({
            "tool_name": "Write",
            "tool_input": { "file_path": "/foo.rs", "content": "hi" }
        });
        let key = derive_key(&payload);
        assert!(key.starts_with("h:"));
        assert_eq!(key.len(), 2 + 40); // "h:" + sha1 hex
    }

    #[test]
    fn falls_back_when_id_is_empty_string() {
        let payload = json!({
            "tool_use_id": "",
            "tool_name": "Write",
            "tool_input": {}
        });
        assert!(derive_key(&payload).starts_with("h:"));
    }

    #[test]
    fn canonical_json_sorts_object_keys() {
        let a = json!({ "z": 1, "a": 2, "m": 3 });
        let b = json!({ "a": 2, "m": 3, "z": 1 });
        assert_eq!(canonical_json(&a), canonical_json(&b));
    }

    #[test]
    fn hash_is_stable_across_key_order() {
        let p1 = json!({
            "tool_name": "Edit",
            "tool_input": { "old": "x", "new": "y" }
        });
        let p2 = json!({
            "tool_name": "Edit",
            "tool_input": { "new": "y", "old": "x" }
        });
        assert_eq!(derive_key(&p1), derive_key(&p2));
    }
}
