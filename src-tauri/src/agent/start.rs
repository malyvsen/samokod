// Session-start coordination: one owner for the `warming` /
// `history_loading` / `history_loaded` claims, one generic wait, and
// named timeouts. Every claim has a guard whose drop settles it.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::types::SessionKey;

use super::{State, lock_state};

/// Bounds the `ensure_live` wait for a racing warm to settle: cold
/// spawns cover process start, initialize, new session, mode pin, and
/// model/effort reapply, and the racing prompt already tolerates 30s in
/// `wait_for_history`, so anything tighter becomes the failure link.
pub(crate) const ENSURE_LIVE_TIMEOUT: Duration = Duration::from_secs(60);
/// Bounds history replay waits; prompts block this long before refusing.
pub(crate) const HISTORY_TIMEOUT: Duration = Duration::from_secs(30);
/// Bounds background warm waits for the connection to settle before replay.
pub(crate) const WARM_TIMEOUT: Duration = Duration::from_secs(30);

/// Poll interval for every start wait.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Wait until `predicate` holds, polling on a fixed tick. True when the
/// predicate held within `timeout`, false on expiry. Never holds the
/// state lock across a sleep; the predicate locks itself.
pub(crate) async fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    if predicate() {
        return true;
    }
    let start = std::time::Instant::now();
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        if predicate() {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
    }
}

impl State {
    /// True while a history replay runs for the key. Prompts wait on this;
    /// replayed notifications and approvals render on this.
    pub(crate) fn is_history_loading(&self, key: &SessionKey) -> bool {
        self.history_loading.contains(key)
    }

    /// True once a history replay started or finished for the key. The
    /// transcript owns the key from here on: warms skip it, replays never
    /// run twice, prompts wait for it.
    pub(crate) fn history_started(&self, key: &SessionKey) -> bool {
        self.history_loading.contains(key) || self.history_loaded.contains(key)
    }

    /// True while a background warm runs for the key.
    pub(crate) fn is_warming(&self, key: &SessionKey) -> bool {
        self.warming.contains(key)
    }

    /// Claim the warm slot. Pure `warming` single-flight. Pure.
    pub(crate) fn claim_warm(&mut self, key: &SessionKey) -> bool {
        self.warming.insert(key.clone())
    }

    /// Release the warm slot. Pure.
    pub(crate) fn release_warm(&mut self, key: &SessionKey) {
        self.warming.remove(key);
    }

    /// Claim the history slot. False when the replay already ran, another
    /// replay is in flight, or a live turn owns the session. A racing warm
    /// does not block the claim; callers wait for it to settle instead.
    /// Pure.
    pub(crate) fn claim_history(&mut self, key: &SessionKey) -> bool {
        if self.history_started(key) || self.is_working(key) {
            return false;
        }
        self.history_loading.insert(key.clone())
    }

    /// Settle a finished replay: off the in-flight set, onto the replayed
    /// set. Pure.
    pub(crate) fn finish_history(&mut self, key: &SessionKey) {
        self.history_loading.remove(key);
        self.history_loaded.insert(key.clone());
    }

    /// Release a failed replay so a retry can claim it again. Pure.
    pub(crate) fn abort_history(&mut self, key: &SessionKey) {
        self.history_loading.remove(key);
    }

    /// Drop every start single-flight entry. Repo opens start from scratch.
    pub(crate) fn clear_start_claims(&mut self) {
        self.warming.clear();
        self.history_loading.clear();
        self.history_loaded.clear();
    }

    /// Drop one key from every start set so a vanished pending key cannot
    /// block its timestamp namesake.
    pub(crate) fn drop_start_claims_for(&mut self, key: &SessionKey) {
        self.warming.remove(key);
        self.history_loading.remove(key);
        self.history_loaded.remove(key);
    }
}

/// RAII warm claim: releases on drop so every early return settles it.
pub(crate) struct WarmGuard {
    inner: Option<(Arc<Mutex<State>>, SessionKey)>,
}

impl WarmGuard {
    /// Claim the warm slot, returning the guard on success.
    pub(crate) fn claim(state: &Arc<Mutex<State>>, key: &SessionKey) -> Option<Self> {
        let claimed = lock_state(state)
            .map(|mut guard| guard.claim_warm(key))
            .unwrap_or(false);
        if claimed {
            Some(WarmGuard {
                inner: Some((Arc::clone(state), key.clone())),
            })
        } else {
            None
        }
    }

    /// True while the guard still holds its claim. Test helper.
    #[cfg(test)]
    pub(crate) fn is_held(&self) -> bool {
        self.inner.is_some()
    }
}

impl Drop for WarmGuard {
    fn drop(&mut self) {
        if let Some((state, key)) = self.inner.take()
            && let Some(mut guard) = lock_state(&state)
        {
            guard.release_warm(&key);
        }
    }
}

/// RAII history claim: explicit `finish` / `fail` consume the guard,
/// drop defaults to abort so a missing repo never wedges the key.
pub(crate) struct HistoryGuard {
    inner: Option<(Arc<Mutex<State>>, SessionKey)>,
}

