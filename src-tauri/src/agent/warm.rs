// Background warm: eager `ensure_live` so MODEL/EFFORT pickers turn
// interactive before the first prompt. Single-flight per session key.
use std::path::Path;

use crate::plans;
use crate::types::{AgentError, SessionKey};

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
    use super::*;
    use crate::types::SessionRole;

    fn key() -> SessionKey {
        SessionKey {
            plan: "2026-09-26.08-52-57".to_string(),
            role: SessionRole::Scoping,
        }
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
