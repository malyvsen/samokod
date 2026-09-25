// Agent lifecycle: spawn `opencode acp` once, initialize it, open sessions
// bound to repository roots, and stream prompt turns as events.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
    AgentError, AppEvent, ConfigOptionValueView, ConfigOptionView, PermissionView, PlanInfo,
    SessionInfo, TodoView,
};

/// User decision for one permission card.
#[derive(Debug, Clone)]
enum PermissionDecision {
    Selected(String),
    Cancelled,
}

/// Plan owned by the current chat. One chat owns one plan.
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

    fn plan_ref(&self) -> plans::PlanRef {
        plans::PlanRef {
            name: self.name.clone(),
            phase: self.phase,
        }
    }

    fn is_scoping(&self) -> bool {
        self.phase == plans::Phase::Scoping
    }

    fn info(&self, repo_root: &Path) -> PlanInfo {
        PlanInfo::of(repo_root, &self.plan_ref())
    }
}

#[derive(Default)]
struct State {
    connection: Option<ConnectionTo<Agent>>,
    branch_watch: Option<notify::RecommendedWatcher>,
    supports_close: bool,
    session_id: Option<String>,
    repo_root: Option<PathBuf>,
    branch: String,
    working: bool,
    awake: Option<awake::Guard>,
    last_prompt: Option<String>,
    last_roles: crate::repo_state::RepoState,
    pending: HashMap<String, tokio::sync::oneshot::Sender<PermissionDecision>>,
    todos: Vec<TodoView>,
    plan: Option<ActivePlan>,
}

impl State {
    /// Working and the sleep lock move together.
    fn set_working(&mut self, working: bool) {
        self.working = working;
        if working {
            if self.awake.is_none() {
                self.awake = awake::acquire();
            }
        } else {
            self.awake = None;
        }
    }

