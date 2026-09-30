// Plan directory lifecycle under `.samokod/plans/`.
// Lifecycle transitions first, per-plan state plus naming and filesystem
// helpers below.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::types::SessionRole;

/// Lifecycle phase. One directory per phase under `.samokod/plans/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Scoping,
    Executing,
    Merging,
    Completed,
    Cancelled,
}

impl Phase {
    pub fn dir_name(self) -> &'static str {
        match self {
            Phase::Scoping => "scoping",
            Phase::Executing => "executing",
            Phase::Merging => "merging",
            Phase::Completed => "completed",
            Phase::Cancelled => "cancelled",
        }
    }

    /// Phases with a live chat. Only these auto-cancel on chat switch.
    pub fn is_active(self) -> bool {
        matches!(self, Phase::Scoping | Phase::Executing | Phase::Merging)
    }
}

/// A plan directory inside a phase. `name` starts with the creation
/// timestamp and gains a slug on execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRef {
    pub name: String,
    pub phase: Phase,
}

impl PlanRef {
    /// Repo-relative edit scope for one plan dir, as OpenCode's `edit`
    /// tool sees it. Rule order stays load-bearing, see `opencode.rs`.
    pub fn scope_glob(&self) -> String {
        format!(".samokod/plans/{}/{}/**", self.phase.dir_name(), self.name)
    }

    pub fn path(&self, repo_root: &Path) -> PathBuf {
        phase_dir(repo_root, self.phase).join(&self.name)
    }

    pub fn plan_md(&self, repo_root: &Path) -> PathBuf {
        self.path(repo_root).join("plan.md")
    }

    pub fn has_plan_md(&self, repo_root: &Path) -> bool {
        self.plan_md(repo_root).is_file()
    }
}

/// Per-plan state file. It travels with the plan directory through renames
/// for free and stays machine-local through the existing
/// `.samokod/.gitignore`.
pub const STATE_FILE: &str = "state.json";

/// Legacy session-only file. Read for migration, removed on the next write.
pub(crate) const LEGACY_SESSION_FILE: &str = "session.json";

/// Per-plan state: one OpenCode session ID per role the plan has used,
/// plus the last phase-entry time in epoch millis. `entered_at` stamps the
/// last lifecycle transition into this phase; session spawns preserve it
/// while transitions preserve the IDs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanState {
    #[serde(default)]
    pub scoping: Option<String>,
    #[serde(default)]
    pub executing: Option<String>,
    #[serde(default)]
    pub merging: Option<String>,
    #[serde(default)]
    pub entered_at: Option<i64>,
}

impl PlanState {
    /// Session ID for one role. Pure.
    pub(crate) fn session(&self, role: SessionRole) -> Option<&str> {
        match role {
            SessionRole::Scoping => self.scoping.as_deref(),
            SessionRole::Executing => self.executing.as_deref(),
            SessionRole::Merging => self.merging.as_deref(),
        }
    }

