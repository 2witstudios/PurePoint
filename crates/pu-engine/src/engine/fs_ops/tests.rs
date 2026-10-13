use super::*;
use tempfile::TempDir;

/// An engine with one registered (initialized, git-backed) project, plus a
/// sibling directory outside every allowed root.
async fn setup() -> (Engine, TempDir, PathBuf, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let root = base.join("project");
    let outside = base.join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "secret").unwrap();
    git(&root, &["init", "-q"]);
    let engine = Engine::new();
    engine
        .login_env
        .set(vec![(
            "PATH".into(),
            std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()),
        )])
        .unwrap();
    let init = engine
        .handle_request(Request::Init {
            project_root: path_string(&root),
        })
        .await;
    assert!(matches!(init, Response::InitResult { .. }), "{init:?}");
    (engine, tmp, root, outside)
}

fn git(dir: &Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn p(path: &Path) -> String {
    path_string(path)
}

fn assert_error(response: Response, expected: &str) {
    match response {
        Response::Error { code, .. } => assert_eq!(code, expected),
        other => panic!("expected {expected} error, got {other:?}"),
    }
}

fn b64(data: &[u8]) -> String {
    BASE64_STANDARD.encode(data)
}

// --- Path confinement ---

#[tokio::test(flavor = "current_thread")]
async fn given_dot_dot_escape_should_refuse_path() {
    let (engine, _tmp, root, _outside) = setup().await;
    let escape = format!("{}/../outside/secret.txt", p(&root));
    let resp = engine
        .handle_request(Request::FsRead {
            path: escape,
            max_bytes: 100,
        })
        .await;
    assert_error(resp, "path_not_allowed");
}

#[tokio::test(flavor = "current_thread")]
async fn given_symlink_escape_should_refuse_read_and_write() {
    let (engine, _tmp, root, outside) = setup().await;
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    let resp = engine
        .handle_request(Request::FsRead {
            path: p(&root.join("link/secret.txt")),
            max_bytes: 100,
        })
        .await;
    assert_error(resp, "path_not_allowed");

    let resp = engine
        .handle_request(Request::FsWrite {
            path: p(&root.join("link/new.txt")),
            data_base64: b64(b"x"),
            expected_modified_ms: None,
            create_dirs: false,
        })
        .await;
    assert_error(resp, "path_not_allowed");
    assert!(!outside.join("new.txt").exists());
}

#[tokio::test(flavor = "current_thread")]
async fn given_dangling_symlink_to_outside_should_refuse_write() {
    let (engine, _tmp, root, outside) = setup().await;
    let target = outside.join("created.txt");
    std::os::unix::fs::symlink(&target, root.join("dangling")).unwrap();
    let resp = engine
        .handle_request(Request::FsWrite {
            path: p(&root.join("dangling")),
            data_base64: b64(b"x"),
            expected_modified_ms: None,
            create_dirs: false,
        })
        .await;
    assert_error(resp, "path_not_allowed");
    assert!(!target.exists());
}

#[tokio::test(flavor = "current_thread")]
async fn given_unregistered_or_relative_path_should_refuse_it() {
    let (engine, _tmp, _root, outside) = setup().await;
    for path in [p(&outside), "project/file.txt".to_string()] {
        let resp = engine
            .handle_request(Request::FsList {
                path,
                recursive: false,
                include_hidden: true,
                max_entries: 10,
            })
            .await;
        assert_error(resp, "path_not_allowed");
    }
    let resp = engine
        .handle_request(Request::FsStat {
            path: p(&outside.join("secret.txt")),
        })
        .await;
    assert_error(resp, "path_not_allowed");
}

fn register_worktree(root: &Path, id: &str, path: &Path) {
    let mut m = manifest::read_manifest(root).unwrap();
    m.worktrees.insert(
        id.into(),
        pu_core::types::WorktreeEntry {
            id: id.into(),
            name: id.into(),
            path: p(path),
            branch: format!("pu/{id}"),
            base_branch: None,
            status: pu_core::types::WorktreeStatus::Active,
            agents: Default::default(),
            created_at: chrono::Utc::now(),
            merged_at: None,
            error: None,
        },
    );
    std::fs::write(
        pu_core::paths::manifest_path(root),
        serde_json::to_string(&m).unwrap(),
    )
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn given_manifest_worktree_outside_worktrees_dir_should_refuse_it() {
    let (engine, tmp, root, outside) = setup().await;
    let wt_dir = tmp.path().canonicalize().unwrap().join("elsewhere-wt");
    std::fs::create_dir_all(&wt_dir).unwrap();
    std::fs::write(wt_dir.join("a.txt"), "wt").unwrap();
    register_worktree(&root, "wt-x", &wt_dir);
    // A tampered manifest must not widen confinement to `/` or the home dir.
    register_worktree(&root, "wt-root", Path::new("/"));
    register_worktree(&root, "wt-parent", tmp.path());
    for path in [wt_dir.join("a.txt"), outside.join("secret.txt")] {
        let resp = engine
            .handle_request(Request::FsRead {
                path: p(&path),
                max_bytes: 100,
            })
            .await;
        assert_error(resp, "path_not_allowed");
    }
}

#[test]
fn given_worktree_candidates_should_trust_only_children_of_worktrees_dir() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let project = base.join("project");
    let worktrees = project.join(".pu/worktrees");
    let wt = worktrees.join("wt-a");
    let home = base.join("home");
    for dir in [&wt, &home] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let trust = |path: &Path| trusted_worktree(path, &worktrees, &project, Some(&home));
    assert_eq!(trust(&wt), Some(wt.clone()));
    assert_eq!(trust(&worktrees), None);
    assert_eq!(trust(&project), None);
    assert_eq!(trust(&home), None);
    assert_eq!(trust(Path::new("/")), None);
    assert_eq!(trust(&base), None);
    // `..` inside the recorded path resolves before the check.
    assert_eq!(trust(&wt.join("../../..")), None);
    // A worktrees dir that is itself `/` (symlinked) cannot vouch for ancestors.
    assert_eq!(
        trusted_worktree(&base, Path::new("/"), &project, Some(&home)),
        None
    );
}

#[tokio::test(flavor = "current_thread")]
async fn given_worktree_inside_worktrees_dir_should_allow_it() {
    let (engine, _tmp, root, _outside) = setup().await;
    let wt_dir = pu_core::paths::worktree_path(&root, "wt-y");
    std::fs::create_dir_all(&wt_dir).unwrap();
    std::fs::write(wt_dir.join("a.txt"), "wt").unwrap();
    register_worktree(&root, "wt-y", &wt_dir);
    let resp = engine
        .handle_request(Request::FsRead {
            path: p(&wt_dir.join("a.txt")),
            max_bytes: 100,
        })
        .await;
    assert!(matches!(resp, Response::FsReadResult { .. }), "{resp:?}");
}

// --- Stat / read ---

#[tokio::test(flavor = "current_thread")]
async fn given_missing_file_in_project_should_stat_as_not_existing() {
    let (engine, _tmp, root, _outside) = setup().await;
    let resp = engine
        .handle_request(Request::FsStat {
            path: p(&root.join("nope/missing.txt")),
        })
        .await;
    match resp {
        Response::FsStatResult { exists, is_dir, .. } => {
            assert!(!exists);
            assert!(!is_dir);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_file_over_max_bytes_should_return_truncated_prefix() {
    let (engine, _tmp, root, _outside) = setup().await;
    let file = root.join("big.txt");
    std::fs::write(&file, "0123456789").unwrap();
    let resp = engine
        .handle_request(Request::FsRead {
            path: p(&file),
            max_bytes: 4,
        })
        .await;
    match resp {
        Response::FsReadResult {
            path,
            data_base64,
            size,
            truncated,
            ..
        } => {
            assert_eq!(path, p(&file));
            assert_eq!(BASE64_STANDARD.decode(data_base64).unwrap(), b"0123");
            assert_eq!(size, 10);
            assert!(truncated);
        }
        other => panic!("unexpected {other:?}"),
    }
    let resp = engine
        .handle_request(Request::FsRead {
            path: p(&file),
            max_bytes: 10,
        })
        .await;
    assert!(matches!(
        resp,
        Response::FsReadResult {
            truncated: false,
            ..
        }
    ));
}

// --- Write ---

#[tokio::test(flavor = "current_thread")]
async fn given_huge_max_bytes_should_clamp_read_to_cap() {
    let (engine, _tmp, root, _outside) = setup().await;
    let file = root.join("big.bin");
    std::fs::write(&file, vec![b'x'; FS_READ_MAX_BYTES as usize + 1]).unwrap();
    let resp = engine
        .handle_request(Request::FsRead {
            path: p(&file),
            max_bytes: u64::MAX,
        })
        .await;
    match resp {
        Response::FsReadResult {
            data_base64,
            size,
            truncated,
            ..
        } => {
            assert_eq!(size, FS_READ_MAX_BYTES + 1);
            assert!(truncated);
            let data = BASE64_STANDARD.decode(data_base64).unwrap();
            assert_eq!(data.len() as u64, FS_READ_MAX_BYTES);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_stale_expected_modified_ms_should_reject_write_with_conflict() {
    let (engine, _tmp, root, _outside) = setup().await;
    let file = root.join("doc.txt");
    std::fs::write(&file, "v1").unwrap();
    let modified = match engine
        .handle_request(Request::FsStat { path: p(&file) })
        .await
    {
        Response::FsStatResult { modified_ms, .. } => modified_ms,
        other => panic!("unexpected {other:?}"),
    };

    let resp = engine
        .handle_request(Request::FsWrite {
            path: p(&file),
            data_base64: b64(b"v2"),
            expected_modified_ms: Some(modified - 5_000),
            create_dirs: false,
        })
        .await;
    assert_error(resp, "conflict");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "v1");

    let resp = engine
        .handle_request(Request::FsWrite {
            path: p(&file),
            data_base64: b64(b"v2"),
            expected_modified_ms: Some(modified),
            create_dirs: false,
        })
        .await;
    match resp {
        Response::FsWriteResult { size, .. } => assert_eq!(size, 2),
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "v2");
}

#[tokio::test(flavor = "current_thread")]
async fn given_missing_parent_should_create_dirs_only_when_asked() {
    let (engine, _tmp, root, _outside) = setup().await;
    let file = root.join("a/b/c.txt");
    let resp = engine
        .handle_request(Request::FsWrite {
            path: p(&file),
            data_base64: b64(b"hello"),
            expected_modified_ms: None,
            create_dirs: false,
        })
        .await;
    assert_error(resp, "not_found");

    let resp = engine
        .handle_request(Request::FsWrite {
            path: p(&file),
            data_base64: b64(b"hello"),
            expected_modified_ms: None,
            create_dirs: true,
        })
        .await;
    match resp {
        Response::FsWriteResult {
            path,
            size,
            modified_ms,
        } => {
            assert_eq!(path, p(&file));
            assert_eq!(size, 5);
            assert!(modified_ms > 0);
        }
        other => panic!("unexpected {other:?}"),
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello");
    // No temp files left behind.
    assert_eq!(std::fs::read_dir(root.join("a/b")).unwrap().count(), 1);
}

// --- List ---

#[tokio::test(flavor = "current_thread")]
async fn given_recursive_list_should_skip_git_contents_and_flag_ignored() {
    let (engine, _tmp, root, _outside) = setup().await;
    std::fs::write(root.join(".gitignore"), "build/\n*.log\n").unwrap();
    std::fs::create_dir_all(root.join("src/nested")).unwrap();
    std::fs::write(root.join("src/nested/lib.rs"), "fn main() {}").unwrap();
    std::fs::create_dir_all(root.join("build")).unwrap();
    std::fs::write(root.join("build/out.bin"), "x").unwrap();
    std::fs::write(root.join("debug.log"), "x").unwrap();

    let resp = engine
        .handle_request(Request::FsList {
            path: p(&root),
            recursive: true,
            include_hidden: true,
            max_entries: 20_000,
        })
        .await;
    let Response::FsListResult {
        entries, truncated, ..
    } = resp
    else {
        panic!("unexpected {resp:?}");
    };
    assert!(!truncated);
    let git_dir = p(&root.join(".git"));
    assert!(entries.iter().any(|e| e.path == git_dir && e.is_dir));
    assert!(
        !entries
            .iter()
            .any(|e| e.path.starts_with(&format!("{git_dir}/"))),
        "listed .git contents"
    );
    let find = |rel: &str| {
        let path = p(&root.join(rel));
        entries
            .iter()
            .find(|e| e.path == path)
            .unwrap_or_else(|| panic!("missing {rel}"))
            .clone()
    };
    assert!(find("src/nested/lib.rs").size > 0);
    assert!(!find("src/nested/lib.rs").ignored);
    assert!(find("build").ignored);
    assert!(find("debug.log").ignored);
    assert!(!find(".gitignore").ignored);

    let resp = engine
        .handle_request(Request::FsList {
            path: p(&root),
            recursive: false,
            include_hidden: false,
            max_entries: 20_000,
        })
        .await;
    let Response::FsListResult { entries, .. } = resp else {
        panic!("unexpected {resp:?}");
    };
    assert!(entries.iter().all(|e| !e.name.starts_with('.')));
    assert!(!entries.iter().any(|e| e.name == "lib.rs"));
}

#[tokio::test(flavor = "current_thread")]
async fn given_more_entries_than_max_should_truncate() {
    let (engine, _tmp, root, _outside) = setup().await;
    for i in 0..5 {
        std::fs::write(root.join(format!("f{i}.txt")), "x").unwrap();
    }
    let resp = engine
        .handle_request(Request::FsList {
            path: p(&root),
            recursive: true,
            include_hidden: false,
            max_entries: 3,
        })
        .await;
    match resp {
        Response::FsListResult {
            entries, truncated, ..
        } => {
            assert_eq!(entries.len(), 3);
            assert!(truncated);
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_directory_outside_any_repo_should_list_without_ignored_flags() {
    let (engine, _tmp, _root, _outside) = setup().await;
    let home = TempDir::new().unwrap();
    let home = home.path().canonicalize().unwrap();
    let transcripts = home.join(".claude/projects");
    std::fs::create_dir_all(&transcripts).unwrap();
    std::fs::write(transcripts.join("t.jsonl"), "{}").unwrap();
    let roots = allowed_roots(&engine.registered_projects(), Some(&home));
    match fs_list(&p(&transcripts), &roots.all, false, true, 10) {
        Response::FsListResult { entries, .. } => {
            assert_eq!(entries.len(), 1);
            assert!(!entries[0].ignored);
        }
        other => panic!("unexpected {other:?}"),
    }
    // Other directories under home stay out of reach.
    assert_error(
        fs_stat(&p(&home.join(".ssh/id_rsa")), &roots.all),
        "path_not_allowed",
    );
    // Transcript directories are read-only.
    let transcript = transcripts.join("t.jsonl");
    assert_error(
        fs_write(&p(&transcript), &roots.writable, &b64(b"x"), None, false),
        "path_not_allowed",
    );
    assert_error(fs_delete(&p(&transcript), &roots), "path_not_allowed");
    assert_eq!(std::fs::read_to_string(&transcript).unwrap(), "{}");
}

#[test]
fn given_ancestor_of_a_root_should_refuse_delete() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().canonicalize().unwrap().join("p");
    let worktree = project.join(".pu/worktrees/wt-1");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(project.join(".pu/manifest.json"), "{}").unwrap();
    let roots = FsRoots {
        writable: vec![worktree.clone(), project.clone()],
        all: vec![worktree.clone(), project.clone()],
    };
    for path in [
        project.join(".pu/worktrees"),
        project.join(".pu"),
        worktree.clone(),
    ] {
        assert_error(fs_delete(&p(&path), &roots), "path_not_allowed");
    }
    assert!(worktree.exists());
    let resp = fs_delete(&p(&project.join(".pu/manifest.json")), &roots);
    assert!(matches!(resp, Response::Ok), "{resp:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn given_fifo_should_refuse_read_without_blocking() {
    let (engine, _tmp, root, _outside) = setup().await;
    let fifo = root.join("pipe");
    let c_path = std::ffi::CString::new(p(&fifo)).unwrap();
    // SAFETY: plain mkfifo on a NUL-terminated path.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let resp = tokio::time::timeout(
        Duration::from_secs(5),
        engine.handle_request(Request::FsRead {
            path: p(&fifo),
            max_bytes: 10,
        }),
    )
    .await
    .expect("fs_read on a FIFO blocked");
    assert_error(resp, "not_a_file");
}

#[tokio::test(flavor = "current_thread")]
async fn given_rewrite_should_keep_file_mode() {
    let (engine, _tmp, root, _outside) = setup().await;
    for mode in [0o600, 0o664, 0o755] {
        let file = root.join(format!("f-{mode:o}"));
        std::fs::write(&file, "old").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
        let resp = engine
            .handle_request(Request::FsWrite {
                path: p(&file),
                data_base64: b64(b"new"),
                expected_modified_ms: None,
                create_dirs: false,
            })
            .await;
        assert!(matches!(resp, Response::FsWriteResult { .. }), "{resp:?}");
        let meta = std::fs::metadata(&file).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, mode);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "new");
    }
}

#[test]
fn given_root_home_or_home_ancestor_should_be_too_broad_to_register() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let home = base.join("users/me");
    let project = home.join("code/app");
    std::fs::create_dir_all(&project).unwrap();
    for broad in [Path::new("/"), &base, &base.join("users"), &home] {
        assert!(
            is_too_broad_project_root(broad, Some(&home)),
            "{broad:?} should be refused"
        );
    }
    assert!(!is_too_broad_project_root(&project, Some(&home)));
    assert!(!is_too_broad_project_root(&home.join("code"), Some(&home)));
}

#[tokio::test(flavor = "current_thread")]
async fn given_filesystem_root_should_refuse_init() {
    let engine = Engine::new();
    let resp = engine
        .handle_request(Request::Init {
            project_root: "/".into(),
        })
        .await;
    assert_error(resp, "project_root_not_allowed");
}

// --- Delete ---

#[tokio::test(flavor = "current_thread")]
async fn given_symlink_delete_should_remove_link_not_target_and_refuse_root() {
    let (engine, _tmp, root, outside) = setup().await;
    std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link")).unwrap();
    let resp = engine
        .handle_request(Request::FsDelete {
            path: p(&root.join("link")),
        })
        .await;
    assert!(matches!(resp, Response::Ok), "{resp:?}");
    assert!(!root.join("link").exists());
    assert!(outside.join("secret.txt").exists());

    let resp = engine
        .handle_request(Request::FsDelete { path: p(&root) })
        .await;
    assert_error(resp, "path_not_allowed");
    assert!(root.exists());

    std::fs::create_dir_all(root.join("dir/sub")).unwrap();
    let resp = engine
        .handle_request(Request::FsDelete {
            path: p(&root.join("dir")),
        })
        .await;
    assert!(matches!(resp, Response::Ok), "{resp:?}");
    assert!(!root.join("dir").exists());
}

// --- git_exec ---

#[test]
fn given_git_should_run_with_hooks_and_user_config_disabled() {
    let args = exec_args("git", strings(&["-c", "core.quotePath=true", "status"]));
    assert_eq!(
        args,
        strings(&[
            "-c",
            "core.fsmonitor=",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "safe.bareRepository=explicit",
            "-c",
            "core.quotePath=true",
            "status"
        ])
    );
    assert_eq!(
        exec_env("git"),
        vec![
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1")
        ]
    );
    assert_eq!(
        exec_args("gh", strings(&["auth", "status"])),
        strings(&["auth", "status"])
    );
    assert!(
        exec_env("gh")
            .iter()
            .any(|(k, v)| *k == "GIT_CONFIG_PARAMETERS"
                && v.contains("safe.bareRepository=explicit"))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn given_repo_fsmonitor_hook_should_not_run_under_git_exec() {
    let (engine, tmp, root, _outside) = setup().await;
    let marker = tmp.path().join("fsmonitor-ran");
    let hook = tmp.path().join("fsmonitor-hook");
    std::fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    git(&root, &["config", "core.fsmonitor", &p(&hook)]);
    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&root),
            program: "git".into(),
            args: strings(&["status", "--porcelain=v1"]),
            stdin_base64: None,
            timeout_ms: 10_000,
        })
        .await;
    assert!(
        matches!(resp, Response::GitExecResult { exit_code: 0, .. }),
        "{resp:?}"
    );
    assert!(!marker.exists(), "repository fsmonitor hook ran");
}

#[tokio::test(flavor = "current_thread")]
async fn given_program_other_than_git_or_gh_should_refuse_it() {
    let (engine, _tmp, root, _outside) = setup().await;
    for program in ["sh", "/usr/bin/git", "git ", "bash"] {
        let resp = engine
            .handle_request(Request::GitExec {
                cwd: p(&root),
                program: program.into(),
                args: vec!["-c".into(), "true".into()],
                stdin_base64: None,
                timeout_ms: 5_000,
            })
            .await;
        assert_error(resp, "program_not_allowed");
    }
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|a| a.to_string()).collect()
}

/// Every git/gh argument vector `GitService.swift` sends (after `runGit`
/// prepends `--literal-pathspecs`), with representative revisions and paths.
#[test]
fn given_app_git_and_gh_invocations_should_pass_arg_filter() {
    let lp = "--literal-pathspecs";
    let diff_common = [
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--find-renames",
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ];
    let mut git: Vec<Vec<&str>> = vec![
        vec![
            lp,
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ],
        vec![
            lp,
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        vec![lp, "symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
        vec![lp, "rev-parse", "--verify", "HEAD^{commit}"],
        vec![
            lp,
            "rev-parse",
            "--verify",
            "--end-of-options",
            "main^{commit}",
        ],
        vec![lp, "merge-base", "abc", "def"],
        vec![
            lp,
            "log",
            "--format=%H%x00%s%x00%an%x00%aI%x00",
            "abc..def",
            "--",
        ],
        vec![lp, "ls-files", "--others", "--exclude-standard", "-z"],
        vec![lp, "rev-list", "--count", "abc..def"],
        vec![
            lp,
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
        ],
        vec![lp, "rev-list", "--parents", "-n", "1", "abc"],
        vec![lp, "rev-parse", "--verify", "HEAD"],
        vec![lp, "ls-files", "--unmerged", "-z", "--", "src/a.rs"],
        vec![
            lp,
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ],
        vec![
            lp,
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--numstat",
            "-z",
            "--no-renames",
            "HEAD",
            "--",
        ],
        vec![
            lp,
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--numstat",
            "-z",
            "--no-renames",
            "--cached",
            "--",
        ],
        vec![
            lp,
            lp,
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--no-renames",
            "HEAD",
            "--",
            "src/a.rs",
        ],
        vec![
            lp,
            "diff",
            "--no-index",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "--",
            "/dev/null",
            "new dir/file.txt",
        ],
        vec![
            lp,
            "-c",
            "core.quotePath=true",
            "diff",
            "--no-index",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--",
            "/dev/null",
            "new.txt",
        ],
    ];
    for (command, revisions) in [
        (vec!["diff-tree", "--root", "--no-commit-id", "-r"], vec![]),
        (vec!["diff"], vec!["--ours"]),
        (vec!["diff"], vec!["--cached"]),
        (vec!["diff"], vec!["abc", "def"]),
    ] {
        for options in [
            vec!["--name-status", "-z"],
            vec!["--raw", "--no-abbrev", "--numstat", "-z"],
            vec!["--numstat", "-z"],
        ] {
            let mut args = vec![lp];
            args.extend(&command);
            args.extend(diff_common);
            args.extend(&options);
            args.extend(&revisions);
            args.push("--");
            git.push(args);
        }
        let mut patch = vec![lp];
        patch.extend(&command);
        patch.extend(diff_common);
        patch.push("--patch");
        patch.extend(&revisions);
        patch.extend(["--", "src/a.rs", "src/b.rs"]);
        git.push(patch);
        let mut quoted = vec![lp, "-c", "core.quotePath=true"];
        quoted.extend(&command);
        quoted.extend(diff_common);
        quoted.push("--patch");
        quoted.extend(&revisions);
        quoted.push("--");
        git.push(quoted);
    }
    for args in git {
        assert_eq!(check_exec_args("git", &strings(&args)), Ok(()), "{args:?}");
    }
    let fields = "number,title,url,state,headRefName,baseRefName,author,labels";
    for args in [
        vec!["pr", "list", "--json", fields, "--limit", "50"],
        vec![
            "pr", "list", "--json", fields, "--limit", "50", "--head", "pu/x",
        ],
        vec!["pr", "diff", "226"],
        vec!["auth", "status"],
    ] {
        assert_eq!(check_exec_args("gh", &strings(&args)), Ok(()), "{args:?}");
    }
}

#[test]
fn given_escaping_git_args_should_refuse_them() {
    for args in [
        vec!["-C", "/", "status"],
        vec!["--literal-pathspecs", "-c", "core.sshCommand=sh", "fetch"],
        vec!["-c", "alias.x=!sh", "x"],
        vec!["-c", "core.fsmonitor=/tmp/x", "status"],
        vec!["--git-dir=/etc", "log"],
        vec!["--git-dir", "/etc", "log"],
        vec!["--work-tree=/", "status"],
        vec!["--exec-path=/tmp", "status"],
        vec!["--namespace=x", "log"],
        vec!["--config-env=core.pager=X", "log"],
        vec!["log", "--output=/tmp/out"],
        vec!["log", "--outp=/tmp/out"],
        vec!["fetch", "--upload-pack=sh -c id", "origin"],
        vec!["fetch", "--upload-p", "sh", "origin"],
        vec!["push", "--receive-pack=sh", "origin"],
        vec!["status", "--git-dir=/etc"],
        vec!["clone", "/x", "/tmp/y"],
        vec!["archive", "-o", "/tmp/x.tar", "HEAD"],
        vec!["diff", "--no-index", "/etc/passwd", "/dev/null"],
        vec!["diff", "--no-index", "--", "/dev/null", "/etc/passwd"],
        vec!["diff", "--no-index", "--", "/dev/null", "../outside"],
        vec!["log", "--no-index", "--", "/dev/null", "a"],
        vec!["--literal-pathspecs"],
        vec![],
        // Writers and aliases: only allowlisted read-only subcommands run.
        vec!["config", "--file", "/x", "alias.x", "y"],
        vec!["config", "--global", "alias.x", "!sh"],
        vec!["x"],
        vec!["--literal-pathspecs", "x"],
        vec!["format-patch", "-o", "/tmp/x", "HEAD~1"],
        vec!["format-patch", "--output-directory", "/tmp/x", "HEAD~1"],
        vec!["log", "--output-directory=/tmp/x"],
        vec!["log", "-o", "/tmp/x"],
        vec!["diff", "-o/tmp/x"],
        vec!["worktree", "add", "/anywhere"],
        vec!["init", "/anywhere"],
        vec!["bundle", "create", "/abs/x.bundle", "HEAD"],
        vec!["fetch", "origin"],
        vec!["checkout", "main"],
        vec!["branch", "-D", "main"],
        vec!["diff", "--ext-diff"],
        vec!["log", "-p", "--textconv"],
        // symbolic-ref only reads.
        vec!["symbolic-ref", "HEAD", "refs/heads/x"],
        vec!["symbolic-ref", "-d", "HEAD"],
        vec!["symbolic-ref", "-m", "msg", "HEAD", "refs/heads/x"],
        vec!["symbolic-ref", "--", "HEAD", "refs/heads/x"],
        vec!["symbolic-ref"],
        // Reproduced reviewer bypasses.
        vec!["grep", "-O", "touch X; :", "-e", "x"],
        vec!["grep", "-Otouch X; :", "-e", "x"],
        vec!["grep", "--open-files-in-pager=touch X", "-e", "x"],
        vec!["config", "core.fsmonitor", "touch X"],
        vec!["config", "alias.x", "!touch X"],
        vec!["hash-object", "-w", "/etc/passwd"],
        vec!["cat-file", "-p", "HEAD"],
        vec!["rebase", "-x", "sh", "main"],
        vec!["difftool", "-x", "sh"],
        vec!["submodule", "foreach", "sh"],
        vec!["bisect", "run", "sh"],
        // Paths outside the repository, with or without an explicit --no-index
        // (git switches diff to no-index mode by itself for an outside path).
        vec!["diff", "/dev/null", "/etc/hosts"],
        vec!["diff", "--", "/dev/null", "/etc/hosts"],
        vec!["log", "-p", "--", "../outside"],
        vec!["diff", "HEAD", "--", "src/../../outside"],
        vec!["ls-files", "/etc"],
        vec!["diff", "-O/dev/zero"],
        vec!["diff", "-O", "/dev/zero"],
        vec!["diff", "--orderfile=/dev/zero"],
        // Bundled short flags and per-subcommand options outside the app's set.
        vec!["diff", "-RO/abs/outside", "--name-only"],
        vec!["diff", "-Ro", "/tmp/x"],
        vec!["ls-files", "-o", "-i", "-X/abs/secret"],
        vec!["ls-files", "--exclude-from=/abs/secret"],
        vec!["ls-files", "--exclude-per-directory=../../outside/file"],
        vec!["rev-list", "-n", "1; rm"],
        vec!["log", "--follow", "--", "a"],
        vec!["status", "--porcelain=v2"],
    ] {
        assert!(
            check_exec_args("git", &strings(&args)).is_err(),
            "{args:?} should be refused"
        );
    }
    // After `--` everything is a path, so option-like names are fine there.
    assert_eq!(
        check_exec_args("git", &strings(&["log", "--", "--output"])),
        Ok(())
    );
}

#[test]
fn given_gh_outside_app_commands_should_refuse_them() {
    for args in [
        vec!["extension", "exec", "x"],
        vec!["my-alias"],
        vec!["alias", "set", "x", "!sh"],
        vec!["api", "/user"],
        vec!["pr", "checkout", "1"],
        vec!["repo", "clone", "a/b"],
        vec!["--repo", "a/b", "pr", "list"],
        vec![],
    ] {
        assert!(
            check_exec_args("gh", &strings(&args)).is_err(),
            "{args:?} should be refused"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_disallowed_git_args_should_refuse_git_exec() {
    let (engine, _tmp, root, _outside) = setup().await;
    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&root),
            program: "git".into(),
            args: strings(&["-C", "/", "status"]),
            stdin_base64: None,
            timeout_ms: 5_000,
        })
        .await;
    assert_error(resp, "args_not_allowed");
}

#[tokio::test(flavor = "current_thread")]
async fn given_git_exec_timeout_should_kill_spawned_helpers() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    let root = base.join("project");
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    let pid_file = base.join("helper.pid");
    // git_exec runs `git` from the login PATH: a wrapper that, for `diff`,
    // starts a helper process and waits on it, like a slow credential helper
    // or pager would (repository drivers can no longer be configured).
    let real_git = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let bin = base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("git"),
        format!(
            "#!/bin/sh\ncase \" $* \" in *\" diff \"*) sleep 30 & echo $! > '{}'; wait; exit 0;; esac\nexec '{}' \"$@\"\n",
            pid_file.display(),
            real_git.trim()
        ),
    )
    .unwrap();
    std::fs::set_permissions(bin.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let engine = Engine::new();
    engine
        .login_env
        .set(vec![(
            "PATH".into(),
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into())
            ),
        )])
        .unwrap();
    let init = engine
        .handle_request(Request::Init {
            project_root: path_string(&root),
        })
        .await;
    assert!(matches!(init, Response::InitResult { .. }), "{init:?}");
    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&root),
            program: "git".into(),
            args: strings(&["diff"]),
            stdin_base64: None,
            timeout_ms: 1_500,
        })
        .await;
    assert_error(resp, "timeout");
    let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        // SAFETY: signal 0 only checks for existence.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if !alive {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "helper {pid} survived the git_exec timeout"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_cwd_outside_projects_should_refuse_git_exec() {
    let (engine, _tmp, _root, outside) = setup().await;
    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&outside),
            program: "git".into(),
            args: vec!["status".into()],
            stdin_base64: None,
            timeout_ms: 5_000,
        })
        .await;
    assert_error(resp, "path_not_allowed");
}

