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
mod landing_queue;
mod permissions;
mod plans_list;
mod session;
mod session_ids;
mod start;
mod turns;
mod warm;

pub(crate) use session::LiveSession;

#[derive(Default)]
pub(crate) struct WindowState {
    /// Incarnation counter: bumped on every `open_repo` reset and on first
    /// creation from the backend counter. Async tasks capture it at spawn
    /// and drop their emits when it moved, so a reused label never shows
    /// another repo's transcript.
    generation: u64,
    sessions: HashMap<SessionKey, LiveSession>,
    current: Option<SessionKey>,
    repo_root: Option<PathBuf>,
    checkout_branch: String,
    branch_watch: Option<notify::RecommendedWatcher>,
    /// Plans with a sent user prompt. Memory mirror of the `prompted`
    /// flag in each plan's `state.json`: the memory copy keeps the hot
    /// gate read lock-only, the disk copy survives restarts and travels
    /// with renames.
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

#[derive(Default)]
pub(crate) struct BackendState {
    windows: HashMap<String, WindowState>,
    awake: Option<awake::Guard>,
    /// Monotonic incarnation source for `WindowState::generation`.
    /// Never reused, so a closed label reopened later always differs.
    next_generation: u64,
}

impl BackendState {
    /// Mutable per-window entry, created on first use. Keeps one short
    /// critical section per access; callers never hold the guard across
    /// await, spawn, or agent I/O. New entries take the next incarnation
    /// from the monotonic counter so label reuse never collides.
    pub(crate) fn ensure_window(&mut self, window: &str) -> &mut WindowState {
        if !self.windows.contains_key(window) {
            let generation = self.next_generation;
            self.next_generation += 1;
            self.windows.insert(
                window.to_string(),
                WindowState {
                    generation,
                    ..Default::default()
                },
            );
        }
        self.windows.get_mut(window).expect("window just ensured")
    }

    /// Fresh incarnation for an `open_repo` reset. The caller assigns it
    /// after `reset` so the same label reopened later always differs.
    pub(crate) fn fresh_generation(&mut self) -> u64 {
        let generation = self.next_generation;
        self.next_generation += 1;
        generation
    }

    /// True when the window still owns the spawn-time incarnation and repo.
    /// Pure read; async emits drop on false instead of crossing windows.
    pub(crate) fn is_current(&self, window: &str, generation: u64, repo: &Option<PathBuf>) -> bool {
        match self.windows.get(window) {
            Some(state) => state.generation == generation && &state.repo_root == repo,
            None => false,
        }
    }

    /// Recompute the global sleep guard as working-in-any-window. Runs
    /// after every working-flag mutation.
    fn recompute_awake(&mut self) {
        let any_working = self
            .windows
            .values()
            .flat_map(|window| window.sessions.values())
            .any(|session| session.working);
        if any_working {
            if self.awake.is_none() {
                self.awake = awake::acquire();
            }
        } else {
            self.awake = None;
        }
    }

    /// Repos owned across windows, sorted by path. Pure.
    pub(crate) fn open_repos(&self) -> Vec<crate::types::OpenRepoEntry> {
        let mut repos: Vec<crate::types::OpenRepoEntry> = self
            .windows
            .iter()
            .filter_map(|(window, state)| {
                state
                    .repo_root
                    .as_ref()
                    .map(|root| crate::types::OpenRepoEntry {
                        path: root.to_string_lossy().to_string(),
                        window: window.clone(),
                    })
            })
            .collect();
        repos.sort_by(|a, b| a.path.cmp(&b.path).then_with(|| a.window.cmp(&b.window)));
        repos
    }

    /// Drop one window's state, killing its live sessions by dropping
    /// their connections. Returns true when the window existed.
    pub(crate) fn remove_window(&mut self, window: &str) -> bool {
        let existed = self.windows.remove(window).is_some();
        if existed {
            self.recompute_awake();
        }
        existed
    }

    /// Owner of a canonical repo path excluding the caller. Reads the
    /// filesystem: every stored root is canonicalized for comparison so
    /// symlinks and trailing slashes cannot bypass the single-owner gate.
    pub(crate) fn owner_of(&self, caller: &str, canonical: &str) -> Option<String> {
        for (label, window) in &self.windows {
            if label == caller {
                continue;
            }
            let Some(root) = window.repo_root.as_ref() else {
                continue;
            };
            if crate::repo::canonical_repo_path(&root.to_string_lossy()) == canonical {
                return Some(label.clone());
            }
        }
        None
    }
}

impl WindowState {
    /// Reset one window to empty, ready to claim a new repo. Drops live
    /// sessions by dropping their connections, the branch watcher, and all
    /// pending claims; the caller sets the new repo root next.
    fn reset(&mut self) {
        self.sessions.clear();
        self.current = None;
        self.pending_scoping.clear();
        self.worktrees.clear();
        self.clear_start_claims();
        self.branch_watch = None;
    }

