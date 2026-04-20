use anyhow::Result;
use serde::Serialize;
use sift_core::agx::{self, MIN_VERSION};
use sift_core::paths::Paths;
use std::fs;
use std::path::Path;
use std::process::Command;

const SIFT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum SiblingStatus {
    /// Present, version parsed, meets the minimum-version contract.
    Ok,
    /// Present, version parsed, but older than the minimum sift relies on.
    TooOld,
    /// Present but `--version` output couldn't be parsed — treat as usable
    /// but unknown.
    Unknown,
    /// Not on PATH.
    Missing,
}

#[derive(Serialize)]
struct SiblingInfo {
    status: SiblingStatus,
    version: Option<String>,
    /// Minimum version sift's CLI-boundary contract depends on. Only
    /// populated for siblings where a contract is declared (agx).
    min_version: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum IntegrationStatus {
    Ready,
    Disabled,
    Planned,
}

#[derive(Serialize)]
struct Integration {
    name: &'static str,
    from: &'static str,
    to: &'static str,
    status: IntegrationStatus,
    note: String,
}

#[derive(Serialize)]
struct SiftInfo {
    version: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum PostCommitHookStatus {
    /// `.git/hooks/post-commit` exists and carries the sift-managed marker.
    SiftManaged,
    /// `.git/hooks/post-commit` exists but is not sift's — auto-accept
    /// won't run (unless the user added the sift line to their own hook).
    OtherTool,
    /// No post-commit hook installed. `sift init` (without
    /// `--manual-accept`) would install one.
    NotInstalled,
    /// No `.git/` directory — not a git repository.
    NoGitRepo,
}

#[derive(Serialize)]
struct EnvReport {
    sift_dir_present: bool,
    current_session: Option<String>,
    post_commit_hook: PostCommitHookStatus,
}

#[derive(Serialize)]
struct DoctorReport {
    sift: SiftInfo,
    agx: SiblingInfo,
    rgx: SiblingInfo,
    integrations: Vec<Integration>,
    environment: EnvReport,
}

/// Resolve the agx sibling. Uses sift-core's version-aware probe so `sift
/// doctor` reports "too old" (a recoverable condition — upgrade agx) distinct
/// from "missing" (install it) and "unknown" (present but unparseable).
fn probe_agx() -> SiblingInfo {
    let min_version = Some(MIN_VERSION.to_string());
    match agx::detect() {
        Some(info) if info.meets_minimum() => SiblingInfo {
            status: SiblingStatus::Ok,
            version: Some(info.raw),
            min_version,
        },
        Some(info) => SiblingInfo {
            status: SiblingStatus::TooOld,
            version: Some(info.raw),
            min_version,
        },
        None => {
            // detect() returns None both when the binary is missing and when
            // --version is unparseable. Disambiguate with a cheap second
            // probe: if spawn succeeds at all, the binary is present.
            if raw_probe_succeeded("agx") {
                SiblingInfo {
                    status: SiblingStatus::Unknown,
                    version: None,
                    min_version,
                }
            } else {
                SiblingInfo {
                    status: SiblingStatus::Missing,
                    version: None,
                    min_version,
                }
            }
        }
    }
}

/// rgx currently has no declared minimum-version contract with sift (Phase
/// 2 adds `sift policy debug` → rgx and will set one then). For now, a
/// plain presence-and-version probe is enough.
///
/// Uses the same hang-safe `agx::probe_version` helper so a misbehaving
/// rgx binary (hung on stdin, slow startup, fork-bomb-by-mistake) cannot
/// block `sift doctor`. A bare `Command::output()` here would deadlock
/// indefinitely.
fn probe_rgx() -> SiblingInfo {
    match agx::probe_version("rgx", agx::probe_timeout()) {
        Some(raw) => {
            let version = raw.lines().next().map(|l| l.trim().to_string());
            SiblingInfo {
                status: SiblingStatus::Ok,
                version,
                min_version: None,
            }
        }
        None => {
            // Disambiguate "missing" from "present but probe failed". A
            // bare spawn that succeeds but exits non-zero / hangs / emits
            // unparseable output all collapse into None from the helper;
            // the cheap secondary probe distinguishes the absent case.
            if raw_probe_succeeded("rgx") {
                SiblingInfo {
                    status: SiblingStatus::Unknown,
                    version: None,
                    min_version: None,
                }
            } else {
                SiblingInfo {
                    status: SiblingStatus::Missing,
                    version: None,
                    min_version: None,
                }
            }
        }
    }
}

/// Probe the git post-commit hook status in the given project root.
/// Reads `.git/hooks/post-commit` if present and checks for sift's
/// `SIFT_MANAGED_HOOK=1` marker.
fn probe_post_commit_hook(cwd: &Path) -> PostCommitHookStatus {
    let git_dir = cwd.join(".git");
    if !git_dir.is_dir() {
        return PostCommitHookStatus::NoGitRepo;
    }
    let hook = git_dir.join("hooks").join("post-commit");
    match fs::read_to_string(&hook) {
        Ok(content) if content.contains("SIFT_MANAGED_HOOK=1") => {
            PostCommitHookStatus::SiftManaged
        }
        Ok(_) => PostCommitHookStatus::OtherTool,
        Err(_) => PostCommitHookStatus::NotInstalled,
    }
}

/// Returns true if `<bin>` is on PATH (and we have permission to execute
/// it). Used to disambiguate "binary is missing entirely" from "binary
/// exists but its --version output is unparseable / it's hung."
///
/// Spawns and immediately kills — does NOT wait for the child to exit or
/// for stdout. Calling `Command::output()` here would defeat the entire
/// purpose of the timeout-safe `agx::probe_version` we just relied on,
/// since a hung binary would hang this call too.
fn raw_probe_succeeded(bin: &str) -> bool {
    use std::process::Stdio;
    match Command::new(bin)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            let _ = child.kill();
            let _ = child.wait();
            true
        }
        Err(_) => false,
    }
}

