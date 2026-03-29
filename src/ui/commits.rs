use crate::app::{App, SidePanel};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.active_side == SidePanel::Commits;
    let border_color = if is_focused { Color::Green } else { Color::DarkGray };

    if app.commit_log.is_empty() {
        let block = Block::default()
            .title(" 4 Commits ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));
        f.render_widget(
            Paragraph::new("  No commits loaded")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
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
        .title(" 4 Commits ")
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
        state.select(Some(app.commit_log_selected));
    }
    f.render_stateful_widget(list, area, &mut state);
}
