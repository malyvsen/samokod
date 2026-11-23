// Permissions: answering approval cards and routing agent permission
// requests to per-session cards.
use std::sync::Mutex;

use tauri::AppHandle;

use crate::acp;
use crate::types::{AgentError, AppEvent, PermissionOptionView, PermissionView, SessionKey};

use super::AgentManager;
use super::BackendState;
use super::plans_list::push_sorted;
use super::session::PermissionDecision;
use super::{emit_event, lock_backend, set_approval};

impl AgentManager {
    /// Answer one permission card. `Some` selects the allow-once or reject
    /// option; `None` answers cancelled.
    pub fn answer_permission(
        &self,
        window: &str,
        session: SessionKey,
        tool_call_id: &str,
        option_id: Option<String>,
    ) -> Result<(), AgentError> {
        let sender = self
            .state
            .lock()
            .map_err(|_| AgentError::RequestFailed {
                raw: "permission state poisoned".to_string(),
            })
            .map(|mut backend| {
                backend
                    .windows
                    .get_mut(window)?
                    .sessions
                    .get_mut(&session)
                    .and_then(|live| live.pending.remove(tool_call_id))
            });
        match sender {
            Ok(Some(sender)) => {
                let decision = match option_id {
                    Some(id) => PermissionDecision::Selected(id),
                    None => PermissionDecision::Cancelled,
                };
                if sender.send(decision).is_err() {
                    log::debug!("permission decision receiver gone for {tool_call_id}");
                }
                Ok(())
            }
            Ok(None) => Err(AgentError::RequestFailed {
                raw: format!("no pending permission for {tool_call_id}"),
            }),
            Err(error) => Err(error),
        }
    }
}

