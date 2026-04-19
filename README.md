# sift

git status for AI-generated writes.

## What this is

`sift` is a session-scoped ledger for AI coding assistants. It intercepts every
file write the assistant makes, snapshots the pre-write state, and lets you
review, accept, revert, or edit each change — one turn at a time.

Supports **Claude Code**, **Gemini CLI**, and **Cline** via hook integration.
Bash-tool file mutations are also captured.

## What this is not

- Not a git replacement — sift operates on the working tree at sub-commit grain.
- Not a linter or security scanner — it tracks *what* changed, not whether it's safe.
- Not a general-purpose backup — snapshots are scoped to the session under `.sift/`.

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

## TUI keybindings (`sift review`)

| Key | Action |
|-----|--------|
| `j`/`k` | navigate entries |
| `Enter` / `Space` | accept current entry (suite-conventions primary) |
| `a` | accept current entry (legacy — moves to annotate in v0.4) |
| `r` | revert current entry |
| `e` | edit post-state in `$EDITOR` before accepting |
| `n` | annotate entry with a note |
| `t` | hand off to agx on this session's transcript (feature-detected) |
| `q` | quit |

The `t` keybind requires [agx](https://github.com/brevity1swos/agx)
on `PATH`. When agx is missing, `t` shows a one-line install hint in
the status bar instead of failing. Session-level jump only — agx
0.1.x does not ship `--jump-to <path>:<step>` yet; the user navigates
to the relevant turn inside agx with `:N` or `/` search.

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

- **[rgx](https://github.com/brevity1swos/rgx)** — terminal regex
  debugger. Sift will use rgx for interactive policy-rule debugging
  (planned, v0.4): iterate on a `.sift/policy.yml` pattern with
  step-through visibility before committing the rule.
- **[agx](https://github.com/brevity1swos/agx)** — terminal agent
  session viewer. Sift's `t` keybind in `sift review` hands off to
  agx on the current session's transcript (shipped session-level in
  v0.3; step-level awaits agx `--jump-to`), so review decisions can
  be informed by full timeline context without leaving the terminal.

All three tools are independent — each earns its keep alone. Combined,
they form **[stepwise](https://github.com/brevity1swos/stepwise)**,
the terminal-native step-through debugger stack for the AI-development
workflow.

## License

MIT OR Apache-2.0
