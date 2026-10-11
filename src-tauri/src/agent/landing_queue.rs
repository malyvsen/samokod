// Landing queue order: oldest-first waiters in `landing/`, one agent.
// The pump moves diverged plans into `landing/` in arrival order; only the
// head runs the landing agent, the rest hold with no live session. This
// module owns that order: pure core over `(name, arrival)` pairs plus a
// thin disk wrapper. Moves, spawns, rebases, and guards all share it.
use std::path::Path;

use crate::plans::{self, Phase};

/// Where one plan sits in the landing queue. No bools: callers match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LandingPosition {
    NotLanding,
    Head,
    Queued,
}

/// Queue position for one plan name. Scans `landing/` once, resolves
/// arrivals via `plans::arrival_ms`, oldest first.
pub(crate) fn position(repo_root: &Path, plan_name: &str) -> LandingPosition {
    position_in(&sorted_landing_names(repo_root), plan_name)
}

/// True when the plan waits behind the head. Guards (warm, prompts,
/// history) share this instead of ad-hoc scans.
pub(crate) fn is_queued(repo_root: &Path, plan_name: &str) -> bool {
    position(repo_root, plan_name) == LandingPosition::Queued
}

/// Landing names oldest first: `(arrival, name)` order, unstamped last.
pub(crate) fn sorted_landing_names(repo_root: &Path) -> Vec<String> {
    let entries: Vec<(String, i64)> = plans::scan_plans(repo_root)
        .into_iter()
        .filter(|plan| plan.phase == Phase::Landing)
        .map(|plan| {
            let arrival = plans::arrival_ms(
                repo_root,
                &plans::PlanRef {
                    name: plan.name.clone(),
                    phase: Phase::Landing,
                },
            )
            .unwrap_or(i64::MAX);
            (plan.name, arrival)
        })
        .collect();
    sort_names(entries)
}

/// Position inside a precomputed queue. Pure.
pub(crate) fn position_in(queue: &[String], plan_name: &str) -> LandingPosition {
    if !queue.contains(&plan_name.to_string()) {
        return LandingPosition::NotLanding;
    }
    if head(queue) == Some(plan_name) {
        LandingPosition::Head
    } else {
        LandingPosition::Queued
    }
}

/// Oldest first by `(arrival, name)`. Pure.
pub(crate) fn sort_names(mut entries: Vec<(String, i64)>) -> Vec<String> {
    entries.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    entries.into_iter().map(|(name, _)| name).collect()
}

/// Oldest entry in a precomputed queue. Pure.
pub(crate) fn head(queue: &[String]) -> Option<&str> {
    queue.first().map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_has_no_head() {
        let queue: Vec<String> = sort_names(Vec::new());
        assert_eq!(head(&queue), None);
        assert_eq!(
            position_in(&queue, "2026-10-11.02-00-00"),
            LandingPosition::NotLanding
        );
    }

    #[test]
    fn single_landing_is_head() {
        let queue = sort_names(vec![("2026-10-11.02-00-00".to_string(), 100)]);
        assert_eq!(head(&queue), Some("2026-10-11.02-00-00"));
        assert_eq!(
            position_in(&queue, "2026-10-11.02-00-00"),
            LandingPosition::Head
        );
    }

    #[test]
    fn oldest_arrival_wins() {
        let queue = sort_names(vec![("newer".to_string(), 200), ("older".to_string(), 100)]);
        assert_eq!(queue, vec!["older".to_string(), "newer".to_string()]);
        assert_eq!(position_in(&queue, "older"), LandingPosition::Head);
        assert_eq!(position_in(&queue, "newer"), LandingPosition::Queued);
    }

    #[test]
    fn name_breaks_arrival_ties() {
        let queue = sort_names(vec![
            ("b-plan".to_string(), 100),
            ("a-plan".to_string(), 100),
        ]);
        assert_eq!(queue, vec!["a-plan".to_string(), "b-plan".to_string()]);
        assert_eq!(position_in(&queue, "a-plan"), LandingPosition::Head);
        assert_eq!(position_in(&queue, "b-plan"), LandingPosition::Queued);
    }

    #[test]
    fn non_landing_is_not_landing() {
        let queue = sort_names(vec![("a-plan".to_string(), 100)]);
        assert_eq!(
            position_in(&queue, "other-plan"),
            LandingPosition::NotLanding
        );
    }
}
