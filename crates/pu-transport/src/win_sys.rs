//! Owned wrappers over the Win32 security and pipe calls the transport needs
//! (from the WIN-1.2 spike).
#![allow(dead_code)]
use std::ffi::c_void;
use std::io;
use std::os::windows::io::{FromRawHandle, OwnedHandle, RawHandle};
use std::path::PathBuf;
use std::ptr::null_mut;

use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::Foundation::{HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW, GetSecurityInfo,
    SDDL_REVISION_1, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    DACL_SECURITY_INFORMATION, GetLengthSid, GetTokenInformation, OWNER_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Pipes::{
    GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, PeekNamedPipe,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn check(ok: i32) -> io::Result<()> {
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

unsafe fn take_wide(p: *mut u16) -> String {
    let mut len = 0;
    while unsafe { *p.add(len) } != 0 {
        len += 1;
    }
    let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) });
    unsafe { LocalFree(p.cast()) };
    s
}

/// A SID copied into owned memory. Binary SIDs are canonical, so byte
/// equality is `EqualSid`.
#[derive(Clone, PartialEq, Eq)]
pub struct Sid(Vec<u8>);

impl Sid {
    pub fn parse(text: &str) -> io::Result<Sid> {
        let w = wide(text);
        let mut p: PSID = null_mut();
        check(unsafe { ConvertStringSidToSidW(w.as_ptr(), &mut p) })?;
        let sid = unsafe { Sid::copy_from(p) };
        unsafe { LocalFree(p) };
        Ok(sid)
    }

    unsafe fn copy_from(p: PSID) -> Sid {
        let len = unsafe { GetLengthSid(p) } as usize;
        Sid(unsafe { std::slice::from_raw_parts(p as *const u8, len) }.to_vec())
    }

    fn as_psid(&self) -> PSID {
        self.0.as_ptr() as PSID
    }
}

impl std::fmt::Display for Sid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = null_mut();
        if unsafe { ConvertSidToStringSidW(self.as_psid(), &mut out) } == 0 {
            return f.write_str("<invalid sid>");
        }
        f.write_str(&unsafe { take_wide(out) })
    }
}

impl std::fmt::Debug for Sid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Sid({self})")
    }
}

fn token_user(token: HANDLE) -> io::Result<Sid> {
    let mut len = 0u32;
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len) };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    // u64 backing keeps TOKEN_USER suitably aligned.
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    check(unsafe {
        GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len)
    })?;
    let user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
    Ok(unsafe { Sid::copy_from(user.User.Sid) })
}

/// Fixed-size token information (`TokenIsAppContainer` and the like).
fn token_u32(token: HANDLE, class: i32) -> io::Result<u32> {
    let mut value = 0u32;
    let mut len = 0u32;
    check(unsafe {
        GetTokenInformation(
            token,
            class,
            (&mut value as *mut u32).cast(),
            size_of::<u32>() as u32,
            &mut len,
        )
    })?;
    Ok(value)
}

/// The mandatory integrity RID of a token (`SECURITY_MANDATORY_*_RID`).
fn token_integrity_rid(token: HANDLE) -> io::Result<u32> {
    use windows_sys::Win32::Security::{
        GetSidSubAuthority, GetSidSubAuthorityCount, TOKEN_MANDATORY_LABEL, TokenIntegrityLevel,
    };
    let mut len = 0u32;
    unsafe { GetTokenInformation(token, TokenIntegrityLevel, null_mut(), 0, &mut len) };
    if len == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    check(unsafe {
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            buf.as_mut_ptr().cast(),
            len,
            &mut len,
        )
    })?;
    let label = unsafe { &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL) };
    let sid = label.Label.Sid;
    let count = unsafe { *GetSidSubAuthorityCount(sid) };
    if count == 0 {
        return Err(io::Error::other("integrity SID has no RID"));
    }
    Ok(unsafe { *GetSidSubAuthority(sid, u32::from(count) - 1) })
}

/// How far a process's primary token can be trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenTrust {
    /// Mandatory integrity RID (0x2000 medium, 0x3000 high, ...).
    pub integrity: u32,
    pub app_container: bool,
    pub restricted: bool,
}

