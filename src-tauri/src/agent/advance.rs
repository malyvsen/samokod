// Automatic plan progression: one pump, one pure decision, one shared core.
// The pump scans automatic idle plans in phase then arrival order and fires
// at most one transition per plan. Finished executing plans serialize
// through `queued/`, and at most one plan sits in `rebasing/` or
// `evergreening/` at a time: promotion from queued is gated on the rebase
// mutex, so no two rebases ever run together.
// Scoping never rides the pump: approving a plan stays a manual `>` click,
// and only executing, queued, rebasing, and evergreening plans carry the
// auto-manual switch. Executing plans always queue first; queued plans
// promote while the mutex is free; rebasing plans with evergreen intent and
// fresh commits clean up in evergreening, otherwise they finish directly.
// `next_action` is pure over a plain view plus the mutex snapshot
// (unit-tested with no git repo); `transition` is the single effectful core
// that the manual approve and the pump go through, so the pump adds no
// second copy of any step.
use std::collections::HashSet;

use crate::opencode;
use crate::plans::{self, Phase};
use crate::types::{AgentError, PlansUpdate, SessionKey, SessionRole};

use super::rebasing_queue;
use super::session::ActivePlan;
use super::{AgentManager, lock_backend};

/// Pump: advance every automatic idle executing, queued, rebasing, or
/// evergreening plan that is ready, oldest arrivals first, at most one
/// transition per plan per pass. Scoping plans always wait for the manual
/// `>` click and never fire here. Queued plans promote oldest first while
/// the rebase mutex is free; at most one plan sits in rebasing or
/// evergreening, so promotions never race a running rebase.
// Dirty or diverged plans hold and are skipped until a later trigger,
// showing only their existing markers. A rebasing whose target moved under
// it re-rebases automatically while still on A. Selection never moves here
// except through follow-own-move; the pump itself never touches `current`.
impl AgentManager {
    /// Fire-and-forget pump for background triggers (turn ends, branch
    /// moves). The pump future is not `Send` (it drives the turn
    /// machinery), so it runs blocking on its own thread instead of the
    /// async runtime.
    pub(crate) fn spawn_pump(&self, window: &str) {
        let manager = self.clone();
        let window = window.to_string();
        tauri::async_runtime::spawn_blocking(move || {
            tauri::async_runtime::block_on(manager.pump(&window));
        });
    }

