use crate::app::App;
use super::centered_rect;
use ratatui::Frame;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

pub fn render(f: &mut Frame, app: &App) {
    let area = centered_rect(90, 90, f.area());
    let viewport_height = area.height.saturating_sub(2) as usize; // minus borders
    let total_lines = app.blame_content.len();
    let scroll = app.blame_scroll;

    let visible: Vec<Line> = app
        .blame_content
        .iter()
        .skip(scroll)
        .take(viewport_height)
        .map(|bl| {
            let hash_span = Span::styled(
                format!("{:<7} ", bl.hash),
                Style::default().fg(Color::Yellow),
            );
            let author_span = Span::styled(
                format!("{:<15} ", truncate_str(&bl.author, 15)),
                Style::default().fg(Color::Cyan),
            );
            let line_no_span = Span::styled(
                format!("{:>4} ", bl.line_no),
                Style::default().fg(Color::DarkGray),
            );
            let content_span = Span::styled(
                bl.content.clone(),
                Style::default().fg(Color::White),
            );
            Line::from(vec![hash_span, author_span, line_no_span, content_span])
        })
        .collect();

    let block = Block::default()
        .title(" Blame (Esc to close, j/k to scroll) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let paragraph = Paragraph::new(visible).block(block);

    f.render_widget(ratatui::widgets::Clear, area);
    f.render_widget(paragraph, area);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
    f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}..", &s[..max_len - 2])
    }
}
