// Plans list: sorting, per-session statuses, and the plans payloads for
// responses and pushes.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::plans;
use crate::todos::estimate_remaining;
use crate::types::{
    AppEvent, OpenRepoResult, PlanEntry, PlansUpdate, ProgressView, RepoDefaults, SessionKey,
    SessionRole, SessionStatusView, SpendStatusView, TodoStatus,
};

use super::AgentManager;
use super::scoping_preview_for;
use super::session::LiveSession;
use super::{BackendState, WindowState, emit_event, lock_backend};

impl AgentManager {
    /// Current repo payload: plans plus the most-recent session.
    pub(crate) fn open_result(&self, window: &str) -> OpenRepoResult {
        let backend = lock_backend(&self.state).expect("state poisoned");
        let state = backend.windows.get(window).expect("window gone");
        let branch = state.checkout_branch.clone();
        let (repo_root, plans, selected) = listed(state, None);
        let prefs = match self.app.path().app_data_dir() {
            Ok(dir) => crate::prefs::load_prefs(&dir),
            Err(error) => {
                log::warn!("failed to resolve app data dir: {error}");
                crate::types::Prefs::default()
            }
        };
        let stored = crate::repo_state::load_repo_state(&repo_root);
        let config_defaults = defaults_for(&stored, &prefs);
        let scoping_preview = scoping_preview_for(state, &repo_root, &selected);
        OpenRepoResult {
            repo_root: repo_root.to_string_lossy().to_string(),
            branch,
            plans,
            selected,
            config_defaults,
            scoping_preview,
        }
    }

    /// Fresh plans payload with the current selection. Emitted after every
    /// prompt, transition, or title change.
    pub(crate) fn plans_update(&self, window: &str) -> PlansUpdate {
        let backend = lock_backend(&self.state).expect("state poisoned");
        let state = backend.windows.get(window).expect("window gone");
        let (repo_root, plans, selected) = listed(state, state.current.as_ref());
        let prefs = match self.app.path().app_data_dir() {
            Ok(dir) => crate::prefs::load_prefs(&dir),
            Err(error) => {
                log::warn!("failed to resolve app data dir: {error}");
                crate::types::Prefs::default()
            }
        };
        let stored = crate::repo_state::load_repo_state(&repo_root);
        let config_defaults = defaults_for(&stored, &prefs);
        let scoping_preview = scoping_preview_for(state, &repo_root, &selected);
        PlansUpdate {
            plans,
            selected,
            config_defaults,
            scoping_preview,
        }
    }

    /// Lightweight push: list plus selection only. Responses carry the
    /// preview and defaults; pushes never do.
    pub(crate) fn push_plans(&self, window: &str) {
        let (plans, selected) = match lock_backend(&self.state) {
            Some(backend) => {
                let Some(state) = backend.windows.get(window) else {
                    log::debug!("skip plans push window={window}: window closed");
                    return;
                };
                let (_, plans, selected) = listed(state, state.current.as_ref());
                (plans, selected)
            }
            None => {
                log::warn!("failed to rebuild plans list: state poisoned");
                return;
            }
        };
        emit_event(
            &self.app,
            window,
            AppEvent::PlansChanged { plans, selected },
        );
    }

    /// Record one user prompt for the empty-scoping gate, in memory and
    /// on disk. No-ops when the window has since closed.
    pub(crate) fn mark_prompted(&self, window: &str, plan: &str) {
        let repo_root = self.with_window_mut(window, |state| {
            state.prompted.insert(plan.to_string());
            state.repo_root.clone()
        });
        if let Some(Some(root)) = repo_root {
            super::session_ids::mark_prompted(&root, plan);
        }
        self.push_plans(window);
    }

    /// Carry the memory copy of the gate flag across a plan rename. The
    /// disk copy travels with the directory move.
    pub(crate) fn carry_prompted(&self, window: &str, from: &str, to: &str) {
        if let Some(mut backend) = lock_backend(&self.state)
            && let Some(state) = backend.windows.get_mut(window)
            && state.prompted.remove(from)
        {
            state.prompted.insert(to.to_string());
        }
    }

    /// Pin the selection to one session key. No-ops when the window has
    /// since closed.
    pub(crate) fn select_key(&self, window: &str, key: SessionKey) {
        if self
            .with_window_mut(window, |win| {
                win.current = Some(key);
            })
            .is_none()
        {
            log::warn!("failed to select session for {window}: window closed or state poisoned");
        }
    }

    /// Follow the acted plan's own move: pin the selection to `next` only
    /// when the viewed plan is the acted one, otherwise leave it alone so
    /// background work never steals the selection.
    pub(crate) fn select_if_viewed(&self, window: &str, acted_plan: &str, next: SessionKey) {
        let follows = lock_backend(&self.state).is_some_and(|backend| {
            backend
                .windows
                .get(window)
                .is_some_and(|state| is_viewed(state.current.as_ref(), acted_plan))
        });
        if follows {
            self.select_key(window, next);
        }
    }
}

