use crate::app::{App, LogPanel};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(cols[0]);

    render_commit_list(f, app, left[0]);
    render_commit_files(f, app, left[1]);
    render_diff_preview(f, app, cols[1]);
}

fn render_commit_list(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.active_log_panel == LogPanel::Commits;
    let border_color = if is_focused { Color::Cyan } else { Color::DarkGray };

    if app.commit_log.is_empty() {
        let block = Block::default()
            .title(" Commits ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        f.render_widget(
            Paragraph::new("  No commits (press r to load)").style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .commit_log
        .iter()
        .map(|entry| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!(" {} ", entry.hash),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    &entry.message,
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("  {}", entry.date),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    let block = Block::default()
        .title(" Commits ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let list = List::new(items)
        .block(block)
        .highlight_style(if is_focused {
            Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
        } else {
            Style::default().bg(Color::DarkGray)
        })
        .highlight_symbol(if is_focused { "▸" } else { " " });

    let mut state = ListState::default();
    state.select(Some(app.commit_log_selected));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_commit_files(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.active_log_panel == LogPanel::CommitFiles;
    let border_color = if is_focused { Color::Green } else { Color::DarkGray };

    if app.commit_files.is_empty() {
        let block = Block::default()
            .title(" Files ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        f.render_widget(
            Paragraph::new("  Select a commit").style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .commit_files
        .iter()
        .map(|entry| {
            let (status_color, status_char) = match entry.status {
                'M' => (Color::Yellow, "M"),
                'A' => (Color::Green, "A"),
                'D' => (Color::Red, "D"),
                'R' => (Color::Cyan, "R"),
                _ => (Color::White, "?"),
            };
            ListItem::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!("{} ", status_char),
                    Style::default().fg(status_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    &entry.path,
                    Style::default().fg(Color::White),
                ),
            ]))
        })
        .collect();

    let block = Block::default()
        .title(" Files ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let list = List::new(items)
        .block(block)
        .highlight_style(if is_focused {
            Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        })
        .highlight_symbol(if is_focused { "▸" } else { " " });

    let mut state = ListState::default();
    if is_focused {
        state.select(Some(app.commit_files_selected));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn render_diff_preview(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.active_log_panel == LogPanel::DiffPreview;
    let border_color = if is_focused { Color::Yellow } else { Color::DarkGray };

    if app.commit_diff_preview.is_empty() {
        let block = Block::default()
            .title(" Diff ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        f.render_widget(
            Paragraph::new("  Select a commit to preview diff").style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    }

    let viewport_height = area.height.saturating_sub(2) as usize;
    let total_lines = app.commit_diff_preview.lines().count();
    let scroll = app.commit_diff_scroll;

    let lines: Vec<Line> = app.highlighter.highlight_diff_window(
        &app.commit_diff_preview,
        scroll,
        viewport_height + 10,
    );

    let block = Block::default()
        .title(" Diff ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let paragraph = Paragraph::new(lines).block(block);

    f.render_widget(paragraph, area);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
    f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}
