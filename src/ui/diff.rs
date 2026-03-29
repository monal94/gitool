use crate::app::App;
use super::centered_rect;
use ratatui::Frame;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

pub fn render(f: &mut Frame, app: &App) {
    let area = centered_rect(90, 90, f.area());

    let lines: Vec<Line> = if let Some(ref highlighted) = app.highlighted_diff {
        highlighted.clone()
    } else {
        fallback_lines(&app.diff_content)
    };

    let total_lines = lines.len();

    let block = Block::default()
        .title(" Diff (Esc to close, j/k to scroll) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((app.diff_scroll, 0));

    f.render_widget(paragraph, area);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    let mut scrollbar_state =
        ScrollbarState::new(total_lines).position(app.diff_scroll as usize);
    f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

/// Fallback line coloring when syntect highlighting is not available.
pub fn fallback_lines(content: &str) -> Vec<Line<'static>> {
    content
        .lines()
        .map(|line| {
            let style = if line.starts_with('+') && !line.starts_with("+++") {
                Style::default().fg(Color::Green)
            } else if line.starts_with('-') && !line.starts_with("---") {
                Style::default().fg(Color::Red)
            } else if line.starts_with("@@") {
                Style::default().fg(Color::Cyan)
            } else if line.starts_with("diff ") || line.starts_with("index ") {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::from(Span::styled(line.to_string(), style))
        })
        .collect()
}
