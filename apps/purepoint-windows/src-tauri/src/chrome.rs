//! Window chrome for the undecorated main window (DEC-10, WIN-7.1): Snap Layouts on the
//! app-drawn maximize button, a native caption over the title bar's empty space, and the
//! title-bar double-click and system menu.
//!
//! The UI draws the title bar and reports where it laid it out (`chrome_set_layout`): the
//! caption strip, the maximize button and the interactive elements inside the strip (menus,
//! buttons). Windows asks the window under the cursor for `WM_NCHITTEST`, and over the client
//! area that window is WebView2's own child, which this process cannot answer for. So, as
//! Windows Terminal does, an invisible "caption sink" child sits on top of the webview, cut by
//! a window region to the caption minus the interactive elements, plus the maximize button.
//! It answers `HTCAPTION`, `HTMAXBUTTON` and the top resize edges; everywhere else the region
//! lets the mouse through to the webview. The main window is subclassed (`SetWindowSubclass`)
//! to keep the sink sized and on top, and answers the same hit test for any point no child
//! covers. Tauri's own undecorated-resize child keeps the other edges resizable.
//!
//! Over `HTMAXBUTTON` the mouse messages are non-client ones, so the sink tracks hover and
//! press itself, toggles maximize on release, and emits [`CAPTION_BUTTONS_EVENT`] so the UI
//! paints the button's hover and pressed states.

use serde::{Deserialize, Serialize};

/// The event the backend emits with [`CaptionButtonsState`] when the maximize button's hover
/// or pressed state changes (the sink owns its mouse input, so CSS `:hover` never fires).
pub const CAPTION_BUTTONS_EVENT: &str = "chrome-caption-buttons";

/// A rectangle in client coordinates: CSS px from the UI, physical px once scaled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn is_empty(&self) -> bool {
        !(self.width > 0.0 && self.height > 0.0)
    }

    /// Half-open: the right and bottom edges belong to the next rectangle.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        !self.is_empty()
            && x >= self.x
            && x < self.x + self.width
            && y >= self.y
            && y < self.y + self.height
    }

    pub fn scaled(&self, scale: f64) -> Self {
        Self::new(
            self.x * scale,
            self.y * scale,
            self.width * scale,
            self.height * scale,
        )
    }

    /// Whole pixels covering the rectangle: `(left, top, right, bottom)`.
    pub fn pixels(&self) -> (i32, i32, i32, i32) {
        (
            self.x.floor() as i32,
            self.y.floor() as i32,
            (self.x + self.width).ceil() as i32,
            (self.y + self.height).ceil() as i32,
        )
    }
}

/// Where the UI laid out the title bar, in CSS px relative to the window's client area.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChromeLayout {
    /// The whole title-bar strip.
    pub caption: Rect,
    /// The app-drawn maximize/restore button.
    pub maximize: Rect,
    /// Menus, buttons and anything else in the strip that takes its own clicks.
    #[serde(default)]
    pub interactive: Vec<Rect>,
}

impl ChromeLayout {
    pub fn scaled(&self, scale: f64) -> Self {
        Self {
            caption: self.caption.scaled(scale),
            maximize: self.maximize.scaled(scale),
            interactive: self.interactive.iter().map(|r| r.scaled(scale)).collect(),
        }
    }

    /// The rectangles the caption sink's window region is built from: the union of `include`,
    /// minus each of `exclude`, with the maximize button added back on top.
    pub fn region_parts(&self) -> RegionParts {
        RegionParts {
            caption: self.caption,
            exclude: self
                .interactive
                .iter()
                .copied()
                .filter(|r| !r.is_empty())
                .collect(),
            maximize: self.maximize,
        }
    }
}

/// See [`ChromeLayout::region_parts`].
#[derive(Clone, Debug, PartialEq)]
pub struct RegionParts {
    pub caption: Rect,
    pub exclude: Vec<Rect>,
    pub maximize: Rect,
}

/// The window's client size and resize border, in the same units as the layout it is tested
/// against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub width: f64,
    pub height: f64,
    /// Resize border thickness; no border applies while maximized.
    pub border: f64,
    pub maximized: bool,
}

/// What a point in the client area is, for `WM_NCHITTEST`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// The webview's: let it through.
    Client,
    Caption,
    MaxButton,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

