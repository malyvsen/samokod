// Agent lifecycle: one `opencode acp` child process per live session,
// sessions keyed by plan directory name plus role, plans listed from disk.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::{AppHandle, Emitter};

use crate::acp::{
    self, AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo, SessionConfigKind,
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOptions,
};
use crate::awake;
use crate::error_hint::{classify_error, is_transport_error};
use crate::opencode;
use crate::plans;
use crate::spend::context_pct;
use crate::todos::{diff_todos, todos_from_call, todos_from_update};
use crate::types::{
    AgentError, AppEvent, ConfigOptionValueView, ConfigOptionView, OpenRepoResult, PermissionView,
    PlanEntry, PlanInfo, PlansUpdate, SessionKey, SessionRole, SessionStatusView, TodoView,
};

/// User decision for one permission card.
#[derive(Debug, Clone)]
enum PermissionDecision {
    Selected(String),
    Cancelled,
}

/// Role phases: sessions only run on active plans.
fn role_phase(role: SessionRole) -> plans::Phase {
    match role {
        SessionRole::Scoping => plans::Phase::Scoping,
        SessionRole::Executing => plans::Phase::Executing,
    }
}

/// Plan owned by one live session.
#[derive(Debug, Clone)]
struct ActivePlan {
    name: String,
    phase: plans::Phase,
    /// Planner role already prepended to the first scoping message.
    prefixed: bool,
}

impl ActivePlan {
    fn scoping(name: String) -> Self {
        ActivePlan {
            name,
            phase: plans::Phase::Scoping,
            prefixed: false,
        }
    }

    fn executing(name: String) -> Self {
        ActivePlan {
            name,
            phase: plans::Phase::Executing,
            prefixed: true,
        }
    }

    fn key(&self) -> Option<SessionKey> {
        let role = match self.phase {
            plans::Phase::Scoping => SessionRole::Scoping,
            plans::Phase::Executing => SessionRole::Executing,
            plans::Phase::Completed | plans::Phase::Cancelled => return None,
        };
        Some(SessionKey {
            plan: self.name.clone(),
            role,
        })
    }

    fn plan_ref(&self) -> plans::PlanRef {
        plans::PlanRef {
            name: self.name.clone(),
            phase: self.phase,
        }
    }

    fn is_scoping(&self) -> bool {
        self.phase == plans::Phase::Scoping
    }
}

/// One live agent session: its own `opencode acp` process, connection, ACP
/// session id, and turn state.
struct LiveSession {
    connection: Option<ConnectionTo<Agent>>,
    supports_close: bool,
    session_id: Option<String>,
    working: bool,
    approval: bool,
    failed: bool,
    last_prompt: Option<String>,
    last_roles: crate::repo_state::RepoState,
    pending: HashMap<String, tokio::sync::oneshot::Sender<PermissionDecision>>,
    todos: Vec<TodoView>,
    plan: ActivePlan,
}

impl LiveSession {
    fn fresh(plan: ActivePlan, roles: crate::repo_state::RepoState) -> Self {
        LiveSession {
            connection: None,
            supports_close: false,
            session_id: None,
            working: false,
            approval: false,
            failed: false,
            last_prompt: None,
            last_roles: roles,
            pending: HashMap::new(),
            todos: Vec::new(),
            plan,
        }
    }

    fn is_live(&self) -> bool {
        self.session_id.is_some()
            && self
                .connection
                .as_ref()
                .map(|connection| !connection.is_incoming_closed())
                .unwrap_or(false)
    }
}

#[derive(Default)]
struct State {
    sessions: HashMap<SessionKey, LiveSession>,
    current: Option<SessionKey>,
    repo_root: Option<PathBuf>,
    branch: String,
    branch_watch: Option<notify::RecommendedWatcher>,
    awake: Option<awake::Guard>,
    /// Last user-sent prompt per plan directory name. Drives plan sorting;
    /// migrated across renames so an approved plan keeps its recency.
    activity: HashMap<String, Instant>,
}

impl State {
    /// Working and the sleep lock move together. The guard is held while
    /// any session works.
    fn set_working(&mut self, key: &SessionKey, working: bool) {
        if let Some(session) = self.sessions.get_mut(key) {
            session.working = working;
        }
        if self.sessions.values().any(|session| session.working) {
            if self.awake.is_none() {
                self.awake = awake::acquire();
            }
        } else {
            self.awake = None;
        }
    }
}

pub struct AgentManager {
    state: Arc<Mutex<State>>,
    app: AppHandle,
}

