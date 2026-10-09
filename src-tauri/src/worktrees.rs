// Worktree operations: one isolated checkout per executing plan.
// Lifecycle first, pure naming helpers below, git edge at the bottom.
use std::path::{Path, PathBuf};

/// One plan checkout: where it lives and which branch it spans. The
/// landing target is never stored: callers resolve the live checkout branch
/// at decision time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRecord {
    pub path: PathBuf,
    pub worktree_branch: String,
}

/// Create a worktree for one plan on a fresh branch at `base`.
/// Fails loud with git stderr preserved.
pub fn create(
    repo_root: &Path,
    plan_name: &str,
    base: &str,
) -> Result<WorktreeRecord, WorktreeError> {
    let worktree_branch = branch_name(plan_name);
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
            worktree_branch.as_str(),
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
        worktree_branch,
    })
}

/// Tip commit of the main checkout. Snapshot at approval so the worktree
/// starts exactly where the checkout was.
pub fn head_commit(repo_root: &Path) -> Result<String, WorktreeError> {
    Ok(run_git(repo_root, &["rev-parse", "HEAD"])?
        .trim()
        .to_string())
}

/// Whether the worktree has uncommitted changes.
/// A worktree with uncommitted changes never lands.
pub fn is_dirty(path: &Path) -> Result<bool, WorktreeError> {
    let output = run_git(path, &["status", "--porcelain"])?;
    Ok(!output.trim().is_empty())
}

