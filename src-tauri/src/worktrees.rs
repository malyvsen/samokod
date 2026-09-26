// Worktree operations: one isolated checkout per executing plan.
// Lifecycle first, pure naming helpers below, git edge at the bottom.
use std::path::{Path, PathBuf};
use std::process::Command;

/// One plan checkout: where it lives and which branches it spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRecord {
    pub path: PathBuf,
    pub branch: String,
    pub base: String,
    pub main_branch: String,
}

/// Create a worktree for one plan on a fresh branch at `base`.
/// Fails loud with git stderr preserved.
pub fn create(
    repo_root: &Path,
    plan_name: &str,
    base: &str,
    main_branch: &str,
) -> Result<WorktreeRecord, WorktreeError> {
    let branch = branch_name(plan_name);
    let path = worktree_path(repo_root, plan_name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    run_git(
        repo_root,
        &[
            "worktree",
            "add",
            "-b",
            branch.as_str(),
            path.to_string_lossy().as_ref(),
            base,
        ],
    )?;
    // Worktree checkouts hold their own untracked empty `.samokod/` so
    // agents never mistake it for the main checkout's plan storage.
    if let Err(error) = std::fs::create_dir_all(path.join(".samokod")) {
        log::warn!("failed to seed worktree .samokod dir: {error}");
    }
    Ok(WorktreeRecord {
        path,
        branch,
        base: base.to_string(),
        main_branch: main_branch.to_string(),
    })
}

/// Tip commit of the main checkout. Snapshot at approval so the worktree
/// starts exactly where the main branch was.
pub fn head_commit(repo_root: &Path) -> Result<String, WorktreeError> {
    Ok(run_git(repo_root, &["rev-parse", "HEAD"])?
        .trim()
        .to_string())
}

/// Whether the worktree has uncommitted changes.
/// A worktree with uncommitted changes never merges.
// Live until the MERGING phase wires them into merge gating.
#[allow(dead_code)]
pub fn is_dirty(path: &Path) -> Result<bool, WorktreeError> {
    let output = run_git(path, &["status", "--porcelain"])?;
    Ok(!output.trim().is_empty())
}

/// Whether `main_branch` is an ancestor of `branch`: the fast path.
/// Exit 0 means ancestor, exit 1 means diverged; other failures are loud.
// Live until the MERGING phase wires them into merge gating.
#[allow(dead_code)]
pub fn is_ffable(repo_root: &Path, main_branch: &str, branch: &str) -> Result<bool, WorktreeError> {
    let output = git_command(repo_root)
        .args(["merge-base", "--is-ancestor", main_branch, branch])
        .output()
        .map_err(WorktreeError::Io)?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(WorktreeError::Git {
            args: format!("merge-base --is-ancestor {main_branch} {branch}"),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
    }
}

/// Delete the worktree and then its branch. The branch is always deleted,
/// including on cancel. Only explicit user action passes `force`.
pub fn remove(
    repo_root: &Path,
    path: &Path,
    branch: &str,
    force: bool,
) -> Result<(), WorktreeError> {
    if path.exists() {
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        let path_str = path.to_string_lossy().to_string();
        args.push(path_str.as_str());
        run_git(repo_root, &args)?;
    }
    run_git(repo_root, &["branch", "-D", branch])?;
    let _ = run_git(repo_root, &["worktree", "prune"]);
    Ok(())
}

/// Reconcile worktree metadata after crashes. Never deletes branches.
pub fn prune(repo_root: &Path) -> Result<(), WorktreeError> {
    run_git(repo_root, &["worktree", "prune"])?;
    Ok(())
}

/// Worktree checkout for one plan: `<repo>/.samokod/worktrees/<plan-name>`.
/// Pure.
pub fn worktree_path(repo_root: &Path, plan_name: &str) -> PathBuf {
    repo_root.join(".samokod").join("worktrees").join(plan_name)
}

/// Plan branch: `samokod/<slug>`, the plan name after its timestamp prefix.
/// Pure.
pub fn branch_name(plan_name: &str) -> String {
    format!("samokod/{}", plan_slug(plan_name))
}

/// Slug after the `YYYY-MM-DD.HH-MM-SS` timestamp prefix (plus an optional
/// `-N` collision suffix). Bare timestamps yield the whole name. Pure.
pub fn plan_slug(plan_name: &str) -> String {
    const TIMESTAMP_LEN: usize = 19;
    if plan_name.len() <= TIMESTAMP_LEN {
        return plan_name.to_string();
    }
    let mut rest = &plan_name[TIMESTAMP_LEN..];
    // Optional `-N` collision suffix from `unique_name`.
    if let Some(dash) = rest.strip_prefix('-') {
        let digits = dash
            .bytes()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digits > 0 {
            rest = &dash[digits..];
        }
    }
    if let Some(slug) = rest.strip_prefix('.')
        && !slug.is_empty()
    {
        return slug.to_string();
    }
    plan_name.to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum WorktreeError {
    #[error("git {args} failed: {stderr}")]
    Git { args: String, stderr: String },
    #[error("worktree io failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Git directed by `cwd` alone. Ambient location vars (inherited from a
/// git hook's environment, e.g. a relative `GIT_INDEX_FILE`) never
/// override it: inside a fresh worktree `.git` is a file, so a relative
/// index path fails with `Not a directory`.
fn git_command(cwd: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(cwd);
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
    ] {
        command.env_remove(var);
    }
    command
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<String, WorktreeError> {
    let output = git_command(cwd)
        .args(args)
        .output()
        .map_err(WorktreeError::Io)?;
    if !output.status.success() {
        return Err(WorktreeError::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }
    String::from_utf8(output.stdout).map_err(|_| WorktreeError::Git {
        args: args.join(" "),
        stderr: "git output was not utf-8".to_string(),
    })
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

    fn commit_file(repo: &Path, name: &str, contents: &str, message: &str) {
        std::fs::write(repo.join(name), contents).expect("write");
        for args in [vec!["add", name], vec!["commit", "-m", message]] {
            let status = Command::new("git")
                .args(&args)
                .current_dir(repo)
                .output()
                .expect("git");
            assert!(status.status.success(), "{args:?}");
        }
    }

    #[test]
    fn head_commit_reports_main_tip() {
        let dir = git_repo();
        let root = dir.path();
        let expected = head_commit_shell(root);
        assert_eq!(head_commit(root).expect("head"), expected);
    }

    fn head_commit_shell(repo: &Path) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo)
            .output()
            .expect("git");
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .expect("utf8")
            .trim()
            .to_string()
    }

    fn current_branch(repo: &Path) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(repo)
            .output()
            .expect("git");
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .expect("utf8")
            .trim()
            .to_string()
    }

    #[test]
    fn naming_maps_plan_to_path_and_branch() {
        let root = Path::new("/repo");
        let name = "2026-09-26.14-53-26.shiny-feature";
        assert_eq!(
            worktree_path(root, name),
            PathBuf::from("/repo/.samokod/worktrees/2026-09-26.14-53-26.shiny-feature")
        );
        assert_eq!(branch_name(name), "samokod/shiny-feature");
    }

    #[test]
    fn slug_strips_timestamp_and_collision_suffix() {
        assert_eq!(
            plan_slug("2026-09-26.14-53-26.shiny-feature"),
            "shiny-feature"
        );
        assert_eq!(
            plan_slug("2026-09-26.14-53-26-1.shiny-feature"),
            "shiny-feature"
        );
        assert_eq!(plan_slug("2026-09-26.14-53-26"), "2026-09-26.14-53-26");
    }

    #[test]
    fn create_checks_out_branch_with_empty_samokod() {
        let dir = git_repo();
        let root = dir.path();
        let main = current_branch(root);
        let base = head_commit(root).expect("head");
        let record =
            create(root, "2026-09-26.14-53-26.shiny-feature", &base, &main).expect("create");
        assert_eq!(record.branch, "samokod/shiny-feature");
        assert_eq!(record.base, base);
        assert_eq!(record.main_branch, main);
        assert!(record.path.is_dir());
        assert!(record.path.join(".samokod").is_dir());
        let branch = current_branch(&record.path);
        assert_eq!(branch, "samokod/shiny-feature");
    }

    #[test]
    fn dirty_tracks_uncommitted_changes() {
        let dir = git_repo();
        let root = dir.path();
        let main = current_branch(root);
        let base = head_commit(root).expect("head");
        let record = create(root, "2026-09-26.14-53-26.dirty-check", &base, &main).expect("create");
        assert!(!is_dirty(&record.path).expect("clean"));
        std::fs::write(record.path.join("wip.txt"), "wip\n").expect("write");
        assert!(is_dirty(&record.path).expect("dirty"));
    }

    #[test]
    fn ffable_tracks_main_ancestry() {
        let dir = git_repo();
        let root = dir.path();
        let main = current_branch(root);
        let base = head_commit(root).expect("head");
        let name = "2026-09-26.14-53-26.ff-check";
        let branch = branch_name(name);
        let record = create(root, name, &base, &main).expect("create");
        assert!(is_ffable(root, &main, &branch).expect("ffable"));
        commit_file(root, "main.txt", "main\n", "main moves on");
        assert!(!is_ffable(root, &main, &branch).expect("diverged"));
        remove(root, &record.path, &record.branch, true).expect("remove");
    }

    #[test]
    fn remove_deletes_worktree_and_branch() {
        let dir = git_repo();
        let root = dir.path();
        let main = current_branch(root);
        let base = head_commit(root).expect("head");
        let record = create(root, "2026-09-26.14-53-26.gone-soon", &base, &main).expect("create");
        let path = record.path.clone();
        let branch = record.branch.clone();
        remove(root, &path, &branch, false).expect("remove");
        assert!(!path.exists());
        let output = Command::new("git")
            .args(["branch", "--list", branch.as_str()])
            .current_dir(root)
            .output()
            .expect("git");
        assert!(
            String::from_utf8(output.stdout)
                .expect("utf8")
                .trim()
                .is_empty()
        );
    }

    #[test]
    fn prune_reconciles_without_deleting_branches() {
        let dir = git_repo();
        let root = dir.path();
        let main = current_branch(root);
        let base = head_commit(root).expect("head");
        let record = create(root, "2026-09-26.14-53-26.stale-check", &base, &main).expect("create");
        std::fs::remove_dir_all(&record.path).expect("rmdir");
        prune(root).expect("prune");
        // The branch survives pruning; only metadata reconciles.
        let output = Command::new("git")
            .args(["branch", "--list", record.branch.as_str()])
            .current_dir(root)
            .output()
            .expect("git");
        assert!(
            !String::from_utf8(output.stdout)
                .expect("utf8")
                .trim()
                .is_empty()
        );
        run_git(root, &["branch", "-D", record.branch.as_str()]).expect("cleanup");
    }
}
