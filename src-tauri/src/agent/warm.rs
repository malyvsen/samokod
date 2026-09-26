// Background warm: eager `ensure_live` so MODEL/EFFORT pickers turn
// interactive before the first prompt. Single-flight per session key.
use std::collections::HashSet;
use std::path::Path;

use crate::plans;
use crate::types::{AgentError, SessionKey};

use super::AgentManager;
use super::lock_state;

/// Claim the warm slot. True when this caller owns the warm; false when
/// another warm is already in flight. Pure.
pub(crate) fn try_claim_warm(warming: &mut HashSet<SessionKey>, key: &SessionKey) -> bool {
    warming.insert(key.clone())
}

/// Release the warm slot. Pure.
pub(crate) fn release_warm(warming: &mut HashSet<SessionKey>, key: &SessionKey) {
    warming.remove(key);
}

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
    /// Finished phases return early. Failures log and release so the next
    /// explicit prompt retries through the existing error path.
    pub async fn warm_session(&self, session: SessionKey) -> Result<(), AgentError> {
        let repo_root = lock_state(&self.state).and_then(|state| state.repo_root.clone());
        let Some(repo_root) = repo_root else {
            return Ok(());
        };
        if is_finished(&repo_root, &session) {
            return Ok(());
        }
        if let Some((connection, _, _, _)) = self.session_snapshot_for(&session)
            && !connection.is_incoming_closed()
        {
            return Ok(());
        }
        let claimed = match self.state.lock() {
            Ok(mut state) => try_claim_warm(&mut state.warming, &session),
            Err(error) => {
                log::warn!("failed to claim warm session: {error}");
                return Ok(());
            }
        };
        if !claimed {
            return Ok(());
        }
        let result = self.ensure_live_claimed(&session).await;
        match self.state.lock() {
            Ok(mut state) => release_warm(&mut state.warming, &session),
            Err(error) => log::warn!("failed to release warm session: {error}"),
        }
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
    fn claim_is_single_flight() {
        let mut warming = HashSet::new();
        let key = key();
        assert!(try_claim_warm(&mut warming, &key));
        assert!(!try_claim_warm(&mut warming, &key));
    }

    #[test]
    fn release_allows_reclaim_after_failure() {
        let mut warming = HashSet::new();
        let key = key();
        assert!(try_claim_warm(&mut warming, &key));
        release_warm(&mut warming, &key);
        assert!(try_claim_warm(&mut warming, &key));
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
