use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Typechange,
    Conflicted,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub status: FileStatus,
    pub staged: bool,
}

#[derive(Debug, Clone)]
pub struct BranchEntry {
    pub name: String,
    pub is_current: bool,
    pub is_head_ref: bool,        // origin/HEAD points here
    pub has_local: bool,
    pub has_remote: bool,
    pub ahead_main: Option<usize>,
    pub behind_main: Option<usize>,
    pub ahead_remote: Option<usize>,  // local ahead of origin (only if both exist)
    pub behind_remote: Option<usize>, // local behind origin (only if both exist)
}

#[derive(Debug, Clone)]
pub struct RepoStatus {
    pub name: String,
    pub path: PathBuf,
    pub branch: String,
    pub ahead: usize,
    pub behind: usize,
    pub dirty: usize,
    pub stash: usize,
    pub branches: Vec<BranchEntry>,
    pub branches_loaded: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- FileStatus tests ---

    #[test]
    fn file_status_equality() {
        assert_eq!(FileStatus::Modified, FileStatus::Modified);
        assert_eq!(FileStatus::Added, FileStatus::Added);
        assert_eq!(FileStatus::Deleted, FileStatus::Deleted);
        assert_eq!(FileStatus::Renamed, FileStatus::Renamed);
        assert_eq!(FileStatus::Untracked, FileStatus::Untracked);
        assert_eq!(FileStatus::Typechange, FileStatus::Typechange);
        assert_eq!(FileStatus::Conflicted, FileStatus::Conflicted);
    }

    #[test]
    fn file_status_inequality() {
        assert_ne!(FileStatus::Modified, FileStatus::Added);
        assert_ne!(FileStatus::Deleted, FileStatus::Renamed);
        assert_ne!(FileStatus::Untracked, FileStatus::Conflicted);
        assert_ne!(FileStatus::Typechange, FileStatus::Modified);
    }

