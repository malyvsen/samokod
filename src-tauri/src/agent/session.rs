// Live sessions: one `opencode acp` child process per session, keyed by
// plan directory name plus role. Spawning is lazy on the first prompt,
// except the eager executing start after approval.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::acp::{self, AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo};
use crate::error_hint::classify_error;
use crate::opencode;
use crate::plans;
use crate::types::{AgentError, AppEvent, SessionKey, SessionRole};

use super::AgentManager;
use super::config::{
    config_views, log_roles, option_id_for_role, pin_agent_mode, send_config_option,
};
use super::permissions::handle_permission_request;
use super::plans_list::push_sorted;
use super::turns::handle_notification;
use super::{emit_event, lock_state, map_startup_error, set_failed};

/// User decision for one permission card.
#[derive(Debug, Clone)]
pub(crate) enum PermissionDecision {
    Selected(String),
    Cancelled,
}

/// Role phases: sessions only run on active plans.
pub(crate) fn role_phase(role: SessionRole) -> plans::Phase {
    match role {
        SessionRole::Scoping => plans::Phase::Scoping,
        SessionRole::Executing => plans::Phase::Executing,
        SessionRole::Landing => plans::Phase::Landing,
    }
}

/// Plan owned by one live session.
#[derive(Debug, Clone)]
pub(crate) struct ActivePlan {
    pub(crate) name: String,
    pub(crate) phase: plans::Phase,
    /// Role already delivered for this ACP conversation. Executing and
    /// landing send once, hidden or prefixed to the first prompt;
    /// scoping prefixes its template server-side to the first prompt.
    pub(crate) prefixed: bool,
}

impl ActivePlan {
    pub(crate) fn scoping(name: String) -> Self {
        ActivePlan {
            name,
            phase: plans::Phase::Scoping,
            prefixed: false,
        }
    }

    pub(crate) fn executing(name: String) -> Self {
        ActivePlan {
            name,
            phase: plans::Phase::Executing,
            prefixed: false,
        }
    }

    pub(crate) fn landing(name: String) -> Self {
        ActivePlan {
            name,
            phase: plans::Phase::Landing,
            prefixed: false,
        }
    }

    /// Plan for one session key: the role decides the phase. Pure.
    pub(crate) fn for_session(session: &SessionKey) -> Self {
        match session.role {
            SessionRole::Scoping => ActivePlan::scoping(session.plan.clone()),
            SessionRole::Executing => ActivePlan::executing(session.plan.clone()),
            SessionRole::Landing => ActivePlan::landing(session.plan.clone()),
        }
    }

    pub(crate) fn key(&self) -> Option<SessionKey> {
        let role = match self.phase {
            plans::Phase::Scoping => SessionRole::Scoping,
            plans::Phase::Executing => SessionRole::Executing,
            plans::Phase::Landing => SessionRole::Landing,
            plans::Phase::Completed | plans::Phase::Cancelled => return None,
        };
        Some(SessionKey {
            plan: self.name.clone(),
            role,
        })
    }

    /// Working directory for the session: the plan worktree for executing
    /// and landing plans, the main root for scoping plans. Plan files
    /// always stay in the main checkout; only the agent's cwd moves.
    pub(crate) fn cwd(&self, repo_root: &Path) -> PathBuf {
        match self.phase {
            plans::Phase::Executing | plans::Phase::Landing => {
                crate::worktrees::worktree_path(repo_root, &self.name)
            }
            plans::Phase::Scoping | plans::Phase::Completed | plans::Phase::Cancelled => {
                repo_root.to_path_buf()
            }
        }
    }

    pub(crate) fn plan_ref(&self) -> plans::PlanRef {
        plans::PlanRef {
            name: self.name.clone(),
            phase: self.phase,
        }
    }
}

