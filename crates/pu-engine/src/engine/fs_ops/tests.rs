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

#[tokio::test(flavor = "current_thread")]
async fn given_worktree_outside_project_dir_should_allow_it() {
    let (engine, tmp, root, _outside) = setup().await;
    let wt_dir = tmp.path().canonicalize().unwrap().join("elsewhere-wt");
    std::fs::create_dir_all(&wt_dir).unwrap();
    std::fs::write(wt_dir.join("a.txt"), "wt").unwrap();
    let mut m = manifest::read_manifest(&root).unwrap();
    m.worktrees.insert(
        "wt-x".into(),
        pu_core::types::WorktreeEntry {
            id: "wt-x".into(),
            name: "x".into(),
            path: p(&wt_dir),
            branch: "pu/x".into(),
            base_branch: None,
            status: pu_core::types::WorktreeStatus::Active,
            agents: Default::default(),
            created_at: chrono::Utc::now(),
            merged_at: None,
            error: None,
        },
    );
    std::fs::write(
        pu_core::paths::manifest_path(&root),
        serde_json::to_string(&m).unwrap(),
    )
    .unwrap();
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
    match fs_list(&p(&transcripts), &roots, false, true, 10) {
        Response::FsListResult { entries, .. } => {
            assert_eq!(entries.len(), 1);
            assert!(!entries[0].ignored);
        }
        other => panic!("unexpected {other:?}"),
    }
    // Other directories under home stay out of reach.
    assert_error(
        fs_stat(&p(&home.join(".ssh/id_rsa")), &roots),
        "path_not_allowed",
    );
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
            args: vec!["status".into(), "--porcelain".into()],
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
    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&root),
            program: "git".into(),
            args: vec!["hash-object".into(), "--stdin".into()],
            stdin_base64: Some(b64(b"hello\n")),
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
            assert_eq!(stdout, b"ce013625030ba8dba906f756967f9e9ca394464a\n");
        }
        other => panic!("unexpected {other:?}"),
    }

    let resp = engine
        .handle_request(Request::GitExec {
            cwd: p(&root),
            program: "git".into(),
            args: vec!["definitely-not-a-command".into()],
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