    #[test]
    fn file_status_clone() {
        let status = FileStatus::Modified;
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    #[test]
    fn file_status_all_variants_are_distinct() {
        let variants = vec![
            FileStatus::Modified,
            FileStatus::Added,
            FileStatus::Deleted,
            FileStatus::Renamed,
            FileStatus::Untracked,
            FileStatus::Typechange,
            FileStatus::Conflicted,
        ];
        for (i, a) in variants.iter().enumerate() {
            for (j, b) in variants.iter().enumerate() {
                if i == j {
                    assert_eq!(a, b);
                } else {
                    assert_ne!(a, b, "Variants at index {} and {} should differ", i, j);
                }
            }
        }
    }

    // --- FileEntry tests ---

    #[test]
    fn file_entry_construction_and_field_access() {
        let entry = FileEntry {
            path: "src/main.rs".to_string(),
            status: FileStatus::Modified,
            staged: false,
        };
        assert_eq!(entry.path, "src/main.rs");
        assert_eq!(entry.status, FileStatus::Modified);
        assert!(!entry.staged);
    }

    #[test]
    fn file_entry_staged_true() {
        let entry = FileEntry {
            path: "README.md".to_string(),
            status: FileStatus::Added,
            staged: true,
        };
        assert!(entry.staged);
        assert_eq!(entry.status, FileStatus::Added);
    }

    #[test]
    fn file_entry_clone() {
        let entry = FileEntry {
            path: "lib.rs".to_string(),
            status: FileStatus::Deleted,
            staged: false,
        };
        let cloned = entry.clone();
        assert_eq!(cloned.path, "lib.rs");
        assert_eq!(cloned.status, FileStatus::Deleted);
        assert!(!cloned.staged);
    }

    // --- BranchEntry tests ---

    #[test]
    fn branch_entry_construction_all_fields() {
        let branch = BranchEntry {
            name: "feature/login".to_string(),
            is_current: true,
            is_head_ref: false,
            has_local: true,
            has_remote: true,
            ahead_main: Some(3),
            behind_main: Some(1),
            ahead_remote: Some(2),
            behind_remote: None,
        };
        assert_eq!(branch.name, "feature/login");
        assert!(branch.is_current);
        assert!(!branch.is_head_ref);
        assert!(branch.has_local);
        assert!(branch.has_remote);
        assert_eq!(branch.ahead_main, Some(3));
        assert_eq!(branch.behind_main, Some(1));
        assert_eq!(branch.ahead_remote, Some(2));
        assert_eq!(branch.behind_remote, None);
    }

    #[test]
    fn branch_entry_remote_only() {
        let branch = BranchEntry {
            name: "origin/main".to_string(),
            is_current: false,
            is_head_ref: true,
            has_local: false,
            has_remote: true,
            ahead_main: None,
            behind_main: None,
            ahead_remote: None,
            behind_remote: None,
        };
        assert!(!branch.has_local);
        assert!(branch.has_remote);
        assert!(branch.is_head_ref);
        assert!(!branch.is_current);
    }

    #[test]
    fn branch_entry_clone() {
        let branch = BranchEntry {
            name: "develop".to_string(),
            is_current: false,
            is_head_ref: false,
            has_local: true,
            has_remote: false,
            ahead_main: Some(5),
            behind_main: Some(0),
            ahead_remote: None,
            behind_remote: None,
        };
        let cloned = branch.clone();
        assert_eq!(cloned.name, "develop");
        assert_eq!(cloned.ahead_main, Some(5));
        assert_eq!(cloned.behind_main, Some(0));
    }

    // --- RepoStatus tests ---

    #[test]
    fn repo_status_construction() {
        let repo = RepoStatus {
            name: "my-repo".to_string(),
            path: PathBuf::from("/home/user/repos/my-repo"),
            branch: "main".to_string(),
            ahead: 2,
            behind: 1,
            dirty: 3,
            stash: 0,
            branches: vec![],
            branches_loaded: false,
        };
        assert_eq!(repo.name, "my-repo");
        assert_eq!(repo.path, PathBuf::from("/home/user/repos/my-repo"));
        assert_eq!(repo.branch, "main");
        assert_eq!(repo.ahead, 2);
        assert_eq!(repo.behind, 1);
        assert_eq!(repo.dirty, 3);
        assert_eq!(repo.stash, 0);
        assert!(repo.branches.is_empty());
        assert!(!repo.branches_loaded);
    }

    #[test]
    fn repo_status_branches_loaded_default_false() {
        let repo = RepoStatus {
            name: "test".to_string(),
            path: PathBuf::from("/tmp/test"),
            branch: "main".to_string(),
            ahead: 0,
            behind: 0,
            dirty: 0,
            stash: 0,
            branches: vec![],
            branches_loaded: false,
        };
        assert!(!repo.branches_loaded);
    }

    #[test]
    fn repo_status_with_branches() {
        let branches = vec![
            BranchEntry {
                name: "main".to_string(),
                is_current: true,
                is_head_ref: true,
                has_local: true,
                has_remote: true,
                ahead_main: Some(0),
                behind_main: Some(0),
                ahead_remote: Some(0),
                behind_remote: Some(0),
            },
            BranchEntry {
                name: "feature".to_string(),
                is_current: false,
                is_head_ref: false,
                has_local: true,
                has_remote: false,
                ahead_main: Some(2),
                behind_main: None,
                ahead_remote: None,
                behind_remote: None,
            },
        ];
        let repo = RepoStatus {
            name: "multi-branch".to_string(),
            path: PathBuf::from("/repos/multi-branch"),
            branch: "main".to_string(),
            ahead: 0,
            behind: 0,
            dirty: 0,
            stash: 1,
            branches,
            branches_loaded: true,
        };
        assert_eq!(repo.branches.len(), 2);
        assert!(repo.branches_loaded);
        assert_eq!(repo.stash, 1);
        assert!(repo.branches[0].is_current);
        assert!(!repo.branches[1].is_current);
    }

    #[test]
    fn repo_status_clone() {
        let repo = RepoStatus {
            name: "clone-test".to_string(),
            path: PathBuf::from("/tmp/clone-test"),
            branch: "develop".to_string(),
            ahead: 1,
            behind: 2,
            dirty: 5,
            stash: 3,
            branches: vec![],
            branches_loaded: true,
        };
        let cloned = repo.clone();
        assert_eq!(cloned.name, "clone-test");
        assert_eq!(cloned.branch, "develop");
        assert_eq!(cloned.ahead, 1);
        assert_eq!(cloned.behind, 2);
        assert_eq!(cloned.dirty, 5);
        assert_eq!(cloned.stash, 3);
        assert!(cloned.branches_loaded);
    }
}