pub(crate) fn cancel_pending(state: &Mutex<BackendState>, window: &str, key: &SessionKey) {
    let senders: Vec<tokio::sync::oneshot::Sender<PermissionDecision>> = match state.lock() {
        Ok(mut backend) => backend
            .windows
            .get_mut(window)
            .and_then(|win| win.sessions.get_mut(key))
            .map(|session| session.pending.drain().map(|(_, sender)| sender).collect())
            .unwrap_or_default(),
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

pub(crate) async fn handle_permission_request(
    state: &Mutex<BackendState>,
    app: &AppHandle,
    window: &str,
    key: &SessionKey,
    request: acp::RequestPermissionRequest,
    responder: agent_client_protocol::Responder<acp::RequestPermissionResponse>,
    expected: &super::WindowIncarnation,
) {
    let is_current = super::check_current(state, window, expected);
    let current = lock_backend(state).and_then(|backend| {
        backend
            .windows
            .get(window)?
            .sessions
            .get(key)
            .and_then(|session| session.session_id.clone())
    });
    let session_matches = match current {
        Some(current) => request.session_id.to_string() == current,
        None => true,
    };
    let options = crate::permissions::to_view(&request.options);
    let has_options = !options.is_empty();
    let is_replaying = lock_backend(state)
        .map(|backend| {
            backend
                .windows
                .get(window)
                .map(|win| win.is_history_loading(key))
                .unwrap_or(false)
        })
        .unwrap_or(false);
    match route_permission(is_current, session_matches, has_options, is_replaying) {
        PermissionRoute::Cancel => {
            let context = if !is_current {
                "for stale incarnation"
            } else if !session_matches {
                "for stale session"
            } else {
                "for empty options"
            };
            answer_cancelled(responder, context);
            return;
        }
        PermissionRoute::Replay => {
            emit_replayed_approval(app, window, key, &request, options, responder);
            return;
        }
        PermissionRoute::Ask => {}
    }
    let permission = permission_view(&request, options);
    let tool_call_id = permission.tool_call_id.clone();
    let (tx, rx) = tokio::sync::oneshot::channel::<PermissionDecision>();
    {
        let Some(mut backend) = lock_backend(state) else {
            answer_cancelled(responder, "while state locked");
            return;
        };
        let Some(win) = backend.windows.get_mut(window) else {
            answer_cancelled(responder, "for closed window");
            return;
        };
        let Some(session) = win.sessions.get_mut(key) else {
            answer_cancelled(responder, "for missing session");
            return;
        };
        session.pending.insert(tool_call_id.clone(), tx);
    }
    set_approval(state, window, key, true);
    push_sorted(state, app, window);
    emit_event(
        app,
        window,
        AppEvent::PermissionAsked {
            session: key.clone(),
            permission,
        },
    );
    let decision = match rx.await {
        Ok(decision) => decision,
        Err(_) => {
            log::debug!("permission decision sender gone; treating as cancelled");
            PermissionDecision::Cancelled
        }
    };
    if let Some(mut backend) = lock_backend(state)
        && let Some(win) = backend.windows.get_mut(window)
        && let Some(session) = win.sessions.get_mut(key)
    {
        session.pending.remove(&tool_call_id);
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
    set_approval(state, window, key, false);
    push_sorted(state, app, window);
    emit_event(
        app,
        window,
        AppEvent::PermissionResolved {
            session: key.clone(),
            tool_call_id,
        },
    );
}

/// Inline card for one permission request. Pure.
fn permission_view(
    request: &acp::RequestPermissionRequest,
    options: Vec<PermissionOptionView>,
) -> PermissionView {
    let fields = &request.tool_call.fields;
    PermissionView {
        tool_call_id: request.tool_call.tool_call_id.to_string(),
        title: fields
            .title
            .clone()
            .unwrap_or_else(|| "run this action?".to_string()),
        tool: crate::updates::tool_label(fields.name.as_deref(), fields.kind),
        options,
        rule_hint: crate::permissions::rule_hint(fields.name.as_deref()),
    }
}

/// Answer one permission request as cancelled. The context names the call
/// site in the debug log so dropped responders stay traceable.
fn answer_cancelled(
    responder: agent_client_protocol::Responder<acp::RequestPermissionResponse>,
    context: &str,
) {
    if responder
        .respond(acp::RequestPermissionResponse::new(
            acp::RequestPermissionOutcome::Cancelled,
        ))
        .is_err()
    {
        log::debug!("permission responder gone {context}");
    }
}

/// Render a replayed approval as asked plus immediately resolved. History
/// never waits on the user, answers cancelled so the replay continues,
/// and leaves the approval flag alone so the dot stays truthful to the
/// replay.
fn emit_replayed_approval(
    app: &AppHandle,
    window: &str,
    key: &SessionKey,
    request: &acp::RequestPermissionRequest,
    options: Vec<PermissionOptionView>,
    responder: agent_client_protocol::Responder<acp::RequestPermissionResponse>,
) {
    let permission = permission_view(request, options);
    let tool_call_id = permission.tool_call_id.clone();
    emit_event(
        app,
        window,
        AppEvent::PermissionAsked {
            session: key.clone(),
            permission,
        },
    );
    answer_cancelled(responder, "for replayed approval");
    emit_event(
        app,
        window,
        AppEvent::PermissionResolved {
            session: key.clone(),
            tool_call_id,
        },
    );
}

/// Where one permission request goes. Pure so the stall fix stays testable
/// without an `AppHandle`.
#[derive(Debug, PartialEq, Eq)]
enum PermissionRoute {
    Ask,
    Cancel,
    Replay,
}

fn route_permission(
    is_current: bool,
    session_matches: bool,
    has_options: bool,
    is_replaying: bool,
) -> PermissionRoute {
    if !is_current {
        return PermissionRoute::Cancel;
    }
    if !session_matches {
        return PermissionRoute::Cancel;
    }
    if !has_options {
        return PermissionRoute::Cancel;
    }
    if is_replaying {
        return PermissionRoute::Replay;
    }
    PermissionRoute::Ask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_incarnation_cancels() {
        assert_eq!(
            route_permission(false, true, true, false),
            PermissionRoute::Cancel
        );
    }

    #[test]
    fn session_mismatch_cancels() {
        assert_eq!(
            route_permission(true, false, true, false),
            PermissionRoute::Cancel
        );
    }

    #[test]
    fn empty_options_cancel() {
        assert_eq!(
            route_permission(true, true, false, false),
            PermissionRoute::Cancel
        );
    }

    #[test]
    fn replaying_auto_resolves() {
        assert_eq!(
            route_permission(true, true, true, true),
            PermissionRoute::Replay
        );
    }

    #[test]
    fn live_request_proceeds() {
        assert_eq!(
            route_permission(true, true, true, false),
            PermissionRoute::Ask
        );
    }
}