    /// Record one role's session ID. Pure.
    pub(crate) fn set_session(&mut self, role: SessionRole, id: String) {
        match role {
            SessionRole::Scoping => self.scoping = Some(id),
            SessionRole::Executing => self.executing = Some(id),
            SessionRole::Merging => self.merging = Some(id),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("plan.md has no top heading to name the plan")]
    Untitled,
    #[error("plan is not {expected}, it is {actual}")]
    WrongPhase {
        expected: &'static str,
        actual: &'static str,
    },
    #[error("plan storage failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Create the full structure plus the self-ignoring `.gitignore`.
/// Idempotent: safe to run on every repo open.
pub fn ensure_structure(repo_root: &Path) -> Result<(), PlanError> {
    for phase in [
        Phase::Scoping,
        Phase::Executing,
        Phase::Merging,
        Phase::Completed,
        Phase::Cancelled,
    ] {
        std::fs::create_dir_all(phase_dir(repo_root, phase))?;
    }
    std::fs::write(samokod_dir(repo_root).join(".gitignore"), "*\n")?;
    Ok(())
}

/// Materialize a reserved scoping session on first send. Idempotent:
/// tolerates a race-created dir and returns the scoping `PlanRef`.
/// Fails loud on real IO errors.
pub fn materialize_scoping(repo_root: &Path, name: &str) -> Result<PlanRef, PlanError> {
    std::fs::create_dir_all(phase_dir(repo_root, Phase::Scoping).join(name))?;
    let plan = PlanRef {
        name: name.to_string(),
        phase: Phase::Scoping,
    };
    stamp_entered_at(&plan.path(repo_root));
    Ok(plan)
}

/// Approve a scoping plan: slugify its first heading and move it to
/// executing. Fails loud without `plan.md` or without a heading.
pub fn execute(repo_root: &Path, plan: &PlanRef) -> Result<PlanRef, PlanError> {
    require_phase(plan, Phase::Scoping)?;
    let text = std::fs::read_to_string(plan.plan_md(repo_root)).map_err(|error| {
        PlanError::Io(std::io::Error::new(
            error.kind(),
            format!("cannot read {}: {error}", plan.plan_md(repo_root).display()),
        ))
    })?;
    let title = extract_title(&text).ok_or(PlanError::Untitled)?;
    let next = PlanRef {
        name: format!("{}.{}", plan.name, slugify(&title)),
        phase: Phase::Executing,
    };
    rename(repo_root, plan, &next)?;
    Ok(next)
}

/// Finish an executing plan. Name travels unchanged.
pub fn complete(repo_root: &Path, plan: &PlanRef) -> Result<PlanRef, PlanError> {
    require_phase(plan, Phase::Executing)?;
    let next = PlanRef {
        name: plan.name.clone(),
        phase: Phase::Completed,
    };
    rename(repo_root, plan, &next)?;
    Ok(next)
}

/// Move a diverged executing plan to merging without renaming. The fast
/// path never touches `merging/`: it completes straight from executing.
pub fn begin_merge(repo_root: &Path, plan: &PlanRef) -> Result<PlanRef, PlanError> {
    require_phase(plan, Phase::Executing)?;
    let next = PlanRef {
        name: plan.name.clone(),
        phase: Phase::Merging,
    };
    rename(repo_root, plan, &next)?;
    Ok(next)
}

/// Finish a rebased merging plan. Name travels unchanged.
pub fn finish_merge(repo_root: &Path, plan: &PlanRef) -> Result<PlanRef, PlanError> {
    require_phase(plan, Phase::Merging)?;
    let next = PlanRef {
        name: plan.name.clone(),
        phase: Phase::Completed,
    };
    rename(repo_root, plan, &next)?;
    Ok(next)
}

/// Cancel an active plan. Scoping plans gain a slug when they have a heading,
/// everything else keeps its name.
pub fn cancel(repo_root: &Path, plan: &PlanRef) -> Result<PlanRef, PlanError> {
    if !plan.phase.is_active() {
        return Err(PlanError::WrongPhase {
            expected: "an active plan",
            actual: plan.phase.dir_name(),
        });
    }
    let name = match plan.phase {
        Phase::Scoping => slugged_name(repo_root, plan).unwrap_or_else(|| plan.name.clone()),
        _ => plan.name.clone(),
    };
    let next = PlanRef {
        name,
        phase: Phase::Cancelled,
    };
    rename(repo_root, plan, &next)?;
    Ok(next)
}

/// Slug suffix for a scoping plan with a titled `plan.md`.
/// Missing files and untitled plans yield no slug; the caller keeps the
/// bare name. Pure except the read.
fn slugged_name(repo_root: &Path, plan: &PlanRef) -> Option<String> {
    let text = std::fs::read_to_string(plan.plan_md(repo_root)).ok()?;
    let title = extract_title(&text)?;
    Some(format!("{}.{}", plan.name, slugify(&title)))
}

/// Read the persisted state for one plan directory. Missing files yield a
/// default; corrupt files log and yield a default so a broken file never
/// blocks a fresh spawn. Legacy `session.json` files read as state without
/// a timestamp.
pub(crate) fn load_state(plan_dir: &Path) -> PlanState {
    let path = plan_dir.join(STATE_FILE);
    if let Some(state) = read_state_file(&path) {
        return state;
    }
    let legacy = plan_dir.join(LEGACY_SESSION_FILE);
    if let Some(state) = read_state_file(&legacy) {
        return state;
    }
    PlanState::default()
}

/// Persist the state for one plan directory. Best-effort: failures log
/// and the live session continues. A migrated legacy file is removed once
/// the new file lands.
pub(crate) fn store_state(plan_dir: &Path, state: &PlanState) {
    let text = match serde_json::to_string_pretty(state) {
        Ok(text) => text,
        Err(error) => {
            log::warn!("failed to serialize state {}: {error}", plan_dir.display());
            return;
        }
    };
    if let Err(error) = std::fs::write(plan_dir.join(STATE_FILE), text) {
        log::warn!("failed to record state {}: {error}", plan_dir.display());
        return;
    }
    let legacy = plan_dir.join(LEGACY_SESSION_FILE);
    if legacy.is_file()
        && let Err(error) = std::fs::remove_file(&legacy)
    {
        log::warn!("failed to remove legacy {}: {error}", legacy.display());
    }
}

/// Stamp the phase-entry clock for one plan directory, preserving session
/// IDs. Best-effort: a missing directory skips, failures log and the
/// transition stands.
fn stamp_entered_at(plan_dir: &Path) {
    if !plan_dir.is_dir() {
        return;
    }
    let mut state = load_state(plan_dir);
    state.entered_at = Some(unix_ms(std::time::SystemTime::now()).unwrap_or(0));
    store_state(plan_dir, &state);
}

/// Epoch millis for a timestamp. Pure.
pub(crate) fn unix_ms(time: std::time::SystemTime) -> Option<i64> {
    time.duration_since(std::time::SystemTime::UNIX_EPOCH)
        .ok()
        .map(|span| span.as_millis() as i64)
}

/// Arrival time for sorting: the stamped phase entry when present, else
/// the directory `ctime` (a rename bumps `ctime` but not `mtime`), else
/// the existing `plan_mtime`, else `None` so the caller falls back to the
/// name. Pure except the metadata probes.
pub(crate) fn arrival_ms(repo_root: &Path, plan: &PlanRef) -> Option<i64> {
    let dir = plan.path(repo_root);
    if let Some(stamped) = load_state(&dir).entered_at {
        return Some(stamped);
    }
    if let Some(ctime) = dir_ctime_ms(&dir) {
        return Some(ctime);
    }
    plan_mtime(repo_root, plan).and_then(unix_ms)
}

/// Directory `ctime` in epoch millis. Only a fallback for plans without
/// stamped state: later files inside the plan bump it again.
#[cfg(unix)]
fn dir_ctime_ms(path: &Path) -> Option<i64> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::metadata(path).ok()?;
    Some(meta.ctime() * 1000 + meta.ctime_nsec() / 1_000_000)
}

#[cfg(not(unix))]
fn dir_ctime_ms(_path: &Path) -> Option<i64> {
    None
}

fn read_state_file(path: &Path) -> Option<PlanState> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            log::warn!("failed to read {}: {error}", path.display());
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(state) => Some(state),
        Err(error) => {
            log::warn!("failed to parse {}: {error}", path.display());
            None
        }
    }
}
/// Sort rank for the plans list: scoping, executing, merging,
/// completed, cancelled.
pub fn phase_rank(phase: Phase) -> u8 {
    match phase {
        Phase::Scoping => 0,
        Phase::Executing => 1,
        Phase::Merging => 2,
        Phase::Completed => 3,
        Phase::Cancelled => 4,
    }
}

