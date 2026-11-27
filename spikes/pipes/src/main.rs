//! `pipes-spike serve <pipe>`        NDJSON server; prints `READY <pid>`.
//! `pipes-spike signal-send <pipe>`  Rust signal helper: stdin payload +
//!                                   PU_SIGNAL_TOKEN -> one frame.
//! `pipes-spike name <home>`         normalized ~/.pu and daemon pipe name.
#[cfg(windows)]
fn main() {
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::time::Duration;

    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("serve") => {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let server =
                    pipes_spike::server::Server::bind(pipes_spike::server::Config::new(&args[2]))
                        .expect("bind");
                println!("READY {}", std::process::id());
                std::io::stdout().flush().unwrap();
                server.run(Arc::default()).await.unwrap();
            });
        }
        Some("signal-send") => {
            // Fail-closed for the spike so tests see errors; the real helper
            // stays fail-open (exit 0) behind a 100 ms watchdog thread.
            let mut input = Vec::new();
            std::io::stdin()
                .take(pipes_spike::signal::MAX_SIGNAL as u64)
                .read_to_end(&mut input)
                .unwrap();
            let payload: serde_json::Value = serde_json::from_slice(&input).unwrap();
            let frame = serde_json::json!({
                "token": std::env::var("PU_SIGNAL_TOKEN").unwrap_or_default(),
                "payload": payload,
            });
            let data = serde_json::to_vec(&frame).unwrap();
            if let Err(e) =
                pipes_spike::signal::send_frame(&args[2], &data, Duration::from_millis(50))
            {
                eprintln!("send failed: {e}");
                std::process::exit(1);
            }
        }
        Some("name") => {
            let home = std::path::Path::new(&args[2]);
            let n = pipes_spike::naming::normalize_pu_dir(home).unwrap();
            println!("{n}\n{}", pipes_spike::naming::pipe_name(&n, "daemon"));
        }
        _ => {
            eprintln!("usage: pipes-spike serve|signal-send|name <arg>");
            std::process::exit(2);
        }
    }
}

#[cfg(not(windows))]
fn main() {}
