// Background warm: eager `ensure_live` so MODEL/EFFORT pickers turn
// interactive before the first prompt. Single-flight per session key.
// Queued landings never warm: only the head runs the agent.
use std::path::Path;

use crate::plans;
use crate::types::{AgentError, SessionKey, SessionRole};

use super::AgentManager;
use super::lock_state;
use super::start::WarmGuard;

/// Finished plans never warm: completed/cancelled dirs hold the name.
fn is_finished(repo_root: &Path, key: &SessionKey) -> bool {
    for phase in [plans::Phase::Completed, plans::Phase::Cancelled] {
        let candidate = plans::PlanRef {
            name: key.plan.clone(),
            phase,
        };
        if candidate.path(repo_root).is_dir() {
            return true;
        }
    }
    false
}

impl AgentManager {
    /// Warm one session in the background: `ensure_live` without a prompt.
    /// Finished phases return early. History replays imply their warm, so
    /// started histories skip; live sessions skip since the warm would fork
    /// a fresh session beside them. Failures log and release so the next
    /// explicit prompt retries through the existing error path.
    pub async fn warm_session(&self, session: SessionKey) -> Result<(), AgentError> {
        let repo_root = lock_state(&self.state).and_then(|state| state.repo_root.clone());
        let Some(repo_root) = repo_root else {
            log::debug!("skipping warm: no repository open");
            return Ok(());
        };
        if is_finished(&repo_root, &session) {
            log::debug!(
                "skipping warm for {} {:?}: finished plan",
                session.plan,
                session.role
            );
            return Ok(());
        }
        if session.role == SessionRole::Landing
            && super::landing_queue::is_queued(&repo_root, &session.plan)
        {
            log::debug!(
                "skipping warm for {} {:?}: queued behind landing head",
                session.plan,
                session.role
            );
            return Ok(());
        }
        if let Some((connection, _, _, _)) = self.session_snapshot_for(&session)
            && !connection.is_incoming_closed()
        {
            log::debug!(
                "skipping warm for {} {:?}: already live",
                session.plan,
                session.role
            );
            return Ok(());
        }
        if lock_state(&self.state)
            .map(|state| state.history_started(&session))
            .unwrap_or(false)
        {
            log::debug!(
                "skipping warm for {} {:?}: history started, transcript owns the key",
                session.plan,
                session.role
            );
            return Ok(());
        }
        let Some(_guard) = WarmGuard::claim(&self.state, &session) else {
            log::debug!(
                "skipping warm for {} {:?}: warm already in flight",
                session.plan,
                session.role
            );
            return Ok(());
        };
        let result = self.ensure_live_claimed(&session).await;
        if let Err(error) = &result {
            log::warn!("failed to warm session {}: {error}", session.plan);
        }
        result.map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::super::landing_queue;
    use super::*;
    use crate::types::SessionRole;

    fn key() -> SessionKey {
        SessionKey {
            plan: "2026-09-26.08-52-57".to_string(),
            role: SessionRole::Scoping,
        }
    }

    fn landing_dir(root: &std::path::Path, name: &str, entered_at: i64) {
        let plan = plans::PlanRef {
            name: name.to_string(),
            phase: plans::Phase::Landing,
        };
        std::fs::create_dir_all(plan.path(root)).expect("mkdir");
        std::fs::write(plan.path(root).join("plan.md"), "# T\n").expect("write");
        let mut state = plans::load_state(&plan.path(root));
        state.entered_at = Some(entered_at);
        plans::store_state(&plan.path(root), &state);
    }

    #[test]
    fn queued_landings_skip_warm_without_live_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        plans::ensure_structure(root).expect("ensure");
        landing_dir(root, "2026-10-11.02-00-00.old", 100);
        landing_dir(root, "2026-10-11.02-00-01.new", 200);
        let head = SessionKey {
            plan: "2026-10-11.02-00-00.old".to_string(),
            role: SessionRole::Landing,
        };
        let queued = SessionKey {
            plan: "2026-10-11.02-00-01.new".to_string(),
            role: SessionRole::Landing,
        };
        assert!(!landing_queue::is_queued(root, &head.plan));
        assert!(landing_queue::is_queued(root, &queued.plan));
        // Guard mints no live session: early return before any spawn.
        assert!(
            queued.role == SessionRole::Landing && landing_queue::is_queued(root, &queued.plan)
        );
        assert!(!(head.role == SessionRole::Landing && landing_queue::is_queued(root, &head.plan)));
    }

    #[test]
    fn finished_needs_completed_or_cancelled_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let key = key();
        assert!(!is_finished(dir.path(), &key));
        for phase in [plans::Phase::Completed, plans::Phase::Cancelled] {
            let candidate = plans::PlanRef {
                name: key.plan.clone(),
                phase,
            };
            std::fs::create_dir_all(candidate.path(dir.path())).expect("mkdir");
            assert!(is_finished(dir.path(), &key));
            std::fs::remove_dir_all(candidate.path(dir.path())).expect("rmdir");
        }
    }
}
