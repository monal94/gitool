use crate::app::App;
use super::centered_rect;
use ratatui::Frame;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

pub fn render(f: &mut Frame, app: &App) {
    let area = centered_rect(90, 90, f.area());
    let viewport_height = area.height.saturating_sub(2) as usize; // minus borders
    let total_lines = app.diff_content.lines().count();
    let scroll = app.diff_scroll as usize;

    // Windowed rendering: only highlight + render visible lines
    let lines: Vec<Line> = app.highlighter.highlight_diff_window(
        &app.diff_content,
        scroll,
        viewport_height + 10, // small buffer
    );

    let block = Block::default()
        .title(" Diff (Esc to close, j/k to scroll) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let paragraph = Paragraph::new(lines).block(block);

    f.render_widget(paragraph, area);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
    f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

/// Fallback line coloring when syntect highlighting is not available.
#[allow(dead_code)]
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
