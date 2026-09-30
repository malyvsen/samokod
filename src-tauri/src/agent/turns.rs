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
        self.wait_for_history(&session).await?;
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
    /// done or failed. Scoping text goes through verbatim: the draft bubble
    /// prefills the template as editable user content. A reserved
    /// pending scoping session materializes its directory here, before
    /// the `ensure_live` path.
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
        // A prompt never races a replay: it waits for an in-flight load
        // rather than interleaving live chunks with history chunks.
        self.wait_for_history(&session).await?;
        let (connection, session_id) = self.ensure_live(&session).await?;
        // A warmed pending session spawns before its directory exists, so
        // its ID goes unrecorded; persisting here heals it once the first
        // prompt materializes the directory, along with any missing file.
        if let Some((repo_root, _)) = self.reopen_snapshot() {
            super::session_ids::record(&repo_root, &session, &session_id);
        }
        // A fresh prompt clears the failed flag; the dot goes green while
        // the turn runs.
        if let Some(mut state) = lock_state(&self.state)
            && let Some(live) = state.sessions.get_mut(&session)
        {
            live.failed = false;
        }
        self.select_key(session.clone());
        self.mark_prompted(&session.plan);
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
    /// prefixes twice. Executing and landing prefix; scoping goes through
    /// verbatim. Paths are absolute into the main checkout, since both
    /// roles run with the worktree as their working directory.
    fn claim_role_prefix(&self, key: &SessionKey, user_text: &str) -> Option<String> {
        let mut state = lock_state(&self.state)?;
        let repo_root = state.repo_root.clone()?;
        let live = state.sessions.get(key)?;
        if live.plan.prefixed {
            return None;
        }
        let plan = live.plan.clone();
        let text = match plan.phase {
            plans::Phase::Executing => {
                let plan_dir_abs = plan
                    .plan_ref()
                    .path(&repo_root)
                    .to_string_lossy()
                    .to_string();
                executing_prefix_text(&plan_dir_abs, user_text)
            }
            plans::Phase::Landing => {
                let path = match state.worktrees.get(&plan.name) {
                    Some(record) => record.path.clone(),
                    None => crate::worktrees::worktree_path(&repo_root, &plan.name),
                };
                let worktree_branch = crate::worktrees::branch_name(&plan.name);
                let target_branch = state.checkout_branch.clone();
                let plan_md_abs = plan
                    .plan_ref()
                    .plan_md(&repo_root)
                    .to_string_lossy()
                    .to_string();
                landing_prefix_text(
                    &worktree_branch,
                    &target_branch,
                    &path.to_string_lossy(),
                    &plan_md_abs,
                    user_text,
                )
            }
            _ => return None,
        };
        state.sessions.get_mut(key)?.plan.prefixed = true;
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
    // Replayed user messages only exist while history replays. Live turns
    // append the user bubble optimistically, so mapping them outside a
    // replay would double-add every prompt.
    if let Some(chunk) = crate::updates::user_text_of(&notification.update)
        && lock_state(state)
            .map(|guard| guard.is_history_loading(key))
            .unwrap_or(false)
    {
        emit_event(
            app,
            AppEvent::UserText {
                session: key.clone(),
                chunk,
            },
        );
    }
    match &notification.update {
        acp::SessionUpdate::ToolCall(call) => {
            // Todo-carrying calls render as a TODOS block instead of a `▸` line.
            if let Some(fresh) = todos_from_call(call.raw_input.as_ref()) {
                log::debug!("todo call {} todos {}", call.tool_call_id, fresh.len());
                update_todos(state, app, key, fresh);
            } else {
                let line = crate::updates::format_tool_line(call);
                emit_event(
                    app,
                    AppEvent::ToolLine {
                        session: key.clone(),
                        line,
                    },
                );
            }
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            if let Some(fresh) = todos_from_update(
                update.fields.raw_input.as_ref(),
                update.fields.raw_output.as_ref(),
            ) {
                log::debug!("todo update {} todos {}", update.tool_call_id, fresh.len());
                update_todos(state, app, key, fresh);
            } else {
                let line = crate::updates::format_tool_update(update);
                emit_event(
                    app,
                    AppEvent::ToolLine {
                        session: key.clone(),
                        line,
                    },
                );
            }
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

/// Executing role template for the first message of one ACP
/// conversation. Pure.
fn executing_prefix_text(display: &str, user_text: &str) -> String {
    format!(
        "{}\n\n{}",
        opencode::executing_first_message(display),
        user_text.trim()
    )
}

/// Landing role template for the first message of one ACP conversation.
/// Pure.
fn landing_prefix_text(
    worktree_branch: &str,
    target_branch: &str,
    worktree_path: &str,
    plan_md_abs_path: &str,
    user_text: &str,
) -> String {
    format!(
        "{}\n\n{}",
        opencode::landing_first_message(
            worktree_branch,
            target_branch,
            worktree_path,
            plan_md_abs_path
        ),
        user_text.trim()
    )
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

    #[test]
    fn executing_prefix_combines_role_and_user_text() {
        let display = ".samokod/plans/executing/ts.slug";
        let text = executing_prefix_text(display, "  do things  ");
        assert!(text.contains(&opencode::executing_first_message(display)));
        assert!(text.contains("do things"));
    }

    #[test]
    fn landing_prefix_combines_role_and_user_text() {
        let text = landing_prefix_text(
            "samokod/shiny",
            "feature",
            "/repo/.samokod/worktrees/plan",
            "/repo/.samokod/plans/landing/plan/plan.md",
            "  keep going  ",
        );
        assert!(text.contains("samokod/shiny"));
        assert!(text.contains("keep going"));
    }
}
