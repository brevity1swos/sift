# Phase 1 — agx synergy + fsck + suite-conventions retrofits

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Status update 2026-04-19 (later same day):** A subsequent design
> conversation produced **Phase 1.7** ("snapshot oracle"), which sharpens
> sift's positioning from "writable sibling of agx" to a three-layer
> stack — `sift = snapshot oracle, agx = navigator, git = approval signal`.
> The Phase 1 work in this plan stays accurate (most of it shipped or
> is in flight). The new framing introduces three sift-side primitives
> (`sift state --at-turn N`, `sift export --format json`,
> `sift accept --by-commit <ref>`) that make the snapshot store
> queryable. Implementation plan: see
> `2026-04-19-phase1.7-snapshot-oracle.md` in this directory. Phase 1.7
> ships orthogonally to the Phase 1.4 validation gate (sift-side only,
> no agx changes).

**Goal.** Validate (or falsify) the core sift ↔ agx thesis — *agx shows you what happened, sift gates what you keep* — while hardening the ledger and aligning sift with the stepwise suite conventions. Three parallel tracks that ship as v0.3.

**Scope boundary.** No agx code changes. Integration is process-boundary only. Every agx-dependent feature must degrade silently when agx is absent — status-bar hint, never a hard failure.

**Philosophy check.** Every task below passes the stepwise trial test: *does this meaningfully slow the agent's end-to-end cycle?* If yes, cut scope until it doesn't. Review features that add perceptible friction are worse than no review features.

**Tech stack.** Rust, serde_json, tokio (already in-tree), std::process::Command. No new runtime deps for tracks A + C; track B ("fsck") reuses existing store.rs internals.

---

## File structure

| File | Responsibility |
|------|---------------|
| `crates/sift-core/src/agx.rs` | **New.** Feature-detect agx on PATH, parse `agx --version`, cache `agx --export json` per session. |
| `crates/sift-core/src/doctor.rs` | **New.** Report siblings detected (agx, rgx) with versions + contract compatibility. |
| `crates/sift-core/src/fsck.rs` | **New.** Byte-granular JSONL validator + repair for `ledger.jsonl` / `pending.jsonl` / `*_changes.jsonl`. |
| `crates/sift-core/src/lib.rs` | **Modify.** Re-export new modules. |
| `crates/sift-cli/src/cmd_doctor.rs` | **New.** `sift doctor` subcommand. |
| `crates/sift-cli/src/cmd_fsck.rs` | **New.** `sift fsck [--repair]` subcommand. |
| `crates/sift-cli/src/main.rs` | **Modify.** Register `Doctor`, `Fsck` subcommands. |
| `crates/sift-hook/src/post_tool.rs` | **Modify.** When agx is detected, prefer `agx --export json` lookup over the in-tree transcript parser for rationale extraction. |
| `crates/sift-tui/src/app.rs` | **Modify.** Add `t` keybind (agx jump, session-level), `p` keybind (rgx policy debug — stub for Phase 2), `/` search, remap accept → `Enter`, annotate → `a`. |
| `crates/sift-tui/src/help.rs` | **Modify.** Help overlay reflects new keymap + soft-dependency hints. |
| `docs/suite-conventions.md` | **Modify.** Move sift retrofits from §10 to §1 as they ship. |
| `README.md` | **Modify.** Keys table reflects the new keymap. Add "Pairs well with" section per conventions §9. |
| `CHANGELOG.md` | **Modify.** Document keymap migration with one-release grace period. |

---

## Track A — agx synergy (subplans 1.1, 1.2, 1.3)

### Task A1: Feature-detect agx (subplan 1.1)

**Files:**
- Create: `crates/sift-core/src/agx.rs`
- Modify: `crates/sift-core/src/lib.rs`

