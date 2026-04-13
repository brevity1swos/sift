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

pub fn run(cwd: &Path, global: bool, tool: &str) -> Result<()> {
    let target = ToolTarget::from_str(tool)?;

    match target {
        ToolTarget::Claude => init_claude(cwd, global)?,
        ToolTarget::Gemini => init_gemini(cwd, global)?,
        ToolTarget::Cline => init_cline(cwd)?,
    }

    // Add .sift/ to .gitignore (project-level only).
    if !global {
        ensure_gitignore(cwd)?;
    }

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
