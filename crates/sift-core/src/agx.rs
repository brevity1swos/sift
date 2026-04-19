//! agx sibling detection: version probe, timeout-safe spawn, minimum-version gate.
//!
//! Used by `sift doctor` (present/version report), the post-tool hook (optional
//! rationale extraction via `agx --export json`), and the review TUI (optional
//! `t` keybind to jump into agx on a session file).
//!
//! Every call site must tolerate `None` — agx is never a hard dependency.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Minimum agx version whose CLI surface is contract-compatible with sift.
///
/// agx 0.1.0 is the first version that shipped `--export json` with a stable
/// `{totals, steps}` shape — the schema sift's optional rationale lookup
/// depends on (see docs/suite-conventions.md §5).
pub const MIN_VERSION: Version = Version {
    major: 0,
    minor: 1,
    patch: 0,
};

/// Default timeout for a single `--version` probe. Long enough for a cold
/// binary start on a loaded machine; short enough that a hung sibling never
/// blocks the sift CLI. Override via `SIFT_AGX_TIMEOUT_MS`.
const DEFAULT_TIMEOUT: Duration = Duration::from_millis(500);

/// Three-part semver slice. Pre-release / build-metadata suffixes (`-rc1`,
/// `+sha.abc`) are intentionally dropped — the contract is on the numeric
/// triple, not the suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// What we know about the agx binary on PATH.
#[derive(Debug, Clone)]
pub struct AgxInfo {
    /// Resolved path (usually just the `agx` name — we don't canonicalize).
    pub path: PathBuf,
    /// Parsed numeric version.
    pub version: Version,
    /// Raw first-line stdout from `agx --version`, for display.
    pub raw: String,
}

impl AgxInfo {
    /// Returns `true` if this agx version meets the minimum contract sift
    /// relies on. Used by `sift doctor` to flag too-old installs.
    pub fn meets_minimum(&self) -> bool {
        self.version >= MIN_VERSION
    }
}

/// Probe `agx` on PATH, using the default timeout, caching the result for the
/// lifetime of the process.
///
/// Returns `None` when agx is absent, hung, crashed, or emits unparseable
/// version output. The caller must handle `None` as the "not installed" case
/// — there is no "agx is broken" state in sift.
pub fn detect() -> Option<AgxInfo> {
    static CACHE: OnceLock<Option<AgxInfo>> = OnceLock::new();
    CACHE.get_or_init(|| detect_with_timeout(probe_timeout())).clone()
}

/// Uncached variant — runs the subprocess every call. Use `detect` in hot
/// paths; use this one in tests and in `sift doctor --refresh`-style flows.
pub fn detect_with_timeout(timeout: Duration) -> Option<AgxInfo> {
    let raw = probe_version("agx", timeout)?;
    let first_line = raw.lines().next().unwrap_or(&raw).trim().to_string();
    let version = parse_version(&first_line)?;
    Some(AgxInfo {
        path: PathBuf::from("agx"),
        version,
        raw: first_line,
    })
}

/// Parse the first numeric-triple token out of a `--version` line.
///
/// Tolerates:
/// - leading binary-name token (`agx 0.1.0`)
/// - trailing feature suffix (`agx 0.1.2 (otel-proto)`)
/// - pre-release / build-metadata (`0.2.0-rc1`, `0.2.0+sha.abc`) — stripped
///
/// Returns `None` on non-numeric components or fewer than three parts.
pub fn parse_version(s: &str) -> Option<Version> {
    for token in s.split_whitespace() {
        if let Some(v) = parse_triple(token) {
            return Some(v);
        }
    }
    None
}

fn parse_triple(token: &str) -> Option<Version> {
    // Strip pre-release / build-metadata suffix.
    let core = token.split(['-', '+']).next().unwrap_or(token);

    let mut parts = core.split('.');
    let major = parts.next()?.parse::<u16>().ok()?;
    let minor = parts.next()?.parse::<u16>().ok()?;
    let patch = parts.next()?.parse::<u16>().ok()?;
    if parts.next().is_some() {
        // Four-or-more-part "versions" are not semver — reject.
        return None;
    }
    Some(Version {
        major,
        minor,
        patch,
    })
}

/// Spawn `<bin> --version`, wait up to `timeout`, return captured stdout.
/// Kills the child on timeout so we don't leak zombies.
///
/// Public so other sift-cli sibling probes (rgx, future tools) can reuse
/// the same hang-safe machinery instead of reinventing it with a bare
/// `Command::output()` that would block forever on a misbehaving binary.
pub fn probe_version(bin: &str, timeout: Duration) -> Option<String> {
    let mut child = Command::new(bin)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .stdin(Stdio::null())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + timeout;
    let poll = Duration::from_millis(10);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut buf = String::new();
                child.stdout.as_mut()?.read_to_string(&mut buf).ok()?;
                return Some(buf);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    // Hung or slow — kill and give up. `--version` should be
                    // instantaneous on any reasonable binary.
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(poll);
            }
            Err(_) => return None,
        }
    }
}

