mod app;
mod config;
mod git;
mod highlight;
mod types;
mod ui;

use app::{App, ConfirmAction, Mode, SidePanel, TextInputAction};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, EnableMouseCapture, DisableMouseCapture, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::Terminal;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "gitool", about = "A lazygit-inspired TUI for managing multiple git repositories")]
struct Cli {
    /// Workspace directory path
    #[arg(default_value = ".")]
    path: PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let workspace_path = cli.path.canonicalize().unwrap_or(cli.path);

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(workspace_path);
    let result = run_app(&mut terminal, &mut app);

    // Terminal cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        app.clear_stale_notification();
        app.poll_results();

        if app.dirty {
            terminal.draw(|f| ui::render(f, app))?;
            app.dirty = false;
        }

        if event::poll(Duration::from_millis(50))? {
            match event::read()? {
                Event::Key(key) => {
                    match &app.mode {
                        Mode::Normal => handle_normal_mode(app, key.code, key.modifiers),
                        Mode::DiffView => handle_diff_mode(app, key.code),
                        Mode::CommandLog => handle_command_log_mode(app, key.code),
                        Mode::WorkspaceSwitcher => handle_workspace_mode(app, key.code),
                        Mode::Confirm { .. } => handle_confirm_mode(app, key.code),
                        Mode::TextInput { .. } => handle_text_input_mode(app, key.code),
                        Mode::Filter => handle_filter_mode(app, key.code),
                        Mode::BlameView => handle_blame_mode(app, key.code),
                    }
                    app.mark_dirty();
                }
                Event::Mouse(mouse) => {
                    if matches!(app.mode, Mode::Normal) {
                        let size = terminal.size()?;
                        let area = Rect::new(0, 0, size.width, size.height);
                        handle_mouse(app, mouse.kind, mouse.column, mouse.row, area);
                        app.mark_dirty();
                    }
                }
                Event::Resize(_, _) => app.mark_dirty(),
                _ => {}
            }
        }

        if let Some((editor, path)) = app.editor_command.take() {
            disable_raw_mode()?;
            execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
            let _ = std::process::Command::new(&editor).arg(&path).status();
            enable_raw_mode()?;
            execute!(terminal.backend_mut(), EnterAlternateScreen, EnableMouseCapture)?;
            terminal.clear()?;
            app.mark_dirty();
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_normal_mode(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    // ── Global keys (work regardless of active panel) ──────────────────
    match key {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.should_quit = true;
            return;
        }
        KeyCode::Char(n @ '1'..='5') => {
            if let Some(panel) = SidePanel::from_num(n) {
                app.switch_panel(panel);
            }
            return;
        }
        KeyCode::Tab => {
            let next = app.active_side.next();
            app.switch_panel(next);
            return;
        }
        KeyCode::BackTab => {
            let prev = app.active_side.prev();
            app.switch_panel(prev);
            return;
        }
        KeyCode::Char('/') => {
            app.filter_text.clear();
            app.filter_active = true;
            app.mode = Mode::Filter;
            return;
        }
        KeyCode::Char('`') => {
            app.command_log_scroll = 0;
            app.mode = Mode::CommandLog;
            return;
        }
        KeyCode::Char('w') => {
            app.workspace_selector_index = 0;
            app.mode = Mode::WorkspaceSwitcher;
            return;
        }
        KeyCode::Char('r') => {
            app.refresh();
            app.notify("Refreshed".to_string(), false);
            return;
        }
        KeyCode::Char('z') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.undo();
            return;
        }
        _ => {}
    }

    // ── Panel-specific keys ────────────────────────────────────────────
    match app.active_side {
        SidePanel::Repos => handle_repos_panel(app, key, modifiers),
        SidePanel::Files => handle_files_panel(app, key, modifiers),
        SidePanel::Branches => handle_branches_panel(app, key, modifiers),
        SidePanel::Commits => handle_commits_panel(app, key, modifiers),
        SidePanel::Stash => handle_stash_panel(app, key, modifiers),
    }
}

fn handle_repos_panel(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    match key {
        KeyCode::Char('j') | KeyCode::Down => app.side_move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.side_move_up(),
        KeyCode::Char(' ') => app.toggle_mark_repo(),
        KeyCode::Char('a') if modifiers.contains(KeyModifiers::CONTROL) => app.mark_all_repos(),
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => app.unmark_all_repos(),
        KeyCode::Char('p') if modifiers.contains(KeyModifiers::SHIFT) => app.push(),
        KeyCode::Char('P') => app.push(),
        KeyCode::Char('p') => app.pull(),
        KeyCode::Char('f') => app.fetch(),
        KeyCode::Enter => {
            app.switch_panel(SidePanel::Files);
        }
        _ => {}
    }
}

