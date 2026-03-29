use crate::app::App;
use super::centered_rect;
use ratatui::Frame;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState};

pub fn render_workspace_switcher(f: &mut Frame, app: &App) {
    let area = centered_rect(40, 50, f.area());

    // Clear the area behind the modal
    f.render_widget(Clear, area);

    let names = app.workspace_names();
    let items: Vec<ListItem> = names
        .iter()
        
        .map(|name| {
            let is_current = *name == app.workspace_name;
            let mut spans = vec![Span::styled(
                format!(" {}", name),
                if is_current {
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                },
            )];
            if is_current {
                spans.push(Span::styled(" (current)", Style::default().fg(Color::DarkGray)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let block = Block::default()
        .title(" Switch Workspace (Enter to select, Esc to cancel) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    state.select(Some(app.workspace_selector_index));
    f.render_stateful_widget(list, area, &mut state);
}

