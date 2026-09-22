// Backend failures for the agent lifecycle. The command edge renders these
// as strings for the frontend.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum AgentError {
    #[error("opencode binary not found. Install it with: npm install -g opencode-ai")]
    MissingBinary,
    #[error("agent exited: {raw}")]
    AgentExited { raw: String },
    #[error("request failed: {raw}")]
    RequestFailed { raw: String },
}

/// One selectable value inside a generic config option.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigValueView {
    pub value: String,
    pub name: String,
}

/// Generic agent config option.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigOptionView {
    pub id: String,
    pub name: String,
    pub current: String,
    pub options: Vec<ConfigValueView>,
}

/// Session info returned after opening a repo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: String,
    pub repo_root: String,
    pub branch: String,
    pub config_options: Vec<ConfigOptionView>,
}

/// Recent repo row for the picker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentRepo {
    pub path: String,
    pub branch: String,
}

/// Local preferences persisted under the OS app-data directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Prefs {
    #[serde(default)]
    pub recent: Vec<RecentRepo>,
    #[serde(default)]
    pub last_repo: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binary_names_opencode() {
        assert!(AgentError::MissingBinary.to_string().contains("opencode"));
    }
}