fn process_token_trust(process: HANDLE) -> io::Result<TokenTrust> {
    use windows_sys::Win32::Security::{IsTokenRestricted, TokenIsAppContainer};
    let mut token: HANDLE = null_mut();
    check(unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) })?;
    let token = unsafe { OwnedHandle::from_raw_handle(token as RawHandle) };
    let raw = token_raw(&token);
    Ok(TokenTrust {
        integrity: token_integrity_rid(raw)?,
        app_container: token_u32(raw, TokenIsAppContainer)? != 0,
        restricted: unsafe { IsTokenRestricted(raw) } != 0,
    })
}

/// Integrity level and sandboxing of this process's token.
pub fn current_token_trust() -> io::Result<TokenTrust> {
    process_token_trust(unsafe { GetCurrentProcess() })
}

/// Integrity level and sandboxing of another process's token.
pub fn process_trust(pid: u32) -> io::Result<TokenTrust> {
    let process = open_process(pid)?;
    process_token_trust(token_raw(&process))
}

/// Why a server with token `server` is not trusted by a client running at
/// integrity `client_integrity`; `None` when it is. A sandboxed or
/// lower-integrity process of the same user could squat on the pipe name.
pub fn untrusted_server_reason(server: TokenTrust, client_integrity: u32) -> Option<String> {
    if server.app_container {
        Some("server runs in an AppContainer".into())
    } else if server.restricted {
        Some("server runs with a restricted token".into())
    } else if server.integrity < client_integrity {
        Some(format!(
            "server integrity {:#x} is below the client's {client_integrity:#x}",
            server.integrity
        ))
    } else {
        None
    }
}

/// True while `pid` is running. A process that exited but whose object is
/// still held open by some handle is not alive.
pub fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::System::Threading::GetExitCodeProcess;
    const STILL_ACTIVE: u32 = 259;
    let Ok(process) = open_process(pid) else {
        return false;
    };
    let mut code = 0u32;
    unsafe { GetExitCodeProcess(token_raw(&process), &mut code) != 0 && code == STILL_ACTIVE }
}

fn open_process(pid: u32) -> io::Result<OwnedHandle> {
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(h as RawHandle) })
}

fn process_token_user(process: HANDLE) -> io::Result<Sid> {
    let mut token: HANDLE = null_mut();
    check(unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) })?;
    let token = unsafe { OwnedHandle::from_raw_handle(token as RawHandle) };
    token_user(token_raw(&token))
}

fn token_raw(h: &OwnedHandle) -> HANDLE {
    use std::os::windows::io::AsRawHandle;
    h.as_raw_handle() as HANDLE
}

/// The user SID of this process's primary token.
pub fn current_user_sid() -> io::Result<Sid> {
    process_token_user(unsafe { GetCurrentProcess() })
}

/// The user SID of another process's primary token.
pub fn process_user_sid(pid: u32) -> io::Result<Sid> {
    let process = open_process(pid)?;
    process_token_user(token_raw(&process))
}

/// Full Win32 image path of a process.
pub fn process_image(pid: u32) -> io::Result<PathBuf> {
    let process = open_process(pid)?;
    let mut buf = vec![0u16; 32768];
    let mut size = buf.len() as u32;
    check(unsafe {
        QueryFullProcessImageNameW(
            token_raw(&process),
            PROCESS_NAME_WIN32,
            buf.as_mut_ptr(),
            &mut size,
        )
    })?;
    Ok(PathBuf::from(String::from_utf16_lossy(
        &buf[..size as usize],
    )))
}

pub fn pipe_client_pid(pipe: RawHandle) -> io::Result<u32> {
    let mut pid = 0;
    check(unsafe { GetNamedPipeClientProcessId(pipe as HANDLE, &mut pid) })?;
    Ok(pid)
}

/// The user of a pipe client, from the security context captured when it
/// connected (valid after the client exits). Needs a completed read first.
/// Impersonates on the calling thread only between these calls, with no await.
pub fn pipe_client_token_sid(pipe: RawHandle) -> io::Result<Sid> {
    use windows_sys::Win32::Security::RevertToSelf;
    use windows_sys::Win32::System::Pipes::ImpersonateNamedPipeClient;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, OpenThreadToken};
    check(unsafe { ImpersonateNamedPipeClient(pipe as HANDLE) })?;
    let mut token: HANDLE = null_mut();
    // OpenAsSelf: open with the process's own identity, not the client's.
    let opened = unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) };
    let opened_err = io::Error::last_os_error();
    let reverted = unsafe { RevertToSelf() };
    if reverted == 0 {
        // Never keep running as the client.
        std::process::abort();
    }
    if opened == 0 {
        return Err(opened_err);
    }
    let token = unsafe { OwnedHandle::from_raw_handle(token as RawHandle) };
    token_user(token_raw(&token))
}

