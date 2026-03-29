use crate::app::{App, Mode, Panel};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let Some(repo) = app.selected_repo() else {
        let block = Block::default()
            .title(" Details ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        f.render_widget(
            Paragraph::new("No repos found").block(block),
            area,
        );
        return;
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // summary header
            Constraint::Min(3),   // branches
        ])
        .split(area);

    render_summary(f, repo, chunks[0]);
    render_branches(f, app, repo, chunks[1]);
}

fn render_summary(f: &mut Frame, repo: &crate::types::RepoStatus, area: Rect) {
    let line = Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(&repo.name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(
            format!("({})", repo.branch),
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(format!("↑{}", repo.ahead), Style::default().fg(if repo.ahead > 0 { Color::Yellow } else { Color::DarkGray })),
        Span::raw(" "),
        Span::styled(format!("↓{}", repo.behind), Style::default().fg(if repo.behind > 0 { Color::Yellow } else { Color::DarkGray })),
        Span::raw(" "),
        Span::styled(format!("Δ{}", repo.dirty), Style::default().fg(if repo.dirty > 0 { Color::Red } else { Color::DarkGray })),
        Span::raw(" "),
        Span::styled(format!("Stash:{}", repo.stash), Style::default().fg(if repo.stash > 0 { Color::Cyan } else { Color::DarkGray })),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(Paragraph::new(line).block(block), area);
}

fn render_branches(
    f: &mut Frame,
    app: &App,
    repo: &crate::types::RepoStatus,
    area: Rect,
) {
    let is_focused = app.active_panel == Panel::Branches;
    let border_color = if is_focused { Color::Yellow } else { Color::DarkGray };

    let show_filter = app.filter_active && app.active_panel == Panel::Branches;
    let branch_area = if show_filter {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(area);
        // Render filter bar
        let filter = ratatui::widgets::Paragraph::new(Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::styled(&app.filter_text, Style::default().fg(Color::White)),
            if matches!(app.mode, Mode::Filter) {
                Span::styled("▎", Style::default().fg(Color::Yellow))
            } else {
                Span::raw("")
            },
        ]));
        f.render_widget(filter, chunks[1]);
        chunks[0]
    } else {
        area
    };

    let filtered_indices = app.filtered_branch_indices();
    let branch_refs: Vec<&crate::types::BranchEntry> = match &filtered_indices {
        Some(indices) => indices.iter().filter_map(|&i| repo.branches.get(i)).collect(),
        None => repo.branches.iter().collect(),
    };

    let items: Vec<ListItem> = branch_refs
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

            // Ref decorations like git log: (HEAD -> main, origin/main)
            let mut refs = Vec::new();
            if b.is_current {
                refs.push(("HEAD -> ", Color::Cyan));
            }
            if b.has_local {
                refs.push((&b.name as &str, Color::Green));
            }
            if b.has_remote {
                refs.push(("", Color::Reset)); // separator marker
            }

            // Show tracking status inline
            let mut tracking = Vec::new();
            if b.has_local && b.has_remote {
                tracking.push(Span::styled(" origin/", Style::default().fg(Color::Red)));
                tracking.push(Span::styled(&b.name, Style::default().fg(Color::Red)));
            } else if b.has_local && !b.has_remote {
                tracking.push(Span::styled(" [local]", Style::default().fg(Color::DarkGray)));
            } else if !b.has_local && b.has_remote {
                tracking.push(Span::styled(" origin/", Style::default().fg(Color::Red)));
                tracking.push(Span::styled(&b.name, Style::default().fg(Color::Red)));
                tracking.push(Span::styled(" [remote only]", Style::default().fg(Color::DarkGray)));
            }
            spans.extend(tracking);

            // Drift: local vs remote
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

            // Drift: vs main
            let ahead_m = b.ahead_main.unwrap_or(0);
            let behind_m = b.behind_main.unwrap_or(0);
            if (ahead_m > 0 || behind_m > 0) && b.name != "main" {
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
        .title(" Branches ")
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
    f.render_stateful_widget(list, branch_area, &mut state);
}
