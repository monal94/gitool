mod scan;
mod status;
mod diff;
mod ops;
mod log;

pub use scan::*;
pub use status::*;
pub use diff::*;
pub use ops::*;
pub use log::*;

#[cfg(test)]
pub(crate) mod tests {
    use git2::Repository;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Create a unique temporary directory for each test to avoid collisions.
    pub fn tmp_dir() -> PathBuf {
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
    pub fn init_repo_with_commit(name: &str, parent: &Path) -> PathBuf {
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
}
