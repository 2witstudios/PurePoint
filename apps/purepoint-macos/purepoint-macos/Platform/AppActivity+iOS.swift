#if os(iOS)
import SwiftUI
import UIKit

/// Whether the iPad app is in the background, from the scene phase `AppState` receives through
/// `AppLifecycleHooks.scenePhaseChanged`. It replaces the Mac's `NSApplication` activity checks
/// for background work: pollers wait in `untilForeground()` while the app is in the background
/// instead of making SSH round trips, and refresh as soon as it returns; stream reconnect loops
/// wait there instead of spending their retries on a suspended network.
@MainActor
final class AppActivity {
    static let shared = AppActivity()

    private(set) var isInBackground = false
    private var waiters: [UUID: CheckedContinuation<Void, Never>] = [:]
    private var backgroundTask: UIBackgroundTaskIdentifier = .invalid
    private let backgroundTasks: any BackgroundTaskRunner

    init(backgroundTasks: any BackgroundTaskRunner = UIApplication.shared) {
        self.backgroundTasks = backgroundTasks
    }

    /// Apply a scene phase. Returns true when the app returned from the background.
    @discardableResult
    func apply(_ phase: ScenePhase) -> Bool {
        switch phase {
        case .background:
            guard !isInBackground else { return false }
            isInBackground = true
            beginBackgroundGrace()
            return false
        case .active, .inactive:
            guard isInBackground else { return false }
            isInBackground = false
            endBackgroundGrace()
            let resumed = waiters.values
            waiters.removeAll()
            for waiter in resumed { waiter.resume() }
            return true
        @unknown default:
            return false
        }
    }

    /// Returns at once in the foreground; in the background, when the app returns to the
    /// foreground or the calling task is cancelled.
    func untilForeground() async {
        guard isInBackground else { return }
        let id = UUID()
        await withTaskCancellationHandler {
            await withCheckedContinuation { continuation in
                if !isInBackground || Task.isCancelled {
                    continuation.resume()
                } else {
                    waiters[id] = continuation
                }
            }
        } onCancel: {
            Task { @MainActor [weak self] in self?.resumeWaiter(id) }
        }
    }

    private func resumeWaiter(_ id: UUID) {
        waiters.removeValue(forKey: id)?.resume()
    }

    /// Asks iOS for the usual grace period after leaving the foreground, so streams already
    /// open keep delivering agent signals (and their local notifications) and pending layout
    /// saves finish before the app is suspended.
    private func beginBackgroundGrace() {
        guard backgroundTask == .invalid else { return }
        backgroundTask = backgroundTasks.beginBackgroundTask(withName: "PurePoint background grace") {
            [weak self] in
            // iOS suspends the app once the handler returns, so the task ends before that,
            // not in a later main-actor turn. The handler runs on the main thread.
            MainActor.assumeIsolated { self?.endBackgroundGrace() }
        }
    }

    private func endBackgroundGrace() {
        guard backgroundTask != .invalid else { return }
        backgroundTasks.endBackgroundTask(backgroundTask)
        backgroundTask = .invalid
    }
}

/// Begins and ends background tasks: `UIApplication` in the app, a fake in tests.
@MainActor
protocol BackgroundTaskRunner: AnyObject {
    func beginBackgroundTask(
        withName taskName: String?, expirationHandler handler: (@MainActor @Sendable () -> Void)?
    ) -> UIBackgroundTaskIdentifier
    func endBackgroundTask(_ identifier: UIBackgroundTaskIdentifier)
}

extension UIApplication: BackgroundTaskRunner {}
#endif