impl AgentManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            app,
        }
    }

    /// Open a repository: ensure the plan structure, rescan every plan,
    /// and select the most-recent session. Spawns no agents: each session
    /// starts lazily on its first prompt, with transcripts and TODOs kept
    /// run-local.
    pub async fn open_repo(&self, repo_root: PathBuf) -> Result<OpenRepoResult, AgentError> {
        plans::ensure_structure(&repo_root)?;
        self.clear_sessions();
        {
            let mut state = self.state.lock().expect("state poisoned");
            state.repo_root = Some(repo_root.clone());
            state.branch =
                crate::branch::current_branch(&repo_root).unwrap_or_else(|| "HEAD".to_string());
            if plans::scan_plans(&repo_root)
                .iter()
                .all(|plan| plan.phase != plans::Phase::Scoping)
            {
                plans::create_scoping(&repo_root)?;
            }
        }
        self.watch_branch(&repo_root);
        Ok(self.open_result())
    }

    /// Current repo payload: plans plus the most-recent session.
    fn open_result(&self) -> OpenRepoResult {
        let state = self.state.lock().expect("state poisoned");
        let repo_root = state.repo_root.clone().unwrap_or_default();
        let branch = state.branch.clone();
        let plans = sorted_entries(&repo_root, &state.sessions, &state.activity);
        let selected = most_recent_key(&plans).unwrap_or_else(|| SessionKey {
            plan: plans
                .first()
                .map(|entry| entry.name.clone())
                .unwrap_or_default(),
            role: SessionRole::Scoping,
        });
        OpenRepoResult {
            repo_root: repo_root.to_string_lossy().to_string(),
            branch,
            plans,
            selected,
        }
    }

    /// Fresh plans payload with the current selection. Emitted after every
    /// transition, activity, or title change.
    fn plans_update(&self) -> PlansUpdate {
        let state = self.state.lock().expect("state poisoned");
        let repo_root = state.repo_root.clone().unwrap_or_default();
        let plans = sorted_entries(&repo_root, &state.sessions, &state.activity);
        let selected = state.current.clone().unwrap_or_else(|| {
            most_recent_key(&plans).unwrap_or_else(|| SessionKey {
                plan: plans
                    .first()
                    .map(|entry| entry.name.clone())
                    .unwrap_or_default(),
                role: SessionRole::Scoping,
            })
        });
        PlansUpdate { plans, selected }
    }

    fn push_plans(&self) {
        let update = self.plans_update();
        emit_event(
            &self.app,
            AppEvent::PlansChanged {
                plans: update.plans,
                selected: update.selected,
            },
        );
    }

    /// Create a fresh scoping plan and select it. Its agent spawns lazily
    /// on the first prompt, so this never blocks on other sessions.
    pub async fn create_plan(&self) -> Result<PlansUpdate, AgentError> {
        let repo_root = self.current_repo().ok_or_else(|| AgentError::NoSession {
            raw: "open a repository first".to_string(),
        })?;
        let scoping = plans::create_scoping(&repo_root)?;
        let selected = SessionKey {
            plan: scoping.name,
            role: SessionRole::Scoping,
        };
        match self.state.lock() {
            Ok(mut state) => {
                state.current = Some(selected);
            }
            Err(error) => {
                log::warn!("failed to select new plan: {error}");
            }
        }
        Ok(self.plans_update())
    }

    /// Re-check the branch for the open repo. Failures keep the last value.
    pub async fn refresh_branch(&self) -> Result<String, AgentError> {
        let repo_root = lock_state(&self.state)
            .and_then(|state| state.repo_root.clone())
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let branch =
            crate::branch::current_branch(&repo_root).unwrap_or_else(|| "HEAD".to_string());
        self.set_branch(branch.clone());
        Ok(branch)
    }

    /// Set the held branch and emit it. Returns true when it moved.
    fn set_branch(&self, branch: String) -> bool {
        let moved = match self.state.lock() {
            Ok(mut state) => {
                if state.branch == branch {
                    false
                } else {
                    state.branch = branch.clone();
                    true
                }
            }
            Err(error) => {
                log::warn!("failed to record branch change: {error}");
                false
            }
        };
        if moved {
            emit_event(&self.app, AppEvent::BranchChanged { branch });
        }
        moved
    }

    /// Watch `.git/HEAD` for the open repo, refreshing `state.branch` per
    /// event so execute/retry snapshots go fresh free.
    fn watch_branch(&self, repo_root: &Path) {
        let root = repo_root.to_path_buf();
        let state = Arc::clone(&self.state);
        let app = self.app.clone();
        let watcher = crate::branch::watch_branch(root.clone(), move |branch| {
            let moved = match state.lock() {
                Ok(mut guard) => {
                    if guard.repo_root.as_ref() != Some(&root) || guard.branch == branch {
                        false
                    } else {
                        guard.branch = branch.clone();
                        true
                    }
                }
                Err(error) => {
                    log::warn!("failed to record branch change: {error}");
                    false
                }
            };
            if moved {
                emit_event(&app, AppEvent::BranchChanged { branch });
            }
        });
        match self.state.lock() {
            Ok(mut state) => {
                state.branch_watch = watcher;
            }
            Err(error) => {
                log::warn!("failed to hold branch watcher: {error}");
            }
        }
    }

    /// Set one session config option without restarting the session.
    /// Returns the agent's complete option list, including dependent updates.
    /// Also stores the choice as the repo-wide default new sessions start
    /// from.
    pub async fn set_config_option(
        &self,
        session: SessionKey,
        config_id: String,
        value: String,
    ) -> Result<Vec<ConfigOptionView>, AgentError> {
        let (connection, session_id, _, _) =
            self.session_snapshot_for(&session)
                .ok_or_else(|| AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                })?;
        let response = send_config_option(
            &connection,
            &acp::SessionId::new(session_id),
            &config_id,
            &value,
        )
        .await
        .map_err(|error| AgentError::RequestFailed { raw: error })?;
        // Re-sync both roles from the full response list: dependent options
        // can vanish in the same response, clearing the stored role.
        let roles = crate::repo_state::roles_from_options(&response);
        let moved = match self.state.lock() {
            Ok(mut state) => {
                let current_roles = state
                    .sessions
                    .get(&session)
                    .map(|live| live.last_roles.clone())
                    .unwrap_or_default();
                let moved: Vec<crate::repo_state::ConfigRole> = crate::repo_state::ConfigRole::ALL
                    .into_iter()
                    .filter(|role| role.get(&current_roles) != role.get(&roles))
                    .collect();
                if let Some(live) = state.sessions.get_mut(&session) {
                    live.last_roles = roles.clone();
                }
                Some(moved)
            }
            Err(error) => {
                log::warn!("failed to remember config choice: {error}");
                None
            }
        };
        if let (Some(moved), Some(repo)) = (moved, self.current_repo()) {
            for role in moved {
                crate::repo_state::set_role(&repo, role, role.get(&roles).map(String::as_str));
            }
        }
        log_roles("selected", &roles);
        Ok(response)
    }

    /// Approve the scoping plan: move it to executing, switch to a fresh
    /// executor process, and start it immediately. The prompt stays hidden:
    /// no user bubble, the chat opens working. The selection follows the
    /// new execution session.
    pub async fn execute_plan(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        ensure_idle(&self.state, &session)?;
        let (repo_root, branch) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let from = plans::PlanRef {
            name: session.plan.clone(),
            phase: plans::Phase::Scoping,
        };
        if session.role != SessionRole::Scoping || !from.path(&repo_root).is_dir() {
            return Err(AgentError::RequestFailed {
                raw: "no scoping plan to execute".to_string(),
            });
        }
        let next = plans::execute(&repo_root, &from)?;
        plans::mark_executed(&repo_root, &next);
        self.drop_live(&session).await;
        self.move_activity(&from.name, &next.name);
        let plan = ActivePlan::executing(next.name.clone());
        let agent = opencode::agent_for(plan.phase);
        let (connection, session_id, key) =
            self.spawn_session(&repo_root, &branch, plan, agent).await?;
        let text = opencode::executor_first_message(&opencode::plan_display(&next));
        self.touch_activity(&next.name);
        self.start_turn(connection, session_id, key, text, None)
            .await?;
        Ok(self.plans_update())
    }

    /// Finish the executing plan. The selection stays on the completed
    /// (now read-only) execution session; fresh plans come from `+ NEW PLAN`.
    pub async fn mark_completed(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        ensure_idle(&self.state, &session)?;
        let (repo_root, _) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let from = plans::PlanRef {
            name: session.plan.clone(),
            phase: plans::Phase::Executing,
        };
        if session.role != SessionRole::Executing || !from.path(&repo_root).is_dir() {
            return Err(AgentError::RequestFailed {
                raw: "no executing plan to complete".to_string(),
            });
        }
        let next = plans::complete(&repo_root, &from)?;
        self.drop_live(&session).await;
        self.move_activity(&from.name, &next.name);
        self.select_key(SessionKey {
            plan: next.name,
            role: SessionRole::Executing,
        });
        Ok(self.plans_update())
    }

    /// Drop a scoping plan. The selection stays on the cancelled (now
    /// read-only) scoping session.
    pub async fn abandon_plan(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        ensure_idle(&self.state, &session)?;
        let (repo_root, _) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let from = plans::PlanRef {
            name: session.plan.clone(),
            phase: plans::Phase::Scoping,
        };
        if session.role != SessionRole::Scoping || !from.path(&repo_root).is_dir() {
            return Err(AgentError::RequestFailed {
                raw: "no active plan to abandon".to_string(),
            });
        }
        let next = plans::abandon(&repo_root, &from)?;
        self.drop_live(&session).await;
        self.move_activity(&from.name, &next.name);
        self.select_key(SessionKey {
            plan: next.name,
            role: SessionRole::Scoping,
        });
        Ok(self.plans_update())
    }

    /// Cancel an executing plan. The selection stays on the cancelled (now
    /// read-only) execution session.
    pub async fn cancel_execution(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        ensure_idle(&self.state, &session)?;
        let (repo_root, _) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let from = plans::PlanRef {
            name: session.plan.clone(),
            phase: plans::Phase::Executing,
        };
        if session.role != SessionRole::Executing || !from.path(&repo_root).is_dir() {
            return Err(AgentError::RequestFailed {
                raw: "no executing plan to cancel".to_string(),
            });
        }
        let next = plans::abandon(&repo_root, &from)?;
        self.drop_live(&session).await;
        self.move_activity(&from.name, &next.name);
        self.select_key(SessionKey {
            plan: next.name,
            role: SessionRole::Executing,
        });
        Ok(self.plans_update())
    }

    /// Record one user prompt for plan sorting. Pure timestamp edge.
    fn touch_activity(&self, plan: &str) {
        if let Some(mut state) = lock_state(&self.state) {
            state.activity.insert(plan.to_string(), Instant::now());
        }
        self.push_plans();
    }

    /// Carry recency across a plan rename. Pure timestamp edge.
    fn move_activity(&self, from: &str, to: &str) {
        if let Some(mut state) = lock_state(&self.state)
            && let Some(when) = state.activity.remove(from)
        {
            state.activity.insert(to.to_string(), when);
        }
    }

    /// Pin the selection to one session key.
    fn select_key(&self, key: SessionKey) {
        match self.state.lock() {
            Ok(mut state) => {
                state.current = Some(key);
            }
            Err(error) => {
                log::warn!("failed to select session: {error}");
            }
        }
    }

    /// Shut down one live session and forget it. Its transcript stays
    /// frontend-side; the row turns gray.
    async fn drop_live(&self, key: &SessionKey) {
        let snapshot = self.session_snapshot_for(key);
        match self.state.lock() {
            Ok(mut state) => {
                if let Some(session) = state.sessions.get_mut(key) {
                    session.connection = None;
                    session.session_id = None;
                    for (_, sender) in session.pending.drain() {
                        if sender.send(PermissionDecision::Cancelled).is_err() {
                            log::debug!("pending permission receiver gone during drop");
                        }
                    }
                }
                state.sessions.remove(key);
                if state.sessions.values().all(|live| !live.working) {
                    state.awake = None;
                }
            }
            Err(error) => {
                log::warn!("failed to drop live session: {error}");
            }
        }
        if let Some((connection, id, supports_close, _)) = snapshot
            && supports_close
            && let Err(error) = connection
                .send_request(acp::CloseSessionRequest::new(acp::SessionId::new(
                    id.as_str(),
                )))
                .block_task()
                .await
        {
            log::warn!("failed to close previous session {id}: {error}");
        }
    }

    /// Spawn a fresh agent process scoped to one plan and open a session on
    /// it: pin the agent mode, reapply the repo default model then effort
    /// (effort-last, since a model switch can reshape effort options).
    /// Shared by lazy first prompts and the eager executor start.
    async fn spawn_session(
        &self,
        repo_root: &Path,
        branch: &str,
        plan: ActivePlan,
        agent: &str,
    ) -> Result<(ConnectionTo<Agent>, String, SessionKey), AgentError> {
        let key = plan.key().ok_or_else(|| AgentError::RequestFailed {
            raw: "cannot open a session for a finished plan".to_string(),
        })?;
        let stored = crate::repo_state::load_repo_state(repo_root);
        let connection = self
            .ensure_connection_for(&key, &plan, opencode::agent_env(&plan.plan_ref()))
            .await?;
        let response = connection
            .send_request(acp::build_new_session_request(repo_root))
            .block_task()
            .await
            .map_err(|error| AgentError::RequestFailed {
                raw: error.to_string(),
            })?;
        let session_id = response.session_id.to_string();
        let mut options =
            pin_agent_mode(&connection, &acp::SessionId::new(session_id.clone()), agent).await?;
        // Reapply model first, then effort; each absent or inapplicable role
        // is skipped with a warn-log while the open continues on defaults.
        let mut applied = crate::repo_state::RepoState::default();
        for role in crate::repo_state::ConfigRole::ALL {
            let Some(wanted) = role.get(&stored).cloned() else {
                continue;
            };
            let Some(id) = option_id_for_role(&options, role) else {
                log::warn!("stored {} has no matching option, skipping", role.name());
                continue;
            };
            let session = acp::SessionId::new(session_id.clone());
            match send_config_option(&connection, &session, &id, &wanted).await {
                Ok(updated) => {
                    options = updated;
                    role.set(&mut applied, Some(wanted));
                }
                Err(error) => {
                    log::warn!("failed to reapply stored {} {wanted}: {error}", role.name())
                }
            }
        }
        log_roles("applied", &applied);
        {
            let mut state = self.state.lock().expect("state poisoned");
            let supports_close = state
                .sessions
                .get(&key)
                .map(|session| session.supports_close)
                .unwrap_or(false);
            // A reopened session keeps its one-time prefix state; a fresh
            // plan starts unprefixed.
            let prefixed = state
                .sessions
                .get(&key)
                .map(|session| session.plan.prefixed)
                .unwrap_or(plan.prefixed);
            let mut plan = plan;
            plan.prefixed = prefixed;
            state.sessions.insert(
                key.clone(),
                LiveSession {
                    connection: Some(connection.clone()),
                    supports_close,
                    session_id: Some(session_id.clone()),
                    working: false,
                    approval: false,
                    failed: false,
                    last_prompt: None,
                    last_roles: applied,
                    pending: HashMap::new(),
                    todos: Vec::new(),
                    plan,
                },
            );
            state.repo_root = Some(repo_root.to_path_buf());
            state.branch = branch.to_string();
            state.current = Some(key.clone());
        }
        emit_event(
            &self.app,
            AppEvent::SessionReset {
                session: key.clone(),
            },
        );
        emit_event(
            &self.app,
            AppEvent::ConfigOptions {
                session: key.clone(),
                options,
            },
        );
        Ok((connection, session_id, key))
    }

    /// Live connection for one session, spawning lazily on the first
    /// prompt: same plan directory, mode pin, stored model/effort
    /// reapplied. Restored sessions start unprefixed, so the role template
    /// prepends to their first message again.
    async fn ensure_live(
        &self,
        key: &SessionKey,
    ) -> Result<(ConnectionTo<Agent>, String), AgentError> {
        if let Some((connection, session_id, _, _)) = self.session_snapshot_for(key)
            && !connection.is_incoming_closed()
        {
            return Ok((connection, session_id));
        }
        let (repo_root, branch) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let plan_ref = plans::PlanRef {
            name: key.plan.clone(),
            phase: role_phase(key.role),
        };
        if !plan_ref.path(&repo_root).is_dir() {
            return Err(AgentError::NoSession {
                raw: "plan is gone".to_string(),
            });
        }
        let stored_prefixed = lock_state(&self.state)
            .and_then(|state| state.sessions.get(key).map(|live| live.plan.prefixed));
        let mut plan = match key.role {
            SessionRole::Scoping => ActivePlan::scoping(key.plan.clone()),
            SessionRole::Executing => ActivePlan::executing(key.plan.clone()),
        };
        // A known session keeps its prefix state across transport deaths;
        // a restored one starts unprefixed so the role prepends again.
        if let Some(prefixed) = stored_prefixed {
            plan.prefixed = prefixed;
        }
        let agent = opencode::agent_for(plan.phase);
        let (connection, session_id, _) =
            self.spawn_session(&repo_root, &branch, plan, agent).await?;
        Ok((connection, session_id))
    }

    /// Drop every live session entry. Repo opens start from scratch.
    fn clear_sessions(&self) {
        match self.state.lock() {
            Ok(mut state) => {
                state.sessions.clear();
                state.current = None;
                state.awake = None;
            }
            Err(error) => {
                log::warn!("failed to clear sessions: {error}");
            }
        }
    }

    /// Current repository root, when a session is open.
    pub fn current_repo(&self) -> Option<PathBuf> {
        lock_state(&self.state)?.repo_root.clone()
    }

    /// Resend the last prompt. When the transport is closed, reopen the
    /// session on the held plan first. Returns false when nothing ran.
    pub async fn retry_last(&self, session: SessionKey) -> Result<bool, AgentError> {
        let Some(text) = lock_state(&self.state)
            .and_then(|state| state.sessions.get(&session)?.last_prompt.clone())
        else {
            return Ok(false);
        };
        let (connection, session_id) = self.ensure_live(&session).await?;
        let watch = self.scoping_watch_for(&session);
        self.start_turn(connection, session_id, session, text, watch)
            .await?;
        Ok(true)
    }

    /// Send one plain-text prompt, spawning the session lazily on its
    /// first message. Streams arrive as events; the turn end arrives as
    /// done or failed. The role template prefixes the first message per
    /// ACP conversation; the transcript keeps the raw text.
    pub async fn send_prompt(&self, session: SessionKey, text: String) -> Result<(), AgentError> {
        let (connection, session_id) = self.ensure_live(&session).await?;
        // A fresh prompt clears the failed flag; the dot goes green while
        // the turn runs.
        if let Some(mut state) = lock_state(&self.state)
            && let Some(live) = state.sessions.get_mut(&session)
        {
            live.failed = false;
        }
        self.select_key(session.clone());
        self.touch_activity(&session.plan);
        let watch = self.scoping_watch_for(&session);
        let mut text = text;
        if let Some(plan) = self.claim_planner_prefix(&session) {
            text = opencode::planner_first_message(&opencode::plan_display(&plan), &text);
        }
        self.start_turn(connection, session_id, session, text, watch)
            .await
    }

    /// Plan watch for scoping turns: re-emit `plan.md` presence when the
    /// turn lands.
    fn scoping_watch_for(&self, key: &SessionKey) -> Option<(PathBuf, plans::PlanRef)> {
        let state = lock_state(&self.state)?;
        let repo_root = state.repo_root.clone()?;
        let live = state.sessions.get(key)?;
        if !live.plan.is_scoping() {
            return None;
        }
        Some((repo_root, live.plan.plan_ref()))
    }

    /// Claim the one-time role prefix for the first message of one ACP
    /// conversation. One locked check-and-mark, so a retried turn never
    /// prefixes twice.
    fn claim_planner_prefix(&self, key: &SessionKey) -> Option<plans::PlanRef> {
        let mut state = lock_state(&self.state)?;
        let live = state.sessions.get_mut(key)?;
        if !live.plan.is_scoping() || live.plan.prefixed {
            return None;
        }
        live.plan.prefixed = true;
        Some(live.plan.plan_ref())
    }

    /// Guard one turn and stream it as events. Records the prompt so retry
    /// resends exactly what ran, prefixed or not.
    async fn start_turn(
        &self,
        connection: ConnectionTo<Agent>,
        session_id: String,
        key: SessionKey,
        text: String,
        watch: Option<(PathBuf, plans::PlanRef)>,
    ) -> Result<(), AgentError> {
        {
            let mut state = self.state.lock().expect("state poisoned");
            let busy = state
                .sessions
                .get(&key)
                .map(|session| session.working)
                .unwrap_or(false);
            if busy {
                return Err(AgentError::RequestFailed {
                    raw: "a turn is already running".to_string(),
                });
            }
            state.set_working(&key, true);
            if let Some(session) = state.sessions.get_mut(&key) {
                session.last_prompt = Some(text.clone());
            }
        }
        // Dots go green while the turn runs; titles and order refresh when
        // it lands.
        self.push_plans();
        let state = Arc::clone(&self.state);
        let app = self.app.clone();
        let push_state = Arc::clone(&self.state);
        let push_app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let prompt = acp::PromptRequest::new(
                acp::SessionId::new(session_id),
                vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
            );
            match connection.send_request(prompt).block_task().await {
                Ok(_) => {
                    set_working(&state, &key, false);
                    set_failed(&state, &key, false);
                    emit_event(
                        &app,
                        AppEvent::TurnDone {
                            session: key.clone(),
                        },
                    );
                    if let Some((repo_root, plan)) = watch {
                        emit_event(
                            &app,
                            AppEvent::PlanChanged {
                                session: key.clone(),
                                plan: PlanInfo::of(&repo_root, &plan),
                            },
                        );
                    }
                    push_sorted(&push_state, &push_app);
                }
                Err(error) => {
                    let raw = error.to_string();
                    let transport_gone = is_transport_error(&raw);
                    set_working(&state, &key, false);
                    set_failed(&state, &key, true);
                    if transport_gone {
                        match state.lock() {
                            Ok(mut guard) => {
                                if let Some(session) = guard.sessions.get_mut(&key) {
                                    session.connection = None;
                                    session.session_id = None;
                                }
                            }
                            Err(error) => {
                                log::warn!("failed to drop dead agent connection: {error}");
                            }
                        }
                    }
                    let hint = classify_error(&raw);
                    emit_event(
                        &app,
                        AppEvent::TurnFailed {
                            session: key.clone(),
                            raw,
                            hint: hint.text,
                            retryable: hint.retryable,
                        },
                    );
                    push_sorted(&push_state, &push_app);
                }
            }
        });
        Ok(())
    }

    /// Stop the turn via `session/cancel`, answering every open card as
    /// cancelled per protocol.
    pub async fn cancel_turn(&self, session: SessionKey) -> Result<(), AgentError> {
        let (connection, session_id, _, _) =
            self.session_snapshot_for(&session)
                .ok_or_else(|| AgentError::NoSession {
                    raw: "no active turn".to_string(),
                })?;
        cancel_pending(&self.state, &session);
        if let Err(error) = connection.send_notification(acp::build_cancel_notification(
            acp::SessionId::new(session_id),
        )) {
            log::warn!("failed to send session cancel: {error}");
        }
        set_working(&self.state, &session, false);
        self.push_plans();
        Ok(())
    }

    /// Answer one permission card. `Some` selects the allow-once or reject
    /// option; `None` answers cancelled.
    pub fn answer_permission(
        &self,
        session: SessionKey,
        tool_call_id: &str,
        option_id: Option<String>,
    ) -> Result<(), AgentError> {
        let sender = self
            .state
            .lock()
            .map_err(|_| AgentError::RequestFailed {
                raw: "permission state poisoned".to_string(),
            })
            .map(|mut guard| {
                guard
                    .sessions
                    .get_mut(&session)
                    .and_then(|live| live.pending.remove(tool_call_id))
            });
        match sender {
            Ok(Some(sender)) => {
                let decision = match option_id {
                    Some(id) => PermissionDecision::Selected(id),
                    None => PermissionDecision::Cancelled,
                };
                if sender.send(decision).is_err() {
                    log::debug!("permission decision receiver gone for {tool_call_id}");
                }
                Ok(())
            }
            Ok(None) => Err(AgentError::RequestFailed {
                raw: format!("no pending permission for {tool_call_id}"),
            }),
            Err(error) => Err(error),
        }
    }

    /// Ensure a live, initialized connection for one session. Spawns a fresh
    /// `opencode acp` process per session, reuses it for later turns, and
    /// respawns after a closed transport. `extra_env` (agent definitions)
    /// merges over the process environment.
    async fn ensure_connection_for(
        &self,
        key: &SessionKey,
        plan: &ActivePlan,
        extra_env: HashMap<String, String>,
    ) -> Result<ConnectionTo<Agent>, AgentError> {
        if let Some(connection) = self.connection_snapshot_for(key) {
            if !connection.is_incoming_closed() {
                return Ok(connection);
            }
            match self.state.lock() {
                Ok(mut state) => {
                    if let Some(session) = state.sessions.get_mut(key) {
                        session.connection = None;
                    }
                }
                Err(error) => {
                    log::warn!("failed to drop closed agent connection: {error}");
                }
            }
        }
        let binary = acp::resolve_opencode_binary()?;
        let path_value = acp::agent_path_value();
        log::info!("spawning {} with PATH={}", binary.display(), path_value);
        let mut env = HashMap::from([("PATH".to_string(), path_value)]);
        env.extend(extra_env);
        let config = AcpAgentConfig::new(binary).arg("acp").envs(env);
        let agent = AcpAgent::new(config);
        let slot = Arc::clone(&self.state);
        let notify_state = Arc::clone(&self.state);
        let notify_app = self.app.clone();
        let notify_key = key.clone();
        let ask_state = Arc::clone(&self.state);
        let ask_app = self.app.clone();
        let ask_key = key.clone();
        let exit_app = self.app.clone();
        let exit_key = key.clone();
        let slot_key = key.clone();
        let slot_plan = plan.clone();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        let ready = Arc::new(Mutex::new(Some(ready_tx)));
        let ready_for_handler = Arc::clone(&ready);
        let ready_for_error = Arc::clone(&ready);

        tauri::async_runtime::spawn(async move {
            let result = Client
                .builder()
                .on_receive_notification(
                    move |notification: acp::SessionNotification, _cx| {
                        let state = Arc::clone(&notify_state);
                        let app = notify_app.clone();
                        let key = notify_key.clone();
                        async move {
                            handle_notification(&state, &app, &key, &notification);
                            Ok(())
                        }
                    },
                    agent_client_protocol::on_receive_notification!(),
                )
                .on_receive_request(
                    move |request: acp::RequestPermissionRequest, responder, _cx| {
                        let state = Arc::clone(&ask_state);
                        let app = ask_app.clone();
                        let key = ask_key.clone();
                        async move {
                            handle_permission_request(&state, &app, &key, request, responder).await;
                            Ok(())
                        }
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .connect_with(agent, |connection: ConnectionTo<Agent>| {
                    let slot = Arc::clone(&slot);
                    let ready = Arc::clone(&ready_for_handler);
                    let key = slot_key.clone();
                    let plan = slot_plan.clone();
                    async move {
                        let init_response = connection
                            .send_request(acp::build_initialize_request())
                            .block_task()
                            .await
                            .map_err(|error| acp::internal_error(error.to_string()))?;
                        {
                            let mut state = slot.lock().expect("state poisoned");
                            let supports_close = init_response
                                .agent_capabilities
                                .session_capabilities
                                .close
                                .is_some();
                            match state.sessions.get_mut(&key) {
                                Some(session) => {
                                    session.connection = Some(connection.clone());
                                    session.supports_close = supports_close;
                                }
                                None => {
                                    let mut fresh = LiveSession::fresh(
                                        plan,
                                        crate::repo_state::RepoState::default(),
                                    );
                                    fresh.connection = Some(connection.clone());
                                    fresh.supports_close = supports_close;
                                    state.sessions.insert(key.clone(), fresh);
                                }
                            }
                        }
                        if let Some(tx) = ready.lock().expect("ready poisoned").take()
                            && tx.send(Ok(())).is_err()
                        {
                            log::debug!("agent ready receiver gone during initialize");
                        }
                        connection.incoming_closed().await;
                        Ok(())
                    }
                })
                .await;
            if let Err(error) = result {
                let raw = error.to_string();
                if let Some(tx) = ready_for_error.lock().expect("ready poisoned").take() {
                    let hint = classify_error(&raw);
                    // Startup failures land on the failed flag so the row
                    // dot turns red even before any prompt ran.
                    set_failed(&slot, &exit_key, true);
                    push_sorted(&slot, &exit_app);
                    emit_event(
                        &exit_app,
                        AppEvent::AgentExited {
                            session: exit_key,
                            raw: raw.clone(),
                            hint: hint.text,
                            retryable: hint.retryable,
                        },
                    );
                    if tx.send(Err(raw)).is_err() {
                        log::debug!("agent startup receiver gone");
                    }
                } else {
                    log::warn!("agent task failed after startup: {raw}");
                }
            }
        });

        match ready_rx.await {
            Ok(Ok(())) => {
                self.connection_snapshot_for(key)
                    .ok_or_else(|| AgentError::RequestFailed {
                        raw: "agent connection vanished during initialize".to_string(),
                    })
            }
            Ok(Err(raw)) => Err(map_startup_error(&raw)),
            Err(_) => Err(AgentError::RequestFailed {
                raw: "agent startup was cancelled".to_string(),
            }),
        }
    }

    fn reopen_snapshot(&self) -> Option<(PathBuf, String)> {
        let state = lock_state(&self.state)?;
        Some((state.repo_root.clone()?, state.branch.clone()))
    }

    fn connection_snapshot_for(&self, key: &SessionKey) -> Option<ConnectionTo<Agent>> {
        lock_state(&self.state).and_then(|state| state.sessions.get(key)?.connection.clone())
    }

    fn session_snapshot_for(
        &self,
        key: &SessionKey,
    ) -> Option<(ConnectionTo<Agent>, String, bool, SessionKey)> {
        let state = lock_state(&self.state)?;
        let session = state.sessions.get(key)?;
        Some((
            session.connection.clone()?,
            session.session_id.clone()?,
            session.supports_close,
            key.clone(),
        ))
    }
}

/// Plans sorted by phase, then most recent user activity first. Before any
/// activity, `plan.md` modification time newest first, falling back to the
/// directory name (which starts with a creation timestamp).
fn sorted_entries(
    repo_root: &Path,
    sessions: &HashMap<SessionKey, LiveSession>,
    activity: &HashMap<String, Instant>,
) -> Vec<PlanEntry> {
    let mut plans = plans::scan_plans(repo_root);
    plans.sort_by(|left, right| {
        plans::phase_rank(left.phase)
            .cmp(&plans::phase_rank(right.phase))
            .then_with(|| {
                // Active plans sort before idle ones; recency decides
                // within each group.
                match (activity.get(&left.name), activity.get(&right.name)) {
                    (Some(left_at), Some(right_at)) => right_at.cmp(left_at),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => plans::plan_mtime(repo_root, right)
                        .cmp(&plans::plan_mtime(repo_root, left))
                        .then_with(|| right.name.cmp(&left.name)),
                }
            })
    });
    plans
        .iter()
        .map(|plan| PlanEntry {
            name: plan.name.clone(),
            phase: plan.phase,
            title: plans::plan_title(repo_root, plan),
            sessions: session_statuses(repo_root, plan, sessions),
        })
        .collect()
}

/// One status row per session a plan owns: its scoping session, plus its
/// execution session once approved.
fn session_statuses(
    repo_root: &Path,
    plan: &plans::PlanRef,
    sessions: &HashMap<SessionKey, LiveSession>,
) -> Vec<SessionStatusView> {
    let mut roles = vec![SessionRole::Scoping];
    if plans::has_execution(repo_root, plan) {
        roles.push(SessionRole::Executing);
    }
    roles
        .into_iter()
        .map(|role| {
            let key = SessionKey {
                plan: plan.name.clone(),
                role,
            };
            let live = sessions.get(&key);
            SessionStatusView {
                role,
                working: live.map(|live| live.working).unwrap_or(false),
                approval: live.map(|live| live.approval).unwrap_or(false),
                failed: live.map(|live| live.failed).unwrap_or(false),
                live: live.map(|live| live.is_live()).unwrap_or(false),
            }
        })
        .collect()
}

/// Most-recent session across the sorted plans: the execution session when
/// the plan owns one, else scoping.
fn most_recent_key(plans: &[PlanEntry]) -> Option<SessionKey> {
    plans.first().map(|entry| {
        let role = entry
            .sessions
            .iter()
            .find(|status| status.role == SessionRole::Executing)
            .map(|_| SessionRole::Executing)
            .unwrap_or(SessionRole::Scoping);
        SessionKey {
            plan: entry.name.clone(),
            role,
        }
    })
}

/// Refuse plan transitions while the session's turn runs. A poisoned lock
/// only logs, matching `lock_state`.
fn ensure_idle(state: &Mutex<State>, key: &SessionKey) -> Result<(), AgentError> {
    match state.lock() {
        Ok(state) => {
            let busy = state
                .sessions
                .get(key)
                .map(|session| session.working)
                .unwrap_or(false);
            if busy {
                return Err(AgentError::RequestFailed {
                    raw: "a turn is already running".to_string(),
                });
            }
            Ok(())
        }
        Err(error) => {
            log::warn!("failed to check working state: {error}");
            Ok(())
        }
    }
}

/// Emit one app event. Emissions are fire-and-forget, but a failure desyncs
/// the UI from the agent, so it is always logged.
fn emit_event(app: &AppHandle, event: AppEvent) {
    if let Err(error) = app.emit("samokod://event", event) {
        log::warn!("failed to emit app event: {error}");
    }
}

/// Lock agent state. A poisoned mutex means a prior panic elsewhere; log it
/// instead of silently dropping the update.
fn lock_state(state: &Mutex<State>) -> Option<std::sync::MutexGuard<'_, State>> {
    match state.lock() {
        Ok(guard) => Some(guard),
        Err(error) => {
            log::warn!("agent state lock poisoned: {error}");
            None
        }
    }
}

