use crate::app::App;
use super::centered_rect;
use ratatui::Frame;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

pub fn render(f: &mut Frame, app: &App) {
    let area = centered_rect(90, 90, f.area());

    let lines: Vec<Line> = app
        .commit_log
        .iter()
        .map(|entry| {
            Line::from(vec![
                Span::styled(
                    format!(" {} ", entry.hash),
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    &entry.message,
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("  {}", entry.author),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("  {}", entry.date),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        })
        .collect();

    let total_lines = lines.len() as u16;

    let block = Block::default()
        .title(" Commit Log (Esc to close, j/k to scroll) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((app.commit_log_scroll, 0));

    f.render_widget(paragraph, area);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    let mut scrollbar_state =
        ScrollbarState::new(total_lines as usize).position(app.commit_log_scroll as usize);
    f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

