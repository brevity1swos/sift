//! Rendering: list view on the left, detail view on the right.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, InputMode};

pub fn draw(f: &mut Frame, app: &App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(if app.input_mode == InputMode::Annotating {
            vec![Constraint::Min(3), Constraint::Length(3)]
        } else {
            vec![Constraint::Min(3), Constraint::Length(1)]
        })
        .split(f.area());

    let content_area = main_chunks[0];
    let bottom_area = main_chunks[1];

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(content_area);
    draw_list(f, app, chunks[0]);
    draw_detail(f, app, chunks[1]);

    match app.input_mode {
        InputMode::Annotating => draw_input(f, app, bottom_area),
        InputMode::Normal => draw_help_bar(f, app, bottom_area),
    }
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
        Some(e) => {
            let mut lines = format!(
                "ID: {}\nTurn: {}\nTool: {:?}\nOp: {:?}\nPath: {}\n+{} -{}",
                e.id,
                e.turn,
                e.tool,
                e.op,
                e.path.display(),
                e.diff_stats.added,
                e.diff_stats.removed
            );
            if !e.rationale.is_empty() {
                lines.push_str(&format!("\n\nNote: {}", e.rationale));
            }
            lines
        }
        None => "no entry selected".to_string(),
    };
    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title("detail"))
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let input = Paragraph::new(format!("  {}_", app.input_buf))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("annotate (Enter=save, Esc=cancel)"),
        )
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(input, area);
}

fn draw_help_bar(f: &mut Frame, app: &App, area: Rect) {
    // When a status message is set (agx missing, deprecation hint, etc.)
    // show it instead of the help bar. The message is one-shot — the next
    // keypress clears it (see events::handle_normal).
    if let Some(msg) = app.status_msg.as_deref() {
        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled(msg, Style::default().fg(Color::Yellow)),
        ]);
        let bar = Paragraph::new(line);
        f.render_widget(bar, area);
        return;
    }

    // Keys align with docs/suite-conventions.md §1. `Enter` accepts per
    // the new suite-wide primary; `a` still works for compatibility
    // until the v0.4 keymap flip. `t` jumps to agx (feature-detected).
    let help = Line::from(vec![
        Span::styled(" Enter", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" accept "),
        Span::styled("r", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("evert "),
        Span::styled("e", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("dit "),
        Span::styled("n", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("ote "),
        Span::styled("t", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" agx "),
        Span::styled("q", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw("uit"),
    ]);
    let bar = Paragraph::new(help).style(Style::default().fg(Color::DarkGray));
    f.render_widget(bar, area);
}