fn set_working(state: &Mutex<State>, key: &SessionKey, working: bool) {
    let Some(mut guard) = lock_state(state) else {
        return;
    };
    guard.set_working(key, working);
}

fn set_failed(state: &Mutex<State>, key: &SessionKey, failed: bool) {
    let Some(mut guard) = lock_state(state) else {
        return;
    };
    if let Some(session) = guard.sessions.get_mut(key) {
        session.failed = failed;
    }
}

fn set_approval(state: &Mutex<State>, key: &SessionKey, approval: bool) {
    let Some(mut guard) = lock_state(state) else {
        return;
    };
    if let Some(session) = guard.sessions.get_mut(key) {
        session.approval = approval;
    }
}

/// Rebuild the plans list from a spawned task holding only state and app.
/// Titles re-read here, so every finished turn refreshes plan names.
fn push_sorted(state: &Mutex<State>, app: &AppHandle) {
    let (plans, selected) = match state.lock() {
        Ok(guard) => {
            let repo_root = guard.repo_root.clone().unwrap_or_default();
            let plans = sorted_entries(&repo_root, &guard.sessions, &guard.activity);
            let selected = guard.current.clone().or_else(|| most_recent_key(&plans));
            (plans, selected)
        }
        Err(error) => {
            log::warn!("failed to rebuild plans list: {error}");
            return;
        }
    };
    if let Some(selected) = selected {
        emit_event(app, AppEvent::PlansChanged { plans, selected });
    }
}

