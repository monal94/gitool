use crate::config::Config;
use crate::git;
use crate::types::{FileEntry, RepoStatus};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Panel {
    RepoList,
    Branches,
    Files,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    WorkspaceSwitcher,
    DiffView,
    CommandLog,
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
    RenameBranch(String), // old name
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    Push(PathBuf),
    BulkPush(Vec<PathBuf>),
    StashPop(PathBuf),
    DiscardFile(PathBuf, String),
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
    pub marked_repos: HashSet<usize>,
    pub filter_text: String,
    pub filter_active: bool,
    pub files: Vec<FileEntry>,
    pub selected_file: usize,
    pub command_log: Vec<CommandLogEntry>,
    pub command_log_scroll: u16,
    pub dirty: bool,
    result_rx: Receiver<GitResult>,
    task_tx: Sender<GitResult>,
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
            command_log: Vec::new(),
            command_log_scroll: 0,
            dirty: true,
            result_rx,
            task_tx,
        };
        app.ensure_branches_loaded();
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
                    // Rescan the affected repo
                    if let Some(new_status) = git::scan_repo(&repo_path) {
                        let is_selected = self.repos.get(self.selected_repo)
                            .is_some_and(|r| r.path == repo_path);
                        if let Some(pos) = self.repos.iter().position(|r| r.path == repo_path) {
                            self.repos[pos] = new_status.clone();
                        }
                        if let Some(pos) = self.all_repos.iter().position(|r| r.path == repo_path) {
                            self.all_repos[pos] = new_status;
                        }
                        if is_selected {
                            self.ensure_branches_loaded();
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
        if let Some(ref n) = self.notification {
            if n.created.elapsed().as_secs() >= 3 {
                self.notification = None;
                self.dirty = true;
            }
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
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
            }
            self.reload_files();
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
                if let Some(repo) = self.selected_repo() {
                    if self.selected_branch + 1 < repo.branches.len() {
                        self.selected_branch += 1;
                    }
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
        if self.marked_repos.contains(&self.selected_repo) {
            self.marked_repos.remove(&self.selected_repo);
        } else {
            self.marked_repos.insert(self.selected_repo);
        }
    }

    pub fn mark_all_repos(&mut self) {
        for i in 0..self.repos.len() {
            self.marked_repos.insert(i);
        }
    }

    pub fn unmark_all_repos(&mut self) {
        self.marked_repos.clear();
    }

    fn bulk_targets(&self) -> Vec<PathBuf> {
        if self.marked_repos.is_empty() {
            // No marks: operate on selected repo only
            self.repos.get(self.selected_repo)
                .map(|r| vec![r.path.clone()])
                .unwrap_or_default()
        } else {
            self.marked_repos.iter()
                .filter_map(|&i| self.repos.get(i).map(|r| r.path.clone()))
                .collect()
        }
    }

    // Git actions — all non-blocking via dispatch

    pub fn pull(&mut self) {
        let targets = self.bulk_targets();
        for path in targets {
            self.dispatch(path, "Pull", |p| git::git_pull(p));
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
            self.dispatch(path, "Fetch", |p| git::git_fetch(p));
        }
    }

    pub fn stash_toggle(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = repo.path.clone();
        if repo.dirty > 0 {
            self.dispatch(path, "Stash", |p| git::git_stash(p));
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
            action: ConfirmAction::DiscardFile(repo.path.clone(), file.path.clone()),
        };
    }

    fn rescan_selected_repo(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = repo.path.clone();
        if let Some(new_status) = git::scan_repo(&path) {
            if let Some(pos) = self.repos.iter().position(|r| r.path == path) {
                self.repos[pos] = new_status.clone();
            }
            if let Some(pos) = self.all_repos.iter().position(|r| r.path == path) {
                self.all_repos[pos] = new_status;
            }
            self.ensure_branches_loaded();
        }
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
                    self.dispatch(path, "Push", |p| git::git_push(p));
                }
                ConfirmAction::BulkPush(paths) => {
                    for path in paths {
                        self.dispatch(path, "Push", |p| git::git_push(p));
                    }
                }
                ConfirmAction::StashPop(path) => {
                    self.dispatch(path, "Stash pop", |p| git::git_stash_pop(p));
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
                ConfirmAction::DiscardFile(repo_path, file_path) => {
                    match git::git_discard(&repo_path, &file_path) {
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

    pub fn is_repo_busy(&self, path: &PathBuf) -> bool {
        self.pending_ops.contains(path)
    }
}
