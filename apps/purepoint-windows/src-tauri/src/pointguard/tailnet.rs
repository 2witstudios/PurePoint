//! Port of `Services/PointGuardTailnet.swift`: the app discovers this PC's tailnet IPv4 and
//! passes it to the bridge as `PI_MOBILE_HOST`, as the Mac does.
//!
//! An address is used only when a live interface holds it and the installed Tailscale CLI
//! reports exactly that one address (`tailscale ip -4`). The CLI is looked up at its fixed
//! per-machine location only (`%ProgramFiles%\Tailscale\tailscale.exe`, never `PATH`) and run
//! with `CREATE_NO_WINDOW`, an explicit minimal environment and a deadline that covers both
//! the process and its output.
//!
//! The Mac matches `utun*` interfaces; Windows adapters have no such naming, so a candidate
//! is any up, non-loopback adapter's IPv4 in Tailscale's CGNAT range (as `tailnet.js`).

use std::collections::BTreeSet;
use std::io::Read;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Tailscale's CGNAT range, 100.64.0.0/10.
pub fn tailnet_ipv4(address: Ipv4Addr) -> bool {
    let [a, b, ..] = address.octets();
    a == 100 && (64..=127).contains(&b)
}

/// One interface address that may be the tailnet's (the Swift `candidate`).
pub fn candidate(is_up: bool, is_loopback: bool, address: Ipv4Addr) -> bool {
    is_up && !is_loopback && tailnet_ipv4(address)
}

/// The CLI's single reported address, when it is one of the live candidates.
pub fn matched_address(candidates: &BTreeSet<String>, reported: &str) -> Option<String> {
    match reported.split_whitespace().collect::<Vec<_>>().as_slice() {
        [one] if candidates.contains(*one) => Some((*one).to_string()),
        _ => None,
    }
}

/// Fixed per-machine install locations of the Tailscale CLI.
pub fn tailscale_paths(program_files: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dir) = program_files.filter(|p| p.is_absolute()) {
        paths.push(dir.join("Tailscale").join("tailscale.exe"));
    }
    let fallback = PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe");
    if !paths.contains(&fallback) {
        paths.push(fallback);
    }
    paths
}

/// Confirm the installed Tailscale client agrees with a live tunnel address. Blocking and
/// bounded; callers run it off the async runtime.
pub fn address() -> Option<String> {
    let candidates: BTreeSet<String> = interface_addresses()
        .into_iter()
        .filter(|(up, loopback, ip)| candidate(*up, *loopback, *ip))
        .map(|(_, _, ip)| ip.to_string())
        .collect();
    if candidates.is_empty() {
        return None;
    }
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from);
    let executable = tailscale_paths(program_files.as_deref())
        .into_iter()
        .find(|p| p.is_file())?;
    query(
        &executable,
        &["ip", "-4"],
        &candidates,
        Duration::from_secs(3),
    )
}

/// Run the CLI and match its answer. The deadline covers the process lifetime AND pipe EOF
/// (a descendant that inherited stdout cannot hold the caller past it).
pub fn query(
    executable: &Path,
    args: &[&str],
    candidates: &BTreeSet<String>,
    timeout: Duration,
) -> Option<String> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for key in [
        "SystemRoot",
        "windir",
        "USERPROFILE",
        "LOCALAPPDATA",
        "ProgramData",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut data = Vec::new();
        let result = (&mut stdout)
            .take(4097)
            .read_to_end(&mut data)
            .map(|_| data);
        let _ = send.send(result);
    });
    let deadline = Instant::now() + timeout;
    let output = receive.recv_timeout(timeout);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => break None,
        }
    };
    let (Some(status), Ok(Ok(data))) = (status, output) else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    if !status.success() || data.len() > 4096 {
        return None;
    }
    matched_address(candidates, std::str::from_utf8(&data).ok()?)
}

