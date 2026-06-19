# Changelog

All notable changes to this project will be documented in this file.

## [0.1.1] - 2026-06-19

### Bug Fixes

- *(cli)* Expose --json on status
- *(cli)* Expose --json on diff
- *(cli)* Accept --json on state for agent-guide consistency

### Documentation

- *(cli)* Correct state --json doc comment (no-op, not 'forces json')
- Reconcile agent-guide with real --json surface; guard with test
- Add every_read_command_emits_json_under_json_flag integration test
    that discovers a pending entry id, then asserts status/list/log/
    history/fsck/state/diff each emit parseable JSON under --json.
  - Fix Principle #1 in agent-guide: --format json is only accepted by
    state and export (their default); all other read/query commands use
    --json. Update wording to reflect the actual flag surface.
  - Copy docs/agent-guide.md → crates/sift-cli/agent-guide.md;
    confirmed byte-identical (diff empty).
  - suite-conventions.md unchanged: no section governs agent-facing flags.
- *(agent-guide)* Add status --json active-field note and diff --json example
Document the two-shape contract for `sift status --json` (active:true
  object vs {"active":false} singleton) so agents know to check the
  `active` field before accessing other fields. Add a `sift diff <id>
  --json` cookbook entry showing the `unified` string field for
  machine-parseable diff output.

### Refactoring

- *(core)* Extract tally() for accepted/reverted counts
Three sites (status, history, stop summaries) ran the same two-pass ledger
  filter. Consolidate into one siftcore helper; Pending/Edited count toward
  neither, matching the replaced filters.

### Testing

- *(cli)* Cover doctor --json in the read-command JSON guard


## [0.1.0] - 2026-06-06

### Bug Fixes

- *(cli)* Cap gc --days at u16 and move !apply inversion into handler
- Change days arg type from u32 to u16 to prevent chrono::Duration::days
    panic on very large values (u16::MAX ~= 179 years, more than enough)
  - Change cmd_gc::run signature from dry_run to apply, matching cmd_sweep;
    the inversion now lives inside the handler rather than the call site
- *(core)* Truncate pending_changes.jsonl in rewrite_pending to prevent tombstone bloat
After the TUI edit flow calls rewrite_pending_entries, previously-finalized
  entries' tombstones in pending_changes.jsonl become orphans. They are
  harmless to correctness (apply_changes ignores them) but accumulate
  forever. Since rewrite_pending writes a post-fold view, the change file
  is logically empty at that point — truncate it.
