#if os(iOS)
import SwiftUI
import UIKit

/// iOS twin of the Mac editor container: a TextKit 2 `EditorTextView` with the line-number gutter
/// beside it. Wrapped by EditorContentRepresentable for SwiftUI integration.
class EditorNSView: UIView {
    private let textView: EditorTextView
    private var lineRuler: LineNumberRulerView?
    private var highlightManager: SyntaxHighlightManager?
    private var suppressCallbacks = false

    var onContentChanged: ((String) -> Void)?
    var onSave: (() -> Void)?

    /// The text view, for tests and for focus.
    var editorTextView: EditorTextView { textView }

    override init(frame: CGRect) {
        // An explicit TextKit 2 stack, as the Mac builds its own: the text view must never fall
        // back to TextKit 1, which Neon and the gutter do not use.
        let contentStorage = NSTextContentStorage()
        let layoutManager = NSTextLayoutManager()
        contentStorage.addTextLayoutManager(layoutManager)
        let textContainer = NSTextContainer(size: CGSize(width: 0, height: CGFloat.greatestFiniteMagnitude))
        textContainer.widthTracksTextView = true
        layoutManager.textContainer = textContainer

        textView = EditorTextView(frame: .zero, textContainer: textContainer)
        super.init(frame: frame)
        setupViews()
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) not supported")
    }

    private func setupViews() {
        backgroundColor = Theme.cardBackground
        textView.backgroundColor = Theme.cardBackground
        textView.translatesAutoresizingMaskIntoConstraints = false
        addSubview(textView)

        // Line numbers
        let ruler = LineNumberRulerView(textView: textView)
        ruler.translatesAutoresizingMaskIntoConstraints = false
        addSubview(ruler)
        lineRuler = ruler

        NSLayoutConstraint.activate([
            ruler.topAnchor.constraint(equalTo: topAnchor),
            ruler.leadingAnchor.constraint(equalTo: leadingAnchor),
            ruler.bottomAnchor.constraint(equalTo: bottomAnchor),
            ruler.widthAnchor.constraint(equalToConstant: LineNumberRulerView.gutterWidth),
            textView.topAnchor.constraint(equalTo: topAnchor),
            textView.leadingAnchor.constraint(equalTo: ruler.trailingAnchor),
            textView.trailingAnchor.constraint(equalTo: trailingAnchor),
            textView.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])

        // Callbacks
        textView.onContentChanged = { [weak self] newContent in
            guard let self, !self.suppressCallbacks else { return }
            self.onContentChanged?(newContent)
        }

        textView.onSave = { [weak self] in
            self?.onSave?()
        }

        textView.onVisibleTextChanged = { [weak self] in
            self?.lineRuler?.setNeedsDisplay()
        }

        highlightManager = SyntaxHighlightManager(textView: textView)

        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (self: Self, _) in
            self.textView.backgroundColor = Theme.cardBackground
            self.highlightManager?.invalidate()
        }
    }

    func setLanguage(_ language: EditorLanguage) {
        highlightManager?.setLanguage(language)
    }

    func setContent(_ content: String) {
        suppressCallbacks = true
        textView.replaceAllText(content)
        suppressCallbacks = false
        // Programmatic text changes post no change notification, so the gutter rebuilds here.
        lineRuler?.rebuildLineStarts()
        lineRuler?.setNeedsDisplay()
    }

    func getContent() -> String {
        textView.text
    }

    func setEditable(_ editable: Bool) {
        textView.isEditable = editable
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

        // Only update content if it differs (avoid resetting cursor)
        if !isBinary && uiView.getContent() != content {
            uiView.setContent(content)
        }

        uiView.setLanguage(language)
    }
}
#endif
