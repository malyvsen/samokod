// Agent lifecycle: one `opencode acp` child process per live session,
// sessions keyed by plan directory name plus role, plans listed from disk.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tauri::{AppHandle, Emitter};

use crate::awake;
use crate::opencode;
use crate::plans;
use crate::types::{AgentError, AppEvent, OpenRepoResult, PlansUpdate, SessionKey, SessionRole};

mod config;
mod permissions;
mod plans_list;
mod session;
mod turns;
mod warm;

pub(crate) use session::{ActivePlan, LiveSession};

#[derive(Default)]
pub(crate) struct State {
    sessions: HashMap<SessionKey, LiveSession>,
    current: Option<SessionKey>,
    repo_root: Option<PathBuf>,
    branch: String,
    branch_watch: Option<notify::RecommendedWatcher>,
    awake: Option<awake::Guard>,
    /// Last user-sent prompt per plan directory name. Drives plan sorting;
    /// migrated across renames so an approved plan keeps its recency.
    activity: HashMap<String, Instant>,
    /// Reserved scoping timestamp with no directory yet. Listed as a
    /// normal `Untitled` entry until the first `send_prompt` materializes
    /// it; vanishing on abandon or select-away leaves no trace.
    pending_scoping: Option<String>,
    /// Background warms in flight, one per session key. Single-flights
    /// `warm_session` against a racing `send_prompt`.
    warming: HashSet<SessionKey>,
}

impl State {
    /// Working and the sleep lock move together. The guard is held while
    /// any session works.
    fn set_working(&mut self, key: &SessionKey, working: bool) {
        if let Some(session) = self.sessions.get_mut(key) {
            session.working = working;
        }
        if self.sessions.values().any(|session| session.working) {
            if self.awake.is_none() {
                self.awake = awake::acquire();
            }
        } else {
            self.awake = None;
        }
    }
}

/// Empty scoping session: no `plan.md` and no sent message, whether or
/// not the directory exists. Takes `&State` (called under an existing
/// lock or a snapshot; never locks itself).
pub(crate) fn is_empty_scoping(state: &State, repo_root: &Path, name: &str) -> bool {
    let plan = plans::PlanRef {
        name: name.to_string(),
        phase: plans::Phase::Scoping,
    };
    if plan.has_plan_md(repo_root) {
        return false;
    }
    if state.activity.contains_key(name) {
        return false;
    }
    let key = SessionKey {
        plan: name.to_string(),
        role: SessionRole::Scoping,
    };
    if state
        .sessions
        .get(&key)
        .and_then(|live| live.last_prompt.clone())
        .is_some()
    {
        return false;
    }
    true
}

/// Discard an empty scoping session without a `cancelled/` trace:
/// `remove_dir_all` when present (`NotFound` is fine, other errors are
/// logged per the preserve-evidence rule), plus the `LiveSession` and
/// `activity` entry. Clears `pending_scoping` on match. Caller owns the
/// lock.
pub(crate) fn vanish_scoping(state: &mut State, repo_root: &Path, name: &str) {
    let plan = plans::PlanRef {
        name: name.to_string(),
        phase: plans::Phase::Scoping,
    };
    match std::fs::remove_dir_all(plan.path(repo_root)) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!(
            "failed to vanish empty scoping {}: {error}",
            plan.path(repo_root).display()
        ),
    }
    state.sessions.remove(&SessionKey {
        plan: name.to_string(),
        role: SessionRole::Scoping,
    });
    state.activity.remove(name);
    if state.pending_scoping.as_deref() == Some(name) {
        state.pending_scoping = None;
    }
}

/// Prefilled scoping draft for one session. Returns the rendered
/// template only while the session is fresh under `is_empty_scoping`,
/// `None` for non-fresh or non-scoping sessions, and an error when the
/// plan is gone. Reads the locked state plus disk.
pub(crate) fn scoping_draft_for(
    state: &State,
    repo_root: &Path,
    session: &SessionKey,
) -> Result<Option<String>, AgentError> {
    if session.role != SessionRole::Scoping {
        return Ok(None);
    }
    let is_pending = state.pending_scoping.as_deref() == Some(session.plan.as_str());
    let plan_ref = plans::PlanRef {
        name: session.plan.clone(),
        phase: plans::Phase::Scoping,
    };
    if !is_pending && !plan_ref.path(repo_root).is_dir() {
        return Err(AgentError::NoSession {
            raw: "plan is gone".to_string(),
        });
    }
    if !is_empty_scoping(state, repo_root, &session.plan) {
        return Ok(None);
    }
    let display = opencode::plan_display(&plan_ref);
    Ok(Some(opencode::scoping_draft(&display)))
}

/// Reserved timestamp name for a lazy scoping session: collision-proof
/// against on-disk scoping names plus the current pending name. Pure
/// except the directory read.
pub(crate) fn reserve_scoping_name(repo_root: &Path, pending: Option<&str>) -> String {
    let mut taken: std::collections::HashSet<String> = plans::scan_plans(repo_root)
        .into_iter()
        .filter(|plan| plan.phase == plans::Phase::Scoping)
        .map(|plan| plan.name)
        .collect();
    if let Some(pending) = pending {
        taken.insert(pending.to_string());
    }
    plans::unique_name(&plans::timestamp_now(), &taken)
}

