use super::*;

impl App {
    pub fn move_up(&mut self) {
        match self.active_side {
            SidePanel::Repos => {
                if self.selected_repo > 0 {
                    self.selected_repo -= 1;
                    self.selected_branch = 0;
                    self.ensure_branches_loaded();
                }
            }
            SidePanel::Branches => {
                if self.selected_branch > 0 {
                    self.selected_branch -= 1;
                }
            }
            SidePanel::Files => {
                if self.selected_file > 0 {
                    self.selected_file -= 1;
                }
            }
            SidePanel::Commits => {
                self.log_move_up();
            }
            SidePanel::Stash => {
                if self.selected_stash > 0 {
                    self.selected_stash -= 1;
                }
            }
        }
    }

    pub fn move_down(&mut self) {
        match self.active_side {
            SidePanel::Repos => {
                if self.selected_repo + 1 < self.repos.len() {
                    self.selected_repo += 1;
                    self.selected_branch = 0;
                    self.ensure_branches_loaded();
                }
            }
            SidePanel::Branches => {
                if let Some(repo) = self.selected_repo()
                    && self.selected_branch + 1 < repo.branches.len() {
                        self.selected_branch += 1;
                    }
            }
            SidePanel::Files => {
                if self.selected_file + 1 < self.files.len() {
                    self.selected_file += 1;
                }
            }
            SidePanel::Commits => {
                self.log_move_down();
            }
            SidePanel::Stash => {
                if self.selected_stash + 1 < self.stash_list.len() {
                    self.selected_stash += 1;
                }
            }
        }
    }

    /// Alias for `move_up` — used by key handlers.
    pub fn side_move_up(&mut self) {
        self.move_up();
        self.load_preview();
    }

    /// Alias for `move_down` — used by key handlers.
    pub fn side_move_down(&mut self) {
        self.move_down();
        self.load_preview();
    }

    /// Request a preview load (debounced — actual dispatch happens in poll_results after 80ms).
    pub fn load_preview(&mut self) {
        self.preview_pending = true;
        self.preview_requested_at = std::time::Instant::now();
    }

    /// Actually dispatch the preview load (called from poll_results after debounce).
    pub fn dispatch_preview(&mut self) {
        match self.active_side {
            SidePanel::Repos => {
                // Rendered directly from repo data, no async needed
                self.preview_content.clear();
                self.dirty = true;
            }
            SidePanel::Files => {
                let Some(repo) = self.repos.get(self.selected_repo) else { return };
                let Some(file) = self.files.get(self.selected_file) else {
                    self.preview_content.clear();
                    return;
                };
                let path = repo.path.clone();
                let file_path = file.path.clone();
                let staged = file.staged;
                let tx = self.task_tx.clone();
                rayon::spawn(move || {
                    let content = git::git_diff_file(&path, &file_path, staged)
                        .unwrap_or_default();
                    let _ = tx.send(GitResult::PreviewReady { content });
                });
            }
            SidePanel::Branches => {
                let Some(repo) = self.repos.get(self.selected_repo) else { return };
                let path = repo.path.clone();
                let tx = self.task_tx.clone();
                rayon::spawn(move || {
                    let commits = git::git_log(&path, 50);
                    let _ = tx.send(GitResult::LogReady { commits });
                });
            }
            SidePanel::Commits => {
                // Already handled by load_commit_detail in log_move_up/down
            }
            SidePanel::Stash => {
                let Some(repo) = self.repos.get(self.selected_repo) else { return };
                let stash_idx = self.selected_stash;
                let path = repo.path.clone();
                let tx = self.task_tx.clone();
                rayon::spawn(move || {
                    let content = std::process::Command::new("git")
                        .args(["stash", "show", "-p", &format!("stash@{{{}}}", stash_idx)])
                        .current_dir(&path)
                        .output()
                        .ok()
                        .filter(|o| o.status.success())
                        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                        .unwrap_or_default();
                    let _ = tx.send(GitResult::PreviewReady { content });
                });
            }
        }
    }

    pub fn next_panel(&mut self) {
        self.active_side = self.active_side.next();
    }

    pub fn prev_panel(&mut self) {
        self.active_side = self.active_side.prev();
    }

    pub fn switch_panel(&mut self, panel: SidePanel) {
        self.active_side = panel;
        match panel {
            SidePanel::Repos => {
                self.ensure_branches_loaded();
            }
            SidePanel::Files => {
                self.reload_files();
            }
            SidePanel::Branches => {
                self.ensure_branches_loaded();
            }
            SidePanel::Commits => {
                self.load_log();
            }
            SidePanel::Stash => {
                self.load_stash_list();
            }
        }
        self.load_preview();
    }

    pub fn load_stash_list(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = &repo.path;
        if let Ok(repo) = git2::Repository::open(path) {
            let mut entries = Vec::new();
            if let Ok(reflog) = repo.reflog("refs/stash") {
                for i in 0..reflog.len() {
                    if let Some(entry) = reflog.get(i) {
                        entries.push(StashEntry {
                            index: i,
                            message: entry.message().unwrap_or("").to_string(),
                        });
                    }
                }
            }
            self.stash_list = entries;
            self.selected_stash = 0;
        }
    }

    /// Load branches for the currently selected repo if not already loaded.
    pub fn ensure_branches_loaded(&mut self) {
        if let Some(repo) = self.repos.get(self.selected_repo) {
            let path = repo.path.clone();
            if !repo.branches_loaded {
                self.ensure_cached_repo(&path);
                let branches = if let Some(ref cached) = self.cached_repo {
                    git::load_branches_with_repo(cached)
                } else {
                    git::load_branches(&path)
                };
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
    pub(crate) fn reload_files(&mut self) {
        if let Some(repo) = self.repos.get(self.selected_repo) {
            // Skip if generation hasn't changed (files already up to date)
            if repo.generation == self.files_generation && !self.files.is_empty() {
                return;
            }
            let path = repo.path.clone();
            let current_gen = repo.generation;
            self.ensure_cached_repo(&path);
            self.files = if let Some(ref cached) = self.cached_repo {
                git::get_file_statuses_with_repo(cached)
            } else {
                git::get_file_statuses(&path)
            };
            self.files_generation = current_gen;
            if self.selected_file >= self.files.len() {
                self.selected_file = self.files.len().saturating_sub(1);
            }
        } else {
            self.files.clear();
            self.selected_file = 0;
            self.files_generation = 0;
        }
    }

    pub fn next_log_panel(&mut self) {
        // In commits view, cycle focus: commit list -> commit files -> back
        // The diff preview is always shown in the right panel.
        if self.commit_files_selected == 0 && !self.commit_files.is_empty() {
            // Focus shifts to commit files sub-list
            self.commit_files_selected = 0;
        }
    }

    pub fn log_move_up(&mut self) {
        if self.commit_log_selected > 0 {
            self.commit_log_selected -= 1;
            self.load_commit_detail();
        }
    }

    pub fn log_move_down(&mut self) {
        if self.commit_log_selected + 1 < self.commit_log.len() {
            self.commit_log_selected += 1;
            self.load_commit_detail();
        }
    }

    pub fn log_page_down(&mut self) {
        self.preview_scroll += 20;
    }

    pub fn log_page_up(&mut self) {
        self.preview_scroll = self.preview_scroll.saturating_sub(20);
    }
}
