# Agent guide for sift

**Audience: AI coding assistants** (Claude Code, Gemini CLI, Cline, Codex,
and any other agent that may operate sift on a user's behalf).

This is the cookbook. When the user asks about file history, reach
for the command on the right. The user almost never types sift
commands directly — they ask you in natural language and you run
the right thing. Your job is to keep this layer invisible to them
unless something needs their attention.

## Operating principles

1. **Default to JSON output.** Every sift command supports
   `--format json` (or `--json`). Parse the JSON; render a brief
   natural-language summary back to the user. Do not paste raw JSON
   into the chat unless they ask to see it.
2. **Resolve id prefixes.** Sift entry ids are 26-char ULIDs. The
   user will say "the third edit" or "that one with auth" — you
   look up the matching entry via `sift list --json`, pick its
   `id`, then operate on its 8-char prefix.
3. **Confirm destructive operations.** `sift undo` rewrites files.
   Before running it on multiple entries (e.g.
   `sift undo turn-4`, `sift undo all`), tell the user what you
   are about to revert and wait for explicit confirmation.
4. **Surface divergent state.** If a `sift fsck` reports issues, or
   `sift accept --by-commit` flags entries as "diverged", surface
   that to the user — these are the cases where their attention
   actually matters.
5. **Use `sift state` for "what changed between two turns" questions.**
   Don't re-derive from `sift list` — `sift state --at-turn N`
   gives you the file-world snapshot at that point. Composing two
   calls + diffing the maps is the canonical pattern.

## Command cookbook

### "What did you change in turn N?"

```bash
sift list --turn 7 --json
```

Returns the array of entries with `turn == 7`. Summarize: count,
paths, op type per path. Mention pending vs accepted vs reverted.

### "What changed in src/auth.rs?"

```bash
sift log --path src/auth.rs --json
```

Returns ledger entries where the path contains the substring
(case-insensitive). Walk the entries chronologically; each entry's
`diff_stats` gives `+N -M` line counts.

### "What changed between turn 5 and turn 8?"

```bash
sift state --at-turn 5 --format json > /tmp/a.json
sift state --at-turn 8 --format json > /tmp/b.json
diff <(jq -S . /tmp/a.json) <(jq -S . /tmp/b.json)
```

Or do the diff in your own logic: parse both JSON maps (`path → SHA-1`),
compute symmetric difference + per-path hash mismatch. Report what
appeared, what disappeared, and what changed content. Not in v0.5
yet but shipped: `sift state` (Phase 1.7.1).

### "Revert that file" / "undo what you did"

When the user is vague, narrow first:

```bash
sift list --pending --json
```

Show the user the pending entries with id prefix + path. Ask which
one (or which subset). Then:

```bash
sift undo <id-prefix>          # one entry
sift undo turn-4               # whole turn
sift undo all                  # everything pending (confirm explicitly first)
```

If the user already accepted the entry but now wants it undone,
`sift undo <id-prefix>` works on accepted entries too — sift's
restore is byte-exact via the SHA-1-addressed snapshot store.

### "What's still pending review?"

```bash
sift status --json
```

Returns mode (loose / strict), session id, pending count, ledger
totals. If pending > 0 and the user is about to commit, suggest
they review.

### "Show me the diff for that one"

```bash
sift d <id-prefix>
```

(Auto-pages with `$PAGER`.) For agent-side rendering, use the JSON
exports — `sift export --session <id> --format json` (Phase 1.7.2,
shipped soon) gives you full ledger entries including snapshot
hashes you can pull from `.sift/sessions/<id>/snapshots/`.

### "Are agx and rgx installed?"

```bash
sift doctor --json
```

Returns the four-state sibling status (`ok` / `too-old` /
`unknown` / `missing`) for each. Use `t` keybind in `sift review`
for agx integration; `p` keybind (planned v0.5) for rgx.

### "Check the ledger for corruption"

```bash
sift fsck --json
```

Reports truncated tails, duplicate ids, orphan tombstones. If the
user has an active session, `--repair` refuses (won't race with
the hook); use `sift fsck --repair --session <closed-id>`.

### After `git commit`

If `sift init --auto-accept-on-commit` was used, the post-commit
hook handles this automatically. Otherwise, you should run:

```bash
sift accept --by-commit HEAD --apply --quiet
```

(Phase 1.7.3 — planned v0.5.) This accepts every pending entry
whose post-state matches the committed file content. Diverged
entries (file edited between agent write and commit) stay pending
with a hint.

## When NOT to use sift

- For cross-commit work or anything older than the current session,
  use `git log` / `git diff` instead. sift's grain is per-turn
  inside a session.
- For binary file inspection, sift's diff defaults to UTF-8;
  binary files render as garbage. Phase 5+ work.
- For multi-session corpus analysis, use agx (`agx corpus <dir>`)
  rather than walking sift session dirs by hand.

## How to know sift is initialized

```bash
sift status
```

Exits 0 with session info if sift is set up; prints a no-active-
session message if not. If the user mentions agentic work and
there's no active sift session, ask if they want to run
`sift init` (one-time, per-project).

## How sift learns about you (the agent)

Optionally, `sift init` can write a sift-aware section into
`CLAUDE.md` (or equivalent) that includes a pointer to this guide.
That way, future sessions in the project pick up the command
cookbook automatically — no user briefing needed.

## Stable contracts you can rely on

- `sift --version` — machine-parseable.
- `sift export --format json` schema — versioned via
  `sift_export_version` integer; documented in `docs/export-schema.md`.
- `sift state --format json` output — `path → SHA-1-hex` map,
  paths sorted alphabetically (`BTreeMap`).
- All `--json` flags produce `serde_json`-parseable output.
- Error structure (planned, currently text-only) — when JSON
  errors land, expect `{"error": "<code>", ...}` shapes.

## Versioning

This guide is versioned alongside sift itself. The command cookbook
above reflects the v0.5 surface. If commands you remember from
training data don't exist (or work differently), trust the guide
and `sift --help`, not your memory.
