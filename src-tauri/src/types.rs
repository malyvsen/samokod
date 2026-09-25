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

/// Role of one session inside a plan. A plan owns one scoping session,
/// plus one executing session once approved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    Scoping,
    Executing,
}

/// Key of one live session: plan directory name plus role.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionKey {
    pub plan: String,
    pub role: SessionRole,
}

/// Per-session status for the plans list. The frontend derives the dot:
///
/// failed red, approval amber, working green, idle active blue, the rest
/// gray.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionStatusView {
    pub role: SessionRole,
    pub working: bool,
    pub approval: bool,
    pub failed: bool,
    pub live: bool,
}

/// One plan row for the plans list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanEntry {
    pub name: String,
    pub phase: crate::plans::Phase,
    pub title: String,
    pub sessions: Vec<SessionStatusView>,
}

/// Plans list pushed after every transition, activity, or title change,
/// and returned by plan commands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlansUpdate {
    pub plans: Vec<PlanEntry>,
    pub selected: SessionKey,
}

/// Repository payload returned after opening a repo. No agents spawn here;
/// every session starts lazily on its first prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OpenRepoResult {
    pub repo_root: String,
    pub branch: String,
    pub plans: Vec<PlanEntry>,
    pub selected: SessionKey,
}

/// Frontend event envelope. One Tauri event name carries every variant so
/// capabilities stay tight. Every session-specific variant carries its
/// session key; `branch_changed` stays global.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    AgentText {
        session: SessionKey,
        chunk: String,
    },
    ToolLine {
        session: SessionKey,
        line: ToolLineView,
    },
    TurnDone {
        session: SessionKey,
    },
    TurnFailed {
        session: SessionKey,
        raw: String,
        hint: String,
        retryable: bool,
    },
    AgentExited {
        session: SessionKey,
        raw: String,
        hint: String,
        retryable: bool,
    },
    PermissionAsked {
        session: SessionKey,
        permission: PermissionView,
    },
    PermissionResolved {
        session: SessionKey,
        tool_call_id: String,
    },
    ConfigOptions {
        session: SessionKey,
        options: Vec<ConfigOptionView>,
    },
    TodosChanged {
        session: SessionKey,
        todos: Vec<TodoView>,
        changes: Vec<TodoChangeView>,
    },
    SpendTick {
        session: SessionKey,
        cost: f64,
        ctx_pct: f64,
    },
    PlanChanged {
        session: SessionKey,
        plan: PlanInfo,
    },
    BranchChanged {
        branch: String,
    },
    SessionReset {
        session: SessionKey,
    },
    PlansChanged {
        plans: Vec<PlanEntry>,
        selected: SessionKey,
    },
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

/// Plan state for one session. Mirrors `plans::Phase`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanInfo {
    pub name: String,
    pub phase: crate::plans::Phase,
    pub has_plan_md: bool,
    /// First markdown heading of `plan.md`, `Untitled` without one.
    #[serde(default = "untitled")]
    pub title: String,
}

fn untitled() -> String {
    "Untitled".to_string()
}

impl PlanInfo {
    /// Frontend plan payload with fresh `plan.md` presence and title. Pure
    /// except the existence check and the title read.
    pub fn of(repo_root: &Path, plan: &PlanRef) -> Self {
        PlanInfo {
            name: plan.name.clone(),
            phase: plan.phase,
            has_plan_md: plan.has_plan_md(repo_root),
            title: crate::plans::plan_title(repo_root, plan),
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
