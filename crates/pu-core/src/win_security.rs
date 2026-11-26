//! Windows security primitives shared by private files, pipes and peer checks.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    GetNamedSecurityInfoW, GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
    DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation, GetSecurityDescriptorDacl,
    GetTokenInformation, IsWellKnownSid, OWNER_SECURITY_INFORMATION,
    PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, TOKEN_QUERY, TOKEN_USER,
    TokenUser, WinBuiltinAdministratorsSid, WinLocalSystemSid,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;

pub(crate) fn wide(s: &OsStr) -> Vec<u16> {
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// A SID copied out of a token buffer, kept alive with its buffer.
pub struct OwnedSid {
    buf: Vec<u8>,
    offset: usize,
}

impl OwnedSid {
    pub fn as_psid(&self) -> PSID {
        self.buf[self.offset..].as_ptr() as PSID
    }

    /// `S-1-5-21-...` form.
    pub fn to_string_sid(&self) -> io::Result<String> {
        let mut out: *mut u16 = null_mut();
        // SAFETY: valid SID pointer; out receives a LocalAlloc'd string we free.
        if unsafe { ConvertSidToStringSidW(self.as_psid(), &mut out) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let s = unsafe { wide_ptr_to_string(out) };
        unsafe { LocalFree(out as _) };
        Ok(s)
    }

    /// # Safety
    /// `other` must point to a valid SID.
    pub unsafe fn equals(&self, other: PSID) -> bool {
        // SAFETY: both pointers are valid SIDs.
        unsafe { EqualSid(self.as_psid(), other) != 0 }
    }
}

unsafe fn wide_ptr_to_string(p: *const u16) -> String {
    let mut len = 0;
    // SAFETY: caller passes a NUL-terminated wide string.
    unsafe {
        while *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    }
}

/// The user SID of the token of `process` (a process handle with
/// PROCESS_QUERY_LIMITED_INFORMATION).
/// # Safety
/// `process` must be a valid process handle.
pub unsafe fn process_user_sid(process: HANDLE) -> io::Result<OwnedSid> {
    let mut token: HANDLE = null_mut();
    // SAFETY: process is a valid handle; token receives a handle we close.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut len = 0u32;
        unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len) };
        let mut buf = vec![0u8; len as usize];
        // SAFETY: buf has len bytes.
        if unsafe { GetTokenInformation(token, TokenUser, buf.as_mut_ptr() as _, len, &mut len) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        let tu = buf.as_ptr() as *const TOKEN_USER;
        // SAFETY: GetTokenInformation filled a TOKEN_USER whose Sid points into buf.
        let sid_ptr = unsafe { (*tu).User.Sid } as usize;
        let offset = sid_ptr - buf.as_ptr() as usize;
        Ok(OwnedSid { buf, offset })
    })();
    unsafe { CloseHandle(token) };
    result
}

/// The current process user's SID.
pub fn current_user_sid() -> io::Result<OwnedSid> {
    // SAFETY: pseudo-handle, never needs closing.
    // SAFETY: the current-process pseudo-handle is always valid.
    unsafe { process_user_sid(GetCurrentProcess()) }
}

/// # Safety
/// `sid` must point to a valid SID.
pub unsafe fn is_system_or_admins(sid: PSID) -> bool {
    // SAFETY: valid SID.
    unsafe {
        IsWellKnownSid(sid, WinLocalSystemSid) != 0
            || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
    }
}

/// SDDL for an owner-only object: user and SYSTEM full control,
/// Administrators read. `P` blocks inheritance from the parent.
pub fn private_sddl(user_sid: &str, inherit: bool) -> String {
    let f = if inherit { "OICI" } else { "" };
    format!("D:P(A;{f};FA;;;{user_sid})(A;{f};FA;;;SY)(A;{f};GR;;;BA)")
}

struct Sd(PSECURITY_DESCRIPTOR);
impl Drop for Sd {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { LocalFree(self.0 as _) };
        }
    }
}

fn sd_from_sddl(sddl: &str) -> io::Result<Sd> {
    let w = wide(OsStr::new(sddl));
    let mut psd: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: w is NUL-terminated; psd receives LocalAlloc'd memory freed by Sd.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            w.as_ptr(),
            SDDL_REVISION_1,
            &mut psd,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(Sd(psd))
}

/// Build a self-relative security descriptor from SDDL; callers pass its
/// pointer in SECURITY_ATTRIBUTES (pipes) while the returned value lives.
pub struct SecurityDescriptor(Sd);
impl SecurityDescriptor {
    pub fn from_sddl(sddl: &str) -> io::Result<Self> {
        Ok(Self(sd_from_sddl(sddl)?))
    }
    pub fn as_ptr(&self) -> PSECURITY_DESCRIPTOR {
        self.0.0
    }
}

fn apply_sddl(path: &Path, sddl: &str) -> io::Result<()> {
    let sd = sd_from_sddl(sddl)?;
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl: *mut ACL = null_mut();
    // SAFETY: sd is a valid descriptor.
    if unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let name = wide(path.as_os_str());
    // SAFETY: name is NUL-terminated, dacl points into sd which outlives the call.
    let rc = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            dacl,
            null_mut(),
        )
    };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    Ok(())
}

