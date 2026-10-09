// Automatic plan progression: one pump, one pure decision, one shared core.
// The pump scans automatic idle plans in phase then arrival order and fires
// at most one transition per plan, serializing landing behind the scenes.
// Scoping never rides the pump: approving a plan stays a manual `>` click,
// and only executing, evergreening, and landing plans carry the
// auto-manual switch. Executing plans with evergreen intent and fresh
// commits clean up in evergreening first; empty work means nothing changed
// and the plan takes the landing path as if intent were off.
// `next_action` is pure over a plain view (unit-tested with no git repo);
// `transition` is the single effectful core that the manual commands and
// the pump all go through, so the pump adds no second copy of any step.
use std::collections::HashSet;

use crate::opencode;
use crate::plans::{self, Phase};
use crate::types::{AgentError, PlansUpdate, SessionKey, SessionRole};

use super::session::ActivePlan;
use super::{AgentManager, lock_state};

/// Pump: advance every automatic idle executing, evergreening, or landing
/// plan that is ready, oldest arrivals first, at most one transition per
/// plan per pass. Scoping plans always wait for the manual `>` click and
/// never fire here. Landing serializes by only ever moving one waiter into
/// `landing/` at a time; fast finishes never touch `landing/` so they
/// proceed even while a rebase is active. Evergreening needs no gate:
/// worktrees are independent.
// Dirty or diverged plans hold and are skipped until a later trigger,
// showing only their existing markers. A landing whose target moved under
// it re-rebases automatically while still on A. Background plans never
// steal the selection; the selected plan follows its own moves.
impl AgentManager {
    /// Fire-and-forget pump for background triggers (turn ends, branch
    /// moves). The pump future is not `Send` (it drives the turn
    /// machinery), so it runs blocking on its own thread instead of the
    /// async runtime.
    pub(crate) fn spawn_pump(&self) {
        let manager = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            tauri::async_runtime::block_on(manager.pump());
        });
    }

    pub(crate) async fn pump(&self) {
        let mut selected = lock_state(&self.state).and_then(|state| state.current.clone());
        let mut fired_any = false;
        // Every fired transition makes its plan ineligible for the next
        // pass (its turn starts, or it leaves the phase), so the pump
        // settles on its own. The set below only backstops that invariant:
        // a plan firing twice means a transition stopped sticking.
        let mut fired_plans = HashSet::new();
        'pump: while let Some((repo_root, _)) = self.reopen_snapshot() {
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
            let mut fired = false;
            for plan in &scanned {
                let view = self.plan_view(&repo_root, plan);
                let Some(action) = next_action(&view) else {
                    continue;
                };
                let plan_name = action.plan_name().to_string();
                match self.transition(action).await {
                    Ok(_) => {
                        fired = true;
                        fired_any = true;
                        if !fired_plans.insert(plan_name.clone()) {
                            log::warn!("pump fired twice for {plan_name}; stopping");
                            break 'pump;
                        }
                        // The selected plan follows its own moves; background
                        // plans never steal the selection.
                        match selected {
                            Some(ref current) if current.plan == plan_name => {
                                selected =
                                    lock_state(&self.state).and_then(|state| state.current.clone());
                            }
                            Some(ref current) => {
                                self.select_key(current.clone());
                            }
                            None => {}
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
            self.push_plans();
        }
    }

    /// Plain view of one plan for the pure decision. Worktree probes fail
    /// safe (dirty holds, ffable false, no commits) with a warn-log, per
    /// the preserve-evidence rule.
    fn plan_view(&self, repo_root: &std::path::Path, plan: &plans::PlanRef) -> PlanView {
        let stored = plans::load_state(&plan.path(repo_root));
        let manual = stored.manual;
        let evergreen = stored.evergreen;
        let role = match plan.phase {
            Phase::Scoping => SessionRole::Scoping,
            Phase::Executing => SessionRole::Executing,
            Phase::Evergreening => SessionRole::Evergreening,
            Phase::Landing => SessionRole::Landing,
            Phase::Completed | Phase::Cancelled => SessionRole::Scoping,
        };
        let idle = lock_state(&self.state)
            .map(|state| {
                state
                    .sessions
                    .get(&SessionKey {
                        plan: plan.name.clone(),
                        role,
                    })
                    .map(|live| !live.working)
                    .unwrap_or(true)
            })
            .unwrap_or(true);
        let (dirty, ffable, has_commits) = match plan.phase {
            Phase::Executing | Phase::Evergreening => {
                match self.worktree_status_for(repo_root, &plan.name) {
                    Ok(status) => (status.dirty, status.ffable, !status.commits.is_empty()),
                    Err(error) => {
                        log::warn!("automatic view for {} held: {error}", plan.name);
                        (true, false, false)
                    }
                }
            }
            Phase::Landing => match self.worktree_status_for(repo_root, &plan.name) {
                Ok(status) => (status.dirty, status.ffable, false),
                Err(error) => {
                    log::warn!("automatic view for {} held: {error}", plan.name);
                    (true, false, false)
                }
            },
            Phase::Scoping | Phase::Completed | Phase::Cancelled => (false, false, false),
        };
        let landing_active = plans::scan_plans(repo_root)
            .iter()
            .any(|other| other.phase == Phase::Landing && other.name != plan.name);
        PlanView {
            name: plan.name.clone(),
            phase: plan.phase,
            manual,
            evergreen,
            idle,
            dirty,
            ffable,
            has_commits,
            landing_active,
        }
    }

    /// Clean ffable check for a landing plan: true when a rebase that
    /// finds its landing ffable after all should finish instead. Dirty or
    /// missing plans report false so the caller falls through to the
    /// normal branch, which refuses loud with the usual message.
    fn landing_is_ffable(&self, plan: &str) -> Result<bool, AgentError> {
        let Some((repo_root, _)) = self.reopen_snapshot() else {
            return Err(AgentError::NoSession {
                raw: "open a repository first".to_string(),
            });
        };
        let from = plans::PlanRef {
            name: plan.to_string(),
            phase: Phase::Landing,
        };
        if !from.path(&repo_root).is_dir() {
            return Ok(false);
        }
        match self.worktree_status_for(&repo_root, plan) {
            Ok(status) => Ok(!status.dirty && status.ffable),
            Err(_) => Ok(false),
        }
    }

    /// Shared transition core: idle gate, repo snapshot, `PlanRef` build,
    /// existence check, live-session drop, prompted carry, selection, and
    /// plans update happen once here for every entry point. The manual
    /// commands map their session onto a `Transition` and delegate; the
    /// pump calls this directly.
    pub(crate) async fn transition(&self, action: Transition) -> Result<PlansUpdate, AgentError> {
        // A rebase that finds its landing ffable after all finishes
        // instead. Redirected up front so the core below stays a single
        // straight match with no recursion.
        let action = match action {
            Transition::RebaseLanding { plan } if self.landing_is_ffable(&plan)? => {
                Transition::FinishLanding { plan }
            }
            action => action,
        };
        match action {
            Transition::Execute { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Scoping,
                };
                super::ensure_idle(&self.state, &session)?;
                let (repo_root, branch) =
                    self.reopen_snapshot()
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
                match self.state.lock() {
                    Ok(mut state) => {
                        state.worktrees.insert(next.name.clone(), record);
                    }
                    Err(error) => {
                        log::warn!("failed to record worktree: {error}");
                    }
                }
                self.drop_live(&session).await;
                self.carry_prompted(&from.name, &next.name);
                let mut active = ActivePlan::executing(next.name.clone());
                // The role goes out as the full first turn below, so later
                // turns never prefix again.
                active.prefixed = true;
                let plan_dir_abs = next.path(&repo_root).to_string_lossy().to_string();
                let text = opencode::executing_first_message(&plan_dir_abs);
                self.spawn_with_first_turn(&repo_root, &branch, active, text)
                    .await
            }
            Transition::BeginEvergreening { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Executing,
                };
                super::ensure_idle(&self.state, &session)?;
                let (repo_root, branch) =
                    self.reopen_snapshot()
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let from = plans::PlanRef {
                    name: plan.clone(),
                    phase: Phase::Executing,
                };
                if !from.path(&repo_root).is_dir() {
                    return Err(AgentError::RequestFailed {
                        raw: "no executing plan to evergreen".to_string(),
                    });
                }
                // Same worktree, so no landing-busy gate and no rebase
                // precondition; a dirty tree still refuses so user edits
                // never mix into the cleanup.
                let status = self.worktree_status_for(&repo_root, &from.name)?;
                if status.dirty {
                    return Err(AgentError::RequestFailed {
                        raw: "commit or discard worktree changes first".to_string(),
                    });
                }
                let next = plans::begin_evergreening(&repo_root, &from)?;
                self.drop_live(&session).await;
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
                self.spawn_with_first_turn(&repo_root, &branch, active, text)
                    .await
            }
            Transition::BeginLanding { plan } => {
                let (repo_root, branch) =
                    self.reopen_snapshot()
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                // The work phase travels: diverged executing plans land
                // straight from executing, diverged evergreening plans from
                // evergreening. Names stay unique across phases.
                let (from, session) = {
                    let executing = plans::PlanRef {
                        name: plan.clone(),
                        phase: Phase::Executing,
                    };
                    if executing.path(&repo_root).is_dir() {
                        let session = SessionKey {
                            plan: plan.clone(),
                            role: SessionRole::Executing,
                        };
                        (executing, session)
                    } else {
                        let evergreening = plans::PlanRef {
                            name: plan.clone(),
                            phase: Phase::Evergreening,
                        };
                        if !evergreening.path(&repo_root).is_dir() {
                            return Err(AgentError::RequestFailed {
                                raw: "no executing plan to land".to_string(),
                            });
                        }
                        let session = SessionKey {
                            plan: plan.clone(),
                            role: SessionRole::Evergreening,
                        };
                        (evergreening, session)
                    }
                };
                super::ensure_idle(&self.state, &session)?;
                if plans::scan_plans(&repo_root)
                    .iter()
                    .any(|other| other.phase == Phase::Landing)
                {
                    return Err(AgentError::RequestFailed {
                        raw: "landing is busy, waiting its turn".to_string(),
                    });
                }
                let status = self.worktree_status_for(&repo_root, &from.name)?;
                super::gate_landing(status.dirty, status.ffable, super::LandingStep::Begin)?;
                let next = plans::begin_landing(&repo_root, &from)?;
                self.drop_live(&session).await;
                let mut active = ActivePlan::landing(next.name.clone());
                // The role goes out as the full first turn below, so later
                // turns never prefix again.
                active.prefixed = true;
                let plan_md_abs = next.plan_md(&repo_root).to_string_lossy().to_string();
                let text = opencode::landing_first_message(
                    &status.worktree_branch,
                    &status.target_branch,
                    &status.path.to_string_lossy(),
                    &plan_md_abs,
                );
                self.spawn_with_first_turn(&repo_root, &branch, active, text)
                    .await
            }
            Transition::FinishExecuting { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Executing,
                };
                super::ensure_idle(&self.state, &session)?;
                let (repo_root, _) =
                    self.reopen_snapshot()
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let from = plans::PlanRef {
                    name: plan.clone(),
                    phase: Phase::Executing,
                };
                if !from.path(&repo_root).is_dir() {
                    return Err(AgentError::RequestFailed {
                        raw: "no active plan to complete".to_string(),
                    });
                }
                let status = self.worktree_status_for(&repo_root, &from.name)?;
                super::gate_landing(status.dirty, status.ffable, super::LandingStep::Finish)?;
                crate::worktrees::fast_forward(
                    &repo_root,
                    &status.target_branch,
                    &status.worktree_branch,
                )?;
                self.remove_worktree(&repo_root, &from.name, false)?;
                let next = plans::complete(&repo_root, &from)?;
                self.drop_plan_lives(&from.name).await;
                self.carry_prompted(&from.name, &next.name);
                self.select_key(SessionKey {
                    plan: next.name,
                    role: SessionRole::Executing,
                });
                Ok(self.plans_update())
            }
            Transition::FinishEvergreening { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Evergreening,
                };
                super::ensure_idle(&self.state, &session)?;
                let (repo_root, _) =
                    self.reopen_snapshot()
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
                let status = self.worktree_status_for(&repo_root, &from.name)?;
                super::gate_landing(status.dirty, status.ffable, super::LandingStep::Finish)?;
                crate::worktrees::fast_forward(
                    &repo_root,
                    &status.target_branch,
                    &status.worktree_branch,
                )?;
                self.remove_worktree(&repo_root, &from.name, false)?;
                let next = plans::complete(&repo_root, &from)?;
                self.drop_plan_lives(&from.name).await;
                self.carry_prompted(&from.name, &next.name);
                self.select_key(SessionKey {
                    plan: next.name,
                    role: SessionRole::Evergreening,
                });
                Ok(self.plans_update())
            }
            Transition::FinishLanding { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Landing,
                };
                super::ensure_idle(&self.state, &session)?;
                let (repo_root, _) =
                    self.reopen_snapshot()
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let from = plans::PlanRef {
                    name: plan.clone(),
                    phase: Phase::Landing,
                };
                if !from.path(&repo_root).is_dir() {
                    return Err(AgentError::RequestFailed {
                        raw: "no active plan to complete".to_string(),
                    });
                }
                let status = self.worktree_status_for(&repo_root, &from.name)?;
                super::gate_landing(status.dirty, status.ffable, super::LandingStep::Finish)?;
                crate::worktrees::fast_forward(
                    &repo_root,
                    &status.target_branch,
                    &status.worktree_branch,
                )?;
                self.remove_worktree(&repo_root, &from.name, false)?;
                let next = plans::finish_landing(&repo_root, &from)?;
                self.drop_plan_lives(&from.name).await;
                self.carry_prompted(&from.name, &next.name);
                self.select_key(SessionKey {
                    plan: next.name,
                    role: SessionRole::Landing,
                });
                Ok(self.plans_update())
            }
            Transition::RebaseLanding { plan } => {
                let session = SessionKey {
                    plan: plan.clone(),
                    role: SessionRole::Landing,
                };
                super::ensure_idle(&self.state, &session)?;
                let (repo_root, _) =
                    self.reopen_snapshot()
                        .ok_or_else(|| AgentError::NoSession {
                            raw: "open a repository first".to_string(),
                        })?;
                let from = plans::PlanRef {
                    name: plan.clone(),
                    phase: Phase::Landing,
                };
                if !from.path(&repo_root).is_dir() {
                    return Err(AgentError::RequestFailed {
                        raw: "no active plan to complete".to_string(),
                    });
                }
                let status = self.worktree_status_for(&repo_root, &from.name)?;
                if status.dirty {
                    return Err(AgentError::RequestFailed {
                        raw: "commit or discard worktree changes first".to_string(),
                    });
                }
                let plan_md_abs = from.plan_md(&repo_root).to_string_lossy().to_string();
                let text = opencode::landing_first_message(
                    &status.worktree_branch,
                    &status.target_branch,
                    &status.path.to_string_lossy(),
                    &plan_md_abs,
                );
                let (connection, session_id) = self.ensure_live(&session).await?;
                self.mark_prompted(&from.name);
                self.start_turn(connection, session_id, session.clone(), text.clone())
                    .await?;
                self.announce_first_prompt(&session, &text);
                Ok(self.plans_update())
            }
        }
    }

    /// Shared spawn-plus-first-turn for the three agent-start transitions
    /// (`Execute`, `BeginEvergreening`, `BeginLanding`): spawn the session
    /// for the caller's `ActivePlan`, send its role as the full first turn,
    /// and announce it as a live user bubble. The caller moves the plan and
    /// drops the old live session first; the plan name travels inside
    /// `active` so the prompted flag follows it.
    async fn spawn_with_first_turn(
        &self,
        repo_root: &std::path::Path,
        branch: &str,
        active: ActivePlan,
        text: String,
    ) -> Result<PlansUpdate, AgentError> {
        let plan_name = active.name.clone();
        let agent = opencode::agent_for(active.phase);
        let (connection, session_id, key) =
            self.spawn_session(repo_root, branch, active, agent).await?;
        self.mark_prompted(&plan_name);
        self.start_turn(connection, session_id, key.clone(), text.clone())
            .await?;
        self.announce_first_prompt(&key, &text);
        Ok(self.plans_update())
    }
}

