// Live branch tracking: resolve `.git/HEAD` (following the `gitdir:`
// pointer for worktrees) and watch it with a debounced `notify` watcher.
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Resolve the HEAD file for a repo root. Follows the `gitdir:` pointer in
/// `.git` for linked worktrees; otherwise `<root>/.git/HEAD`.
pub fn head_path(repo_root: &Path) -> Option<PathBuf> {
    let dot_git = repo_root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git.join("HEAD"));
    }
    if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let pointer = text.strip_prefix("gitdir:")?.trim();
        let gitdir = if Path::new(pointer).is_absolute() {
            PathBuf::from(pointer)
        } else {
            repo_root.join(pointer)
        };
        return Some(gitdir.join("HEAD"));
    }
    None
}

/// Current branch via `git rev-parse`. Detached HEAD keeps `"HEAD"`.
pub fn current_branch(repo_root: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Watch `.git/HEAD` for `repo_root`. Debounced; re-runs `rev-parse` on
/// change and calls `on_branch` with the fresh value. Returns the guard.
/// No timing-sensitive behavior is asserted in tests.
pub fn watch_branch(
    repo_root: PathBuf,
    on_branch: impl Fn(String) + Send + 'static,
) -> Option<notify::RecommendedWatcher> {
    use notify::{RecursiveMode, Watcher};
    let head = head_path(&repo_root)?;
    let watch_root = head.parent()?.to_path_buf();
    let mut watcher =
        notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
            let Ok(event) = event else { return };
            if !event.paths.iter().any(|path| path == &head) {
                return;
            }
            // Debounce: coalesce rapid successive writes.
            std::thread::sleep(Duration::from_millis(100));
            if let Some(branch) = current_branch(&repo_root)
                && !branch.is_empty()
            {
                on_branch(branch);
            }
        })
        .ok()?;
    watcher
        .watch(&watch_root, RecursiveMode::NonRecursive)
        .ok()?;
    Some(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "test"],
            vec!["commit", "--allow-empty", "-m", "init"],
        ] {
            let status = Command::new("git")
                .args(&args)
                .current_dir(dir.path())
                .output()
                .expect("git");
            assert!(status.status.success(), "{args:?}");
        }
        dir
    }

    #[test]
    fn resolves_plain_repo_head() {
        let dir = git_repo();
        assert_eq!(head_path(dir.path()), Some(dir.path().join(".git/HEAD")));
    }

    #[test]
    fn resolves_worktree_pointer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("real-git");
        std::fs::create_dir_all(&target).expect("mkdir");
        std::fs::write(target.join("HEAD"), "ref: refs/heads/main\n").expect("write");
        std::fs::write(dir.path().join(".git"), "gitdir: real-git\n").expect("write");
        assert_eq!(head_path(dir.path()), Some(target.join("HEAD")));
    }

    #[test]
    fn missing_git_yields_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(head_path(dir.path()), None);
    }
}
