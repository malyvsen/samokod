// Backend failures for the agent lifecycle. The command edge renders these
// as strings for the frontend.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum AgentError {
    #[error("opencode binary not found. Install it with: npm install -g opencode-ai")]
    MissingBinary,
    #[error("agent exited: {raw}")]
    AgentExited { raw: String },
    #[error("request failed: {raw}")]
    RequestFailed { raw: String },
    #[error("no session: {raw}")]
    NoSession { raw: String },
}

/// One `▸` tool status line regardless of tool kind.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolLineView {
    pub id: String,
    pub text: String,
    pub status: String,
}

/// One one-shot permission option (`allow` or `reject`). `always` variants
/// never reach the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionOptionView {
    pub id: String,
    pub kind: String,
}

/// Inline permission card payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionView {
    pub tool_call_id: String,
    pub title: String,
    pub kind: String,
    pub options: Vec<PermissionOptionView>,
    /// Effective rule only, never the config file it came from.
    pub rule_hint: String,
}

/// Frontend event envelope. One Tauri event name carries every variant so
/// capabilities stay tight.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    AgentText {
        chunk: String,
    },
    ToolLine {
        line: ToolLineView,
    },
    TurnDone,
    TurnFailed {
        raw: String,
        hint: String,
        retryable: bool,
    },
    AgentExited {
        raw: String,
        hint: String,
        retryable: bool,
    },
    PermissionAsked {
        permission: PermissionView,
    },
    PermissionResolved {
        tool_call_id: String,
    },
    ConfigOptions {
        options: Vec<ConfigOptionView>,
    },
    TodosChanged {
        todos: Vec<TodoView>,
        changes: Vec<TodoChangeView>,
    },
    SpendTick {
        cost: f64,
        ctx_pct: f64,
    },
    SessionReset,
}

/// One todo row mirrored from the agent's list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoView {
    pub content: String,
    pub status: String,
    pub priority: String,
}

/// Changed rows since the previous list: added rows plus status changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoChangeView {
    pub content: String,
    pub status: String,
}

/// One value inside a session config option (ACP `ConfigOptionValue`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigOptionValueView {
    pub value: String,
    pub name: String,
}

/// Session config option (ACP `ConfigOption`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOptionView {
    pub id: String,
    pub name: String,
    pub current_value: String,
    pub options: Vec<ConfigOptionValueView>,
    /// Spec `category`, absent when the agent omits it.
    #[serde(default)]
    pub category: Option<String>,
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
    #[serde(default)]
    pub models: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binary_names_opencode() {
        assert!(AgentError::MissingBinary.to_string().contains("opencode"));
    }
}