fn handle_files_panel(app: &mut App, key: KeyCode, _modifiers: KeyModifiers) {
    match key {
        KeyCode::Char('j') | KeyCode::Down => app.side_move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.side_move_up(),
        KeyCode::Char('a') => app.stage_selected_file(),
        KeyCode::Char('u') => app.unstage_selected_file(),
        KeyCode::Char('x') => app.discard_selected_file(),
        KeyCode::Char('c') => app.create_commit_prompt(),
        KeyCode::Char('A') => app.amend_commit_prompt(),
        KeyCode::Char('d') | KeyCode::Enter => app.show_file_diff(),
        KeyCode::Char('b') => app.show_blame(),
        KeyCode::Char('e') => app.open_in_editor(),
        _ => {}
    }
}

fn handle_blame_mode(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
        KeyCode::Char('j') | KeyCode::Down => {
            app.blame_scroll = app.blame_scroll.saturating_add(1);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.blame_scroll = app.blame_scroll.saturating_sub(1);
        }
        KeyCode::Char('d') => {
            app.blame_scroll = app.blame_scroll.saturating_add(20);
        }
        KeyCode::Char('u') => {
            app.blame_scroll = app.blame_scroll.saturating_sub(20);
        }
        _ => {}
    }
}

fn handle_branches_panel(app: &mut App, key: KeyCode, _modifiers: KeyModifiers) {
    match key {
        KeyCode::Char('j') | KeyCode::Down => app.side_move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.side_move_up(),
        KeyCode::Enter => app.checkout_selected(),
        KeyCode::Char('n') => app.create_branch_prompt(),
        KeyCode::Char('D') => app.delete_branch(),
        KeyCode::Char('R') => app.rename_branch_prompt(),
        KeyCode::Char('m') => app.merge_branch(),
        KeyCode::Char('s') => app.stash_toggle(),
        _ => {}
    }
}

fn handle_commits_panel(app: &mut App, key: KeyCode, _modifiers: KeyModifiers) {
    match key {
        KeyCode::Char('j') | KeyCode::Down => app.side_move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.side_move_up(),
        KeyCode::Char('d') => {
            app.preview_scroll = app.preview_scroll.saturating_add(20);
        }
        KeyCode::Char('u') => {
            app.preview_scroll = app.preview_scroll.saturating_sub(20);
        }
        KeyCode::Char('y') => {
            if let Some(entry) = app.commit_log.get(app.commit_log_selected) {
                let hash = entry.hash.clone();
                match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&hash)) {
                    Ok(()) => app.notify(format!("Copied: {}", hash), false),
                    Err(e) => app.notify(format!("Clipboard error: {}", e), true),
                }
            }
        }
        KeyCode::Char('C') => {
            if let Some(entry) = app.commit_log.get(app.commit_log_selected) {
                let hash = entry.hash.clone();
                if let Some(repo) = app.repos.get(app.selected_repo) {
                    let path = repo.path.clone();
                    app.mode = Mode::Confirm {
                        message: format!("Cherry-pick {}? [y/n]", hash),
                        action: ConfirmAction::CherryPick(path, hash),
                    };
                }
            }
        }
        KeyCode::Char('X') => {
            if let Some(entry) = app.commit_log.get(app.commit_log_selected) {
                let hash = entry.hash.clone();
                if let Some(repo) = app.repos.get(app.selected_repo) {
                    let path = repo.path.clone();
                    app.mode = Mode::Confirm {
                        message: format!("Revert {}? [y/n]", hash),
                        action: ConfirmAction::RevertCommit(path, hash),
                    };
                }
            }
        }
        KeyCode::Char('t') => {
            if let Some(entry) = app.commit_log.get(app.commit_log_selected) {
                let hash = entry.hash.clone();
                app.mode = Mode::TextInput {
                    prompt: format!("Tag name for {}: ", hash),
                    input: String::new(),
                    action: TextInputAction::CreateTag(hash),
                };
            }
        }
        _ => {}
    }
}

fn handle_stash_panel(app: &mut App, key: KeyCode, _modifiers: KeyModifiers) {
    match key {
        KeyCode::Char('j') | KeyCode::Down => app.side_move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.side_move_up(),
        KeyCode::Char('s') | KeyCode::Enter => app.stash_toggle(),
        KeyCode::Char('x') => app.stash_drop_selected(),
        _ => {}
    }
}

fn handle_diff_mode(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
        KeyCode::Char('j') | KeyCode::Down => app.diff_scroll = app.diff_scroll.saturating_add(1),
        KeyCode::Char('k') | KeyCode::Up => app.diff_scroll = app.diff_scroll.saturating_sub(1),
        KeyCode::Char('d') => app.diff_scroll = app.diff_scroll.saturating_add(20),
        KeyCode::Char('u') => app.diff_scroll = app.diff_scroll.saturating_sub(20),
        _ => {}
    }
}