    pub(crate) async fn pump(&self, window: &str) {
        let mut fired_any = false;
        // Every fired transition makes its plan ineligible for the next
        // pass (its turn starts, or it leaves the phase), so the pump
        // settles on its own. The set below only backstops that invariant:
        // the same transition firing twice means it stopped sticking. A
        // `BeginRebasing` followed by `StartRebasing` for the same plan is
        // one move plus one spawn, so distinct transitions may fire once
        // each per pump run.
        let mut fired_once: HashSet<Transition> = HashSet::new();
        'pump: while let Some((repo_root, _)) = self.reopen_snapshot(window) {
            let mut scanned = plans::scan_plans(&repo_root);
            scanned.retain(|plan| plan.phase.is_active());
            if scanned.is_empty() {
                break;
            }
            scanned.sort_by(|left, right| {
                plans::phase_rank(left.phase)
                    .cmp(&plans::phase_rank(right.phase))
                    .then_with(|| {
                        arrival_key(&repo_root, left).cmp(&arrival_key(&repo_root, right))
                    })
            });
            let queued = rebasing_queue::sorted_queued_names(&repo_root);
            let mutex_held = rebasing_queue::rebase_mutex_held(&repo_root);
            let mut fired = false;
            for plan in &scanned {
                let view = self.plan_view(window, &repo_root, plan, &queued);
                let Some(action) = next_action(&view, mutex_held) else {
                    continue;
                };
                let plan_name = action.plan_name().to_string();
                match self.transition(window, action.clone()).await {
                    Ok(_) => {
                        fired = true;
                        fired_any = true;
                        if !fired_once.insert(action) {
                            log::warn!("pump fired twice for {plan_name}; stopping");
                            break 'pump;
                        }
                        break;
                    }
                    Err(error) => {
                        log::warn!("automatic transition for {plan_name} skipped: {error}");
                        continue;
                    }
                }
            }
            if !fired {
                break;
            }
        }
        if fired_any {
            self.push_plans(window);
        }
    }

    /// Plain view of one plan for the pure decision. Worktree probes fail
    /// safe (dirty holds, ffable false, no commits) with a warn-log, per
    /// the preserve-evidence rule. The queued list arrives precomputed
    /// once per pump pass so every plan shares one scan.
    fn plan_view(
        &self,
        window: &str,
        repo_root: &std::path::Path,
        plan: &plans::PlanRef,
        queued: &[String],
    ) -> PlanView {
        let stored = plans::load_state(&plan.path(repo_root));
        let manual = stored.manual;
        let evergreen = stored.evergreen;
        let role = match plan.phase {
            Phase::Scoping => SessionRole::Scoping,
            Phase::Executing | Phase::Queued => SessionRole::Executing,
            Phase::Evergreening => SessionRole::Evergreening,
            Phase::Rebasing => SessionRole::Rebasing,
            Phase::Completed | Phase::Cancelled => SessionRole::Scoping,
        };
        let idle = lock_backend(&self.state)
            .map(|backend| {
                backend
                    .windows
                    .get(window)
                    .and_then(|win| {
                        win.sessions
                            .get(&SessionKey {
                                plan: plan.name.clone(),
                                role,
                            })
                            .map(|live| !live.working)
                    })
                    .unwrap_or(true)
            })
            .unwrap_or(true);
        let (dirty, ffable, has_commits) = match plan.phase {
            Phase::Executing | Phase::Evergreening | Phase::Rebasing => {
                match self.worktree_status_for(window, repo_root, &plan.name) {
                    Ok(status) => (status.dirty, status.ffable, !status.commits.is_empty()),
                    Err(error) => {
                        log::warn!("automatic view for {} held: {error}", plan.name);
                        (true, false, false)
                    }
                }
            }
            Phase::Queued => match self.worktree_status_for(window, repo_root, &plan.name) {
                Ok(status) => (status.dirty, false, false),
                Err(error) => {
                    log::warn!("automatic view for {} held: {error}", plan.name);
                    (true, false, false)
                }
            },
            Phase::Scoping | Phase::Completed | Phase::Cancelled => (false, false, false),
        };
        let queue_position = match plan.phase {
            Phase::Queued => rebasing_queue::position_in(queued, &plan.name),
            _ => None,
        };
        let started = match plan.phase {
            Phase::Executing | Phase::Evergreening | Phase::Rebasing => {
                stored.session(role).is_some()
            }
            Phase::Scoping | Phase::Queued | Phase::Completed | Phase::Cancelled => false,
        };
        if !started && matches!(plan.phase, Phase::Executing | Phase::Evergreening) {
            log::warn!(
                "automatic view for {} held: no {:?} session yet",
                plan.name,
                plan.phase
            );
        }
        PlanView {
            name: plan.name.clone(),
            phase: plan.phase,
            manual,
            evergreen,
            idle,
            dirty,
            ffable,
            has_commits,
            queue_position,
            started,
        }
    }

    /// Clean ffable check for a rebasing plan: true when a rebase that
    /// finds its rebasing ffable after all should hand off instead. Dirty or
    /// missing plans report false so the caller falls through to the
    /// normal branch, which refuses loud with the usual message.
    fn rebasing_is_ffable(&self, window: &str, plan: &str) -> Result<bool, AgentError> {
        let Some((repo_root, _)) = self.reopen_snapshot(window) else {
            return Err(AgentError::NoSession {
                raw: "open a repository first".to_string(),
            });
        };
        let from = plans::PlanRef {
            name: plan.to_string(),
            phase: Phase::Rebasing,
        };
        if !from.path(&repo_root).is_dir() {
            return Ok(false);
        }
        match self.worktree_status_for(window, &repo_root, plan) {
            Ok(status) => Ok(!status.dirty && status.ffable),
            Err(_) => Ok(false),
        }
    }

    /// Handoff for a rebasing plan that finds itself ffable after all:
    /// evergreening when cleanup is still due, else finishing. Probe
    /// failures yield `None` so the caller falls through to the normal
    /// branch, which refuses loud without moving anything.
    fn ffable_redirect(&self, window: &str, plan: &str) -> Option<Transition> {
        if !self.rebasing_is_ffable(window, plan).ok()? {
            return None;
        }
        let (repo_root, _) = self.reopen_snapshot(window)?;
        let from = plans::PlanRef {
            name: plan.to_string(),
            phase: Phase::Rebasing,
        };
        if !from.path(&repo_root).is_dir() {
            return None;
        }
        let stored = plans::load_state(&from.path(&repo_root));
        let status = self.worktree_status_for(window, &repo_root, plan).ok()?;
        if status.dirty {
            return None;
        }
        if stored.evergreen && !status.commits.is_empty() {
            Some(Transition::BeginEvergreening {
                plan: plan.to_string(),
            })
        } else {
            Some(Transition::FinishRebasing {
                plan: plan.to_string(),
            })
        }
    }

    /// Shared transition core: idle gate, repo snapshot, `PlanRef` build,
    /// existence check, live-session drop, prompted carry, follow-own-move
    /// selection, and plans update happen once here for every entry point.
    /// The manual approve maps its session onto a `Transition` and
    /// delegates; the pump calls this directly.
    pub(crate) async fn transition(
        &self,
        window: &str,
        action: Transition,
    ) -> Result<PlansUpdate, AgentError> {
        // A rebasing that finds itself ffable after all hands off instead:
        // evergreening when cleanup is still due, else finishing.
        // Redirected up front so the core below stays a single straight
        // match with no recursion.
        let action = match action {
            Transition::StartRebasing { plan } | Transition::RebaseRebasing { plan }
                if self.rebasing_is_ffable(window, &plan)? =>
            {
                self.ffable_redirect(window, &plan)
                    .unwrap_or(Transition::FinishRebasing { plan })
            }
            action => action,
        };
        match action {
            Transition::Execute { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Scoping,
                };
                super::ensure_idle(&self.state, window, &session)?;
                let (repo_root, branch) =
                    self.reopen_snapshot(window)
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let base = crate::worktrees::head_commit(&repo_root)?;
                let from = plans::PlanRef {
                    name: plan.clone(),
                    phase: Phase::Scoping,
                };
                if !from.path(&repo_root).is_dir() {
                    return Err(AgentError::RequestFailed {
                        raw: "no scoping plan to execute".to_string(),
                    });
                }
                let next = plans::execute(&repo_root, &from)?;
                let record = crate::worktrees::create(&repo_root, &next.name, &base)?;
                match lock_backend(&self.state) {
                    Some(mut backend) => {
                        backend
                            .ensure_window(window)
                            .worktrees
                            .insert(next.name.clone(), record);
                        backend.recompute_awake();
                    }
                    None => {
                        log::warn!("failed to record worktree: state poisoned");
                    }
                }
                self.drop_live(window, &session).await;
                self.carry_prompted(window, &from.name, &next.name);
                let mut active = ActivePlan::executing(next.name.clone());
                // The role goes out as the full first turn below, so later
                // turns never prefix again.
                active.prefixed = true;
                let plan_dir_abs = next.path(&repo_root).to_string_lossy().to_string();
                let text = opencode::executing_first_message(&plan_dir_abs);
                self.spawn_with_first_turn(window, &repo_root, &branch, &from.name, active, text)
                    .await
            }
            Transition::Queue { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Executing,
                };
                super::ensure_idle(&self.state, window, &session)?;
                let (repo_root, _) =
                    self.reopen_snapshot(window)
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let from = plans::PlanRef {
                    name: plan.clone(),
                    phase: Phase::Executing,
                };
                if !from.path(&repo_root).is_dir() {
                    return Err(AgentError::RequestFailed {
                        raw: "no executing plan to queue".to_string(),
                    });
                }
                // Move-only: a dirty tree holds so user edits never queue
                // behind a stale snapshot. The pump promotes the plan once
                // the rebase mutex frees.
                let status = self.worktree_status_for(window, &repo_root, &from.name)?;
                if status.dirty {
                    return Err(AgentError::RequestFailed {
                        raw: "commit or discard worktree changes first".to_string(),
                    });
                }
                let next = plans::queue(&repo_root, &from)?;
                self.drop_live(window, &session).await;
                self.carry_prompted(window, &from.name, &next.name);
                self.select_if_viewed(
                    window,
                    &plan,
                    SessionKey {
                        plan: next.name,
                        role: SessionRole::Executing,
                    },
                );
                Ok(self.plans_update(window))
            }
            Transition::BeginEvergreening { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Rebasing,
                };
                super::ensure_idle(&self.state, window, &session)?;
                let (repo_root, branch) =
                    self.reopen_snapshot(window)
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let from = plans::PlanRef {
                    name: plan.clone(),
                    phase: Phase::Rebasing,
                };
                if !from.path(&repo_root).is_dir() {
                    return Err(AgentError::RequestFailed {
                        raw: "no rebasing plan to evergreen".to_string(),
                    });
                }
                // Same worktree, so no mutex gate: the plan already holds
                // it. A dirty tree still refuses so user edits never mix
                // into the cleanup, and a diverged tree routes back through
                // rebasing instead of cleaning up a stale base.
                let status = self.worktree_status_for(window, &repo_root, &from.name)?;
                if status.dirty {
                    return Err(AgentError::RequestFailed {
                        raw: "commit or discard worktree changes first".to_string(),
                    });
                }
                if !status.ffable {
                    return Err(AgentError::RequestFailed {
                        raw: "target moved on, start rebasing first".to_string(),
                    });
                }
                let next = plans::begin_evergreening(&repo_root, &from)?;
                self.drop_live(window, &session).await;
                let mut active = ActivePlan::evergreening(next.name.clone());
                // The role goes out as the full first turn below, so later
                // turns never prefix again.
                active.prefixed = true;
                let plan_md_abs = next.plan_md(&repo_root).to_string_lossy().to_string();
                let text = opencode::evergreening_first_message(
                    &status.path.to_string_lossy(),
                    &status.worktree_branch,
                    &plan_md_abs,
                    &status.target_branch,
                    &status.commits.join("\n"),
                );
                self.spawn_with_first_turn(window, &repo_root, &branch, &from.name, active, text)
                    .await
            }
            Transition::BeginRebasing { plan } => {
                let (repo_root, _) =
                    self.reopen_snapshot(window)
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                // Move-only while the mutex is free: the pump starts the
                // plan with `StartRebasing` on a later pass. Queued plans
                // carry no live session; the executing key below only gates
                // a racing turn.
                if rebasing_queue::rebase_mutex_held(&repo_root) {
                    return Err(AgentError::RequestFailed {
                        raw: "another plan is already rebasing".to_string(),
                    });
                }
                let from = plans::PlanRef {
                    name: plan.clone(),
                    phase: Phase::Queued,
                };
                if !from.path(&repo_root).is_dir() {
                    return Err(AgentError::RequestFailed {
                        raw: "no queued plan to rebase".to_string(),
                    });
                }
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Executing,
                };
                super::ensure_idle(&self.state, window, &session)?;
                let status = self.worktree_status_for(window, &repo_root, &from.name)?;
                super::gate_rebasing(status.dirty, status.ffable, super::RebasingStep::Begin)?;
                let next = plans::begin_rebasing(&repo_root, &from)?;
                self.drop_plan_lives(window, &from.name).await;
                self.carry_prompted(window, &from.name, &next.name);
                self.select_if_viewed(
                    window,
                    &plan,
                    SessionKey {
                        plan: next.name,
                        role: SessionRole::Rebasing,
                    },
                );
                Ok(self.plans_update(window))
            }
            Transition::StartRebasing { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Rebasing,
                };
                super::ensure_idle(&self.state, window, &session)?;
                let (repo_root, branch) =
                    self.reopen_snapshot(window)
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let from = require_rebasing_plan(&repo_root, &plan)?;
                let status = self.worktree_status_for(window, &repo_root, &from.name)?;
                super::gate_rebasing(status.dirty, status.ffable, super::RebasingStep::Start)?;
                let mut active = ActivePlan::rebasing(from.name.clone());
                // The role goes out as the full first turn below, so later
                // turns never prefix again.
                active.prefixed = true;
                let plan_md_abs = from.plan_md(&repo_root).to_string_lossy().to_string();
                let text = rebasing_first_turn_text(&status, &plan_md_abs);
                self.spawn_with_first_turn(window, &repo_root, &branch, &plan, active, text)
                    .await
            }
            Transition::BackToRebasing { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Evergreening,
                };
                super::ensure_idle(&self.state, window, &session)?;
                let (repo_root, _) =
                    self.reopen_snapshot(window)
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let from = plans::PlanRef {
                    name: plan.clone(),
                    phase: Phase::Evergreening,
                };
                if !from.path(&repo_root).is_dir() {
                    return Err(AgentError::RequestFailed {
                        raw: "no evergreening plan to rebase".to_string(),
                    });
                }
                // Move-only: the target moved under the cleanup, so the plan
                // keeps the rebase mutex and re-queues for a rebase instead
                // of releasing it to queued. A dirty tree holds; an already
                // ffable tree finishes instead of diverging back.
                let status = self.worktree_status_for(window, &repo_root, &from.name)?;
                if status.dirty {
                    return Err(AgentError::RequestFailed {
                        raw: "commit or discard worktree changes first".to_string(),
                    });
                }
                if status.ffable {
                    return Err(AgentError::RequestFailed {
                        raw: "already fast-forwardable, finish it directly".to_string(),
                    });
                }
                let next = plans::back_to_rebasing(&repo_root, &from)?;
                self.drop_live(window, &session).await;
                self.carry_prompted(window, &from.name, &next.name);
                self.select_if_viewed(
                    window,
                    &plan,
                    SessionKey {
                        plan: next.name,
                        role: SessionRole::Rebasing,
                    },
                );
                Ok(self.plans_update(window))
            }
            Transition::FinishEvergreening { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Evergreening,
                };
                super::ensure_idle(&self.state, window, &session)?;
                let (repo_root, _) =
                    self.reopen_snapshot(window)
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let from = plans::PlanRef {
                    name: plan.clone(),
                    phase: Phase::Evergreening,
                };
                if !from.path(&repo_root).is_dir() {
                    return Err(AgentError::RequestFailed {
                        raw: "no active plan to complete".to_string(),
                    });
                }
                let status = self.worktree_status_for(window, &repo_root, &from.name)?;
                super::gate_rebasing(status.dirty, status.ffable, super::RebasingStep::Finish)?;
                crate::worktrees::fast_forward(
                    &repo_root,
                    &status.target_branch,
                    &status.worktree_branch,
                )?;
                self.remove_worktree(window, &repo_root, &from.name, false)?;
                let next = plans::complete(&repo_root, &from)?;
                self.drop_plan_lives(window, &from.name).await;
                self.carry_prompted(window, &from.name, &next.name);
                self.select_if_viewed(
                    window,
                    &plan,
                    SessionKey {
                        plan: next.name,
                        role: SessionRole::Evergreening,
                    },
                );
                Ok(self.plans_update(window))
            }
            Transition::FinishRebasing { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Rebasing,
                };
                super::ensure_idle(&self.state, window, &session)?;
                let (repo_root, _) =
                    self.reopen_snapshot(window)
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let from = plans::PlanRef {
                    name: plan.clone(),
                    phase: Phase::Rebasing,
                };
                if !from.path(&repo_root).is_dir() {
                    return Err(AgentError::RequestFailed {
                        raw: "no active plan to complete".to_string(),
                    });
                }
                let status = self.worktree_status_for(window, &repo_root, &from.name)?;
                super::gate_rebasing(status.dirty, status.ffable, super::RebasingStep::Finish)?;
                crate::worktrees::fast_forward(
                    &repo_root,
                    &status.target_branch,
                    &status.worktree_branch,
                )?;
                self.remove_worktree(window, &repo_root, &from.name, false)?;
                let next = plans::finish_rebasing(&repo_root, &from)?;
                self.drop_plan_lives(window, &from.name).await;
                self.carry_prompted(window, &from.name, &next.name);
                self.select_if_viewed(
                    window,
                    &plan,
                    SessionKey {
                        plan: next.name,
                        role: SessionRole::Rebasing,
                    },
                );
                Ok(self.plans_update(window))
            }
            Transition::RebaseRebasing { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Rebasing,
                };
                super::ensure_idle(&self.state, window, &session)?;
                let (repo_root, _) =
                    self.reopen_snapshot(window)
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let from = require_rebasing_plan(&repo_root, &plan)?;
                let status = self.worktree_status_for(window, &repo_root, &from.name)?;
                if status.dirty {
                    return Err(AgentError::RequestFailed {
                        raw: "commit or discard worktree changes first".to_string(),
                    });
                }
                let plan_md_abs = from.plan_md(&repo_root).to_string_lossy().to_string();
                let text = rebasing_first_turn_text(&status, &plan_md_abs);
                let (connection, session_id) = self.ensure_live(window, &session).await?;
                self.mark_prompted(window, &from.name);
                self.start_turn(
                    window,
                    connection,
                    session_id,
                    session.clone(),
                    text.clone(),
                )
                .await?;
                self.announce_first_prompt(window, &session, &text);
                Ok(self.plans_update(window))
            }
        }
    }

    /// Shared spawn-plus-first-turn for the three agent-start transitions
    /// (`Execute`, `BeginEvergreening`, `StartRebasing`): spawn the session
    /// for the caller's `ActivePlan`, send its role as the full first turn,
    /// and announce it as a live user bubble. The caller moves the plan and
    /// drops the old live session first; the plan name travels inside
    /// `active` so the prompted flag follows it. Follows its own move only.
    async fn spawn_with_first_turn(
        &self,
        window: &str,
        repo_root: &std::path::Path,
        branch: &str,
        acted_plan: &str,
        active: ActivePlan,
        text: String,
    ) -> Result<PlansUpdate, AgentError> {
        let plan_name = active.name.clone();
        let agent = opencode::agent_for(active.phase);
        let (connection, session_id, key) = self
            .spawn_session(window, repo_root, branch, active, agent)
            .await?;
        self.select_if_viewed(window, acted_plan, key.clone());
        self.mark_prompted(window, &plan_name);
        self.start_turn(window, connection, session_id, key.clone(), text.clone())
            .await?;
        self.announce_first_prompt(window, &key, &text);
        Ok(self.plans_update(window))
    }
}

