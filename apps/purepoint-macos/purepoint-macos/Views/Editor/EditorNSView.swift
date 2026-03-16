import AppKit
import SwiftUI

/// Container NSView: NSScrollView with EditorTextView + LineNumberRulerView.
/// Wrapped by EditorContentRepresentable for SwiftUI integration.
class EditorNSView: NSView {
    private let scrollView = NSScrollView()
    private let textView: EditorTextView
    private var lineRuler: LineNumberRulerView?
    private var highlightManager: SyntaxHighlightManager?
    private var suppressCallbacks = false

    var onContentChanged: ((String) -> Void)?
    var onSave: (() -> Void)?

    override init(frame frameRect: NSRect) {
        let textStorage = NSTextStorage()
        let layoutManager = NSLayoutManager()
        textStorage.addLayoutManager(layoutManager)
        let textContainer = NSTextContainer(containerSize: NSSize(width: 0, height: CGFloat.greatestFiniteMagnitude))
        textContainer.widthTracksTextView = true
        layoutManager.addTextContainer(textContainer)

        textView = EditorTextView(frame: .zero, textContainer: textContainer)
        super.init(frame: frameRect)
        setupViews()
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) not supported")
    }

    private func setupViews() {
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.autoresizingMask = [.width]
        textView.maxSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        textView.backgroundColor = Theme.cardBackground

        scrollView.documentView = textView
        scrollView.hasVerticalScroller = true
        scrollView.hasHorizontalScroller = false
        scrollView.drawsBackground = false
        scrollView.translatesAutoresizingMaskIntoConstraints = false
        addSubview(scrollView)

        NSLayoutConstraint.activate([
            scrollView.topAnchor.constraint(equalTo: topAnchor),
            scrollView.leadingAnchor.constraint(equalTo: leadingAnchor),
            scrollView.trailingAnchor.constraint(equalTo: trailingAnchor),
            scrollView.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])

        // Line numbers
        scrollView.rulersVisible = true
        let ruler = LineNumberRulerView(textView: textView)
        scrollView.verticalRulerView = ruler
        lineRuler = ruler

        // Callbacks
        textView.onContentChanged = { [weak self] newContent in
            guard let self, !self.suppressCallbacks else { return }
            self.onContentChanged?(newContent)
        }

        textView.onSave = { [weak self] in
            self?.onSave?()
        }

        highlightManager = SyntaxHighlightManager(textView: textView)
    }

    func setLanguage(_ language: EditorLanguage) {
        highlightManager?.setLanguage(language)
    }

    func setContent(_ content: String) {
        suppressCallbacks = true
        textView.string = content
        suppressCallbacks = false
        lineRuler?.rebuildLineStarts()
        lineRuler?.needsDisplay = true
    }

    func getContent() -> String {
        textView.string
    }

    func setEditable(_ editable: Bool) {
        textView.isEditable = editable
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        textView.backgroundColor = Theme.cardBackground
        highlightManager?.invalidate()
    }
}

// MARK: - SwiftUI Bridge

struct EditorContentRepresentable: NSViewRepresentable {
    let content: String
    let language: EditorLanguage
    let isBinary: Bool
    let isEditable: Bool
    var onContentChanged: ((String) -> Void)?
    var onSave: (() -> Void)?

    func makeNSView(context: Context) -> EditorNSView {
        let view = EditorNSView(frame: .zero)
        view.onContentChanged = onContentChanged
        view.onSave = onSave
        view.setEditable(isEditable && !isBinary)
        view.setContent(isBinary ? "" : content)
        view.setLanguage(language)
        return view
    }

    func updateNSView(_ nsView: EditorNSView, context: Context) {
        nsView.onContentChanged = onContentChanged
        nsView.onSave = onSave
        nsView.setEditable(isEditable && !isBinary)

        // Only update content if it differs (avoid resetting cursor)
        if !isBinary && nsView.getContent() != content {
            nsView.setContent(content)
        }

        nsView.setLanguage(language)
    }
}
