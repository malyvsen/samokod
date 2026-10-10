// Plans list: sorting, per-session statuses, and the payloads pushed to
// the frontend after every prompt, transition, or title change.
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use tauri::AppHandle;

use crate::plans;
use crate::todos::estimate_remaining;
use crate::types::{
    AppEvent, OpenRepoResult, PlanEntry, PlansUpdate, RepoDefaults, SessionKey, SessionRole,
    SessionStatusView, SpendStatusView, TodoProgressView, TodoStatus,
};

use super::AgentManager;
use super::session::LiveSession;
use super::{State, emit_event, lock_state};

impl AgentManager {
    /// Current repo payload: plans plus the most-recent session.
    pub(crate) fn open_result(&self) -> OpenRepoResult {
        let state = self.state.lock().expect("state poisoned");
        let repo_root = state.repo_root.clone().unwrap_or_default();
        let branch = state.checkout_branch.clone();
        let plans = sorted_entries(&repo_root, &state.sessions, &state.pending_scoping);
        let selected = most_recent_key(&plans).unwrap_or_else(|| SessionKey {
            plan: plans
                .first()
                .map(|entry| entry.name.clone())
                .unwrap_or_default(),
            role: SessionRole::Scoping,
        });
        let config_defaults = defaults_for(&repo_root);
        OpenRepoResult {
            repo_root: repo_root.to_string_lossy().to_string(),
            branch,
            plans,
            selected,
            config_defaults,
        }
    }

    /// Fresh plans payload with the current selection. Emitted after every
    /// prompt, transition, or title change.
    pub(crate) fn plans_update(&self) -> PlansUpdate {
        let state = self.state.lock().expect("state poisoned");
        let repo_root = state.repo_root.clone().unwrap_or_default();
        let plans = sorted_entries(&repo_root, &state.sessions, &state.pending_scoping);
        let selected = state.current.clone().unwrap_or_else(|| {
            most_recent_key(&plans).unwrap_or_else(|| SessionKey {
                plan: plans
                    .first()
                    .map(|entry| entry.name.clone())
                    .unwrap_or_default(),
                role: SessionRole::Scoping,
            })
        });
        let config_defaults = defaults_for(&repo_root);
        PlansUpdate {
            plans,
            selected,
            config_defaults,
        }
    }

    pub(crate) fn push_plans(&self) {
        let update = self.plans_update();
        emit_event(
            &self.app,
            AppEvent::PlansChanged {
                plans: update.plans,
                selected: update.selected,
            },
        );
    }

    /// Record one user prompt for the empty-scoping gate, in memory and
    /// on disk.
    pub(crate) fn mark_prompted(&self, plan: &str) {
        let repo_root = match lock_state(&self.state) {
            Some(mut state) => {
                state.prompted.insert(plan.to_string());
                state.repo_root.clone()
            }
            None => None,
        };
        if let Some(root) = repo_root {
            super::session_ids::mark_prompted(&root, plan);
        }
        self.push_plans();
    }

    /// Carry the memory copy of the gate flag across a plan rename. The
    /// disk copy travels with the directory move.
    pub(crate) fn carry_prompted(&self, from: &str, to: &str) {
        if let Some(mut state) = lock_state(&self.state)
            && state.prompted.remove(from)
        {
            state.prompted.insert(to.to_string());
        }
    }

    /// Pin the selection to one session key.
    pub(crate) fn select_key(&self, key: SessionKey) {
        match self.state.lock() {
            Ok(mut state) => {
                state.current = Some(key);
            }
            Err(error) => {
                log::warn!("failed to select session: {error}");
            }
        }
    }

    /// Follow the acted plan's own move: pin the selection to `next` only
    /// when the viewed plan is the acted one, otherwise leave it alone so
    /// background work never steals the selection.
    pub(crate) fn select_if_viewed(&self, acted_plan: &str, next: SessionKey) {
        let follows = lock_state(&self.state)
            .is_some_and(|state| is_viewed(state.current.as_ref(), acted_plan));
        if follows {
            self.select_key(next);
        }
    }
}

/// True when the viewed plan is the acted one. Pure.
pub(crate) fn is_viewed(current: Option<&SessionKey>, acted_plan: &str) -> bool {
    current.is_some_and(|key| key.plan == acted_plan)
}