/// Session roles one plan owns: scoping always, executing once approved,
/// merging on the conflict path. Completed and cancelled plans keep their
/// rows as history from the stored session IDs. Pure except the state read.
pub fn roles_for(repo_root: &Path, plan: &PlanRef) -> Vec<SessionRole> {
    match plan.phase {
        Phase::Scoping => vec![SessionRole::Scoping],
        Phase::Executing => vec![SessionRole::Scoping, SessionRole::Executing],
        Phase::Merging => vec![
            SessionRole::Scoping,
            SessionRole::Executing,
            SessionRole::Merging,
        ],
        Phase::Completed | Phase::Cancelled => {
            let state = load_state(&plan.path(repo_root));
            let mut roles = vec![SessionRole::Scoping];
            if state.session(SessionRole::Executing).is_some() {
                roles.push(SessionRole::Executing);
            }
            if state.session(SessionRole::Merging).is_some() {
                roles.push(SessionRole::Merging);
            }
            roles
        }
    }
}

/// Display title: first markdown heading of `plan.md`, `Untitled` without
/// one. Missing and unreadable files also yield `Untitled`.
pub fn plan_title(repo_root: &Path, plan: &PlanRef) -> String {
    let text = std::fs::read_to_string(plan.plan_md(repo_root)).unwrap_or_default();
    extract_title(&text).unwrap_or_else(|| "Untitled".to_string())
}