/// Every adapter's IPv4 unicast addresses as (up, loopback, address).
#[cfg(windows)]
fn interface_addresses() -> Vec<(bool, bool, Ipv4Addr)> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST,
        GetAdaptersAddresses, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_IN};
    const ERROR_BUFFER_OVERFLOW: u32 = 111;
    const IF_OPER_STATUS_UP: i32 = 1;
    const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
    let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
    let mut size: u32 = 16 * 1024;
    let mut buffer: Vec<u64>;
    let mut tries = 0;
    loop {
        buffer = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: the buffer is `size` bytes, 8-byte aligned, and outlives the call.
        let rc = unsafe {
            GetAdaptersAddresses(
                u32::from(AF_INET),
                flags,
                std::ptr::null(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if rc == 0 {
            break;
        }
        tries += 1;
        if rc != ERROR_BUFFER_OVERFLOW || tries > 3 {
            return Vec::new();
        }
    }
    let mut out = Vec::new();
    let mut adapter = buffer.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    // SAFETY: GetAdaptersAddresses filled a linked list inside `buffer`, which is alive.
    unsafe {
        while let Some(current) = adapter.as_ref() {
            let up = current.OperStatus == IF_OPER_STATUS_UP;
            let loopback = current.IfType == IF_TYPE_SOFTWARE_LOOPBACK;
            let mut unicast = current.FirstUnicastAddress;
            while let Some(address) = unicast.as_ref() {
                let sockaddr = address.Address.lpSockaddr;
                if !sockaddr.is_null() && (*sockaddr).sa_family == AF_INET {
                    let v4 = &*sockaddr.cast::<SOCKADDR_IN>();
                    let raw = v4.sin_addr.S_un.S_addr;
                    out.push((up, loopback, Ipv4Addr::from(u32::from_be(raw))));
                }
                unicast = address.Next;
            }
            adapter = current.Next;
        }
    }
    out
}

#[cfg(not(windows))]
fn interface_addresses() -> Vec<(bool, bool, Ipv4Addr)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn given_interfaces_should_accept_only_live_tailnet_ipv4() {
        let ip = |s: &str| s.parse::<Ipv4Addr>().unwrap();
        assert!(candidate(true, false, ip("100.64.0.1")));
        assert!(candidate(true, false, ip("100.127.255.254")));
        assert!(!candidate(false, false, ip("100.100.1.1")));
        assert!(!candidate(true, true, ip("100.100.1.1")));
        assert!(!candidate(true, false, ip("100.128.0.1")));
        assert!(!candidate(true, false, ip("100.63.0.1")));
        assert!(!candidate(true, false, ip("192.168.1.2")));
    }

    #[test]
    fn given_cli_output_should_match_exactly_one_candidate() {
        let candidates = set(&["100.100.1.2"]);
        assert_eq!(
            matched_address(&candidates, "100.100.1.2\r\n"),
            Some("100.100.1.2".into())
        );
        assert_eq!(matched_address(&candidates, "100.100.1.3\n"), None);
        assert_eq!(
            matched_address(&candidates, "100.100.1.2\n100.100.1.2"),
            None
        );
        assert_eq!(matched_address(&candidates, ""), None);
    }

    #[test]
    fn given_program_files_should_look_only_at_fixed_paths() {
        assert_eq!(
            tailscale_paths(Some(Path::new(r"D:\Apps"))),
            vec![
                PathBuf::from(r"D:\Apps\Tailscale\tailscale.exe"),
                PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe")
            ]
        );
        assert_eq!(
            tailscale_paths(Some(Path::new("relative"))),
            vec![PathBuf::from(r"C:\Program Files\Tailscale\tailscale.exe")]
        );
    }

    #[cfg(windows)]
    fn cmd() -> PathBuf {
        PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into()))
            .join(r"System32\cmd.exe")
    }

    #[cfg(windows)]
    #[test]
    fn given_a_cli_should_read_its_answer_without_a_window() {
        let candidates = set(&["100.100.1.2"]);
        let timeout = Duration::from_secs(5);
        assert_eq!(
            query(&cmd(), &["/c", "echo 100.100.1.2"], &candidates, timeout),
            Some("100.100.1.2".into())
        );
        assert_eq!(
            query(
                &cmd(),
                &["/c", "echo 100.100.1.2& exit 1"],
                &candidates,
                timeout
            ),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn given_a_hung_cli_should_give_up_at_the_deadline() {
        let started = Instant::now();
        let candidates = set(&["100.100.1.2"]);
        assert_eq!(
            query(
                &cmd(),
                &["/c", "ping -n 30 127.0.0.1 >nul"],
                &candidates,
                Duration::from_millis(500)
            ),
            None
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[cfg(windows)]
    #[test]
    fn given_this_pc_should_enumerate_adapters() {
        // Loopback is always present; the call itself must not fail.
        let _ = interface_addresses();
    }
}
