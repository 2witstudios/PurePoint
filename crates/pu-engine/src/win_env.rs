//! Windows agent environment (WINAPP-007). Windows has no login shell to
//! replay, so the daemon starts from its own environment and merges the
//! current registry environment, which picks up `PATH` edits made since logon.

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ, RegCloseKey,
    RegEnumValueW, RegOpenKeyExW, RegQueryInfoKeyW,
};

const SYSTEM_ENVIRONMENT: &str = r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

/// Where the registry environment comes from; injectable so tests can supply
/// an empty or unusual user `PATH` without touching the real registry.
pub(crate) trait RegistryEnv {
    /// Current `HKCU\Environment` string values, `REG_EXPAND_SZ` expanded.
    fn user_vars(&self) -> Vec<(String, String)>;
    /// The system `PATH` from `HKLM\...\Session Manager\Environment`, expanded.
    fn system_path(&self) -> Option<String>;
}

/// The live registry.
pub(crate) struct LiveRegistry;

impl RegistryEnv for LiveRegistry {
    fn user_vars(&self) -> Vec<(String, String)> {
        read_string_values(HKEY_CURRENT_USER, "Environment")
    }

    fn system_path(&self) -> Option<String> {
        read_string_values(HKEY_LOCAL_MACHINE, SYSTEM_ENVIRONMENT)
            .into_iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("PATH"))
            .map(|(_, value)| value)
    }
}

/// The base agent environment: the daemon's variables (first spelling of each
/// case-insensitive name wins), user registry variables the daemon lacks, and
/// `PATH` = user registry `PATH`, then system `PATH`, then the daemon's `PATH`,
/// with duplicates removed. `PATH` is always spelled `PATH`.
pub(crate) fn login_env(
    daemon: impl IntoIterator<Item = (String, String)>,
    registry: &dyn RegistryEnv,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();
    let mut daemon_path = None;
    let add = |env: &mut Vec<(String, String)>, key: String, value: String| {
        if !env.iter().any(|(k, _)| k.eq_ignore_ascii_case(&key)) {
            env.push((key, value));
        }
    };
    for (key, value) in daemon {
        if key.eq_ignore_ascii_case("PATH") {
            daemon_path.get_or_insert(value);
        } else {
            add(&mut env, key, value);
        }
    }
    let mut user_path = None;
    for (key, value) in registry.user_vars() {
        if key.eq_ignore_ascii_case("PATH") {
            user_path.get_or_insert(value);
        } else {
            add(&mut env, key, value);
        }
    }
    let dirs: Vec<PathBuf> = [user_path, registry.system_path(), daemon_path]
        .into_iter()
        .flatten()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .collect();
    env.push(("PATH".into(), pu_core::env_path::join_unique(dirs)));
    env
}

/// The daemon's environment as UTF-8 pairs (non-UTF-8 entries are skipped,
/// where `std::env::vars` would panic).
pub(crate) fn daemon_env() -> Vec<(String, String)> {
    std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect()
}

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    OsString::from_wide(&buf[..end])
        .to_string_lossy()
        .into_owned()
}

/// Expand `%VAR%` references against the daemon's environment.
fn expand(value: &str) -> String {
    let src = wide(value);
    let mut buf = vec![0u16; src.len().max(260)];
    for _ in 0..4 {
        // SAFETY: `src` is NUL-terminated and `buf` holds `buf.len()` u16s.
        let needed =
            unsafe { ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) }
                as usize;
        if needed == 0 {
            return value.to_string();
        }
        if needed <= buf.len() {
            return from_wide(&buf);
        }
        buf.resize(needed, 0);
    }
    value.to_string()
}