fn cancel_pending(state: &Mutex<State>, key: &SessionKey) {
    let senders: Vec<tokio::sync::oneshot::Sender<PermissionDecision>> = match state.lock() {
        Ok(mut guard) => guard
            .sessions
            .get_mut(key)
            .map(|session| session.pending.drain().map(|(_, sender)| sender).collect())
            .unwrap_or_default(),
        Err(error) => {
            log::warn!("failed to cancel pending permissions: {error}");
            Vec::new()
        }
    };
    for sender in senders {
        if sender.send(PermissionDecision::Cancelled).is_err() {
            log::debug!("pending permission receiver gone during cancel");
        }
    }
}

async fn handle_permission_request(
    state: &Mutex<State>,
    app: &AppHandle,
    key: &SessionKey,
    request: acp::RequestPermissionRequest,
    responder: agent_client_protocol::Responder<acp::RequestPermissionResponse>,
) {
    let current = lock_state(state).and_then(|guard| {
        guard
            .sessions
            .get(key)
            .and_then(|session| session.session_id.clone())
    });
    if let Some(current) = current
        && request.session_id.to_string() != current
    {
        if responder
            .respond(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled,
            ))
            .is_err()
        {
            log::debug!("permission responder gone for stale session");
        }
        return;
    }
    let options = crate::permissions::to_view(&request.options);
    if options.is_empty() {
        if responder
            .respond(acp::RequestPermissionResponse::new(
                acp::RequestPermissionOutcome::Cancelled,
            ))
            .is_err()
        {
            log::debug!("permission responder gone for empty options");
        }
        return;
    }
    let tool_call_id = request.tool_call.tool_call_id.to_string();
    let title = request
        .tool_call
        .fields
        .title
        .clone()
        .unwrap_or_else(|| "run this action?".to_string());
    let kind = crate::updates::kind_label(request.tool_call.fields.kind).to_string();
    let permission = PermissionView {
        tool_call_id: tool_call_id.clone(),
        title,
        kind,
        options,
        rule_hint: crate::permissions::rule_hint(request.tool_call.fields.name.as_deref()),
    };
    let (tx, rx) = tokio::sync::oneshot::channel::<PermissionDecision>();
    {
        let Some(mut guard) = lock_state(state) else {
            if responder
                .respond(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ))
                .is_err()
            {
                log::debug!("permission responder gone while state locked");
            }
            return;
        };
        let Some(session) = guard.sessions.get_mut(key) else {
            if responder
                .respond(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ))
                .is_err()
            {
                log::debug!("permission responder gone for missing session");
            }
            return;
        };
        session.pending.insert(tool_call_id.clone(), tx);
    }
    set_approval(state, key, true);
    push_sorted(state, app);
    emit_event(
        app,
        AppEvent::PermissionAsked {
            session: key.clone(),
            permission,
        },
    );
    let decision = match rx.await {
        Ok(decision) => decision,
        Err(_) => {
            log::debug!("permission decision sender gone; treating as cancelled");
            PermissionDecision::Cancelled
        }
    };
    if let Some(mut guard) = lock_state(state)
        && let Some(session) = guard.sessions.get_mut(key)
    {
        session.pending.remove(&tool_call_id);
    }
    match decision {
        PermissionDecision::Selected(option_id) => {
            if responder
                .respond(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
                        acp::PermissionOptionId::new(option_id),
                    )),
                ))
                .is_err()
            {
                log::debug!("permission responder gone after allow");
            }
        }
        PermissionDecision::Cancelled => {
            if responder
                .respond(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ))
                .is_err()
            {
                log::debug!("permission responder gone after cancel");
            }
        }
    }
    set_approval(state, key, false);
    push_sorted(state, app);
    emit_event(
        app,
        AppEvent::PermissionResolved {
            session: key.clone(),
            tool_call_id,
        },
    );
}

