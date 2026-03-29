use crate::app::{App, SidePanel};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

pub fn render(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    match app.active_side {
        SidePanel::Repos => render_repo_info(f, app, area, block),
        SidePanel::Files => render_diff_preview(f, app, area, block),
        SidePanel::Branches => render_branch_commits(f, app, area, block),
        SidePanel::Commits => render_diff_preview(f, app, area, block),
        SidePanel::Stash => render_diff_preview(f, app, area, block),
    }
}

fn render_repo_info(f: &mut Frame, app: &App, area: Rect, block: Block) {
    let Some(repo) = app.selected_repo() else {
        f.render_widget(
            Paragraph::new("  No repo selected")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    };

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Name:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(&repo.name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Branch:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(&repo.branch, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Ahead:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", repo.ahead),
                Style::default().fg(if repo.ahead > 0 { Color::Yellow } else { Color::White }),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Behind:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", repo.behind),
                Style::default().fg(if repo.behind > 0 { Color::Yellow } else { Color::White }),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Dirty:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", repo.dirty),
                Style::default().fg(if repo.dirty > 0 { Color::Red } else { Color::White }),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Stash:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", repo.stash),
                Style::default().fg(if repo.stash > 0 { Color::Cyan } else { Color::White }),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Path:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                repo.path.to_string_lossy().to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
    ];

    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_diff_preview(f: &mut Frame, app: &App, area: Rect, block: Block) {
    if app.preview_content.is_empty() {
        f.render_widget(
            Paragraph::new("  No preview available")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    }

    let viewport_height = area.height.saturating_sub(2) as usize;
    let total_lines = app.preview_content.lines().count();
    let scroll = app.preview_scroll;

    let lines: Vec<Line> = app.highlighter.highlight_diff_window(
        &app.preview_content,
        scroll,
        viewport_height + 10,
    );

    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    let mut scrollbar_state = ScrollbarState::new(total_lines).position(scroll);
    f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}

fn render_branch_commits(f: &mut Frame, app: &App, area: Rect, block: Block) {
    if app.commit_log.is_empty() {
        f.render_widget(
            Paragraph::new("  No commits loaded")
                .style(Style::default().fg(Color::DarkGray))
                .block(block),
            area,
        );
        return;
    }

    let lines: Vec<Line> = app
        .commit_log
        .iter()
        .take(50) // show recent commits
        .map(|entry| {
            Line::from(vec![
                Span::styled(
                    format!(" {} ", entry.hash),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    &entry.message,
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("  {}", entry.date),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
        })
        .collect();

    let total_lines = lines.len();
    let paragraph = Paragraph::new(lines).block(block);
    f.render_widget(paragraph, area);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight);
    let mut scrollbar_state = ScrollbarState::new(total_lines).position(0);
    f.render_stateful_widget(scrollbar, area, &mut scrollbar_state);
}