/// True when the viewed plan is the acted one. Pure.
pub(crate) fn is_viewed(current: Option<&SessionKey>, acted_plan: &str) -> bool {
    current.is_some_and(|key| key.plan == acted_plan)
}

/// Branch commits and fast-forward state for one plan row. Empty and false
/// without touching git when the phase cannot own a worktree or when the
/// checkout branch is empty; empty and false on any git error so the list
/// always renders. Pure except the git reads.
fn branch_state_for(
    repo_root: &Path,
    checkout_branch: &str,
    plan: &plans::PlanRef,
) -> (Vec<String>, bool) {
    match plan.phase {
        plans::Phase::Scoping | plans::Phase::Completed | plans::Phase::Cancelled => {
            return (Vec::new(), false);
        }
        plans::Phase::Executing
        | plans::Phase::Queued
        | plans::Phase::Evergreening
        | plans::Phase::Rebasing => {}
    }
    if checkout_branch.is_empty() {
        return (Vec::new(), false);
    }
    let worktree_branch = crate::worktrees::branch_name(&plan.name);
    let commits = match crate::worktrees::commits(repo_root, checkout_branch, &worktree_branch) {
        Ok(commits) => commits,
        Err(error) => {
            log::warn!("failed to list commits for {}: {error}", plan.name);
            return (Vec::new(), false);
        }
    };
    let ffable = match crate::worktrees::is_ffable(repo_root, checkout_branch, &worktree_branch) {
        Ok(ffable) => ffable,
        Err(error) => {
            log::warn!("failed to check ffable for {}: {error}", plan.name);
            return (Vec::new(), false);
        }
    };
    (commits, ffable)
}

/// Plans sorted by phase, then phase-entry arrival newest first, then
/// name descending. Each pending name missing on disk appends a synthetic
/// scoping `PlanRef`; with no timestamp it orders by name, newest first.
/// Each plan reads its `state.json` once and reuses it for arrival, title,
/// flags, roles, and branch state; missing files yield the automatic
/// evergreen default. `checkout_branch` is empty when no repo is open.
/// Pure except the file and git reads.
pub(crate) fn sorted_entries(
    repo_root: &Path,
    checkout_branch: &str,
    sessions: &HashMap<SessionKey, LiveSession>,
    pending: &HashMap<String, Option<String>>,
) -> Vec<PlanEntry> {
    let mut plans = plans::scan_plans(repo_root);
    for name in pending.keys() {
        if !plans
            .iter()
            .any(|plan| plan.phase == plans::Phase::Scoping && &plan.name == name)
        {
            plans.push(plans::PlanRef {
                name: name.clone(),
                phase: plans::Phase::Scoping,
            });
        }
    }
    let mut rows: Vec<(plans::PlanRef, plans::PlanState, Option<i64>)> = plans
        .into_iter()
        .map(|plan| {
            let state = plans::load_state(&plan.path(repo_root));
            let arrival = plans::arrival_ms_with_state(repo_root, &plan, &state);
            (plan, state, arrival)
        })
        .collect();
    rows.sort_by(|left, right| {
        plans::phase_rank(left.0.phase)
            .cmp(&plans::phase_rank(right.0.phase))
            .then_with(|| {
                right
                    .2
                    .cmp(&left.2)
                    .then_with(|| right.0.name.cmp(&left.0.name))
            })
    });
    rows.iter()
        .map(|(plan, state, _)| {
            let (commits, ffable) = branch_state_for(repo_root, checkout_branch, plan);
            let rebasing_progress = rebasing_progress_for(repo_root, plan, state, &commits, ffable);
            PlanEntry {
                name: plan.name.clone(),
                phase: plan.phase,
                title: entry_title(repo_root, plan, pending, state),
                has_plan_md: plan.has_plan_md(repo_root),
                sessions: session_statuses(plan, sessions, state),
                manual: state.manual,
                evergreen: state.evergreen,
                commits,
                rebasing_progress,
            }
        })
        .collect()
}

/// Done/total progress with an ETA from the completion rate since
/// `started`. `estimate_remaining` already yields `None` for empty,
/// estimating, and done. Pure except the clock read.
fn progress_view(
    done: usize,
    total: usize,
    started: Option<std::time::SystemTime>,
) -> ProgressView {
    let eta_secs = started.and_then(|started| {
        estimate_remaining(done, total, started, std::time::SystemTime::now())
            .map(|remaining| remaining.as_secs())
    });
    ProgressView {
        done,
        total,
        eta_secs,
    }
}

