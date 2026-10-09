// Agent lifecycle: one `opencode acp` child process per live session,
// sessions keyed by plan directory name plus role, plans listed from disk.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use crate::awake;
use crate::opencode;
use crate::plans;
use crate::types::{AgentError, AppEvent, OpenRepoResult, PlansUpdate, SessionKey, SessionRole};

mod advance;
mod config;
mod permissions;
mod plans_list;
mod session;
mod session_ids;
mod start;
mod turns;
mod warm;

pub(crate) use session::LiveSession;

#[derive(Default)]
pub(crate) struct State {
    sessions: HashMap<SessionKey, LiveSession>,
    current: Option<SessionKey>,
    repo_root: Option<PathBuf>,
    checkout_branch: String,
    branch_watch: Option<notify::RecommendedWatcher>,
    awake: Option<awake::Guard>,
    /// Plans with a sent user prompt. Feeds the empty-scoping gate;
    /// carried across renames with the plan.
    prompted: HashSet<String>,
    /// Diskless scoping sessions: reserved name plus draft-inferred title.
    /// Presence means pending; the value is the draft-inferred title
    /// (`None` renders as `Untitled`). Listed until the first `send_prompt`
    /// materializes the directory; vanishing leaves no trace.
    pending_scoping: HashMap<String, Option<String>>,
    /// Background warms in flight, one per session key. Single-flights
    /// `warm_session` against a racing `send_prompt`.
    warming: HashSet<SessionKey>,
    /// History replays in flight, one per session key. Single-flights
    /// `load_history` against a racing `send_prompt`, which waits.
    history_loading: HashSet<SessionKey>,
    /// Replayed histories: keys whose replay already streamed. Skips
    /// re-replay on reselect.
    history_loaded: HashSet<SessionKey>,
    /// Worktree checkouts per executing plan: path and branch. Rebuilt
    /// from disk on open. The landing target is never stored: it always
    /// resolves live from `checkout_branch` at decision time.
    worktrees: HashMap<String, crate::worktrees::WorktreeRecord>,
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

