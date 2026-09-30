// OpenCode session IDs keyed by plan plus role. Persisted IDs win; missing
// ones recover via the CLI on demand below and persist for next time.
// Storage lives in `plans::PlanState`, so ID writes preserve the
// phase-entry clock.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;

use crate::opencode;
use crate::types::{SessionKey, SessionRole};

use super::session::ActivePlan;

/// Resolved ACP session ID for one session key, across all phases.
/// Persisted IDs win; missing ones recover via the CLI and persist for
/// next time. `None` means a brand-new plan with no past to replay.
pub(crate) fn resolve(repo_root: &Path, session: &SessionKey) -> Option<String> {
    let plan_dir = locate(repo_root, &session.plan)?;
    let mut state = crate::plans::load_state(&plan_dir);
    if let Some(id) = state.session(session.role) {
        return Some(id.to_string());
    }
    let recovered = recover(repo_root, session, &plan_dir, &state)?;
    state.set_session(session.role, recovered.clone());
    crate::plans::store_state(&plan_dir, &state);
    Some(recovered)
}

/// Record one role's session ID inside its plan directory, preserving the
/// other roles and the phase-entry clock. Best-effort like the executed
/// markers: missing directories (a warmed pending session spawns before
/// its directory exists) skip quietly, failures log and the live session
/// continues, and an unchanged ID skips the rewrite. The next prompt heals
/// anything skipped here.
pub(crate) fn record(repo_root: &Path, key: &SessionKey, session_id: &str) {
    let Some(plan_dir) = locate(repo_root, &key.plan) else {
        return;
    };
    let mut state = crate::plans::load_state(&plan_dir);
    if state.session(key.role) == Some(session_id) {
        return;
    }
    state.set_session(key.role, session_id.to_string());
    crate::plans::store_state(&plan_dir, &state);
}

/// True when a plan name exists in any phase. Name-based: transitions
/// rename rather than copy, so names stay unique across phases. Pure
/// except the directory probes.
pub(crate) fn plan_exists(repo_root: &Path, plan_name: &str) -> bool {
    locate(repo_root, plan_name).is_some()
}

/// True when a plan name lives in a finished phase. Finished rows stay
/// read-only and never warm or spawn. Pure except the directory probes.
pub(crate) fn is_finished(repo_root: &Path, plan_name: &str) -> bool {
    for phase in [
        crate::plans::Phase::Completed,
        crate::plans::Phase::Cancelled,
    ] {
        let candidate = crate::plans::PlanRef {
            name: plan_name.to_string(),
            phase,
        }
        .path(repo_root);
        if candidate.is_dir() {
            return true;
        }
    }
    false
}

/// Locate the on-disk directory for a plan name across all phases. Plan
/// names are unique across phases since transitions rename rather than
/// copy. Pure except the directory probes.
pub(crate) fn locate(repo_root: &Path, plan_name: &str) -> Option<PathBuf> {
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

/// One listed OpenCode session. Only the matching fields parse; the list
/// output carries no owning agent, so recovery verifies it via export.
#[derive(Debug, Clone, Deserialize)]
struct ListedSession {
    id: String,
    #[serde(default)]
    directory: String,
    #[serde(default)]
    created: i64,
}

/// Minimal export shape: only the owning agent reads out. Pure data.
#[derive(Debug, Deserialize)]
struct SessionExport {
    info: SessionInfo,
}

#[derive(Debug, Deserialize)]
struct SessionInfo {
    #[serde(default)]
    agent: String,
}

/// One listed session with its exported agent. Pure match input.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoveredSession {
    id: String,
    agent: String,
    directory: String,
    created: i64,
}

