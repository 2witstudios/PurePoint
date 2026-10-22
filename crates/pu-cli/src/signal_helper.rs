//! Fail-open observer path; runs before CLI routing, plugin updates or daemon discovery.
use std::io;
use std::os::unix::net::UnixDatagram;
use std::time::{Duration, Instant};

const MAX_INPUT: usize = 256 * 1024;

extern "C" fn deadline_exit(_: libc::c_int) {
    // Async-signal-safe escape even if a platform socket syscall stalls.
    unsafe { libc::_exit(0) }
}

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
    let socket = UnixDatagram::unbound()?;
    socket.set_nonblocking(true)?;
    let path = std::env::var_os("PU_SIGNAL_SOCKET").ok_or(io::ErrorKind::NotFound)?;
    socket.connect(path)?;
    socket.send(&data)?;
    Ok(())
}

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
