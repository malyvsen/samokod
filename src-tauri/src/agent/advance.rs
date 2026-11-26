// Automatic plan progression: one pump, one pure decision, one shared core.
// The pump scans automatic idle plans in phase then arrival order and fires
// at most one transition per plan. Landing plans skip `queued/` and
// `rebasing/` when already fast-forwardable and the mutex is free: with
// commits and evergreen intent they evergreen, without it they finish;
// diverged plans and held-mutex plans queue instead. Empty plans hold for
// input and never auto-advance. At most one plan sits in `rebasing/` or
// `evergreening/` at a time: every landing still serializes through the
// rebase mutex, it just skips phases.
// Scoping never rides the pump: approving a plan stays a manual `>` click,
// and only executing, queued, rebasing, and evergreening plans carry the
// auto-manual switch.
// `next_action` is pure over a plain view plus the mutex snapshot
// (unit-tested with no git repo); `transition` is the single effectful core
// that the manual approve and the pump go through, so the pump adds no
// second copy of any step.
use std::collections::HashSet;

use crate::opencode;
use crate::plans::{self, Phase};
use crate::types::{AgentError, PlansUpdate, SessionKey, SessionRole};

use super::queue;
use super::session::{ActivePlan, spawn_system_text};
use super::{AgentManager, WorktreeLive, lock_backend};

/// Minimal trigger waking an auto-started agent. Role instructions already
/// travel as `systemPrompt` on `session/new`, so automatic starts send no
/// user text.
const AUTO_START_TRIGGER: &str = "Proceed.";

