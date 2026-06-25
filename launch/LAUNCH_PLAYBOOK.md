# sift Launch Playbook

Step-by-step guidance for promoting sift — *git status for AI-generated writes*.
sift records every file an AI agent writes, keyed by the conversation turn that
caused it, and gives you (and the agent) a queryable per-turn ledger. Lead with
the **"git status for your agent's writes"** framing — concrete, developer-native,
and it sidesteps the AI-content policies that close some channels.

---

## Status (Updated 2026-06-24)

| Channel | Status | Notes |
|---------|--------|-------|
| crates.io | **Published** | `sift-tui` v0.1.1 (binary `sift`); also `siftcore`, `sift-hook`, `sift-render` |
| GitHub release | **Cut** | `sift-*-v0.1.1` per-crate releases, demo GIF |
| CI + release-plz | **Live & green** | 3-OS test matrix + clippy + fmt; releases automated (Unix-only runtime) |
| Agent-tooling lists | **Draft ready** | `agent_tooling_lists.md` — sift-only channel (Claude Code / Cline hook directories) |
| Show HN | **Draft ready** | `show_hn.md` — post manually (US weekday AM ET) |
| Lobste.rs | **Draft ready** | `lobsters.md` — needs an invite; tags `rust` + `ai` |
| Terminal Trove | **Draft ready** | `terminal_trove.md` — submit via terminaltrove.com/submit |
| Twitter / X | **Draft ready** | `twitter.md` |
| awesome-ratatui | **Optional** | the `sift review` sidecar uses ratatui, but it's secondary to the CLI — only submit if framed honestly as a review TUI, not the main UX |
| awesome-rust | **Deferred** | bar is >50★ OR >2000 dl; sift is below both — revisit after a spike |
| r/rust | **Closed** | AI-generated-projects policy — do not attempt |
| r/commandline | **Closed** | AI disclosure rules — do not attempt |

**Current metrics (2026-06-24):** 1 star · `sift-tui` 30 dl / `siftcore` 62 / `sift-hook` 31 / `sift-render` 43 · v0.1.1.

---

## Pre-launch checklist

1. ~~**Generate `assets/preview.png`**~~ — **DONE** (2026-06-25). 1180×720 still
   of `sift status` showing the per-turn pending ledger, committed.
2. ~~**Set the repo homepage** to `https://docs.rs/siftcore`~~ — **DONE** (2026-06-25).
3. **Verify asset URLs return 200** before each submission (raw.githubusercontent
   propagation lags a push by a minute or two):
   `https://raw.githubusercontent.com/brevity1swos/sift/main/assets/{preview.png,demo.gif}`.

> Launch-doc commits use the `chore:` prefix so they stay out of the release-plz
> changelog.

---

## Immediate Next Actions (priority order)

### 1. Submit to agent-tooling lists — *the sift-only channel, do this first*
Use `agent_tooling_lists.md`. sift installs as a hook for Claude Code / Cline /
Gemini, so the `awesome-claude-code` / Claude-Code-hooks directories are warm,
on-topic, and uncontested — no other per-turn file oracle sits there. Highest
signal-to-effort.

### 2. Post Show HN
Use `show_hn.md`. Hook = "git status, but per-turn, for the files your AI agent
writes." Be ready for "isn't this just `git`?" (sift operates *between* commits at
per-turn grain; `git commit` → `sift accept --by-commit` closes the gap) and
"who's it for — me or the agent?" (the agent is the primary user).

### 3. Submit to Terminal Trove
Use `terminal_trove.md`. Curated form, no hard star bar; rgx/agx are already
listed, so the channel is warm. Needs the `preview.png` from the checklist.

### 4. Lobste.rs (if you have an invite)
Use `lobsters.md`, tags `rust` + `ai`. Lead with the content-addressed per-turn
ledger design, not the AI hype.

### 5. Twitter / X
Use `twitter.md` once the GIF renders well in-feed.

### 6. awesome-rust — WAIT for the bar
Do **not** submit until sift clears **>50 stars OR >2000 downloads**.

---

## Positioning

sift's niche is the **per-turn file-world ledger**: every write an agent makes is
content-addressed (SHA-1) and keyed to the conversation turn that caused it, so
you can ask "what changed in turn 7", "what did the agent write between turns 5
and 8", "revert that one but keep the others" — without scrolling the transcript
or running a coarse `git diff`. Two things to make crisp:

1. **The agent is the primary user.** Agents can't cheaply scroll their own
   transcript (tokens, context-window cost). sift gives the agent a queryable
   index the conversation can't provide; you just ask in natural language and the
   agent runs the right `sift` command. Your only direct touchpoint is `git commit`.
2. **It lives *between* commits.** sift is not a git replacement — git stays the
   coarse human approval signal; sift is the per-turn grain underneath it
   (`git commit` → `sift accept --by-commit`).

**Honest framing.** sift is *not* a linter or security scanner (it tracks *what*
changed, not whether it's safe) and *not* a general backup (snapshots are
session-scoped and GC'd). Say so — the restraint is the credibility.

---

## Monitoring

```bash
# Stars
gh api repos/brevity1swos/sift --jq '.stargazers_count'

# crates.io downloads (all four crates)
for c in sift-tui siftcore sift-hook sift-render; do
  curl -s https://crates.io/api/v1/crates/$c | jq -c "{($c): .crate | {downloads, recent_downloads}}"
done

# Traffic referrers (auth)
gh api repos/brevity1swos/sift/traffic/popular/referrers

# Open PRs and issues
gh pr list --repo brevity1swos/sift
gh issue list --repo brevity1swos/sift
```

**Decision rule:** flat signal after a promotion wave → back to maintenance mode;
a spike that clears 50★ → open the awesome-rust PR.