/// Recover one role's session ID via the CLI: list sessions in the plan's
/// working directory, verify each candidate's owning agent via export,
/// select by recency, and let the caller persist. Failures yield `None`
/// so the caller falls through to the empty-history path.
fn recover(
    repo_root: &Path,
    session: &SessionKey,
    plan_dir: &Path,
    state: &crate::plans::PlanState,
) -> Option<String> {
    let plan = ActivePlan::for_session(session);
    let directory = plan.cwd(repo_root).to_string_lossy().to_string();
    let agent = opencode::agent_for(plan.phase);
    let claimed: HashSet<String> = [
        SessionRole::Scoping,
        SessionRole::Executing,
        SessionRole::Merging,
    ]
    .into_iter()
    .filter_map(|role| state.session(role))
    .map(str::to_string)
    .collect();
    let mtime = plan_mtime_ms(plan_dir);
    let binary = crate::acp::resolve_opencode_binary().ok()?;
    let listed = list_sessions(&binary)?;
    // Filter by directory before exporting: each export spawns the CLI,
    // so only sessions in this plan's cwd pay that probe.
    let mut verified = Vec::new();
    for entry in listed.iter().filter(|entry| entry.directory == directory) {
        if let Some(agent) = export_agent(&binary, &entry.id) {
            verified.push(DiscoveredSession {
                id: entry.id.clone(),
                agent,
                directory: entry.directory.clone(),
                created: entry.created,
            });
        }
    }
    best_match(&verified, &directory, agent, mtime, &claimed)
}

/// Select the session for one plan role: the directory and agent must
/// match, and a session already claimed by another role never matches.
/// Newest `created` at or before the plan mtime wins, else newest overall.
/// Pure.
fn best_match(
    candidates: &[DiscoveredSession],
    directory: &str,
    agent: &str,
    plan_mtime_ms: Option<i64>,
    claimed: &HashSet<String>,
) -> Option<String> {
    let mut eligible: Vec<&DiscoveredSession> = candidates
        .iter()
        .filter(|candidate| {
            candidate.directory == directory
                && candidate.agent == agent
                && !claimed.contains(&candidate.id)
        })
        .collect();
    eligible.sort_by_key(|candidate| candidate.created);
    if let Some(mtime) = plan_mtime_ms
        && let Some(picked) = eligible
            .iter()
            .rev()
            .find(|candidate| candidate.created <= mtime)
    {
        return Some(picked.id.clone());
    }
    eligible
        .iter()
        .next_back()
        .map(|candidate| candidate.id.clone())
}

/// List OpenCode sessions via the CLI. The CLI is the public contract, so
/// no schema drift versus reading the database directly. Failures yield
/// `None`; the caller falls through.
fn list_sessions(binary: &Path) -> Option<Vec<ListedSession>> {
    let output = Command::new(binary)
        .args(["session", "list", "--format", "json"])
        .output()
        .ok()?;
    if !output.status.success() {
        log::warn!("opencode session list failed: {}", output.status);
        return None;
    }
    match serde_json::from_slice(&output.stdout) {
        Ok(listed) => Some(listed),
        Err(error) => {
            log::warn!("failed to parse opencode session list: {error}");
            None
        }
    }
}

