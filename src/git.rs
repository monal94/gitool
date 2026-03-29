use crate::types::{BranchEntry, FileEntry, FileStatus, RepoStatus};
use git2::{BranchType, Repository, StatusOptions, Status};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

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
    })
}

/// Full scan including branches — single Repository::open call.
pub fn scan_repo_full(path: &Path) -> Option<RepoStatus> {
    let repo = Repository::open(path).ok()?;
    let name = path.file_name()?.to_string_lossy().to_string();

    let branch = current_branch(&repo);
    let (ahead, behind) = upstream_drift(&repo);
    let dirty = dirty_count(&repo);
    let stash = stash_count(&repo);
    let branches = collect_branches(&repo, &branch);

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
    })
}

/// Load branches and drift for a single repo (called on-demand).
pub fn load_branches(path: &Path) -> Vec<BranchEntry> {
    let Some(repo) = Repository::open(path).ok() else {
        return Vec::new();
    };
    load_branches_with_repo(&repo)
}

/// Load branches using an already-opened Repository handle.
pub fn load_branches_with_repo(repo: &Repository) -> Vec<BranchEntry> {
    let branch = current_branch(repo);
    collect_branches(repo, &branch)
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

fn stash_count(repo: &Repository) -> usize {
    repo.reflog("refs/stash").map(|r| r.len()).unwrap_or(0)
}

/// Collect a unified branch list merging local and remote refs by name.
fn collect_branches(repo: &Repository, current: &str) -> Vec<BranchEntry> {
    let main_oid = repo
        .find_branch("main", BranchType::Local)
        .ok()
        .and_then(|b| b.get().target());

    let origin_main_oid = repo
        .find_branch("origin/main", BranchType::Remote)
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

    // Collect remote branches — only add those not already in map (remote-only)
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
                // Already added from local — just ensure is_head_ref is set
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
        let a_main = if a.name == "main" { 0 } else { 1 };
        let b_main = if b.name == "main" { 0 } else { 1 };
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
            // No HEAD (initial commit) — remove from index
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

/// Convert a git2::Diff to a patch string.
fn diff_to_string(diff: &git2::Diff) -> Result<String, String> {
    let mut output = String::new();
    diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        match line.origin() {
            '+' | '-' | ' ' => output.push(line.origin()),
            _ => {}
        }
        output.push_str(&String::from_utf8_lossy(line.content()));
        true
    }).map_err(|e| e.to_string())?;
    Ok(output)
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
    // git2 merge is complex (conflict resolution) — keep CLI for this
    run_git(path, &["merge", "--", branch])
}

// Git mutation operations — shell out to git CLI

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

pub fn git_stash_pop(path: &Path) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let mut repo = repo;
    repo.stash_pop(0, None).map_err(|e| e.to_string())?;
    Ok("Stash popped".to_string())
}

pub fn git_diff(path: &Path) -> Result<String, String> {
    let repo = Repository::open(path).map_err(|e| e.to_string())?;
    let diff = repo.diff_index_to_workdir(None, None)
        .map_err(|e| e.to_string())?;
    diff_to_string(&diff)
}

fn run_git(path: &Path, args: &[&str]) -> Result<String, String> {
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
    use crate::types::FileStatus;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Create a unique temporary directory for each test to avoid collisions.
    fn tmp_dir() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir()
            .join(format!("gitool_test_{}_{}", std::process::id(), id));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Helper: create a git repo inside `parent/name` with user config and
    /// one initial commit so that HEAD exists.
    fn init_repo_with_commit(name: &str, parent: &Path) -> PathBuf {
        let repo_path = parent.join(name);
        fs::create_dir_all(&repo_path).unwrap();
        let repo = Repository::init(&repo_path).unwrap();

        // Configure user so commits work
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test User").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();

        // Create an initial commit (empty tree)
        let sig = repo.signature().unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();

        repo_path
    }

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
}
