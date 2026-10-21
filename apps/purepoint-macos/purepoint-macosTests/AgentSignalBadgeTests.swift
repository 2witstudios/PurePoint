import Foundation
import Testing

@testable import PurePoint

#if os(iOS)
import UserNotifications

// MARK: - App badge

struct AgentSignalBadgeTests {
    @Test func givenNoDecisionYetShouldAskOnlyWhenThereIsSomethingToCount() {
        #expect(AgentSignalBadge.action(count: 2, authorization: .notDetermined, badgeSetting: .notSupported)
            == .requestAuthorization)
        #expect(AgentSignalBadge.action(count: 0, authorization: .notDetermined, badgeSetting: .notSupported) == .skip)
    }

    @Test func givenPermissionShouldSetTheBadgeUnlessBadgesAreOff() {
        #expect(AgentSignalBadge.action(count: 3, authorization: .authorized, badgeSetting: .enabled) == .set)
        #expect(AgentSignalBadge.action(count: 0, authorization: .provisional, badgeSetting: .enabled) == .set)
        #expect(AgentSignalBadge.action(count: 3, authorization: .authorized, badgeSetting: .disabled) == .skip)
        #expect(AgentSignalBadge.action(count: 3, authorization: .denied, badgeSetting: .enabled) == .skip)
    }
}
#endif
