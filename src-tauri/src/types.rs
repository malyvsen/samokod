// Shared backend views: failures, events, and session payloads crossing the
// command edge as JSON. The command edge renders errors as strings.
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::plans::PlanError;

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum AgentError {
    #[error("already open: {path}")]
    AlreadyOpen { path: String, window: String },
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    #[default]
    Pending,
    InProgress,
    Completed,
    Failed,
}

/// Fallback label when the wire tool name is absent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type)]
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Type)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// One `▸` tool status line shaped as `{label}: {body}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct ToolLineView {
    pub id: String,
    pub text: String,
    pub status: ToolStatus,
    pub label: String,
}

/// One one-shot permission option (`allow` or `reject`). `always` variants
/// never reach the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct PermissionOptionView {
    pub id: String,
    pub kind: String,
}

/// Inline permission card payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
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
/// on the cleanup path, plus one rebasing session on the conflict path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SessionRole {
    Scoping,
    Executing,
    Evergreening,
    Rebasing,
}

impl SessionRole {
    /// True for the work roles that auto-advance on the pump. Scoping
    /// always waits for its manual approval click, so STOP and plan drops
    /// leave it untouched. Pure.
    pub fn is_work(self) -> bool {
        matches!(
            self,
            SessionRole::Executing | SessionRole::Evergreening | SessionRole::Rebasing
        )
    }
}

/// Key of one live session: plan directory name plus role.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct SessionKey {
    pub plan: String,
    pub role: SessionRole,
}

/// Per-session status for the plans list. The frontend derives the dot:
///
/// failed red, approval amber, working green, idle active blue, the rest
/// gray.
#[derive(Debug, Clone, Serialize, PartialEq, Type)]
pub struct SessionStatusView {
    pub role: SessionRole,
    pub working: bool,
    pub approval: bool,
    pub failed: bool,
    pub live: bool,
    pub progress: Option<ProgressView>,
    pub spend: SpendStatusView,
    pub todos: Vec<TodoView>,
}

/// Spend totals for one session, mirrored from the latest `UsageUpdate`.
/// `ctx_pct` matches the `SpendTick` event field name.
#[derive(Debug, Clone, Serialize, PartialEq, Default, Type)]
pub struct SpendStatusView {
    pub cost: f64,
    pub ctx_pct: f64,
}

/// Done/total progress with an optional ETA for the plans list.
/// `eta_secs` is `None` when no ETA text shows (empty, estimating, or
/// done). `elapsed_secs` is `None` when no start time is known. Shared by
/// per-session todo progress and rebasing commit progress.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Type)]
pub struct ProgressView {
    pub done: usize,
    pub total: usize,
    pub eta_secs: Option<u64>,
    /// Elapsed seconds since `started`.
    pub elapsed_secs: Option<u64>,
}

/// One plan row for the plans list.
#[derive(Debug, Clone, Serialize, PartialEq, Type)]
pub struct PlanEntry {
    pub name: String,
    pub phase: crate::plans::Phase,
    pub title: String,
    pub has_plan_md: bool,
    pub sessions: Vec<SessionStatusView>,
    /// Manual mode: true holds the plan in place, false lets it advance on
    /// its own. The single copy lives in `PlanState`; this is its view.
    pub manual: bool,
    /// Evergreen intent: true cleans up after rebasing, false rebases then
    /// finishes directly. The single copy lives in `PlanState`.
    pub evergreen: bool,
    /// Branch commits ahead of the target, oldest first. Empty when the
    /// plan owns no worktree.
    pub commits: Vec<String>,
    /// Rebasing commit progress with ETA. `Some` for rebasing plans only.
    pub rebasing_progress: Option<ProgressView>,
}

/// Effective model/effort defaults for the instant disabled picker paint.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq, Type)]
pub struct RepoDefaults {
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// Plans list pushed after every prompt, transition, or title change,
/// and returned by plan commands.
#[derive(Debug, Clone, Serialize, PartialEq, Type)]
pub struct PlansUpdate {
    pub plans: Vec<PlanEntry>,
    pub selected: SessionKey,
    pub config_defaults: RepoDefaults,
}

/// Repository payload returned after opening a repo. No agents spawn here;
/// every session starts lazily on its first prompt.
#[derive(Debug, Clone, Serialize, PartialEq, Type)]
pub struct OpenRepoResult {
    pub repo_root: String,
    pub branch: String,
    pub plans: Vec<PlanEntry>,
    pub selected: SessionKey,
    pub config_defaults: RepoDefaults,
}

/// Frontend event envelope. One Tauri event name carries every variant so
/// capabilities stay tight. Session events and `branch_changed` emit to
/// their owning window only; `open_repos_changed` broadcasts to every
/// window. Every session-specific variant carries its session key.
#[derive(Debug, Clone, Serialize, PartialEq, Type)]
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
    /// loads.
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
    /// Repos owned across windows. Broadcast to every window so pickers
    /// refresh their already-open marks.
    OpenReposChanged {
        repos: Vec<OpenRepoEntry>,
    },
}

