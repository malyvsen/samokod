// Turns: sending prompts, streaming them as events, retry, and cancel.
// Each turn records its prompt so retry resends exactly what ran.
use std::sync::{Arc, Mutex};

use tauri::AppHandle;

use crate::acp::{self, Agent, ConnectionTo};
use crate::error_hint::{classify_error, is_transport_error};
use crate::opencode;
use crate::plans;
use crate::spend::context_pct;
use crate::todos::{diff_todos, todos_from_call, todos_from_update};
use crate::types::{AgentError, AppEvent, SessionKey, SessionRole, TodoView};

use super::AgentManager;
use super::State;
use super::config::config_views;
use super::permissions::cancel_pending;
use super::plans_list::push_sorted;
use super::{emit_event, lock_state, set_failed, set_working};

impl AgentManager {
    /// Resend the last prompt. When the transport is closed, reopen the
    /// session on the held plan first. Returns false when nothing ran.
    pub async fn retry_last(&self, session: SessionKey) -> Result<bool, AgentError> {
        let Some(text) = lock_state(&self.state)
            .and_then(|state| state.sessions.get(&session)?.last_prompt.clone())
        else {
            return Ok(false);
        };
        let (connection, session_id) = self.ensure_live(&session).await?;
        self.start_turn(connection, session_id, session, text)
            .await?;
        Ok(true)
    }

    /// Send one plain-text prompt, spawning the session lazily on its
    /// first message. Streams arrive as events; the turn end arrives as
    /// done or failed. The role template prefixes the first message per
    /// ACP conversation; the transcript keeps the raw text. A reserved
    /// pending scoping session materializes its directory here, before
    /// the unchanged `ensure_live` path.
    pub async fn send_prompt(&self, session: SessionKey, text: String) -> Result<(), AgentError> {
        if session.role == SessionRole::Scoping {
            let repo_root = self
                .reopen_snapshot()
                .map(|(root, _)| root)
                .ok_or_else(|| AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                })?;
            let pending_match = match self.state.lock() {
                Ok(state) => state.pending_scoping.as_deref() == Some(session.plan.as_str()),
                Err(error) => {
                    log::warn!("failed to check pending session: {error}");
                    false
                }
            };
            if pending_match {
                plans::materialize_scoping(&repo_root, &session.plan)?;
                match self.state.lock() {
                    Ok(mut state) => {
                        if state.pending_scoping.as_deref() == Some(session.plan.as_str()) {
                            state.pending_scoping = None;
                        }
                    }
                    Err(error) => {
                        log::warn!("failed to clear pending session: {error}");
                    }
                }
            }
        }
        let (connection, session_id) = self.ensure_live(&session).await?;
        // A fresh prompt clears the failed flag; the dot goes green while
        // the turn runs.
        if let Some(mut state) = lock_state(&self.state)
            && let Some(live) = state.sessions.get_mut(&session)
        {
            live.failed = false;
        }
        self.select_key(session.clone());
        self.touch_activity(&session.plan);
        let mut text = text;
        if let Some(prefixed) = self.claim_role_prefix(&session, &text) {
            text = prefixed;
        }
        self.start_turn(connection, session_id, session, text).await
    }

    /// Guard one turn and stream it as events. Records the prompt so retry
    /// resends exactly what ran, prefixed or not.
    pub(crate) async fn start_turn(
        &self,
        connection: ConnectionTo<Agent>,
        session_id: String,
        key: SessionKey,
        text: String,
    ) -> Result<(), AgentError> {
        {
            let mut state = self.state.lock().expect("state poisoned");
            let busy = state
                .sessions
                .get(&key)
                .map(|session| session.working)
                .unwrap_or(false);
            if busy {
                return Err(AgentError::RequestFailed {
                    raw: "a turn is already running".to_string(),
                });
            }
            state.set_working(&key, true);
            if let Some(session) = state.sessions.get_mut(&key) {
                session.last_prompt = Some(text.clone());
            }
        }
        // Dots go green while the turn runs; titles and order refresh when
        // it lands.
        self.push_plans();
        let state = Arc::clone(&self.state);
        let app = self.app.clone();
        let push_state = Arc::clone(&self.state);
        let push_app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let prompt = acp::PromptRequest::new(
                acp::SessionId::new(session_id),
                vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
            );
            match connection.send_request(prompt).block_task().await {
                Ok(_) => {
                    set_working(&state, &key, false);
                    set_failed(&state, &key, false);
                    emit_event(
                        &app,
                        AppEvent::TurnDone {
                            session: key.clone(),
                        },
                    );
                    push_sorted(&push_state, &push_app);
                }
                Err(error) => {
                    let raw = error.to_string();
                    let transport_gone = is_transport_error(&raw);
                    set_working(&state, &key, false);
                    set_failed(&state, &key, true);
                    if transport_gone {
                        match state.lock() {
                            Ok(mut guard) => {
                                if let Some(session) = guard.sessions.get_mut(&key) {
                                    session.connection = None;
                                    session.session_id = None;
                                }
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
                            session: key.clone(),
                            raw,
                            hint: hint.text,
                            retryable: hint.retryable,
                        },
                    );
                    push_sorted(&push_state, &push_app);
                }
            }
        });
        Ok(())
    }

    /// Stop the turn via `session/cancel`, answering every open card as
    /// cancelled per protocol.
    pub async fn cancel_turn(&self, session: SessionKey) -> Result<(), AgentError> {
        let (connection, session_id, _, _) =
            self.session_snapshot_for(&session)
                .ok_or_else(|| AgentError::NoSession {
                    raw: "no active turn".to_string(),
                })?;
        cancel_pending(&self.state, &session);
        if let Err(error) = connection.send_notification(acp::build_cancel_notification(
            acp::SessionId::new(session_id),
        )) {
            log::warn!("failed to send session cancel: {error}");
        }
        set_working(&self.state, &session, false);
        self.push_plans();
        Ok(())
    }

    /// Claim the one-time role prefix for the first message of one ACP
    /// conversation. One locked check-and-mark, so a retried turn never
    /// prefixes twice. Covers both roles: a restored execution session
    /// gets the executor template again, mirroring the planner path.
    fn claim_role_prefix(&self, key: &SessionKey, user_text: &str) -> Option<String> {
        let mut state = lock_state(&self.state)?;
        let live = state.sessions.get_mut(key)?;
        if live.plan.prefixed {
            return None;
        }
        let text = role_prefix_text(
            live.plan.phase,
            &opencode::plan_display(&live.plan.plan_ref()),
            user_text,
        )?;
        live.plan.prefixed = true;
        Some(text)
    }
}

