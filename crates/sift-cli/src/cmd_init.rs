//! `sift init` — auto-wire hooks into the project (or globally).

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolTarget {
    Claude,
    Gemini,
    Cline,
}

impl ToolTarget {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "claude" => Ok(Self::Claude),
            "gemini" => Ok(Self::Gemini),
            "cline" => Ok(Self::Cline),
            other => anyhow::bail!(
                "unknown tool '{other}' — expected 'claude', 'gemini', or 'cline'"
            ),
        }
    }
}

pub fn run(
    cwd: &Path,
    global: bool,
    tool: &str,
    manual_accept: bool,
    no_claude_md: bool,
) -> Result<()> {
    let target = ToolTarget::from_str(tool)?;

    match target {
        ToolTarget::Claude => init_claude(cwd, global)?,
        ToolTarget::Gemini => init_gemini(cwd, global)?,
        ToolTarget::Cline => init_cline(cwd)?,
    }

    // Project-level init also gets .sift/ in .gitignore, the
    // post-commit hook (unless opted out), and the CLAUDE.md
    // sift section (unless opted out) so agents discover sift's
    // commands without user briefing.
    if !global {
        ensure_gitignore(cwd)?;
        if !manual_accept {
            install_post_commit_hook(cwd)?;
        }
        if !no_claude_md {
            ensure_claude_md_section(cwd)?;
        }
    }

    Ok(())
}

/// Append a sift-aware section to `CLAUDE.md` (or create it) so the
/// agent learns sift's command cookbook without per-session user
/// briefing. Idempotent via the `<!-- SIFT_MANAGED_SECTION -->`
/// marker pair; existing user-authored CLAUDE.md content is
/// preserved verbatim.
fn ensure_claude_md_section(cwd: &Path) -> Result<()> {
    let path = cwd.join("CLAUDE.md");
    let existing = fs::read_to_string(&path).unwrap_or_default();

    if existing.contains(SIFT_MARKER_OPEN) {
        println!("  CLAUDE.md already has a sift section");
        return Ok(());
    }

    let mut content = existing.clone();
    if !content.is_empty() && !content.ends_with("\n\n") {
        if !content.ends_with('\n') {
            content.push('\n');
        }
        content.push('\n');
    }
    content.push_str(CLAUDE_MD_SECTION_TEMPLATE);

    fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
    if existing.is_empty() {
        println!("  created CLAUDE.md with sift section");
    } else {
        println!("  appended sift section to CLAUDE.md");
    }
    Ok(())
}

const SIFT_MARKER_OPEN: &str = "<!-- SIFT_MANAGED_SECTION -->";

/// The CLAUDE.md section sift writes. Kept short on purpose — the
/// full agent-facing reference lives in `docs/agent-guide.md` and is
/// emitted by `sift ai-help`. This section is the discovery anchor:
/// just enough so the agent knows what to reach for when the user
/// asks a file-history question.
const CLAUDE_MD_SECTION_TEMPLATE: &str = concat!(
    "<!-- SIFT_MANAGED_SECTION -->\n",
    "## File history (sift)\n",
    "\n",
    "This project uses [sift](https://github.com/brevity1swos/sift)\n",
    "to record every file you write, keyed by conversation turn.\n",
    "When the user asks about file history, reach for these commands\n",
    "(full cookbook: run `sift ai-help` or see `docs/agent-guide.md`\n",
    "in the sift repo):\n",
    "\n",
    "- \"what did you change in turn N?\" → `sift list --turn N --json`\n",
    "- \"revert that\" → find id via `sift list --json`, then `sift undo <id-prefix>`\n",
    "- \"what's different between turn 5 and turn 8?\" → compose two `sift state --at-turn N --format json` calls and diff\n",
    "- \"what happened to src/foo.rs?\" → `sift log --path src/foo.rs --json`\n",
    "- \"what's still pending?\" → `sift status --json`\n",
    "\n",
    "After `git commit`, the post-commit hook auto-accepts matching\n",
    "pending entries — no manual `sift accept` needed in the common\n",
    "case. Divergent entries (file edited between agent write and\n",
    "commit) stay pending and are worth surfacing to the user.\n",
    "<!-- /SIFT_MANAGED_SECTION -->\n",
);

