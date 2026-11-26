//! The main window's size, position and maximized state across launches (WIN-7.1).
//!
//! Saved as `window-state.json` in the app data directory when the window closes, and put
//! back before the window is first shown (it starts hidden, `visible: false`, so it never
//! flashes at the default spot). The rectangle is the window's *restored* placement from
//! `GetWindowPlacement`, so a maximized window comes back maximized on the same monitor and
//! restores to where it was. A placement no monitor shows any more (an unplugged display) is
//! ignored and the window opens centered at its default size.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "window-state.json";

/// Smallest and largest window edge accepted from the file, in physical px.
const MIN_EDGE: i32 = 100;
const MAX_EDGE: i32 = 32_000;

/// The restored rectangle (`rcNormalPosition`, physical px) and whether it was maximized.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedPlacement {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub maximized: bool,
}

impl SavedPlacement {
    pub fn width(&self) -> i32 {
        self.right.saturating_sub(self.left)
    }

    pub fn height(&self) -> i32 {
        self.bottom.saturating_sub(self.top)
    }

    /// Whether the rectangle could be a real window: positive, not absurdly large, and with
    /// coordinates inside the virtual-screen range Windows uses.
    pub fn is_plausible(&self) -> bool {
        let edges_ok = |edge: i32| (MIN_EDGE..=MAX_EDGE).contains(&edge);
        let coords_ok = |c: i32| (-MAX_EDGE..=MAX_EDGE).contains(&c);
        edges_ok(self.width())
            && edges_ok(self.height())
            && [self.left, self.top, self.right, self.bottom]
                .into_iter()
                .all(coords_ok)
    }
}

pub fn path_in(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// The saved placement, or None when there is none or it is unreadable or implausible.
pub fn load(path: &Path) -> Option<SavedPlacement> {
    let text = std::fs::read_to_string(path).ok()?;
    let placement: SavedPlacement = serde_json::from_str(&text).ok()?;
    placement.is_plausible().then_some(placement)
}

/// Writes the placement, replacing the file in one rename so a crash never leaves half of it.
pub fn save(path: &Path, placement: &SavedPlacement) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(placement).map_err(std::io::Error::other)?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, path)
}

#[cfg(windows)]
mod win {
    use super::SavedPlacement;
    use windows_sys::Win32::Foundation::{HWND, RECT};
    use windows_sys::Win32::Graphics::Gdi::{MONITOR_DEFAULTTONULL, MonitorFromRect};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowPlacement, SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SW_SHOWNORMAL, SetWindowPlacement,
        WINDOWPLACEMENT,
    };

    fn empty_placement() -> WINDOWPLACEMENT {
        // SAFETY: WINDOWPLACEMENT is plain data; all-zero is a valid value.
        let mut placement: WINDOWPLACEMENT = unsafe { std::mem::zeroed() };
        placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        placement
    }

    /// The window's current placement.
    pub fn capture(hwnd: HWND) -> Option<SavedPlacement> {
        let mut placement = empty_placement();
        // SAFETY: `placement` is a correctly sized WINDOWPLACEMENT.
        if unsafe { GetWindowPlacement(hwnd, &mut placement) } == 0 {
            return None;
        }
        let rect = placement.rcNormalPosition;
        // A window closed while minimized remembers whether it was maximized before.
        let maximized = placement.showCmd == SW_SHOWMAXIMIZED as u32
            || (placement.showCmd == SW_SHOWMINIMIZED as u32 && placement.flags & 2 != 0);
        Some(SavedPlacement {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
            maximized,
        })
    }

    /// Applies a saved placement and shows the window. False when no monitor shows it.
    pub fn apply(hwnd: HWND, saved: &SavedPlacement) -> bool {
        let rect = RECT {
            left: saved.left,
            top: saved.top,
            right: saved.right,
            bottom: saved.bottom,
        };
        // SAFETY: plain queries and a correctly sized WINDOWPLACEMENT.
        unsafe {
            if MonitorFromRect(&rect, MONITOR_DEFAULTTONULL).is_null() {
                return false;
            }
            let mut placement = empty_placement();
            if GetWindowPlacement(hwnd, &mut placement) == 0 {
                return false;
            }
            placement.rcNormalPosition = rect;
            placement.showCmd = if saved.maximized {
                SW_SHOWMAXIMIZED as u32
            } else {
                SW_SHOWNORMAL as u32
            };
            SetWindowPlacement(hwnd, &placement) != 0
        }
    }
}

/// Puts the saved placement back and shows the window (centered at the default size when
/// there is nothing usable to restore).
pub fn restore(window: &tauri::WebviewWindow, dir: Option<&Path>) {
    let saved = dir.and_then(|dir| load(&path_in(dir)));
    #[cfg(windows)]
    if let (Some(saved), Ok(hwnd)) = (saved, window.hwnd())
        && win::apply(hwnd.0 as _, &saved)
    {
        return;
    }
    #[cfg(not(windows))]
    let _ = saved;
    if let Err(error) = window.show() {
        eprintln!("PurePoint: could not show the main window: {error}");
    }
}

/// Saves the window's placement for the next launch.
pub fn persist(window: &tauri::Window, dir: Option<&Path>) {
    let Some(dir) = dir else { return };
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd()
        && let Some(placement) = win::capture(hwnd.0 as _)
        && let Err(error) = save(&path_in(dir), &placement)
    {
        eprintln!("PurePoint: could not save the window placement: {error}");
    }
    #[cfg(not(windows))]
    let _ = (window, dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placement() -> SavedPlacement {
        SavedPlacement {
            left: 100,
            top: 80,
            right: 1300,
            bottom: 880,
            maximized: true,
        }
    }

    #[test]
    fn round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = path_in(dir.path());
        save(&path, &placement()).unwrap();
        assert_eq!(load(&path), Some(placement()));
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn creates_the_app_data_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = path_in(&dir.path().join("dev.purepoint.app"));
        save(&path, &placement()).unwrap();
        assert_eq!(load(&path), Some(placement()));
    }

    #[test]
    fn missing_or_corrupt_files_restore_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = path_in(dir.path());
        assert_eq!(load(&path), None);
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load(&path), None);
    }

    #[test]
    fn implausible_rectangles_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = path_in(dir.path());
        for bad in [
            SavedPlacement {
                right: 100,
                ..placement()
            },
            SavedPlacement {
                bottom: 90,
                ..placement()
            },
            SavedPlacement {
                left: -40_000,
                ..placement()
            },
            SavedPlacement {
                right: 50_000,
                ..placement()
            },
        ] {
            assert!(!bad.is_plausible(), "{bad:?}");
            save(&path, &bad).unwrap();
            assert_eq!(load(&path), None);
        }
    }

    #[test]
    fn negative_coordinates_on_a_left_monitor_are_fine() {
        let left_monitor = SavedPlacement {
            left: -1800,
            top: 100,
            right: -400,
            bottom: 900,
            maximized: false,
        };
        assert!(left_monitor.is_plausible());
        assert_eq!(left_monitor.width(), 1400);
        assert_eq!(left_monitor.height(), 800);
    }
}
