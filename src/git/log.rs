use git2::Repository;
use std::path::Path;

pub fn git_log(path: &Path, limit: usize) -> Vec<crate::app::CommitEntry> {
    let Ok(repo) = Repository::open(path) else { return Vec::new() };
    let Ok(mut revwalk) = repo.revwalk() else { return Vec::new() };
    revwalk.set_sorting(git2::Sort::TIME).ok();
    if revwalk.push_head().is_err() { return Vec::new(); }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    revwalk
        .filter_map(|oid| oid.ok())
        .filter_map(|oid| repo.find_commit(oid).ok())
        .take(limit)
        .map(|commit| {
            let hash = format!("{:.7}", commit.id());
            let author = commit.author().name().unwrap_or("").to_string();
            let message = commit.summary().unwrap_or("").to_string();
            let time = commit.time().seconds();
            let elapsed = now - time;
            let date = format_elapsed(elapsed);
            crate::app::CommitEntry { hash, author, date, message }
        })
        .collect()
}

fn format_elapsed(secs: i64) -> String {
    if secs < 60 { return format!("{} seconds ago", secs); }
    let mins = secs / 60;
    if mins < 60 { return format!("{} minutes ago", mins); }
    let hours = mins / 60;
    if hours < 24 { return format!("{} hours ago", hours); }
    let days = hours / 24;
    if days < 30 { return format!("{} days ago", days); }
    let months = days / 30;
    if months < 12 { return format!("{} months ago", months); }
    format!("{} years ago", days / 365)
}

/// Get files changed in a specific commit using git2.
pub fn git_show_files(path: &Path, hash: &str) -> Result<Vec<crate::app::CommitFileEntry>, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let obj = repo.revparse_single(hash).map_err(|e| e.to_string())?;
    let commit = obj.peel_to_commit().map_err(|e| e.to_string())?;
    let tree = commit.tree().map_err(|e| e.to_string())?;

    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

    let diff = repo.diff_tree_to_tree(
        parent_tree.as_ref(), Some(&tree), None,
    ).map_err(|e| e.to_string())?;
    let mut find_opts = git2::DiffFindOptions::new();
    find_opts.renames(true);
    let mut diff = diff;
    let _ = diff.find_similar(Some(&mut find_opts));

    let mut files = Vec::new();
    for delta in diff.deltas() {
        let status = match delta.status() {
            git2::Delta::Added => 'A',
            git2::Delta::Deleted => 'D',
            git2::Delta::Modified => 'M',
            git2::Delta::Renamed => 'R',
            git2::Delta::Copied => 'C',
            _ => '?',
        };
        let file_path = delta.new_file().path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        files.push(crate::app::CommitFileEntry { status, path: file_path });
    }
    Ok(files)
}

