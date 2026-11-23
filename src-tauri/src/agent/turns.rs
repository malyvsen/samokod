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
use super::BackendState;
use super::WindowState;
use super::config::config_views;
use super::permissions::cancel_pending;
use super::plans_list::push_sorted;
use super::{check_current, emit_event, incarnation, lock_backend, set_failed, set_working};

impl AgentManager {
    /// Resend the last prompt. When the transport is closed, reopen the
    /// session on the held plan first. Returns false when nothing ran.
    pub async fn retry_last(&self, window: &str, session: SessionKey) -> Result<bool, AgentError> {
        log::info!(
            "retry_last window={window} session={}::{:?}",
            session.plan,
            session.role
        );
        self.wait_for_history(window, &session).await?;
        let Some(text) = lock_backend(&self.state).and_then(|backend| {
            backend
                .windows
                .get(window)?
                .sessions
                .get(&session)?
                .last_prompt
                .clone()
        }) else {
            return Ok(false);
        };
        let (connection, session_id) = self.ensure_live(window, &session).await?;
        self.start_turn(window, connection, session_id, session, text)
            .await?;
        Ok(true)
    }

    /// Send one plain-text prompt, spawning the session lazily on its
    /// first message. Streams arrive as events; the turn end arrives as
    /// done or failed. The scoping first turn takes the user's text and
    /// prefixes the template server-side as one message. A reserved
    /// pending scoping session materializes its directory here, before
    /// the `ensure_live` path.
    pub async fn send_prompt(
        &self,
        window: &str,
        session: SessionKey,
        text: String,
    ) -> Result<(), AgentError> {
        log::info!(
            "send_prompt window={window} session={}::{:?} text_len {}",
            session.plan,
            session.role,
            text.len()
        );
        if session.role == SessionRole::Scoping {
            let repo_root = self
                .reopen_snapshot(window)
                .map(|(root, _)| root)
                .ok_or_else(|| AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                })?;
            // Working title from the user's text before the server prefix.
            // Set-once; headings win; empty stays `Untitled`.
            store_first_title(&self.state, window, &repo_root, &session.plan, &text);
            let pending_match = match lock_backend(&self.state) {
                Some(backend) => backend
                    .windows
                    .get(window)
                    .map(|win| win.pending_scoping.contains_key(&session.plan))
                    .unwrap_or(false),
                None => {
                    log::warn!("failed to check pending session: state poisoned");
                    false
                }
            };
            if pending_match {
                plans::materialize_scoping(&repo_root, &session.plan)?;
                carry_pending_title(&self.state, window, &repo_root, &session.plan);
                match lock_backend(&self.state) {
                    Some(mut backend) => {
                        if let Some(win) = backend.windows.get_mut(window) {
                            win.pending_scoping.remove(&session.plan);
                        }
                    }
                    None => {
                        log::warn!("failed to clear pending session: state poisoned");
                    }
                }
            }
        }
        // A prompt never races a replay: it waits for an in-flight load
        // rather than interleaving live chunks with history chunks.
        self.wait_for_history(window, &session).await?;
        // Queued landings hold with no live session: prompting one fails
        // fast instead of minting a second rebase. Cancel and finish paths
        // stay allowed.
        if let Some((repo_root, _)) = self.reopen_snapshot(window)
            && super::landing_queue::is_held(&repo_root, &session)
        {
            return Err(AgentError::RequestFailed {
                raw: "landing is queued, waiting its turn".to_string(),
            });
        }
        let (connection, session_id) = self.ensure_live(window, &session).await?;
        // A warmed pending session spawns before its directory exists, so
        // its ID goes unrecorded; persisting here heals it once the first
        // prompt materializes the directory, along with any missing file.
        if let Some((repo_root, _)) = self.reopen_snapshot(window) {
            super::session_ids::record_session(&repo_root, &session, &session_id);
        }
        // A fresh prompt clears the failed flag; the dot goes green while
        // the turn runs.
        if let Some(mut backend) = lock_backend(&self.state)
            && let Some(win) = backend.windows.get_mut(window)
            && let Some(live) = win.sessions.get_mut(&session)
        {
            live.failed = false;
        }
        self.mark_prompted(window, &session.plan);
        let mut text = text;
        if let Some(prefixed) = self.claim_role_prefix(window, &session, &text) {
            text = prefixed;
        }
        self.start_turn(window, connection, session_id, session, text)
            .await
    }

    /// Guard one turn and stream it as events. Records the prompt so retry
    /// resends exactly what ran, prefixed or not.
    pub(crate) async fn start_turn(
        &self,
        window: &str,
        connection: ConnectionTo<Agent>,
        session_id: String,
        key: SessionKey,
        text: String,
    ) -> Result<(), AgentError> {
        {
            let mut backend = lock_backend(&self.state).expect("state poisoned");
            let Some(win) = backend.windows.get_mut(window) else {
                return Err(AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                });
            };
            if win.is_working(&key) {
                log::warn!(
                    "start_turn already running window={window} session={}::{:?}",
                    key.plan,
                    key.role
                );
                return Err(AgentError::RequestFailed {
                    raw: "a turn is already running".to_string(),
                });
            }
            win.set_working(&key, true);
            if let Some(session) = win.sessions.get_mut(&key) {
                session.last_prompt = Some(text.clone());
            }
            backend.recompute_awake();
        }
        // Dots go green while the turn runs; titles and order refresh when
        // it lands.
        self.push_plans(window);
        let spawn_incarnation = incarnation(&self.state, window);
        let state = Arc::clone(&self.state);
        let window_owned = window.to_string();
        let app = self.app.clone();
        let pump_manager = self.clone();
        let pump_window = window.to_string();
        tauri::async_runtime::spawn(async move {
            let Some(spawn) = spawn_incarnation else {
                log::warn!("dropping turn emit window={window_owned}: window closed at spawn");
                return;
            };
            let prompt = acp::PromptRequest::new(
                acp::SessionId::new(session_id),
                vec![acp::ContentBlock::Text(acp::TextContent::new(text))],
            );
            match connection.send_request(prompt).block_task().await {
                Ok(response) => {
                    if !check_current(&state, &window_owned, &spawn) {
                        return;
                    }
                    if let Some(raw) = stop_failure_raw(&response.stop_reason) {
                        fail_turn(&state, &app, &window_owned, &key, raw.to_string());
                        return;
                    }
                    set_working(&state, &window_owned, &key, false);
                    set_failed(&state, &window_owned, &key, false);
                    log::info!(
                        "TurnDone window={window_owned} session={}::{:?} stop_reason={:?}",
                        key.plan,
                        key.role,
                        response.stop_reason,
                    );
                    emit_event(
                        &app,
                        &window_owned,
                        AppEvent::TurnDone {
                            session: key.clone(),
                        },
                    );
                    push_sorted(&state, &app, &window_owned);
                    // Automatic plans advance once the turn ends clean.
                    drop(connection);
                    pump_manager.spawn_pump(&pump_window);
                }
                Err(error) => {
                    if !check_current(&state, &window_owned, &spawn) {
                        return;
                    }
                    let raw = error.to_string();
                    if is_transport_error(&raw) {
                        match lock_backend(&state) {
                            Some(mut backend) => {
                                if let Some(win) = backend.windows.get_mut(&window_owned)
                                    && let Some(session) = win.sessions.get_mut(&key)
                                {
                                    session.connection = None;
                                    session.session_id = None;
                                }
                            }
                            None => {
                                log::warn!("failed to drop dead agent connection: state poisoned");
                            }
                        }
                    }
                    fail_turn(&state, &app, &window_owned, &key, raw);
                }
            }
        });
        Ok(())
    }

    /// Stop the turn via `session/cancel`, answering every open card as
    /// cancelled per protocol. Work plans hold afterwards: the cancel
    /// persists `manual:true` before clearing the working flag, so the
    /// pump re-reads the hold on its next pass. Scoping never
    /// auto-advances, so it stays untouched.
    pub async fn cancel_turn(&self, window: &str, session: SessionKey) -> Result<(), AgentError> {
        let (connection, session_id, _, _) = self
            .session_snapshot_for(window, &session)
            .ok_or_else(|| AgentError::NoSession {
                raw: "no active turn".to_string(),
            })?;
        cancel_pending(&self.state, window, &session);
        if let Err(error) = connection.send_notification(acp::build_cancel_notification(
            acp::SessionId::new(session_id),
        )) {
            log::warn!("failed to send session cancel: {error}");
        }
        if session.role.is_work()
            && let Some((repo_root, _)) = self.reopen_snapshot(window)
        {
            super::session_ids::hold_manual(&repo_root, &session.plan);
        }
        set_working(&self.state, window, &session, false);
        self.push_plans(window);
        Ok(())
    }

    /// Claim the one-time role prefix for the first message of one ACP
    /// conversation. One locked check-and-mark, so a retried turn never
    /// prefixes twice. Scoping uses the repo-relative display path since it
    /// runs with cwd at the repo root, while executing and landing use
    /// absolute paths into the checkout. Evergreening never prefixes here:
    /// the transition core sends its full first turn at spawn.
    fn claim_role_prefix(&self, window: &str, key: &SessionKey, user_text: &str) -> Option<String> {
        let mut backend = lock_backend(&self.state)?;
        let state = backend.windows.get_mut(window)?;
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
            plans::Phase::Evergreening | plans::Phase::Completed | plans::Phase::Cancelled => {
                return None;
            }
        };
        state.sessions.get_mut(key)?.plan.prefixed = true;
        Some(text)
    }
}

