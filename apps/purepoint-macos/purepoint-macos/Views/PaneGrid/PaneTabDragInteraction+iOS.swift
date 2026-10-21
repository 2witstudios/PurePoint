#if os(iOS)
import SwiftUI
import UIKit
import UniformTypeIdentifiers

extension TabDragPayload {
    /// The tab a drop onto `targetSurfaceId` moves: the first tab payload that is not the
    /// target itself (a tab dropped on itself does nothing, as on the Mac).
    static func movedSurface(in payloads: [String], onto targetSurfaceId: Int?) -> Int? {
        guard let moved = payloads.lazy.compactMap(surfaceId(from:)).first, moved != targetSurfaceId else {
            return nil
        }
        return moved
    }
}

/// The iPad's tab drag and drop: a `UIDragInteraction` that lifts the tab with the Mac's
/// payload and title preview, and a `UIDropInteraction` that inserts a dropped tab before this
/// one (the Mac's leading accent bar while targeted). It also takes the tab's taps and pointer
/// hover, which the overlay would otherwise hide, and lets touches through to the close button.
/// Drops past the last tab fall to the strip's shared drop destination, which appends.
struct TabDragDropInteraction: UIViewRepresentable {
    let surfaceId: Int
    let title: String
    let closeVisible: Bool
    @Binding var isHovered: Bool
    @Binding var isDropTargeted: Bool
    let onSelect: () -> Void
    let onDrop: (Int) -> Void

    func makeUIView(context: Context) -> TabInteractionView {
        let view = TabInteractionView()
        update(view)
        return view
    }

    func updateUIView(_ view: TabInteractionView, context: Context) {
        update(view)
    }

    private func update(_ view: TabInteractionView) {
        view.surfaceId = surfaceId
        view.title = title
        view.closeVisible = closeVisible
        view.onSelect = onSelect
        view.onDrop = onDrop
        view.onHover = { hovering in if isHovered != hovering { isHovered = hovering } }
        view.onTargeted = { targeted in if isDropTargeted != targeted { isDropTargeted = targeted } }
    }
}

final class TabInteractionView: UIView, UIDragInteractionDelegate, UIDropInteractionDelegate {
    var surfaceId = 0
    var title = ""
    var closeVisible = false
    var onSelect: () -> Void = {}
    var onDrop: (Int) -> Void = { _ in }
    var onHover: (Bool) -> Void = { _ in }
    var onTargeted: (Bool) -> Void = { _ in }

    init() {
        super.init(frame: .zero)
        backgroundColor = .clear
        // The SwiftUI tab underneath carries the accessibility element and its actions.
        isAccessibilityElement = false
        let drag = UIDragInteraction(delegate: self)
        drag.isEnabled = true
        addInteraction(drag)
        addInteraction(UIDropInteraction(delegate: self))
        addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(handleTap)))
        addGestureRecognizer(UIHoverGestureRecognizer(target: self, action: #selector(handleHover(_:))))
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) not supported") }

    /// The tab's trailing 16-point close button (after 6 points of padding), with a little
    /// slop for touch.
    var closeButtonFrame: CGRect {
        CGRect(x: bounds.maxX - 6 - 16 - 2, y: bounds.midY - 10, width: 20, height: 20)
    }

    override func hitTest(_ point: CGPoint, with event: UIEvent?) -> UIView? {
        if closeVisible, closeButtonFrame.contains(point) { return nil }
        return super.hitTest(point, with: event)
    }

    @objc private func handleTap() {
        onSelect()
    }

    @objc private func handleHover(_ gesture: UIHoverGestureRecognizer) {
        switch gesture.state {
        case .began, .changed: onHover(true)
        default: onHover(false)
        }
    }

    // MARK: - Drag

    func dragInteraction(_ interaction: UIDragInteraction, itemsForBeginning session: UIDragSession) -> [UIDragItem] {
        let payload = TabDragPayload.string(for: surfaceId)
        let item = UIDragItem(itemProvider: NSItemProvider(object: payload as NSString))
        item.localObject = payload
        let title = title
        item.previewProvider = { UIDragPreview(view: Self.previewLabel(title)) }
        return [item]
    }

    func dragInteraction(
        _ interaction: UIDragInteraction, previewForLifting item: UIDragItem, session: UIDragSession
    ) -> UITargetedDragPreview? {
        let parameters = UIDragPreviewParameters()
        parameters.backgroundColor = .clear
        let target = UIDragPreviewTarget(container: self, center: CGPoint(x: bounds.midX, y: bounds.midY))
        return UITargetedDragPreview(view: Self.previewLabel(title), parameters: parameters, target: target)
    }

    /// The Mac's drag image: the title in 12-point text with 6 points of padding, in the tab
    /// bar's dark appearance.
    static func previewLabel(_ title: String) -> UIView {
        let label = UILabel()
        label.text = title
        label.font = .systemFont(ofSize: 12)
        label.overrideUserInterfaceStyle = .dark
        label.textColor = .label
        label.sizeToFit()
        let container = UIView(frame: label.bounds.insetBy(dx: -6, dy: -6))
        label.frame.origin = CGPoint(x: 6, y: 6)
        container.addSubview(label)
        container.backgroundColor = .clear
        return container
    }

    // MARK: - Drop

    func dropInteraction(_ interaction: UIDropInteraction, canHandle session: UIDropSession) -> Bool {
        session.canLoadObjects(ofClass: NSString.self)
    }

    func dropInteraction(_ interaction: UIDropInteraction, sessionDidEnter session: UIDropSession) {
        onTargeted(true)
    }

    func dropInteraction(_ interaction: UIDropInteraction, sessionDidUpdate session: UIDropSession) -> UIDropProposal {
        onTargeted(true)
        return UIDropProposal(operation: session.localDragSession == nil ? .copy : .move)
    }

    func dropInteraction(_ interaction: UIDropInteraction, sessionDidExit session: UIDropSession) {
        onTargeted(false)
    }

    func dropInteraction(_ interaction: UIDropInteraction, sessionDidEnd session: UIDropSession) {
        onTargeted(false)
    }

    func dropInteraction(_ interaction: UIDropInteraction, performDrop session: UIDropSession) {
        onTargeted(false)
        let local = session.items.compactMap { $0.localObject as? String }
        if !local.isEmpty {
            if let moved = TabDragPayload.movedSurface(in: local, onto: surfaceId) { onDrop(moved) }
            return
        }
        let target = surfaceId
        _ = session.loadObjects(ofClass: NSString.self) { [weak self] objects in
            let payloads = objects.compactMap { $0 as? String }
            guard let moved = TabDragPayload.movedSurface(in: payloads, onto: target) else { return }
            DispatchQueue.main.async { self?.onDrop(moved) }
        }
    }
}
#endif