/// Rebasing commit progress for one plan row. `Some` for rebasing plans
/// only: rebase `done/total` while a `rebase -i` is in flight, else the
/// commit fallback (`ffable ? n : 0` over `n`). The ETA runs from the
/// phase-entry time. Pure except the file and clock reads.
fn rebasing_progress_for(
    repo_root: &Path,
    plan: &plans::PlanRef,
    state: &plans::PlanState,
    commits: &[String],
    ffable: bool,
) -> Option<ProgressView> {
    if plan.phase != plans::Phase::Rebasing {
        return None;
    }
    let worktree_path = crate::worktrees::worktree_path(repo_root, &plan.name);
    let (done, total) = match crate::worktrees::rebase_progress(&worktree_path) {
        Some((done, total)) => (done.min(total), total),
        None => (if ffable { commits.len() } else { 0 }, commits.len()),
    };
    Some(progress_view(
        done,
        total,
        crate::plans::entered_at_time(state),
    ))
}

/// Display title: `plan.md` heading first, else stored working title
/// (disk or the pending-map value), else `Untitled`. Pure except the read.
fn entry_title(
    repo_root: &Path,
    plan: &plans::PlanRef,
    pending: &HashMap<String, Option<String>>,
    stored: &plans::PlanState,
) -> String {
    let text = std::fs::read_to_string(plan.plan_md(repo_root)).unwrap_or_default();
    if let Some(heading) = plans::extract_title(&text) {
        return heading;
    }
    if let Some(title) = &stored.working_title {
        return title.clone();
    }
    if let Some(Some(title)) = pending.get(&plan.name) {
        return title.clone();
    }
    "Untitled".to_string()
}

/// One status row per session a plan owns, from `roles_for`: scoping
/// always, executing once approved, finished plans keeping their rows as
/// history from the stored session IDs. Rebasing and evergreening rows are
/// emitted only once that agent actually started — a stored session ID or
/// a live session in the map — so a plan that only passes through the
/// phase never shows a row for work that never ran. Spend falls back to
/// the stored cost once the live session is gone, so relaunched apps still
/// show the real total.
pub(crate) fn session_statuses(
    plan: &plans::PlanRef,
    sessions: &HashMap<SessionKey, LiveSession>,
    stored: &plans::PlanState,
) -> Vec<SessionStatusView> {
    plans::roles_for(stored, plan.phase)
        .into_iter()
        .filter(|role| row_started(plan, sessions, stored, *role))
        .map(|role| {
            let key = SessionKey {
                plan: plan.name.clone(),
                role,
            };
            let live = sessions.get(&key);
            SessionStatusView {
                role,
                working: live.map(|live| live.working).unwrap_or(false),
                approval: live.map(|live| live.approval).unwrap_or(false),
                failed: live.map(|live| live.failed).unwrap_or(false),
                live: live.map(|live| live.is_live()).unwrap_or(false),
                progress: live.and_then(progress_for),
                spend: spend_for(live, stored.cost(role)),
                todos: live.map(|live| live.todos.clone()).unwrap_or_default(),
            }
        })
        .collect()
}

/// True when a session row should render: scoping, executing, and finished
/// history always show; rebasing and evergreening show only once that
/// agent started (stored ID or live session), so a plan passing through
/// the phase shows no row for work that never ran. Pure except the map
/// lookup.
fn row_started(
    plan: &plans::PlanRef,
    sessions: &HashMap<SessionKey, LiveSession>,
    stored: &plans::PlanState,
    role: SessionRole,
) -> bool {
    if !matches!(role, SessionRole::Rebasing | SessionRole::Evergreening) {
        return true;
    }
    if stored.session(role).is_some() {
        return true;
    }
    sessions.contains_key(&SessionKey {
        plan: plan.name.clone(),
        role,
    })
}

/// Spend for one session row: the live totals while the session runs, else
/// the stored cost with no live context. Pure.
fn spend_for(live: Option<&LiveSession>, stored_cost: f64) -> SpendStatusView {
    match live {
        Some(live) => SpendStatusView {
            cost: live.cost,
            ctx_pct: live.ctx_pct,
        },
        None => SpendStatusView {
            cost: stored_cost,
            ctx_pct: 0.0,
        },
    }
}

/// Todo progress for one live session: `None` when it holds no todos.
/// Every role reports uniformly; the frontend picks the executing one.
/// Pure except the clock read.
fn progress_for(live: &LiveSession) -> Option<ProgressView> {
    if live.todos.is_empty() {
        return None;
    }
    let done = live
        .todos
        .iter()
        .filter(|todo| todo.status == TodoStatus::Completed)
        .count();
    let total = live.todos.len();
    Some(progress_view(done, total, live.todos_started_at))
}

/// Stored model/effort defaults for the instant picker paint, with the
/// global fallback for repos without a per-repo choice. Pure over its
/// inputs; missing files yield empty defaults.
fn defaults_for(
    stored: &crate::repo_state::RepoState,
    prefs: &crate::types::Prefs,
) -> RepoDefaults {
    crate::repo_state::effective_defaults(stored, prefs)
}

