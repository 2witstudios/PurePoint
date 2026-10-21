import Foundation
import Testing

@testable import PurePoint

#if os(iOS)
import SwiftUI

// MARK: - Scene lifecycle

@MainActor
private final class FakeBackgroundTasks: BackgroundTaskRunner {
    private(set) var begun = 0
    private(set) var ended: [UIBackgroundTaskIdentifier] = []
    private var handler: (@MainActor @Sendable () -> Void)?

    func beginBackgroundTask(
        withName taskName: String?, expirationHandler handler: (@MainActor @Sendable () -> Void)?
    ) -> UIBackgroundTaskIdentifier {
        begun += 1
        self.handler = handler
        return UIBackgroundTaskIdentifier(rawValue: begun)
    }

    func endBackgroundTask(_ identifier: UIBackgroundTaskIdentifier) {
        ended.append(identifier)
    }

    func expire() { handler?() }
}

@MainActor
struct AppActivityTests {
    @Test func givenTheBackgroundShouldHoldPollersUntilTheForeground() async {
        let activity = AppActivity()
        #expect(!activity.apply(.background))
        #expect(activity.isInBackground)
        let resumed = Task { @MainActor in
            await activity.untilForeground()
            return true
        }
        await Task.yield()
        // Inactive (the app switcher) is not the background: pollers resume.
        #expect(activity.apply(.inactive))
        #expect(await resumed.value)
        #expect(!activity.isInBackground)
        #expect(!activity.apply(.active))
    }

    @Test func givenTheForegroundShouldNotWait() async {
        let activity = AppActivity()
        await activity.untilForeground()
        #expect(!activity.isInBackground)
    }

    @Test func givenACancelledPollerShouldStopWaiting() async {
        let activity = AppActivity()
        activity.apply(.background)
        let waiting = Task { @MainActor in await activity.untilForeground() }
        await Task.yield()
        waiting.cancel()
        await waiting.value
        #expect(activity.isInBackground)
        activity.apply(.active)
    }

    @Test func givenTheGraceExpiresShouldEndTheTaskBeforeTheHandlerReturns() {
        let tasks = FakeBackgroundTasks()
        let activity = AppActivity(backgroundTasks: tasks)
        activity.apply(.background)
        #expect(tasks.begun == 1)
        tasks.expire()
        // Synchronously: iOS suspends the app as soon as the handler returns.
        #expect(tasks.ended == [UIBackgroundTaskIdentifier(rawValue: 1)])
        // Returning to the foreground does not end it a second time.
        activity.apply(.active)
        #expect(tasks.ended.count == 1)
    }

    @Test func givenTheForegroundShouldEndTheGrace() {
        let tasks = FakeBackgroundTasks()
        let activity = AppActivity(backgroundTasks: tasks)
        activity.apply(.background)
        activity.apply(.active)
        #expect(tasks.ended == [UIBackgroundTaskIdentifier(rawValue: 1)])
    }

    @Test func givenTheSceneHookShouldReachAppState() {
        let appState = AppState(startProject: { _ in })
        #expect(AppLifecycleHooks.scenePhaseChanged != nil)
        _ = appState
    }
}
#endif