pub(crate) fn handle_notification(
    state: &Mutex<State>,
    app: &AppHandle,
    key: &SessionKey,
    notification: &acp::SessionNotification,
) {
    let current = lock_state(state).and_then(|guard| guard.sessions.get(key)?.session_id.clone());
    if let Some(current) = current
        && notification.session_id.to_string() != current
    {
        log::debug!(
            "dropping notification for stale session {}",
            notification.session_id
        );
        return;
    }
    if let Some(chunk) = crate::updates::agent_text_of(&notification.update) {
        emit_event(
            app,
            AppEvent::AgentText {
                session: key.clone(),
                chunk,
            },
        );
    }
    match &notification.update {
        acp::SessionUpdate::ToolCall(call) => {
            let line = crate::updates::format_tool_line(call);
            emit_event(
                app,
                AppEvent::ToolLine {
                    session: key.clone(),
                    line,
                },
            );
            snoop_todos_from_call(state, app, key, call);
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            let line = crate::updates::format_tool_update(update);
            emit_event(
                app,
                AppEvent::ToolLine {
                    session: key.clone(),
                    line,
                },
            );
            snoop_todos_from_update(state, app, key, update);
        }
        acp::SessionUpdate::UsageUpdate(update) => {
            emit_event(
                app,
                AppEvent::SpendTick {
                    session: key.clone(),
                    cost: update.cost.as_ref().map(|cost| cost.amount).unwrap_or(0.0),
                    ctx_pct: context_pct(update.used, update.size),
                },
            );
        }
        acp::SessionUpdate::ConfigOptionUpdate(update) => {
            let options = config_views(&update.config_options);
            emit_event(
                app,
                AppEvent::ConfigOptions {
                    session: key.clone(),
                    options,
                },
            );
        }
        acp::SessionUpdate::AgentMessageChunk(_) => {}
        // Already streamed as agent text above. Known-but-unrendered kinds
        // are routine; an unknown kind means protocol drift.
        update => match crate::updates::update_kind(update) {
            Some(kind) => log::debug!("unhandled session update: {kind}"),
            None => log::warn!("unknown session update: {update:?}"),
        },
    }
}