/// Raw failure message for an aborted ACP stop reason, or `None` when the
/// turn ended clean. `EndTurn` and user-cancelled turns are clean ends;
/// every other reason surfaces as `TurnFailed` so truncated turns never
/// idle silently. Pure.
fn stop_failure_raw(reason: &acp::StopReason) -> Option<&'static str> {
    match reason {
        acp::StopReason::EndTurn | acp::StopReason::Cancelled => None,
        acp::StopReason::MaxTokens => Some("turn stopped: max_tokens"),
        acp::StopReason::MaxTurnRequests => Some("turn stopped: max_turn_requests"),
        acp::StopReason::Refusal => Some("turn stopped: refusal"),
        _ => Some("turn stopped: aborted"),
    }
}

/// Settle a failed turn: clear working, mark failed, classify the raw
/// reason for the retry bar, emit it, and refresh the list.
fn fail_turn(
    state: &Mutex<BackendState>,
    app: &AppHandle,
    window: &str,
    key: &SessionKey,
    raw: String,
) {
    set_working(state, window, key, false);
    set_failed(state, window, key, true);
    let hint = classify_error(&raw);
    log::info!(
        "TurnFailed window={window} session={}::{:?} raw={raw}",
        key.plan,
        key.role
    );
    emit_event(
        app,
        window,
        AppEvent::TurnFailed {
            session: key.clone(),
            raw,
            hint: hint.text,
            retryable: hint.retryable,
        },
    );
    push_sorted(state, app, window);
}