/// Get blame information for a file using git2 (no subprocess).
pub fn git_blame(path: &Path, file: &str) -> Result<Vec<crate::app::BlameLine>, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let blame = repo.blame_file(std::path::Path::new(file), None)
        .map_err(|e| e.to_string())?;

    // Read the file content to get line text
    let file_path = path.join(file);
    let content = std::fs::read_to_string(&file_path)
        .map_err(|e| e.to_string())?;
    let file_lines: Vec<&str> = content.lines().collect();

    let mut lines = Vec::new();
    for hunk in blame.iter() {
        let hash = format!("{:.7}", hunk.final_commit_id());
        let author = hunk.final_signature().name().unwrap_or("").to_string();
        let start = hunk.final_start_line();
        let hunk_lines = hunk.lines_in_hunk();

        for offset in 0..hunk_lines {
            let line_no = start + offset;
            let content = file_lines
                .get(line_no.saturating_sub(1))
                .unwrap_or(&"")
                .to_string();
            lines.push(crate::app::BlameLine {
                hash: hash.clone(),
                author: author.clone(),
                line_no,
                content,
            });
        }
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{tmp_dir, init_repo_with_commit};
    use git2::Repository;
    use std::fs;
    use std::path::Path;

    // ---------------------------------------------------------------
    // 10. git_log on this repo -> non-empty commit list with correct fields
    // ---------------------------------------------------------------
    #[test]
    fn git_log_returns_commits() {
        // Use the actual gitool repository for this test
        let repo_path = Path::new(env!("CARGO_MANIFEST_DIR"));

        let commits = git_log(repo_path, 10);
        assert!(!commits.is_empty(), "git_log should return at least one commit");

        let first = &commits[0];
        assert!(!first.hash.is_empty(), "Commit hash should not be empty");
        assert!(!first.author.is_empty(), "Author should not be empty");
        assert!(!first.date.is_empty(), "Date should not be empty");
        assert!(!first.message.is_empty(), "Message should not be empty");
    }

    #[test]
    fn git_log_respects_limit() {
        let repo_path = Path::new(env!("CARGO_MANIFEST_DIR"));

        let commits = git_log(repo_path, 1);
        assert_eq!(
            commits.len(),
            1,
            "git_log with limit=1 should return exactly 1 commit"
        );
    }

    #[test]
    fn git_log_on_fresh_repo_with_commits() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("log-repo", &tmp);

        // Add a second commit
        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("file.txt"), "data").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Second commit", &tree, &[&parent])
            .unwrap();

        let commits = git_log(&repo_path, 10);
        assert_eq!(commits.len(), 2, "Should have exactly 2 commits");
        assert_eq!(commits[0].message, "Second commit");
        assert_eq!(commits[1].message, "Initial commit");
        assert_eq!(commits[0].author, "Test User");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn git_log_on_invalid_path_returns_empty() {
        let commits = git_log(Path::new("/nonexistent/repo"), 10);
        assert!(commits.is_empty());
    }

    // ---------------------------------------------------------------
    // git_show_files tests
    // ---------------------------------------------------------------
    #[test]
    fn git_show_files_returns_added_file() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("show-files", &tmp);

        // Add and commit a file
        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("new.txt"), "content").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("new.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add new.txt", &tree, &[&parent]).unwrap();

        // Get the latest commit hash
        let commits = git_log(&repo_path, 1);
        assert!(!commits.is_empty());
        let hash = &commits[0].hash;

        let files = git_show_files(&repo_path, hash).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "new.txt");
        assert_eq!(files[0].status, 'A');
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn git_show_files_returns_modified_file() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("show-mod", &tmp);

        let repo = Repository::open(&repo_path).unwrap();
        // Create and commit a file
        fs::write(repo_path.join("file.txt"), "original").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add file", &tree, &[&parent]).unwrap();

        // Modify and commit again
        fs::write(repo_path.join("file.txt"), "modified").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Modify file", &tree, &[&parent]).unwrap();

        let commits = git_log(&repo_path, 1);
        let files = git_show_files(&repo_path, &commits[0].hash).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, 'M');
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // git_blame tests
    // ---------------------------------------------------------------
    #[test]
    fn git_blame_returns_lines() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("blame-test", &tmp);

        // Create and commit a file with multiple lines
        let repo = Repository::open(&repo_path).unwrap();
        fs::write(repo_path.join("code.rs"), "line1\nline2\nline3\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("code.rs")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Add code.rs", &tree, &[&parent]).unwrap();

        let lines = git_blame(&repo_path, "code.rs").unwrap();
        assert_eq!(lines.len(), 3, "Should have 3 blame lines");
        assert_eq!(lines[0].content, "line1");
        assert_eq!(lines[1].content, "line2");
        assert_eq!(lines[2].content, "line3");
        assert!(!lines[0].hash.is_empty(), "Hash should not be empty");
        assert!(!lines[0].author.is_empty(), "Author should not be empty");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn git_blame_on_invalid_file_returns_error() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("blame-err", &tmp);

        let result = git_blame(&repo_path, "nonexistent.txt");
        assert!(result.is_err(), "Blaming a non-existent file should return an error");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // format_elapsed tests
    // ---------------------------------------------------------------
    #[test]
    fn format_elapsed_seconds() {
        assert_eq!(format_elapsed(30), "30 seconds ago");
    }

    #[test]
    fn format_elapsed_minutes() {
        assert_eq!(format_elapsed(120), "2 minutes ago");
    }

    #[test]
    fn format_elapsed_hours() {
        assert_eq!(format_elapsed(7200), "2 hours ago");
    }

    #[test]
    fn format_elapsed_days() {
        assert_eq!(format_elapsed(172800), "2 days ago");
    }
}