pub struct AgentManager {
    state: Arc<Mutex<State>>,
    app: AppHandle,
}

impl AgentManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            app,
        }
    }

    /// Open a repository: ensure the plan structure, rescan every plan,
    /// and select the most-recent session. Spawns no agents: each session
    /// starts lazily on its first prompt, with transcripts and TODOs kept
    /// run-local. With zero scoping dirs, reserves a pending session
    /// without touching disk.
    pub async fn open_repo(&self, repo_root: PathBuf) -> Result<OpenRepoResult, AgentError> {
        plans::ensure_structure(&repo_root)?;
        self.clear_sessions();
        {
            let mut state = self.state.lock().expect("state poisoned");
            state.repo_root = Some(repo_root.clone());
            state.branch =
                crate::branch::current_branch(&repo_root).unwrap_or_else(|| "HEAD".to_string());
            if plans::scan_plans(&repo_root)
                .iter()
                .all(|plan| plan.phase != plans::Phase::Scoping)
            {
                let name = reserve_scoping_name(&repo_root, state.pending_scoping.as_deref());
                state.pending_scoping = Some(name.clone());
                state.current = Some(SessionKey {
                    plan: name,
                    role: SessionRole::Scoping,
                });
            }
        }
        self.watch_branch(&repo_root);
        Ok(self.open_result())
    }

    /// Reserve a fresh scoping session without touching disk. Reuses the
    /// pending session when the selection is still on an empty one.
    pub async fn create_plan(&self) -> Result<PlansUpdate, AgentError> {
        let repo_root = self.current_repo().ok_or_else(|| AgentError::NoSession {
            raw: "open a repository first".to_string(),
        })?;
        {
            let state = self.state.lock().expect("state poisoned");
            if let (Some(current), Some(pending)) =
                (state.current.clone(), state.pending_scoping.clone())
                && current.role == SessionRole::Scoping
                && current.plan == pending
                && is_empty_scoping(&state, &repo_root, &pending)
            {
                drop(state);
                let update = self.plans_update();
                let manager = AgentManager {
                    state: Arc::clone(&self.state),
                    app: self.app.clone(),
                };
                let key = update.selected.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = manager.warm_session(key).await;
                });
                return Ok(update);
            }
        }
        let name = {
            let state = self.state.lock().expect("state poisoned");
            reserve_scoping_name(&repo_root, state.pending_scoping.as_deref())
        };
        match self.state.lock() {
            Ok(mut state) => {
                state.pending_scoping = Some(name.clone());
                state.current = Some(SessionKey {
                    plan: name,
                    role: SessionRole::Scoping,
                });
            }
            Err(error) => {
                log::warn!("failed to select new plan: {error}");
            }
        }
        let update = self.plans_update();
        let manager = AgentManager {
            state: Arc::clone(&self.state),
            app: self.app.clone(),
        };
        let key = update.selected.clone();
        tauri::async_runtime::spawn(async move {
            let _ = manager.warm_session(key).await;
        });
        Ok(update)
    }

    /// Re-check the branch for the open repo. Failures keep the last value.
    pub async fn refresh_branch(&self) -> Result<String, AgentError> {
        let repo_root = lock_state(&self.state)
            .and_then(|state| state.repo_root.clone())
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let branch =
            crate::branch::current_branch(&repo_root).unwrap_or_else(|| "HEAD".to_string());
        self.set_branch(branch.clone());
        Ok(branch)
    }

    /// Set the held branch and emit it. Returns true when it moved.
    fn set_branch(&self, branch: String) -> bool {
        let moved = match self.state.lock() {
            Ok(mut state) => {
                if state.branch == branch {
                    false
                } else {
                    state.branch = branch.clone();
                    true
                }
            }
            Err(error) => {
                log::warn!("failed to record branch change: {error}");
                false
            }
        };
        if moved {
            emit_event(&self.app, AppEvent::BranchChanged { branch });
        }
        moved
    }

    /// Watch `.git/HEAD` for the open repo, refreshing `state.branch` per
    /// event so execute/retry snapshots go fresh free.
    fn watch_branch(&self, repo_root: &Path) {
        let root = repo_root.to_path_buf();
        let state = Arc::clone(&self.state);
        let app = self.app.clone();
        let watcher = crate::branch::watch_branch(root.clone(), move |branch| {
            let moved = match state.lock() {
                Ok(mut guard) => {
                    if guard.repo_root.as_ref() != Some(&root) || guard.branch == branch {
                        false
                    } else {
                        guard.branch = branch.clone();
                        true
                    }
                }
                Err(error) => {
                    log::warn!("failed to record branch change: {error}");
                    false
                }
            };
            if moved {
                emit_event(&app, AppEvent::BranchChanged { branch });
            }
        });
        match self.state.lock() {
            Ok(mut state) => {
                state.branch_watch = watcher;
            }
            Err(error) => {
                log::warn!("failed to hold branch watcher: {error}");
            }
        }
    }

    /// Approve the scoping plan: move it to executing, switch to a fresh
    /// executor process, and start it immediately. The prompt stays hidden:
    /// no user bubble, the chat opens working. The selection follows the
    /// new execution session.
    pub async fn execute_plan(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        ensure_idle(&self.state, &session)?;
        let (repo_root, branch) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let from = plans::PlanRef {
            name: session.plan.clone(),
            phase: plans::Phase::Scoping,
        };
        if session.role != SessionRole::Scoping || !from.path(&repo_root).is_dir() {
            return Err(AgentError::RequestFailed {
                raw: "no scoping plan to execute".to_string(),
            });
        }
        let next = plans::execute(&repo_root, &from)?;
        plans::mark_executed(&repo_root, &next);
        self.drop_live(&session).await;
        self.move_activity(&from.name, &next.name);
        let mut plan = ActivePlan::executing(next.name.clone());
        // The role goes out hidden below, so later turns never prefix again.
        plan.prefixed = true;
        let agent = opencode::agent_for(plan.phase);
        let (connection, session_id, key) =
            self.spawn_session(&repo_root, &branch, plan, agent).await?;
        let text = opencode::executor_first_message(&opencode::plan_display(&next));
        self.touch_activity(&next.name);
        self.start_turn(connection, session_id, key, text).await?;
        Ok(self.plans_update())
    }

    /// Finish the executing plan. The selection stays on the completed
    /// (now read-only) execution session; fresh plans come from `+ NEW PLAN`.
    pub async fn mark_completed(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        ensure_idle(&self.state, &session)?;
        let (repo_root, _) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let from = plans::PlanRef {
            name: session.plan.clone(),
            phase: plans::Phase::Executing,
        };
        if session.role != SessionRole::Executing || !from.path(&repo_root).is_dir() {
            return Err(AgentError::RequestFailed {
                raw: "no executing plan to complete".to_string(),
            });
        }
        let next = plans::complete(&repo_root, &from)?;
        self.drop_live(&session).await;
        self.move_activity(&from.name, &next.name);
        self.select_key(SessionKey {
            plan: next.name,
            role: SessionRole::Executing,
        });
        Ok(self.plans_update())
    }

    /// Drop a scoping plan. An empty session vanishes without a
    /// `cancelled/` trace, falling back to `most_recent_key` (reserving a
    /// fresh pending session when nothing remains); otherwise the plan
    /// moves to `cancelled/` as today.
    pub async fn abandon_plan(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        ensure_idle(&self.state, &session)?;
        let (repo_root, _) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        if session.role == SessionRole::Scoping {
            let empty = match self.state.lock() {
                Ok(state) => is_empty_scoping(&state, &repo_root, &session.plan),
                Err(error) => {
                    log::warn!("failed to check empty session: {error}");
                    false
                }
            };
            if empty {
                match self.state.lock() {
                    Ok(mut state) => {
                        vanish_scoping(&mut state, &repo_root, &session.plan);
                        let plans = plans_list::sorted_entries(
                            &repo_root,
                            &state.sessions,
                            &state.activity,
                            state.pending_scoping.as_deref(),
                        );
                        if let Some(key) = plans_list::most_recent_key(&plans) {
                            state.current = Some(key);
                        } else {
                            let name =
                                reserve_scoping_name(&repo_root, state.pending_scoping.as_deref());
                            state.pending_scoping = Some(name.clone());
                            state.current = Some(SessionKey {
                                plan: name,
                                role: SessionRole::Scoping,
                            });
                        }
                    }
                    Err(error) => {
                        log::warn!("failed to vanish empty session: {error}");
                    }
                }
                return Ok(self.plans_update());
            }
        }
        let from = plans::PlanRef {
            name: session.plan.clone(),
            phase: plans::Phase::Scoping,
        };
        if session.role != SessionRole::Scoping || !from.path(&repo_root).is_dir() {
            return Err(AgentError::RequestFailed {
                raw: "no active plan to abandon".to_string(),
            });
        }
        let next = plans::abandon(&repo_root, &from)?;
        self.drop_live(&session).await;
        self.move_activity(&from.name, &next.name);
        self.select_key(SessionKey {
            plan: next.name,
            role: SessionRole::Scoping,
        });
        Ok(self.plans_update())
    }

    /// Select one session, discarding a previously-selected empty scoping
    /// session the same way as abandon. Returns the target selection.
    /// No `ensure_idle` gate: selection is allowed anytime, and an empty
    /// previous session is never working.
    pub async fn select_plan(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        let (repo_root, _) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        {
            let state = self.state.lock().expect("state poisoned");
            let target_exists = state.pending_scoping.as_deref() == Some(session.plan.as_str())
                && session.role == SessionRole::Scoping
                || plans::PlanRef {
                    name: session.plan.clone(),
                    phase: session::role_phase(session.role),
                }
                .path(&repo_root)
                .is_dir();
            if !target_exists {
                return Err(AgentError::NoSession {
                    raw: "plan is gone".to_string(),
                });
            }
        }
        match self.state.lock() {
            Ok(mut state) => {
                if let Some(prev) = state.current.clone()
                    && prev != session
                    && prev.role == SessionRole::Scoping
                    && is_empty_scoping(&state, &repo_root, &prev.plan)
                {
                    vanish_scoping(&mut state, &repo_root, &prev.plan);
                }
                state.current = Some(session);
            }
            Err(error) => {
                log::warn!("failed to select session: {error}");
            }
        }
        Ok(self.plans_update())
    }

    /// Prefilled scoping draft: the planner template with its plan dir
    /// filled in, returned only while the session is fresh under the
    /// `is_empty_scoping` gate (no `plan.md`, no activity, no sent prompt).
    /// Non-fresh and non-scoping sessions get `None`; missing repos and
    /// gone plans are errors, never silent fallbacks.
    pub fn scoping_draft(&self, session: SessionKey) -> Result<Option<String>, AgentError> {
        let Some(state_guard) = lock_state(&self.state) else {
            return Err(AgentError::RequestFailed {
                raw: "agent state unavailable".to_string(),
            });
        };
        let Some(repo_root) = state_guard.repo_root.clone() else {
            return Err(AgentError::NoSession {
                raw: "open a repository first".to_string(),
            });
        };
        scoping_draft_for(&state_guard, &repo_root, &session)
    }

    /// Cancel an executing plan. The selection stays on the cancelled (now
    /// read-only) execution session.
    pub async fn cancel_execution(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        ensure_idle(&self.state, &session)?;
        let (repo_root, _) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let from = plans::PlanRef {
            name: session.plan.clone(),
            phase: plans::Phase::Executing,
        };
        if session.role != SessionRole::Executing || !from.path(&repo_root).is_dir() {
            return Err(AgentError::RequestFailed {
                raw: "no executing plan to cancel".to_string(),
            });
        }
        let next = plans::abandon(&repo_root, &from)?;
        self.drop_live(&session).await;
        self.move_activity(&from.name, &next.name);
        self.select_key(SessionKey {
            plan: next.name,
            role: SessionRole::Executing,
        });
        Ok(self.plans_update())
    }
}

