// Failure taxonomy: the raw agent output stays visible and a short hint is
// appended underneath it.
use serde::{Deserialize, Serialize};

/// Appended hint for a failure row.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorHintKind {
    MissingBinary,
    Auth,
    RateLimit,
    ApprovalStalled,
    NoSavedSession,
    StillStarting,
    PlainRetry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorHint {
    pub kind: ErrorHintKind,
    pub text: String,
    pub retryable: bool,
}

/// Classify raw agent output into an appended hint. Pure.
pub fn classify_error(raw: &str) -> ErrorHint {
    let lowered = raw.to_lowercase();
    if lowered.contains("no saved session") {
        return ErrorHint {
            kind: ErrorHintKind::NoSavedSession,
            text:
                "this plan's history is unavailable - it predates session recording or was pruned"
                    .to_string(),
            retryable: false,
        };
    }
    if lowered.contains("opencode binary not found") || lowered.contains("missing binary") {
        return ErrorHint {
            kind: ErrorHintKind::MissingBinary,
            text: "opencode is not installed - install it with npm install -g opencode-ai, then reopen the repo"
                .to_string(),
            retryable: false,
        };
    }
    if lowered.contains("401")
        || lowered.contains("unauthorized")
        || lowered.contains("auth")
        || lowered.contains("/connect")
    {
        return ErrorHint {
            kind: ErrorHintKind::Auth,
            text: "looks like auth - run /connect in opencode, then retry".to_string(),
            retryable: true,
        };
    }
    if lowered.contains("429")
        || lowered.contains("rate limit")
        || lowered.contains("too many requests")
    {
        return ErrorHint {
            kind: ErrorHintKind::RateLimit,
            text: "rate limited - wait a minute, then retry".to_string(),
            retryable: true,
        };
    }
    if lowered.contains("permission queue stalled")
        || lowered.contains("request_permission") && lowered.contains("stalled")
    {
        return ErrorHint {
            kind: ErrorHintKind::ApprovalStalled,
            text: "the approval card above may be out of view - scroll up to answer it".to_string(),
            retryable: true,
        };
    }
    if lowered.contains("still starting") || lowered.contains("still loading") {
        return ErrorHint {
            kind: ErrorHintKind::StillStarting,
            text: "the agent is still starting - wait a moment, then retry".to_string(),
            retryable: true,
        };
    }
    ErrorHint {
        kind: ErrorHintKind::PlainRetry,
        text: "retry the turn".to_string(),
        retryable: true,
    }
}

/// True when the turn failed because the transport is gone and the session
/// must be reopened before the next prompt.
pub fn is_transport_error(raw: &str) -> bool {
    let lowered = raw.to_lowercase();
    lowered.contains("transport")
        || lowered.contains("incoming_closed")
        || lowered.contains("connection closed")
        || lowered.contains("broken pipe")
        || lowered.contains("exited")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_hint_for_401() {
        let hint = classify_error(
            "opencode acp exited (code 1): provider anthropic returned 401 Unauthorized",
        );
        assert_eq!(hint.kind, ErrorHintKind::Auth);
        assert!(hint.retryable);
        assert!(hint.text.contains("/connect"));
    }

    #[test]
    fn missing_binary_has_no_retry() {
        let hint = classify_error("opencode binary not found");
        assert_eq!(hint.kind, ErrorHintKind::MissingBinary);
        assert!(!hint.retryable);
    }

    #[test]
    fn rate_limit_hint() {
        let hint = classify_error("request failed with 429 rate limit");
        assert_eq!(hint.kind, ErrorHintKind::RateLimit);
    }

    #[test]
    fn stalled_permission_hint() {
        let hint = classify_error(
            "permission queue stalled 30s waiting on session/request_permission for tool call_9f3a",
        );
        assert_eq!(hint.kind, ErrorHintKind::ApprovalStalled);
    }

    #[test]
    fn still_starting_hint_for_start_waits() {
        for raw in [
            "agent is still starting, try again",
            "history is still loading, try again",
        ] {
            let hint = classify_error(raw);
            assert_eq!(hint.kind, ErrorHintKind::StillStarting);
            assert!(hint.retryable);
            assert!(hint.text.contains("still starting"));
        }
    }

    #[test]
    fn unknown_falls_back_to_retry() {
        let hint = classify_error("something odd happened");
        assert_eq!(hint.kind, ErrorHintKind::PlainRetry);
        assert!(hint.retryable);
    }

    #[test]
    fn no_saved_session_has_no_retry() {
        let hint = classify_error(
            "no saved session found for this plan - it predates session recording or its session was pruned",
        );
        assert_eq!(hint.kind, ErrorHintKind::NoSavedSession);
        assert!(!hint.retryable);
        assert!(!hint.text.to_lowercase().contains("plan is gone"));
    }

    #[test]
    fn closed_transport_needs_reopen() {
        assert!(is_transport_error("transport incoming_closed"));
        assert!(!is_transport_error("provider returned 401 Unauthorized"));
    }
}
