// Agent lifecycle: spawn `opencode acp` once, initialize it, open sessions
// bound to repository roots, and stream prompt turns as events.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use crate::acp::{
    self, AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo, SessionConfigKind,
    SessionConfigOption, SessionConfigSelectOptions,
};
use crate::types::{AgentError, AppEvent, ConfigOptionView, ConfigValueView, SessionInfo};

#[derive(Default)]
struct State {
    connection: Option<ConnectionTo<Agent>>,
    supports_close: bool,
    session_id: Option<String>,
    working: bool,
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
    /// open a fresh session with `cwd` set to the repo root, and report the
    /// agent's config options for the model selector.
    pub async fn open_repo(
        &self,
        repo_root: PathBuf,
        branch: String,
    ) -> Result<SessionInfo, AgentError> {
        let connection = self.ensure_connection().await?;
        if let Some((old_connection, old_id, supports_close)) = self.session_snapshot()
            && supports_close
        {
            let _ = old_connection
                .send_request(acp::CloseSessionRequest::new(acp::SessionId::new(
                    old_id.as_str(),
                )))
                .block_task()
                .await;
        }
        let response = connection
            .send_request(acp::build_new_session_request(&repo_root))
            .block_task()
            .await
            .map_err(|error| AgentError::RequestFailed {
                raw: error.to_string(),
            })?;
        let session_id = response.session_id.to_string();
        let options = config_views(&response.config_options.unwrap_or_default());
        {
            let mut state = self.state.lock().expect("state poisoned");
            state.session_id = Some(session_id.clone());
        }
        Ok(SessionInfo {
            session_id,
            repo_root: repo_root.to_string_lossy().to_string(),
            branch,
            config_options: options,
        })
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
            state.working = true;
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
                    let _ = app.emit("samokod://event", AppEvent::TurnDone);
                }
                Err(error) => {
                    set_working(&state, false);
                    let _ = app.emit(
                        "samokod://event",
                        AppEvent::TurnFailed {
                            raw: error.to_string(),
                        },
                    );
                }
            }
        });
        Ok(())
    }

    /// Stop the turn via `session/cancel`.
    pub async fn cancel_turn(&self) -> Result<(), AgentError> {
        let (connection, session_id, _) =
            self.session_snapshot()
                .ok_or_else(|| AgentError::NoSession {
                    raw: "no active turn".to_string(),
                })?;
        let _ = connection.send_notification(acp::build_cancel_notification(acp::SessionId::new(
            session_id,
        )));
        set_working(&self.state, false);
        Ok(())
    }

    /// Ensure a live, initialized connection. Spawns `opencode acp` once and
    /// reuses it for every later session.
    pub async fn ensure_connection(&self) -> Result<ConnectionTo<Agent>, AgentError> {
        if let Some(connection) = self.connection_snapshot() {
            return Ok(connection);
        }
        let binary = acp::resolve_opencode_binary()?;
        let config = AcpAgentConfig::new(binary).arg("acp");
        let agent = AcpAgent::new(config);
        let slot = Arc::clone(&self.state);
        let notify_state = Arc::clone(&self.state);
        let notify_app = self.app.clone();
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
                        if let Some(tx) = ready.lock().expect("ready poisoned").take() {
                            let _ = tx.send(Ok(()));
                        }
                        connection.incoming_closed().await;
                        Ok(())
                    }
                })
                .await;
            if let Err(error) = result
                && let Some(tx) = ready_for_error.lock().expect("ready poisoned").take()
            {
                let _ = tx.send(Err(error.to_string()));
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

    fn connection_snapshot(&self) -> Option<ConnectionTo<Agent>> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.connection.clone())
    }

    fn session_snapshot(&self) -> Option<(ConnectionTo<Agent>, String, bool)> {
        let state = self.state.lock().ok()?;
        Some((
            state.connection.clone()?,
            state.session_id.clone()?,
            state.supports_close,
        ))
    }
}

fn set_working(state: &Mutex<State>, working: bool) {
    if let Ok(mut guard) = state.lock() {
        guard.working = working;
    }
}

fn handle_notification(
    state: &Mutex<State>,
    app: &AppHandle,
    notification: &acp::SessionNotification,
) {
    let current = state.lock().ok().and_then(|guard| guard.session_id.clone());
    if let Some(current) = current
        && notification.session_id.to_string() != current
    {
        return;
    }
    if let Some(chunk) = crate::updates::agent_text_of(&notification.update) {
        let _ = app.emit("samokod://event", AppEvent::AgentText { chunk });
    }
    match &notification.update {
        acp::SessionUpdate::ToolCall(call) => {
            let line = crate::updates::format_tool_line(call);
            let _ = app.emit("samokod://event", AppEvent::ToolLine { line });
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            let line = crate::updates::format_tool_update(update);
            let _ = app.emit("samokod://event", AppEvent::ToolLine { line });
        }
        _ => {}
    }
}

/// Convert wire config options to generic views.
pub fn config_views(options: &[SessionConfigOption]) -> Vec<ConfigOptionView> {
    options
        .iter()
        .map(|option| {
            let (current, values) = match &option.kind {
                SessionConfigKind::Select(select) => (
                    select.current_value.to_string(),
                    select_values(&select.options),
                ),
                SessionConfigKind::Boolean(boolean) => (
                    boolean.current_value.to_string(),
                    vec![
                        ConfigValueView {
                            value: "true".to_string(),
                            name: "true".to_string(),
                        },
                        ConfigValueView {
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
                current,
                options: values,
            }
        })
        .collect()
}

fn select_values(options: &SessionConfigSelectOptions) -> Vec<ConfigValueView> {
    match options {
        SessionConfigSelectOptions::Ungrouped(values) => values
            .iter()
            .map(|value| ConfigValueView {
                value: value.value.to_string(),
                name: value.name.clone(),
            })
            .collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| {
                group.options.iter().map(|value| ConfigValueView {
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
    use crate::acp::{SessionConfigSelectOption, SessionConfigValueId};
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
        );
        let views = config_views(std::slice::from_ref(&option));
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, "model");
        assert_eq!(views[0].current, "opencode/big-pickle");
        assert_eq!(views[0].options.len(), 1);
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
