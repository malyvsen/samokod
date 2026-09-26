// Plans list: sorting, per-session statuses, and the payloads pushed to
// the frontend after every transition, activity, or title change.
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use tauri::AppHandle;

use crate::plans;
use crate::types::{
    AppEvent, OpenRepoResult, PlanEntry, PlansUpdate, SessionKey, SessionRole, SessionStatusView,
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
        );
        let selected = most_recent_key(&plans).unwrap_or_else(|| SessionKey {
            plan: plans
                .first()
                .map(|entry| entry.name.clone())
                .unwrap_or_default(),
            role: SessionRole::Scoping,
        });
        OpenRepoResult {
            repo_root: repo_root.to_string_lossy().to_string(),
            branch,
            plans,
            selected,
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
        PlansUpdate { plans, selected }
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
        })
        .collect()
}

/// One status row per session a plan owns: its scoping session, plus its
/// execution session once approved.
pub(crate) fn session_statuses(
    repo_root: &Path,
    plan: &plans::PlanRef,
    sessions: &HashMap<SessionKey, LiveSession>,
) -> Vec<SessionStatusView> {
    let mut roles = vec![SessionRole::Scoping];
    if plans::has_execution(repo_root, plan) {
        roles.push(SessionRole::Executing);
    }
    roles
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

/// Most-recent session across the sorted plans: the execution session when
/// the plan owns one, else scoping.
pub(crate) fn most_recent_key(plans: &[PlanEntry]) -> Option<SessionKey> {
    plans.first().map(|entry| {
        let role = entry
            .sessions
            .iter()
            .find(|status| status.role == SessionRole::Executing)
            .map(|_| SessionRole::Executing)
            .unwrap_or(SessionRole::Scoping);
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
