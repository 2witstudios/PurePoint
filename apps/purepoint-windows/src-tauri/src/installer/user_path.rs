//! The user `PATH` in `HKCU\Environment` (REQ-WINAPP-036): add `%USERPROFILE%\.pu\bin` once,
//! keep the value's registry type (`REG_EXPAND_SZ` stays expandable), then broadcast
//! `WM_SETTINGCHANGE` so Explorer and new terminals pick it up without a sign-out.
//!
//! The registry sits behind [`UserEnvironment`]; tests use [`MemoryEnvironment`] and never
//! touch the real user's `PATH`.

use std::io;
use std::sync::Mutex;

/// The registry type of the `Path` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    /// `REG_SZ`: taken literally.
    String,
    /// `REG_EXPAND_SZ`: `%VAR%` references expand when a process environment is built.
    ExpandString,
}

/// The `Path` value and its registry type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathValue {
    pub value: String,
    pub kind: ValueKind,
}

/// Read and write the user `PATH`.
pub trait UserEnvironment: Send + Sync {
    /// The `Path` value, or `None` when the user has none.
    fn read_path(&self) -> io::Result<Option<PathValue>>;
    fn write_path(&self, value: &PathValue) -> io::Result<()>;
    /// Expand `%VAR%` references, to recognise an entry such as `%USERPROFILE%\.pu\bin`.
    fn expand(&self, value: &str) -> String;
    /// Tell running programs the environment changed.
    fn broadcast_change(&self);
}

