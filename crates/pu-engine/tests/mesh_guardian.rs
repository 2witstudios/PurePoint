//! Real-process tests for the mesh run guardian, using the compiled
//! `pu-engine --mesh-guardian` binary. Every process a test creates carries a
//! unique per-test marker on argv; `Cleanup` SIGKILLs only processes whose argv
//! contains that marker, so failures never leak or touch foreign processes.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use pu_engine::mesh_guardian::{Outcome, RUN_TOKEN_ENV, Trigger, new_token, read_outcome};

const EXE: &str = env!("CARGO_BIN_EXE_pu-engine");
const TOKEN_ENV: &str = "PU_TEST_GUARDIAN_TOKEN";

/// Writes `$$` atomically to `$ARGV[0]`.
const PIDFILE: &str = r#"open(my $f, ">", "$ARGV[0].tmp") or die; print $f $$; close $f; rename("$ARGV[0].tmp", $ARGV[0]) or die;"#;

fn ignoring_child() -> String {
    format!(r#"$SIG{{TERM}} = $SIG{{HUP}} = $SIG{{INT}} = "IGNORE"; {PIDFILE} sleep 300;"#)
}

fn detached_descendant() -> String {
    format!(r#"use POSIX; exit 0 if fork; POSIX::setsid(); exit 0 if fork; {PIDFILE} sleep 300;"#)
}

struct Cleanup {
    marker: String,
}

impl Cleanup {
    fn new(tag: &str) -> Self {
        Self {
            marker: format!("pu-guardian-test-{tag}-{}", &new_token()[..16]),
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        for (pid, args) in processes() {
            if args.contains(&self.marker) && pid != std::process::id() as i32 {
                // SAFETY: plain kill(2) on a pid whose argv carries our marker.
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
        }
    }
}

fn processes() -> Vec<(i32, String)> {
    let out = Command::new("/bin/ps")
        .args(["-axww", "-o", "pid=,args="])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let (pid, args) = l.split_once(' ')?;
            Some((pid.parse().ok()?, args.to_string()))
        })
        .collect()
}

fn private_dir() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    tmp
}

fn wait_pidfile(path: &Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(s) = std::fs::read_to_string(path)
            && let Ok(pid) = s.trim().parse()
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "pidfile {path:?} never appeared");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// True when the pid is gone or a zombie awaiting reaping by launchd/init.
fn is_dead(pid: i32) -> bool {
    // SAFETY: signal 0 only probes existence.
    if unsafe { libc::kill(pid, 0) } != 0 {
        return true;
    }
    let out = Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let stat = String::from_utf8_lossy(&out.stdout);
    stat.trim().is_empty() || stat.trim_start().starts_with('Z')
}

fn wait_dead(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if is_dead(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    is_dead(pid)
}

fn wait_outcome(dir: &Path, run: &str, token: &str) -> Outcome {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(Some(outcome)) = read_outcome(dir, run, token) {
            return outcome;
        }
        assert!(Instant::now() < deadline, "no outcome for {run}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn guardian(run: &str, token: &str, parent: u32, dir: &Path, agent_script: &str) -> Command {
    let mut cmd = Command::new(EXE);
    cmd.args(["--mesh-guardian", "--run", run, "--token-env", TOKEN_ENV])
        .args(["--parent", &parent.to_string()])
        .arg("--outcome-dir")
        .arg(dir)
        .args(["--grace-ms", "500", "--", "/bin/sh", "-c", agent_script])
        .env(TOKEN_ENV, token)
        .stdin(Stdio::null());
    cmd
}

fn perl(code: &str, pidfile: &Path, marker: &str) -> String {
    format!("perl -e '{code}' {} {marker}", pidfile.display())
}

fn wait_child(child: &mut Child) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status.code().unwrap_or(-1);
        }
        assert!(Instant::now() < deadline, "guardian did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn run_id(cleanup: &Cleanup) -> String {
    cleanup.marker.clone()
}

#[test]
fn given_killed_daemon_should_terminate_ignoring_and_detached_descendants() {
    // given: a fake daemon (sh) launches the guardian, whose agent starts a
    // SIGTERM/SIGHUP-ignoring child and a double-forked setsid descendant.
    let cleanup = Cleanup::new("daemon");
    let run = run_id(&cleanup);
    let token = new_token();
    let dir = private_dir();
    let work = tempfile::TempDir::new().unwrap();
    let (ign, det) = (work.path().join("ign"), work.path().join("det"));
    let agent = format!(
        "{} & {} & wait",
        perl(&ignoring_child(), &ign, &cleanup.marker),
        perl(&detached_descendant(), &det, &cleanup.marker)
    );
    let mut daemon = Command::new("/bin/sh")
        .args([
            "-c",
            r#""$0" --mesh-guardian --run "$1" --token-env "$2" --parent $$ --outcome-dir "$3" --grace-ms 500 -- /bin/sh -c "$4" & echo $! > "$5"; wait"#,
            EXE,
            &run,
            TOKEN_ENV,
        ])
        .arg(dir.path())
        .arg(&agent)
        .arg(work.path().join("guardian"))
        .env(TOKEN_ENV, &token)
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let ign_pid = wait_pidfile(&ign);
    let det_pid = wait_pidfile(&det);
    let guardian_pid = wait_pidfile(&work.path().join("guardian"));
    // when
    let killed_at = Instant::now();
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let outcome = wait_outcome(dir.path(), &run, &token);
    let latency = killed_at.elapsed();
    // then
    eprintln!("daemon-exit termination latency (grace 500ms, ignoring child): {latency:?}");
    assert_eq!(outcome.trigger, Trigger::DaemonExit);
    assert!(outcome.verified, "{outcome:?}");
    assert_eq!(outcome.survivors, Vec::<u32>::new());
    assert!(wait_dead(ign_pid), "SIGTERM-ignoring child survived");
    assert!(wait_dead(det_pid), "detached setsid descendant survived");
    assert!(wait_dead(guardian_pid), "guardian did not exit");
}

#[test]
fn given_agent_exit_with_detached_descendant_should_kill_it_and_record_exit_code() {
    // given
    let cleanup = Cleanup::new("agentexit");
    let run = run_id(&cleanup);
    let token = new_token();
    let dir = private_dir();
    let work = tempfile::TempDir::new().unwrap();
    let det = work.path().join("det");
    let agent = format!(
        "{}; while [ ! -f {} ]; do sleep 0.02; done; exit 7",
        perl(&detached_descendant(), &det, &cleanup.marker),
        det.display()
    );
    // when
    let mut child = guardian(&run, &token, std::process::id(), dir.path(), &agent)
        .spawn()
        .unwrap();
    let det_pid = wait_pidfile(&det);
    let code = wait_child(&mut child);
    // then
    let outcome = read_outcome(dir.path(), &run, &token).unwrap().unwrap();
    assert_eq!(code, 7);
    assert_eq!(outcome.trigger, Trigger::AgentExit);
    assert_eq!(outcome.agent_exit, Some(7));
    assert!(outcome.verified, "{outcome:?}");
    assert!(wait_dead(det_pid), "detached marked descendant survived");
}

#[test]
fn given_sigterm_to_guardian_should_forward_and_terminate_tree() {
    // given
    let cleanup = Cleanup::new("sigterm");
    let run = run_id(&cleanup);
    let token = new_token();
    let dir = private_dir();
    let work = tempfile::TempDir::new().unwrap();
    let (ign, det) = (work.path().join("ign"), work.path().join("det"));
    let agent = format!(
        "{} & {} & wait",
        perl(&ignoring_child(), &ign, &cleanup.marker),
        perl(&detached_descendant(), &det, &cleanup.marker)
    );
    let mut child = guardian(&run, &token, std::process::id(), dir.path(), &agent)
        .spawn()
        .unwrap();
    let ign_pid = wait_pidfile(&ign);
    let det_pid = wait_pidfile(&det);
    // when
    // SAFETY: signalling the guardian child this test spawned.
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let code = wait_child(&mut child);
    // then
    let outcome = read_outcome(dir.path(), &run, &token).unwrap().unwrap();
    assert_eq!(outcome.trigger, Trigger::Signal);
    assert!(outcome.verified, "{outcome:?}");
    assert_eq!(outcome.agent_exit, Some(128 + libc::SIGTERM));
    assert_eq!(code, 128 + libc::SIGTERM);
    assert!(wait_dead(ign_pid));
    assert!(wait_dead(det_pid));
}

const SLEEPER_ENV: &str = "PU_GUARDIAN_TEST_SLEEPER";

/// Helper, not a behavior check: re-executed by the token-channel test with
/// `SLEEPER_ENV=<pidfile>`, this non-platform binary (whose environment macOS
/// exposes) records its pid and sleeps. A no-op in normal suite runs.
#[test]
fn given_sleeper_env_should_act_as_detached_helper() {
    let Some(path) = std::env::var_os(SLEEPER_ENV) else {
        return;
    };
    let tmp = PathBuf::from(format!("{}.tmp", path.to_string_lossy()));
    std::fs::write(&tmp, std::process::id().to_string()).unwrap();
    std::fs::rename(&tmp, &path).unwrap();
    std::thread::sleep(Duration::from_secs(300));
}

#[test]
fn given_detached_descendant_without_lease_or_ancestry_should_be_found_by_token_env() {
    // given: a double-forked setsid descendant that closes every inherited
    // descriptor (dropping the lease) and execs a non-platform binary, so
    // only the PU_MESH_RUN_TOKEN environment entry links it to the run.
    let cleanup = Cleanup::new("tokenenv");
    let run = run_id(&cleanup);
    let token = new_token();
    let dir = private_dir();
    let work = tempfile::TempDir::new().unwrap();
    let sleeper = work.path().join("sleeper");
    let code = r#"use POSIX; exit 0 if fork; POSIX::setsid(); exit 0 if fork; POSIX::close($_) for 3..4095; exec($ARGV[0], "--exact", "given_sleeper_env_should_act_as_detached_helper", "--nocapture", "--skip", $ARGV[1]) or die;"#;
    let helper = std::env::current_exe().unwrap();
    let agent = format!(
        "{SLEEPER_ENV}={} perl -e '{code}' {} {}; while [ ! -f {} ]; do sleep 0.02; done; exit 0",
        sleeper.display(),
        helper.display(),
        cleanup.marker,
        sleeper.display()
    );
    // when
    let mut child = guardian(&run, &token, std::process::id(), dir.path(), &agent)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = wait_pidfile(&sleeper);
    assert_eq!(wait_child(&mut child), 0);
    // then
    let outcome = read_outcome(dir.path(), &run, &token).unwrap().unwrap();
    assert_eq!(outcome.trigger, Trigger::AgentExit);
    assert!(outcome.verified, "{outcome:?}");
    assert!(wait_dead(pid), "token-marked detached descendant survived");
}

#[test]
fn given_scrubbed_descendant_observed_before_reparenting_should_still_be_killed() {
    // given: a descendant execs with an empty environment (no token), closes
    // inherited descriptors (no lease), leaves the agent process group, is
    // observed while its ancestry reaches
    // the guardian, then is orphaned. Only the remembered (pid, start) links it.
    let cleanup = Cleanup::new("scrubbed");
    let run = run_id(&cleanup);
    let token = new_token();
    let dir = private_dir();
    let work = tempfile::TempDir::new().unwrap();
    let scrubbed = work.path().join("scrubbed");
    let agent = format!(
        "( env -i /usr/bin/{} & sleep 1 ); perl -e \"sleep 300\" {}",
        perl(
            &format!(
                "use POSIX; POSIX::setsid(); POSIX::close($_) for 3..4095; {PIDFILE} sleep 300;"
            ),
            &scrubbed,
            &cleanup.marker
        ),
        cleanup.marker
    );
    let mut child = guardian(&run, &token, std::process::id(), dir.path(), &agent)
        .spawn()
        .unwrap();
    let pid = wait_pidfile(&scrubbed);
    // macOS reparents orphans to launchd. On Linux the guardian is a child
    // subreaper, so the orphan reparents to the guardian itself.
    let reaper = if cfg!(target_os = "linux") {
        child.id().to_string()
    } else {
        "1".to_string()
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let out = Command::new("ps")
            .args(["-o", "ppid=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        if String::from_utf8_lossy(&out.stdout).trim() == reaper {
            break;
        }
        assert!(Instant::now() < deadline, "descendant never reparented");
        std::thread::sleep(Duration::from_millis(20));
    }
    let env = Command::new("/bin/ps")
        .args(["eww", "-o", "command=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&env.stdout).contains(&token));
    // when
    // SAFETY: signalling the guardian child this test spawned.
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    wait_child(&mut child);
    // then
    let outcome = read_outcome(dir.path(), &run, &token).unwrap().unwrap();
    assert!(outcome.verified, "{outcome:?}");
    assert!(wait_dead(pid), "scrubbed, reparented descendant survived");
}

#[test]
fn given_foreign_processes_without_exact_token_should_never_be_signalled() {
    // given: same-uid processes the run did not start, created after the
    // guardian (so the start-time gate does not exclude them) from a binary
    // whose environment is readable: one unmarked, one with a different
    // token, one whose entry only has the run token as a prefix.
    let cleanup = Cleanup::new("foreign");
    let run = run_id(&cleanup);
    let token = new_token();
    let dir = private_dir();
    let work = tempfile::TempDir::new().unwrap();
    let mut child = guardian(&run, &token, std::process::id(), dir.path(), "sleep 1.5")
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let helper = std::env::current_exe().unwrap();
    let mut foreign = Vec::new();
    for (name, value) in [
        ("plain", None),
        ("other", Some(new_token())),
        ("prefix", Some(format!("{token}0"))),
    ] {
        let path = work.path().join(name);
        let mut cmd = Command::new(&helper);
        cmd.args(["--exact", "given_sleeper_env_should_act_as_detached_helper"])
            .args(["--nocapture", "--skip", &cleanup.marker])
            .env(SLEEPER_ENV, &path)
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        if let Some(value) = &value {
            cmd.env(RUN_TOKEN_ENV, value);
        }
        let process = cmd.spawn().unwrap();
        let pid = wait_pidfile(&path);
        if let Some(value) = &value {
            let env = Command::new("/bin/ps")
                .args(["eww", "-o", "command=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let shown = String::from_utf8_lossy(&env.stdout);
            assert!(
                shown.contains(&format!("{RUN_TOKEN_ENV}={value}")),
                "{shown}"
            );
        }
        foreign.push((pid, process));
    }
    // when: the run's agent exits and the guardian terminates the run.
    assert_eq!(wait_child(&mut child), 0);
    // then
    let outcome = read_outcome(dir.path(), &run, &token).unwrap().unwrap();
    assert!(outcome.verified);
    for (pid, mut process) in foreign {
        let survived = !is_dead(pid);
        process.kill().unwrap();
        process.wait().unwrap();
        assert!(survived, "foreign process {pid} was signalled");
    }
}

#[test]
fn given_unsafe_outcome_dir_should_refuse_before_forking_and_reader_should_reject() {
    // given
    let cleanup = Cleanup::new("unsafe");
    let token = new_token();
    let work = tempfile::TempDir::new().unwrap();
    let private = private_dir();
    let link = work.path().join("link");
    std::os::unix::fs::symlink(private.path(), &link).unwrap();
    let open = work.path().join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
    let foreign_owned = PathBuf::from("/usr");
    for (i, dir) in [link, open, foreign_owned].iter().enumerate() {
        let run = format!("{}-{i}", cleanup.marker);
        let marker = work.path().join(format!("forked-{i}"));
        // when
        let status = guardian(
            &run,
            &token,
            std::process::id(),
            dir,
            &format!("touch {}", marker.display()),
        )
        .status()
        .unwrap();
        // then
        assert_ne!(status.code(), Some(0), "{dir:?}");
        assert!(!marker.exists(), "agent forked despite unsafe {dir:?}");
        assert!(read_outcome(dir, &run, &token).is_err(), "{dir:?}");
    }
}

#[test]
fn given_tampered_outcome_or_wrong_token_should_reader_reject() {
    // given
    let cleanup = Cleanup::new("tamper");
    let run = run_id(&cleanup);
    let token = new_token();
    let dir = private_dir();
    let status = guardian(&run, &token, std::process::id(), dir.path(), "exit 0")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0));
    assert!(
        read_outcome(dir.path(), &run, &token)
            .unwrap()
            .unwrap()
            .verified
    );
    // when / then: a different token never verifies this outcome.
    assert!(read_outcome(dir.path(), &run, &new_token()).is_err());
    // when / then: a forged digest is rejected even for the right token.
    let path = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.file_name().unwrap().to_string_lossy().starts_with(&run))
        .unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["token_digest"] = serde_json::Value::String("0".repeat(64));
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(read_outcome(dir.path(), &run, &token).is_err());
}

#[test]
fn given_running_guardian_should_not_expose_token_on_argv() {
    // given
    let cleanup = Cleanup::new("argv");
    let run = run_id(&cleanup);
    let token = new_token();
    let dir = private_dir();
    let work = tempfile::TempDir::new().unwrap();
    let ready = work.path().join("ready");
    let agent = format!(
        "{} & wait",
        perl(&format!("{PIDFILE} sleep 300;"), &ready, &cleanup.marker)
    );
    let mut child = guardian(&run, &token, std::process::id(), dir.path(), &agent)
        .spawn()
        .unwrap();
    let agent_child = wait_pidfile(&ready);
    // when
    let args_of = |pid: i32| {
        let out = Command::new("/bin/ps")
            .args(["-ww", "-o", "args=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let guardian_args = args_of(child.id() as i32);
    let agent_args = args_of(agent_child);
    // then
    assert!(guardian_args.contains(&run), "{guardian_args}");
    assert!(!guardian_args.contains(&token));
    assert!(!agent_args.contains(&token));
    // SAFETY: signalling the guardian child this test spawned.
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    wait_child(&mut child);
    assert!(wait_dead(agent_child));
}

#[tokio::test(flavor = "current_thread")]
async fn given_guarded_pty_spawn_should_hand_agent_the_terminal_and_contain_daemon_kill() {
    // given: the daemon's real PTY spawn path, wrapped by `wrap_spawn`.
    use pu_engine::mesh_guardian::{GuardianLaunch, wrap_spawn};
    use pu_engine::pty_manager::{NativePtyHost, SpawnConfig};
    let cleanup = Cleanup::new("pty");
    let run = run_id(&cleanup);
    let token = new_token();
    let dir = private_dir();
    let work = tempfile::TempDir::new().unwrap();
    let (ign, det) = (work.path().join("ign"), work.path().join("det"));
    let agent = format!(
        "{}; read line; echo \"GOT:[$line]\"; {} & wait",
        perl(&detached_descendant(), &det, &cleanup.marker),
        perl(&ignoring_child(), &ign, &cleanup.marker)
    );
    let launch = GuardianLaunch {
        run_id: run.clone(),
        token: token.clone(),
        outcome_dir: dir.path().to_path_buf(),
    };
    let (command, args, env) = wrap_spawn(
        &launch,
        Path::new(EXE),
        std::process::id(),
        "/bin/sh".into(),
        vec!["-c".into(), agent],
    );
    let host = NativePtyHost::new();
    let handle = host
        .spawn(SpawnConfig {
            command,
            args,
            cwd: "/tmp".into(),
            env,
            env_remove: vec![],
            cols: 80,
            rows: 24,
        })
        .await
        .unwrap();
    let det_pid = wait_pidfile(&det);
    // when: input typed into the PTY reaches the agent in the foreground.
    host.write_input(&handle, b"hello\n").await.unwrap();
    let ign_pid = wait_pidfile(&ign);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !String::from_utf8_lossy(&handle.output_buffer.read_all()).contains("GOT:[hello]") {
        assert!(Instant::now() < deadline, "agent never read terminal input");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    // when: the daemon kills the run (killpg + SIGTERM its own snapshot).
    host.kill(&handle, Duration::from_secs(8)).await.unwrap();
    // then
    let outcome = wait_outcome(dir.path(), &run, &token);
    assert_eq!(outcome.trigger, Trigger::Signal);
    assert!(outcome.verified, "{outcome:?}");
    assert!(
        wait_dead(ign_pid),
        "SIGTERM-ignoring child survived daemon kill"
    );
    assert!(
        wait_dead(det_pid),
        "detached descendant survived daemon kill"
    );
    assert!(wait_dead(handle.pid as i32), "guardian survived");
}