/// One todo row mirrored from the agent's list.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct TodoView {
    pub content: String,
    pub status: TodoStatus,
}

/// Changed rows since the previous list: added rows plus status changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct TodoChangeView {
    pub content: String,
    pub status: TodoStatus,
}

/// One value inside a session config option (ACP `ConfigOptionValue`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct ConfigOptionValueView {
    pub value: String,
    pub name: String,
}

/// Session config option (ACP `ConfigOption`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOptionView {
    pub id: String,
    pub name: String,
    pub current_value: String,
    pub options: Vec<ConfigOptionValueView>,
    /// Spec `category`, absent when the agent omits it.
    pub category: Option<String>,
}

/// Recent repo row for the picker: path only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct RecentRepo {
    pub path: String,
}

/// One repo owned by one window: canonical path plus owner label.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct OpenRepoEntry {
    pub path: String,
    pub window: String,
}

impl AppEvent {
    /// Log level for the emit line: turn lifecycle stays at `info` so it
    /// stays greppable in the log file, high-volume per-chunk progress
    /// drops to `debug`. Pure.
    pub fn log_level(&self) -> log::Level {
        match self {
            AppEvent::AgentText { .. }
            | AppEvent::AgentThought { .. }
            | AppEvent::ToolLine { .. }
            | AppEvent::SpendTick { .. } => log::Level::Debug,
            _ => log::Level::Info,
        }
    }

    /// One-line log summary: wire name plus session key or selection.
    /// Pure, so the log shape is unit-tested without emitting.
    pub fn log_summary(&self) -> String {
        match self {
            AppEvent::AgentText { session, .. } => {
                format!("agent_text session={}::{:?}", session.plan, session.role)
            }
            AppEvent::AgentThought { session, .. } => {
                format!("agent_thought session={}::{:?}", session.plan, session.role)
            }
            AppEvent::UserText { session, .. } => {
                format!("user_text session={}::{:?}", session.plan, session.role)
            }
            AppEvent::ToolLine { session, .. } => {
                format!("tool_line session={}::{:?}", session.plan, session.role)
            }
            AppEvent::TurnDone { session } => {
                format!("turn_done session={}::{:?}", session.plan, session.role)
            }
            AppEvent::TurnFailed { session, .. } => {
                format!("turn_failed session={}::{:?}", session.plan, session.role)
            }
            AppEvent::AgentExited { session, .. } => {
                format!("agent_exited session={}::{:?}", session.plan, session.role)
            }
            AppEvent::PermissionAsked { session, .. } => {
                format!(
                    "permission_asked session={}::{:?}",
                    session.plan, session.role
                )
            }
            AppEvent::PermissionResolved { session, .. } => {
                format!(
                    "permission_resolved session={}::{:?}",
                    session.plan, session.role
                )
            }
            AppEvent::ConfigOptions { session, .. } => {
                format!(
                    "config_options session={}::{:?}",
                    session.plan, session.role
                )
            }
            AppEvent::TodosChanged { session, .. } => {
                format!("todos_changed session={}::{:?}", session.plan, session.role)
            }
            AppEvent::SpendTick { session, .. } => {
                format!("spend_tick session={}::{:?}", session.plan, session.role)
            }
            AppEvent::SessionReset { session } => {
                format!("session_reset session={}::{:?}", session.plan, session.role)
            }
            AppEvent::HistoryPreparing { session } => {
                format!(
                    "history_preparing session={}::{:?}",
                    session.plan, session.role
                )
            }
            AppEvent::HistoryBegin { session } => {
                format!("history_begin session={}::{:?}", session.plan, session.role)
            }
            AppEvent::HistoryDone { session } => {
                format!("history_done session={}::{:?}", session.plan, session.role)
            }
            AppEvent::HistoryFailed { session, .. } => {
                format!(
                    "history_failed session={}::{:?}",
                    session.plan, session.role
                )
            }
            AppEvent::BranchChanged { branch } => {
                format!("branch_changed branch={branch}")
            }
            AppEvent::PlansChanged { selected, .. } => {
                format!(
                    "plans_changed selected={}::{:?}",
                    selected.plan, selected.role
                )
            }
            AppEvent::OpenReposChanged { repos } => {
                format!("open_repos_changed count={}", repos.len())
            }
        }
    }
}