// winuser.h hit-test codes, kept here so the math is testable on any target.
const HTCLIENT: u32 = 1;
const HTCAPTION: u32 = 2;
const HTMAXBUTTON: u32 = 9;
const HTLEFT: u32 = 10;
const HTRIGHT: u32 = 11;
const HTTOP: u32 = 12;
const HTTOPLEFT: u32 = 13;
const HTTOPRIGHT: u32 = 14;
const HTBOTTOM: u32 = 15;
const HTBOTTOMLEFT: u32 = 16;
const HTBOTTOMRIGHT: u32 = 17;

impl Hit {
    /// The `WM_NCHITTEST` return value.
    pub fn code(self) -> u32 {
        match self {
            Hit::Client => HTCLIENT,
            Hit::Caption => HTCAPTION,
            Hit::MaxButton => HTMAXBUTTON,
            Hit::Left => HTLEFT,
            Hit::Right => HTRIGHT,
            Hit::Top => HTTOP,
            Hit::Bottom => HTBOTTOM,
            Hit::TopLeft => HTTOPLEFT,
            Hit::TopRight => HTTOPRIGHT,
            Hit::BottomLeft => HTBOTTOMLEFT,
            Hit::BottomRight => HTBOTTOMRIGHT,
        }
    }

    pub fn from_code(code: u32) -> Option<Hit> {
        Some(match code {
            HTCLIENT => Hit::Client,
            HTCAPTION => Hit::Caption,
            HTMAXBUTTON => Hit::MaxButton,
            HTLEFT => Hit::Left,
            HTRIGHT => Hit::Right,
            HTTOP => Hit::Top,
            HTBOTTOM => Hit::Bottom,
            HTTOPLEFT => Hit::TopLeft,
            HTTOPRIGHT => Hit::TopRight,
            HTBOTTOMLEFT => Hit::BottomLeft,
            HTBOTTOMRIGHT => Hit::BottomRight,
            _ => return None,
        })
    }

    pub fn is_resize_edge(self) -> bool {
        !matches!(self, Hit::Client | Hit::Caption | Hit::MaxButton)
    }
}

/// Hit-tests a client-area point. Resize edges win over everything while the window is not
/// maximized (Windows 11 resizes from the top edge even over the caption buttons), then the
/// maximize button, then the caption minus its interactive elements.
pub fn hit_test(layout: &ChromeLayout, frame: Frame, x: f64, y: f64) -> Hit {
    if !frame.maximized && frame.border > 0.0 {
        let left = x < frame.border;
        let right = x >= frame.width - frame.border;
        let top = y < frame.border;
        let bottom = y >= frame.height - frame.border;
        match (left, right, top, bottom) {
            (true, _, true, _) => return Hit::TopLeft,
            (_, true, true, _) => return Hit::TopRight,
            (true, _, _, true) => return Hit::BottomLeft,
            (_, true, _, true) => return Hit::BottomRight,
            (true, ..) => return Hit::Left,
            (_, true, ..) => return Hit::Right,
            (_, _, true, _) => return Hit::Top,
            (.., true) => return Hit::Bottom,
            _ => {}
        }
    }
    if layout.maximize.contains(x, y) {
        return Hit::MaxButton;
    }
    if layout.caption.contains(x, y) && !layout.interactive.iter().any(|r| r.contains(x, y)) {
        return Hit::Caption;
    }
    Hit::Client
}

/// The maximize button's visual state, as [`CAPTION_BUTTONS_EVENT`] reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptionButtonsState {
    pub maximize_hover: bool,
    pub maximize_pressed: bool,
}

/// Records where the UI laid out the title bar, and re-cuts the caption sink to match.
#[tauri::command]
pub fn chrome_set_layout(layout: ChromeLayout) {
    #[cfg(windows)]
    win::set_layout(layout);
    #[cfg(not(windows))]
    let _ = layout;
}

/// Installs the caption sink and the main-window subclass.
#[cfg(windows)]
pub fn install(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    win::install(window)
}

