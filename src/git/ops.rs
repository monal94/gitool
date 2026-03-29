use git2::{BranchType, Repository};
use std::path::Path;
use std::process::Command;

pub fn git_stage(path: &Path, file: &str) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let mut index = repo.index().map_err(|e| e.to_string())?;
    index.add_path(Path::new(file)).map_err(|e| e.to_string())?;
    index.write().map_err(|e| e.to_string())?;
    Ok(format!("Staged {}", file))
}

pub fn git_unstage(path: &Path, file: &str) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let head = repo.head().and_then(|h| h.peel_to_tree());
    let mut index = repo.index().map_err(|e| e.to_string())?;
    match head {
        Ok(tree) => {
            repo.reset_default(Some(&tree.into_object()), [file])
                .map_err(|e| e.to_string())?;
        }
        Err(_) => {
            // No HEAD (initial commit) -- remove from index
            index.remove_path(Path::new(file)).map_err(|e| e.to_string())?;
            index.write().map_err(|e| e.to_string())?;
        }
    }
    Ok(format!("Unstaged {}", file))
}

pub fn git_discard(path: &Path, file: &str, is_untracked: bool) -> Result<String, String> {
    if is_untracked {
        std::fs::remove_file(path.join(file))
            .map(|_| format!("Removed {}", file))
            .map_err(|e| e.to_string())
    } else {
        let repo = Repository::open(path).map_err(|e| e.to_string())?;
        repo.checkout_head(Some(
            git2::build::CheckoutBuilder::new()
                .force()
                .path(file),
        )).map_err(|e| e.to_string())?;
        Ok(format!("Discarded {}", file))
    }
}

pub fn git_commit(path: &Path, message: &str) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let sig = repo.signature().map_err(|e| e.to_string())?;
    let mut index = repo.index().map_err(|e| e.to_string())?;
    let tree_id = index.write_tree().map_err(|e| e.to_string())?;
    let tree = repo.find_tree(tree_id).map_err(|e| e.to_string())?;
    let parent = repo.head().ok()
        .and_then(|h| h.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent.as_ref().into_iter().collect();
    let oid = repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
        .map_err(|e| e.to_string())?;
    Ok(format!("{:.7}", oid))
}

pub fn git_amend_commit(path: &Path, message: &str) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let head = repo.head().map_err(|e| format!("No HEAD: {}", e))?;
    let head_commit = head.peel_to_commit().map_err(|e| e.to_string())?;
    let mut index = repo.index().map_err(|e| e.to_string())?;
    let tree_id = index.write_tree().map_err(|e| e.to_string())?;
    let tree = repo.find_tree(tree_id).map_err(|e| e.to_string())?;
    let oid = head_commit
        .amend(Some("HEAD"), None, None, None, Some(message), Some(&tree))
        .map_err(|e| e.to_string())?;
    Ok(format!("Amended {:.7}", oid))
}

pub fn git_create_branch(path: &Path, name: &str) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let head = repo.head().map_err(|e| e.to_string())?;
    let commit = head.peel_to_commit().map_err(|e| e.to_string())?;
    repo.branch(name, &commit, false).map_err(|e| e.to_string())?;
    // Also checkout the new branch
    let refname = format!("refs/heads/{}", name);
    repo.set_head(&refname).map_err(|e| e.to_string())?;
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .map_err(|e| e.to_string())?;
    Ok(format!("Created and switched to {}", name))
}

pub fn git_delete_branch(path: &Path, name: &str) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let mut branch = repo.find_branch(name, BranchType::Local)
        .map_err(|e| e.to_string())?;
    branch.delete().map_err(|e| e.to_string())?;
    Ok(format!("Deleted {}", name))
}

pub fn git_rename_branch(path: &Path, old: &str, new: &str) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    repo.find_branch(old, BranchType::Local)
        .map_err(|e| e.to_string())?
        .rename(new, false)
        .map_err(|e| e.to_string())?;
    Ok(format!("Renamed {} -> {}", old, new))
}

pub fn git_merge(path: &Path, branch: &str) -> Result<String, String> {
    // git2 merge is complex (conflict resolution) -- keep CLI for this
    run_git(path, &["merge", "--", branch])
}

// Git mutation operations -- shell out to git CLI

pub fn git_pull(path: &Path) -> Result<String, String> {
    run_git(path, &["pull"])
}

pub fn git_push(path: &Path) -> Result<String, String> {
    run_git(path, &["push"])
}

pub fn git_fetch(path: &Path) -> Result<String, String> {
    run_git(path, &["fetch", "--all", "--prune"])
}

