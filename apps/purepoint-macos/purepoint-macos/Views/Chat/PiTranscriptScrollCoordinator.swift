import Foundation

/// Keeps ScrollViewReader actions out of SwiftUI/AppKit's layout stack.
@MainActor final class PiTranscriptScrollCoordinator {
    struct Geometry: Equatable {
        let contentHeight: CGFloat
        let viewportHeight: CGFloat
        let visibleBottom: CGFloat
    }

    private struct Size: Hashable {
        let content: CGFloat
        let viewport: CGFloat
    }

    private var size: Size?
    private var atBottom = false
    private var attempted = Set<Size>()
    private var pending: UUID?

    func update(contentHeight: CGFloat, viewportHeight: CGFloat, visibleBottom: CGFloat) {
        size = Size(content: contentHeight, viewport: viewportHeight)
        atBottom = viewportHeight > 0 && visibleBottom >= contentHeight - 1
    }

    func outputChanged() {
        attempted.removeAll()
    }

    func cancel() {
        pending = nil
    }

    func reset() {
        cancel()
        size = nil
        atBottom = false
        outputChanged()
    }

    func request(canScroll: @escaping @MainActor () -> Bool, scroll: @escaping @MainActor () -> Void) {
        guard pending == nil else { return }
        let token = UUID()
        pending = token
        // A main-queue hop unwinds NSHostingView.layout before touching ScrollActionDispatcher.
        DispatchQueue.main.async { [weak self] in
            guard let self, self.pending == token else { return }
            self.pending = nil
            guard canScroll(), !self.atBottom else { return }
            if let size = self.size {
                guard size.viewport > 0, self.attempted.insert(size).inserted else { return }
            }
            // Remember every attempted size until output changes, including alternating lazy
            // layout estimates. Our own scrolling must not create an unbounded feedback loop.
            scroll()
        }
    }
}