    /// Working flag only; the global sleep guard recomputes from all
    /// windows via `BackendState::recompute_awake`.
    fn set_working(&mut self, key: &SessionKey, working: bool) {
        if let Some(session) = self.sessions.get_mut(key) {
            session.working = working;
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

/// Empty scoping session: no `plan.md`, no prompted flag (memory or
/// disk), and no live last prompt, whether or not the directory exists.
/// Takes `&WindowState` (called under an existing lock or a snapshot; never
/// locks itself).
pub(crate) fn is_empty_scoping(state: &WindowState, repo_root: &Path, name: &str) -> bool {
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
    if plans::load_state(&plan.path(repo_root)).prompted {
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

/// Discard an empty scoping session without a `cancelled/` trace, logging
/// the plan name with the trigger (`select`, `create`, `cancel`):
/// `remove_dir_all` when present (`NotFound` is fine, other errors are
/// logged per the preserve-evidence rule), plus the `LiveSession` and
/// `prompted` entry. Removes the single pending-map entry on match.
/// Caller owns the lock.
pub(crate) fn vanish_scoping(state: &mut WindowState, repo_root: &Path, name: &str, trigger: &str) {
    log::info!("vanishing empty scoping {name} on {trigger}");
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
    state: &mut WindowState,
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
    log::info!("keeping empty scoping {} on draft text", prev.plan);
    true
}

/// Abandon the previous scoping session when it is still empty: a
/// non-blank draft keeps it with its inferred title, otherwise it vanishes
/// with the trigger logged. Returns true when vanished. Caller owns the
/// lock.
pub(crate) fn abandon_empty_scoping(
    state: &mut WindowState,
    repo_root: &Path,
    prev: &SessionKey,
    draft: Option<String>,
    trigger: &str,
) -> bool {
    if prev.role != SessionRole::Scoping {
        return false;
    }
    if !is_empty_scoping(state, repo_root, &prev.plan) {
        return false;
    }
    if keep_or_vanish_empty(state, repo_root, prev, draft) {
        return false;
    }
    vanish_scoping(state, repo_root, &prev.plan, trigger);
    true
}

/// Scoping lead preview for one session. Returns the rendered template
/// only while the session is fresh under `is_empty_scoping`, `None` for
/// non-fresh, non-scoping, or gone plans. List payloads stay infallible,
/// so a gone plan yields no preview instead of an error. Reads the locked
/// state plus disk.
pub(crate) fn scoping_preview_for(
    state: &WindowState,
    repo_root: &Path,
    session: &SessionKey,
) -> Option<String> {
    if session.role != SessionRole::Scoping {
        return None;
    }
    let is_pending = state.pending_scoping.contains_key(&session.plan);
    let plan_ref = plans::PlanRef {
        name: session.plan.clone(),
        phase: plans::Phase::Scoping,
    };
    if !is_pending && !plan_ref.path(repo_root).is_dir() {
        return None;
    }
    if !is_empty_scoping(state, repo_root, &session.plan) {
        return None;
    }
    let display = opencode::plan_display(&plan_ref);
    Some(opencode::scoping_template(&display))
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

/// Live `plan.md` text for one session's plan, or `None` when the file is
/// absent. Locates the plan by name since the file travels with renames.
/// Pure file read.
pub(crate) fn plan_md_text_for(
    repo_root: &Path,
    session: &SessionKey,
) -> Result<Option<String>, String> {
    let Some(plan_dir) = plans::locate(repo_root, &session.plan) else {
        return Ok(None);
    };
    let path = plan_dir.join("plan.md");
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Ok(Some(text))
}

#[derive(Clone)]
pub struct AgentManager {
    state: Arc<Mutex<BackendState>>,
    app: AppHandle,
}

impl AgentManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            state: Arc::new(Mutex::new(BackendState::default())),
            app,
        }
    }

    /// Per-window read for one-liner snapshots. Short critical section,
    /// `None` when the window has since closed or the lock is poisoned.
    /// Multi-step transactions lock the backend directly instead.
    pub(crate) fn with_window<R>(
        &self,
        window: &str,
        f: impl FnOnce(&WindowState) -> R,
    ) -> Option<R> {
        let backend = lock_backend(&self.state)?;
        let state = backend.windows.get(window)?;
        Some(f(state))
    }

    /// Per-window mutation for one-liner updates. Never creates the entry:
    /// only `open_repo` creates windows, so `None` means the window closed
    /// (or the lock is poisoned) and the caller must not proceed.
    /// Recomputes the global sleep guard; never holds across await or I/O.
    /// Multi-step transactions lock the backend directly instead.
    pub(crate) fn with_window_mut<R>(
        &self,
        window: &str,
        f: impl FnOnce(&mut WindowState) -> R,
    ) -> Option<R> {
        let mut backend = lock_backend(&self.state)?;
        let state = backend.windows.get_mut(window)?;
        let result = f(state);
        backend.recompute_awake();
        Some(result)
    }

    /// Open a repository: ensure the plan structure, prune stale worktree
    /// metadata, re-register surviving worktrees, rescan every plan, and
    /// select the most-recent session. Lazy sessions still start on their
    /// first prompt, with transcripts and TODOs kept run-local; automatic
    /// plans that are already ready advance at once via the pump. With zero
    /// scoping dirs, reserves a pending session without touching disk.
    pub async fn open_repo(
        &self,
        window: &str,
        repo_root: PathBuf,
    ) -> Result<OpenRepoResult, AgentError> {
        plans::ensure_structure(&repo_root)?;
        crate::worktrees::prune(&repo_root)?;
        let canonical = crate::repo::canonical_repo_path(&repo_root.to_string_lossy());
        log::info!("open_repo window={window} path={canonical}");
        {
            let mut backend = lock_backend(&self.state).expect("state poisoned");
            // Same-repo-once gate: the canonical path is claimed under the
            // same lock that guards window state, so concurrent opens cannot
            // both pass. Another live owner keeps the repo; the caller stays
            // on the picker while the owner focuses.
            if let Some(owner) = backend.owner_of(window, &canonical) {
                log::info!("open_repo window={window} already open in {owner} path={canonical}");
                drop(backend);
                let _ = crate::windows::focus_window(&self.app, &owner);
                return Err(AgentError::AlreadyOpen {
                    path: canonical,
                    window: owner,
                });
            }
            let fresh = backend.fresh_generation();
            let state = backend.ensure_window(window);
            state.reset();
            state.generation = fresh;
            state.repo_root = Some(repo_root.clone());
            state.checkout_branch =
                crate::branch::current_branch(&repo_root).unwrap_or_else(|| "HEAD".to_string());
            // Crash recovery: surviving worktrees rejoin the map with
            // derived names; the landing target stays live.
            let scanned = plans::scan_plans(&repo_root);
            for plan in &scanned {
                if !matches!(
                    plan.phase,
                    plans::Phase::Executing | plans::Phase::Evergreening
                ) {
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
            backend.recompute_awake();
        }
        self.watch_branch(window, &repo_root);
        self.pump(window).await;
        let result = self.open_result(window);
        log::info!(
            "open_repo window={window} claimed path={canonical} selected={}::{:?}",
            result.selected.plan,
            result.selected.role
        );
        self.push_open_repos();
        Ok(result)
    }

    /// Reserve a fresh scoping session without touching disk. Reuses the
    /// pending session when the selection is still on an empty one with a
    /// blank draft; a non-blank draft keeps the old pending with its
    /// inferred title and reserves a new one. The start flow owns liveness:
    /// empty keys load history, non-empty keys warm, so this path spawns
    /// neither. `draft` is the unsent text of `state.current` at call time.
    pub async fn create_plan(
        &self,
        window: &str,
        draft: Option<String>,
    ) -> Result<PlansUpdate, AgentError> {
        let repo_root = self
            .current_repo(window)
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        {
            let backend = lock_backend(&self.state).expect("state poisoned");
            let state = backend.windows.get(window).expect("window poisoned");
            if let Some(current) = state.current.clone()
                && current.role == SessionRole::Scoping
                && state.pending_scoping.contains_key(&current.plan)
                && is_empty_scoping(state, &repo_root, &current.plan)
                && draft.as_deref().map(str::trim).unwrap_or("").is_empty()
            {
                drop(backend);
                return Ok(self.plans_update(window));
            }
        }
        match lock_backend(&self.state) {
            Some(mut backend) => {
                let state = backend.ensure_window(window);
                if let Some(prev) = state.current.clone() {
                    abandon_empty_scoping(state, &repo_root, &prev, draft, "create");
                }
                let name = reserve_scoping_name(&repo_root, &state.pending_scoping);
                state.pending_scoping.insert(name.clone(), None);
                state.current = Some(SessionKey {
                    plan: name,
                    role: SessionRole::Scoping,
                });
                backend.recompute_awake();
            }
            None => {
                log::warn!("failed to select new plan: state poisoned");
            }
        }
        Ok(self.plans_update(window))
    }

    /// Re-check the branch for the open repo. Failures keep the last value.
    /// A moved target re-resolves `ffable` and may unblock finishes or
    /// re-run a landing rebase, so a move pumps at once.
    pub async fn refresh_branch(&self, window: &str) -> Result<String, AgentError> {
        let repo_root = lock_backend(&self.state)
            .and_then(|backend| backend.windows.get(window)?.repo_root.clone())
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let branch =
            crate::branch::current_branch(&repo_root).unwrap_or_else(|| "HEAD".to_string());
        if self.set_branch(window, branch.clone()) {
            self.pump(window).await;
        }
        Ok(branch)
    }

    /// Set the held branch and emit it, then refresh the plans list so
    /// `ffable` recomputes against the new live target.
    /// Returns true when it moved.
    fn set_branch(&self, window: &str, branch: String) -> bool {
        let moved = match lock_backend(&self.state) {
            Some(mut backend) => {
                let Some(state) = backend.windows.get_mut(window) else {
                    return false;
                };
                if state.checkout_branch == branch {
                    false
                } else {
                    state.checkout_branch = branch.clone();
                    true
                }
            }
            None => {
                log::warn!("failed to record branch change: state poisoned");
                false
            }
        };
        if moved {
            emit_event(&self.app, window, AppEvent::BranchChanged { branch });
            self.push_plans(window);
        }
        moved
    }

    /// Watch `.git/HEAD` for the open repo, refreshing
    /// `state.checkout_branch` per event so execute/retry snapshots go
    /// fresh free. Branch moves also push a refreshed plans list so
    /// `ffable` recomputes against the new live target, then pump so a
    /// moved target unblocks finishes or re-runs a landing rebase.
    fn watch_branch(&self, window: &str, repo_root: &Path) {
        let root = repo_root.to_path_buf();
        let label = window.to_string();
        let state = Arc::clone(&self.state);
        let app = self.app.clone();
        let push_state = Arc::clone(&self.state);
        let push_app = self.app.clone();
        let pump_manager = self.clone();
        let pump_label = label.clone();
        let watcher = crate::branch::watch_branch(root.clone(), move |branch| {
            let moved = match lock_backend(&state) {
                Some(mut backend) => {
                    let Some(guard) = backend.windows.get_mut(&label) else {
                        return;
                    };
                    if guard.repo_root.as_ref() != Some(&root) || guard.checkout_branch == branch {
                        false
                    } else {
                        guard.checkout_branch = branch.clone();
                        true
                    }
                }
                None => {
                    log::warn!("failed to record branch change: state poisoned");
                    false
                }
            };
            if moved {
                emit_event(&app, &label, AppEvent::BranchChanged { branch });
                plans_list::push_sorted(&push_state, &push_app, &label);
                pump_manager.spawn_pump(&pump_label);
            }
        });
        match lock_backend(&self.state) {
            Some(mut backend) => {
                backend.ensure_window(window).branch_watch = watcher;
            }
            None => {
                log::warn!("failed to hold branch watcher: state poisoned");
            }
        }
    }

    /// Approve the scoping plan: move it to executing and start the
    /// executing agent. Thin over the shared transition core; the pump
    /// drives the same core automatically.
    pub async fn execute_plan(
        &self,
        window: &str,
        session: SessionKey,
    ) -> Result<PlansUpdate, AgentError> {
        if session.role != SessionRole::Scoping {
            return Err(AgentError::RequestFailed {
                raw: "no scoping plan to execute".to_string(),
            });
        }
        self.transition(window, advance::Transition::Execute { plan: session.plan })
            .await
    }

    /// Finish a plan via the shared transition core: a clean
    /// fast-forwardable executing or evergreening plan completes directly,
    /// a clean landing plan finishes the same way once it lands.
    pub async fn finish_landing(
        &self,
        window: &str,
        session: SessionKey,
    ) -> Result<PlansUpdate, AgentError> {
        let action = match session.role {
            SessionRole::Executing => advance::Transition::FinishExecuting { plan: session.plan },
            SessionRole::Evergreening => {
                advance::Transition::FinishEvergreening { plan: session.plan }
            }
            SessionRole::Landing => advance::Transition::FinishLanding { plan: session.plan },
            SessionRole::Scoping => {
                return Err(AgentError::RequestFailed {
                    raw: "no executing plan to complete".to_string(),
                });
            }
        };
        self.transition(window, action).await
    }

    /// Move a clean diverged executing or evergreening plan into the landing
    /// queue via the shared core. Move-only: explicit commands never spawn;
    /// the pump starts the head. Thin over `transition`.
    pub async fn begin_landing(
        &self,
        window: &str,
        session: SessionKey,
    ) -> Result<PlansUpdate, AgentError> {
        if !matches!(
            session.role,
            SessionRole::Executing | SessionRole::Evergreening
        ) {
            return Err(AgentError::RequestFailed {
                raw: "no executing or evergreening plan to land".to_string(),
            });
        }
        self.transition(
            window,
            advance::Transition::BeginLanding { plan: session.plan },
        )
        .await
    }

    /// Cancel an active plan. An empty scoping session vanishes without a
    /// `cancelled/` trace; otherwise the plan moves to `cancelled/`,
    /// force-removing the worktree and branch for executing and landing
    /// plans. The selection follows only when the cancelled plan is viewed,
    /// landing on the cancelled (now read-only) session.
    pub async fn cancel_plan(
        &self,
        window: &str,
        session: SessionKey,
    ) -> Result<PlansUpdate, AgentError> {
        ensure_idle(&self.state, window, &session)?;
        let (repo_root, _) = self
            .reopen_snapshot(window)
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        if session.role == SessionRole::Scoping {
            let vanished = match lock_backend(&self.state) {
                Some(mut backend) => {
                    let state = backend.ensure_window(window);
                    if abandon_empty_scoping(state, &repo_root, &session, None, "cancel") {
                        if plans_list::is_viewed(state.current.as_ref(), &session.plan) {
                            let checkout_branch = state.checkout_branch.clone();
                            let plans = plans_list::sorted_entries(
                                &repo_root,
                                &checkout_branch,
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
                        backend.recompute_awake();
                        true
                    } else {
                        backend.recompute_awake();
                        false
                    }
                }
                None => {
                    log::warn!("failed to check empty session: state poisoned");
                    false
                }
            };
            if vanished {
                return Ok(self.plans_update(window));
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
        if matches!(
            phase,
            plans::Phase::Executing | plans::Phase::Evergreening | plans::Phase::Landing
        ) {
            self.remove_worktree(window, &repo_root, &from.name, true)?;
        }
        let next = plans::cancel(&repo_root, &from)?;
        if phase == plans::Phase::Scoping {
            self.drop_live(window, &session).await;
        } else {
            self.drop_plan_lives(window, &from.name).await;
        }
        self.carry_prompted(window, &from.name, &next.name);
        self.select_if_viewed(
            window,
            &session.plan,
            SessionKey {
                plan: next.name,
                role: session.role,
            },
        );
        Ok(self.plans_update(window))
    }

    /// Flip one plan between automatic and manual mode. Persists `manual`
    /// into `state.json` and returns the refreshed list. Never touches live
    /// sessions: flipping modes never kills a running turn. Flipping back
    /// to A pumps at once so a held plan joins the flow immediately.
    pub async fn set_plan_mode(
        &self,
        window: &str,
        plan: String,
        manual: bool,
    ) -> Result<PlansUpdate, AgentError> {
        self.update_plan_state(window, plan, |state| state.set_manual(manual))
            .await
    }

    /// Flip one plan's evergreen intent. Persists `evergreen` into
    /// `state.json` and returns the refreshed list. Never touches live
    /// sessions. Pumps only when the plan is on auto, so flipping intent
    /// while paused never fires: the held plan joins the flow on resume.
    pub async fn set_plan_evergreen(
        &self,
        window: &str,
        plan: String,
        evergreen: bool,
    ) -> Result<PlansUpdate, AgentError> {
        self.update_plan_state(window, plan, |state| state.set_evergreen(evergreen))
            .await
    }

    /// Shared plan-state mutator for the mode and evergreen setters: locate
    /// plus load plus mutate plus store, then a pump when the stored plan
    /// is on auto, then the refreshed list.
    async fn update_plan_state(
        &self,
        window: &str,
        plan: String,
        mutate: impl FnOnce(&mut plans::PlanState) -> bool,
    ) -> Result<PlansUpdate, AgentError> {
        let (repo_root, _) = self
            .reopen_snapshot(window)
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        let Some(plan_dir) = plans::locate(&repo_root, &plan) else {
            return Err(AgentError::NoSession {
                raw: "plan is gone".to_string(),
            });
        };
        let mut state = plans::load_state(&plan_dir);
        if mutate(&mut state) {
            plans::store_state(&plan_dir, &state);
        }
        if !state.manual {
            self.pump(window).await;
        }
        Ok(self.plans_update(window))
    }

    /// Select one session, keeping a previously-selected empty scoping
    /// session when its draft holds text (with its inferred title) and
    /// discarding it the same way as cancel otherwise. Returns the target
    /// selection. `draft` is the unsent text of `state.current` at call
    /// time. No `ensure_idle` gate: selection is allowed anytime, and an
    /// empty previous session is never working.
    pub async fn select_plan(
        &self,
        window: &str,
        session: SessionKey,
        draft: Option<String>,
    ) -> Result<PlansUpdate, AgentError> {
        let (repo_root, _) = self
            .reopen_snapshot(window)
            .ok_or_else(|| AgentError::NoSession {
                raw: "open a repository first".to_string(),
            })?;
        {
            let backend = lock_backend(&self.state).expect("state poisoned");
            let state = backend.windows.get(window).expect("window gone");
            // Name-based across all phases: finished plans live under
            // `completed/` or `cancelled/` and stay selectable read-only.
            // Only names missing everywhere report "plan is gone".
            let target_exists = (session.role == SessionRole::Scoping
                && state.pending_scoping.contains_key(&session.plan))
                || plans::locate(&repo_root, &session.plan).is_some();
            if !target_exists {
                return Err(AgentError::NoSession {
                    raw: "plan is gone".to_string(),
                });
            }
        }
        match lock_backend(&self.state) {
            Some(mut backend) => {
                let state = backend.ensure_window(window);
                if let Some(prev) = state.current.clone()
                    && prev != session
                {
                    abandon_empty_scoping(state, &repo_root, &prev, draft, "select");
                }
                state.current = Some(session);
                backend.recompute_awake();
            }
            None => {
                log::warn!("failed to select session: state poisoned");
            }
        }
        Ok(self.plans_update(window))
    }

    /// Live `plan.md` text for one session's plan, or `None` when the file
    /// does not exist yet (fresh scoping, pending, or bare). Searches every
    /// phase by name since the file travels with renames; read-only, never
    /// creates directories. Pure file read except the repo-root snapshot.
    pub fn plan_md_text(
        &self,
        window: &str,
        session: SessionKey,
    ) -> Result<Option<String>, AgentError> {
        let Some(repo_root) = lock_backend(&self.state)
            .and_then(|backend| backend.windows.get(window)?.repo_root.clone())
        else {
            return Err(AgentError::NoSession {
                raw: "open a repository first".to_string(),
            });
        };
        plan_md_text_for(&repo_root, &session).map_err(|raw| AgentError::RequestFailed { raw })
    }

    /// Live worktree coordinates plus landing state for one plan. The
    /// target always resolves live from `checkout_branch` at call time, so
    /// a branch switch mid-flight retargets in-flight plans.
    fn worktree_status_for(
        &self,
        window: &str,
        repo_root: &Path,
        plan_name: &str,
    ) -> Result<WorktreeLive, AgentError> {
        let (record, target_branch) = match lock_backend(&self.state) {
            Some(backend) => match backend.windows.get(window) {
                Some(state) => (
                    state.worktrees.get(plan_name).cloned(),
                    state.checkout_branch.clone(),
                ),
                None => (None, String::new()),
            },
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
        let commits = crate::worktrees::commits(repo_root, &target_branch, &worktree_branch)?;
        Ok(WorktreeLive {
            path,
            worktree_branch,
            target_branch,
            dirty,
            ffable,
            commits,
        })
    }

    /// Drop every live session a plan owns. Finishes and cancels land on
    /// history rows; the transcripts stay frontend-side.
    async fn drop_plan_lives(&self, window: &str, plan_name: &str) {
        for role in [
            SessionRole::Executing,
            SessionRole::Evergreening,
            SessionRole::Landing,
        ] {
            self.drop_live(
                window,
                &SessionKey {
                    plan: plan_name.to_string(),
                    role,
                },
            )
            .await;
        }
    }

    /// Delete one plan worktree and its branch, forgetting the map entry.
    /// Force only on explicit user action; finishes run clean-gated
    /// without force. A missing map entry falls back to the derived path
    /// and branch so recovered plans still clean up.
    fn remove_worktree(
        &self,
        window: &str,
        repo_root: &Path,
        plan_name: &str,
        force: bool,
    ) -> Result<(), AgentError> {
        let record = lock_backend(&self.state).and_then(|backend| {
            backend
                .windows
                .get(window)?
                .worktrees
                .get(plan_name)
                .cloned()
        });
        let (path, worktree_branch) = match record {
            Some(record) => (record.path, record.worktree_branch),
            None => (
                crate::worktrees::worktree_path(repo_root, plan_name),
                crate::worktrees::branch_name(plan_name),
            ),
        };
        crate::worktrees::remove(repo_root, &path, &worktree_branch, force)?;
        if let Some(mut backend) = lock_backend(&self.state)
            && let Some(state) = backend.windows.get_mut(window)
        {
            state.worktrees.remove(plan_name);
        }
        Ok(())
    }

    /// Announce an eager executing/landing first prompt as a live YOU
    /// bubble, matching what history replay shows. Shared so the two
    /// call sites cannot drift apart; retries never call here.
    fn announce_first_prompt(&self, window: &str, key: &SessionKey, text: &str) {
        emit_event(
            &self.app,
            window,
            AppEvent::UserText {
                session: key.clone(),
                chunk: text.to_string(),
            },
        );
    }

    /// Repos owned across windows, sorted by path.
    pub fn list_open_repos(&self) -> Vec<crate::types::OpenRepoEntry> {
        lock_backend(&self.state)
            .map(|backend| backend.open_repos())
            .unwrap_or_default()
    }

    /// Drop one window's state, killing its live sessions. Broadcasts the
    /// refreshed owned-repos list when the window existed.
    pub fn remove_window(&self, window: &str) {
        let (existed, repos) = match lock_backend(&self.state) {
            Some(mut backend) => {
                let existed = backend.remove_window(window);
                let repos = backend.open_repos();
                (existed, repos)
            }
            None => (false, Vec::new()),
        };
        log::info!("remove_window window={window} existed={existed}");
        if existed {
            emit_open_repos(&self.app, repos);
        }
    }

    /// Broadcast the current owned-repos list to every window.
    fn push_open_repos(&self) {
        let repos = self.list_open_repos();
        emit_open_repos(&self.app, repos);
    }
}

/// Live worktree coordinates plus landing state for one plan. `commits`
/// lists the worktree branch commits on top of the target, oldest first;
/// empty means nothing changed.
pub(crate) struct WorktreeLive {
    pub path: PathBuf,
    pub worktree_branch: String,
    pub target_branch: String,
    pub dirty: bool,
    pub ffable: bool,
    pub commits: Vec<String>,
}

/// Which landing step the user asked for. `Begin` moves a diverged work
/// plan into the queue; `Start` spawns the head's agent fresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LandingStep {
    Begin,
    Start,
    Finish,
}

/// Refuse a landing step that cannot run. A dirty worktree never lands;
/// an already fast-forwardable branch finishes directly instead of
/// moving or starting landing; a diverged branch moves and starts before
/// finishing. Pure.
fn gate_landing(dirty: bool, ffable: bool, step: LandingStep) -> Result<(), AgentError> {
    if dirty {
        return Err(AgentError::RequestFailed {
            raw: "commit or discard worktree changes first".to_string(),
        });
    }
    match step {
        LandingStep::Begin | LandingStep::Start => {
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
/// only logs, matching `lock_backend`.
fn ensure_idle(
    state: &Mutex<BackendState>,
    window: &str,
    key: &SessionKey,
) -> Result<(), AgentError> {
    match state.lock() {
        Ok(backend) => {
            let working = backend
                .windows
                .get(window)
                .map(|win| win.is_working(key))
                .unwrap_or(false);
            if working {
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

/// Capture the window incarnation for an async task: generation plus repo.
/// The task drops its emits when `is_current` later fails, so a reused
/// label never shows another repo's transcript.
pub(crate) fn incarnation(
    state: &Mutex<BackendState>,
    window: &str,
) -> Option<(u64, Option<PathBuf>)> {
    lock_backend(state).and_then(|backend| {
        backend
            .windows
            .get(window)
            .map(|win| (win.generation, win.repo_root.clone()))
    })
}

/// True when the window still owns the spawn-time incarnation and repo.
/// A moved window logs the drop at warn so the file proves containment.
pub(crate) fn check_current(
    state: &Mutex<BackendState>,
    window: &str,
    generation: u64,
    repo: &Option<PathBuf>,
) -> bool {
    match lock_backend(state) {
        Some(backend) => {
            if backend.is_current(window, generation, repo) {
                true
            } else {
                log::warn!("dropping stale emit window={window} generation={generation}");
                false
            }
        }
        None => false,
    }
}

/// Emit one app event to its owning window. Emissions are fire-and-forget,
/// but a failure desyncs the UI from the agent, so it is always logged.
/// Every emission logs its window label plus session key or selection so
/// the log file alone proves which window each event targeted.
fn emit_event(app: &AppHandle, window: &str, event: AppEvent) {
    let summary = crate::types::event_summary(&event);
    if let Err(error) = app.emit_to(window, "samokod://event", event) {
        log::warn!("failed to emit window={window} {summary}: {error}");
    } else {
        log::info!("emit window={window} {summary}");
    }
}

/// Broadcast the owned-repos list to every window so pickers refresh
/// their already-open marks. Fire-and-forget with a warn-log.
fn emit_open_repos(app: &AppHandle, repos: Vec<crate::types::OpenRepoEntry>) {
    let count = repos.len();
    if let Err(error) = app.emit("samokod://event", AppEvent::OpenReposChanged { repos }) {
        log::warn!("failed to emit open repos count={count}: {error}");
    } else {
        log::info!("emit broadcast open_repos_changed count={count}");
    }
}

/// Lock backend state. A poisoned mutex means a prior panic elsewhere; log
/// it instead of silently dropping the update.
fn lock_backend(state: &Mutex<BackendState>) -> Option<std::sync::MutexGuard<'_, BackendState>> {
    match state.lock() {
        Ok(guard) => Some(guard),
        Err(error) => {
            log::warn!("agent state lock poisoned: {error}");
            None
        }
    }
}

fn set_working(state: &Mutex<BackendState>, window: &str, key: &SessionKey, working: bool) {
    let Some(mut backend) = lock_backend(state) else {
        return;
    };
    let Some(win) = backend.windows.get_mut(window) else {
        return;
    };
    win.set_working(key, working);
    backend.recompute_awake();
}

fn set_failed(state: &Mutex<BackendState>, window: &str, key: &SessionKey, failed: bool) {
    let Some(mut backend) = lock_backend(state) else {
        return;
    };
    if let Some(win) = backend.windows.get_mut(window)
        && let Some(session) = win.sessions.get_mut(key)
    {
        session.failed = failed;
    }
}

fn set_approval(state: &Mutex<BackendState>, window: &str, key: &SessionKey, approval: bool) {
    let Some(mut backend) = lock_backend(state) else {
        return;
    };
    if let Some(win) = backend.windows.get_mut(window)
        && let Some(session) = win.sessions.get_mut(key)
    {
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

    fn state_with_session() -> Mutex<BackendState> {
        let mut backend = BackendState::default();
        let key = test_key();
        let win = backend.ensure_window("main");
        win.sessions.insert(
            key.clone(),
            LiveSession::fresh(
                ActivePlan::scoping("2026-09-25.10-54-59".to_string()),
                crate::repo_state::RepoState::default(),
            ),
        );
        win.current = Some(key);
        Mutex::new(backend)
    }

    #[test]
    fn working_releases_awake_guard() {
        let state = state_with_session();
        let key = test_key();
        set_working(&state, "main", &key, true);
        {
            let guard = state.lock().expect("state poisoned");
            let win = guard.windows.get("main").expect("window");
            assert!(win.sessions.get(&key).expect("session").working);
        }
        set_working(&state, "main", &key, false);
        {
            let guard = state.lock().expect("state poisoned");
            let win = guard.windows.get("main").expect("window");
            assert!(!win.sessions.get(&key).expect("session").working);
            assert!(guard.awake.is_none());
        }
    }

    #[test]
    fn working_holds_guard_while_any_session_runs() {
        let state = Mutex::new(BackendState::default());
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
            let win = guard.ensure_window("main");
            win.sessions.insert(
                first.clone(),
                LiveSession::fresh(
                    ActivePlan::scoping("a".to_string()),
                    crate::repo_state::RepoState::default(),
                ),
            );
            win.sessions.insert(
                second.clone(),
                LiveSession::fresh(
                    ActivePlan::scoping("b".to_string()),
                    crate::repo_state::RepoState::default(),
                ),
            );
        }
        set_working(&state, "main", &first, true);
        set_working(&state, "main", &second, true);
        set_working(&state, "main", &first, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.awake.is_some());
        }
        set_working(&state, "main", &second, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.awake.is_none());
        }
    }

    #[test]
    fn failed_flag_tracks_turn_outcome() {
        let state = state_with_session();
        let key = test_key();
        set_failed(&state, "main", &key, true);
        {
            let guard = state.lock().expect("state poisoned");
            let win = guard.windows.get("main").expect("window");
            assert!(win.sessions.get(&key).expect("session").failed);
        }
        set_failed(&state, "main", &key, false);
        {
            let guard = state.lock().expect("state poisoned");
            let win = guard.windows.get("main").expect("window");
            assert!(!win.sessions.get(&key).expect("session").failed);
        }
    }

    #[test]
    fn idle_gate_is_per_session() {
        let state = Mutex::new(BackendState::default());
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
            let win = guard.ensure_window("main");
            for (key, name) in [(&first, "a"), (&second, "b")] {
                win.sessions.insert(
                    key.clone(),
                    LiveSession::fresh(
                        ActivePlan::scoping(name.to_string()),
                        crate::repo_state::RepoState::default(),
                    ),
                );
            }
        }
        set_working(&state, "main", &first, true);
        assert!(ensure_idle(&state, "main", &first).is_err());
        assert!(ensure_idle(&state, "main", &second).is_ok());
    }

    #[test]
    fn windows_hold_separate_repos_and_selections() {
        let state = Mutex::new(BackendState::default());
        {
            let mut backend = state.lock().expect("state poisoned");
            let a = backend.ensure_window("a");
            a.repo_root = Some(PathBuf::from("/repo-a"));
            a.current = Some(SessionKey {
                plan: "plan-a".to_string(),
                role: SessionRole::Scoping,
            });
            a.pending_scoping
                .insert("plan-a".to_string(), Some("A".to_string()));
            let b = backend.ensure_window("b");
            b.repo_root = Some(PathBuf::from("/repo-b"));
            b.current = Some(SessionKey {
                plan: "plan-b".to_string(),
                role: SessionRole::Scoping,
            });
            b.pending_scoping.insert("plan-b".to_string(), None);
        }
        let (repo_a, repo_b) = {
            let backend = state.lock().expect("state poisoned");
            (
                backend.windows.get("a").expect("a").repo_root.clone(),
                backend.windows.get("b").expect("b").repo_root.clone(),
            )
        };
        assert_eq!(repo_a, Some(PathBuf::from("/repo-a")));
        assert_eq!(repo_b, Some(PathBuf::from("/repo-b")));
        // Clearing B must not clear A.
        {
            let mut backend = state.lock().expect("state poisoned");
            backend.windows.get_mut("b").expect("b").reset();
            backend.recompute_awake();
        }
        {
            let backend = state.lock().expect("state poisoned");
            let a = backend.windows.get("a").expect("a");
            assert_eq!(a.repo_root, Some(PathBuf::from("/repo-a")));
            assert!(a.pending_scoping.contains_key("plan-a"));
            let b = backend.windows.get("b").expect("b");
            assert!(b.pending_scoping.is_empty());
        }
    }

    #[test]
    fn identical_plan_names_stay_in_their_own_window() {
        let state = Mutex::new(BackendState::default());
        let key = SessionKey {
            plan: "2026-10-21.10-03-18".to_string(),
            role: SessionRole::Scoping,
        };
        let (gen_a, repo_a, gen_b, repo_b) = {
            let mut backend = state.lock().expect("state poisoned");
            let a = backend.ensure_window("window-1");
            a.repo_root = Some(PathBuf::from("/repo-a"));
            a.current = Some(key.clone());
            let gen_a = a.generation;
            let repo_a = a.repo_root.clone();
            let b = backend.ensure_window("window-2");
            b.repo_root = Some(PathBuf::from("/repo-b"));
            b.current = Some(key.clone());
            let gen_b = b.generation;
            let repo_b = b.repo_root.clone();
            (gen_a, repo_a, gen_b, repo_b)
        };
        // Same plan name, different repos and generations: each window is
        // current only for its own incarnation.
        {
            let backend = state.lock().expect("state poisoned");
            assert!(backend.is_current("window-1", gen_a, &repo_a));
            assert!(!backend.is_current("window-1", gen_b, &repo_b));
            assert!(backend.is_current("window-2", gen_b, &repo_b));
            assert!(!backend.is_current("window-2", gen_a, &repo_a));
            // Selections stay separate even with identical names.
            assert_eq!(
                backend.windows.get("window-1").expect("a").current,
                Some(key.clone())
            );
            assert_eq!(
                backend.windows.get("window-2").expect("b").current,
                Some(key)
            );
        }
    }

    #[test]
    fn stale_turn_after_label_reuse_is_dropped() {
        let state = Mutex::new(BackendState::default());
        // Window-1 opens repo-a; capture the turn's incarnation.
        let (old_generation, old_repo) = {
            let mut backend = state.lock().expect("state poisoned");
            let fresh = backend.fresh_generation();
            let win = backend.ensure_window("window-1");
            win.reset();
            win.generation = fresh;
            win.repo_root = Some(PathBuf::from("/repo-a"));
            (win.generation, win.repo_root.clone())
        };
        // Window-1 closes; a new repo reopens under the same label.
        {
            let mut backend = state.lock().expect("state poisoned");
            assert!(backend.remove_window("window-1"));
            let fresh = backend.fresh_generation();
            let win = backend.ensure_window("window-1");
            win.reset();
            win.generation = fresh;
            win.repo_root = Some(PathBuf::from("/repo-b"));
        }
        // The old turn's incarnation no longer matches: its emit drops.
        {
            let backend = state.lock().expect("state poisoned");
            assert!(!backend.is_current("window-1", old_generation, &old_repo));
            let current = backend.windows.get("window-1").expect("reopened");
            assert_eq!(current.repo_root, Some(PathBuf::from("/repo-b")));
            assert!(backend.is_current("window-1", current.generation, &current.repo_root.clone()));
        }
        assert!(!super::check_current(
            &state,
            "window-1",
            old_generation,
            &old_repo
        ));
    }

    #[test]
    fn warm_and_history_claims_never_resurrect_closed_windows() {
        use super::start::{HistoryGuard, WarmGuard};
        use std::sync::Arc;
        let state = Arc::new(Mutex::new(BackendState::default()));
        let key = SessionKey {
            plan: "2026-10-21.10-03-18".to_string(),
            role: SessionRole::Scoping,
        };
        // No window yet: claims fail and must not create the entry.
        assert!(WarmGuard::claim(&state, "gone", &key).is_none());
        assert!(HistoryGuard::claim(&state, "gone", &key).is_none());
        {
            let backend = state.lock().expect("state poisoned");
            assert!(!backend.windows.contains_key("gone"));
        }
        // Live window: claims succeed once (single-flight).
        {
            let mut backend = state.lock().expect("state poisoned");
            backend.ensure_window("live");
        }
        let warm = WarmGuard::claim(&state, "live", &key).expect("warm claims");
        assert!(warm.is_held());
        drop(warm);
        let history = HistoryGuard::claim(&state, "live", &key).expect("history claims");
        assert!(history.is_held());
        history.finish();
    }

    #[test]
    fn awake_covers_working_in_any_window() {
        let state = Mutex::new(BackendState::default());
        let key_a = SessionKey {
            plan: "a".to_string(),
            role: SessionRole::Scoping,
        };
        let key_b = SessionKey {
            plan: "b".to_string(),
            role: SessionRole::Scoping,
        };
        {
            let mut backend = state.lock().expect("state poisoned");
            for (label, key) in [("a", &key_a), ("b", &key_b)] {
                backend.ensure_window(label).sessions.insert(
                    key.clone(),
                    LiveSession::fresh(
                        ActivePlan::scoping(key.plan.clone()),
                        crate::repo_state::RepoState::default(),
                    ),
                );
            }
        }
        set_working(&state, "b", &key_b, true);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.awake.is_some());
        }
        set_working(&state, "b", &key_b, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.awake.is_none());
        }
        set_working(&state, "a", &key_a, true);
        set_working(&state, "b", &key_b, true);
        set_working(&state, "a", &key_a, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.awake.is_some());
        }
        set_working(&state, "b", &key_b, false);
        {
            let guard = state.lock().expect("state poisoned");
            assert!(guard.awake.is_none());
        }
    }

    #[test]
    fn close_cleanup_drops_window_and_releases_awake() {
        let backend = Mutex::new(BackendState::default());
        let key = SessionKey {
            plan: "a".to_string(),
            role: SessionRole::Scoping,
        };
        {
            let mut guard = backend.lock().expect("state poisoned");
            guard.ensure_window("window-1").sessions.insert(
                key.clone(),
                LiveSession::fresh(
                    ActivePlan::scoping("a".to_string()),
                    crate::repo_state::RepoState::default(),
                ),
            );
            guard.ensure_window("window-1").repo_root = Some(PathBuf::from("/repo"));
        }
        set_working(&backend, "window-1", &key, true);
        assert!(backend.lock().expect("state poisoned").awake.is_some());
        {
            let mut guard = backend.lock().expect("state poisoned");
            assert!(guard.remove_window("window-1"));
            assert!(!guard.windows.contains_key("window-1"));
        }
        {
            let guard = backend.lock().expect("state poisoned");
            assert!(guard.awake.is_none());
            assert!(guard.open_repos().is_empty());
        }
        {
            let mut guard = backend.lock().expect("state poisoned");
            assert!(!guard.remove_window("window-9"));
        }
    }

    #[test]
    fn open_repos_lists_sorted_owners() {
        let mut backend = BackendState::default();
        backend.ensure_window("window-2").repo_root = Some(PathBuf::from("/repo-b"));
        backend.ensure_window("window-1").repo_root = Some(PathBuf::from("/repo-a"));
        backend.ensure_window("empty");
        let repos = backend.open_repos();
        assert_eq!(
            repos,
            vec![
                crate::types::OpenRepoEntry {
                    path: "/repo-a".to_string(),
                    window: "window-1".to_string(),
                },
                crate::types::OpenRepoEntry {
                    path: "/repo-b".to_string(),
                    window: "window-2".to_string(),
                },
            ]
        );
    }

    #[test]
    fn double_open_finds_live_owner() {
        let mut backend = BackendState::default();
        backend.ensure_window("window-1").repo_root = Some(PathBuf::from("/repo"));
        assert_eq!(
            backend.owner_of("window-2", "/repo"),
            Some("window-1".to_string())
        );
        // Same window never owns against itself.
        assert_eq!(backend.owner_of("window-1", "/repo"), None);
        // Empty windows own nothing.
        assert_eq!(
            backend.owner_of("window-3", "/repo"),
            Some("window-1".to_string())
        );
    }

    #[test]
    fn reopen_after_close_releases_owner() {
        let mut backend = BackendState::default();
        backend.ensure_window("window-1").repo_root = Some(PathBuf::from("/repo"));
        assert!(backend.remove_window("window-1"));
        assert_eq!(backend.owner_of("window-2", "/repo"), None);
        backend.ensure_window("window-2").repo_root = Some(PathBuf::from("/repo"));
        assert_eq!(
            backend.owner_of("window-1", "/repo"),
            Some("window-2".to_string())
        );
    }

    #[test]
    fn owner_check_normalizes_trailing_slashes() {
        let mut backend = BackendState::default();
        backend.ensure_window("window-1").repo_root = Some(PathBuf::from("/repo"));
        let canonical = crate::repo::canonical_repo_path("/repo/");
        assert_eq!(
            backend.owner_of("window-2", &canonical),
            Some("window-1".to_string())
        );
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
        write_plan_dir(root, plans::Phase::Evergreening, "g", Some("G"));
        write_plan_dir(root, plans::Phase::Executing, "a", Some("A"));
        write_plan_dir(root, plans::Phase::Scoping, "s", Some("S"));
        let entries = sorted_entries(root, "", &HashMap::new(), &HashMap::new());
        let phases: Vec<plans::Phase> = entries.iter().map(|entry| entry.phase).collect();
        assert_eq!(
            phases,
            vec![
                plans::Phase::Scoping,
                plans::Phase::Executing,
                plans::Phase::Evergreening,
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
        let entries = sorted_entries(root, "", &HashMap::new(), &HashMap::new());
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
        let entries = sorted_entries(root, "", &HashMap::new(), &HashMap::new());
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
        let entries = sorted_entries(root, "", &HashMap::new(), &HashMap::new());
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
        let entries = sorted_entries(root, "", &HashMap::new(), &HashMap::new());
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
        let entries = sorted_entries(root, "", &HashMap::new(), &HashMap::new());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sessions.len(), 4);
        assert_eq!(
            most_recent_key(&entries),
            Some(SessionKey {
                plan: "m".to_string(),
                role: SessionRole::Landing,
            })
        );
    }

    #[test]
    fn most_recent_prefers_evergreening_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        write_plan_dir(root, plans::Phase::Evergreening, "g", Some("G"));
        let entries = sorted_entries(root, "", &HashMap::new(), &HashMap::new());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sessions.len(), 3);
        assert_eq!(
            most_recent_key(&entries),
            Some(SessionKey {
                plan: "g".to_string(),
                role: SessionRole::Evergreening,
            })
        );
    }

    #[test]
    fn landing_gate_refuses_dirty_and_misplaced_steps() {
        assert!(gate_landing(false, true, LandingStep::Finish).is_ok());
        assert!(gate_landing(false, false, LandingStep::Begin).is_ok());
        assert!(gate_landing(false, false, LandingStep::Start).is_ok());
        assert!(gate_landing(true, true, LandingStep::Finish).is_err());
        assert!(gate_landing(true, false, LandingStep::Begin).is_err());
        assert!(gate_landing(true, false, LandingStep::Start).is_err());
        assert!(gate_landing(false, false, LandingStep::Finish).is_err());
        assert!(gate_landing(false, true, LandingStep::Begin).is_err());
        assert!(gate_landing(false, true, LandingStep::Start).is_err());
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
        let entries = sorted_entries(root, "", &sessions, &HashMap::new());
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
        let entries = sorted_entries(root, "", &HashMap::new(), &HashMap::new());
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
        let entries = sorted_entries(root, "", &HashMap::new(), &pending);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, name);
        assert_eq!(entries[0].title, "Untitled");
        assert!(!entries[0].has_plan_md);
        assert_eq!(entries[0].sessions.len(), 1);
        assert_eq!(entries[0].sessions[0].role, SessionRole::Scoping);
        assert!(!entries[0].manual);
        assert!(entries[0].evergreen);
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
        let entries = sorted_entries(root, "", &HashMap::new(), &pending);
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
        let entries = sorted_entries(root, "", &HashMap::new(), &pending);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "Login flow fixes");
    }

    #[test]
    fn entries_reflect_stored_flags() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let plan = plans::materialize_scoping(root, "2026-09-26.08-41-03").expect("materialize");
        let mut stored = plans::load_state(&plan.path(root));
        stored.manual = true;
        stored.evergreen = false;
        plans::store_state(&plan.path(root), &stored);
        let entries = sorted_entries(root, "", &HashMap::new(), &HashMap::new());
        assert!(entries[0].manual);
        assert!(!entries[0].evergreen);
        let mut stored = plans::load_state(&plan.path(root));
        stored.manual = false;
        stored.evergreen = true;
        plans::store_state(&plan.path(root), &stored);
        let entries = sorted_entries(root, "", &HashMap::new(), &HashMap::new());
        assert!(!entries[0].manual);
        assert!(entries[0].evergreen);
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
        let mut state = WindowState::default();
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
        let state = WindowState::default();
        plans::materialize_scoping(root, "2026-09-26.08-41-04").expect("materialize");
        std::fs::write(
            root.join(".samokod/plans/scoping/2026-09-26.08-41-04/plan.md"),
            "# T\n",
        )
        .expect("write");
        assert!(!is_empty_scoping(&state, root, "2026-09-26.08-41-04"));
    }

    #[test]
    fn prompted_flag_writes_to_state_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        plans::materialize_scoping(root, name).expect("materialize");
        let plan = plans::PlanRef {
            name: name.to_string(),
            phase: plans::Phase::Scoping,
        };
        assert!(!plans::load_state(&plan.path(root)).prompted);
        session_ids::mark_prompted(root, name);
        assert!(plans::load_state(&plan.path(root)).prompted);
    }

    #[test]
    fn empty_gate_survives_restart_via_persisted_flag() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        plans::materialize_scoping(root, name).expect("materialize");
        session_ids::mark_prompted(root, name);
        let state = WindowState::default();
        assert!(!is_empty_scoping(&state, root, name));
    }

    #[test]
    fn abandon_vanishes_empty_without_draft() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let mut state = WindowState {
            pending_scoping: HashMap::from([(name.to_string(), None)]),
            ..Default::default()
        };
        let prev = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        assert!(abandon_empty_scoping(&mut state, root, &prev, None, "test"));
        assert!(!state.pending_scoping.contains_key(name));
    }

    #[test]
    fn abandon_keeps_empty_with_draft() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let mut state = WindowState {
            pending_scoping: HashMap::from([(name.to_string(), None)]),
            ..Default::default()
        };
        let prev = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        assert!(!abandon_empty_scoping(
            &mut state,
            root,
            &prev,
            Some("Fix the login flow login errors on login retry.".to_string()),
            "test"
        ));
        assert!(state.pending_scoping.contains_key(name));
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
    fn scoping_preview_follows_freshness_gate() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let key = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        let state = WindowState {
            pending_scoping: HashMap::from([(name.to_string(), None)]),
            ..Default::default()
        };
        let preview = scoping_preview_for(&state, root, &key).expect("fresh pending renders");
        assert!(preview.contains(".samokod/plans/scoping/2026-09-26.08-41-03"));
        assert!(!preview.contains("{{PLAN_DIR}}"));

        plans::materialize_scoping(root, name).expect("materialize");
        std::fs::write(
            root.join(".samokod/plans/scoping/2026-09-26.08-41-03/plan.md"),
            "# T\n",
        )
        .expect("write");
        assert_eq!(
            scoping_preview_for(&WindowState::default(), root, &key),
            None
        );

        let mut prompted = WindowState::default();
        plans::materialize_scoping(root, "2026-09-26.08-41-04").expect("materialize");
        let other = SessionKey {
            plan: "2026-09-26.08-41-04".to_string(),
            role: SessionRole::Scoping,
        };
        assert!(scoping_preview_for(&WindowState::default(), root, &other).is_some());
        prompted.prompted.insert(other.plan.clone());
        assert_eq!(scoping_preview_for(&prompted, root, &other), None);

        let mut sent_state = WindowState::default();
        plans::materialize_scoping(root, "2026-09-26.08-41-05").expect("materialize");
        let sent = SessionKey {
            plan: "2026-09-26.08-41-05".to_string(),
            role: SessionRole::Scoping,
        };
        sent_state.sessions.insert(
            sent.clone(),
            LiveSession::fresh(
                ActivePlan::scoping(sent.plan.clone()),
                crate::repo_state::RepoState::default(),
            ),
        );
        sent_state
            .sessions
            .get_mut(&sent)
            .expect("live")
            .last_prompt = Some("hi".to_string());
        assert_eq!(scoping_preview_for(&sent_state, root, &sent), None);

        let executing = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Executing,
        };
        assert_eq!(
            scoping_preview_for(&WindowState::default(), root, &executing),
            None
        );

