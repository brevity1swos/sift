//! Session garbage collection: scan sessions dir, filter by age, delete old closed sessions.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use std::fs;

use crate::paths::Paths;
use crate::session::SessionMeta;

/// Result of a garbage collection run.
#[derive(Debug, Default)]
pub struct GcResult {
    /// Session IDs that were deleted (or would be deleted in dry-run mode).
    pub deleted: Vec<String>,
    /// Sessions with no `ended_at` (still open).
    pub skipped_open: usize,
    /// Sessions within the retention window.
    pub skipped_young: usize,
    /// Sessions with unparseable meta.json.
    pub skipped_corrupt: usize,
}

/// Scan `.sift/sessions/`, read each session's `meta.json`, and delete
/// directories for closed sessions older than `retention`.
///
/// - Never deletes open sessions (those with no `ended_at`).
/// - In dry-run mode, populates `GcResult::deleted` but does not remove anything.
pub fn collect(paths: &Paths, retention: Duration, dry_run: bool) -> Result<GcResult> {
    let sessions_dir = paths.sessions_dir();
    let mut result = GcResult::default();

    let entries = match fs::read_dir(&sessions_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // No sessions directory at all — nothing to collect.
            return Ok(result);
        }
        Err(e) => {
            return Err(e)
                .with_context(|| format!("reading sessions dir {}", sessions_dir.display()));
        }
    };

    let now = Utc::now();

    for entry in entries {
        let entry =
            entry.with_context(|| format!("iterating sessions dir {}", sessions_dir.display()))?;
        let session_dir = entry.path();

        // Only consider directories.
        if !session_dir.is_dir() {
            continue;
        }

        let session_id = match session_dir.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => continue,
        };

        let meta_path = session_dir.join("meta.json");
        let meta_text = match fs::read_to_string(&meta_path) {
            Ok(text) => text,
            Err(_) => {
                result.skipped_corrupt += 1;
                continue;
            }
        };

        let meta: SessionMeta = match serde_json::from_str(&meta_text) {
            Ok(m) => m,
            Err(_) => {
                result.skipped_corrupt += 1;
                continue;
            }
        };

        // Never delete open sessions.
        let ended_at = match meta.ended_at {
            Some(t) => t,
            None => {
                result.skipped_open += 1;
                continue;
            }
        };

        // Check if the session is older than retention.
        if !is_expired(ended_at, now, retention) {
            result.skipped_young += 1;
            continue;
        }

        // Delete (or record for dry-run).
        result.deleted.push(session_id.clone());
        if !dry_run {
            fs::remove_dir_all(&session_dir).with_context(|| {
                format!("deleting session dir {}", session_dir.display())
            })?;
        }
    }

    // Sort for deterministic output.
    result.deleted.sort();

    Ok(result)
}

