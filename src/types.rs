use std::path::PathBuf;

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
