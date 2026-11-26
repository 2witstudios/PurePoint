//! Port of `GitReviewTests.swift`. One test per Swift `@Test` (snake_case names); each keeps
//! the Swift checks' messages. Assertions on `DiffState` itself (loading flags, tab
//! switching, generation tokens, in-flight PR refresh) belong to the DiffState port
//! (WIN-6.0, `ui/src/state/diffState.ts`); the GitService results those assertions rest on
//! are checked here. `WorktreeWatcher.metadataPaths` is WIN-5.6.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::super::host::LocalProjectHost;
use super::super::model::{FileDiff, GitError, LineType, ReviewPage, ReviewPageCursor};
use super::super::runner::CancelToken;
use super::super::service::GitService;
use super::*;

fn find<'a>(files: &'a [FileDiff], name: &str) -> &'a FileDiff {
    files
        .iter()
        .find(|file| file.filename == name)
        .unwrap_or_else(|| panic!("no {name} in {:?}", names(files)))
}

fn names(files: &[FileDiff]) -> Vec<&str> {
    files.iter().map(|file| file.filename.as_str()).collect()
}

fn has_line(file: &FileDiff, content: &str, line_type: LineType) -> bool {
    file.hunks.first().is_some_and(|hunk| {
        hunk.lines
            .iter()
            .any(|l| l.content == content && l.line_type == line_type)
    })
}

fn page_has(page: &ReviewPage, content: &str) -> bool {
    page.hunks
        .first()
        .is_some_and(|hunk| hunk.lines.iter().any(|l| l.content == content))
}

fn last_content(page: &ReviewPage) -> Option<&str> {
    page.hunks
        .first()
        .and_then(|hunk| hunk.lines.last())
        .map(|line| line.content.as_str())
}

/// A name git C-quotes in patches that NTFS can still hold (the Swift test uses
/// `quoted "name"<TAB>line<LF>ü.txt`).
const QUOTED: &str = "quoted 'name' [line] ü.txt";

