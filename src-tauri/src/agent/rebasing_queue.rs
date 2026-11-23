// Rebasing queue order: oldest-first waiters in `rebasing/`, one agent.
// The pump moves diverged plans into `rebasing/` in arrival order; only the
// head runs the rebasing agent, the rest hold with no live session. This
// module owns that order plus the shared held-guard: moves, spawns,
// rebases, and all no-spawn guards branch on it.
use std::path::Path;

use crate::plans::{self, Phase};
use crate::types::{SessionKey, SessionRole};

/// Where one plan sits in the rebasing queue. No bools: callers match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RebasingPosition {
    NotRebasing,
    Head,
    Queued,
}

/// Queue position for one plan name. Scans `rebasing/` once, resolves
/// arrivals via `plans::arrival_ms`, oldest first.
pub(crate) fn position(repo_root: &Path, plan_name: &str) -> RebasingPosition {
    position_in(&sorted_rebasing_names(repo_root), plan_name)
}

/// True when the plan waits behind the head.
pub(crate) fn is_queued(repo_root: &Path, plan_name: &str) -> bool {
    is_queued_in(&sorted_rebasing_names(repo_root), plan_name)
}

/// True when a rebasing session must not mint a live agent: it waits behind
/// the head. Warm, prompts, and history share this instead of ad-hoc
/// role-plus-scan checks.
pub(crate) fn is_held(repo_root: &Path, session: &SessionKey) -> bool {
    session.role == SessionRole::Rebasing && is_queued(repo_root, &session.plan)
}

/// Rebasing names oldest first: `(arrival, name)` order, unstamped last.
pub(crate) fn sorted_rebasing_names(repo_root: &Path) -> Vec<String> {
    let entries: Vec<(String, i64)> = plans::scan_plans(repo_root)
        .into_iter()
        .filter(|plan| plan.phase == Phase::Rebasing)
        .map(|plan| {
            let arrival = plans::arrival_ms(
                repo_root,
                &plans::PlanRef {
                    name: plan.name.clone(),
                    phase: Phase::Rebasing,
                },
            )
            .unwrap_or(i64::MAX);
            (plan.name, arrival)
        })
        .collect();
    sort_names(entries)
}

/// Position inside a precomputed queue. Pure.
pub(crate) fn position_in(queue: &[String], plan_name: &str) -> RebasingPosition {
    if head(queue) == Some(plan_name) {
        return RebasingPosition::Head;
    }
    if queue.iter().any(|name| name == plan_name) {
        return RebasingPosition::Queued;
    }
    RebasingPosition::NotRebasing
}

/// True when the plan waits behind the head of a precomputed queue. Pure.
pub(crate) fn is_queued_in(queue: &[String], plan_name: &str) -> bool {
    position_in(queue, plan_name) == RebasingPosition::Queued
}

/// Oldest first by `(arrival, name)`. Pure.
fn sort_names(mut entries: Vec<(String, i64)>) -> Vec<String> {
    entries.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    entries.into_iter().map(|(name, _)| name).collect()
}

/// Oldest entry in a precomputed queue. Pure.
fn head(queue: &[String]) -> Option<&str> {
    queue.first().map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebasing_dir(root: &Path, name: &str, entered_at: i64) {
        let plan = plans::PlanRef {
            name: name.to_string(),
            phase: plans::Phase::Rebasing,
        };
        std::fs::create_dir_all(plan.path(root)).expect("mkdir");
        std::fs::write(plan.path(root).join("plan.md"), "# T\n").expect("write");
        let mut state = plans::load_state(&plan.path(root));
        state.entered_at = Some(entered_at);
        plans::store_state(&plan.path(root), &state);
    }

    fn rebasing_session(plan: &str) -> SessionKey {
        SessionKey {
            plan: plan.to_string(),
            role: SessionRole::Rebasing,
        }
    }

    #[test]
    fn empty_has_no_head() {
        let queue: Vec<String> = sort_names(Vec::new());
        assert_eq!(head(&queue), None);
        assert_eq!(
            position_in(&queue, "2026-10-11.02-00-00"),
            RebasingPosition::NotRebasing
        );
    }

    #[test]
    fn single_rebasing_is_head() {
        let queue = sort_names(vec![("2026-10-11.02-00-00".to_string(), 100)]);
        assert_eq!(head(&queue), Some("2026-10-11.02-00-00"));
        assert_eq!(
            position_in(&queue, "2026-10-11.02-00-00"),
            RebasingPosition::Head
        );
    }

    #[test]
    fn oldest_arrival_wins() {
        let queue = sort_names(vec![("newer".to_string(), 200), ("older".to_string(), 100)]);
        assert_eq!(queue, vec!["older".to_string(), "newer".to_string()]);
        assert_eq!(position_in(&queue, "older"), RebasingPosition::Head);
        assert_eq!(position_in(&queue, "newer"), RebasingPosition::Queued);
    }

    #[test]
    fn name_breaks_arrival_ties() {
        let queue = sort_names(vec![
            ("b-plan".to_string(), 100),
            ("a-plan".to_string(), 100),
        ]);
        assert_eq!(queue, vec!["a-plan".to_string(), "b-plan".to_string()]);
        assert_eq!(position_in(&queue, "a-plan"), RebasingPosition::Head);
        assert_eq!(position_in(&queue, "b-plan"), RebasingPosition::Queued);
    }

    #[test]
    fn non_rebasing_is_not_rebasing() {
        let queue = sort_names(vec![("a-plan".to_string(), 100)]);
        assert_eq!(
            position_in(&queue, "other-plan"),
            RebasingPosition::NotRebasing
        );
    }

    #[test]
    fn disk_queue_marks_head_queued_and_outsiders() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        rebasing_dir(root, "2026-10-11.02-00-00.old", 100);
        rebasing_dir(root, "2026-10-11.02-00-01.new", 200);
        let queue = sorted_rebasing_names(root);
        assert_eq!(
            queue,
            vec![
                "2026-10-11.02-00-00.old".to_string(),
                "2026-10-11.02-00-01.new".to_string()
            ]
        );
        assert_eq!(
            position(root, "2026-10-11.02-00-00.old"),
            RebasingPosition::Head
        );
        assert_eq!(
            position(root, "2026-10-11.02-00-01.new"),
            RebasingPosition::Queued
        );
        assert_eq!(position(root, "missing"), RebasingPosition::NotRebasing);
        assert!(!is_queued(root, "2026-10-11.02-00-00.old"));
        assert!(is_queued(root, "2026-10-11.02-00-01.new"));
        assert!(is_queued_in(&queue, "2026-10-11.02-00-01.new"));
        assert!(!is_held(root, &rebasing_session("2026-10-11.02-00-00.old")));
        assert!(is_held(root, &rebasing_session("2026-10-11.02-00-01.new")));
        assert!(!is_held(
            root,
            &SessionKey {
                plan: "2026-10-11.02-00-01.new".to_string(),
                role: SessionRole::Executing,
            }
        ));
    }
}