/// Wait up to `timeout` for an instance of `name` to be free (`WaitNamedPipeW`).
pub fn wait_named_pipe(name: &str, timeout: std::time::Duration) {
    use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
    let w = wide(name);
    let ms = u32::try_from(timeout.as_millis())
        .unwrap_or(u32::MAX)
        .max(1);
    unsafe { WaitNamedPipeW(w.as_ptr(), ms) };
}

pub fn pipe_server_pid(pipe: RawHandle) -> io::Result<u32> {
    let mut pid = 0;
    check(unsafe { GetNamedPipeServerProcessId(pipe as HANDLE, &mut pid) })?;
    Ok(pid)
}

/// What `PeekNamedPipe` (zero-length, so no payload is consumed) says about
/// the other end.
#[derive(Debug)]
pub enum PeerState {
    /// Connected, nothing unread in the kernel buffer.
    Idle,
    /// Connected with this many unread bytes.
    Pending(u32),
    /// The peek failed: ERROR_BROKEN_PIPE (109) once the client closed.
    Gone(io::Error),
}

pub fn peek(pipe: RawHandle) -> PeerState {
    let mut avail = 0u32;
    let ok = unsafe {
        PeekNamedPipe(
            pipe as HANDLE,
            null_mut(),
            0,
            null_mut(),
            &mut avail,
            null_mut(),
        )
    };
    if ok == 0 {
        PeerState::Gone(io::Error::last_os_error())
    } else if avail > 0 {
        PeerState::Pending(avail)
    } else {
        PeerState::Idle
    }
}

/// Owner and DACL of a kernel object as SDDL. The handle needs READ_CONTROL
/// (a client handle opened for GENERIC_READ has it).
pub fn handle_sddl(handle: RawHandle) -> io::Result<String> {
    let info = OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION;
    let mut sd: PSECURITY_DESCRIPTOR = null_mut();
    let err = unsafe {
        GetSecurityInfo(
            handle as HANDLE,
            SE_KERNEL_OBJECT,
            info,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut sd,
        )
    };
    if err != 0 {
        return Err(io::Error::from_raw_os_error(err as i32));
    }
    let mut out = null_mut();
    let ok = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            sd,
            SDDL_REVISION_1,
            info,
            &mut out,
            null_mut(),
        )
    };
    unsafe { LocalFree(sd) };
    check(ok)?;
    Ok(unsafe { take_wide(out) })
}

/// A self-relative security descriptor applied to every pipe instance.
pub struct PipeSecurity {
    sd: PSECURITY_DESCRIPTOR,
    pub sddl: String,
}

// The descriptor is immutable after construction and only read by
// CreateNamedPipeW.
unsafe impl Send for PipeSecurity {}
unsafe impl Sync for PipeSecurity {}

impl PipeSecurity {
    /// Owner = current user; protected DACL with GENERIC_ALL for the
    /// current user and LocalSystem only. No inherited ACEs, no Everyone,
    /// no Administrators, no network.
    pub fn current_user() -> io::Result<Self> {
        let sid = current_user_sid()?;
        Self::from_sddl(&private_sddl(&sid))
    }

    pub fn from_sddl(sddl: &str) -> io::Result<Self> {
        let w = wide(sddl);
        let mut sd: PSECURITY_DESCRIPTOR = null_mut();
        check(unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                w.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                null_mut(),
            )
        })?;
        Ok(Self {
            sd,
            sddl: sddl.to_owned(),
        })
    }

    /// Creates one instance. `first` sets FILE_FLAG_FIRST_PIPE_INSTANCE, so
    /// the call fails with ERROR_ACCESS_DENIED if anyone already owns the name.
    pub fn create(&self, name: &str, first: bool) -> io::Result<NamedPipeServer> {
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.sd,
            bInheritHandle: 0,
        };
        unsafe {
            ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(name, &mut sa as *mut _ as *mut c_void)
        }
    }
}

impl Drop for PipeSecurity {
    fn drop(&mut self) {
        unsafe { LocalFree(self.sd) };
    }
}

/// The SDDL the daemon should use: `O:<user>D:P(A;;GA;;;<user>)(A;;GA;;;SY)`.
pub fn private_sddl(user: &Sid) -> String {
    format!("O:{user}D:P(A;;GA;;;{user})(A;;GA;;;SY)")
}
