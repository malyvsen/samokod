// Live sessions: one `opencode acp` child process per session, keyed by
// plan directory name plus role. Spawning is lazy on the first prompt,
// except the eager executor start after approval.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::acp::{self, AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo};
use crate::error_hint::classify_error;
use crate::opencode;
use crate::plans;
use crate::types::{AgentError, AppEvent, SessionKey, SessionRole};

use super::AgentManager;
use super::config::{log_roles, option_id_for_role, pin_agent_mode, send_config_option};
use super::permissions::handle_permission_request;
use super::plans_list::push_sorted;
use super::turns::handle_notification;
use super::warm::{release_warm, try_claim_warm};
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
    }
}

/// Plan owned by one live session.
#[derive(Debug, Clone)]
pub(crate) struct ActivePlan {
    pub(crate) name: String,
    pub(crate) phase: plans::Phase,
    /// Role already delivered for this ACP conversation. Scoping never
    /// sends one (the composer holds the template as editable text);
    /// executing sends once, hidden on approval or prefixed to the first
    /// prompt otherwise.
    pub(crate) prefixed: bool,
}

impl ActivePlan {
    pub(crate) fn scoping(name: String) -> Self {
        ActivePlan {
            name,
            phase: plans::Phase::Scoping,
            prefixed: true,
        }
    }

    pub(crate) fn executing(name: String) -> Self {
        ActivePlan {
            name,
            phase: plans::Phase::Executing,
            prefixed: false,
        }
    }

    pub(crate) fn key(&self) -> Option<SessionKey> {
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
    /// Shared by lazy first prompts and the eager executor start.
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
        let claimed = match self.state.lock() {
            Ok(mut state) => try_claim_warm(&mut state.warming, key),
            Err(error) => {
                log::warn!("failed to claim live session: {error}");
                false
            }
        };
        if claimed {
            let result = self.ensure_live_claimed(key).await;
            match self.state.lock() {
                Ok(mut state) => release_warm(&mut state.warming, key),
                Err(error) => log::warn!("failed to release live session: {error}"),
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
                .map(|state| state.warming.contains(key))
                .unwrap_or(false);
            if !still_warming {
                if let Some((connection, session_id, _, _)) = self.session_snapshot_for(key)
                    && !connection.is_incoming_closed()
                {
                    return Ok((connection, session_id));
                }
                let reclaimed = match self.state.lock() {
                    Ok(mut state) => try_claim_warm(&mut state.warming, key),
                    Err(error) => {
                        log::warn!("failed to reclaim live session: {error}");
                        false
                    }
                };
                if reclaimed {
                    let result = self.ensure_live_claimed(key).await;
                    match self.state.lock() {
                        Ok(mut state) => release_warm(&mut state.warming, key),
                        Err(error) => log::warn!("failed to release live session: {error}"),
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
        let mut plan = match key.role {
            SessionRole::Scoping => ActivePlan::scoping(key.plan.clone()),
            SessionRole::Executing => ActivePlan::executing(key.plan.clone()),
        };
        // A known session keeps its prefix state across transport deaths;
        // anything without an entry starts with the caller's flag.
        // Eager executors bypass this path: `execute_plan` marks their
        // prefixed flag, since their role already went out hidden.
        plan.prefixed = stored_prefixed.unwrap_or(plan.prefixed);
        let agent = opencode::agent_for(plan.phase);
        let (connection, session_id, _) =
            self.spawn_session(&repo_root, &branch, plan, agent).await?;
        Ok((connection, session_id))
    }

    /// Drop every live session entry. Repo opens start from scratch.
    pub(crate) fn clear_sessions(&self) {
        match self.state.lock() {
            Ok(mut state) => {
                state.sessions.clear();
                state.current = None;
                state.awake = None;
                state.pending_scoping = None;
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
        Some((state.repo_root.clone()?, state.branch.clone()))
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