/// One transition the pump or the manual approve can perform. At most one per
/// plan per pump pass. `Execute` is manual-only (the header `>` click) and
/// never fires from the pump; `Queue` moves a finished executing plan into
/// the queue; `BeginRebasing` promotes the oldest queued plan while the
/// rebase mutex is free; `StartRebasing` spawns the rebasing agent fresh and
/// `RebaseRebasing` re-runs it once started; `BeginEvergreening` moves a
/// rebasing plan into cleanup and spawns the evergreening agent;
/// `BackToRebasing` moves a diverged evergreening plan back for another
/// rebase; `FinishRebasing` and `FinishEvergreening` fast-forward the target
/// and complete.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Transition {
    Execute { plan: String },
    Queue { plan: String },
    BeginRebasing { plan: String },
    StartRebasing { plan: String },
    RebaseRebasing { plan: String },
    BeginEvergreening { plan: String },
    BackToRebasing { plan: String },
    FinishRebasing { plan: String },
    FinishEvergreening { plan: String },
}

impl Transition {
    fn plan_name(&self) -> &str {
        match self {
            Transition::Execute { plan }
            | Transition::Queue { plan }
            | Transition::BeginRebasing { plan }
            | Transition::StartRebasing { plan }
            | Transition::RebaseRebasing { plan }
            | Transition::BeginEvergreening { plan }
            | Transition::BackToRebasing { plan }
            | Transition::FinishRebasing { plan }
            | Transition::FinishEvergreening { plan } => plan,
        }
    }
}