/// One live agent session: its own `opencode acp` process, connection, ACP
/// session id, and turn state.
pub(crate) struct LiveSession {
    pub(crate) connection: Option<ConnectionTo<Agent>>,
    pub(crate) supports_close: bool,
    pub(crate) session_id: Option<String>,
    pub(crate) working: bool,
    pub(crate) approval: bool,
    pub(crate) failed: bool,
    pub(crate) last_prompt: Option<String>,
    pub(crate) last_roles: crate::repo_state::RepoState,
    pub(crate) pending: HashMap<String, tokio::sync::oneshot::Sender<PermissionDecision>>,
    pub(crate) todos: Vec<crate::types::TodoView>,
    pub(crate) tool_calls: HashMap<String, crate::acp::ToolCall>,
    pub(crate) plan: ActivePlan,
}

impl LiveSession {
    pub(crate) fn fresh(plan: ActivePlan, roles: crate::repo_state::RepoState) -> Self {
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
            tool_calls: HashMap::new(),
            plan,
        }
    }

    pub(crate) fn is_live(&self) -> bool {
        self.session_id.is_some()
            && self
                .connection
                .as_ref()
                .map(|connection| !connection.is_incoming_closed())
                .unwrap_or(false)
    }
}

impl AgentManager {
    /// Shut down one live session and forget it. Its transcript stays
    /// frontend-side; the row turns gray.
    pub(crate) async fn drop_live(&self, key: &SessionKey) {
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
    /// Shared by lazy first prompts and the eager executing start.
    pub(crate) async fn spawn_session(
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
        let cwd = plan.cwd(repo_root);
        let response = connection
            .send_request(acp::build_new_session_request(&cwd))
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
            // plan starts with the caller's flag.
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
                    tool_calls: HashMap::new(),
                    plan,
                },
            );
            state.repo_root = Some(repo_root.to_path_buf());
            state.checkout_branch = branch.to_string();
            state.current = Some(key.clone());
        }
        super::session_ids::record(repo_root, &key, &session_id);
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
    /// reapplied. A reopened session keeps its one-time prefix state; a
    /// fresh plan starts with the caller's flag. Single-flights against a
    /// racing warm: one spawner wins, the other polls for liveness.
    pub(crate) async fn ensure_live(
        &self,
        key: &SessionKey,
    ) -> Result<(ConnectionTo<Agent>, String), AgentError> {
        if let Some((connection, session_id, _, _)) = self.session_snapshot_for(key)
            && !connection.is_incoming_closed()
        {
            return Ok((connection, session_id));
        }
        let claimed = lock_state(&self.state)
            .map(|mut state| state.claim_warm(key))
            .unwrap_or(false);
        if claimed {
            let result = self.ensure_live_claimed(key).await;
            if let Some(mut state) = lock_state(&self.state) {
                state.release_warm(key);
            }
            return result;
        }
        let timeout = std::time::Duration::from_secs(10);
        let interval = std::time::Duration::from_millis(50);
        let start = std::time::Instant::now();
        loop {
            tokio::time::sleep(interval).await;
            if let Some((connection, session_id, _, _)) = self.session_snapshot_for(key)
                && !connection.is_incoming_closed()
            {
                return Ok((connection, session_id));
            }
            let still_warming = lock_state(&self.state)
                .map(|state| state.is_warming(key))
                .unwrap_or(false);
            if !still_warming {
                if let Some((connection, session_id, _, _)) = self.session_snapshot_for(key)
                    && !connection.is_incoming_closed()
                {
                    return Ok((connection, session_id));
                }
                let reclaimed = lock_state(&self.state)
                    .map(|mut state| state.claim_warm(key))
                    .unwrap_or(false);
                if reclaimed {
                    let result = self.ensure_live_claimed(key).await;
                    if let Some(mut state) = lock_state(&self.state) {
                        state.release_warm(key);
                    }
                    return result;
                }
            }
            if start.elapsed() >= timeout {
                if let Some((connection, session_id, _, _)) = self.session_snapshot_for(key)
                    && !connection.is_incoming_closed()
                {
                    return Ok((connection, session_id));
                }
                return Err(AgentError::RequestFailed {
                    raw: "session is warming, try again".to_string(),
                });
            }
        }
    }

    /// Spawn path assuming the warm claim is already held. Shared by
    /// `ensure_live` (which claims) and `warm_session` (which holds its
    /// own claim). Pending scoping names spawn without a directory; the
    /// first prompt materializes it later.
    pub(crate) async fn ensure_live_claimed(
        &self,
        key: &SessionKey,
    ) -> Result<(ConnectionTo<Agent>, String), AgentError> {
        let (repo_root, branch) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let plan_ref = plans::PlanRef {
            name: key.plan.clone(),
            phase: role_phase(key.role),
        };
        let is_pending = lock_state(&self.state)
            .map(|state| {
                state.pending_scoping.as_deref() == Some(key.plan.as_str())
                    && key.role == SessionRole::Scoping
            })
            .unwrap_or(false);
        if !plan_ref.path(&repo_root).is_dir() && !is_pending {
            return Err(AgentError::NoSession {
                raw: "plan is gone".to_string(),
            });
        }
        let stored_prefixed = lock_state(&self.state)
            .and_then(|state| state.sessions.get(key).map(|live| live.plan.prefixed));
        let mut plan = ActivePlan::for_session(key);
        // A known session keeps its prefix state across transport deaths;
        // anything without an entry starts with the caller's flag.
        // Eager executing sessions bypass this path: `execute_plan` marks their
        // prefixed flag, since their role already went out hidden.
        plan.prefixed = stored_prefixed.unwrap_or(plan.prefixed);
        let agent = opencode::agent_for(plan.phase);
        let (connection, session_id, _) =
            self.spawn_session(&repo_root, &branch, plan, agent).await?;
        Ok((connection, session_id))
    }

    /// Restore a past session's history via `session/load`: the replay
    /// streams through the same global notification path as live turns, so
    /// the transcript rebuilds with no extra mapping except user bubbles.
    /// Single-flights on the history slot; a live turn owns its session,
    /// so replays never run over a working key. Finished plans replay
    /// read-only history the same way. Plans without a persisted ID recover
    /// it from the CLI on demand; with no past at all (a brand-new plan) a
    /// live session starts instead so the pickers turn live and the
    /// transcript stays empty.
    pub async fn load_history(&self, session: SessionKey) -> Result<(), AgentError> {
        {
            let mut state = self.state.lock().expect("state poisoned");
            if !state.claim_history(&session) {
                return Ok(());
            }
        }
        let (repo_root, _) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        emit_event(
            &self.app,
            AppEvent::HistoryPreparing {
                session: session.clone(),
            },
        );
        emit_event(
            &self.app,
            AppEvent::SessionReset {
                session: session.clone(),
            },
        );
        // After HistoryPreparing so the spinner covers the slow CLI probes.
        // HistoryBegin only follows when past exists and the connection is
        // ready, just before `session/load`; empty keys finish without it.
        let Some(session_id) = super::session_ids::resolve(&repo_root, &session) else {
            return self.finish_empty_history(session).await;
        };
        let plan = ActivePlan::for_session(&session);
        let connection = match self
            .ensure_connection_for(&session, &plan, opencode::agent_env(&plan.plan_ref()))
            .await
        {
            Ok(connection) => connection,
            Err(error) => {
                self.fail_history(&session, error.to_string());
                return Err(error);
            }
        };
        if let Some(mut state) = lock_state(&self.state)
            && let Some(live) = state.sessions.get_mut(&session)
        {
            live.session_id = Some(session_id.clone());
        }
        emit_event(
            &self.app,
            AppEvent::HistoryBegin {
                session: session.clone(),
            },
        );
        let cwd = plan.cwd(&repo_root);
        match connection
            .send_request(acp::build_load_session_request(&session_id, &cwd))
            .block_task()
            .await
        {
            Ok(response) => {
                let views = config_views(&response.config_options.unwrap_or_default());
                let roles = crate::repo_state::roles_from_options(&views);
                if let Some(mut state) = lock_state(&self.state) {
                    if let Some(live) = state.sessions.get_mut(&session) {
                        live.last_roles = roles;
                    }
                    state.finish_history(&session);
                }
                emit_event(
                    &self.app,
                    AppEvent::ConfigOptions {
                        session: session.clone(),
                        options: views,
                    },
                );
                emit_event(&self.app, AppEvent::HistoryDone { session });
                self.push_plans();
                Ok(())
            }
            Err(error) => {
                let raw = error.to_string();
                self.fail_history(&session, raw.clone());
                Err(AgentError::RequestFailed { raw })
            }
        }
    }

    /// Empty-history path for keys with no saved session: a brand-new plan
    /// gets a live connection and an empty transcript instead of an error
    /// bar, going straight to `ConfigOptions` + `HistoryDone` without ever
    /// emitting `HistoryBegin`. Finished plans never say "plan is gone":
    /// with no recoverable past they fail with an honest non-retryable
    /// reason instead.
    async fn finish_empty_history(&self, session: SessionKey) -> Result<(), AgentError> {
        if let Some((repo_root, _)) = self.reopen_snapshot()
            && super::session_ids::is_finished(&repo_root, &session.plan)
        {
            let raw = "no saved session found for this plan - it predates session recording or its session was pruned"
                .to_string();
            self.fail_history(&session, raw.clone());
            return Err(AgentError::RequestFailed { raw });
        }
        match self.ensure_live(&session).await {
            Ok(_) => {
                if let Some(mut state) = lock_state(&self.state) {
                    state.finish_history(&session);
                }
                emit_event(&self.app, AppEvent::HistoryDone { session });
                Ok(())
            }
            Err(error) => {
                self.fail_history(&session, error.to_string());
                Err(error)
            }
        }
    }

    /// Fail one history load: release the single-flight and emit an error
    /// bar with the classified hint. The partial replay stays in the
    /// transcript; a retry clears it on `HistoryPreparing`.
    fn fail_history(&self, session: &SessionKey, raw: String) {
        if let Some(mut state) = lock_state(&self.state) {
            state.abort_history(session);
        }
        let hint = classify_error(&raw);
        emit_event(
            &self.app,
            AppEvent::HistoryFailed {
                session: session.clone(),
                raw,
                hint: hint.text,
                retryable: hint.retryable,
            },
        );
    }

    /// Wait for an in-flight history replay to settle. Prompts never
    /// interleave a live turn with the replay; on timeout the caller
    /// refuses instead.
    pub(crate) async fn wait_for_history(&self, key: &SessionKey) -> Result<(), AgentError> {
        let timeout = std::time::Duration::from_secs(30);
        let interval = std::time::Duration::from_millis(50);
        let start = std::time::Instant::now();
        loop {
            let loading = lock_state(&self.state)
                .map(|state| state.is_history_loading(key))
                .unwrap_or(false);
            if !loading {
                return Ok(());
            }
            if start.elapsed() >= timeout {
                return Err(AgentError::RequestFailed {
                    raw: "history is still loading, try again".to_string(),
                });
            }
            tokio::time::sleep(interval).await;
        }
    }

    /// Drop every live session entry. Repo opens start from scratch.
    pub(crate) fn clear_sessions(&self) {
        match self.state.lock() {
            Ok(mut state) => {
                state.sessions.clear();
                state.current = None;
                state.awake = None;
                state.pending_scoping = None;
                state.pending_titles.clear();
                state.worktrees.clear();
                state.history_loading.clear();
                state.history_loaded.clear();
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

    pub(crate) fn reopen_snapshot(&self) -> Option<(PathBuf, String)> {
        let state = lock_state(&self.state)?;
        Some((state.repo_root.clone()?, state.checkout_branch.clone()))
    }

    pub(crate) fn connection_snapshot_for(&self, key: &SessionKey) -> Option<ConnectionTo<Agent>> {
        lock_state(&self.state).and_then(|state| state.sessions.get(key)?.connection.clone())
    }

    pub(crate) fn session_snapshot_for(
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