/// Owning agent for one session via `opencode export`. Failures log and
/// yield `None` so the caller skips the session. Stdout redirects to a
/// temp file instead of a pipe: `opencode export` truncates large session
/// JSON written to a pipe, while files land whole.
fn export_agent(binary: &Path, session_id: &str) -> Option<String> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let out_path = std::env::temp_dir().join(format!(
        "samokod-export-{}-{}.json",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let out_file = match std::fs::File::create(&out_path) {
        Ok(file) => file,
        Err(error) => {
            log::warn!("failed to stage export {session_id}: {error}");
            return None;
        }
    };
    let status = Command::new(binary)
        .args(["export", session_id])
        .stdout(out_file)
        .stderr(std::process::Stdio::null())
        .status();
    let agent = match status {
        Ok(status) if status.success() => std::fs::read(&out_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<SessionExport>(&bytes).ok())
            .map(|export| export.info.agent),
        Ok(status) => {
            log::warn!("failed to export {session_id}: exit {status}");
            None
        }
        Err(error) => {
            log::warn!("failed to export {session_id}: {error}");
            None
        }
    };
    if let Err(error) = std::fs::remove_file(&out_path) {
        log::warn!("failed to clean export {session_id}: {error}");
    }
    agent
}

/// Plan modification time in epoch millis, preferring `plan.md` like
/// `plans::plan_mtime` but from the located directory without its phase.
/// Pure except the metadata probes.
fn plan_mtime_ms(plan_dir: &Path) -> Option<i64> {
    let modified = std::fs::metadata(plan_dir.join("plan.md"))
        .or_else(|_| std::fs::metadata(plan_dir))
        .ok()?
        .modified()
        .ok()?;
    crate::plans::unix_ms(modified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plans::{self, Phase, PlanRef};

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
    fn missing_file_yields_no_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = plan_dir(dir.path(), Phase::Scoping, "2026-09-30.10-00-00");
        let state = plans::load_state(&plan);
        assert_eq!(state.session(SessionRole::Scoping), None);
        assert_eq!(state.session(SessionRole::Executing), None);
        assert_eq!(state.session(SessionRole::Merging), None);
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
        let state = plans::load_state(&plan.path(dir.path()));
        assert_eq!(state.session(SessionRole::Scoping), Some("ses_scoping"));
        assert_eq!(state.session(SessionRole::Executing), Some("ses_executing"));
        assert_eq!(state.session(SessionRole::Merging), Some("ses_merging"));
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
        let state = plans::load_state(&plan.path(dir.path()));
        assert_eq!(state.session(SessionRole::Scoping), Some("ses_new"));
        assert_eq!(state.session(SessionRole::Executing), Some("ses_exec"));
    }

    #[test]
    fn corrupt_file_yields_no_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let plan = plan_dir(dir.path(), Phase::Scoping, "2026-09-30.10-00-00");
        std::fs::write(plan.join(plans::STATE_FILE), "{ not json").expect("write corrupt");
        let state = plans::load_state(&plan);
        assert_eq!(state.session(SessionRole::Scoping), None);
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
    fn plan_exists_covers_all_phases() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        for phase in [
            Phase::Scoping,
            Phase::Executing,
            Phase::Merging,
            Phase::Completed,
            Phase::Cancelled,
        ] {
            let name = format!("plan-{}", phase.dir_name());
            plan_dir(dir.path(), phase, &name);
            assert!(plan_exists(dir.path(), &name), "{phase:?} exists");
        }
        assert!(!plan_exists(dir.path(), "missing"));
    }

    #[test]
    fn is_finished_only_for_completed_and_cancelled() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        for phase in [Phase::Scoping, Phase::Executing, Phase::Merging] {
            let name = format!("active-{}", phase.dir_name());
            plan_dir(dir.path(), phase, &name);
            assert!(!is_finished(dir.path(), &name), "{phase:?} active");
        }
        for phase in [Phase::Completed, Phase::Cancelled] {
            let name = format!("done-{}", phase.dir_name());
            plan_dir(dir.path(), phase, &name);
            assert!(is_finished(dir.path(), &name), "{phase:?} finished");
        }
        assert!(!is_finished(dir.path(), "missing"));
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
        assert!(!plan.path(dir.path()).join(plans::STATE_FILE).is_file());
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
        let path = plan.path(dir.path()).join(plans::STATE_FILE);
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

    #[test]
    fn record_preserves_entered_at() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        let scoping = crate::plans::materialize_scoping(dir.path(), name).expect("create");
        let stamped = crate::plans::load_state(&scoping.path(dir.path()))
            .entered_at
            .expect("stamped");
        record(dir.path(), &key(name, SessionRole::Scoping), "ses_scoping");
        record(dir.path(), &key(name, SessionRole::Executing), "ses_exec");
        let kept = crate::plans::load_state(&scoping.path(dir.path()));
        assert_eq!(kept.entered_at, Some(stamped));
        assert_eq!(kept.session(SessionRole::Scoping), Some("ses_scoping"));
        assert_eq!(kept.session(SessionRole::Executing), Some("ses_exec"));
    }

    #[test]
    fn legacy_session_file_migrates_to_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        crate::plans::ensure_structure(dir.path()).expect("ensure");
        let name = "2026-09-30.10-00-00";
        let plan = plan_dir(dir.path(), Phase::Scoping, name);
        std::fs::write(
            plan.join(plans::LEGACY_SESSION_FILE),
            r#"{"scoping":"ses_old"}"#,
        )
        .expect("legacy");
        record(dir.path(), &key(name, SessionRole::Executing), "ses_exec");
        let kept = crate::plans::load_state(&plan);
        assert_eq!(kept.session(SessionRole::Scoping), Some("ses_old"));
        assert_eq!(kept.session(SessionRole::Executing), Some("ses_exec"));
        assert!(plan.join(plans::STATE_FILE).is_file());
        assert!(!plan.join(plans::LEGACY_SESSION_FILE).is_file());
    }

    fn discovered(id: &str, agent: &str, directory: &str, created: i64) -> DiscoveredSession {
        DiscoveredSession {
            id: id.to_string(),
            agent: agent.to_string(),
            directory: directory.to_string(),
            created,
        }
    }

    fn claimed_set(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn best_match_requires_directory_and_agent() {
        let candidates = vec![
            discovered("ses_other_dir", "samokod-scoping", "/elsewhere", 300),
            discovered("ses_other_agent", "samokod-executing", "/repo", 300),
            discovered("ses_match", "samokod-scoping", "/repo", 200),
        ];
        assert_eq!(
            best_match(
                &candidates,
                "/repo",
                "samokod-scoping",
                Some(250),
                &claimed_set(&[])
            ),
            Some("ses_match".to_string())
        );
    }

    #[test]
    fn best_match_prefers_newest_at_or_before_plan_mtime() {
        let candidates = vec![
            discovered("ses_old", "samokod-scoping", "/repo", 100),
            discovered("ses_mid", "samokod-scoping", "/repo", 200),
            discovered("ses_new", "samokod-scoping", "/repo", 300),
        ];
        assert_eq!(
            best_match(
                &candidates,
                "/repo",
                "samokod-scoping",
                Some(250),
                &claimed_set(&[])
            ),
            Some("ses_mid".to_string())
        );
    }

    #[test]
    fn best_match_falls_back_to_newest_overall() {
        let candidates = vec![
            discovered("ses_old", "samokod-scoping", "/repo", 100),
            discovered("ses_new", "samokod-scoping", "/repo", 200),
        ];
        assert_eq!(
            best_match(
                &candidates,
                "/repo",
                "samokod-scoping",
                Some(50),
                &claimed_set(&[])
            ),
            Some("ses_new".to_string())
        );
        assert_eq!(
            best_match(
                &candidates,
                "/repo",
                "samokod-scoping",
                None,
                &claimed_set(&[])
            ),
            Some("ses_new".to_string())
        );
    }

    #[test]
    fn best_match_never_reuses_claimed_sessions() {
        let candidates = vec![
            discovered("ses_taken", "samokod-scoping", "/repo", 300),
            discovered("ses_free", "samokod-scoping", "/repo", 200),
        ];
        assert_eq!(
            best_match(
                &candidates,
                "/repo",
                "samokod-scoping",
                Some(400),
                &claimed_set(&["ses_taken"])
            ),
            Some("ses_free".to_string())
        );
        assert_eq!(
            best_match(
                &candidates,
                "/repo",
                "samokod-scoping",
                Some(400),
                &claimed_set(&["ses_taken", "ses_free"])
            ),
            None
        );
        assert_eq!(
            best_match(
                &[],
                "/repo",
                "samokod-scoping",
                Some(400),
                &claimed_set(&[])
            ),
            None
        );
    }

    #[test]
    fn list_shape_parses_cli_output() {
        let listed: Vec<ListedSession> = serde_json::from_value(serde_json::json!([
            {
                "id": "ses_abc",
                "title": "Shiny",
                "updated": 1790724727740_i64,
                "created": 1790721159907_i64,
                "projectId": "p1",
                "directory": "/repo"
            }
        ]))
        .expect("list parses");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "ses_abc");
        assert_eq!(listed[0].directory, "/repo");
        assert_eq!(listed[0].created, 1790721159907);
    }

    #[test]
    fn export_shape_reads_agent_without_messages() {
        let export: SessionExport = serde_json::from_value(serde_json::json!({
            "info": {
                "id": "ses_abc",
                "directory": "/repo",
                "agent": "samokod-executing",
                "time": { "created": 1, "updated": 2 }
            },
            "messages": [{ "info": { "role": "user" } }]
        }))
        .expect("export parses");
        assert_eq!(export.info.agent, "samokod-executing");
    }
}
