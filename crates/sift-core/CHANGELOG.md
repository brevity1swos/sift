# Changelog

All notable changes to this project will be documented in this file.

## [0.1.1] - 2026-06-19

### Refactoring

- *(core)* Extract tally() for accepted/reverted counts
Three sites (status, history, stop summaries) ran the same two-pass ledger
  filter. Consolidate into one siftcore helper; Pending/Edited count toward
  neither, matching the replaced filters.


## [0.1.0] - 2026-06-06

### Bug Fixes

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

### Documentation

- *(core)* Clarify append-only ledger semantics and add edge-case tests
- StatusChange.timestamp is informational; file order is authoritative.
  - finalize may leave duplicate rows under concurrency/crash; readers tolerate.
  - Add test: orphan change for nonexistent id is ignored.
  - Add test: edit flow preserves existing tombstones across rewrite_pending_entries.

### Features

- Policy-gated writes — .sift/policy.yml with allow/review/deny rules
Per-path policy rules enforced in PreToolUse hook:
  - allow: hook exits 0 (default when no policy or no rule matches)
  - review: hook exits 0 but prints a note to stderr
  - deny: hook exits 2, blocking the tool call

  Rules use glob patterns (e.g., 'src/**', '*.sql', '.env*') and are
  evaluated top-to-bottom, first match wins. Policy is loaded from
  .sift/policy.yml via the serde_yaml_ng crate.

  Also adds policy_file() path helper and policy module to sift-core.
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

### Miscellaneous

- Cargo fmt
- Ignore workspace-root scratch files and document SHA-1 hash choice
Two small housekeeping items surfaced during a project audit:

  - Workspace root kept attracting ad-hoc scratch files (`scratch.rs`,
    `src/*.rs`) that weren't workspace members so they never compiled but
    still polluted `git status`. Add defensive gitignore rules for
    `/scratch.rs` and `/src/`.
  - Add a module docstring to `sift-core/src/snapshot.rs` explaining why
    SHA-1 is the right choice for this blob store (session-scoped,
    non-adversarial threat model, integrity via quarantine-on-mismatch).
    Future readers won't have to ask — and won't be tempted to bump to
    SHA-256 without the migration cost analysis the comment summarizes.
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
- *(core)* Append-only ledger via side-file status changes
finalize() and update_ledger_status() no longer rewrite pending.jsonl /
  ledger.jsonl on every status change. Instead, status changes are appended
  to pending_changes.jsonl / ledger_changes.jsonl side-files; readers fold
  them over the bare entries on load.

  Eliminates O(N) rewrites — accept/revert are now O(1) file appends.
  Existing pending.jsonl / ledger.jsonl files remain byte-compatible.

  Also removes the dead-code pending_with_status helper whose semantics
  no longer make sense under the new design.
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
- *(core)* Derive Default for SessionState
The manual `impl Default for SessionState` mirrored exactly what
  `#[derive(Default)]` produces:

  - turn: 0 (u32::default)
  - mode: Mode::Loose (Mode has #[default] on Loose)
  - last_hook_ts: None (Option::default)

  Caught in the audit loop's iter-5 sweep over state.rs. All four
  field types implement Default and the derived behavior is
  indistinguishable. Drops 8 lines of boilerplate.

  163 tests pass, clippy clean.

### Style

- Add rustfmt.toml and format workspace
The repo had no rustfmt.toml and the source had drifted from rustfmt's
  defaults (no CI fmt gate enforced it). Add rustfmt.toml matching the
  sibling repos (max_width = 100, use_field_init_shorthand = true) and run
  `cargo fmt` across the workspace so the upcoming CI fmt job passes.

  Pure formatting — no behavior change. 190 tests pass, clippy clean.

### Testing

- *(core)* Harden orphan-markdown fixtures against fuzzy-dup interference
`detects_orphan_markdown` was flaky: both md files used identical "content"
  content, so the fuzzy-dup detector (added later in c16f64f) matched them
  at ratio=1.0 and flagged `referenced.md` as a FuzzyDuplicate, which
  suppressed its orphan check and left `lonely.md` still flagged as orphan —
  net count of 2, not the expected 1.

  `multiple_orphan_md_files_each_flagged` masked the same bug: its assertion
  only checked entry_ids appeared in the candidate set, not what reason. Both
  files hit ratio=1.0 ("solo" == "solo"), so one entry was actually flagged
  as FuzzyDuplicate rather than OrphanMarkdown — test passed but lied about
  what it verified.

  Fix both tests to use distinct content, and add `matches!` assertions on
  `SweepReason::OrphanMarkdown` so a future detector-interaction regression
  fails loudly instead of silently mutating reason codes.

  Also switch single-letter basenames ("a", "b") to multi-char
  ("alpha", "omega") to avoid substring collisions with other file content
  during the `is_referenced` byte scan.

### Cli

- *(revert)* Allow reverting accepted entries from the ledger

### Core

- Add Paths for .sift/ discovery with sharded snapshot layout
- *(paths)* Return Result from snapshot_path instead of panicking
Panics in hook-critical paths kill the hook process with no chance to
  recover or log. Convert snapshot_path to return anyhow::Result<PathBuf>
  so callers in Tasks 6+ can propagate errors gracefully. Adds tests for
  the short-hex and non-ASCII error branches.
- Add LedgerEntry with Tool/Op/Status enums and serde round-trip tests
- *(entry)* Add PartialEq, rename new_ulid->new_entry_id, u32 diff stats
Apply code review feedback from Task 3:
  - Derive PartialEq, Eq on LedgerEntry so tests can assert_eq whole structs
  - Move new_ulid from associated method to module-level new_entry_id
    (the function does not construct a LedgerEntry, so living on the struct
    was misleading)
  - Change DiffStats fields from usize to u32 for portable wire format
  - Document the Unix-only portability assumption on PathBuf serialization
  - Document the rationale for Tool's PascalCase serde rename (matches
    Claude Code's hook payload wire format; Op/Status stay lowercase
    because they are sift-internal)
