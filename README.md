# sift

The per-turn snapshot oracle for your AI agent's file world.

## Who actually uses sift

**The agent — not you.** sift records every file the agent writes
silently in the background; you don't see it. When you ask the
assistant something like *"what did you change in turn 7?"* or
*"revert the second edit you made to src/auth.rs"* or *"what's
different between turn 5 and turn 8?"*, the assistant runs the
appropriate `sift` command and answers. Your only direct sift
touchpoint is `git commit` — which (with the optional post-commit
hook) auto-accepts the matching pending entries so sift's pending
list and git's recorded state stay in sync. The `sift review` TUI
exists as a power-user escape hatch but is not the primary
interface.

The unique value: **agents can't scroll their own transcripts
efficiently** (tokens, context-window cost, conversational state
lives in your head, not the agent's). sift's queryable per-turn
ledger gives the agent a precise index the conversation can't
provide. Git + scroll-the-transcript would suffice for you alone,
but neither scales for the agent acting on your behalf.

## What this is

`sift` records every file the agent writes, keyed by the conversation turn
that caused it. The result is a queryable index of the agent's file actions —
"which files changed in turn 7", "what did the agent write between turn 5
and turn 8", "revert that one but keep the others" — that doesn't depend on
scrolling the transcript or running coarse `git diff`.

Each write is content-addressed (SHA-1) and stored under `.sift/sessions/<id>/`,
along with a per-turn ledger entry that records the path, the pre/post-state
snapshots, and the eventual decision (pending → accepted / reverted / edited).

Supports **Claude Code**, **Gemini CLI**, and **Cline** via hook integration.
Bash-tool file mutations are also captured.

### Three-layer stack

`sift` is one layer of a three-tool workflow:

- **sift** = per-turn snapshot oracle for the agent's file world
- **[agx](https://github.com/brevity1swos/agx)** = navigator that addresses
  it (timeline + corpus + diff)
- **git** = coarse approval signal that closes the loop

The layers are independent — sift earns its keep without agx, agx without
sift, both without git — but the value compounds when stacked. The sharpest
move sift unlocks: pick any two turns A and B, ask "what changed in the file
world between them" (Phase 1.7, in progress).

## What this is not

- **Not a git replacement.** sift operates *between* commits at per-turn
  grain; git remains the human's coarse approval signal. The intended workflow
  is `git commit` → `sift accept --by-commit` (closes the grain gap; planned
  Phase 1.7).
- **Not a linter or security scanner.** sift tracks *what* changed, not
  whether it's safe.
- **Not a general-purpose backup.** snapshots are scoped to the session
  under `.sift/` and garbage-collected by `sift gc`.
- **Not an automation of judgment.** sift exposes file-world changes for a
  human to review. It does not classify writes as "safe" and auto-accept them.
  See `docs/suite-conventions.md` for the design philosophy.

## Install

```sh
cargo install --path crates/sift-cli
cargo install --path crates/sift-hook
```

## Quick setup

```sh
cd your-project
sift init                    # wire hooks into .claude/settings.json
sift init --tool gemini      # or for Gemini CLI
sift init --tool cline       # or for Cline
sift init --global           # wire into user-level config instead
```

This creates the hook configuration and adds `.sift/` to `.gitignore`.

## Usage

```sh
sift                         # show session status (default command)
sift ls                      # list pending entries
sift ls --path src           # filter by path substring (case-insensitive)
sift log --path README.md    # what happened to this file across all turns?
sift d <id>                  # show unified diff (auto-pages with $PAGER)
sift ok all                  # accept all pending
sift undo all                # revert all pending
sift undo <id>               # revert a specific entry (works on accepted too)
sift undo turn-3             # revert a whole turn
sift sweep                   # detect junk: duplicates, near-duplicates, slop files
sift sweep --apply           # auto-revert the junk
sift mode strict             # block next prompt until pending is cleared
sift mode loose              # back to default
sift review                  # launch TUI sidecar
sift watch                   # open review in a tmux split pane
sift history                 # list all past sessions
sift history --json          # machine-readable session list
sift init                    # wire hooks for current project
sift doctor                  # report agx/rgx sibling detection and integration readiness
sift doctor --json           # machine-readable doctor output
sift fsck                    # check ledger JSONL for corruption (exit 1 if any)
sift fsck --repair           # archive bad file + write cleaned replacement (closed sessions only)
sift fsck --json             # machine-readable fsck report
```

## How the agent uses sift

When sift is initialized in a project, the agent (Claude Code, Gemini
CLI, Cline, etc.) should reach for these commands in response to
natural-language requests. Full reference: [`docs/agent-guide.md`](docs/agent-guide.md).

| User says (in conversation) | Agent runs |
|----|----|
| "what files did you change in turn 7?" | `sift list --turn 7 --json` |
| "what changed between turn 5 and turn 8?" | `sift state --at-turn 5 --json` then `--at-turn 8 --json`, diff the maps |
| "show me what you did to README" | `sift log --path README --json` |
| "revert the third edit to src/auth.rs" | `sift list --path src/auth.rs --json`, pick the third id, `sift undo <id-prefix>` |
| "undo everything from turn 4" | `sift undo turn-4` |
| "what's still pending review?" | `sift status --json` |
| "are agx and rgx wired up?" | `sift doctor --json` |

After `git commit`, the agent (or the post-commit hook installed by
`sift init --auto-accept-on-commit`) runs
`sift accept --by-commit HEAD --apply --quiet` so the pending list
clears for the writes git already settled. (Phase 1.7.3 — planned
v0.5.)

## TUI keybindings (`sift review`) — power-user escape hatch

| Key | Action |
|-----|--------|
| `j`/`k` | navigate entries |
| `Enter` / `Space` | accept current entry |
| `r` | revert current entry |
| `e` | edit post-state in `$EDITOR` before accepting |
| `a` | annotate entry with a note |
| `/` | search entries by path (Enter jumps to first match, Esc cancels) |
| `n` / `N` | cycle to next / previous search match |
| `t` | hand off to agx on this session's transcript (feature-detected) |
| `q` | quit |

Keys align with the [stepwise suite conventions](docs/suite-conventions.md)
so `a` / `/` / `n` / `N` mean the same thing across rgx, agx, and sift.

The `t` keybind requires [agx](https://github.com/brevity1swos/agx)
on `PATH`. When agx is missing, `t` shows a one-line install hint in
the status bar instead of failing. Session-level jump only — agx
0.1.x does not ship `--jump-to <path>:<step>` yet; the user navigates
to the relevant turn inside agx with `:N` or its own `/` search.

## Policy

Create `.sift/policy.yml` to control which writes are allowed:

```yaml
rules:
  - path: "src/**"
    action: allow
  - path: "*.sql"
    action: review    # prints a note, doesn't block
  - path: ".env*"
    action: deny      # blocks the write with exit 2
```

Rules are evaluated top-to-bottom; first match wins. Default is `allow`.

## How it works

1. **SessionStart** — creates `.sift/sessions/<timestamp>/` with meta, state, snapshots.
2. **UserPromptSubmit** — bumps the turn counter. In strict mode, blocks if pending entries exist.
3. **PreToolUse** — snapshots the file's current content before Write/Edit/MultiEdit. For Bash, records a timestamp marker.
4. **PostToolUse** — captures post-state, computes diff stats, appends a pending ledger entry. For Bash, walks the project to find modified files.
5. **Policy check** — PreToolUse evaluates `.sift/policy.yml` rules; deny exits 2.
6. **Stop** — closes the session, prints a summary to stderr.

## Sweep heuristics

`sift sweep` detects junk with four rules:

1. **Exact duplicate** — two files with identical content (sha1 hash match)
2. **Fuzzy duplicate** — two files >80% similar by line content
3. **Slop pattern** — filenames matching `*_v2`, `*_new`, `*_final`, `scratch_*`, `tmp_*`, etc.
4. **Orphan markdown** — new `.md` files whose basename isn't referenced by any other project file

## Pairs well with

The full three-layer stack:

- **git** is the coarse approval signal. Use `sift accept --by-commit
  HEAD` (planned Phase 1.7) after each commit to settle the per-turn
  ledger against what git already considers approved. Removes the
  "approve once for sift, again for git" workflow tax.
- **[agx](https://github.com/brevity1swos/agx)** — the navigator
  layer above sift. Sift's `t` keybind in `sift review` hands off to
  agx on the current session's transcript (shipped session-level in
  v0.3; step-level awaits agx `--jump-to`). When sift's
  `sift export --format json` ships (Phase 1.7), agx will be able
  to overlay sift status on each timeline step and diff the file
  world between any two turns the user navigates to.
- **[rgx](https://github.com/brevity1swos/rgx)** — terminal regex
  debugger. Sift will use rgx for interactive policy-rule debugging
  (planned, v0.5): iterate on a `.sift/policy.yml` pattern with
  step-through visibility before committing the rule.

All three tools are independent — each earns its keep alone. Combined,
they form **[stepwise](https://github.com/brevity1swos/stepwise)**,
the terminal-native step-through debugger stack for the AI-development
workflow. The reframed pitch (as of Phase 1.7): **sift is the
snapshot oracle, agx is the navigator, git is the approval signal.**

## License

MIT OR Apache-2.0
