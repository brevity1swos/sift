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

pub struct Session {
    pub paths: Paths,
    pub id: String,
    pub dir: PathBuf,
}

impl Session {
    /// Create a new session directory, write meta.json, flip the `current` symlink,
    /// initialize state.json with turn=0.
    pub fn create(paths: Paths) -> Result<Self> {
        let id = Utc::now().format("%Y-%m-%d-%H%M%S").to_string();
        let dir = paths.session_dir(&id);
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
            started_at: Utc::now(),
            ended_at: None,
        };
        let meta_path = dir.join("meta.json");
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)
            .with_context(|| format!("writing {}", meta_path.display()))?;

        let state = SessionState::default();
        state.save(&dir.join("state.json"))?;

        // Replace the `current` symlink atomically.
        let link = paths.current_symlink();
        if link.symlink_metadata().is_ok() {
            fs::remove_file(&link)
                .with_context(|| format!("removing existing symlink {}", link.display()))?;
        }
        // Ensure the parent (.sift/) exists before symlinking.
        if let Some(parent) = link.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        unix_fs::symlink(&dir, &link)
            .with_context(|| format!("symlinking {} -> {}", link.display(), dir.display()))?;

        Ok(Self { paths, id, dir })
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
    pub fn close(&self) -> Result<()> {
        let meta_path = self.dir.join("meta.json");
        let text = fs::read_to_string(&meta_path)
            .with_context(|| format!("reading {}", meta_path.display()))?;
        let mut meta: SessionMeta = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", meta_path.display()))?;
        meta.ended_at = Some(Utc::now());
        fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)
            .with_context(|| format!("writing {}", meta_path.display()))?;
        Ok(())
    }

    pub fn state_path(&self) -> PathBuf { self.dir.join("state.json") }
    pub fn meta_path(&self) -> PathBuf { self.dir.join("meta.json") }
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
}
