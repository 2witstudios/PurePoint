//! Fail-open observer path; runs before CLI routing, plugin updates or daemon discovery.
use std::io;
#[cfg(unix)]
use std::os::unix::net::UnixDatagram;
use std::time::{Duration, Instant};

const MAX_INPUT: usize = 256 * 1024;

#[cfg(unix)]
extern "C" fn deadline_exit(_: libc::c_int) {
    // Async-signal-safe escape even if a platform socket syscall stalls.
    unsafe { libc::_exit(0) }
}

/// Windows has no `SIGALRM`/`setitimer`: a watchdog thread ends the process
/// at the same 100ms deadline, with the same (success) exit status.
#[cfg(windows)]
pub fn run() {
    let spawned = std::thread::Builder::new().spawn(|| {
        std::thread::sleep(Duration::from_millis(100));
        std::process::exit(0);
    });
    if spawned.is_err() {
        return;
    }
    let _ = relay();
}

#[cfg(unix)]
pub fn run() {
    unsafe {
        if libc::signal(libc::SIGALRM, deadline_exit as libc::sighandler_t) == libc::SIG_ERR {
            return;
        }
        let timer = libc::itimerval {
            it_interval: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: libc::timeval {
                tv_sec: 0,
                tv_usec: 100_000,
            },
        };
        if libc::setitimer(libc::ITIMER_REAL, &timer, std::ptr::null_mut()) != 0 {
            return;
        }
    }
    let _ = relay();
}

fn relay() -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_millis(50);
    let input = bounded_stdin(deadline)?;
    let mut payload: serde_json::Value = serde_json::from_slice(&input)?;
    if let Some(object) = payload.as_object_mut() {
        object
            .entry("occurred_at_ms")
            .or_insert_with(|| chrono::Utc::now().timestamp_millis().into());
    }
    let frame = serde_json::json!({
        "token": std::env::var("PU_SIGNAL_TOKEN").unwrap_or_default(),
        "payload": payload,
    });
    let data = serde_json::to_vec(&frame)?;
    if data.len() > MAX_INPUT || Instant::now() >= deadline {
        return Ok(());
    }
    send(&data, deadline)
}

#[cfg(unix)]
fn send(data: &[u8], _deadline: Instant) -> io::Result<()> {
    let socket = UnixDatagram::unbound()?;
    socket.set_nonblocking(true)?;
    let path = std::env::var_os("PU_SIGNAL_SOCKET").ok_or(io::ErrorKind::NotFound)?;
    socket.connect(path)?;
    socket.send(data)?;
    Ok(())
}

/// One frame per pipe connection; a busy pipe is waited out only until the
/// stdin deadline, so a full listener drops the frame as a full datagram
/// socket does.
#[cfg(windows)]
fn send(data: &[u8], deadline: Instant) -> io::Result<()> {
    let name = std::env::var("PU_SIGNAL_SOCKET").map_err(|_| io::ErrorKind::NotFound)?;
    if !name.starts_with(r"\\.\pipe\") {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    pu_transport::signal::send_frame(&name, data, deadline)
}

#[cfg(unix)]
fn bounded_stdin(deadline: Instant) -> io::Result<Vec<u8>> {
    let mut data = Vec::new();
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut poll = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut poll, 1, remaining.as_millis().max(1) as i32) };
        if ready <= 0 {
            return Err(io::ErrorKind::TimedOut.into());
        }
        let mut chunk = [0_u8; 8192];
        let count = unsafe { libc::read(0, chunk.as_mut_ptr().cast(), chunk.len()) };
        if count == 0 {
            return Ok(data);
        }
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        if data.len() + count as usize > MAX_INPUT {
            return Err(io::ErrorKind::InvalidData.into());
        }
        data.extend_from_slice(&chunk[..count as usize]);
    }
}

/// `std::io::stdin` read on a thread (Windows has no `poll` on a console or
/// pipe handle); the caller gives up at `deadline` and the watchdog ends the
/// process with the reader still blocked.
#[cfg(windows)]
fn bounded_stdin(deadline: Instant) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new().spawn(move || {
        let mut data = Vec::new();
        let result = io::stdin()
            .lock()
            .take(MAX_INPUT as u64 + 1)
            .read_to_end(&mut data)
            .map(|_| data);
        let _ = tx.send(result);
    })?;
    let remaining = deadline.saturating_duration_since(Instant::now());
    let data = rx
        .recv_timeout(remaining)
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
    if data.len() > MAX_INPUT {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(data)
}