/// Plans sorted by phase, then phase-entry arrival newest first, then
/// name descending. Each pending name missing on disk appends a synthetic
/// scoping `PlanRef`; with no timestamp it orders by name, newest first.
/// Each plan reads its `state.json` once and reuses it for arrival, title,
/// flags, and roles; missing files yield the automatic evergreen default.
pub(crate) fn sorted_entries(
    repo_root: &Path,
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
        .map(|(plan, state, _)| PlanEntry {
            name: plan.name.clone(),
            phase: plan.phase,
            title: entry_title(repo_root, plan, pending, state),
            has_plan_md: plan.has_plan_md(repo_root),
            sessions: session_statuses(plan, sessions, state),
            manual: state.manual,
            evergreen: state.evergreen,
        })
        .collect()
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
/// always, executing once approved, landing on the conflict path, finished
/// plans keeping their rows as history from the stored session IDs.
pub(crate) fn session_statuses(
    plan: &plans::PlanRef,
    sessions: &HashMap<SessionKey, LiveSession>,
    stored: &plans::PlanState,
) -> Vec<SessionStatusView> {
    plans::roles_for(stored, plan.phase)
        .into_iter()
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
                spend: live
                    .map(|live| SpendStatusView {
                        cost: live.cost,
                        ctx_pct: live.ctx_pct,
                    })
                    .unwrap_or_default(),
                todos: live.map(|live| live.todos.clone()).unwrap_or_default(),
            }
        })
        .collect()
}

/// Todo progress for one live session: `None` when it holds no todos.
/// Every role reports uniformly; the frontend picks the executing one.
/// Pure except the clock read.
fn progress_for(live: &LiveSession) -> Option<TodoProgressView> {
    if live.todos.is_empty() {
        return None;
    }
    let done = live
        .todos
        .iter()
        .filter(|todo| todo.status == TodoStatus::Completed)
        .count();
    let total = live.todos.len();
    let eta_secs = live.todos_started_at.and_then(|started| {
        estimate_remaining(done, total, started, std::time::SystemTime::now())
            .map(|remaining| remaining.as_secs())
    });
    Some(TodoProgressView {
        done,
        total,
        eta_secs,
    })
}

/// Stored model/effort defaults for the instant picker paint. Pure file
/// read; missing files yield empty defaults.
fn defaults_for(repo_root: &Path) -> RepoDefaults {
    let stored = crate::repo_state::load_repo_state(repo_root);
    RepoDefaults {
        model: stored.model,
        effort: stored.effort,
    }
}

/// Most-recent session across the sorted plans: the landing session when
/// the plan owns one, else evergreening, else executing, else scoping.
pub(crate) fn most_recent_key(plans: &[PlanEntry]) -> Option<SessionKey> {
    plans.first().map(|entry| {
        let role = if entry
            .sessions
            .iter()
            .any(|status| status.role == SessionRole::Landing)
        {
            SessionRole::Landing
        } else if entry
            .sessions
            .iter()
            .any(|status| status.role == SessionRole::Evergreening)
        {
            SessionRole::Evergreening
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

/// Rebuild the plans list from a spawned task holding only state and app.
/// Titles re-read here, so every finished turn refreshes plan names.
pub(crate) fn push_sorted(state: &Mutex<State>, app: &AppHandle) {
    let (plans, selected) = match state.lock() {
        Ok(guard) => {
            let repo_root = guard.repo_root.clone().unwrap_or_default();
            let plans = sorted_entries(&repo_root, &guard.sessions, &guard.pending_scoping);
            let selected = guard.current.clone().or_else(|| most_recent_key(&plans));
            (plans, selected)
        }
        Err(error) => {
            log::warn!("failed to rebuild plans list: {error}");
            return;
        }
    };
    if let Some(selected) = selected {
        emit_event(app, AppEvent::PlansChanged { plans, selected });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_mirror_stored_repo_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            defaults_for(dir.path()),
            RepoDefaults {
                model: None,
                effort: None,
            }
        );
        crate::repo_state::set_role(dir.path(), crate::repo_state::ConfigRole::Model, Some("m1"));
        crate::repo_state::set_role(
            dir.path(),
            crate::repo_state::ConfigRole::Effort,
            Some("high"),
        );
        assert_eq!(
            defaults_for(dir.path()),
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
}
