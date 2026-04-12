//! Session lifecycle: create, open, close, resolve current.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::PathBuf;

use crate::paths::Paths;
use crate::state::SessionState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub project: String,
    pub cwd: PathBuf,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
pub struct Session {
    pub paths: Paths,
    pub id: String,
    pub dir: PathBuf,
}

impl Session {
    /// Create a new session directory, write meta.json, flip the `current` symlink,
    /// initialize state.json with turn=0.
    ///
    /// Id collision handling: if `<timestamp>` is already taken (two sessions
    /// created in the same second), append `-1`, `-2`, ... until a free slot
    /// is found. This prevents silent stomping of an existing session dir.
    pub fn create(paths: Paths) -> Result<Self> {
        let started_at = Utc::now();
        let base_id = started_at.format("%Y-%m-%d-%H%M%S").to_string();
        let (id, dir) = Self::reserve_unique_dir(&paths, &base_id)?;

        fs::create_dir_all(dir.join("snapshots"))
            .with_context(|| format!("creating {}", dir.join("snapshots").display()))?;
        fs::create_dir_all(dir.join("staging"))
            .with_context(|| format!("creating {}", dir.join("staging").display()))?;

        let project = paths
            .project_root()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".into());
        let meta = SessionMeta {
            id: id.clone(),
            project,
            cwd: paths.project_root().to_path_buf(),
            started_at,
            ended_at: None,
        };
        write_json_atomic(&dir.join("meta.json"), &meta)?;

        let state = SessionState::default();
        state.save(&dir.join("state.json"))?;

        // Ensure the parent (.sift/) exists before symlinking.
        let link = paths.current_symlink();
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        // Atomically replace the `current` symlink via tmp-symlink + rename, so
        // a crash between old-remove and new-create can never leave the link
        // missing. POSIX `rename` is atomic and works on symlink destinations.
        let tmp_link = link.with_extension("current.tmp");
        if tmp_link.symlink_metadata().is_ok() {
            fs::remove_file(&tmp_link)
                .with_context(|| format!("removing stale tmp symlink {}", tmp_link.display()))?;
        }
        unix_fs::symlink(&dir, &tmp_link)
            .with_context(|| format!("symlinking {} -> {}", tmp_link.display(), dir.display()))?;
        fs::rename(&tmp_link, &link)
            .with_context(|| format!("renaming {} -> {}", tmp_link.display(), link.display()))?;

        Ok(Self { paths, id, dir })
    }

    /// Find an unused session directory name for `base_id`. Tries `base_id`
    /// first, then `base_id-1`, `base_id-2`, etc., up to a sane cap.
    fn reserve_unique_dir(paths: &Paths, base_id: &str) -> Result<(String, PathBuf)> {
        let first = paths.session_dir(base_id);
        if !first.exists() {
            return Ok((base_id.to_string(), first));
        }
        for suffix in 1..=999u32 {
            let id = format!("{base_id}-{suffix}");
            let dir = paths.session_dir(&id);
            if !dir.exists() {
                return Ok((id, dir));
            }
        }
        anyhow::bail!(
            "could not allocate a unique session id starting from {base_id} after 999 attempts"
        );
    }

    /// Open the session at `paths/.sift/current`.
    pub fn open_current(paths: Paths) -> Result<Self> {
        let link = paths.current_symlink();
        let dir = fs::read_link(&link)
            .with_context(|| format!("reading current symlink {}", link.display()))?;
        // Normalize relative symlinks.
        let dir = if dir.is_absolute() {
            dir
        } else {
            paths.sift_dir().join(dir)
        };
        let id = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .context("current symlink target has no file name")?;
        Ok(Self { paths, id, dir })
    }

    /// Close the session: write `ended_at` to meta.json.
    ///
    /// Uses atomic tmp+rename so a crash mid-write cannot truncate meta.json
    /// to zero bytes. Single-writer-per-session invariant applies: two
    /// concurrent `close` calls on the same session dir will race on the
    /// rename and one timestamp will silently lose.
    pub fn close(&self) -> Result<()> {
        let meta_path = self.dir.join("meta.json");
        let text = fs::read_to_string(&meta_path)
            .with_context(|| format!("reading {}", meta_path.display()))?;
        let mut meta: SessionMeta = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", meta_path.display()))?;
        meta.ended_at = Some(Utc::now());
        write_json_atomic(&meta_path, &meta)?;
        Ok(())
    }