#[test]
fn real_repositories() {
    let repo = repo();
    let (dir, path) = (repo.path(), repo.str());
    let path = path.as_str();
    let mut service = GitService::default();
    write("original\n", "same.txt", dir);
    write("remove\n", "deleted.txt", dir);
    write("rename\n", "old name.txt", dir);
    commit("initial", dir);
    let root = git(&["rev-parse", "HEAD"], dir);
    let initial_patch = service.fetch_commit_diff(path, &root, false).unwrap();
    assert_eq!(initial_patch.len(), 3, "initial commit patches");
    git(&["checkout", "-b", "feature"], dir);
    write("branch\n", "same.txt", dir);
    commit("branch one", dir);
    let first = git(&["rev-parse", "HEAD"], dir);
    write("branch two\n", "other.txt", dir);
    commit("branch two", dir);
    git(&["checkout", "main"], dir);
    write("base advanced\n", "base-only.txt", dir);
    commit("base advances", dir);
    git(&["checkout", "feature"], dir);
    let branch = service.fetch_branch_review(path, Some("main"), false);
    assert!(
        branch.error.is_none() && branch.commits.len() == 2,
        "base-exclusive commits: {branch:?}"
    );
    assert_eq!(
        names(&branch.files).into_iter().collect::<HashSet<_>>(),
        HashSet::from(["same.txt", "other.txt"]),
        "cumulative merge-base patch excludes base-only changes"
    );
    let selected = service.fetch_commit_diff(path, &first, false).unwrap();
    assert_eq!(names(&selected), ["same.txt"], "selected commit patch");
    write("staged\n", "same.txt", dir);
    git(&["add", "same.txt"], dir);
    write("unstaged\n", "same.txt", dir);
    write("untracked content\n", QUOTED, dir);
    write([0u8, 1, 255, 0], "binary.bin", dir);
    git(&["mv", "old name.txt", "new name.txt"], dir);
    std::fs::remove_file(dir.join("deleted.txt")).unwrap();
    let local = service.fetch_local_review(path, false);
    assert!(
        local.error.is_none(),
        "local patch success: {:?}",
        local.error
    );
    assert!(
        has_line(
            find(&local.staged, "same.txt"),
            "staged",
            LineType::Addition
        ),
        "actual staged content"
    );
    assert!(
        has_line(
            find(&local.unstaged, "same.txt"),
            "unstaged",
            LineType::Addition
        ),
        "actual unstaged content"
    );
    assert_eq!(
        find(&local.untracked, QUOTED).added,
        1,
        "exact quoted UTF8 path with patch"
    );
    assert!(
        find(&local.untracked, "binary.bin").is_binary,
        "binary evidence"
    );
    assert_eq!(
        find(&local.staged, "new name.txt").old_filename.as_deref(),
        Some("old name.txt"),
        "rename evidence"
    );
    assert_eq!(
        find(&local.unstaged, "deleted.txt").status_code,
        "D",
        "deleted path patch"
    );
    assert_eq!(
        local.unique_path_count(),
        5,
        "staged+unstaged path counted once"
    );
    let lazy = service.fetch_local_review(path, true);
    assert!(
        lazy.error.is_none()
            && find(&lazy.staged, "new name.txt").old_filename.as_deref() == Some("old name.txt"),
        "lazy rename metadata"
    );
    let lazy_staged = service
        .fetch_review_page(find(&lazy.staged, "same.txt"), path, None)
        .unwrap();
    let lazy_unstaged = service
        .fetch_review_page(find(&lazy.unstaged, "same.txt"), path, None)
        .unwrap();
    assert!(
        lazy_staged.hunks[0]
            .lines
            .iter()
            .any(|l| l.content == "staged" && l.line_type == LineType::Addition),
        "lazy staged comparison remains distinct"
    );
    assert!(
        lazy_unstaged.hunks[0]
            .lines
            .iter()
            .any(|l| l.content == "unstaged" && l.line_type == LineType::Addition),
        "lazy unstaged comparison remains distinct"
    );
    let quoted_page = service
        .fetch_review_page(find(&lazy.untracked, QUOTED), path, None)
        .unwrap();
    assert_eq!(
        last_content(&quoted_page),
        Some("untracked content"),
        "lazy quoted path"
    );
    let summary = service.review_summary(path, Some("main"));
    assert!(
        summary.error.is_none() && summary.commit_count == 2 && summary.local_file_count == 5,
        "compact summary: {summary:?}"
    );
    let invalid = service.fetch_branch_review(path, Some("missing-base"), false);
    assert!(
        invalid.error.is_some() && invalid.comparison_base == "missing-base",
        "recorded invalid base never falls back"
    );
    git(&["reset", "--hard"], dir);
    git(&["checkout", "--orphan", "unrelated"], dir);
    git(&["rm", "-rf", "."], dir);
    write("orphan\n", "orphan.txt", dir);
    commit("unrelated root", dir);
    let unrelated = service.fetch_branch_review(path, Some("main"), false);
    assert!(
        unrelated.error.is_some() && unrelated.files.is_empty(),
        "unrelated base error"
    );

    let unborn = super::repo();
    write("staged root\n", "root.txt", unborn.path());
    git(&["add", "root.txt"], unborn.path());
    write("local root\n", "root.txt", unborn.path());
    write("new\n", "new.txt", unborn.path());
    let unborn_local = service.fetch_local_review(&unborn.str(), false);
    let unborn_branch = service.fetch_branch_review(&unborn.str(), Some("main"), false);
    assert!(
        unborn_local.error.is_none()
            && unborn_local.staged.len() == 1
            && unborn_local.unstaged.len() == 1
            && unborn_local.untracked.len() == 1,
        "unborn local patches: {unborn_local:?}"
    );
    assert!(
        unborn_branch.error.is_some(),
        "unborn branch error disclosed"
    );
    // The linked-worktree `WorktreeWatcher.metadataPaths` checks are ported with WIN-5.6.

    // Track the difficult paths as well, then verify branch/commit metadata is exact.
    git(&["checkout", "feature"], dir);
    write("quoted committed\n", QUOTED, dir);
    write([0u8, 255, 0, 1], "tracked-binary.bin", dir);
    git(&["mv", "old name.txt", "committed rename.txt"], dir);
    commit("quoted binary rename", dir);
    let difficult = service.fetch_branch_review(path, Some("main"), false);
    assert_eq!(
        find(&difficult.files, QUOTED).added,
        1,
        "quoted committed branch patch"
    );
    assert!(
        find(&difficult.files, "tracked-binary.bin").is_binary,
        "binary committed branch patch"
    );
    assert_eq!(
        find(&difficult.files, "committed rename.txt")
            .old_filename
            .as_deref(),
        Some("old name.txt"),
        "cumulative committed rename"
    );
    // Prefer the configured upstream only when there is no recorded base metadata.
    git(&["branch", "--set-upstream-to=main"], dir);
    let fallback = service.fetch_branch_review(path, None, false);
    assert!(
        fallback.comparison_base == "main" && fallback.error.is_none(),
        "upstream fallback: {fallback:?}"
    );

    let conflict = super::repo();
    let (cdir, cpath) = (conflict.path(), conflict.str());
    let cpath = cpath.as_str();
    write("base\n", "conflict.txt", cdir);
    write("unchanged\n", "ordinary.txt", cdir);
    commit("base", cdir);
    git(&["checkout", "-b", "side"], cdir);
    write("side\n", "conflict.txt", cdir);
    commit("side", cdir);
    git(&["checkout", "main"], cdir);
    write("ours\n", "conflict.txt", cdir);
    commit("ours", cdir);
    // The merge must fail and leave real index stages 1/2/3.
    conflicting_merge("side", cdir);
    write("staged ordinary\n", "ordinary.txt", cdir);
    git(&["add", "ordinary.txt"], cdir);
    write("unstaged ordinary\n", "ordinary.txt", cdir);
    write("new file\n", "untracked.txt", cdir);
    let conflicted = service.fetch_local_review(cpath, false);
    assert!(
        conflicted.error.is_none()
            && conflicted.staged.len() == 2
            && conflicted.unstaged.len() == 2
            && conflicted.untracked.len() == 1,
        "unresolved merge retains all local groups: {conflicted:?}"
    );
    assert_eq!(
        find(&conflicted.staged, "conflict.txt").status_code,
        "U",
        "explicit staged conflict"
    );
    assert!(
        find(&conflicted.unstaged, "conflict.txt").hunks[0]
            .lines
            .iter()
            .any(|l| l.content == "<<<<<<< HEAD"),
        "working conflict patch against ours"
    );
    assert_eq!(
        conflicted.unique_path_count(),
        3,
        "conflict path counted once"
    );
    let lazy_conflict = service.fetch_local_review(cpath, true);
    assert!(
        lazy_conflict.error.is_none()
            && lazy_conflict.staged.len() == 2
            && lazy_conflict.unstaged.len() == 2,
        "lazy conflicts retain unique paths in both groups"
    );
    let staged_conflict_page = service
        .fetch_review_page(find(&lazy_conflict.staged, "conflict.txt"), cpath, None)
        .unwrap();
    assert!(
        staged_conflict_page.hunks.is_empty() && !staged_conflict_page.has_more,
        "marker-only staged conflict is a valid no-body preview"
    );
    let conflict_page = service
        .fetch_review_page(find(&lazy_conflict.unstaged, "conflict.txt"), cpath, None)
        .unwrap();
    assert!(
        page_has(&conflict_page, "<<<<<<< HEAD"),
        "lazy conflict baseline retains actual working content"
    );
    git(&["reset", "--hard", "HEAD"], cdir);
    git(&["checkout", "-b", "ours-deleted"], cdir);
    git(&["rm", "conflict.txt"], cdir);
    commit("ours deletes", cdir);
    conflicting_merge("side", cdir);
    let no_ours = service.fetch_local_review(cpath, false);
    let surviving = find(&no_ours.unstaged, "conflict.txt");
    assert!(
        no_ours.error.is_none() && surviving.status_code == "U" && surviving.added == 1,
        "ours-deleted surviving file patch: {surviving:?}"
    );
    assert!(
        surviving.conflict_baseline.as_deref() == Some("empty (ours deleted)")
            && surviving.hunks[0].lines.last().map(|l| l.content.as_str()) == Some("side"),
        "explicit empty baseline and actual surviving content"
    );
    write([0u8, 1, 255], "conflict.txt", cdir);
    let no_ours_binary = service.fetch_local_review(cpath, false);
    assert!(
        find(&no_ours_binary.unstaged, "conflict.txt").is_binary,
        "ours-deleted binary evidence"
    );
    git(&["reset", "--hard", "HEAD"], cdir);
    git(&["checkout", "side"], cdir);
    conflicting_merge("ours-deleted", cdir);
    std::fs::remove_file(cdir.join("conflict.txt")).unwrap();
    let ours_only = service.fetch_local_review(cpath, false);
    let removed = find(&ours_only.unstaged, "conflict.txt");
    assert!(
        ours_only.error.is_none()
            && removed.removed == 1
            && removed.conflict_baseline.as_deref() == Some("ours (index stage 2)"),
        "theirs-deleted working deletion uses existing ours baseline: {removed:?}"
    );

    // Merge reconciliation preserves workspace HEAD-to-working-file APIs and capture caps.
    let workspace = super::repo();
    let (wdir, wpath) = (workspace.path(), workspace.str());
    write("root\n", "file.txt", wdir);
    commit("root", wdir);
    write("staged\n", "file.txt", wdir);
    git(&["add", "file.txt"], wdir);
    write("working\n", "file.txt", wdir);
    write("new\n", "quoted 'new'.txt", wdir);
    let workspace_files = service.fetch_working_tree_changes(&wpath).unwrap();
    assert_eq!(
        workspace_files.len(),
        2,
        "workspace staged/unstaged/untracked overview preserved"
    );
    let workspace_patch = service
        .fetch_working_tree_file_diff(&wpath, find(&workspace_files, "file.txt"))
        .unwrap();
    assert!(
        has_line(&workspace_patch, "working", LineType::Addition),
        "workspace diff keeps final working contents"
    );
    let limited = service.run_git(&["diff", "HEAD", "--", "file.txt"], &wpath, Some(8));
    assert!(
        limited.output_exceeded_limit && limited.stdout.is_empty(),
        "workspace output capture cap preserved"
    );
    if std::env::var("GIT_REVIEW_SLOW_LOCAL_CHECK").as_deref() == Ok("1") {
        let slow = service.run_git(
            &["-c", "alias.slow=!sleep 26; printf complete", "slow"],
            &wpath,
            None,
        );
        assert!(
            slow.success() && slow.stdout == "complete",
            "local Git must outlive remote-only 25-second deadline"
        );
    }

    let previews = super::repo();
    let (pdir, ppath) = (previews.path(), previews.str());
    for index in 0..40 {
        write(
            format!("file {index}\n"),
            &format!("new{index:02}.txt"),
            pdir,
        );
    }
    write("x".repeat(1_100_000), "aa-large.txt", pdir);
    std::fs::create_dir_all(pdir.join("aa-nested")).unwrap();
    git(&["init"], &pdir.join("aa-nested"));
    write("nested\n", "aa-nested/file.txt", pdir);
    let preview_review = service.fetch_local_review(&ppath, false);
    assert!(
        preview_review.untracked_error.is_none() && preview_review.untracked.len() == 42,
        "one bad untracked path must not discard group: {}",
        preview_review.untracked.len()
    );
    assert!(
        find(&preview_review.untracked, "aa-large.txt")
            .preview_error
            .is_some(),
        "large untracked capture bounded"
    );
    assert!(
        preview_review
            .untracked
            .iter()
            .find(|f| f.filename.starts_with("aa-nested"))
            .unwrap()
            .preview_error
            .is_some(),
        "nested repository placeholder"
    );
    let deferred = preview_review
        .untracked
        .iter()
        .find(|f| f.preview_deferred)
        .unwrap()
        .filename
        .clone();
    let explicit = service.untracked_preview(&deferred, &ppath, None);
    assert_eq!(
        explicit.hunks[0].lines.len(),
        1,
        "deferred preview explicitly loadable"
    );
    let reloaded = service.fetch_local_review(&ppath, false);
    assert!(
        !find(&reloaded.untracked, &deferred).preview_deferred,
        "explicit preview survives polling"
    );
    write("changed\n", &deferred, pdir);
    let changed = service.untracked_preview(&deferred, &ppath, None);
    assert_eq!(
        changed.hunks[0].lines[0].content, "changed",
        "metadata cache invalidated after edit"
    );
}