/// Every plan directory across all five phases. Missing phase dirs yield
/// no rows; callers run `ensure_structure` first on open.
pub fn scan_plans(repo_root: &Path) -> Vec<PlanRef> {
    let mut plans = Vec::new();
    for phase in [
        Phase::Scoping,
        Phase::Executing,
        Phase::Merging,
        Phase::Completed,
        Phase::Cancelled,
    ] {
        let dir = phase_dir(repo_root, phase);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.filter_map(|entry| entry.ok()) {
            if entry.path().is_dir()
                && let Some(name) = entry.file_name().to_str().map(str::to_string)
            {
                plans.push(PlanRef { name, phase });
            }
        }
    }
    plans
}

/// Arrival fallback for plans without stamped state: `plan.md`
/// modification time, else the directory time. Missing times sort last.
pub fn plan_mtime(repo_root: &Path, plan: &PlanRef) -> Option<std::time::SystemTime> {
    std::fs::metadata(plan.plan_md(repo_root))
        .and_then(|meta| meta.modified())
        .or_else(|_| std::fs::metadata(plan.path(repo_root)).and_then(|meta| meta.modified()))
        .ok()
}

/// Base timestamp name `2026-09-25.10-54-59` in local time.
pub fn timestamp_now() -> String {
    jiff::Zoned::now().strftime("%Y-%m-%d.%H-%M-%S").to_string()
}

/// Collision-proof name: `base`, then `base-1`, `base-2` while taken. Pure.
pub fn unique_name(base: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    let mut counter = 1;
    loop {
        let candidate = format!("{base}-{counter}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        counter += 1;
    }
}

/// Lowercase slug: runs of alphanumerics joined by single hyphens,
/// capped at 60 chars. Pure.
pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut dash = false;
    for ch in title.to_lowercase().chars() {
        if ch.is_alphanumeric() {
            slug.push(ch);
            dash = false;
        } else if !dash && !slug.is_empty() {
            slug.push('-');
            dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    trimmed.chars().take(60).collect::<String>()
}

/// First markdown heading: first line starting with `#` plus a space.
/// Pure.
pub fn extract_title(plan_md: &str) -> Option<String> {
    plan_md.lines().find_map(|line| {
        let trimmed = line.trim();
        let hashes = trimmed.bytes().take_while(|&byte| byte == b'#').count();
        if hashes == 0 || hashes > 6 {
            return None;
        }
        let rest = trimmed.get(hashes..)?.strip_prefix([' ', '\t'])?;
        let title = rest.trim();
        if title.is_empty() {
            return None;
        }
        Some(title.to_string())
    })
}

fn samokod_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".samokod")
}

fn plans_root(repo_root: &Path) -> PathBuf {
    samokod_dir(repo_root).join("plans")
}

fn phase_dir(repo_root: &Path, phase: Phase) -> PathBuf {
    plans_root(repo_root).join(phase.dir_name())
}

fn require_phase(plan: &PlanRef, expected: Phase) -> Result<(), PlanError> {
    if plan.phase != expected {
        return Err(PlanError::WrongPhase {
            expected: expected.dir_name(),
            actual: plan.phase.dir_name(),
        });
    }
    Ok(())
}

