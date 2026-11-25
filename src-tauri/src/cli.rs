// CLI launch path: `samokod <path>` opens the repo directly in a window.
// The validated root is staged before the window is built, so the new
// window's mount claim cannot race the frontend load.
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// First-instance terminal launch: validate the path synchronously so errors
/// stay visible, then respawn detached with stdio nulled and exit the parent
/// so the terminal returns immediately. Child and non-tty launches fall
/// through to the normal startup path.
pub fn maybe_detach() {
    if !should_detach(
        is_detached(),
        log_opt_in(std::env::var("SAMOKOD_LOG").ok().as_deref()),
        std::io::stdout().is_terminal(),
        cfg!(debug_assertions),
    ) {
        return;
    }
    if let Some(raw) = first_positional(std::env::args_os()) {
        validated_root(&raw, &current_dir_or_fail());
    }
    let exe =
        std::env::current_exe().unwrap_or_else(|error| fail(&format!("cannot relaunch: {error}")));
    match detach_command(exe, std::env::args_os().skip(1))
        .env(DETACHED_ENV, "1")
        .spawn()
    {
        Ok(_) => std::process::exit(0),
        Err(error) => fail(&format!("cannot relaunch: {error}")),
    }
}

/// Child respawn that cannot break the launching terminal: stdio is nulled
/// and, on Unix, the child joins a new session so it shares neither the
/// session nor the controlling TTY with the shell that ran `samokod <path>`.
fn detach_command(exe: PathBuf, args: impl IntoIterator<Item = OsString>) -> std::process::Command {
    let mut command = std::process::Command::new(exe);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: `setsid` is async-signal-safe; the closure runs between
        // `fork` and `exec` where only that call and returning apply.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    command
}

/// Validated repo root for `samokod <path>`, if a path was given. Exits 1
/// with a stderr diagnostic on failure before any window exists: the
/// single-instance plugin would otherwise swallow the failure in the
/// already-running process.
pub fn launch_root() -> Option<PathBuf> {
    let raw = first_positional(std::env::args_os())?;
    Some(validated_root(&raw, &current_dir_or_fail()))
}

/// Detach only for a foreground terminal launch without the `SAMOKOD_LOG`
/// opt-in. Debug builds never detach so dev and tests stay in the foreground.
fn should_detach(
    already_detached: bool,
    log_opted_in: bool,
    stdout_is_tty: bool,
    is_debug: bool,
) -> bool {
    !already_detached && !log_opted_in && stdout_is_tty && !is_debug
}

/// Non-empty `SAMOKOD_LOG` keeps the launch in the foreground and enables
/// stdout logs; unset or empty stays file-only in release.
pub fn log_opt_in(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.is_empty())
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

const DETACHED_ENV: &str = "SAMOKOD_DETACHED";

/// True in the detached child spawned by [`maybe_detach`].
fn is_detached() -> bool {
    std::env::var(DETACHED_ENV).is_ok()
}

fn current_dir_or_fail() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|error| fail(&format!("cannot resolve path: {error}")))
}

fn validated_root(raw: &Path, cwd: &Path) -> PathBuf {
    match resolve_and_validate(raw, cwd) {
        Ok(root) => root,
        Err(error) => fail(&error),
    }
}

fn fail(message: &str) -> ! {
    eprintln!("samokod: {message}");
    std::process::exit(1);
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

    #[test]
    fn detach_only_foreground_terminal_release_without_log_opt_in() {
        assert!(should_detach(false, false, true, false));
        assert!(!should_detach(true, false, true, false));
        assert!(!should_detach(false, true, true, false));
        assert!(!should_detach(false, false, false, false));
        assert!(!should_detach(false, false, true, true));
        assert!(!should_detach(true, true, false, true));
    }

    #[cfg(unix)]
    #[test]
    fn detached_child_lands_in_new_session() {
        let mut child = detach_command(PathBuf::from("sleep"), vec![OsString::from("10")])
            .spawn()
            .expect("spawn probe");
        // SAFETY: `getsid` with a live pid has no side effects.
        let parent_sid = unsafe { libc::getsid(0) };
        let child_sid = unsafe { libc::getsid(child.id() as libc::pid_t) };
        let _ = child.kill();
        let _ = child.wait();
        assert_ne!(parent_sid, -1, "getsid(parent) must succeed");
        assert_ne!(child_sid, -1, "getsid(child) must succeed");
        assert_ne!(
            parent_sid, child_sid,
            "detached child must leave the terminal session"
        );
    }

    #[test]
    fn log_opt_in_requires_non_empty_value() {
        assert!(!log_opt_in(None));
        assert!(!log_opt_in(Some("")));
        assert!(log_opt_in(Some("info")));
        assert!(log_opt_in(Some("1")));
    }
}
