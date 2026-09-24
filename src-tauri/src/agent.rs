// Agent lifecycle: spawn `opencode acp` once, initialize it, open sessions
// bound to repository roots, and stream prompt turns as events.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use crate::acp::{
    self, AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo, SessionConfigKind,
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOptions,
};
use crate::awake;
use crate::error_hint::{classify_error, is_transport_error};
use crate::spend::context_pct;
use crate::todos::{diff_todos, todos_from_call, todos_from_update};
use crate::types::{
    AgentError, AppEvent, ConfigOptionValueView, ConfigOptionView, PermissionView, SessionInfo,
    TodoView,
};

/// User decision for one permission card.
#[derive(Debug, Clone)]
enum PermissionDecision {
    Selected(String),
    Cancelled,
}

#[derive(Default)]
struct State {
    connection: Option<ConnectionTo<Agent>>,
    supports_close: bool,
    session_id: Option<String>,
    repo_root: Option<PathBuf>,
    branch: String,
    working: bool,
    awake: Option<awake::Guard>,
    last_prompt: Option<String>,
    last_model: Option<String>,
    pending: HashMap<String, tokio::sync::oneshot::Sender<PermissionDecision>>,
    todos: Vec<TodoView>,
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
    fn reset_session(
        &mut self,
        session_id: String,
        repo_root: PathBuf,
        branch: String,
        model: Option<String>,
    ) {
        self.session_id = Some(session_id);
        self.repo_root = Some(repo_root);
        self.branch = branch;
        self.set_working(false);
        self.last_prompt = None;
        self.last_model = model;
        self.pending.clear();
        self.todos.clear();
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

    /// Open a repository: close the old session when the agent advertises it,
    /// open a fresh session with `cwd` set to the repo root, pin mode to
    /// build, reapply the stored model, and report the agent's config options.
    pub async fn open_repo(
        &self,
        repo_root: PathBuf,
        branch: String,
        stored_model: Option<String>,
    ) -> Result<SessionInfo, AgentError> {
        let connection = self.ensure_connection().await?;
        if let Some((old_connection, old_id, supports_close)) = self.session_snapshot()
            && supports_close
            && let Err(error) = old_connection
                .send_request(acp::CloseSessionRequest::new(acp::SessionId::new(
                    old_id.as_str(),
                )))
                .block_task()
                .await
        {
            log::warn!("failed to close previous session {old_id}: {error}");
        }
        let response = connection
            .send_request(acp::build_new_session_request(&repo_root))
            .block_task()
            .await
            .map_err(|error| AgentError::RequestFailed {
                raw: error.to_string(),
            })?;
        let session_id = response.session_id.to_string();
        let mut options = config_views(&response.config_options.unwrap_or_default());
        if let Some(pinned) =
            pin_build_mode(&connection, &acp::SessionId::new(session_id.clone())).await
        {
            options = pinned;
        }
        let mut applied_model: Option<String> = None;
        if let Some(model) = stored_model
            && let Some(updated) = set_model_value(
                &connection,
                &acp::SessionId::new(session_id.clone()),
                &model,
            )
            .await
        {
            options = updated;
            applied_model = Some(model);
        }
        {
            let mut state = self.state.lock().expect("state poisoned");
            state.reset_session(
                session_id.clone(),
                repo_root.clone(),
                branch.clone(),
                applied_model,
            );
        }
        emit_event(&self.app, AppEvent::SessionReset);
        Ok(SessionInfo {
            session_id,
            repo_root: repo_root.to_string_lossy().to_string(),
            branch,
            config_options: options,
        })
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
        let response = connection
            .send_request(acp::build_set_config_request(
                &acp::SessionId::new(session_id),
                &config_id,
                &value,
            ))
            .block_task()
            .await
            .map_err(|error| AgentError::RequestFailed {
                raw: error.to_string(),
            })?;
        if config_id == "model" {
            match self.state.lock() {
                Ok(mut state) => {
                    state.last_model = Some(value);
                }
                Err(error) => {
                    log::warn!("failed to remember model choice: {error}");
                }
            }
        }
        Ok(config_views(&response.config_options))
    }

    /// Open a fresh session on the same root, closing the old one when the
    /// agent advertises it. Reuses the in-memory model choice.
    pub async fn new_chat(&self) -> Result<SessionInfo, AgentError> {
        let (repo_root, branch, model) =
            self.reopen_snapshot()
                .ok_or_else(|| AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                })?;
        self.open_repo(repo_root, branch, model).await
    }

