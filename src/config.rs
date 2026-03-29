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
            .unwrap_or_else(|| PathBuf::from("~/.config"))
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

impl Default for Config {
    fn default() -> Self {
        Self {
            workspaces: BTreeMap::new(),
            defaults: Defaults::default(),
        }
    }
}

pub fn expand_path(path: &str) -> String {
    shellexpand(path)
}

fn shellexpand(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    path.to_string()
}