/// One transition the pump or a manual command can perform. At most one per
/// plan per pump pass. `Execute` is manual-only (the header `>` click) and
/// never fires from the pump; `BeginEvergreening` starts the cleanup agent
/// in the same worktree; `FinishExecuting` and `FinishEvergreening` are the
/// fast paths straight to completed when ffable; `RebaseLanding` re-runs
/// the rebase when the target moved under an active landing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Transition {
    Execute { plan: String },
    BeginEvergreening { plan: String },
    BeginLanding { plan: String },
    FinishExecuting { plan: String },
    FinishEvergreening { plan: String },
    FinishLanding { plan: String },
    RebaseLanding { plan: String },
}

impl Transition {
    fn plan_name(&self) -> &str {
        match self {
            Transition::Execute { plan }
            | Transition::BeginEvergreening { plan }
            | Transition::BeginLanding { plan }
            | Transition::FinishExecuting { plan }
            | Transition::FinishEvergreening { plan }
            | Transition::FinishLanding { plan }
            | Transition::RebaseLanding { plan } => plan,
        }
    }
}

/// Plain view of one plan for the pure decision: phase, stored mode and
/// evergreen intent, idle (no turn running), worktree dirtiness and
/// fast-forwardability, commit presence, and whether another landing
/// already occupies `landing/`.
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
    pub landing_active: bool,
}

