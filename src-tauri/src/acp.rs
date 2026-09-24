// ACP wire boundary. SDK schema types enter the app through this module;
// nothing past it imports `agent-client-protocol` directly.
pub use agent_client_protocol::schema::v1::{
    CancelNotification, CloseSessionRequest, ContentBlock, PermissionOption, PermissionOptionId,
    PermissionOptionKind, PromptRequest, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SelectedPermissionOutcome, SessionConfigId, SessionConfigKind,
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigOptionValue,
    SessionConfigSelectOptions, SessionConfigValueId, SessionId, SessionNotification,
    SessionUpdate, SetSessionConfigOptionRequest, TextContent, ToolCall, ToolCallUpdate, ToolKind,
    UsageUpdate,
};
#[cfg(test)]
pub use agent_client_protocol::schema::v1::{
    ContentChunk, SessionConfigSelectOption, ToolCallStatus, ToolCallUpdateFields,
};
use agent_client_protocol::schema::{
    ProtocolVersion,
    v1::{ClientCapabilities, InitializeRequest, NewSessionRequest},
};
pub use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo};
#[cfg(test)]
pub use agent_client_protocol::{on_receive_notification, on_receive_request};
use std::path::{Path, PathBuf};

use crate::types::AgentError;

/// Locate the `opencode` binary, restoring the shell PATH when launched
/// from the app launcher with a minimal system PATH. A missing binary is a
/// typed error with install guidance and never enters retry logic.
pub fn resolve_opencode_binary() -> Result<PathBuf, AgentError> {
    resolve_in_dirs(&candidate_dirs())
}

/// Build the ACP `initialize` request. The client advertises no filesystem or
/// terminal capabilities so OpenCode keeps tool execution server-side.
pub fn build_initialize_request() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1).client_capabilities(ClientCapabilities::default())
}

/// Build the ACP `session/new` request for a repository root. No MCP servers
/// are passed; OpenCode owns them.
pub fn build_new_session_request(cwd: &Path) -> NewSessionRequest {
    NewSessionRequest::new(cwd)
}

/// Build a `session/set_config_option` request from a string config id and value id.
pub fn build_set_config_request(
    session_id: &SessionId,
    config_id: &str,
    value: &str,
) -> SetSessionConfigOptionRequest {
    SetSessionConfigOptionRequest::new(
        session_id.clone(),
        SessionConfigId::new(config_id),
        SessionConfigOptionValue::value_id(SessionConfigValueId::new(value)),
    )
}

/// Build the ACP `session/cancel` notification payload for a session.
pub fn build_cancel_notification(session_id: SessionId) -> CancelNotification {
    CancelNotification::new(session_id)
}

/// Wrap a failure as an ACP internal error for handler closures.
pub fn internal_error(message: impl ToString) -> agent_client_protocol::Error {
    agent_client_protocol::util::internal_error(message)
}

fn candidate_dirs() -> Vec<PathBuf> {
    process_path_dirs()
        .into_iter()
        .chain(path_helper_dirs())
        .chain(login_shell_dirs())
        .collect()
}

fn resolve_in_dirs(dirs: &[PathBuf]) -> Result<PathBuf, AgentError> {
    let mut seen = std::collections::HashSet::new();
    dirs.iter()
        .filter(|dir| !dir.as_os_str().is_empty())
        .filter(|dir| seen.insert(dir.as_path()))
        .find_map(|dir| executable_in(dir))
        .ok_or(AgentError::MissingBinary)
}

fn process_path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn path_helper_dirs() -> Vec<PathBuf> {
    match std::process::Command::new("/usr/libexec/path_helper")
        .arg("-s")
        .output()
    {
        Ok(output) if output.status.success() => {
            parse_path_helper(&String::from_utf8_lossy(&output.stdout))
        }
        Ok(output) => {
            log::warn!("path_helper exited unsuccessfully: {}", output.status);
            Vec::new()
        }
        Err(error) => {
            log::warn!("failed to run path_helper: {error}");
            Vec::new()
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn path_helper_dirs() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(unix)]
fn login_shell_dirs() -> Vec<PathBuf> {
    login_shells()
        .iter()
        .map(|shell| query_login_shell(shell))
        .find(|dirs| !dirs.is_empty())
        .unwrap_or_default()
}

#[cfg(not(unix))]
fn login_shell_dirs() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(unix)]
fn login_shells() -> Vec<PathBuf> {
    let mut shells = Vec::new();
    if let Some(shell) = std::env::var_os("SHELL").map(PathBuf::from)
        && !shell.as_os_str().is_empty()
    {
        shells.push(shell);
    }
    #[cfg(target_os = "macos")]
    shells.push(PathBuf::from("/bin/zsh"));
    shells.push(PathBuf::from("/bin/sh"));
    let mut unique = Vec::with_capacity(shells.len());
    for shell in shells {
        if !unique.contains(&shell) {
            unique.push(shell);
        }
    }
    unique
}

#[cfg(unix)]
fn query_login_shell(shell: &Path) -> Vec<PathBuf> {
    let program = shell.to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let output = std::process::Command::new(&program)
            .args(["-lic", "echo $PATH"])
            .output();
        let _ = tx.send(output);
    });
    let timeout = std::time::Duration::from_secs(2);
    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) if output.status.success() => {
            split_path_value(&String::from_utf8_lossy(&output.stdout))
        }
        Ok(Ok(output)) => {
            log::warn!("shell PATH query exited unsuccessfully: {}", output.status);
            Vec::new()
        }
        Ok(Err(error)) => {
            log::warn!("shell PATH query failed to spawn: {error}");
            Vec::new()
        }
        Err(_) => {
            log::warn!("shell PATH query timed out after {timeout:?}");
            Vec::new()
        }
    }
}

