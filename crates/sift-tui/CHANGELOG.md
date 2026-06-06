# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] - 2026-06-06

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
- *(tui)* T keybind agx handoff, Enter/Space accept per suite conventions
Ships Phase 1 track A4 (session-level agx jump) and track C partial
  (additive-only keymap migration, breaking flip deferred to v0.4).

  `t` keybind (sift review → agx)
    - Reads `session/meta.json::transcript_path` (populated in the
      companion core commit) and spawns `agx <transcript>` as a
      subprocess when sift_core::agx::detect() reports the sibling is
      installed and contract-compatible
    - Session-level jump only — agx 0.1.x has no
      `--jump-to <path>:<step>` flag (verified against agx/src/timeline.rs
      and tui.rs). Honest help text, no step-level claim.
    - Silent degrade per suite-conventions §6 rule 2: missing agx → status
      bar hint pointing at install docs; missing transcript → hint that a
      post-v0.3 session is needed; spawn failure → status surfaces the
      error. Never hard-fails, never quits the TUI.
    - `handle_jump_to_agx` in sift-tui/lib.rs mirrors the
      suspend → spawn → resume pattern already established by
      `handle_edit` (disable raw mode → LeaveAlternateScreen → spawn →
      enable raw mode → EnterAlternateScreen → clear)

  Keymap additive migration
    - `Enter` and `Space` now accept the current entry, matching
      suite-conventions §1 (the suite-wide primary). `a` retained as a
      compatibility alias for the v0.3 release window; the full flip
      (`a` → annotate, `/` + `n`/`N` search) is deferred to v0.4 because
      moving `a` off accept and onto annotate in the same release creates
      an unavoidable same-key double-meaning.
    - Help bar shows `Enter accept` and `t agx`
    - One-shot `status_msg` field on App; cleared on the next keypress
      so deprecation / degrade hints don't linger

  Tests
    - 5 new sift-tui event tests: Enter/Space/legacy-a each accept,
      revert still works, quit still works, `t` without agx or without
      transcript surfaces a status message, any subsequent keypress
      clears a stale status message
    - `chrono` + `serde_json` added as sift-tui dev+dep deps (serde_json
      needed for meta.json parsing in App::transcript_path, chrono for
      ledger-entry test fixtures)

  Full workspace clippy clean under `-D warnings`. 146 tests pass.
- *(tui)* Complete v0.4 keymap migration and add /-search
Closes the three sift-owned retrofits tracked in
  docs/suite-conventions.md §10. The v0.3 compatibility window was one
  release; this commit removes the legacy bindings and introduces the
  convention-aligned replacements.

  Keymap flip (suite-conventions §1)
    - Drop `a`=accept. Accept is `Enter` / `Space` only. Users on muscle
      memory from v0.3 now land in annotate mode, which is at least
      not-destructive compared to the v0.3 double-meaning we wanted to
      avoid.
    - `a` now opens the annotate prompt (moved from `n`). Matches agx's
      annotation key so the stepwise suite feels coordinated.
    - `n` is freed for search-cycling below.

  Search (new)
    - `/` opens a search prompt (InputMode::Searching). Enter commits the
      query, runs a case-insensitive substring match against each entry's
      path, jumps the cursor to the first hit, and returns to Normal.
      Esc cancels without moving the cursor.
    - `n` / `N` cycle to next / previous match after a committed search,
      wrapping around. Match guards let us run the side-effecting
      `cycle_search` once per keypress without tripping `collapsible_if`.
    - No active search + `n`/`N` surfaces a status hint pointing at `/`.
    - Matches reload when the entry list changes (accept / revert shifts
      rows; rebuild_search_matches runs from `App::reload`).

  UI
    - List rows matching the active search get a cyan `*` marker prefix.
    - Panel title shows `/{query} ({n} match)` when a search is active.
    - Help bar reorganized to the full v0.4 set:
      `Enter accept r evert e dit a note / search n/N match t agx q uit`.

  docs/suite-conventions.md §10
    - Three sift rows moved from "open retrofits" into a new "Closed
      (shipped)" sub-list with the version the retrofit shipped in, per
      §11 ("move the item from §10 into the appropriate earlier section
      and record the retrofit date").

  README
    - Keybind table rewritten for v0.4. Adds a one-line pointer to the
      suite-conventions doc so users learning one tool's keys carry them
      across the other two.

### Miscellaneous

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

### Style

- Add rustfmt.toml and format workspace
The repo had no rustfmt.toml and the source had drifted from rustfmt's
  defaults (no CI fmt gate enforced it). Add rustfmt.toml matching the
  sibling repos (max_width = 100, use_field_init_shorthand = true) and run
  `cargo fmt` across the workspace so the upcoming CI fmt job passes.

  Pure formatting — no behavior change. 190 tests pass, clippy clean.

### Scaffold

- Workspace with sift-core, sift-hook, sift-cli, sift-tui

### Tui

- Add App state, list view, detail pane, and basic keybindings
Replaces the stub in sift-tui with a working ratatui TUI: App struct with
  cursor navigation and reload, split list/detail layout, and a/r/q/j/k
  keybindings. The 'r' keybinding marks entries as Reverted in the ledger
  only; on-disk restoration requires `sift revert` (documented limitation).
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

