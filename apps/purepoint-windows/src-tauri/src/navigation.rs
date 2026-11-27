//! What the main webview may load (WIN-7.3): only the app itself.
//!
//! Every link the UI shows opens through the opener plugin, so the webview never navigates on
//! its own. Should a top-level navigation still start (a link the UI missed, a script, a
//! redirect), [`plugin`]'s guard refuses it, handing an `http(s)` address to the user's
//! browser instead. New-window requests are refused by wry already; `webview.rs` hands their
//! `http(s)` addresses to the browser the same way. The same origin check decides which pages
//! WebView2 grants clipboard reads (`webview.rs`).

use tauri::Url;

/// What to do with a top-level navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    /// The app's own page.
    Allow,
    /// Somewhere else on the web: open it in the browser, stay on the app.
    OpenExternally,
    /// Anything else (`file:`, `javascript:`, a custom protocol): refuse.
    Block,
}

/// The app's own origin: Tauri's bundled assets (`http://tauri.localhost` on Windows, or
/// `https://` / `tauri://localhost`), or the dev server when this is a debug build.
pub fn is_app_url(url: &Url, dev_url: Option<&Url>) -> bool {
    let bundled = match url.scheme() {
        "http" | "https" => url.host_str() == Some("tauri.localhost") && url.port().is_none(),
        "tauri" => url.host_str() == Some("localhost"),
        _ => false,
    };
    bundled
        || dev_url.is_some_and(|dev| {
            cfg!(debug_assertions)
                && url.scheme() == dev.scheme()
                && url.host_str() == dev.host_str()
                && url.port_or_known_default() == dev.port_or_known_default()
        })
}

/// The decision for a top-level navigation to `url`.
pub fn navigation(url: &Url, dev_url: Option<&Url>) -> Navigation {
    if is_app_url(url, dev_url) || url.as_str() == "about:blank" {
        Navigation::Allow
    } else if matches!(url.scheme(), "http" | "https") {
        Navigation::OpenExternally
    } else {
        Navigation::Block
    }
}

/// The guard as a plugin: its `on_navigation` sees every webview's top-level navigations.
pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("pu-navigation")
        .on_navigation(|webview, url| {
            use tauri::Manager;
            let dev_url = webview.config().build.dev_url.clone();
            match navigation(url, dev_url.as_ref()) {
                Navigation::Allow => true,
                Navigation::OpenExternally => {
                    if let Err(error) = tauri_plugin_opener::open_url(url.as_str(), None::<&str>) {
                        eprintln!("PurePoint: could not open {url}: {error}");
                    }
                    false
                }
                Navigation::Block => {
                    eprintln!("PurePoint: refused to navigate the window to {url}");
                    false
                }
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn given_the_app_should_allow_it() {
        for app in [
            "http://tauri.localhost/",
            "http://tauri.localhost/index.html#x",
            "https://tauri.localhost/",
            "tauri://localhost/",
            "about:blank",
        ] {
            assert_eq!(navigation(&url(app), None), Navigation::Allow, "{app}");
        }
    }

    #[test]
    fn given_the_web_should_open_it_in_the_browser() {
        for web in [
            "https://example.com/",
            "http://tauri.localhost.example.com/",
            "http://tauri.localhost:8080/",
            "http://localhost:1420/",
        ] {
            assert_eq!(
                navigation(&url(web), None),
                Navigation::OpenExternally,
                "{web}"
            );
        }
    }

    #[test]
    fn given_other_schemes_should_block_them() {
        for other in [
            "file:///C:/Windows/System32/calc.exe",
            "file://server/share/x.html",
            "javascript:alert(1)",
            "ms-settings:privacy",
            "search-ms:query=x",
            "data:text/html,x",
        ] {
            assert_eq!(navigation(&url(other), None), Navigation::Block, "{other}");
        }
    }

    #[test]
    fn given_the_dev_server_should_allow_it_in_debug_builds_only() {
        let dev = url("http://localhost:1420");
        let expected = if cfg!(debug_assertions) {
            Navigation::Allow
        } else {
            Navigation::OpenExternally
        };
        assert_eq!(
            navigation(&url("http://localhost:1420/"), Some(&dev)),
            expected
        );
        assert_eq!(
            navigation(&url("http://localhost:1421/"), Some(&dev)),
            Navigation::OpenExternally
        );
    }
}