const PRS_JSON: &str = r#"[{"number":1,"title":"One","url":"https://example.test/1","state":"OPEN","headRefName":"feature","baseRefName":"main","author":{"login":"test"},"labels":[],"additions":1,"deletions":0,"changedFiles":1,"isDraft":false,"createdAt":"today","updatedAt":"today"}]"#;
const PR_PATCH: &str = "diff --git a/remote.txt b/remote.txt\nnew file mode 100644\n--- /dev/null\n+++ b/remote.txt\n@@ -0,0 +1 @@\n+remote first\n";

/// `git diff --cached` of `quoted "name"<TAB>ü.txt` and `binary "name".bin`, as the Swift
/// test captures it (those names cannot be created on NTFS).
const QUOTED_PR_PATCH: &str = concat!(
    "diff --git \"a/binary \\\"name\\\".bin\" \"b/binary \\\"name\\\".bin\"\n",
    "new file mode 100644\n",
    "index 0000000..bd0b5a4\n",
    "Binary files /dev/null and \"b/binary \\\"name\\\".bin\" differ\n",
    "diff --git \"a/quoted \\\"name\\\"\\t\\303\\274.txt\" \"b/quoted \\\"name\\\"\\t\\303\\274.txt\"\n",
    "new file mode 100644\n",
    "index 0000000..a3a2d4a\n",
    "--- /dev/null\n",
    "+++ \"b/quoted \\\"name\\\"\\t\\303\\274.txt\"\n",
    "@@ -0,0 +1 @@\n",
    "+quoted\n",
    "\n",
);

