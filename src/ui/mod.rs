mod repo_list;
mod command_log;
mod confirm;
mod detail;
mod diff;
mod files;
mod modal;

use crate::app::{App, Mode};
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
        Mode::Normal | Mode::Filter => {}
    }
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let header = Line::from(vec![
        Span::styled(" WORKSPACE: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(&app.workspace_name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw("  "),
        Span::styled(
            app.workspace_path.to_string_lossy().to_string(),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    f.render_widget(Paragraph::new(header), area);
}

fn render_main(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(area);

    repo_list::render(f, app, cols[0]);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(cols[1]);

    detail::render(f, app, right[0]);
    files::render(f, app, right[1]);
}

fn render_footer(f: &mut Frame, _app: &App, area: Rect) {
    let keys = vec![
        ("j/k", "nav"),
        ("Enter", "checkout"),
        ("p", "pull"),
        ("P", "push"),
        ("f", "fetch"),
        ("s", "stash"),
        ("d", "diff"),
        ("Tab", "panel"),
        ("w", "workspace"),
        ("h", "hide"),
        ("H", "show hidden"),
        ("/", "filter"),
        ("`", "log"),
        ("r", "refresh"),
        ("q", "quit"),
    ];

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
