// Plans list: sorting, per-session statuses, and the payloads pushed to
// the frontend after every prompt, transition, or title change.
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use tauri::AppHandle;

use crate::plans;
use crate::types::{
    AppEvent, OpenRepoResult, PlanEntry, PlansUpdate, RepoDefaults, SessionKey, SessionRole,
    SessionStatusView, WorktreeStatusView,
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
        let plans = sorted_entries(
            &repo_root,
            &state.sessions,
            state.pending_scoping.as_deref(),
            &state.pending_titles,
            &state.worktrees,
            &state.checkout_branch,
        );
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
        let plans = sorted_entries(
            &repo_root,
            &state.sessions,
            state.pending_scoping.as_deref(),
            &state.pending_titles,
            &state.worktrees,
            &state.checkout_branch,
        );
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

    /// Record one user prompt for the empty-scoping gate.
    pub(crate) fn mark_prompted(&self, plan: &str) {
        if let Some(mut state) = lock_state(&self.state) {
            state.prompted.insert(plan.to_string());
        }
        self.push_plans();
    }

    /// Carry the gate flag across a plan rename.
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
}

/// Plans sorted by phase, then phase-entry arrival newest first, then
/// name descending. A reserved pending name appends a synthetic scoping
/// `PlanRef` rendering like an on-disk bare plan; with no timestamp it
/// orders by name, newest first.
pub(crate) fn sorted_entries(
    repo_root: &Path,
    sessions: &HashMap<SessionKey, LiveSession>,
    pending: Option<&str>,
    pending_titles: &HashMap<String, String>,
    worktrees: &HashMap<String, crate::worktrees::WorktreeRecord>,
    target_branch: &str,
) -> Vec<PlanEntry> {
    let mut plans = plans::scan_plans(repo_root);
    if let Some(name) = pending
        && !plans
            .iter()
            .any(|plan| plan.phase == plans::Phase::Scoping && plan.name == name)
    {
        plans.push(plans::PlanRef {
            name: name.to_string(),
            phase: plans::Phase::Scoping,
        });
    }
    plans.sort_by(|left, right| {
        plans::phase_rank(left.phase)
            .cmp(&plans::phase_rank(right.phase))
            .then_with(|| {
                plans::arrival_ms(repo_root, right)
                    .cmp(&plans::arrival_ms(repo_root, left))
                    .then_with(|| right.name.cmp(&left.name))
            })
    });
    plans
        .iter()
        .map(|plan| PlanEntry {
            name: plan.name.clone(),
            phase: plan.phase,
            title: entry_title(repo_root, plan, pending_titles),
            has_plan_md: plan.has_plan_md(repo_root),
            sessions: session_statuses(repo_root, plan, sessions),
            worktree: worktree_status(repo_root, worktrees, target_branch, plan),
            manual: entry_manual(repo_root, plan),
        })
        .collect()
}

/// Display title: `plan.md` heading first, else stored working title
/// (disk or pending map), else `Untitled`. Pure except the reads.
fn entry_title(
    repo_root: &Path,
    plan: &plans::PlanRef,
    pending_titles: &HashMap<String, String>,
) -> String {
    let text = std::fs::read_to_string(plan.plan_md(repo_root)).unwrap_or_default();
    if let Some(heading) = plans::extract_title(&text) {
        return heading;
    }
    if plan.path(repo_root).is_dir() {
        let stored = plans::load_state(&plan.path(repo_root));
        if let Some(title) = stored.working_title {
            return title;
        }
    }
    if let Some(title) = pending_titles.get(&plan.name) {
        return title.clone();
    }
    "Untitled".to_string()
}

/// Manual flag from disk state; pending names with no directory stay
/// automatic. Pure except the state read.
fn entry_manual(repo_root: &Path, plan: &plans::PlanRef) -> bool {
    if !plan.path(repo_root).is_dir() {
        return false;
    }
    plans::load_state(&plan.path(repo_root)).manual
}

/// One status row per session a plan owns, from `roles_for`: scoping
/// always, executing once approved, landing on the conflict path, finished
/// plans keeping their rows as history from the stored session IDs.
pub(crate) fn session_statuses(
    repo_root: &Path,
    plan: &plans::PlanRef,
    sessions: &HashMap<SessionKey, LiveSession>,
) -> Vec<SessionStatusView> {
    plans::roles_for(repo_root, plan)
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
            }
        })
        .collect()
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

/// Live worktree state for one executing or landing plan. Missing
/// checkouts fail safe: dirty blocks either landing button, and the backend
/// refuses the transition the same way. Git errors fail safe the same
/// way with a warn-log, per the preserve-evidence rule. The target branch
/// is the live value passed in, echoed so rows can name it.
fn worktree_status(
    repo_root: &Path,
    worktrees: &HashMap<String, crate::worktrees::WorktreeRecord>,
    target_branch: &str,
    plan: &plans::PlanRef,
) -> Option<WorktreeStatusView> {
    if !matches!(plan.phase, plans::Phase::Executing | plans::Phase::Landing) {
        return None;
    }
    let (path, worktree_branch) = match worktrees.get(&plan.name) {
        Some(record) => (record.path.clone(), record.worktree_branch.clone()),
        None => (
            crate::worktrees::worktree_path(repo_root, &plan.name),
            crate::worktrees::branch_name(&plan.name),
        ),
    };
    if !path.is_dir() {
        return Some(WorktreeStatusView {
            worktree_branch,
            target_branch: target_branch.to_string(),
            dirty: true,
            ffable: false,
        });
    }
    let dirty = match crate::worktrees::is_dirty(&path) {
        Ok(dirty) => dirty,
        Err(error) => {
            log::warn!("failed to check worktree dirtiness: {error}");
            true
        }
    };
    let ffable = match crate::worktrees::is_ffable(repo_root, target_branch, &worktree_branch) {
        Ok(ffable) => ffable,
        Err(error) => {
            log::warn!("failed to check fast-forwardability: {error}");
            false
        }
    };
    Some(WorktreeStatusView {
        worktree_branch,
        target_branch: target_branch.to_string(),
        dirty,
        ffable,
    })
}

/// Most-recent session across the sorted plans: the landing session when
/// the plan owns one, else executing, else scoping.
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
            let plans = sorted_entries(
                &repo_root,
                &guard.sessions,
                guard.pending_scoping.as_deref(),
                &guard.pending_titles,
                &guard.worktrees,
                &guard.checkout_branch,
            );
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
        assert_eq!(entry_title(root, &bare, &empty), "Untitled");
        let mut stored = plans::load_state(&bare.path(root));
        stored.working_title = Some("Login flow fixes".to_string());
        plans::store_state(&bare.path(root), &stored);
        assert_eq!(entry_title(root, &bare, &empty), "Login flow fixes");
        std::fs::write(bare.plan_md(root), "# Real heading\n").expect("write");
        assert_eq!(entry_title(root, &bare, &empty), "Real heading");
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
        let empty = HashMap::new();
        assert_eq!(entry_title(root, &pending, &empty), "Untitled");
        let titled = HashMap::from([(name.to_string(), "Login flow fixes".to_string())]);
        assert_eq!(entry_title(root, &pending, &titled), "Login flow fixes");
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
        assert_eq!(entry_title(root, &executing, &empty), "Real heading");
    }
}
