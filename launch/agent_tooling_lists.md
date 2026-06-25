# Agent-Tooling / Claude Code Hook Lists — sift

The sift-only channel. sift installs as a **hook** for Claude Code, Cline, and
Gemini CLI, so it belongs in the agent-tooling and Claude-Code-hook directories —
a warm, on-topic audience where no other per-turn file oracle is listed.
**Highest signal-to-effort channel; do this first.**

## What to list

- **Tool:** sift (`cargo install sift-tui`, binary `sift`)
- **Repo:** https://github.com/brevity1swos/sift
- **One-liner:** git status for AI-generated writes — records every file the agent
  writes, keyed to the conversation turn, so you can query/revert per-turn without
  scrolling the transcript or running a coarse `git diff`.
- **Integration:** pre/post-tool hooks via `sift init --tool claude|cline|gemini`
  (also captures Bash-tool file mutations). Optional post-commit hook keeps the
  ledger in sync with git.

## Targets (open a PR / submit per each list's CONTRIBUTING)

| List | URL | Notes |
|------|-----|-------|
| hesreallyhim/awesome-claude-code | github.com/hesreallyhim/awesome-claude-code | Has a Hooks / Tooling section — sift fits cleanly |
| awesome-claude-code-agents / -hooks | (search the topic) | Several community lists tag `claude-code` + `hooks` |
| cline ecosystem / awesome-cline | (search the topic) | sift supports Cline via the same hook surface |
| awesome-ai-devtools / LLM-tooling lists | (search the topic) | Broader nets; the "agent file-world ledger" angle is novel |

> Before opening a PR, sync the relevant `brevity1swos` fork to upstream so the PR
> is a clean single-entry diff. Match each list's existing entry format exactly.

## Suggested entry text (markdown list item)

```markdown
- [sift](https://github.com/brevity1swos/sift) — *git status for AI-generated
  writes.* A hook that records every file the agent writes, keyed to the
  conversation turn, into a queryable per-turn ledger: "what changed in turn 7",
  "revert the second edit but keep the rest". Works with Claude Code, Cline, and
  Gemini CLI; git stays the coarse approval signal. (Rust)
```

## Positioning note

Lead with **"the agent is the primary user."** Most agent tooling gives the agent
new *capabilities* (search, browser, MCP servers); sift gives the agent (and you)
*accountability over its own file writes* at a grain git can't express. That
"per-turn ledger the conversation can't provide" framing is the hook in a list
full of capability-adders. Pair it with the honest boundary — sift tracks *what*
changed, not whether it's *safe* — so reviewers trust it isn't overclaiming.