fn handle_mouse(app: &mut App, kind: MouseEventKind, col: u16, row: u16, size: Rect) {
    // Layout: header(1) + main(rest) + footer(2) + notif(1)
    let header_h = 1;
    let footer_h = 2;
    let notif_h = 1;
    let main_h = size.height.saturating_sub(header_h + footer_h + notif_h);
    let main_top = header_h;

    // Left column holds 5 vertically stacked panels (each ~20% of main height).
    // Right column is the preview pane.
    let left_w = size.width * 30 / 100;

    // Only handle clicks in the left column.
    if col >= left_w {
        return;
    }

    // Determine which of the 5 panels the row falls in.
    let panel_h = main_h / 5;

    let panel_boundaries: [(u16, SidePanel); 5] = [
        (main_top, SidePanel::Repos),
        (main_top + panel_h, SidePanel::Files),
        (main_top + panel_h * 2, SidePanel::Branches),
        (main_top + panel_h * 3, SidePanel::Commits),
        (main_top + panel_h * 4, SidePanel::Stash),
    ];

    // Find which panel was hit.
    let hit = panel_boundaries
        .iter()
        .rev()
        .find(|(top, _)| row >= *top && row < main_top + main_h)
        .map(|(top, panel)| (*top, *panel));

    let Some((panel_top, panel)) = hit else { return };

    match kind {
        MouseEventKind::Down(_) => {
            app.switch_panel(panel);
            // Approximate the clicked item index (subtract 1 for border).
            let idx = (row.saturating_sub(panel_top)).saturating_sub(1) as usize;
            match panel {
                SidePanel::Repos => {
                    if idx < app.repos.len() {
                        app.selected_repo = idx;
                        app.selected_branch = 0;
                        app.ensure_branches_loaded();
                    }
                }
                SidePanel::Files => {
                    if idx < app.files.len() {
                        app.selected_file = idx;
                    }
                }
                SidePanel::Branches => {
                    if let Some(repo) = app.selected_repo()
                        && idx < repo.branches.len() {
                            app.selected_branch = idx;
                        }
                }
                SidePanel::Commits => {
                    if idx < app.commit_log.len() {
                        app.commit_log_selected = idx;
                    }
                }
                SidePanel::Stash => {
                    if idx < app.stash_list.len() {
                        app.selected_stash = idx;
                    }
                }
            }
        }
        MouseEventKind::ScrollUp => {
            app.active_side = panel;
            app.side_move_up();
        }
        MouseEventKind::ScrollDown => {
            app.active_side = panel;
            app.side_move_down();
        }
        _ => {}
    }
}

fn handle_command_log_mode(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('`') => app.mode = Mode::Normal,
        KeyCode::Char('j') | KeyCode::Down => {
            app.command_log_scroll = app.command_log_scroll.saturating_add(1);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.command_log_scroll = app.command_log_scroll.saturating_sub(1);
        }
        KeyCode::Char('d') => {
            app.command_log_scroll = app.command_log_scroll.saturating_add(20);
        }
        KeyCode::Char('u') => {
            app.command_log_scroll = app.command_log_scroll.saturating_sub(20);
        }
        _ => {}
    }
}

fn handle_workspace_mode(app: &mut App, key: KeyCode) {
    let count = app.workspace_names().len();
    match key {
        KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
        KeyCode::Char('j') | KeyCode::Down => {
            if app.workspace_selector_index + 1 < count {
                app.workspace_selector_index += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.workspace_selector_index = app.workspace_selector_index.saturating_sub(1);
        }
        KeyCode::Enter => {
            let names = app.workspace_names();
            if let Some(name) = names.get(app.workspace_selector_index) {
                let name = name.clone();
                app.switch_workspace(&name);
                app.mode = Mode::Normal;
            }
        }
        _ => {}
    }
}

fn handle_confirm_mode(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Char('y') | KeyCode::Char('Y') => app.execute_confirm(),
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.cancel_confirm(),
        _ => {}
    }
}

fn handle_text_input_mode(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.notify("Cancelled".to_string(), false);
        }
        KeyCode::Enter => app.execute_text_input(),
        KeyCode::Backspace => {
            if let Mode::TextInput { ref mut input, .. } = app.mode {
                input.pop();
            }
        }
        KeyCode::Char(c) => {
            if let Mode::TextInput { ref mut input, .. } = app.mode {
                input.push(c);
            }
        }
        _ => {}
    }
}

fn handle_filter_mode(app: &mut App, key: KeyCode) {
    match key {
        KeyCode::Esc => {
            app.filter_text.clear();
            app.filter_active = false;
            app.mode = Mode::Normal;
        }
        KeyCode::Enter => {
            // Keep filter active, return to normal navigation
            app.mode = Mode::Normal;
        }
        KeyCode::Backspace => {
            app.filter_text.pop();
        }
        KeyCode::Char(c) => {
            app.filter_text.push(c);
        }
        _ => {}
    }
}