/// Every `REG_SZ` / `REG_EXPAND_SZ` value under `root\subkey`, expanded.
/// A missing or unreadable key yields nothing.
fn read_string_values(root: HKEY, subkey: &str) -> Vec<(String, String)> {
    let mut key: HKEY = std::ptr::null_mut();
    let subkey = wide(subkey);
    // SAFETY: `subkey` is NUL-terminated; `key` receives an open handle on success.
    if unsafe { RegOpenKeyExW(root, subkey.as_ptr(), 0, KEY_READ, &mut key) } != ERROR_SUCCESS {
        return Vec::new();
    }
    let mut values = Vec::new();
    let (mut count, mut max_name, mut max_data) = (0u32, 0u32, 0u32);
    // SAFETY: `key` is open; every out-pointer is valid or null as allowed.
    let info = unsafe {
        RegQueryInfoKeyW(
            key,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut count,
            &mut max_name,
            &mut max_data,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if info == ERROR_SUCCESS {
        let mut name = vec![0u16; max_name as usize + 1];
        // Room for a terminating NUL the writer may have omitted.
        let mut data = vec![0u16; max_data as usize / 2 + 1];
        for index in 0..count {
            let mut name_len = name.len() as u32;
            let mut data_len = (data.len() * 2) as u32;
            let mut kind = 0u32;
            // SAFETY: the buffers hold `name_len` u16s and `data_len` bytes.
            let status = unsafe {
                RegEnumValueW(
                    key,
                    index,
                    name.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null(),
                    &mut kind,
                    data.as_mut_ptr().cast(),
                    &mut data_len,
                )
            };
            if status != ERROR_SUCCESS || (kind != REG_SZ && kind != REG_EXPAND_SZ) {
                continue;
            }
            let value_name = from_wide(&name[..name_len as usize]);
            let raw = from_wide(&data[..(data_len as usize / 2).min(data.len())]);
            let value = if kind == REG_EXPAND_SZ {
                expand(&raw)
            } else {
                raw
            };
            values.push((value_name, value));
        }
    }
    // SAFETY: `key` was opened above and is closed once.
    unsafe { RegCloseKey(key) };
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRegistry {
        user: Vec<(String, String)>,
        system: Option<String>,
    }

    impl RegistryEnv for FakeRegistry {
        fn user_vars(&self) -> Vec<(String, String)> {
            self.user.clone()
        }
        fn system_path(&self) -> Option<String> {
            self.system.clone()
        }
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn path_of(env: &[(String, String)]) -> &str {
        let paths: Vec<_> = env
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("PATH"))
            .collect();
        assert_eq!(paths.len(), 1, "{env:?}");
        assert_eq!(paths[0].0, "PATH");
        &paths[0].1
    }

    #[test]
    fn given_registry_path_should_precede_daemon_path_without_duplicates() {
        let registry = FakeRegistry {
            user: pairs(&[
                (
                    "Path",
                    r"C:\Users\me\new tool;C:\Users\me\AppData\Roaming\npm",
                ),
                ("NEW_VAR", "fresh"),
                ("TEMP", r"C:\registry\temp"),
            ]),
            system: Some(r"C:\WINDOWS\system32;C:\Program Files\Git\cmd".into()),
        };
        let env = login_env(
            pairs(&[
                ("Path", r"c:\windows\SYSTEM32;C:\daemon only"),
                ("TEMP", r"C:\daemon\temp"),
            ]),
            &registry,
        );
        assert_eq!(
            path_of(&env),
            r"C:\Users\me\new tool;C:\Users\me\AppData\Roaming\npm;C:\WINDOWS\system32;C:\Program Files\Git\cmd;C:\daemon only"
        );
        let get = |key: &str| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        assert_eq!(get("NEW_VAR"), Some("fresh"));
        // The daemon's own value wins (tests and callers redirect variables).
        assert_eq!(get("TEMP"), Some(r"C:\daemon\temp"));
    }

    #[test]
    fn given_empty_user_path_should_keep_system_and_daemon_path() {
        let registry = FakeRegistry {
            user: pairs(&[("Path", "")]),
            system: Some(r"C:\WINDOWS\system32".into()),
        };
        let env = login_env(pairs(&[("PATH", r"C:\daemon")]), &registry);
        assert_eq!(path_of(&env), r"C:\WINDOWS\system32;C:\daemon");

        let none = FakeRegistry {
            user: Vec::new(),
            system: None,
        };
        let env = login_env(pairs(&[("PATH", r"C:\daemon")]), &none);
        assert_eq!(path_of(&env), r"C:\daemon");
    }

    #[test]
    fn given_live_registry_should_read_expanded_system_path() {
        let system = LiveRegistry.system_path().expect("system PATH");
        assert!(!system.contains('%'), "{system}");
        assert!(system.to_lowercase().contains(r"\system32"), "{system}");
        // The user key always exists for an interactive account.
        let _ = LiveRegistry.user_vars();
    }

    #[test]
    fn given_reference_should_expand_against_process_env() {
        let expanded = expand("%SystemRoot%\\x");
        assert!(!expanded.starts_with('%'), "{expanded}");
        assert_eq!(expand("plain"), "plain");
    }
}
