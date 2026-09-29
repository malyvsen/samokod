// Persisted OpenCode session IDs: one `session.json` per plan directory
// mapping each role the plan has used to its `ses_..` ID. The file travels
// with the plan directory through renames for free and stays machine-local
// through the existing `.samokod/.gitignore`.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::opencode::SESSION_FILE;
use crate::types::{SessionKey, SessionRole};

/// Read the persisted IDs for one plan directory. Missing files yield an
/// empty map; corrupt files log and yield an empty map so a broken file
/// never blocks a fresh spawn.
pub(crate) fn load(plan_dir: &Path) -> HashMap<SessionRole, String> {
    let path = file(plan_dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return HashMap::new(),
        Err(error) => {
            log::warn!("failed to read {}: {error}", path.display());
            return HashMap::new();
        }
    };
    match serde_json::from_str(&text) {
        Ok(ids) => ids,
        Err(error) => {
            log::warn!("failed to parse {}: {error}", path.display());
            HashMap::new()
        }
    }
}

/// Record one role's session ID inside its plan directory, preserving the
/// other roles. Best-effort like the executed markers: missing directories
/// (a warmed pending session spawns before its directory exists) skip
/// quietly, failures log and the live session continues, and an unchanged
/// ID skips the rewrite. The next prompt heals anything skipped here.
pub(crate) fn record(repo_root: &Path, key: &SessionKey, session_id: &str) {
    let Some(plan_dir) = locate(repo_root, &key.plan) else {
        return;
    };
    let mut ids = load(&plan_dir);
    if ids.get(&key.role).map(String::as_str) == Some(session_id) {
        return;
    }
    ids.insert(key.role, session_id.to_string());
    let text = serde_json::to_string_pretty(&ids).expect("session IDs serialize");
    if let Err(error) = std::fs::write(file(&plan_dir), text) {
        log::warn!("failed to record session {}: {error}", plan_dir.display());
    }
}

/// Locate the on-disk directory for a plan name across all phases. Plan
/// names are unique across phases since transitions rename rather than
/// copy. Pure except the directory probes.
fn locate(repo_root: &Path, plan_name: &str) -> Option<PathBuf> {
    for phase in [
        crate::plans::Phase::Scoping,
        crate::plans::Phase::Executing,
        crate::plans::Phase::Merging,
        crate::plans::Phase::Completed,
        crate::plans::Phase::Cancelled,
    ] {
        let candidate = crate::plans::PlanRef {
            name: plan_name.to_string(),
            phase,
        }
        .path(repo_root);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

/// Path of the mapping file inside one plan directory. Pure.
fn file(plan_dir: &Path) -> PathBuf {
    plan_dir.join(SESSION_FILE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plans::{Phase, PlanRef};

    fn key(plan: &str, role: SessionRole) -> SessionKey {
        SessionKey {
            plan: plan.to_string(),
            role,
        }
    }

    fn plan_dir(root: &Path, phase: Phase, name: &str) -> PathBuf {
        let plan = PlanRef {
            name: name.to_string(),
            phase,
        };
        std::fs::create_dir_all(plan.path(root)).expect("mkdir");
        plan.path(root)
    }

    #[test]
    fn missing_file_yields_empty_map() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = plan_dir(dir.path(), Phase::Scoping, "2026-09-30.10-00-00");
        assert!(load(&plan).is_empty());
    }

    #[test]
    fn round_trip_preserves_all_roles() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        plan_dir(dir.path(), Phase::Scoping, name);
        record(dir.path(), &key(name, SessionRole::Scoping), "ses_scoping");
        record(
            dir.path(),
            &key(name, SessionRole::Executing),
            "ses_executing",
        );
        record(dir.path(), &key(name, SessionRole::Merging), "ses_merging");
        let plan = PlanRef {
            name: name.to_string(),
            phase: Phase::Scoping,
        };
        let ids = load(&plan.path(dir.path()));
        assert_eq!(
            ids.get(&SessionRole::Scoping).map(String::as_str),
            Some("ses_scoping")
        );
        assert_eq!(
            ids.get(&SessionRole::Executing).map(String::as_str),
            Some("ses_executing")
        );
        assert_eq!(
            ids.get(&SessionRole::Merging).map(String::as_str),
            Some("ses_merging")
        );
    }

    #[test]
    fn overwrite_keeps_other_roles() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        plan_dir(dir.path(), Phase::Scoping, name);
        record(dir.path(), &key(name, SessionRole::Scoping), "ses_old");
        record(dir.path(), &key(name, SessionRole::Executing), "ses_exec");
        record(dir.path(), &key(name, SessionRole::Scoping), "ses_new");
        let plan = PlanRef {
            name: name.to_string(),
            phase: Phase::Scoping,
        };
        let ids = load(&plan.path(dir.path()));
        assert_eq!(
            ids.get(&SessionRole::Scoping).map(String::as_str),
            Some("ses_new")
        );
        assert_eq!(
            ids.get(&SessionRole::Executing).map(String::as_str),
            Some("ses_exec")
        );
    }

    #[test]
    fn corrupt_file_yields_empty_map() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = plan_dir(dir.path(), Phase::Scoping, "2026-09-30.10-00-00");
        std::fs::write(file(&plan), "{ not json").expect("write corrupt");
        assert!(load(&plan).is_empty());
    }

    #[test]
    fn locate_searches_every_phase() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        for phase in [Phase::Executing, Phase::Completed, Phase::Cancelled] {
            let name = format!("plan-{}", phase.dir_name());
            plan_dir(dir.path(), phase, &name);
            let found = locate(dir.path(), &name).expect("found");
            assert!(found.ends_with(&name));
        }
        assert_eq!(locate(dir.path(), "missing"), None);
    }

    #[test]
    fn missing_dir_records_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        record(
            dir.path(),
            &key("2026-09-30.10-00-00", SessionRole::Scoping),
            "ses_pending",
        );
        let plan = PlanRef {
            name: "2026-09-30.10-00-00".to_string(),
            phase: Phase::Scoping,
        };
        assert!(!file(&plan.path(dir.path())).is_file());
    }

    #[test]
    fn same_id_skips_rewrite() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        plan_dir(dir.path(), Phase::Scoping, name);
        record(dir.path(), &key(name, SessionRole::Scoping), "ses_same");
        let plan = PlanRef {
            name: name.to_string(),
            phase: Phase::Scoping,
        };
        let path = file(&plan.path(dir.path()));
        let before = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        std::thread::sleep(std::time::Duration::from_millis(10));
        record(dir.path(), &key(name, SessionRole::Scoping), "ses_same");
        let after = std::fs::metadata(&path)
            .expect("metadata")
            .modified()
            .expect("mtime");
        assert_eq!(before, after);
    }
}
