// Permissions: answering approval cards and routing agent permission
// requests to per-session cards.
use std::sync::Mutex;

use tauri::AppHandle;

use crate::acp;
use crate::types::{AgentError, AppEvent, PermissionView, SessionKey};

use super::AgentManager;
use super::State;
use super::plans_list::push_sorted;
use super::session::PermissionDecision;
use super::{emit_event, lock_state, set_approval};

impl AgentManager {
    /// Answer one permission card. `Some` selects the allow-once or reject
    /// option; `None` answers cancelled.
    pub fn answer_permission(
        &self,
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
            .map(|mut guard| {
                guard
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

pub(crate) fn cancel_pending(state: &Mutex<State>, key: &SessionKey) {
    let senders: Vec<tokio::sync::oneshot::Sender<PermissionDecision>> = match state.lock() {
        Ok(mut guard) => guard
            .sessions
            .get_mut(key)
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
    state: &Mutex<State>,
    app: &AppHandle,
    key: &SessionKey,
    request: acp::RequestPermissionRequest,
    responder: agent_client_protocol::Responder<acp::RequestPermissionResponse>,
) {
    let current = lock_state(state).and_then(|guard| {
        guard
            .sessions
            .get(key)
            .and_then(|session| session.session_id.clone())
    });
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
        let Some(session) = guard.sessions.get_mut(key) else {
            if responder
                .respond(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ))
                .is_err()
            {
                log::debug!("permission responder gone for missing session");
            }
            return;
        };
        session.pending.insert(tool_call_id.clone(), tx);
    }
    set_approval(state, key, true);
    push_sorted(state, app);
    emit_event(
        app,
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
    if let Some(mut guard) = lock_state(state)
        && let Some(session) = guard.sessions.get_mut(key)
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
    set_approval(state, key, false);
    push_sorted(state, app);
    emit_event(
        app,
        AppEvent::PermissionResolved {
            session: key.clone(),
            tool_call_id,
        },
    );
}