/// Pure automation decision over one plan: at most one transition. Scoping
/// always waits for the manual `>` click and never fires here. Manual
/// plans hold; running turns hold; dirty worktrees hold. Executing plans
/// with evergreen intent and fresh commits clean up first; without intent
/// or without commits they take today's finish-or-land path. Evergreening
/// plans finish directly when ffable, else wait while another landing
/// occupies `landing/`; clean non-ffable landings rebase again (the
/// moved-target case). Pure.
pub(crate) fn next_action(view: &PlanView) -> Option<Transition> {
    if view.manual || !view.idle {
        return None;
    }
    match view.phase {
        Phase::Scoping | Phase::Completed | Phase::Cancelled => None,
        Phase::Executing => {
            if view.dirty {
                None
            } else if view.evergreen && view.has_commits {
                Some(Transition::BeginEvergreening {
                    plan: view.name.clone(),
                })
            } else if view.ffable {
                Some(Transition::FinishExecuting {
                    plan: view.name.clone(),
                })
            } else if view.landing_active {
                None
            } else {
                Some(Transition::BeginLanding {
                    plan: view.name.clone(),
                })
            }
        }
        Phase::Evergreening => {
            if view.dirty {
                None
            } else if view.ffable {
                Some(Transition::FinishEvergreening {
                    plan: view.name.clone(),
                })
            } else if view.landing_active {
                None
            } else {
                Some(Transition::BeginLanding {
                    plan: view.name.clone(),
                })
            }
        }
        Phase::Landing => {
            if view.dirty {
                None
            } else if view.ffable {
                Some(Transition::FinishLanding {
                    plan: view.name.clone(),
                })
            } else {
                Some(Transition::RebaseLanding {
                    plan: view.name.clone(),
                })
            }
        }
    }
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
            landing_active: false,
        }
    }

    #[test]
    fn scoping_always_waits_for_the_button() {
        assert_eq!(next_action(&view("a", Phase::Scoping)), None);
        assert_eq!(
            next_action(&PlanView {
                ffable: true,
                ..view("a", Phase::Scoping)
            }),
            None
        );
        assert_eq!(
            next_action(&PlanView {
                idle: false,
                ..view("a", Phase::Scoping)
            }),
            None
        );
    }

    #[test]
    fn manual_plans_hold_in_place() {
        for phase in [
            Phase::Scoping,
            Phase::Executing,
            Phase::Evergreening,
            Phase::Landing,
        ] {
            let held = PlanView {
                manual: true,
                ffable: true,
                has_commits: true,
                ..view("a", phase)
            };
            assert_eq!(next_action(&held), None, "{phase:?} holds on M");
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
        assert_eq!(next_action(&working), None);
        let greening = PlanView {
            idle: false,
            ffable: true,
            ..view("a", Phase::Evergreening)
        };
        assert_eq!(next_action(&greening), None);
    }

    #[test]
    fn executing_cleans_up_first_with_intent_and_commits() {
        let greening = PlanView {
            has_commits: true,
            ..view("a", Phase::Executing)
        };
        assert_eq!(
            next_action(&greening),
            Some(Transition::BeginEvergreening {
                plan: "a".to_string()
            })
        );
    }

    #[test]
    fn evergreening_needs_no_landing_gate() {
        let busy = PlanView {
            has_commits: true,
            landing_active: true,
            ..view("a", Phase::Executing)
        };
        assert_eq!(
            next_action(&busy),
            Some(Transition::BeginEvergreening {
                plan: "a".to_string()
            })
        );
    }

    #[test]
    fn executing_without_commits_takes_the_landing_path() {
        let done = PlanView {
            ffable: true,
            landing_active: true,
            ..view("a", Phase::Executing)
        };
        assert_eq!(
            next_action(&done),
            Some(Transition::FinishExecuting {
                plan: "a".to_string()
            })
        );
        let diverged = view("a", Phase::Executing);
        assert_eq!(
            next_action(&diverged),
            Some(Transition::BeginLanding {
                plan: "a".to_string()
            })
        );
    }

    #[test]
    fn executing_without_intent_takes_the_landing_path() {
        let done = PlanView {
            evergreen: false,
            has_commits: true,
            ffable: true,
            landing_active: true,
            ..view("a", Phase::Executing)
        };
        assert_eq!(
            next_action(&done),
            Some(Transition::FinishExecuting {
                plan: "a".to_string()
            })
        );
        let diverged = PlanView {
            evergreen: false,
            has_commits: true,
            ..view("a", Phase::Executing)
        };
        assert_eq!(
            next_action(&diverged),
            Some(Transition::BeginLanding {
                plan: "a".to_string()
            })
        );
    }

    #[test]
    fn executing_finishes_directly_when_ffable() {
        let done = PlanView {
            ffable: true,
            landing_active: true,
            ..view("a", Phase::Executing)
        };
        assert_eq!(
            next_action(&done),
            Some(Transition::FinishExecuting {
                plan: "a".to_string()
            })
        );
    }

    #[test]
    fn executing_waits_serially_while_landing_is_active() {
        let waiter = PlanView {
            landing_active: true,
            ..view("b", Phase::Executing)
        };
        assert_eq!(next_action(&waiter), None);
        let first = view("a", Phase::Executing);
        assert_eq!(
            next_action(&first),
            Some(Transition::BeginLanding {
                plan: "a".to_string()
            })
        );
    }

    #[test]
    fn evergreening_finishes_directly_when_ffable() {
        let done = PlanView {
            ffable: true,
            landing_active: true,
            ..view("a", Phase::Evergreening)
        };
        assert_eq!(
            next_action(&done),
            Some(Transition::FinishEvergreening {
                plan: "a".to_string()
            })
        );
    }

    #[test]
    fn evergreening_lands_once_diverged_and_free() {
        let diverged = view("a", Phase::Evergreening);
        assert_eq!(
            next_action(&diverged),
            Some(Transition::BeginLanding {
                plan: "a".to_string()
            })
        );
        let waiter = PlanView {
            landing_active: true,
            ..view("b", Phase::Evergreening)
        };
        assert_eq!(next_action(&waiter), None);
    }

    #[test]
    fn dirty_plans_hold_for_the_user() {
        let dirty_exec = PlanView {
            dirty: true,
            has_commits: true,
            ..view("a", Phase::Executing)
        };
        assert_eq!(next_action(&dirty_exec), None);
        let dirty_green = PlanView {
            dirty: true,
            ffable: true,
            ..view("c", Phase::Evergreening)
        };
        assert_eq!(next_action(&dirty_green), None);
        let dirty_land = PlanView {
            dirty: true,
            ffable: true,
            ..view("b", Phase::Landing)
        };
        assert_eq!(next_action(&dirty_land), None);
    }

    #[test]
    fn landing_finishes_once_ffable() {
        let done = PlanView {
            ffable: true,
            ..view("a", Phase::Landing)
        };
        assert_eq!(
            next_action(&done),
            Some(Transition::FinishLanding {
                plan: "a".to_string()
            })
        );
    }

    #[test]
    fn moved_target_rebases_while_still_automatic() {
        let moved = view("a", Phase::Landing);
        assert_eq!(
            next_action(&moved),
            Some(Transition::RebaseLanding {
                plan: "a".to_string()
            })
        );
        let held = PlanView {
            manual: true,
            ..view("a", Phase::Landing)
        };
        assert_eq!(next_action(&held), None);
    }

    #[test]
    fn finished_plans_never_move() {
        for phase in [Phase::Completed, Phase::Cancelled] {
            let done = PlanView {
                ffable: true,
                ..view("a", phase)
            };
            assert_eq!(next_action(&done), None, "{phase:?} stays put");
        }
    }
}