/// Install `.git/hooks/post-commit` that runs
/// `sift accept --by-commit HEAD --apply --quiet` after each commit.
/// Safe in three senses:
///
/// 1. **No git repo** → silently skipped with a note. `sift init` works
///    outside a git repo; the hook just can't be installed there.
/// 2. **Existing sift-managed hook** → treated as already-installed.
///    Detected by the `SIFT_MANAGED_HOOK=1` marker line inside the
///    script.
/// 3. **Existing non-sift hook** → refuses to overwrite. Prints a
///    suggestion to manually add
///    `sift accept --by-commit HEAD --apply --quiet` to whatever
///    hook framework the user is using (husky, lefthook, pre-commit,
///    a custom script, …).
fn install_post_commit_hook(cwd: &Path) -> Result<()> {
    let hooks_dir = cwd.join(".git").join("hooks");
    if !cwd.join(".git").is_dir() {
        println!(
            "  no .git/ directory — skipping post-commit hook install \
             (run `sift init --manual-accept` to silence this note in the future)"
        );
        return Ok(());
    }
    fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("creating {}", hooks_dir.display()))?;

    let hook_path = hooks_dir.join("post-commit");
    if hook_path.exists() {
        let existing = fs::read_to_string(&hook_path).unwrap_or_default();
        if existing.contains("SIFT_MANAGED_HOOK=1") {
            println!("  post-commit hook already installed (sift-managed)");
            return Ok(());
        }
        println!(
            "  post-commit hook exists and is not sift-managed — not overwriting."
        );
        println!(
            "  to enable auto-accept-on-commit, add this line to your existing hook:"
        );
        println!("      sift accept --by-commit HEAD --apply --quiet");
        return Ok(());
    }

    let script = POST_COMMIT_HOOK_TEMPLATE;
    fs::write(&hook_path, script)
        .with_context(|| format!("writing {}", hook_path.display()))?;
    make_executable(&hook_path)?;
    println!(
        "  installed post-commit hook at {} (commits will auto-accept matching sift entries)",
        hook_path.display()
    );
    Ok(())
}

