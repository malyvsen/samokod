// Per-session state keyed by plan plus role: OpenCode session IDs and
// last-seen costs. `state.json` is the sole source of truth; missing IDs
// mean no past to replay. Storage lives in `plans::PlanState`, so writes
// preserve `entered_at`.
use std::path::Path;

use crate::types::SessionKey;

/// Resolved ACP session ID for one session key, across all phases.
/// Read-only lookup of the persisted ID; `None` means a brand-new plan
/// with no past to replay.
pub(crate) fn resolve(repo_root: &Path, session: &SessionKey) -> Option<String> {
    let plan_dir = crate::plans::locate(repo_root, &session.plan)?;
    let state = crate::plans::load_state(&plan_dir);
    state.session(session.role).map(str::to_string)
}

/// Record one session's OpenCode ID inside its plan directory. The next
/// prompt heals anything skipped here.
pub(crate) fn record_session(repo_root: &Path, key: &SessionKey, session_id: &str) {
    update_state(repo_root, &key.plan, |state| {
        if state.session(key.role) == Some(session_id) {
            return false;
        }
        state.set_session(key.role, session_id.to_string());
        true
    });
}

/// Record one session's last-seen cost inside its plan directory, so the
/// plans list shows the real total without a live session.
pub(crate) fn record_cost(repo_root: &Path, key: &SessionKey, cost: f64) {
    update_state(repo_root, &key.plan, |state| {
        if state.cost(key.role) == cost {
            return false;
        }
        state.set_cost(key.role, cost);
        true
    });
}

/// Mark one plan as prompted for the empty-scoping gate. The flag travels
/// with renames inside the plan directory.
pub(crate) fn mark_prompted(repo_root: &Path, plan_name: &str) {
    update_state(repo_root, plan_name, |state| {
        if state.prompted {
            return false;
        }
        state.prompted = true;
        true
    });
}

/// Best-effort state rewrite inside a plan directory, preserving every
/// field the mutation leaves alone. Missing directories (a warmed pending
/// session spawns before its directory exists) skip quietly, failures log
/// and the live session continues, and a mutation reporting no change
/// skips the rewrite.
fn update_state(
    repo_root: &Path,
    plan_name: &str,
    mutate: impl FnOnce(&mut crate::plans::PlanState) -> bool,
) {
    let Some(plan_dir) = crate::plans::locate(repo_root, plan_name) else {
        return;
    };
    let mut state = crate::plans::load_state(&plan_dir);
    if mutate(&mut state) {
        crate::plans::store_state(&plan_dir, &state);
    }
}

/// True when a plan name exists in any phase. Name-based: moves
/// rename rather than copy, so names stay unique across phases. Pure
/// except the directory probes.
pub(crate) fn plan_exists(repo_root: &Path, plan_name: &str) -> bool {
    crate::plans::locate(repo_root, plan_name).is_some()
}