    /// Reset every session-scoped field, keeping the agent connection.
    /// The new plan arrives with the session: one chat owns one plan.
    fn reset_session(
        &mut self,
        session_id: String,
        repo_root: PathBuf,
        branch: String,
        roles: crate::repo_state::RepoState,
        plan: ActivePlan,
    ) {
        self.session_id = Some(session_id);
        self.repo_root = Some(repo_root);
        self.branch = branch;
        self.set_working(false);
        self.last_prompt = None;
        self.last_roles = roles;
        self.pending.clear();
        self.todos.clear();
        self.plan = Some(plan);
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

    /// Open a repository: ensure the plan structure, abandon any active
    /// plan, spawn a fresh planner process scoped to a new scoping plan,
    /// and open a session in planner mode with stored model+effort reapplied.
    pub async fn open_repo(
        &self,
        repo_root: PathBuf,
        branch: String,
        stored: crate::repo_state::RepoState,
    ) -> Result<SessionInfo, AgentError> {
        plans::ensure_structure(&repo_root)?;
        self.abandon_stored_plan()?;
        self.shutdown_connection().await;
        let scoping = plans::create_scoping(&repo_root)?;
        let plan = ActivePlan::scoping(scoping.name);
        let agent = opencode::agent_for(plan.phase);
        let (_, _, info) = self
            .spawn_session(&repo_root, &branch, stored, plan, agent)
            .await?;
        self.watch_branch(&repo_root);
        Ok(info)
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
    pub async fn set_config_option(
        &self,
        config_id: String,
        value: String,
    ) -> Result<Vec<ConfigOptionView>, AgentError> {
        let (connection, session_id, _) =
            self.session_snapshot()
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
                let moved: Vec<crate::repo_state::ConfigRole> = crate::repo_state::ConfigRole::ALL
                    .into_iter()
                    .filter(|role| role.get(&state.last_roles) != role.get(&roles))
                    .collect();
                state.last_roles = roles.clone();
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
    /// no user bubble, the chat opens working.
    pub async fn execute_plan(&self) -> Result<SessionInfo, AgentError> {
        ensure_idle(&self.state)?;
        let (repo_root, branch, stored) =
            self.reopen_snapshot()
                .ok_or_else(|| AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                })?;
        let active = self
            .plan_snapshot()
            .map(|(_, plan)| plan)
            .filter(|plan| plan.phase == plans::Phase::Scoping)
            .ok_or_else(|| AgentError::RequestFailed {
                raw: "no scoping plan to execute".to_string(),
            })?;
        let next = plans::execute(&repo_root, &active.plan_ref())?;
        let plan = ActivePlan::executing(next.name.clone());
        // Record the move before any fallible spawn: disk and state agree
        // from here on, and a failed spawn recovers via auto-abandon.
        self.set_plan(plan.clone());
        self.shutdown_connection().await;
        let agent = opencode::agent_for(plan.phase);
        let (connection, session_id, info) = self
            .spawn_session(&repo_root, &branch, stored, plan, agent)
            .await?;
        let text = opencode::executor_first_message(&opencode::plan_display(&next));
        self.start_turn(connection, session_id, text, None).await?;
        Ok(info)
    }

    /// Finish the executing plan, then open a fresh scoping chat; the
    /// completed plan stays on disk.
    pub async fn mark_completed(&self) -> Result<SessionInfo, AgentError> {
        ensure_idle(&self.state)?;
        let (repo_root, branch, stored) =
            self.reopen_snapshot()
                .ok_or_else(|| AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                })?;
        let active = self
            .plan_snapshot()
            .map(|(_, plan)| plan)
            .filter(|plan| plan.phase == plans::Phase::Executing)
            .ok_or_else(|| AgentError::RequestFailed {
                raw: "no executing plan to complete".to_string(),
            })?;
        plans::complete(&repo_root, &active.plan_ref())?;
        let scoping = plans::create_scoping(&repo_root)?;
        let plan = ActivePlan::scoping(scoping.name.clone());
        self.set_plan(plan.clone());
        self.shutdown_connection().await;
        let agent = opencode::agent_for(plan.phase);
        let (_, _, info) = self
            .spawn_session(&repo_root, &branch, stored, plan, agent)
            .await?;
        Ok(info)
    }

    /// Drop an active plan, then open a fresh scoping chat.
    pub async fn abandon_plan(&self) -> Result<SessionInfo, AgentError> {
        ensure_idle(&self.state)?;
        let (repo_root, branch, stored) =
            self.reopen_snapshot()
                .ok_or_else(|| AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                })?;
        let active = self
            .plan_snapshot()
            .map(|(_, plan)| plan)
            .filter(|plan| plan.phase.is_active())
            .ok_or_else(|| AgentError::RequestFailed {
                raw: "no active plan to abandon".to_string(),
            })?;
        plans::abandon(&repo_root, &active.plan_ref())?;
        let scoping = plans::create_scoping(&repo_root)?;
        let plan = ActivePlan::scoping(scoping.name.clone());
        self.set_plan(plan.clone());
        self.shutdown_connection().await;
        let agent = opencode::agent_for(plan.phase);
        let (_, _, info) = self
            .spawn_session(&repo_root, &branch, stored, plan, agent)
            .await?;
        Ok(info)
    }

    /// Spawn a fresh agent process scoped to one plan and open a session on
    /// it: pin the agent mode, reapply stored model then effort
    /// (effort-last, since a model switch can reshape effort options), and
    /// report the session. Shared by every session opener.
    async fn spawn_session(
        &self,
        repo_root: &Path,
        branch: &str,
        stored: crate::repo_state::RepoState,
        plan: ActivePlan,
        agent: &str,
    ) -> Result<(ConnectionTo<Agent>, String, SessionInfo), AgentError> {
        let scope = opencode::scope_glob(repo_root, plan.phase, &plan.name);
        let connection = self
            .ensure_connection_with_env(opencode::agent_env(&scope))
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
        let info = {
            let mut state = self.state.lock().expect("state poisoned");
            let view = plan.info(repo_root);
            state.reset_session(
                session_id.clone(),
                repo_root.to_path_buf(),
                branch.to_string(),
                applied,
                plan,
            );
            session_info(&session_id, repo_root, branch, options, view)
        };
        emit_event(&self.app, AppEvent::SessionReset);
        Ok((connection, session_id, info))
    }