fn handle_notification(
    state: &Mutex<State>,
    app: &AppHandle,
    key: &SessionKey,
    notification: &acp::SessionNotification,
) {
    let current = lock_state(state).and_then(|guard| guard.sessions.get(key)?.session_id.clone());
    if let Some(current) = current
        && notification.session_id.to_string() != current
    {
        log::debug!(
            "dropping notification for stale session {}",
            notification.session_id
        );
        return;
    }
    if let Some(chunk) = crate::updates::agent_text_of(&notification.update) {
        emit_event(
            app,
            AppEvent::AgentText {
                session: key.clone(),
                chunk,
            },
        );
    }
    match &notification.update {
        acp::SessionUpdate::ToolCall(call) => {
            let line = crate::updates::format_tool_line(call);
            emit_event(
                app,
                AppEvent::ToolLine {
                    session: key.clone(),
                    line,
                },
            );
            snoop_todos_from_call(state, app, key, call);
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            let line = crate::updates::format_tool_update(update);
            emit_event(
                app,
                AppEvent::ToolLine {
                    session: key.clone(),
                    line,
                },
            );
            snoop_todos_from_update(state, app, key, update);
        }
        acp::SessionUpdate::UsageUpdate(update) => {
            emit_event(
                app,
                AppEvent::SpendTick {
                    session: key.clone(),
                    cost: update.cost.as_ref().map(|cost| cost.amount).unwrap_or(0.0),
                    ctx_pct: context_pct(update.used, update.size),
                },
            );
        }
        acp::SessionUpdate::ConfigOptionUpdate(update) => {
            let options = config_views(&update.config_options);
            emit_event(
                app,
                AppEvent::ConfigOptions {
                    session: key.clone(),
                    options,
                },
            );
        }
        acp::SessionUpdate::AgentMessageChunk(_) => {}
        // Already streamed as agent text above. Known-but-unrendered kinds
        // are routine; an unknown kind means protocol drift.
        update => match crate::updates::update_kind(update) {
            Some(kind) => log::debug!("unhandled session update: {kind}"),
            None => log::warn!("unknown session update: {update:?}"),
        },
    }
}

