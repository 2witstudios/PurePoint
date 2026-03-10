import Foundation
import Testing

@testable import PurePoint

@Suite
struct SpawnTargetResolverTests {

    private func resolve(
        isWorktree: Bool = false,
        selection: SidebarSelection? = nil,
        knownWorktrees: [String: String] = [:]
    ) -> SpawnTarget {
        SpawnTargetResolver.resolve(
            isWorktree: isWorktree,
            selection: selection,
            worktreeIdForAgent: { knownWorktrees[$0] }
        )
    }

    @Test func isWorktreeReturnsNonRootWithNilWorktree() {
        let target = resolve(isWorktree: true)
        #expect(target.root == false)
        #expect(target.worktree == nil)
    }

    @Test func worktreeSelectionReturnsWorktreeId() {
        let target = resolve(selection: .worktree("wt1"))
        #expect(target.root == false)
        #expect(target.worktree == "wt1")
    }

    @Test func agentSelectionWithKnownWorktreeReturnsWorktreeId() {
        let target = resolve(
            selection: .agent("a1"),
            knownWorktrees: ["a1": "wt-matched"]
        )
        #expect(target.root == false)
        #expect(target.worktree == "wt-matched")
    }

    @Test func agentSelectionWithNoWorktreeReturnsRoot() {
        let target = resolve(selection: .agent("a1"))
        #expect(target.root == true)
        #expect(target.worktree == nil)
    }

    @Test func terminalSelectionWithKnownWorktreeReturnsWorktreeId() {
        let target = resolve(
            selection: .terminal("a1"),
            knownWorktrees: ["a1": "wt-matched"]
        )
        #expect(target.root == false)
        #expect(target.worktree == "wt-matched")
    }

    @Test func terminalSelectionWithNoWorktreeReturnsRoot() {
        let target = resolve(selection: .terminal("a1"))
        #expect(target.root == true)
        #expect(target.worktree == nil)
    }

    @Test func nilSelectionReturnsRoot() {
        let target = resolve()
        #expect(target.root == true)
        #expect(target.worktree == nil)
    }

    @Test func navSelectionReturnsRoot() {
        let target = resolve(selection: .nav(.dashboard))
        #expect(target.root == true)
        #expect(target.worktree == nil)
    }

    @Test func projectSelectionReturnsRoot() {
        let target = resolve(selection: .project("/some/path"))
        #expect(target.root == true)
        #expect(target.worktree == nil)
    }
}
