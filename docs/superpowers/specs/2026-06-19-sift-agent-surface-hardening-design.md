# sift agent-surface hardening — design

**Date:** 2026-06-19
**Status:** approved (brainstorming) → ready for plan
**Scope:** sift (primary), agx (one companion data fix)
**Discipline:** pure hardening, no new product surface — fits maintenance-mode posture. No MCP server (that is a separate, later round: see "Explicitly out of scope").

## Motivation

sift's primary user is the agent, not the human (per its own README). An
evidence-first dogfood probe (2026-06-19) drove agx + sift against a live
Claude Code session and found that **sift's agent contract is broken: its
own agent-guide tells the agent things that are not true.**

Verified findings (real runs, not speculation):

- `sift ai-help` principle #1: *"every sift command supports `--json`."*
  Principle #5: *"use `sift state` for what-changed-between-turns."*
- Reality:
  - `--json` works on: `list` / `ls`, `log`, `history`, `doctor`, `fsck`.
  - `state` and `export` use `--format json` (json-by-default) and
    **reject `--json`** → `error: unexpected argument '--json'`.
  - `status` and `diff` have **no machine-readable mode at all** — neither
    `--json` nor `--format`.
- Consequence: an agent following the cookbook verbatim constructs
  `sift state --at-turn N --json` (or `sift status --json`, `sift diff <id>
  --json`) and gets a hard error. The guide's own examples are wrong.

This is invisible to a human (they read the human text output directly) and
high-impact for the agent (the documented path errors out). It is a
correctness bug in the tool's agent-facing contract, not a feature request.

Two companion findings from the same probe:

- **S3 (root cause of the above):** inconsistent flag convention across the
  CLI — `--json` bool vs `--format json` default vs nothing.
- **A3 (agx, trivial):** `agx_session_summary` returns `cost_usd: null` for
  `claude-opus-4-8` because the model is missing from agx's pricing table.
  Token counts work; dollar cost does not, for the current flagship model.

## Goals

1. Make the agent-guide's promise literally true for sift's **read/query
   surface**: `status`, `diff`, `state` accept `--json` and emit valid,
   parseable JSON.
2. Standardize on `--json` as the canonical agent flag across the read/query
   surface, while preserving the existing `--format` contract where it is
   already public (`export`, `state`).
3. Reconcile both copies of `agent-guide.md` with reality, and add a
   regression guard so the promise cannot silently rot again.
4. (Companion) Add current Claude model pricing rows to agx so cost
   self-budgeting works for the models actually in use.

## Non-goals / explicitly out of scope (YAGNI)

- **No `sift-mcp` server.** The structural asymmetry with agx (typed MCP
  tools vs shell-out + parse) is real but is a *new crate* — out of scope for
  a hardening round, and it would wrap exactly these commands, so their
  output contracts must be sound first. Deferred to a later round.
- **No `--json` on mutation/config commands** (`accept`, `revert`, `sweep`,
  `gc`, `mode`, `init`). The guide already routes those through human
  confirmation; agents don't parse their output today. Adding it now is
  speculative surface.
- **No progress/convergence surface on agx** (the known gap from the same
  probe) — deferred per the earlier scoping decision.

## Design

### Unit 1 — `sift status --json`

`cmd_status.rs` is currently `println!`-only. Add a `json: bool` parameter
(mirroring `cmd_list.rs`). Introduce a serde-serializable view struct so the
JSON shape is explicit and testable rather than ad-hoc:

```rust
#[derive(Serialize)]
struct StatusView<'a> {
    session_id: &'a str,
    turn: u32,
    mode: &'a str,                 // "loose" | "strict"
    pending: Vec<EntrySummary>,    // id, op, path, added, removed
    ledger: LedgerCounts,          // accepted, reverted
    active: bool,                  // false when "no active session"
}
```

- `--json` path: serialize `StatusView` with `serde_json::to_string_pretty`.
- The "no active session" case emits `{ "active": false, ... }` rather than
  the human help text, so the agent gets a structured signal instead of
  having to string-match "no active session".
- Default (no `--json`): byte-for-byte unchanged human output.

### Unit 2 — `sift diff <id> --json`

`cmd_diff.rs` currently produces a unified-diff string and pages it. Add
`json: bool`. When set, skip the pager and emit a structured wrapper:

```rust
#[derive(Serialize)]
struct DiffView {
    id: String,
    path: String,
    op: String,                    // create | modify | delete
    added: u32,
    removed: u32,
    snapshot_before: Option<String>, // hash
    snapshot_after: Option<String>,  // hash
    unified: String,               // the same unified diff text, as a field
}
```