    /// True while a live turn runs for the key. The turn owns the
    /// session pickers while it runs.
    fn is_working(&self, key: &SessionKey) -> bool {
        self.sessions
            .get(key)
            .map(|session| session.working)
            .unwrap_or(false)
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
    if state.prompted.contains(name) {
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
/// `prompted` entry. Removes the single pending-map entry on match.
/// Caller owns the lock.
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
    state.drop_start_claims_for(&SessionKey {
        plan: name.to_string(),
        role: SessionRole::Scoping,
    });
    state.prompted.remove(name);
    state.pending_scoping.remove(name);
}

/// Keep an empty scoping session when its draft box holds non-blank text,
/// inferring its working title from the draft (overwrite, `None` renders
/// as `Untitled`). Returns true when kept; false means the caller should
/// vanish it. Caller owns the lock; the pending entry stays diskless.
pub(crate) fn keep_or_vanish_empty(
    state: &mut State,
    repo_root: &Path,
    prev: &SessionKey,
    draft: Option<String>,
) -> bool {
    if prev.role != SessionRole::Scoping {
        return false;
    }
    if !is_empty_scoping(state, repo_root, &prev.plan) {
        return false;
    }
    let Some(raw) = draft.as_deref() else {
        return false;
    };
    if raw.trim().is_empty() {
        return false;
    }
    turns::store_draft_title(state, repo_root, &prev.plan, raw.trim());
    true
}

/// Scoping template for one session. Returns the rendered template
/// only while the session is fresh under `is_empty_scoping`, `None` for
/// non-fresh or non-scoping sessions, and an error when the plan is
/// gone. Reads the locked state plus disk.
pub(crate) fn scoping_template_for(
    state: &State,
    repo_root: &Path,
    session: &SessionKey,
) -> Result<Option<String>, AgentError> {
    if session.role != SessionRole::Scoping {
        return Ok(None);
    }
    let is_pending = state.pending_scoping.contains_key(&session.plan);
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
    Ok(Some(opencode::scoping_template(&display)))
}

/// Reserved timestamp name for a lazy scoping session: collision-proof
/// against on-disk scoping names plus every pending name. Pure
/// except the directory read.
pub(crate) fn reserve_scoping_name(
    repo_root: &Path,
    pending: &HashMap<String, Option<String>>,
) -> String {
    let mut taken: std::collections::HashSet<String> = plans::scan_plans(repo_root)
        .into_iter()
        .filter(|plan| plan.phase == plans::Phase::Scoping)
        .map(|plan| plan.name)
        .collect();
    taken.extend(pending.keys().cloned());
    plans::unique_name(&plans::timestamp_now(), &taken)
}

#[derive(Clone)]
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

    /// Open a repository: ensure the plan structure, prune stale worktree
    /// metadata, re-register surviving worktrees, rescan every plan, and
    /// select the most-recent session. Lazy sessions still start on their
    /// first prompt, with transcripts and TODOs kept run-local; automatic
    /// plans that are already ready advance at once via the pump. With zero
    /// scoping dirs, reserves a pending session without touching disk.
    pub async fn open_repo(&self, repo_root: PathBuf) -> Result<OpenRepoResult, AgentError> {
        plans::ensure_structure(&repo_root)?;
        crate::worktrees::prune(&repo_root)?;
        self.clear_sessions();
        {
            let mut state = self.state.lock().expect("state poisoned");
            state.repo_root = Some(repo_root.clone());
            state.checkout_branch =
                crate::branch::current_branch(&repo_root).unwrap_or_else(|| "HEAD".to_string());
            // Crash recovery: surviving worktrees rejoin the map with
            // derived names; the landing target stays live.
            let scanned = plans::scan_plans(&repo_root);
            for plan in &scanned {
                if plan.phase != plans::Phase::Executing {
                    continue;
                }
                let path = crate::worktrees::worktree_path(&repo_root, &plan.name);
                if !path.is_dir() {
                    continue;
                }
                state.worktrees.insert(
                    plan.name.clone(),
                    crate::worktrees::WorktreeRecord {
                        path,
                        worktree_branch: crate::worktrees::branch_name(&plan.name),
                    },
                );
            }
            if scanned
                .iter()
                .all(|plan| plan.phase != plans::Phase::Scoping)
            {
                let name = reserve_scoping_name(&repo_root, &state.pending_scoping);
                state.pending_scoping.insert(name.clone(), None);
                state.current = Some(SessionKey {
                    plan: name,
                    role: SessionRole::Scoping,
                });
            }
        }
        self.watch_branch(&repo_root);
        self.pump().await;
        Ok(self.open_result())
    }

    /// Reserve a fresh scoping session without touching disk. Reuses the
    /// pending session when the selection is still on an empty one with a
    /// blank draft; a non-blank draft keeps the old pending with its
    /// inferred title and reserves a new one. The start flow owns liveness:
    /// empty keys load history, non-empty keys warm, so this path spawns
    /// neither. `draft` is the unsent text of `state.current` at call time.
    pub async fn create_plan(&self, draft: Option<String>) -> Result<PlansUpdate, AgentError> {
        let repo_root = self.current_repo().ok_or_else(|| AgentError::NoSession {
            raw: "open a repository first".to_string(),
        })?;
        {
            let state = self.state.lock().expect("state poisoned");
            if let Some(current) = state.current.clone()
                && current.role == SessionRole::Scoping
                && state.pending_scoping.contains_key(&current.plan)
                && is_empty_scoping(&state, &repo_root, &current.plan)
                && draft.as_deref().map(str::trim).unwrap_or("").is_empty()
            {
                drop(state);
                return Ok(self.plans_update());
            }
        }
        match self.state.lock() {
            Ok(mut state) => {
                if let Some(prev) = state.current.clone()
                    && prev.role == SessionRole::Scoping
                    && is_empty_scoping(&state, &repo_root, &prev.plan)
                    && !keep_or_vanish_empty(&mut state, &repo_root, &prev, draft)
                {
                    vanish_scoping(&mut state, &repo_root, &prev.plan);
                }
                let name = reserve_scoping_name(&repo_root, &state.pending_scoping);
                state.pending_scoping.insert(name.clone(), None);
                state.current = Some(SessionKey {
                    plan: name,
                    role: SessionRole::Scoping,
                });
            }
            Err(error) => {
                log::warn!("failed to select new plan: {error}");
            }
        }
        Ok(self.plans_update())
    }

    /// Re-check the branch for the open repo. Failures keep the last value.
    /// A moved target re-resolves `ffable` and may unblock finishes or
    /// re-run a landing rebase, so a move pumps at once.
    pub async fn refresh_branch(&self) -> Result<String, AgentError> {
        let repo_root = lock_state(&self.state)
            .and_then(|state| state.repo_root.clone())
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let branch =
            crate::branch::current_branch(&repo_root).unwrap_or_else(|| "HEAD".to_string());
        if self.set_branch(branch.clone()) {
            self.pump().await;
        }
        Ok(branch)
    }

    /// Set the held branch and emit it, then refresh the plans list so
    /// `ffable` recomputes against the new live target.
    /// Returns true when it moved.
    fn set_branch(&self, branch: String) -> bool {
        let moved = match self.state.lock() {
            Ok(mut state) => {
                if state.checkout_branch == branch {
                    false
                } else {
                    state.checkout_branch = branch.clone();
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
            self.push_plans();
        }
        moved
    }

    /// Watch `.git/HEAD` for the open repo, refreshing
    /// `state.checkout_branch` per event so execute/retry snapshots go
    /// fresh free. Branch moves also push a refreshed plans list so
    /// `ffable` recomputes against the new live target, then pump so a
    /// moved target unblocks finishes or re-runs a landing rebase.
    fn watch_branch(&self, repo_root: &Path) {
        let root = repo_root.to_path_buf();
        let state = Arc::clone(&self.state);
        let app = self.app.clone();
        let push_state = Arc::clone(&self.state);
        let push_app = self.app.clone();
        let pump_manager = self.clone();
        let watcher = crate::branch::watch_branch(root.clone(), move |branch| {
            let moved = match state.lock() {
                Ok(mut guard) => {
                    if guard.repo_root.as_ref() != Some(&root) || guard.checkout_branch == branch {
                        false
                    } else {
                        guard.checkout_branch = branch.clone();
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
                plans_list::push_sorted(&push_state, &push_app);
                pump_manager.spawn_pump();
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

    /// Approve the scoping plan: move it to executing and start the
    /// executing agent. Thin over the shared transition core; the pump
    /// drives the same core automatically.
    pub async fn execute_plan(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        if session.role != SessionRole::Scoping {
            return Err(AgentError::RequestFailed {
                raw: "no scoping plan to execute".to_string(),
            });
        }
        self.transition(advance::Transition::Execute { plan: session.plan })
            .await
    }

    /// Finish a plan via the shared transition core: a clean
    /// fast-forwardable executing plan completes directly, a clean landing
    /// plan finishes the same way once it lands.
    pub async fn finish_landing(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        let action = match session.role {
            SessionRole::Executing => advance::Transition::FinishExecuting { plan: session.plan },
            SessionRole::Landing => advance::Transition::FinishLanding { plan: session.plan },
            SessionRole::Scoping => {
                return Err(AgentError::RequestFailed {
                    raw: "no executing plan to complete".to_string(),
                });
            }
        };
        self.transition(action).await
    }

    /// Move a clean diverged executing plan to landing via the shared core.
    /// Thin over `transition`; the pump drives the same core automatically.
    pub async fn begin_landing(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
        if session.role != SessionRole::Executing {
            return Err(AgentError::RequestFailed {
                raw: "no executing plan to land".to_string(),
            });
        }
        self.transition(advance::Transition::BeginLanding { plan: session.plan })
            .await
    }

    /// Cancel an active plan. An empty scoping session vanishes without a
    /// `cancelled/` trace; otherwise the plan moves to `cancelled/`,
    /// force-removing the worktree and branch for executing and landing
    /// plans. The selection stays on the cancelled (now read-only) session.
    pub async fn cancel_plan(&self, session: SessionKey) -> Result<PlansUpdate, AgentError> {
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
                            &state.pending_scoping,
                        );
                        if let Some(key) = plans_list::most_recent_key(&plans) {
                            state.current = Some(key);
                        } else {
                            let name = reserve_scoping_name(&repo_root, &state.pending_scoping);
                            state.pending_scoping.insert(name.clone(), None);
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
        let phase = session::role_phase(session.role);
        let from = plans::PlanRef {
            name: session.plan.clone(),
            phase,
        };
        if !from.path(&repo_root).is_dir() {
            return Err(AgentError::RequestFailed {
                raw: "no active plan to cancel".to_string(),
            });
        }
        if matches!(phase, plans::Phase::Executing | plans::Phase::Landing) {
            self.remove_worktree(&repo_root, &from.name, true)?;
        }
        let next = plans::cancel(&repo_root, &from)?;
        if phase == plans::Phase::Scoping {
            self.drop_live(&session).await;
        } else {
            self.drop_plan_lives(&from.name).await;
        }
        self.carry_prompted(&from.name, &next.name);
        self.select_key(SessionKey {
            plan: next.name,
            role: session.role,
        });
        Ok(self.plans_update())
    }

    /// Flip one plan between automatic and manual mode. Persists `manual`
    /// into `state.json` and returns the refreshed list. Never touches live
    /// sessions: flipping modes never kills a running turn. Flipping back
    /// to A pumps at once so a held plan joins the flow immediately.
    pub async fn set_plan_mode(
        &self,
        plan: String,
        manual: bool,
    ) -> Result<PlansUpdate, AgentError> {
        let (repo_root, _) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let Some(plan_dir) = session_ids::locate(&repo_root, &plan) else {
            return Err(AgentError::NoSession {
                raw: "plan is gone".to_string(),
            });
        };
        let mut state = plans::load_state(&plan_dir);
        state.manual = manual;
        plans::store_state(&plan_dir, &state);
        if !manual {
            self.pump().await;
        }
        Ok(self.plans_update())
    }

    /// Select one session, keeping a previously-selected empty scoping
    /// session when its draft holds text (with its inferred title) and
    /// discarding it the same way as cancel otherwise. Returns the target
    /// selection. `draft` is the unsent text of `state.current` at call
    /// time. No `ensure_idle` gate: selection is allowed anytime, and an
    /// empty previous session is never working.
    pub async fn select_plan(
        &self,
        session: SessionKey,
        draft: Option<String>,
    ) -> Result<PlansUpdate, AgentError> {
        let (repo_root, _) = self
            .reopen_snapshot()
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        {
            let state = self.state.lock().expect("state poisoned");
            // Name-based across all phases: finished plans live under
            // `completed/` or `cancelled/` and stay selectable read-only.
            // Only names missing everywhere report "plan is gone".
            let target_exists = (session.role == SessionRole::Scoping
                && state.pending_scoping.contains_key(&session.plan))
                || session_ids::plan_exists(&repo_root, &session.plan);
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
                    && !keep_or_vanish_empty(&mut state, &repo_root, &prev, draft)
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

    /// Scoping template: the template with its plan dir filled in,
    /// returned only while the session is fresh under the
    /// `is_empty_scoping` gate. Non-fresh and non-scoping sessions get
    /// `None`; missing repos and gone plans are errors, never silent
    /// fallbacks.
    pub fn scoping_template(&self, session: SessionKey) -> Result<Option<String>, AgentError> {
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
        scoping_template_for(&state_guard, &repo_root, &session)
    }

    /// Live worktree coordinates plus landing state for one plan. The
    /// target always resolves live from `checkout_branch` at call time, so
    /// a branch switch mid-flight retargets in-flight plans.
    fn worktree_status_for(
        &self,
        repo_root: &Path,
        plan_name: &str,
    ) -> Result<WorktreeLive, AgentError> {
        let (record, target_branch) = match lock_state(&self.state) {
            Some(state) => (
                state.worktrees.get(plan_name).cloned(),
                state.checkout_branch.clone(),
            ),
            None => (None, String::new()),
        };
        let (path, worktree_branch) = match record {
            Some(record) => (record.path, record.worktree_branch),
            None => (
                crate::worktrees::worktree_path(repo_root, plan_name),
                crate::worktrees::branch_name(plan_name),
            ),
        };
        let dirty = crate::worktrees::is_dirty(&path)?;
        let ffable = crate::worktrees::is_ffable(repo_root, &target_branch, &worktree_branch)?;
        Ok(WorktreeLive {
            path,
            worktree_branch,
            target_branch,
            dirty,
            ffable,
        })
    }

    /// Drop every live session a plan owns. Finishes and cancels land on
    /// history rows; the transcripts stay frontend-side.
    async fn drop_plan_lives(&self, plan_name: &str) {
        for role in [SessionRole::Executing, SessionRole::Landing] {
            self.drop_live(&SessionKey {
                plan: plan_name.to_string(),
                role,
            })
            .await;
        }
    }

    /// Delete one plan worktree and its branch, forgetting the map entry.
    /// Force only on explicit user action; finishes run clean-gated
    /// without force. A missing map entry falls back to the derived path
    /// and branch so recovered plans still clean up.
    fn remove_worktree(
        &self,
        repo_root: &Path,
        plan_name: &str,
        force: bool,
    ) -> Result<(), AgentError> {
        let record =
            lock_state(&self.state).and_then(|state| state.worktrees.get(plan_name).cloned());
        let (path, worktree_branch) = match record {
            Some(record) => (record.path, record.worktree_branch),
            None => (
                crate::worktrees::worktree_path(repo_root, plan_name),
                crate::worktrees::branch_name(plan_name),
            ),
        };
        crate::worktrees::remove(repo_root, &path, &worktree_branch, force)?;
        if let Some(mut state) = lock_state(&self.state) {
            state.worktrees.remove(plan_name);
        }
        Ok(())
    }

    /// Announce an eager executing/landing first prompt as a live YOU
    /// bubble, matching what history replay shows. Shared so the two
    /// call sites cannot drift apart; retries never call here.
    fn announce_first_prompt(&self, key: &SessionKey, text: &str) {
        emit_event(
            &self.app,
            AppEvent::UserText {
                session: key.clone(),
                chunk: text.to_string(),
            },
        );
    }
}

/// Live worktree coordinates plus landing state for one plan.
struct WorktreeLive {
    path: PathBuf,
    worktree_branch: String,
    target_branch: String,
    dirty: bool,
    ffable: bool,
}

/// Which landing step the user asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LandingStep {
    Begin,
    Finish,
}

/// Refuse a landing step that cannot run. A dirty worktree never lands;
/// an already fast-forwardable branch finishes directly instead of
/// starting landing; a diverged branch starts landing before finishing.
/// Pure.
fn gate_landing(dirty: bool, ffable: bool, step: LandingStep) -> Result<(), AgentError> {
    if dirty {
        return Err(AgentError::RequestFailed {
            raw: "commit or discard worktree changes first".to_string(),
        });
    }
    match step {
        LandingStep::Begin => {
            if ffable {
                return Err(AgentError::RequestFailed {
                    raw: "already fast-forwardable, land it directly".to_string(),
                });
            }
            Ok(())
        }
        LandingStep::Finish => {
            if !ffable {
                return Err(AgentError::RequestFailed {
                    raw: "target moved on, start landing first".to_string(),
                });
            }
            Ok(())
        }
    }
}

/// Refuse plan transitions while the session's turn runs. A poisoned lock
/// only logs, matching `lock_state`.
fn ensure_idle(state: &Mutex<State>, key: &SessionKey) -> Result<(), AgentError> {
    match state.lock() {
        Ok(state) => {
            if state.is_working(key) {
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
    use super::session::ActivePlan;
    use super::*;
    use crate::acp::{
        self, AcpAgent, AcpAgentConfig, Agent, Client, ConnectionTo, SessionConfigOption,
        SessionConfigOptionCategory, SessionConfigSelectOption, SessionConfigValueId,
    };
    use std::collections::HashMap;
    use std::path::Path;

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

    #[test]
    fn entries_sort_scoping_first_then_rest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Cancelled, "c", Some("C"));
        write_plan_dir(root, plans::Phase::Completed, "b", Some("B"));
        write_plan_dir(root, plans::Phase::Landing, "m", Some("M"));
        write_plan_dir(root, plans::Phase::Executing, "a", Some("A"));
        write_plan_dir(root, plans::Phase::Scoping, "s", Some("S"));
        let entries = sorted_entries(root, &HashMap::new(), &HashMap::new());
        let phases: Vec<plans::Phase> = entries.iter().map(|entry| entry.phase).collect();
        assert_eq!(
            phases,
            vec![
                plans::Phase::Scoping,
                plans::Phase::Executing,
                plans::Phase::Landing,
                plans::Phase::Completed,
                plans::Phase::Cancelled,
            ]
        );
        assert_eq!(entries[0].title, "S");
    }

    #[test]
    fn arrival_orders_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        plans::materialize_scoping(root, "2026-09-26.08-41-03").expect("old");
        std::thread::sleep(std::time::Duration::from_millis(10));
        plans::materialize_scoping(root, "2026-09-26.08-41-04").expect("new");
        let entries = sorted_entries(root, &HashMap::new(), &HashMap::new());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "2026-09-26.08-41-04");
        assert_eq!(entries[1].name, "2026-09-26.08-41-03");
    }

    #[test]
    fn unstamped_plans_fall_back_to_name_descending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Scoping, "first", Some("First"));
        write_plan_dir(root, plans::Phase::Scoping, "second", Some("Second"));
        let entries = sorted_entries(root, &HashMap::new(), &HashMap::new());
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
        let entries = sorted_entries(root, &HashMap::new(), &HashMap::new());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Untitled");
        assert_eq!(entries[0].sessions.len(), 1);
    }

    #[test]
    fn most_recent_prefers_executing_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Executing, "a", Some("A"));
        let entries = sorted_entries(root, &HashMap::new(), &HashMap::new());
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
    fn most_recent_prefers_landing_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Landing, "m", Some("M"));
        let entries = sorted_entries(root, &HashMap::new(), &HashMap::new());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sessions.len(), 3);
        assert_eq!(
            most_recent_key(&entries),
            Some(SessionKey {
                plan: "m".to_string(),
                role: SessionRole::Landing,
            })
        );
    }

    #[test]
    fn landing_gate_refuses_dirty_and_misplaced_steps() {
        assert!(gate_landing(false, true, LandingStep::Finish).is_ok());
        assert!(gate_landing(false, false, LandingStep::Begin).is_ok());
        assert!(gate_landing(true, true, LandingStep::Finish).is_err());
        assert!(gate_landing(true, false, LandingStep::Begin).is_err());
        assert!(gate_landing(false, false, LandingStep::Finish).is_err());
        assert!(gate_landing(false, true, LandingStep::Begin).is_err());
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
        let entries = sorted_entries(root, &sessions, &HashMap::new());
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
        let entries = sorted_entries(root, &HashMap::new(), &HashMap::new());
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
        let pending = HashMap::from([(name.to_string(), None)]);
        let entries = sorted_entries(root, &HashMap::new(), &pending);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, name);
        assert_eq!(entries[0].title, "Untitled");
        assert!(!entries[0].has_plan_md);
        assert_eq!(entries[0].sessions.len(), 1);
        assert_eq!(entries[0].sessions[0].role, SessionRole::Scoping);
    }

    #[test]
    fn multiple_pendings_sort_by_name_descending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let pending = HashMap::from([
            ("2026-09-26.08-41-03".to_string(), None),
            ("2026-09-26.08-41-04".to_string(), None),
        ]);
        let entries = sorted_entries(root, &HashMap::new(), &pending);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "2026-09-26.08-41-04");
        assert_eq!(entries[1].name, "2026-09-26.08-41-03");
    }

