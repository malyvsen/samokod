// Plan directory lifecycle under `.samokod/plans/`.
// Lifecycle transitions first, naming and filesystem helpers below.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Lifecycle phase. One directory per phase under `.samokod/plans/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Scoping,
    Executing,
    Completed,
    Cancelled,
}

impl Phase {
    pub fn dir_name(self) -> &'static str {
        match self {
            Phase::Scoping => "scoping",
            Phase::Executing => "executing",
            Phase::Completed => "completed",
            Phase::Cancelled => "cancelled",
        }
    }

    /// Phases with a live chat. Only these auto-abandon on chat switch.
    pub fn is_active(self) -> bool {
        matches!(self, Phase::Scoping | Phase::Executing)
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
        Phase::Completed,
        Phase::Cancelled,
    ] {
        std::fs::create_dir_all(phase_dir(repo_root, phase))?;
    }
    std::fs::write(samokod_dir(repo_root).join(".gitignore"), "*\n")?;
    Ok(())
}

/// Create a fresh timestamped scoping plan. Fails fast on io errors.
pub fn create_scoping(repo_root: &Path) -> Result<PlanRef, PlanError> {
    let dir = phase_dir(repo_root, Phase::Scoping);
    let taken: HashSet<String> = std::fs::read_dir(&dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect();
    let name = unique_name(&timestamp_now(), &taken);
    std::fs::create_dir(dir.join(&name))?;
    Ok(PlanRef {
        name,
        phase: Phase::Scoping,
    })
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

/// Drop an active plan. Scoping plans gain a slug when they have a heading,
/// everything else keeps its name.
pub fn abandon(repo_root: &Path, plan: &PlanRef) -> Result<PlanRef, PlanError> {
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

/// Slugged abandon name for a scoping plan with a titled `plan.md`.
/// Missing files and untitled plans yield no slug; the caller keeps the
/// bare name. Pure except the read.
fn slugged_name(repo_root: &Path, plan: &PlanRef) -> Option<String> {
    let text = std::fs::read_to_string(plan.plan_md(repo_root)).ok()?;
    let title = extract_title(&text)?;
    Some(format!("{}.{}", plan.name, slugify(&title)))
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phases_have_distinct_dirs() {
        let dirs: HashSet<&str> = [
            Phase::Scoping,
            Phase::Executing,
            Phase::Completed,
            Phase::Cancelled,
        ]
        .iter()
        .map(|phase| phase.dir_name())
        .collect();
        assert_eq!(dirs.len(), 4);
    }

    #[test]
    fn only_scoping_and_executing_are_active() {
        assert!(Phase::Scoping.is_active());
        assert!(Phase::Executing.is_active());
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
            Phase::Completed,
            Phase::Cancelled,
        ] {
            assert!(phase_dir(root, phase).is_dir());
        }
        // Idempotent rerun keeps everything.
        ensure_structure(root).expect("re-ensure");

        let scoping = create_scoping(root).expect("create");
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
    fn execute_fails_loud_without_heading() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = create_scoping(root).expect("create");
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
    fn abandon_names_scoping_plan_when_possible() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = create_scoping(root).expect("create");
        std::fs::write(scoping.plan_md(root), "# Draft idea\n").expect("write plan");
        let cancelled = abandon(root, &scoping).expect("abandon");
        assert_eq!(cancelled.phase, Phase::Cancelled);
        assert!(cancelled.name.ends_with(".draft-idea"));
    }

    #[test]
    fn abandon_keeps_bare_name_without_plan_md() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        ensure_structure(root).expect("ensure");
        let scoping = create_scoping(root).expect("create");
        let cancelled = abandon(root, &scoping).expect("abandon");
        assert_eq!(cancelled.name, scoping.name);
    }
}