#[tokio::test(flavor = "current_thread")]
async fn given_git_status_in_project_should_return_output() {
    let (engine, _tmp, root, _outside) = setup().await;
    std::fs::write(root.join("new.txt"), "x").unwrap();
    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&root),
            program: "git".into(),
            args: vec!["status".into(), "--porcelain=v1".into()],
            stdin_base64: None,
            timeout_ms: 10_000,
        })
        .await;
    match resp {
        Response::GitExecResult {
            exit_code,
            stdout_base64,
            truncated,
            ..
        } => {
            assert_eq!(exit_code, 0);
            assert!(!truncated);
            let stdout = String::from_utf8(BASE64_STANDARD.decode(stdout_base64).unwrap()).unwrap();
            assert!(stdout.contains("?? new.txt"), "{stdout}");
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_stdin_and_failing_command_should_pass_input_and_exit_code() {
    let (engine, _tmp, root, _outside) = setup().await;
    git(
        &root,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ],
    );
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .unwrap()
        .stdout;
    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&root),
            program: "git".into(),
            // No allowlisted command reads stdin; supplying it must still work.
            args: strings(&["rev-list", "-n", "1", "HEAD"]),
            stdin_base64: Some(b64(b"HEAD\n")),
            timeout_ms: 10_000,
        })
        .await;
    match resp {
        Response::GitExecResult {
            exit_code,
            stdout_base64,
            ..
        } => {
            assert_eq!(exit_code, 0);
            let stdout = BASE64_STANDARD.decode(stdout_base64).unwrap();
            assert_eq!(stdout, head);
        }
        other => panic!("unexpected {other:?}"),
    }

    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&root),
            program: "git".into(),
            args: strings(&["rev-parse", "--verify", "definitely-not-a-ref"]),
            stdin_base64: None,
            timeout_ms: 10_000,
        })
        .await;
    match resp {
        Response::GitExecResult {
            exit_code,
            stderr_base64,
            ..
        } => {
            assert_ne!(exit_code, 0);
            assert!(!BASE64_STANDARD.decode(stderr_base64).unwrap().is_empty());
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_output_over_cap_should_truncate() {
    let (mut kept, mut cut) = read_capped(&b"abcdef"[..], 4).await;
    assert_eq!(kept, b"abcd");
    assert!(cut);
    (kept, cut) = read_capped(&b"abc"[..], 4).await;
    assert_eq!(kept, b"abc");
    assert!(!cut);
}

// --- subscribe_fs ---

#[tokio::test(flavor = "current_thread")]
async fn given_subscribe_fs_should_confirm_only_allowed_directories() {
    let (engine, _tmp, root, outside) = setup().await;
    let resp = engine
        .handle_request(Request::SubscribeFs {
            root: p(&root),
            recursive: true,
        })
        .await;
    assert!(matches!(resp, Response::FsSubscribed), "{resp:?}");
    let resp = engine
        .handle_request(Request::SubscribeFs {
            root: p(&outside),
            recursive: true,
        })
        .await;
    assert_error(resp, "path_not_allowed");
}

#[tokio::test(flavor = "current_thread")]
async fn given_git_metadata_path_should_refuse_write_and_delete_but_allow_read() {
    let (engine, _tmp, root, _outside) = setup().await;
    let config = root.join(".git/config");
    let original = std::fs::read(&config).unwrap();

    let write = engine
        .handle_request(Request::FsWrite {
            path: p(&config),
            data_base64: b64(b"[diff]\n\texternal = /bin/sh\n"),
            expected_modified_ms: None,
            create_dirs: false,
        })
        .await;
    assert_error(write, "path_not_allowed");
    let delete = engine
        .handle_request(Request::FsDelete {
            path: p(&root.join(".git/HEAD")),
        })
        .await;
    assert_error(delete, "path_not_allowed");
    assert_eq!(std::fs::read(&config).unwrap(), original);
    assert!(root.join(".git/HEAD").exists());

    let read = engine
        .handle_request(Request::FsRead {
            path: p(&config),
            max_bytes: 4096,
        })
        .await;
    assert!(matches!(read, Response::FsReadResult { .. }), "{read:?}");
}

#[test]
fn given_ranges_and_relative_paths_should_pass_the_path_check() {
    for args in [
        vec!["log", "--format=%H", "abc..def", "--"],
        vec!["diff", "HEAD~1..HEAD", "--", "src/main.rs"],
        vec!["diff", "--no-index", "--", "/dev/null", "new.txt"],
        vec![
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ],
    ] {
        assert_eq!(check_exec_args("git", &strings(&args)), Ok(()), "{args:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_planted_bare_repo_should_not_run_its_config() {
    let (engine, tmp, root, _outside) = setup().await;
    let marker = tmp.path().join("bare-ran");
    let evil = root.join("evil");
    for dir in ["objects", "refs"] {
        std::fs::create_dir_all(evil.join(dir)).unwrap();
    }
    std::fs::write(evil.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(
        evil.join("config"),
        format!("[diff]\n\texternal = touch '{}' ;\n", marker.display()),
    )
    .unwrap();
    std::fs::write(evil.join("f"), "x").unwrap();
    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&evil),
            program: "git".into(),
            args: strings(&["diff", "--no-index", "--", "/dev/null", "f"]),
            stdin_base64: None,
            timeout_ms: 10_000,
        })
        .await;
    assert!(matches!(resp, Response::GitExecResult { .. }), "{resp:?}");
    assert!(!marker.exists(), "planted bare repository config ran");
}

#[tokio::test(flavor = "current_thread")]
async fn given_root_without_repo_git_should_not_find_an_enclosing_one() {
    let tmp = TempDir::new().unwrap();
    let base = tmp.path().canonicalize().unwrap();
    // An enclosing repository (like a dotfiles repo at $HOME) above a project
    // that is not a repository itself.
    git(&base, &["init", "-q"]);
    let root = base.join("project");
    std::fs::create_dir_all(&root).unwrap();
    let engine = Engine::new();
    let init = engine
        .handle_request(Request::Init {
            project_root: path_string(&root),
        })
        .await;
    assert!(matches!(init, Response::InitResult { .. }), "{init:?}");
    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&root),
            program: "git".into(),
            args: strings(&["ls-files", "-z"]),
            stdin_base64: None,
            timeout_ms: 10_000,
        })
        .await;
    let Response::GitExecResult { exit_code, .. } = resp else {
        panic!("{resp:?}")
    };
    assert_ne!(exit_code, 0, "git found the enclosing repository");
}