pub(crate) fn handle_notification(
    state: &Mutex<BackendState>,
    app: &AppHandle,
    window: &str,
    key: &SessionKey,
    notification: &acp::SessionNotification,
    expected: &super::WindowIncarnation,
) {
    if !super::check_current(state, window, expected) {
        return;
    }
    let current = lock_backend(state).and_then(|backend| {
        backend
            .windows
            .get(window)?
            .sessions
            .get(key)?
            .session_id
            .clone()
    });
    if let Some(current) = current
        && notification.session_id.to_string() != current
    {
        log::debug!(
            "dropping notification window={window} session={}::{:?} for stale session {}",
            key.plan,
            key.role,
            notification.session_id
        );
        return;
    }
    if let Some(chunk) = crate::updates::agent_text_of(&notification.update) {
        emit_event(
            app,
            window,
            AppEvent::AgentText {
                session: key.clone(),
                chunk,
            },
        );
    }
    if let Some(chunk) = crate::updates::thought_text_of(&notification.update) {
        emit_event(
            app,
            window,
            AppEvent::AgentThought {
                session: key.clone(),
                chunk,
            },
        );
    }
    // User messages only surface here while history replays. Live turns
    // already show their bubble (scoping appends optimistically, eager
    // executing/landing first prompts arrive as their own event), so
    // mapping them outside a replay would double-add every prompt.
    if let Some(chunk) = crate::updates::user_text_of(&notification.update)
        && lock_backend(state)
            .map(|mut backend| {
                backend
                    .windows
                    .get_mut(window)
                    .map(|win| {
                        if !win.is_history_loading(key) {
                            return false;
                        }
                        win.mark_prefixed(key);
                        true
                    })
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    {
        for bubble in replay_bubbles(key, &chunk) {
            emit_event(
                app,
                window,
                AppEvent::UserText {
                    session: key.clone(),
                    chunk: bubble,
                },
            );
        }
    }
    match &notification.update {
        acp::SessionUpdate::ToolCall(call) => {
            let merged = lock_backend(state).and_then(|mut backend| {
                backend
                    .windows
                    .get_mut(window)?
                    .sessions
                    .get_mut(key)
                    .map(|session| {
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
            // Todos and the tool line both emit so `task`/`explore` steps
            // carrying a todos-shaped payload stay visible.
            let fresh = todos_from_call(merged.name.as_deref(), merged.raw_input.as_ref());
            let line = crate::updates::format_tool_line(&merged);
            emit_event(
                app,
                window,
                AppEvent::ToolLine {
                    session: key.clone(),
                    line,
                },
            );
            if let Some(fresh) = fresh {
                log::debug!("todo call {} todos {}", call.tool_call_id, fresh.len());
                update_todos(state, app, window, key, fresh, expected);
            }
        }
        acp::SessionUpdate::ToolCallUpdate(update) => {
            let tool_name_update = update.fields.name.clone();
            let merged = lock_backend(state).and_then(|mut backend| {
                backend
                    .windows
                    .get_mut(window)?
                    .sessions
                    .get_mut(key)
                    .map(|session| {
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
            let tool_name = tool_name_update.as_deref().or(merged.name.as_deref());
            let fresh = todos_from_update(
                tool_name,
                merged.raw_input.as_ref(),
                merged.raw_output.as_ref(),
            );
            let line = crate::updates::format_tool_line(&merged);
            emit_event(
                app,
                window,
                AppEvent::ToolLine {
                    session: key.clone(),
                    line,
                },
            );
            if let Some(fresh) = fresh {
                log::debug!("todo update {} todos {}", update.tool_call_id, fresh.len());
                update_todos(state, app, window, key, fresh, expected);
            }
        }
        acp::SessionUpdate::UsageUpdate(update) => {
            let cost = update.cost.as_ref().map(|cost| cost.amount).unwrap_or(0.0);
            let ctx_pct = context_pct(update.used, update.size);
            // Snapshot the repo root under the lock, then persist after it
            // is released so file IO never runs under the mutex.
            let repo_root = lock_backend(state).and_then(|mut backend| {
                let win = backend.windows.get_mut(window)?;
                let root = win.repo_root.clone();
                if let Some(live) = win.sessions.get_mut(key) {
                    live.cost = cost;
                    live.ctx_pct = ctx_pct;
                }
                root
            });
            emit_event(
                app,
                window,
                AppEvent::SpendTick {
                    session: key.clone(),
                    cost,
                    ctx_pct,
                },
            );
            push_sorted(state, app, window);
            if let Some(repo_root) = repo_root {
                super::session_ids::record_cost(&repo_root, key, cost);
            }
        }
        acp::SessionUpdate::ConfigOptionUpdate(update) => {
            let options = config_views(&update.config_options);
            emit_event(
                app,
                window,
                AppEvent::ConfigOptions {
                    session: key.clone(),
                    options,
                },
            );
        }
        acp::SessionUpdate::AgentMessageChunk(_) | acp::SessionUpdate::AgentThoughtChunk(_) => {}
        // Already streamed as agent text or thought above. Known-but-unrendered kinds
        // are routine; an unknown kind means protocol drift.
        update => match crate::updates::update_kind(update) {
            Some(kind) => log::debug!("unhandled session update: {kind}"),
            None => log::warn!("unknown session update: {update:?}"),
        },
    }
}

/// Shared title store: writes either the pending-map value (no dir) or
/// `state.json:working_title` (dir exists), inserting on `Some` and
/// clearing on `None`. `overwrite` gates existing titles: set-once skips,
/// draft-keep overwrites. Headings win at display time; storing skips when
/// a heading already exists. Caller owns any lock via `&mut WindowState`.
fn store_title(
    state: &mut WindowState,
    repo_root: &std::path::Path,
    plan_name: &str,
    title: Option<String>,
    overwrite: bool,
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
        if !overwrite && stored.working_title.is_some() {
            return;
        }
        if !overwrite && title.is_none() {
            return;
        }
        let mut stored = stored;
        stored.working_title = title;
        plans::store_state(&plan_dir, &stored);
        return;
    }
    let Some(slot) = state.pending_scoping.get_mut(plan_name) else {
        return;
    };
    if !overwrite && slot.is_some() {
        return;
    }
    if !overwrite && title.is_none() {
        return;
    }
    *slot = title;
}

/// Send-path title: set-once from the user's text. Never overwrites a
/// draft-kept title; empty stays `Untitled` via extractor `None`.
fn store_first_title(
    state: &Mutex<BackendState>,
    window: &str,
    repo_root: &std::path::Path,
    plan_name: &str,
    user_text: &str,
) {
    let trimmed = user_text.trim();
    if trimmed.is_empty() {
        return;
    }
    let title = crate::working_title::extract_working_title(trimmed);
    let Some(mut backend) = lock_backend(state) else {
        return;
    };
    let win = backend.ensure_window(window);
    store_title(win, repo_root, plan_name, title, false);
}

/// Keep-path title: overwrites from the switch-away draft on every keep.
/// Extractor `None` clears to `Untitled`.
pub(crate) fn store_draft_title(
    state: &mut WindowState,
    repo_root: &std::path::Path,
    plan_name: &str,
    user_text: &str,
) {
    let trimmed = user_text.trim();
    if trimmed.is_empty() {
        return;
    }
    let title = crate::working_title::extract_working_title(trimmed);
    store_title(state, repo_root, plan_name, title, true);
}

/// Move a pending working title into `state.json` at materialize time.
/// Set-once: never overwrites a disk title. Removes the pending-map entry.
fn carry_pending_title(
    state: &Mutex<BackendState>,
    window: &str,
    repo_root: &std::path::Path,
    plan_name: &str,
) {
    let removed = lock_backend(state).and_then(|mut backend| {
        backend
            .windows
            .get_mut(window)?
            .pending_scoping
            .remove(plan_name)
    });
    let Some(Some(title)) = removed else {
        return;
    };
    let plan_ref = plans::PlanRef {
        name: plan_name.to_string(),
        phase: plans::Phase::Scoping,
    };
    if !plan_ref.path(repo_root).is_dir() {
        if let Some(mut backend) = lock_backend(state)
            && let Some(win) = backend.windows.get_mut(window)
        {
            win.pending_scoping
                .insert(plan_name.to_string(), Some(title));
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
/// Executing and landing intentionally stay combined: no lead preview
/// exists for them, so no duplicate is possible. Their live-versus-reload
/// asymmetry is server-side prefixing, which stays out of scope.
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
/// Identical lists stay silent. Tracks when the current list started for
/// the plans-payload ETA and refreshes headers live.
fn update_todos(
    state: &Mutex<BackendState>,
    app: &AppHandle,
    window: &str,
    key: &SessionKey,
    fresh: Vec<TodoView>,
    expected: &super::WindowIncarnation,
) {
    if !super::check_current(state, window, expected) {
        return;
    }
    let Some(mut backend) = lock_backend(state) else {
        return;
    };
    let Some(win) = backend.windows.get_mut(window) else {
        return;
    };
    let Some(session) = win.sessions.get_mut(key) else {
        return;
    };
    if fresh == session.todos {
        return;
    }
    let changes = diff_todos(&session.todos, &fresh);
    let was_empty = session.todos.is_empty();
    let now_empty = fresh.is_empty();
    if was_empty && !now_empty {
        session.todos_started_at = Some(std::time::SystemTime::now());
    } else if !was_empty && now_empty {
        session.todos_started_at = None;
    }
    session.todos = fresh.clone();
    drop(backend);
    emit_event(
        app,
        window,
        AppEvent::TodosChanged {
            session: key.clone(),
            todos: fresh,
            changes,
        },
    );
    push_sorted(state, app, window);
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
    fn executing_replay_stays_single_bubble() {
        let template = opencode::executing_first_message(".samokod/plans/executing/ts");
        let full = prefixed_first_message(&template, "do things");
        let key = SessionKey {
            plan: "ts".to_string(),
            role: SessionRole::Executing,
        };
        assert_eq!(replay_bubbles(&key, &full), vec![full]);
    }

    #[test]
    fn landing_replay_stays_single_bubble() {
        let template = opencode::landing_first_message(
            "samokod-ts",
            "main",
            "/repo/.samokod/worktrees/ts",
            "/repo/.samokod/plans/landing/ts/plan.md",
        );
        let full = prefixed_first_message(&template, "do things");
        let key = SessionKey {
            plan: "ts".to_string(),
            role: SessionRole::Landing,
        };
        assert_eq!(replay_bubbles(&key, &full), vec![full]);
    }

    #[test]
    fn working_title_set_once_across_two_prompts() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        plans::materialize_scoping(root, name).expect("materialize");
        let state = Mutex::new(BackendState::default());
        let first = "Fix the login flow login errors on login retry.";
        store_first_title(&state, "main", root, name, first);
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
        store_first_title(&state, "main", root, name, second);
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
        let state = Mutex::new(BackendState::default());
        {
            let mut backend = state.lock().expect("state");
            backend
                .ensure_window("main")
                .pending_scoping
                .insert(name.to_string(), None);
        }
        let text = "Fix the login flow login errors on login retry.";
        store_first_title(&state, "main", root, name, text);
        assert!(
            state
                .lock()
                .expect("state")
                .windows
                .get("main")
                .expect("window")
                .pending_scoping
                .get(name)
                .is_some_and(|title| title.is_some())
        );
        plans::materialize_scoping(root, name).expect("materialize");
        carry_pending_title(&state, "main", root, name);
        assert!(
            !state
                .lock()
                .expect("state")
                .windows
                .get("main")
                .expect("window")
                .pending_scoping
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
        let state = Mutex::new(BackendState::default());
        store_first_title(&state, "main", root, name, "   \n  ");
        let stored = plans::load_state(
            &plans::PlanRef {
                name: name.to_string(),
                phase: plans::Phase::Scoping,
            }
            .path(root),
        );
        assert_eq!(stored.working_title, None);
    }

    #[test]
    fn draft_title_overwrites_and_send_preserves_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let state = Mutex::new(BackendState::default());
        {
            let mut backend = state.lock().expect("state");
            backend
                .ensure_window("main")
                .pending_scoping
                .insert(name.to_string(), None);
        }
        {
            let mut backend = state.lock().expect("state");
            let win = backend.ensure_window("main");
            store_draft_title(
                win,
                root,
                name,
                "Fix the login flow login errors on login retry.",
            );
        }
        let first = state
            .lock()
            .expect("state")
            .windows
            .get("main")
            .expect("window")
            .pending_scoping
            .get(name)
            .cloned()
            .expect("pending");
        assert!(first.is_some_and(|title| title.to_lowercase().contains("login")));
        {
            let mut backend = state.lock().expect("state");
            let win = backend.ensure_window("main");
            store_draft_title(
                win,
                root,
                name,
                "Fix the checkout flow checkout errors on checkout retry.",
            );
        }
        let second = state
            .lock()
            .expect("state")
            .windows
            .get("main")
            .expect("window")
            .pending_scoping
            .get(name)
            .cloned()
            .expect("pending");
        assert!(second.is_some_and(|title| title.to_lowercase().contains("checkout")));
        store_first_title(
            &state,
            "main",
            root,
            name,
            "Fix the login flow login errors on login retry.",
        );
        let kept = state
            .lock()
            .expect("state")
            .windows
            .get("main")
            .expect("window")
            .pending_scoping
            .get(name)
            .cloned()
            .expect("pending");
        assert!(kept.is_some_and(|title| title.to_lowercase().contains("checkout")));
    }

    #[test]
    fn clean_stops_end_turn() {
        assert_eq!(stop_failure_raw(&acp::StopReason::EndTurn), None);
        assert_eq!(stop_failure_raw(&acp::StopReason::Cancelled), None);
    }

    #[test]
    fn aborted_stops_fail_turn() {
        for (reason, expected) in [
            (acp::StopReason::MaxTokens, "max_tokens"),
            (acp::StopReason::MaxTurnRequests, "max_turn_requests"),
            (acp::StopReason::Refusal, "refusal"),
        ] {
            let raw = stop_failure_raw(&reason).expect("aborted stop fails the turn");
            assert!(raw.contains(expected), "{raw} mentions {expected}");
            assert!(raw.contains("turn stopped"));
        }
    }

    #[test]
    fn stop_holds_work_and_holds_pump() {
        use super::super::advance::{PlanView, next_action};
        use super::super::landing_queue::LandingPosition;
        use crate::plans::Phase;

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let executing = plans::PlanRef {
            name: name.to_string(),
            phase: Phase::Executing,
        };
        std::fs::create_dir_all(executing.path(root)).expect("mkdir");
        let mut stored = plans::load_state(&executing.path(root));
        assert!(stored.set_manual(true));
        plans::store_state(&executing.path(root), &stored);
        let kept = plans::load_state(&executing.path(root));
        assert!(kept.manual);
        let held = PlanView {
            name: name.to_string(),
            phase: Phase::Executing,
            manual: kept.manual,
            evergreen: true,
            idle: true,
            dirty: false,
            ffable: true,
            has_commits: true,
            landing: LandingPosition::NotLanding,
            started: true,
        };
        assert_eq!(next_action(&held), None);
        let path = executing.path(root).join(plans::STATE_FILE);
        let before = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut again = plans::load_state(&executing.path(root));
        assert!(!again.set_manual(true));
        // `hold_manual` skips the rewrite when nothing changed, so the
        // mtime holds without a second store here.
        let after = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        assert_eq!(before, after);
    }
}