    #[test]
    fn pending_with_title_renders() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let pending = HashMap::from([(name.to_string(), Some("Login flow fixes".to_string()))]);
        let entries = sorted_entries(root, &HashMap::new(), &pending);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Login flow fixes");
    }

    #[test]
    fn reserve_creates_no_dir_and_avoids_taken_names() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        plans::materialize_scoping(root, "2026-09-26.08-41-03").expect("materialize");
        let pending = HashMap::from([("2026-09-26.08-41-04".to_string(), None)]);
        let reserved = reserve_scoping_name(root, &pending);
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
    fn empty_scoping_needs_no_plan_md_no_prompt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let mut state = State::default();
        assert!(is_empty_scoping(&state, root, name));
        plans::materialize_scoping(root, name).expect("materialize");
        assert!(is_empty_scoping(&state, root, name));
        state.prompted.insert(name.to_string());
        assert!(!is_empty_scoping(&state, root, name));
        state.prompted.remove(name);
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
    fn scoping_starts_unprefixed() {
        assert!(!ActivePlan::scoping("n".to_string()).prefixed);
    }

    #[test]
    fn executing_runs_inside_its_worktree() {
        use std::path::Path;
        let root = Path::new("/repo");
        let name = "2026-09-26.14-53-26.shiny-feature".to_string();
        assert_eq!(
            ActivePlan::executing(name.clone()).cwd(root),
            crate::worktrees::worktree_path(root, &name)
        );
        assert_eq!(
            ActivePlan::scoping(name).cwd(root),
            Path::new("/repo").to_path_buf()
        );
    }

    #[test]
    fn template_returns_only_while_fresh() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let key = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        let state = State {
            pending_scoping: HashMap::from([(name.to_string(), None)]),
            ..Default::default()
        };
        let template = scoping_template_for(&state, root, &key)
            .expect("template")
            .expect("fresh session renders");
        assert!(template.contains(".samokod/plans/scoping/2026-09-26.08-41-03"));
        assert!(!template.contains("{{PLAN_DIR}}"));

        let state = State::default();
        plans::materialize_scoping(root, name).expect("materialize");
        std::fs::write(
            root.join(".samokod/plans/scoping/2026-09-26.08-41-03/plan.md"),
            "# T\n",
        )
        .expect("write");
        assert_eq!(
            scoping_template_for(&state, root, &key).expect("gate"),
            None
        );

        let mut state = State::default();
        plans::materialize_scoping(root, "2026-09-26.08-41-04").expect("materialize");
        let other = SessionKey {
            plan: "2026-09-26.08-41-04".to_string(),
            role: SessionRole::Scoping,
        };
        state.prompted.insert(other.plan.clone());
        assert_eq!(
            scoping_template_for(&state, root, &other).expect("gate"),
            None
        );

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
        assert_eq!(
            scoping_template_for(&state, root, &sent).expect("gate"),
            None
        );

        let state = State::default();
        let executing = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Executing,
        };
        assert_eq!(
            scoping_template_for(&state, root, &executing).expect("gate"),
            None
        );

        let state = State::default();
        let gone = SessionKey {
            plan: "2026-09-26.08-41-99".to_string(),
            role: SessionRole::Scoping,
        };
        assert!(scoping_template_for(&state, root, &gone).is_err());
    }

    #[test]
    fn vanish_removes_dir_session_prompt_and_pending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        plans::materialize_scoping(root, name).expect("materialize");
        let mut state = State {
            pending_scoping: HashMap::from([(name.to_string(), None)]),
            ..Default::default()
        };
        state.prompted.insert(name.to_string());
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
        assert!(!state.prompted.contains(name));
        assert!(!state.pending_scoping.contains_key(name));
        vanish_scoping(&mut state, root, "2026-09-26.08-41-99");
    }

    #[test]
    fn double_reserve_reuses_empty_pending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let mut state = State::default();
        let name = reserve_scoping_name(root, &state.pending_scoping);
        state.pending_scoping.insert(name.clone(), None);
        state.current = Some(SessionKey {
            plan: name.clone(),
            role: SessionRole::Scoping,
        });
        let reuse = state.current.clone().is_some_and(|current| {
            state.pending_scoping.contains_key(&current.plan)
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
            pending_scoping: HashMap::from([("2026-09-26.08-41-03".to_string(), None)]),
            ..Default::default()
        };
        let session = SessionKey {
            plan: "2026-09-26.08-41-03".to_string(),
            role: SessionRole::Scoping,
        };
        assert!(state.pending_scoping.contains_key(&session.plan));
        plans::materialize_scoping(root, &session.plan).expect("materialize");
        state.pending_scoping.remove(&session.plan);
        assert!(
            plans::PlanRef {
                name: session.plan.clone(),
                phase: plans::Phase::Scoping,
            }
            .path(root)
            .is_dir()
        );
        assert!(!state.pending_scoping.contains_key(&session.plan));
    }

    #[test]
    fn select_away_discards_only_empty_prev() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        plans::materialize_scoping(root, "2026-09-26.08-41-04").expect("target");
        let mut state = State {
            pending_scoping: HashMap::from([("2026-09-26.08-41-03".to_string(), None)]),
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
        assert!(!state.pending_scoping.contains_key("2026-09-26.08-41-03"));
        assert!(
            plans::PlanRef {
                name: target.plan.clone(),
                phase: plans::Phase::Scoping,
            }
            .path(root)
            .is_dir()
        );
        let entries = sorted_entries(root, &state.sessions, &state.pending_scoping);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, target.plan);
    }

    #[test]
    fn keep_with_draft_sets_title_via_extractor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let mut state = State {
            pending_scoping: HashMap::from([(name.to_string(), None)]),
            ..Default::default()
        };
        let prev = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        let draft = Some("Fix the login flow login errors on login retry.".to_string());
        assert!(keep_or_vanish_empty(&mut state, root, &prev, draft));
        let title = state
            .pending_scoping
            .get(name)
            .expect("pending kept")
            .clone()
            .expect("title inferred");
        assert!(title.to_lowercase().contains("login"));
        assert!(
            !plans::PlanRef {
                name: name.to_string(),
                phase: plans::Phase::Scoping,
            }
            .path(root)
            .exists()
        );
    }

    #[test]
    fn keep_with_whitespace_discards() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let mut state = State {
            pending_scoping: HashMap::from([(name.to_string(), None)]),
            ..Default::default()
        };
        let prev = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        assert!(!keep_or_vanish_empty(
            &mut state,
            root,
            &prev,
            Some("   \n  ".to_string())
        ));
        assert!(!keep_or_vanish_empty(&mut state, root, &prev, None));
        vanish_scoping(&mut state, root, name);
        assert!(!state.pending_scoping.contains_key(name));
    }

    #[test]
    fn second_keep_overwrites_title() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let mut state = State {
            pending_scoping: HashMap::from([(name.to_string(), None)]),
            ..Default::default()
        };
        let prev = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        assert!(keep_or_vanish_empty(
            &mut state,
            root,
            &prev,
            Some("Fix the login flow login errors on login retry.".to_string())
        ));
        assert!(keep_or_vanish_empty(
            &mut state,
            root,
            &prev,
            Some("Fix the checkout flow checkout errors on checkout retry.".to_string())
        ));
        let title = state
            .pending_scoping
            .get(name)
            .expect("pending kept")
            .clone()
            .expect("title inferred");
        assert!(title.to_lowercase().contains("checkout"));
    }

    #[test]
    fn extractor_none_draft_keeps_untitled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let mut state = State {
            pending_scoping: HashMap::from([(
                name.to_string(),
                Some("Login flow fixes".to_string()),
            )]),
            ..Default::default()
        };
        let prev = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        assert!(keep_or_vanish_empty(
            &mut state,
            root,
            &prev,
            Some("... !!! ???".to_string())
        ));
        assert_eq!(state.pending_scoping.get(name), Some(&None));
        let entries = sorted_entries(root, &state.sessions, &state.pending_scoping);
        assert_eq!(entries[0].title, "Untitled");
    }

    #[test]
    fn create_with_draft_keeps_old_and_selects_new() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let mut state = State {
            pending_scoping: HashMap::from([("2026-09-26.08-41-03".to_string(), None)]),
            current: Some(SessionKey {
                plan: "2026-09-26.08-41-03".to_string(),
                role: SessionRole::Scoping,
            }),
            ..Default::default()
        };
        let prev = state.current.clone().expect("current");
        let draft = Some("Fix the login flow login errors on login retry.".to_string());
        assert!(keep_or_vanish_empty(&mut state, root, &prev, draft));
        let name = reserve_scoping_name(root, &state.pending_scoping);
        assert_ne!(name, "2026-09-26.08-41-03");
        state.pending_scoping.insert(name.clone(), None);
        state.current = Some(SessionKey {
            plan: name.clone(),
            role: SessionRole::Scoping,
        });
        assert!(
            state
                .pending_scoping
                .get("2026-09-26.08-41-03")
                .is_some_and(|title| title.is_some())
        );
        assert_eq!(
            state.pending_scoping.get(&name),
            Some(&None),
            "new pending starts untitled"
        );
        assert!(
            !plans::PlanRef {
                name: name.clone(),
                phase: plans::Phase::Scoping,
            }
            .path(root)
            .exists()
        );
    }

    #[test]
    fn cancel_on_draft_kept_pending_still_vanishes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let mut state = State {
            pending_scoping: HashMap::from([(name.to_string(), None)]),
            ..Default::default()
        };
        let prev = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        assert!(keep_or_vanish_empty(
            &mut state,
            root,
            &prev,
            Some("Fix the login flow login errors on login retry.".to_string())
        ));
        assert!(is_empty_scoping(&state, root, name));
        vanish_scoping(&mut state, root, name);
        assert!(!state.pending_scoping.contains_key(name));
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
            let output = crate::git::command(dir.path())
                .args(&args)
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
