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
   jump-to-timeline UX — press `t` on any pending write to see the full
   turn context in agx, then return to sift to accept / revert — is the
   one workflow git genuinely cannot replicate. If this lights up, sift
   has a product beyond its standalone surface. If it doesn't, kill the
   integration ambition and let sift live or die on the standalone
   review gate alone.
2. **Standalone sift stays intact.** Policy, sweep, init, the hook
   shims, and the accept/revert ledger remain first-class. The agx
   integration adds value when agx is installed but never subtracts
   when it's absent. sift and agx ship as two distinct tools with their
   own repos, their own release trains, and their own audiences; the
   synergy when both are installed is discovered, not required.

**Why this matters beyond sift.** The three brevity1swos tools (rgx, agx,
sift) compose into **stepwise** — a trial of a specific hypothesis: *can
humans keep control over an automated agentic workflow without paying the
efficiency of that workflow for it?* sift is where the trial gets stressed
hardest. If review friction bleeds into the agent's end-to-end cycle, users
skip the review and the hypothesis falsifies in real use rather than on
paper. Every sift design choice is pinned to that constraint: terminal-
native (no browser switch), feature-detected integrations (missing siblings
never block flow), fast startup, opt-in strict mode. Review exists to make
judgment *possible*, not to impose it — and not to automate it away.

**Who it serves.** A narrower audience than agx:

- **Indie devs on agentic workflows** who don't commit between turns and want
  a review gate before the AI's writes reach their working tree.
- **Power users of multi-tool agent setups** (Claude Code + Gemini + Codex
  side-by-side) who want a shared review layer across tools.
- **Eval engineers** doing supervised dataset collection — accept/revert
  becomes the labeling action, the ledger becomes the dataset.
- **Agent-iteration developers** running prompt or config variants against
  a fixed baseline and comparing per-file diffs across runs. Proposed
  workflow; see Phase 4.5. Overlaps with eval engineers but pulls the UX
  toward bulk comparison, not per-entry labeling.
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
8. **Human judgment is the point; automation of recognition is a regression.**
   sift exposes writes for a human to review. It does not classify writes
   as "safe" and auto-accept them by heuristic. It does not replace the
   diff with an AI-generated summary of the diff. If a future feature
   would bypass or pre-digest the review gate, it fails the stepwise
   trial's stated hypothesis even if it ships faster — cut it.

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

## Phase 1 — v0.3: agx synergy (opt-in, feature-detected)

**Goal:** Make sift materially better when agx is also installed, without
requiring it and without changing agx. Validate whether the combined flow
— *agx shows you what happened, sift gates what you keep* — feels
load-bearing in real use. If yes, continue with the rest of the roadmap.
If no, drop the integration ambition and let sift live or die on the
standalone review gate.

**Why this first:** standalone sift is a review gate with independent
value (policy, sweep, accept/revert, ledger, multi-tool write capture).
But the thesis — that agx and sift together change how review feels — is
the one claim that justifies the cross-tool coordination investment.
Until that claim is tested on real sessions, there's no point building
out the deeper integration work.

### Non-goals

- No agx code changes. agx is agx; sift consumes its CLI, nothing more.
- No shared library crate. Integration is process-boundary only.
- No removal of sift's built-in Claude / Gemini / Cline payload shims.
  They stay as the fallback path when agx isn't installed.
- No intercept-style coupling where agx signals back into sift. The two
  tools compose at the subprocess boundary, nothing deeper.

### Subplans

**1.1 — Feature-detect agx**
- [ ] On sift startup, probe for `agx` on `PATH` and record its version
- [ ] `sift doctor` subcommand reports agx presence, version, and
      whether `agx --export json --version` matches the minimum
      supported schema
- [ ] All agx-dependent features gracefully degrade when agx is absent:
      status-bar hint, never a hard failure

**1.2 — Subprocess-powered rationale extraction** — **DEFERRED (upstream blocker, 2026-04-19)**
- [x] **Blocker found:** agx's serialized `Step` (in
      `agx/src/timeline.rs`) does not carry `tool_use_id` — the field is
      used internally for tool_use↔tool_result pairing and dropped before
      serialization. `agx --export json` output therefore cannot be
      joined with sift's `HookEvent.tool_use_id`. Attribution-by-proximity
      would work only coincidentally.
