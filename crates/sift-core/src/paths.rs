//! `.sift/` path discovery and blob sharding.

use anyhow::{ensure, Result};
use std::path::{Path, PathBuf};

/// Validate that `path` is safe to join onto a project root:
/// - must be relative (not absolute), and
/// - must not contain `..` components.
///
/// Both `restore_snapshot` (store.rs) and `pre_tool` (sift-hook) use this
/// guard to prevent a poisoned ledger or hook payload from directing writes
/// to locations outside the project root.
pub fn validate_relative_path(path: &std::path::Path) -> anyhow::Result<()> {
    if path.is_absolute() {
        anyhow::bail!(
            "path must be relative, got absolute: {}",
            path.display()
        );
    }
    if path
        .components()
        .any(|c| c == std::path::Component::ParentDir)
    {
        anyhow::bail!(
            "path must not contain '..': {}",
            path.display()
        );
    }
    Ok(())
}

/// Paths for a sift-managed project root.
#[derive(Debug, Clone)]
pub struct Paths {
    root: PathBuf,
}

impl Paths {
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            root: project_root.into(),
        }
    }

    pub fn project_root(&self) -> &Path {
        &self.root
    }

    /// `.sift/`
    pub fn sift_dir(&self) -> PathBuf {
        self.root.join(".sift")
    }

    /// `.sift/config.toml`
    pub fn config_file(&self) -> PathBuf {
        self.sift_dir().join("config.toml")
    }

    /// `.sift/policy.yml`
    pub fn policy_file(&self) -> PathBuf {
        self.sift_dir().join("policy.yml")
    }

    /// `.sift/sessions/`
    pub fn sessions_dir(&self) -> PathBuf {
        self.sift_dir().join("sessions")
    }

    /// `.sift/current` — symlink to the active session directory
    pub fn current_symlink(&self) -> PathBuf {
        self.sift_dir().join("current")
    }

    /// `.sift/sessions/<id>/`
    pub fn session_dir(&self, id: &str) -> PathBuf {
        self.sessions_dir().join(id)
    }

    /// Two-char sharded blob path: `<session_dir>/snapshots/ab/cd1234...`
    ///
    /// `sha1_hex` is expected to be the full 40-character lowercase hex SHA-1.
    /// Returns `Err` if it is shorter than 3 characters or not entirely ASCII,
    /// so callers in hook paths can log and continue instead of panicking.
    pub fn snapshot_path(&self, session_id: &str, sha1_hex: &str) -> Result<PathBuf> {
        ensure!(
            sha1_hex.len() >= 3,
            "sha1 hex too short ({}), need at least 3 chars",
            sha1_hex.len()
        );
        ensure!(
            sha1_hex.is_ascii(),
            "sha1 hex must be ASCII, got non-ASCII bytes"
        );
        let (prefix, rest) = sha1_hex.split_at(2);
        Ok(self
            .session_dir(session_id)
            .join("snapshots")
            .join(prefix)
            .join(rest))
    }

    /// Staging record path for in-flight pre/post correlation.
    pub fn staging_path(&self, session_id: &str, correlation_key: &str) -> PathBuf {
        self.session_dir(session_id)
            .join("staging")
            .join(format!("{correlation_key}.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sift_dir_is_under_project_root() {
        let p = Paths::new("/tmp/project");
        assert_eq!(p.sift_dir(), PathBuf::from("/tmp/project/.sift"));
    }

    #[test]
    fn session_dir_joins_id() {
        let p = Paths::new("/tmp/project");
        assert_eq!(
            p.session_dir("2026-04-11-143208"),
            PathBuf::from("/tmp/project/.sift/sessions/2026-04-11-143208"),
        );
    }

    #[test]
    fn snapshot_path_is_sharded() {
        let p = Paths::new("/tmp/project");
        let sha = "abcdef1234567890";
        let snap = p.snapshot_path("sess1", sha).unwrap();
        assert!(snap.ends_with("ab/cdef1234567890"));
        assert!(snap.starts_with("/tmp/project/.sift/sessions/sess1/snapshots"));
    }

    #[test]
    fn snapshot_path_errors_on_short_hex() {
        let p = Paths::new("/tmp/project");
        let err = p.snapshot_path("sess1", "ab").unwrap_err();
        assert!(err.to_string().contains("too short"));
    }

    #[test]
    fn snapshot_path_errors_on_non_ascii() {
        let p = Paths::new("/tmp/project");
        let err = p.snapshot_path("sess1", "αβγδef").unwrap_err();
        assert!(err.to_string().contains("ASCII"));
    }

    #[test]
    fn staging_path_uses_correlation_key() {
        let p = Paths::new("/tmp/project");
        let s = p.staging_path("sess1", "deadbeef");
        assert!(s.ends_with("staging/deadbeef.json"));
    }
}
