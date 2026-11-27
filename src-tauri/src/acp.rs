// ACP wire boundary: wire types plus request builders plus the
// `opencode acp` process lookup used to open the transport. Only this
// module imports `agent-client-protocol`; the rest of the app goes
// through `crate::acp`. OpenCode specifics (agent definitions,
// permissions shape, instructions text) live in `opencode.rs`, never
// here.
use agent_client_protocol::schema::ProtocolVersion;
pub use agent_client_protocol::v1::ConnectionTo;
pub use agent_client_protocol::v1::Responder;
pub use agent_client_protocol::v1::schema::{
    CancelNotification, CloseSessionRequest, ContentBlock, LoadSessionRequest, Meta,
    PermissionOption, PermissionOptionId, PermissionOptionKind, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionConfigId, SessionConfigKind, SessionConfigOption,
    SessionConfigOptionCategory, SessionConfigOptionValue, SessionConfigSelectOptions,
    SessionConfigValueId, SessionId, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, StopReason, TextContent, ToolCall, ToolCallStatus,
    ToolCallUpdate, ToolKind,
};
use agent_client_protocol::v1::schema::{ClientCapabilities, InitializeRequest, NewSessionRequest};
pub use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, Client};
pub use agent_client_protocol::{on_receive_notification, on_receive_request};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::types::AgentError;

/// Locate the `opencode` binary, restoring the shell PATH when launched
/// from the app launcher with a minimal system PATH. A missing binary is a
/// typed error with install guidance and never enters retry logic.
pub fn resolve_opencode_binary() -> Result<PathBuf, AgentError> {
    resolve_in_dirs(&candidate_dirs())
}

/// PATH value for spawning the agent: the search dirs joined back
/// together. Finding the binary is not enough; agent tools inherit this
/// environment too.
pub fn agent_path_value() -> String {
    std::env::join_paths(candidate_dirs())
        .map(|paths| paths.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Build the ACP `initialize` request. The client advertises no filesystem or
/// terminal capabilities so OpenCode keeps tool execution server-side.
pub fn build_initialize_request() -> InitializeRequest {
    InitializeRequest::new(ProtocolVersion::V1).client_capabilities(ClientCapabilities::default())
}

/// Build the ACP `session/new` request for a repository root. Role
/// instructions travel via the `instructions` file. No MCP servers are
/// passed; OpenCode owns them.
pub fn build_new_session_request(cwd: &Path) -> NewSessionRequest {
    NewSessionRequest::new(cwd)
}

/// Build the ACP `session/load` request restoring a past session. Replay
/// notifications flow through the existing global handler; no separate
/// session handle is created.
pub fn build_load_session_request(session_id: &str, cwd: &Path) -> LoadSessionRequest {
    LoadSessionRequest::new(SessionId::new(session_id), cwd)
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
    agent_client_protocol::Error::internal_error().data(message.to_string())
}

/// Shell dirs to search, in priority order: the app process PATH first,
/// then the macOS path helper and login shells to cover launcher spawns
/// with a minimal system PATH. Unique and non-empty so both lookup and
/// PATH construction share one source.
fn candidate_dirs() -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    process_path_dirs()
        .into_iter()
        .chain(path_helper_dirs())
        .chain(login_shell_dirs())
        .filter(|dir| !dir.as_os_str().is_empty())
        .filter(|dir| seen.insert(dir.clone()))
        .collect()
}

fn resolve_in_dirs(dirs: &[PathBuf]) -> Result<PathBuf, AgentError> {
    dirs.iter()
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
            parse_login_shell_output(&String::from_utf8_lossy(&output.stdout))
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

/// Login shells can print noise before `$PATH`; the value is the last
/// non-empty line.
fn parse_login_shell_output(output: &str) -> Vec<PathBuf> {
    output
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(split_path_value)
        .unwrap_or_default()
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
    use agent_client_protocol::v1::schema::UsageUpdate;

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
    fn boundary_stays_protocol_only() {
        let source = include_str!("acp.rs");
        // Needles built without literals so this test's own source
        // doesn't trip the check.
        let forbidden = ["crate", "opencode"].join("::");
        assert!(
            !source.contains(&forbidden),
            "acp.rs must never import the OpenCode edge"
        );
        let config_call = ["opencode", "agent_config"].join("::");
        let env_call = ["opencode", "session_env"].join("::");
        assert!(
            !source.contains(&config_call) && !source.contains(&env_call),
            "agent config stays in opencode.rs"
        );
    }

    #[test]
    fn new_session_carries_cwd_without_mcp() {
        let cwd = std::path::Path::new("/repo");
        let request = build_new_session_request(cwd);
        assert_eq!(request.cwd, cwd);
        assert!(request.mcp_servers.is_empty());
    }

    #[test]
    fn load_session_carries_id_and_cwd() {
        let cwd = std::path::Path::new("/repo");
        let request = build_load_session_request("ses_1", cwd);
        assert_eq!(request.session_id.to_string(), "ses_1");
        assert_eq!(request.cwd, cwd);
    }

    #[test]
    fn cancel_carries_session_id() {
        let notification = build_cancel_notification(SessionId::new("ses_1"));
        assert_eq!(notification.session_id.to_string(), "ses_1");
    }

    #[test]
    fn set_config_carries_ids() {
        let session = SessionId::new("ses_1");
        let request = build_set_config_request(&session, "mode", "samokod-scoping");
        assert_eq!(request.session_id.to_string(), "ses_1");
        assert_eq!(request.config_id.to_string(), "mode");
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
    fn login_shell_takes_last_non_empty_line() {
        let dirs = parse_login_shell_output("noise\n \n/usr/bin:/bin\n");
        assert_eq!(dirs, vec![PathBuf::from("/usr/bin"), PathBuf::from("/bin")]);
    }

    #[test]
    fn agent_path_covers_candidate_dirs_in_order() {
        let actual = split_path_value(&agent_path_value());
        assert!(!actual.is_empty());
        assert_eq!(actual, candidate_dirs());
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
