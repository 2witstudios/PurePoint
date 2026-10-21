#if os(iOS)
import SwiftUI
import UIKit
import WebKit

/// iOS twin of the Mac read-only rendered markdown. Content is converted by
/// `MarkdownHTMLRenderer` (which escapes all text), and navigation is intercepted so the web
/// view never leaves the rendered page.
struct MarkdownPreviewView: UIViewRepresentable {
    let markdown: String
    /// Called with a resolved absolute path when a relative link is clicked.
    var baseDirectory: String?
    /// The document lives on another machine: relative links still resolve against its
    /// directory, but never as `file://` URLs that would read this device's disk.
    var isRemote = false
    var onOpenFile: ((String) -> Void)?

    nonisolated static let remoteScheme = "purepoint-remote"

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeUIView(context: Context) -> WKWebView {
        let config = WKWebViewConfiguration()
        config.defaultWebpagePreferences.allowsContentJavaScript = false
        config.dataDetectorTypes = []
        let view = WKWebView(frame: .zero, configuration: config)
        view.navigationDelegate = context.coordinator
        // The Mac's `drawsBackground = false`: the page's own Canvas background shows through.
        view.isOpaque = false
        view.backgroundColor = .clear
        view.scrollView.backgroundColor = .clear
        view.allowsLinkPreview = false
        return view
    }

    func updateUIView(_ view: WKWebView, context: Context) {
        context.coordinator.baseDirectory = baseDirectory
        context.coordinator.onOpenFile = onOpenFile
        guard context.coordinator.lastMarkdown != markdown else { return }
        context.coordinator.lastMarkdown = markdown
        view.loadHTMLString(
            MarkdownHTMLRenderer.page(body: MarkdownHTMLRenderer.render(markdown)),
            baseURL: Self.baseURL(directory: baseDirectory, isRemote: isRemote))
    }

    /// The page's base URL: the document's directory, as a `file://` URL only when local.
    static func baseURL(directory: String?, isRemote: Bool) -> URL? {
        directory.flatMap { directory -> URL? in
            guard isRemote else { return URL(fileURLWithPath: directory, isDirectory: true) }
            var components = URLComponents()
            components.scheme = remoteScheme
            components.host = "files"
            components.path = directory.hasSuffix("/") ? directory : directory + "/"
            return components.url
        }
    }

    /// What a clicked link does: open externally, open a file below the document's directory,
    /// or nothing.
    enum LinkAction: Equatable {
        case openExternally(URL)
        case openFile(String)
        case ignore
    }

    nonisolated static func linkAction(for url: URL, baseDirectory: String?) -> LinkAction {
        if url.scheme == "http" || url.scheme == "https" || url.scheme == "mailto" {
            return .openExternally(url)
        } else if url.isFileURL || url.scheme == remoteScheme, let base = baseDirectory {
            let path = url.standardized.path
            if path.hasPrefix(base) { return .openFile(path) }
        }
        return .ignore
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
            switch MarkdownPreviewView.linkAction(for: url, baseDirectory: baseDirectory) {
            case .openExternally(let url): UIApplication.shared.open(url)
            case .openFile(let path): onOpenFile?(path)
            case .ignore: break
            }
        }
    }
}
#endif