/// Default probe timeout, honoring `SIFT_AGX_TIMEOUT_MS`. Public so
/// non-agx sibling probes (e.g. rgx in `sift doctor`) get the same
/// override knob without duplicating the env-var name.
pub fn probe_timeout() -> Duration {
    std::env::var("SIFT_AGX_TIMEOUT_MS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(DEFAULT_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_handles_plain_semver() {
        let v = parse_version("agx 0.2.0\n").unwrap();
        assert_eq!(
            v,
            Version {
                major: 0,
                minor: 2,
                patch: 0,
            }
        );
    }

    #[test]
    fn parse_version_handles_feature_suffix() {
        // agx ships version strings like `agx 0.1.2 (otel-proto)` per
        // suite-conventions §5.
        let v = parse_version("agx 0.1.2 (otel-proto)").unwrap();
        assert_eq!(v.major, 0);
        assert_eq!(v.minor, 1);
        assert_eq!(v.patch, 2);
    }

    #[test]
    fn parse_version_strips_prerelease() {
        let v = parse_version("agx 1.0.0-rc1").unwrap();
        assert_eq!(v, Version::new(1, 0, 0));
    }

    #[test]
    fn parse_version_strips_build_metadata() {
        let v = parse_version("agx 0.3.0+sha.abcdef").unwrap();
        assert_eq!(v, Version::new(0, 3, 0));
    }

    #[test]
    fn parse_version_picks_first_triple_token() {
        // A hypothetical wrapper might prefix the output.
        let v = parse_version("wrapped agx 0.1.0").unwrap();
        assert_eq!(v, Version::new(0, 1, 0));
    }

    #[test]
    fn parse_version_rejects_two_parts() {
        assert!(parse_version("agx 0.1").is_none());
    }

    #[test]
    fn parse_version_rejects_four_parts() {
        assert!(parse_version("agx 0.1.0.0").is_none());
    }

    #[test]
    fn parse_version_rejects_non_numeric() {
        assert!(parse_version("agx unknown").is_none());
        assert!(parse_version("agx a.b.c").is_none());
    }

    #[test]
    fn parse_version_returns_none_on_empty() {
        assert!(parse_version("").is_none());
        assert!(parse_version("   \n").is_none());
    }

    #[test]
    fn version_ordering_is_lexicographic() {
        assert!(Version::new(0, 1, 0) < Version::new(0, 1, 1));
        assert!(Version::new(0, 1, 9) < Version::new(0, 2, 0));
        assert!(Version::new(0, 9, 9) < Version::new(1, 0, 0));
        assert!(Version::new(1, 0, 0) > Version::new(0, 99, 99));
    }

    #[test]
    fn meets_minimum_accepts_version_at_floor() {
        let info = AgxInfo {
            path: "agx".into(),
            version: MIN_VERSION,
            raw: "agx 0.1.0".into(),
        };
        assert!(info.meets_minimum());
    }

    #[test]
    fn meets_minimum_rejects_older_version() {
        let info = AgxInfo {
            path: "agx".into(),
            version: Version::new(0, 0, 9),
            raw: "agx 0.0.9".into(),
        };
        assert!(!info.meets_minimum());
    }

    #[test]
    fn meets_minimum_accepts_newer_version() {
        let info = AgxInfo {
            path: "agx".into(),
            version: Version::new(0, 5, 0),
            raw: "agx 0.5.0".into(),
        };
        assert!(info.meets_minimum());
    }

    #[test]
    fn detect_with_timeout_does_not_panic_or_hang() {
        // Short timeout — we expect either a fast None (no agx on PATH, the
        // CI case) or a fast Some (agx installed). The only requirement is
        // that the call completes and, if it returns Some, the version at
        // least parses as a valid numeric triple.
        let start = Instant::now();
        let result = detect_with_timeout(Duration::from_millis(500));
        assert!(start.elapsed() < Duration::from_secs(2));
        if let Some(info) = result {
            assert!(info.version >= Version::new(0, 0, 0));
        }
    }

    #[test]
    fn probe_version_of_missing_binary_is_fast_none() {
        // A binary name that cannot exist — spawn fails immediately.
        let start = Instant::now();
        let result = probe_version(
            "sift-agx-probe-definitely-not-a-real-binary-xyz",
            Duration::from_millis(500),
        );
        assert!(result.is_none());
        // Spawn failure should be instant; we tolerate up to 50ms for process
        // creation overhead. Never the full timeout.
        assert!(start.elapsed() < Duration::from_millis(50));
    }

    impl Version {
        // Test-only constructor for ergonomic asserts.
        fn new(major: u16, minor: u16, patch: u16) -> Self {
            Self {
                major,
                minor,
                patch,
            }
        }
    }
}