/// Pump: advance every automatic idle executing, queued, rebasing, or
/// evergreening plan that is ready, oldest arrivals first, at most one
/// transition per plan per pass. Scoping plans always wait for the manual
/// `>` click and never fire here. Landing plans skip phases while the
/// rebase mutex is free; at most one plan sits in rebasing or
/// evergreening, so landings never race a running rebase or each other.
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
            let queued = queue::sorted_names(&repo_root);
            let mutex_held = queue::mutex_held(&repo_root);
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
        let (dirty, ffable, has_commits) =
            if plan.phase.is_active() && !matches!(plan.phase, Phase::Scoping) {
                match self.worktree_status_for(window, repo_root, &plan.name) {
                    Ok(status) => (status.dirty, status.ffable, !status.commits.is_empty()),
                    Err(error) => {
                        log::warn!("automatic view for {} held: {error}", plan.name);
                        (true, false, false)
                    }
                }
            } else {
                (false, false, false)
            };
        let queue_position = match plan.phase {
            Phase::Queued => queue::position_in(queued, &plan.name),
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
        // A rebasing that finds itself ffable after all hands off on the next
        // pass: the pure decision already routes ffable rebasing plans to
        // evergreening or finishing, so a stale spawn or re-run refuses loud
        // in its core and heals on the following pass.
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
                let active = ActivePlan::new(next.name.clone(), Phase::Executing);
                self.spawn_with_trigger(window, &repo_root, &branch, &from.name, active, None)
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
                // behind a stale snapshot. The pump lands the plan once
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
                let (repo_root, branch) =
                    self.reopen_snapshot(window)
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                // Landings skip queued/rebasing but still serialize through
                // the mutex: only the rebasing source already holds it.
                let from = locate_source(
                    &repo_root,
                    &plan,
                    &[Phase::Executing, Phase::Queued, Phase::Rebasing],
                )
                .ok_or_else(|| AgentError::RequestFailed {
                    raw: "no executing, queued, or rebasing plan to evergreen".to_string(),
                })?;
                let direct = !matches!(from.phase, Phase::Rebasing);
                if direct && queue::mutex_held(&repo_root) {
                    return Err(AgentError::RequestFailed {
                        raw: "another plan is already rebasing".to_string(),
                    });
                }
                let session = SessionKey {
                    plan: plan.clone(),
                    role: source_role(from.phase),
                };
                super::ensure_idle(&self.state, window, &session)?;
                // A dirty tree still refuses so user edits never mix
                // into the cleanup, and a diverged tree routes back through
                // rebasing instead of cleaning up a stale base. The rebasing
                // source already holds the mutex; landing sources checked it
                // above and skip phases, not the gate.
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
                let active = ActivePlan::new(next.name.clone(), Phase::Evergreening);
                self.spawn_with_trigger(
                    window,
                    &repo_root,
                    &branch,
                    &from.name,
                    active,
                    Some(&status),
                )
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
                // a racing turn. Executing plans land here when diverged
                // instead of queueing first.
                if queue::mutex_held(&repo_root) {
                    return Err(AgentError::RequestFailed {
                        raw: "another plan is already rebasing".to_string(),
                    });
                }
                let from = locate_source(&repo_root, &plan, &[Phase::Executing, Phase::Queued])
                    .ok_or_else(|| AgentError::RequestFailed {
                        raw: "no executing or queued plan to rebase".to_string(),
                    })?;
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
                let active = ActivePlan::new(from.name.clone(), Phase::Rebasing);
                self.spawn_with_trigger(window, &repo_root, &branch, &plan, active, Some(&status))
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
            Transition::Finish { plan } => {
                let (repo_root, _) =
                    self.reopen_snapshot(window)
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                // Landings skip phases but still serialize through
                // the mutex: concurrent `merge --ff-only` race the target
                // so the second source stops being ffable. Names stay
                // unique across phases, so the plan's current phase decides
                // the idle gate and the follow selection.
                let from = locate_source(
                    &repo_root,
                    &plan,
                    &[
                        Phase::Executing,
                        Phase::Queued,
                        Phase::Rebasing,
                        Phase::Evergreening,
                    ],
                )
                .ok_or_else(|| AgentError::RequestFailed {
                    raw: "no active plan to complete".to_string(),
                })?;
                let direct = matches!(from.phase, Phase::Executing | Phase::Queued);
                let role = source_role(from.phase);
                super::ensure_idle(
                    &self.state,
                    window,
                    &SessionKey {
                        plan: plan.clone(),
                        role,
                    },
                )?;
                if direct {
                    // Queued owns no live session; the executing key above
                    // only gates a racing turn. Empty plans never land, and
                    // evergreen intent greens first.
                    if queue::mutex_held(&repo_root) {
                        return Err(AgentError::RequestFailed {
                            raw: "another plan is already rebasing".to_string(),
                        });
                    }
                    let stored = plans::load_state(&from.path(&repo_root));
                    if stored.evergreen {
                        return Err(AgentError::RequestFailed {
                            raw: "plan needs evergreening first".to_string(),
                        });
                    }
                }
                let status = self.worktree_status_for(window, &repo_root, &from.name)?;
                if direct && status.commits.is_empty() {
                    return Err(AgentError::RequestFailed {
                        raw: "no commits to finish".to_string(),
                    });
                }
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
                        role,
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
                let (connection, session_id) = self.ensure_live(window, &session).await?;
                self.mark_prompted(window, &from.name);
                self.start_turn(
                    window,
                    connection,
                    session_id,
                    session.clone(),
                    AUTO_START_TRIGGER.to_string(),
                )
                .await?;
                Ok(self.plans_update(window))
            }
        }
    }

    /// Shared spawn-plus-trigger for the three agent-start transitions
    /// (`Execute`, `BeginEvergreening`, `StartRebasing`): spawn the session
    /// with role instructions as `systemPrompt`, then send a minimal trigger
    /// so the agent wakes. The caller moves the plan and drops the old live
    /// session first; the plan name travels inside `active` so the prompted
    /// flag follows it. `status` carries fresh worktree coordinates for the
    /// rebasing and evergreening starts. Follows its own move only.
    async fn spawn_with_trigger(
        &self,
        window: &str,
        repo_root: &std::path::Path,
        branch: &str,
        acted_plan: &str,
        active: ActivePlan,
        status: Option<&WorktreeLive>,
    ) -> Result<PlansUpdate, AgentError> {
        let system = spawn_system_text(&active, repo_root, branch, status);
        let plan_name = active.name.clone();
        let role = active.role().ok_or_else(|| AgentError::RequestFailed {
            raw: "cannot open a session for a finished plan".to_string(),
        })?;
        let agent = opencode::agent_for(role);
        let (connection, session_id, key) = self
            .spawn_session(window, repo_root, branch, active, agent, &system)
            .await?;
        self.select_if_viewed(window, acted_plan, key.clone());
        self.mark_prompted(window, &plan_name);
        self.start_turn(
            window,
            connection,
            session_id,
            key.clone(),
            AUTO_START_TRIGGER.to_string(),
        )
        .await?;
        Ok(self.plans_update(window))
    }
}

