//! Prompt delivery against a slow-rendering fake Claude TUI.

use std::time::Duration;

use pu_engine::delivery::{DeliveryOptions, deliver_prompt};
use pu_engine::pty_manager::{AgentHandle, NativePtyHost, SpawnConfig};

const LONG_PROMPT: &str = "Run: pagespace pages read abc123 and follow it exactly. Then report back with a summary of every file you touched.";

struct Fake {
    host: NativePtyHost,
    handle: AgentHandle,
    log: std::path::PathBuf,
    _tmp: tempfile::TempDir,
}

async fn spawn_fake(env: &[(&str, &str)]) -> Fake {
    let tmp = tempfile::TempDir::new().unwrap();
    let log = tmp.path().join("submits.jsonl");
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fake_tui.py");
    let mut env: Vec<(String, String)> = env
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    env.push(("SUBMIT_LOG".into(), log.display().to_string()));
    env.push(("PYTHONUNBUFFERED".into(), "1".into()));
    let host = NativePtyHost::new();
    let handle = host
        .spawn(SpawnConfig {
            command: "python3".into(),
            args: vec![script.into()],
            cwd: tmp.path().display().to_string(),
            env,
            env_remove: vec![],
            cols: 80,
            rows: 24,
        })
        .await
        .unwrap();
    Fake {
        host,
        handle,
        log,
        _tmp: tmp,
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        // The PTY reader is a blocking read; without killing the child the
        // runtime never shuts down.
        unsafe { libc::kill(self.handle.pid as i32, libc::SIGKILL) };
    }
}

impl Fake {
    fn submits(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }
}

async fn deliver(f: &Fake, text: &str) -> Result<(), String> {
    let opts = DeliveryOptions {
        ready_timeout: Duration::from_secs(10),
        type_stall: Duration::from_millis(800),
        submit_retry_after: Duration::from_millis(800),
        submit_timeout: Duration::from_secs(3),
        ..DeliveryOptions::default()
    };
    deliver_prompt(
        &f.host,
        &f.handle.master_fd(),
        &f.handle.output_buffer,
        text,
        &opts,
    )
    .await
    .map_err(|e| e.to_string())
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(1500)).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn given_agent_still_starting_should_deliver_full_prompt_once() {
    // given
    let f = spawn_fake(&[("READY_DELAY_MS", "1500")]).await;
    // when
    let r = deliver(&f, LONG_PROMPT).await;
    settle().await;
    // then
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(f.submits(), vec![LONG_PROMPT.to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn given_slow_input_processing_should_not_submit_a_fragment() {
    // given
    let f = spawn_fake(&[("PROCESS_DELAY_MS", "25")]).await;
    // when
    let r = deliver(&f, LONG_PROMPT).await;
    settle().await;
    // then
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(f.submits(), vec![LONG_PROMPT.to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn given_swallowed_enter_should_still_submit_once() {
    // given
    let f = spawn_fake(&[("SWALLOW_ENTER", "1")]).await;
    // when
    let r = deliver(&f, LONG_PROMPT).await;
    settle().await;
    // then
    assert!(r.is_ok(), "{r:?}");
    assert_eq!(f.submits(), vec![LONG_PROMPT.to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn given_healthy_agent_should_deliver_full_prompt_once() {
    // given
    let f = spawn_fake(&[]).await;
    // when
    let r = deliver(&f, LONG_PROMPT).await;
    settle().await;
    // then
    assert!(r.is_ok());
    assert_eq!(f.submits(), vec![LONG_PROMPT.to_string()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn given_input_box_that_never_appears_should_fail_loudly() {
    // given
    let f = spawn_fake(&[("READY_DELAY_MS", "60000")]).await;
    let opts = DeliveryOptions {
        ready_timeout: Duration::from_millis(600),
        ..DeliveryOptions::default()
    };
    // when
    let r = deliver_prompt(
        &f.host,
        &f.handle.master_fd(),
        &f.handle.output_buffer,
        LONG_PROMPT,
        &opts,
    )
    .await;
    // then
    assert!(
        matches!(r, Err(pu_engine::delivery::DeliveryError::NotReady)),
        "{r:?}"
    );
    assert!(f.submits().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn given_enter_that_never_works_should_fail_without_double_submit() {
    // given
    let f = spawn_fake(&[("SWALLOW_ENTER", "99")]).await;
    // when
    let r = deliver(&f, LONG_PROMPT).await;
    // then
    assert!(r.is_err());
    assert!(f.submits().is_empty());
}
