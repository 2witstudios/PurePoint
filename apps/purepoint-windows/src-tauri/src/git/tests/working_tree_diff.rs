//! Port of `WorkingTreeDiffTests.swift`; test names are the Swift names in snake_case.

use super::super::model::{GitError, LineType};
use super::super::review::{PREVIEW_LIMIT, read_local_preview};
use super::super::service::GitService;
use super::*;

fn repository() -> TempDir {
    let root = TempDir::new("WorkingTreeDiffTests");
    git(&["init", "-q"], root.path());
    git(&["config", "user.email", "test@example.com"], root.path());
    git(&["config", "user.name", "Test"], root.path());
    git(&["config", "core.autocrlf", "false"], root.path());
    root
}

#[test]
fn given_staged_and_unstaged_edits_should_review_the_whole_working_tree() {
    let root = repository();
    let service = GitService::default();
    write(
        "let first = 1\nlet second = 2\n",
        "source.swift",
        root.path(),
    );
    git(&["add", "."], root.path());
    git(&["commit", "-qm", "initial"], root.path());
    write(
        "let first = 3\nlet second = 2\n",
        "source.swift",
        root.path(),
    );
    git(&["add", "."], root.path());
    write(
        "let first = 3\nlet second = 4\n",
        "source.swift",
        root.path(),
    );

    let files = service.fetch_working_tree_changes(&root.str()).unwrap();
    assert_eq!(
        files
            .iter()
            .map(|f| f.filename.as_str())
            .collect::<Vec<_>>(),
        ["source.swift"]
    );
    let diff = service
        .fetch_working_tree_file_diff(&root.str(), &files[0])
        .unwrap();
    let added: Vec<&str> = diff
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| l.line_type == LineType::Addition)
        .map(|l| l.content.as_str())
        .collect();
    assert_eq!(added, ["let first = 3", "let second = 4"]);
    assert_eq!((diff.added, diff.removed), (2, 2));
}

#[test]
fn given_untracked_file_with_special_characters_should_expand_its_actual_contents() {
    let root = repository();
    let service = GitService::default();
    // Swift: `new file\t"雪".swift` (NTFS holds no tab or double quote).
    let name = "new file '雪' [draft].swift";
    write("let value = 1\n", name, root.path());
    let files = service.fetch_working_tree_changes(&root.str()).unwrap();
    assert_eq!(
        files
            .iter()
            .map(|f| f.filename.as_str())
            .collect::<Vec<_>>(),
        [name]
    );
    let diff = service
        .fetch_working_tree_file_diff(&root.str(), &files[0])
        .unwrap();
    let contents: Vec<&str> = diff
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .map(|l| l.content.as_str())
        .collect();
    assert_eq!(contents, ["let value = 1"]);
    assert_eq!(diff.added, 1);
}

#[test]
fn given_staged_file_in_a_new_repository_should_show_an_addition() {
    let root = repository();
    let service = GitService::default();
    write("hello\n", "first.txt", root.path());
    git(&["add", "."], root.path());
    let files = service.fetch_working_tree_changes(&root.str()).unwrap();
    let diff = service
        .fetch_working_tree_file_diff(&root.str(), &files[0])
        .unwrap();
    assert_eq!(diff.status_code, "A");
    assert_eq!(
        diff.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .next()
            .map(|l| l.line_type),
        Some(LineType::Addition)
    );
}

#[test]
fn given_tracked_filename_with_spaces_and_newline_should_keep_the_path_and_deleted_lines() {
    let root = repository();
    let service = GitService::default();
    // Swift: `file with\na newline.txt` (NTFS holds no newline).
    let name = "file with spaces, ü and 雪.txt";
    write("before\n", name, root.path());
    git(&["add", "."], root.path());
    git(&["commit", "-qm", "initial"], root.path());
    std::fs::remove_file(root.file(name)).unwrap();
    let files = service.fetch_working_tree_changes(&root.str()).unwrap();
    assert_eq!(
        files
            .iter()
            .map(|f| f.filename.as_str())
            .collect::<Vec<_>>(),
        [name]
    );
    let diff = service
        .fetch_working_tree_file_diff(&root.str(), &files[0])
        .unwrap();
    assert_eq!(diff.status_code, "D");
    assert_eq!(
        diff.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .next()
            .map(|l| l.content.as_str()),
        Some("before")
    );
    assert_eq!(diff.removed, 1);
}

#[test]
fn given_untracked_binary_file_should_not_render_binary_as_text() {
    let root = repository();
    let service = GitService::default();
    write([0u8, 1, 2, 3], "image.bin", root.path());
    let files = service.fetch_working_tree_changes(&root.str()).unwrap();
    let diff = service
        .fetch_working_tree_file_diff(&root.str(), &files[0])
        .unwrap();
    assert!(diff.hunks.is_empty());
}