/// Plain view of one plan for the pure decision: phase, stored mode and
/// evergreen intent, idle (no turn running), worktree dirtiness and
/// fast-forwardability, commit presence, queue position, and whether the
/// phase's agent started yet (stored session ID; unused for scoping,
/// queued, completed, and cancelled, which never fire an agent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanView {
    pub name: String,
    pub phase: Phase,
    pub manual: bool,
    pub evergreen: bool,
    pub idle: bool,
    pub dirty: bool,
    pub ffable: bool,
    pub has_commits: bool,
    pub queue_position: Option<usize>,
    pub started: bool,
}

/// Pure automation decision over one plan: at most one transition. Scoping
/// always waits for the manual `>` click and never fires here. Manual
/// plans hold; running turns hold; dirty worktrees hold. Executing plans
/// also hold until their agent started, so a fresh approval never queues
/// before the first turn. Executing plans always queue; queued plans promote
/// while the rebase mutex is free; rebasing plans hand to evergreening when
/// ffable with evergreen intent and fresh commits, else finish directly
/// when ffable, else start or re-run the rebase agent; evergreening plans
/// finish directly when ffable, else diverge back to rebasing. Pure.
pub(crate) fn next_action(view: &PlanView, rebase_mutex_held: bool) -> Option<Transition> {
    if view.manual || !view.idle {
        return None;
    }
    match view.phase {
        Phase::Scoping | Phase::Completed | Phase::Cancelled => None,
        Phase::Executing => {
            if !view.started || view.dirty {
                None
            } else {
                Some(Transition::Queue {
                    plan: view.name.clone(),
                })
            }
        }
        Phase::Queued => {
            if view.dirty || view.queue_position.is_none() || rebase_mutex_held {
                None
            } else {
                Some(Transition::BeginRebasing {
                    plan: view.name.clone(),
                })
            }
        }
        Phase::Rebasing => {
            if view.dirty {
                None
            } else if view.ffable {
                if view.evergreen && view.has_commits {
                    Some(Transition::BeginEvergreening {
                        plan: view.name.clone(),
                    })
                } else {
                    Some(Transition::FinishRebasing {
                        plan: view.name.clone(),
                    })
                }
            } else if view.started {
                Some(Transition::RebaseRebasing {
                    plan: view.name.clone(),
                })
            } else {
                Some(Transition::StartRebasing {
                    plan: view.name.clone(),
                })
            }
        }
        Phase::Evergreening => {
            if !view.started || view.dirty {
                None
            } else if view.ffable {
                Some(Transition::FinishEvergreening {
                    plan: view.name.clone(),
                })
            } else {
                Some(Transition::BackToRebasing {
                    plan: view.name.clone(),
                })
            }
        }
    }
}

