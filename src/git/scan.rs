use crate::types::{BranchEntry, RepoStatus};
use git2::{BranchType, Repository, StatusOptions};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn scan_workspace(path: &Path, hidden: &[String]) -> Vec<RepoStatus> {
    // If the path itself is a git repo, treat it as a single-repo workspace
    if path.join(".git").is_dir() {
        return scan_repo(path).into_iter().collect();
    }

    let Ok(entries) = std::fs::read_dir(path) else {
        return Vec::new();
    };

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join(".git").is_dir())
        .filter(|e| {
            !hidden.contains(
                &e.file_name().to_string_lossy().to_string(),
            )
        })
        .map(|e| e.path())
        .collect();
    dirs.sort();

    let mut repos: Vec<RepoStatus> = std::thread::scope(|s| {
        let handles: Vec<_> = dirs
            .iter()
            .map(|dir| s.spawn(|| scan_repo(dir)))
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok().flatten())
            .collect()
    });
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    repos
}

pub fn scan_repo(path: &Path) -> Option<RepoStatus> {
    let repo = Repository::open(path).ok()?;
    let name = path.file_name()?.to_string_lossy().to_string();

    let branch = current_branch(&repo);
    let (ahead, behind) = upstream_drift(&repo);
    let dirty = dirty_count(&repo);
    let stash = stash_count(&repo);
    let default_branch = detect_default_branch(&repo);

    Some(RepoStatus {
        name,
        path: path.to_path_buf(),
        branch,
        ahead,
        behind,
        dirty,
        stash,
        branches: Vec::new(),
        branches_loaded: false,
        default_branch,
        generation: 0,
    })
}

/// Full scan including branches -- single Repository::open call.
pub fn scan_repo_full(path: &Path) -> Option<RepoStatus> {
    let repo = Repository::open(path).ok()?;
    let name = path.file_name()?.to_string_lossy().to_string();

    let branch = current_branch(&repo);
    let (ahead, behind) = upstream_drift(&repo);
    let dirty = dirty_count(&repo);
    let stash = stash_count(&repo);
    let default_branch = detect_default_branch(&repo);
    let branches = collect_branches(&repo, &branch, &default_branch);

    Some(RepoStatus {
        name,
        path: path.to_path_buf(),
        branch,
        ahead,
        behind,
        dirty,
        stash,
        branches,
        branches_loaded: true,
        default_branch,
        generation: 0,
    })
}

/// Load branches and drift for a single repo (called on-demand).
pub fn load_branches(path: &Path) -> Vec<BranchEntry> {
    let Ok(repo) = Repository::open(path) else {
        return Vec::new();
    };
    load_branches_with_repo(&repo)
}

/// Load branches using an already-opened Repository handle.
pub fn load_branches_with_repo(repo: &Repository) -> Vec<BranchEntry> {
    let branch = current_branch(repo);
    let default_branch = detect_default_branch(repo);
    collect_branches(repo, &branch, &default_branch)
}

fn current_branch(repo: &Repository) -> String {
    repo.head()
        .ok()
        .and_then(|h| h.shorthand().map(String::from))
        .unwrap_or_else(|| "HEAD".to_string())
}

fn upstream_drift(repo: &Repository) -> (usize, usize) {
    let Ok(head_ref) = repo.head() else { return (0, 0) };
    let Some(head_oid) = head_ref.target() else { return (0, 0) };

    let upstream_oid = head_ref
        .shorthand()
        .and_then(|name| repo.find_branch(name, BranchType::Local).ok())
        .and_then(|b| b.upstream().ok())
        .and_then(|u| u.get().target());

    let Some(upstream_oid) = upstream_oid else { return (0, 0) };

    repo.graph_ahead_behind(head_oid, upstream_oid)
        .unwrap_or((0, 0))
}

fn dirty_count(repo: &Repository) -> usize {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(false);
    repo.statuses(Some(&mut opts))
        .map(|s| s.len())
        .unwrap_or(0)
}

/// Detect the default branch name (main, master, develop, etc.)
fn detect_default_branch(repo: &Repository) -> String {
    // 1. Check origin/HEAD symbolic ref
    if let Ok(reference) = repo.find_reference("refs/remotes/origin/HEAD")
        && let Ok(resolved) = reference.resolve()
            && let Some(name) = resolved.shorthand()
                && let Some(branch) = name.strip_prefix("origin/") {
                    return branch.to_string();
                }
    // 2. Fallback: try common names
    for name in &["main", "master", "develop"] {
        if repo.find_branch(name, BranchType::Local).is_ok()
            || repo.find_branch(&format!("origin/{}", name), BranchType::Remote).is_ok()
        {
            return name.to_string();
        }
    }
    "main".to_string()
}

