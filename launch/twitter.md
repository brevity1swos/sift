# Twitter / X

Post once the demo GIF renders cleanly in-feed (test in the composer). Keep it to
one tweet + an optional reply thread; attach `assets/demo.gif`.

## Main tweet (option A — the pain)

> Your AI agent edited 10 files across 4 turns. Which edit broke the build, and can you undo just that one?
>
> sift = git status for AI-generated writes. It records every write keyed to the turn that caused it, so you can ask "revert the second edit but keep the rest."
>
> cargo install sift-tui

## Main tweet (option B — the agent-as-user angle)

> sift's primary user isn't you — it's the agent.
>
> Agents can't cheaply scroll their own transcript. sift gives them a queryable, per-turn ledger of every file they wrote, so "what did I change in turn 7" is one command, not a context-window tax.
>
> Pure Rust. github.com/brevity1swos/sift

## Optional reply (thread continuation)

> It captures writes silently via a hook (Claude Code / Gemini / Cline), is content-addressed under .sift/, and lives *between* git commits at per-turn grain — `git commit` → `sift accept --by-commit`. Not a git replacement, not a linter, not a backup. Just precise per-turn accountability.

## Hashtags / handles (use sparingly)

`#rustlang` · `#ClaudeCode` if the framing leans on the hook integration. One or
two max — avoid over-tagging.