/// Rebasing first turn shared by the fresh spawn and the re-run. Pure.
fn rebasing_first_turn_text(status: &super::WorktreeLive, plan_md_abs: &str) -> String {
    opencode::rebasing_first_message(
        &status.worktree_branch,
        &status.target_branch,
        &status.path.to_string_lossy(),
        plan_md_abs,
    )
}

/// Rebasing plan plus existence, shared by the fresh spawn and the re-run.
/// The mutex keeps at most one rebasing plan around, so existence is the
/// whole check; missing rebasing plans fail loud. Pure except the
/// existence probe.
fn require_rebasing_plan(
    repo_root: &std::path::Path,
    plan: &str,
) -> Result<plans::PlanRef, AgentError> {
    let from = plans::PlanRef {
        name: plan.to_string(),
        phase: Phase::Rebasing,
    };
    if !from.path(repo_root).is_dir() {
        return Err(AgentError::RequestFailed {
            raw: "no active plan to complete".to_string(),
        });
    }
    Ok(from)
}

/// Arrival key for the pump: oldest phase-entry first, unstamped plans
/// last. Pure except the metadata probes inside `arrival_ms`.
fn arrival_key(repo_root: &std::path::Path, plan: &plans::PlanRef) -> (i64, String) {
    // Unstamped plans sort last: `i64::MAX` is younger than any real stamp.
    (
        plans::arrival_ms(repo_root, plan).unwrap_or(i64::MAX),
        plan.name.clone(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(name: &str, phase: Phase) -> PlanView {
        PlanView {
            name: name.to_string(),
            phase,
            manual: false,
            evergreen: true,
            idle: true,
            dirty: false,
            ffable: false,
            has_commits: false,
            queue_position: if phase == Phase::Queued {
                Some(0)
            } else {
                None
            },
            started: true,
        }
    }

    #[test]
    fn scoping_always_waits_for_the_button() {
        assert_eq!(next_action(&view("a", Phase::Scoping), false), None);
        assert_eq!(
            next_action(
                &PlanView {
                    ffable: true,
                    ..view("a", Phase::Scoping)
                },
                false
            ),
            None
        );
        assert_eq!(
            next_action(
                &PlanView {
                    idle: false,
                    ..view("a", Phase::Scoping)
                },
                false
            ),
            None
        );
    }

    #[test]
    fn manual_plans_hold_in_place() {
        for phase in [
            Phase::Scoping,
            Phase::Executing,
            Phase::Queued,
            Phase::Rebasing,
            Phase::Evergreening,
        ] {
            let held = PlanView {
                manual: true,
                ffable: true,
                has_commits: true,
                ..view("a", phase)
            };
            assert_eq!(next_action(&held, false), None, "{phase:?} holds on M");
            assert_eq!(next_action(&held, true), None, "{phase:?} holds on M");
        }
    }

    #[test]
    fn running_turns_hold() {
        let working = PlanView {
            idle: false,
            ffable: true,
            has_commits: true,
            ..view("a", Phase::Executing)
        };
        assert_eq!(next_action(&working, false), None);
        let queued_working = PlanView {
            idle: false,
            ..view("a", Phase::Queued)
        };
        assert_eq!(next_action(&queued_working, false), None);
        let greening = PlanView {
            idle: false,
            ffable: true,
            ..view("a", Phase::Evergreening)
        };
        assert_eq!(next_action(&greening, false), None);
    }

    #[test]
    fn executing_always_queues() {
        // Intent, commits, and ffable never skip the queue: every finished
        // executing plan serializes through queued.
        for view in [
            view("a", Phase::Executing),
            PlanView {
                has_commits: true,
                ..view("b", Phase::Executing)
            },
            PlanView {
                ffable: true,
                ..view("c", Phase::Executing)
            },
            PlanView {
                evergreen: false,
                has_commits: true,
                ffable: true,
                ..view("d", Phase::Executing)
            },
        ] {
            assert_eq!(
                next_action(&view, false),
                Some(Transition::Queue {
                    plan: view.name.clone()
                })
            );
        }
    }

    #[test]
    fn queued_promotes_while_mutex_free() {
        // No ffable gate: ffable plans queue and promote like any other.
        for view in [
            view("a", Phase::Queued),
            PlanView {
                ffable: true,
                ..view("b", Phase::Queued)
            },
            PlanView {
                ffable: true,
                has_commits: true,
                ..view("c", Phase::Queued)
            },
        ] {
            assert_eq!(
                next_action(&view, false),
                Some(Transition::BeginRebasing {
                    plan: view.name.clone()
                })
            );
        }
    }

    #[test]
    fn queued_waits_on_mutex_even_when_ffable() {
        let waiter = view("a", Phase::Queued);
        assert_eq!(next_action(&waiter, true), None);
        let ffable_waiter = PlanView {
            ffable: true,
            has_commits: true,
            ..view("b", Phase::Queued)
        };
        assert_eq!(next_action(&ffable_waiter, true), None);
    }

    #[test]
    fn queued_holds_when_dirty_or_unlisted() {
        let dirty = PlanView {
            dirty: true,
            ..view("a", Phase::Queued)
        };
        assert_eq!(next_action(&dirty, false), None);
        let unlisted = PlanView {
            queue_position: None,
            ..view("b", Phase::Queued)
        };
        assert_eq!(next_action(&unlisted, false), None);
    }

    #[test]
    fn rebasing_hands_to_evergreening_with_intent_and_commits() {
        let greening = PlanView {
            ffable: true,
            has_commits: true,
            ..view("a", Phase::Rebasing)
        };
        assert_eq!(
            next_action(&greening, false),
            Some(Transition::BeginEvergreening {
                plan: "a".to_string()
            })
        );
    }

    #[test]
    fn rebasing_finishes_without_intent_or_commits() {
        let no_intent = PlanView {
            evergreen: false,
            ffable: true,
            has_commits: true,
            ..view("a", Phase::Rebasing)
        };
        assert_eq!(
            next_action(&no_intent, false),
            Some(Transition::FinishRebasing {
                plan: "a".to_string()
            })
        );
        let empty = PlanView {
            ffable: true,
            ..view("b", Phase::Rebasing)
        };
        assert_eq!(
            next_action(&empty, false),
            Some(Transition::FinishRebasing {
                plan: "b".to_string()
            })
        );
        // Finishing never needs the mutex: the plan already holds it.
        assert_eq!(
            next_action(&empty, true),
            Some(Transition::FinishRebasing {
                plan: "b".to_string()
            })
        );
    }

    #[test]
    fn rebasing_starts_fresh_and_rebases_once_started() {
        let fresh = PlanView {
            started: false,
            ..view("a", Phase::Rebasing)
        };
        assert_eq!(
            next_action(&fresh, false),
            Some(Transition::StartRebasing {
                plan: "a".to_string()
            })
        );
        let started = PlanView {
            started: true,
            ..view("a", Phase::Rebasing)
        };
        assert_eq!(
            next_action(&started, false),
            Some(Transition::RebaseRebasing {
                plan: "a".to_string()
            })
        );
        let held = PlanView {
            manual: true,
            ..view("a", Phase::Rebasing)
        };
        assert_eq!(next_action(&held, false), None);
    }

    #[test]
    fn evergreening_finishes_directly_when_ffable() {
        let done = PlanView {
            ffable: true,
            ..view("a", Phase::Evergreening)
        };
        assert_eq!(
            next_action(&done, false),
            Some(Transition::FinishEvergreening {
                plan: "a".to_string()
            })
        );
    }

    #[test]
    fn evergreening_diverges_back_to_rebasing() {
        let diverged = view("a", Phase::Evergreening);
        assert_eq!(
            next_action(&diverged, false),
            Some(Transition::BackToRebasing {
                plan: "a".to_string()
            })
        );
        // Diverging keeps the mutex instead of releasing it to queued.
        assert_eq!(
            next_action(&diverged, true),
            Some(Transition::BackToRebasing {
                plan: "a".to_string()
            })
        );
    }

    #[test]
    fn dirty_plans_hold_for_the_user() {
        let dirty_exec = PlanView {
            dirty: true,
            has_commits: true,
            ..view("a", Phase::Executing)
        };
        assert_eq!(next_action(&dirty_exec, false), None);
        let dirty_queued = PlanView {
            dirty: true,
            ..view("b", Phase::Queued)
        };
        assert_eq!(next_action(&dirty_queued, false), None);
        let dirty_green = PlanView {
            dirty: true,
            ffable: true,
            ..view("c", Phase::Evergreening)
        };
        assert_eq!(next_action(&dirty_green, false), None);
        let dirty_rebasing = PlanView {
            dirty: true,
            ffable: true,
            ..view("d", Phase::Rebasing)
        };
        assert_eq!(next_action(&dirty_rebasing, false), None);
    }

    #[test]
    fn begin_then_start_chain_are_distinct_transitions() {
        let begin = Transition::BeginRebasing {
            plan: "a".to_string(),
        };
        let start = Transition::StartRebasing {
            plan: "a".to_string(),
        };
        assert_ne!(begin, start);
    }

    #[test]
    fn finished_plans_never_move() {
        for phase in [Phase::Completed, Phase::Cancelled] {
            let done = PlanView {
                ffable: true,
                ..view("a", phase)
            };
            assert_eq!(next_action(&done, false), None, "{phase:?} stays put");
        }
    }

    #[test]
    fn executing_holds_until_agent_starts() {
        let fresh = PlanView {
            ffable: true,
            started: false,
            ..view("a", Phase::Executing)
        };
        assert_eq!(next_action(&fresh, false), None);
        let with_commits = PlanView {
            has_commits: true,
            started: false,
            ..view("a", Phase::Executing)
        };
        assert_eq!(next_action(&with_commits, false), None);
    }

    #[test]
    fn evergreening_holds_until_agent_starts() {
        let fresh = PlanView {
            ffable: true,
            started: false,
            ..view("a", Phase::Evergreening)
        };
        assert_eq!(next_action(&fresh, false), None);
    }
}