- [ ] **Step 1: Failing test for agx probe**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_none_when_agx_missing() {
        // Force empty PATH so the probe cannot resolve agx.
        let orig = std::env::var_os("PATH");
        std::env::set_var("PATH", "");
        let result = detect();
        if let Some(orig) = orig { std::env::set_var("PATH", orig); }
        assert!(result.is_none());
    }

    #[test]
    fn parse_version_handles_stable_format() {
        // Conventions §5: agx --version must be machine-parseable.
        let v = parse_version("agx 0.1.2 (otel-proto)\n").unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 1);
        assert_eq!(v.patch, 2);
    }

    #[test]
    fn parse_version_handles_plain_semver() {
        let v = parse_version("agx 0.2.0\n").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (0, 2, 0));
    }
}
```

- [ ] **Step 2: Implement `detect` and `parse_version`**

  - `pub struct AgxInfo { pub path: PathBuf, pub version: Version }`
  - `pub fn detect() -> Option<AgxInfo>` — runs `agx --version` via `std::process::Command` with a 200ms timeout (use `wait_timeout` or spawn-and-kill pattern; don't block startup forever on a hung binary).
  - `pub fn parse_version(s: &str) -> Option<Version>` — regex-free parse: split on whitespace, find the token containing two dots, split on `.`, parse as `u16`. Tolerate trailing `(features)` suffix.
  - Cache the probe result in `OnceLock<Option<AgxInfo>>` so we don't re-exec per keystroke.

- [ ] **Step 3: Minimum-supported contract**

  - Define `const MIN_AGX_VERSION: Version = Version { major: 0, minor: 1, patch: 0 };` — the first agx version that shipped `--export json` per the conventions doc §5.
  - `AgxInfo::meets_minimum(&self) -> bool` — used by doctor and by the rationale-extraction path.

- [ ] **Step 4: `--agx-timeout-ms` env var**

  - Read `SIFT_AGX_TIMEOUT_MS` with default 200. Probe failures (timeout, non-zero exit, unparseable output) resolve to `None`, logged to stderr only when `RUST_LOG=sift=debug`.

- [ ] **Run `cargo test -p sift-core`** — all three tests pass.

### Task A2: `sift doctor` subcommand (subplan 1.6.1 — ordered early because 1.1/1.2/1.3 all need it for user-visible diagnostics)

**Files:**
- Create: `crates/sift-core/src/doctor.rs`
- Create: `crates/sift-cli/src/cmd_doctor.rs`
- Modify: `crates/sift-cli/src/main.rs`

- [ ] **Step 1: Failing test for doctor report shape**

```rust
#[test]
fn doctor_report_flags_missing_agx() {
    let report = Report::new_for_test(ProbeResults {
        agx: None,
        rgx: None,
    });
    let out = report.to_text();
    assert!(out.contains("agx: not found"));
    assert!(out.contains("rgx: not found"));
    // Non-fatal: doctor reports, never exits non-zero for missing siblings.
    assert_eq!(report.exit_code(), 0);
}

