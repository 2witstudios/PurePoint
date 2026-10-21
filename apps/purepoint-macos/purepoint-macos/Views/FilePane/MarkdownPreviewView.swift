#if os(macOS)
import SwiftUI
import WebKit

/// Read-only rendered markdown. Content is converted by `MarkdownHTMLRenderer` (which escapes
/// all text), and navigation is intercepted so the web view never leaves the rendered page.
struct MarkdownPreviewView: NSViewRepresentable {
    let markdown: String
    /// Called with a resolved absolute path when a relative link is clicked.
    var baseDirectory: String?
    /// The document lives on another machine: relative links still resolve against its
    /// directory, but never as `file://` URLs that would read this Mac's disk.
    var isRemote = false
    var onOpenFile: ((String) -> Void)?

    static let remoteScheme = "purepoint-remote"

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        view.setValue(false, forKey: "drawsBackground")
        return view
    }

    func updateNSView(_ view: WKWebView, context: Context) {
        context.coordinator.baseDirectory = baseDirectory
        context.coordinator.onOpenFile = onOpenFile
        guard context.coordinator.lastMarkdown != markdown else { return }
        context.coordinator.lastMarkdown = markdown
        let base = baseDirectory.flatMap { directory -> URL? in
            guard isRemote else { return URL(fileURLWithPath: directory, isDirectory: true) }
            var components = URLComponents()
            components.scheme = Self.remoteScheme
            components.host = "files"
            components.path = directory.hasSuffix("/") ? directory : directory + "/"
            return components.url
        }
        view.loadHTMLString(MarkdownHTMLRenderer.page(body: MarkdownHTMLRenderer.render(markdown)), baseURL: base)
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        var lastMarkdown: String?
        var baseDirectory: String?
        var onOpenFile: ((String) -> Void)?

        func webView(
            _ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
            decisionHandler: @escaping @MainActor @Sendable (WKNavigationActionPolicy) -> Void
        ) {
            guard action.navigationType == .linkActivated, let url = action.request.url else {
                decisionHandler(.allow)
                return
            }
            decisionHandler(.cancel)
            if url.scheme == "http" || url.scheme == "https" || url.scheme == "mailto" {
                NSWorkspace.shared.open(url)
            } else if url.isFileURL || url.scheme == MarkdownPreviewView.remoteScheme, let base = baseDirectory {
                let path = url.standardized.path
                if path.hasPrefix(base) { onOpenFile?(path) }
            }
        }
    }
}
#endif