/// Snoop a todo list off a tool call input, when it carries one.
fn snoop_todos_from_call(
    state: &Mutex<State>,
    app: &AppHandle,
    key: &SessionKey,
    call: &acp::ToolCall,
) {
    if let Some(fresh) = todos_from_call(call.raw_input.as_ref()) {
        log::debug!(
            "todo snoop call {} todos {}",
            call.tool_call_id,
            fresh.len()
        );
        update_todos(state, app, key, fresh);
    }
}

/// Snoop a todo list off a tool update, preferring the output over the input.
fn snoop_todos_from_update(
    state: &Mutex<State>,
    app: &AppHandle,
    key: &SessionKey,
    update: &acp::ToolCallUpdate,
) {
    if let Some(fresh) = todos_from_update(
        update.fields.raw_input.as_ref(),
        update.fields.raw_output.as_ref(),
    ) {
        log::debug!(
            "todo snoop update {} todos {}",
            update.tool_call_id,
            fresh.len()
        );
        update_todos(state, app, key, fresh);
    }
}

/// Replace the held list with a fresh todo list and emit when it moved.
/// Identical lists stay silent.
fn update_todos(state: &Mutex<State>, app: &AppHandle, key: &SessionKey, fresh: Vec<TodoView>) {
    let Some(mut guard) = lock_state(state) else {
        return;
    };
    let Some(session) = guard.sessions.get_mut(key) else {
        return;
    };
    if fresh == session.todos {
        return;
    }
    let changes = diff_todos(&session.todos, &fresh);
    session.todos = fresh.clone();
    drop(guard);
    emit_event(
        app,
        AppEvent::TodosChanged {
            session: key.clone(),
            todos: fresh,
            changes,
        },
    );
}

