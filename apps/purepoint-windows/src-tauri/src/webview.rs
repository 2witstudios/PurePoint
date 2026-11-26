//! WebView2 settings Tauri does not expose in `tauri.conf.json`.

use tauri::{Manager, Url, WebviewWindow};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ,
    COREWEBVIEW2_PERMISSION_STATE_ALLOW, ICoreWebView2Settings, ICoreWebView2Settings3,
};
use webview2_com::{NewWindowRequestedEventHandler, PermissionRequestedEventHandler, take_pwstr};
use windows_core::{Interface, PWSTR};

use crate::navigation;

/// When `lock_down` is set, turns off the WebView2 browser accelerator keys
/// (F5 and Ctrl+R reload, Ctrl+P print, Ctrl+F find, F12 DevTools and so on),
/// the default context menu and DevTools, so the window behaves like an app
/// rather than a browser tab. Otherwise turns them on.
///
/// Either way, grants the app's own page clipboard reads without a prompt: a
/// terminal's Ctrl+Shift+V reads the clipboard (WIN-7.3). Any other origin gets
/// WebView2's default (a prompt). A new-window request (wry refuses them all)
/// with an `http(s)` address opens it in the user's browser instead.
pub fn configure(window: &WebviewWindow, lock_down: bool) -> tauri::Result<()> {
    let dev_url = window.config().build.dev_url.clone();
    window.with_webview(move |webview| {
        // SAFETY: plain COM getters and setters on the live controller of this
        // window's WebView2, called on the thread Tauri hands the webview to.
        let applied = unsafe { apply(&webview, !lock_down, dev_url) };
        if let Err(error) = applied {
            eprintln!("PurePoint: could not configure WebView2 settings: {error}");
        }
    })
}

/// The URI an event's getter returns, freed after reading.
fn uri(read: impl FnOnce(*mut PWSTR) -> windows_core::Result<()>) -> Option<Url> {
    let mut raw = PWSTR::null();
    read(&mut raw).ok()?;
    Url::parse(&take_pwstr(raw)).ok()
}

unsafe fn apply(
    webview: &tauri::webview::PlatformWebview,
    enabled: bool,
    dev_url: Option<Url>,
) -> windows_core::Result<()> {
    unsafe {
        let core = webview.controller().CoreWebView2()?;
        let settings: ICoreWebView2Settings = core.Settings()?;
        settings.SetAreDefaultContextMenusEnabled(enabled)?;
        settings.SetAreDevToolsEnabled(enabled)?;
        settings
            .cast::<ICoreWebView2Settings3>()?
            .SetAreBrowserAcceleratorKeysEnabled(enabled)?;
        let mut token = 0i64;
        core.add_PermissionRequested(
            &PermissionRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                let from_app = uri(|raw| args.Uri(raw))
                    .is_some_and(|url| navigation::is_app_url(&url, dev_url.as_ref()));
                if kind == COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ && from_app {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
        let mut token = 0i64;
        core.add_NewWindowRequested(
            &NewWindowRequestedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                args.SetHandled(true)?;
                if let Some(url) = uri(|raw| args.Uri(raw))
                    && matches!(url.scheme(), "http" | "https")
                    && let Err(error) = tauri_plugin_opener::open_url(url.as_str(), None::<&str>)
                {
                    eprintln!("PurePoint: could not open {url}: {error}");
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}