/// Snoop a todo list off a tool call input, when it carries one.
fn snoop_todos_from_call(
    state: &Mutex<State>,
    app: &AppHandle,
    key: &SessionKey,
    call: &acp::ToolCall,
) {
    if let Some(fresh) = todos_from_call(call.raw_input.as_ref()) {
        log::debug!(
            "todo snoop call {} todos {}",
            call.tool_call_id,
            fresh.len()
        );
        update_todos(state, app, key, fresh);
    }
}

/// Snoop a todo list off a tool update, preferring the output over the input.
fn snoop_todos_from_update(
    state: &Mutex<State>,
    app: &AppHandle,
    key: &SessionKey,
    update: &acp::ToolCallUpdate,
) {
    if let Some(fresh) = todos_from_update(
        update.fields.raw_input.as_ref(),
        update.fields.raw_output.as_ref(),
    ) {
        log::debug!(
            "todo snoop update {} todos {}",
            update.tool_call_id,
            fresh.len()
        );
        update_todos(state, app, key, fresh);
    }
}

/// Role template for the first message of one ACP conversation. Pure:
/// restored sessions of either role get their template again, so a fresh
/// ACP conversation still knows its job; finished plans never prefix.
fn role_prefix_text(phase: plans::Phase, display: &str, user_text: &str) -> Option<String> {
    match phase {
        plans::Phase::Scoping => Some(opencode::planner_first_message(display, user_text)),
        plans::Phase::Executing => Some(format!(
            "{}\n\n{}",
            opencode::executor_first_message(display),
            user_text.trim()
        )),
        plans::Phase::Completed | plans::Phase::Cancelled => None,
    }
}

/// Replace the held list with a fresh todo list and emit when it moved.
/// Identical lists stay silent.
fn update_todos(state: &Mutex<State>, app: &AppHandle, key: &SessionKey, fresh: Vec<TodoView>) {
    let Some(mut guard) = lock_state(state) else {
        return;
    };
    let Some(session) = guard.sessions.get_mut(key) else {
        return;
    };
    if fresh == session.todos {
        return;
    }
    let changes = diff_todos(&session.todos, &fresh);
    session.todos = fresh.clone();
    drop(guard);
    emit_event(
        app,
        AppEvent::TodosChanged {
            session: key.clone(),
            todos: fresh,
            changes,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plans::Phase;

    #[test]
    fn scoping_prefix_combines_role_and_user_text() {
        let display = ".samokod/plans/scoping/ts";
        let text =
            role_prefix_text(Phase::Scoping, display, "  do things  ").expect("scoping prefixes");
        assert_eq!(
            text,
            opencode::planner_first_message(display, "  do things  ")
        );
    }

    #[test]
    fn executing_prefix_combines_role_and_user_text() {
        let display = ".samokod/plans/executing/ts.slug";
        let text = role_prefix_text(Phase::Executing, display, "  do things  ")
            .expect("executing prefixes");
        assert!(text.contains(&opencode::executor_first_message(display)));
        assert!(text.contains("do things"));
    }

    #[test]
    fn finished_phases_never_prefix() {
        for phase in [Phase::Completed, Phase::Cancelled] {
            assert!(role_prefix_text(phase, "dir", "hi").is_none());
        }
    }
}