fn parse_path_helper(output: &str) -> Vec<PathBuf> {
    let path_value = output
        .split("PATH=\"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap_or("");
    split_path_value(path_value)
}

fn split_path_value(value: &str) -> Vec<PathBuf> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    std::env::split_paths(trimmed).collect()
}

fn executable_in(dir: &Path) -> Option<PathBuf> {
    let candidate = dir.join("opencode");
    if is_executable(&candidate) {
        return Some(candidate);
    }
    #[cfg(windows)]
    {
        let exe = candidate.with_extension("exe");
        if is_executable(&exe) {
            return Some(exe);
        }
    }
    None
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_advertises_no_fs_or_terminal() {
        let request = build_initialize_request();
        assert!(!request.client_capabilities.terminal);
        assert_eq!(
            request.client_capabilities.fs,
            Default::default(),
            "filesystem capabilities must stay empty so tools run server-side"
        );
    }

    #[test]
    fn missing_binary_is_typed() {
        let result = resolve_in_dirs(&["/nonexistent-samokod-path".into()]);
        assert!(matches!(result, Err(AgentError::MissingBinary)));
    }

    #[test]
    fn falls_back_to_later_dirs() {
        let shell_dir = tempfile::tempdir().expect("tempdir");
        let expected = fake_opencode_in(shell_dir.path());
        let result = resolve_in_dirs(&[
            "/nonexistent-samokod-path".into(),
            shell_dir.path().to_path_buf(),
        ]);
        assert_eq!(result.ok(), Some(expected));
    }

    #[test]
    fn prefers_earlier_dirs() {
        let first = tempfile::tempdir().expect("tempdir");
        let second = tempfile::tempdir().expect("tempdir");
        let third = tempfile::tempdir().expect("tempdir");
        let expected = fake_opencode_in(first.path());
        fake_opencode_in(second.path());
        fake_opencode_in(third.path());
        let result = resolve_in_dirs(&[
            first.path().to_path_buf(),
            second.path().to_path_buf(),
            third.path().to_path_buf(),
        ]);
        assert_eq!(result.ok(), Some(expected));
    }

    #[test]
    fn duplicate_dirs_stay_typed() {
        let bogus: PathBuf = "/nonexistent-samokod-path".into();
        let result = resolve_in_dirs(&[bogus.clone(), bogus]);
        assert!(matches!(result, Err(AgentError::MissingBinary)));
    }

    #[test]
    fn path_helper_parses_quoted_path() {
        let dirs = parse_path_helper("PATH=\"/usr/bin:/bin\"; export PATH;\n");
        assert_eq!(dirs, vec![PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    }

    #[test]
    fn path_value_splits_on_colon() {
        let dirs = split_path_value("/usr/bin:/bin\n");
        assert_eq!(dirs, vec![PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    }

    #[test]
    fn real_binary_resolves_when_present() {
        if resolve_opencode_binary().is_err() {
            return;
        }
        assert!(resolve_opencode_binary().is_ok());
    }

    #[test]
    fn usage_update_without_cost_decodes_to_none() {
        let update: UsageUpdate = serde_json::from_value(
            serde_json::json!({"sessionUpdate": "usage_update", "used": 1, "size": 2}),
        )
        .expect("cost is optional");
        assert!(update.cost.is_none());
        assert_eq!(update.used, 1);
    }

    fn fake_opencode_in(dir: &Path) -> PathBuf {
        let binary = dir.join("opencode");
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").expect("write fake opencode");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
                .expect("chmod fake opencode");
        }
        binary
    }
}