/// Compare form of a `PATH` entry: no quotes, `\` separators, no trailing separator,
/// lowercase (Windows paths are case-insensitive, WINAPP-014).
fn normalize(entry: &str) -> String {
    entry
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

/// The `PATH` value with `dir` appended, or `None` when an entry already names `dir`
/// (as written or once expanded, case-insensitively). The value's type is kept; a new
/// value is `REG_EXPAND_SZ`, the type Windows itself gives the user `Path`.
pub fn with_dir(
    current: Option<&PathValue>,
    dir: &str,
    expand: impl Fn(&str) -> String,
) -> Option<PathValue> {
    let wanted = normalize(dir);
    let existing = current.map(|c| c.value.as_str()).unwrap_or("");
    let present = existing
        .split(';')
        .filter(|entry| !entry.trim().is_empty())
        .any(|entry| normalize(entry) == wanted || normalize(&expand(entry)) == wanted);
    if present {
        return None;
    }
    let base = existing.trim_end_matches(';');
    let value = if base.trim().is_empty() {
        dir.to_string()
    } else {
        format!("{base};{dir}")
    };
    Some(PathValue {
        value,
        kind: current.map_or(ValueKind::ExpandString, |c| c.kind),
    })
}

/// Add `dir` to the user `PATH` unless it is there. Returns true when the value was written
/// (and the change broadcast).
pub fn ensure_on_path(env: &dyn UserEnvironment, dir: &str) -> io::Result<bool> {
    let current = env.read_path()?;
    let Some(updated) = with_dir(current.as_ref(), dir, |entry| env.expand(entry)) else {
        return Ok(false);
    };
    env.write_path(&updated)?;
    env.broadcast_change();
    Ok(true)
}

/// In-memory [`UserEnvironment`] for tests: `%NAME%` expands from `vars`.
#[derive(Default)]
pub struct MemoryEnvironment {
    pub path: Mutex<Option<PathValue>>,
    pub vars: Vec<(String, String)>,
    pub writes: Mutex<u32>,
    pub broadcasts: Mutex<u32>,
}

impl MemoryEnvironment {
    pub fn with_path(value: &str, kind: ValueKind) -> Self {
        Self {
            path: Mutex::new(Some(PathValue {
                value: value.into(),
                kind,
            })),
            ..Self::default()
        }
    }

    pub fn current(&self) -> Option<PathValue> {
        lock(&self.path).clone()
    }
}

impl UserEnvironment for MemoryEnvironment {
    fn read_path(&self) -> io::Result<Option<PathValue>> {
        Ok(self.current())
    }

    fn write_path(&self, value: &PathValue) -> io::Result<()> {
        *lock(&self.path) = Some(value.clone());
        *lock(&self.writes) += 1;
        Ok(())
    }

    fn expand(&self, value: &str) -> String {
        self.vars.iter().fold(value.to_string(), |acc, (name, v)| {
            let pattern = format!("%{name}%");
            // Case-insensitive, as Windows expands.
            match acc.to_lowercase().find(&pattern.to_lowercase()) {
                Some(at) => format!("{}{v}{}", &acc[..at], &acc[at + pattern.len()..]),
                None => acc,
            }
        })
    }

    fn broadcast_change(&self) {
        *lock(&self.broadcasts) += 1;
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The real `HKCU\Environment` `Path`.
pub struct RegistryEnvironment;

#[cfg(windows)]
mod registry {
    use std::io;

    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ,
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };

    use super::{PathValue, RegistryEnvironment, UserEnvironment, ValueKind};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn error(status: u32) -> io::Error {
        io::Error::from_raw_os_error(status as i32)
    }

    struct Key(HKEY);

    impl Key {
        fn environment(access: u32) -> io::Result<Self> {
            let subkey = wide("Environment");
            let mut key: HKEY = std::ptr::null_mut();
            // SAFETY: subkey is NUL-terminated; key receives the opened handle.
            let status =
                unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, access, &mut key) };
            if status != ERROR_SUCCESS {
                return Err(error(status));
            }
            Ok(Self(key))
        }
    }

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: a key we opened, closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    impl UserEnvironment for RegistryEnvironment {
        fn read_path(&self) -> io::Result<Option<PathValue>> {
            let key = Key::environment(KEY_QUERY_VALUE)?;
            let name = wide("Path");
            let mut buf: Vec<u16> = vec![0; 1024];
            loop {
                let mut kind = 0u32;
                let mut bytes = (buf.len() * 2) as u32;
                // SAFETY: buf holds `bytes` writable bytes; name is NUL-terminated.
                let status = unsafe {
                    RegQueryValueExW(
                        key.0,
                        name.as_ptr(),
                        std::ptr::null(),
                        &mut kind,
                        buf.as_mut_ptr().cast(),
                        &mut bytes,
                    )
                };
                match status {
                    ERROR_SUCCESS => {
                        let kind = match kind {
                            REG_EXPAND_SZ => ValueKind::ExpandString,
                            REG_SZ => ValueKind::String,
                            other => {
                                return Err(io::Error::other(format!(
                                    "HKCU\\Environment Path has registry type {other}, not a string"
                                )));
                            }
                        };
                        let len = (bytes as usize / 2).min(buf.len());
                        let text = &buf[..len];
                        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
                        return Ok(Some(PathValue {
                            value: String::from_utf16_lossy(&text[..end]),
                            kind,
                        }));
                    }
                    ERROR_FILE_NOT_FOUND => return Ok(None),
                    ERROR_MORE_DATA => buf.resize(bytes as usize / 2 + 1, 0),
                    other => return Err(error(other)),
                }
            }
        }

        fn write_path(&self, value: &PathValue) -> io::Result<()> {
            let key = Key::environment(KEY_SET_VALUE)?;
            let name = wide("Path");
            let data = wide(&value.value);
            let kind = match value.kind {
                ValueKind::ExpandString => REG_EXPAND_SZ,
                ValueKind::String => REG_SZ,
            };
            // SAFETY: data is a NUL-terminated UTF-16 buffer of the given byte length.
            let status = unsafe {
                RegSetValueExW(
                    key.0,
                    name.as_ptr(),
                    0,
                    kind,
                    data.as_ptr().cast(),
                    (data.len() * 2) as u32,
                )
            };
            if status != ERROR_SUCCESS {
                return Err(error(status));
            }
            Ok(())
        }

        fn expand(&self, value: &str) -> String {
            let src = wide(value);
            let mut buf: Vec<u16> = vec![0; 1024];
            loop {
                // SAFETY: src is NUL-terminated; buf has buf.len() writable u16s.
                let needed = unsafe {
                    ExpandEnvironmentStringsW(src.as_ptr(), buf.as_mut_ptr(), buf.len() as u32)
                } as usize;
                if needed == 0 {
                    return value.to_string();
                }
                if needed <= buf.len() {
                    let end = buf.iter().position(|&c| c == 0).unwrap_or(needed);
                    return String::from_utf16_lossy(&buf[..end]);
                }
                buf.resize(needed, 0);
            }
        }

        fn broadcast_change(&self) {
            let area = wide("Environment");
            let mut result = 0usize;
            // SAFETY: area is NUL-terminated and outlives the call, which waits at most 5 s
            // and skips hung windows.
            unsafe {
                SendMessageTimeoutW(
                    HWND_BROADCAST,
                    WM_SETTINGCHANGE,
                    0,
                    area.as_ptr() as isize,
                    SMTO_ABORTIFHUNG,
                    5000,
                    &mut result,
                )
            };
        }
    }
}