/// Returns `true` if a session that ended at `ended_at` is past the retention window.
fn is_expired(ended_at: DateTime<Utc>, now: DateTime<Utc>, retention: Duration) -> bool {
    now.signed_duration_since(ended_at) > retention
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionMeta;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Helper: create a fake session directory with a meta.json.
    fn write_session(
        paths: &Paths,
        id: &str,
        started_at: DateTime<Utc>,
        ended_at: Option<DateTime<Utc>>,
    ) {
        let dir = paths.session_dir(id);
        fs::create_dir_all(dir.join("snapshots")).unwrap();
        let meta = SessionMeta {
            id: id.to_string(),
            project: "test".into(),
            cwd: PathBuf::from("/tmp/test"),
            started_at,
            ended_at,
        };
        let text = serde_json::to_string_pretty(&meta).unwrap();
        fs::write(dir.join("meta.json"), text).unwrap();
    }

    #[test]
    fn deletes_old_closed_sessions() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        let now = Utc::now();

        // Session closed 10 days ago — should be deleted with 7-day retention.
        let old_ended = now - Duration::days(10);
        write_session(&paths, "old-session", old_ended - Duration::hours(1), Some(old_ended));

        let result = collect(&paths, Duration::days(7), false).unwrap();
        assert_eq!(result.deleted, vec!["old-session"]);
        assert!(!paths.session_dir("old-session").exists());
    }

    #[test]
    fn skips_young_sessions() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        let now = Utc::now();

        // Session closed 2 days ago — should be skipped with 7-day retention.
        let young_ended = now - Duration::days(2);
        write_session(
            &paths,
            "young-session",
            young_ended - Duration::hours(1),
            Some(young_ended),
        );

        let result = collect(&paths, Duration::days(7), false).unwrap();
        assert!(result.deleted.is_empty());
        assert_eq!(result.skipped_young, 1);
        assert!(paths.session_dir("young-session").exists());
    }

    #[test]
    fn skips_open_sessions() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        let now = Utc::now();

        // Open session (no ended_at) started 30 days ago — must never be deleted.
        write_session(&paths, "open-session", now - Duration::days(30), None);

        let result = collect(&paths, Duration::days(7), false).unwrap();
        assert!(result.deleted.is_empty());
        assert_eq!(result.skipped_open, 1);
        assert!(paths.session_dir("open-session").exists());
    }

    #[test]
    fn dry_run_does_not_delete() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        let now = Utc::now();

        let old_ended = now - Duration::days(10);
        write_session(&paths, "old-session", old_ended - Duration::hours(1), Some(old_ended));

        let result = collect(&paths, Duration::days(7), true).unwrap();
        assert_eq!(result.deleted, vec!["old-session"]);
        // Directory must still exist in dry-run mode.
        assert!(paths.session_dir("old-session").exists());
    }

    #[test]
    fn skips_corrupt_meta() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());

        // Create a session dir with invalid meta.json.
        let dir = paths.session_dir("corrupt-session");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("meta.json"), "NOT VALID JSON").unwrap();

        let result = collect(&paths, Duration::days(7), false).unwrap();
        assert!(result.deleted.is_empty());
        assert_eq!(result.skipped_corrupt, 1);
        assert!(dir.exists());
    }

    #[test]
    fn skips_session_dir_without_meta() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());

        // Create a session dir with no meta.json at all.
        let dir = paths.session_dir("no-meta");
        fs::create_dir_all(&dir).unwrap();

        let result = collect(&paths, Duration::days(7), false).unwrap();
        assert!(result.deleted.is_empty());
        assert_eq!(result.skipped_corrupt, 1);
    }

    #[test]
    fn empty_sessions_dir_returns_empty_result() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        fs::create_dir_all(paths.sessions_dir()).unwrap();

        let result = collect(&paths, Duration::days(7), false).unwrap();
        assert!(result.deleted.is_empty());
        assert_eq!(result.skipped_open, 0);
        assert_eq!(result.skipped_young, 0);
        assert_eq!(result.skipped_corrupt, 0);
    }

    #[test]
    fn no_sessions_dir_returns_empty_result() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        // Don't create sessions dir at all.

        let result = collect(&paths, Duration::days(7), false).unwrap();
        assert!(result.deleted.is_empty());
    }

    #[test]
    fn mixed_sessions_categorized_correctly() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        let now = Utc::now();

        // Old closed — should be deleted.
        let old_ended = now - Duration::days(10);
        write_session(&paths, "old-1", old_ended - Duration::hours(2), Some(old_ended));
        write_session(
            &paths,
            "old-2",
            old_ended - Duration::hours(3),
            Some(old_ended - Duration::days(1)),
        );

        // Young closed — should be skipped.
        let young_ended = now - Duration::days(1);
        write_session(&paths, "young-1", young_ended - Duration::hours(1), Some(young_ended));

        // Open — should be skipped.
        write_session(&paths, "open-1", now - Duration::days(20), None);

        // Corrupt — should be skipped.
        let corrupt_dir = paths.session_dir("corrupt-1");
        fs::create_dir_all(&corrupt_dir).unwrap();
        fs::write(corrupt_dir.join("meta.json"), "???").unwrap();

        let result = collect(&paths, Duration::days(7), false).unwrap();
        assert_eq!(result.deleted, vec!["old-1", "old-2"]);
        assert_eq!(result.skipped_young, 1);
        assert_eq!(result.skipped_open, 1);
        assert_eq!(result.skipped_corrupt, 1);

        // Verify old ones are actually gone.
        assert!(!paths.session_dir("old-1").exists());
        assert!(!paths.session_dir("old-2").exists());
        // Others still exist.
        assert!(paths.session_dir("young-1").exists());
        assert!(paths.session_dir("open-1").exists());
        assert!(paths.session_dir("corrupt-1").exists());
    }
}