#[test]
fn doctor_report_flags_incompatible_agx() {
    let report = Report::new_for_test(ProbeResults {
        agx: Some(AgxInfo { path: "/usr/local/bin/agx".into(), version: Version { major: 0, minor: 0, patch: 5 } }),
        rgx: None,
    });
    let out = report.to_text();
    assert!(out.contains("agx: 0.0.5 (too old; needs >= 0.1.0)"));
}
```

- [ ] **Step 2: `Report` struct + probes**

  - `pub struct Report { agx: Option<AgxInfo>, rgx: Option<RgxInfo>, sift: Version }`
  - Probes reuse `agx::detect` and a new symmetric `rgx::detect` (stub for now; wire to real rgx probe when Phase 2 lands).
  - `to_text(&self) -> String` — tabular, one row per sibling, with status in {ok, missing, too-old, not-on-PATH}.

- [ ] **Step 3: Register clap subcommand**

  - `sift doctor` — no flags, prints report to stdout, exits 0 unless sift itself is broken.
  - `sift doctor --json` — machine-readable; useful for CI and for `sift doctor --json | jq` dogfooding.

- [ ] **Step 4: Test on a workstation with and without agx installed.**

  - Document both outputs in a doctor.snap file under `crates/sift-cli/tests/snapshots/` via `insta`.

### Task A3: Subprocess-powered rationale extraction (subplan 1.2) — **DEFERRED**

**Status: deferred 2026-04-19.** Verified by reading `agx/src/timeline.rs` that the serialized `Step` struct does **not** carry `tool_use_id` — the field is used internally during `build()` for pairing tool_use with tool_result, then dropped before serialization. Consequence: `agx --export json` output cannot be joined with sift's `HookEvent.tool_use_id`, so A3's entire correlation story fails. Current shim's "last assistant text preceding the hook fire" is actually not much worse in practice since the hook fires microseconds after the tool call.

**Three unblocking paths, none on this phase's critical path:**

1. **agx exposes `tool_use_id` on `StepKind::ToolUse` serialization.** Cleanest. Requires conventions §5 update and an agx release. Would unlock A3 verbatim.
2. **sift grows its own mini-parser for Claude Code / Gemini / Codex sessions** to do the join itself. Duplicates agx; violates "one tool does one thing."
3. **Keep the shim; drop A3.** Honest, cheap. The marginal gain agx would give on rationale quality isn't worth duplicating the parser.

**Phase 1 takes path 3.** The shim stays authoritative for rationale. agx synergy ships only via A1 (doctor probe) and A4 (`t` keybind). If a future dogfood surfaces rationale-quality complaints, revisit with path 1 as the cheapest lift.

<details><summary>Original spec (retained for history / if agx ships tool_use_id later)</summary>

**Files:**
- Create: `.sift/sessions/<id>/agx_cache.json` at runtime (no repo-level file).
- Modify: `crates/sift-hook/src/post_tool.rs`
- Modify: `crates/sift-core/src/agx.rs` (add cache-aware loader)

- [ ] **Step 1: Failing test for cache-hit / cache-miss paths**

```rust
#[test]
fn agx_cache_is_reused_when_session_file_unchanged() { /* ... */ }

#[test]
fn agx_cache_invalidates_on_session_mtime_bump() { /* ... */ }
```

- [ ] **Step 2: `rationale_from_agx(tool_use_id, session_path) -> Option<String>`**

  - If agx is absent: return `None`. Caller falls through to the existing transcript parser.
  - If agx is present: stat the session file; if `agx_cache.json` exists and its `session_mtime == stat.mtime`, read the cached JSON. Otherwise re-exec `agx --export json <session>` and write the cache.
  - Look up the step whose `tool_use_id` matches; extract its rationale (assistant text preceding the tool call in the same turn). Fall back to `None` if the lookup fails.

- [ ] **Step 3: Wire into `post_tool.rs`**

  - Try `rationale_from_agx` first. On `None`, fall back to the in-tree transcript parser (already shipped).
  - **Do not delete the in-tree parser.** The fallback is the only reason sift still works without agx; removing it would hard-couple the two tools, violating the roadmap's "no agx code changes, and agx never required" constraint.

- [ ] **Step 4: Validation gate (subplan 1.2's "measure before committing")**

  - Instrument both paths: record which one produced each rationale in `ledger.jsonl` entry (`rationale_source: "agx" | "shim" | "none"`).
  - After 50+ real post-tool hooks have fired with both paths available, spot-check 20 cases. If agx's rationales are **not** materially richer (same content, or noisier), revert Task A3 and keep only the shim.
  - Document the verdict in `HANDOFF.md`.

</details>

### Task A4: `t` keybind — jump to agx timeline (subplan 1.3)

**Files:**
- Modify: `crates/sift-tui/src/app.rs`
- Modify: `crates/sift-tui/src/help.rs`

- [ ] **Step 1: Honest-scope keybind**

  - `t` on the selected entry — if agx is detected and compatible: `std::process::Command::new(agx_path).arg(session_file).spawn()?.wait()`.
  - sift TUI goes to cooked mode + hides its own screen before spawning; restores on agx exit.
  - **Session-level only** — we do not pass a step index because agx v0.1.x has no `--jump-to` flag. The user lands on the session's first step; they `:N` or `n`-search to the specific turn.
  - README + help overlay wording: *"jumps into agx on this session's timeline"* — never claim step-level precision.

- [ ] **Step 2: Graceful degrade when agx missing**

  - Status bar: `t: agx not on PATH — see https://github.com/brevity1swos/agx`.
  - Don't block, don't error-exit, don't retry.