#[test]
fn given_filename_containing_pathspec_characters_should_only_diff_that_file() {
    let root = repository();
    let service = GitService::default();
    let name = "[draft].txt";
    write("before\n", name, root.path());
    write("unchanged\n", "d.txt", root.path());
    git(&["add", "."], root.path());
    git(&["commit", "-qm", "initial"], root.path());
    write("diff --git is part of this file\n", name, root.path());
    let files = service.fetch_working_tree_changes(&root.str()).unwrap();
    let diff = service
        .fetch_working_tree_file_diff(&root.str(), &files[0])
        .unwrap();
    assert_eq!(diff.filename, name);
    assert_eq!(
        diff.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .last()
            .map(|l| l.content.as_str()),
        Some("diff --git is part of this file")
    );
    assert_eq!((diff.added, diff.removed), (1, 1));
}

#[test]
fn given_oversized_file_should_decline_inline_preview() {
    let root = repository();
    write(vec![65u8; 1_000_001], "large.txt", root.path());
    // Expected: do not allocate a huge inline text layout.
    assert_eq!(
        read_local_preview(&root.file("large.txt"), PREVIEW_LIMIT),
        Err(GitError::TooLarge)
    );
}

#[test]
fn given_large_tracked_diff_should_bound_capture_and_reject_preview() {
    let root = repository();
    let service = GitService::default();
    write("before\n", "large.txt", root.path());
    git(&["add", "."], root.path());
    git(&["commit", "-qm", "initial"], root.path());
    write(
        "a line of changed text\n".repeat(100_000),
        "large.txt",
        root.path(),
    );

    let result = service.run_git(
        &["diff", "HEAD", "--", "large.txt"],
        &root.str(),
        Some(1_000_000),
    );
    assert!(result.output_exceeded_limit);
    assert!(result.stdout.is_empty());
    let files = service.fetch_working_tree_changes(&root.str()).unwrap();
    assert_eq!(
        service.fetch_working_tree_file_diff(&root.str(), &files[0]),
        Err(GitError::TooLarge)
    );

    // A later smaller diff must still render after the oversized process exits.
    write("after\n", "large.txt", root.path());
    let diff = service
        .fetch_working_tree_file_diff(&root.str(), &files[0])
        .unwrap();
    assert_eq!(
        diff.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .last()
            .map(|l| l.content.as_str()),
        Some("after")
    );
}

#[test]
fn given_cached_removal_with_disk_copy_should_return_one_deletion_per_path() {
    let root = repository();
    let service = GitService::default();
    write("before\n", "a.txt", root.path());
    git(&["add", "."], root.path());
    git(&["commit", "-qm", "initial"], root.path());
    git(&["rm", "--cached", "a.txt"], root.path());
    write("untracked copy\n", "a.txt", root.path());

    let files = service.fetch_working_tree_changes(&root.str()).unwrap();
    assert_eq!(
        files
            .iter()
            .map(|f| f.filename.as_str())
            .collect::<Vec<_>>(),
        ["a.txt"]
    );
    let file = &files[0];
    assert_eq!(file.status_code, "D");
    assert_eq!((file.added, file.removed), (0, 1));
    let diff = service
        .fetch_working_tree_file_diff(&root.str(), file)
        .unwrap();
    let lines: Vec<_> = diff.hunks.iter().flat_map(|h| &h.lines).collect();
    assert_eq!(
        lines.iter().map(|l| l.content.as_str()).collect::<Vec<_>>(),
        ["before"]
    );
    assert!(lines.iter().all(|l| l.line_type == LineType::Deletion));
    let preview = read_local_preview(&root.file("a.txt"), PREVIEW_LIMIT).unwrap();
    assert_eq!(preview.0, "untracked copy\n");
}

#[test]
fn given_staged_path_deleted_or_renamed_before_first_commit_should_only_show_existing_files() {
    let root = repository();
    let service = GitService::default();
    write("deleted\n", "deleted.txt", root.path());
    write("renamed\n", "old.txt", root.path());
    git(&["add", "."], root.path());
    std::fs::remove_file(root.file("deleted.txt")).unwrap();
    std::fs::rename(root.file("old.txt"), root.file("new.txt")).unwrap();
    let files = service.fetch_working_tree_changes(&root.str()).unwrap();
    assert_eq!(
        files
            .iter()
            .map(|f| f.filename.as_str())
            .collect::<Vec<_>>(),
        ["new.txt"]
    );
    let diff = service
        .fetch_working_tree_file_diff(&root.str(), &files[0])
        .unwrap();
    let contents: Vec<&str> = diff
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .map(|l| l.content.as_str())
        .collect();
    assert_eq!(contents, ["renamed"]);
    assert_eq!(diff.added, 1);
}

#[test]
fn working_tree_rows_sort_like_finder() {
    use super::super::review::standard_compare;
    let mut names = vec!["file10.txt", "File2.txt", "file1.txt", "a.txt"];
    names.sort_by(|a, b| standard_compare(a, b));
    assert_eq!(names, ["a.txt", "file1.txt", "File2.txt", "file10.txt"]);
}