#[test]
fn selections_and_remote_refresh() {
    let repo = repo();
    let (dir, path) = (repo.path(), repo.str());
    let path = path.as_str();
    write("root\n", "root.txt", dir);
    commit("root", dir);
    git(&["checkout", "-b", "feature"], dir);
    write("first\n", "first.txt", dir);
    commit("first", dir);
    write("second\n", "second.txt", dir);
    commit("second", dir);
    write(PRS_JSON, "prs.json", dir);
    write(PR_PATCH, "pr.patch", dir);
    // The fake files must not count as review changes; keep them out of git's view.
    write(
        "prs.json\npr.patch\nfail\nheld-object\n",
        ".git/info/exclude",
        dir,
    );
    let mut service = fake_gh_service();

    let branch = service.fetch_branch_review(path, Some("main"), false);
    assert!(
        branch.commits.len() == 2 && branch.comparison_base == "main",
        "default branch and recorded base"
    );
    let prs = service
        .fetch_pr_list_checked(path, Some("feature"))
        .unwrap();
    assert!(
        prs.len() == 1 && prs[0].number == 1 && prs[0].author.login == "test",
        "PR list decodes"
    );
    let full = service.fetch_pr_diff_checked(path, 1, false).unwrap();
    assert_eq!(full.files[0].added, 1, "remote initial patch");
    let lazy = service.fetch_pr_diff_checked(path, 1, true).unwrap();
    assert!(
        lazy.files[0].hunks.is_empty() && lazy.files[0].added == 1,
        "PR tab loads metadata with deferred bodies"
    );
    let lazy_page = service
        .fetch_review_page(&lazy.files[0], path, None)
        .unwrap();
    assert_eq!(
        last_content(&lazy_page),
        Some("remote first"),
        "PR body loads through overview page routing"
    );

    // A selected commit whose objects are unreadable reports an error; it loads once they return.
    let blob = git(&["rev-parse", "HEAD:second.txt"], dir);
    let object = dir.join(".git/objects").join(&blob[..2]).join(&blob[2..]);
    let held = dir.join("held-object");
    std::fs::rename(&object, &held).unwrap();
    let newest = branch.commits[0].sha.clone();
    assert!(
        service.fetch_commit_diff(path, &newest, false).is_err(),
        "failed selected commit retains an actionable error"
    );
    std::fs::rename(&held, &object).unwrap();
    let commit_files = service.fetch_commit_diff(path, &newest, true).unwrap();
    assert!(
        commit_files[0].filename == "second.txt" && commit_files[0].hunks.is_empty(),
        "selected commit loads metadata only"
    );
    let older = service
        .fetch_commit_diff(path, &branch.commits[1].sha, false)
        .unwrap();
    assert_eq!(
        older[0].filename, "first.txt",
        "each commit selection loads its own patch"
    );

    write(
        PR_PATCH.replace("remote first", "remote refreshed"),
        "pr.patch",
        dir,
    );
    let refreshed = service.fetch_pr_diff_checked(path, 1, false).unwrap();
    assert_eq!(
        refreshed.files[0].hunks[0].lines.last().unwrap().content,
        "remote refreshed",
        "explicit refresh updates selected PR"
    );
    write("fail", "fail", dir);
    let error = service.fetch_pr_diff_checked(path, 1, false).unwrap_err();
    assert!(
        error.to_string().contains("remote-unavailable"),
        "PR error retains evidence"
    );
    let list_error = service.fetch_pr_list_checked(path, None).unwrap_err();
    assert!(
        list_error.to_string().contains("remote-unavailable"),
        "PR list error is gh's message"
    );
    std::fs::remove_file(dir.join("fail")).unwrap();

    // PR patches have Git C-quoted paths, including binary files with no +++ header.
    write(QUOTED_PR_PATCH, "pr.patch", dir);
    let quoted = service.fetch_pr_diff_checked(path, 1, false).unwrap();
    assert!(
        quoted
            .files
            .iter()
            .any(|f| f.filename == "quoted \"name\"\tü.txt" && f.added == 1),
        "quoted PR filename"
    );
    assert!(
        quoted
            .files
            .iter()
            .any(|f| f.filename == "binary \"name\".bin" && f.is_binary),
        "quoted binary PR filename"
    );
    let remote_summary = service.fetch_pr_diff_checked(path, 1, true).unwrap();
    assert!(
        remote_summary.files.len() == 2 && remote_summary.files.iter().all(|f| f.hunks.is_empty()),
        "PR metadata excludes all patch bodies"
    );
    let remote_file = find(&remote_summary.files, "quoted \"name\"\tü.txt");
    let remote_page = service.fetch_review_page(remote_file, path, None).unwrap();
    assert_eq!(
        last_content(&remote_page),
        Some("quoted"),
        "disk-backed quoted PR path loads exact patch"
    );
    assert!(
        find(&remote_summary.files, "binary \"name\".bin").is_binary,
        "disk-backed PR binary metadata"
    );
    write("unexpected output", "pr.patch", dir);
    assert!(
        service.fetch_pr_diff_checked(path, 1, false).is_err(),
        "malformed PR patch is an error"
    );
    assert!(
        service.fetch_pr_diff_checked(path, 1, true).is_err(),
        "malformed PR patch summary is an error"
    );

    // An explicit base that fails is reported and never falls back; a valid one recovers.
    let failed = service.fetch_branch_review(path, Some("missing-base"), false);
    assert!(
        failed.error.is_some()
            && failed.comparison_base == "missing-base"
            && failed.files.is_empty(),
        "explicit base failure"
    );
    let recovered = service.fetch_branch_review(path, Some("main"), false);
    assert!(
        recovered.error.is_none() && recovered.files.len() == 2,
        "base error recovers"
    );
}

#[test]
fn gh_missing_or_signed_out_reports_the_macs_pr_errors() {
    let repo = repo();
    let path = repo.str();
    // No gh on PATH or in its install locations: `gh not found`, and gh is unavailable.
    let mut missing = GitService::new(None, Arc::new(LocalProjectHost));
    missing.gh_lookup = || None;
    assert_eq!(
        missing.fetch_pr_list_checked(&path, None).unwrap_err(),
        GitError::review("gh not found")
    );
    assert!(missing.fetch_pr_diff_checked(&path, 1, false).is_err());
    assert!(!missing.is_gh_available(&path));
    assert!(missing.fetch_pr_list(&path, None).is_empty());
    // Signed out: gh's own hint is the error the PR tab shows.
    let hint = "To get started with GitHub CLI, please run:  gh auth login\n";
    let host = Arc::new(FakeGhHost {
        failure: Some((hint.into(), 4)),
    });
    let mut signed_out = GitService::new(Some("gh".into()), host);
    assert_eq!(
        signed_out
            .fetch_pr_list_checked(&path, None)
            .unwrap_err()
            .to_string(),
        hint
    );
    assert!(!signed_out.is_gh_available(&path));
    // gh that fails silently still gets an actionable message.
    let host = Arc::new(FakeGhHost {
        failure: Some((String::new(), 1)),
    });
    let mut silent = GitService::new(Some("gh".into()), host);
    assert_eq!(
        silent
            .fetch_pr_list_checked(&path, None)
            .unwrap_err()
            .to_string(),
        "GitHub PR lookup failed"
    );
    assert_eq!(
        silent
            .fetch_pr_diff_checked(&path, 1, false)
            .unwrap_err()
            .to_string(),
        "GitHub PR diff failed"
    );
}