#[test]
fn given_registered_home_should_never_be_an_fs_root() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().canonicalize().unwrap();
    let project = home.join("code");
    std::fs::create_dir_all(&project).unwrap();
    let roots = allowed_roots(&[path_string(&home), path_string(&project)], Some(&home));
    assert_eq!(roots.writable, vec![project.clone()]);
    assert!(!roots.all.contains(&home));
}

#[test]
fn given_diff_or_log_git_should_never_run_external_drivers() {
    assert_eq!(
        exec_args(
            "git",
            strings(&[
                "--literal-pathspecs",
                "-c",
                "core.quotePath=true",
                "diff",
                "HEAD"
            ])
        ),
        strings(&[
            "-c",
            "core.fsmonitor=",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "safe.bareRepository=explicit",
            "--literal-pathspecs",
            "-c",
            "core.quotePath=true",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "HEAD"
        ])
    );
    let status = exec_args("git", strings(&["status"]));
    assert_eq!(status.last().map(String::as_str), Some("status"));
}

#[test]
fn given_case_or_ignorable_variants_of_dot_git_should_be_metadata() {
    for name in [".git", ".GIT", ".Git", ".g\u{200C}it", "\u{FEFF}.git"] {
        assert!(is_dot_git(name), "{name:?}");
    }
    for name in [".github", "git", ".gitignore", ".git2"] {
        assert!(!is_dot_git(name), "{name:?}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_uppercase_git_dir_write_should_be_refused() {
    let (engine, _tmp, root, _outside) = setup().await;
    let resp = engine
        .handle_request(Request::FsWrite {
            path: p(&root.join("sub/.GIT/config")),
            data_base64: b64(b"[diff]\n\texternal = /bin/sh\n"),
            expected_modified_ms: None,
            create_dirs: true,
        })
        .await;
    assert_error(resp, "path_not_allowed");
    assert!(!root.join("sub/.GIT").exists());
}

#[tokio::test(flavor = "current_thread")]
async fn given_planted_bare_repo_listing_should_not_run_its_fsmonitor() {
    let (engine, tmp, root, _outside) = setup().await;
    let marker = tmp.path().join("bare-fsmonitor-ran");
    let evil = root.join("evil");
    for dir in ["objects", "refs"] {
        std::fs::create_dir_all(evil.join(dir)).unwrap();
    }
    std::fs::write(evil.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(
        evil.join("config"),
        format!(
            "[core]\n\tbare = false\n\tworktree = {}\n\tfsmonitor = touch '{}' ;\n",
            evil.display(),
            marker.display()
        ),
    )
    .unwrap();
    let resp = engine
        .handle_request(Request::FsList {
            path: p(&evil),
            recursive: false,
            include_hidden: true,
            max_entries: 100,
        })
        .await;
    assert!(matches!(resp, Response::FsListResult { .. }), "{resp:?}");
    assert!(
        !marker.exists(),
        "planted bare repository fsmonitor ran on fs_list"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn given_symlink_operand_pointing_outside_should_be_refused() {
    let (engine, _tmp, root, outside) = setup().await;
    std::os::unix::fs::symlink(&outside, root.join("outlink")).unwrap();
    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&root),
            program: "git".into(),
            args: strings(&[
                "diff",
                "--no-index",
                "--",
                "/dev/null",
                "outlink/secret.txt",
            ]),
            stdin_base64: None,
            timeout_ms: 10_000,
        })
        .await;
    assert_error(resp, "args_not_allowed");
}

#[test]
fn given_gh_options_outside_the_apps_should_refuse_them() {
    for args in [
        vec!["pr", "list", "-R", "other/repo"],
        vec!["pr", "list", "--repo=other/repo"],
        vec!["pr", "diff", "226", "--web"],
        vec!["pr", "view", "226"],
        vec!["pr", "list", "--json"],
        vec!["auth", "status", "--show-token"],
    ] {
        assert!(
            check_exec_args("gh", &strings(&args)).is_err(),
            "{args:?} should be refused"
        );
    }
}

#[test]
fn given_gh_positionals_should_allow_only_a_pr_number() {
    assert_eq!(
        check_exec_args("gh", &strings(&["pr", "diff", "226"])),
        Ok(())
    );
    for args in [
        vec!["pr", "diff", "https://github.com/other/repo/pull/1"],
        vec!["pr", "list", "extra"],
        vec!["repo", "view", "other/repo"],
        vec!["auth", "status", "x"],
    ] {
        assert!(
            check_exec_args("gh", &strings(&args)).is_err(),
            "{args:?} should be refused"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_executable_pu_definitions_should_refuse_fs_writes() {
    let (engine, _tmp, root, _outside) = setup().await;
    for path in [
        ".pu/schedules/x.yaml",
        ".pu/agents/x.yaml",
        ".pu/Triggers/x.yaml",
    ] {
        let resp = engine
            .handle_request(Request::FsWrite {
                path: p(&root.join(path)),
                data_base64: b64(b"command: touch /tmp/x\n"),
                expected_modified_ms: None,
                create_dirs: true,
            })
            .await;
        assert_error(resp, "path_not_allowed");
    }
    // The app's own layout file stays writable.
    let resp = engine
        .handle_request(Request::FsWrite {
            path: p(&root.join(".pu/workspaces.json")),
            data_base64: b64(b"{}"),
            expected_modified_ms: None,
            create_dirs: true,
        })
        .await;
    assert!(matches!(resp, Response::FsWriteResult { .. }), "{resp:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn given_operand_symlink_pointing_outside_should_diff_the_link_itself() {
    let (engine, _tmp, root, outside) = setup().await;
    std::fs::write(outside.join("payload.txt"), "PAYLOAD-CONTENT").unwrap();
    std::os::unix::fs::symlink(outside.join("payload.txt"), root.join("link")).unwrap();
    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&root),
            program: "git".into(),
            args: strings(&["diff", "--no-index", "--", "/dev/null", "link"]),
            stdin_base64: None,
            timeout_ms: 10_000,
        })
        .await;
    let Response::GitExecResult { stdout_base64, .. } = resp else {
        panic!("{resp:?}")
    };
    let stdout = String::from_utf8(BASE64_STANDARD.decode(stdout_base64).unwrap()).unwrap();
    assert!(
        !stdout.contains("PAYLOAD-CONTENT"),
        "the target's contents leaked: {stdout}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn given_forged_manifest_worktree_diff_and_pulse_should_not_run_git_there() {
    let (engine, _tmp, root, outside) = setup().await;
    // A client rewrote the manifest so a worktree points outside .pu/worktrees.
    register_worktree(&root, "wt-forged", &outside);
    let resp = engine
        .handle_request(Request::Diff {
            project_root: path_string(&root),
            worktree_id: Some("wt-forged".into()),
            stat: false,
        })
        .await;
    let Response::DiffResult { diffs } = resp else {
        panic!("{resp:?}")
    };
    assert_eq!(diffs.len(), 1);
    assert!(diffs[0].diff_output.is_empty());
    assert!(
        diffs[0]
            .error
            .as_deref()
            .is_some_and(|e| e.contains(".pu/worktrees")),
        "{:?}",
        diffs[0].error
    );

    let resp = engine
        .handle_request(Request::Pulse {
            project_root: path_string(&root),
        })
        .await;
    let Response::PulseReport { worktrees, .. } = resp else {
        panic!("{resp:?}")
    };
    let forged = worktrees
        .iter()
        .find(|w| w.worktree_id == "wt-forged")
        .expect("forged worktree reported");
    assert!(
        forged
            .diff_error
            .as_deref()
            .is_some_and(|e| e.contains(".pu/worktrees"))
    );
}

#[test]
fn given_trailing_slash_or_dot_operands_should_be_refused() {
    for args in [
        vec!["diff", "--no-index", "--", "dir", "link/"],
        vec!["diff", "--no-index", "--", "/dev/null", "link/."],
        vec!["diff", "HEAD", "--", "link/"],
        vec!["diff", "HEAD", "--", "./link"],
        vec!["log", "--", "a//b"],
    ] {
        assert!(
            check_exec_args("git", &strings(&args)).is_err(),
            "{args:?} should be refused"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn given_dot_pu_config_yaml_should_refuse_fs_write() {
    let (engine, _tmp, root, _outside) = setup().await;
    let resp = engine
        .handle_request(Request::FsWrite {
            path: p(&root.join(".pu/config.yaml")),
            data_base64: b64(b"agents:\n  claude:\n    command: sh\n"),
            expected_modified_ms: None,
            create_dirs: true,
        })
        .await;
    assert_error(resp, "path_not_allowed");
}

#[tokio::test(flavor = "current_thread")]
async fn given_dash_named_symlink_operand_should_be_checked_after_separator() {
    let (engine, _tmp, root, outside) = setup().await;
    std::fs::create_dir_all(outside.join("d")).unwrap();
    std::fs::write(outside.join("d/f"), "DASH-SECRET").unwrap();
    std::os::unix::fs::symlink(outside.join("d"), root.join("-dir")).unwrap();
    for args in [
        vec![
            "diff",
            "--no-index",
            "--no-color",
            "--",
            "/dev/null",
            "-dir/f",
        ],
        vec!["diff", "--", "-dir/f", "other"],
    ] {
        let resp = engine
            .handle_request(Request::GitExec {
                cwd: p(&root),
                program: "git".into(),
                args: strings(&args),
                stdin_base64: None,
                timeout_ms: 10_000,
            })
            .await;
        assert_error(resp, "args_not_allowed");
    }
}