- Add Config with Mode enum, TOML load/save, default fallback
- *(config)* Wrap load/save errors with path context
Bare toml parse errors give no hint which config file is broken. Wrap
  all I/O and parse calls with anyhow::Context carrying the path so users
  get an actionable error message. Adds a test asserting the path appears
  in the rendered error chain.
- Add SessionState with atomic rename-based save
- *(state)* Wrap load/save errors with path context
Match the config.rs pattern: every fs::read_to_string, serde_json parse,
  create_dir_all, fs::write, and fs::rename call gets wrapped with
  anyhow::Context carrying the affected path. Also documents the fsync
  trade-off on the rename and adds a test asserting the path appears in
  the rendered error chain on invalid JSON.
- Add SnapshotStore with sha1 content addressing and quarantine on corruption
- *(snapshot)* Collision-free quarantine, include hash in get errors
Apply code review feedback from Task 6:
  - quarantine() now takes expected_hash and names files
    <expected>.<ulid>.bad to prevent silent overwrite of prior forensic
    evidence when repeated corruption hits the same slot
  - get() error context includes the requested hash, not just the path
  - put() documents the exists() TOCTOU as benign (concurrent puts race
    on rename, last rename wins with identical bytes)
  - rename error context mentions potential tmp orphan
  - 4 new tests: missing-blob error, has() happy/sad, two-blob shard
    coexistence, repeated-corruption non-collision
- Add Store with JSONL append/list and malformed-line skipping
- *(store)* Add ReadStats, list_ledger coverage, TOCTOU fix
Apply code review feedback from Task 7:
  - Expose malformed-line skip count via new ReadStats struct and
    list_pending_with_stats / list_ledger_with_stats methods. Existing
    list_pending / list_ledger still return bare Vec for convenience.
  - Replace path.exists() + File::open with a direct match on File::open
    and ErrorKind::NotFound so a concurrent delete no longer turns an
    empty ledger into an I/O error.
  - Document the partial-write worst case precisely: a write missing a
    trailing newline causes BOTH the partial record and the following
    valid record to be counted as one skipped line.
  - Switch all error-context strings from path:? to path.display() for
    consistency with state.rs.
  - Add three new tests: skip count assertion, list_ledger exercising
    the ledger.jsonl branch, and pending_with_status filtering.
