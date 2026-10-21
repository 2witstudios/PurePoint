#if os(iOS)
import CoreText
import UIKit

/// A read-only Core Text body with the Mac text view's selection: the iPad base of the diff
/// and inline code twins. It lays `text` out with `CoreTextLayout`, draws it, and selects it
/// with a long press and drag, a double tap (word) or triple tap (line), a pointer drag, or
/// ⌘A, copying with ⌘C or the edit menu. Scrolling belongs to the enclosing scroll view, as
/// on the Mac.
class CoreTextBodyView: UIView, UIEditMenuInteractionDelegate, UIGestureRecognizerDelegate,
    UIPointerInteractionDelegate
{
    /// The text, with UIKit attributes; `.backgroundColor` runs are drawn by subclasses.
    var text = NSAttributedString() {
        didSet {
            selectedRange = nil
            invalidateTextLayout()
        }
    }

    private(set) var selectedRange: NSRange? {
        didSet { setNeedsDisplay() }
    }

    private let inset: CGSize
    private let padding: CGFloat
    private let textLeft: CGFloat
    private var cachedLayout: CoreTextLayout?
    private var cachedWidth: CGFloat?
    private var selectionAnchor: NSRange?
    private lazy var editMenu = UIEditMenuInteraction(delegate: self)

    /// - Parameters:
    ///   - inset: The text container inset.
    ///   - padding: The line fragment padding.
    ///   - textLeft: Where the text container's frame starts in the view (after any gutter).
    init(inset: CGSize, padding: CGFloat, textLeft: CGFloat = 0) {
        self.inset = inset
        self.padding = padding
        self.textLeft = textLeft
        super.init(frame: .zero)
        isOpaque = true
        contentMode = .redraw
        isAccessibilityElement = true
        accessibilityTraits = .staticText
        addInteraction(editMenu)
        addInteraction(UIPointerInteraction(delegate: self))
        installGestures()
        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (view: CoreTextBodyView, _) in
            view.traitsDidChange()
        }
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) not supported") }

    // MARK: - Layout

    /// The text container's width for a view `width` wide; the Mac twins size their containers
    /// 8 points narrower than their text view.
    func containerWidth(forViewWidth width: CGFloat) -> CGFloat {
        max(1, width - textLeft - 8)
    }

    /// The layout at `width`, cached until the text, width or appearance changes.
    func textLayout(forWidth width: CGFloat) -> CoreTextLayout {
        if let cachedLayout, cachedWidth == width { return cachedLayout }
        let layout = CoreTextLayout(
            text: text, width: containerWidth(forViewWidth: width),
            inset: CGSize(width: textLeft + inset.width, height: inset.height), padding: padding,
            traits: traitCollection)
        cachedLayout = layout
        cachedWidth = width
        return layout
    }

    /// The measured height at `width`: the used text height plus the insets, rounded up.
    func textHeight(for width: CGFloat) -> CGFloat {
        ceil(textLayout(forWidth: width).height)
    }

    func invalidateTextLayout() {
        cachedLayout = nil
        cachedWidth = nil
        accessibilityValue = text.string
        invalidateIntrinsicContentSize()
        setNeedsDisplay()
    }

    /// Re-resolve dynamic colours when the appearance changes, as the Mac views re-render.
    func traitsDidChange() {
        invalidateTextLayout()
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        setNeedsDisplay()
    }

    // MARK: - Drawing

    /// Subclasses fill row backgrounds and gutters here, under the selection and text.
    func drawBackground(_ layout: CoreTextLayout, in rect: CGRect) {}

    override func draw(_ rect: CGRect) {
        guard let context = UIGraphicsGetCurrentContext() else { return }
        let layout = textLayout(forWidth: bounds.width)
        drawBackground(layout, in: rect)

        if let selectedRange {
            (isFirstResponder ? UIColor.tintColor.withAlphaComponent(0.3) : UIColor.systemFill).setFill()
            for selection in layout.selectionRects(for: selectedRange) where selection.intersects(rect) {
                UIRectFillUsingBlendMode(selection, .normal)
            }
        }

        context.saveGState()
        context.textMatrix = .identity
        context.translateBy(x: 0, y: bounds.height)
        context.scaleBy(x: 1, y: -1)
        for line in layout.lines where line.frame.intersects(rect) {
            context.textPosition = CGPoint(x: line.origin.x, y: bounds.height - line.baseline)
            CTLineDraw(line.ctLine, context)
        }
        context.restoreGState()
    }

    // MARK: - Selection

    func select(_ range: NSRange?) {
        let length = (text.string as NSString).length
        selectedRange = range.map { NSIntersectionRange($0, NSRange(location: 0, length: length)) }
            .flatMap { $0.length > 0 ? $0 : nil }
    }

    var selectedText: String? {
        guard let selectedRange else { return nil }
        return (text.string as NSString).substring(with: selectedRange)
    }

    private func installGestures() {
        let longPress = UILongPressGestureRecognizer(target: self, action: #selector(handleLongPress(_:)))
        longPress.minimumPressDuration = 0.35
        addGestureRecognizer(longPress)

        let tripleTap = UITapGestureRecognizer(target: self, action: #selector(handleTripleTap(_:)))
        tripleTap.numberOfTapsRequired = 3
        addGestureRecognizer(tripleTap)

        let doubleTap = UITapGestureRecognizer(target: self, action: #selector(handleDoubleTap(_:)))
        doubleTap.numberOfTapsRequired = 2
        doubleTap.require(toFail: tripleTap)
        addGestureRecognizer(doubleTap)

        let tap = UITapGestureRecognizer(target: self, action: #selector(handleTap(_:)))
        tap.require(toFail: doubleTap)
        addGestureRecognizer(tap)

        let secondaryClick = UITapGestureRecognizer(target: self, action: #selector(handleSecondaryClick(_:)))
        secondaryClick.buttonMaskRequired = .secondary
        addGestureRecognizer(secondaryClick)

        // A pointer drag selects, as a mouse drag does in the Mac text view. Touch drags are
        // left to the enclosing scroll view.
        let pointerDrag = UIPanGestureRecognizer(target: self, action: #selector(handlePointerDrag(_:)))
        pointerDrag.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.indirectPointer.rawValue)]
        pointerDrag.delegate = self
        addGestureRecognizer(pointerDrag)
    }

    @objc private func handleLongPress(_ gesture: UILongPressGestureRecognizer) {
        let layout = textLayout(forWidth: bounds.width)
        let index = layout.characterIndex(at: gesture.location(in: self))
        switch gesture.state {
        case .began:
            becomeFirstResponder()
            let word = layout.wordRange(at: index)
            selectionAnchor = word
            select(word)
        case .changed:
            guard let anchor = selectionAnchor else { return }
            select(NSUnionRange(anchor, NSRange(location: index, length: 0)))
        case .ended:
            selectionAnchor = nil
            showEditMenu(at: gesture.location(in: self))
        default:
            selectionAnchor = nil
        }
    }

    @objc private func handleDoubleTap(_ gesture: UITapGestureRecognizer) {
        let layout = textLayout(forWidth: bounds.width)
        becomeFirstResponder()
        select(layout.wordRange(at: layout.characterIndex(at: gesture.location(in: self))))
        showEditMenu(at: gesture.location(in: self))
    }

    @objc private func handleTripleTap(_ gesture: UITapGestureRecognizer) {
        let layout = textLayout(forWidth: bounds.width)
        becomeFirstResponder()
        select(layout.paragraphRange(at: layout.characterIndex(at: gesture.location(in: self))))
        showEditMenu(at: gesture.location(in: self))
    }

    @objc private func handleTap(_ gesture: UITapGestureRecognizer) {
        becomeFirstResponder()
        select(nil)
    }

    @objc private func handleSecondaryClick(_ gesture: UITapGestureRecognizer) {
        becomeFirstResponder()
        showEditMenu(at: gesture.location(in: self))
    }

    @objc private func handlePointerDrag(_ gesture: UIPanGestureRecognizer) {
        let layout = textLayout(forWidth: bounds.width)
        let index = layout.characterIndex(at: gesture.location(in: self))
        switch gesture.state {
        case .began:
            becomeFirstResponder()
            let start = layout.characterIndex(at: CGPoint(
                x: gesture.location(in: self).x - gesture.translation(in: self).x,
                y: gesture.location(in: self).y - gesture.translation(in: self).y))
            selectionAnchor = NSRange(location: start, length: 0)
            select(NSUnionRange(selectionAnchor!, NSRange(location: index, length: 0)))
        case .changed:
            guard let anchor = selectionAnchor else { return }
            select(NSUnionRange(anchor, NSRange(location: index, length: 0)))
        default:
            selectionAnchor = nil
        }
    }

    private func showEditMenu(at point: CGPoint) {
        editMenu.presentEditMenu(with: UIEditMenuConfiguration(identifier: nil, sourcePoint: point))
    }

    func editMenuInteraction(
        _ interaction: UIEditMenuInteraction, targetRectFor configuration: UIEditMenuConfiguration
    ) -> CGRect {
        guard let selectedRange else { return .null }
        return textLayout(forWidth: bounds.width).boundingRect(for: selectedRange)
    }

    // MARK: - Responder (⌘C, ⌘A, edit menu)

    override var canBecomeFirstResponder: Bool { true }

    override func becomeFirstResponder() -> Bool {
        let became = super.becomeFirstResponder()
        setNeedsDisplay()
        return became
    }

    override func resignFirstResponder() -> Bool {
        let resigned = super.resignFirstResponder()
        setNeedsDisplay()
        return resigned
    }

    override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        switch action {
        case #selector(copy(_:)): selectedRange != nil
        case #selector(selectAll(_:)): text.length > 0
        default: false
        }
    }

    override func copy(_ sender: Any?) {
        guard let selectedText else { return }
        UIPasteboard.general.string = selectedText
    }

    override func selectAll(_ sender: Any?) {
        select(NSRange(location: 0, length: text.length))
    }

    // MARK: - Pointer (I-beam over text, as the Mac text view's cursor)

    func pointerInteraction(
        _ interaction: UIPointerInteraction, regionFor request: UIPointerRegionRequest,
        defaultRegion: UIPointerRegion
    ) -> UIPointerRegion? {
        defaultRegion
    }

    func pointerInteraction(_ interaction: UIPointerInteraction, styleFor region: UIPointerRegion) -> UIPointerStyle? {
        UIPointerStyle(shape: .verticalBeam(length: 14), constrainedAxes: [])
    }
}
#endif
