import Foundation

/// Coalesces palette open requests across all entry points while their data loads.
@MainActor
final class CommandPaletteOpenCoordinator {
    private var isOpening = false

    @discardableResult
    func request(_ open: @escaping @MainActor () async -> Void) -> Task<Void, Never>? {
        guard !isOpening else { return nil }
        // Set this before scheduling the task so same-turn requests also coalesce.
        isOpening = true
        return Task {
            defer { isOpening = false }
            await open()
        }
    }
}