/// Most-recent session across the sorted plans: the evergreening session
/// when the plan owns one, else rebasing, else executing, else scoping.
pub(crate) fn most_recent_key(plans: &[PlanEntry]) -> Option<SessionKey> {
    plans.first().map(|entry| {
        let role = if entry
            .sessions
            .iter()
            .any(|status| status.role == SessionRole::Evergreening)
        {
            SessionRole::Evergreening
        } else if entry
            .sessions
            .iter()
            .any(|status| status.role == SessionRole::Rebasing)
        {
            SessionRole::Rebasing
        } else if entry
            .sessions
            .iter()
            .any(|status| status.role == SessionRole::Executing)
        {
            SessionRole::Executing
        } else {
            SessionRole::Scoping
        };
        SessionKey {
            plan: entry.name.clone(),
            role,
        }
    })
}

/// Current selection with fallback: the held key, else the most-recent
/// session, else a scoping key for the first plan (empty when no plans).
/// Pure.
fn fallback_selected(plans: &[PlanEntry], current: Option<&SessionKey>) -> SessionKey {
    current
        .cloned()
        .or_else(|| most_recent_key(plans))
        .unwrap_or_else(|| SessionKey {
            plan: plans
                .first()
                .map(|entry| entry.name.clone())
                .unwrap_or_default(),
            role: SessionRole::Scoping,
        })
}

/// Sorted plans plus selection for one locked state: the shared prefix of
/// every plans payload. Responses extend it with defaults and the scoping
/// preview; pushes emit it as-is.
fn listed(
    state: &WindowState,
    current: Option<&SessionKey>,
) -> (PathBuf, Vec<PlanEntry>, SessionKey) {
    let repo_root = state.repo_root.clone().unwrap_or_default();
    let plans = sorted_entries(
        &repo_root,
        &state.checkout_branch,
        &state.sessions,
        &state.pending_scoping,
    );
    let selected = fallback_selected(&plans, current);
    (repo_root, plans, selected)
}