/// Refuse plan transitions while the session's turn runs. A poisoned lock
/// only logs, matching `lock_state`.
fn ensure_idle(state: &Mutex<State>, key: &SessionKey) -> Result<(), AgentError> {
    match state.lock() {
        Ok(state) => {
            let busy = state
                .sessions
                .get(key)
                .map(|session| session.working)
                .unwrap_or(false);
            if busy {
                return Err(AgentError::RequestFailed {
                    raw: "a turn is already running".to_string(),
                });
            }
            Ok(())
        }
        Err(error) => {
            log::warn!("failed to check working state: {error}");
            Ok(())
        }
    }
}

/// Emit one app event. Emissions are fire-and-forget, but a failure desyncs
/// the UI from the agent, so it is always logged.
fn emit_event(app: &AppHandle, event: AppEvent) {
    if let Err(error) = app.emit("samokod://event", event) {
        log::warn!("failed to emit app event: {error}");
    }
}

/// Lock agent state. A poisoned mutex means a prior panic elsewhere; log it
/// instead of silently dropping the update.
fn lock_state(state: &Mutex<State>) -> Option<std::sync::MutexGuard<'_, State>> {
    match state.lock() {
        Ok(guard) => Some(guard),
        Err(error) => {
            log::warn!("agent state lock poisoned: {error}");
            None
        }
    }
}

