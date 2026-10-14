import Foundation
import Testing

@testable import PurePoint

/// Sidebar expansion has to outlive the rows it describes: the outline view allocates new
/// node objects on every data refresh, so what is closed is recorded outside the tree.
@MainActor
struct SidebarExpansionStoreTests {
    /// A store over a private suite, so a test's rows never reach the real defaults and
    /// the suite is removed with the test.
    private func makeStore() throws -> (store: SidebarExpansionStore, suite: String) {
        let suite = "pp-sidebar-expansion-\(UUID().uuidString)"
        let defaults = try #require(UserDefaults(suiteName: suite))
        return (SidebarExpansionStore(defaults: defaults), suite)
    }

    @Test func givenNothingRecordedShouldClaimNoRowsAreClosed() throws {
        let (store, suite) = try makeStore()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }

        #expect(store.closedRowKeys.isEmpty)
    }

    @Test func givenARowTheUserClosedShouldStayClosedAcrossALaunch() throws {
        let (store, suite) = try makeStore()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        store.setExpanded("project:/a", isExpanded: true)
        store.setExpanded("project:/a", isExpanded: false)
        store.setExpanded("project:/b", isExpanded: false)

        let relaunched = SidebarExpansionStore(defaults: UserDefaults(suiteName: suite)!)
        #expect(relaunched.closedRowKeys == ["project:/a", "project:/b"])
    }

    @Test func givenARowTheUserReopenedShouldNotStayClosed() throws {
        let (store, suite) = try makeStore()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        store.setExpanded("project:/a", isExpanded: false)
        store.setExpanded("project:/a", isExpanded: true)

        #expect(store.closedRowKeys.isEmpty)
        let relaunched = SidebarExpansionStore(defaults: UserDefaults(suiteName: suite)!)
        #expect(relaunched.closedRowKeys.isEmpty)
    }

    @Test func givenARowThatNoLongerExistsShouldDropItWhenTheTreeIsRecorded() throws {
        let (store, suite) = try makeStore()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        store.record(["project:/gone", "project:/kept"])

        // The tree as it stands now: only the kept row still exists.
        store.record(["project:/kept"])

        #expect(store.closedRowKeys == ["project:/kept"])
        let relaunched = SidebarExpansionStore(defaults: UserDefaults(suiteName: suite)!)
        #expect(relaunched.closedRowKeys == ["project:/kept"])
    }

    @Test func givenTheSameRowsRecordedAgainShouldNotRewriteThem() throws {
        let (store, suite) = try makeStore()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        store.record(["project:/a"])
        let defaults = UserDefaults(suiteName: suite)!

        store.record(["project:/a"])

        #expect(defaults.stringArray(forKey: "PurePointSidebarExpansion") == ["project:/a"])
    }
}