/// Rebuild the plans list from a spawned task holding only state and app.
/// Titles re-read here, so every finished turn refreshes plan names.
pub(crate) fn push_sorted(state: &Mutex<BackendState>, app: &AppHandle, window: &str) {
    let (plans, selected) = match state.lock() {
        Ok(guard) => {
            let Some(win) = guard.windows.get(window) else {
                log::debug!("skip plans push window={window}: window closed");
                return;
            };
            let repo_root = win.repo_root.clone().unwrap_or_default();
            let checkout_branch = win.checkout_branch.clone();
            let plans = sorted_entries(
                &repo_root,
                &checkout_branch,
                &win.sessions,
                &win.pending_scoping,
            );
            let selected = win.current.clone().or_else(|| most_recent_key(&plans));
            (plans, selected)
        }
        Err(error) => {
            log::warn!("failed to rebuild plans list: {error}");
            return;
        }
    };
    if let Some(selected) = selected {
        emit_event(app, window, AppEvent::PlansChanged { plans, selected });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_mirror_stored_repo_state() {
        let empty_prefs = crate::types::Prefs::default();
        assert_eq!(
            defaults_for(&crate::repo_state::RepoState::default(), &empty_prefs),
            RepoDefaults {
                model: None,
                effort: None,
            }
        );
        let stored = crate::repo_state::RepoState {
            model: Some("m1".to_string()),
            effort: Some("high".to_string()),
        };
        assert_eq!(
            defaults_for(&stored, &empty_prefs),
            RepoDefaults {
                model: Some("m1".to_string()),
                effort: Some("high".to_string()),
            }
        );
        let prefs = crate::types::Prefs {
            default_model: Some("m2".to_string()),
            default_effort: Some("low".to_string()),
            ..Default::default()
        };
        assert_eq!(
            defaults_for(&crate::repo_state::RepoState::default(), &prefs),
            RepoDefaults {
                model: Some("m2".to_string()),
                effort: Some("low".to_string()),
            }
        );
        assert_eq!(
            defaults_for(&stored, &prefs),
            RepoDefaults {
                model: Some("m1".to_string()),
                effort: Some("high".to_string()),
            }
        );
    }

    #[test]
    fn heading_beats_working_title_beats_untitled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let empty = HashMap::new();
        let bare = plans::materialize_scoping(root, "2026-09-26.08-41-03").expect("bare");
        let stored = plans::load_state(&bare.path(root));
        assert_eq!(entry_title(root, &bare, &empty, &stored), "Untitled");
        let mut stored = plans::load_state(&bare.path(root));
        stored.working_title = Some("Login flow fixes".to_string());
        plans::store_state(&bare.path(root), &stored);
        let stored = plans::load_state(&bare.path(root));
        assert_eq!(
            entry_title(root, &bare, &empty, &stored),
            "Login flow fixes"
        );
        std::fs::write(bare.plan_md(root), "# Real heading\n").expect("write");
        assert_eq!(entry_title(root, &bare, &empty, &stored), "Real heading");
    }

    #[test]
    fn pending_title_shows_until_materialized() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let name = "2026-09-26.08-41-03";
        let pending = plans::PlanRef {
            name: name.to_string(),
            phase: plans::Phase::Scoping,
        };
        let empty: HashMap<String, Option<String>> = HashMap::new();
        let stored = plans::PlanState::default();
        assert_eq!(entry_title(root, &pending, &empty, &stored), "Untitled");
        let titled = HashMap::from([(name.to_string(), Some("Login flow fixes".to_string()))]);
        assert_eq!(
            entry_title(root, &pending, &titled, &stored),
            "Login flow fixes"
        );
    }

    #[test]
    fn working_title_travels_through_rename() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let scoping = plans::materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        let mut stored = plans::load_state(&scoping.path(root));
        stored.working_title = Some("Login flow fixes".to_string());
        plans::store_state(&scoping.path(root), &stored);
        std::fs::write(scoping.plan_md(root), "# Real heading\n").expect("write");
        let executing = plans::execute(root, &scoping).expect("execute");
        let kept = plans::load_state(&executing.path(root));
        assert_eq!(kept.working_title.as_deref(), Some("Login flow fixes"));
        let empty = HashMap::new();
        assert_eq!(entry_title(root, &executing, &empty, &kept), "Real heading");
    }

    #[test]
    fn only_the_viewed_plan_follows() {
        let viewed = SessionKey {
            plan: "aaa".to_string(),
            role: SessionRole::Scoping,
        };
        assert!(is_viewed(Some(&viewed), "aaa"));
        assert!(!is_viewed(Some(&viewed), "bbb"));
        assert!(!is_viewed(None, "aaa"));
    }

    fn test_git(repo: &Path, args: &[&str]) -> std::process::Output {
        crate::git::command(repo).args(args).output().expect("git")
    }

    fn git_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "test"],
            vec!["commit", "--allow-empty", "-m", "init"],
        ] {
            let status = test_git(dir.path(), &args);
            assert!(status.status.success(), "{args:?}");
        }
        dir
    }

    fn commit_file(repo: &Path, name: &str, contents: &str, message: &str) {
        std::fs::write(repo.join(name), contents).expect("write");
        for args in [vec!["add", name], vec!["commit", "-m", message]] {
            let status = test_git(repo, &args);
            assert!(status.status.success(), "{args:?}");
        }
    }

    fn current_branch(repo: &Path) -> String {
        let output = test_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .expect("utf8")
            .trim()
            .to_string()
    }

    #[test]
    fn branch_state_lists_diverged_commits_oldest_first() {
        let dir = git_repo();
        let root = dir.path();
        let target = current_branch(root);
        let base = crate::worktrees::head_commit(root).expect("head");
        let name = "2026-09-26.14-53-26.branch-check";
        let plan = plans::PlanRef {
            name: name.to_string(),
            phase: plans::Phase::Rebasing,
        };
        let record = crate::worktrees::create(root, name, &base).expect("create");
        commit_file(&record.path, "work.txt", "work\n", "plan work");
        commit_file(&record.path, "more.txt", "more\n", "more work");
        commit_file(root, "target.txt", "target\n", "target moves on");
        let (commits, ffable) = branch_state_for(root, &target, &plan);
        assert_eq!(commits.len(), 2);
        assert!(commits[0].ends_with("plan work"));
        assert!(commits[1].ends_with("more work"));
        assert!(!ffable);
        crate::worktrees::remove(root, &record.path, &record.worktree_branch, true)
            .expect("remove");
    }

    #[test]
    fn branch_state_is_ffable_once_target_catches_up() {
        let dir = git_repo();
        let root = dir.path();
        let target = current_branch(root);
        let base = crate::worktrees::head_commit(root).expect("head");
        let name = "2026-09-26.14-53-27.ready-check";
        let plan = plans::PlanRef {
            name: name.to_string(),
            phase: plans::Phase::Rebasing,
        };
        let record = crate::worktrees::create(root, name, &base).expect("create");
        commit_file(&record.path, "work.txt", "work\n", "plan work");
        let branch = crate::worktrees::branch_name(name);
        crate::worktrees::fast_forward(root, &target, &branch).expect("ff");
        let (commits, ffable) = branch_state_for(root, &target, &plan);
        assert!(ffable);
        assert!(commits.is_empty());
        crate::worktrees::remove(root, &record.path, &record.worktree_branch, true)
            .expect("remove");
    }

    #[test]
    fn branch_state_skips_scoping_without_git() {
        let plan = plans::PlanRef {
            name: "2026-09-26.08-41-03".to_string(),
            phase: plans::Phase::Scoping,
        };
        let missing = Path::new("/definitely/missing/repo");
        let (commits, ffable) = branch_state_for(missing, "main", &plan);
        assert!(commits.is_empty());
        assert!(!ffable);
        let executing = plans::PlanRef {
            name: "2026-09-26.08-41-03.slug".to_string(),
            phase: plans::Phase::Executing,
        };
        let (commits, ffable) = branch_state_for(missing, "", &executing);
        assert!(commits.is_empty());
        assert!(!ffable);
    }

    #[test]
    fn branch_state_lists_queued_commits_like_executing() {
        let dir = git_repo();
        let root = dir.path();
        let target = current_branch(root);
        let base = crate::worktrees::head_commit(root).expect("head");
        let name = "2026-09-26.14-53-26.queued-check";
        let plan = plans::PlanRef {
            name: name.to_string(),
            phase: plans::Phase::Queued,
        };
        let record = crate::worktrees::create(root, name, &base).expect("create");
        commit_file(&record.path, "work.txt", "work\n", "plan work");
        commit_file(root, "target.txt", "target\n", "target moves on");
        let (commits, ffable) = branch_state_for(root, &target, &plan);
        assert_eq!(commits.len(), 1);
        assert!(commits[0].ends_with("plan work"));
        assert!(!ffable);
        crate::worktrees::remove(root, &record.path, &record.worktree_branch, true)
            .expect("remove");
    }

    fn rebasing_plan(name: &str) -> plans::PlanRef {
        plans::PlanRef {
            name: name.to_string(),
            phase: plans::Phase::Rebasing,
        }
    }

    fn fake_rebase(repo_root: &Path, plan_name: &str, msgnum: &str, end: &str) {
        let worktree = crate::worktrees::worktree_path(repo_root, plan_name);
        let gitdir = repo_root.join("git-admin").join(plan_name);
        std::fs::create_dir_all(&worktree).expect("worktree dir");
        std::fs::create_dir_all(gitdir.join("rebase-merge")).expect("gitdir dir");
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", gitdir.display()),
        )
        .expect("git file");
        std::fs::write(gitdir.join("rebase-merge").join("msgnum"), msgnum).expect("msgnum");
        std::fs::write(gitdir.join("rebase-merge").join("end"), end).expect("end");
    }

    fn entered_at_ms(time: std::time::SystemTime) -> i64 {
        crate::plans::unix_ms(time).expect("millis")
    }

    #[test]
    fn rebasing_falls_back_to_diverged_commits_without_rebase() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = rebasing_plan("2026-09-26.14-53-26.branch-check");
        let state = plans::PlanState::default();
        let commits = vec!["abc plan work".to_string(), "def more work".to_string()];
        let progress = rebasing_progress_for(dir.path(), &plan, &state, &commits, false)
            .expect("rebasing progress");
        assert_eq!(progress.done, 0);
        assert_eq!(progress.total, 2);
        assert_eq!(progress.eta_secs, None);
    }

    #[test]
    fn rebasing_falls_back_to_full_when_ffable_without_rebase() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = rebasing_plan("2026-09-26.14-53-27.ready-check");
        let state = plans::PlanState::default();
        let commits = vec!["abc plan work".to_string(), "def more work".to_string()];
        let progress = rebasing_progress_for(dir.path(), &plan, &state, &commits, true)
            .expect("rebasing progress");
        assert_eq!(progress.done, 2);
        assert_eq!(progress.total, 2);
        assert_eq!(progress.eta_secs, None);
    }

    #[test]
    fn rebasing_empty_commits_show_empty_track() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = rebasing_plan("2026-09-26.14-53-28.empty-check");
        let state = plans::PlanState::default();
        let progress = rebasing_progress_for(dir.path(), &plan, &state, &[], false)
            .expect("rebasing progress");
        assert_eq!(progress.done, 0);
        assert_eq!(progress.total, 0);
        assert_eq!(progress.eta_secs, None);
    }

    #[test]
    fn rebasing_rebase_without_entered_at_has_no_eta() {
        let dir = tempfile::tempdir().expect("tempdir");
        let name = "2026-09-26.14-53-29.rebase-check";
        fake_rebase(dir.path(), name, "2\n", "2\n");
        let plan = rebasing_plan(name);
        let state = plans::PlanState::default();
        let commits = vec!["abc plan work".to_string(), "def more work".to_string()];
        let progress =
            rebasing_progress_for(dir.path(), &plan, &state, &commits, false).expect("rebasing");
        assert_eq!((progress.done, progress.total), (1, 2));
        assert_eq!(progress.eta_secs, None);
    }

    #[test]
    fn rebasing_rebase_with_entered_at_scales_eta() {
        let dir = tempfile::tempdir().expect("tempdir");
        let name = "2026-09-26.14-53-30.eta-check";
        fake_rebase(dir.path(), name, "2\n", "2\n");
        let started = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        let state = plans::PlanState {
            entered_at: Some(entered_at_ms(started)),
            ..Default::default()
        };
        let plan = rebasing_plan(name);
        let commits = vec!["abc plan work".to_string(), "def more work".to_string()];
        let progress =
            rebasing_progress_for(dir.path(), &plan, &state, &commits, false).expect("rebasing");
        assert_eq!((progress.done, progress.total), (1, 2));
        let eta = progress.eta_secs.expect("eta");
        let expected = estimate_remaining(1, 2, started, std::time::SystemTime::now())
            .expect("estimate")
            .as_secs();
        assert!(
            eta.abs_diff(expected) <= 2,
            "eta {eta} close to expected {expected}"
        );
    }

    #[test]
    fn non_rebasing_phases_send_no_rebasing_progress() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = plans::PlanState::default();
        let commits = vec!["abc plan work".to_string()];
        for phase in [
            plans::Phase::Scoping,
            plans::Phase::Executing,
            plans::Phase::Queued,
            plans::Phase::Evergreening,
            plans::Phase::Completed,
            plans::Phase::Cancelled,
        ] {
            let plan = plans::PlanRef {
                name: "2026-09-26.08-41-03.slug".to_string(),
                phase,
            };
            assert_eq!(
                rebasing_progress_for(dir.path(), &plan, &state, &commits, false),
                None,
                "{phase:?}"
            );
        }
    }

    #[test]
    fn inactive_session_falls_back_to_stored_cost() {
        let plan = plans::PlanRef {
            name: "2026-09-26.08-41-03".to_string(),
            phase: plans::Phase::Scoping,
        };
        let mut stored = plans::PlanState::default();
        stored.set_session(SessionRole::Scoping, "ses_scoping".to_string());
        stored.set_cost(SessionRole::Scoping, 1.24);
        let statuses = session_statuses(&plan, &HashMap::new(), &stored);
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].role, SessionRole::Scoping);
        assert_eq!(statuses[0].spend.cost, 1.24);
        assert_eq!(statuses[0].spend.ctx_pct, 0.0);
    }

    #[test]
    fn live_session_prefers_live_spend_over_stored() {
        let plan = plans::PlanRef {
            name: "2026-09-26.08-41-03".to_string(),
            phase: plans::Phase::Scoping,
        };
        let mut stored = plans::PlanState::default();
        stored.set_session(SessionRole::Scoping, "ses_scoping".to_string());
        stored.set_cost(SessionRole::Scoping, 1.24);
        let key = SessionKey {
            plan: plan.name.clone(),
            role: SessionRole::Scoping,
        };
        let mut live = LiveSession::fresh(
            super::super::session::ActivePlan::scoping(plan.name.clone()),
            crate::repo_state::RepoState::default(),
        );
        live.cost = 2.5;
        live.ctx_pct = 42.0;
        let sessions = HashMap::from([(key, live)]);
        let statuses = session_statuses(&plan, &sessions, &stored);
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].spend.cost, 2.5);
        assert_eq!(statuses[0].spend.ctx_pct, 42.0);
    }

    #[test]
    fn never_spent_plan_renders_zero_spend() {
        let plan = plans::PlanRef {
            name: "2026-09-26.08-41-03".to_string(),
            phase: plans::Phase::Scoping,
        };
        let stored = plans::PlanState::default();
        let statuses = session_statuses(&plan, &HashMap::new(), &stored);
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].spend.cost, 0.0);
        assert_eq!(statuses[0].spend.ctx_pct, 0.0);
    }

    #[test]
    fn queued_rows_hold_with_no_rebasing_progress() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        let plan = plans::PlanRef {
            name: "2026-09-26.08-41-03.slug".to_string(),
            phase: plans::Phase::Queued,
        };
        std::fs::create_dir_all(plan.path(root)).expect("mkdir");
        let entries = sorted_entries(root, "", &HashMap::new(), &HashMap::new());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].phase, plans::Phase::Queued);
        assert_eq!(entries[0].rebasing_progress, None);
        assert_eq!(
            entries[0]
                .sessions
                .iter()
                .map(|status| status.role)
                .collect::<Vec<_>>(),
            vec![SessionRole::Scoping, SessionRole::Executing]
        );
    }

    fn seed_ids(root: &Path, name: &str, ids: &[(SessionRole, &str)]) {
        for (role, id) in ids {
            super::super::session_ids::record_session(
                root,
                &SessionKey {
                    plan: name.to_string(),
                    role: *role,
                },
                id,
            );
        }
    }

    fn entry_roles(entry: &PlanEntry) -> Vec<SessionRole> {
        entry.sessions.iter().map(|status| status.role).collect()
    }

    fn listed_entry(
        root: &Path,
        phase: plans::Phase,
        ids: &[(SessionRole, &str)],
        with_live_rebasing: bool,
    ) -> PlanEntry {
        let name = "2026-09-26.08-41-03.slug";
        let plan = plans::PlanRef {
            name: name.to_string(),
            phase,
        };
        std::fs::create_dir_all(plan.path(root)).expect("mkdir");
        seed_ids(root, name, ids);
        let sessions = if with_live_rebasing {
            let key = SessionKey {
                plan: name.to_string(),
                role: SessionRole::Rebasing,
            };
            HashMap::from([(
                key,
                LiveSession::fresh(
                    super::super::session::ActivePlan::rebasing(name.to_string()),
                    crate::repo_state::RepoState::default(),
                ),
            )])
        } else {
            HashMap::new()
        };
        let entries = sorted_entries(root, "", &sessions, &HashMap::new());
        assert_eq!(entries.len(), 1);
        entries.into_iter().next().expect("entry")
    }

    const BASE_IDS: [(SessionRole, &str); 2] = [
        (SessionRole::Scoping, "ses_scoping"),
        (SessionRole::Executing, "ses_executing"),
    ];
    const REBASING_IDS: [(SessionRole, &str); 3] = [
        (SessionRole::Scoping, "ses_scoping"),
        (SessionRole::Executing, "ses_executing"),
        (SessionRole::Rebasing, "ses_rebasing"),
    ];
    const GREENING_IDS: [(SessionRole, &str); 4] = [
        (SessionRole::Scoping, "ses_scoping"),
        (SessionRole::Executing, "ses_executing"),
        (SessionRole::Rebasing, "ses_rebasing"),
        (SessionRole::Evergreening, "ses_evergreening"),
    ];

    #[test]
    fn rebasing_without_agent_hides_row_but_keeps_progress() {
        let dir = tempfile::tempdir().expect("tempdir");
        plans::ensure_structure(dir.path()).expect("ensure");
        let entry = listed_entry(dir.path(), plans::Phase::Rebasing, &BASE_IDS, false);
        assert_eq!(
            entry_roles(&entry),
            vec![SessionRole::Scoping, SessionRole::Executing]
        );
        assert!(entry.rebasing_progress.is_some());
        assert_eq!(
            most_recent_key(std::slice::from_ref(&entry)),
            Some(SessionKey {
                plan: entry.name.clone(),
                role: SessionRole::Executing,
            })
        );
    }

    #[test]
    fn rebasing_with_started_agent_keeps_rebasing_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        plans::ensure_structure(dir.path()).expect("ensure");
        let entry = listed_entry(dir.path(), plans::Phase::Rebasing, &REBASING_IDS, false);
        assert_eq!(
            entry_roles(&entry),
            vec![
                SessionRole::Scoping,
                SessionRole::Executing,
                SessionRole::Rebasing
            ]
        );
        assert_eq!(
            most_recent_key(std::slice::from_ref(&entry)),
            Some(SessionKey {
                plan: entry.name.clone(),
                role: SessionRole::Rebasing,
            })
        );
    }

    #[test]
    fn rebasing_with_live_session_keeps_rebasing_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        plans::ensure_structure(dir.path()).expect("ensure");
        let entry = listed_entry(dir.path(), plans::Phase::Rebasing, &BASE_IDS, true);
        assert_eq!(
            entry_roles(&entry),
            vec![
                SessionRole::Scoping,
                SessionRole::Executing,
                SessionRole::Rebasing
            ]
        );
    }

    #[test]
    fn evergreening_without_cleanup_shows_no_evergreening_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        plans::ensure_structure(dir.path()).expect("ensure");
        let entry = listed_entry(dir.path(), plans::Phase::Evergreening, &REBASING_IDS, false);
        assert_eq!(
            entry_roles(&entry),
            vec![
                SessionRole::Scoping,
                SessionRole::Executing,
                SessionRole::Rebasing
            ]
        );
        assert_eq!(
            most_recent_key(std::slice::from_ref(&entry)),
            Some(SessionKey {
                plan: entry.name.clone(),
                role: SessionRole::Rebasing,
            })
        );
    }

    #[test]
    fn evergreening_with_cleanup_keeps_evergreening_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        plans::ensure_structure(dir.path()).expect("ensure");
        let entry = listed_entry(dir.path(), plans::Phase::Evergreening, &GREENING_IDS, false);
        assert_eq!(
            entry_roles(&entry),
            vec![
                SessionRole::Scoping,
                SessionRole::Executing,
                SessionRole::Rebasing,
                SessionRole::Evergreening
            ]
        );
        assert_eq!(
            most_recent_key(std::slice::from_ref(&entry)),
            Some(SessionKey {
                plan: entry.name.clone(),
                role: SessionRole::Evergreening,
            })
        );
    }

    #[test]
    fn completed_keeps_stored_id_rows() {
        let dir = tempfile::tempdir().expect("tempdir");
        plans::ensure_structure(dir.path()).expect("ensure");
        let entry = listed_entry(dir.path(), plans::Phase::Completed, &REBASING_IDS, false);
        assert_eq!(
            entry_roles(&entry),
            vec![
                SessionRole::Scoping,
                SessionRole::Executing,
                SessionRole::Rebasing
            ]
        );
    }
}
