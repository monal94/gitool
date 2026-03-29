use crate::app::{App, Panel};
use crate::types::FileStatus;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.active_panel == Panel::Files;
    let border_color = if is_focused { Color::Green } else { Color::DarkGray };

    if app.files.is_empty() {
        let block = Block::default()
            .title(" Files ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        f.render_widget(
            Paragraph::new("  No changes").style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app
        .files
        .iter()
        .map(|file| {
            let (status_char, status_color) = match file.status {
                FileStatus::Modified => ("M", Color::Yellow),
                FileStatus::Added => ("A", Color::Green),
                FileStatus::Deleted => ("D", Color::Red),
                FileStatus::Renamed => ("R", Color::Cyan),
                FileStatus::Untracked => ("?", Color::Magenta),
                FileStatus::Typechange => ("T", Color::Blue),
                FileStatus::Conflicted => ("C", Color::Red),
            };

            let staged_indicator = if file.staged {
                Span::styled("● ", Style::default().fg(Color::Green))
            } else {
                Span::styled("○ ", Style::default().fg(Color::DarkGray))
            };

            ListItem::new(Line::from(vec![
                Span::raw(" "),
                staged_indicator,
                Span::styled(
                    format!("{} ", status_char),
                    Style::default().fg(status_color).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    &file.path,
                    Style::default().fg(Color::White),
                ),
            ]))
        })
        .collect();

    let block = Block::default()
        .title(" Files (a:stage u:unstage x:discard) ")
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
        state.select(Some(app.selected_file));
    }
    f.render_stateful_widget(list, area, &mut state);
}
