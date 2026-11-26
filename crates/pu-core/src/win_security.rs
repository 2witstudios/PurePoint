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
    SetSecurityInfo,
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
const ACCESS_DENIED_ACE_TYPE: u8 = 1;

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

/// The DACL inside `sd`, valid while `sd` lives.
fn sd_dacl(sd: &Sd) -> io::Result<*mut ACL> {
    let mut present = 0;
    let mut defaulted = 0;
    let mut dacl: *mut ACL = null_mut();
    // SAFETY: sd is a valid descriptor.
    if unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(dacl)
}

/// The owner-only descriptor for a new file or directory, for
/// `SECURITY_ATTRIBUTES` at creation, so it never has an inherited DACL.
pub fn private_descriptor(is_dir: bool) -> io::Result<SecurityDescriptor> {
    let user = current_user_sid()?.to_string_sid()?;
    SecurityDescriptor::from_sddl(&private_sddl(&user, is_dir))
}

/// Replace an open file's or directory's DACL with the owner-only DACL. The
/// handle needs WRITE_DAC; no path is resolved, so no link is followed.
pub fn set_private_dacl_handle(handle: HANDLE, is_dir: bool) -> io::Result<()> {
    let sd = private_descriptor(is_dir)?;
    let dacl = sd_dacl(&sd.0)?;
    // SAFETY: handle is live; dacl points into sd which outlives the call.
    let rc = unsafe {
        SetSecurityInfo(
            handle,
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

pub(crate) fn apply_sddl(path: &Path, sddl: &str) -> io::Result<()> {
    let sd = sd_from_sddl(sddl)?;
    let dacl = sd_dacl(&sd)?;
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
        match unsafe { (*header).AceType } {
            ACCESS_ALLOWED_ACE_TYPE => {}
            ACCESS_DENIED_ACE_TYPE => continue,
            // Object, callback and conditional ACEs: fail closed rather
            // than guess whom they grant.
            _ => return Err(denied(path, "has an access entry of an unsupported type")),
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

/// The TrustedInstaller service SID, which owns OS-installed files.
const TRUSTED_INSTALLER_SID: &str =
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";
const INHERIT_ONLY_ACE: u8 = 0x08;
/// Rights that let a principal change what an executable runs: its data, its
/// DACL or owner, or deleting it to put another file in its place.
const BINARY_WRITE_RIGHTS: u32 = 0x0002 // FILE_WRITE_DATA
    | 0x0004 // FILE_APPEND_DATA
    | 0x0001_0000 // DELETE
    | 0x0004_0000 // WRITE_DAC
    | 0x0008_0000 // WRITE_OWNER
    | 0x1000_0000 // GENERIC_ALL
    | 0x4000_0000; // GENERIC_WRITE

/// # Safety
/// `sid` must point to a valid SID.
unsafe fn sid_string(sid: PSID) -> Option<String> {
    let mut out: *mut u16 = null_mut();
    // SAFETY: valid SID; out receives a LocalAlloc'd string we free.
    if unsafe { ConvertSidToStringSidW(sid, &mut out) } == 0 {
        return None;
    }
    let s = unsafe { wide_ptr_to_string(out) };
    unsafe { LocalFree(out as _) };
    Some(s)
}

/// # Safety
/// `sid` must point to a valid SID.
unsafe fn is_binary_trustee(user: &OwnedSid, sid: PSID) -> bool {
    // SAFETY: valid SIDs.
    unsafe {
        user.equals(sid)
            || is_system_or_admins(sid)
            || sid_string(sid).is_some_and(|s| s == TRUSTED_INSTALLER_SID)
    }
}

/// Whether `path` is an executable this user may start without handing
/// control to someone else: owned by this user, SYSTEM, Administrators or
/// TrustedInstaller, and with no allow entry that grants another principal
/// a right to modify or replace it. Read and execute grants to others
/// (e.g. `Users` on an installed program) are fine.
pub fn check_trusted_binary(path: &Path) -> Result<(), &'static str> {
    let meta = std::fs::metadata(path).map_err(|_| "not found")?;
    if !meta.is_file() {
        return Err("not a file");
    }
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    {
        return Err("not an .exe");
    }
    check_trusted_file(path)
}

/// [`check_trusted_binary`] for any file this user will run or load (a
/// script, say): the same owner and no-write-for-others rule, any extension.
pub fn check_trusted_file(path: &Path) -> Result<(), &'static str> {
    let meta = std::fs::metadata(path).map_err(|_| "not found")?;
    if !meta.is_file() {
        return Err("not a file");
    }
    let user = current_user_sid().map_err(|_| "cannot read the current user")?;
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
        return Err("cannot read its security descriptor");
    }
    let _sd = Sd(psd);
    // SAFETY: owner lives in psd.
    if owner.is_null() || !unsafe { is_binary_trustee(&user, owner) } {
        return Err("owned by another user");
    }
    if dacl.is_null() {
        return Err("writable by other users (no DACL)");
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
        return Err("cannot read its DACL");
    }
    for i in 0..info.AceCount {
        let mut ace: *mut core::ffi::c_void = null_mut();
        // SAFETY: i < AceCount.
        if unsafe { GetAce(dacl, i, &mut ace) } == 0 {
            return Err("cannot read its DACL");
        }
        let header = ace as *const ACE_HEADER;
        // SAFETY: every ACE starts with an ACE_HEADER.
        let (ace_type, flags) = unsafe { ((*header).AceType, (*header).AceFlags) };
        if flags & INHERIT_ONLY_ACE != 0 || ace_type == ACCESS_DENIED_ACE_TYPE {
            continue;
        }
        if ace_type != ACCESS_ALLOWED_ACE_TYPE {
            // Object, callback and conditional ACEs: fail closed.
            return Err("has an access entry of an unsupported type");
        }
        let allowed = ace as *const ACCESS_ALLOWED_ACE;
        // SAFETY: allow ACEs carry their mask and SID at SidStart.
        let (mask, sid) = unsafe { ((*allowed).Mask, &(*allowed).SidStart as *const u32 as PSID) };
        // SAFETY: sid points into the ACE inside the live DACL.
        if mask & BINARY_WRITE_RIGHTS != 0 && !unsafe { is_binary_trustee(&user, sid) } {
            return Err("writable by other users");
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

    fn exe(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("pu-engine.exe");
        std::fs::write(&path, b"MZ").unwrap();
        set_private_dacl(&path, false).unwrap();
        path
    }

    #[test]
    fn given_private_exe_should_be_trusted() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(check_trusted_binary(&exe(tmp.path())), Ok(()));
    }

    #[test]
    fn given_exe_others_can_write_should_not_be_trusted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = exe(tmp.path());
        for trustee in ["WD", "BU", "AU"] {
            grant_for_test(&path, trustee).unwrap();
            assert_eq!(
                check_trusted_binary(&path),
                Err("writable by other users"),
                "{trustee}"
            );
        }
    }

    #[test]
    fn given_exe_others_can_only_read_should_be_trusted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = exe(tmp.path());
        let user = current_user_sid().unwrap().to_string_sid().unwrap();
        let sddl = format!(
            "{}(A;;GRGX;;;BU)(A;;FA;;;{TRUSTED_INSTALLER_SID})",
            private_sddl(&user, false)
        );
        apply_sddl(&path, &sddl).unwrap();
        assert_eq!(check_trusted_binary(&path), Ok(()));
    }

    /// A conditional (callback) allow ACE: type 9, not type 0.
    const CONDITIONAL_ALLOW_EVERYONE: &str = "(XA;;FA;;;WD;(Member_of {SID(BA)}))";

    #[test]
    fn given_conditional_allow_ace_should_not_be_private() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.json");
        std::fs::write(&path, b"{}").unwrap();
        let user = current_user_sid().unwrap().to_string_sid().unwrap();
        apply_sddl(&path, &private_sddl(&user, false)).unwrap();
        check_private(&path).unwrap();
        let deny = format!("D:P(D;;FA;;;AN){}", &private_sddl(&user, false)[3..]);
        apply_sddl(&path, &deny).unwrap();
        check_private(&path).unwrap();
        let sddl = format!("{}{CONDITIONAL_ALLOW_EVERYONE}", private_sddl(&user, false));
        apply_sddl(&path, &sddl).unwrap();
        let err = check_private(&path).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{err}");
    }

    #[test]
    fn given_exe_with_conditional_allow_ace_should_not_be_trusted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = exe(tmp.path());
        let user = current_user_sid().unwrap().to_string_sid().unwrap();
        let sddl = format!("{}{CONDITIONAL_ALLOW_EVERYONE}", private_sddl(&user, false));
        apply_sddl(&path, &sddl).unwrap();
        assert_eq!(
            check_trusted_binary(&path),
            Err("has an access entry of an unsupported type")
        );
    }

    #[test]
    fn given_missing_or_non_exe_should_not_be_trusted() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(
            check_trusted_binary(&tmp.path().join("pu-engine.exe")),
            Err("not found")
        );
        let script = tmp.path().join("pu-engine.cmd");
        std::fs::write(&script, b"@echo off").unwrap();
        assert_eq!(check_trusted_binary(&script), Err("not an .exe"));
        assert_eq!(check_trusted_binary(tmp.path()), Err("not a file"));
    }
}
