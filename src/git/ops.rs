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
}
