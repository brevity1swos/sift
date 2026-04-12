//! App state for the sidecar TUI.

use anyhow::Result;
use sift_core::{entry::LedgerEntry, store::Store};
use std::path::{Path, PathBuf};

pub struct App {
    pub session_dir: PathBuf,
    pub entries: Vec<LedgerEntry>,
    pub cursor: usize,
    pub should_quit: bool,
}

impl App {
    pub fn new(session_dir: &Path) -> Result<Self> {
        let mut app = Self {
            session_dir: session_dir.to_path_buf(),
            entries: vec![],
            cursor: 0,
            should_quit: false,
        };
        app.reload()?;
        Ok(app)
    }

    pub fn reload(&mut self) -> Result<()> {
        let store = Store::new(&self.session_dir);
        self.entries = store.list_pending()?;
        if self.cursor >= self.entries.len() && !self.entries.is_empty() {
            self.cursor = self.entries.len() - 1;
        }
        Ok(())
    }

    pub fn cursor_down(&mut self) {
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
        }
    }

    pub fn cursor_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn current(&self) -> Option<&LedgerEntry> {
        self.entries.get(self.cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn new_app_reads_pending_entries() {
        let td = TempDir::new().unwrap();
        let app = App::new(td.path()).unwrap();
        assert_eq!(app.entries.len(), 0);
        assert_eq!(app.cursor, 0);
    }

    #[test]
    fn cursor_bounds() {
        let td = TempDir::new().unwrap();
        let mut app = App::new(td.path()).unwrap();
        app.cursor_down();
        assert_eq!(app.cursor, 0);
        app.cursor_up();
        assert_eq!(app.cursor, 0);
    }
}
