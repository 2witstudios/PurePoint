#if os(iOS)
import SwiftUI
import UIKit

/// Placeholder twin of the Mac editor container, with the same content and callback API. It
/// keeps the content it is given, so the file pane's save and dirty logic is unchanged, and
/// shows a label until the `UITextView` editor lands (IPAD-6.5).
class EditorNSView: UIView {
    private var content = ""
    private var language: EditorLanguage?
    private var isEditable = true

    var onContentChanged: ((String) -> Void)?
    var onSave: (() -> Void)?

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = Theme.cardBackground
        NotYetOnIPad.addLabel("EditorNSView", to: self)
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) not supported")
    }

    func setLanguage(_ language: EditorLanguage) {
        self.language = language
    }

    func setContent(_ content: String) {
        self.content = content
    }

    func getContent() -> String {
        content
    }

    func setEditable(_ editable: Bool) {
        isEditable = editable
    }
}

// MARK: - SwiftUI Bridge

struct EditorContentRepresentable: UIViewRepresentable {
    let content: String
    let language: EditorLanguage
    let isBinary: Bool
    let isEditable: Bool
    var onContentChanged: ((String) -> Void)?
    var onSave: (() -> Void)?

    func makeUIView(context: Context) -> EditorNSView {
        let view = EditorNSView(frame: .zero)
        view.onContentChanged = onContentChanged
        view.onSave = onSave
        view.setEditable(isEditable && !isBinary)
        view.setContent(isBinary ? "" : content)
        view.setLanguage(language)
        return view
    }

    func updateUIView(_ uiView: EditorNSView, context: Context) {
        uiView.onContentChanged = onContentChanged
        uiView.onSave = onSave
        uiView.setEditable(isEditable && !isBinary)
        if !isBinary && uiView.getContent() != content {
            uiView.setContent(content)
        }
        uiView.setLanguage(language)
    }
}
#endif
