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
        store.setExpanded("project:%2Fa", isExpanded: true)
        store.setExpanded("project:%2Fa", isExpanded: false)
        store.setExpanded("project:%2Fb", isExpanded: false)

        let relaunched = SidebarExpansionStore(defaults: UserDefaults(suiteName: suite)!)
        #expect(relaunched.closedRowKeys == ["project:%2Fa", "project:%2Fb"])
    }

    @Test func givenARowTheUserReopenedShouldNotStayClosed() throws {
        let (store, suite) = try makeStore()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        store.setExpanded("project:%2Fa", isExpanded: false)
        store.setExpanded("project:%2Fa", isExpanded: true)

        #expect(store.closedRowKeys.isEmpty)
        let relaunched = SidebarExpansionStore(defaults: UserDefaults(suiteName: suite)!)
        #expect(relaunched.closedRowKeys.isEmpty)
    }

    @Test func givenARowThatNoLongerExistsShouldDropItWhenTheTreeIsRecorded() throws {
        let (store, suite) = try makeStore()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        store.record(["project:%2Fgone", "project:%2Fkept"])

        // The tree as it stands now: only the kept row still exists.
        store.record(["project:%2Fkept"])

        #expect(store.closedRowKeys == ["project:%2Fkept"])
        let relaunched = SidebarExpansionStore(defaults: UserDefaults(suiteName: suite)!)
        #expect(relaunched.closedRowKeys == ["project:%2Fkept"])
    }

    @Test func givenTheSameRowsRecordedAgainShouldNotRewriteThem() throws {
        let (store, suite) = try makeStore()
        defer { UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite) }
        store.record(["project:%2Fa"])
        let defaults = UserDefaults(suiteName: suite)!

        store.record(["project:%2Fa"])

        #expect(defaults.stringArray(forKey: SidebarExpansionStore.storageKey) == ["project:%2Fa"])
    }
}
