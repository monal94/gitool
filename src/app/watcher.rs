use super::*;

impl App {
    pub(crate) fn start_watcher(&mut self) {
        let tx = self.task_tx.clone();
        let repo_paths: Vec<PathBuf> = self.all_repos.iter().map(|r| r.path.clone()).collect();
        let debouncer = new_debouncer(
            Duration::from_millis(500),
            move |res: Result<Vec<notify_debouncer_mini::DebouncedEvent>, _>| {
                if let Ok(events) = res {
                    // Find which repo the event belongs to
                    let repo_path = events.iter()
                        .filter(|e| matches!(e.kind, DebouncedEventKind::Any))
                        .find_map(|e| {
                            let event_path = e.path.to_string_lossy();
                            repo_paths.iter().find(|rp| {
                                event_path.starts_with(rp.to_string_lossy().as_ref())
                            }).cloned()
                        });
                    if repo_path.is_some() || events.iter().any(|e| matches!(e.kind, DebouncedEventKind::Any)) {
                        let _ = tx.send(GitResult::FileChanged { repo_path });
                    }
                }
            },
        );

        if let Ok(mut debouncer) = debouncer {
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

    /// Get or cache a Repository handle for the given path.
    pub(crate) fn ensure_cached_repo(&mut self, path: &Path) {
        if self.cached_repo_path.as_deref() != Some(path) {
            self.cached_repo = git2::Repository::open(path).ok();
            self.cached_repo_path = Some(path.to_path_buf());
        }
    }

    /// Invalidate the cached repo (call after mutations that change repo state).
    pub(crate) fn invalidate_repo_cache(&mut self) {
        self.cached_repo = None;
        self.cached_repo_path = None;
    }
}
