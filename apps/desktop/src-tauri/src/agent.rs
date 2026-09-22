// Agent lifecycle: spawn `opencode acp` once, initialize it, and open
// sessions on the shared connection.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::acp::{self, AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo};
use crate::types::AgentError;

#[derive(Default)]
pub struct AgentManager {
    connection: Arc<Mutex<Option<ConnectionTo<Agent>>>>,
}

impl AgentManager {
    /// Open one session with `cwd` set to the given repository root.
    pub async fn open_session(&self, cwd: PathBuf) -> Result<String, AgentError> {
        let connection = self.ensure_connection().await?;
        let response = connection
            .send_request(acp::build_new_session_request(&cwd))
            .block_task()
            .await
            .map_err(|error| AgentError::RequestFailed {
                raw: error.to_string(),
            })?;
        Ok(response.session_id.to_string())
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
        let slot = Arc::clone(&self.connection);
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        let ready = Arc::new(Mutex::new(Some(ready_tx)));
        let ready_for_handler = Arc::clone(&ready);
        let ready_for_error = Arc::clone(&ready);

        tauri::async_runtime::spawn(async move {
            let result = Client
                .builder()
                .connect_with(agent, |connection: ConnectionTo<Agent>| {
                    let slot = Arc::clone(&slot);
                    let ready = Arc::clone(&ready_for_handler);
                    async move {
                        connection
                            .send_request(acp::build_initialize_request())
                            .block_task()
                            .await
                            .map_err(|error| acp::internal_error(error.to_string()))?;
                        {
                            let mut guard = slot.lock().expect("connection poisoned");
                            *guard = Some(connection.clone());
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
        self.connection.lock().ok().and_then(|guard| guard.clone())
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
    use std::collections::HashMap;
    use std::path::Path;

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