- [ ] **Options** (none on Phase 1's critical path):
      (a) agx adds `tool_use_id` to `StepKind::ToolUse` serialization —
      cheapest, requires conventions §5 bump and an agx release.
      (b) sift grows its own session parser — duplicates agx; cut.
      (c) ship only the shim — honest, no regression. **Chosen for
      Phase 1.**
- [ ] Revisit only if a dogfood session surfaces a specific
      rationale-quality complaint the shim cannot address. The shim's
      "last assistant text" heuristic is close to agx's "assistant text
      preceding the tool call" in practice because the hook fires
      within microseconds of the tool call.

**1.3 — `t` keybind: jump to agx timeline**
- [ ] `sift review` TUI adds a `t` action on the selected entry that
      spawns agx as a subprocess. **Honest initial scope is session-
      level jump** — agx currently exposes no `--jump-to <session>:<step>`
      CLI flag (verified 2026-04-19: only in-TUI `:N` and `jump_to_mark`
      exist). sift's `t` opens `agx <session-file>` and the user
      navigates with `:N` / `n`. Don't claim step-level precision the
      upstream binary can't deliver.
- [ ] On agx exit, sift TUI restores state (entry selection, scroll,
      mode)
- [ ] When agx is absent, `t` shows an install hint in the status bar
      pointing at agx's install docs
- [ ] Help overlay documents the keybind and the soft dependency
- [ ] **Upstream tracking:** step-level jump naturally lands alongside
      agx's Phase 5 replay / branch work. Don't lobby agx to add it —
      the integration must remain useful at session grain too, so that
      the conventions doc's §5 public-surface contract does not silently
      require a feature agx hasn't committed to yet. When agx ships
      `--jump-to`, upgrade sift's subprocess call and drop this note.

**1.4 — Validation dogfood**
- [ ] Dogfood `sift review` with agx installed across 20+ real
      sessions. No fixed calendar window; continue until the signal is
      clear.
- [ ] Track in a notes file: how often `t` gets pressed, whether
      timeline context changed review decisions, which entries it
      helped on
- [ ] Validation criterion:

  | Signal | What it means |
  |--------|---------------|
  | `t` gets pressed reflexively; timeline context visibly changes review decisions | **Thesis validated. Continue to Phase 2.** |
  | `t` gets pressed occasionally; mostly confirms what the entry already told you | **Thesis partial. Keep the integration, proceed with reduced investment in Phase 2+.** |
  | `t` rarely or never gets pressed | **Thesis falsified. Drop agx integration work; keep sift's standalone surface.** |

- [ ] Write the verdict into `HANDOFF.md` before starting Phase 2, with
      the notes file committed as evidence

**1.5 — `sift fsck` and partial-write recovery** (parallel track, orthogonal to agx)
- [ ] Independent of the agx work above; ship in parallel so the
      dogfood in 1.4 runs on a more robust ledger
- [ ] `sift fsck` parses `pending.jsonl` / `ledger.jsonl` byte-for-byte
      so a hook crash that leaves a newline-less partial write no
      longer costs the next valid entry on the subsequent
      `BufReader::lines()` parse (see `sift-core/src/store.rs::read_jsonl`)
- [ ] Detect and report: duplicate ids from the crash-between-
      append-ledger-and-append-change window, orphan tombstones in
      `*_changes.jsonl`, truncated trailing records
- [ ] `--repair` flag writes a recovered JSONL and moves the original
      to `ledger.jsonl.bad.<ulid>` so forensic inspection stays possible
- [ ] Graduates the "known edge case" from code comment to covered
      invariant; unblocks confident use in long unattended agent runs

**1.6 — Suite-conventions retrofits** (parallel track; coordinates with 1.3)
- [ ] `docs/suite-conventions.md` §10 lists three sift-owned convention
      drifts. Ship them together so TUI keymap churn is a single
      release-noted event:
      - accept key: `a` → `Enter` / `Space` (frees `a` for annotate)
      - annotate key: `n` → `a` (aligns with agx; frees `n` for
        next-search-match per §1)
      - review TUI search: add `/` + `n` / `N` to match agx
- [ ] Ship `sift doctor` subcommand per conventions §2: reports agx
      and rgx presence + version + whether each sibling's CLI surface
      matches the minimum-supported contract from conventions §5.
      Reuses the feature-detection work from 1.1.
- [ ] Update TUI help overlay, README keys table, and `--help` text in
      lockstep with the keymap change. For one release, `a` still
      accepts with a status-bar hint pointing at the new binding — then
      remove the legacy binding in the next minor.
- [ ] Move the retrofit rows out of `suite-conventions.md` §10 and into
      §1 as the rows flip from "retrofit" to "shipped."

A user with `sift` and `agx` both on `PATH` opens a session in
`sift review`, navigates to a pending Write, presses `t` to see the full
turn context in agx, returns to sift, and makes an accept or revert
decision that was informed by the timeline. Without agx installed,
everything else in sift works identically — the only visible difference
is that `t` shows an install hint.

### Depends on

- Nothing upstream. agx's `--export json` is already shipped (agx Phase
  1.4) and committed to schema stability.
