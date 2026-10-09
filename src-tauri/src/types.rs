// Shared backend views: failures, events, and session payloads crossing the
// command edge as JSON. The command edge renders errors as strings.
use serde::{Deserialize, Serialize};

use crate::plans::PlanError;

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

impl From<crate::worktrees::WorktreeError> for AgentError {
    fn from(error: crate::worktrees::WorktreeError) -> Self {
        AgentError::RequestFailed {
            raw: error.to_string(),
        }
    }
}

/// Lifecycle status for one tool line. Ongoing rows append ` …` in text;
/// finished rows render the bare `{label}: {body}` line.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// Fallback label when the wire tool name is absent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolKindLabel {
    Read,
    Edit,
    Delete,
    Move,
    Search,
    #[serde(rename = "bash")]
    Execute,
    Think,
    Fetch,
    #[serde(rename = "mode")]
    SwitchMode,
    #[serde(rename = "tool")]
    Other,
}

impl ToolKindLabel {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolKindLabel::Read => "read",
            ToolKindLabel::Edit => "edit",
            ToolKindLabel::Delete => "delete",
            ToolKindLabel::Move => "move",
            ToolKindLabel::Search => "search",
            ToolKindLabel::Execute => "bash",
            ToolKindLabel::Think => "think",
            ToolKindLabel::Fetch => "fetch",
            ToolKindLabel::SwitchMode => "mode",
            ToolKindLabel::Other => "tool",
        }
    }
}

impl ToolStatus {
    pub fn is_ongoing(&self) -> bool {
        match self {
            ToolStatus::Pending | ToolStatus::InProgress => true,
            ToolStatus::Completed | ToolStatus::Failed => false,
        }
    }
}

/// Status for one todo row.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// One `▸` tool status line shaped as `{label}: {body}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolLineView {
    pub id: String,
    pub text: String,
    pub status: ToolStatus,
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
    pub tool: String,
    pub options: Vec<PermissionOptionView>,
    /// Effective rule only, never the config file it came from.
    pub rule_hint: String,
}

/// Role of one session inside a plan. A plan owns one scoping session,
/// plus one executing session once approved, plus one evergreening session
/// on the cleanup path, plus one landing session on the conflict path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    Scoping,
    Executing,
    Evergreening,
    Landing,
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionStatusView {
    pub role: SessionRole,
    pub working: bool,
    pub approval: bool,
    pub failed: bool,
    pub live: bool,
    #[serde(default)]
    pub progress: Option<TodoProgressView>,
    #[serde(default)]
    pub spend: SpendStatusView,
    #[serde(default)]
    pub todos: Vec<TodoView>,
}

/// Spend totals for one session, mirrored from the latest `UsageUpdate`.
/// `ctx_pct` matches the `SpendTick` event field name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SpendStatusView {
    #[serde(default)]
    pub cost: f64,
    #[serde(default)]
    pub ctx_pct: f64,
}

/// Per-session todo progress for the plans list. `eta_secs` is `None`
/// when no ETA text shows (empty, estimating, or done).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoProgressView {
    pub done: usize,
    pub total: usize,
    #[serde(default)]
    pub eta_secs: Option<u64>,
}

/// One plan row for the plans list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanEntry {
    pub name: String,
    pub phase: crate::plans::Phase,
    pub title: String,
    pub has_plan_md: bool,
    pub sessions: Vec<SessionStatusView>,
    /// Manual mode: true holds the plan in place, false lets it advance on
    /// its own. The single copy lives in `PlanState`; this is its view.
    #[serde(default)]
    pub manual: bool,
}

/// Stored model/effort defaults for the instant disabled picker paint.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoDefaults {
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
}

/// Plans list pushed after every prompt, transition, or title change,
/// and returned by plan commands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlansUpdate {
    pub plans: Vec<PlanEntry>,
    pub selected: SessionKey,
    pub config_defaults: RepoDefaults,
}

/// Repository payload returned after opening a repo. No agents spawn here;
/// every session starts lazily on its first prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenRepoResult {
    pub repo_root: String,
    pub branch: String,
    pub plans: Vec<PlanEntry>,
    pub selected: SessionKey,
    pub config_defaults: RepoDefaults,
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
    /// Streamed thought tail. Transient only: the frontend keeps the tail
    /// in ephemeral state and stores only the frozen burst duration.
    AgentThought {
        session: SessionKey,
        chunk: String,
    },
    /// Replayed user message chunk. Only emitted while a session's history
    /// loads, plus the eager executing/landing first prompt which emits
    /// live; live scoping turns append the user bubble optimistically instead.
    UserText {
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
    BranchChanged {
        branch: String,
    },
    SessionReset {
        session: SessionKey,
    },
    /// History load started for one session. Emitted immediately after the
    /// history slot is claimed, before the slow session-ID probes: the
    /// frontend shows a neutral preparing state with no replay claim.
    HistoryPreparing {
        session: SessionKey,
    },
    /// History replay started for one session. Only emitted when past
    /// exists and the connection is ready, just before `session/load`.
    /// The transcript streams through the normal update events; `working`
    /// stays false throughout.
    HistoryBegin {
        session: SessionKey,
    },
    /// History replay finished. Configuration options arrive separately as
    /// `ConfigOptions` alongside this event.
    HistoryDone {
        session: SessionKey,
    },
    /// History replay failed. `hint` renders under the raw reason and
    /// `retryable` drives the transcript retry bar.
    HistoryFailed {
        session: SessionKey,
        raw: String,
        hint: String,
        retryable: bool,
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
    pub status: TodoStatus,
}

/// Changed rows since the previous list: added rows plus status changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoChangeView {
    pub content: String,
    pub status: TodoStatus,
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

    #[test]
    fn thought_event_serializes_as_snake_case() {
        let event = AppEvent::AgentThought {
            session: SessionKey {
                plan: "p".to_string(),
                role: SessionRole::Scoping,
            },
            chunk: "considering".to_string(),
        };
        let value = serde_json::to_value(&event).expect("serializes");
        assert_eq!(
            value.get("type").and_then(|v| v.as_str()),
            Some("agent_thought")
        );
        assert_eq!(
            value.get("chunk").and_then(|v| v.as_str()),
            Some("considering")
        );
    }
}
