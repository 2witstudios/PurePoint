use std::path::Path;

pub async fn create_worktree(
    repo_root: &Path,
    worktree_path: &Path,
    branch: &str,
    base: &str,
) -> Result<(), std::io::Error> {
    let output = tokio::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "-b",
            branch,
            &worktree_path.to_string_lossy(),
            base,
        ])
        .current_dir(repo_root)
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::other(
            format!("git worktree add failed: {stderr}"),
        ));
    }
    Ok(())
}

pub async fn remove_worktree(
    repo_root: &Path,
    worktree_path: &Path,
) -> Result<(), std::io::Error> {
    let output = tokio::process::Command::new("git")
        .args([
            "worktree",
            "remove",
            "--force",
            &worktree_path.to_string_lossy(),
        ])
        .current_dir(repo_root)
        .output()
        .await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(std::io::Error::other(
            format!("git worktree remove failed: {stderr}"),
        ));
    }
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
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "init"])
            .current_dir(dir)
            .output()
            .unwrap();
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
}
