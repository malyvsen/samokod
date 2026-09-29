// Git subprocess edge: every `git` invocation goes through here so
// hook-inherited location vars never redirect it.
use std::path::Path;
use std::process::Command;

/// Git directed by `cwd` alone. Ambient location vars (inherited from a
/// git hook's environment, e.g. a relative `GIT_INDEX_FILE`) never
/// override it: inside a fresh worktree `.git` is a file, so a relative
/// index path fails with `Not a directory`.
pub fn command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(cwd);
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_PREFIX",
    ] {
        command.env_remove(var);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_repo(path: &Path) {
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "test"],
            vec!["commit", "--allow-empty", "-m", "init"],
        ] {
            let output = command(path).args(&args).output().expect("git");
            assert!(output.status.success(), "{args:?}");
        }
    }

    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn hostile() -> Self {
            // Hook env captured in a worktree: absolute GIT_DIR and
            // GIT_INDEX_FILE pointing elsewhere, empty GIT_PREFIX.
            let vars = [
                "GIT_DIR",
                "GIT_WORK_TREE",
                "GIT_INDEX_FILE",
                "GIT_COMMON_DIR",
                "GIT_PREFIX",
            ];
            let saved = vars
                .iter()
                .map(|key| (*key, std::env::var(key).ok()))
                .collect();
            // SAFETY: tests run with all git call sites isolated, so a
            // hostile hook env cannot redirect parallel tests.
            unsafe {
                std::env::set_var("GIT_DIR", "/tmp/samokod-hook-repro.git");
                std::env::set_var("GIT_WORK_TREE", "/tmp/samokod-hook-repro-worktree");
                std::env::set_var("GIT_INDEX_FILE", "/tmp/samokod-hook-repro.git/index");
                std::env::set_var("GIT_COMMON_DIR", "/tmp/samokod-hook-repro.git");
                std::env::set_var("GIT_PREFIX", "");
            }
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: restores the pre-test environment.
            unsafe {
                for (key, value) in std::mem::take(&mut self.saved) {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

    #[test]
    fn hook_location_env_does_not_redirect_git() {
        let _guard = EnvGuard::hostile();
        let dir = tempfile::tempdir().expect("tempdir");
        init_repo(dir.path());
        let info = crate::repo::validate_repo(dir.path()).expect("valid repo");
        assert!(!info.root.is_empty());
        assert!(!info.branch.is_empty());

        let empty = tempfile::tempdir().expect("tempdir");
        let error = crate::repo::validate_repo(empty.path()).expect_err("must reject");
        assert!(error.contains("not a git repository"));

        assert_eq!(
            crate::branch::head_path(dir.path()),
            Some(dir.path().join(".git/HEAD"))
        );
    }
}