    /// Record the held plan. A poisoned lock only logs, matching
    /// `lock_state`.
    fn set_plan(&self, plan: ActivePlan) {
        match self.state.lock() {
            Ok(mut state) => {
                state.plan = Some(plan);
            }
            Err(error) => {
                log::warn!("failed to record plan transition: {error}");
            }
        }
    }

    /// Move the stored plan to cancelled. No-op without one or when its
    /// directory is already gone, so a retried open never bricks.
    fn abandon_stored_plan(&self) -> Result<(), AgentError> {
        let Some((repo_root, active)) = self.plan_snapshot() else {
            return Ok(());
        };
        let plan = active.plan_ref();
        if !plan.phase.is_active() {
            return Ok(());
        }
        if !plan.path(&repo_root).exists() {
            return Ok(());
        }
        plans::abandon(&repo_root, &plan)?;
        Ok(())
    }

    /// Current repository root, when a session is open.
    pub fn current_repo(&self) -> Option<PathBuf> {
        lock_state(&self.state)?.repo_root.clone()
    }

    /// Resend the last prompt. When the transport is closed, reopen the
    /// session on the held plan first. Returns false when nothing ran.
    pub async fn retry_last(&self) -> Result<bool, AgentError> {
        let Some(text) = lock_state(&self.state).and_then(|state| state.last_prompt.clone()) else {
            return Ok(false);
        };
        let (connection, session_id) = match self
            .session_snapshot()
            .filter(|(connection, _, _)| !connection.is_incoming_closed())
        {
            Some((connection, session_id, _)) => (connection, session_id),
            None => self.reopen_session().await?,
        };
        let watch = Self::scoping_watch(&self.plan_snapshot());
        self.start_turn(connection, session_id, text, watch).await?;
        Ok(true)
    }

