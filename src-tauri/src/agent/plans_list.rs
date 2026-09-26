// Plans list: sorting, per-session statuses, and the payloads pushed to
// the frontend after every transition, activity, or title change.
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

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
        let branch = state.branch.clone();
        let plans = sorted_entries(
            &repo_root,
            &state.sessions,
            &state.activity,
            state.pending_scoping.as_deref(),
            &state.worktrees,
            &state.branch,
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
    /// transition, activity, or title change.
    pub(crate) fn plans_update(&self) -> PlansUpdate {
        let state = self.state.lock().expect("state poisoned");
        let repo_root = state.repo_root.clone().unwrap_or_default();
        let plans = sorted_entries(
            &repo_root,
            &state.sessions,
            &state.activity,
            state.pending_scoping.as_deref(),
            &state.worktrees,
            &state.branch,
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

    /// Record one user prompt for plan sorting. Pure timestamp edge.
    pub(crate) fn touch_activity(&self, plan: &str) {
        if let Some(mut state) = lock_state(&self.state) {
            state.activity.insert(plan.to_string(), Instant::now());
        }
        self.push_plans();
    }

    /// Carry recency across a plan rename. Pure timestamp edge.
    pub(crate) fn move_activity(&self, from: &str, to: &str) {
        if let Some(mut state) = lock_state(&self.state)
            && let Some(when) = state.activity.remove(from)
        {
            state.activity.insert(to.to_string(), when);
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

/// Plans sorted by phase, then most recent user activity first. Before any
/// activity, `plan.md` modification time newest first, falling back to the
/// directory name (which starts with a creation timestamp). A reserved
/// pending name appends a synthetic scoping `PlanRef` rendering like an
/// on-disk bare plan; with no mtime it orders by name, newest first.
pub(crate) fn sorted_entries(
    repo_root: &Path,
    sessions: &HashMap<SessionKey, LiveSession>,
    activity: &HashMap<String, Instant>,
    pending: Option<&str>,
    worktrees: &HashMap<String, crate::worktrees::WorktreeRecord>,
    main_branch: &str,
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
                // Active plans sort before idle ones; recency decides
                // within each group.
                match (activity.get(&left.name), activity.get(&right.name)) {
                    (Some(left_at), Some(right_at)) => right_at.cmp(left_at),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => plans::plan_mtime(repo_root, right)
                        .cmp(&plans::plan_mtime(repo_root, left))
                        .then_with(|| right.name.cmp(&left.name)),
                }
            })
    });
    plans
        .iter()
        .map(|plan| PlanEntry {
            name: plan.name.clone(),
            phase: plan.phase,
            title: plans::plan_title(repo_root, plan),
            has_plan_md: plan.has_plan_md(repo_root),
            sessions: session_statuses(repo_root, plan, sessions),
            worktree: worktree_status(repo_root, worktrees, main_branch, plan),
        })
        .collect()
}

/// One status row per session a plan owns, from `roles_for`: scoping
/// always, executing once approved, each staying as history.
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

/// Live worktree state for one executing or merging plan. Missing
/// checkouts fail safe: dirty blocks either merge button, and the backend
/// refuses the transition the same way. Git errors fail safe the same
/// way with a warn-log, per the preserve-evidence rule.
fn worktree_status(
    repo_root: &Path,
    worktrees: &HashMap<String, crate::worktrees::WorktreeRecord>,
    main_branch: &str,
    plan: &plans::PlanRef,
) -> Option<WorktreeStatusView> {
    if !matches!(plan.phase, plans::Phase::Executing | plans::Phase::Merging) {
        return None;
    }
    let (path, branch, main) = match worktrees.get(&plan.name) {
        Some(record) => (
            record.path.clone(),
            record.branch.clone(),
            record.main_branch.clone(),
        ),
        None => (
            crate::worktrees::worktree_path(repo_root, &plan.name),
            crate::worktrees::branch_name(&plan.name),
            main_branch.to_string(),
        ),
    };
    if !path.is_dir() {
        return Some(WorktreeStatusView {
            branch,
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
    let ffable = match crate::worktrees::is_ffable(repo_root, &main, &branch) {
        Ok(ffable) => ffable,
        Err(error) => {
            log::warn!("failed to check fast-forwardability: {error}");
            false
        }
    };
    Some(WorktreeStatusView {
        branch,
        dirty,
        ffable,
    })
}

/// Most-recent session across the sorted plans: the merging session when
/// the plan owns one, else executing, else scoping.
pub(crate) fn most_recent_key(plans: &[PlanEntry]) -> Option<SessionKey> {
    plans.first().map(|entry| {
        let role = if entry
            .sessions
            .iter()
            .any(|status| status.role == SessionRole::Merging)
        {
            SessionRole::Merging
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
                &guard.activity,
                guard.pending_scoping.as_deref(),
                &guard.worktrees,
                &guard.branch,
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
}
