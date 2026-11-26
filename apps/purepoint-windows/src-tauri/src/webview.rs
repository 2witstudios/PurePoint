//! WebView2 settings Tauri does not expose in `tauri.conf.json`.

use tauri::WebviewWindow;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ,
    COREWEBVIEW2_PERMISSION_STATE_ALLOW, ICoreWebView2Settings, ICoreWebView2Settings3,
};
use webview2_com::PermissionRequestedEventHandler;
use windows_core::Interface;

/// When `lock_down` is set, turns off the WebView2 browser accelerator keys
/// (F5 and Ctrl+R reload, Ctrl+P print, Ctrl+F find, F12 DevTools and so on),
/// the default context menu and DevTools, so the window behaves like an app
/// rather than a browser tab. Otherwise turns them on.
///
/// Either way, grants the page clipboard reads without a prompt: a terminal's
/// Ctrl+Shift+V reads the clipboard (WIN-7.3). The window only ever loads the
/// app's own bundle.
pub fn configure(window: &WebviewWindow, lock_down: bool) -> tauri::Result<()> {
    window.with_webview(move |webview| {
        // SAFETY: plain COM getters and setters on the live controller of this
        // window's WebView2, called on the thread Tauri hands the webview to.
        let applied = unsafe { apply(&webview, !lock_down) };
        if let Err(error) = applied {
            eprintln!("PurePoint: could not configure WebView2 settings: {error}");
        }
    })
}

unsafe fn apply(
    webview: &tauri::webview::PlatformWebview,
    enabled: bool,
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
            &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                let Some(args) = args else { return Ok(()) };
                let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                args.PermissionKind(&mut kind)?;
                if kind == COREWEBVIEW2_PERMISSION_KIND_CLIPBOARD_READ {
                    args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}