/// Replace `path`'s DACL with the owner-only DACL.
pub fn set_private_dacl(path: &Path, is_dir: bool) -> io::Result<()> {
    let user = current_user_sid()?.to_string_sid()?;
    apply_sddl(path, &private_sddl(&user, is_dir))
}

#[doc(hidden)]
pub fn grant_for_test(path: &Path, trustee: &str) -> io::Result<()> {
    let user = current_user_sid()?.to_string_sid()?;
    let sddl = format!("{}(A;;FA;;;{trustee})", private_sddl(&user, false));
    apply_sddl(path, &sddl)
}

fn denied(path: &Path, why: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("{} {why}", path.display()),
    )
}

/// Owner must be the current user (or SYSTEM/Administrators); every allow ACE
/// must be for the current user, SYSTEM or Administrators.
pub fn check_private(path: &Path) -> io::Result<()> {
    let name = wide(path.as_os_str());
    let mut owner: PSID = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut psd: PSECURITY_DESCRIPTOR = null_mut();
    // SAFETY: out-pointers are valid; psd is freed by Sd.
    let rc = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut psd,
        )
    };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    let _sd = Sd(psd);
    // SAFETY: owner and dacl were filled by GetNamedSecurityInfoW and live in psd.
    unsafe { check_descriptor(path, owner, dacl) }
}

/// [`check_private`] for an open file: no second path lookup, so a swapped
/// path cannot change what is checked. The handle needs READ_CONTROL (any
/// handle opened for reading has it).
pub fn check_private_handle(handle: HANDLE, label: &Path) -> io::Result<()> {
    let (owner, dacl, sd) = handle_security(handle, true)?;
    let _sd = sd;
    // SAFETY: owner and dacl were filled by GetSecurityInfo and live in sd.
    unsafe { check_descriptor(label, owner, dacl) }
}

/// True when an open file is owned by the current user, or by SYSTEM or
/// Administrators (who already control the machine): the Windows meaning of
/// `st_uid == geteuid()`.
pub fn handle_owned_by_current_user(handle: HANDLE) -> io::Result<bool> {
    let user = current_user_sid()?;
    let (owner, _, sd) = handle_security(handle, false)?;
    let _sd = sd;
    // SAFETY: owner was filled by GetSecurityInfo and lives in sd.
    Ok(unsafe { user.equals(owner) || is_system_or_admins(owner) })
}

fn handle_security(handle: HANDLE, with_dacl: bool) -> io::Result<(PSID, *mut ACL, Sd)> {
    let mut owner: PSID = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut psd: PSECURITY_DESCRIPTOR = null_mut();
    let info = if with_dacl {
        OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION
    } else {
        OWNER_SECURITY_INFORMATION
    };
    // SAFETY: handle is a live file handle; out-pointers are valid; psd is
    // freed by Sd.
    let rc = unsafe {
        GetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            info,
            &mut owner,
            null_mut(),
            if with_dacl { &mut dacl } else { null_mut() },
            null_mut(),
            &mut psd,
        )
    };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc as i32));
    }
    Ok((owner, dacl, Sd(psd)))
}

/// # Safety
/// `owner` must be a valid SID and `dacl` null or a valid ACL, both alive
/// for the call.
unsafe fn check_descriptor(path: &Path, owner: PSID, dacl: *mut ACL) -> io::Result<()> {
    let user = current_user_sid()?;
    // SAFETY: caller guarantees owner is a valid SID.
    if !unsafe { user.equals(owner) || is_system_or_admins(owner) } {
        return Err(denied(path, "is not owned by the current user"));
    }
    if dacl.is_null() {
        return Err(denied(path, "has no DACL (everyone has access)"));
    }
    let mut info = ACL_SIZE_INFORMATION {
        AceCount: 0,
        AclBytesInUse: 0,
        AclBytesFree: 0,
    };
    // SAFETY: dacl is valid; info sized correctly.
    if unsafe {
        GetAclInformation(
            dacl,
            &mut info as *mut _ as _,
            std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
            AclSizeInformation,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    for i in 0..info.AceCount {
        let mut ace: *mut core::ffi::c_void = null_mut();
        // SAFETY: i < AceCount.
        if unsafe { GetAce(dacl, i, &mut ace) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let header = ace as *const ACE_HEADER;
        // SAFETY: every ACE starts with an ACE_HEADER.
        if unsafe { (*header).AceType } != ACCESS_ALLOWED_ACE_TYPE {
            continue;
        }
        let allowed = ace as *const ACCESS_ALLOWED_ACE;
        // SAFETY: allow ACEs carry their SID at SidStart.
        let sid = unsafe { &(*allowed).SidStart } as *const u32 as PSID;
        // SAFETY: sid points into the ACE inside the live DACL.
        if !unsafe { user.equals(sid) || is_system_or_admins(sid) } {
            return Err(denied(path, "is accessible by other users"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_current_process_should_have_a_user_sid() {
        let sid = current_user_sid().unwrap().to_string_sid().unwrap();
        assert!(sid.starts_with("S-1-5-"), "{sid}");
    }

    #[test]
    fn given_private_sddl_should_parse() {
        let sid = current_user_sid().unwrap().to_string_sid().unwrap();
        SecurityDescriptor::from_sddl(&private_sddl(&sid, true)).unwrap();
        SecurityDescriptor::from_sddl(&private_sddl(&sid, false)).unwrap();
    }
}
