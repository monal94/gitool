use crate::app::App;
use super::centered_rect;
use ratatui::Frame;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

pub fn render(f: &mut Frame, app: &App) {
    let area = centered_rect(90, 90, f.area());

    let lines: Vec<Line> = if app.command_log.is_empty() {
        vec![Line::from(Span::styled(
            "  No commands executed yet.",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        app.command_log
            .iter()
            .rev()
            .flat_map(|entry| {
                let status_icon = if entry.success { "✓" } else { "✗" };
                let status_color = if entry.success { Color::Green } else { Color::Red };
                let elapsed = entry.timestamp.elapsed().as_secs();
                let time_str = if elapsed < 60 {
                    format!("{}s ago", elapsed)
                } else if elapsed < 3600 {
                    format!("{}m ago", elapsed / 60)
                } else {
                    format!("{}h ago", elapsed / 3600)
                };

                let mut result = vec![Line::from(vec![
                    Span::styled(
                        format!(" {} ", status_icon),
                        Style::default().fg(status_color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        entry.command.to_string(),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("  {}", entry.repo_name),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("  {}", time_str),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])];

                if !entry.output.is_empty() {
                    for line in entry.output.lines().take(3) {
                        result.push(Line::from(Span::styled(
                            format!("   {}", line),
                            Style::default().fg(Color::DarkGray),
                        )));
                    }
                }

                result.push(Line::from(""));
                result
            })
            .collect()
    };

    let total_lines = lines.len() as u16;

    let block = Block::default()
        .title(" Command Log (Esc to close, j/k to scroll) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .scroll((app.command_log_scroll, 0));

    f.render_widget(paragraph, area);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    let mut scrollbar_state =
        ScrollbarState::new(total_lines as usize).position(app.command_log_scroll as usize);
    f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