- *(security)* Block path traversal in sift fsck --session and bound ledger record size
Two findings from a /security-scan over the recently shipped fsck
  surface. Both are real but bounded — sift is terminal-native with no
  network or auth surface, so impact is "user can mess up their own
  filesystem if they pass an attacker-supplied flag" rather than a
  remote-exploitable issue.

  sift fsck --session traversal (MEDIUM)
    `Paths::session_dir(id).join(id)` uses `PathBuf::join`, which does
    NOT block `..` traversal. A user running
    `sift fsck --session "../../etc" --repair` would walk outside the
    sessions dir. Repair renames files via `fs::rename` (and writes a
    `.bad.<ulid>` archive next to the original), so the worst case is
    silent file moves under the user's own permissions in another
    project's `.sift/` or wherever the path resolved.

    validate_session_id rejects empty ids, anything containing `/` or
    `\`, the literal `.` and `..`, and any character outside
    `[A-Za-z0-9_-]` — the charset Session::create produces (timestamp
    ids look like `2026-04-19-144125` with optional `-N` collision
    suffix). 5 tests cover canonical id, collision-suffix id, traversal
    attempts (.., ../etc, foo/bar, foo\\bar), empty, and special
    characters (space, colon, command-substitution-like).

  fsck unbounded record size (LOW-MEDIUM)
    `read_until_newline` read byte-by-byte until `\n` or EOF, with no
    upper bound. A pathological / corrupt ledger with a 10GB single line
    (no newline) would OOM the process. Real ledger entries are well
    under 8KB; introduce `MAX_RECORD_BYTES = 1MB` as a generous ceiling
    and bail with a clear "may be corrupt or adversarial" error if a
    record exceeds it.

    This protects sift fsck even when the ledger has been hand-crafted
    to be hostile (e.g., a previous adversarial `sift-hook` write that
    never terminated its line). Caller sees a clean `Result::Err`
    instead of OOM-killer.

  163 tests pass workspace-wide, clippy clean. test module in
  cmd_fsck.rs moved to end-of-file to satisfy clippy's
  items_after_test_module lint.

### Features

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
- Fuzzy dup detection, session history, tmux watch command
Batch 3 features:
  - Fuzzy duplicate detection in sweep: pairwise similarity scoring via
    similar::TextDiff::ratio(). Files >80% similar are flagged as
    near-duplicates. O(n^2) but n is typically <50 per session.
  - sift history: lists all past sessions with timestamps, write counts,
    accepted/reverted/pending stats, current-session marker. --json flag.
  - sift watch: opens sift review in a new tmux split pane. Falls back
    gracefully if tmux is not available.
  - FuzzyDuplicate variant added to SweepReason with similarity percentage.
  - policy.yml support (from previous commit) with serde_yaml_ng dep.
- *(cli)* Add sift gc command for session garbage collection
Wires sift_core::gc::collect into a new `sift gc` subcommand.
  Dry-run by default; pass --apply to actually delete. --days sets the
  retention window (default 7). Reports skipped open/corrupt counts.
- Add sift gc --compact to fold JSONL status changes
Over long sessions, pending_changes.jsonl and ledger_changes.jsonl
  accumulate status-change tombstones. `sift gc --compact` rewrites the
  current session's pending.jsonl and ledger.jsonl from their
  folded-and-filtered views and removes the changes side-files.

  Adds Store::compact_pending() and Store::compact_ledger(), wired via
  the --compact flag on the existing gc command.
- *(core)* Agx sibling detection, sift doctor, sift fsck, session transcript path
Ships Phase 1 tracks A (agx synergy foundations) and B (fsck durability)
  at the core + CLI layer. TUI integration lands in a follow-up commit.
- *(cli)* --path filter on sift list and sift log
Both commands accept `--path <SUBSTRING>` (case-insensitive). On `sift
  log` it answers "what happened to this file across all the turns of
  this session?" — the foundation for the eventual Phase 5.2 `sift blame`
  command. On `sift list` it lets users narrow review to a directory or
  file pattern when there's a long pending list.

  Implementation is a `retain` over the loaded entries — no schema or
  storage change. Filter applies after the existing `--turn` filter on
  list, so they compose ("--turn 5 --path src/").

  E2E test seeds three writes across `src/`, `tests/`, and `docs/`,
  then asserts `--path src` returns only the src entry and `--path DOCS`
  (uppercase) still matches docs/c.md.

  README usage block gains two example lines.
- *(core+cli)* Sift state --at-turn N (Phase 1.7.1 — snapshot oracle foundation)
Ships the diff primitive Phase 1.7 is built around: pick any turn N,
  get the file world at that point as a path → SHA-1 JSON map. Compose
  two calls (turns A and B) and the symmetric difference + per-path
  hash comparison answers "what changed in the file world between
  these two turns?" — the question nothing else in the AI-dev workflow
  exposes (git is commit-grain; the agent transcript records intent,
  not state; existing diff tools operate on file pairs).
- *(core+cli)* Sift export --format json (Phase 1.7.2 — publish surface)
Schema-stable session-export contract that downstream consumers (agx
  overlay rendering, eval scripts, third-party tools) read. JSON-first
  posture per the Phase 1.7 agent-as-user reframe — text formats (md,
  patch, bundle) are reserved for Phase 4.3.
- *(cli)* Sift accept --by-commit <ref> (Phase 1.7.3 active CLI)
Closes the git/sift grain gap on the active CLI side: after
  `git commit`, accept every pending entry whose path is in the
  commit AND whose post-state hash matches the file's current
  content. Diverged entries (file was edited between the agent's
  write and the commit) stay pending with a hint pointing at the
  exact path + recorded vs current short-hashes — the divergence
  is the case where the user's attention actually matters.

  The passive half (`sift init` installing a `.git/hooks/post-commit`
  that calls `sift accept --by-commit HEAD --apply --quiet` for the
  "sift is invisible to the user" UX the agent-as-user reframe
  called for) lands in a follow-up commit.
- *(cli)* Sift init installs post-commit hook by default (Phase 1.7.3 passive mode)
Completes Phase 1.7.3 and the agent-as-user reframe that drove it.
  With the active CLI (`sift accept --by-commit <ref>`) already shipped,
  this commit makes the common case **invisible**: `sift init` writes a
  `.git/hooks/post-commit` that calls the active command with
  `--apply --quiet`. The user never types a sift command; they
  `git commit` normally, and sift silently settles matching pending
  entries so the ledger stays consistent with git's view of reality.

  UX per the conversation that produced Phase 1.7: opt-OUT, not opt-in.
  `sift init` installs the hook by default. `sift init --manual-accept`
  skips it for power users who want `sift accept` to remain an explicit
  step (e.g., for active-review workflows or when another tool already
  manages post-commit).
- *(cli)* Complete agent-as-user UX — ai-help, CLAUDE.md section, doctor hook status
Ships the three polish items that round out Phase 1.7's agent-as-user
  reframe. Each closes a specific self-discovery gap: the agent learns
  sift's commands without per-session briefing, the user learns whether
  the auto-accept plumbing is wired up correctly, and running
  `sift init` now gives an agent enough CLAUDE.md context to operate
  sift on the user's behalf from the next conversation onward.

  sift ai-help (new subcommand)
    - `include_str!("../../../docs/agent-guide.md")` embeds the guide
      in the binary at build time; no file dependency at runtime.
    - Raw dump to stdout — no pager, no colorization. Agents parse
      this; users who want paging do `sift ai-help | less`.
    - An agent in an unfamiliar project can run `sift ai-help` and
      learn the command cookbook ("when user says X, run Y") without
      having to resolve a filesystem path to the sift repo.

  sift init writes CLAUDE.md section (on by default)
    - New `ensure_claude_md_section(cwd)` helper. Runs after
      `ensure_gitignore` in the project-level init path. Skipped
      with `--no-claude-md` (opt-out, same ergonomics as
      `--manual-accept` for the post-commit hook).
    - `CLAUDE_MD_SECTION_TEMPLATE` is a short block (~10 lines)
      wrapped in `<!-- SIFT_MANAGED_SECTION -->` markers. The
      content is a five-row cookbook pointing at `sift ai-help`
      for the full reference. Deliberately short — the discovery
      anchor belongs in CLAUDE.md; the detail belongs in the guide.
    - Idempotent via the marker: re-running `sift init` sees the
      existing `<!-- SIFT_MANAGED_SECTION -->` and prints "already
      has a sift section" rather than duplicating.
    - Preserves user content: appends to existing CLAUDE.md rather
      than overwriting. Creates the file if absent.
    - Main.rs `Init` variant gains `no_claude_md: bool`; help text
      on `Init` now mentions both the hook and the CLAUDE.md
      write so the default behavior is discoverable from
      `sift init --help`.

  sift doctor reports post-commit hook status
    - New `PostCommitHookStatus` enum with four kebab-cased variants:
      `sift-managed`, `other-tool`, `not-installed`, `no-git-repo`.
      Serialized in `--json` output under `environment.post_commit_hook`.
    - `probe_post_commit_hook(cwd)` reads `.git/hooks/post-commit`
      and looks for the `SIFT_MANAGED_HOOK=1` marker (same marker
      `cmd_init::install_post_commit_hook` writes, so the two
      stay in sync).
    - Text render adds a one-liner per state:
      - sift-managed → "installed (sift-managed; commits auto-accept)"
      - other-tool   → "present but not sift's (run `sift init`
        for guidance)"
      - not-installed → "not installed (run `sift init` to enable
        auto-accept on commit)"
      - no-git-repo  → "n/a (no .git directory)"
    - Existing JSON-shape test updated to assert the new field
      serializes with `sift-managed` as the kebab-case variant name.

  Live smoke on this session:
    - `sift doctor` now closes with
      `post-commit hook: not installed (run \`sift init\` to enable
      auto-accept on commit)`
    - `sift ai-help` emits the embedded guide starting at
      `# Agent guide for sift`

  182 tests pass workspace-wide (no new unit tests this commit —
  behavior was verified via smoke; adding `ai-help` / CLAUDE.md-section
  tests would be duplicative with the existing init hook tests and
  the doctor shape test). Clippy clean.

  Phase 1.7 fully shipped. The fresh-install UX:

      cd my-project
      sift init
      # Claude / Gemini / Cline hooks wired ✓
      # .sift/ in .gitignore        ✓
      # .git/hooks/post-commit      ✓  (auto-accept on commit)
      # CLAUDE.md sift section      ✓  (agent self-discovers commands)

  ...and from the next conversation: "what did you change in turn 7?"
  Just works.

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
- Share sibling probe + simplify TUI help bar
Three cleanup wins from a /techdebt + /simplify pass over the recently
  shipped Phase 1 + v0.4 surface.
- *(fsck)* Plumb records_dropped through rewrite_file
Picks up the MEDIUM finding deferred from the iter-1 sweep:
  `Rewrite::records_dropped` was hardcoded to 0 with a comment about
  "future versions may track drops explicitly." Caller already knows
  the parsed-vs-kept delta — passing it through is one line each at
  the two call sites in `repair_session`.

  Now `sift fsck --repair` reports `kept N record(s), dropped M, archive
  = ...` instead of `kept N record(s), archive = ...`. The dropped
  count makes the silent-data-loss surface visible: a duplicate-id
  collapse drops the duplicate; an orphan-tombstone drop removes the
  stale change row. Both are recoverable from the .bad.<ulid> archive,
  but the user shouldn't have to dig into the archive to learn how
  much was discarded.

  Existing repair test strengthened: `records_dropped == 1` for the
  duplicate-id fixture (parsed 2, kept 1).

  163 tests pass, clippy clean.
- *(cli)* Reuse paths in cmd_diff, drop guarded unwraps in cmd_accept
Two LOW-severity clarity fixes from an iter-3 sweep over the
  previously-unaudited CLI surface (cmd_diff, cmd_accept).
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

- Add end-to-end full-turn integration test across hook + CLI
- E2E tests for sift gc dry-run, apply, and compact

### Cli

- Add list and log subcommands with JSON output
- Add diff, accept, revert with prefix/turn/all targeting
- Add sweep, mode, and review subcommands
- Add status default command + ls/d/ok/undo aliases
- *(status)* Show entry ID prefix so users can copy-paste into sift d
- *(diff)* Auto-pipe through $PAGER when output exceeds terminal height
- *(revert)* Allow reverting accepted entries from the ledger
- *(revert)* Bulk targets only touch pending, specific ID also checks accepted
- Accept both turn1 and turn-1 syntax, extract is_bulk_target helper
- Improve empty-state messages for ok/undo commands
- Add sift init with --tool claude|gemini|cline and --global

### Scaffold

- Workspace with sift-core, sift-hook, sift-cli, sift-tui

