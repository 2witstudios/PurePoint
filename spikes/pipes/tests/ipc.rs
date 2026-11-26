//! Criteria 1-5: private DACL + first instance, client PID/SID check,
//! PeekNamedPipe disconnects, 1024 cap + BUSY, Node round-trip.
#![cfg(windows)]
mod common;

use std::os::windows::io::AsRawHandle;
use std::sync::atomic::Ordering::SeqCst;
use std::time::{Duration, Instant};

use common::*;
use pipes_spike::server::Config;
use pipes_spike::win::{self, PipeSecurity, Sid};
use tokio::net::windows::named_pipe::ClientOptions;

const SEC: Duration = Duration::from_secs(5);

#[tokio::test(flavor = "multi_thread")]
async fn given_private_pipe_should_grant_only_user_and_system_and_own_the_name() {
    let name = unique("dacl");
    let (_stats, _s) = start(Config::new(&name));
    let client = connect(&name).await;
    let sddl = win::handle_sddl(client.as_raw_handle()).unwrap();
    let me = win::current_user_sid().unwrap().to_string();
    println!("pipe SDDL: {sddl}");
    assert_eq!(sddl, format!("O:{me}D:P(A;;FA;;;{me})(A;;FA;;;SY)"));

    // FILE_FLAG_FIRST_PIPE_INSTANCE: a second owner of the name is refused.
    let err = pipes_spike::server::Server::bind(Config::new(&name))
        .err()
        .expect("second first-instance must fail");
    println!("second first-instance: {err}");
    assert_eq!(err.raw_os_error(), Some(5));

    // The DACL is enforced: a pipe granting only SYSTEM refuses us.
    let sys_only = unique("sysonly");
    let sec = PipeSecurity::from_sddl("D:P(A;;GA;;;SY)").unwrap();
    let _server = sec.create(&sys_only, true).unwrap();
    let err = ClientOptions::new().open(&sys_only).unwrap_err();
    println!("open SYSTEM-only pipe: {err}");
    assert_eq!(err.raw_os_error(), Some(5));
}

#[tokio::test(flavor = "multi_thread")]
async fn given_same_user_client_should_read_pid_and_accept() {
    let name = unique("pid");
    let (stats, _s) = start(Config::new(&name));
    let mut client = connect(&name).await;
    let resp = request(&mut client, r#"{"type":"health"}"#, SEC)
        .await
        .unwrap();
    assert_eq!(resp["type"], "health_report");
    assert_eq!(stats.last_client_pid.load(SeqCst), std::process::id());
    assert_eq!(stats.rejected_sid.load(SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn given_client_token_sid_differs_should_reject() {
    let name = unique("sid");
    let mut cfg = Config::new(&name);
    // Injected: pretend the daemon runs as LocalSystem.
    cfg.expected_client_sid = Some(Sid::parse("S-1-5-18").unwrap());
    let (stats, _s) = start(cfg);
    let mut client = connect(&name).await;
    let resp = request(&mut client, r#"{"type":"health"}"#, SEC).await;
    assert!(
        resp.is_none(),
        "rejected client must get no reply: {resp:?}"
    );
    assert_eq!(stats.rejected_sid.load(SeqCst), 1);
    assert_eq!(stats.accepted.load(SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn given_client_vanishes_mid_operation_should_detect_via_peek() {
    let name = unique("peek");
    let (stats, _s) = start(Config::new(&name));
    let mut client = connect(&name).await;
    use tokio::io::AsyncWriteExt;
    client
        .write_all(b"{\"type\":\"slow\",\"ms\":10000}\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let dropped = Instant::now();
    drop(client);
    while stats.disconnects_detected.load(SeqCst) == 0 {
        assert!(dropped.elapsed() < Duration::from_secs(2), "not detected");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    println!("disconnect detected after {:?}", dropped.elapsed());
}

#[tokio::test(flavor = "multi_thread")]
async fn given_bytes_arrive_during_operation_peek_should_report_them_unread() {
    let name = unique("pending");
    let (_stats, _s) = start(Config::new(&name));
    let mut client = connect(&name).await;
    use tokio::io::AsyncWriteExt;
    client
        .write_all(b"{\"type\":\"peek\",\"ms\":300}\n")
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    client.write_all(b"{\"type\":\"health\"}\n").await.unwrap();
    let resp = read_line(&mut client, SEC).await.unwrap();
    println!("peek while bytes pending: {resp}");
    // Records whether the bytes stayed in the kernel buffer (Pending) or
    // tokio/mio's prefetching read already took them (Idle).
    let state = resp["state"].as_str().unwrap().to_owned();
    assert!(state.starts_with("Pending") || state == "Idle", "{state}");
    let idle = request(&mut client, r#"{"type":"peek"}"#, SEC).await;
    println!("follow-up: {idle:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn given_1100_concurrent_clients_should_accept_1024_and_busy_the_rest() {
    let name = unique("busy");
    let (stats, _s) = start(Config::new(&name));
    let started = Instant::now();
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..1100 {
        let name = name.clone();
        tasks.spawn(async move {
            let mut c = connect(&name).await;
            let resp = request(&mut c, r#"{"type":"health"}"#, Duration::from_secs(20)).await;
            (c, resp) // keep the client alive (and its permit held)
        });
    }
    let mut held = Vec::new();
    let (mut ok, mut busy, mut other) = (0, 0, 0);
    while let Some(r) = tasks.join_next().await {
        let (c, resp) = r.unwrap();
        match resp {
            Some(v) if v["type"] == "health_report" => ok += 1,
            Some(v) if v["code"] == "BUSY" => busy += 1,
            v => {
                other += 1;
                eprintln!("unexpected: {v:?}");
            }
        }
        held.push(c);
    }
    println!(
        "1100 clients in {:?}: accepted={ok} busy={busy} other={other}; server stats {stats:?}",
        started.elapsed()
    );
    assert_eq!((ok, busy, other), (1024, 76, 0));
    assert_eq!(stats.accepted.load(SeqCst), 1024);
    assert_eq!(stats.busy.load(SeqCst), 76);
    assert_eq!(stats.busy_write_failed.load(SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn given_node_client_should_round_trip_health() {
    let name = unique("node");
    let (_stats, _s) = start(Config::new(&name));
    let out = tokio::process::Command::new("node")
        .arg(node_script("health.mjs"))
        .arg(&name)
        .output()
        .await
        .expect("node on PATH");
    let stdout = String::from_utf8_lossy(&out.stdout);
    println!(
        "node stdout: {stdout} stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(v["type"], "health_report");
    assert_eq!(v["pid"], std::process::id());
}
