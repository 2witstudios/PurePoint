//! Pull requests through `gh`, from `Services/GitService.swift`.
//!
//! gh missing or signed out is reported the way the Mac reports it: the PR list fails with
//! gh's own message (`gh not found` when there is no gh; gh's `gh auth login` hint when it is
//! signed out), which the review's PR tab shows with a Retry button.

use super::model::{DiffData, GitError, GitResult, PullRequestInfo};
use super::patch;
use super::service::GitService;

/// The `--json` fields `PullRequestInfo` decodes.
pub const PR_FIELDS: &str = "number,title,url,state,headRefName,baseRefName,author,labels,reviewDecision,additions,deletions,changedFiles,isDraft,createdAt,updatedAt";

fn stderr_or(stderr: String, fallback: &str) -> String {
    if stderr.is_empty() {
        fallback.to_string()
    } else {
        stderr
    }
}

impl GitService {
    /// Up to 50 open PRs, optionally only those whose head is `branch`.
    pub fn fetch_pr_list_checked(
        &mut self,
        cwd: &str,
        branch: Option<&str>,
    ) -> GitResult<Vec<PullRequestInfo>> {
        let mut args = vec!["pr", "list", "--json", PR_FIELDS, "--limit", "50"];
        if let Some(branch) = branch {
            args.extend(["--head", branch]);
        }
        let result = self.run_gh(&args, cwd, None);
        if !result.success() {
            return Err(GitError::Review(stderr_or(
                result.stderr,
                "GitHub PR lookup failed",
            )));
        }
        serde_json::from_str(&result.stdout)
            .map_err(|error| GitError::Review(format!("The PR list could not be read: {error}")))
    }

    /// A PR's patch: parsed in full, or (`summaries_only`) indexed on disk with deferred bodies.
    pub fn fetch_pr_diff_checked(
        &mut self,
        cwd: &str,
        pr_number: i64,
        summaries_only: bool,
    ) -> GitResult<DiffData> {
        if summaries_only {
            return self.fetch_pr_summary(cwd, pr_number);
        }
        let number = pr_number.to_string();
        let result = self.run_gh(&["pr", "diff", &number], cwd, None);
        if !result.success() {
            return Err(GitError::Review(stderr_or(
                result.stderr,
                "GitHub PR diff failed",
            )));
        }
        let stdout = result.stdout;
        if !(stdout.is_empty() || stdout.starts_with("diff --git ")) {
            return Err(GitError::review(
                "GitHub returned an unrecognized patch; refresh to retry.",
            ));
        }
        // Split only actual file headers, never matching text inside a hunk.
        let files: Vec<_> = stdout
            .split("\ndiff --git ")
            .filter_map(|section| {
                let patch_text = if section.starts_with("diff --git ") {
                    section.to_string()
                } else {
                    format!("diff --git {section}")
                };
                let metadata: Vec<&str> = patch_text.split('\n').collect();
                let (name, status, old) = patch::patch_identity(&metadata, None)?;
                Some(patch::parse_patch(&patch_text, &name, &status, old))
            })
            .collect();
        if !stdout.is_empty() && files.is_empty() {
            return Err(GitError::review(
                "GitHub patch paths could not be read; refresh to retry.",
            ));
        }
        Ok(DiffData { files })
    }

    /// Index a PR patch on disk without retaining its lines.
    fn fetch_pr_summary(&mut self, cwd: &str, pr_number: i64) -> GitResult<DiffData> {
        let snapshot = self.snapshots.create("purepoint-pr");
        let number = pr_number.to_string();
        let result = self.run_gh(&["pr", "diff", &number], cwd, Some(snapshot.clone()));
        self.check_cancelled()?;
        if !result.success() {
            return Err(GitError::Review(stderr_or(
                result.stderr,
                "GitHub PR diff failed",
            )));
        }
        Ok(DiffData {
            files: patch::index_patch(&snapshot, None, &self.cancel)?.files,
        })
    }

    pub fn fetch_pr_list(&mut self, cwd: &str, branch: Option<&str>) -> Vec<PullRequestInfo> {
        self.fetch_pr_list_checked(cwd, branch).unwrap_or_default()
    }

    pub fn fetch_pr_diff(&mut self, cwd: &str, pr_number: i64) -> DiffData {
        self.fetch_pr_diff_checked(cwd, pr_number, false)
            .unwrap_or_default()
    }

    /// Whether gh is installed and signed in (`gh auth status`).
    pub fn is_gh_available(&mut self, cwd: &str) -> bool {
        self.run_gh(&["auth", "status"], cwd, None).success()
    }
}
