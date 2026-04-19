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
    // Both Annotating and Searching modes use a 3-line boxed input below
    // the main content. Normal mode uses a single-line help bar.
    let bottom_height = match app.input_mode {
        InputMode::Annotating | InputMode::Searching => 3,
        InputMode::Normal => 1,
    };
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Min(3), Constraint::Length(bottom_height)])
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
        InputMode::Annotating => draw_annotate_input(f, app, bottom_area),
        InputMode::Searching => draw_search_input(f, app, bottom_area),
        InputMode::Normal => draw_help_bar(f, app, bottom_area),
    }
}

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
    // Rows in `search_matches` get a cyan hit marker prefix so users can
    // see at a glance which entries the current query hit. The selected
    // row still uses ListState's highlight_symbol + highlight_style.
    let items: Vec<ListItem> = app
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let is_match = app.search_matches.contains(&i);
            let marker = if is_match {
                Span::styled("* ", Style::default().fg(Color::Cyan))
            } else {
                Span::raw("  ")
            };
            let line = Line::from(vec![
                marker,
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
    let title = if app.search_query.is_empty() {
        format!("sift — {} pending", app.entries.len())
    } else {
        format!(
            "sift — {} pending · /{} ({} match)",
            app.entries.len(),
            app.search_query,
            app.search_matches.len()
        )
    };
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

fn draw_annotate_input(f: &mut Frame, app: &App, area: Rect) {
    let input = Paragraph::new(format!("  {}_", app.input_buf))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("annotate (Enter=save, Esc=cancel)"),
        )
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(input, area);
}

fn draw_search_input(f: &mut Frame, app: &App, area: Rect) {
    let input = Paragraph::new(format!("  /{}_", app.input_buf))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("search path (Enter=jump, Esc=cancel)"),
        )
        .style(Style::default().fg(Color::Cyan));
    f.render_widget(input, area);
}

fn draw_help_bar(f: &mut Frame, app: &App, area: Rect) {
    // When a status message is set (agx missing, no-match hint, etc.)
    // show it instead of the help bar. The message is one-shot — the
    // next keypress clears it (see events::handle_normal).
    if let Some(msg) = app.status_msg.as_deref() {
        let line = Line::from(vec![
            Span::raw(" "),
            Span::styled(msg, Style::default().fg(Color::Yellow)),
        ]);
        let bar = Paragraph::new(line);
        f.render_widget(bar, area);
        return;
    }

    // Keys align with docs/suite-conventions.md §1 as of v0.4. `Enter`
    // accepts (suite-wide primary), `a` annotates (moved from `n`),
    // `/`+`n`/`N` search/cycle, `t` jumps to agx.
    let mut spans: Vec<Span> = Vec::with_capacity(16);
    spans.push(Span::raw(" "));
    push_key_hint(&mut spans, "Enter", " accept ");
    push_key_hint(&mut spans, "r", "evert ");
    push_key_hint(&mut spans, "e", "dit ");
    push_key_hint(&mut spans, "a", " note ");
    push_key_hint(&mut spans, "/", " search ");
    push_key_hint(&mut spans, "n", "");
    spans.push(Span::raw("/"));
    push_key_hint(&mut spans, "N", " match ");
    push_key_hint(&mut spans, "t", " agx ");
    push_key_hint(&mut spans, "q", "uit");
    let bar =
        Paragraph::new(Line::from(spans)).style(Style::default().fg(Color::DarkGray));
    f.render_widget(bar, area);
}

/// Append a bold key glyph followed by its plain-text action label to
/// `spans`. Keeps `draw_help_bar` linear and one-line-per-binding so
/// adding or removing a key is a single line edit.
fn push_key_hint<'a>(spans: &mut Vec<Span<'a>>, key: &'a str, action: &'a str) {
    spans.push(Span::styled(key, Style::default().add_modifier(Modifier::BOLD)));
    if !action.is_empty() {
        spans.push(Span::raw(action));
    }
}
