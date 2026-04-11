//! Mutable session state stored at `<session>/state.json`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

use crate::config::Mode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionState {
    pub turn: u32,
    pub mode: Mode,
    pub last_hook_ts: Option<DateTime<Utc>>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self { turn: 0, mode: Mode::Loose, last_hook_ts: None }
    }
}

impl SessionState {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Write-then-rename for atomicity against crash mid-write.
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn bump_turn(&mut self) {
        self.turn += 1;
        self.last_hook_ts = Some(Utc::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_turn_is_zero() {
        assert_eq!(SessionState::default().turn, 0);
    }

    #[test]
    fn bump_turn_increments_and_sets_timestamp() {
        let mut s = SessionState::default();
        s.bump_turn();
        assert_eq!(s.turn, 1);
        assert!(s.last_hook_ts.is_some());
    }

    #[test]
    fn save_is_atomic_via_rename() {
        let td = TempDir::new().unwrap();
        let p = td.path().join("state.json");
        let mut s = SessionState::default();
        s.bump_turn();
        s.save(&p).unwrap();
        assert!(p.exists());
        assert!(!td.path().join("state.json.tmp").exists());
        let back = SessionState::load(&p).unwrap();
        assert_eq!(back.turn, 1);
    }
}