    /// Send one plain-text prompt. Streams arrive as events; the turn end
    /// arrives as done or failed. The planner role prefixes the user's
    /// first scoping message only; the transcript keeps the raw text.
    pub async fn send_prompt(&self, text: String) -> Result<(), AgentError> {
        let (connection, session_id, _) =
            self.session_snapshot()
                .ok_or_else(|| AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                })?;
        let watch = Self::scoping_watch(&self.plan_snapshot());
        let mut text = text;
        if let Some(plan) = self.claim_planner_prefix() {
            text = opencode::planner_first_message(&opencode::plan_display(&plan), &text);
        }
        self.start_turn(connection, session_id, text, watch).await
    }

    /// Plan watch for scoping turns: re-emit `plan.md` presence when the
    /// turn lands. Pure.
    fn scoping_watch(
        snapshot: &Option<(PathBuf, ActivePlan)>,
    ) -> Option<(PathBuf, plans::PlanRef)> {
        let (repo_root, plan) = snapshot.as_ref().filter(|(_, plan)| plan.is_scoping())?;
        Some((repo_root.clone(), plan.plan_ref()))
    }

    /// Claim the one-time planner prefix for the first scoping message.
    /// One locked check-and-mark, so a retried turn never prefixes twice.
    fn claim_planner_prefix(&self) -> Option<plans::PlanRef> {
        let mut state = lock_state(&self.state)?;
        let plan = state
            .plan
            .as_mut()
            .filter(|plan| plan.is_scoping() && !plan.prefixed)?;
        plan.prefixed = true;
        Some(plan.plan_ref())
    }

    /// Reopen the held plan in a fresh session after transport death: same
    /// plan, same dir, no flag changes. Retry path only.
    async fn reopen_session(&self) -> Result<(ConnectionTo<Agent>, String), AgentError> {
        let (repo_root, branch, stored, plan) = {
            let state = lock_state(&self.state).ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
            let repo_root = state
                .repo_root
                .clone()
                .ok_or_else(|| AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                })?;
            let plan = state.plan.clone().ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
            (
                repo_root,
                state.branch.clone(),
                state.last_roles.clone(),
                plan,
            )
        };
        self.shutdown_connection().await;
        let agent = opencode::agent_for(plan.phase);
        let (connection, session_id, _) = self
            .spawn_session(&repo_root, &branch, stored, plan, agent)
            .await?;
        Ok((connection, session_id))
    }

    /// Guard one turn and stream it as events. Records the prompt so retry
    /// resends exactly what ran, prefixed or not.
    async fn start_turn(
        &self,
        connection: ConnectionTo<Agent>,
        session_id: String,
        text: String,
        watch: Option<(PathBuf, plans::PlanRef)>,
    ) -> Result<(), AgentError> {
        {
            let mut state = self.state.lock().expect("state poisoned");
            if state.working {
                return Err(AgentError::RequestFailed {
                    raw: "a turn is already running".to_string(),
                });
            }
            state.set_working(true);
            state.last_prompt = Some(text.clone());
        }
        let state = Arc::clone(&self.state);
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let prompt = acp::PromptRequest::new(
                acp::SessionId::new(session_id),
                vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
            );
            match connection.send_request(prompt).block_task().await {
                Ok(_) => {
                    set_working(&state, false);
                    emit_event(&app, AppEvent::TurnDone);
                    if let Some((repo_root, plan)) = watch {
                        emit_event(
                            &app,
                            AppEvent::PlanChanged {
                                plan: PlanInfo::of(&repo_root, &plan),
                            },
                        );
                    }
                }
                Err(error) => {
                    let raw = error.to_string();
                    let transport_gone = is_transport_error(&raw);
                    set_working(&state, false);
                    if transport_gone {
                        match state.lock() {
                            Ok(mut guard) => {
                                guard.connection = None;
                                guard.session_id = None;
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
                            raw,
                            hint: hint.text,
                            retryable: hint.retryable,
                        },
                    );
                }
            }
        });
        Ok(())
    }

    /// Stop the turn via `session/cancel`, answering every open card as
    /// cancelled per protocol.
    pub async fn cancel_turn(&self) -> Result<(), AgentError> {
        let (connection, session_id, _) =
            self.session_snapshot()
                .ok_or_else(|| AgentError::NoSession {
                    raw: "no active turn".to_string(),
                })?;
        cancel_pending(&self.state);
        if let Err(error) = connection.send_notification(acp::build_cancel_notification(
            acp::SessionId::new(session_id),
        )) {
            log::warn!("failed to send session cancel: {error}");
        }
        set_working(&self.state, false);
        Ok(())
    }

    /// Answer one permission card. `Some` selects the allow-once or reject
    /// option; `None` answers cancelled.
    pub fn answer_permission(
        &self,
        tool_call_id: &str,
        option_id: Option<String>,
    ) -> Result<(), AgentError> {
        let sender = self
            .state
            .lock()
            .map_err(|_| AgentError::RequestFailed {
                raw: "permission state poisoned".to_string(),
            })?
            .pending
            .remove(tool_call_id);
        match sender {
            Some(sender) => {
                let decision = match option_id {
                    Some(id) => PermissionDecision::Selected(id),
                    None => PermissionDecision::Cancelled,
                };
                if sender.send(decision).is_err() {
                    log::debug!("permission decision receiver gone for {tool_call_id}");
                }
                Ok(())
            }
            None => Err(AgentError::RequestFailed {
                raw: format!("no pending permission for {tool_call_id}"),
            }),
        }
    }

    /// Drop the live connection so the next ensure spawns a fresh process.
    /// Pending permission cards resolve as cancelled; the old child exits
    /// when its stdio closes. Best-effort closes the old session first.
    async fn shutdown_connection(&self) {
        let snapshot = self.session_snapshot();
        match self.state.lock() {
            Ok(mut state) => {
                state.connection = None;
                state.session_id = None;
                state.branch_watch = None;
            }
            Err(error) => {
                log::warn!("failed to drop agent connection: {error}");
            }
        }
        cancel_pending(&self.state);
        if let Some((connection, id, supports_close)) = snapshot
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

    /// Ensure a live, initialized connection. Spawns a fresh `opencode acp`
    /// process per chat, reuses it for later turns, and respawns after a
    /// closed transport. `extra_env` (agent definitions) merges over the
    /// process environment.
    async fn ensure_connection_with_env(
        &self,
        extra_env: HashMap<String, String>,
    ) -> Result<ConnectionTo<Agent>, AgentError> {
        if let Some(connection) = self.connection_snapshot() {
            if !connection.is_incoming_closed() {
                return Ok(connection);
            }
            match self.state.lock() {
                Ok(mut state) => {
                    state.connection = None;
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
        let ask_state = Arc::clone(&self.state);
        let ask_app = self.app.clone();
        let exit_app = self.app.clone();
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
                        async move {
                            handle_notification(&state, &app, &notification);
                            Ok(())
                        }
                    },
                    agent_client_protocol::on_receive_notification!(),
                )
                .on_receive_request(
                    move |request: acp::RequestPermissionRequest, responder, _cx| {
                        let state = Arc::clone(&ask_state);
                        let app = ask_app.clone();
                        async move {
                            handle_permission_request(&state, &app, request, responder).await;
                            Ok(())
                        }
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .connect_with(agent, |connection: ConnectionTo<Agent>| {
                    let slot = Arc::clone(&slot);
                    let ready = Arc::clone(&ready_for_handler);
                    async move {
                        let init_response = connection
                            .send_request(acp::build_initialize_request())
                            .block_task()
                            .await
                            .map_err(|error| acp::internal_error(error.to_string()))?;
                        {
                            let mut state = slot.lock().expect("state poisoned");
                            state.connection = Some(connection.clone());
                            state.supports_close = init_response
                                .agent_capabilities
                                .session_capabilities
                                .close
                                .is_some();
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
                    emit_event(
                        &exit_app,
                        AppEvent::AgentExited {
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
            Ok(Ok(())) => self
                .connection_snapshot()
                .ok_or_else(|| AgentError::RequestFailed {
                    raw: "agent connection vanished during initialize".to_string(),
                }),
            Ok(Err(raw)) => Err(map_startup_error(&raw)),
            Err(_) => Err(AgentError::RequestFailed {
                raw: "agent startup was cancelled".to_string(),
            }),
        }
    }

    fn reopen_snapshot(&self) -> Option<(PathBuf, String, crate::repo_state::RepoState)> {
        let state = lock_state(&self.state)?;
        Some((
            state.repo_root.clone()?,
            state.branch.clone(),
            state.last_roles.clone(),
        ))
    }

    fn connection_snapshot(&self) -> Option<ConnectionTo<Agent>> {
        lock_state(&self.state).and_then(|state| state.connection.clone())
    }

    fn session_snapshot(&self) -> Option<(ConnectionTo<Agent>, String, bool)> {
        let state = lock_state(&self.state)?;
        Some((
            state.connection.clone()?,
            state.session_id.clone()?,
            state.supports_close,
        ))
    }

    /// Current repo plus owned plan, when a chat is open.
    fn plan_snapshot(&self) -> Option<(PathBuf, ActivePlan)> {
        let state = lock_state(&self.state)?;
        Some((state.repo_root.clone()?, state.plan.clone()?))
    }
}

/// Build the frontend session payload with its plan. Pure.
fn session_info(
    session_id: &str,
    repo_root: &Path,
    branch: &str,
    options: Vec<ConfigOptionView>,
    plan: PlanInfo,
) -> SessionInfo {
    SessionInfo {
        session_id: session_id.to_string(),
        repo_root: repo_root.to_string_lossy().to_string(),
        branch: branch.to_string(),
        config_options: options,
        plan,
    }
}

/// Refuse plan transitions while a turn runs. A poisoned lock only logs,
/// matching `lock_state`.
fn ensure_idle(state: &Mutex<State>) -> Result<(), AgentError> {
    match state.lock() {
        Ok(state) if state.working => Err(AgentError::RequestFailed {
            raw: "a turn is already running".to_string(),
        }),
        Ok(_) => Ok(()),
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

fn set_working(state: &Mutex<State>, working: bool) {
    let Some(mut guard) = lock_state(state) else {
        return;
    };
    guard.set_working(working);
}

fn cancel_pending(state: &Mutex<State>) {
    let senders: Vec<tokio::sync::oneshot::Sender<PermissionDecision>> = match state.lock() {
        Ok(mut guard) => guard.pending.drain().map(|(_, sender)| sender).collect(),
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
    request: acp::RequestPermissionRequest,
    responder: agent_client_protocol::Responder<acp::RequestPermissionResponse>,
) {
    let current = lock_state(state).and_then(|guard| guard.session_id.clone());
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
        guard.pending.insert(tool_call_id.clone(), tx);
    }
    emit_event(app, AppEvent::PermissionAsked { permission });
    let decision = match rx.await {
        Ok(decision) => decision,
        Err(_) => {
            log::debug!("permission decision sender gone; treating as cancelled");
            PermissionDecision::Cancelled
        }
    };
    if let Some(mut guard) = lock_state(state) {
        guard.pending.remove(&tool_call_id);
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
    emit_event(app, AppEvent::PermissionResolved { tool_call_id });
}

fn handle_notification(
    state: &Mutex<State>,
    app: &AppHandle,
    notification: &acp::SessionNotification,
) {
    let current = lock_state(state).and_then(|guard| guard.session_id.clone());
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
        emit_event(app, AppEvent::AgentText { chunk });
    }
    match &notification.update {
        acp::SessionUpdate::ToolCall(call) => {
            let line = crate::updates::format_tool_line(call);
            emit_event(app, AppEvent::ToolLine { line });
            snoop_todos_from_call(state, app, call);
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            let line = crate::updates::format_tool_update(update);
            emit_event(app, AppEvent::ToolLine { line });
            snoop_todos_from_update(state, app, update);
        }
        acp::SessionUpdate::UsageUpdate(update) => {
            emit_event(app, spend_tick(update));
        }
        acp::SessionUpdate::ConfigOptionUpdate(update) => {
            let options = config_views(&update.config_options);
            emit_event(app, AppEvent::ConfigOptions { options });
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
fn snoop_todos_from_call(state: &Mutex<State>, app: &AppHandle, call: &acp::ToolCall) {
    if let Some(fresh) = todos_from_call(call.raw_input.as_ref()) {
        log::debug!(
            "todo snoop call {} todos {}",
            call.tool_call_id,
            fresh.len()
        );
        update_todos(state, app, fresh);
    }
}

/// Snoop a todo list off a tool update, preferring the output over the input.
fn snoop_todos_from_update(state: &Mutex<State>, app: &AppHandle, update: &acp::ToolCallUpdate) {
    if let Some(fresh) = todos_from_update(
        update.fields.raw_input.as_ref(),
        update.fields.raw_output.as_ref(),
    ) {
        log::debug!(
            "todo snoop update {} todos {}",
            update.tool_call_id,
            fresh.len()
        );
        update_todos(state, app, fresh);
    }
}

/// Replace the held list with a fresh todo list and emit when it moved.
/// Identical lists stay silent.
fn update_todos(state: &Mutex<State>, app: &AppHandle, fresh: Vec<TodoView>) {
    let Some(mut guard) = lock_state(state) else {
        return;
    };
    if fresh == guard.todos {
        return;
    }
    let changes = diff_todos(&guard.todos, &fresh);
    guard.todos = fresh.clone();
    drop(guard);
    emit_event(
        app,
        AppEvent::TodosChanged {
            todos: fresh,
            changes,
        },
    );
}

/// Spend tick from a `usage_update`. Pure.
fn spend_tick(update: &acp::UsageUpdate) -> AppEvent {
    AppEvent::SpendTick {
        cost: update.cost.as_ref().map(|cost| cost.amount).unwrap_or(0.0),
        ctx_pct: context_pct(update.used, update.size),
    }
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

    #[test]
    fn working_releases_awake_guard() {
        let state = Mutex::new(State::default());
        set_working(&state, true);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.working);
        }
        set_working(&state, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(!guard.working);
            assert!(guard.awake.is_none());
        }
    }

    #[test]
    fn reset_session_releases_awake_guard() {
        let mut state = State {
            working: true,
            awake: awake::acquire(),
            ..Default::default()
        };
        state.reset_session(
            "session".to_string(),
            PathBuf::from("/tmp"),
            "main".to_string(),
            crate::repo_state::RepoState::default(),
            ActivePlan::scoping("2026-09-25.10-54-59".to_string()),
        );
        assert!(!state.working);
        assert!(state.awake.is_none());
        assert!(state.plan.is_some());
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