- Add Store::finalize and restore_snapshot for accept/revert
- *(store)* Reorder finalize, cover all restore_snapshot branches
CRITICAL fix: finalize() previously called rewrite_pending BEFORE
  append_ledger, so an append_ledger failure (disk full, permission error)
  would lose the entry entirely — it was removed from pending.jsonl but
  never reached ledger.jsonl. Reorder so append_ledger runs first: a
  subsequent rewrite_pending failure leaves the entry duplicated (visible,
  recoverable via fsck) instead of vanished.
- Add Session::create/open_current/close with meta.json and symlink
- *(session)* Atomic writes, unique ids, collision probe
Apply code review feedback from Task 9:
  - Share a single Utc::now() between id and started_at so they cannot
    disagree by a second (regression test asserts id starts with the
    formatted started_at timestamp).
  - reserve_unique_dir() probes base_id, then base_id-1, -2, ..., up to 999
    to guarantee two sessions created in the same second get distinct
    directories instead of silently stomping.
  - Atomic symlink replacement: create tmp symlink, then fs::rename over
    the live 'current'. POSIX rename is atomic on symlink targets, so a
    crash can never leave .sift/current missing.
  - write_json_atomic() helper shared by create and close: tmp+rename
    pattern so meta.json cannot be truncated by a mid-write crash (matches
    the crash-safety pattern already established in state.rs).
  - Derive Debug on Session (needed for Result::unwrap_err in tests).
  - New tests cover: collision probing (distinct ids when base is taken),
    open_current on missing symlink (errors with 'current' in message),
    close on corrupted meta.json (errors with 'meta.json' or 'parsing'),
    id/started_at regression.
- Add diff module with stats() and unified() via similar crate
- *(diff)* Cover empty and newline-less inputs with tests
Edge-case tests pin observable behavior of similar's line-splitting on
  inputs that are empty or unterminated. No implementation change —
  similar already handles these correctly. Pin the semantics before
  callers (TUI diff view, sift diff CLI) proliferate.
- Add correlation key derivation (tool_use_id primary, sha1 fallback)
- *(correlation)* Expect over unwrap, add negative and collision tests
Apply code review feedback from Task 11:
  - Replace unwrap_or_default() on the canonical_json serde call with
    expect() — the error path is unreachable for a serde_json::Value by
    construction, and the silent empty-string fallback previously conflated
    a (hypothetical) serialization failure with a genuinely empty input.
  - Map::with_capacity(m.len()) avoids incremental reallocations while
    building the canonicalized object.
  - keys.sort_unstable() is faster and semantically equivalent for strings.
  - Three new tests: non-string tool_use_id (number + null), different
    tool_inputs produce different hashes (regression guard), and the '|'
    delimiter correctly prevents prefix-concatenation collisions.
- Add sweep with exact-dup, slop-glob, and orphan-markdown heuristics
- *(sweep)* Lexical path clean, canonical-swap, byte search, delete skip
Apply code review feedback from Task 12:
  - BLOCKING: lexical_clean() helper strips Component::CurDir so 'exclude'
    paths with './' prefixes compare equal to WalkDir output. Previously
    an md file passed as './foo.md' would scan itself, find its own
    basename, and falsely clear from the orphan list.
  - Canonical-swap in Rule 1: if the first-seen path for a hash is a slop
    match and a later path is not, promote the later path to canonical
    and flag the slop one as the duplicate. This fixes the case where
    Claude writes foo_v2.py before foo.py and the recommendation should
    point at the scratch copy, not the stable name.
  - is_referenced now uses fs::read + byte window search instead of
    read_to_string + str::contains, avoiding full in-memory allocation
    of binary files that happen to be valid UTF-8.
  - Rule 2 skips Op::Delete entries (a delete for a scratch file should
    not be flagged as slop since it's already being removed).
  - Five new tests: empty input, delete-not-flagged, ./ path handling,
    multiple orphan md files, and dup-direction flip with slop first.

### Scaffold

- Workspace with sift-core, sift-hook, sift-cli, sift-tui

### Tui

- Add edit-before-accept — e key opens post-state in $EDITOR
Press e on a pending entry in sift review. The TUI suspends, opens the
  entry's post-state snapshot in $EDITOR (with correct file extension for
  syntax highlighting). On save:
  - If unchanged: accepts the entry as-is
  - If edited: writes the edited content to the real project file, stores
    a new snapshot, updates the pending entry's hash, and finalizes as
    status=edited

  Also derives project_root and session_id from session_dir path so the
  TUI can access SnapshotStore without API changes.

