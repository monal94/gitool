use super::*;

impl App {
    /// Dispatch a git operation to a background thread.
    pub(crate) fn dispatch<F>(&mut self, path: PathBuf, label: &str, op: F)
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
        rayon::spawn(move || {
            let result = op(&path);
            let _ = tx.send(GitResult::Done {
                repo_path: path,
                label,
                result,
            });
        });
    }

    pub(crate) fn bulk_targets(&self) -> Vec<PathBuf> {
        if self.marked_repos.is_empty() {
            self.repos.get(self.selected_repo)
                .map(|r| vec![r.path.clone()])
                .unwrap_or_default()
        } else {
            self.marked_repos.iter().cloned().collect()
        }
    }

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

    pub fn show_diff(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = repo.path.clone();
        let tx = self.task_tx.clone();
        rayon::spawn(move || {
            match git::git_diff(&path) {
                Ok(content) => { let _ = tx.send(GitResult::DiffReady { content }); }
                Err(e) => { let _ = tx.send(GitResult::DiffError { message: e }); }
            }
        });
    }

    pub fn show_file_diff(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let Some(file) = self.files.get(self.selected_file) else { return };
        let path = repo.path.clone();
        let file_path = file.path.clone();
        let staged = file.staged;
        let tx = self.task_tx.clone();
        rayon::spawn(move || {
            match git::git_diff_file(&path, &file_path, staged) {
                Ok(content) => { let _ = tx.send(GitResult::DiffReady { content }); }
                Err(e) => { let _ = tx.send(GitResult::DiffError { message: e }); }
            }
        });
    }
}
