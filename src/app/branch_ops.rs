use super::*;

impl App {
    pub fn create_branch_prompt(&mut self) {
        self.mode = Mode::TextInput {
            prompt: "New branch name: ".to_string(),
            input: String::new(),
            action: TextInputAction::CreateBranch,
        };
    }

    pub fn delete_branch(&mut self) {
        if self.active_side != SidePanel::Branches { return; }
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
        if self.active_side != SidePanel::Branches { return; }
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
        if self.active_side != SidePanel::Branches { return; }
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
                TextInputAction::AmendCommit => {
                    let msg = input;
                    self.dispatch(path, "Amend", move |p| {
                        git::git_amend_commit(p, &msg)
                    });
                }
                TextInputAction::StashMessage => {
                    self.push_undo(UndoOp::Stash { repo_path: path.clone() });
                    let msg = input;
                    self.dispatch(path, "Stash", move |p| {
                        git::git_stash_with_message(p, &msg)
                    });
                }
                TextInputAction::CreateTag(hash) => {
                    let name = input;
                    let h = hash.clone();
                    self.dispatch(path, &format!("Tag {} at {}", name, hash), move |p| {
                        git::git_create_tag(p, &name, &h)
                    });
                }
            }
        }
    }
}
