import Foundation
import Testing
@testable import PurePoint

@MainActor
struct WorkspaceFilesStateTests {
    @Test func givenCollapsedFilesShouldLoadSummariesAndOnlyExpandRequestedDiffs() async throws {
        let root = NSTemporaryDirectory() + "WorkspaceFilesStateTests-\(UUID().uuidString)"
        try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: root) }
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = ["init", "-q", root]
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        process.waitUntilExit()
        #expect(process.terminationStatus == 0)
        try "first\n".write(toFile: root + "/one.txt", atomically: true, encoding: .utf8)
        try "second\n".write(toFile: root + "/two.txt", atomically: true, encoding: .utf8)
        let state = WorkspaceFilesState(rootPath: root)
        await state.refresh()
        #expect(state.files.count == 2)
        #expect(state.diffs.isEmpty)
        let first = try #require(state.files.first)
        state.toggleFile(first)
        await state.loadDiff(first)
        #expect(state.diffs.keys.sorted() == ["one.txt"])
        #expect(state.expandedFiles == ["one.txt"])

        try "updated\n".write(toFile: root + "/one.txt", atomically: true, encoding: .utf8)
        await state.refresh()
        #expect(state.diffs["one.txt"]?.hunks.flatMap(\.lines).first?.content == "updated")
        #expect(state.expandedFiles == ["one.txt"])
        #expect(state.diffs["two.txt"] == nil)

        try FileManager.default.removeItem(atPath: root + "/one.txt")
        await state.refresh()
        #expect(state.expandedFiles.isEmpty)
        #expect(state.diffs.isEmpty)
    }

    @Test func givenNonRepositoryShouldShowAnErrorButStillAllowFileBrowsing() async throws {
        let root = NSTemporaryDirectory() + "WorkspaceFilesStateTests-\(UUID().uuidString)"
        try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: root) }
        try "hello\n".write(toFile: root + "/readme.txt", atomically: true, encoding: .utf8)
        let state = WorkspaceFilesState(rootPath: root)
        await state.refresh()
        #expect(state.error != nil)
        #expect(!state.isLoading)
        state.mode = .files
        state.loadTreeIfNeeded()
        #expect(state.fileTree.rootNodes.map(\.name) == ["readme.txt"])
        state.fileTree.stopWatching()
    }
}
