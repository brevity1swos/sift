# sift Roadmap

Long-term development plan for sift, organized into phases. Phases are ordered
by dependency, not calendar — ship when ready. Each phase is a minor version
(v0.2, v0.3, …).

## Executive summary

**What changed in this revision.** The prior positioning ("git status for AI
writes") undersold the tool to most of its audience and left sift vulnerable
to the dismissive "just use git" critique. Our honest answer to that critique
is narrow: sift only earns its keep when (a) the user hasn't committed yet so
git can't recover the pre-write state, or (b) the review UX for AI-generated
diffs is materially better than `git add -p`. Both conditions hold for *some*
users, but not enough to sustain a standalone tool.

The sharper positioning, arrived at via the 2026-04-15 roadmap discussion, is:

> **agx : what the agent did (read-only timeline) :: sift : what you kept (writable review gate)**

sift is the **writable sibling** of agx. Where agx shows you the agent's
trajectory as an immutable trace, sift lets you accept, revert, edit, or gate
the writes that trajectory produced. Together they form a two-tool audit
stack: agx for observability, sift for action.

This repositioning has two concrete consequences for the roadmap:

1. **Integration with agx is the load-bearing phase (Phase 1).** The
   review-while-replaying UX — step through agx's timeline and accept/revert
   inline at each write — is the one workflow git genuinely cannot replicate.
   If this lights up, sift has a product. If it doesn't, kill sift and
   recommend agx + commit-discipline instead.
2. **Standalone sift features get deprioritized.** Anything that doesn't move
   sift closer to the agx-integrated review flow is polish. We ship the
   integration first, see if the thesis holds, then decide whether the
   standalone surface (policy, sweep, init) deserves continued investment.

**Who it serves.** A narrower audience than agx:

- **Indie devs on agentic workflows** who don't commit between turns and want
  a review gate before the AI's writes reach their working tree.
- **Power users of multi-tool agent setups** (Claude Code + Gemini + Codex
  side-by-side) who want a shared review layer across tools.
- **Eval engineers** doing supervised dataset collection — accept/revert
  becomes the labeling action, the ledger becomes the dataset.
- **Teams dogfooding autonomous agents** who need to audit long unattended
  runs without re-committing after every turn.

Explicitly *not* the audience: enterprise teams with PR review, Anthropic
engineers with internal tooling, devs comfortable with `git add -p`.

**Guiding principles** (kept in sync with CLAUDE.md):

1. **Write-through for the working tree, read-only for git.** sift intercepts
   writes before git sees them but never creates commits on the user's behalf.
2. **Safe by default.** Dry-run wherever destructive. Never delete open
   sessions. Never lose a write silently — duplicates recoverable, vanishes
   not.
3. **Format-lenient hook payloads.** Claude, Gemini, Cline, Codex all emit
   slightly different hook shapes. Tolerate field drift with `#[serde(default)]`.
4. **Append-only at the storage layer.** Status changes append; reads fold.
   Compaction is opt-in. (Shipped in v0.2.)
5. **Terminal-native, no hosted components, no telemetry.** Same posture as
   agx and rgx — the brevity1swos trio share this stance.
6. **MSRV locked at 1.75** until a phase bumps it with an explicit note.
7. **Composition over feature bloat.** sift does one thing (review gate for
   AI writes); agx does one thing (timeline viewer); rgx does one thing
   (regex debugger). The suite's power is their intersection, not any one
   tool's feature list.

---

## Phase 0 — v0.1.x / v0.2.x Shipped ✅

**Goal:** Ship the standalone review gate. Close scalability ceilings before
inviting integration work.

**Duration:** v0.1.0 through v0.2.x (2026-04).

### Subplans

**0.1 — Core hook + snapshot pipeline** ✅
- [x] PreToolUse / PostToolUse / UserPromptSubmit / Stop hook binaries
- [x] Content-addressed SHA-1 snapshot store, sharded
- [x] Session lifecycle with atomic `current` symlink
- [x] Ledger entries: turn, tool, path, op, diff stats, snapshots, status

**0.2 — Review workflows** ✅
- [x] `sift review` interactive TUI with accept/revert per entry
- [x] `sift accept` / `sift revert` with `all`, `turn-N`, ID prefix targets
- [x] `sift diff`, `sift list`, `sift log`, `sift status`
- [x] Strict / loose mode; strict blocks on pending writes
- [x] Edit-before-accept (`e` key opens post-state in `$EDITOR`)
- [x] Rationale annotation (`n` key) + auto-extract from transcript

**0.3 — Multi-tool bootstrap** ✅
- [x] `sift init --tool claude|gemini|cline --global` — wires hooks into
      the target assistant's config
- [x] Bash-tool file mutation capture via timestamp-based detection

**0.4 — Workspace polish** ✅
- [x] Policy-gated writes (`.sift/policy.yml`: allow / review / deny)
- [x] Sweep (fuzzy duplicate detection, orphan markdown)
- [x] Session history, tmux watch command
- [x] `sift gc` + `sift gc --compact` (session retention and ledger
      compaction)
- [x] Append-only ledger via side-file status changes (O(N) → O(1) on
      accept/revert)

**Shipped:** ~90 tests across 4 crates (core, cli, tui, hook). Clippy
clean. Runs on macOS and Linux. Windows support out of scope.

**Explicit non-goals at v0.2:** remote sync, team features, Windows, hosted
dashboard, any replacement for git.

---

## Phase 1 — v0.3: Agx Integration (the killer feature)

**Goal:** Validate the sift-as-agx-sibling thesis. Ship the minimum viable
integration, test whether review-while-replaying feels transformative in real
use, decide whether to invest further or sunset sift.

**Why this first, nothing else matters until this lands:** standalone sift
is marginal. sift + agx is the product. The roadmap past Phase 1 is
contingent on Phase 1 results — phases 2+ only get built if the integration
validates the thesis.

### Subplans

**1.1 — Shared session parsing (blocked on agx Phase 7.1)**
- [ ] Depend on `agx-core` once it ships as a workspace crate from the agx
      repo (agx roadmap Phase 7.1)
- [ ] Replace `sift-hook/src/post_tool.rs:239` transcript rationale
      extractor with `agx_core::Step::rationale_for(tool_use_id)`
- [ ] Replace ad-hoc Claude/Gemini/Cline payload parsing in `sift-hook`
      with agx-core's format-aware Step model
- [ ] If agx Phase 7.1 slips, do a local copy of `agx-core`'s parser
      modules as an interim; delete when agx ships the crate

**Rationale for depending on agx:** sift currently reinvents agent-session
parsing. This is the largest single source of bugs and format-drift churn.
Outsourcing it to a crate whose sole job is correct parsing is a pure win.

**1.2 — Cross-launch (cheap, big UX win)**
- [ ] `sift review` accepts `--open-in-agx` flag: selecting an entry in the
      TUI launches `agx --jump-to <session>:<step>` in a subprocess
- [ ] Sift TUI gains `t` (timeline) keybind: open agx at the turn that
      produced the selected write, return to sift on exit
- [ ] Agx TUI gains `r` (review) keybind on Write/Edit steps: shells out to
      `sift review --entry <id>` (requires agx side changes — file a PR
      upstream)
- [ ] Implementation: each tool passes `--jump-to <session_id>:<step_index>`
      via env or flag; no tight coupling, subprocess boundary is the API

**1.3 — Review-while-replaying (the validation experiment)**
- [ ] `sift review --through-agx <session>` command: opens agx on the named
      session, intercepts `r` presses on Write/Edit steps, drops into sift's
      accept/revert flow inline without leaving the agx view
- [ ] On exit, returns agx view to where the user was; sift TUI state
      persists across opens
- [ ] Ship behind `--experimental` flag for v0.3.0; graduate or kill in
      v0.4 based on usage signal

**Validation criteria for the Phase 1 thesis:**

| Signal | What it means |
|--------|---------------|
| The maintainer reaches for `sift review --through-agx` on their own sessions without thinking | **Thesis validated. Continue to Phase 2.** |
| The maintainer finds it "mildly nicer" but not load-bearing | **Thesis not validated. Archive sift, recommend agx + commit discipline.** |
| The UX feels awkward, requires context-switching, or surfaces integration bugs faster than features | **Thesis falsified. Sunset sift; agx is the tool that matters.** |

**1.4 — `sift fsck` and partial-write recovery**
- [ ] `sift fsck` subcommand that parses `pending.jsonl` / `ledger.jsonl`
      byte-for-byte (not line-oriented) so a hook crash that leaves a
      newline-less partial write no longer costs the next valid entry on
      the subsequent `BufReader::lines()` parse (see comment in
      `sift-core/src/store.rs::read_jsonl`)
- [ ] Detect and report: duplicate ids from the crash-between-append-ledger-
      and-append-change window, orphan tombstones in `*_changes.jsonl`,
      truncated trailing records
- [ ] `--repair` flag writes a recovered JSONL and moves the original to
      `ledger.jsonl.bad.<ulid>` so forensic inspection is possible
- [ ] Graduates the "known edge case" from code comment to covered
      invariant; unblocks confident use in long unattended agent runs

**1.5 — Unified detail view**
- [ ] In both tools, the detail pane for a Write/Edit step shows: prompt
      that led to it → assistant's reasoning → tool call input → tool call
      result → sift pre-state snapshot → sift post-state snapshot → current
      sift status (pending/accepted/reverted/edited)
- [ ] Shared schema definition (lives in `agx-core` once that crate ships)
- [ ] Both TUIs render from the same shape; visual diff is stylistic only

**Acceptance:** a user opens a past Claude Code session in agx, steps through
the timeline with `j/k`, presses `r` on each Write step, accepts or reverts
inline. Session file is never modified. Sift ledger captures the verdict.
After the walkthrough, `git add && git commit` captures exactly what the user
accepted.

**Depends on:** agx Phase 7.1 (agx-core crate) for full decoupling; partial
implementation possible earlier with vendored parsers.

**Feeds:** everything else. If this validates, phases 2+ matter. If it
doesn't, the rest of this roadmap is void.

---

## Phase 2 — v0.4: Policy Patterns (rgx integration)

**Goal:** Make policy rules expressive enough to cover realistic project
patterns. Integrate with rgx for regex debugging of policy matchers.

**Why this second (contingent on Phase 1):** sift's policy system (`.sift/policy.yml`,
shipped in v0.2) uses globs. Real projects need regex — "auto-allow edits to
`tests/.*\.rs$` but review anything matching `.*unsafe.*`". rgx is the team's
existing regex tooling; reusing its engine keeps the toolkit coherent.

