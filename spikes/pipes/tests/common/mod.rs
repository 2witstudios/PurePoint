#![allow(dead_code)]
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pipes_spike::server::{Config, Server, Stats};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::sync::Notify;

pub fn unique(tag: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!(
        r"\\.\pipe\purepoint-spike-{}-{tag}-{nanos}",
        std::process::id()
    )
}

pub fn start(cfg: Config) -> (Arc<Stats>, Arc<Notify>) {
    let server = Server::bind(cfg).expect("bind");
    let stats = server.stats.clone();
    let shutdown = Arc::new(Notify::new());
    tokio::spawn(server.run(shutdown.clone()));
    (stats, shutdown)
}

pub async fn connect(name: &str) -> NamedPipeClient {
    pipes_spike::open_client(name, Duration::from_secs(30))
        .await
        .expect("open client")
}

/// Writes one request line (ignoring a write failure) and reads one line.
/// `None` = EOF/error/timeout.
pub async fn request(
    client: &mut NamedPipeClient,
    line: &str,
    wait: Duration,
) -> Option<serde_json::Value> {
    let _ = client.write_all(format!("{line}\n").as_bytes()).await;
    read_line(client, wait).await
}

pub async fn read_line(client: &mut NamedPipeClient, wait: Duration) -> Option<serde_json::Value> {
    let mut reader = BufReader::new(client);
    let mut out = String::new();
    match tokio::time::timeout(wait, reader.read_line(&mut out)).await {
        Ok(Ok(n)) if n > 0 => serde_json::from_str(&out).ok(),
        _ => None,
    }
}

pub fn exe() -> &'static str {
    env!("CARGO_BIN_EXE_pipes-spike")
}

pub fn node_script(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("node")
        .join(name)
}
