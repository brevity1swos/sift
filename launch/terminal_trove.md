# Terminal Trove Submission

Submit via https://terminaltrove.com/submit (curated form, no hard star bar;
rgx and agx are already listed, so the channel is warm). Fields below map 1:1 to
the submission form; description fields respect the form's character limits.

> Image assets are ready: `assets/preview.png` (1180×720 still of `sift status` +
> the per-turn ledger) and `assets/demo.gif` are both committed and serve over
> raw.githubusercontent.

## Basic Info

| Field | Value |
|-------|-------|
| Name | sift |
| Website | https://docs.rs/siftcore |
| Repository | https://github.com/brevity1swos/sift |
| Tagline | git status for the files your AI agent writes |

## Description

**What it is** (≤300)

> sift is a per-turn snapshot oracle for an AI agent's file world. It records every file the agent writes — content-addressed and keyed to the conversation turn that caused it — into a queryable ledger. Ask "what changed in turn 7" or "revert the second edit but keep the rest" instead of scrolling the transcript or running a coarse git diff.

**Core features** (≤300)

> Captures writes silently via hooks (Claude Code, Gemini CLI, Cline; Bash-tool mutations too). Per-turn list/log/diff, point-in-time file-world state at any turn, and selective revert by id. Operates between commits at per-turn grain; `git commit` → `sift accept --by-commit` keeps the ledger in sync with git.

**Other features** (≤300)

> Every read/query command emits `--json` for scripting. A `sift review` TUI is a power-user escape hatch. Snapshots are content-addressed under .sift/, session-scoped, and garbage-collected by `sift gc`. It is deliberately not a git replacement, not a linter, and not a backup.

**Who it's for** (≤250)

> sift is for developers using AI coding agents who want precise, per-turn accountability over what the agent changed — and easy selective undo — without leaving the terminal. Install with `cargo install sift-tui` (binary `sift`). Unix (Linux / macOS).

## Technical Details — Image Preview

| Field | URL |
|-------|-----|
| PNG | https://raw.githubusercontent.com/brevity1swos/sift/main/assets/preview.png |
| GIF | https://raw.githubusercontent.com/brevity1swos/sift/main/assets/demo.gif |

## Categories (select all that apply)

- [x] **Development** — primary fit (dev tool / version-control-adjacent)
- [x] **Data & Text** — file-snapshot / diff tooling
- [ ] DevOps & Infrastructure — optional
- [ ] Operating Systems
- [ ] Databases
- [ ] Networking

The sharpest angle ("AI agent accountability") has no dedicated category, so
**Development + Data & Text** is the best available mapping.
