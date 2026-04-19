//! App state for the sidecar TUI.

use anyhow::Result;
use sift_core::{entry::LedgerEntry, session::SessionMeta, store::Store};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Annotating,
    /// Typing a search query in the `/`-prompt. Confirmed with Enter
    /// (jumps cursor to first match and goes back to Normal); Esc
    /// cancels without touching the cursor.
    Searching,
}

pub struct App {
    pub session_dir: PathBuf,
    pub entries: Vec<LedgerEntry>,
    pub cursor: usize,
    pub should_quit: bool,
    /// Set by `e` key — the main loop suspends the TUI and spawns $EDITOR.
    pub edit_request: Option<String>,
    /// Set by `t` key — the main loop suspends the TUI and spawns agx.
    pub jump_to_agx_request: bool,
    /// One-line hint shown below the help bar; cleared on the next keypress.
    /// Used for "agx not installed" and deprecation notices per
    /// `docs/suite-conventions.md` §6 rule 2 (silent degrade).
    pub status_msg: Option<String>,
    /// Current input mode (Normal, typing an annotation, or typing a
    /// search query).
    pub input_mode: InputMode,
    /// Text buffer: shared by Annotating and Searching modes (mutually
    /// exclusive via `input_mode`, so one buffer suffices).
    pub input_buf: String,
    /// Entry ID being annotated.
    pub annotating_id: Option<String>,
    /// Last committed search query (persists across Normal mode so
    /// `n`/`N` can cycle matches without a re-prompt).
    pub search_query: String,
    /// Entry indices matching `search_query`, in position order. Rebuilt
    /// on query commit; cleared when the query is cleared.
    pub search_matches: Vec<usize>,
    /// Agent transcript path read from session meta.json, if any. Loaded
    /// lazily on `t` keypress — no I/O on every keystroke.
    transcript_path: Option<PathBuf>,
    transcript_loaded: bool,
}

impl App {
    /// Derive project root from session_dir: `<root>/.sift/sessions/<id>` → `<root>`
    pub fn project_root(&self) -> PathBuf {
        self.session_dir
            .parent() // .sift/sessions
            .and_then(|p| p.parent()) // .sift
            .and_then(|p| p.parent()) // project root
            .unwrap_or(Path::new("."))
            .to_path_buf()
    }

    /// Derive session id from the last path component.
    pub fn session_id(&self) -> String {
        self.session_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

impl App {
    pub fn new(session_dir: &Path) -> Result<Self> {
        let mut app = Self {
            session_dir: session_dir.to_path_buf(),
            entries: vec![],
            cursor: 0,
            should_quit: false,
            edit_request: None,
            jump_to_agx_request: false,
            status_msg: None,
            input_mode: InputMode::Normal,
            input_buf: String::new(),
            annotating_id: None,
            search_query: String::new(),
            search_matches: Vec::new(),
            transcript_path: None,
            transcript_loaded: false,
        };
        app.reload()?;
        Ok(app)
    }

    /// Commit a search query: recompute match indices against the current
    /// entry list and jump the cursor to the first hit. Empty query
    /// clears any prior search. Returns `true` if at least one match was
    /// found (caller can surface "no matches" via status_msg).
    pub fn commit_search(&mut self, query: &str) -> bool {
        self.search_query = query.to_string();
        self.rebuild_search_matches();
        if let Some(&first) = self.search_matches.first() {
            self.cursor = first;
            true
        } else {
            false
        }
    }

    /// Jump to the next (`delta = 1`) or previous (`delta = -1`) search
    /// match, wrapping around. No-op if no search is active.
    pub fn cycle_search(&mut self, delta: i32) -> bool {
        if self.search_matches.is_empty() {
            return false;
        }
        // Find the current match position relative to cursor, or start
        // from 0 if the cursor isn't on a match.
        let cur_pos = self
            .search_matches
            .iter()
            .position(|&i| i == self.cursor)
            .unwrap_or(0);
        let len = self.search_matches.len() as i32;
        let next = ((cur_pos as i32 + delta).rem_euclid(len)) as usize;
        self.cursor = self.search_matches[next];
        true
    }

    fn rebuild_search_matches(&mut self) {
        self.search_matches.clear();
        if self.search_query.is_empty() {
            return;
        }
        let needle = self.search_query.to_lowercase();
        for (i, e) in self.entries.iter().enumerate() {
            if e.path.to_string_lossy().to_lowercase().contains(&needle) {
                self.search_matches.push(i);
            }
        }
    }

    /// Return the host agent's transcript path from meta.json, lazy-loading
    /// on first call. Returns `None` if the session predates v0.3 (no field)
    /// or if meta.json is missing / unparseable — either way, a `None`
    /// result means "tell the user we don't have a transcript to hand to
    /// agx" rather than surfacing an error.
    pub fn transcript_path(&mut self) -> Option<&Path> {
        if !self.transcript_loaded {
            self.transcript_loaded = true;
            let meta_path = self.session_dir.join("meta.json");
            if let Ok(text) = std::fs::read_to_string(&meta_path) {
                if let Ok(meta) = serde_json::from_str::<SessionMeta>(&text) {
                    self.transcript_path = meta.transcript_path;
                }
            }
        }
        self.transcript_path.as_deref()
    }

    pub fn reload(&mut self) -> Result<()> {
        let store = Store::new(&self.session_dir);
        self.entries = store.list_pending()?;
        if self.cursor >= self.entries.len() && !self.entries.is_empty() {
            self.cursor = self.entries.len() - 1;
        }
        // Entry list may have shifted (accept/revert drops a row) —
        // rebuild match indices to stay valid.
        self.rebuild_search_matches();
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
