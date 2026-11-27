// Session-start coordination: one owner for the `warming` /
// `history_loading` / `history_loaded` claims, one generic wait over
// shared flag snapshots, and named timeouts. Every claim has a guard
// whose drop settles it. All polling funnels through `wait_until` with
// one timeout source per wait (`ENSURE_LIVE_TIMEOUT`, `HISTORY_TIMEOUT`,
// `WARM_TIMEOUT`); callers never inline their own lock plus poll blocks.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::types::SessionKey;

use super::{BackendState, WindowState, lock_backend};

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

/// Snapshot: true while a background warm runs for the key. Short
/// critical section; closed windows and poisoned locks read as not
/// warming so waiters stop instead of hanging. Pure read.
pub(crate) fn is_warming(state: &Mutex<BackendState>, window: &str, key: &SessionKey) -> bool {
    has_flag(state, window, key, WindowState::is_warming)
}

/// Snapshot: true while a history replay runs for the key. Short
/// critical section; closed windows and poisoned locks read as not
/// loading so waiters stop instead of hanging. Pure read.
pub(crate) fn is_history_loading(
    state: &Mutex<BackendState>,
    window: &str,
    key: &SessionKey,
) -> bool {
    has_flag(state, window, key, WindowState::is_history_loading)
}

/// One flag read without creating the window: closed windows and
/// poisoned locks read as clear so waiters stop. Pure read.
fn has_flag(
    state: &Mutex<BackendState>,
    window: &str,
    key: &SessionKey,
    probe: impl FnOnce(&WindowState, &SessionKey) -> bool,
) -> bool {
    lock_backend(state)
        .map(|backend| {
            backend
                .windows
                .get(window)
                .map(|win| probe(win, key))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

/// Wait until no warm runs for the key. True when the slot cleared within
/// `timeout`, false on expiry. Callers pass `WARM_TIMEOUT` (history
/// replay waits on a racing warm) or the remaining `ENSURE_LIVE_TIMEOUT`
/// budget (live waits on a racing warm).
pub(crate) async fn wait_while_warming(
    state: &Mutex<BackendState>,
    window: &str,
    key: &SessionKey,
    timeout: Duration,
) -> bool {
    wait_while_flag(state, window, key, timeout, WindowState::is_warming).await
}

/// Wait until no history replay runs for the key. True when the replay
/// settled within `timeout`, false on expiry. Callers pass
/// `HISTORY_TIMEOUT`.
pub(crate) async fn wait_while_history_loading(
    state: &Mutex<BackendState>,
    window: &str,
    key: &SessionKey,
    timeout: Duration,
) -> bool {
    wait_while_flag(state, window, key, timeout, WindowState::is_history_loading).await
}

/// Wait until one start flag clears. Short critical section per poll;
/// closed windows read as clear so waiters stop. Pure polling.
async fn wait_while_flag(
    state: &Mutex<BackendState>,
    window: &str,
    key: &SessionKey,
    timeout: Duration,
    probe: impl Fn(&WindowState, &SessionKey) -> bool + Copy,
) -> bool {
    wait_until(timeout, || !has_flag(state, window, key, probe)).await
}

impl WindowState {
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

/// One held start slot: window plus key with the backend it settles in.
/// Never creates the window on claim or release: a closed window simply
/// fails the claim and drops the release.
type SlotHandle = (Arc<Mutex<BackendState>>, String, SessionKey);

/// Claim one start slot without creating the window. `None` means the
/// window closed or another owner holds the slot. Pure lock dance.
fn claim_slot(
    state: &Arc<Mutex<BackendState>>,
    window: &str,
    key: &SessionKey,
    claim: impl FnOnce(&mut WindowState) -> bool,
) -> Option<SlotHandle> {
    let claimed = lock_backend(state)
        .map(|mut backend| backend.claim_start_slot(window, claim))
        .unwrap_or(false);
    claimed.then(|| (Arc::clone(state), window.to_string(), key.clone()))
}

/// Settle one held slot without creating the window. Pure lock dance.
fn settle_slot(handle: Option<SlotHandle>, settle: impl FnOnce(&mut WindowState, &SessionKey)) {
    if let Some((state, window, key)) = handle
        && let Some(mut backend) = lock_backend(&state)
        && let Some(win) = backend.windows.get_mut(&window)
    {
        settle(win, &key);
    }
}

/// RAII warm claim: releases on drop so every early return settles it.
pub(crate) struct WarmGuard {
    inner: Option<SlotHandle>,
}

impl WarmGuard {
    /// Claim the warm slot, returning the guard on success. Never creates
    /// the window: a closed window cannot warm, so the claim fails.
    pub(crate) fn claim(
        state: &Arc<Mutex<BackendState>>,
        window: &str,
        key: &SessionKey,
    ) -> Option<Self> {
        claim_slot(state, window, key, |win| win.claim_warm(key))
            .map(|inner| WarmGuard { inner: Some(inner) })
    }

    /// True while the guard still holds its claim. Test helper.
    #[cfg(test)]
    pub(crate) fn is_held(&self) -> bool {
        self.inner.is_some()
    }
}

impl Drop for WarmGuard {
    fn drop(&mut self) {
        settle_slot(self.inner.take(), WindowState::release_warm);
    }
}

/// RAII history claim: explicit `finish` / `fail` consume the guard,
/// drop defaults to abort so a missing repo never wedges the key.
pub(crate) struct HistoryGuard {
    inner: Option<SlotHandle>,
}

impl HistoryGuard {
    /// Claim the history slot, returning the guard on success. Never creates
    /// the window: a closed window cannot replay, so the claim fails.
    pub(crate) fn claim(
        state: &Arc<Mutex<BackendState>>,
        window: &str,
        key: &SessionKey,
    ) -> Option<Self> {
        claim_slot(state, window, key, |win| win.claim_history(key))
            .map(|inner| HistoryGuard { inner: Some(inner) })
    }

    /// Settle a finished replay, consuming the guard.
    pub(crate) fn finish(self) {
        self.settle(WindowState::finish_history);
    }

    /// Release a failed replay, consuming the guard.
    pub(crate) fn fail(self) {
        self.settle(WindowState::abort_history);
    }

    fn settle(mut self, settle: impl FnOnce(&mut WindowState, &SessionKey)) {
        settle_slot(self.inner.take(), settle);
    }

    /// True while the guard still holds its claim. Test helper.
    #[cfg(test)]
    pub(crate) fn is_held(&self) -> bool {
        self.inner.is_some()
    }
}

impl Drop for HistoryGuard {
    fn drop(&mut self) {
        settle_slot(self.inner.take(), WindowState::abort_history);
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
        let mut state = WindowState::default();
        let key = key();
        assert!(state.claim_warm(&key));
        assert!(!state.claim_warm(&key));
    }

    #[test]
    fn release_allows_reclaim_after_failure() {
        let mut state = WindowState::default();
        let key = key();
        assert!(state.claim_warm(&key));
        state.release_warm(&key);
        assert!(state.claim_warm(&key));
    }

    #[test]
    fn warm_claims_during_history() {
        let mut state = WindowState::default();
        let key = key();
        assert!(state.claim_history(&key));
        assert!(state.claim_warm(&key));
        assert!(state.history_started(&key));
    }

    #[test]
    fn history_refused_during_turn() {
        let mut state = WindowState::default();
        let key = key();
        state.sessions.insert(
            key.clone(),
            crate::agent::LiveSession::fresh(crate::repo_state::RepoState::default()),
        );
        state.set_working(&key, true);
        assert!(!state.claim_history(&key));
    }

    #[test]
    fn history_claims_during_warm() {
        let mut state = WindowState::default();
        let key = key();
        assert!(state.claim_warm(&key));
        assert!(state.claim_history(&key));
    }

    fn window_is(
        state: &Arc<Mutex<BackendState>>,
        window: &str,
        key: &SessionKey,
    ) -> (bool, bool, bool) {
        let backend = lock_backend(state).expect("lock");
        let win = backend.windows.get(window).expect("window");
        (
            win.is_warming(key),
            win.is_history_loading(key),
            win.history_started(key),
        )
    }

    #[test]
    fn empty_path_holds_both_guards() {
        let state = Arc::new(Mutex::new(BackendState::default()));
        let key = key();
        lock_backend(&state).expect("lock").ensure_window("main");
        let history = HistoryGuard::claim(&state, "main", &key).expect("history");
        assert!(history.is_held());
        let warm = WarmGuard::claim(&state, "main", &key).expect("warm");
        assert!(warm.is_held());
        let (warming, loading, _) = window_is(&state, "main", &key);
        assert!(warming && loading);
        drop(warm);
        let (warming, loading, _) = window_is(&state, "main", &key);
        assert!(!warming && loading);
        history.finish();
        let (_, loading, started) = window_is(&state, "main", &key);
        assert!(!loading && started);
    }

    #[test]
    fn warm_guard_releases_on_drop() {
        let state = Arc::new(Mutex::new(BackendState::default()));
        let key = key();
        lock_backend(&state).expect("lock").ensure_window("main");
        for _ in 0..2 {
            let guard = WarmGuard::claim(&state, "main", &key).expect("claim");
            assert!(guard.is_held());
            assert!(window_is(&state, "main", &key).0);
            drop(guard);
            assert!(!window_is(&state, "main", &key).0);
        }
    }

    #[test]
    fn history_guard_aborts_on_drop() {
        let state = Arc::new(Mutex::new(BackendState::default()));
        let key = key();
        lock_backend(&state).expect("lock").ensure_window("main");
        {
            let guard = HistoryGuard::claim(&state, "main", &key).expect("claim");
            assert!(guard.is_held());
            assert!(window_is(&state, "main", &key).1);
        }
        assert!(!window_is(&state, "main", &key).1);
        assert!(HistoryGuard::claim(&state, "main", &key).is_some());
    }

    #[test]
    fn claims_fail_on_closed_windows_without_resurrecting() {
        let state = Arc::new(Mutex::new(BackendState::default()));
        let key = key();
        assert!(WarmGuard::claim(&state, "gone", &key).is_none());
        assert!(HistoryGuard::claim(&state, "gone", &key).is_none());
        let backend = lock_backend(&state).expect("lock");
        assert!(!backend.windows.contains_key("gone"));
    }

    #[test]
    fn snapshots_read_clear_on_missing_windows() {
        let state = Arc::new(Mutex::new(BackendState::default()));
        let key = key();
        assert!(!is_warming(&state, "gone", &key));
        assert!(!is_history_loading(&state, "gone", &key));
        lock_backend(&state).expect("lock").ensure_window("main");
        assert!(!is_warming(&state, "main", &key));
        assert!(!is_history_loading(&state, "main", &key));
    }

    #[tokio::test]
    async fn waits_settle_once_guards_release() {
        let state = Arc::new(Mutex::new(BackendState::default()));
        let key = key();
        lock_backend(&state).expect("lock").ensure_window("main");
        let warm = WarmGuard::claim(&state, "main", &key).expect("warm");
        let history = HistoryGuard::claim(&state, "main", &key).expect("history");
        assert!(is_warming(&state, "main", &key));
        assert!(is_history_loading(&state, "main", &key));
        drop(warm);
        assert!(wait_while_warming(&state, "main", &key, Duration::from_millis(200)).await);
        assert!(!is_warming(&state, "main", &key));
        history.finish();
        assert!(wait_while_history_loading(&state, "main", &key, Duration::from_millis(200)).await);
        assert!(!is_history_loading(&state, "main", &key));
    }

    #[tokio::test]
    async fn waits_time_out_on_held_flags() {
        let state = Arc::new(Mutex::new(BackendState::default()));
        let key = key();
        lock_backend(&state).expect("lock").ensure_window("main");
        let _warm = WarmGuard::claim(&state, "main", &key).expect("warm");
        let _history = HistoryGuard::claim(&state, "main", &key).expect("history");
        assert!(!wait_while_warming(&state, "main", &key, Duration::from_millis(60)).await);
        assert!(!wait_while_history_loading(&state, "main", &key, Duration::from_millis(60)).await);
    }
}
