// Waiting line for finished work: oldest-first waiters in `queued/`,
// at most one plan in `rebasing/` or `evergreening/` at a time. The pump
// promotes queued plans while the mutex is free; queued plans hold with no
// live session. This module owns that order plus the shared queued-guard:
// warm, prompts, and history share it instead of ad-hoc phase checks.
use std::path::Path;

use crate::plans::{self, Phase};
use crate::types::SessionKey;

/// Queue position inside a precomputed queue: the index oldest-first, or
/// `None` when the plan is not queued. Pure.
pub(crate) fn position_in(queue: &[String], plan_name: &str) -> Option<usize> {
    queue.iter().position(|name| name == plan_name)
}

/// True when a session must not mint a live agent: its plan sits in
/// `queued/`, which owns no live session. Warm, prompts, and history share
/// this instead of ad-hoc phase checks.
pub(crate) fn is_queued(repo_root: &Path, session: &SessionKey) -> bool {
    plans::PlanRef {
        name: session.plan.clone(),
        phase: Phase::Queued,
    }
    .path(repo_root)
    .is_dir()
}

/// True when any plan sits in `rebasing/` or `evergreening/`. Promotion
/// from queued is gated on this, so at most one plan rebases at a time.
pub(crate) fn mutex_held(repo_root: &Path) -> bool {
    plans::scan_plans(repo_root)
        .iter()
        .any(|plan| matches!(plan.phase, Phase::Rebasing | Phase::Evergreening))
}

/// Queued names oldest first: `(arrival, name)` order, unstamped last.
pub(crate) fn sorted_names(repo_root: &Path) -> Vec<String> {
    let entries: Vec<(String, i64)> = plans::scan_plans(repo_root)
        .into_iter()
        .filter(|plan| plan.phase == Phase::Queued)
        .map(|plan| {
            let arrival = plans::arrival_ms(
                repo_root,
                &plans::PlanRef {
                    name: plan.name.clone(),
                    phase: Phase::Queued,
                },
            )
            .unwrap_or(i64::MAX);
            (plan.name, arrival)
        })
        .collect();
    sort_names(entries)
}

/// Oldest first by `(arrival, name)`. Pure.
fn sort_names(mut entries: Vec<(String, i64)>) -> Vec<String> {
    entries.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    entries.into_iter().map(|(name, _)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::types::SessionRole;

    fn queued_dir(root: &Path, name: &str, entered_at: i64) {
        let plan = plans::PlanRef {
            name: name.to_string(),
            phase: plans::Phase::Queued,
        };
        std::fs::create_dir_all(plan.path(root)).expect("mkdir");
        std::fs::write(plan.path(root).join("plan.md"), "# T\n").expect("write");
        let mut state = plans::load_state(&plan.path(root));
        state.entered_at = Some(entered_at);
        plans::store_state(&plan.path(root), &state);
    }

    fn queued_session(plan: &str) -> SessionKey {
        SessionKey {
            plan: plan.to_string(),
            role: SessionRole::Executing,
        }
    }

    #[test]
    fn empty_queue_has_no_positions() {
        let queue: Vec<String> = sort_names(Vec::new());
        assert_eq!(position_in(&queue, "2026-10-11.02-00-00"), None);
    }

    #[test]
    fn oldest_arrival_wins() {
        let queue = sort_names(vec![("newer".to_string(), 200), ("older".to_string(), 100)]);
        assert_eq!(queue, vec!["older".to_string(), "newer".to_string()]);
        assert_eq!(position_in(&queue, "older"), Some(0));
        assert_eq!(position_in(&queue, "newer"), Some(1));
    }

    #[test]
    fn name_breaks_arrival_ties() {
        let queue = sort_names(vec![
            ("b-plan".to_string(), 100),
            ("a-plan".to_string(), 100),
        ]);
        assert_eq!(queue, vec!["a-plan".to_string(), "b-plan".to_string()]);
        assert_eq!(position_in(&queue, "a-plan"), Some(0));
        assert_eq!(position_in(&queue, "b-plan"), Some(1));
    }

    #[test]
    fn outsiders_have_no_position() {
        let queue = sort_names(vec![("a-plan".to_string(), 100)]);
        assert_eq!(position_in(&queue, "other-plan"), None);
    }

    #[test]
    fn disk_queue_orders_oldest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        queued_dir(root, "2026-10-11.02-00-00.old", 100);
        queued_dir(root, "2026-10-11.02-00-01.new", 200);
        let queue = sorted_names(root);
        assert_eq!(
            queue,
            vec![
                "2026-10-11.02-00-00.old".to_string(),
                "2026-10-11.02-00-01.new".to_string()
            ]
        );
        assert_eq!(position_in(&queue, "2026-10-11.02-00-00.old"), Some(0));
        assert_eq!(position_in(&queue, "2026-10-11.02-00-01.new"), Some(1));
        assert_eq!(position_in(&queue, "missing"), None);
    }

    #[test]
    fn mutex_tracks_rebasing_and_evergreening() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        assert!(!mutex_held(root));
        queued_dir(root, "2026-10-11.02-00-00.waiting", 100);
        assert!(!mutex_held(root));
        let rebasing = plans::PlanRef {
            name: "2026-10-11.02-00-01.active".to_string(),
            phase: plans::Phase::Rebasing,
        };
        std::fs::create_dir_all(rebasing.path(root)).expect("mkdir");
        assert!(mutex_held(root));
        let greening = plans::PlanRef {
            name: "2026-10-11.02-00-02.green".to_string(),
            phase: plans::Phase::Evergreening,
        };
        std::fs::create_dir_all(greening.path(root)).expect("mkdir");
        assert!(mutex_held(root));
    }

    #[test]
    fn queued_plans_hold_without_live_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        queued_dir(root, "2026-10-11.02-00-01.new", 200);
        assert!(is_queued(root, &queued_session("2026-10-11.02-00-01.new")));
        assert!(!is_queued(root, &queued_session("missing")));
    }
}