#[test]
fn empty_results_stay_visible_during_refresh() {
    let repo = repo();
    let (dir, path) = (repo.path(), repo.str());
    let path = path.as_str();
    write("root\n", "root.txt", dir);
    commit("root", dir);
    let mut service = GitService::default();
    for _ in 0..3 {
        // A project with no recorded base compares with main; a clean project stays empty.
        let branch = service.fetch_branch_review(path, None, true);
        let local = service.fetch_local_review(path, true);
        let summary = service.review_summary(path, None);
        assert!(
            branch.error.is_none() && branch.files.is_empty() && branch.commits.is_empty(),
            "clean project remains empty"
        );
        assert!(
            local.error.is_none()
                && local.unique_path_count() == 0
                && summary.local_file_count == 0,
            "clean working tree"
        );
    }
    write("edited\n", "root.txt", dir);
    assert_eq!(
        service.fetch_local_review(path, true).unstaged.len(),
        1,
        "quiet refresh still publishes file edits"
    );
    git(&["checkout", "-b", "feature"], dir);
    git(&["commit", "--allow-empty", "-m", "empty"], dir);
    let branch = service.fetch_branch_review(path, Some("main"), true);
    assert_eq!(
        branch.commits.len(),
        1,
        "worktree selection loads its commits"
    );
    let empty = service
        .fetch_commit_diff(path, &branch.commits[0].sha, true)
        .unwrap();
    assert!(
        empty.is_empty(),
        "empty selected commit is an empty patch, not an error"
    );
    let same_base = service.fetch_branch_review(path, Some("feature"), true);
    assert!(
        same_base.error.is_none() && same_base.commits.is_empty(),
        "new comparison replaces commit list"
    );
    let unavailable = "0".repeat(40);
    for _ in 0..2 {
        assert!(
            service.fetch_commit_diff(path, &unavailable, true).is_err(),
            "unavailable commit exposes fetch failure"
        );
    }
}

#[test]
fn bounded_review_pages() {
    let repo = repo();
    let (dir, path) = (repo.path(), repo.str());
    let path = path.as_str();
    write("original\n", "large.txt", dir);
    commit("root", dir);
    git(&["checkout", "-b", "feature"], dir);
    write(
        (0..1000)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
        "large.txt",
        dir,
    );
    commit("large change", dir);
    let mut service = GitService::default();
    let review = service.fetch_branch_review(path, Some("main"), true);
    assert!(
        review.error.is_none() && review.files.len() == 1,
        "large changes remain navigable"
    );
    let file = review.files[0].clone();
    assert!(
        file.hunks.is_empty() && file.added == 1000 && file.removed == 1,
        "overview loads counts without patch bodies"
    );
    assert!(file.should_collapse(), "large files start collapsed");
    let mut page = service.fetch_review_page(&file, path, None).unwrap();
    assert_eq!(
        page.hunks.iter().map(|h| h.lines.len()).sum::<usize>(),
        200,
        "expanded files have bounded text layout"
    );
    let mut lines: Vec<_> = page.hunks.iter().flat_map(|h| h.lines.clone()).collect();
    while page.has_more {
        page = service
            .fetch_review_page(&file, path, Some(&page.continuation()))
            .unwrap();
        lines.extend(page.hunks.iter().flat_map(|h| h.lines.clone()));
    }
    let last = lines.last().unwrap();
    assert!(
        lines.len() == 1001 && last.content == "line 999" && last.new_line_no == Some(1000),
        "all lower lines remain reachable without gaps"
    );
    write("local\n", "large.txt", dir);
    write("new\n", "new.txt", dir);
    let local = service.fetch_local_review(path, true);
    assert!(
        local.unstaged[0].hunks.is_empty() && local.untracked[0].hunks.is_empty(),
        "local overview defers all bodies"
    );
    let new_page = service
        .fetch_review_page(&local.untracked[0], path, None)
        .unwrap();
    assert_eq!(
        new_page.hunks[0].lines[0].content, "new",
        "untracked content remains reachable"
    );
    let selected_sha = git(&["rev-parse", "HEAD"], dir);
    let commit_files = service
        .fetch_commit_diff(path, &selected_sha, true)
        .unwrap();
    assert!(
        commit_files[0].hunks.is_empty() && commit_files[0].added == 1000,
        "commit summaries defer bodies"
    );
    let initial_sha = git(&["rev-parse", "main"], dir);
    let initial_files = service.fetch_commit_diff(path, &initial_sha, true).unwrap();
    let initial_page = service
        .fetch_review_page(&initial_files[0], path, None)
        .unwrap();
    assert_eq!(
        initial_page.hunks[0].lines[0].content, "original",
        "initial commit uses empty baseline"
    );
    write("x".repeat(1_100_000) + "\n", "minified.txt", dir);
    let long_summary = service.fetch_local_review(path, true);
    let long_page = service
        .fetch_review_page(find(&long_summary.untracked, "minified.txt"), path, None)
        .unwrap();
    assert!(
        long_page.has_long_lines && long_page.hunks[0].lines[0].content.len() < 2200,
        "minified lines cannot trigger unbounded layout"
    );
    let raw = std::fs::read_to_string(&long_page.slice.snapshot).unwrap();
    assert!(
        raw.contains(&"x".repeat(1_100_000)),
        "full patch retains shortened line content"
    );
    for index in 0..1000 {
        write("new file\n", &format!("file{index:04}.txt"), dir);
    }
    commit("many files", dir);
    let started = Instant::now();
    let many = service.fetch_branch_review(path, Some("main"), true);
    assert!(
        many.error.is_none()
            && many.files.len() == 1003
            && many.files.iter().all(|f| f.hunks.is_empty()),
        "complete navigation survives thousands of changed files: {}",
        many.files.len()
    );
    let last_page = service
        .fetch_review_page(find(&many.files, "file0999.txt"), path, None)
        .unwrap();
    assert_eq!(
        last_content(&last_page),
        Some("new file"),
        "direct access to lower files bypasses other patch bodies"
    );
    println!(
        "1,003-file metadata and direct-jump check: {} ms",
        started.elapsed().as_millis()
    );
    // The overview defaults to metadata-only loading (DiffState's summariesOnly).
    assert!(
        service
            .fetch_branch_review(path, None, true)
            .files
            .iter()
            .all(|f| f.hunks.is_empty())
    );
}