    /// Current repository root, when a session is open.
    pub fn current_repo(&self) -> Option<PathBuf> {
        lock_state(&self.state)?.repo_root.clone()
    }

    /// Resend the last prompt. When the transport is closed, reopen the
    /// session on the same root first. Returns false when nothing ran.
    pub async fn retry_last(&self) -> Result<bool, AgentError> {
        let text = match self.state.lock() {
            Ok(state) => state.last_prompt.clone(),
            Err(error) => {
                log::warn!("failed to read last prompt: {error}");
                None
            }
        };
        let Some(text) = text else {
            return Ok(false);
        };
        let live = self
            .session_snapshot()
            .is_some_and(|(connection, _, _)| !connection.is_incoming_closed());
        if !live {
            let (repo_root, branch, model) =
                self.reopen_snapshot()
                    .ok_or_else(|| AgentError::NoSession {
                        raw: "open a repository first".to_string(),
                    })?;
            self.open_repo(repo_root, branch, model).await?;
        }
        self.send_prompt(text).await?;
        Ok(true)
    }

    /// Send one plain-text prompt. Streams arrive as events; the turn end
    /// arrives as done or failed.
    pub async fn send_prompt(&self, text: String) -> Result<(), AgentError> {
        let (connection, session_id, _) =
            self.session_snapshot()
                .ok_or_else(|| AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                })?;
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

    /// Ensure a live, initialized connection. Spawns `opencode acp` once,
    /// reuses it for later sessions, and respawns after a closed transport.
    pub async fn ensure_connection(&self) -> Result<ConnectionTo<Agent>, AgentError> {
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
        let config = AcpAgentConfig::new(binary).arg("acp");
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

    fn reopen_snapshot(&self) -> Option<(PathBuf, String, Option<String>)> {
        let state = lock_state(&self.state)?;
        Some((
            state.repo_root.clone()?,
            state.branch.clone(),
            state.last_model.clone(),
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
        _ => {}
    }
}

/// Snoop a todo list off a tool call input, when it carries one.
fn snoop_todos_from_call(state: &Mutex<State>, app: &AppHandle, call: &acp::ToolCall) {
    let fresh = todos_from_call(call.raw_input.as_ref());
    log::debug!(
        "todo snoop call {} todos {:?}",
        call.tool_call_id,
        fresh.as_ref().map(|todos| todos.len()),
    );
    if let Some(fresh) = fresh {
        update_todos(state, app, fresh);
    }
}

/// Snoop a todo list off a tool update, preferring the output over the input.
fn snoop_todos_from_update(state: &Mutex<State>, app: &AppHandle, update: &acp::ToolCallUpdate) {
    let fresh = todos_from_update(
        update.fields.raw_input.as_ref(),
        update.fields.raw_output.as_ref(),
    );
    log::debug!(
        "todo snoop update {} todos {:?}",
        update.tool_call_id,
        fresh.as_ref().map(|todos| todos.len()),
    );
    if let Some(fresh) = fresh {
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

async fn pin_build_mode(
    connection: &ConnectionTo<Agent>,
    session_id: &acp::SessionId,
) -> Option<Vec<ConfigOptionView>> {
    match connection
        .send_request(acp::build_set_config_request(session_id, "mode", "build"))
        .block_task()
        .await
    {
        Ok(response) => Some(config_views(&response.config_options)),
        Err(error) => {
            log::warn!("failed to pin session {session_id} to build mode: {error}");
            None
        }
    }
}

async fn set_model_value(
    connection: &ConnectionTo<Agent>,
    session_id: &acp::SessionId,
    model: &str,
) -> Option<Vec<ConfigOptionView>> {
    match connection
        .send_request(acp::build_set_config_request(session_id, "model", model))
        .block_task()
        .await
    {
        Ok(response) => Some(config_views(&response.config_options)),
        Err(error) => {
            log::warn!("failed to reapply stored model {model}: {error}");
            None
        }
    }
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
            None,
        );
        assert!(!state.working);
        assert!(state.awake.is_none());
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