/// Local state persisted under the OS app-data directory.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Type)]
pub struct Prefs {
    #[serde(default)]
    pub recent: Vec<RecentRepo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binary_names_opencode() {
        assert!(AgentError::MissingBinary.to_string().contains("opencode"));
    }

    #[test]
    fn only_work_roles_auto_advance() {
        assert!(!SessionRole::Scoping.is_work());
        assert!(SessionRole::Executing.is_work());
        assert!(SessionRole::Evergreening.is_work());
        assert!(SessionRole::Rebasing.is_work());
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

    #[test]
    fn log_summary_carries_session_or_selection() {
        let key = SessionKey {
            plan: "2026-10-21.10-03-18".to_string(),
            role: SessionRole::Scoping,
        };
        assert_eq!(
            AppEvent::TurnDone {
                session: key.clone()
            }
            .log_summary(),
            "turn_done session=2026-10-21.10-03-18::Scoping"
        );
        assert_eq!(
            AppEvent::HistoryFailed {
                session: key.clone(),
                raw: "gone".to_string(),
                hint: "retry".to_string(),
                retryable: true,
            }
            .log_summary(),
            "history_failed session=2026-10-21.10-03-18::Scoping"
        );
        assert_eq!(
            AppEvent::PlansChanged {
                plans: Vec::new(),
                selected: key.clone(),
            }
            .log_summary(),
            "plans_changed selected=2026-10-21.10-03-18::Scoping"
        );
    }

    #[test]
    fn per_chunk_events_log_at_debug_and_lifecycle_at_info() {
        let key = SessionKey {
            plan: "2026-10-21.10-03-18".to_string(),
            role: SessionRole::Scoping,
        };
        let line = ToolLineView {
            id: "call_1".to_string(),
            text: "read: foo".to_string(),
            status: ToolStatus::Completed,
            label: "read".to_string(),
        };
        for event in [
            AppEvent::AgentText {
                session: key.clone(),
                chunk: "hi".to_string(),
            },
            AppEvent::AgentThought {
                session: key.clone(),
                chunk: "hmm".to_string(),
            },
            AppEvent::ToolLine {
                session: key.clone(),
                line,
            },
            AppEvent::SpendTick {
                session: key.clone(),
                cost: 1.0,
                ctx_pct: 2.0,
            },
        ] {
            assert_eq!(event.log_level(), log::Level::Debug);
        }
        for event in [
            AppEvent::TurnDone {
                session: key.clone(),
            },
            AppEvent::TurnFailed {
                session: key.clone(),
                raw: "boom".to_string(),
                hint: "retry".to_string(),
                retryable: true,
            },
            AppEvent::HistoryDone {
                session: key.clone(),
            },
            AppEvent::HistoryFailed {
                session: key.clone(),
                raw: "gone".to_string(),
                hint: "retry".to_string(),
                retryable: true,
            },
            AppEvent::PermissionAsked {
                session: key.clone(),
                permission: PermissionView {
                    tool_call_id: "call_1".to_string(),
                    title: "Run".to_string(),
                    tool: "bash".to_string(),
                    options: Vec::new(),
                    rule_hint: "once".to_string(),
                },
            },
            AppEvent::PlansChanged {
                plans: Vec::new(),
                selected: key.clone(),
            },
        ] {
            assert_eq!(event.log_level(), log::Level::Info);
        }
    }

    #[test]
    fn plan_entry_json_field_names() {
        let entry = PlanEntry {
            name: "2026-09-26.08-41-03".to_string(),
            phase: crate::plans::Phase::Scoping,
            title: "Untitled".to_string(),
            has_plan_md: false,
            sessions: Vec::new(),
            manual: false,
            evergreen: true,
            commits: Vec::new(),
            rebasing_progress: None,
        };
        let value = serde_json::to_value(&entry).expect("serializes");
        let actual: std::collections::BTreeSet<String> =
            value.as_object().expect("object").keys().cloned().collect();
        let expected: std::collections::BTreeSet<String> = [
            "name",
            "phase",
            "title",
            "has_plan_md",
            "sessions",
            "manual",
            "evergreen",
            "commits",
            "rebasing_progress",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        assert_eq!(actual, expected);
    }
}
