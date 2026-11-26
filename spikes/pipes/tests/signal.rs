//! Criterion 6: per-launch token-guarded signal pipe, Rust helper + Node writer.
#![cfg(windows)]
mod common;

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

use common::*;
use pipes_spike::signal::{Delivery, SignalPipe, send_frame};
use tokio::sync::mpsc;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

fn start_signal(tag: &str) -> (String, mpsc::Receiver<Delivery>) {
    let name = unique(&format!("signal-{tag}"));
    let pipe = SignalPipe::bind(&name, TOKEN).unwrap();
    let (tx, rx) = mpsc::channel(256);
    tokio::spawn(pipe.run(tx));
    (name, rx)
}

async fn next(rx: &mut mpsc::Receiver<Delivery>) -> Delivery {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("delivery")
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn given_rust_helper_should_deliver_one_message() {
    let (name, mut rx) = start_signal("rust");
    let name2 = name.clone();
    let status = tokio::task::spawn_blocking(move || {
        let mut child = Command::new(exe())
            .args(["signal-send", &name2])
            .env("PU_SIGNAL_TOKEN", TOKEN)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(br#"{"type":"agent_start","sequence":1}"#)
            .unwrap();
        child.wait().unwrap()
    })
    .await
    .unwrap();
    assert!(status.success());
    match next(&mut rx).await {
        Delivery::Accepted(p) => {
            println!("rust helper delivered: {p}");
            assert_eq!(p["type"], "agent_start");
        }
        d => panic!("{d:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn given_node_writer_should_deliver_one_message() {
    let (name, mut rx) = start_signal("node");
    let out = tokio::process::Command::new("node")
        .arg(node_script("signal-writer.mjs"))
        .arg(r#"{"type":"agent_settled","sequence":2}"#)
        .env("PU_SIGNAL_PIPE", &name)
        .env("PU_SIGNAL_TOKEN", TOKEN)
        .output()
        .await
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    match next(&mut rx).await {
        Delivery::Accepted(p) => {
            println!("node writer delivered: {p}");
            assert_eq!(p["type"], "agent_settled");
            assert!(p["occurred_at_ms"].is_u64());
        }
        d => panic!("{d:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn given_wrong_token_should_reject() {
    let (name, mut rx) = start_signal("badtoken");
    let frame = br#"{"token":"0123456789abcdef0123456789abcdeX","payload":{"type":"x"}}"#;
    let n = name.clone();
    tokio::task::spawn_blocking(move || send_frame(&n, frame, Duration::from_millis(50)))
        .await
        .unwrap()
        .unwrap();
    match next(&mut rx).await {
        Delivery::Rejected(r) => assert_eq!(r, "bad token"),
        d => panic!("{d:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn given_concurrent_writers_each_message_should_arrive_once() {
    let (name, mut rx) = start_signal("burst");
    let mut handles = Vec::new();
    for i in 0..32 {
        let n = name.clone();
        handles.push(tokio::task::spawn_blocking(move || {
            let frame = format!(r#"{{"token":"{TOKEN}","payload":{{"sequence":{i}}}}}"#);
            send_frame(&n, frame.as_bytes(), Duration::from_millis(500))
        }));
    }
    for h in handles {
        h.await.unwrap().unwrap();
    }
    let mut seen = Vec::new();
    for _ in 0..32 {
        match next(&mut rx).await {
            Delivery::Accepted(p) => seen.push(p["sequence"].as_u64().unwrap()),
            d => panic!("{d:?}"),
        }
    }
    seen.sort();
    assert_eq!(seen, (0..32).collect::<Vec<_>>());
}