    pub fn state_path(&self) -> PathBuf { self.dir.join("state.json") }
    pub fn meta_path(&self) -> PathBuf { self.dir.join("meta.json") }
}

/// Serialize `value` to JSON and write it to `path` atomically via tmp+rename.
/// A crash or SIGKILL during the write will leave the original file intact.
fn write_json_atomic<T: Serialize>(path: &std::path::Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating parent {}", parent.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(value)
        .with_context(|| format!("serializing {}", path.display()))?;
    fs::write(&tmp, text)
        .with_context(|| format!("writing tmp {}", tmp.display()))?;
    fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn create_makes_dirs_meta_state_and_symlink() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        let s = Session::create(paths).unwrap();
        assert!(s.dir.exists());
        assert!(s.dir.join("snapshots").exists());
        assert!(s.dir.join("staging").exists());
        assert!(s.dir.join("meta.json").exists());
        assert!(s.dir.join("state.json").exists());
        let link = s.paths.current_symlink();
        assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
        let target = fs::read_link(&link).unwrap();
        assert!(target.ends_with(&s.id));
    }

    #[test]
    fn open_current_reads_the_symlink() {
        let td = TempDir::new().unwrap();
        let paths1 = Paths::new(td.path());
        let created = Session::create(paths1).unwrap();
        let paths2 = Paths::new(td.path());
        let opened = Session::open_current(paths2).unwrap();
        assert_eq!(opened.id, created.id);
    }

    #[test]
    fn close_writes_ended_at() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        let s = Session::create(paths).unwrap();
        s.close().unwrap();
        let meta: SessionMeta =
            serde_json::from_str(&fs::read_to_string(s.meta_path()).unwrap()).unwrap();
        assert!(meta.ended_at.is_some());
    }

    #[test]
    fn create_twice_in_same_second_gets_distinct_ids() {
        // Simulate the collision directly by pre-creating the base-id directory,
        // so `create` is forced onto the -1 / -2 branch regardless of clock jitter.
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        let first = Session::create(paths.clone()).unwrap();
        // Immediately create another. Even if the clock rolls, we want to know
        // the probe loop works when the id would otherwise collide — so we
        // manually pre-create a directory matching the NEXT likely id.
        let next_base = chrono::Utc::now().format("%Y-%m-%d-%H%M%S").to_string();
        fs::create_dir_all(paths.session_dir(&next_base)).unwrap();
        let second = Session::create(paths).unwrap();
        assert_ne!(first.id, second.id, "two sessions must have distinct ids");
        assert!(second.id != next_base, "probe loop should have skipped the pre-existing dir");
    }

    #[test]
    fn open_current_errors_when_no_symlink() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        let err = Session::open_current(paths).unwrap_err();
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("current"),
            "error should mention the missing current symlink, got: {rendered}"
        );
    }

    #[test]
    fn close_errors_on_corrupted_meta() {
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        let s = Session::create(paths).unwrap();
        fs::write(s.meta_path(), "{ not valid json").unwrap();
        let err = s.close().unwrap_err();
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("meta.json") || rendered.contains("parsing"),
            "error should mention meta.json or parsing, got: {rendered}"
        );
    }

    #[test]
    fn create_meta_id_matches_started_at_to_the_second() {
        // Regression test: previously id and started_at came from two separate
        // Utc::now() calls and could disagree by a second. They now share one,
        // so the id must always START with the formatted started_at timestamp
        // (a "-N" collision suffix may follow, but the prefix is exact).
        let td = TempDir::new().unwrap();
        let paths = Paths::new(td.path());
        let s = Session::create(paths).unwrap();
        let meta: SessionMeta =
            serde_json::from_str(&fs::read_to_string(s.meta_path()).unwrap()).unwrap();
        let expected_prefix = meta.started_at.format("%Y-%m-%d-%H%M%S").to_string();
        assert!(
            meta.id.starts_with(&expected_prefix),
            "id {} should start with timestamp {}",
            meta.id,
            expected_prefix,
        );
    }
}
