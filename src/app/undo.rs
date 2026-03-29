use super::*;

impl App {
    // Zoom mode removed in new layout.

    pub(crate) fn push_undo(&mut self, op: UndoOp) {
        if self.undo_stack.len() >= 50 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(op);
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
}
