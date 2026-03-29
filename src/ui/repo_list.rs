use crate::app::{App, Mode, Panel};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.active_panel == Panel::RepoList;
    let border_color = if is_focused { Color::Cyan } else { Color::DarkGray };
    let hidden = app.config.hidden_repos(&app.workspace_name);

    let show_filter = app.filter_active && app.active_panel == Panel::RepoList;
    let chunks = if show_filter {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3)])
            .split(area)
    };

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
        .title(" Repos ")
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
    f.render_stateful_widget(list, chunks[0], &mut state);

    // Filter bar
    if show_filter {
        let filter = Paragraph::new(Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::styled(&app.filter_text, Style::default().fg(Color::White)),
            if matches!(app.mode, Mode::Filter) {
                Span::styled("▎", Style::default().fg(Color::Yellow))
            } else {
                Span::raw("")
            },
        ]));
        f.render_widget(filter, chunks[1]);
    }
}
