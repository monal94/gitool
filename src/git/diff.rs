use git2::Repository;
use std::path::Path;

const MAX_DIFF_LINES: usize = 50_000;

#[allow(dead_code)]
pub fn git_diff(path: &Path) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let diff = repo.diff_index_to_workdir(None, None)
        .map_err(|e| e.to_string())?;
    diff_to_string(&diff)
}

/// Get the diff for a specific file in the working tree using git2.
pub fn git_diff_file(path: &Path, file: &str, staged: bool) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let mut opts = git2::DiffOptions::new();
    opts.pathspec(file);

    let diff = if staged {
        let head_tree = repo.head().ok()
            .and_then(|h| h.peel_to_tree().ok());
        repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut opts))
    } else {
        repo.diff_index_to_workdir(None, Some(&mut opts))
    }.map_err(|e| e.to_string())?;

    diff_to_string(&diff)
}

/// Get the full diff for a specific commit using git2.
pub fn git_diff_commit(path: &Path, hash: &str) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let obj = repo.revparse_single(hash).map_err(|e| e.to_string())?;
    let commit = obj.peel_to_commit().map_err(|e| e.to_string())?;
    let tree = commit.tree().map_err(|e| e.to_string())?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), None)
        .map_err(|e| e.to_string())?;
    diff_to_string(&diff)
}

#[allow(dead_code)]
/// Get the diff for a specific file in a commit using git2.
pub fn git_diff_commit_file(path: &Path, hash: &str, file: &str) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let obj = repo.revparse_single(hash).map_err(|e| e.to_string())?;
    let commit = obj.peel_to_commit().map_err(|e| e.to_string())?;
    let tree = commit.tree().map_err(|e| e.to_string())?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

    let mut opts = git2::DiffOptions::new();
    opts.pathspec(file);
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut opts))
        .map_err(|e| e.to_string())?;
    diff_to_string(&diff)
}

/// Convert a git2::Diff to a patch string, capped at MAX_DIFF_LINES.
fn diff_to_string(diff: &git2::Diff) -> Result<String, String> {
    let mut output = String::with_capacity(64 * 1024); // pre-alloc 64KB
    let mut line_count = 0usize;
    let mut truncated = false;
    let print_result = diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        if line_count >= MAX_DIFF_LINES {
            truncated = true;
            return false; // stop iteration
        }
        match line.origin() {
            '+' | '-' | ' ' => output.push(line.origin()),
            _ => {}
        }
        output.push_str(&String::from_utf8_lossy(line.content()));
        line_count += 1;
        true
    });
    // When truncated, diff.print returns an error because the callback returned false.
    // Only propagate unexpected errors (i.e., not caused by our intentional truncation).
    if !truncated {
        print_result.map_err(|e| e.to_string())?;
    }
    if truncated {
        output.push_str("\n\n... diff truncated (50000+ lines) ...\n");
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{tmp_dir, init_repo_with_commit};
    use super::super::log::git_log;
    use git2::Repository;
    use std::fs;
    use std::path::Path;

    // ---------------------------------------------------------------
    // git_diff_commit / git_diff_commit_file tests
    // ---------------------------------------------------------------
    #[test]
    fn git_diff_commit_returns_diff_content() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("diff-commit", &tmp);

        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("code.rs"), "fn main() {}").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("code.rs")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add code", &tree, &[&parent]).unwrap();

        let commits = git_log(&repo_path, 1);
        let diff = git_diff_commit(&repo_path, &commits[0].hash).unwrap();
        assert!(diff.contains("code.rs"));
        assert!(diff.contains("+fn main() {}"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn git_diff_commit_file_returns_file_diff() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("diff-file", &tmp);

        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("a.txt"), "aaa").unwrap();
        fs::write(repo_path.join("b.txt"), "bbb").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("a.txt")).unwrap();
        index.add_path(Path::new("b.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add files", &tree, &[&parent]).unwrap();

        let commits = git_log(&repo_path, 1);
        let diff = git_diff_commit_file(&repo_path, &commits[0].hash, "a.txt").unwrap();
        assert!(diff.contains("a.txt"));
        assert!(diff.contains("+aaa"));
        // Should NOT contain b.txt
        assert!(!diff.contains("b.txt"));
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // git_diff_file tests
    // ---------------------------------------------------------------
    #[test]
    fn git_diff_file_unstaged() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("diff-wt", &tmp);

        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("tracked.txt"), "original").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add tracked", &tree, &[&parent]).unwrap();

        // Modify without staging
        fs::write(repo_path.join("tracked.txt"), "changed").unwrap();

        let diff = git_diff_file(&repo_path, "tracked.txt", false).unwrap();
        assert!(diff.contains("-original"));
        assert!(diff.contains("+changed"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn git_diff_file_staged() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("diff-staged", &tmp);

        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("tracked.txt"), "original").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add tracked", &tree, &[&parent]).unwrap();

        // Modify and stage
        fs::write(repo_path.join("tracked.txt"), "staged content").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("tracked.txt")).unwrap();
        index.write().unwrap();

        let diff = git_diff_file(&repo_path, "tracked.txt", true).unwrap();
        assert!(diff.contains("-original"));
        assert!(diff.contains("+staged content"));
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // diff_to_string truncation test
    // ---------------------------------------------------------------
    #[test]
    fn diff_to_string_caps_at_max_lines() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("diff-truncate", &tmp);

        let repo = Repository::open(&repo_path).unwrap();

        // Create a file with more than MAX_DIFF_LINES lines
        let line_count = MAX_DIFF_LINES + 1000;
        let content: String = (0..line_count)
            .map(|i| format!("line {}\n", i))
            .collect();
        fs::write(repo_path.join("big.txt"), &content).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("big.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add big file", &tree, &[&parent]).unwrap();

        let commits = git_log(&repo_path, 1);
        let diff = git_diff_commit(&repo_path, &commits[0].hash).unwrap();
        assert!(
            diff.contains("diff truncated"),
            "Diff should contain truncation message when exceeding MAX_DIFF_LINES"
        );
        let _ = fs::remove_dir_all(&tmp);
    }
}