- [ ] **Step 3: State restoration**

  - After `agx` exits, sift restores: selected entry, scroll offset, overlay state, focused pane.
  - Test by invoking the keybind in a manual smoke run; automated testing of raw-mode handoff is out of scope.

- [ ] **Step 4: Upgrade-path comment**

  - Leave a `// TODO(upstream): when agx ships --jump-to <path>:<step>, pass the step index here.` next to the spawn call, with a pointer to this plan file. Do not lobby agx; wait.

---

## Track B — `sift fsck` + partial-write recovery (subplan 1.5)

Orthogonal to agx; ships in parallel so Track A's validation dogfood runs on a more robust ledger.

### Task B1: Byte-granular JSONL validator

**Files:**
- Create: `crates/sift-core/src/fsck.rs`
- Create: `crates/sift-cli/src/cmd_fsck.rs`

- [ ] **Step 1: Failing test corpus**

  ```rust
  #[test]
  fn detects_truncated_trailing_record() { /* ... */ }

  #[test]
  fn detects_duplicate_ids_from_crash_window() { /* ... */ }

  #[test]
  fn detects_orphan_tombstones_in_changes_jsonl() { /* ... */ }

  #[test]
  fn valid_ledger_reports_no_issues() { /* ... */ }
  ```

- [ ] **Step 2: Validator**

  - Parse ledger.jsonl byte-by-byte using `BufReader::read_until(b'\n', ...)`. Each record: must parse as JSON, must end in `\n`, must carry a non-empty `id`.
  - Tracked issues: `TruncatedTail`, `DuplicateId { id, offsets: Vec<u64> }`, `OrphanTombstone { id }`, `InvalidJson { offset, err }`.
  - **Do not** read via `BufReader::lines()` — that silently swallows trailing non-newline fragments, which is the exact bug this task closes (see `sift-core/src/store.rs::read_jsonl`).

- [ ] **Step 3: Readable report**

  - `sift fsck` — prints issues per-session, exit code 0 if clean, 1 if any issue found.
  - `sift fsck --json` — machine-readable.

### Task B2: `--repair` flag

- [ ] **Step 1: Failing test — round-trip repair leaves a clean ledger**

  ```rust
  #[test]
  fn repair_moves_bad_file_aside_and_writes_clean_one() {
      // seed a ledger.jsonl with a truncated tail, run repair,
      // assert the .bad.<ulid> file exists with the original bytes
      // and the new ledger.jsonl parses clean.
  }
  ```

- [ ] **Step 2: Implement repair**

  - Collect all records that parsed validly.
  - Write them to `ledger.jsonl.new.<ulid>`.
  - Rename the original to `ledger.jsonl.bad.<ulid>`.
  - Rename the new file over `ledger.jsonl`.
  - For duplicates: keep the first, drop later copies; log which id survived.
  - Orphan tombstones: dropped from the repaired file, retained in the `.bad` archive.

- [ ] **Step 3: Refuse to repair open sessions**

  - A session whose `meta.json` has no `closed_at` is active. Repair might race with the hook. Exit with a clear message; suggest `sift gc --force-close` first.

---

## Track C — Suite-conventions retrofits (subplan 1.6)

Ship as one release-noted keymap migration. All three drifts land together; no partial flips.

### Task C1: Keymap migration

