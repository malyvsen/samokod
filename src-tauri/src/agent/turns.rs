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
    /// done or failed. The scoping first turn takes the user's text and
    /// prefixes the template server-side as one message. A reserved
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
            // Working title from the user's text before the server prefix.
            // Set-once; headings win; empty stays `Untitled`.
            store_working_title(&self.state, &repo_root, &session.plan, &text);
            let pending_match = match self.state.lock() {
                Ok(state) => state.pending_scoping.as_deref() == Some(session.plan.as_str()),
                Err(error) => {
                    log::warn!("failed to check pending session: {error}");
                    false
                }
            };
            if pending_match {
                plans::materialize_scoping(&repo_root, &session.plan)?;
                carry_pending_title(&self.state, &repo_root, &session.plan);
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
            if state.is_working(&key) {
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
    /// prefixes twice. Every active role prefixes; scoping uses the
    /// repo-relative display path since it runs with cwd at the repo root,
    /// while executing and landing use absolute paths into the checkout.
    fn claim_role_prefix(&self, key: &SessionKey, user_text: &str) -> Option<String> {
        let mut state = lock_state(&self.state)?;
        let repo_root = state.repo_root.clone()?;
        let live = state.sessions.get(key)?;
        if live.plan.prefixed {
            return None;
        }
        let plan = live.plan.clone();
        let text = match plan.phase {
            plans::Phase::Scoping => {
                let display = opencode::plan_display(&plan.plan_ref());
                prefixed_first_message(&opencode::scoping_template(&display), user_text)
            }
            plans::Phase::Executing => {
                let plan_dir_abs = plan
                    .plan_ref()
                    .path(&repo_root)
                    .to_string_lossy()
                    .to_string();
                prefixed_first_message(&opencode::executing_first_message(&plan_dir_abs), user_text)
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
                prefixed_first_message(
                    &opencode::landing_first_message(
                        &worktree_branch,
                        &target_branch,
                        &path.to_string_lossy(),
                        &plan_md_abs,
                    ),
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
        for bubble in replay_bubbles(key, &chunk) {
            emit_event(
                app,
                AppEvent::UserText {
                    session: key.clone(),
                    chunk: bubble,
                },
            );
        }
    }
    match &notification.update {
        acp::SessionUpdate::ToolCall(call) => {
            let merged = lock_state(state).and_then(|mut guard| {
                guard.sessions.get_mut(key).map(|session| {
                    session
                        .tool_calls
                        .insert(call.tool_call_id.to_string(), call.clone());
                    call.clone()
                })
            });
            let Some(merged) = merged else {
                log::warn!("tool call {} has no live session", call.tool_call_id);
                return;
            };
            // Todo-carrying calls render as a TODOS block instead of a `▸` line.
            if let Some(fresh) = todos_from_call(merged.raw_input.as_ref()) {
                log::debug!("todo call {} todos {}", call.tool_call_id, fresh.len());
                update_todos(state, app, key, fresh);
            } else {
                let line = crate::updates::format_tool_line(&merged);
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
            let merged = lock_state(state).and_then(|mut guard| {
                guard.sessions.get_mut(key).map(|session| {
                    let existing = session
                        .tool_calls
                        .get(&update.tool_call_id.to_string())
                        .cloned();
                    let merged = crate::updates::with_update(existing, update);
                    session
                        .tool_calls
                        .insert(update.tool_call_id.to_string(), merged.clone());
                    merged
                })
            });
            let Some(merged) = merged else {
                log::warn!("tool call {} has no live session", update.tool_call_id);
                return;
            };
            if let Some(fresh) =
                todos_from_update(merged.raw_input.as_ref(), merged.raw_output.as_ref())
            {
                log::debug!("todo update {} todos {}", update.tool_call_id, fresh.len());
                update_todos(state, app, key, fresh);
            } else {
                let line = crate::updates::format_tool_line(&merged);
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

/// Store a working title from the scoping message text. Set-once: skips
/// when a `plan.md` heading exists or a title is already stored (disk or
/// pending map). Stores to disk when materialized, to the pending map
/// otherwise. Empty prompts stay `Untitled` via extractor `None`.
fn store_working_title(
    state: &Mutex<State>,
    repo_root: &std::path::Path,
    plan_name: &str,
    user_text: &str,
) {
    let plan_ref = plans::PlanRef {
        name: plan_name.to_string(),
        phase: plans::Phase::Scoping,
    };
    let plan_dir = plan_ref.path(repo_root);
    if plan_ref.has_plan_md(repo_root) {
        let text = std::fs::read_to_string(plan_ref.plan_md(repo_root)).unwrap_or_default();
        if plans::extract_title(&text).is_some() {
            return;
        }
    }
    if plan_dir.is_dir() {
        let stored = plans::load_state(&plan_dir);
        if stored.working_title.is_some() {
            return;
        }
    }
    if lock_state(state)
        .map(|guard| guard.pending_titles.contains_key(plan_name))
        .unwrap_or(false)
    {
        return;
    }
    let Some(title) = crate::working_title::extract_working_title(user_text) else {
        return;
    };
    if plan_dir.is_dir() {
        let mut stored = plans::load_state(&plan_dir);
        if stored.working_title.is_none() {
            stored.working_title = Some(title);
            plans::store_state(&plan_dir, &stored);
        }
    } else if let Some(mut guard) = lock_state(state) {
        guard.pending_titles.insert(plan_name.to_string(), title);
    }
}

/// Move a pending working title into `state.json` at materialize time.
/// Set-once: never overwrites a disk title.
fn carry_pending_title(state: &Mutex<State>, repo_root: &std::path::Path, plan_name: &str) {
    let title = lock_state(state).and_then(|mut guard| guard.pending_titles.remove(plan_name));
    let Some(title) = title else {
        return;
    };
    let plan_ref = plans::PlanRef {
        name: plan_name.to_string(),
        phase: plans::Phase::Scoping,
    };
    if !plan_ref.path(repo_root).is_dir() {
        if let Some(mut guard) = lock_state(state) {
            guard.pending_titles.insert(plan_name.to_string(), title);
        }
        return;
    }
    let mut stored = plans::load_state(&plan_ref.path(repo_root));
    if stored.working_title.is_none() {
        stored.working_title = Some(title);
        plans::store_state(&plan_ref.path(repo_root), &stored);
    }
}

/// First message of one ACP conversation: the role template plus the
/// user's text. Pure.
fn prefixed_first_message(template: &str, user_text: &str) -> String {
    format!("{}\n\n{}", template, user_text.trim())
}

/// Replayed user text as display bubbles: a scoping first prompt carries
/// its template, so it splits back into template and message; anything
/// else is one bubble. Pure.
fn replay_bubbles(key: &SessionKey, chunk: &str) -> Vec<String> {
    if key.role != SessionRole::Scoping {
        return vec![chunk.to_string()];
    }
    let plan_ref = plans::PlanRef {
        name: key.plan.clone(),
        phase: plans::Phase::Scoping,
    };
    let template = opencode::scoping_template(&opencode::plan_display(&plan_ref));
    match split_scoping_replay(&template, chunk) {
        Some((first, second)) if !second.is_empty() => vec![first, second],
        Some((first, _)) => vec![first],
        None => vec![chunk.to_string()],
    }
}

/// Split a scoping first prompt into its template and message parts.
/// Returns `None` when the text carries no template (e.g. a manually
/// edited template). Pure.
fn split_scoping_replay(template: &str, text: &str) -> Option<(String, String)> {
    let remainder = text.strip_prefix(template)?.strip_prefix("\n\n")?;
    Some((template.to_string(), remainder.to_string()))
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
    fn prefixed_message_combines_template_and_user_text() {
        let template = "ROLE TEMPLATE";
        let text = prefixed_first_message(template, "  do things  ");
        assert_eq!(text, format!("{template}\n\ndo things"));
    }

    #[test]
    fn scoping_replay_splits_prefixed_message() {
        let display = ".samokod/plans/scoping/ts";
        let template = opencode::scoping_template(display);
        let full = format!("{template}\n\ndo things");
        let key = SessionKey {
            plan: "ts".to_string(),
            role: SessionRole::Scoping,
        };
        assert_eq!(
            replay_bubbles(&key, &full),
            vec![template.clone(), "do things".to_string()]
        );
        let (first, second) =
            split_scoping_replay(&template, &full).expect("splits prefixed message");
        assert_eq!(first, template);
        assert_eq!(second, "do things");
    }

    #[test]
    fn scoping_replay_keeps_edited_template_whole() {
        let display = ".samokod/plans/scoping/ts";
        let template = opencode::scoping_template(display);
        assert!(split_scoping_replay(&template, "edited template text").is_none());
        assert!(split_scoping_replay(&template, &template).is_none());
    }

    #[test]
    fn working_title_set_once_across_two_prompts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        plans::materialize_scoping(root, name).expect("materialize");
        let state = Mutex::new(State::default());
        let first = "Fix the login flow login errors on login retry.";
        store_working_title(&state, root, name, first);
        let stored = plans::load_state(
            &plans::PlanRef {
                name: name.to_string(),
                phase: plans::Phase::Scoping,
            }
            .path(root),
        );
        let title = stored.working_title.clone().expect("title stored");
        assert!(title.to_lowercase().contains("login"));
        let second = "Fix the checkout flow checkout errors on checkout retry.";
        store_working_title(&state, root, name, second);
        let kept = plans::load_state(
            &plans::PlanRef {
                name: name.to_string(),
                phase: plans::Phase::Scoping,
            }
            .path(root),
        );
        assert_eq!(kept.working_title, Some(title));
    }

    #[test]
    fn working_title_pending_carries_to_disk_on_first_send() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let state = Mutex::new(State::default());
        let text = "Fix the login flow login errors on login retry.";
        store_working_title(&state, root, name, text);
        assert!(
            state
                .lock()
                .expect("state")
                .pending_titles
                .contains_key(name)
        );
        plans::materialize_scoping(root, name).expect("materialize");
        carry_pending_title(&state, root, name);
        assert!(
            !state
                .lock()
                .expect("state")
                .pending_titles
                .contains_key(name)
        );
        let stored = plans::load_state(
            &plans::PlanRef {
                name: name.to_string(),
                phase: plans::Phase::Scoping,
            }
            .path(root),
        );
        assert!(
            stored
                .working_title
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains("login")
        );
    }

    #[test]
    fn empty_first_prompt_stays_untitled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        plans::materialize_scoping(root, name).expect("materialize");
        let state = Mutex::new(State::default());
        store_working_title(&state, root, name, "   \n  ");
        let stored = plans::load_state(
            &plans::PlanRef {
                name: name.to_string(),
                phase: plans::Phase::Scoping,
            }
            .path(root),
        );
        assert_eq!(stored.working_title, None);
    }
}