/// `ReviewPageHistory` from `DiffModel.swift`: keeps one body and rereads visited pages from
/// their disk checkpoints.
#[derive(Default)]
struct ReviewPageHistory {
    checkpoints: Vec<Option<ReviewPageCursor>>,
    current: Option<(usize, ReviewPage)>,
}

impl ReviewPageHistory {
    fn new() -> Self {
        Self {
            checkpoints: vec![None],
            current: None,
        }
    }
    fn retained_line_count(&self) -> usize {
        self.current.as_ref().map_or(0, |(_, page)| {
            page.hunks.iter().map(|h| h.lines.len()).sum()
        })
    }
    fn page(&self, index: usize) -> Option<&ReviewPage> {
        self.current
            .as_ref()
            .filter(|(at, _)| *at == index)
            .map(|(_, page)| page)
    }
    fn cursor(&self, index: usize) -> Option<ReviewPageCursor> {
        self.checkpoints
            .get(index)
            .expect("Load the preceding page first.")
            .clone()
    }
    fn store(&mut self, page: ReviewPage, index: usize) {
        if index == 0 {
            self.checkpoints[0] = Some(ReviewPageCursor {
                slice: page.slice.clone(),
                next_offset: page.slice.start,
                old_line: 0,
                new_line: 0,
                header: String::new(),
                is_binary: page.is_binary,
            });
        }
        if self.checkpoints.len() == index + 1 {
            self.checkpoints.push(Some(page.continuation()));
        } else if index + 1 < self.checkpoints.len() {
            self.checkpoints[index + 1] = Some(page.continuation());
        }
        self.current = Some((index, page));
    }
    fn release_body(&mut self) {
        self.current = None;
    }
}

