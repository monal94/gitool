use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    pub path: String,
    #[serde(default)]
    pub hidden_repos: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_workspace")]
    pub workspace: String,
}

fn default_workspace() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct Config {
    pub workspaces: BTreeMap<String, WorkspaceConfig>,
    #[serde(default)]
    pub defaults: Defaults,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            workspace: default_workspace(),
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .unwrap_or_else(|| PathBuf::from(".config"))
            .join("ws")
            .join("config.yaml")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            let content = fs::read_to_string(&path).unwrap_or_default();
            serde_yaml::from_str(&content).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = serde_yaml::to_string(self)?;
        fs::write(&path, content)?;
        Ok(())
    }

    /// Ensure a workspace entry exists for the given path. Returns the workspace name.
    pub fn ensure_workspace(&mut self, workspace_path: &Path) -> String {
        // Check if any existing workspace matches this path
        let path_str = workspace_path.to_string_lossy().to_string();
        for (name, ws) in &self.workspaces {
            let ws_expanded = shellexpand(&ws.path);
            if ws_expanded == path_str {
                return name.clone();
            }
        }

        // Create new entry from directory name
        let name = workspace_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string());

        self.workspaces.insert(
            name.clone(),
            WorkspaceConfig {
                path: path_str,
                hidden_repos: Vec::new(),
            },
        );
        name
    }

    pub fn hidden_repos(&self, workspace_name: &str) -> Vec<String> {
        self.workspaces
            .get(workspace_name)
            .map(|ws| ws.hidden_repos.clone())
            .unwrap_or_default()
    }

    pub fn toggle_hidden(&mut self, workspace_name: &str, repo_name: &str) {
        if let Some(ws) = self.workspaces.get_mut(workspace_name) {
            if let Some(pos) = ws.hidden_repos.iter().position(|r| r == repo_name) {
                ws.hidden_repos.remove(pos);
            } else {
                ws.hidden_repos.push(repo_name.to_string());
            }
        }
    }
}


pub fn expand_path(path: &str) -> String {
    shellexpand(path)
}