fn set_working(state: &Mutex<State>, key: &SessionKey, working: bool) {
    let Some(mut guard) = lock_state(state) else {
        return;
    };
    guard.set_working(key, working);
}

fn set_failed(state: &Mutex<State>, key: &SessionKey, failed: bool) {
    let Some(mut guard) = lock_state(state) else {
        return;
    };
    if let Some(session) = guard.sessions.get_mut(key) {
        session.failed = failed;
    }
}

fn set_approval(state: &Mutex<State>, key: &SessionKey, approval: bool) {
    let Some(mut guard) = lock_state(state) else {
        return;
    };
    if let Some(session) = guard.sessions.get_mut(key) {
        session.approval = approval;
    }
}

// Spawn failures arrive as strings through the ready channel, so missing
// binaries are classified by matching the process output text.
fn map_startup_error(raw: &str) -> AgentError {
    let lowered = raw.to_lowercase();
    if lowered.contains("no such file")
        || (lowered.contains("not found") && lowered.contains("opencode"))
    {
        return AgentError::MissingBinary;
    }
    AgentError::AgentExited {
        raw: raw.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::config::config_views;
    use super::plans_list::{most_recent_key, sorted_entries};
    use super::*;
    use crate::acp::{
        self, AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo, SessionConfigOption,
        SessionConfigOptionCategory, SessionConfigSelectOption, SessionConfigValueId,
    };
    use std::collections::HashMap;
    use std::path::Path;
    use std::time::Instant;

    #[test]
    fn config_views_keep_agent_ordering() {
        let option = SessionConfigOption::select(
            "model",
            "Model",
            SessionConfigValueId::new("opencode/big-pickle"),
            vec![SessionConfigSelectOption::new(
                SessionConfigValueId::new("opencode/big-pickle"),
                "Big Pickle",
            )],
        )
        .category(SessionConfigOptionCategory::Model);
        let views = config_views(std::slice::from_ref(&option));
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, "model");
        assert_eq!(views[0].category.as_deref(), Some("model"));
        assert_eq!(views[0].current_value, "opencode/big-pickle");
        assert_eq!(views[0].options.len(), 1);
    }

    fn select_fixture(
        id: &'static str,
        name: &'static str,
        category: impl Into<Option<SessionConfigOptionCategory>>,
    ) -> SessionConfigOption {
        let category: Option<SessionConfigOptionCategory> = category.into();
        SessionConfigOption::select(
            id,
            name,
            SessionConfigValueId::new("v"),
            vec![SessionConfigSelectOption::new(
                SessionConfigValueId::new("v"),
                "V",
            )],
        )
        .category(category)
    }

    #[test]
    fn config_views_maps_categories() {
        let options = vec![
            select_fixture("llm", "LLM", SessionConfigOptionCategory::Model),
            select_fixture("mode", "Session Mode", SessionConfigOptionCategory::Mode),
            select_fixture(
                "effort",
                "Effort",
                SessionConfigOptionCategory::ThoughtLevel,
            ),
            select_fixture("ctx", "Context", SessionConfigOptionCategory::ModelConfig),
            select_fixture(
                "custom",
                "Custom",
                SessionConfigOptionCategory::Other("_custom".to_string()),
            ),
            select_fixture(
                "legacy",
                "Legacy",
                Option::<SessionConfigOptionCategory>::None,
            ),
        ];
        let views = config_views(&options);
        let categories: Vec<Option<&str>> =
            views.iter().map(|view| view.category.as_deref()).collect();
        assert_eq!(
            categories,
            [
                Some("model"),
                Some("mode"),
                Some("thought_level"),
                Some("model_config"),
                Some("_custom"),
                None,
            ]
        );
    }

    fn test_key() -> SessionKey {
        SessionKey {
            plan: "2026-09-25.10-54-59".to_string(),
            role: SessionRole::Scoping,
        }
    }

    fn state_with_session() -> Mutex<State> {
        let mut state = State::default();
        let key = test_key();
        state.sessions.insert(
            key.clone(),
            LiveSession::fresh(
                ActivePlan::scoping("2026-09-25.10-54-59".to_string()),
                crate::repo_state::RepoState::default(),
            ),
        );
        state.current = Some(key);
        Mutex::new(state)
    }

    #[test]
    fn working_releases_awake_guard() {
        let state = state_with_session();
        let key = test_key();
        set_working(&state, &key, true);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.sessions.get(&key).expect("session").working);
        }
        set_working(&state, &key, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(!guard.sessions.get(&key).expect("session").working);
            assert!(guard.awake.is_none());
        }
    }

    #[test]
    fn working_holds_guard_while_any_session_runs() {
        let state = Mutex::new(State::default());
        let first = SessionKey {
            plan: "a".to_string(),
            role: SessionRole::Scoping,
        };
        let second = SessionKey {
            plan: "b".to_string(),
            role: SessionRole::Scoping,
        };
        {
            let mut guard = state.lock().expect("state poisoned");
            guard.sessions.insert(
                first.clone(),
                LiveSession::fresh(
                    ActivePlan::scoping("a".to_string()),
                    crate::repo_state::RepoState::default(),
                ),
            );
            guard.sessions.insert(
                second.clone(),
                LiveSession::fresh(
                    ActivePlan::scoping("b".to_string()),
                    crate::repo_state::RepoState::default(),
                ),
            );
        }
        set_working(&state, &first, true);
        set_working(&state, &second, true);
        set_working(&state, &first, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.awake.is_some());
        }
        set_working(&state, &second, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.awake.is_none());
        }
    }

    #[test]
    fn failed_flag_tracks_turn_outcome() {
        let state = state_with_session();
        let key = test_key();
        set_failed(&state, &key, true);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.sessions.get(&key).expect("session").failed);
        }
        set_failed(&state, &key, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(!guard.sessions.get(&key).expect("session").failed);
        }
    }

    #[test]
    fn idle_gate_is_per_session() {
        let state = Mutex::new(State::default());
        let first = SessionKey {
            plan: "a".to_string(),
            role: SessionRole::Scoping,
        };
        let second = SessionKey {
            plan: "b".to_string(),
            role: SessionRole::Scoping,
        };
        {
            let mut guard = state.lock().expect("state poisoned");
            for (key, name) in [(&first, "a"), (&second, "b")] {
                guard.sessions.insert(
                    key.clone(),
                    LiveSession::fresh(
                        ActivePlan::scoping(name.to_string()),
                        crate::repo_state::RepoState::default(),
                    ),
                );
            }
        }
        set_working(&state, &first, true);
        assert!(ensure_idle(&state, &first).is_err());
        assert!(ensure_idle(&state, &second).is_ok());
    }

    fn write_plan_dir(root: &Path, phase: plans::Phase, name: &str, title: Option<&str>) {
        let dir = root
            .join(".samokod/plans")
            .join(phase.dir_name())
            .join(name);
        std::fs::create_dir_all(&dir).expect("mkdir");
        if let Some(title) = title {
            std::fs::write(dir.join("plan.md"), format!("# {title}\n")).expect("write");
        }
    }

    fn empty_activity() -> HashMap<String, Instant> {
        HashMap::new()
    }

    #[test]
    fn entries_sort_scoping_first_then_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Cancelled, "c", Some("C"));
        write_plan_dir(root, plans::Phase::Completed, "b", Some("B"));
        write_plan_dir(root, plans::Phase::Executing, "a", Some("A"));
        write_plan_dir(root, plans::Phase::Scoping, "s", Some("S"));
        let entries = sorted_entries(root, &HashMap::new(), &empty_activity(), None);
        let phases: Vec<plans::Phase> = entries.iter().map(|entry| entry.phase).collect();
        assert_eq!(
            phases,
            vec![
                plans::Phase::Scoping,
                plans::Phase::Executing,
                plans::Phase::Completed,
                plans::Phase::Cancelled,
            ]
        );
        assert_eq!(entries[0].title, "S");
    }

    #[test]
    fn activity_beats_newer_mtime() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Scoping, "old", Some("Old"));
        write_plan_dir(root, plans::Phase::Scoping, "new", Some("New"));
        let mut activity = empty_activity();
        activity.insert("old".to_string(), Instant::now());
        let entries = sorted_entries(root, &HashMap::new(), &activity, None);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "old");
        assert_eq!(entries[1].name, "new");
    }

    #[test]
    fn idle_plans_fall_back_to_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Scoping, "first", Some("First"));
        write_plan_dir(root, plans::Phase::Scoping, "second", Some("Second"));
        let entries = sorted_entries(root, &HashMap::new(), &empty_activity(), None);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "second");
        assert_eq!(entries[1].name, "first");
    }

    #[test]
    fn untitled_plans_list_without_heading() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Scoping, "bare", None);
        let entries = sorted_entries(root, &HashMap::new(), &empty_activity(), None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Untitled");
        assert_eq!(entries[0].sessions.len(), 1);
    }

    #[test]
    fn most_recent_prefers_execution_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Executing, "a", Some("A"));
        let entries = sorted_entries(root, &HashMap::new(), &empty_activity(), None);
        assert_eq!(
            most_recent_key(&entries),
            Some(SessionKey {
                plan: "a".to_string(),
                role: SessionRole::Executing,
            })
        );
        assert!(most_recent_key(&[]).is_none());
    }

    #[test]
    fn statuses_cover_sessions_and_live_flags() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Executing, "a", Some("A"));
        let key = SessionKey {
            plan: "a".to_string(),
            role: SessionRole::Executing,
        };
        let mut live = LiveSession::fresh(
            ActivePlan::executing("a".to_string()),
            crate::repo_state::RepoState::default(),
        );
        live.working = true;
        live.approval = true;
        let sessions = HashMap::from([(key, live)]);
        let entries = sorted_entries(root, &sessions, &empty_activity(), None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sessions.len(), 2);
        let executing = entries[0]
            .sessions
            .iter()
            .find(|status| status.role == SessionRole::Executing)
            .expect("executing status");
        assert!(executing.working);
        assert!(executing.approval);
        assert!(!executing.failed);
        assert!(!executing.live);
        let scoping = entries[0]
            .sessions
            .iter()
            .find(|status| status.role == SessionRole::Scoping)
            .expect("scoping status");
        assert!(!scoping.working);
    }

    #[test]
    fn cancelled_scoping_lists_one_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Cancelled, "c", Some("C"));
        let entries = sorted_entries(root, &HashMap::new(), &empty_activity(), None);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sessions.len(), 1);
        assert_eq!(entries[0].sessions[0].role, SessionRole::Scoping);
    }

    #[test]
    fn pending_lists_as_untitled_without_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        assert!(
            !plans::PlanRef {
                name: name.to_string(),
                phase: plans::Phase::Scoping,
            }
            .path(root)
            .exists()
        );
        let entries = sorted_entries(root, &HashMap::new(), &empty_activity(), Some(name));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, name);
        assert_eq!(entries[0].title, "Untitled");
        assert!(!entries[0].has_plan_md);
        assert_eq!(entries[0].sessions.len(), 1);
        assert_eq!(entries[0].sessions[0].role, SessionRole::Scoping);
    }

    #[test]
    fn reserve_creates_no_dir_and_avoids_taken_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        plans::materialize_scoping(root, "2026-09-26.08-41-03").expect("materialize");
        let reserved = reserve_scoping_name(root, Some("2026-09-26.08-41-04"));
        assert_ne!(reserved, "2026-09-26.08-41-03");
        assert_ne!(reserved, "2026-09-26.08-41-04");
        assert!(
            !plans::PlanRef {
                name: reserved,
                phase: plans::Phase::Scoping,
            }
            .path(root)
            .exists()
        );
    }

    #[test]
    fn empty_scoping_needs_no_plan_md_no_activity_no_prompt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let mut state = State::default();
        assert!(is_empty_scoping(&state, root, name));
        plans::materialize_scoping(root, name).expect("materialize");
        assert!(is_empty_scoping(&state, root, name));
        state.activity.insert(name.to_string(), Instant::now());
        assert!(!is_empty_scoping(&state, root, name));
        state.activity.remove(name);
        let key = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        state.sessions.insert(
            key.clone(),
            LiveSession::fresh(
                ActivePlan::scoping(name.to_string()),
                crate::repo_state::RepoState::default(),
            ),
        );
        assert!(is_empty_scoping(&state, root, name));
        state.sessions.get_mut(&key).expect("live").last_prompt = Some("hi".to_string());
        assert!(!is_empty_scoping(&state, root, name));
        let state = State::default();
        plans::materialize_scoping(root, "2026-09-26.08-41-04").expect("materialize");
        std::fs::write(
            root.join(".samokod/plans/scoping/2026-09-26.08-41-04/plan.md"),
            "# T\n",
        )
        .expect("write");
        assert!(!is_empty_scoping(&state, root, "2026-09-26.08-41-04"));
    }

    #[test]
    fn scoping_starts_prefixed() {
        assert!(ActivePlan::scoping("n".to_string()).prefixed);
    }

    #[test]
    fn draft_returns_template_only_while_fresh() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let key = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        let state = State {
            pending_scoping: Some(name.to_string()),
            ..Default::default()
        };
        let draft = scoping_draft_for(&state, root, &key)
            .expect("draft")
            .expect("fresh prefills");
        assert!(draft.contains(".samokod/plans/scoping/2026-09-26.08-41-03"));
        assert!(!draft.contains("{{PLAN_DIR}}"));

        let state = State::default();
        plans::materialize_scoping(root, name).expect("materialize");
        std::fs::write(
            root.join(".samokod/plans/scoping/2026-09-26.08-41-03/plan.md"),
            "# T\n",
        )
        .expect("write");
        assert_eq!(scoping_draft_for(&state, root, &key).expect("gate"), None);

        let mut state = State::default();
        plans::materialize_scoping(root, "2026-09-26.08-41-04").expect("materialize");
        let other = SessionKey {
            plan: "2026-09-26.08-41-04".to_string(),
            role: SessionRole::Scoping,
        };
        state.activity.insert(other.plan.clone(), Instant::now());
        assert_eq!(scoping_draft_for(&state, root, &other).expect("gate"), None);

        let mut state = State::default();
        plans::materialize_scoping(root, "2026-09-26.08-41-05").expect("materialize");
        let sent = SessionKey {
            plan: "2026-09-26.08-41-05".to_string(),
            role: SessionRole::Scoping,
        };
        state.sessions.insert(
            sent.clone(),
            LiveSession::fresh(
                ActivePlan::scoping(sent.plan.clone()),
                crate::repo_state::RepoState::default(),
            ),
        );
        state.sessions.get_mut(&sent).expect("live").last_prompt = Some("hi".to_string());
        assert_eq!(scoping_draft_for(&state, root, &sent).expect("gate"), None);

        let state = State::default();
        let executing = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Executing,
        };
        assert_eq!(
            scoping_draft_for(&state, root, &executing).expect("gate"),
            None
        );

        let state = State::default();
        let gone = SessionKey {
            plan: "2026-09-26.08-41-99".to_string(),
            role: SessionRole::Scoping,
        };
        assert!(scoping_draft_for(&state, root, &gone).is_err());
    }

    #[test]
    fn vanish_removes_dir_session_activity_and_pending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        plans::materialize_scoping(root, name).expect("materialize");
        let mut state = State {
            pending_scoping: Some(name.to_string()),
            ..Default::default()
        };
        state.activity.insert(name.to_string(), Instant::now());
        state.sessions.insert(
            SessionKey {
                plan: name.to_string(),
                role: SessionRole::Scoping,
            },
            LiveSession::fresh(
                ActivePlan::scoping(name.to_string()),
                crate::repo_state::RepoState::default(),
            ),
        );
        vanish_scoping(&mut state, root, name);
        assert!(
            !plans::PlanRef {
                name: name.to_string(),
                phase: plans::Phase::Scoping,
            }
            .path(root)
            .exists()
        );
        assert!(state.sessions.is_empty());
        assert!(!state.activity.contains_key(name));
        assert_eq!(state.pending_scoping, None);
        vanish_scoping(&mut state, root, "2026-09-26.08-41-99");
    }

    #[test]
    fn double_reserve_reuses_empty_pending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let mut state = State::default();
        let name = reserve_scoping_name(root, state.pending_scoping.as_deref());
        state.pending_scoping = Some(name.clone());
        state.current = Some(SessionKey {
            plan: name.clone(),
            role: SessionRole::Scoping,
        });
        let reuse = state.current.clone().is_some_and(|current| {
            state.pending_scoping.as_deref() == Some(current.plan.as_str())
                && current.role == SessionRole::Scoping
                && is_empty_scoping(&state, root, &current.plan)
        });
        assert!(reuse);
        assert!(
            !plans::PlanRef {
                name,
                phase: plans::Phase::Scoping,
            }
            .path(root)
            .exists()
        );
    }

    #[test]
    fn send_path_materializes_and_clears_pending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let mut state = State {
            pending_scoping: Some("2026-09-26.08-41-03".to_string()),
            ..Default::default()
        };
        let session = SessionKey {
            plan: "2026-09-26.08-41-03".to_string(),
            role: SessionRole::Scoping,
        };
        assert!(state.pending_scoping.as_deref() == Some(session.plan.as_str()));
        plans::materialize_scoping(root, &session.plan).expect("materialize");
        if state.pending_scoping.as_deref() == Some(session.plan.as_str()) {
            state.pending_scoping = None;
        }
        assert!(
            plans::PlanRef {
                name: session.plan.clone(),
                phase: plans::Phase::Scoping,
            }
            .path(root)
            .is_dir()
        );
        assert_eq!(state.pending_scoping, None);
    }

    #[test]
    fn select_away_discards_only_empty_prev() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        plans::materialize_scoping(root, "2026-09-26.08-41-04").expect("target");
        let mut state = State {
            pending_scoping: Some("2026-09-26.08-41-03".to_string()),
            current: Some(SessionKey {
                plan: "2026-09-26.08-41-03".to_string(),
                role: SessionRole::Scoping,
            }),
            ..Default::default()
        };
        let target = SessionKey {
            plan: "2026-09-26.08-41-04".to_string(),
            role: SessionRole::Scoping,
        };
        if let Some(prev) = state.current.clone()
            && prev != target
            && prev.role == SessionRole::Scoping
            && is_empty_scoping(&state, root, &prev.plan)
        {
            vanish_scoping(&mut state, root, &prev.plan);
        }
        state.current = Some(target.clone());
        assert_eq!(state.pending_scoping, None);
        assert!(
            plans::PlanRef {
                name: target.plan.clone(),
                phase: plans::Phase::Scoping,
            }
            .path(root)
            .is_dir()
        );
        let entries = sorted_entries(
            root,
            &state.sessions,
            &state.activity,
            state.pending_scoping.as_deref(),
        );
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, target.plan);
    }

    async fn open_test_session(
        cwd: &Path,
        env: HashMap<String, String>,
    ) -> Result<String, AgentError> {
        let binary = acp::resolve_opencode_binary()?;
        let config = AcpAgentConfig::new(binary).arg("acp").envs(env);
        let agent = AcpAgent::new(config);
        let cwd_owned = cwd.to_path_buf();
        Client
            .builder()
            .on_receive_notification(
                async move |_notification: acp::SessionNotification, _cx| Ok(()),
                acp::on_receive_notification!(),
            )
            .on_receive_request(
                async move |_request: acp::RequestPermissionRequest, responder, _connection| {
                    responder.respond(acp::RequestPermissionResponse::new(
                        acp::RequestPermissionOutcome::Cancelled,
                    ))
                },
                acp::on_receive_request!(),
            )
            .connect_with(agent, |connection: ConnectionTo<Agent>| async move {
                connection
                    .send_request(acp::build_initialize_request())
                    .block_task()
                    .await
                    .map_err(|error| acp::internal_error(error.to_string()))?;
                let response = connection
                    .send_request(acp::build_new_session_request(&cwd_owned))
                    .block_task()
                    .await
                    .map_err(|error| acp::internal_error(error.to_string()))?;
                Ok(response.session_id.to_string())
            })
            .await
            .map_err(|error| AgentError::RequestFailed {
                raw: error.to_string(),
            })
    }

    #[tokio::test]
    async fn lifecycle_opens_session_in_temp_git_repo() {
        if crate::acp::resolve_opencode_binary().is_err() {
            return;
        }
        let dir = tempfile::tempdir().expect("tempdir");
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "test"],
            vec!["commit", "--allow-empty", "-m", "init"],
        ] {
            let output = std::process::Command::new("git")
                .args(&args)
                .current_dir(dir.path())
                .output()
                .expect("git");
            assert!(output.status.success(), "{args:?}");
        }
        let scratch = tempfile::tempdir().expect("scratch");
        let mut env = HashMap::new();
        env.insert(
            "XDG_DATA_HOME".to_string(),
            scratch.path().join("data").to_string_lossy().to_string(),
        );
        env.insert(
            "XDG_CONFIG_HOME".to_string(),
            scratch.path().join("config").to_string_lossy().to_string(),
        );
        let session_id = open_test_session(dir.path(), env)
            .await
            .expect("initialize plus session/new");
        assert!(!session_id.is_empty());
    }
}