#[cfg(windows)]
mod win {
    use super::{CAPTION_BUTTONS_EVENT, CaptionButtonsState, ChromeLayout, Frame, Hit, hit_test};
    use std::sync::{Mutex, OnceLock};
    use tauri::{AppHandle, Emitter, Manager};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        CombineRgn, CreateRectRgn, DeleteObject, HRGN, RGN_DIFF, RGN_OR, ScreenToClient,
        SetWindowRgn,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
    };
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, EnableMenuItem, GetClientRect, GetSystemMenu, HWND_TOP,
        IsZoomed, LWA_ALPHA, MF_BYCOMMAND, MF_ENABLED, MF_GRAYED, PostMessageW, RegisterClassExW,
        SC_MAXIMIZE, SC_MINIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE, SM_CXPADDEDBORDER, SM_CYFRAME,
        SWP_NOACTIVATE, SWP_NOOWNERZORDER, SetLayeredWindowAttributes, SetWindowPos, TPM_RETURNCMD,
        TPM_RIGHTBUTTON, TrackPopupMenu, WM_APP, WM_DPICHANGED, WM_NCDESTROY, WM_NCHITTEST,
        WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE,
        WM_NCRBUTTONUP, WM_SIZE, WM_SYSCOMMAND, WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_VISIBLE,
    };

    /// Posted to the sink to re-cut its region on the window thread.
    const WM_RELAYOUT: u32 = WM_APP + 0x71;
    const SUBCLASS_ID: usize = 0x5050_4348; // "PPCH"

    struct Chrome {
        app: AppHandle,
        parent: isize,
        sink: Mutex<isize>,
        layout: Mutex<ChromeLayout>,
        buttons: Mutex<CaptionButtonsState>,
    }

    static CHROME: OnceLock<Chrome> = OnceLock::new();

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn install(window: &tauri::WebviewWindow) -> tauri::Result<()> {
        let parent = window.hwnd()?.0 as isize;
        let chrome = Chrome {
            app: window.app_handle().clone(),
            parent,
            sink: Mutex::new(0),
            layout: Mutex::new(ChromeLayout::default()),
            buttons: Mutex::new(CaptionButtonsState::default()),
        };
        if CHROME.set(chrome).is_err() {
            return Ok(());
        }
        // Window creation and subclassing belong on the thread that owns the window.
        window.run_on_main_thread(move || {
            // SAFETY: `parent` is the live main window, and this runs on its thread.
            unsafe { create_sink(parent as HWND) };
        })
    }

    unsafe fn create_sink(parent: HWND) {
        unsafe {
            let class_name = wide("PurePointCaptionSink");
            let instance = GetModuleHandleW(std::ptr::null());
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: 0,
                lpfnWndProc: Some(sink_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                lpszClassName: class_name.as_ptr(),
                hIconSm: std::ptr::null_mut(),
            };
            RegisterClassExW(&class);
            // Layered with no redirection bitmap: it never draws anything. A layered window is
            // hit-tested by its opacity, so it is made opaque below (alpha 255), which keeps it
            // taking the mouse inside its region while the webview shows through.
            let sink = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_NOREDIRECTIONBITMAP | WS_EX_NOACTIVATE,
                class_name.as_ptr(),
                std::ptr::null(),
                WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
                0,
                0,
                0,
                0,
                parent,
                std::ptr::null_mut(),
                instance,
                std::ptr::null(),
            );
            if sink.is_null() {
                eprintln!(
                    "PurePoint: could not create the caption sink window (error {})",
                    windows_sys::Win32::Foundation::GetLastError()
                );
                return;
            }
            SetLayeredWindowAttributes(sink, 0, 255, LWA_ALPHA);
            if let Some(chrome) = CHROME.get() {
                *lock(&chrome.sink) = sink as isize;
            }
            SetWindowSubclass(parent, Some(parent_proc), SUBCLASS_ID, 0);
            relayout(parent, sink);
        }
    }

    fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_layout(layout: ChromeLayout) {
        let Some(chrome) = CHROME.get() else { return };
        *lock(&chrome.layout) = layout;
        let sink = *lock(&chrome.sink);
        if sink != 0 {
            // SAFETY: posting to a window handle is safe even if it has since been destroyed.
            unsafe { PostMessageW(sink as HWND, WM_RELAYOUT, 0, 0) };
        }
    }

    fn scale(hwnd: HWND) -> f64 {
        // SAFETY: a plain query on a window handle.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi == 0 { 1.0 } else { f64::from(dpi) / 96.0 }
    }

    /// The main window's client size and resize border, in physical px.
    unsafe fn frame(parent: HWND) -> Frame {
        unsafe {
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(parent, &mut rect);
            let dpi = GetDpiForWindow(parent);
            let border = GetSystemMetricsForDpi(SM_CYFRAME, dpi)
                + GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
            Frame {
                width: f64::from(rect.right - rect.left),
                height: f64::from(rect.bottom - rect.top),
                border: f64::from(border),
                maximized: IsZoomed(parent) != 0,
            }
        }
    }

    fn physical_layout(parent: HWND) -> ChromeLayout {
        let layout = CHROME
            .get()
            .map(|c| lock(&c.layout).clone())
            .unwrap_or_default();
        layout.scaled(scale(parent))
    }

    unsafe fn rect_region(rect: &super::Rect) -> HRGN {
        let (left, top, right, bottom) = rect.pixels();
        unsafe { CreateRectRgn(left, top, right, bottom) }
    }

    /// Covers the client area and cuts the sink to the caption (minus its interactive
    /// elements) plus the maximize button, and puts it above the webview.
    unsafe fn relayout(parent: HWND, sink: HWND) {
        unsafe {
            let frame = frame(parent);
            SetWindowPos(
                sink,
                HWND_TOP,
                0,
                0,
                frame.width as i32,
                frame.height as i32,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            );
            let parts = physical_layout(parent).region_parts();
            let region = rect_region(&parts.caption);
            for hole in &parts.exclude {
                let cut = rect_region(hole);
                CombineRgn(region, region, cut, RGN_DIFF);
                DeleteObject(cut);
            }
            let maximize = rect_region(&parts.maximize);
            CombineRgn(region, region, maximize, RGN_OR);
            DeleteObject(maximize);
            // The window owns the region from here on.
            SetWindowRgn(sink, region, 1);
        }
    }

    /// The hit test for a screen point (`WM_NCHITTEST`'s lParam).
    unsafe fn hit_at(parent: HWND, lparam: LPARAM) -> Hit {
        let mut point = POINT {
            x: i32::from(lparam as u32 as u16 as i16),
            y: i32::from((lparam as u32 >> 16) as u16 as i16),
        };
        unsafe {
            ScreenToClient(parent, &mut point);
            hit_test(
                &physical_layout(parent),
                frame(parent),
                f64::from(point.x),
                f64::from(point.y),
            )
        }
    }

    fn set_buttons(next: CaptionButtonsState) {
        let Some(chrome) = CHROME.get() else { return };
        let changed = {
            let mut current = lock(&chrome.buttons);
            let changed = *current != next;
            *current = next;
            changed
        };
        if changed {
            let _ = chrome.app.emit(CAPTION_BUTTONS_EVENT, next);
        }
    }

    fn buttons() -> CaptionButtonsState {
        CHROME.get().map(|c| *lock(&c.buttons)).unwrap_or_default()
    }

    unsafe fn toggle_maximize(parent: HWND) {
        unsafe {
            let command = if IsZoomed(parent) != 0 {
                SC_RESTORE
            } else {
                SC_MAXIMIZE
            };
            PostMessageW(parent, WM_SYSCOMMAND, command as WPARAM, 0);
        }
    }

    /// The window menu (Restore, Move, Size, Minimize, Maximize, Close) at a screen point.
    unsafe fn show_system_menu(parent: HWND, lparam: LPARAM) {
        unsafe {
            let menu = GetSystemMenu(parent, 0);
            if menu.is_null() {
                return;
            }
            let zoomed = IsZoomed(parent) != 0;
            let state = |enabled: bool| MF_BYCOMMAND | if enabled { MF_ENABLED } else { MF_GRAYED };
            EnableMenuItem(menu, SC_RESTORE, state(zoomed));
            EnableMenuItem(menu, SC_MOVE, state(!zoomed));
            EnableMenuItem(menu, SC_SIZE, state(!zoomed));
            EnableMenuItem(menu, SC_MINIMIZE, state(true));
            EnableMenuItem(menu, SC_MAXIMIZE, state(!zoomed));
            let x = i32::from(lparam as u32 as u16 as i16);
            let y = i32::from((lparam as u32 >> 16) as u16 as i16);
            let command = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                x,
                y,
                0,
                parent,
                std::ptr::null(),
            );
            if command != 0 {
                PostMessageW(parent, WM_SYSCOMMAND, command as WPARAM, 0);
            }
        }
    }

    /// The non-client mouse messages both windows share. `forward` says whether caption and
    /// edge presses go to the main window (the sink's) or to the default handling (the main
    /// window's own). Returns None for default handling.
    unsafe fn nc_message(
        hwnd: HWND,
        parent: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        forward: bool,
    ) -> Option<LRESULT> {
        unsafe {
            match msg {
                WM_NCHITTEST => match hit_at(parent, lparam) {
                    Hit::Client => None,
                    hit => Some(hit.code() as LRESULT),
                },
                WM_NCMOUSEMOVE => {
                    let over = Hit::from_code(wparam as u32) == Some(Hit::MaxButton);
                    let state = buttons();
                    set_buttons(CaptionButtonsState {
                        maximize_hover: over,
                        maximize_pressed: over && state.maximize_pressed,
                    });
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE | TME_NONCLIENT,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut track);
                    // Default handling too: Snap Layouts watch the hover over HTMAXBUTTON.
                    None
                }
                WM_NCMOUSELEAVE => {
                    set_buttons(CaptionButtonsState::default());
                    None
                }
                WM_NCLBUTTONDOWN => match Hit::from_code(wparam as u32) {
                    Some(Hit::MaxButton) => {
                        set_buttons(CaptionButtonsState {
                            maximize_hover: true,
                            maximize_pressed: true,
                        });
                        // Not DefWindowProc: it would draw a classic caption button.
                        Some(0)
                    }
                    Some(hit) if forward && (hit == Hit::Caption || hit.is_resize_edge()) => {
                        // The main window runs the move or size loop, as for its own caption.
                        ReleaseCapture();
                        PostMessageW(parent, WM_NCLBUTTONDOWN, wparam, lparam);
                        Some(0)
                    }
                    _ => None,
                },
                WM_NCLBUTTONUP => match Hit::from_code(wparam as u32) {
                    Some(Hit::MaxButton) => {
                        let pressed = buttons().maximize_pressed;
                        set_buttons(CaptionButtonsState {
                            maximize_hover: true,
                            maximize_pressed: false,
                        });
                        if pressed {
                            toggle_maximize(parent);
                        }
                        Some(0)
                    }
                    _ => None,
                },
                WM_NCLBUTTONDBLCLK => match Hit::from_code(wparam as u32) {
                    Some(Hit::Caption) => {
                        toggle_maximize(parent);
                        Some(0)
                    }
                    Some(Hit::MaxButton) => Some(0),
                    _ => None,
                },
                WM_NCRBUTTONUP => match Hit::from_code(wparam as u32) {
                    Some(Hit::Caption) | Some(Hit::MaxButton) => {
                        show_system_menu(parent, lparam);
                        Some(0)
                    }
                    _ => None,
                },
                _ => None,
            }
        }
    }

    unsafe extern "system" fn sink_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            let parent = CHROME.get().map(|c| c.parent as HWND);
            if let Some(parent) = parent {
                if msg == WM_RELAYOUT {
                    relayout(parent, hwnd);
                    return 0;
                }
                if let Some(result) = nc_message(hwnd, parent, msg, wparam, lparam, true) {
                    return result;
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }

    unsafe extern "system" fn parent_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        unsafe {
            match msg {
                WM_SIZE | WM_DPICHANGED => {
                    let result = DefSubclassProc(hwnd, msg, wparam, lparam);
                    if let Some(chrome) = CHROME.get() {
                        let sink = *lock(&chrome.sink);
                        if sink != 0 {
                            relayout(hwnd, sink as HWND);
                        }
                    }
                    return result;
                }
                WM_NCDESTROY => {
                    windows_sys::Win32::UI::Shell::RemoveWindowSubclass(
                        hwnd,
                        Some(parent_proc),
                        SUBCLASS_ID,
                    );
                }
                _ => {
                    if let Some(result) = nc_message(hwnd, hwnd, msg, wparam, lparam, false) {
                        return result;
                    }
                }
            }
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1000x700 window with a 32px title bar: a menu at 0..200, the toggle at 200..232,
    /// and caption buttons of 46px at the right (minimize 862, maximize 908, close 954).
    fn layout() -> ChromeLayout {
        ChromeLayout {
            caption: Rect::new(0.0, 0.0, 1000.0, 32.0),
            maximize: Rect::new(908.0, 0.0, 46.0, 32.0),
            interactive: vec![
                Rect::new(0.0, 0.0, 200.0, 32.0),
                Rect::new(200.0, 0.0, 32.0, 32.0),
                Rect::new(862.0, 0.0, 46.0, 32.0),
                Rect::new(954.0, 0.0, 46.0, 32.0),
            ],
        }
    }

    fn frame(maximized: bool) -> Frame {
        Frame {
            width: 1000.0,
            height: 700.0,
            border: 8.0,
            maximized,
        }
    }

    #[test]
    fn maximize_button_answers_htmaxbutton() {
        let hit = hit_test(&layout(), frame(false), 930.0, 20.0);
        assert_eq!(hit, Hit::MaxButton);
        assert_eq!(hit.code(), 9);
    }

    #[test]
    fn empty_title_bar_is_the_caption() {
        assert_eq!(hit_test(&layout(), frame(false), 500.0, 16.0), Hit::Caption);
        assert_eq!(Hit::Caption.code(), 2);
    }

    #[test]
    fn interactive_elements_and_content_stay_client() {
        for (x, y) in [
            (100.0, 16.0),
            (210.0, 16.0),
            (880.0, 16.0),
            (970.0, 16.0),
            (500.0, 300.0),
        ] {
            assert_eq!(
                hit_test(&layout(), frame(false), x, y),
                Hit::Client,
                "({x}, {y})"
            );
        }
    }

    #[test]
    fn edges_and_corners_resize() {
        let cases = [
            ((2.0, 2.0), Hit::TopLeft, 13),
            ((998.0, 2.0), Hit::TopRight, 14),
            ((2.0, 698.0), Hit::BottomLeft, 16),
            ((998.0, 698.0), Hit::BottomRight, 17),
            ((2.0, 300.0), Hit::Left, 10),
            ((998.0, 300.0), Hit::Right, 11),
            ((500.0, 2.0), Hit::Top, 12),
            ((500.0, 698.0), Hit::Bottom, 15),
        ];
        for ((x, y), hit, code) in cases {
            assert_eq!(hit_test(&layout(), frame(false), x, y), hit, "({x}, {y})");
            assert_eq!(hit.code(), code);
            assert!(hit.is_resize_edge());
        }
    }

    #[test]
    fn top_edge_wins_over_the_maximize_button() {
        assert_eq!(hit_test(&layout(), frame(false), 930.0, 3.0), Hit::Top);
    }

    #[test]
    fn maximized_window_has_no_resize_border() {
        assert_eq!(hit_test(&layout(), frame(true), 930.0, 0.0), Hit::MaxButton);
        assert_eq!(hit_test(&layout(), frame(true), 500.0, 0.0), Hit::Caption);
        assert_eq!(hit_test(&layout(), frame(true), 0.0, 300.0), Hit::Client);
    }

    #[test]
    fn rect_edges_are_half_open() {
        let rect = Rect::new(10.0, 10.0, 10.0, 10.0);
        assert!(rect.contains(10.0, 10.0));
        assert!(!rect.contains(20.0, 15.0));
        assert!(!rect.contains(15.0, 20.0));
        assert!(!Rect::default().contains(0.0, 0.0));
    }

    #[test]
    fn layout_scales_to_physical_pixels() {
        let scaled = layout().scaled(1.5);
        assert_eq!(scaled.maximize, Rect::new(1362.0, 0.0, 69.0, 48.0));
        assert_eq!(scaled.interactive.len(), 4);
        let physical = Frame {
            width: 1500.0,
            height: 1050.0,
            border: 12.0,
            maximized: false,
        };
        assert_eq!(hit_test(&scaled, physical, 1395.0, 30.0), Hit::MaxButton);
        assert_eq!(hit_test(&scaled, physical, 750.0, 30.0), Hit::Caption);
    }

    #[test]
    fn pixels_cover_fractional_rects() {
        assert_eq!(Rect::new(1.5, 0.25, 10.0, 31.5).pixels(), (1, 0, 12, 32));
    }

    #[test]
    fn region_parts_drop_empty_holes() {
        let mut with_empty = layout();
        with_empty.interactive.push(Rect::default());
        let parts = with_empty.region_parts();
        assert_eq!(parts.exclude.len(), 4);
        assert_eq!(parts.maximize, layout().maximize);
    }

    #[test]
    fn codes_round_trip() {
        for code in [1, 2, 9, 10, 11, 12, 13, 14, 15, 16, 17] {
            assert_eq!(Hit::from_code(code).map(Hit::code), Some(code));
        }
        assert_eq!(Hit::from_code(20), None);
    }

    #[test]
    fn layout_deserializes_from_the_ui() {
        let json = r#"{"caption":{"x":0,"y":0,"width":800,"height":32},
            "maximize":{"x":708,"y":0,"width":46,"height":32}}"#;
        let layout: ChromeLayout = serde_json::from_str(json).unwrap();
        assert!(layout.interactive.is_empty());
        assert_eq!(layout.maximize.width, 46.0);
    }
}