fn shellexpand(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Helper: create a unique temporary directory for a test.
    /// Returns the path; caller is responsible for cleanup.
    fn temp_dir_for_test(label: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("gitool_tests")
            .join(format!("{}_{}", label, std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    // ---------------------------------------------------------------
    // 1. Config default values
    // ---------------------------------------------------------------
    #[test]
    fn test_config_default_values() {
        let cfg = Config::default();
        assert!(cfg.workspaces.is_empty(), "default config should have no workspaces");
        assert_eq!(
            cfg.defaults.workspace, "default",
            "default workspace name should be \"default\""
        );
    }

    #[test]
    fn test_defaults_struct_default() {
        let d = Defaults::default();
        assert_eq!(d.workspace, "default");
    }

    // ---------------------------------------------------------------
    // 2. Config load from non-existent path (should return default)
    // ---------------------------------------------------------------
    #[test]
    fn test_load_from_nonexistent_path_returns_default() {
        // Build a path that almost certainly does not exist.
        let bogus = std::env::temp_dir()
            .join("gitool_tests_nonexistent_dir_42")
            .join("config.yaml");
        assert!(!bogus.exists());

        // Simulate what `Config::load` does for a missing file.
        let cfg: Config = if bogus.exists() {
            let content = fs::read_to_string(&bogus).unwrap_or_default();
            serde_yaml::from_str(&content).unwrap_or_default()
        } else {
            Config::default()
        };

        assert!(cfg.workspaces.is_empty());
        assert_eq!(cfg.defaults.workspace, "default");
    }

    // ---------------------------------------------------------------
    // 3. Config save and load roundtrip using a temp dir
    // ---------------------------------------------------------------
    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = temp_dir_for_test("roundtrip");
        let config_path = dir.join("config.yaml");

        // Build a non-trivial config.
        let mut cfg = Config::default();
        cfg.defaults.workspace = "myws".to_string();
        cfg.workspaces.insert(
            "myws".to_string(),
            WorkspaceConfig {
                path: "/home/user/projects".to_string(),
                hidden_repos: vec!["secret-repo".to_string()],
            },
        );

        // Serialize and write.
        let yaml = serde_yaml::to_string(&cfg).expect("serialize");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&config_path, &yaml).expect("write");

        // Read back and deserialize.
        let content = fs::read_to_string(&config_path).expect("read");
        let loaded: Config = serde_yaml::from_str(&content).expect("deserialize");

        assert_eq!(loaded.defaults.workspace, "myws");
        assert_eq!(loaded.workspaces.len(), 1);
        let ws = loaded.workspaces.get("myws").expect("workspace present");
        assert_eq!(ws.path, "/home/user/projects");
        assert_eq!(ws.hidden_repos, vec!["secret-repo".to_string()]);

        // cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    // ---------------------------------------------------------------
    // 4. ensure_workspace – new workspace added correctly
    // ---------------------------------------------------------------
    #[test]
    fn test_ensure_workspace_adds_new() {
        let mut cfg = Config::default();
        let path = PathBuf::from("/tmp/my_projects");
        let name = cfg.ensure_workspace(&path);

        assert_eq!(name, "my_projects");
        assert!(cfg.workspaces.contains_key("my_projects"));
        let ws = cfg.workspaces.get("my_projects").unwrap();
        assert_eq!(ws.path, "/tmp/my_projects");
        assert!(ws.hidden_repos.is_empty());
    }

    #[test]
    fn test_ensure_workspace_uses_directory_basename() {
        let mut cfg = Config::default();
        let path = PathBuf::from("/a/b/c/deep_workspace");
        let name = cfg.ensure_workspace(&path);
        assert_eq!(name, "deep_workspace");
    }

    #[test]
    fn test_ensure_workspace_falls_back_to_default_name() {
        // A path with no file_name component (root "/").
        let mut cfg = Config::default();
        let path = PathBuf::from("/");
        let name = cfg.ensure_workspace(&path);
        // file_name() on "/" returns None, so we expect "default".
        assert_eq!(name, "default");
    }

    // ---------------------------------------------------------------
    // 5. ensure_workspace – existing workspace found by path
    // ---------------------------------------------------------------
    #[test]
    fn test_ensure_workspace_finds_existing_by_path() {
        let mut cfg = Config::default();
        cfg.workspaces.insert(
            "existing".to_string(),
            WorkspaceConfig {
                path: "/tmp/existing_workspace".to_string(),
                hidden_repos: vec!["hidden1".to_string()],
            },
        );

        // Calling ensure_workspace with the same path should return the existing name.
        let path = PathBuf::from("/tmp/existing_workspace");
        let name = cfg.ensure_workspace(&path);
        assert_eq!(name, "existing");
        // Should NOT have created a second entry.
        assert_eq!(cfg.workspaces.len(), 1);
    }

    #[test]
    fn test_ensure_workspace_does_not_duplicate() {
        let mut cfg = Config::default();
        let path = PathBuf::from("/tmp/ws");

        let name1 = cfg.ensure_workspace(&path);
        let name2 = cfg.ensure_workspace(&path);

        assert_eq!(name1, name2);
        assert_eq!(cfg.workspaces.len(), 1);
    }

    // ---------------------------------------------------------------
    // 6. hidden_repos – returns correct hidden list
    // ---------------------------------------------------------------
    #[test]
    fn test_hidden_repos_returns_list() {
        let mut cfg = Config::default();
        cfg.workspaces.insert(
            "ws".to_string(),
            WorkspaceConfig {
                path: "/tmp/ws".to_string(),
                hidden_repos: vec!["a".to_string(), "b".to_string(), "c".to_string()],
            },
        );

        let hidden = cfg.hidden_repos("ws");
        assert_eq!(hidden, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_hidden_repos_unknown_workspace_returns_empty() {
        let cfg = Config::default();
        let hidden = cfg.hidden_repos("nonexistent");
        assert!(hidden.is_empty());
    }

    #[test]
    fn test_hidden_repos_empty_when_none_hidden() {
        let mut cfg = Config::default();
        cfg.workspaces.insert(
            "ws".to_string(),
            WorkspaceConfig {
                path: "/tmp/ws".to_string(),
                hidden_repos: Vec::new(),
            },
        );

        assert!(cfg.hidden_repos("ws").is_empty());
    }

    // ---------------------------------------------------------------
    // 7. toggle_hidden – adds and removes repos
    // ---------------------------------------------------------------
    #[test]
    fn test_toggle_hidden_adds_repo() {
        let mut cfg = Config::default();
        cfg.workspaces.insert(
            "ws".to_string(),
            WorkspaceConfig {
                path: "/tmp/ws".to_string(),
                hidden_repos: Vec::new(),
            },
        );

        cfg.toggle_hidden("ws", "repo_x");
        assert_eq!(cfg.hidden_repos("ws"), vec!["repo_x"]);
    }

    #[test]
    fn test_toggle_hidden_removes_repo() {
        let mut cfg = Config::default();
        cfg.workspaces.insert(
            "ws".to_string(),
            WorkspaceConfig {
                path: "/tmp/ws".to_string(),
                hidden_repos: vec!["repo_x".to_string()],
            },
        );

        cfg.toggle_hidden("ws", "repo_x");
        assert!(cfg.hidden_repos("ws").is_empty());
    }

    #[test]
    fn test_toggle_hidden_roundtrip() {
        let mut cfg = Config::default();
        cfg.workspaces.insert(
            "ws".to_string(),
            WorkspaceConfig {
                path: "/tmp/ws".to_string(),
                hidden_repos: Vec::new(),
            },
        );

        // Add
        cfg.toggle_hidden("ws", "repo_a");
        assert_eq!(cfg.hidden_repos("ws"), vec!["repo_a"]);

        // Remove
        cfg.toggle_hidden("ws", "repo_a");
        assert!(cfg.hidden_repos("ws").is_empty());

        // Add again
        cfg.toggle_hidden("ws", "repo_a");
        assert_eq!(cfg.hidden_repos("ws"), vec!["repo_a"]);
    }

    #[test]
    fn test_toggle_hidden_multiple_repos() {
        let mut cfg = Config::default();
        cfg.workspaces.insert(
            "ws".to_string(),
            WorkspaceConfig {
                path: "/tmp/ws".to_string(),
                hidden_repos: Vec::new(),
            },
        );

        cfg.toggle_hidden("ws", "alpha");
        cfg.toggle_hidden("ws", "beta");
        cfg.toggle_hidden("ws", "gamma");

        let hidden = cfg.hidden_repos("ws");
        assert_eq!(hidden.len(), 3);
        assert!(hidden.contains(&"alpha".to_string()));
        assert!(hidden.contains(&"beta".to_string()));
        assert!(hidden.contains(&"gamma".to_string()));

        // Remove the middle one.
        cfg.toggle_hidden("ws", "beta");
        let hidden = cfg.hidden_repos("ws");
        assert_eq!(hidden.len(), 2);
        assert!(!hidden.contains(&"beta".to_string()));
    }

    #[test]
    fn test_toggle_hidden_nonexistent_workspace_is_noop() {
        let mut cfg = Config::default();
        // Should not panic.
        cfg.toggle_hidden("does_not_exist", "repo");
        assert!(cfg.workspaces.is_empty());
    }

    // ---------------------------------------------------------------
    // 8. expand_path / shellexpand – tilde expansion
    // ---------------------------------------------------------------
    #[test]
    fn test_expand_path_tilde() {
        let expanded = expand_path("~/projects");
        if let Some(home) = dirs::home_dir() {
            let expected = home.join("projects").to_string_lossy().to_string();
            assert_eq!(expanded, expected);
        } else {
            // If no home dir is available, the path should be returned as-is.
            assert_eq!(expanded, "~/projects");
        }
    }

    #[test]
    fn test_expand_path_no_tilde() {
        let input = "/absolute/path/to/dir";
        assert_eq!(expand_path(input), input);
    }

    #[test]
    fn test_expand_path_only_tilde_slash() {
        // "~/" should expand to the home directory itself (with trailing component empty).
        let expanded = expand_path("~/");
        if let Some(home) = dirs::home_dir() {
            let expected = home.join("").to_string_lossy().to_string();
            assert_eq!(expanded, expected);
        }
    }

    #[test]
    fn test_shellexpand_preserves_non_tilde_path() {
        assert_eq!(shellexpand("relative/path"), "relative/path");
        assert_eq!(shellexpand("/absolute/path"), "/absolute/path");
        assert_eq!(shellexpand(""), "");
    }

    #[test]
    fn test_shellexpand_tilde_without_slash_is_not_expanded() {
        // "~user" style is not supported; only "~/" prefix triggers expansion.
        let result = shellexpand("~user/something");
        assert_eq!(result, "~user/something");
    }

    #[test]
    fn test_shellexpand_deep_path() {
        let expanded = shellexpand("~/a/b/c/d");
        if let Some(home) = dirs::home_dir() {
            let expected = home.join("a/b/c/d").to_string_lossy().to_string();
            assert_eq!(expanded, expected);
        }
    }

    // ---------------------------------------------------------------
    // 9. Config serialization / deserialization with serde_yaml
    // ---------------------------------------------------------------
    #[test]
    fn test_serialize_default_config() {
        let cfg = Config::default();
        let yaml = serde_yaml::to_string(&cfg).expect("serialize default config");
        assert!(yaml.contains("workspaces"));
        assert!(yaml.contains("defaults"));
    }

    #[test]
    fn test_deserialize_minimal_yaml() {
        let yaml = r#"
workspaces: {}
defaults:
  workspace: minimal
"#;
        let cfg: Config = serde_yaml::from_str(yaml).expect("deserialize minimal yaml");
        assert!(cfg.workspaces.is_empty());
        assert_eq!(cfg.defaults.workspace, "minimal");
    }

    #[test]
    fn test_deserialize_with_workspaces() {
        let yaml = r#"
workspaces:
  project_a:
    path: /home/user/project_a
    hidden_repos:
      - secret
      - internal
  project_b:
    path: /home/user/project_b
defaults:
  workspace: project_a
"#;
        let cfg: Config = serde_yaml::from_str(yaml).expect("deserialize");
        assert_eq!(cfg.workspaces.len(), 2);

        let a = cfg.workspaces.get("project_a").unwrap();
        assert_eq!(a.path, "/home/user/project_a");
        assert_eq!(a.hidden_repos, vec!["secret", "internal"]);

        let b = cfg.workspaces.get("project_b").unwrap();
        assert_eq!(b.path, "/home/user/project_b");
        assert!(b.hidden_repos.is_empty());

        assert_eq!(cfg.defaults.workspace, "project_a");
    }

    #[test]
    fn test_deserialize_missing_defaults_uses_default() {
        // "defaults" field is missing; #[serde(default)] should kick in.
        let yaml = r#"
workspaces: {}
"#;
        let cfg: Config = serde_yaml::from_str(yaml).expect("deserialize");
        assert_eq!(cfg.defaults.workspace, "default");
    }

    #[test]
    fn test_deserialize_missing_hidden_repos_defaults_to_empty() {
        let yaml = r#"
workspaces:
  ws:
    path: /tmp/ws
defaults:
  workspace: ws
"#;
        let cfg: Config = serde_yaml::from_str(yaml).expect("deserialize");
        let ws = cfg.workspaces.get("ws").unwrap();
        assert!(ws.hidden_repos.is_empty());
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let mut original = Config::default();
        original.defaults.workspace = "work".to_string();
        original.workspaces.insert(
            "work".to_string(),
            WorkspaceConfig {
                path: "/home/dev/work".to_string(),
                hidden_repos: vec!["private".to_string(), "archive".to_string()],
            },
        );
        original.workspaces.insert(
            "oss".to_string(),
            WorkspaceConfig {
                path: "/home/dev/oss".to_string(),
                hidden_repos: Vec::new(),
            },
        );

        let yaml = serde_yaml::to_string(&original).expect("serialize");
        let restored: Config = serde_yaml::from_str(&yaml).expect("deserialize");

        assert_eq!(restored.defaults.workspace, original.defaults.workspace);
        assert_eq!(restored.workspaces.len(), original.workspaces.len());

        for (name, orig_ws) in &original.workspaces {
            let rest_ws = restored.workspaces.get(name).expect("workspace present");
            assert_eq!(rest_ws.path, orig_ws.path);
            assert_eq!(rest_ws.hidden_repos, orig_ws.hidden_repos);
        }
    }

    #[test]
    fn test_workspace_config_serde_roundtrip() {
        let ws = WorkspaceConfig {
            path: "~/repos".to_string(),
            hidden_repos: vec!["x".to_string(), "y".to_string()],
        };
        let yaml = serde_yaml::to_string(&ws).expect("serialize");
        let restored: WorkspaceConfig = serde_yaml::from_str(&yaml).expect("deserialize");
        assert_eq!(restored.path, ws.path);
        assert_eq!(restored.hidden_repos, ws.hidden_repos);
    }

    #[test]
    fn test_config_path_is_under_ws_directory() {
        let path = Config::config_path();
        // The path should end with ws/config.yaml regardless of platform.
        let path_str = path.to_string_lossy();
        assert!(
            path_str.ends_with("ws/config.yaml") || path_str.ends_with("ws\\config.yaml"),
            "config_path should end with ws/config.yaml, got: {path_str}"
        );
    }
}
