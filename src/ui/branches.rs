use crate::app::{App, SidePanel};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let Some(repo) = app.selected_repo() else {
        let block = Block::default()
            .title(" 3 Branches ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        f.render_widget(
            Paragraph::new("  No repo selected").style(Style::default().fg(Color::DarkGray)).block(block),
            area,
        );
        return;
    };

    let is_focused = app.active_side == SidePanel::Branches;
    let border_color = if is_focused { Color::Green } else { Color::DarkGray };

    let items: Vec<ListItem> = repo
        .branches
        .iter()
        .map(|b| {
            let mut spans = Vec::new();

            // Current branch indicator
            if b.is_current {
                spans.push(Span::styled(
                    " ● ",
                    Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::raw("   "));
            }

            // Branch name
            let name_style = if b.is_current {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else if b.has_local {
                Style::default().fg(Color::White)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            spans.push(Span::styled(&b.name, name_style));

            // Tracking info
            if b.has_local && b.has_remote {
                spans.push(Span::styled(" origin/", Style::default().fg(Color::Red)));
                spans.push(Span::styled(&b.name, Style::default().fg(Color::Red)));
            } else if b.has_local && !b.has_remote {
                spans.push(Span::styled(" [local]", Style::default().fg(Color::DarkGray)));
            } else if !b.has_local && b.has_remote {
                spans.push(Span::styled(" origin/", Style::default().fg(Color::Red)));
                spans.push(Span::styled(&b.name, Style::default().fg(Color::Red)));
                spans.push(Span::styled(" [remote only]", Style::default().fg(Color::DarkGray)));
            }

            // Drift vs remote
            if b.has_local && b.has_remote {
                let ahead_r = b.ahead_remote.unwrap_or(0);
                let behind_r = b.behind_remote.unwrap_or(0);
                if ahead_r > 0 || behind_r > 0 {
                    let mut parts = Vec::new();
                    if ahead_r > 0 { parts.push(format!("↑{}", ahead_r)); }
                    if behind_r > 0 { parts.push(format!("↓{}", behind_r)); }
                    spans.push(Span::styled(
                        format!(" [{}]", parts.join(" ")),
                        Style::default().fg(Color::Yellow),
                    ));
                }
            }

            // Drift vs main
            let ahead_m = b.ahead_main.unwrap_or(0);
            let behind_m = b.behind_main.unwrap_or(0);
            if (ahead_m > 0 || behind_m > 0) && b.name != repo.default_branch {
                let mut parts = Vec::new();
                if ahead_m > 0 { parts.push(format!("↑{}", ahead_m)); }
                if behind_m > 0 { parts.push(format!("↓{}", behind_m)); }
                spans.push(Span::styled(
                    format!(" ({} main)", parts.join(" ")),
                    Style::default().fg(Color::Magenta),
                ));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let block = Block::default()
        .title(" 3 Branches ")
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
        state.select(Some(app.selected_branch));
    }
    f.render_stateful_widget(list, area, &mut state);
}
