mod blame;
mod repo_list;
mod branches;
mod command_log;
mod commits;
mod confirm;
mod diff;
mod files;
mod modal;
mod preview;
mod stash_panel;

use crate::app::{App, Mode, SidePanel};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),  // header
            Constraint::Min(5),    // main content
            Constraint::Length(2), // footer
            Constraint::Length(1), // notification
        ])
        .split(f.area());

    render_header(f, app, chunks[0]);
    render_main(f, app, chunks[1]);
    render_footer(f, app, chunks[2]);
    render_notification(f, app, chunks[3]);

    // Overlay modes
    match &app.mode {
        Mode::DiffView => diff::render(f, app),
        Mode::CommandLog => command_log::render(f, app),
        Mode::WorkspaceSwitcher => modal::render_workspace_switcher(f, app),
        Mode::Confirm { .. } => confirm::render(f, app),
        Mode::TextInput { .. } => render_text_input(f, app),
        Mode::BlameView => blame::render(f, app),
        Mode::Normal | Mode::Filter => {}
    }
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let active = Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD);
    let inactive = Style::default().fg(Color::DarkGray);

    let panels = [
        (" 1 Repos ", SidePanel::Repos),
        (" 2 Files ", SidePanel::Files),
        (" 3 Branches ", SidePanel::Branches),
        (" 4 Commits ", SidePanel::Commits),
        (" 5 Stash ", SidePanel::Stash),
    ];

    let mut spans = vec![Span::raw(" ")];
    for (label, panel) in &panels {
        let style = if app.active_side == *panel { active } else { inactive };
        spans.push(Span::styled(*label, style));
        spans.push(Span::raw("  "));
    }

    spans.push(Span::styled(
        &app.workspace_name,
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(
        app.workspace_path.to_string_lossy().to_string(),
        Style::default().fg(Color::DarkGray),
    ));

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_main(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(area);

    // Left side: 5 vertically stacked panels, each ~20%
    let left_panels = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
        ])
        .split(cols[0]);

    repo_list::render(f, app, left_panels[0]);
    files::render(f, app, left_panels[1]);
    branches::render(f, app, left_panels[2]);
    commits::render(f, app, left_panels[3]);
    stash_panel::render(f, app, left_panels[4]);

    // Right side: preview panel
    preview::render(f, app, cols[1]);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect) {
    let keys = match app.active_side {
        SidePanel::Repos => vec![
            ("j/k", "nav"),
            ("J/K", "scroll"),
            ("Tab", "panel"),
            ("Enter", "select"),
            ("p", "pull"),
            ("P", "push"),
            ("f", "fetch"),
            ("`", "cmdlog"),
            ("q", "quit"),
        ],
        SidePanel::Files => vec![
            ("j/k", "nav"),
            ("J/K", "scroll"),
            ("Tab", "panel"),
            ("a", "stage"),
            ("u", "unstage"),
            ("x", "discard"),
            ("d", "diff"),
            ("b", "blame"),
            ("e", "edit"),
            ("q", "quit"),
        ],
        SidePanel::Branches => vec![
            ("j/k", "nav"),
            ("J/K", "scroll"),
            ("Tab", "panel"),
            ("Enter", "checkout"),
            ("n", "new"),
            ("D", "delete"),
            ("m", "merge"),
            ("q", "quit"),
        ],
        SidePanel::Commits => vec![
            ("j/k", "nav"),
            ("J/K", "scroll"),
            ("Tab", "panel"),
            ("y", "copy"),
            ("C", "cherry-pick"),
            ("t", "tag"),
            ("q", "quit"),
        ],
        SidePanel::Stash => vec![
            ("j/k", "nav"),
            ("J/K", "scroll"),
            ("Tab", "panel"),
            ("s", "stash"),
            ("Enter", "pop"),
            ("x", "drop"),
            ("q", "quit"),
        ],
    };

    let spans: Vec<Span> = keys
        .iter()
        .flat_map(|(key, desc)| {
            vec![
                Span::styled(format!(" {} ", key), Style::default().fg(Color::Black).bg(Color::DarkGray)),
                Span::styled(format!("{} ", desc), Style::default().fg(Color::DarkGray)),
            ]
        })
        .collect();

    let footer = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::TOP).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(footer, area);
}

fn render_text_input(f: &mut Frame, app: &App) {
    if let Mode::TextInput { prompt, input, .. } = &app.mode {
        let area = centered_rect(50, 15, f.area());
        let block = Block::default()
            .title(" Input ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));
        let text = Line::from(vec![
            Span::styled(prompt, Style::default().fg(Color::White)),
            Span::styled(input, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("▎", Style::default().fg(Color::Yellow)),
        ]);
        f.render_widget(ratatui::widgets::Clear, area);
        f.render_widget(Paragraph::new(text).block(block), area);
    }
}

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn render_notification(f: &mut Frame, app: &App, area: Rect) {
    if let Some(ref notif) = app.notification {
        let color = if notif.is_error { Color::Red } else { Color::Green };
        let line = Line::from(Span::styled(
            format!(" {} ", notif.message),
            Style::default().fg(color),
        ));
        f.render_widget(Paragraph::new(line), area);
    }
}
