#!/usr/bin/env bash
# Populates a throwaway demo project with a realistic sift ledger so
# assets/demo.tape can show real `sift` output. Drives the sift-hook
# pipeline the same way Claude Code / Gemini / Cline would at runtime:
# session-start -> user-prompt (turn bump) -> pre-tool / write / post-tool.
#
# Requires `sift` and `sift-hook` on PATH (e.g. export PATH=$PWD/target/release:$PATH).
set -euo pipefail

DEMO="${SIFT_DEMO_DIR:-/tmp/sift-demo}"
rm -rf "$DEMO"
mkdir -p "$DEMO/src" "$DEMO/tests"
cd "$DEMO"
git init -q

ss()   { echo "{\"cwd\":\"$DEMO\"}" | sift-hook session-start >/dev/null 2>&1; }
turn() { echo "{\"cwd\":\"$DEMO\"}" | sift-hook user-prompt   >/dev/null 2>&1; }
wr() { # wr <relpath> <content>
  local f="$DEMO/$1"
  local evt="{\"cwd\":\"$DEMO\",\"tool_name\":\"Write\",\"tool_input\":{\"file_path\":\"$f\"},\"tool_use_id\":\"tid_$(echo "$1" | tr '/.' '__')\"}"
  echo "$evt" | sift-hook pre-tool  >/dev/null 2>&1
  printf '%s' "$2" > "$f"
  echo "$evt" | sift-hook post-tool >/dev/null 2>&1
}

ss

# Turn 1 — the agent implements login.
turn
wr src/auth.rs $'pub fn login(user: &str, pw: &str) -> bool {\n    verify(user, pw)\n}\n'

# Turn 2 — the agent adds tests, but also creates a sibling file
# instead of editing the original (classic AI slop).
turn
wr src/auth_v2.rs $'// near-duplicate of auth.rs with a small tweak\npub fn login_v2() {}\n'
wr tests/auth_test.rs $'#[test]\nfn test_login() { assert!(true); }\n'

# Turn 3 — the agent leaves a scratch note behind.
turn
wr notes_scratch.md $'TODO: remember to delete this before committing\n'