/// Select one agent by mode id. Fails loud: a planner session running as
/// the wrong agent would silently break write confinement.
async fn pin_agent_mode(
    connection: &ConnectionTo<Agent>,
    session_id: &acp::SessionId,
    mode: &str,
) -> Result<Vec<ConfigOptionView>, AgentError> {
    send_config_option(connection, session_id, "mode", mode)
        .await
        .map_err(|error| AgentError::RequestFailed {
            raw: format!("failed to select agent {mode}: {error}"),
        })
}

/// Agent-advertised option id currently filling a storage role.
fn option_id_for_role(
    options: &[ConfigOptionView],
    role: crate::repo_state::ConfigRole,
) -> Option<String> {
    options
        .iter()
        .find(|option| crate::repo_state::classify_option(option) == Some(role))
        .map(|option| option.id.clone())
}

/// Log value for a role, marking cleared roles. Pure.
fn role_label(value: Option<&str>) -> &str {
    value.unwrap_or("<cleared>")
}

/// One info-log for both roles after a select or reapply. Pure formatting,
/// logging edge.
fn log_roles(action: &str, roles: &crate::repo_state::RepoState) {
    log::info!(
        "{action} model={} effort={}",
        role_label(roles.model.as_deref()),
        role_label(roles.effort.as_deref())
    );
}

/// One `session/set_config_option` round trip. Callers decide how loud
/// the failure is.
async fn send_config_option(
    connection: &ConnectionTo<Agent>,
    session_id: &acp::SessionId,
    config_id: &str,
    value: &str,
) -> Result<Vec<ConfigOptionView>, String> {
    connection
        .send_request(acp::build_set_config_request(session_id, config_id, value))
        .block_task()
        .await
        .map(|response| config_views(&response.config_options))
        .map_err(|error| error.to_string())
}

/// Convert wire session config options to views.
pub fn config_views(options: &[SessionConfigOption]) -> Vec<ConfigOptionView> {
    options
        .iter()
        .map(|option| {
            let (current_value, values) = match &option.kind {
                SessionConfigKind::Select(select) => (
                    select.current_value.to_string(),
                    select_values(&select.options),
                ),
                SessionConfigKind::Boolean(boolean) => (
                    boolean.current_value.to_string(),
                    vec![
                        ConfigOptionValueView {
                            value: "true".to_string(),
                            name: "true".to_string(),
                        },
                        ConfigOptionValueView {
                            value: "false".to_string(),
                            name: "false".to_string(),
                        },
                    ],
                ),
                _ => (String::new(), Vec::new()),
            };
            ConfigOptionView {
                id: option.id.to_string(),
                name: option.name.clone(),
                current_value,
                options: values,
                category: option_category(&option.category),
            }
        })
        .collect()
}

/// Spec `category` as a plain string, absent when the agent omits it.
fn option_category(category: &Option<SessionConfigOptionCategory>) -> Option<String> {
    match category {
        None => None,
        Some(SessionConfigOptionCategory::Mode) => Some("mode".to_string()),
        Some(SessionConfigOptionCategory::Model) => Some("model".to_string()),
        Some(SessionConfigOptionCategory::ModelConfig) => Some("model_config".to_string()),
        Some(SessionConfigOptionCategory::ThoughtLevel) => Some("thought_level".to_string()),
        Some(SessionConfigOptionCategory::Other(name)) => Some(name.clone()),
        // Future SDK variants surface as absent.
        Some(_) => None,
    }
}

