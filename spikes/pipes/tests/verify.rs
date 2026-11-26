//! Criterion 7: client-side server verification and impostors.
#![cfg(windows)]
mod common;

use std::io::BufRead;
use std::process::{Command, Stdio};

use common::*;
use pipes_spike::client::{Refusal, ServerPolicy, verify_server};
use pipes_spike::server::{Config, Server};
use pipes_spike::win::Sid;

#[tokio::test(flavor = "multi_thread")]
async fn given_genuine_daemon_should_pass_sid_pid_and_image() {
    let name = unique("genuine");
    let (_stats, _s) = start(Config::new(&name));
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("daemon.sock.pid");
    std::fs::write(&pid_file, std::process::id().to_string()).unwrap();

    let client = connect(&name).await;
    let mut policy = ServerPolicy::current_user()
        .unwrap()
        .with_pid_file(&pid_file)
        .unwrap();
    policy.expected_image = Some(std::env::current_exe().unwrap());
    let id = verify_server(&client, &policy).expect("genuine daemon");
    println!("server identity: {id:?}");
    assert_eq!(id.pid, std::process::id());
}

#[tokio::test(flavor = "multi_thread")]
async fn given_server_token_sid_differs_should_refuse() {
    let name = unique("othersid");
    let (_stats, _s) = start(Config::new(&name));
    let client = connect(&name).await;
    let mut policy = ServerPolicy::current_user().unwrap();
    // Simulated other-user squatter: the caller expects LocalService.
    policy.expected_sid = Sid::parse("S-1-5-19").unwrap();
    let refusal = verify_server(&client, &policy).unwrap_err();
    println!("refused: {refusal:?}");
    assert!(matches!(refusal, Refusal::Sid { .. }));
}

/// A real second process, same user, squatting the name first.
#[tokio::test(flavor = "multi_thread")]
async fn given_same_user_impostor_sid_check_passes_but_pid_and_image_refuse() {
    let name = unique("impostor");
    let mut impostor = Command::new(exe())
        .args(["serve", &name])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(impostor.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.starts_with("READY"), "{line}");

    // The real daemon arriving later cannot take the name.
    let err = Server::bind(Config::new(&name)).err().expect("squatted");
    println!("daemon bind on squatted name: {err}");
    assert_eq!(err.raw_os_error(), Some(5));

    let client = connect(&name).await;
    let sid_only = ServerPolicy::current_user().unwrap();
    let id = verify_server(&client, &sid_only).expect("same SID passes");
    println!("SID-only accepts impostor: {id:?}");
    assert_eq!(id.pid, impostor.id());

    // daemon.sock.pid records the real daemon (here: this test process).
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("daemon.sock.pid");
    std::fs::write(&pid_file, std::process::id().to_string()).unwrap();
    let with_pid = sid_only.clone().with_pid_file(&pid_file).unwrap();
    let refusal = verify_server(&client, &with_pid).unwrap_err();
    println!("PID check refuses: {refusal:?}");
    assert!(matches!(refusal, Refusal::Pid { .. }));

    let mut with_image = sid_only.clone();
    with_image.expected_image = Some(std::env::current_exe().unwrap());
    let refusal = verify_server(&client, &with_image).unwrap_err();
    println!("image check refuses: {refusal:?}");
    assert!(matches!(refusal, Refusal::Image { .. }));

    impostor.kill().unwrap();
    let _ = impostor.wait();
}
