//! Rendering: list view on the left, detail view on the right.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::app::App;

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(f.area());
    draw_list(f, app, chunks[0]);
    draw_detail(f, app, chunks[1]);
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .entries
        .iter()
        .map(|e| {
            let line = Line::from(vec![
                Span::raw(format!("turn{} ", e.turn)),
                Span::styled(
                    format!("{:?} ", e.op),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("{}", e.path.display())),
            ]);
            ListItem::new(line).style(Style::default().fg(Color::Yellow))
        })
        .collect();

    let mut state = ListState::default();
    if !app.entries.is_empty() {
        state.select(Some(app.cursor));
    }
    let title = format!("sift — {} pending", app.entries.len());
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().bg(Color::DarkGray));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_detail(f: &mut Frame, app: &App, area: Rect) {
    let text = match app.current() {
        Some(e) => format!(
            "ID: {}\nTurn: {}\nTool: {:?}\nOp: {:?}\nPath: {}\n+{} -{}",
            e.id,
            e.turn,
            e.tool,
            e.op,
            e.path.display(),
            e.diff_stats.added,
            e.diff_stats.removed
        ),
        None => "no entry selected".to_string(),
    };
    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("detail"))
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}