/// True when a plan name lives in a finished phase. Finished rows stay
/// read-only and never warm or spawn. Pure except the directory probes.
pub(crate) fn is_finished(repo_root: &Path, plan_name: &str) -> bool {
    for phase in [
        crate::plans::Phase::Completed,
        crate::plans::Phase::Cancelled,
    ] {
        let candidate = crate::plans::PlanRef {
            name: plan_name.to_string(),
            phase,
        }
        .path(repo_root);
        if candidate.is_dir() {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plans::{self, Phase, PlanRef};
    use crate::types::SessionRole;
    use std::path::PathBuf;

    fn key(plan: &str, role: SessionRole) -> SessionKey {
        SessionKey {
            plan: plan.to_string(),
            role,
        }
    }

    fn plan_dir(root: &Path, phase: Phase, name: &str) -> PathBuf {
        let plan = PlanRef {
            name: name.to_string(),
            phase,
        };
        std::fs::create_dir_all(plan.path(root)).expect("mkdir");
        plan.path(root)
    }

    #[test]
    fn missing_file_yields_no_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = plan_dir(dir.path(), Phase::Scoping, "2026-09-30.10-00-00");
        let state = plans::load_state(&plan);
        assert_eq!(state.session(SessionRole::Scoping), None);
        assert_eq!(state.session(SessionRole::Executing), None);
        assert_eq!(state.session(SessionRole::Evergreening), None);
        assert_eq!(state.session(SessionRole::Landing), None);
    }

    #[test]
    fn round_trip_preserves_all_roles() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        plan_dir(dir.path(), Phase::Scoping, name);
        record_session(dir.path(), &key(name, SessionRole::Scoping), "ses_scoping");
        record_session(
            dir.path(),
            &key(name, SessionRole::Executing),
            "ses_executing",
        );
        record_session(dir.path(), &key(name, SessionRole::Landing), "ses_landing");
        let plan = PlanRef {
            name: name.to_string(),
            phase: Phase::Scoping,
        };
        let state = plans::load_state(&plan.path(dir.path()));
        assert_eq!(state.session(SessionRole::Scoping), Some("ses_scoping"));
        assert_eq!(state.session(SessionRole::Executing), Some("ses_executing"));
        assert_eq!(state.session(SessionRole::Landing), Some("ses_landing"));
    }

    #[test]
    fn overwrite_keeps_other_roles() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        plan_dir(dir.path(), Phase::Scoping, name);
        record_session(dir.path(), &key(name, SessionRole::Scoping), "ses_old");
        record_session(dir.path(), &key(name, SessionRole::Executing), "ses_exec");
        record_session(dir.path(), &key(name, SessionRole::Scoping), "ses_new");
        let plan = PlanRef {
            name: name.to_string(),
            phase: Phase::Scoping,
        };
        let state = plans::load_state(&plan.path(dir.path()));
        assert_eq!(state.session(SessionRole::Scoping), Some("ses_new"));
        assert_eq!(state.session(SessionRole::Executing), Some("ses_exec"));
    }

    #[test]
    fn corrupt_file_yields_no_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = plan_dir(dir.path(), Phase::Scoping, "2026-09-30.10-00-00");
        std::fs::write(plan.join(plans::STATE_FILE), "{ not json").expect("write corrupt");
        let state = plans::load_state(&plan);
        assert_eq!(state.session(SessionRole::Scoping), None);
    }

    #[test]
    fn plan_exists_covers_all_phases() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        for phase in [
            Phase::Scoping,
            Phase::Executing,
            Phase::Evergreening,
            Phase::Landing,
            Phase::Completed,
            Phase::Cancelled,
        ] {
            let name = format!("plan-{}", phase.dir_name());
            plan_dir(dir.path(), phase, &name);
            assert!(plan_exists(dir.path(), &name), "{phase:?} exists");
        }
        assert!(!plan_exists(dir.path(), "missing"));
    }

    #[test]
    fn is_finished_only_for_completed_and_cancelled() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        for phase in [
            Phase::Scoping,
            Phase::Executing,
            Phase::Evergreening,
            Phase::Landing,
        ] {
            let name = format!("active-{}", phase.dir_name());
            plan_dir(dir.path(), phase, &name);
            assert!(!is_finished(dir.path(), &name), "{phase:?} active");
        }
        for phase in [Phase::Completed, Phase::Cancelled] {
            let name = format!("done-{}", phase.dir_name());
            plan_dir(dir.path(), phase, &name);
            assert!(is_finished(dir.path(), &name), "{phase:?} finished");
        }
        assert!(!is_finished(dir.path(), "missing"));
    }

    #[test]
    fn missing_dir_records_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        record_session(
            dir.path(),
            &key("2026-09-30.10-00-00", SessionRole::Scoping),
            "ses_pending",
        );
        let plan = PlanRef {
            name: "2026-09-30.10-00-00".to_string(),
            phase: Phase::Scoping,
        };
        assert!(!plan.path(dir.path()).join(plans::STATE_FILE).is_file());
    }

    #[test]
    fn missing_dir_marks_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        mark_prompted(dir.path(), "2026-09-30.10-00-00");
        let plan = PlanRef {
            name: "2026-09-30.10-00-00".to_string(),
            phase: Phase::Scoping,
        };
        assert!(!plan.path(dir.path()).join(plans::STATE_FILE).is_file());
    }

    #[test]
    fn already_prompted_skips_rewrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        plan_dir(dir.path(), Phase::Scoping, name);
        mark_prompted(dir.path(), name);
        let plan = PlanRef {
            name: name.to_string(),
            phase: Phase::Scoping,
        };
        let path = plan.path(dir.path()).join(plans::STATE_FILE);
        let before = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        std::thread::sleep(std::time::Duration::from_millis(10));
        mark_prompted(dir.path(), name);
        let after = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        assert_eq!(before, after);
    }

    #[test]
    fn same_id_skips_rewrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        plan_dir(dir.path(), Phase::Scoping, name);
        record_session(dir.path(), &key(name, SessionRole::Scoping), "ses_same");
        let plan = PlanRef {
            name: name.to_string(),
            phase: Phase::Scoping,
        };
        let path = plan.path(dir.path()).join(plans::STATE_FILE);
        let before = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        std::thread::sleep(std::time::Duration::from_millis(10));
        record_session(dir.path(), &key(name, SessionRole::Scoping), "ses_same");
        let after = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        assert_eq!(before, after);
    }

    #[test]
    fn session_record_preserves_entered_at() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        let scoping = crate::plans::materialize_scoping(dir.path(), name).expect("create");
        let stamped = crate::plans::load_state(&scoping.path(dir.path()))
            .entered_at
            .expect("stamped");
        record_session(dir.path(), &key(name, SessionRole::Scoping), "ses_scoping");
        record_session(dir.path(), &key(name, SessionRole::Executing), "ses_exec");
        let kept = crate::plans::load_state(&scoping.path(dir.path()));
        assert_eq!(kept.entered_at, Some(stamped));
        assert_eq!(kept.session(SessionRole::Scoping), Some("ses_scoping"));
        assert_eq!(kept.session(SessionRole::Executing), Some("ses_exec"));
    }

    #[test]
    fn cost_round_trip_per_role() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        plan_dir(dir.path(), Phase::Scoping, name);
        let roles = [
            SessionRole::Scoping,
            SessionRole::Executing,
            SessionRole::Evergreening,
            SessionRole::Landing,
        ];
        for (index, role) in roles.iter().enumerate() {
            record_cost(dir.path(), &key(name, *role), 1.5 * (index as f64 + 1.0));
        }
        let plan = PlanRef {
            name: name.to_string(),
            phase: Phase::Scoping,
        };
        let state = plans::load_state(&plan.path(dir.path()));
        for (index, role) in roles.iter().enumerate() {
            assert_eq!(state.cost(*role), 1.5 * (index as f64 + 1.0));
        }
    }

    #[test]
    fn cost_preserves_sessions_and_entered_at() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        let scoping = crate::plans::materialize_scoping(dir.path(), name).expect("create");
        let stamped = crate::plans::load_state(&scoping.path(dir.path()))
            .entered_at
            .expect("stamped");
        record_session(dir.path(), &key(name, SessionRole::Scoping), "ses_scoping");
        record_session(dir.path(), &key(name, SessionRole::Executing), "ses_exec");
        record_cost(dir.path(), &key(name, SessionRole::Scoping), 1.24);
        record_cost(dir.path(), &key(name, SessionRole::Executing), 2.5);
        let kept = crate::plans::load_state(&scoping.path(dir.path()));
        assert_eq!(kept.entered_at, Some(stamped));
        assert_eq!(kept.session(SessionRole::Scoping), Some("ses_scoping"));
        assert_eq!(kept.session(SessionRole::Executing), Some("ses_exec"));
        assert_eq!(kept.cost(SessionRole::Scoping), 1.24);
        assert_eq!(kept.cost(SessionRole::Executing), 2.5);
        assert_eq!(kept.cost(SessionRole::Landing), 0.0);
    }

    #[test]
    fn missing_dir_records_no_cost() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        record_cost(
            dir.path(),
            &key("2026-09-30.10-00-00", SessionRole::Scoping),
            1.24,
        );
        let plan = PlanRef {
            name: "2026-09-30.10-00-00".to_string(),
            phase: Phase::Scoping,
        };
        assert!(!plan.path(dir.path()).join(plans::STATE_FILE).is_file());
    }

    #[test]
    fn same_cost_skips_rewrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        plan_dir(dir.path(), Phase::Scoping, name);
        record_cost(dir.path(), &key(name, SessionRole::Scoping), 1.24);
        let plan = PlanRef {
            name: name.to_string(),
            phase: Phase::Scoping,
        };
        let path = plan.path(dir.path()).join(plans::STATE_FILE);
        let before = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        std::thread::sleep(std::time::Duration::from_millis(10));
        record_cost(dir.path(), &key(name, SessionRole::Scoping), 1.24);
        let after = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        assert_eq!(before, after);
    }

    #[test]
    fn finished_with_id_resolves_for_load() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        plan_dir(dir.path(), Phase::Completed, name);
        record_session(dir.path(), &key(name, SessionRole::Executing), "ses_exec");
        assert!(is_finished(dir.path(), name));
        assert_eq!(
            resolve(dir.path(), &key(name, SessionRole::Executing)),
            Some("ses_exec".to_string())
        );
    }

    #[test]
    fn finished_without_id_yields_none_without_spawning() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        plan_dir(dir.path(), Phase::Completed, name);
        assert!(is_finished(dir.path(), name));
        assert_eq!(
            resolve(dir.path(), &key(name, SessionRole::Executing)),
            None
        );
        let plan = PlanRef {
            name: name.to_string(),
            phase: Phase::Completed,
        };
        assert!(!plan.path(dir.path()).join(plans::STATE_FILE).is_file());
    }
}
