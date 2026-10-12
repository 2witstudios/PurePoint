use super::*;

fn launch(token: &str) -> GuardianLaunch {
    GuardianLaunch {
        run_id: "run-1".into(),
        token: token.into(),
        outcome_dir: PathBuf::from("/tmp/outcomes"),
    }
}

#[test]
fn given_new_tokens_should_be_distinct_256_bit_hex() {
    // given / when
    let a = new_token();
    let b = new_token();
    // then
    assert_eq!(a.len(), 64);
    assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    assert_ne!(a, b);
}

#[test]
fn given_launch_should_wrap_command_without_token_on_argv() {
    // given
    let token = new_token();
    // when
    let (command, args, env) = wrap_spawn(
        &launch(&token),
        Path::new("/opt/pu-engine"),
        42,
        "claude".into(),
        vec!["--flag".into(), "x".into()],
    );
    // then
    assert_eq!(command, "/opt/pu-engine");
    assert_eq!(
        args,
        [
            "--mesh-guardian",
            "--run",
            "run-1",
            "--token-env",
            GUARDIAN_TOKEN_ENV,
            "--parent",
            "42",
            "--outcome-dir",
            "/tmp/outcomes",
            "--",
            "claude",
            "--flag",
            "x",
        ]
    );
    assert!(args.iter().all(|a| !a.contains(&token)));
    assert_eq!(env, vec![(GUARDIAN_TOKEN_ENV.to_string(), token)]);
}

#[test]
fn given_token_should_report_platform_mechanism_and_sha256_digest() {
    // given
    let token = "ab".repeat(32);
    // when
    let (mechanism, digest) = supervision_identity(&token);
    // then
    #[cfg(target_os = "macos")]
    assert_eq!(mechanism, "guardian-v1-macos");
    #[cfg(target_os = "linux")]
    assert_eq!(mechanism, "guardian-v1-linux");
    assert_eq!(digest, token_digest(&token));
    assert_eq!(digest.len(), 64);
    assert!(!digest.contains(&token));
}

#[test]
fn given_invalid_arguments_should_reject_before_launch() {
    // given
    let ok = |run: &str| -> Vec<String> {
        [
            "--run",
            run,
            "--token-env",
            "PU_X_TOKEN",
            "--parent",
            "4242",
            "--outcome-dir",
            "/tmp",
            "--",
            "true",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    };
    // when / then
    assert!(parse_args(&ok("good_run-1")).is_ok());
    for bad in ["", "a/b", "..", "has space", &"x".repeat(65)] {
        assert!(parse_args(&ok(bad)).is_err(), "{bad:?}");
    }
    let mut no_command = ok("r");
    no_command.pop();
    assert!(parse_args(&no_command).is_err());
    let mut bad_env = ok("r");
    bad_env[3] = RUN_TOKEN_ENV.into();
    assert!(parse_args(&bad_env).is_err());
    let mut bad_parent = ok("r");
    for pid in ["0", "1", "-5", "x"] {
        bad_parent[5] = pid.into();
        assert!(parse_args(&bad_parent).is_err(), "{pid}");
    }
}

#[test]
fn given_short_or_non_hex_token_should_be_invalid() {
    // given / when / then
    assert!(validate_token(&"a".repeat(32)).is_ok());
    assert!(validate_token(&"a".repeat(31)).is_err());
    assert!(validate_token(&"g".repeat(40)).is_err());
    assert!(validate_token(&"a".repeat(1025)).is_err());
}

#[test]
fn given_env_block_should_match_only_exact_token_entry() {
    // given
    let token = "c".repeat(64);
    let entry = format!("{RUN_TOKEN_ENV}={token}");
    let block = |entries: &[&str]| -> Vec<u8> {
        let mut out = Vec::new();
        for e in entries {
            out.extend_from_slice(e.as_bytes());
            out.push(0);
        }
        out
    };
    // when / then
    assert!(env_block_has_entry(
        &block(&["A=1", &entry]),
        entry.as_bytes()
    ));
    assert!(!env_block_has_entry(
        &block(&["A=1", &format!("{entry}x")]),
        entry.as_bytes()
    ));
    assert!(!env_block_has_entry(
        &block(&[&format!("X{entry}")]),
        entry.as_bytes()
    ));
    assert!(!env_block_has_entry(&block(&["A=1"]), entry.as_bytes()));
}

#[test]
fn given_process_tree_should_find_chain_to_guardian_only() {
    // given: 10 -> 11 -> 12, and 20 -> 1
    let parents: std::collections::HashMap<i32, i32> = [(11, 10), (12, 11), (20, 1), (30, 30)]
        .into_iter()
        .collect();
    // when / then
    assert!(chain_reaches(&parents, 12, 10));
    assert!(chain_reaches(&parents, 11, 10));
    assert!(!chain_reaches(&parents, 20, 10));
    assert!(!chain_reaches(&parents, 10, 10));
    assert!(!chain_reaches(&parents, 30, 10));
}

#[test]
fn given_child_inheriting_lease_should_be_listed_and_hold_it_while_stranger_does_not() {
    // given: an unlinked file whose descriptor one child inherits.
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::process::CommandExt;
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("lease");
    let lease = std::fs::File::create(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    let meta = lease.metadata().unwrap();
    let id = (meta.dev(), meta.ino());
    let raw = lease.as_raw_fd();
    let spawn = |inherit: bool| {
        let mut cmd = std::process::Command::new("/bin/sleep");
        cmd.arg("30");
        if inherit {
            // SAFETY: dup2 is async-signal-safe; it clears CLOEXEC on fd 9.
            unsafe {
                cmd.pre_exec(move || {
                    libc::dup2(raw, 9);
                    Ok(())
                });
            }
        }
        cmd.spawn().unwrap()
    };
    let mut holder = spawn(true);
    let mut stranger = spawn(false);
    std::thread::sleep(std::time::Duration::from_millis(200));
    // when
    let holds = platform::holds_vnode(holder.id() as i32, id);
    let stranger_holds = platform::holds_vnode(stranger.id() as i32, id);
    let info = platform::process(holder.id() as i32);
    let listed = platform::list_processes()
        .iter()
        .any(|p| p.pid == holder.id() as i32);
    for child in [&mut holder, &mut stranger] {
        child.kill().unwrap();
        child.wait().unwrap();
    }
    // then
    assert!(holds, "inherited lease descriptor not visible");
    assert!(!stranger_holds);
    let info = info.unwrap();
    assert_eq!(info.ppid, std::process::id() as i32);
    assert!(info.start > 0);
    assert!(listed);
}

#[cfg(target_os = "macos")]
#[test]
fn given_procargs2_block_should_extract_only_environment_strings() {
    // given: argc=2, exec path, NUL padding, argv, env, terminator, junk.
    let mut data = 2i32.to_ne_bytes().to_vec();
    data.extend_from_slice(b"/bin/x\0\0\0\0/bin/x\0arg\0A=1\0B=2\0\0junk\0");
    // when
    let env = platform::parse_procargs2(&data).unwrap();
    // then
    assert_eq!(env, b"A=1\0B=2\0");
    assert!(platform::parse_procargs2(&data[..3]).is_none());
}
