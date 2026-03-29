mod app;
mod config;
mod git;
mod types;
mod ui;

use app::{App, Mode, Panel};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::backend::CrosstermBackend;
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
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(workspace_path);
    let result = run_app(&mut terminal, &mut app);

    // Terminal cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
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

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match &app.mode {
                    Mode::Normal => handle_normal_mode(app, key.code, key.modifiers),
                    Mode::DiffView => handle_diff_mode(app, key.code),
                    Mode::CommandLog => handle_command_log_mode(app, key.code),
                    Mode::WorkspaceSwitcher => handle_workspace_mode(app, key.code),
                    Mode::Confirm { .. } => handle_confirm_mode(app, key.code),
                    Mode::TextInput { .. } => handle_text_input_mode(app, key.code),
                    Mode::Filter => handle_filter_mode(app, key.code),
                }
                app.mark_dirty();
            }
        }

        if app.should_quit {
            return Ok(());
        }
    }
}

fn handle_normal_mode(app: &mut App, key: KeyCode, modifiers: KeyModifiers) {
    match key {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('j') | KeyCode::Down => app.move_down(),
        KeyCode::Char('k') | KeyCode::Up => app.move_up(),
        KeyCode::Char(' ') if app.active_panel == Panel::RepoList => app.toggle_mark_repo(),
        KeyCode::Char('a') if modifiers.contains(KeyModifiers::CONTROL) => app.mark_all_repos(),
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => app.unmark_all_repos(),
        KeyCode::Tab => app.next_panel(),
        KeyCode::Enter => app.checkout_selected(),
        KeyCode::Char('p') => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                app.push();
            } else {
                app.pull();
            }
        }
        KeyCode::Char('P') => app.push(),
        KeyCode::Char('a') if app.active_panel == Panel::Files => app.stage_selected_file(),
        KeyCode::Char('u') if app.active_panel == Panel::Files => app.unstage_selected_file(),
        KeyCode::Char('x') if app.active_panel == Panel::Files => app.discard_selected_file(),
        KeyCode::Char('f') => app.fetch(),
        KeyCode::Char('s') => app.stash_toggle(),
        KeyCode::Char('d') => app.show_diff(),
        KeyCode::Char('r') => {
            app.refresh();
            app.notify("Refreshed".to_string(), false);
        }
        KeyCode::Char('h') => app.toggle_hide(),
        KeyCode::Char('H') => app.toggle_show_hidden(),
        KeyCode::Char('/') => {
            app.filter_text.clear();
            app.filter_active = true;
            app.mode = Mode::Filter;
        }
        KeyCode::Char('n') => app.create_branch_prompt(),
        KeyCode::Char('D') => app.delete_branch(),
        KeyCode::Char('R') => app.rename_branch_prompt(),
        KeyCode::Char('m') => app.merge_branch(),
        KeyCode::Char('`') => {
            app.command_log_scroll = 0;
            app.mode = Mode::CommandLog;
        }
        KeyCode::Char('w') => {
            app.workspace_selector_index = 0;
            app.mode = Mode::WorkspaceSwitcher;
        }
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
