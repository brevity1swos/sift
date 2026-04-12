# sift

git status for AI-generated writes.

## What this is

`sift` is a lightweight audit layer for Claude Code (and similar AI coding
assistants). It intercepts every file write the assistant makes, snapshots the
pre-write state, and lets you review, accept, or revert each change
individually — or all at once.

## What this is not

- Not a Git replacement. `sift` operates on the working tree; it does not
  create commits. Use `git` for history.
- Not a real-time linter or security scanner. It tracks *what* changed, not
  whether the change is safe.
- Not a general-purpose backup tool. Snapshots live under `.sift/` and are
  scoped to the current session.

## Install

```sh
cargo install --path crates/sift-cli
cargo install --path crates/sift-hook
```

Both binaries must be on your `PATH`. `sift` is the user-facing CLI. `sift-hook`
is the Claude Code hook sidecar.

## Hook setup

Add the following to your Claude Code `settings.json`
(`~/.claude/settings.json` or `.claude/settings.json` in a project):

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": ".*",
        "hooks": [
          { "type": "command", "command": "sift-hook pre-tool" }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": ".*",
        "hooks": [
          { "type": "command", "command": "sift-hook post-tool" }
        ]
      }
    ],
    "UserPromptSubmit": [
      {
        "hooks": [
          { "type": "command", "command": "sift-hook user-prompt" }
        ]
      }
    ],
    "Stop": [
      {
        "hooks": [
          { "type": "command", "command": "sift-hook stop" }
        ]
      }
    ]
  }
}
```

`session-start` can be wired to a shell alias or run manually before starting
Claude Code in a project:

```sh
echo '{}' | sift-hook session-start
```

## Usage

```sh
# Review all pending writes interactively (TUI).
sift review

# List writes waiting for your decision.
sift list --pending

# List all writes (including accepted/reverted).
sift list --all

# Show a unified diff for a specific entry by ID prefix.
sift diff <id>

# Accept all pending writes.
sift accept all

# Accept writes from a specific turn.
sift accept turn-3

# Accept a single write by ID prefix.
sift accept <id>

# Revert all pending writes (restore pre-write snapshots).
sift revert all

# Revert writes from a specific turn.
sift revert turn-3

# Scan for redundant or suspicious AI-generated files (dry run).
sift sweep

# Apply sweep deletions.
sift sweep --apply

# Switch to strict mode (blocks Claude on pending writes).
sift mode strict

# Switch back to loose mode (default).
sift mode loose
```

## How it works

1. **session-start** — creates a `.sift/sessions/<id>/` directory and sets
   `.sift/current` to point at it.
2. **user-prompt** — records the start of a new turn; in strict mode, blocks
   Claude if there are unreviewed pending writes.
3. **pre-tool** — when Claude calls `Write` or `Edit`, sift snapshots the
   current file contents before the write happens.
4. **post-tool** — after the write, sift records the new path in the pending
   ledger and correlates it with the pre-tool snapshot.
5. **stop** — closes the session and prints a one-line summary to stderr
   (`N writes · N accepted · N reverted · N pending`).
6. **sift review / accept / revert** — you decide what to keep. Reverts
   restore the pre-write snapshot atomically.

## License

MIT OR Apache-2.0
