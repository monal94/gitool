use super::*;

impl App {
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
            self.mode = Mode::TextInput {
                prompt: "Stash message: ".to_string(),
                input: String::new(),
                action: TextInputAction::StashMessage,
            };
        } else if repo.stash > 0 {
            self.mode = Mode::Confirm {
                message: format!("Pop stash for {}? [y/n]", repo.name),
                action: ConfirmAction::StashPop(path),
            };
        } else {
            self.notify("Nothing to stash/pop".to_string(), false);
        }
    }

    pub fn stash_drop_selected(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        if self.stash_list.is_empty() {
            self.notify("No stash entries".to_string(), false);
            return;
        }
        let Some(entry) = self.stash_list.get(self.selected_stash) else { return };
        let path = repo.path.clone();
        let index = entry.index;
        self.mode = Mode::Confirm {
            message: format!("Drop stash@{{{}}}? [y/n]", index),
            action: ConfirmAction::StashDrop(path, index),
        };
    }

    pub fn checkout_selected(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = repo.path.clone();

        let branch_name = match self.active_side {
            SidePanel::Branches => {
                repo.branches.get(self.selected_branch).map(|b| b.name.clone())
            }
            _ => None,
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

    pub fn stage_selected_file(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let Some(file) = self.files.get(self.selected_file) else { return };
        if file.staged {
            self.notify("Already staged".to_string(), false);
            return;
        }
        let path = repo.path.clone();
        let file_path = file.path.clone();
        self.dispatch(path, &format!("Stage {}", file_path), move |p| {
            git::git_stage(p, &file_path)
        });
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
        self.dispatch(path, &format!("Unstage {}", file_path), move |p| {
            git::git_unstage(p, &file_path)
        });
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

    pub fn show_commit_log(&mut self) {
        self.switch_panel(SidePanel::Commits);
    }

    pub fn create_commit_prompt(&mut self) {
        self.mode = Mode::TextInput {
            prompt: "Commit message: ".to_string(),
            input: String::new(),
            action: TextInputAction::CommitMessage,
        };
    }

    pub fn amend_commit_prompt(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let path = &repo.path;
        let old_message = match git2::Repository::open(path) {
            Ok(r) => match r.head().and_then(|h| h.peel_to_commit()) {
                Ok(commit) => commit.message().unwrap_or("").to_string(),
                Err(_) => {
                    self.notify("No commits to amend".to_string(), true);
                    return;
                }
            },
            Err(e) => {
                self.notify(format!("Cannot open repo: {}", e), true);
                return;
            }
        };
        self.mode = Mode::TextInput {
            prompt: "Amend commit message: ".to_string(),
            input: old_message,
            action: TextInputAction::AmendCommit,
        };
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
                ConfirmAction::StashDrop(path, index) => {
                    match git::git_stash_drop(&path, index) {
                        Ok(msg) => self.notify(msg, false),
                        Err(e) => self.notify(format!("Stash drop failed: {}", e), true),
                    }
                    self.rescan_selected_repo();
                }
                ConfirmAction::CherryPick(path, hash) => {
                    let h = hash.clone();
                    self.dispatch(path, &format!("Cherry-pick {}", hash), move |p| {
                        git::git_cherry_pick(p, &h)
                    });
                }
                ConfirmAction::RevertCommit(path, hash) => {
                    let h = hash.clone();
                    self.dispatch(path, &format!("Revert {}", hash), move |p| {
                        git::git_revert(p, &h)
                    });
                }
            }
        }
    }

    pub fn cancel_confirm(&mut self) {
        self.mode = Mode::Normal;
        self.notify("Cancelled".to_string(), false);
    }

    pub fn open_in_editor(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let Some(file) = self.files.get(self.selected_file) else { return };
        let full_path = repo.path.join(&file.path);
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vim".to_string());
        self.editor_command = Some((editor, full_path));
    }

    pub fn show_blame(&mut self) {
        let Some(repo) = self.repos.get(self.selected_repo) else { return };
        let Some(file) = self.files.get(self.selected_file) else { return };
        let path = repo.path.clone();
        let file_path = file.path.clone();
        match git::git_blame(&path, &file_path) {
            Ok(lines) if !lines.is_empty() => {
                self.blame_content = lines;
                self.blame_scroll = 0;
                self.mode = Mode::BlameView;
            }
            Ok(_) => self.notify("No blame data".to_string(), false),
            Err(e) => self.notify(format!("Blame failed: {}", e), true),
        }
    }

    pub(crate) fn rescan_selected_repo(&mut self) {
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
}
