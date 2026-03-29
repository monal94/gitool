use crate::types::{BranchEntry, RepoStatus};
use git2::{BranchType, Repository, StatusOptions};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn scan_workspace(path: &Path, hidden: &[String]) -> Vec<RepoStatus> {
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
    let stash = stash_count(path);
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
    })
}

fn current_branch(repo: &Repository) -> String {
    repo.head()
        .ok()
        .and_then(|h| h.shorthand().map(String::from))
        .unwrap_or_else(|| "HEAD".to_string())
}

fn upstream_drift(repo: &Repository) -> (usize, usize) {
    let head = match repo.head().ok().and_then(|h| h.target()) {
        Some(oid) => oid,
        None => return (0, 0),
    };

    let upstream = match repo
        .head()
        .ok()
        .and_then(|h| {
            let branch_name = h.shorthand()?.to_string();
            repo.find_branch(&branch_name, BranchType::Local).ok()
        })
        .and_then(|b| b.upstream().ok())
        .and_then(|u| u.get().target())
    {
        Some(oid) => oid,
        None => return (0, 0),
    };

    repo.graph_ahead_behind(head, upstream)
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

fn stash_count(path: &Path) -> usize {
    Command::new("git")
        .args(["stash", "list"])
        .current_dir(path)
        .output()
        .ok()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .count()
        })
        .unwrap_or(0)
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
                if head_ref.as_deref() == Some(&name) {
                    if let Some(entry) = map.get_mut(&name) {
                        entry.is_head_ref = true;
                    }
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
    run_git(path, &["checkout", branch])
}

pub fn git_stash(path: &Path) -> Result<String, String> {
    run_git(path, &["stash"])
}

pub fn git_stash_pop(path: &Path) -> Result<String, String> {
    run_git(path, &["stash", "pop"])
}

pub fn git_diff(path: &Path) -> Result<String, String> {
    run_git(path, &["diff"])
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
