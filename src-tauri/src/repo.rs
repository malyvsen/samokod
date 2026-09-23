// Repository validation with `git rev-parse`.
use std::path::Path;
use std::process::Command;

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

fn run_git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
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
}