- sift 0.2.x baseline (shipped).

### Feeds

- Phase 2+ only matter for the agx-integrated slice of users if 1.4
  lands on "validated" or "partial." The standalone-sift slice of the
  roadmap (policy, sweep polish, multi-tool format maturity) proceeds
  regardless.

### Rationale vs prior revision

The prior revision of Phase 1 assumed agx would ship an `agx-core`
workspace crate that sift could depend on as a library (agx Phase 7.1)
and that agx would add a `r`-to-sift keybind upstream. Both forced agx
to restructure itself for sift's benefit. Under the revised *"agx is
agx"* boundary, sift integrates at the CLI boundary only: `agx --export
json` is the contract, no shared Rust code, no coordinated release
train, no upstream agx changes. Both tools ship on their own cadence;
users install either or both independently; the synergy when both are
present is discovered, not required.

The prior "review-while-replaying" subplan — sift wrapping agx and
intercepting `r` presses — is deferred indefinitely. It required agx to
signal back into sift, which violates the boundary. If 1.4 validates
and users ask for deeper integration, revisit then; dogfood evidence
comes first.

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

**2.2 — `sift policy debug <pattern>` launches rgx (named integration: "Policy debug")**
- [ ] Subcommand that shells out to `rgx --pattern <pat> --test <path>`
      per conventions §1 cross-tool key table (`p` → rgx) and §5
      (rgx's public CLI surface: `--pattern`, `--test`, `--print`,
      `-P`). Named integration because the flow has a name that appears
      identically in rgx's and sift's docs.
- [ ] In `sift review` TUI, add `p` keybind on a pending entry whose
      path was gated by a regex rule — opens rgx pre-loaded with the
      matched rule's pattern and the entry's path as test string
- [ ] Requires rgx on PATH; missing sibling → status-bar hint per
      conventions §6 rule 2 (silent degrade — never hard-fail)

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

## Phase 4.5 — v0.6.x: Comparison Mode (candidate, gated on Phase 1 validation)

**Status: proposed 2026-04-19.** Candidate phase raised during the Phase 1
dogfood conversation. Not on the critical path until the validation in
Phase 1.4 lands and the audience evidence points this direction. Recorded
here so the feature set doesn't get reinvented under a different name
later.

**Goal:** Let an agentic-workflow developer iterate on prompts / configs
by running agent variants against a **fixed baseline** and comparing
what each variant wrote. sift already has the substrate — content-
addressed snapshots, per-turn ledger — to make the comparison cheap.
What's missing is the UX for labeling runs, resetting to a baseline, and
diffing across sessions.

**Why this phase exists.** The "A/B a prompt" workflow is currently done
ad hoc: `git reset --hard`, run the agent, `git diff`, `git reset --hard`
again, tweak the prompt, re-run, compare by eye. sift's session dir
already captures what each run wrote at per-file grain; with a small UX
layer on top, sift becomes the tool that makes this iteration loop
first-class.

**Why candidate, not committed.** It pulls sift's audience away from
"solo indie reviewing writes" toward "eval / prompt engineer running
regression passes." Those two audiences share substrate but want
different UX — indie users want fast per-entry keybinds, eval users want
bulk filters + stats. Building this without the Phase 1 verdict risks
scope dilution on both sides. Ship only if Phase 1.4 validates and real
usage surfaces this specific pain.

### Non-goals

- **No statistical / aggregate analysis.** That's agx's domain (corpus
  view, per-tool stats). Comparison mode surfaces the substrate; agx or
  an external tool computes summaries.
- **No automated "which variant was better" verdict.** Violates the
  stepwise trial's human-in-the-loop principle (guiding principle #8).
  Show both diffs; human decides.
- **No CI gating on "variant A wrote fewer lines than B."** That is a
  policy decision, not a sift decision. `sift export --format patch`
  (Phase 4.3) is the CI-gating primitive.

### Subplans

**4.5.1 — Session labels**
- [ ] `sift label <session> <tag>` assigns a short human-readable tag
      (e.g., `prompt-v3`, `claude-4.6-baseline`). Stored in session
      `meta.json` as `labels: Vec<String>` (`#[serde(default)]` for
      back-compat).