        let gone = SessionKey {
            plan: "2026-09-26.08-41-99".to_string(),
            role: SessionRole::Scoping,
        };
        assert_eq!(
            scoping_preview_for(&WindowState::default(), root, &gone),
            None
        );
    }

    #[test]
    fn plan_md_text_reads_live_file_or_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let key = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Scoping,
        };
        assert_eq!(plan_md_text_for(root, &key).expect("missing"), None);
        let scoping = plans::materialize_scoping(root, name).expect("materialize");
        assert_eq!(plan_md_text_for(root, &key).expect("bare"), None);
        std::fs::write(scoping.plan_md(root), "# Shiny\n\nSteps.\n").expect("write");
        let text = plan_md_text_for(root, &key)
            .expect("read")
            .expect("present");
        assert!(text.contains("# Shiny"));
        let executing = SessionKey {
            plan: name.to_string(),
            role: SessionRole::Executing,
        };
        let moved = plan_md_text_for(root, &executing)
            .expect("read executing")
            .expect("present");
        assert_eq!(moved, text);
    }

    #[test]
    fn vanish_removes_dir_session_prompt_and_pending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        plans::materialize_scoping(root, name).expect("materialize");
        let mut state = WindowState {
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
        vanish_scoping(&mut state, root, name, "test");
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
        vanish_scoping(&mut state, root, "2026-09-26.08-41-99", "test");
    }

    #[test]
    fn double_reserve_reuses_empty_pending() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let mut state = WindowState::default();
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
        let mut state = WindowState {
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
        let mut state = WindowState {
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
        {
            abandon_empty_scoping(&mut state, root, &prev, None, "select");
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
        let entries = sorted_entries(root, "", &state.sessions, &state.pending_scoping);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, target.plan);
    }

    #[test]
    fn keep_with_draft_sets_title_via_extractor() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let mut state = WindowState {
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
        let mut state = WindowState {
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
        vanish_scoping(&mut state, root, name, "test");
        assert!(!state.pending_scoping.contains_key(name));
    }

    #[test]
    fn second_keep_overwrites_title() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let mut state = WindowState {
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
        let mut state = WindowState {
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
        let entries = sorted_entries(root, "", &state.sessions, &state.pending_scoping);
        assert_eq!(entries[0].title, "Untitled");
    }

    #[test]
    fn create_with_draft_keeps_old_and_selects_new() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let mut state = WindowState {
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
        let mut state = WindowState {
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
        vanish_scoping(&mut state, root, name, "test");
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