pub fn git_checkout(path: &Path, branch: &str) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let refname = format!("refs/heads/{}", branch);
    // Try local branch first, then remote tracking
    if repo.find_reference(&refname).is_ok() {
        repo.set_head(&refname).map_err(|e| e.to_string())?;
    } else {
        // Remote-only branch: create local tracking branch
        let remote_ref = format!("refs/remotes/origin/{}", branch);
        let reference = repo.find_reference(&remote_ref).map_err(|e| e.to_string())?;
        let commit = reference.peel_to_commit().map_err(|e| e.to_string())?;
        repo.branch(branch, &commit, false).map_err(|e| e.to_string())?;
        repo.set_head(&refname).map_err(|e| e.to_string())?;
    }
    repo.checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .map_err(|e| e.to_string())?;
    Ok(format!("Switched to {}", branch))
}

pub fn git_stash(path: &Path) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let sig = repo.signature().map_err(|e| e.to_string())?;
    // git2 stash_save requires &mut
    let mut repo = repo;
    repo.stash_save(&sig, "gitool stash", None)
        .map_err(|e| e.to_string())?;
    Ok("Stashed".to_string())
}

pub fn git_stash_with_message(path: &Path, message: &str) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let sig = repo.signature().map_err(|e| e.to_string())?;
    let mut repo = repo;
    repo.stash_save(&sig, message, None).map_err(|e| e.to_string())?;
    Ok(format!("Stashed: {}", message))
}

pub fn git_stash_drop(path: &Path, index: usize) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let mut repo = repo;
    repo.stash_drop(index).map_err(|e| e.to_string())?;
    Ok(format!("Dropped stash@{{{}}}", index))
}

pub fn git_stash_pop(path: &Path) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let mut repo = repo;
    repo.stash_pop(0, None).map_err(|e| e.to_string())?;
    Ok("Stash popped".to_string())
}

pub fn git_cherry_pick(path: &Path, hash: &str) -> Result<String, String> {
    run_git(path, &["cherry-pick", hash])
}

pub fn git_revert(path: &Path, hash: &str) -> Result<String, String> {
    run_git(path, &["revert", "--no-edit", hash])
}

pub fn git_create_tag(path: &Path, name: &str, hash: &str) -> Result<String, String> {
    run_git(path, &["tag", name, hash])
}

