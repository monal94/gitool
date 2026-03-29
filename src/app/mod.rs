#![allow(dead_code)]
use crate::config::Config;
use crate::git;
use crate::types::{FileEntry, RepoStatus};
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

mod branch_ops;
mod dispatch;
mod git_ops;
mod navigation;
mod state;
mod undo;
mod watcher;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SidePanel {
    Repos,     // 1
    Files,     // 2
    Branches,  // 3
    Commits,   // 4
    Stash,     // 5
}

impl SidePanel {
    pub fn next(self) -> Self {
        match self {
            Self::Repos => Self::Files,
            Self::Files => Self::Branches,
            Self::Branches => Self::Commits,
            Self::Commits => Self::Stash,
            Self::Stash => Self::Repos,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Repos => Self::Stash,
            Self::Files => Self::Repos,
            Self::Branches => Self::Files,
            Self::Commits => Self::Branches,
            Self::Stash => Self::Commits,
        }
    }

    pub fn from_num(n: char) -> Option<Self> {
        match n {
            '1' => Some(Self::Repos),
            '2' => Some(Self::Files),
            '3' => Some(Self::Branches),
            '4' => Some(Self::Commits),
            '5' => Some(Self::Stash),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StashEntry {
    pub index: usize,
    pub message: String,
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
    BlameView,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextInputAction {
    CreateBranch,
    RenameBranch(String),
    CommitMessage,
    AmendCommit,
    StashMessage,
    CreateTag(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfirmAction {
    Push(PathBuf),
    BulkPush(Vec<PathBuf>),
    StashPop(PathBuf),
    DiscardFile(PathBuf, String, bool), // repo_path, file_path, is_untracked
    DeleteBranch(PathBuf, String),
    MergeBranch(PathBuf, String),
    StashDrop(PathBuf, usize),
    CherryPick(PathBuf, String),
    RevertCommit(PathBuf, String),
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
    #[allow(dead_code)]
    pub author: String,
    pub date: String,
    pub message: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CommitFileEntry {
    pub path: String,
    pub status: char, // 'M', 'A', 'D', 'R'
}

#[derive(Debug, Clone)]
pub struct BlameLine {
    pub hash: String,
    pub author: String,
    pub line_no: usize,
    pub content: String,
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
    FileChanged {
        repo_path: Option<PathBuf>,
    },
    WorkspaceReady {
        all_repos: Vec<RepoStatus>,
    },
    LogReady {
        commits: Vec<CommitEntry>,
    },
    CommitDetailReady {
        files: Vec<CommitFileEntry>,
        diff: String,
    },
    #[allow(dead_code)]
    CommitFileDiffReady {
        diff: String,
    },
}

pub struct App {
    // Data
    pub repos: Vec<RepoStatus>,
    pub all_repos: Vec<RepoStatus>,
    pub files: Vec<FileEntry>,
    pub commit_log: Vec<CommitEntry>,
    pub commit_files: Vec<CommitFileEntry>,
    pub stash_list: Vec<StashEntry>,
    pub command_log: Vec<CommandLogEntry>,
    // Selection indices
    pub selected_repo: usize,
    pub selected_file: usize,
    pub selected_branch: usize,
    pub commit_log_selected: usize,
    pub commit_files_selected: usize,
    pub selected_stash: usize,
    // UI state
    pub active_side: SidePanel,
    pub mode: Mode,
    pub notification: Option<Notification>,
    pub dirty: bool,
    pub should_quit: bool,
    // Preview (right panel)
    pub preview_content: String,
    pub preview_scroll: usize,
    // Diff overlay
    pub diff_content: String,
    pub diff_scroll: u16,
    // Blame overlay
    pub blame_content: Vec<BlameLine>,
    pub blame_scroll: usize,
    // Command log overlay
    pub command_log_scroll: u16,
    // Workspace
    pub config: Config,
    pub workspace_path: PathBuf,
    pub workspace_name: String,
    pub workspace_selector_index: usize,
    pub show_hidden: bool,
    pub filter_text: String,
    pub filter_active: bool,
    // Operations
    pub pending_ops: HashSet<PathBuf>,
    pub marked_repos: HashSet<PathBuf>,
    pub undo_stack: Vec<UndoOp>,
    // Editor
    pub editor_command: Option<(String, std::path::PathBuf)>,
    // Internal
    pub highlighter: crate::highlight::Highlighter,
    cached_repo: Option<git2::Repository>,
    cached_repo_path: Option<PathBuf>,
    files_generation: u64,
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
            files: Vec::new(),
            commit_log: Vec::new(),
            commit_files: Vec::new(),
            stash_list: Vec::new(),
            command_log: Vec::new(),
            selected_repo: 0,
            selected_file: 0,
            selected_branch: 0,
            commit_log_selected: 0,
            commit_files_selected: 0,
            selected_stash: 0,
            active_side: SidePanel::Repos,
            mode: Mode::Normal,
            notification: None,
            dirty: true,
            should_quit: false,
            preview_content: String::new(),
            preview_scroll: 0,
            diff_content: String::new(),
            diff_scroll: 0,
            blame_content: Vec::new(),
            blame_scroll: 0,
            command_log_scroll: 0,
            config,
            workspace_path,
            workspace_name,
            workspace_selector_index: 0,
            show_hidden: false,
            filter_text: String::new(),
            filter_active: false,
            pending_ops: HashSet::new(),
            marked_repos: HashSet::new(),
            undo_stack: Vec::new(),
            editor_command: None,
            highlighter: crate::highlight::Highlighter::new(),
            cached_repo: None,
            cached_repo_path: None,
            files_generation: 0,
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
        if self.filter_active && !self.filter_text.is_empty() && self.active_side == SidePanel::Repos {
            let ft = self.filter_text.to_lowercase();
            self.repos.iter().filter(|r| r.name.to_lowercase().contains(&ft)).collect()
        } else {
            self.repos.iter().collect()
        }
    }

    pub fn filtered_branch_indices(&self) -> Option<Vec<usize>> {
        if !self.filter_active || self.filter_text.is_empty() || self.active_side != SidePanel::Branches {
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

    pub fn is_repo_marked(&self, path: &Path) -> bool {
        self.marked_repos.contains(path)
    }

    pub fn is_repo_busy(&self, path: &Path) -> bool {
        self.pending_ops.contains(path)
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

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
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
                    // Invalidate cached repo since state changed
                    self.invalidate_repo_cache();
                    self.files_generation = self.files_generation.wrapping_add(1); // force file reload
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
                GitResult::WorkspaceReady { all_repos } => {
                    self.apply_workspace_ready(all_repos);
                    self.dirty = true;
                }
                GitResult::FileChanged { repo_path } => {
                    if let Some(repo_path) = repo_path {
                        // Bump generation for the changed repo
                        if let Some(pos) = self.repos.iter().position(|r| r.path == repo_path) {
                            self.repos[pos].generation += 1;
                        }
                        if let Some(pos) = self.all_repos.iter().position(|r| r.path == repo_path) {
                            self.all_repos[pos].generation += 1;
                        }
                        // Selective refresh: only rescan the changed repo
                        self.invalidate_repo_cache();
                        let is_selected = self.repos.get(self.selected_repo)
                            .is_some_and(|r| r.path == repo_path);
                        let new_status = if is_selected {
                            git::scan_repo_full(&repo_path)
                        } else {
                            git::scan_repo(&repo_path)
                        };
                        if let Some(mut new_status) = new_status {
                            // Preserve bumped generation
                            if let Some(old) = self.repos.iter().find(|r| r.path == repo_path) {
                                new_status.generation = old.generation;
                            }
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
                    } else {
                        self.refresh();
                    }
                    self.dirty = true;
                }
                GitResult::LogReady { commits } => {
                    self.commit_log = commits;
                    self.commit_log_selected = 0;
                    if !self.commit_log.is_empty() {
                        self.load_commit_detail();
                    }
                    self.dirty = true;
                }
                GitResult::CommitDetailReady { files, diff } => {
                    self.commit_files = files;
                    self.commit_files_selected = 0;
                    self.preview_content = diff;
                    self.preview_scroll = 0;
                    self.dirty = true;
                }
                GitResult::CommitFileDiffReady { diff } => {
                    self.preview_content = diff;
                    self.preview_scroll = 0;
                    self.dirty = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- SidePanel cycling tests ---

    #[test]
    fn panel_next_repos_to_files() {
        assert_eq!(SidePanel::Repos.next(), SidePanel::Files);
    }

    #[test]
    fn panel_next_files_to_branches() {
        assert_eq!(SidePanel::Files.next(), SidePanel::Branches);
    }

    #[test]
    fn panel_next_branches_to_commits() {
        assert_eq!(SidePanel::Branches.next(), SidePanel::Commits);
    }

    #[test]
    fn panel_next_commits_to_stash() {
        assert_eq!(SidePanel::Commits.next(), SidePanel::Stash);
    }

    #[test]
    fn panel_next_stash_to_repos() {
        assert_eq!(SidePanel::Stash.next(), SidePanel::Repos);
    }

    #[test]
    fn panel_full_cycle() {
        let start = SidePanel::Repos;
        let mut current = start;
        for _ in 0..5 {
            current = current.next();
        }
        assert_eq!(current, SidePanel::Repos);
    }

    #[test]
    fn panel_prev_repos_to_stash() {
        assert_eq!(SidePanel::Repos.prev(), SidePanel::Stash);
    }

    #[test]
    fn panel_equality() {
        assert_eq!(SidePanel::Repos, SidePanel::Repos);
        assert_eq!(SidePanel::Branches, SidePanel::Branches);
        assert_eq!(SidePanel::Files, SidePanel::Files);
        assert_eq!(SidePanel::Commits, SidePanel::Commits);
        assert_eq!(SidePanel::Stash, SidePanel::Stash);
    }

    #[test]
    fn panel_inequality() {
        assert_ne!(SidePanel::Repos, SidePanel::Branches);
        assert_ne!(SidePanel::Branches, SidePanel::Files);
        assert_ne!(SidePanel::Files, SidePanel::Commits);
        assert_ne!(SidePanel::Commits, SidePanel::Stash);
        assert_ne!(SidePanel::Stash, SidePanel::Repos);
    }

    #[test]
    fn panel_clone_and_copy() {
        let p = SidePanel::Branches;
        let cloned = p;
        let copied = p;
        assert_eq!(cloned, SidePanel::Branches);
        assert_eq!(copied, SidePanel::Branches);
    }

    #[test]
    fn panel_from_num() {
        assert_eq!(SidePanel::from_num('1'), Some(SidePanel::Repos));
        assert_eq!(SidePanel::from_num('2'), Some(SidePanel::Files));
        assert_eq!(SidePanel::from_num('3'), Some(SidePanel::Branches));
        assert_eq!(SidePanel::from_num('4'), Some(SidePanel::Commits));
        assert_eq!(SidePanel::from_num('5'), Some(SidePanel::Stash));
        assert_eq!(SidePanel::from_num('0'), None);
        assert_eq!(SidePanel::from_num('6'), None);
    }

    // --- Mode enum tests ---

    #[test]
    fn mode_normal_is_distinct() {
        assert_eq!(Mode::Normal, Mode::Normal);
        assert_ne!(Mode::Normal, Mode::Filter);
        assert_ne!(Mode::Normal, Mode::DiffView);
        assert_ne!(Mode::Normal, Mode::CommandLog);
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