- [ ] `sift history --label <tag>` filters; `sift list --session <tag>`
      resolves a tag to a session id for all existing commands.
- [ ] No global uniqueness — tags are per-project organizational hints,
      not primary keys.

**4.5.2 — Baseline reset**
- [ ] `sift reset --to <session-or-tag>` restores working-tree files to
      the **pre-state** snapshots of the named session. Equivalent to
      "revert all" at the working-tree grain but pinned to a specific
      session's starting point.
- [ ] `--dry-run` by default; `--apply` actually writes. Refuses if the
      working tree has uncommitted non-sift changes (safety: don't clobber
      the user's in-flight work).
- [ ] Records a "reset marker" entry in the ledger so `sift log` tells
      you a reset happened — otherwise the next session would start from
      an apparently-mysterious clean state.

**4.5.3 — Cross-session diff**
- [ ] `sift diff --between <A> <B>` shows, per path, what session A did
      vs what session B did to the same baseline. Output modes:
      - unified diff (default, pipes to `$PAGER`)
      - side-by-side TUI (reuses Phase 5.1's split-pane primitive)
      - `--json` for downstream tooling
- [ ] Only compares sessions with a common baseline — uses
      `snapshot_before` equivalence to detect. If baselines diverge,
      prints a clear error pointing at `sift reset`.
- [ ] Handles the three asymmetry cases: A wrote, B didn't; B wrote, A
      didn't; both wrote (diff the post-states).

**4.5.4 — Workflow example in docs**
- [ ] `docs/comparison-mode.md` walks through a full iteration loop:
      baseline session, variant A, `sift reset --to baseline --apply`,
      variant B, `sift diff --between A B`. Keeps the docs honest about
      what the commands compose to in practice.

### Dependencies

- **Phase 1.4 validation must land in 'validated' or 'partial'** before
  starting. If the `t` keybind thesis falsifies, sift shrinks to the
  standalone review gate and Phase 4.5 is dropped.
- **Phase 4.3 `sift export --format patch`** for CI-facing comparison.
- **Phase 5.1 side-by-side diff TUI** (for the TUI-mode half of 4.5.3).

### Acceptance

A prompt-engineer iterating on a Claude Code system prompt can:
1. Run their agent once at baseline — `sift label <id> baseline`.
2. Tweak the prompt, run again — `sift label <id> v2`.
3. `sift reset --to baseline --apply` to re-seed the working tree.
4. Run a third variant — `sift label <id> v3`.
5. `sift diff --between v2 v3` to see exactly what changed in the
   agent's output between the two prompt versions, per file.

### When to kill

If Phase 1.4 falsifies (no agx synergy), sift stays a standalone review
gate and this phase is dropped. If Phase 1.4 partially validates but
dogfood never surfaces comparison-mode demand, keep it as a proposal
indefinitely — don't build speculative features.

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

## Appendix: Relationship to the stepwise suite

sift is one of three terminal-native debugging tools that make up
**stepwise** (brevity1swos is the GitHub org; stepwise is the product
umbrella, landing page at `brevity1swos/stepwise`):

| Tool | Role | Status |
|------|------|--------|
| **rgx** | Regex debugger — step through matches, visualize capture groups | v0.11.0, stable |
| **agx** | Agent trace viewer — timeline scrubber for session files | v0.1.x (Phases 0–4 shipped) |
| **sift** | AI write review gate — accept/revert what the agent wrote | v0.2.x, validating thesis |

Shared DNA: Rust + ratatui + crossterm, terminal-first, zero hosted
components, no telemetry, cross-tool support where applicable. Each tool
does one thing; the value compounds at the intersection. Shared conventions
(keybindings, CLI grammar, color palette, cross-tool contracts) are
codified in `docs/suite-conventions.md`, copied verbatim across the three
repos — divergence is a smell, fix forward.

Pitch for the suite: **"Your agent wrote 50 files. agx shows you what
happened. sift lets you pick what to keep. rgx debugs your policy
patterns. All three live in your terminal, all three work across Claude /
Codex / Gemini."**

Pitch for the trial: **"stepwise is a bet that humans can stay in the loop
on agentic workflows without paying efficiency for the privilege. If that
bet fails — if oversight measurably slows the agent's cycle without a
compensating catch — the trial ends and the tools get archived. Every
feature in all three tools passes that test before it ships."**

If any one of these stops earning its place, cut it. Don't let suite logic
rescue a tool that isn't working on its own merits.