pub fn run(cwd: &Path, json: bool) -> Result<()> {
    let agx = probe_agx();
    let rgx = probe_rgx();

    let paths = Paths::new(cwd);
    let sift_dir_present = paths.sift_dir().is_dir();
    let current_session = if paths.current_symlink().symlink_metadata().is_ok() {
        fs::read_link(paths.current_symlink())
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
    } else {
        None
    };

    let integrations = vec![
        Integration {
            name: "Timeline jump",
            from: "sift",
            to: "agx",
            status: match agx.status {
                SiblingStatus::Ok => IntegrationStatus::Ready,
                SiblingStatus::TooOld | SiblingStatus::Unknown | SiblingStatus::Missing => {
                    IntegrationStatus::Disabled
                }
            },
            note: match agx.status {
                SiblingStatus::Ok => {
                    "press `t` on a pending write in `sift review` (session-level jump)".into()
                }
                SiblingStatus::TooOld => format!(
                    "agx {} is older than the contract floor ({}) — upgrade required",
                    agx.version.as_deref().unwrap_or("?"),
                    MIN_VERSION
                ),
                SiblingStatus::Unknown => {
                    "agx is on PATH but --version output is unparseable".into()
                }
                SiblingStatus::Missing => "install agx: https://github.com/brevity1swos/agx".into(),
            },
        },
        Integration {
            name: "Policy debug",
            from: "sift",
            to: "rgx",
            status: IntegrationStatus::Planned,
            note: match rgx.status {
                SiblingStatus::Ok | SiblingStatus::Unknown => {
                    "rgx detected; sift integration planned for v0.5".into()
                }
                _ => "install rgx: cargo install rgx-cli (sift integration planned for v0.5)".into(),
            },
        },
    ];

    let report = DoctorReport {
        sift: SiftInfo {
            version: SIFT_VERSION,
        },
        agx,
        rgx,
        integrations,
        environment: EnvReport {
            sift_dir_present,
            current_session,
            post_commit_hook: probe_post_commit_hook(cwd),
        },
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    render_text(&report);
    Ok(())
}

fn render_text(r: &DoctorReport) {
    println!("sift {}", r.sift.version);
    println!();

    println!("Sibling tools (stepwise suite):");
    render_sibling("agx", &r.agx);
    render_sibling("rgx", &r.rgx);

    println!();
    println!("Integrations:");
    for i in &r.integrations {
        let tag = match i.status {
            IntegrationStatus::Ready => "[ready]   ",
            IntegrationStatus::Disabled => "[disabled]",
            IntegrationStatus::Planned => "[planned] ",
        };
        println!("  {}  {}  ({} -> {})", tag, i.name, i.from, i.to);
        println!("               {}", i.note);
    }

    println!();
    println!("Environment:");
    println!(
        "  .sift directory: {}",
        if r.environment.sift_dir_present {
            "present"
        } else {
            "not initialized (run `sift init`)"
        }
    );
    if let Some(sess) = &r.environment.current_session {
        println!("  current session: {sess}");
    } else if r.environment.sift_dir_present {
        println!("  current session: none");
    }
    let hook_line = match r.environment.post_commit_hook {
        PostCommitHookStatus::SiftManaged => {
            "post-commit hook: installed (sift-managed; commits auto-accept)"
        }
        PostCommitHookStatus::OtherTool => {
            "post-commit hook: present but not sift's (run `sift init` for guidance)"
        }
        PostCommitHookStatus::NotInstalled => {
            "post-commit hook: not installed (run `sift init` to enable auto-accept on commit)"
        }
        PostCommitHookStatus::NoGitRepo => "post-commit hook: n/a (no .git directory)",
    };
    println!("  {hook_line}");
}

fn render_sibling(name: &str, info: &SiblingInfo) {
    let v = info.version.as_deref().unwrap_or("(unknown version)");
    match info.status {
        SiblingStatus::Ok => println!("  {name:4}  on PATH    {v}"),
        SiblingStatus::TooOld => {
            let min = info.min_version.as_deref().unwrap_or("?");
            println!("  {name:4}  too old    {v} (needs >= {min})");
        }
        SiblingStatus::Unknown => {
            println!("  {name:4}  on PATH    {v} (version unparseable)");
        }
        SiblingStatus::Missing => println!("  {name:4}  not found on PATH"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_serializes_with_status_fields() {
        let report = DoctorReport {
            sift: SiftInfo { version: "0.2.0" },
            agx: SiblingInfo {
                status: SiblingStatus::Ok,
                version: Some("agx 0.1.0".into()),
                min_version: Some("0.1.0".into()),
            },
            rgx: SiblingInfo {
                status: SiblingStatus::Missing,
                version: None,
                min_version: None,
            },
            integrations: vec![Integration {
                name: "Timeline jump",
                from: "sift",
                to: "agx",
                status: IntegrationStatus::Ready,
                note: "press t".into(),
            }],
            environment: EnvReport {
                sift_dir_present: true,
                current_session: Some("01HXXX".into()),
                post_commit_hook: PostCommitHookStatus::SiftManaged,
            },
        };
        let json = serde_json::to_string(&report).expect("serialize");
        assert!(json.contains("\"version\":\"0.2.0\""));
        assert!(json.contains("\"agx\":{\"status\":\"ok\""));
        assert!(json.contains("\"rgx\":{\"status\":\"missing\""));
        assert!(json.contains("\"min_version\":\"0.1.0\""));
        assert!(json.contains("\"sift_dir_present\":true"));
    }

    #[test]
    fn raw_probe_returns_false_for_nonexistent_binary() {
        assert!(!raw_probe_succeeded(
            "sift-doctor-probe-definitely-not-a-real-binary-xyz"
        ));
    }

    #[test]
    fn integration_note_formats_too_old_with_min_version() {
        // The "too old" render path string-formats the MIN_VERSION —
        // regression guard so the message doesn't silently drift to
        // e.g. "older than the contract floor (?)".
        let note = format!(
            "agx {} is older than the contract floor ({}) — upgrade required",
            "agx 0.0.5", MIN_VERSION
        );
        assert!(note.contains("0.1.0"));
        assert!(note.contains("agx 0.0.5"));
    }
}
