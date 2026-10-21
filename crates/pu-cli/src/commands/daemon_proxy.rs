//! `pu daemon proxy`: one daemon connection over stdin/stdout, for SSH exec
//! channels (the iPad cannot forward a Unix socket). Stdout carries only
//! daemon bytes; diagnostics go to stderr. Never starts a daemon (MACH-003).

use std::path::Path;

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio::net::UnixStream;

use crate::error::CliError;

pub async fn run(socket: &Path) -> Result<(), CliError> {
    let stream = connect(socket).await?;
    relay(stream, tokio::io::stdin(), tokio::io::stdout()).await
}

async fn connect(socket: &Path) -> Result<UnixStream, CliError> {
    UnixStream::connect(socket).await.map_err(|e| {
        CliError::Other(format!(
            "cannot connect to the daemon at {} ({e}); start it with `pu daemon start`",
            socket.display()
        ))
    })
}

/// Copy `input` to the daemon and the daemon to `output` until the daemon
/// closes. Input EOF half-closes the socket so the daemon still finishes its
/// replies; the daemon closing ends the relay even while input stays open.
pub async fn relay<R, W>(stream: UnixStream, input: R, output: W) -> Result<(), CliError>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin,
{
    let (mut from_daemon, mut to_daemon) = stream.into_split();
    let upload = tokio::spawn(async move {
        let mut input = input;
        tokio::io::copy(&mut input, &mut to_daemon).await?;
        to_daemon.shutdown().await
    });
    let mut output = output;
    let download = tokio::io::copy(&mut from_daemon, &mut output).await;
    let flushed = output.flush().await;
    // Still blocked on input after the daemon hung up: nothing left to deliver.
    upload.abort();
    if let Ok(Err(e)) = upload.await
        && e.kind() != std::io::ErrorKind::BrokenPipe
    {
        eprintln!("pu daemon proxy: sending to the daemon failed: {e}");
    }
    download?;
    flushed?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;

    /// Echoes everything it reads, then closes once its peer half-closes.
    fn echo_server(listener: UnixListener) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (mut reader, mut writer) = stream.into_split();
            tokio::io::copy(&mut reader, &mut writer).await.unwrap();
            writer.shutdown().await.unwrap();
        })
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_echo_daemon_should_relay_bytes_unchanged_both_ways() {
        // given
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("d.sock");
        let server = echo_server(UnixListener::bind(&socket).unwrap());
        let payload: Vec<u8> = (0..=255u8).cycle().take(256 * 1024).collect();
        let (mut stdin_tx, stdin_rx) = tokio::io::duplex(4096);
        let (stdout_tx, mut stdout_rx) = tokio::io::duplex(4096);
        let sent = payload.clone();
        tokio::spawn(async move {
            stdin_tx.write_all(&sent).await.unwrap();
            // dropping closes stdin
        });
        let reader = tokio::spawn(async move {
            let mut out = Vec::new();
            stdout_rx.read_to_end(&mut out).await.unwrap();
            out
        });

        // when
        let stream = connect(&socket).await.unwrap();
        relay(stream, stdin_rx, stdout_tx).await.unwrap();

        // then
        assert_eq!(reader.await.unwrap(), payload);
        server.await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_stdin_eof_should_half_close_and_keep_draining_replies() {
        // given: a daemon that replies only after its peer half-closes
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("d.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            stream.read_to_end(&mut request).await.unwrap();
            stream.write_all(b"late reply\n").await.unwrap();
            request
        });
        let (stdout_tx, mut stdout_rx) = tokio::io::duplex(4096);

        // when
        let stream = connect(&socket).await.unwrap();
        relay(stream, &b"request\n"[..], stdout_tx).await.unwrap();

        // then
        let mut out = Vec::new();
        stdout_rx.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, b"late reply\n");
        assert_eq!(server.await.unwrap(), b"request\n");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_daemon_closes_should_finish_while_stdin_stays_open() {
        // given: a daemon that answers one line and hangs up
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("d.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            stream.write_all(b"bye\n").await.unwrap();
        });
        let (_stdin_tx, stdin_rx) = tokio::io::duplex(64); // never closed
        let (stdout_tx, mut stdout_rx) = tokio::io::duplex(64);

        // when
        let stream = connect(&socket).await.unwrap();
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            relay(stream, stdin_rx, stdout_tx),
        )
        .await;

        // then
        assert!(
            matches!(result, Ok(Ok(()))),
            "relay did not end: {result:?}"
        );
        let mut out = Vec::new();
        stdout_rx.read_to_end(&mut out).await.unwrap();
        assert_eq!(out, b"bye\n");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn given_missing_socket_should_fail_naming_socket_and_start_hint() {
        // given
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("missing.sock");

        // when
        let result = connect(&socket).await;

        // then
        match result {
            Err(CliError::Other(message)) => {
                assert!(message.contains(&socket.display().to_string()), "{message}");
                assert!(message.contains("pu daemon start"), "{message}");
            }
            other => panic!("expected connect failure, got {other:?}"),
        }
    }
}
