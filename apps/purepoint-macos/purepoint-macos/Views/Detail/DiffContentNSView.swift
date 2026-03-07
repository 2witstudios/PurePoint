import AppKit
import SwiftUI

/// AppKit view that renders diff content using NSTextView with per-line background colors,
/// line-number gutters, and tab-stop alignment. Wrapped via DiffContentRepresentable for SwiftUI.
class DiffContentNSView: NSView {
    private let textView = NSTextView()
    private var hunks: [Hunk] = []
    private var cachedHeight: CGFloat = 0

    init(hunks: [Hunk]) {
        self.hunks = hunks
        super.init(frame: .zero)
        setupTextView()
        renderDiff()
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) not supported")
    }

    func update(hunks: [Hunk]) {
        guard hunks != self.hunks else { return }
        self.hunks = hunks
        renderDiff()
    }

    private func setupTextView() {
        textView.isEditable = false
        textView.isSelectable = true
        textView.drawsBackground = true
        textView.backgroundColor = Theme.cardBackground
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.textContainer?.widthTracksTextView = true
        textView.textContainerInset = NSSize(width: 4, height: 6)
        textView.autoresizingMask = [.width]
        textView.translatesAutoresizingMaskIntoConstraints = false
        addSubview(textView)

        NSLayoutConstraint.activate([
            textView.topAnchor.constraint(equalTo: topAnchor),
            textView.leadingAnchor.constraint(equalTo: leadingAnchor),
            textView.trailingAnchor.constraint(equalTo: trailingAnchor),
            textView.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
    }

    private func renderDiff() {
        let attributed = buildDiffAttributedString()
        textView.textStorage?.setAttributedString(attributed)

        // Compute and cache height once per render, so intrinsicContentSize
        // never calls ensureLayout — that was causing freezes during rapid scroll
        // as SwiftUI queries intrinsicContentSize repeatedly during layout.
        if let container = textView.textContainer,
            let layoutManager = textView.layoutManager,
            textView.textStorage?.length ?? 0 > 0
        {
            layoutManager.ensureLayout(for: container)
            cachedHeight = layoutManager.usedRect(for: container).height + 12
        } else {
            cachedHeight = 0
        }
        invalidateIntrinsicContentSize()
    }

    override var intrinsicContentSize: NSSize {
        NSSize(width: NSView.noIntrinsicMetric, height: cachedHeight)
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        textView.backgroundColor = Theme.cardBackground
        renderDiff()
    }

    private func buildDiffAttributedString() -> NSAttributedString {
        let result = NSMutableAttributedString()
        let monoFont = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        let lineNoFont = NSFont.monospacedSystemFont(ofSize: 10, weight: .regular)

        let tabStops = [
            NSTextTab(textAlignment: .right, location: 36),
            NSTextTab(textAlignment: .right, location: 72),
            NSTextTab(textAlignment: .left, location: 82),
        ]

        for (hunkIdx, hunk) in hunks.enumerated() {
            // Hunk separator between hunks
            if hunkIdx > 0 {
                let sepPara = NSMutableParagraphStyle()
                sepPara.alignment = .center
                sepPara.paragraphSpacingBefore = 4
                sepPara.paragraphSpacing = 4
                result.append(
                    NSAttributedString(
                        string: "···\n",
                        attributes: [
                            .font: NSFont.systemFont(ofSize: 11),
                            .foregroundColor: NSColor.tertiaryLabelColor,
                            .paragraphStyle: sepPara,
                            .backgroundColor: Theme.hunkSeparator,
                        ]))
            }

            for line in hunk.lines {
                let para = NSMutableParagraphStyle()
                para.tabStops = tabStops

                let bgColor: NSColor
                let fgColor: NSColor
                switch line.type {
                case .addition:
                    bgColor = Theme.additionBackground
                    fgColor = Theme.additionText
                case .deletion:
                    bgColor = Theme.deletionBackground
                    fgColor = Theme.deletionText
                case .context:
                    bgColor = Theme.cardBackground
                    fgColor = Theme.primaryText
                }

                // Old line number
                let oldNo = line.oldLineNo.map { String($0) } ?? ""
                result.append(
                    NSAttributedString(
                        string: "\t\(oldNo)",
                        attributes: [
                            .font: lineNoFont,
                            .foregroundColor: NSColor.tertiaryLabelColor,
                            .backgroundColor: bgColor,
                            .paragraphStyle: para,
                        ]))

                // New line number
                let newNo = line.newLineNo.map { String($0) } ?? ""
                result.append(
                    NSAttributedString(
                        string: "\t\(newNo)",
                        attributes: [
                            .font: lineNoFont,
                            .foregroundColor: NSColor.tertiaryLabelColor,
                            .backgroundColor: bgColor,
                        ]))

                // Code content
                result.append(
                    NSAttributedString(
                        string: "\t\(line.content)\n",
                        attributes: [
                            .font: monoFont,
                            .foregroundColor: fgColor,
                            .backgroundColor: bgColor,
                        ]))
            }
        }

        return result
    }
}

// MARK: - SwiftUI Bridge

struct DiffContentRepresentable: NSViewRepresentable {
    let hunks: [Hunk]

    func makeNSView(context: Context) -> DiffContentNSView {
        DiffContentNSView(hunks: hunks)
    }

    func updateNSView(_ nsView: DiffContentNSView, context: Context) {
        nsView.update(hunks: hunks)
    }
}