#[test]
fn review_regressions() {
    let repo = repo();
    let (dir, path) = (repo.path(), repo.str());
    let path = path.as_str();
    write(
        "fake-gh\nprs.json\npr.patch\nfail-diff\n",
        ".git/info/exclude",
        dir,
    );
    let mut service = GitService::default();
    write("first\n", "baseline.txt", dir);
    commit("first", dir);
    let first = git(&["rev-parse", "HEAD"], dir);
    write("second\n", "baseline.txt", dir);
    commit("second", dir);
    write("third\n", "baseline.txt", dir);
    git(&["add", "baseline.txt"], dir);
    let before = service.fetch_local_review(path, true);
    let old_page = service
        .fetch_review_page(&before.staged[0], path, None)
        .unwrap();
    git(&["reset", "--soft", &first], dir);
    let after = service.fetch_local_review(path, true);
    assert_ne!(
        before.staged[0].preview_version, after.staged[0].preview_version,
        "soft reset invalidates staged preview independently of index/work file"
    );
    let new_page = service
        .fetch_review_page(&after.staged[0], path, None)
        .unwrap();
    assert!(
        old_page.hunks[0]
            .lines
            .iter()
            .any(|l| l.content == "second" && l.line_type == LineType::Deletion),
        "old staged snapshot baseline"
    );
    assert!(
        new_page.hunks[0]
            .lines
            .iter()
            .any(|l| l.content == "first" && l.line_type == LineType::Deletion),
        "fresh staged snapshot follows moved HEAD"
    );

    let staged_version = after.staged[0].preview_version.clone();
    write("working only\n", "baseline.txt", dir);
    let working_edit = service.fetch_local_review(path, true);
    assert_eq!(
        working_edit.staged[0].preview_version, staged_version,
        "working edits do not reset staged preview pages"
    );
    let unstaged_version = working_edit.unstaged[0].preview_version.clone();
    write("unrelated\n", "other-index.txt", dir);
    git(&["add", "other-index.txt"], dir);
    git(&["status", "--porcelain"], dir);
    let unrelated_index = service.fetch_local_review(path, true);
    assert_eq!(
        find(&unrelated_index.staged, "baseline.txt").preview_version,
        staged_version,
        "unrelated index changes do not reset staged preview pages"
    );
    assert_eq!(
        unrelated_index.unstaged[0].preview_version, unstaged_version,
        "unrelated index changes do not reset unstaged preview pages"
    );
    git(&["add", "baseline.txt"], dir);
    let restaged = service.fetch_local_review(path, true);
    assert_ne!(
        find(&restaged.staged, "baseline.txt").preview_version,
        staged_version,
        "changing this path's index blob invalidates its staged preview"
    );

    write(
        (1..=20_000)
            .map(|i| format!("history line {i}"))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
        "many.txt",
        dir,
    );
    let summary = service.fetch_local_review(path, true);
    let file = find(&summary.untracked, "many.txt").clone();
    let mut history = ReviewPageHistory::new();
    let mut index = 0;
    loop {
        let page = service
            .fetch_review_page(&file, path, history.cursor(index).as_ref())
            .unwrap();
        let has_more = page.has_more;
        history.store(page, index);
        assert!(
            history.retained_line_count() <= 200,
            "visited pages retain only one bounded body"
        );
        if !has_more {
            break;
        }
        index += 1;
    }
    let last_line = history
        .page(index)
        .and_then(|p| p.hunks.last())
        .and_then(|h| h.lines.last())
        .and_then(|l| l.new_line_no);
    assert!(
        index >= 99 && last_line == Some(20_000),
        "all pages remain reachable with bounded retention"
    );
    history.release_body();
    assert_eq!(
        history.retained_line_count(),
        0,
        "collapsing/offscreen cards release page bodies"
    );
    std::fs::remove_file(dir.join("many.txt")).unwrap();
    let revisited = service
        .fetch_review_page(&file, path, history.cursor(0).as_ref())
        .unwrap();
    assert_eq!(
        revisited.hunks[0].lines[0].content, "history line 1",
        "previous pages reread the existing snapshot without regenerating Git"
    );

    // PR downloads that fail are errors; the next poll succeeds once gh recovers.
    let list = r#"[{"number":1,"title":"One","url":"https://example.test/1","state":"OPEN","headRefName":"main","baseRefName":"main","author":{"login":"test"},"labels":[],"additions":1,"deletions":0,"changedFiles":1,"isDraft":false,"createdAt":"today","updatedAt":"v1"}]"#;
    let patch = "diff --git a/remote.txt b/remote.txt\n--- a/remote.txt\n+++ b/remote.txt\n@@ -0,0 +1 @@\n+first remote\n";
    write(list, "prs.json", dir);
    write(patch, "pr.patch", dir);
    let mut prs = fake_gh_service();
    let initial = prs.fetch_pr_diff_checked(path, 1, true).unwrap();
    assert_eq!(
        prs.fetch_pr_list_checked(path, None).unwrap()[0].updated_at,
        "v1",
        "initial PR snapshot is current"
    );
    write(list.replace("v1", "v2"), "prs.json", dir);
    write(
        patch.replace("first remote", "updated remote"),
        "pr.patch",
        dir,
    );
    assert_eq!(
        prs.fetch_pr_list_checked(path, None).unwrap()[0].updated_at,
        "v2",
        "updated PR revision is listed"
    );
    let stale = prs
        .fetch_review_page(&initial.files[0], path, None)
        .unwrap();
    assert_eq!(
        last_content(&stale),
        Some("first remote"),
        "a loaded PR snapshot keeps its evidence"
    );
    let replaced = prs.fetch_pr_diff_checked(path, 1, true).unwrap();
    let refreshed = prs
        .fetch_review_page(&replaced.files[0], path, None)
        .unwrap();
    assert_eq!(
        last_content(&refreshed),
        Some("updated remote"),
        "reentry replaces stale PR patch content"
    );
    write("fail", "fail-diff", dir);
    assert!(
        prs.fetch_pr_diff_checked(path, 1, true)
            .unwrap_err()
            .to_string()
            .contains("temporary-diff-error"),
        "failed selected PR has an actionable load error"
    );
    std::fs::remove_file(dir.join("fail-diff")).unwrap();
    assert!(
        prs.fetch_pr_diff_checked(path, 1, true).is_ok(),
        "ordinary PR polling retries an unloaded selected PR"
    );

    let rename = super::repo();
    let (rdir, rpath) = (rename.path(), rename.str());
    let rpath = rpath.as_str();
    write("original\n", "old.txt", rdir);
    commit("root", rdir);
    git(&["checkout", "-b", "renamed"], rdir);
    git(&["mv", "old.txt", "new.txt"], rdir);
    write("replacement\n", "old.txt/child.txt", rdir);
    git(&["add", "--all"], rdir);
    let staged = service.fetch_local_review(rpath, true);
    let staged_rename = find(&staged.staged, "new.txt").clone();
    let staged_page = service
        .fetch_review_page(&staged_rename, rpath, None)
        .unwrap();
    assert!(
        staged_rename.old_filename.as_deref() == Some("old.txt") && staged_page.hunks.is_empty(),
        "staged rename cannot render a child file's patch"
    );
    commit("rename with directory replacement", rdir);
    let branch = service.fetch_branch_review(rpath, Some("main"), true);
    let branch_page = service
        .fetch_review_page(find(&branch.files, "new.txt"), rpath, None)
        .unwrap();
    assert!(
        branch_page.hunks.is_empty(),
        "branch rename remains scoped to its exact entry"
    );
    let sha = git(&["rev-parse", "HEAD"], rdir);
    let commit_files = service.fetch_commit_diff(rpath, &sha, true).unwrap();
    let commit_page = service
        .fetch_review_page(find(&commit_files, "new.txt"), rpath, None)
        .unwrap();
    assert!(
        commit_page.hunks.is_empty(),
        "commit rename remains scoped to its exact entry"
    );
}

#[test]
fn review_pages_only_read_snapshots_the_service_created() {
    let repo = repo();
    write("a\n", "a.txt", repo.path());
    let mut service = GitService::default();
    let mut file = FileDiff::new("a.txt", "M", 1, 0, Vec::new());
    file.patch_slice = Some(super::super::model::PatchSlice {
        snapshot: repo.file("a.txt").to_string_lossy().into_owned(),
        start: 0,
        end: 2,
    });
    assert_eq!(
        service
            .fetch_review_page(&file, &repo.str(), None)
            .unwrap_err(),
        GitError::review("Refresh to load this file's diff.")
    );
    let nested = FileDiff::new("nested/", "??", 0, 0, Vec::new());
    assert_eq!(
        service
            .fetch_review_page(&nested, &repo.str(), None)
            .unwrap_err()
            .to_string(),
        "This is an untracked nested repository; it has no diff here."
    );
}