The unified diff text is preserved verbatim inside the `unified` field so the
agent can show it to the user or parse hunks itself. Default path
(pager/human) is unchanged.

### Unit 3 — `sift state --json` alias

`state` already emits JSON by default via `--format json`; the only defect is
that it **rejects** `--json`, which the guide tells the agent to pass. Add a
`#[arg(long)] json: bool` that is accepted and is a no-op relative to the
default (output is already JSON). Rationale: cheapest possible change that
makes the documented invocation succeed, without disturbing the existing
`--format` public contract on `export`/`state`. If `--json` and
`--format <other>` were ever to conflict, `--json` wins (forces json); today
only `json` is implemented so there is no conflict.

### Unit 4 — agent-guide reconciliation + regression guard

- Edit **both** `docs/agent-guide.md` (canonical) and
  `crates/sift-cli/agent-guide.md` (embedded via `include_str!`) — they must
  stay identical (known dual-copy sync hazard). After this change, principle
  #1 is true as written; keep the wording but verify every cookbook example
  uses a flag that now works.
- Add an integration test in `crates/sift-cli/tests/` that, for each
  read/query command (`status`, `list`, `log`, `history`, `diff`, `state`,
  `doctor`, `fsck`), runs it with `--json` against a fixture session and
  asserts the stdout parses as JSON. This is the guard that keeps the guide
  honest — if someone adds a read command without `--json`, the test fails.
- Check whether `docs/suite-conventions.md` documents an agent-surface flag
  convention; if it does, update it (and propagate verbatim to rgx/agx per
  the suite-conventions sync rule). If it does not, add a one-line convention
  ("read/query commands expose `--json`") so the rule is captured for the
  siblings. This is a check, not an assumption.

### Unit 5 (companion, agx repo) — pricing rows

In `agx/src/pricing.rs`, add `ModelPricing` rows for the current Claude
family so `cost_usd` is non-null:
`claude-opus-4-8`, `claude-sonnet-4-6`, `claude-haiku-4-5-20251001`,
`claude-fable-5`. Rates from Anthropic's public pricing page; set
`last_verified` to today. The existing `no_duplicate_model_names` and
`every_entry_has_last_verified_date` tests guard the rows. agx uses
case-insensitive exact match (no fuzzy fallback by design), so each id in use
must be present explicitly. This is a separate commit in the agx repo.

## Data flow

No new data flows. Each command already loads its data (`Store`,
`SnapshotStore`, `SessionState`); the change is an additional serialization
branch at the output boundary. The JSON view structs are pure functions of
already-loaded domain objects — no new I/O, no new dependencies (serde +
serde_json are already workspace deps).

## Error handling

- JSON output paths reuse existing `anyhow::Result` propagation. A failure to
  load a session surfaces the same error as today; it is not swallowed into a
  JSON `{"error": ...}` envelope (keeps exit-code semantics intact for the
  agent — non-zero exit + stderr, consistent with the other `--json`
  commands).
- `diff --json` on an entry with no snapshots keeps the existing
  `bail!("entry has no snapshots to diff")` behavior.

## Testing

- Per-command JSON-validity integration test (Unit 4) — the central guard.
- Unit tests for the `StatusView` / `DiffView` shapes (field presence,
  `active: false` branch for the no-session case).
- agx: rely on the existing pricing-table guard tests; add an assertion that
  `claude-opus-4-8` resolves to a non-`None` cost for a non-zero token input.
- Full suite green both feature configs per each repo's CI matrix; clippy
  `-D warnings` and `fmt --check` clean.

## Commit / identity discipline

- Both repos are `brevity1swos` OSS tier: Conventional Commits, strict;
  `Co-Authored-By` trailer allowed. Run `gh auth switch --user brevity1swos`
  before any gh operation.
- sift changes and the agx pricing change are **separate commits in separate
  repos**. Suggested: `fix(cli): expose --json on status/diff/state` +
  `test(cli): guard --json on every read command` + `docs: reconcile
  agent-guide with real flag surface` (sift); `fix(pricing): add current
  Claude model rates` (agx).
- Pre-push: sift public surface must not mention agx/sift/stepwise synergy
  beyond what already exists; this change touches none of that.

## Open question for implementation

- Unit 3: accept `--json` as a silent no-op on `state`, or take the larger
  step of unifying `--format`→`--json` across `state`/`export`? Recommend the
  minimal no-op now (preserves the public `export` contract); revisit
  unification only if a sift-mcp round makes it worthwhile.
