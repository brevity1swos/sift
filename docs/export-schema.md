# `sift export --format json` schema reference

The contract `sift` publishes for downstream consumers (agx overlay
rendering, eval harnesses, third-party tools, the assistant when
asked "show me the full ledger of this session"). Versioned via the
`sift_export_version` integer at the top level.

## Stability commitment

- **Within a major version**, sift will not change the meaning or
  type of any field documented here. New fields may be added; consumers
  must tolerate unknown fields (`#[serde(default)]` or equivalent).
- **Removing a field, changing a field's type, or changing a field's
  semantics requires bumping `sift_export_version`.** Bumping is
  accompanied by an entry in `CHANGELOG.md` (when one exists) and a
  parallel update to this document.
- Consumers **must** check the version field. If
  `sift_export_version` exceeds the major version they recognize,
  they should refuse rather than guess at semantics.

## Top-level shape (`sift_export_version: 1`)

```json
{
  "sift_export_version": 1,
  "session_id": "2026-04-19-144125",
  "project": "sift",
  "cwd": "/Users/me/project/sift",
  "started_at": "2026-04-19T14:41:25Z",
  "ended_at": "2026-04-19T15:02:11Z",
  "transcript_path": "/Users/me/.claude/projects/sift/sess-abc.jsonl",
  "turn_count": 8,
  "entry_count": 14,
  "turns": [
    { "turn": 1, "entries": [...] },
    { "turn": 2, "entries": [...] }
  ]
}
```

| Field | Type | Notes |
|---|---|---|
| `sift_export_version` | integer | Schema version. Currently `1`. |
| `session_id` | string | Sift's session id (timestamp-based, see `Session::create`). |
| `project` | string | Project name (basename of `cwd`). |
| `cwd` | string | Absolute path to the project root at session-start time. |
| `started_at` | RFC 3339 string | When the session was created. |
| `ended_at` | RFC 3339 string \| null | When `sift-hook stop` ran; `null` for sessions still active or that crashed. |
| `transcript_path` | string \| null | Path to the host agent's transcript (Claude Code JSONL, etc.) as reported by the SessionStart hook. `null` for pre-v0.3 sessions or assistants that don't pass it. |
| `turn_count` | integer | Number of distinct turns observed. |
| `entry_count` | integer | Total ledger entries (pending + finalized). |
| `turns` | array of TurnExport | Entries grouped by turn ascending. Within a turn, insertion order is preserved. |

## `TurnExport`

```json
{
  "turn": 7,
  "entries": [LedgerEntry, ...]
}
```

| Field | Type | Notes |
|---|---|---|
| `turn` | integer | Turn number (≥ 0). UserPromptSubmit bumps it; PostToolUse records the entry under the current turn. |
| `entries` | array of LedgerEntry | All writes recorded under this turn. |

## `LedgerEntry`

The same shape sift writes to `pending.jsonl` and `ledger.jsonl`.
Documented separately because consumers will look it up:

```json
{
  "id": "01HXXXXXXXXXXXXXXXXXXXXXXX",
  "turn": 7,
  "tool": "Write",
  "path": "src/auth.rs",
  "op": "create",
  "rationale": "...",
  "diff_stats": { "added": 23, "removed": 0 },
  "snapshot_before": null,
  "snapshot_after": "abc123...",
  "status": "pending",
  "timestamp": "2026-04-19T14:42:00Z"
}
```

| Field | Type | Notes |
|---|---|---|
| `id` | string (26-char ULID) | Stable id; agents and consumers may use 8-char prefixes for display. |
| `turn` | integer | Turn this write happened in. |
| `tool` | enum | `"Write"` \| `"Edit"` \| `"MultiEdit"` (matches Claude Code's tool names verbatim). |
| `path` | string | Project-relative path the write affected. |
| `op` | enum | `"create"` \| `"modify"` \| `"delete"`. |
| `rationale` | string | Best-effort one-line summary from the assistant transcript; empty when not available. |
| `diff_stats.added` | integer | Lines added vs `snapshot_before`. |
| `diff_stats.removed` | integer | Lines removed vs `snapshot_before`. |
| `snapshot_before` | string \| null | SHA-1 hex of pre-state content. `null` for create ops. |
| `snapshot_after` | string \| null | SHA-1 hex of post-state content. `null` for delete ops. |
| `status` | enum | `"pending"` \| `"accepted"` \| `"reverted"` \| `"edited"`. |
| `timestamp` | RFC 3339 string | When the entry was created. |

Snapshot blobs themselves live under
`<cwd>/.sift/sessions/<session_id>/snapshots/<sha1[..2]>/<sha1[2..]>`.
Consumers that want the actual file content read those blobs directly
or shell out to `sift d <id>` for a unified diff.

## Worked example: agx-style overlay rendering

Given an export, an agx overlay would group steps by turn, then for
each Write step look up the matching `LedgerEntry` by `path` (and
optionally `tool_use_id` if a future schema version adds it) to
decorate the timeline:

```python
import json, subprocess
export = json.loads(subprocess.check_output(["sift", "export", "--format", "json"]))
assert export["sift_export_version"] == 1, "unsupported export schema"
for turn in export["turns"]:
    for entry in turn["entries"]:
        marker = {"pending": "⋯", "accepted": "✓", "reverted": "✗", "edited": "✎"}[entry["status"]]
        print(f"  turn{entry['turn']:>3} {marker} {entry['path']:<40} +{entry['diff_stats']['added']:>3} -{entry['diff_stats']['removed']:>3}")
```

## Worked example: `sift state` composition

To diff the file world between two turns A and B, two `sift state`
invocations beat re-deriving from the export — `sift state` already
applies the fold (latest-write-per-path, reverted-excluded-by-default,
delete-removes):

```bash
sift state --at-turn 5 --format json > a.json
sift state --at-turn 8 --format json > b.json
diff <(jq -S . a.json) <(jq -S . b.json)
```

Use `sift export --format json` when you want the full ledger
(rationales, statuses, timestamps); use `sift state` when you want
the file world at a point.

## Versioning history

| `sift_export_version` | Shipped in | Changes |
|---|---|---|
| 1 | v0.5 (Phase 1.7) | Initial release. |
