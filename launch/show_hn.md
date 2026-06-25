# Show HN Post

## Title

Show HN: sift – git status for the files your AI coding agent writes

## URL

https://github.com/brevity1swos/sift

## Text

When a coding agent edits ten files across a few turns, "what did it actually change, and can I undo just one of those edits?" is annoyingly hard to answer. `git diff` is too coarse (it doesn't know about turns), and scrolling the transcript is worse. sift records every file the agent writes, content-addressed and keyed to the conversation turn that caused it, so you get a queryable per-turn ledger.

It captures writes silently via a hook (Claude Code, Gemini CLI, Cline; Bash-tool file mutations too) — no change to how you work. Then you ask in natural language and the assistant runs the right command: "what did you change in turn 7" → `sift list --turn 7`, "revert the second edit to auth.rs but keep the rest" → `sift undo <id>`, "what changed in the file world between turns 5 and 8" → two `sift state` calls diffed.

The design choice that surprised me: the primary user isn't me, it's the agent. Agents can't cheaply scroll their own transcript (tokens, context-window cost), so a queryable index is genuinely more useful to them than to me. My only direct touchpoint is `git commit`, which (with an optional post-commit hook) auto-accepts the matching pending entries so sift's ledger and git stay in sync.

It's deliberately narrow: it operates *between* commits at per-turn grain, with git as the coarse human approval signal (`git commit` → `sift accept --by-commit`). It is **not** a git replacement, not a linter/scanner (it tracks what changed, not whether it's safe), and not a backup (snapshots are session-scoped and GC'd). Pure Rust, Unix-only for now. Feedback welcome.

`cargo install sift-tui` (binary is `sift`).

## Likely questions to prep

- **Isn't this just git?** git is the coarse, human-grained approval signal; sift is the per-turn grain *between* commits. The intended flow is `git commit` → `sift accept --by-commit`. They compose; sift doesn't replace git.
- **Who is it for — me or the agent?** Primarily the agent. You ask in natural language; it runs the sift command and answers. The `sift review` TUI is a power-user escape hatch, not the main interface.
- **How does it capture writes?** Hook integration — pre/post-tool hooks for Claude Code / Cline / Gemini, plus Bash-tool file mutations. `sift init --tool claude|gemini|cline` wires it up.
- **Is it safe / does it auto-approve?** It does not classify writes as safe. It exposes what changed for a human to review (via git or the TUI). No automation of judgment.
- **Storage?** Content-addressed (SHA-1) under `.sift/`, session-scoped, garbage-collected by `sift gc`.
- **AI-built?** Yes — happy to discuss the workflow. Judge it on whether the per-turn ledger is useful.
