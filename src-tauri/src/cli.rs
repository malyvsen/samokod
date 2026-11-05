// CLI path handoff: parse `samokod <path>`, resolve it against the
// invoking cwd, and fail fast before Tauri starts.
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Pending repo roots by window label. `run` inserts the validated CLI root
/// under `main` before the builder starts, so the insert strictly precedes
/// any frontend load and the handoff is race-free.
static PENDING: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();

fn pending_map() -> &'static Mutex<HashMap<String, PathBuf>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Remember a validated root for one window label. Insert-before-build keeps
/// the handoff race-free.
pub fn insert_pending(label: &str, root: PathBuf) {
    let Some(mut pending) = pending_map().lock().ok() else {
        return;
    };
    pending.insert(label.to_string(), root);
}

/// Pop the pending root for the caller's window label, if any.
pub fn take_pending(label: &str) -> Option<PathBuf> {
    let mut pending = pending_map().lock().ok()?;
    pending.remove(label)
}

/// Remember the validated root for the first window. Runs before the Tauri
/// builder starts, so the insert strictly precedes any frontend load.
pub fn remember_initial_root(root: PathBuf) {
    insert_pending("main", root);
}

/// First positional after `argv[0]`, lossy like the single-instance plugin
/// itself. `None` when absent (or empty); everything after it is ignored.
pub fn parse_os_args(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut iter = args.into_iter();
    iter.next()?;
    let raw = iter.next()?;
    let text = raw.to_string_lossy();
    if text.is_empty() {
        return None;
    }
    Some(PathBuf::from(text.into_owned()))
}

/// Same as `parse_os_args` for the already-lossy forwarded args
/// (`Vec<String>`) the single-instance plugin hands to its callback.
/// Used from step 3's second-instance callback.
#[allow(dead_code)]
pub fn parse_forwarded_args(args: &[String]) -> Option<PathBuf> {
    let raw = args.get(1)?;
    if raw.is_empty() {
        return None;
    }
    Some(PathBuf::from(raw))
}

/// Join a relative CLI path onto the invoking cwd; absolute paths pass
/// through untouched.
pub fn resolve_path(raw: &Path, cwd: &Path) -> PathBuf {
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    }
}

/// Resolve a raw CLI path against `cwd`, then validate it as a git repo.
/// Returns the repo root on success.
pub fn resolve_and_validate(raw: &Path, cwd: &Path) -> Result<PathBuf, String> {
    let resolved = resolve_path(raw, cwd);
    let info = crate::repo::validate_repo(&resolved)?;
    Ok(PathBuf::from(info.root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn parses_first_positional_only() {
        assert_eq!(parse_os_args(args_os(&["samokod"])), None);
        assert_eq!(
            parse_os_args(args_os(&["samokod", "/repo"])),
            Some(PathBuf::from("/repo"))
        );
        assert_eq!(
            parse_os_args(args_os(&["samokod", "/a", "/b"])),
            Some(PathBuf::from("/a"))
        );
        assert_eq!(parse_os_args(args_os(&["samokod", ""])), None);
    }

    #[test]
    fn parses_forwarded_args_like_plugin() {
        let args = vec!["samokod".to_string(), "/repo".to_string()];
        assert_eq!(parse_forwarded_args(&args), Some(PathBuf::from("/repo")));
        assert_eq!(parse_forwarded_args(&["samokod".to_string()]), None);
        assert_eq!(
            parse_forwarded_args(&["samokod".to_string(), String::new()]),
            None
        );
    }

    #[test]
    fn resolves_relative_against_cwd() {
        let cwd = Path::new("/work");
        assert_eq!(
            resolve_path(Path::new("repo"), cwd),
            PathBuf::from("/work/repo")
        );
        assert_eq!(resolve_path(Path::new("."), cwd), PathBuf::from("/work/."));
        assert_eq!(
            resolve_path(Path::new("/abs/repo"), cwd),
            PathBuf::from("/abs/repo")
        );
    }

    #[test]
    fn validates_subdirectory_to_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "test"],
            vec!["commit", "--allow-empty", "-m", "init"],
        ] {
            let status = crate::git::command(dir.path())
                .args(&args)
                .output()
                .expect("git");
            assert!(status.status.success(), "{args:?}");
        }
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).expect("mkdir sub");
        let root =
            resolve_and_validate(&sub, Path::new("/")).expect("subdirectory resolves to root");
        assert_eq!(
            crate::repo::canonical_repo_path(&root.to_string_lossy()),
            crate::repo::canonical_repo_path(&dir.path().to_string_lossy())
        );

        let missing = dir.path().join("missing");
        let error =
            resolve_and_validate(&missing, Path::new("/")).expect_err("missing path must fail");
        assert!(!error.is_empty());
    }

    #[test]
    fn rejects_non_repo() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = resolve_and_validate(dir.path(), Path::new("/")).expect_err("must reject");
        assert!(error.contains("not a git repository"));
    }

    #[test]
    fn pending_pops_once_per_window() {
        let suffix = rand_suffix();
        let label = format!("test-main-{suffix}");
        let other = format!("test-other-{suffix}");
        assert_eq!(take_pending(&label), None);
        insert_pending(&label, PathBuf::from("/repo-a"));
        insert_pending(&other, PathBuf::from("/repo-b"));
        assert_eq!(take_pending(&label), Some(PathBuf::from("/repo-a")));
        assert_eq!(take_pending(&label), None);
        assert_eq!(take_pending(&other), Some(PathBuf::from("/repo-b")));
    }

    #[test]
    fn initial_root_targets_main() {
        let label = "main";
        let _ = take_pending(label);
        remember_initial_root(PathBuf::from("/repo-main"));
        assert_eq!(take_pending(label), Some(PathBuf::from("/repo-main")));
        assert_eq!(take_pending(label), None);
    }

    fn rand_suffix() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("{}-{}", std::process::id(), nanos)
    }
}