**Files:**
- Modify: `crates/sift-tui/src/app.rs`
- Modify: `crates/sift-tui/src/help.rs`
- Modify: `README.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Rebind**

  - Accept: `Enter` (primary) and `Space` (secondary). `a` still accepts for **one release** with a status-bar hint (`"'a' is deprecated; use Enter"`).
  - Annotate: `a` (primary). `n` still annotates for **one release** with the same hint.
  - Search: add `/` prompt + `n` / `N` cycling. Implementation mirrors agx's `tui::apply_search` — same grammar (plain substring; `regex:` prefix reserved for Phase 2).

- [ ] **Step 2: Help overlay + README sync**

  - Help overlay keys table reflects the new bindings; deprecated bindings listed under a "Deprecated (one release)" subsection.
  - README "TUI keybindings" table updated; `sift review` description rewritten.

- [ ] **Step 3: Test matrix**

  - Unit test: `Enter` triggers `Action::Accept`.
  - Unit test: `a` still triggers `Action::Accept` + emits deprecation hint.
  - Unit test: `/` enters search mode; `n`/`N` cycle.
  - Manual smoke: open a real session with pending entries, accept via `Enter`, annotate via `a`, search via `/foo`, jump to next via `n`.

- [ ] **Step 4: CHANGELOG migration note**

  - Entry under "v0.3.0 — Keymap migration" explaining the change, the one-release grace period, and the conventions §1 table row that now aligns.

### Task C2: Conventions doc sync

**Files:**
- Modify: `docs/suite-conventions.md`

- [ ] Move the three sift retrofit rows from §10 into §1 (mark them "✓") once Task C1 lands.
- [ ] In the "Current" column of §10, update any rows that remain to reflect the new state.
- [ ] Add a new §10 row for `sift doctor` pointing back to Task A2 ("✓").

---

## Acceptance

1. **Track A** — On a workstation with agx on PATH, `sift doctor` reports agx with a green "ok" row. In `sift review`, pressing `t` on a pending entry hands off to agx on the session file; exiting agx returns to sift with the same selection. `rationale_source: "agx"` appears in new ledger entries. Without agx, every flow is visually identical except the status bar shows the install hint on `t`, and `rationale_source: "shim"` on ledger entries.

2. **Track B** — `sift fsck` on a ledger artificially corrupted with (a) a truncated last record, (b) a duplicate id, and (c) an orphan tombstone reports all three. `sift fsck --repair` produces a clean ledger and archives the original as `ledger.jsonl.bad.<ulid>`. The repaired ledger parses without issue through `sift log`.

3. **Track C** — In `sift review`, `Enter` accepts the current entry, `a` annotates, `/` starts a search, `n`/`N` cycle matches. Legacy `a`-accepts still work for one release with a deprecation hint. `docs/suite-conventions.md` §10 is three rows shorter than it was at the start of the phase.

4. **Trial check** — The end-to-end review cycle (open session → `t` to agx → return → accept) measured on a real 20-turn session completes in under 5 seconds of human-perceived latency. If the agx subprocess handoff adds >1s cold-start, scope it down (cache the probe result, pre-spawn agx lazily, etc.) — friction that pushes users to skip review is a phase failure even if all three tracks ship green.

---

## Validation dogfood (subplan 1.4)

After all three tracks land, dogfood `sift review` against 20+ real sessions with agx installed. Track in a notes file under `docs/validation/2026-q2-phase1.md`:

| Signal | Verdict |
|--------|---------|
| `t` gets pressed reflexively; timeline context visibly changes review decisions | Thesis validated → proceed to Phase 2 |
| `t` gets pressed occasionally; mostly confirms the entry | Thesis partial → keep the integration, reduce Phase 2+ investment |
| `t` rarely or never gets pressed | Thesis falsified → drop agx integration, keep standalone sift |

Write the verdict into `HANDOFF.md` with the notes file committed as evidence before starting Phase 2.

---

## Out of scope for this plan

- **rgx integration.** Phase 2 owns the `p`-key policy-debug flow. A stub keybind is acceptable in sift-tui; the actual subprocess wiring waits for Phase 2.
- **agx --jump-to.** Tracked as upstream; sift's `t` is session-level until agx ships it.
- **Annotation storage format alignment with agx.** agx stores notes under `~/.agx/notes/`; sift under `.sift/sessions/<id>/notes.json`. These are independent per conventions §5 rule 5 (one-way coupling). Do not unify storage.
- **Performance benchmarks.** Track A must not regress startup or hook latency; no formal criterion bench is required for v0.3.