#[test]
fn prefix_regressions() {
    let repo = repo();
    let (dir, path) = (repo.path(), repo.str());
    let path = path.as_str();
    let mut service = GitService::default();
    write("original a\n", "a/plain.txt", dir);
    write("original b\n", "b/plain.txt", dir);
    write("mode only\n", "b/mode.txt", dir);
    commit("root", dir);
    git(&["checkout", "-b", "prefixes"], dir);
    git(&["config", "core.filemode", "true"], dir);
    git(&["config", "diff.mnemonicPrefix", "true"], dir);
    write("staged a\n", "a/plain.txt", dir);
    git(&["add", "a/plain.txt"], dir);
    write("working b\n", "b/plain.txt", dir);
    let mnemonic = service.fetch_local_review(path, true);
    let staged = service
        .fetch_review_page(find(&mnemonic.staged, "a/plain.txt"), path, None)
        .unwrap();
    let unstaged = service
        .fetch_review_page(find(&mnemonic.unstaged, "b/plain.txt"), path, None)
        .unwrap();
    assert!(
        staged.hunks[0]
            .lines
            .iter()
            .any(|l| l.content == "staged a" && l.line_type == LineType::Addition),
        "mnemonic prefixes preserve exact staged file identity in an a directory"
    );
    assert!(
        unstaged.hunks[0]
            .lines
            .iter()
            .any(|l| l.content == "working b" && l.line_type == LineType::Addition),
        "mnemonic prefixes preserve exact unstaged file identity in a b directory"
    );
    git(&["config", "diff.noprefix", "true"], dir);
    write([0u8, 1, 255, 0], "a/new.bin", dir);
    // NTFS has no executable bit. Mark the index entry executable instead: the working file
    // (100644) then differs from the index in mode only, as the Mac's chmod 755 makes it.
    git(&["update-index", "--chmod=+x", "b/mode.txt"], dir);
    let no_prefix = service.fetch_local_review(path, true);
    let no_prefix_staged = service
        .fetch_review_page(find(&no_prefix.staged, "a/plain.txt"), path, None)
        .unwrap();
    assert!(
        page_has(&no_prefix_staged, "staged a"),
        "no-prefix config cannot strip the real a directory"
    );
    let binary = service
        .fetch_review_page(find(&no_prefix.untracked, "a/new.bin"), path, None)
        .unwrap();
    assert!(
        binary.is_binary && binary.hunks.is_empty(),
        "no-prefix binary untracked files remain previewable"
    );
    let mode = service
        .fetch_review_page(find(&no_prefix.unstaged, "b/mode.txt"), path, None)
        .unwrap();
    assert!(
        mode.hunks.is_empty(),
        "no-prefix mode-only file keeps exact b-directory identity"
    );
    git(&["add", "--all"], dir);
    git(&["update-index", "--chmod=+x", "b/mode.txt"], dir);
    git(&["commit", "-m", "prefix changes"], dir);
    let branch = service.fetch_branch_review(path, Some("main"), true);
    let branch_mode = service
        .fetch_review_page(find(&branch.files, "b/mode.txt"), path, None)
        .unwrap();
    assert!(
        branch_mode.hunks.is_empty(),
        "branch mode-only previews normalize configured prefixes"
    );
    let head = git(&["rev-parse", "HEAD"], dir);
    let commit_files = service.fetch_commit_diff(path, &head, true).unwrap();
    let commit_binary = service
        .fetch_review_page(find(&commit_files, "a/new.bin"), path, None)
        .unwrap();
    assert!(
        commit_binary.is_binary,
        "commit binary previews normalize configured prefixes"
    );
}

#[test]
fn raw_conflict_paths() {
    let repo = repo();
    let (dir, path) = (repo.path(), repo.str());
    let path = path.as_str();
    // The Swift names are `"quoted"` and `line<LF>break.txt`; NTFS holds neither. These are
    // C-quoted in patch headers but raw in `* Unmerged path` markers, the case under test.
    let names = ["'quoted' ü.txt", "line 雪 break.txt"];
    for name in names {
        write("base\n", name, dir);
    }
    commit("base", dir);
    git(&["checkout", "-b", "side"], dir);
    for name in names {
        write("side\n", name, dir);
    }
    commit("side", dir);
    git(&["checkout", "main"], dir);
    for name in names {
        write("ours\n", name, dir);
    }
    commit("ours", dir);
    conflicting_merge("side", dir);
    let mut service = GitService::default();
    let local = service.fetch_local_review(path, true);
    assert!(
        local.error.is_none(),
        "conflict metadata preserves raw quoted paths: {:?}",
        local.error
    );
    for name in names {
        let staged = service
            .fetch_review_page(find(&local.staged, name), path, None)
            .unwrap();
        assert!(
            staged.hunks.is_empty() && !staged.has_more,
            "raw unusual-path staged marker is a valid conflict page"
        );
        let working = service
            .fetch_review_page(find(&local.unstaged, name), path, None)
            .unwrap();
        assert!(
            page_has(&working, "<<<<<<< HEAD"),
            "raw unusual-path marker preserves actual unstaged conflict evidence"
        );
    }
}

#[test]
fn concurrent_git_pipes_complete() {
    let cwd = TempDir::new("purepoint-pipes");
    let workers: Vec<_> = (0..32)
        .map(|_| {
            let cwd = cwd.str();
            std::thread::spawn(move || {
                let service = GitService::default();
                // Both pipes exceed their buffers and must drain while many run at once.
                let result = service.run_git(
                    &[
                        "-c",
                        "alias.pipe-check=!i=0; while [ $i -lt 10000 ]; do printf 'stdout line\\n'; printf 'stderr line\\n' >&2; i=$((i+1)); done",
                        "pipe-check",
                    ],
                    &cwd,
                    None,
                );
                (result.success(), result.stdout.split('\n').count(), result.stderr.split('\n').count())
            })
        })
        .collect();
    for worker in workers {
        let (success, stdout_lines, stderr_lines) = worker.join().unwrap();
        assert!(success, "concurrent Git process completed");
        assert_eq!(stdout_lines, 10001, "all stdout drained");
        assert_eq!(stderr_lines, 10001, "all stderr drained");
    }

    // Cancel once the child is known to be running; it must end long before its 60 s sleep.
    let markers = TempDir::new("purepoint-cancel");
    let token = CancelToken::new();
    let mut service = GitService::default();
    service.set_cancel_token(token.clone());
    let cwd = markers.str();
    let started = Instant::now();
    let cancelled = std::thread::spawn(move || {
        service.run_git(
            &[
                "-c",
                "alias.cancel-check=!touch started && exec sleep 60",
                "cancel-check",
            ],
            &cwd,
            None,
        )
    });
    let marker = markers.file("started");
    while !marker.exists() && started.elapsed() < Duration::from_secs(30) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(marker.exists(), "cancellation check child started");
    token.cancel();
    let result = cancelled.join().unwrap();
    assert!(
        !result.success(),
        "canceled Git work terminates instead of completing obsolete reads"
    );
    assert!(
        started.elapsed() < Duration::from_secs(45),
        "canceled Git work ends promptly"
    );
}
