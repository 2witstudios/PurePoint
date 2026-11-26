//! Attach over a real ConPTY session. These drive the engine's
//! attach calls exactly as `ipc_server::streams::run_attachment` does; the
//! named-pipe framing around them is the IPC server's (Windows pipe leaf).

use std::time::Duration;

use super::{AttachmentCursor, Engine};
use crate::pty_manager::{NativePtyHost, SpawnConfig};

const SCRIPT: &str = "Write-Output pu-attach-marker; $l = Read-Host; Write-Output ('got=' + $l); \
    while ($true) { $s = $Host.UI.RawUI.WindowSize; Write-Output ('SIZE=' + $s.Width + 'x' + $s.Height); \
    Start-Sleep -Milliseconds 200 }";

/// A viewer that renders what the attach stream sends it.
struct Viewer {
    cursor: AttachmentCursor,
    parser: vt100::Parser,
    sizes: Vec<(u16, u16, Option<String>)>,
    resyncs: usize,
}

impl Viewer {
    fn screen(&self) -> String {
        self.parser.screen().contents()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn given_conpty_agent_should_stream_input_marker_and_resize_through_attach() {
    let tmp = tempfile::TempDir::new().unwrap();
    let engine = Engine::new();
    let id = "conpty-attach".to_string();
    let session = NativePtyHost::new()
        .spawn(SpawnConfig {
            command: "pwsh".into(),
            args: vec![
                "-NoProfile".into(),
                "-NoLogo".into(),
                "-Command".into(),
                SCRIPT.into(),
            ],
            cwd: tmp.path().display().to_string(),
            env: vec![],
            env_remove: vec![],
            cols: 80,
            rows: 24,
        })
        .await
        .unwrap();
    engine.sessions.lock().await.insert(id.clone(), session);

    let (buffer, pty, _exit) = engine.get_attach_handles(&id).await.unwrap();
    let (attachment, mut size_rx) = engine.register_attachment(&id, Some("viewer".into())).await;
    let (data, offset, size) = engine
        .attachment_replay(&id, &buffer, true, &mut size_rx)
        .await;
    let mut parser = vt100::Parser::new(size.1, size.0, 0);
    parser.process(&data);
    let mut viewer = Viewer {
        cursor: AttachmentCursor {
            offset,
            size,
            snapshot: true,
            identified: true,
        },
        parser,
        sizes: vec![],
        resyncs: 0,
    };

    async fn pump_until(
        engine: &Engine,
        id: &str,
        buffer: &crate::output_buffer::OutputBuffer,
        size_rx: &mut tokio::sync::broadcast::Receiver<super::PtySize>,
        viewer: &mut Viewer,
        needle: &str,
    ) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        while !viewer.screen().contains(needle) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "never saw {needle:?}; screen:\n{}",
                viewer.screen()
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
            let update = engine
                .attachment_update(id, buffer, &mut viewer.cursor, size_rx, None)
                .await;
            if update.resync {
                viewer.resyncs += 1;
                let (cols, rows, _) = viewer.cursor.size.clone();
                viewer.parser = vt100::Parser::new(rows, cols, 0);
            }
            viewer.sizes.extend(update.sizes);
            viewer.parser.process(&update.data);
        }
    }

    // Output streams to the viewer.
    pump_until(
        &engine,
        &id,
        &buffer,
        &mut size_rx,
        &mut viewer,
        "pu-attach-marker",
    )
    .await;

    // Input from the viewer reaches the agent (the attach `Input` path).
    engine
        .write_to_pty(&pty, b"ping-from-attach\r")
        .await
        .unwrap();
    pump_until(
        &engine,
        &id,
        &buffer,
        &mut size_rx,
        &mut viewer,
        "got=ping-from-attach",
    )
    .await;
    pump_until(
        &engine,
        &id,
        &buffer,
        &mut size_rx,
        &mut viewer,
        "SIZE=80x24",
    )
    .await;

    // A size claim resizes the pseudoconsole; the viewer gets the new size
    // and a resync at that geometry, and the agent sees it.
    engine
        .attachment_resize(&id, attachment, 120, 40, true)
        .await;
    assert_eq!(pty.size(), Some((120, 40)));
    assert_eq!(buffer.terminal_size(), (40, 120));
    pump_until(
        &engine,
        &id,
        &buffer,
        &mut size_rx,
        &mut viewer,
        "SIZE=120x40",
    )
    .await;
    assert!(
        viewer
            .sizes
            .contains(&(120, 40, Some("viewer".to_string()))),
        "{:?}",
        viewer.sizes
    );
    assert!(viewer.resyncs >= 1);
    assert_eq!(viewer.parser.screen().size(), (40, 120));

    engine.unregister_attachment(&id, attachment).await;
    // Dropping the session closes the pseudoconsole and the kill-on-close job.
    engine.sessions.lock().await.clear();
}