/// Commits the worktree branch added on top of the target, oldest first,
/// as `short-hash subject` lines. Empty when nothing changed. Fails loud
/// with git stderr preserved.
pub fn commits(
    repo_root: &Path,
    target_branch: &str,
    worktree_branch: &str,
) -> Result<Vec<String>, WorktreeError> {
    let range = format!("{target_branch}..{worktree_branch}");
    let output = run_git(
        repo_root,
        &["log", "--format=%h %s", "--reverse", range.as_str()],
    )?;
    Ok(output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

/// Whether `target_branch` is an ancestor of `worktree_branch`: the fast path.
/// Exit 0 means ancestor, exit 1 means diverged; other failures are loud.
pub fn is_ffable(
    repo_root: &Path,
    target_branch: &str,
    worktree_branch: &str,
) -> Result<bool, WorktreeError> {
    let output = crate::git::command(repo_root)
        .args([
            "merge-base",
            "--is-ancestor",
            target_branch,
            worktree_branch,
        ])
        .output()
        .map_err(WorktreeError::Io)?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(WorktreeError::Git {
            args: format!("merge-base --is-ancestor {target_branch} {worktree_branch}"),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
    }
}

/// Delete the worktree and then its branch. The branch is always deleted,
/// including on cancel. Only explicit user action passes `force`. A
/// missing worktree path still deletes the branch, but a missing branch
/// is quiet so cancelling after a partial failure still cleans up.
pub fn remove(
    repo_root: &Path,
    path: &Path,
    worktree_branch: &str,
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
        run_git(repo_root, &["branch", "-D", worktree_branch])?;
    } else {
        match run_git(repo_root, &["branch", "-D", worktree_branch]) {
            Ok(_) => {}
            Err(WorktreeError::Git { stderr, .. }) if stderr.contains("not found") => {}
            Err(error) => return Err(error),
        }
    }
    let _ = run_git(repo_root, &["worktree", "prune"]);
    Ok(())
}

/// Fast-forward the live checkout branch to `worktree_branch` with
/// `git merge --ff-only`. Refuses a diverged branch loud. The target is
/// always the live checkout branch the caller passes in.
pub fn fast_forward(
    repo_root: &Path,
    target_branch: &str,
    worktree_branch: &str,
) -> Result<(), WorktreeError> {
    if !is_ffable(repo_root, target_branch, worktree_branch)? {
        return Err(WorktreeError::NotAncestor {
            target_branch: target_branch.to_string(),
            worktree_branch: worktree_branch.to_string(),
        });
    }
    run_git(repo_root, &["merge", "--ff-only", worktree_branch])?;
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
    #[error("{worktree_branch} is not a fast-forward of {target_branch}")]
    NotAncestor {
        target_branch: String,
        worktree_branch: String,
    },
    #[error("worktree io failed: {0}")]
    Io(#[from] std::io::Error),
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<String, WorktreeError> {
    let output = crate::git::command(cwd)
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
            let status = test_git(dir.path(), &args);
            assert!(status.status.success(), "{args:?}");
        }
        dir
    }

    fn commit_file(repo: &Path, name: &str, contents: &str, message: &str) {
        std::fs::write(repo.join(name), contents).expect("write");
        for args in [vec!["add", name], vec!["commit", "-m", message]] {
            let status = test_git(repo, &args);
            assert!(status.status.success(), "{args:?}");
        }
    }

    fn test_git(repo: &Path, args: &[&str]) -> std::process::Output {
        crate::git::command(repo).args(args).output().expect("git")
    }

    #[test]
    fn head_commit_reports_checkout_tip() {
        let dir = git_repo();
        let root = dir.path();
        let expected = head_commit_shell(root);
        assert_eq!(head_commit(root).expect("head"), expected);
    }

    fn head_commit_shell(repo: &Path) -> String {
        let output = test_git(repo, &["rev-parse", "HEAD"]);
        assert!(output.status.success());
        String::from_utf8(output.stdout)
            .expect("utf8")
            .trim()
            .to_string()
    }

    fn current_branch(repo: &Path) -> String {
        let output = test_git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]);
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
        let base = head_commit(root).expect("head");
        let record = create(root, "2026-09-26.14-53-26.shiny-feature", &base).expect("create");
        assert_eq!(record.worktree_branch, "samokod/shiny-feature");
        assert!(record.path.is_dir());
        assert!(record.path.join(".samokod").is_dir());
        let branch = current_branch(&record.path);
        assert_eq!(branch, "samokod/shiny-feature");
    }

    #[test]
    fn dirty_tracks_uncommitted_changes() {
        let dir = git_repo();
        let root = dir.path();
        let base = head_commit(root).expect("head");
        let record = create(root, "2026-09-26.14-53-26.dirty-check", &base).expect("create");
        assert!(!is_dirty(&record.path).expect("clean"));
        std::fs::write(record.path.join("wip.txt"), "wip\n").expect("write");
        assert!(is_dirty(&record.path).expect("dirty"));
    }

    #[test]
    fn ffable_tracks_target_ancestry() {
        let dir = git_repo();
        let root = dir.path();
        let target = current_branch(root);
        let base = head_commit(root).expect("head");
        let name = "2026-09-26.14-53-26.ff-check";
        let branch = branch_name(name);
        let record = create(root, name, &base).expect("create");
        assert!(is_ffable(root, &target, &branch).expect("ffable"));
        commit_file(root, "target.txt", "target\n", "target moves on");
        assert!(!is_ffable(root, &target, &branch).expect("diverged"));
        remove(root, &record.path, &record.worktree_branch, true).expect("remove");
    }

    #[test]
    fn commits_lists_branch_work_oldest_first() {
        let dir = git_repo();
        let root = dir.path();
        let target = current_branch(root);
        let base = head_commit(root).expect("head");
        let name = "2026-09-26.14-53-26.ever-check";
        let branch = branch_name(name);
        let record = create(root, name, &base).expect("create");
        assert!(commits(root, &target, &branch).expect("commits").is_empty());
        commit_file(&record.path, "work.txt", "work\n", "plan work");
        commit_file(&record.path, "more.txt", "more\n", "more work");
        let listed = commits(root, &target, &branch).expect("commits");
        assert_eq!(listed.len(), 2);
        assert!(listed[0].ends_with("plan work"));
        assert!(listed[1].ends_with("more work"));
        remove(root, &record.path, &record.worktree_branch, true).expect("remove");
    }

    #[test]
    fn fast_forward_advances_target_to_branch() {
        let dir = git_repo();
        let root = dir.path();
        let target = current_branch(root);
        let base = head_commit(root).expect("head");
        let record = create(root, "2026-09-26.14-53-26.ff-soon", &base).expect("create");
        commit_file(&record.path, "work.txt", "work\n", "plan work");
        let tip = head_commit(&record.path).expect("tip");
        fast_forward(root, &target, &record.worktree_branch).expect("ff");
        assert_eq!(head_commit(root).expect("head"), tip);
    }

    #[test]
    fn fast_forward_refuses_diverged_branch() {
        let dir = git_repo();
        let root = dir.path();
        let target = current_branch(root);
        let base = head_commit(root).expect("head");
        let record = create(root, "2026-09-26.14-53-26.no-ff", &base).expect("create");
        commit_file(&record.path, "work.txt", "work\n", "plan work");
        commit_file(root, "target.txt", "target\n", "target moves on");
        assert!(matches!(
            fast_forward(root, &target, &record.worktree_branch),
            Err(WorktreeError::NotAncestor { .. })
        ));
        remove(root, &record.path, &record.worktree_branch, true).expect("remove");
    }

    #[test]
    fn remove_deletes_worktree_and_branch() {
        let dir = git_repo();
        let root = dir.path();
        let base = head_commit(root).expect("head");
        let record = create(root, "2026-09-26.14-53-26.gone-soon", &base).expect("create");
        let path = record.path.clone();
        let branch = record.worktree_branch.clone();
        remove(root, &path, &branch, false).expect("remove");
        assert!(!path.exists());
        let output = test_git(root, &["branch", "--list", branch.as_str()]);
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
        let base = head_commit(root).expect("head");
        let record = create(root, "2026-09-26.14-53-26.stale-check", &base).expect("create");
        std::fs::remove_dir_all(&record.path).expect("rmdir");
        prune(root).expect("prune");
        // The branch survives pruning; only metadata reconciles.
        let output = test_git(root, &["branch", "--list", record.worktree_branch.as_str()]);
        assert!(
            !String::from_utf8(output.stdout)
                .expect("utf8")
                .trim()
                .is_empty()
        );
        run_git(root, &["branch", "-D", record.worktree_branch.as_str()]).expect("cleanup");
    }
}
