//! WebDriver automation of debug builds (the end-to-end tests, WIN-5.7).
//!
//! msedgedriver starts the app with the browser switches it needs (`--remote-debugging-port=0`,
//! `--enable-automation`, ...) in `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS`, expecting WebView2 to
//! add them to the browser process. Some WebView2 runtimes (157 on GitHub's Windows runners)
//! ignore that variable when the app passes its own `AdditionalBrowserArguments`, as wry always
//! does, so the debugging port never opens and msedgedriver fails with "DevToolsActivePort file
//! doesn't exist". Under tauri-driver (`TAURI_WEBVIEW_AUTOMATION=true`) a debug build therefore
//! passes the driver's switches itself, after wry's defaults.

/// wry's default WebView2 switches (wry `webview2::create_environment`, autoplay on), which an
/// explicit argument string replaces.
const WRY_DEFAULT_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required";

const DISABLE_FEATURES: &str = "--disable-features=";

/// The main window's browser arguments: `None` (wry's defaults) unless a WebDriver session
/// launched a debug build, then wry's defaults plus the driver's switches, with one merged
/// `--disable-features` (Chromium honours only the last one).
pub fn browser_args(automation: Option<&str>, driver_args: Option<&str>) -> Option<String> {
    if !cfg!(debug_assertions) || automation != Some("true") {
        return None;
    }
    let driver_args = driver_args.map(str::trim).filter(|a| !a.is_empty())?;
    let mut features: Vec<&str> = Vec::new();
    let mut rest: Vec<&str> = Vec::new();
    for arg in WRY_DEFAULT_ARGS
        .split_whitespace()
        .chain(driver_args.split_whitespace())
    {
        match arg.strip_prefix(DISABLE_FEATURES) {
            Some(list) => {
                for feature in list.split(',').filter(|f| !f.is_empty()) {
                    if !features.contains(&feature) {
                        features.push(feature);
                    }
                }
            }
            None => rest.push(arg),
        }
    }
    let mut args = vec![format!("{DISABLE_FEATURES}{}", features.join(","))];
    args.extend(rest.into_iter().map(str::to_owned));
    Some(args.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_a_webdriver_session_wry_keeps_its_defaults() {
        assert_eq!(browser_args(None, Some("--remote-debugging-port=0")), None);
        assert_eq!(
            browser_args(Some("false"), Some("--remote-debugging-port=0")),
            None
        );
        assert_eq!(browser_args(Some("true"), None), None);
        assert_eq!(browser_args(Some("true"), Some("  ")), None);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn under_tauri_driver_a_debug_build_passes_the_driver_switches() {
        let args = browser_args(
            Some("true"),
            Some(
                "--disable-features=IgnoreDuplicateNavs,Prewarm --remote-debugging-port=0 --enable-automation",
            ),
        );
        assert_eq!(
            args.as_deref(),
            Some(
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,IgnoreDuplicateNavs,Prewarm \
                 --autoplay-policy=no-user-gesture-required --remote-debugging-port=0 --enable-automation"
            )
        );
    }
}