/// One transition the pump or the manual approve can perform. At most one per
/// plan per pump pass. `Execute` is manual-only (the header `>` click) and
/// never fires from the pump; `Queue` moves a finished executing plan into
/// the queue; `BeginRebasing` moves a diverged executing or queued plan
/// into rebasing while the rebase mutex is free; `StartRebasing` spawns
/// the rebasing agent fresh and `RebaseRebasing` re-runs it once started;
/// `BeginEvergreening` moves a fast-forwardable plan with evergreen intent
/// into cleanup and spawns the evergreening agent; `BackToRebasing` moves
/// a diverged evergreening plan back for another rebase; `Finish`
/// fast-forwards the target and completes a fast-forwardable plan without
/// evergreen need. Landings still serialize through the mutex, they just
/// skip phases.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Transition {
    Execute { plan: String },
    Queue { plan: String },
    BeginRebasing { plan: String },
    StartRebasing { plan: String },
    RebaseRebasing { plan: String },
    BeginEvergreening { plan: String },
    BackToRebasing { plan: String },
    Finish { plan: String },
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
            | Transition::Finish { plan } => plan,
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
/// also hold until their agent started, and empty executing/queued plans
/// hold for input (never `Queue`, `Begin`, or `Finish`). Executing plans
/// with commits queue when diverged or held, else land directly when
/// fast-forwardable and free. Queued plans with commits land while the
/// rebase mutex is free (rebase when diverged, evergreen or finish when
/// fast-forwardable); rebasing plans evergreen when fast-forwardable with
/// evergreen intent and fresh commits, else finish when fast-forwardable,
/// else start or re-run the rebase agent; evergreening plans finish when
/// fast-forwardable, else diverge back to rebasing. Pure.
/// Fast-forwardable plans still serialize through the mutex — they skip
/// phases, not the gate — because concurrent rebases diverge from each
/// other, concurrent `merge --ff-only` race the target so the second
/// source stops being fast-forwardable, and a target move landing mid-rebase
/// forces a re-rebase.
pub(crate) fn next_action(view: &PlanView, rebase_mutex_held: bool) -> Option<Transition> {
    if view.manual || !view.idle {
        return None;
    }
    match view.phase {
        Phase::Scoping | Phase::Completed | Phase::Cancelled => None,
        Phase::Executing => {
            if !view.started || view.dirty || !view.has_commits {
                None
            } else if !view.ffable || rebase_mutex_held {
                Some(Transition::Queue {
                    plan: view.name.clone(),
                })
            } else {
                Some(landing_for(&view.name, view.evergreen))
            }
        }
        Phase::Queued => {
            if view.dirty || view.queue_position.is_none() || !view.has_commits || rebase_mutex_held
            {
                None
            } else if !view.ffable {
                Some(Transition::BeginRebasing {
                    plan: view.name.clone(),
                })
            } else {
                Some(landing_for(&view.name, view.evergreen))
            }
        }
        Phase::Rebasing => {
            if view.dirty {
                None
            } else if view.ffable {
                Some(landing_for(&view.name, view.evergreen && view.has_commits))
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
                Some(Transition::Finish {
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

/// First on-disk source for a plan name across the given phases, in order.
/// Lets one transition accept several sources (executing/queued landing
/// straight into rebasing, evergreening, or completion) without repeating
/// the scan. `None` when the plan lives in none of them. Pure except the
/// directory probes.
fn locate_source(
    repo_root: &std::path::Path,
    plan: &str,
    phases: &[Phase],
) -> Option<plans::PlanRef> {
    phases
        .iter()
        .map(|phase| plans::PlanRef {
            name: plan.to_string(),
            phase: *phase,
        })
        .find(|candidate| candidate.path(repo_root).is_dir())
}

/// Landing for a fast-forwardable plan: cleanup when evergreen intent is
/// set, completion otherwise. Shared by the executing, queued, and
/// rebasing tails of `next_action` so all three skip to the same place.
/// Pure.
fn landing_for(plan: &str, evergreen: bool) -> Transition {
    if evergreen {
        Transition::BeginEvergreening {
            plan: plan.to_string(),
        }
    } else {
        Transition::Finish {
            plan: plan.to_string(),
        }
    }
}

/// Live session role for a landing source: queued owns no session of its
/// own, so executing gates its racing turn. Pure.
fn source_role(phase: Phase) -> SessionRole {
    match phase {
        Phase::Executing | Phase::Queued => SessionRole::Executing,
        Phase::Rebasing => SessionRole::Rebasing,
        Phase::Evergreening => SessionRole::Evergreening,
        Phase::Scoping | Phase::Completed | Phase::Cancelled => SessionRole::Scoping,
    }
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
            raw: "no rebasing plan to rebase".to_string(),
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

    fn committed(name: &str, phase: Phase) -> PlanView {
        PlanView {
            has_commits: true,
            ..view(name, phase)
        }
    }

    fn landable(name: &str, phase: Phase, evergreen: bool) -> PlanView {
        PlanView {
            ffable: true,
            has_commits: true,
            evergreen,
            ..view(name, phase)
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
    fn executing_queues_when_diverged_or_held() {
        // Diverged plans queue even when the mutex is free; fast-forwardable
        // plans queue while the mutex is held.
        for view in [
            committed("a", Phase::Executing),
            committed("b", Phase::Executing),
            landable("c", Phase::Executing, true),
            landable("d", Phase::Executing, false),
        ] {
            assert_eq!(
                next_action(&view, true),
                Some(Transition::Queue {
                    plan: view.name.clone()
                }),
                "{} queues while held",
                view.name
            );
        }
        for view in [
            committed("a", Phase::Executing),
            committed("b", Phase::Executing),
        ] {
            assert_eq!(
                next_action(&view, false),
                Some(Transition::Queue {
                    plan: view.name.clone()
                }),
                "{} queues when diverged",
                view.name
            );
        }
    }

    #[test]
    fn executing_lands_when_ffable_and_free() {
        assert_eq!(
            next_action(&landable("a", Phase::Executing, true), false),
            Some(Transition::BeginEvergreening {
                plan: "a".to_string()
            })
        );
        assert_eq!(
            next_action(&landable("b", Phase::Executing, false), false),
            Some(Transition::Finish {
                plan: "b".to_string()
            })
        );
    }

    #[test]
    fn empty_executing_holds_for_input() {
        // No commits: never Queue, Begin, or Finish, whatever the branch.
        for view in [
            view("a", Phase::Executing),
            PlanView {
                ffable: true,
                ..view("b", Phase::Executing)
            },
            PlanView {
                evergreen: false,
                ffable: true,
                ..view("c", Phase::Executing)
            },
        ] {
            assert_eq!(next_action(&view, false), None, "{} holds empty", view.name);
            assert_eq!(
                next_action(&view, true),
                None,
                "{} holds empty while held",
                view.name
            );
        }
    }

    #[test]
    fn queued_lands_or_rebases_when_free() {
        // Diverged plans rebase; fast-forwardable plans skip rebasing.
        assert_eq!(
            next_action(&committed("a", Phase::Queued), false),
            Some(Transition::BeginRebasing {
                plan: "a".to_string()
            })
        );
        assert_eq!(
            next_action(&landable("b", Phase::Queued, true), false),
            Some(Transition::BeginEvergreening {
                plan: "b".to_string()
            })
        );
        assert_eq!(
            next_action(&landable("c", Phase::Queued, false), false),
            Some(Transition::Finish {
                plan: "c".to_string()
            })
        );
    }

    #[test]
    fn empty_queued_holds_for_input() {
        // No commits: never Begin or Finish, whatever the branch.
        for view in [
            view("a", Phase::Queued),
            PlanView {
                ffable: true,
                ..view("b", Phase::Queued)
            },
            PlanView {
                evergreen: false,
                ffable: true,
                has_commits: false,
                ..view("c", Phase::Queued)
            },
        ] {
            assert_eq!(next_action(&view, false), None, "{} holds empty", view.name);
            assert_eq!(
                next_action(&view, true),
                None,
                "{} holds empty while held",
                view.name
            );
        }
    }

    #[test]
    fn queued_waits_on_mutex_even_when_ffable() {
        for view in [
            committed("a", Phase::Queued),
            landable("b", Phase::Queued, true),
            landable("c", Phase::Queued, false),
        ] {
            assert_eq!(
                next_action(&view, true),
                None,
                "{} waits while held",
                view.name
            );
        }
    }

    #[test]
    fn queued_holds_when_dirty_or_unlisted() {
        let dirty = PlanView {
            dirty: true,
            ..landable("a", Phase::Queued, true)
        };
        assert_eq!(next_action(&dirty, false), None);
        let unlisted = PlanView {
            queue_position: None,
            ..landable("b", Phase::Queued, true)
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
            Some(Transition::Finish {
                plan: "a".to_string()
            })
        );
        let empty = PlanView {
            ffable: true,
            ..view("b", Phase::Rebasing)
        };
        assert_eq!(
            next_action(&empty, false),
            Some(Transition::Finish {
                plan: "b".to_string()
            })
        );
        // Finishing never needs the mutex: the plan already holds it.
        assert_eq!(
            next_action(&empty, true),
            Some(Transition::Finish {
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
            Some(Transition::Finish {
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
