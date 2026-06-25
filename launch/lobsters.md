# Lobste.rs Submission

Requires an invite from an existing member. Lobste.rs is data-model literate and
allergic to hype — lead with the design, not "AI."

## Tags

`rust`, `ai` (and `vcs` if available)

## Title

sift: a content-addressed, per-turn ledger of an AI agent's file writes (Rust)

## URL

https://github.com/brevity1swos/sift

## Authored-by

Check the "authored by me" box only if comfortable; otherwise submit as a link.

## Suggested first comment (the engineering angle)

> Author here. The core data model is the interesting part. Every file write an
> agent makes is captured by a pre/post-tool hook, content-addressed (SHA-1), and
> recorded as a ledger entry keyed by `(turn, path)` with the pre- and post-state
> snapshot hashes and a decision (pending → accepted / reverted / edited). That
> lets you reconstruct the file-world *state at any turn* by folding the ledger up
> to that boundary — so "what changed between turn 5 and turn 8" is just two state
> reconstructions diffed, not a transcript scroll.
>
> It sits deliberately between git commits: git is the coarse, human-grained
> approval signal; sift is the per-turn grain underneath it, and `git commit` →
> `sift accept --by-commit` reconciles the two by matching path + content hash.
> Pure Rust, Unix-only, every query command has a `--json` mode. Happy to talk
> about the ledger design or the hook surface.
