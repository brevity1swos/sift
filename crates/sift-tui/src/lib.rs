//! sift-tui: ratatui sidecar for `sift review`.

use anyhow::Result;
use crossterm::{
    event::{self, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::Path;
use std::time::Duration;

pub mod app;
pub mod events;
pub mod ui;

pub fn run(session_dir: &Path) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = app::App::new(session_dir)?;
    let tick_rate = Duration::from_millis(200);

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;
        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                events::handle_key(&mut app, key)?;
            }
        }
        app.reload()?;
        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
