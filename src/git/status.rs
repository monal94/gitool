use crate::types::{FileEntry, FileStatus};
use git2::{Repository, Status, StatusOptions};
use std::path::Path;

/// Get individual file statuses for a repo (staged and unstaged).
pub fn get_file_statuses(path: &Path) -> Vec<FileEntry> {
    let Ok(repo) = Repository::open(path) else {
        return Vec::new();
    };
    get_file_statuses_with_repo(&repo)
}

/// Get file statuses using an already-opened Repository handle.
pub fn get_file_statuses_with_repo(repo: &Repository) -> Vec<FileEntry> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true);
    let Ok(statuses) = repo.statuses(Some(&mut opts)) else {
        return Vec::new();
    };

    let mut files = Vec::new();
    for entry in statuses.iter() {
        let path_str = entry.path().unwrap_or("").to_string();
        let s = entry.status();

        // Index (staged) statuses
        if s.intersects(Status::INDEX_NEW | Status::INDEX_MODIFIED | Status::INDEX_DELETED | Status::INDEX_RENAMED | Status::INDEX_TYPECHANGE) {
            let status = if s.contains(Status::INDEX_NEW) {
                FileStatus::Added
            } else if s.contains(Status::INDEX_MODIFIED) {
                FileStatus::Modified
            } else if s.contains(Status::INDEX_DELETED) {
                FileStatus::Deleted
            } else if s.contains(Status::INDEX_RENAMED) {
                FileStatus::Renamed
            } else {
                FileStatus::Typechange
            };
            files.push(FileEntry { path: path_str.clone(), status, staged: true });
        }

        // Worktree (unstaged) statuses
        if s.intersects(Status::WT_MODIFIED | Status::WT_DELETED | Status::WT_RENAMED | Status::WT_TYPECHANGE) {
            let status = if s.contains(Status::WT_MODIFIED) {
                FileStatus::Modified
            } else if s.contains(Status::WT_DELETED) {
                FileStatus::Deleted
            } else if s.contains(Status::WT_RENAMED) {
                FileStatus::Renamed
            } else {
                FileStatus::Typechange
            };
            files.push(FileEntry { path: path_str.clone(), status, staged: false });
        }

        // Untracked
        if s.contains(Status::WT_NEW) {
            files.push(FileEntry { path: path_str.clone(), status: FileStatus::Untracked, staged: false });
        }

        // Conflicted
        if s.contains(Status::CONFLICTED) {
            files.push(FileEntry { path: path_str, status: FileStatus::Conflicted, staged: false });
        }
    }

    // Sort: staged first, then by path
    files.sort_by(|a, b| b.staged.cmp(&a.staged).then(a.path.cmp(&b.path)));
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{tmp_dir, init_repo_with_commit};
    use git2::Repository;
    use std::fs;
    use std::path::Path;

    // ---------------------------------------------------------------
    // 8. get_file_statuses on a clean repo -> returns empty
    // ---------------------------------------------------------------
    #[test]
    fn get_file_statuses_clean_repo_returns_empty() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("clean-repo", &tmp);

        let statuses = get_file_statuses(&repo_path);
        assert!(
            statuses.is_empty(),
            "Clean repo should have no file statuses"
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // 9. get_file_statuses on a repo with modifications -> correct entries
    // ---------------------------------------------------------------
    #[test]
    fn get_file_statuses_with_untracked_file() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("dirty-repo", &tmp);

        // Create an untracked file
        fs::write(repo_path.join("new_file.txt"), "hello").unwrap();

        let statuses = get_file_statuses(&repo_path);
        assert!(!statuses.is_empty(), "Should detect the untracked file");

        let untracked = statuses
            .iter()
            .find(|f| f.path == "new_file.txt");
        assert!(untracked.is_some(), "Should find new_file.txt");
        assert_eq!(untracked.unwrap().status, FileStatus::Untracked);
        assert!(!untracked.unwrap().staged);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn get_file_statuses_with_staged_file() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("staged-repo", &tmp);

        // Create and stage a file
        fs::write(repo_path.join("staged.txt"), "content").unwrap();
        let repo = Repository::open(&repo_path).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.txt")).unwrap();
        index.write().unwrap();

        let statuses = get_file_statuses(&repo_path);
        let staged = statuses
            .iter()
            .find(|f| f.path == "staged.txt" && f.staged);
        assert!(staged.is_some(), "Should find staged.txt as staged");
        assert_eq!(staged.unwrap().status, FileStatus::Added);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn get_file_statuses_with_modified_file() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("mod-repo", &tmp);

        // Create a tracked file via commit
        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("tracked.txt"), "original").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add tracked file", &tree, &[&parent])
            .unwrap();

        // Now modify the tracked file (unstaged modification)
        fs::write(repo_path.join("tracked.txt"), "modified content").unwrap();

        let statuses = get_file_statuses(&repo_path);
        let modified = statuses
            .iter()
            .find(|f| f.path == "tracked.txt" && !f.staged);
        assert!(modified.is_some(), "Should detect unstaged modification");
        assert_eq!(modified.unwrap().status, FileStatus::Modified);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn get_file_statuses_sorts_staged_before_unstaged() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("sort-repo", &tmp);

        // Create a staged file
        fs::write(repo_path.join("staged.txt"), "staged content").unwrap();
        let repo = Repository::open(&repo_path).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.txt")).unwrap();
        index.write().unwrap();

        // Create an untracked file
        fs::write(repo_path.join("untracked.txt"), "untracked").unwrap();

        let statuses = get_file_statuses(&repo_path);
        assert!(statuses.len() >= 2);

        // Find positions
        let staged_pos = statuses.iter().position(|f| f.staged);
        let unstaged_pos = statuses.iter().position(|f| !f.staged);
        if let (Some(s), Some(u)) = (staged_pos, unstaged_pos) {
            assert!(
                s < u,
                "Staged files should come before unstaged files"
            );
        }
        let _ = fs::remove_dir_all(&tmp);
    }
}
