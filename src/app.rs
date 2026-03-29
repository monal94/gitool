use crate::config::Config;
use crate::git;
use crate::types::{FileEntry, RepoStatus};
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tab {
    Status,
    Log,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Panel {
    RepoList,
    Branches,
    Files,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LogPanel {
    Commits,
    CommitFiles,
    DiffPreview,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    WorkspaceSwitcher,
    DiffView,
    CommandLog,
    CommitLog,
    Confirm {
        message: String,
        action: ConfirmAction,
    },
    TextInput {
        prompt: String,
        input: String,
        action: TextInputAction,
    },
    Filter,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextInputAction {
    CreateBranch,
    RenameBranch(String),
    CommitMessage,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    Push(PathBuf),
    BulkPush(Vec<PathBuf>),
    StashPop(PathBuf),
    DiscardFile(PathBuf, String, bool), // repo_path, file_path, is_untracked
    DeleteBranch(PathBuf, String),
    MergeBranch(PathBuf, String),
}

#[derive(Debug)]
pub struct Notification {
    pub message: String,
    pub is_error: bool,
    pub created: Instant,
}

#[derive(Debug, Clone)]
pub enum UndoOp {
    Checkout { repo_path: PathBuf, previous_branch: String },
    Stash { repo_path: PathBuf },
    StashPop { repo_path: PathBuf },
}

#[derive(Debug, Clone)]
pub struct CommitEntry {
    pub hash: String,
    pub author: String,
    pub date: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct CommitFileEntry {
    pub path: String,
    pub status: char, // 'M', 'A', 'D', 'R'
}

#[derive(Debug, Clone)]
pub struct CommandLogEntry {
    pub timestamp: Instant,
    pub repo_name: String,
    pub command: String,
    pub success: bool,
    pub output: String,
}

/// Result from a background git operation.
pub enum GitResult {
    Done {
        repo_path: PathBuf,
        label: String,
        result: Result<String, String>,
    },
    DiffReady {
        content: String,
    },
    DiffError {
        message: String,
    },
    FileChanged,
}

pub struct App {
    pub repos: Vec<RepoStatus>,
    pub all_repos: Vec<RepoStatus>,
    pub selected_repo: usize,
    pub selected_branch: usize,
    pub active_panel: Panel,
    pub config: Config,
    pub workspace_path: PathBuf,
    pub workspace_name: String,
    pub mode: Mode,
    pub notification: Option<Notification>,
    pub show_hidden: bool,
    pub diff_content: String,
    pub diff_scroll: u16,
    pub workspace_selector_index: usize,
    pub should_quit: bool,
    pub pending_ops: HashSet<PathBuf>,
    pub marked_repos: HashSet<PathBuf>,
    pub filter_text: String,
    pub filter_active: bool,
    pub files: Vec<FileEntry>,
    pub selected_file: usize,
    pub commit_log: Vec<CommitEntry>,
    pub commit_log_scroll: u16,
    pub command_log: Vec<CommandLogEntry>,
    pub command_log_scroll: u16,
    pub dirty: bool,
    pub zoomed_panel: Option<Panel>,
    pub undo_stack: Vec<UndoOp>,
    pub active_tab: Tab,
    pub active_log_panel: LogPanel,
    pub commit_log_selected: usize,
    pub commit_files: Vec<CommitFileEntry>,
    pub commit_files_selected: usize,
    pub commit_diff_preview: String,
    pub commit_diff_scroll: usize,
    result_rx: Receiver<GitResult>,
    task_tx: Sender<GitResult>,
    _watcher: Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>>,
}

impl App {
    pub fn new(workspace_path: PathBuf) -> Self {
        let mut config = Config::load();
        let workspace_name = config.ensure_workspace(&workspace_path);
        let _ = config.save();

        let hidden = config.hidden_repos(&workspace_name);
        let all_repos = git::scan_workspace(&workspace_path, &[]);
        let repos: Vec<RepoStatus> = all_repos
            .iter()
            .filter(|r| !hidden.contains(&r.name))
            .cloned()
            .collect();

        let (task_tx, result_rx) = mpsc::channel();

        let mut app = Self {
            repos,
            all_repos,
            selected_repo: 0,
            selected_branch: 0,
            active_panel: Panel::RepoList,
            config,
            workspace_path,
            workspace_name,
            mode: Mode::Normal,
            notification: None,
            show_hidden: false,
            diff_content: String::new(),
            diff_scroll: 0,
            workspace_selector_index: 0,
            should_quit: false,
            pending_ops: HashSet::new(),
            marked_repos: HashSet::new(),
            filter_text: String::new(),
            filter_active: false,
            files: Vec::new(),
            selected_file: 0,
            commit_log: Vec::new(),
            commit_log_scroll: 0,
            command_log: Vec::new(),
            command_log_scroll: 0,
            dirty: true,
            zoomed_panel: None,
            undo_stack: Vec::new(),
            active_tab: Tab::Status,
            active_log_panel: LogPanel::Commits,
            commit_log_selected: 0,
            commit_files: Vec::new(),
            commit_files_selected: 0,
            commit_diff_preview: String::new(),
            commit_diff_scroll: 0,
            result_rx,
            task_tx,
            _watcher: None,
        };
        app.ensure_branches_loaded();
        app.start_watcher();
        app
    }

    pub fn selected_repo(&self) -> Option<&RepoStatus> {
        self.repos.get(self.selected_repo)
    }

    pub fn visible_repos(&self) -> Vec<&RepoStatus> {
        if self.filter_active && !self.filter_text.is_empty() && self.active_panel == Panel::RepoList {
            let ft = self.filter_text.to_lowercase();
            self.repos.iter().filter(|r| r.name.to_lowercase().contains(&ft)).collect()
        } else {
            self.repos.iter().collect()
        }
    }

    pub fn filtered_branch_indices(&self) -> Option<Vec<usize>> {
        if !self.filter_active || self.filter_text.is_empty() || self.active_panel != Panel::Branches {
            return None;
        }
        let repo = self.repos.get(self.selected_repo)?;
        let ft = self.filter_text.to_lowercase();
        Some(
            repo.branches
                .iter()
                .enumerate()
                .filter(|(_, b)| b.name.to_lowercase().contains(&ft))
                .map(|(i, _)| i)
                .collect(),
        )
    }

    /// Poll for completed background git operations. Call each tick.
    pub fn poll_results(&mut self) {
        while let Ok(result) = self.result_rx.try_recv() {
            match result {
                GitResult::Done { repo_path, label, result } => {
                    self.pending_ops.remove(&repo_path);
                    let repo_name = repo_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if self.command_log.len() >= 200 {
                        self.command_log.remove(0);
                    }
                    self.command_log.push(CommandLogEntry {
                        timestamp: Instant::now(),
                        repo_name,
                        command: label.clone(),
                        success: result.is_ok(),
                        output: match &result {
                            Ok(msg) => msg.trim().to_string(),
                            Err(e) => e.clone(),
                        },
                    });
                    match result {
                        Ok(msg) => {
                            let summary = msg.lines().last().unwrap_or("Done").to_string();
                            self.notify(format!("{}: {}", label, summary), false);
                        }
                        Err(e) => {
                            self.notify(format!("{} failed: {}", label, e), true);
                        }
                    }
                    // Rescan the affected repo (full scan for selected, light for others)
                    let is_selected = self.repos.get(self.selected_repo)
                        .is_some_and(|r| r.path == repo_path);
                    let new_status = if is_selected {
                        git::scan_repo_full(&repo_path)
                    } else {
                        git::scan_repo(&repo_path)
                    };
                    if let Some(new_status) = new_status {
                        if let Some(pos) = self.repos.iter().position(|r| r.path == repo_path) {
                            self.repos[pos] = new_status.clone();
                        }
                        if let Some(pos) = self.all_repos.iter().position(|r| r.path == repo_path) {
                            self.all_repos[pos] = new_status;
                        }
                        if is_selected {
                            self.reload_files();
                        }
                    }
                }
                GitResult::DiffReady { content } => {
                    if content.is_empty() {
                        self.notify("No changes to diff".to_string(), false);
                    } else {
                        self.diff_content = content;
                        self.diff_scroll = 0;
                        self.mode = Mode::DiffView;
                    }
                }
                GitResult::DiffError { message } => {
                    self.notify(format!("Diff failed: {}", message), true);
                }
                GitResult::FileChanged => {
                    self.refresh();
                    self.dirty = true;
                }
            }
        }
    }

    /// Dispatch a git operation to a background thread.
    fn dispatch<F>(&mut self, path: PathBuf, label: &str, op: F)
    where
        F: FnOnce(&std::path::Path) -> Result<String, String> + Send + 'static,
    {
        if self.pending_ops.contains(&path) {
            self.notify("Operation already in progress".to_string(), false);
            return;
        }
        self.pending_ops.insert(path.clone());
        self.notify(format!("{}...", label), false);
        let tx = self.task_tx.clone();
        let label = label.to_string();
        std::thread::spawn(move || {
            let result = op(&path);
            let _ = tx.send(GitResult::Done {
                repo_path: path,
                label,
                result,
            });
        });
    }

    pub fn refresh(&mut self) {
        let hidden = if self.show_hidden {
            vec![]
        } else {
            self.config.hidden_repos(&self.workspace_name)
        };
        self.all_repos = git::scan_workspace(&self.workspace_path, &[]);
        self.repos = self
            .all_repos
            .iter()
            .filter(|r| !hidden.contains(&r.name))
            .cloned()
            .collect();
        if self.selected_repo >= self.repos.len() {
            self.selected_repo = self.repos.len().saturating_sub(1);
        }
        self.selected_branch = 0;
        self.ensure_branches_loaded();
    }

    pub fn notify(&mut self, message: String, is_error: bool) {
        self.notification = Some(Notification {
            message,
            is_error,
            created: Instant::now(),
        });
        self.dirty = true;
    }

    pub fn clear_stale_notification(&mut self) {
        if let Some(ref n) = self.notification
            && n.created.elapsed().as_secs() >= 3 {
                self.notification = None;
                self.dirty = true;
            }
    }

    pub fn undo(&mut self) {
        let Some(op) = self.undo_stack.pop() else {
            self.notify("Nothing to undo".to_string(), false);
            return;
        };
        match op {
            UndoOp::Checkout { repo_path, previous_branch } => {
                let branch = previous_branch.clone();
                self.dispatch(repo_path, &format!("Undo: checkout {}", previous_branch), move |p| {
                    git::git_checkout(p, &branch)
                });
            }
            UndoOp::Stash { repo_path } => {
                self.dispatch(repo_path, "Undo: stash pop", |p| {
                    git::git_stash_pop(p)
                });
            }
            UndoOp::StashPop { repo_path } => {
                self.dispatch(repo_path, "Undo: stash", |p| {
                    git::git_stash(p)
                });
            }
        }
    }

    pub fn switch_tab(&mut self, tab: Tab) {
        if self.active_tab == tab { return; }
        self.active_tab = tab;
        if tab == Tab::Log {
            self.load_log();
        }
    }

    /// Load the commit log for the currently selected repo into the Log tab state.
    pub fn load_log(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        self.commit_log = git::git_log(&repo.path, 100);
        self.commit_log_selected = 0;
        self.commit_files.clear();
        self.commit_files_selected = 0;
        self.commit_diff_preview.clear();
        self.commit_diff_scroll = 0;
        self.active_log_panel = LogPanel::Commits;
        self.load_commit_detail();
    }

    /// Load files and diff for the currently selected commit.
    pub fn load_commit_detail(&mut self) {
        let Some(entry) = self.commit_log.get(self.commit_log_selected) else {
            self.commit_files.clear();
            self.commit_diff_preview.clear();
            return;
        };
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let hash = entry.hash.clone();
        let path = &repo.path;

        self.commit_files = git::git_show_files(path, &hash).unwrap_or_default();
        self.commit_files_selected = 0;
        self.commit_diff_preview = git::git_diff_commit(path, &hash).unwrap_or_default();
        self.commit_diff_scroll = 0;
    }

    /// Load per-file diff for the selected file in commit detail.
    pub fn load_commit_file_diff(&mut self) {
        let Some(file) = self.commit_files.get(self.commit_files_selected) else { return };
        let Some(entry) = self.commit_log.get(self.commit_log_selected) else { return };
        let Some(repo) = self.repos.get(self.selected_repo) else { return };

        self.commit_diff_preview = git::git_diff_commit_file(
            &repo.path, &entry.hash, &file.path,
        ).unwrap_or_default();
        self.commit_diff_scroll = 0;
    }

    pub fn next_log_panel(&mut self) {
        self.active_log_panel = match self.active_log_panel {
            LogPanel::Commits => LogPanel::CommitFiles,
            LogPanel::CommitFiles => LogPanel::DiffPreview,
            LogPanel::DiffPreview => LogPanel::Commits,
        };
    }

    pub fn log_move_up(&mut self) {
        match self.active_log_panel {
            LogPanel::Commits => {
                if self.commit_log_selected > 0 {
                    self.commit_log_selected -= 1;
                    self.load_commit_detail();
                }
            }
            LogPanel::CommitFiles => {
                if self.commit_files_selected > 0 {
                    self.commit_files_selected -= 1;
                    self.load_commit_file_diff();
                }
            }
            LogPanel::DiffPreview => {
                self.commit_diff_scroll = self.commit_diff_scroll.saturating_sub(1);
            }
        }
    }

    pub fn log_move_down(&mut self) {
        match self.active_log_panel {
            LogPanel::Commits => {
                if self.commit_log_selected + 1 < self.commit_log.len() {
                    self.commit_log_selected += 1;
                    self.load_commit_detail();
                }
            }
            LogPanel::CommitFiles => {
                if self.commit_files_selected + 1 < self.commit_files.len() {
                    self.commit_files_selected += 1;
                    self.load_commit_file_diff();
                }
            }
            LogPanel::DiffPreview => {
                self.commit_diff_scroll += 1;
            }
        }
    }

    pub fn log_page_down(&mut self) {
        if self.active_log_panel == LogPanel::DiffPreview {
            self.commit_diff_scroll += 20;
        }
    }

    pub fn log_page_up(&mut self) {
        if self.active_log_panel == LogPanel::DiffPreview {
            self.commit_diff_scroll = self.commit_diff_scroll.saturating_sub(20);
        }
    }

    pub fn toggle_zoom(&mut self) {
        if self.zoomed_panel.is_some() {
            self.zoomed_panel = None;
        } else {
            self.zoomed_panel = Some(self.active_panel);
        }
    }

    fn push_undo(&mut self, op: UndoOp) {
        if self.undo_stack.len() >= 50 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(op);
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn start_watcher(&mut self) {
        let tx = self.task_tx.clone();
        let debouncer = new_debouncer(
            Duration::from_millis(500),
            move |res: Result<Vec<notify_debouncer_mini::DebouncedEvent>, _>| {
                if let Ok(events) = res
                    && events.iter().any(|e| matches!(e.kind, DebouncedEventKind::Any)) {
                        let _ = tx.send(GitResult::FileChanged);
                    }
            },
        );

        if let Ok(mut debouncer) = debouncer {
            // Watch only .git directories to avoid excessive FS events
            for repo in &self.all_repos {
                let git_dir = repo.path.join(".git");
                let _ = debouncer.watcher().watch(
                    &git_dir,
                    notify::RecursiveMode::Recursive,
                );
            }
            self._watcher = Some(debouncer);
        }
    }

    /// Load branches for the currently selected repo if not already loaded.
    pub fn ensure_branches_loaded(&mut self) {
        if let Some(repo) = self.repos.get(self.selected_repo) {
            let path = repo.path.clone();
            if !repo.branches_loaded {
                let branches = git::load_branches(&path);
                if let Some(repo) = self.repos.get_mut(self.selected_repo) {
                    repo.branches = branches.clone();
                    repo.branches_loaded = true;
                }
                if let Some(pos) = self.all_repos.iter().position(|r| r.path == path) {
                    self.all_repos[pos].branches = branches;
                    self.all_repos[pos].branches_loaded = true;
                }
                self.reload_files();
            }
        }
    }

    /// Reload file statuses for the currently selected repo.
    fn reload_files(&mut self) {
        if let Some(repo) = self.repos.get(self.selected_repo) {
            self.files = git::get_file_statuses(&repo.path);
            if self.selected_file >= self.files.len() {
                self.selected_file = self.files.len().saturating_sub(1);
            }
        } else {
            self.files.clear();
            self.selected_file = 0;
        }
    }

    // Navigation

    pub fn move_up(&mut self) {
        match self.active_panel {
            Panel::RepoList => {
                if self.selected_repo > 0 {
                    self.selected_repo -= 1;
                    self.selected_branch = 0;
                    self.ensure_branches_loaded();
                }
            }
            Panel::Branches => {
                if self.selected_branch > 0 {
                    self.selected_branch -= 1;
                }
            }
            Panel::Files => {
                if self.selected_file > 0 {
                    self.selected_file -= 1;
                }
            }
        }
    }

    pub fn move_down(&mut self) {
        match self.active_panel {
            Panel::RepoList => {
                if self.selected_repo + 1 < self.repos.len() {
                    self.selected_repo += 1;
                    self.selected_branch = 0;
                    self.ensure_branches_loaded();
                }
            }
            Panel::Branches => {
                if let Some(repo) = self.selected_repo()
                    && self.selected_branch + 1 < repo.branches.len() {
                        self.selected_branch += 1;
                    }
            }
            Panel::Files => {
                if self.selected_file + 1 < self.files.len() {
                    self.selected_file += 1;
                }
            }
        }
    }

    pub fn next_panel(&mut self) {
        self.active_panel = match self.active_panel {
            Panel::RepoList => Panel::Branches,
            Panel::Branches => Panel::Files,
            Panel::Files => Panel::RepoList,
        };
    }

    // Bulk selection

    pub fn toggle_mark_repo(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = repo.path.clone();
        if self.marked_repos.contains(&path) {
            self.marked_repos.remove(&path);
        } else {
            self.marked_repos.insert(path);
        }
    }

    pub fn mark_all_repos(&mut self) {
        for repo in &self.repos {
            self.marked_repos.insert(repo.path.clone());
        }
    }

    pub fn unmark_all_repos(&mut self) {
        self.marked_repos.clear();
    }

    fn bulk_targets(&self) -> Vec<PathBuf> {
        if self.marked_repos.is_empty() {
            self.repos.get(self.selected_repo)
                .map(|r| vec![r.path.clone()])
                .unwrap_or_default()
        } else {
            self.marked_repos.iter().cloned().collect()
        }
    }

    pub fn is_repo_marked(&self, path: &Path) -> bool {
        self.marked_repos.contains(path)
    }

    // Git actions — all non-blocking via dispatch

    pub fn pull(&mut self) {
        let targets = self.bulk_targets();
        for path in targets {
            self.dispatch(path, "Pull", git::git_pull);
        }
    }

    pub fn push(&mut self) {
        let targets = self.bulk_targets();
        if targets.len() > 1 {
            let count = targets.len();
            self.mode = Mode::Confirm {
                message: format!("Push {} repos? [y/n]", count),
                action: ConfirmAction::BulkPush(targets),
            };
        } else if let Some(repo) = self.repos.get(self.selected_repo) {
            self.mode = Mode::Confirm {
                message: format!("Push {} ({})? [y/n]", repo.name, repo.branch),
                action: ConfirmAction::Push(repo.path.clone()),
            };
        }
    }

    pub fn fetch(&mut self) {
        let targets = self.bulk_targets();
        for path in targets {
            self.dispatch(path, "Fetch", git::git_fetch);
        }
    }

    pub fn stash_toggle(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = repo.path.clone();
        if repo.dirty > 0 {
            self.push_undo(UndoOp::Stash { repo_path: path.clone() });
            self.dispatch(path, "Stash", git::git_stash);
        } else if repo.stash > 0 {
            self.mode = Mode::Confirm {
                message: format!("Pop stash for {}? [y/n]", repo.name),
                action: ConfirmAction::StashPop(path),
            };
        } else {
            self.notify("Nothing to stash/pop".to_string(), false);
        }
    }

    pub fn checkout_selected(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = repo.path.clone();

        let branch_name = match self.active_panel {
            Panel::Branches => {
                repo.branches.get(self.selected_branch).map(|b| b.name.clone())
            }
            Panel::RepoList | Panel::Files => None,
        };

        let Some(branch) = branch_name else {
            self.notify("Select a branch first (Tab to switch panel)".to_string(), false);
            return;
        };

        let previous_branch = repo.branch.clone();
        self.push_undo(UndoOp::Checkout {
            repo_path: path.clone(),
            previous_branch,
        });

        self.dispatch(path, &format!("Checkout {}", branch), move |p| {
            git::git_checkout(p, &branch)
        });
    }

    pub fn show_diff(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = repo.path.clone();
        let tx = self.task_tx.clone();
        std::thread::spawn(move || {
            match git::git_diff(&path) {
                Ok(content) => { let _ = tx.send(GitResult::DiffReady { content }); }
                Err(e) => { let _ = tx.send(GitResult::DiffError { message: e }); }
            }
        });
    }

    pub fn stage_selected_file(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let Some(file) = self.files.get(self.selected_file) else { return };
        if file.staged {
            self.notify("Already staged".to_string(), false);
            return;
        }
        let path = repo.path.clone();
        let file_path = file.path.clone();
        match git::git_stage(&path, &file_path) {
            Ok(_) => self.notify(format!("Staged: {}", file_path), false),
            Err(e) => self.notify(format!("Stage failed: {}", e), true),
        }
        self.reload_files();
        self.rescan_selected_repo();
    }

    pub fn unstage_selected_file(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let Some(file) = self.files.get(self.selected_file) else { return };
        if !file.staged {
            self.notify("Not staged".to_string(), false);
            return;
        }
        let path = repo.path.clone();
        let file_path = file.path.clone();
        match git::git_unstage(&path, &file_path) {
            Ok(_) => self.notify(format!("Unstaged: {}", file_path), false),
            Err(e) => self.notify(format!("Unstage failed: {}", e), true),
        }
        self.reload_files();
        self.rescan_selected_repo();
    }

    pub fn discard_selected_file(&mut self) {
        let Some(file) = self.files.get(self.selected_file) else { return };
        if file.staged {
            self.notify("Unstage first before discarding".to_string(), false);
            return;
        }
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        self.mode = Mode::Confirm {
            message: format!("Discard changes to {}? [y/n]", file.path),
            action: ConfirmAction::DiscardFile(
                repo.path.clone(),
                file.path.clone(),
                file.status == crate::types::FileStatus::Untracked,
            ),
        };
    }

    fn rescan_selected_repo(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = repo.path.clone();
        if let Some(new_status) = git::scan_repo_full(&path) {
            if let Some(pos) = self.repos.iter().position(|r| r.path == path) {
                self.repos[pos] = new_status.clone();
            }
            if let Some(pos) = self.all_repos.iter().position(|r| r.path == path) {
                self.all_repos[pos] = new_status;
            }
            self.reload_files();
        }
    }

    pub fn show_commit_log(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        self.commit_log = git::git_log(&repo.path, 50);
        self.commit_log_scroll = 0;
        if self.commit_log.is_empty() {
            self.notify("No commits found".to_string(), false);
        } else {
            self.mode = Mode::CommitLog;
        }
    }

    pub fn create_commit_prompt(&mut self) {
        self.mode = Mode::TextInput {
            prompt: "Commit message: ".to_string(),
            input: String::new(),
            action: TextInputAction::CommitMessage,
        };
    }

    pub fn create_branch_prompt(&mut self) {
        self.mode = Mode::TextInput {
            prompt: "New branch name: ".to_string(),
            input: String::new(),
            action: TextInputAction::CreateBranch,
        };
    }

    pub fn delete_branch(&mut self) {
        if self.active_panel != Panel::Branches { return; }
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let Some(branch) = repo.branches.get(self.selected_branch) else { return };
        if branch.is_current {
            self.notify("Cannot delete current branch".to_string(), true);
            return;
        }
        self.mode = Mode::Confirm {
            message: format!("Delete branch {}? [y/n]", branch.name),
            action: ConfirmAction::DeleteBranch(repo.path.clone(), branch.name.clone()),
        };
    }

    pub fn rename_branch_prompt(&mut self) {
        if self.active_panel != Panel::Branches { return; }
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let Some(branch) = repo.branches.get(self.selected_branch) else { return };
        if !branch.has_local {
            self.notify("Cannot rename remote-only branch".to_string(), true);
            return;
        }
        let old_name = branch.name.clone();
        self.mode = Mode::TextInput {
            prompt: format!("Rename {} to: ", old_name),
            input: String::new(),
            action: TextInputAction::RenameBranch(old_name),
        };
    }

    pub fn merge_branch(&mut self) {
        if self.active_panel != Panel::Branches { return; }
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let Some(branch) = repo.branches.get(self.selected_branch) else { return };
        if branch.is_current {
            self.notify("Already on this branch".to_string(), false);
            return;
        }
        self.mode = Mode::Confirm {
            message: format!("Merge {} into {}? [y/n]", branch.name, repo.branch),
            action: ConfirmAction::MergeBranch(repo.path.clone(), branch.name.clone()),
        };
    }

    pub fn execute_text_input(&mut self) {
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        if let Mode::TextInput { input, action, .. } = mode {
            if input.is_empty() {
                self.notify("Cancelled (empty input)".to_string(), false);
                return;
            }
            let Some(repo) = self.repos.get(self.selected_repo) else { return };
            let path = repo.path.clone();
            match action {
                TextInputAction::CreateBranch => {
                    let name = input;
                    self.dispatch(path, &format!("Create branch {}", name), move |p| {
                        git::git_create_branch(p, &name)
                    });
                }
                TextInputAction::RenameBranch(old) => {
                    let new = input;
                    self.dispatch(path, &format!("Rename {} -> {}", old, new), move |p| {
                        git::git_rename_branch(p, &old, &new)
                    });
                }
                TextInputAction::CommitMessage => {
                    let msg = input;
                    self.dispatch(path, "Commit", move |p| {
                        git::git_commit(p, &msg)
                    });
                }
            }
        }
    }

    pub fn toggle_hide(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let repo_name = repo.name.clone();
        self.config.toggle_hidden(&self.workspace_name, &repo_name);
        let _ = self.config.save();
        self.notify(format!("Toggled hide: {}", repo_name), false);
        self.refresh();
    }

    pub fn toggle_show_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.refresh();
    }

    pub fn switch_workspace(&mut self, workspace_name: &str) {
        if let Some(ws) = self.config.workspaces.get(workspace_name) {
            let path = crate::config::expand_path(&ws.path);
            self.workspace_path = PathBuf::from(&path);
            self.workspace_name = workspace_name.to_string();
            self.selected_repo = 0;
            self.selected_branch = 0;
            self.refresh();
        }
    }

    pub fn workspace_names(&self) -> Vec<String> {
        self.config.workspaces.keys().cloned().collect()
    }

    pub fn execute_confirm(&mut self) {
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        if let Mode::Confirm { action, .. } = mode {
            match action {
                ConfirmAction::Push(path) => {
                    self.dispatch(path, "Push", git::git_push);
                }
                ConfirmAction::BulkPush(paths) => {
                    for path in paths {
                        self.dispatch(path, "Push", git::git_push);
                    }
                }
                ConfirmAction::StashPop(path) => {
                    self.push_undo(UndoOp::StashPop { repo_path: path.clone() });
                    self.dispatch(path, "Stash pop", git::git_stash_pop);
                }
                ConfirmAction::DeleteBranch(path, name) => {
                    let branch_name = name.clone();
                    self.dispatch(path, &format!("Delete {}", name), move |p| {
                        git::git_delete_branch(p, &branch_name)
                    });
                }
                ConfirmAction::MergeBranch(path, name) => {
                    let branch_name = name.clone();
                    self.dispatch(path, &format!("Merge {}", name), move |p| {
                        git::git_merge(p, &branch_name)
                    });
                }
                ConfirmAction::DiscardFile(repo_path, file_path, is_untracked) => {
                    match git::git_discard(&repo_path, &file_path, is_untracked) {
                        Ok(_) => self.notify(format!("Discarded: {}", file_path), false),
                        Err(e) => self.notify(format!("Discard failed: {}", e), true),
                    }
                    self.reload_files();
                    self.rescan_selected_repo();
                }
            }
        }
    }

    pub fn cancel_confirm(&mut self) {
        self.mode = Mode::Normal;
        self.notify("Cancelled".to_string(), false);
    }

    pub fn is_repo_busy(&self, path: &Path) -> bool {
        self.pending_ops.contains(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Panel cycling tests ---

    fn next_panel(p: Panel) -> Panel {
        match p {
            Panel::RepoList => Panel::Branches,
            Panel::Branches => Panel::Files,
            Panel::Files => Panel::RepoList,
        }
    }

    #[test]
    fn panel_next_repo_list_to_branches() {
        assert_eq!(next_panel(Panel::RepoList), Panel::Branches);
    }

    #[test]
    fn panel_next_branches_to_files() {
        assert_eq!(next_panel(Panel::Branches), Panel::Files);
    }

    #[test]
    fn panel_next_files_to_repo_list() {
        assert_eq!(next_panel(Panel::Files), Panel::RepoList);
    }

    #[test]
    fn panel_full_cycle() {
        let start = Panel::RepoList;
        let second = next_panel(start);
        let third = next_panel(second);
        let back = next_panel(third);
        assert_eq!(back, Panel::RepoList);
    }

    #[test]
    fn panel_equality() {
        assert_eq!(Panel::RepoList, Panel::RepoList);
        assert_eq!(Panel::Branches, Panel::Branches);
        assert_eq!(Panel::Files, Panel::Files);
    }

    #[test]
    fn panel_inequality() {
        assert_ne!(Panel::RepoList, Panel::Branches);
        assert_ne!(Panel::Branches, Panel::Files);
        assert_ne!(Panel::Files, Panel::RepoList);
    }

    #[test]
    fn panel_clone_and_copy() {
        let p = Panel::Branches;
        let cloned = p;
        let copied = p;
        assert_eq!(cloned, Panel::Branches);
        assert_eq!(copied, Panel::Branches);
    }

    // --- Mode enum tests ---

    #[test]
    fn mode_normal_is_distinct() {
        assert_eq!(Mode::Normal, Mode::Normal);
        assert_ne!(Mode::Normal, Mode::Filter);
        assert_ne!(Mode::Normal, Mode::DiffView);
        assert_ne!(Mode::Normal, Mode::CommandLog);
        assert_ne!(Mode::Normal, Mode::CommitLog);
        assert_ne!(Mode::Normal, Mode::WorkspaceSwitcher);
    }

    #[test]
    fn mode_filter_is_distinct() {
        assert_eq!(Mode::Filter, Mode::Filter);
        assert_ne!(Mode::Filter, Mode::Normal);
        assert_ne!(Mode::Filter, Mode::DiffView);
    }

    #[test]
    fn mode_all_simple_variants_distinct() {
        let variants = [Mode::Normal,
            Mode::WorkspaceSwitcher,
            Mode::DiffView,
            Mode::CommandLog,
            Mode::CommitLog,
            Mode::Filter];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b, "Mode variants at index {} and {} should differ", i, j);
                }
            }
        }
    }

    #[test]
    fn mode_confirm_equality() {
        let m1 = Mode::Confirm {
            message: "Push?".to_string(),
            action: ConfirmAction::Push(PathBuf::from("/repo")),
        };
        let m2 = Mode::Confirm {
            message: "Push?".to_string(),
            action: ConfirmAction::Push(PathBuf::from("/repo")),
        };
        assert_eq!(m1, m2);
    }

    #[test]
    fn mode_confirm_not_equal_to_normal() {
        let confirm = Mode::Confirm {
            message: "Delete?".to_string(),
            action: ConfirmAction::DeleteBranch(PathBuf::from("/repo"), "feature".to_string()),
        };
        assert_ne!(confirm, Mode::Normal);
    }

    #[test]
    fn mode_text_input_equality() {
        let m1 = Mode::TextInput {
            prompt: "Branch name:".to_string(),
            input: "feat".to_string(),
            action: TextInputAction::CreateBranch,
        };
        let m2 = Mode::TextInput {
            prompt: "Branch name:".to_string(),
            input: "feat".to_string(),
            action: TextInputAction::CreateBranch,
        };
        assert_eq!(m1, m2);
    }

    #[test]
    fn mode_text_input_different_input() {
        let m1 = Mode::TextInput {
            prompt: "Branch name:".to_string(),
            input: "feat-a".to_string(),
            action: TextInputAction::CreateBranch,
        };
        let m2 = Mode::TextInput {
            prompt: "Branch name:".to_string(),
            input: "feat-b".to_string(),
            action: TextInputAction::CreateBranch,
        };
        assert_ne!(m1, m2);
    }

    // --- ConfirmAction tests ---

    #[test]
    fn confirm_action_push() {
        let action = ConfirmAction::Push(PathBuf::from("/repos/my-repo"));
        if let ConfirmAction::Push(path) = &action {
            assert_eq!(path, &PathBuf::from("/repos/my-repo"));
        } else {
            panic!("Expected ConfirmAction::Push");
        }
    }

    #[test]
    fn confirm_action_bulk_push() {
        let paths = vec![PathBuf::from("/repo1"), PathBuf::from("/repo2")];
        let action = ConfirmAction::BulkPush(paths);
        if let ConfirmAction::BulkPush(p) = &action {
            assert_eq!(p.len(), 2);
            assert_eq!(p[0], PathBuf::from("/repo1"));
            assert_eq!(p[1], PathBuf::from("/repo2"));
        } else {
            panic!("Expected ConfirmAction::BulkPush");
        }
    }

    #[test]
    fn confirm_action_stash_pop() {
        let action = ConfirmAction::StashPop(PathBuf::from("/repo"));
        assert!(matches!(action, ConfirmAction::StashPop(_)));
    }

    #[test]
    fn confirm_action_discard_file() {
        let action = ConfirmAction::DiscardFile(PathBuf::from("/repo"), "main.rs".to_string(), false);
        if let ConfirmAction::DiscardFile(repo, file, untracked) = &action {
            assert_eq!(repo, &PathBuf::from("/repo"));
            assert_eq!(file, "main.rs");
            assert!(!untracked);
        } else {
            panic!("Expected ConfirmAction::DiscardFile");
        }
    }

    #[test]
    fn confirm_action_delete_branch() {
        let action = ConfirmAction::DeleteBranch(PathBuf::from("/repo"), "old-feature".to_string());
        if let ConfirmAction::DeleteBranch(repo, branch) = &action {
            assert_eq!(repo, &PathBuf::from("/repo"));
            assert_eq!(branch, "old-feature");
        } else {
            panic!("Expected ConfirmAction::DeleteBranch");
        }
    }

    #[test]
    fn confirm_action_merge_branch() {
        let action = ConfirmAction::MergeBranch(PathBuf::from("/repo"), "develop".to_string());
        if let ConfirmAction::MergeBranch(repo, branch) = &action {
            assert_eq!(repo, &PathBuf::from("/repo"));
            assert_eq!(branch, "develop");
        } else {
            panic!("Expected ConfirmAction::MergeBranch");
        }
    }

    #[test]
    fn confirm_action_equality() {
        let a1 = ConfirmAction::Push(PathBuf::from("/repo"));
        let a2 = ConfirmAction::Push(PathBuf::from("/repo"));
        assert_eq!(a1, a2);
    }

    #[test]
    fn confirm_action_inequality_different_variants() {
        let a1 = ConfirmAction::Push(PathBuf::from("/repo"));
        let a2 = ConfirmAction::StashPop(PathBuf::from("/repo"));
        assert_ne!(a1, a2);
    }

    // --- TextInputAction tests ---

    #[test]
    fn text_input_action_create_branch() {
        let action = TextInputAction::CreateBranch;
        assert_eq!(action, TextInputAction::CreateBranch);
    }

    #[test]
    fn text_input_action_rename_branch() {
        let action = TextInputAction::RenameBranch("old-name".to_string());
        if let TextInputAction::RenameBranch(name) = &action {
            assert_eq!(name, "old-name");
        } else {
            panic!("Expected TextInputAction::RenameBranch");
        }
    }

    #[test]
    fn text_input_action_commit_message() {
        let action = TextInputAction::CommitMessage;
        assert_eq!(action, TextInputAction::CommitMessage);
    }

    #[test]
    fn text_input_action_variants_distinct() {
        assert_ne!(TextInputAction::CreateBranch, TextInputAction::CommitMessage);
        assert_ne!(
            TextInputAction::CreateBranch,
            TextInputAction::RenameBranch("x".to_string())
        );
        assert_ne!(
            TextInputAction::CommitMessage,
            TextInputAction::RenameBranch("x".to_string())
        );
    }

    #[test]
    fn text_input_action_rename_branch_equality() {
        let a = TextInputAction::RenameBranch("feat".to_string());
        let b = TextInputAction::RenameBranch("feat".to_string());
        assert_eq!(a, b);
    }

    #[test]
    fn text_input_action_rename_branch_inequality() {
        let a = TextInputAction::RenameBranch("feat-a".to_string());
        let b = TextInputAction::RenameBranch("feat-b".to_string());
        assert_ne!(a, b);
    }

    // --- UndoOp tests ---

    #[test]
    fn undo_op_checkout() {
        let op = UndoOp::Checkout {
            repo_path: PathBuf::from("/repos/app"),
            previous_branch: "main".to_string(),
        };
        if let UndoOp::Checkout { repo_path, previous_branch } = &op {
            assert_eq!(repo_path, &PathBuf::from("/repos/app"));
            assert_eq!(previous_branch, "main");
        } else {
            panic!("Expected UndoOp::Checkout");
        }
    }

    #[test]
    fn undo_op_stash() {
        let op = UndoOp::Stash {
            repo_path: PathBuf::from("/repos/lib"),
        };
        assert!(matches!(op, UndoOp::Stash { .. }));
    }

    #[test]
    fn undo_op_stash_pop() {
        let op = UndoOp::StashPop {
            repo_path: PathBuf::from("/repos/lib"),
        };
        if let UndoOp::StashPop { repo_path } = &op {
            assert_eq!(repo_path, &PathBuf::from("/repos/lib"));
        } else {
            panic!("Expected UndoOp::StashPop");
        }
    }

    #[test]
    fn undo_op_clone() {
        let op = UndoOp::Checkout {
            repo_path: PathBuf::from("/repo"),
            previous_branch: "develop".to_string(),
        };
        let cloned = op.clone();
        if let UndoOp::Checkout { repo_path, previous_branch } = cloned {
            assert_eq!(repo_path, PathBuf::from("/repo"));
            assert_eq!(previous_branch, "develop");
        } else {
            panic!("Cloned UndoOp should be Checkout");
        }
    }

    // --- CommandLogEntry tests ---

    #[test]
    fn command_log_entry_construction() {
        let entry = CommandLogEntry {
            timestamp: Instant::now(),
            repo_name: "my-repo".to_string(),
            command: "git pull".to_string(),
            success: true,
            output: "Already up to date.".to_string(),
        };
        assert_eq!(entry.repo_name, "my-repo");
        assert_eq!(entry.command, "git pull");
        assert!(entry.success);
        assert_eq!(entry.output, "Already up to date.");
    }

    #[test]
    fn command_log_entry_failure() {
        let entry = CommandLogEntry {
            timestamp: Instant::now(),
            repo_name: "broken-repo".to_string(),
            command: "git push".to_string(),
            success: false,
            output: "rejected: non-fast-forward".to_string(),
        };
        assert!(!entry.success);
        assert_eq!(entry.output, "rejected: non-fast-forward");
    }

    #[test]
    fn command_log_entry_clone() {
        let entry = CommandLogEntry {
            timestamp: Instant::now(),
            repo_name: "repo".to_string(),
            command: "git fetch".to_string(),
            success: true,
            output: String::new(),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.repo_name, "repo");
        assert_eq!(cloned.command, "git fetch");
        assert!(cloned.success);
        assert!(cloned.output.is_empty());
    }

    // --- CommitEntry tests ---

    #[test]
    fn commit_entry_construction() {
        let entry = CommitEntry {
            hash: "abc1234".to_string(),
            author: "Alice <alice@example.com>".to_string(),
            date: "2025-01-15".to_string(),
            message: "fix: resolve null pointer in parser".to_string(),
        };
        assert_eq!(entry.hash, "abc1234");
        assert_eq!(entry.author, "Alice <alice@example.com>");
        assert_eq!(entry.date, "2025-01-15");
        assert_eq!(entry.message, "fix: resolve null pointer in parser");
    }

    #[test]
    fn commit_entry_clone() {
        let entry = CommitEntry {
            hash: "deadbeef".to_string(),
            author: "Bob".to_string(),
            date: "2025-06-01".to_string(),
            message: "feat: add search".to_string(),
        };
        let cloned = entry.clone();
        assert_eq!(cloned.hash, "deadbeef");
        assert_eq!(cloned.author, "Bob");
        assert_eq!(cloned.date, "2025-06-01");
        assert_eq!(cloned.message, "feat: add search");
    }

    #[test]
    fn commit_entry_empty_fields() {
        let entry = CommitEntry {
            hash: String::new(),
            author: String::new(),
            date: String::new(),
            message: String::new(),
        };
        assert!(entry.hash.is_empty());
        assert!(entry.author.is_empty());
        assert!(entry.date.is_empty());
        assert!(entry.message.is_empty());
    }

    // --- DiscardFile with untracked flag ---

    #[test]
    fn confirm_action_discard_file_untracked() {
        let action = ConfirmAction::DiscardFile(
            PathBuf::from("/repo"),
            "new_file.txt".to_string(),
            true,
        );
        if let ConfirmAction::DiscardFile(_, _, untracked) = &action {
            assert!(untracked);
        } else {
            panic!("Expected ConfirmAction::DiscardFile");
        }
    }

    #[test]
    fn confirm_action_discard_file_tracked() {
        let action = ConfirmAction::DiscardFile(
            PathBuf::from("/repo"),
            "existing.rs".to_string(),
            false,
        );
        if let ConfirmAction::DiscardFile(_, file, untracked) = &action {
            assert_eq!(file, "existing.rs");
            assert!(!untracked);
        } else {
            panic!("Expected ConfirmAction::DiscardFile");
        }
    }
}
