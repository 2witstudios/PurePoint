#if os(iOS)
import SwiftUI
import UIKit

/// Placeholder twin of the Mac Markdown preview, with the same inputs. The `WKWebView` preview
/// over `MarkdownHTMLRenderer` replaces it (IPAD-6.14).
struct MarkdownPreviewView: UIViewRepresentable {
    let markdown: String
    /// Called with a resolved absolute path when a relative link is clicked.
    var baseDirectory: String?
    /// The document lives on another machine.
    var isRemote = false
    var onOpenFile: ((String) -> Void)?

    static let remoteScheme = "purepoint-remote"

    func makeUIView(context: Context) -> UIView {
        let view = UIView()
        NotYetOnIPad.addLabel("MarkdownPreviewView", to: view)
        return view
    }

    func updateUIView(_ uiView: UIView, context: Context) {}
}
#endif