#[cfg(not(windows))]
impl UserEnvironment for RegistryEnvironment {
    fn read_path(&self) -> io::Result<Option<PathValue>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "the user PATH registry exists only on Windows",
        ))
    }
    fn write_path(&self, _value: &PathValue) -> io::Result<()> {
        self.read_path().map(|_| ())
    }
    fn expand(&self, value: &str) -> String {
        value.to_string()
    }
    fn broadcast_change(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = r"C:\Users\u\.pu\bin";

    #[test]
    fn given_no_path_should_create_expandable_value() {
        let env = MemoryEnvironment::default();
        assert!(ensure_on_path(&env, DIR).unwrap());
        assert_eq!(
            env.current(),
            Some(PathValue {
                value: DIR.into(),
                kind: ValueKind::ExpandString
            })
        );
        assert_eq!(
            (*env.writes.lock().unwrap(), *env.broadcasts.lock().unwrap()),
            (1, 1)
        );
    }

    #[test]
    fn given_expand_sz_path_should_append_and_keep_type_and_references() {
        let env = MemoryEnvironment::with_path(
            r"%USERPROFILE%\.cargo\bin;C:\Tools;",
            ValueKind::ExpandString,
        );
        assert!(ensure_on_path(&env, DIR).unwrap());
        let path = env.current().unwrap();
        assert_eq!(path.kind, ValueKind::ExpandString);
        assert_eq!(
            path.value,
            format!(r"%USERPROFILE%\.cargo\bin;C:\Tools;{DIR}")
        );
    }

    #[test]
    fn given_reg_sz_path_should_stay_reg_sz() {
        let env = MemoryEnvironment::with_path(r"C:\Tools", ValueKind::String);
        ensure_on_path(&env, DIR).unwrap();
        assert_eq!(env.current().unwrap().kind, ValueKind::String);
    }

    #[test]
    fn given_dir_already_present_in_any_spelling_should_not_write() {
        for existing in [
            DIR.to_string(),
            r"c:\users\U\.PU\BIN\".to_string(),
            "C:/Users/u/.pu/bin".to_string(),
            format!("\"{DIR}\""),
            r"%UserProfile%\.pu\bin".to_string(),
        ] {
            let mut env =
                MemoryEnvironment::with_path(&format!(r"C:\Tools;{existing}"), ValueKind::String);
            env.vars = vec![("USERPROFILE".into(), r"C:\Users\u".into())];
            assert!(!ensure_on_path(&env, DIR).unwrap(), "{existing}");
            assert_eq!(*env.writes.lock().unwrap(), 0, "{existing}");
            assert_eq!(*env.broadcasts.lock().unwrap(), 0, "{existing}");
        }
    }

    #[test]
    fn given_repeated_runs_should_add_dir_once() {
        let env = MemoryEnvironment::with_path(r"C:\Tools", ValueKind::ExpandString);
        assert!(ensure_on_path(&env, DIR).unwrap());
        assert!(!ensure_on_path(&env, DIR).unwrap());
        assert_eq!(env.current().unwrap().value, format!(r"C:\Tools;{DIR}"));
    }

    #[cfg(windows)]
    #[test]
    fn given_live_registry_should_read_without_writing() {
        // Read-only: the real user PATH is never written by tests.
        let _ = RegistryEnvironment.read_path().unwrap();
        assert_eq!(RegistryEnvironment.expand("plain"), "plain");
    }
}
