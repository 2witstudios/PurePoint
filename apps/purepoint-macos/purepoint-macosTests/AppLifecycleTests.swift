import Foundation
import Testing

@testable import PurePoint

#if os(iOS)
import SwiftUI

// MARK: - Scene lifecycle

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

    @Test func givenTheSceneHookShouldReachAppState() {
        let appState = AppState(startProject: { _ in })
        #expect(AppLifecycleHooks.scenePhaseChanged != nil)
        _ = appState
    }
}
#endif
