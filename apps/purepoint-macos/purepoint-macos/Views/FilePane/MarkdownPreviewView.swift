import SwiftUI
import WebKit

/// Read-only rendered markdown. Content is converted by `MarkdownHTMLRenderer` (which escapes
/// all text), and navigation is intercepted so the web view never leaves the rendered page.
struct MarkdownPreviewView: NSViewRepresentable {
    let markdown: String
    /// Called with a resolved absolute path when a relative link is clicked.
    var baseDirectory: String?
    var onOpenFile: ((String) -> Void)?

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
        let base = baseDirectory.map { URL(fileURLWithPath: $0, isDirectory: true) }
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
            } else if url.isFileURL, let base = baseDirectory {
                let path = url.standardizedFileURL.path
                if path.hasPrefix(base) { onOpenFile?(path) }
            }
        }
    }
}
