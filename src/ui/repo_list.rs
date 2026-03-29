use crate::app::{App, SidePanel};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.active_side == SidePanel::Repos;
    let border_color = if is_focused { Color::Green } else { Color::DarkGray };
    let hidden = app.config.hidden_repos(&app.workspace_name);

    let visible = app.visible_repos();
    let items: Vec<ListItem> = visible
        .iter()
        .map(|repo| {
            let is_hidden = hidden.contains(&repo.name);
            let is_marked = app.is_repo_marked(&repo.path);
            let mut glyphs = Vec::new();

            if app.is_repo_busy(&repo.path) {
                glyphs.push(Span::styled("⟳", Style::default().fg(Color::Yellow)));
            } else if repo.dirty > 0 {
                glyphs.push(Span::styled(
                    format!("Δ{}", repo.dirty),
                    Style::default().fg(Color::Red),
                ));
            } else {
                glyphs.push(Span::styled("●", Style::default().fg(Color::Green)));
            }

            if repo.ahead > 0 {
                glyphs.push(Span::styled(
                    format!(" ↑{}", repo.ahead),
                    Style::default().fg(Color::Yellow),
                ));
            }
            if repo.behind > 0 {
                glyphs.push(Span::styled(
                    format!(" ↓{}", repo.behind),
                    Style::default().fg(Color::Yellow),
                ));
            }
            if repo.stash > 0 {
                glyphs.push(Span::styled(
                    format!(" S{}", repo.stash),
                    Style::default().fg(Color::Cyan),
                ));
            }

            let name_style = if is_hidden {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            };

            let mark = if is_marked {
                Span::styled("✓ ", Style::default().fg(Color::Green))
            } else {
                Span::raw("  ")
            };
            let mut spans = vec![mark, Span::styled(format!("{:<14}", repo.name), name_style)];
            spans.extend(glyphs);

            ListItem::new(Line::from(spans))
        })
        .collect();

    let block = Block::default()
        .title(" 1 Repos ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ");

    let mut state = ListState::default();
    state.select(Some(app.selected_repo));
    f.render_stateful_widget(list, area, &mut state);
}