fn stash_count(repo: &Repository) -> usize {
    repo.reflog("refs/stash").map(|r| r.len()).unwrap_or(0)
}

/// Collect a unified branch list merging local and remote refs by name.
fn collect_branches(repo: &Repository, current: &str, default_branch: &str) -> Vec<BranchEntry> {
    let main_oid = repo
        .find_branch(default_branch, BranchType::Local)
        .ok()
        .and_then(|b| b.get().target());

    let origin_main_oid = repo
        .find_branch(&format!("origin/{}", default_branch), BranchType::Remote)
        .ok()
        .and_then(|b| b.get().target());

    let head_ref = repo
        .find_reference("refs/remotes/origin/HEAD")
        .ok()
        .and_then(|r| r.resolve().ok())
        .and_then(|r| r.shorthand().map(|s| s.strip_prefix("origin/").unwrap_or(s).to_string()));

    // Use BTreeMap to merge local + remote by branch name
    let mut map: BTreeMap<String, BranchEntry> = BTreeMap::new();

    // Collect local branches
    if let Ok(branch_iter) = repo.branches(Some(BranchType::Local)) {
        for branch in branch_iter.flatten() {
            let (b, _) = branch;
            let Some(name) = b.name().ok().flatten().map(String::from) else {
                continue;
            };
            let oid = b.get().target();

            let (ahead_main, behind_main) = drift(repo, oid, main_oid);

            let remote_name = format!("origin/{}", name);
            let remote_oid = repo
                .find_branch(&remote_name, BranchType::Remote)
                .ok()
                .and_then(|b| b.get().target());
            let has_remote = remote_oid.is_some();

            let (ahead_remote, behind_remote) = drift(repo, oid, remote_oid);

            map.insert(name.clone(), BranchEntry {
                name: name.clone(),
                is_current: name == current,
                is_head_ref: head_ref.as_deref() == Some(&name),
                has_local: true,
                has_remote,
                ahead_main,
                behind_main,
                ahead_remote,
                behind_remote,
            });
        }
    }

    // Collect remote branches -- only add those not already in map (remote-only)
    if let Ok(branch_iter) = repo.branches(Some(BranchType::Remote)) {
        for branch in branch_iter.flatten() {
            let (b, _) = branch;
            let Some(full_name) = b.name().ok().flatten().map(String::from) else {
                continue;
            };
            let name = full_name
                .strip_prefix("origin/")
                .unwrap_or(&full_name)
                .to_string();
            if name == "HEAD" {
                continue;
            }

            if map.contains_key(&name) {
                // Already added from local -- just ensure is_head_ref is set
                if head_ref.as_deref() == Some(&name)
                    && let Some(entry) = map.get_mut(&name) {
                        entry.is_head_ref = true;
                    }
                continue;
            }

            let oid = b.get().target();
            let (ahead_main, behind_main) = drift(repo, oid, origin_main_oid);

            map.insert(name.clone(), BranchEntry {
                name: name.clone(),
                is_current: false,
                is_head_ref: head_ref.as_deref() == Some(&name),
                has_local: false,
                has_remote: true,
                ahead_main,
                behind_main,
                ahead_remote: None,
                behind_remote: None,
            });
        }
    }

    // Sort: main first, then alphabetical
    let mut branches: Vec<BranchEntry> = map.into_values().collect();
    branches.sort_by(|a, b| {
        let a_main = if a.name == default_branch { 0 } else { 1 };
        let b_main = if b.name == default_branch { 0 } else { 1 };
        a_main.cmp(&b_main).then(a.name.cmp(&b.name))
    });
    branches
}

