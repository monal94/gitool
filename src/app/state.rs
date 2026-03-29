use super::*;

impl App {
    pub fn refresh(&mut self) {
        let workspace_path = self.workspace_path.clone();
        let tx = self.task_tx.clone();
        self.invalidate_repo_cache();
        rayon::spawn(move || {
            let all_repos = git::scan_workspace(&workspace_path, &[]);
            let _ = tx.send(GitResult::WorkspaceReady { all_repos });
        });
    }

    pub(crate) fn apply_workspace_ready(&mut self, all_repos: Vec<RepoStatus>) {
        let hidden = if self.show_hidden {
            vec![]
        } else {
            self.config.hidden_repos(&self.workspace_name)
        };
        self.all_repos = all_repos;
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

    /// Load the commit log for the currently selected repo (async).
    pub fn load_log(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = repo.path.clone();
        let tx = self.task_tx.clone();
        // Reset state immediately for responsive UI
        self.commit_log.clear();
        self.commit_log_selected = 0;
        self.commit_files.clear();
        self.commit_files_selected = 0;
        self.preview_content.clear();
        self.preview_scroll = 0;
        // Load in background
        rayon::spawn(move || {
            let commits = git::git_log(&path, 200);
            let _ = tx.send(GitResult::LogReady { commits });
        });
    }

    /// Load files and diff for the currently selected commit (async).
    pub fn load_commit_detail(&mut self) {
        let Some(entry) = self.commit_log.get(self.commit_log_selected) else {
            self.commit_files.clear();
            self.preview_content.clear();
            return;
        };
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let hash = entry.hash.clone();
        let path = repo.path.clone();
        let tx = self.task_tx.clone();
        rayon::spawn(move || {
            let files = git::git_show_files(&path, &hash).unwrap_or_default();
            let diff = git::git_diff_commit(&path, &hash).unwrap_or_default();
            let _ = tx.send(GitResult::CommitDetailReady { files, diff });
        });
    }

    /// Load per-file diff for the selected file in commit detail (async).
    pub fn load_commit_file_diff(&mut self) {
        let Some(file) = self.commit_files.get(self.commit_files_selected) else { return };
        let Some(entry) = self.commit_log.get(self.commit_log_selected) else { return };
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = repo.path.clone();
        let hash = entry.hash.clone();
        let file_path = file.path.clone();
        let tx = self.task_tx.clone();
        rayon::spawn(move || {
            let diff = git::git_diff_commit_file(&path, &hash, &file_path).unwrap_or_default();
            let _ = tx.send(GitResult::CommitFileDiffReady { diff });
        });
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
}