### Subplans

**2.1 — Regex-backed policy matchers**
- [ ] Extend `policy.yml` schema: rule patterns accept both `glob:` and
      `regex:` prefix (default `glob:` for backward compat)
- [ ] Use `regex` crate for regex rules (not rgx's full engine abstraction —
      rgx's `rust_regex` is the same underlying crate, so this is compatible)
- [ ] Rule debug output: `sift policy test <path>` reports which rule matched
      and why, with the matched capture highlighted (rgx-style)

**2.2 — `sift policy debug <pattern>` launches rgx**
- [ ] Subcommand that shells out to `rgx --pattern <pat> --test <path>` so
      users can iterate on a policy rule interactively
- [ ] Requires rgx on PATH; print a helpful install message if missing

**2.3 — Policy testing**
- [ ] `sift policy check` runs all rules against the full working tree and
      reports: "path matches no rule" (policy gap), "multiple conflicting
      rules match" (config bug)
- [ ] Exit code non-zero on errors for CI gating

**Acceptance:** a user with a complex codebase (monorepo with tests/,
generated/, vendored/, internal/) writes a policy that covers all four with
a mix of glob and regex rules, debugs a misfiring rule via `sift policy
debug`, and verifies coverage with `sift policy check`.

**Depends on:** Phase 1 (don't build this before the core thesis validates).

---

## Phase 3 — v0.5: Multi-Tool Format Maturity

**Goal:** First-class support for every major agent CLI that matters to sift's
audience. Fewer hook-shape bugs, more forgiving format drift handling.

### Subplans

**3.1 — Codex support**
- [ ] `sift init --tool codex` wires hooks into Codex CLI's config
- [ ] Codex hook payload schema documented in `docs/hook_formats.md`
- [ ] Corpus fixture under `tests/corpus/codex/`

**3.2 — Cursor support (if stable hook model emerges)**
- [ ] Evaluate Cursor's hook/extension API maturity
- [ ] If stable: `sift init --tool cursor` + fixtures
- [ ] If not: document the blocker, punt to Phase 7 long-tail

**3.3 — MCP tool-call capture**
- [ ] When a tool call carries MCP metadata (server, resource URI, prompt
      ID), record it in the ledger entry
- [ ] MCP filesystem writes (via a future MCP filesystem server) captured
      the same way as direct tool writes
- [ ] Depends on agx Phase 5.2 (MCP-aware rendering); reuse the same metadata
      parsing

**3.4 — Upstream dependency refresh**
- [ ] Bump `ratatui` from 0.29 → 0.30+ to shed transitive advisories flagged by
      `cargo audit`: `paste` (unmaintained) and `lru 0.12.5` (unsound per
      RUSTSEC-2026-0002, IterMut aliasing violation)
- [ ] Risk: ratatui 0.29→0.30 is a pre-1.0 breaking bump. Budget a TUI smoke
      test after the upgrade — `sift review` render, keybinds, edit overlay
- [ ] Practical risk from the current advisories is near-zero for a local
      TUI (non-adversarial input, single-threaded render loop), so this is
      tracked as hygiene not as a CVE fix

**3.5 — Format drift resilience**
- [ ] `sift-hook` hardens all `serde` deserialization with
      `#[serde(default)]` + `#[serde(other)]` fallthroughs
- [ ] `--debug-unknowns` flag on `sift-hook` binaries reports unseen payload
      shapes to stderr (mirrors agx Phase 0.3)
- [ ] Monthly format-drift CI (same pattern as agx Phase 8.2) monitors
      Claude / Gemini / Codex / Cline release notes

**Acceptance:** a user running Codex as their primary assistant can `sift
init --tool codex` and get the same review flow as Claude Code users. MCP
filesystem writes show up in the ledger indistinguishably from direct Write
calls.

---

## Phase 4 — v0.6: Batch / CI Workflows

**Goal:** Let sift live in non-interactive pipelines — auto-accept by pattern,
export reviewed-only patches, gate CI on pending reviews.

### Subplans

**4.1 — Rule-based accept**
- [ ] `sift accept --rule <glob>` accepts all pending entries matching a
      path pattern
- [ ] `sift accept --rule 'tests/**' --rule 'docs/**'` composable
- [ ] `--dry-run` by default for destructive rule-based flows; require
      explicit `--apply`

**4.2 — JSON output for scripts**
- [ ] `sift status --json` machine-readable output
- [ ] `sift list --json` ledger entries as structured data
- [ ] Exit code reflects pending-count for CI gating (`sift status --exit-code`
      returns non-zero if any pending)

**4.3 — Export reviewed-only patch**
- [ ] `sift export --session <id> --format patch > reviewed.patch` — produces
      a unified diff of accepted writes only, suitable for `git apply`
- [ ] `sift export --session <id> --format bundle` — tar bundle of the
      session's accepted files for artifact storage

**4.4 — Agx corpus integration**
- [ ] `sift review --corpus <dir>` scans all sessions in a directory (reuses
      agx Phase 3.1's corpus infra), shows aggregate pending count, lets user
      drill into any session
- [ ] Useful for review after long-running eval jobs that touch many sessions

**Acceptance:** a CI job runs an agent, then `sift accept --rule 'src/**'
--rule 'tests/**' --apply` auto-accepts the expected changes, `sift status
--exit-code` fails the build if any unreviewed pending entries remain, and
`sift export --format patch` produces a clean artifact.

**Depends on:** Phase 3 (multi-tool coverage), agx Phase 3 (corpus infra).

---

## Phase 5 — v0.7: Review UX Depth

**Goal:** Turn sift from a list-of-writes into a proper review tool.

### Subplans

**5.1 — Side-by-side diff in TUI**
- [ ] Split-pane diff view for the selected entry: pre-state left, post-state
      right, synchronized scrolling
- [ ] `d` keybind toggles; `Tab` jumps to next hunk
- [ ] Reuse rendering primitives from agx's diff mode (agx Phase 4.1)

**5.2 — Blame**
- [ ] `sift blame <path>` shows per-line: which turn created it, status
      (accepted/reverted/edited), timestamp
- [ ] Helps answer "this line is wrong, when did the agent write it?"

**5.3 — Annotations**
- [ ] `a` keybind on a ledger entry attaches a note
- [ ] Notes stored in `.sift/sessions/<id>/notes.json`
- [ ] Surfaced in `sift log` and `sift list --notes`
- [ ] Mirrors agx Phase 4.3 — same UX, stored in sift's session dir

**5.4 — Search / filter**
- [ ] In TUI, `/` filters pending list by path substring
- [ ] `/` with `regex:` prefix uses regex (leaning on Phase 2 machinery)
- [ ] `F` overlay: filter by status, turn range, file type

**Acceptance:** a user reviewing 50 pending entries can filter to just Rust
files, diff each side-by-side, annotate the interesting ones, and come back
a day later to resume exactly where they left off.

**Depends on:** Phase 4 (JSON output schema feeds annotation persistence).

---

## Phase 6 — v0.8: Library Mode

**Goal:** Make sift consumable as a crate / library by eval harnesses and
custom CI, not just as a CLI.

**Why this phase exists:** eval engineers running large batch agent jobs
want to call `sift::accept(...)` from Python rather than spawn subprocesses.
Mirrors agx Phase 7 exactly.

### Subplans

**6.1 — Clean `sift-core` public API**
- [ ] Audit `pub` surface of `sift-core`; downgrade implementation detail to
      `pub(crate)` (already partially done in the 2026-04 pass)
- [ ] Public API: `Store`, `LedgerEntry`, `StatusChange`, `Session`,
      `Paths`, accept/revert/finalize methods, sweep/gc/compact
- [ ] Document the ledger JSONL schema in `docs/schema.md`; commit to
      stability from v0.8

**6.2 — Publish to crates.io**
- [ ] `sift-core` publishes as a library; `sift-cli` / `sift-tui` / `sift-hook`
      as binary crates
- [ ] Version-lock across the workspace within a major

**6.3 — Python bindings (optional, based on demand)**
- [ ] `sift-py` crate via pyo3 if Phase 1 integration validates and eval
      users surface demand
- [ ] Surface: `sift.accept(session, id)`, `sift.revert(...)`,
      `sift.list_pending(session) -> list[Entry]`

**Acceptance:** a researcher writes a custom eval harness in Python that
runs an agent, auto-accepts writes matching expected patterns, and dumps
the ledger for dataset collection — all without shelling out to `sift` once.

**Depends on:** Phase 1 (integration validation), Phase 4 (stable JSON schema).

---

## Phase 7 — v1.0: Stabilization

**Goal:** SemVer commitment, docs complete, format long-tail covered.

### Subplans

**7.1 — Long-tail tool support**
- [ ] Aider, Windsurf, Zed Assistant, any viable hook-capable CLI that
      surfaced during phases 3–6
- [ ] Drop any tool whose CLI died

**7.2 — Documentation**
- [ ] mdBook user guide: install, every command, every flag, cookbook per
      workflow (solo Claude, multi-tool, batch eval, CI integration)
- [ ] Public API docs clean for `sift-core`

**7.3 — Stabilization commitments**
- [ ] SemVer: breaking changes to CLI flags, ledger JSONL schema, or
      `sift-core` public API require a major-version bump
- [ ] MSRV policy: locked at 1.75 for v1.0; future bumps require minor
      bump + CHANGELOG entry

**7.4 — v1.0 release checklist**
- [ ] All prior phases shipped or explicitly deferred with written rationale
- [ ] `cargo audit` clean, clippy pedantic clean
- [ ] 200+ tests (quality over quantity)
- [ ] README honest about scope and non-goals

**Depends on:** all prior phases.

---

## Cross-phase: Sustainability

Ongoing practices, not tied to a phase:

- **Kill criteria stay active.** If at any Phase transition the maintainer
  hasn't used sift in their own workflow in the preceding month, stop and
  reassess. Roadmaps for unused tools are procrastination.
- **Small releases often.** v0.2.1, v0.3.1, v0.4.1 — don't hoard. release-plz
  is already configured.
- **Answer issues within a week.** With a broader audience (post-Phase 1),
  drift reports and integration bugs will dominate. Fast response is the
  feature.
- **Dogfood with agx.** The team's two tools must compose cleanly. Every
  sift release should be tested alongside the latest agx release.
- **Dogfood with rgx.** Policy patterns get iterated with rgx as the debug
  tool. If you can't write a rule using rgx's UX, the rule language is wrong.
- **Stay lean on deps.** Core (sift-core) has no TUI deps. TUI deps stay
  in sift-tui. Hook binaries stay minimal — they run on every tool call.
- **Terminal-native, no hosted.** Same stance as agx and rgx. If a feature
  requires a server, it doesn't ship.
- **Honest positioning.** Every README revision re-tests the "agx :: sift"
  framing against what the tool actually does. If sift drifts into
  feature-overlap with git or with agx, trim it back.

---

## When to rethink the roadmap

Triggers that should cause a revision:

1. **Phase 1 integration fails validation.** If review-while-replaying feels
   "mildly convenient" rather than transformative, archive sift, recommend
   agx + commit discipline, document what we learned.
2. **Claude Code (or another major tool) ships native per-turn review.** If
   the assistant itself gates writes, sift's value collapses. Reassess
   whether the policy / sweep / multi-tool pieces still justify standalone
   existence.
3. **Agx changes shape significantly.** If agx pivots away from read-only or
   shifts its audience, sift's positioning as "the writable sibling" needs
   to be re-derived.
4. **Hook APIs diverge past the point of unification.** If Claude, Gemini,
   Codex, and Cline's hook payloads fragment faster than the parser layer
   can absorb, consider dropping support for all but the top two.
5. **Maintainer usage drops to zero for 3+ months.** Archive the repo with
   a pinned README note; don't let it rot.

---

## Appendix: Relationship to the brevity1swos toolkit

sift is one of three terminal-native debugging tools under `brevity1swos`:

| Tool | Role | Status |
|------|------|--------|
| **rgx** | Regex debugger — step through matches, visualize capture groups | v0.10.1, stable |
| **agx** | Agent trace viewer — timeline scrubber for session files | v0.1.x, active |
| **sift** | AI write review gate — accept/revert what the agent wrote | v0.2.x, validating thesis |

Shared DNA: Rust + ratatui + crossterm, terminal-first, zero hosted components,
no telemetry, cross-tool support where applicable. Each tool does one thing;
the value compounds at the intersection.

Pitch for the suite: **"Your agent wrote 50 files. agx shows you what happened.
sift lets you pick what to keep. rgx debugs your policy patterns. All three
live in your terminal, all three work across Claude / Codex / Gemini."**

If any one of these stops earning its place, cut it. Don't let suite logic
rescue a tool that isn't working on its own merits.
