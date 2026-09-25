// Shared backend views: failures, events, and session payloads crossing the
// command edge as JSON. The command edge renders errors as strings.
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::plans::{PlanError, PlanRef};

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
    #[error("plan failed: {raw}")]
    Plan { raw: String },
}

impl From<PlanError> for AgentError {
    fn from(error: PlanError) -> Self {
        AgentError::Plan {
            raw: error.to_string(),
        }
    }
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
    PlanChanged {
        plan: PlanInfo,
    },
    BranchChanged {
        branch: String,
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
    /// The chat's plan. Every chat owns exactly one.
    pub plan: PlanInfo,
}

/// Plan state for the topbar dropdown. Mirrors `plans::Phase`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanInfo {
    pub name: String,
    pub phase: crate::plans::Phase,
    pub has_plan_md: bool,
}

impl PlanInfo {
    /// Frontend plan payload with fresh `plan.md` presence. Pure except the
    /// existence check.
    pub fn of(repo_root: &Path, plan: &PlanRef) -> Self {
        PlanInfo {
            name: plan.name.clone(),
            phase: plan.phase,
            has_plan_md: plan.has_plan_md(repo_root),
        }
    }
}

/// Recent repo row for the picker: path only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentRepo {
    pub path: String,
}

/// Local state persisted under the OS app-data directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Prefs {
    #[serde(default)]
    pub recent: Vec<RecentRepo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binary_names_opencode() {
        assert!(AgentError::MissingBinary.to_string().contains("opencode"));
    }
}
