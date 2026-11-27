// Background warm: eager `ensure_live` so MODEL/EFFORT pickers turn
// interactive before the first prompt. Single-flight per session key.
// Queued plans never warm: they own no live session until landed.
use crate::plans;
use crate::types::{AgentError, SessionKey};

use super::AgentManager;
use super::start::WarmGuard;

impl AgentManager {
    /// Warm one session in the background: `ensure_live` without a prompt.
    /// Finished phases return early. History replays imply their warm, so
    /// started histories skip; live sessions skip since the warm would fork
    /// a fresh session beside them. Failures log and release so the next
    /// explicit prompt retries through the existing error path.
    pub async fn warm_session(&self, window: &str, session: SessionKey) -> Result<(), AgentError> {
        log::info!(
            "warm_session window={window} session={}::{:?}",
            session.plan,
            session.role
        );
        let Some((repo_root, _)) = self.reopen_snapshot(window) else {
            log::debug!("skipping warm window={window}: no repository open");
            return Ok(());
        };
        if plans::is_finished(&repo_root, &session.plan) {
            log::debug!(
                "skipping warm window={window} for {} {:?}: finished plan",
                session.plan,
                session.role
            );
            return Ok(());
        }
        if super::queue::is_queued(&repo_root, &session) {
            log::debug!(
                "skipping warm window={window} for {} {:?}: queued plan holds",
                session.plan,
                session.role
            );
            return Ok(());
        }
        if self.live_snapshot_for(window, &session).is_some() {
            log::debug!(
                "skipping warm window={window} for {} {:?}: already live",
                session.plan,
                session.role
            );
            return Ok(());
        }
        if self
            .with_window(window, |win| win.history_started(&session))
            .unwrap_or(false)
        {
            log::debug!(
                "skipping warm window={window} for {} {:?}: history started, transcript owns the key",
                session.plan,
                session.role
            );
            return Ok(());
        }
        let Some(_guard) = WarmGuard::claim(&self.state, window, &session) else {
            log::debug!(
                "skipping warm window={window} for {} {:?}: warm already in flight",
                session.plan,
                session.role
            );
            return Ok(());
        };
        let result = self.ensure_live_claimed(window, &session).await;
        if let Err(error) = &result {
            log::warn!(
                "failed to warm window={window} session={}::{:?}: {error}",
                session.plan,
                session.role
            );
        }
        result.map(|_| ())
    }
}
