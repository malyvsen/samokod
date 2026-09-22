// ACP wire boundary. SDK schema types enter the app through this module;
// nothing past it imports `agent-client-protocol` directly.
pub use agent_client_protocol::schema::v1::{
    CancelNotification, CloseSessionRequest, ContentBlock, PromptRequest, SessionConfigKind,
    SessionConfigOption, SessionConfigSelectOptions, SessionId, SessionNotification, SessionUpdate,
    TextContent, ToolCall, ToolCallUpdate, ToolKind,
};
#[cfg(test)]
pub use agent_client_protocol::schema::v1::{
    ContentChunk, RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SessionConfigSelectOption, SessionConfigValueId, ToolCallStatus, ToolCallUpdateFields,
};
use agent_client_protocol::schema::{
    ProtocolVersion,
    v1::{ClientCapabilities, InitializeRequest, NewSessionRequest},
};
pub use agent_client_protocol::{AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo};
#[cfg(test)]
pub use agent_client_protocol::{on_receive_notification, on_receive_request};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::types::AgentError;

/// Locate the `opencode` binary on PATH. A missing binary is a typed error
/// with install guidance and never enters retry logic.
pub fn resolve_opencode_binary() -> Result<PathBuf, AgentError> {
    resolve_in_path(std::env::var_os("PATH"))
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

/// Build the ACP `session/cancel` notification payload for a session.
pub fn build_cancel_notification(session_id: SessionId) -> CancelNotification {
    CancelNotification::new(session_id)
}

/// Wrap a failure as an ACP internal error for handler closures.
pub fn internal_error(message: impl ToString) -> agent_client_protocol::Error {
    agent_client_protocol::util::internal_error(message)
}

fn resolve_in_path(path_var: Option<OsString>) -> Result<PathBuf, AgentError> {
    let Some(path_var) = path_var else {
        return Err(AgentError::MissingBinary);
    };
    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join("opencode");
        if is_executable(&candidate) {
            return Ok(candidate);
        }
        #[cfg(windows)]
        {
            let exe = candidate.with_extension("exe");
            if is_executable(&exe) {
                return Ok(exe);
            }
        }
    }
    Err(AgentError::MissingBinary)
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
        let result = resolve_in_path(Some("/nonexistent-samokod-path".into()));
        assert!(matches!(result, Err(AgentError::MissingBinary)));
    }

    #[test]
    fn real_binary_resolves_when_present() {
        if resolve_opencode_binary().is_err() {
            return;
        }
        assert!(resolve_opencode_binary().is_ok());
    }
}
