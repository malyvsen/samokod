// Repository validation with `git rev-parse`.
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Branch info for a validated repository.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoInfo {
    pub root: String,
    pub branch: String,
}

/// Validate that a path is inside a git work tree and return its root and
/// branch. Pure IO at the edge; no ACP involved.
pub fn validate_repo(path: &Path) -> Result<RepoInfo, String> {
    let root = run_git(path, &["rev-parse", "--show-toplevel"])?;
    let root = root.trim().to_string();
    let branch = run_git(path, &["rev-parse", "--abbrev-ref", "HEAD"])
        .map(|output| output.trim().to_string())
        .unwrap_or_else(|_| "HEAD".to_string());
    Ok(RepoInfo { root, branch })
}

/// Canonical repo identity for the single-owner gate: `canonicalize` when
/// possible so `/tmp` symlinks and trailing slashes do not bypass it,
/// falling back to the git-reported root string.
pub fn canonical_repo_path(path: &str) -> String {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical.to_string_lossy().to_string();
    }
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    trimmed.to_string()
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = crate::git::command(cwd)
        .args(args)
        .output()
        .map_err(|error| format!("git failed: {error}"))?;
    if !output.status.success() {
        return Err(format!("{} - not a git repository", cwd.display()));
    }
    String::from_utf8(output.stdout).map_err(|_| "git output was not utf-8".to_string())
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
            let status = crate::git::command(dir.path())
                .args(&args)
                .output()
                .expect("git");
            assert!(status.status.success(), "{args:?}");
        }
        dir
    }

    #[test]
    fn accepts_git_repo() {
        let dir = git_repo();
        let info = validate_repo(dir.path()).expect("valid repo");
        assert!(!info.root.is_empty());
        assert!(!info.branch.is_empty());
    }

    #[test]
    fn rejects_non_git_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = validate_repo(dir.path()).expect_err("must reject");
        assert!(error.contains("not a git repository"));
    }

    #[test]
    fn canonical_trims_trailing_slashes() {
        assert_eq!(canonical_repo_path("/repo/"), "/repo");
        assert_eq!(canonical_repo_path("/"), "/");
    }

    #[test]
    fn canonical_falls_back_for_missing_paths() {
        let missing = "/definitely/missing/repo/";
        assert_eq!(canonical_repo_path(missing), "/definitely/missing/repo");
    }

    #[test]
    fn canonical_resolves_symlinks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let link_parent = tempfile::tempdir().expect("link parent");
        let link = link_parent.path().join("link");
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path(), &link).expect("symlink");
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(dir.path(), &link).expect("symlink");
        assert_eq!(
            canonical_repo_path(&link.to_string_lossy()),
            canonical_repo_path(&dir.path().to_string_lossy())
        );
    }
}
