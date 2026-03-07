use std::path::Path;

async fn run_git(args: &[&str], cwd: &Path) -> Result<String, std::io::Error> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::other(format!(
            "git {} failed: {stderr}",
            args.join(" ")
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub async fn create_worktree(
    repo_root: &Path,
    worktree_path: &Path,
    branch: &str,
    base: &str,
) -> Result<(), std::io::Error> {
    // Prune stale worktree references so deleted directories don't block reuse
    let _ = run_git(&["worktree", "prune"], repo_root).await;

    // Delete stale branch if it exists (left over from a previous worktree).
    // Ignore errors — the branch may not exist.
    let _ = run_git(&["branch", "-D", branch], repo_root).await;

    let wt_str = worktree_path.to_string_lossy();
    run_git(&["worktree", "add", "-b", branch, &wt_str, base], repo_root).await?;
    Ok(())
}

pub async fn remove_worktree(repo_root: &Path, worktree_path: &Path) -> Result<(), std::io::Error> {
    let wt_str = worktree_path.to_string_lossy();
    run_git(&["worktree", "remove", "--force", &wt_str], repo_root).await?;
    Ok(())
}

pub async fn delete_local_branch(repo_root: &Path, branch: &str) -> Result<(), std::io::Error> {
    run_git(&["branch", "-D", branch], repo_root).await?;
    Ok(())
}

/// Result of running `git diff` against a worktree.
#[derive(Debug)]
pub struct DiffOutput {
    pub diff: String,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

/// Compute the diff for a worktree against its base branch.
/// If `base` is provided, diffs against that branch. Otherwise diffs uncommitted changes.
/// When `stat` is true, returns `--stat` summary instead of full diff.
pub async fn diff_worktree(
    worktree_path: &Path,
    base: Option<&str>,
    stat: bool,
) -> Result<DiffOutput, std::io::Error> {
    // Get the stat summary (always needed for counts)
    let stat_args: Vec<&str> = match base {
        Some(b) => vec!["diff", "--stat", b],
        None => vec!["diff", "--stat", "HEAD"],
    };
    let stat_output = run_git_allow_empty(&stat_args, worktree_path).await?;

    let (files_changed, insertions, deletions) = parse_diff_stat(&stat_output);

    let diff = if stat {
        stat_output
    } else {
        let diff_args: Vec<&str> = match base {
            Some(b) => vec!["diff", b],
            None => vec!["diff", "HEAD"],
        };
        run_git_allow_empty(&diff_args, worktree_path).await?
    };

    Ok(DiffOutput {
        diff,
        files_changed,
        insertions,
        deletions,
    })
}

/// Like run_git but treats empty output as success (no changes = no error).
async fn run_git_allow_empty(args: &[&str], cwd: &Path) -> Result<String, std::io::Error> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::other(format!(
            "git {} failed: {stderr}",
            args.join(" ")
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Parse the summary line of `git diff --stat` to extract counts.
/// Example: " 3 files changed, 10 insertions(+), 2 deletions(-)"
fn parse_diff_stat(stat: &str) -> (usize, usize, usize) {
    let Some(summary) = stat.lines().last() else {
        return (0, 0, 0);
    };
    let mut files = 0;
    let mut ins = 0;
    let mut del = 0;
    for part in summary.split(',') {
        let part = part.trim();
        if part.contains("file") {
            files = part
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
        } else if part.contains("insertion") {
            ins = part
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
        } else if part.contains("deletion") {
            del = part
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
        }
    }
    (files, ins, del)
}

pub async fn delete_remote_branch(repo_root: &Path, branch: &str) -> Result<(), std::io::Error> {
    run_git(&["push", "origin", "--delete", branch], repo_root).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_git_repo(dir: &std::path::Path) {
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir)
            .output()
            .unwrap();
        let output = std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@test.com",
                "commit",
                "--allow-empty",
                "-m",
                "init",
            ])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git init commit failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_git_repo_should_create_worktree() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());

        let wt_path = tmp.path().join("wt-test");
        let result = create_worktree(tmp.path(), &wt_path, "pu/test-branch", "HEAD").await;
        assert!(result.is_ok(), "create_worktree failed: {result:?}");
        assert!(wt_path.exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_existing_worktree_should_remove_it() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());

        let wt_path = tmp.path().join("wt-remove");
        create_worktree(tmp.path(), &wt_path, "pu/remove-branch", "HEAD")
            .await
            .unwrap();
        assert!(wt_path.exists());

        let result = remove_worktree(tmp.path(), &wt_path).await;
        assert!(result.is_ok(), "remove_worktree failed: {result:?}");
        assert!(!wt_path.exists());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_non_git_dir_should_fail() {
        let tmp = TempDir::new().unwrap();
        let wt_path = tmp.path().join("wt-fail");
        let result = create_worktree(tmp.path(), &wt_path, "pu/fail", "HEAD").await;
        assert!(result.is_err());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_branch_should_delete_locally() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());

        // Create a branch via worktree, then remove worktree to leave the branch
        let wt_path = tmp.path().join("wt-del");
        create_worktree(tmp.path(), &wt_path, "pu/del-branch", "HEAD")
            .await
            .unwrap();
        remove_worktree(tmp.path(), &wt_path).await.unwrap();

        let result = delete_local_branch(tmp.path(), "pu/del-branch").await;
        assert!(result.is_ok(), "delete_local_branch failed: {result:?}");

        // Verify branch is gone
        let output = std::process::Command::new("git")
            .args(["branch", "--list", "pu/del-branch"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        let branches = String::from_utf8_lossy(&output.stdout);
        assert!(branches.trim().is_empty(), "branch should be deleted");
    }

    #[test]
    fn given_stat_summary_should_parse_counts() {
        let stat = " src/main.rs | 10 ++++------\n src/lib.rs  |  5 ++---\n 2 files changed, 6 insertions(+), 9 deletions(-)\n";
        let (files, ins, del) = super::parse_diff_stat(stat);
        assert_eq!(files, 2);
        assert_eq!(ins, 6);
        assert_eq!(del, 9);
    }

    #[test]
    fn given_empty_stat_should_return_zeros() {
        let (files, ins, del) = super::parse_diff_stat("");
        assert_eq!(files, 0);
        assert_eq!(ins, 0);
        assert_eq!(del, 0);
    }

    #[test]
    fn given_insertions_only_should_parse() {
        let stat = " 1 file changed, 3 insertions(+)\n";
        let (files, ins, del) = super::parse_diff_stat(stat);
        assert_eq!(files, 1);
        assert_eq!(ins, 3);
        assert_eq!(del, 0);
    }

    #[test]
    fn given_deletions_only_should_parse() {
        let stat = " 1 file changed, 2 deletions(-)\n";
        let (files, ins, del) = super::parse_diff_stat(stat);
        assert_eq!(files, 1);
        assert_eq!(ins, 0);
        assert_eq!(del, 2);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_worktree_with_changes_should_diff() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());

        let wt_path = tmp.path().join("wt-diff");
        create_worktree(tmp.path(), &wt_path, "pu/diff-test", "HEAD")
            .await
            .unwrap();

        // Make a change in the worktree
        std::fs::write(wt_path.join("test.txt"), "hello\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "test.txt"])
            .current_dir(&wt_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@test.com",
                "commit",
                "-m",
                "add test file",
            ])
            .current_dir(&wt_path)
            .output()
            .unwrap();

        // Diff against base (HEAD of main)
        let result = diff_worktree(&wt_path, Some("HEAD~1"), false).await;
        assert!(result.is_ok(), "diff_worktree failed: {result:?}");
        let output = result.unwrap();
        assert!(output.diff.contains("hello"));
        assert_eq!(output.files_changed, 1);
        assert!(output.insertions > 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_worktree_with_no_changes_should_return_empty_diff() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());

        let wt_path = tmp.path().join("wt-nodiff");
        create_worktree(tmp.path(), &wt_path, "pu/nodiff-test", "HEAD")
            .await
            .unwrap();

        let result = diff_worktree(&wt_path, None, false).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.diff.trim().is_empty());
        assert_eq!(output.files_changed, 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_stat_flag_should_return_stat_output() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());

        let wt_path = tmp.path().join("wt-stat");
        create_worktree(tmp.path(), &wt_path, "pu/stat-test", "HEAD")
            .await
            .unwrap();

        std::fs::write(wt_path.join("file.txt"), "content\n").unwrap();
        std::process::Command::new("git")
            .args(["add", "file.txt"])
            .current_dir(&wt_path)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@test.com",
                "commit",
                "-m",
                "add file",
            ])
            .current_dir(&wt_path)
            .output()
            .unwrap();

        let result = diff_worktree(&wt_path, Some("HEAD~1"), true).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.diff.contains("file.txt"));
        assert!(output.diff.contains("file changed") || output.diff.contains("files changed"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_no_remote_should_fail_delete_remote_branch() {
        let tmp = TempDir::new().unwrap();
        init_git_repo(tmp.path());

        // No remote configured, so this should fail
        let result = delete_remote_branch(tmp.path(), "pu/no-remote").await;
        assert!(result.is_err());
    }
}
