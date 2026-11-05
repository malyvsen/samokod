// CLI launch path: `samokod <path>` opens the repo directly in a window.
// The validated root is staged before the window is built, so the new
// window's mount claim cannot race the frontend load.
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Repo roots staged for window labels, each claimed once on mount.
#[derive(Debug, Default)]
pub struct PendingRepos {
    inner: Mutex<HashMap<String, PathBuf>>,
}

impl PendingRepos {
    /// Stage a validated root for a window label.
    pub fn insert(&self, label: &str, root: PathBuf) {
        if let Ok(mut pending) = self.inner.lock() {
            pending.insert(label.to_string(), root);
        }
    }

    /// Claim the staged root for a window label, if any.
    pub fn take(&self, label: &str) -> Option<PathBuf> {
        self.inner.lock().ok()?.remove(label)
    }
}

static PENDING: OnceLock<PendingRepos> = OnceLock::new();

fn pending() -> &'static PendingRepos {
    PENDING.get_or_init(PendingRepos::default)
}

/// Stage a validated root for a window label.
pub fn insert_pending(label: &str, root: PathBuf) {
    pending().insert(label, root);
}

/// Claim the staged root for the caller's window label, if any.
pub fn take_pending(label: &str) -> Option<PathBuf> {
    pending().take(label)
}

/// First positional after `argv[0]`. `None` when absent or empty; everything
/// after it is ignored. Generic over `OsString` (local process) and `String`
/// (already-lossy args forwarded by the single-instance plugin).
pub fn first_positional<I, S>(args: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut iter = args.into_iter();
    iter.next()?;
    let raw = iter.next()?;
    let text = raw.as_ref().to_string_lossy();
    if text.is_empty() {
        return None;
    }
    Some(PathBuf::from(text.into_owned()))
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
    use std::ffi::OsString;

    #[test]
    fn parses_first_positional_only() {
        let args: Vec<OsString> = vec![];
        assert_eq!(first_positional(args), None);
        assert_eq!(first_positional(vec![OsString::from("samokod")]), None);
        assert_eq!(
            first_positional(vec![OsString::from("samokod"), OsString::from("/repo")]),
            Some(PathBuf::from("/repo"))
        );
        assert_eq!(
            first_positional(vec![
                OsString::from("samokod"),
                OsString::from("/a"),
                OsString::from("/b"),
            ]),
            Some(PathBuf::from("/a"))
        );
        assert_eq!(
            first_positional(vec![OsString::from("samokod"), OsString::from("")]),
            None
        );
    }

    #[test]
    fn parses_forwarded_string_args() {
        let args = ["samokod".to_string(), "/repo".to_string()];
        assert_eq!(first_positional(args.iter()), Some(PathBuf::from("/repo")));
        assert_eq!(first_positional(["samokod".to_string()].iter()), None);
        assert_eq!(
            first_positional(["samokod".to_string(), String::new()].iter()),
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

    fn init_repo(path: &Path) {
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "test"],
            vec!["commit", "--allow-empty", "-m", "init"],
        ] {
            let status = crate::git::command(path).args(&args).output().expect("git");
            assert!(status.status.success(), "{args:?}");
        }
    }

    #[test]
    fn validates_subdirectory_to_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        init_repo(dir.path());
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
    fn pending_claims_once_per_window() {
        let pending = PendingRepos::default();
        assert_eq!(pending.take("a"), None);
        pending.insert("a", PathBuf::from("/repo-a"));
        pending.insert("b", PathBuf::from("/repo-b"));
        assert_eq!(pending.take("a"), Some(PathBuf::from("/repo-a")));
        assert_eq!(pending.take("a"), None);
        assert_eq!(pending.take("b"), Some(PathBuf::from("/repo-b")));
    }
}
