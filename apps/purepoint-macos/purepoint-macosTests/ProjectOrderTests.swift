import Foundation
import Testing

@testable import PurePoint

// The sidebar follows the order each machine's daemon saves, and moves send it back.

struct ProjectOrderTests {
    private func moved(_ root: String, to index: Int, in roots: [String]) -> [String]? {
        ProjectOrder.moving(root, to: index, in: roots, root: { $0 })
    }

    @Test func givenAMoveShouldPlaceTheProjectAtTheIndex() {
        #expect(moved("/c", to: 0, in: ["/a", "/b", "/c"]) == ["/c", "/a", "/b"])
        #expect(moved("/a", to: 1, in: ["/a", "/b", "/c"]) == ["/b", "/a", "/c"])
        #expect(moved("/a", to: 99, in: ["/a", "/b", "/c"]) == ["/b", "/c", "/a"])
    }

    @Test func givenAMoveThatChangesNothingShouldReturnNil() {
        #expect(moved("/a", to: 0, in: ["/a", "/b"]) == nil)
        #expect(moved("/a", to: -1, in: ["/a", "/b"]) == nil)
        #expect(moved("/missing", to: 0, in: ["/a", "/b"]) == nil)
    }

    @Test func givenTheDaemonOrderShouldSortListedProjectsFirstAndKeepTheRest() {
        let sorted = ProjectOrder.sorted(["/a", "/x", "/b", "/y"], by: ["/b", "/gone", "/a"], root: { $0 })
        #expect(sorted == ["/b", "/a", "/x", "/y"])
    }

    @Test func givenASymlinkedRootShouldMatchTheDaemonsCanonicalRoot() throws {
        let base = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: base) }
        let real = base.appendingPathComponent("real")
        let link = base.appendingPathComponent("link")
        try FileManager.default.createDirectory(at: real, withIntermediateDirectories: true)
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: real)
        let other = base.appendingPathComponent("other").path
        let sorted = ProjectOrder.sorted([other, link.path], by: [ProjectOrder.canonical(real.path)], root: { $0 })
        #expect(sorted == [link.path, other])
    }
}