fn select_values(options: &SessionConfigSelectOptions) -> Vec<ConfigOptionValueView> {
    match options {
        SessionConfigSelectOptions::Ungrouped(values) => values
            .iter()
            .map(|value| ConfigOptionValueView {
                value: value.value.to_string(),
                name: value.name.clone(),
            })
            .collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| {
                group.options.iter().map(|value| ConfigOptionValueView {
                    value: value.value.to_string(),
                    name: value.name.clone(),
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

// Spawn failures arrive as strings through the ready channel, so missing
// binaries are classified by matching the process output text.
fn map_startup_error(raw: &str) -> AgentError {
    let lowered = raw.to_lowercase();
    if lowered.contains("no such file")
        || (lowered.contains("not found") && lowered.contains("opencode"))
    {
        return AgentError::MissingBinary;
    }
    AgentError::AgentExited {
        raw: raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{
        SessionConfigOptionCategory, SessionConfigSelectOption, SessionConfigValueId,
    };
    use std::collections::HashMap;
    use std::path::Path;

    #[test]
    fn config_views_keep_agent_ordering() {
        let option = SessionConfigOption::select(
            "model",
            "Model",
            SessionConfigValueId::new("opencode/big-pickle"),
            vec![SessionConfigSelectOption::new(
                SessionConfigValueId::new("opencode/big-pickle"),
                "Big Pickle",
            )],
        )
        .category(SessionConfigOptionCategory::Model);
        let views = config_views(std::slice::from_ref(&option));
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, "model");
        assert_eq!(views[0].category.as_deref(), Some("model"));
        assert_eq!(views[0].current_value, "opencode/big-pickle");
        assert_eq!(views[0].options.len(), 1);
    }

    fn select_fixture(
        id: &'static str,
        name: &'static str,
        category: impl Into<Option<SessionConfigOptionCategory>>,
    ) -> SessionConfigOption {
        let category: Option<SessionConfigOptionCategory> = category.into();
        SessionConfigOption::select(
            id,
            name,
            SessionConfigValueId::new("v"),
            vec![SessionConfigSelectOption::new(
                SessionConfigValueId::new("v"),
                "V",
            )],
        )
        .category(category)
    }

    #[test]
    fn config_views_maps_categories() {
        let options = vec![
            select_fixture("llm", "LLM", SessionConfigOptionCategory::Model),
            select_fixture("mode", "Session Mode", SessionConfigOptionCategory::Mode),
            select_fixture(
                "effort",
                "Effort",
                SessionConfigOptionCategory::ThoughtLevel,
            ),
            select_fixture("ctx", "Context", SessionConfigOptionCategory::ModelConfig),
            select_fixture(
                "custom",
                "Custom",
                SessionConfigOptionCategory::Other("_custom".to_string()),
            ),
            select_fixture(
                "legacy",
                "Legacy",
                Option::<SessionConfigOptionCategory>::None,
            ),
        ];
        let views = config_views(&options);
        let categories: Vec<Option<&str>> =
            views.iter().map(|view| view.category.as_deref()).collect();
        assert_eq!(
            categories,
            [
                Some("model"),
                Some("mode"),
                Some("thought_level"),
                Some("model_config"),
                Some("_custom"),
                None,
            ]
        );
    }

    fn test_key() -> SessionKey {
        SessionKey {
            plan: "2026-09-25.10-54-59".to_string(),
            role: SessionRole::Scoping,
        }
    }

    fn state_with_session() -> Mutex<State> {
        let mut state = State::default();
        let key = test_key();
        state.sessions.insert(
            key.clone(),
            LiveSession::fresh(
                ActivePlan::scoping("2026-09-25.10-54-59".to_string()),
                crate::repo_state::RepoState::default(),
            ),
        );
        state.current = Some(key);
        Mutex::new(state)
    }

    #[test]
    fn working_releases_awake_guard() {
        let state = state_with_session();
        let key = test_key();
        set_working(&state, &key, true);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.sessions.get(&key).expect("session").working);
        }
        set_working(&state, &key, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(!guard.sessions.get(&key).expect("session").working);
            assert!(guard.awake.is_none());
        }
    }

    #[test]
    fn working_holds_guard_while_any_session_runs() {
        let state = Mutex::new(State::default());
        let first = SessionKey {
            plan: "a".to_string(),
            role: SessionRole::Scoping,
        };
        let second = SessionKey {
            plan: "b".to_string(),
            role: SessionRole::Scoping,
        };
        {
            let mut guard = state.lock().expect("state poisoned");
            guard.sessions.insert(
                first.clone(),
                LiveSession::fresh(
                    ActivePlan::scoping("a".to_string()),
                    crate::repo_state::RepoState::default(),
                ),
            );
            guard.sessions.insert(
                second.clone(),
                LiveSession::fresh(
                    ActivePlan::scoping("b".to_string()),
                    crate::repo_state::RepoState::default(),
                ),
            );
        }
        set_working(&state, &first, true);
        set_working(&state, &second, true);
        set_working(&state, &first, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.awake.is_some());
        }
        set_working(&state, &second, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.awake.is_none());
        }
    }

    #[test]
    fn failed_flag_tracks_turn_outcome() {
        let state = state_with_session();
        let key = test_key();
        set_failed(&state, &key, true);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.sessions.get(&key).expect("session").failed);
        }
        set_failed(&state, &key, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(!guard.sessions.get(&key).expect("session").failed);
        }
    }

    #[test]
    fn idle_gate_is_per_session() {
        let state = Mutex::new(State::default());
        let first = SessionKey {
            plan: "a".to_string(),
            role: SessionRole::Scoping,
        };
        let second = SessionKey {
            plan: "b".to_string(),
            role: SessionRole::Scoping,
        };
        {
            let mut guard = state.lock().expect("state poisoned");
            for (key, name) in [(&first, "a"), (&second, "b")] {
                guard.sessions.insert(
                    key.clone(),
                    LiveSession::fresh(
                        ActivePlan::scoping(name.to_string()),
                        crate::repo_state::RepoState::default(),
                    ),
                );
            }
        }
        set_working(&state, &first, true);
        assert!(ensure_idle(&state, &first).is_err());
        assert!(ensure_idle(&state, &second).is_ok());
    }

    async fn open_test_session(
        cwd: &Path,
        env: HashMap<String, String>,
    ) -> Result<String, AgentError> {
        let binary = acp::resolve_opencode_binary()?;
        let config = AcpAgentConfig::new(binary).arg("acp").envs(env);
        let agent = AcpAgent::new(config);
        let cwd_owned = cwd.to_path_buf();
        Client
            .builder()
            .on_receive_notification(
                async move |_notification: acp::SessionNotification, _cx| Ok(()),
                acp::on_receive_notification!(),
            )
            .on_receive_request(
                async move |_request: acp::RequestPermissionRequest, responder, _connection| {
                    responder.respond(acp::RequestPermissionResponse::new(
                        acp::RequestPermissionOutcome::Cancelled,
                    ))
                },
                acp::on_receive_request!(),
            )
            .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
                connection
                    .send_request(acp::build_initialize_request())
                    .block_task()
                    .await
                    .map_err(|error| acp::internal_error(error.to_string()))?;
                let response = connection
                    .send_request(acp::build_new_session_request(&cwd_owned))
                    .block_task()
                    .await
                    .map_err(|error| acp::internal_error(error.to_string()))?;
                Ok(response.session_id.to_string())
            })
            .await
            .map_err(|error| AgentError::RequestFailed {
                raw: error.to_string(),
            })
    }

    #[tokio::test]
    async fn lifecycle_opens_session_in_temp_git_repo() {
        if crate::acp::resolve_opencode_binary().is_err() {
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "test"],
            vec!["commit", "--allow-empty", "-m", "init"],
        ] {
            let output = std::process::Command::new("git")
                .args(&args)
                .current_dir(dir.path())
                .output()
                .expect("git");
            assert!(output.status.success(), "{args:?}");
        }
        let scratch = tempfile::tempdir().expect("scratch");
        let mut env = HashMap::new();
        env.insert(
            "XDG_DATA_HOME".to_string(),
            scratch.path().join("data").to_string_lossy().to_string(),
        );
        env.insert(
            "XDG_CONFIG_HOME".to_string(),
            scratch.path().join("config").to_string_lossy().to_string(),
        );
        let session_id = open_test_session(dir.path(), env)
            .await
            .expect("initialize plus session/new");
        assert!(!session_id.is_empty());
    }
}