fn drift(
    repo: &Repository,
    oid: Option<git2::Oid>,
    target: Option<git2::Oid>,
) -> (Option<usize>, Option<usize>) {
    match (oid, target) {
        (Some(o), Some(t)) if o != t => {
            repo.graph_ahead_behind(o, t)
                .ok()
                .map(|(a, b)| (Some(a), Some(b)))
                .unwrap_or((None, None))
        }
        (Some(_), Some(_)) => (Some(0), Some(0)),
        _ => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{tmp_dir, init_repo_with_commit};
    use std::fs;
    use std::path::Path;

    // ---------------------------------------------------------------
    // 1. scan_workspace on an empty directory -> returns empty vec
    // ---------------------------------------------------------------
    #[test]
    fn scan_workspace_empty_dir_returns_empty() {
        let tmp = tmp_dir();
        let repos = scan_workspace(&tmp, &[]);
        assert!(repos.is_empty(), "Expected no repos in an empty directory");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // 2. scan_workspace on a directory that IS a git repo -> single repo
    // ---------------------------------------------------------------
    #[test]
    fn scan_workspace_on_git_repo_returns_single_repo() {
        let tmp = tmp_dir();
        // Init the temp dir itself as a git repo
        Repository::init(&tmp).unwrap();

        let repos = scan_workspace(&tmp, &[]);
        assert_eq!(repos.len(), 1, "Expected exactly one repo when path is itself a git repo");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // 3. scan_workspace on a directory containing git repos -> multiple repos
    // ---------------------------------------------------------------
    #[test]
    fn scan_workspace_finds_multiple_repos() {
        let tmp = tmp_dir();

        init_repo_with_commit("alpha", &tmp);
        init_repo_with_commit("beta", &tmp);
        init_repo_with_commit("gamma", &tmp);

        // Also create a plain directory that is NOT a repo
        fs::create_dir_all(tmp.join("not-a-repo")).unwrap();

        let repos = scan_workspace(&tmp, &[]);
        assert_eq!(repos.len(), 3, "Expected exactly 3 repos");

        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        assert!(names.contains(&"gamma"));

        // Verify sorted order
        assert_eq!(names, vec!["alpha", "beta", "gamma"]);
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // 4. scan_workspace respects hidden repos filter
    // ---------------------------------------------------------------
    #[test]
    fn scan_workspace_respects_hidden_filter() {
        let tmp = tmp_dir();

        init_repo_with_commit("visible", &tmp);
        init_repo_with_commit("hidden-repo", &tmp);
        init_repo_with_commit("also-visible", &tmp);

        let hidden = vec!["hidden-repo".to_string()];
        let repos = scan_workspace(&tmp, &hidden);

        assert_eq!(repos.len(), 2, "Hidden repo should be excluded");
        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"visible"));
        assert!(names.contains(&"also-visible"));
        assert!(!names.contains(&"hidden-repo"));
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // 5. scan_repo on a valid git repo -> correct name/branch
    // ---------------------------------------------------------------
    #[test]
    fn scan_repo_returns_correct_name_and_branch() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("my-project", &tmp);

        let status = scan_repo(&repo_path).expect("scan_repo should return Some");

        assert_eq!(status.name, "my-project");
        // Default branch after init + commit is typically "master" or "main"
        assert!(
            !status.branch.is_empty(),
            "Branch name should not be empty"
        );
        assert_eq!(status.ahead, 0);
        assert_eq!(status.behind, 0);
        assert_eq!(status.dirty, 0);
        assert_eq!(status.stash, 0);
        assert!(!status.branches_loaded);
        assert!(status.branches.is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_repo_on_nonexistent_path_returns_none() {
        let result = scan_repo(Path::new("/nonexistent/path/to/repo"));
        assert!(result.is_none());
    }

    // ---------------------------------------------------------------
    // 6. scan_repo_full -> returns with branches_loaded = true
    // ---------------------------------------------------------------
    #[test]
    fn scan_repo_full_sets_branches_loaded() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("full-scan", &tmp);

        let status = scan_repo_full(&repo_path).expect("scan_repo_full should return Some");

        assert!(status.branches_loaded, "branches_loaded should be true");
        assert_eq!(status.name, "full-scan");
        // Should have at least one branch (the current one)
        assert!(
            !status.branches.is_empty(),
            "branches should contain at least the current branch"
        );

        // The current branch should be marked as is_current
        let current = status.branches.iter().find(|b| b.is_current);
        assert!(current.is_some(), "One branch should be marked as current");
        assert_eq!(current.unwrap().name, status.branch);
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // 7. load_branches on a repo -> non-empty list including current
    // ---------------------------------------------------------------
    #[test]
    fn load_branches_returns_current_branch() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("branch-test", &tmp);

        let branches = load_branches(&repo_path);
        assert!(!branches.is_empty(), "Should have at least one branch");

        let current = branches.iter().find(|b| b.is_current);
        assert!(current.is_some(), "One branch should be current");
        assert!(current.unwrap().has_local);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_branches_with_multiple_branches() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("multi-branch", &tmp);

        // Create additional branches using git2
        let repo = Repository::open(&repo_path).unwrap();
        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature-a", &head_commit, false).unwrap();
        repo.branch("feature-b", &head_commit, false).unwrap();

        let branches = load_branches(&repo_path);
        assert!(branches.len() >= 3, "Should have at least 3 branches");

        let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"feature-a"));
        assert!(names.contains(&"feature-b"));

        // Exactly one should be current
        let current_count = branches.iter().filter(|b| b.is_current).count();
        assert_eq!(current_count, 1, "Exactly one branch should be current");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_branches_on_invalid_path_returns_empty() {
        let branches = load_branches(Path::new("/nonexistent/repo"));
        assert!(branches.is_empty());
    }

    // ---------------------------------------------------------------
    // Additional edge-case tests
    // ---------------------------------------------------------------

    #[test]
    fn scan_workspace_ignores_non_repo_subdirs() {
        let tmp = tmp_dir();

        // One real repo
        init_repo_with_commit("real-repo", &tmp);

        // A few plain directories
        fs::create_dir_all(tmp.join("plain-dir")).unwrap();
        fs::create_dir_all(tmp.join("another-dir")).unwrap();

        // A directory with a .git FILE (not dir) -- should not be detected
        let fake_git = tmp.join("fake-repo");
        fs::create_dir_all(&fake_git).unwrap();
        fs::write(fake_git.join(".git"), "gitdir: /somewhere/else").unwrap();

        let repos = scan_workspace(&tmp, &[]);
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "real-repo");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_repo_dirty_count_reflects_changes() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("dirty-count", &tmp);

        // Clean state
        let status = scan_repo(&repo_path).unwrap();
        assert_eq!(status.dirty, 0);

        // Add untracked files
        fs::write(repo_path.join("a.txt"), "a").unwrap();
        fs::write(repo_path.join("b.txt"), "b").unwrap();

        let status = scan_repo(&repo_path).unwrap();
        assert_eq!(status.dirty, 2, "Should count 2 dirty files");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_workspace_hidden_filter_multiple_entries() {
        let tmp = tmp_dir();

        init_repo_with_commit("keep", &tmp);
        init_repo_with_commit("hide-a", &tmp);
        init_repo_with_commit("hide-b", &tmp);

        let hidden = vec!["hide-a".to_string(), "hide-b".to_string()];
        let repos = scan_workspace(&tmp, &hidden);

        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "keep");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // stash_count via reflog
    // ---------------------------------------------------------------
    #[test]
    fn stash_count_on_clean_repo_is_zero() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("stash-count", &tmp);
        let status = scan_repo(&repo_path).unwrap();
        assert_eq!(status.stash, 0);
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // detect_default_branch tests
    // ---------------------------------------------------------------
    #[test]
    fn detect_default_branch_falls_back_to_main() {
        let tmp = tmp_dir();
        let repo_path = tmp.join("main-branch-repo");
        fs::create_dir_all(&repo_path).unwrap();

        // Init the repo -- default branch will be whatever git default is
        let repo = Repository::init(&repo_path).unwrap();
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();

        // Create initial commit on a branch called "main"
        let sig = repo.signature().unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[]).unwrap();

        // Rename the current branch to "main" to ensure it exists
        let head = repo.head().unwrap();
        let current_name = head.shorthand().unwrap_or("").to_string();
        if current_name != "main" {
            let mut branch = repo.find_branch(&current_name, git2::BranchType::Local).unwrap();
            branch.rename("main", true).unwrap();
        }

        let detected = detect_default_branch(&repo);
        assert_eq!(detected, "main", "Should detect 'main' as default branch");
        let _ = fs::remove_dir_all(&tmp);
    }

    // ---------------------------------------------------------------
    // scan_repo includes default_branch
    // ---------------------------------------------------------------
    #[test]
    fn scan_repo_includes_default_branch() {
        let tmp = tmp_dir();
        let repo_path = init_repo_with_commit("default-branch-scan", &tmp);

        let status = scan_repo(&repo_path).expect("scan_repo should return Some");
        assert!(
            !status.default_branch.is_empty(),
            "default_branch should be populated"
        );
        // It should be one of the standard fallback names since there's no remote
        let valid = ["main", "master", "develop"];
        assert!(
            valid.contains(&status.default_branch.as_str()),
            "default_branch '{}' should be one of {:?}",
            status.default_branch,
            valid
        );
        let _ = fs::remove_dir_all(&tmp);
    }
}
