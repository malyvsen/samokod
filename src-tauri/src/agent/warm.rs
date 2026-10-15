// Background warm: eager `ensure_live` so MODEL/EFFORT pickers turn
// interactive before the first prompt. Single-flight per session key.
// Queued landings never warm: only the head runs the agent.
use crate::plans;
use crate::types::{AgentError, SessionKey};

use super::AgentManager;
use super::lock_backend;
use super::start::WarmGuard;

impl AgentManager {
    /// Warm one session in the background: `ensure_live` without a prompt.
    /// Finished phases return early. History replays imply their warm, so
    /// started histories skip; live sessions skip since the warm would fork
    /// a fresh session beside them. Failures log and release so the next
    /// explicit prompt retries through the existing error path.
    pub async fn warm_session(&self, window: &str, session: SessionKey) -> Result<(), AgentError> {
        let repo_root = lock_backend(&self.state)
            .and_then(|backend| backend.windows.get(window)?.repo_root.clone());
        let Some(repo_root) = repo_root else {
            log::debug!("skipping warm: no repository open");
            return Ok(());
        };
        if plans::is_finished(&repo_root, &session.plan) {
            log::debug!(
                "skipping warm for {} {:?}: finished plan",
                session.plan,
                session.role
            );
            return Ok(());
        }
        if super::landing_queue::is_held(&repo_root, &session) {
            log::debug!(
                "skipping warm for {} {:?}: queued behind landing head",
                session.plan,
                session.role
            );
            return Ok(());
        }
        if let Some((connection, _, _, _)) = self.session_snapshot_for(window, &session)
            && !connection.is_incoming_closed()
        {
            log::debug!(
                "skipping warm for {} {:?}: already live",
                session.plan,
                session.role
            );
            return Ok(());
        }
        if lock_backend(&self.state)
            .map(|backend| {
                backend
                    .windows
                    .get(window)
                    .map(|win| win.history_started(&session))
                    .unwrap_or(false)
            })
            .unwrap_or(false)
        {
            log::debug!(
                "skipping warm for {} {:?}: history started, transcript owns the key",
                session.plan,
                session.role
            );
            return Ok(());
        }
        let Some(_guard) = WarmGuard::claim(&self.state, window, &session) else {
            log::debug!(
                "skipping warm for {} {:?}: warm already in flight",
                session.plan,
                session.role
            );
            return Ok(());
        };
        let result = self.ensure_live_claimed(window, &session).await;
        if let Err(error) = &result {
            log::warn!("failed to warm session {}: {error}", session.plan);
        }
        result.map(|_| ())
    }
}