impl HistoryGuard {
    /// Claim the history slot, returning the guard on success.
    pub(crate) fn claim(state: &Arc<Mutex<State>>, key: &SessionKey) -> Option<Self> {
        let claimed = lock_state(state)
            .map(|mut guard| guard.claim_history(key))
            .unwrap_or(false);
        if claimed {
            Some(HistoryGuard {
                inner: Some((Arc::clone(state), key.clone())),
            })
        } else {
            None
        }
    }

    /// Settle a finished replay, consuming the guard.
    pub(crate) fn finish(mut self) {
        if let Some((state, key)) = self.inner.take()
            && let Some(mut guard) = lock_state(&state)
        {
            guard.finish_history(&key);
        }
    }

    /// Release a failed replay, consuming the guard.
    pub(crate) fn fail(mut self) {
        if let Some((state, key)) = self.inner.take()
            && let Some(mut guard) = lock_state(&state)
        {
            guard.abort_history(&key);
        }
    }

    /// True while the guard still holds its claim. Test helper.
    #[cfg(test)]
    pub(crate) fn is_held(&self) -> bool {
        self.inner.is_some()
    }
}

impl Drop for HistoryGuard {
    fn drop(&mut self) {
        if let Some((state, key)) = self.inner.take()
            && let Some(mut guard) = lock_state(&state)
        {
            guard.abort_history(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::session::ActivePlan;
    use crate::types::SessionRole;

    fn key() -> SessionKey {
        SessionKey {
            plan: "2026-09-26.08-52-57".to_string(),
            role: SessionRole::Scoping,
        }
    }

    #[test]
    fn claim_is_single_flight() {
        let mut state = State::default();
        let key = key();
        assert!(state.claim_warm(&key));
        assert!(!state.claim_warm(&key));
    }

    #[test]
    fn release_allows_reclaim_after_failure() {
        let mut state = State::default();
        let key = key();
        assert!(state.claim_warm(&key));
        state.release_warm(&key);
        assert!(state.claim_warm(&key));
    }

    #[test]
    fn warm_claims_during_history() {
        let mut state = State::default();
        let key = key();
        assert!(state.claim_history(&key));
        assert!(state.claim_warm(&key));
        assert!(state.history_started(&key));
    }

    #[test]
    fn history_refused_during_turn() {
        let mut state = State::default();
        let key = key();
        state.sessions.insert(
            key.clone(),
            crate::agent::LiveSession::fresh(
                ActivePlan::scoping(key.plan.clone()),
                crate::repo_state::RepoState::default(),
            ),
        );
        state.set_working(&key, true);
        assert!(!state.claim_history(&key));
    }

    #[test]
    fn history_claims_during_warm() {
        let mut state = State::default();
        let key = key();
        assert!(state.claim_warm(&key));
        assert!(state.claim_history(&key));
    }

    #[test]
    fn empty_path_holds_both_guards() {
        let state = Arc::new(Mutex::new(State::default()));
        let key = key();
        let history = HistoryGuard::claim(&state, &key).expect("history");
        assert!(history.is_held());
        let warm = WarmGuard::claim(&state, &key).expect("warm");
        assert!(warm.is_held());
        assert!(
            lock_state(&state)
                .map(|guard| guard.is_warming(&key) && guard.is_history_loading(&key))
                .unwrap_or(false)
        );
        drop(warm);
        assert!(
            lock_state(&state)
                .map(|guard| !guard.is_warming(&key) && guard.is_history_loading(&key))
                .unwrap_or(false)
        );
        history.finish();
        assert!(
            lock_state(&state)
                .map(|guard| !guard.is_history_loading(&key) && guard.history_started(&key))
                .unwrap_or(false)
        );
    }

    #[test]
    fn warm_guard_releases_on_drop() {
        let state = Arc::new(Mutex::new(State::default()));
        let key = key();
        for _ in 0..2 {
            let guard = WarmGuard::claim(&state, &key).expect("claim");
            assert!(guard.is_held());
            assert!(
                lock_state(&state)
                    .map(|guard| guard.is_warming(&key))
                    .unwrap_or(false)
            );
            drop(guard);
            assert!(
                !lock_state(&state)
                    .map(|guard| guard.is_warming(&key))
                    .unwrap_or(true)
            );
        }
    }

    #[test]
    fn history_guard_aborts_on_drop() {
        let state = Arc::new(Mutex::new(State::default()));
        let key = key();
        {
            let guard = HistoryGuard::claim(&state, &key).expect("claim");
            assert!(guard.is_held());
            assert!(
                lock_state(&state)
                    .map(|guard| guard.is_history_loading(&key))
                    .unwrap_or(false)
            );
        }
        assert!(
            !lock_state(&state)
                .map(|guard| guard.is_history_loading(&key))
                .unwrap_or(true)
        );
        assert!(HistoryGuard::claim(&state, &key).is_some());
    }
}