/// The post-commit hook script. The `SIFT_MANAGED_HOOK=1` marker is
/// how `sift init` on a future run detects that it owns this file
/// (and may safely regenerate it) vs a user-owned hook (which must
/// not be overwritten).
///
/// The script is silent on success (via `--quiet`) so normal commits
/// don't spam the terminal. On divergence or error, `sift accept`
/// exits non-zero and git will print its output — that's the right
/// time to surface something to the user.
const POST_COMMIT_HOOK_TEMPLATE: &str = "#!/bin/sh
# Installed by `sift init`. This hook runs after every `git commit` and
# auto-accepts pending sift ledger entries whose post-state matches the
# committed file content. Diverged entries (file edited between agent
# write and commit) stay pending with a hint for manual review.
#
# To regenerate: `sift init`. To disable: delete this file, or run
# `sift init --manual-accept` to skip the auto-install.
#
# SIFT_MANAGED_HOOK=1
exec sift accept --by-commit HEAD --apply --quiet
";

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)
        .with_context(|| format!("stat {}", path.display()))?
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)
        .with_context(|| format!("chmod 755 {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    // Windows: git runs .sh hooks through the bundled shell regardless
    // of executable bits, so nothing to do here.
    Ok(())
}

fn init_claude(cwd: &Path, global: bool) -> Result<()> {
    let settings_dir = if global {
        dirs_for_global(".claude")
    } else {
        cwd.join(".claude")
    };
    let settings_path = settings_dir.join("settings.json");

    fs::create_dir_all(&settings_dir)
        .with_context(|| format!("creating {}", settings_dir.display()))?;

    let hooks_json = serde_json::json!({
        "hooks": {
            "SessionStart": [{
                "matcher": "",
                "hooks": [{"type": "command", "command": "sift-hook session-start"}]
            }],
            "UserPromptSubmit": [{
                "matcher": "",
                "hooks": [{"type": "command", "command": "sift-hook user-prompt"}]
            }],
            "PreToolUse": [{
                "matcher": "Write|Edit|MultiEdit|Bash",
                "hooks": [{"type": "command", "command": "sift-hook pre-tool"}]
            }],
            "PostToolUse": [{
                "matcher": "Write|Edit|MultiEdit|Bash",
                "hooks": [{"type": "command", "command": "sift-hook post-tool"}]
            }],
            "Stop": [{
                "matcher": "",
                "hooks": [{"type": "command", "command": "sift-hook stop"}]
            }]
        }
    });

    write_or_merge_json(&settings_path, hooks_json, "Claude Code")?;
    Ok(())
}

fn init_gemini(cwd: &Path, global: bool) -> Result<()> {
    let settings_dir = if global {
        dirs_for_global(".gemini")
    } else {
        cwd.join(".gemini")
    };
    let settings_path = settings_dir.join("settings.json");

    fs::create_dir_all(&settings_dir)
        .with_context(|| format!("creating {}", settings_dir.display()))?;

    let hooks_json = serde_json::json!({
        "hooks": {
            "BeforeTool": [{
                "matcher": "",
                "hooks": [{"type": "command", "command": "sift-hook pre-tool"}]
            }],
            "AfterTool": [{
                "matcher": "",
                "hooks": [{"type": "command", "command": "sift-hook post-tool"}]
            }],
            "SessionStart": [{
                "matcher": "",
                "hooks": [{"type": "command", "command": "sift-hook session-start"}]
            }],
            "SessionEnd": [{
                "matcher": "",
                "hooks": [{"type": "command", "command": "sift-hook stop"}]
            }]
        }
    });

    write_or_merge_json(&settings_path, hooks_json, "Gemini CLI")?;
    Ok(())
}

fn init_cline(cwd: &Path) -> Result<()> {
    // Cline uses .clinerules/hooks/ directory with script files.
    let hooks_dir = cwd.join(".clinerules").join("hooks");
    fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("creating {}", hooks_dir.display()))?;

    let scripts = [
        ("pre-tool-use.sh", "#!/bin/sh\nsift-hook pre-tool\n"),
        ("post-tool-use.sh", "#!/bin/sh\nsift-hook post-tool\n"),
        ("task-start.sh", "#!/bin/sh\nsift-hook session-start\n"),
        ("task-complete.sh", "#!/bin/sh\nsift-hook stop\n"),
    ];

    for (name, content) in &scripts {
        let path = hooks_dir.join(name);
        if path.exists() {
            println!("  skip {name} (already exists)");
            continue;
        }
        fs::write(&path, content)
            .with_context(|| format!("writing {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        }
        println!("  wrote {name}");
    }

    println!("sift: Cline hooks configured in .clinerules/hooks/");
    Ok(())
}

/// Read existing settings.json, merge sift hooks in, write back.
/// If the file doesn't exist, create it with just the hooks.
/// If hooks already point to sift-hook, skip silently.
fn write_or_merge_json(
    path: &Path,
    new_hooks: serde_json::Value,
    tool_name: &str,
) -> Result<()> {
    let mut existing: serde_json::Value = if path.exists() {
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", path.display()))?
    } else {
        serde_json::json!({})
    };

    // Check if hooks are already configured.
    let existing_hooks = existing.get("hooks");
    let new_hooks_obj = new_hooks.get("hooks");
    if let (Some(existing_h), Some(new_h)) = (existing_hooks, new_hooks_obj) {
        let existing_str = serde_json::to_string(existing_h).unwrap_or_default();
        if existing_str.contains("sift-hook") {
            println!("sift: {tool_name} hooks already configured in {}", path.display());
            return Ok(());
        }
        // Merge: add new hook entries alongside existing ones.
        if let (Some(_), Some(new_map)) = (existing_h.as_object(), new_h.as_object()) {
            let merged_hooks = existing
                .get_mut("hooks")
                .unwrap()
                .as_object_mut()
                .unwrap();
            for (key, value) in new_map {
                if !merged_hooks.contains_key(key) {
                    merged_hooks.insert(key.clone(), value.clone());
                } else {
                    // Hook event already has entries — append sift hooks to the array.
                    if let Some(arr) = merged_hooks.get_mut(key).and_then(|v| v.as_array_mut()) {
                        if let Some(new_arr) = value.as_array() {
                            for item in new_arr {
                                let item_str = serde_json::to_string(item).unwrap_or_default();
                                if !item_str.contains("sift-hook") || !existing_str.contains("sift-hook") {
                                    arr.extend(new_arr.iter().cloned());
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        // No existing hooks — just set them.
        if let Some(obj) = existing.as_object_mut() {
            if let Some(hooks) = new_hooks.get("hooks") {
                obj.insert("hooks".to_string(), hooks.clone());
            }
        }
    }

    let text = serde_json::to_string_pretty(&existing)?;
    fs::write(path, &text)
        .with_context(|| format!("writing {}", path.display()))?;
    println!("sift: {tool_name} hooks configured in {}", path.display());
    Ok(())
}

fn ensure_gitignore(cwd: &Path) -> Result<()> {
    let gitignore = cwd.join(".gitignore");
    if gitignore.exists() {
        let content = fs::read_to_string(&gitignore)?;
        if content.contains(".sift") {
            return Ok(());
        }
        let mut f = fs::OpenOptions::new().append(true).open(&gitignore)?;
        use std::io::Write;
        writeln!(f, "\n# sift session data\n/.sift/")?;
        println!("  appended .sift/ to .gitignore");
    } else {
        fs::write(&gitignore, "# sift session data\n/.sift/\n")?;
        println!("  created .gitignore with .sift/");
    }
    Ok(())
}

fn dirs_for_global(subdir: &str) -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~"))
        .join(subdir)
}