pub(crate) fn run_git(path: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{tmp_dir, init_repo_with_commit};
    use std::fs;
    use std::path::Path;

    // ---------------------------------------------------------------
    // git_discard tests
    // ---------------------------------------------------------------
    #[test]
    fn git_discard_removes_untracked_file() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("discard-untracked", &tmp);

        let file = repo_path.join("untracked.txt");
        fs::write(&file, "temp data").unwrap();
        assert!(file.exists());

        let result = git_discard(&repo_path, "untracked.txt", true);
        assert!(result.is_ok());
        assert!(!file.exists(), "Untracked file should be deleted");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn git_discard_restores_modified_file() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("discard-modified", &tmp);

        // Create and commit a file
        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("tracked.txt"), "original").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add file", &tree, &[&parent]).unwrap();

        // Modify and discard
        fs::write(repo_path.join("tracked.txt"), "changed").unwrap();
        let result = git_discard(&repo_path, "tracked.txt", false);
        assert!(result.is_ok());
        let content = fs::read_to_string(repo_path.join("tracked.txt")).unwrap();
        assert_eq!(content, "original");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // git_commit tests
    // ---------------------------------------------------------------
    #[test]
    fn git_commit_creates_commit() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("commit-test", &tmp);

        // Record original HEAD
        let repo = Repository::open(&repo_path).unwrap();
        let original_head = repo.head().unwrap().target().unwrap();

        // Create and stage a file
        fs::write(repo_path.join("new.txt"), "content").unwrap();
        git_stage(&repo_path, "new.txt").unwrap();

        // Commit
        let result = git_commit(&repo_path, "Add new.txt");
        assert!(result.is_ok(), "git_commit should succeed");

        // Verify HEAD changed
        let repo = Repository::open(&repo_path).unwrap();
        let new_head = repo.head().unwrap().target().unwrap();
        assert_ne!(original_head, new_head, "HEAD should point to a new commit");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // git_create_branch tests
    // ---------------------------------------------------------------
    #[test]
    fn git_create_branch_creates_and_switches() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("create-branch", &tmp);

        let result = git_create_branch(&repo_path, "feature-x");
        assert!(result.is_ok(), "git_create_branch should succeed");

        // Verify HEAD points to the new branch
        let repo = Repository::open(&repo_path).unwrap();
        let head = repo.head().unwrap();
        let branch_name = head.shorthand().unwrap();
        assert_eq!(branch_name, "feature-x", "HEAD should point to the new branch");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // git_delete_branch tests
    // ---------------------------------------------------------------
    #[test]
    fn git_delete_branch_removes_branch() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("delete-branch", &tmp);

        // Create a branch, then switch away from it
        git_create_branch(&repo_path, "to-delete").unwrap();
        git_checkout(&repo_path, "master").unwrap();

        // Delete it
        let result = git_delete_branch(&repo_path, "to-delete");
        assert!(result.is_ok(), "git_delete_branch should succeed");

        // Verify it's gone
        let repo = Repository::open(&repo_path).unwrap();
        let found = repo.find_branch("to-delete", git2::BranchType::Local);
        assert!(found.is_err(), "Deleted branch should not be found");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // git_rename_branch tests
    // ---------------------------------------------------------------
    #[test]
    fn git_rename_branch_renames() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("rename-branch", &tmp);

        // Create a branch
        git_create_branch(&repo_path, "old-name").unwrap();
        git_checkout(&repo_path, "master").unwrap();

        // Rename it
        let result = git_rename_branch(&repo_path, "old-name", "new-name");
        assert!(result.is_ok(), "git_rename_branch should succeed");

        // Verify new name exists and old name is gone
        let repo = Repository::open(&repo_path).unwrap();
        assert!(
            repo.find_branch("new-name", git2::BranchType::Local).is_ok(),
            "New branch name should exist"
        );
        assert!(
            repo.find_branch("old-name", git2::BranchType::Local).is_err(),
            "Old branch name should be gone"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // git_checkout tests
    // ---------------------------------------------------------------
    #[test]
    fn git_checkout_switches_branch() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("checkout-test", &tmp);

        // Create a branch (this also switches to it)
        git_create_branch(&repo_path, "other").unwrap();
        // Switch back to master
        git_checkout(&repo_path, "master").unwrap();

        let repo = Repository::open(&repo_path).unwrap();
        let head = repo.head().unwrap();
        assert_eq!(head.shorthand().unwrap(), "master");

        // Switch to other
        git_checkout(&repo_path, "other").unwrap();
        let repo = Repository::open(&repo_path).unwrap();
        let head = repo.head().unwrap();
        assert_eq!(head.shorthand().unwrap(), "other", "Should be on 'other' branch");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // git_stash and git_stash_pop roundtrip tests
    // ---------------------------------------------------------------
    #[test]
    fn git_stash_and_pop_roundtrip() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("stash-roundtrip", &tmp);

        // Create and commit a tracked file
        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("file.txt"), "original").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add file", &tree, &[&parent]).unwrap();

        // Dirty the file
        fs::write(repo_path.join("file.txt"), "dirty").unwrap();

        // Stash
        let result = git_stash(&repo_path);
        assert!(result.is_ok(), "git_stash should succeed");

        // Verify working tree is clean (file restored to committed state)
        let content = fs::read_to_string(repo_path.join("file.txt")).unwrap();
        assert_eq!(content, "original", "After stash, file should be clean");

        // Pop
        let result = git_stash_pop(&repo_path);
        assert!(result.is_ok(), "git_stash_pop should succeed");

        // Verify dirty state is back
        let content = fs::read_to_string(repo_path.join("file.txt")).unwrap();
        assert_eq!(content, "dirty", "After pop, file should have dirty content");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // git_stash_with_message tests
    // ---------------------------------------------------------------
    #[test]
    fn git_stash_with_message_uses_message() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("stash-msg", &tmp);

        // Create and commit a tracked file
        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("file.txt"), "original").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add file", &tree, &[&parent]).unwrap();

        // Dirty the file and stash with a message
        fs::write(repo_path.join("file.txt"), "changed").unwrap();
        let result = git_stash_with_message(&repo_path, "my custom stash message");
        assert!(result.is_ok(), "git_stash_with_message should succeed");

        // Check reflog for the message
        let repo = Repository::open(&repo_path).unwrap();
        let reflog = repo.reflog("refs/stash").unwrap();
        assert!(reflog.len() > 0, "Stash reflog should have entries");
        let entry_msg = reflog.get(0).unwrap().message().unwrap_or("").to_string();
        assert!(
            entry_msg.contains("my custom stash message"),
            "Stash reflog should contain the custom message, got: {}",
            entry_msg
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // git_stash_drop tests
    // ---------------------------------------------------------------
    #[test]
    fn git_stash_drop_removes_entry() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("stash-drop", &tmp);

        // Create and commit a tracked file
        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("file.txt"), "original").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add file", &tree, &[&parent]).unwrap();

        // Stash twice
        fs::write(repo_path.join("file.txt"), "change-1").unwrap();
        git_stash(&repo_path).unwrap();
        fs::write(repo_path.join("file.txt"), "change-2").unwrap();
        git_stash(&repo_path).unwrap();

        // Verify we have 2 stashes
        let repo = Repository::open(&repo_path).unwrap();
        let before_count = repo.reflog("refs/stash").unwrap().len();
        assert_eq!(before_count, 2, "Should have 2 stash entries");

        // Drop the first (index 0)
        let result = git_stash_drop(&repo_path, 0);
        assert!(result.is_ok(), "git_stash_drop should succeed");

        // Verify count decreased
        let repo = Repository::open(&repo_path).unwrap();
        let after_count = repo.reflog("refs/stash").unwrap().len();
        assert_eq!(after_count, 1, "Should have 1 stash entry after drop");
        let _ = fs::remove_dir_all(&tmp);
    }
}
