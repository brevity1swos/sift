# Changelog

All notable changes to this project will be documented in this file.

## [0.1.1] - 2026-06-19

### Refactoring

- *(hook)* Add HookEvent::project_root() to DRY cwd fallback
Five hook handlers repeated event.cwd.unwrap_or_else(|| PathBuf::from(".")),
  one with a gratuitous clone. Move the fallback onto HookEvent so each handler
  is a single call and the spurious clone is gone.


## [0.1.0] - 2026-06-06

### Bug Fixes

- *(hook)* Reject `..`-bearing rel_path after abs-path strip_prefix fallback
The PreToolUse handler's absolute-path branch falls back to lexical
  strip_prefix when either canonicalize call fails (e.g. the target file
  doesn't exist yet for a Create op). That fallback can succeed on an
  input like `/<project_root>/../<sensitive>` and return a rel_path
  containing a `ParentDir` component, which would let a prompt-injected
  tool_input leak bytes of a file outside the project root into
  `.sift/snapshots/` via the upcoming `fs::read(&target_path)`.

  Add a final `validate_relative_path` check on rel_path before the
  policy eval, read, snapshot, and staging write. Silent-return on
  failure — same ExitCode(0) posture as other "skip this write" paths
  in the hook so Claude's tool call isn't blocked on sift-side rejection.
  `restore_snapshot` already validated at revert time, but catching the
  escape at ingress prevents the sensitive bytes from ever entering the
  snapshot store.

### Features

- Add rationale — n key annotation in TUI + auto-extract from transcript
Two sources of rationale for ledger entries:

  1. TUI annotation (n key): press n on a pending entry, type a one-line
     note, Enter to save. Stored in the entry's rationale field. Displayed
     in the detail pane as 'Note: ...'. Help bar shows available keys.

  2. Transcript-derived (auto): post_tool hook reads transcript_path,
     scans for the last assistant message, extracts the first sentence
     (up to 120 chars). Best-effort — falls back to empty if transcript
     is missing or unparseable.
- Capture Bash-tool file mutations via timestamp-based detection
PreToolUse for Bash saves a millisecond timestamp marker. PostToolUse
  walks the project root, finds files whose modification time is after
  the marker, snapshots each one, and creates pending ledger entries.

  The rationale field carries the Bash command (truncated to 100 chars)
  so sift ls shows what command caused each change. No pre-state
  snapshot (can't know what was there before arbitrary shell commands),
  so sift d won't show a before-diff for Bash entries.

  Also updates sift init to include Bash in the Pre/PostToolUse
  matchers for all three tool targets (claude, gemini, cline).
- Policy-gated writes — .sift/policy.yml with allow/review/deny rules
Per-path policy rules enforced in PreToolUse hook:
  - allow: hook exits 0 (default when no policy or no rule matches)
  - review: hook exits 0 but prints a note to stderr
  - deny: hook exits 2, blocking the tool call

  Rules use glob patterns (e.g., 'src/**', '*.sql', '.env*') and are
  evaluated top-to-bottom, first match wins. Policy is loaded from
  .sift/policy.yml via the serde_yaml_ng crate.

  Also adds policy_file() path helper and policy module to sift-core.
- *(core)* Agx sibling detection, sift doctor, sift fsck, session transcript path
Ships Phase 1 tracks A (agx synergy foundations) and B (fsck durability)
  at the core + CLI layer. TUI integration lands in a follow-up commit.
- *(hook)* Drift detection via SIFT_DEBUG_UNKNOWNS env var
Phase 3.5 subplan 1, partial. Each agent CLI (Claude Code, Gemini,
  Cline, Codex) can introduce new top-level hook payload fields without
  breaking sift — serde silently drops unknown fields because we don't
  set `deny_unknown_fields`. That's the right default for a format-
  lenient hook, but it means drift is invisible: sift keeps working
  while quietly ignoring something the upstream tool wants us to know.

  The fix is a one-shot escape hatch, not a feature:

  - `KNOWN_HOOK_KEYS` constant lists the 10 top-level fields sift
    actively consumes off the payload (`session_id`, `cwd`,
    `hook_event_name`, `tool_name`, `tool_input`, `tool_response`,
    `tool_use_id`, `transcript_path`, `prompt`, `stop_hook_active`).
  - `unknown_keys(&raw)` returns anything not in the set, sorted for
    deterministic stderr output.
  - When `SIFT_DEBUG_UNKNOWNS` is set (any value), `parse()` calls it
    and writes `sift-hook: unknown payload keys: <list>` to stderr
    alongside the normal flow. The hook path is unchanged; the event
    is consumed normally.
  - Off by default so hot-path latency is one env-var lookup per
    hook fire. Enabling it for a single shell session
    (`export SIFT_DEBUG_UNKNOWNS=1`) is enough for a dogfood sweep.

  What's deliberately not done:
  - Not a CLI `--debug-unknowns` flag per binary. Five hook binaries
    would each need clap; an env var covers all of them at once and
    doesn't complicate the hook config JSON users maintain.
  - Not yet `#[serde(other)]` fallthroughs on the typed fields —
    all HookEvent fields are already `Option<T>` so they already
    tolerate missing. Nested enum drift would be the next pass;
    no concrete drift cases yet.
  - Not monthly format-drift CI (Phase 3.5 subplan 3) — no CI in
    this repo at all yet.

### Miscellaneous

- Cargo fmt
- Drop redundant dev-dependencies in sift-cli and sift-hook
Cargo dedupes deps across [dependencies] and [dev-dependencies], so
  listing the same crate in both is dead manifest noise. Caught in
  audit-loop iter 6 (sift-cli) and iter 7 (sift-hook):

  sift-cli/Cargo.toml dev-deps lost: chrono, serde_json, sift-core
    All three were already declared in [dependencies].

  sift-hook/Cargo.toml dev-deps lost: serde_json
    Already declared in [dependencies].

  No behavior change; cleaner manifest invites fewer copy-paste errors
  when adding future dev-only deps.
- Rename crates for crates.io publishability
sift, sift-core, sift-cli are squatted on crates.io. Rename the
  published package names while preserving every import via [lib] name:

  - sift-cli  -> sift-tui    (user-facing binary crate; binary still `sift`)
  - sift-core -> siftcore    ([lib] name = "sift_core", imports untouched)
  - sift-tui  -> sift-render (internal TUI lib; [lib] name = "sift_tui")

  Add version fields to path deps so the workspace is publish-ready.
  No source changes; 190 tests pass, clippy clean. Actual crates.io
  publish stays gated on CI wire-up + CARGO_REGISTRY_TOKEN (deferred).

### Refactoring

- Audit-fix sweep — techdebt, simplify, security
- Uniform Paths::clone reuse across hook handlers and CLI commands
Five sites had the same pattern: build Paths::new(&project_root) once,
  store it as `paths`, then immediately throw away a freshly-built second
  Paths::new() into Session::open_current. The audit loop's iter-4
  sweep caught it in the three hook handlers, iter 6 caught it in
  cmd_revert, and iter 7 caught the last one in cmd_sweep.

  cmd_status was already doing the right thing with paths.clone() —
  making the rest match it. Paths derives Clone (it's just a wrapped
  PathBuf), so .clone() is the cheapest way to consume the same value
  twice without re-stat'ing or re-allocating.

  No behavior change. 163 tests pass workspace-wide, clippy clean.

### Style

- Add rustfmt.toml and format workspace
The repo had no rustfmt.toml and the source had drifted from rustfmt's
  defaults (no CI fmt gate enforced it). Add rustfmt.toml matching the
  sibling repos (max_width = 100, use_field_init_shorthand = true) and run
  `cargo fmt` across the workspace so the upcoming CI fmt job passes.

  Pure formatting — no behavior change. 190 tests pass, clippy clean.

### Testing

- Add cold-start latency benchmark (ignored by default)

### Hook

- Add payload module for parsing hook event JSON from stdin
- Add session-start and stop subcommands with integration tests
- *(stop)* Only suppress errors when current symlink is absent
Previously 'Err(_) => return Ok(())' swallowed any open_current error,
  conflating 'no active session' (benign) with 'symlink corrupted'
  (unusual but worth surfacing). Check symlink_metadata first: if the
  symlink is genuinely absent the stop hook is a noop; otherwise propagate
  the open_current error to the user via anyhow's stderr chain.
- Add user-prompt subcommand with strict-mode exit-2 gate
- Add pre-tool and post-tool subcommands with staging/correlation
- *(post_tool)* Propagate snapshot corruption instead of silent default
snap.get() errors when a blob is missing or fails integrity check (the
  snapshot store quarantines the bad blob on read). The previous code
  used unwrap_or_default, which silently substituted empty bytes and
  computed a wildly wrong diff. Using ? now surfaces corruption to the
  user via a visible hook failure — the staging record remains in place
  for retry/investigation rather than producing a misleading ledger
  entry.

### Scaffold

- Workspace with sift-core, sift-hook, sift-cli, sift-tui