fn rename(repo_root: &Path, from: &PlanRef, to: &PlanRef) -> Result<(), PlanError> {
    std::fs::rename(from.path(repo_root), to.path(repo_root)).map_err(|error| {
        PlanError::Io(std::io::Error::new(
            error.kind(),
            format!(
                "cannot move {} to {}: {error}",
                from.path(repo_root).display(),
                to.path(repo_root).display()
            ),
        ))
    })?;
    stamp_entered_at(&to.path(repo_root));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_session(root: &Path, plan: &PlanRef, role: SessionRole, id: &str) {
        let mut state = load_state(&plan.path(root));
        state.set_session(role, id.to_string());
        store_state(&plan.path(root), &state);
    }

    #[test]
    fn phases_have_distinct_dirs() {
        let dirs: HashSet<&str> = [
            Phase::Scoping,
            Phase::Executing,
            Phase::Merging,
            Phase::Completed,
            Phase::Cancelled,
        ]
        .iter()
        .map(|phase| phase.dir_name())
        .collect();
        assert_eq!(dirs.len(), 5);
    }

    #[test]
    fn only_active_phases_run_chats() {
        assert!(Phase::Scoping.is_active());
        assert!(Phase::Executing.is_active());
        assert!(Phase::Merging.is_active());
        assert!(!Phase::Completed.is_active());
        assert!(!Phase::Cancelled.is_active());
    }

    #[test]
    fn unique_name_passes_through_when_free() {
        let taken = HashSet::new();
        assert_eq!(
            unique_name("2026-09-25.10-54-59", &taken),
            "2026-09-25.10-54-59"
        );
    }

    #[test]
    fn unique_name_suffixes_on_collision() {
        let taken = HashSet::from([
            "2026-09-25.10-54-59".to_string(),
            "2026-09-25.10-54-59-1".to_string(),
        ]);
        assert_eq!(
            unique_name("2026-09-25.10-54-59", &taken),
            "2026-09-25.10-54-59-2"
        );
    }

    #[test]
    fn slugify_joins_runs_with_single_hyphens() {
        assert_eq!(
            slugify("Plan: ship the AD3 diagonal-flow app icon!"),
            "plan-ship-the-ad3-diagonal-flow-app-icon"
        );
        assert_eq!(slugify("  spaced   out  "), "spaced-out");
        assert_eq!(slugify("TODO side panel + spend"), "todo-side-panel-spend");
    }

    #[test]
    fn slugify_caps_length() {
        let slug = slugify(&"a".repeat(100));
        assert_eq!(slug.len(), 60);
    }

    #[test]
    fn extract_title_takes_first_heading() {
        let text = "intro\n\n# Real title\n\n## Sub\n";
        assert_eq!(extract_title(text).as_deref(), Some("Real title"));
    }

    #[test]
    fn extract_title_rejects_hash_without_space() {
        assert_eq!(extract_title("#hashtag\n").as_deref(), None);
        assert_eq!(extract_title("no headings\n").as_deref(), None);
    }

    #[test]
    fn title_reads_first_heading() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        std::fs::write(scoping.plan_md(root), "intro\n\n# Real title\n").expect("write");
        assert_eq!(plan_title(root, &scoping), "Real title");
    }

    #[test]
    fn title_falls_back_to_untitled() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let bare = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        assert_eq!(plan_title(root, &bare), "Untitled");
        std::fs::write(bare.plan_md(root), "no heading here\n").expect("write");
        assert_eq!(plan_title(root, &bare), "Untitled");
        let missing = PlanRef {
            name: "gone".to_string(),
            phase: Phase::Scoping,
        };
        assert_eq!(plan_title(root, &missing), "Untitled");
    }

    #[test]
    fn scan_lists_every_phase() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        std::fs::write(scoping.plan_md(root), "# Titled\n").expect("write");
        let executing = execute(root, &scoping).expect("execute");
        let done = complete(root, &executing).expect("complete");
        let cancelled = cancel(
            root,
            &materialize_scoping(root, "2026-09-26.08-41-04").expect("fresh"),
        )
        .expect("cancel");
        let names: HashSet<(String, Phase)> = scan_plans(root)
            .into_iter()
            .map(|plan| (plan.name, plan.phase))
            .collect();
        assert_eq!(
            names,
            HashSet::from([
                (cancelled.name, Phase::Cancelled),
                (done.name, Phase::Completed),
            ])
        );
    }

    #[test]
    fn finished_roles_derive_from_state() {
        let scoping_only = vec![SessionRole::Scoping];
        let both = vec![SessionRole::Scoping, SessionRole::Executing];
        let all = vec![
            SessionRole::Scoping,
            SessionRole::Executing,
            SessionRole::Merging,
        ];
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let plain = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        assert_eq!(roles_for(root, &plain), scoping_only);
        std::fs::write(plain.plan_md(root), "# Shiny\n").expect("write");
        let executing = execute(root, &plain).expect("execute");
        seed_session(root, &executing, SessionRole::Executing, "ses_executing");
        assert_eq!(roles_for(root, &executing), both);
        let done = complete(root, &executing).expect("complete");
        assert_eq!(roles_for(root, &done), both);
        let second = materialize_scoping(root, "2026-09-26.08-41-04").expect("second");
        std::fs::write(second.plan_md(root), "# Second\n").expect("write");
        let running = execute(root, &second).expect("execute");
        seed_session(
            root,
            &running,
            SessionRole::Executing,
            "ses_second_executing",
        );
        let merging = begin_merge(root, &running).expect("begin");
        seed_session(root, &merging, SessionRole::Merging, "ses_second_merging");
        assert_eq!(roles_for(root, &merging), all);
        let rebased = finish_merge(root, &merging).expect("finish");
        assert_eq!(roles_for(root, &rebased), all);
        let third = materialize_scoping(root, "2026-09-26.08-41-05").expect("third");
        std::fs::write(third.plan_md(root), "# Third\n").expect("write");
        let diverged = execute(root, &third).expect("execute");
        seed_session(
            root,
            &diverged,
            SessionRole::Executing,
            "ses_third_executing",
        );
        let dropped = cancel(root, &diverged).expect("cancel");
        assert_eq!(roles_for(root, &dropped), both);
        let fresh = materialize_scoping(root, "2026-09-26.08-41-06").expect("fresh");
        let bare = cancel(root, &fresh).expect("cancel");
        assert_eq!(roles_for(root, &bare), scoping_only);
    }

    #[test]
    fn merge_moves_through_merging_without_renaming() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        std::fs::write(scoping.plan_md(root), "# Shiny\n").expect("write");
        let executing = execute(root, &scoping).expect("execute");
        let merging = begin_merge(root, &executing).expect("begin");
        assert_eq!(merging.phase, Phase::Merging);
        assert_eq!(merging.name, executing.name);
        assert!(!executing.path(root).exists());
        assert!(merging.path(root).is_dir());
        assert!(matches!(
            begin_merge(root, &merging),
            Err(PlanError::WrongPhase { .. })
        ));
        let done = finish_merge(root, &merging).expect("finish");
        assert_eq!(done.phase, Phase::Completed);
        assert_eq!(done.name, merging.name);
        assert!(matches!(
            finish_merge(root, &done),
            Err(PlanError::WrongPhase { .. })
        ));
        assert!(matches!(
            complete(root, &merging),
            Err(PlanError::WrongPhase { .. })
        ));
    }

    #[test]
    fn mtime_prefers_plan_md_then_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let with_md = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        std::fs::write(with_md.plan_md(root), "# T\n").expect("write");
        let bare = materialize_scoping(root, "2026-09-26.08-41-04").expect("create");
        assert!(plan_mtime(root, &with_md).is_some());
        assert!(plan_mtime(root, &bare).is_some());
        let missing = PlanRef {
            name: "gone".to_string(),
            phase: Phase::Scoping,
        };
        assert_eq!(plan_mtime(root, &missing), None);
    }

    #[test]
    fn structure_and_lifecycle_round_trip_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        assert_eq!(
            std::fs::read_to_string(samokod_dir(root).join(".gitignore")).expect("gitignore"),
            "*\n"
        );
        for phase in [
            Phase::Scoping,
            Phase::Executing,
            Phase::Merging,
            Phase::Completed,
            Phase::Cancelled,
        ] {
            assert!(phase_dir(root, phase).is_dir());
        }
        // Idempotent rerun keeps everything.
        ensure_structure(root).expect("re-ensure");

        let scoping = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        assert_eq!(scoping.phase, Phase::Scoping);
        assert!(scoping.path(root).is_dir());
        assert!(!scoping.has_plan_md(root));

        std::fs::write(scoping.plan_md(root), "# Shiny feature\n\nSteps.\n").expect("write plan");
        assert!(scoping.has_plan_md(root));

        let executing = execute(root, &scoping).expect("execute");
        assert_eq!(executing.phase, Phase::Executing);
        assert!(executing.name.ends_with(".shiny-feature"));
        assert!(!scoping.path(root).exists());
        assert!(executing.has_plan_md(root));

        let completed = complete(root, &executing).expect("complete");
        assert_eq!(completed.phase, Phase::Completed);
        assert_eq!(completed.name, executing.name);
        assert!(completed.has_plan_md(root));
    }

    #[test]
    fn complete_then_scoping_keeps_history_and_starts_fresh() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        std::fs::write(scoping.plan_md(root), "# Shiny feature\n\nSteps.\n").expect("write plan");
        let executing = execute(root, &scoping).expect("execute");
        let completed = complete(root, &executing).expect("complete");
        assert!(completed.has_plan_md(root));
        let fresh = materialize_scoping(root, "2026-09-26.08-41-04").expect("fresh scoping");
        assert_eq!(fresh.phase, Phase::Scoping);
        assert!(fresh.path(root).is_dir());
        assert!(!fresh.has_plan_md(root));
        assert!(completed.path(root).is_dir());
    }

    #[test]
    fn execute_fails_loud_without_heading() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        std::fs::write(scoping.plan_md(root), "no heading here\n").expect("write plan");
        assert!(matches!(execute(root, &scoping), Err(PlanError::Untitled)));
    }

    #[test]
    fn execute_rejects_wrong_phase() {
        let plan = PlanRef {
            name: "x".to_string(),
            phase: Phase::Executing,
        };
        assert!(matches!(
            execute(Path::new("/tmp"), &plan),
            Err(PlanError::WrongPhase { .. })
        ));
    }

    #[test]
    fn cancel_names_scoping_plan_when_possible() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        std::fs::write(scoping.plan_md(root), "# Draft idea\n").expect("write plan");
        let cancelled = cancel(root, &scoping).expect("cancel");
        assert_eq!(cancelled.phase, Phase::Cancelled);
        assert!(cancelled.name.ends_with(".draft-idea"));
    }

    #[test]
    fn cancel_keeps_bare_name_without_plan_md() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        let cancelled = cancel(root, &scoping).expect("cancel");
        assert_eq!(cancelled.name, scoping.name);
    }

    #[test]
    fn materialize_creates_missing_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let materialized = materialize_scoping(root, "2026-09-26.08-41-03").expect("materialize");
        assert_eq!(materialized.phase, Phase::Scoping);
        assert!(materialized.path(root).is_dir());
        assert!(!materialized.has_plan_md(root));
    }

    #[test]
    fn materialize_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        materialize_scoping(root, "2026-09-26.08-41-03").expect("first");
        let second = materialize_scoping(root, "2026-09-26.08-41-03").expect("second");
        assert_eq!(second.name, "2026-09-26.08-41-03");
        assert!(second.path(root).is_dir());
    }

    #[test]
    fn transitions_stamp_entered_at_and_keep_ids() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        let first = load_state(&scoping.path(root)).entered_at.expect("stamped");
        let mut seeded = load_state(&scoping.path(root));
        seeded.set_session(SessionRole::Scoping, "ses_scoping".to_string());
        store_state(&scoping.path(root), &seeded);
        std::fs::write(scoping.plan_md(root), "# Shiny\n").expect("write plan");
        let executing = execute(root, &scoping).expect("execute");
        let stamped = load_state(&executing.path(root));
        assert!(stamped.entered_at.unwrap_or(0) >= first);
        assert_eq!(stamped.session(SessionRole::Scoping), Some("ses_scoping"));
        let merging = begin_merge(root, &executing).expect("begin");
        let kept = load_state(&merging.path(root));
        assert!(kept.entered_at.is_some());
        assert_eq!(kept.session(SessionRole::Scoping), Some("ses_scoping"));
        let done = finish_merge(root, &merging).expect("finish");
        assert!(load_state(&done.path(root)).entered_at.is_some());
        assert_eq!(
            load_state(&done.path(root)).session(SessionRole::Scoping),
            Some("ses_scoping")
        );
    }

    #[test]
    fn later_files_inside_plan_do_not_move_entered_at() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = materialize_scoping(root, "2026-09-26.08-41-03").expect("create");
        let before = load_state(&scoping.path(root)).entered_at.expect("stamped");
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(scoping.path(root).join("notes.md"), "# notes\n").expect("notes");
        assert_eq!(load_state(&scoping.path(root)).entered_at, Some(before));
        assert_eq!(arrival_ms(root, &scoping), Some(before));
    }

    #[test]
    fn arrival_falls_back_without_state_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let plan = PlanRef {
            name: "2026-09-26.08-41-03".to_string(),
            phase: Phase::Scoping,
        };
        std::fs::create_dir_all(plan.path(root)).expect("mkdir");
        assert!(arrival_ms(root, &plan).is_some());
        let missing = PlanRef {
            name: "gone".to_string(),
            phase: Phase::Scoping,
        };
        assert_eq!(arrival_ms(root, &missing), None);
    }
}
