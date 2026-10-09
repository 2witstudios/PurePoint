import Testing
import Foundation
@testable import PurePoint

@MainActor
struct FileTreeStateTests {

    private func makeTempDir(files: [String], dirs: [String] = []) throws -> String {
        let root = NSTemporaryDirectory() + "FileTreeStateTests-\(UUID().uuidString)"
        let fm = FileManager.default
        try fm.createDirectory(atPath: root, withIntermediateDirectories: true)
        for dir in dirs {
            try fm.createDirectory(
                atPath: (root as NSString).appendingPathComponent(dir),
                withIntermediateDirectories: true
            )
        }
        for file in files {
            fm.createFile(
                atPath: (root as NSString).appendingPathComponent(file),
                contents: nil
            )
        }
        return root
    }

    private func cleanup(_ path: String) {
        try? FileManager.default.removeItem(atPath: path)
    }

    // MARK: - Dot file visibility

    @Test func givenDotFileShouldIncludeInTree() throws {
        // given
        let root = try makeTempDir(files: [".gitignore", ".env.example", "README.md"])
        defer { cleanup(root) }
        let state = FileTreeState()

        // when
        state.load(worktreePath: root)

        // then
        let names = state.rootNodes.map(\.name)
        #expect(names.contains(".gitignore"))
        #expect(names.contains(".env.example"))
        #expect(names.contains("README.md"))
    }

    @Test func givenDotDirectoryShouldIncludeInTree() throws {
        // given
        let root = try makeTempDir(files: [], dirs: [".claude", ".pu"])
        defer { cleanup(root) }
        let state = FileTreeState()

        // when
        state.load(worktreePath: root)

        // then
        let names = state.rootNodes.map(\.name)
        #expect(names.contains(".claude"))
        #expect(names.contains(".pu"))
    }

    // MARK: - Hidden names still excluded

    @Test func givenGitDirectoryShouldExcludeFromTree() throws {
        // given
        let root = try makeTempDir(files: [".DS_Store"], dirs: [".git", ".build"])
        defer { cleanup(root) }
        let state = FileTreeState()

        // when
        state.load(worktreePath: root)

        // then
        let names = state.rootNodes.map(\.name)
        #expect(!names.contains(".git"))
        #expect(!names.contains(".DS_Store"))
        #expect(!names.contains(".build"))
    }

    @Test func givenNonDotHiddenNamesShouldExcludeFromTree() throws {
        // given
        let root = try makeTempDir(files: [], dirs: ["node_modules", "DerivedData", "xcuserdata", "__pycache__"])
        defer { cleanup(root) }
        let state = FileTreeState()

        // when
        state.load(worktreePath: root)

        // then
        let names = state.rootNodes.map(\.name)
        #expect(!names.contains("node_modules"))
        #expect(!names.contains("DerivedData"))
        #expect(!names.contains("xcuserdata"))
        #expect(!names.contains("__pycache__"))
    }

    // MARK: - Mixed content

    @Test func givenMixedContentShouldShowDotFilesButNotHiddenNames() throws {
        // given
        let root = try makeTempDir(
            files: [".gitignore", ".env", ".DS_Store", "main.swift"],
            dirs: [".claude", ".git", "Sources"]
        )
        defer { cleanup(root) }
        let state = FileTreeState()

        // when
        state.load(worktreePath: root)

        // then
        let names = Set(state.rootNodes.map(\.name))
        // Visible
        #expect(names.contains(".gitignore"))
        #expect(names.contains(".env"))
        #expect(names.contains(".claude"))
        #expect(names.contains("main.swift"))
        #expect(names.contains("Sources"))
        // Hidden
        #expect(!names.contains(".git"))
        #expect(!names.contains(".DS_Store"))
    }
    private func waitUntil(_ predicate: () -> Bool) async throws {
        for _ in 0..<200 {
            if predicate() { return }
            try await Task.sleep(for: .milliseconds(10))
        }
        #expect(predicate())
    }

    @Test func givenIgnoredFilesShouldStayFilteredAcrossPollingAndRuleChanges() async throws {
        let root = try makeTempDir(files: [".gitignore", "root.ignore", "keep.txt"], dirs: ["Sources"])
        defer { cleanup(root) }
        let git = Process()
        git.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        git.arguments = ["-C", root, "init", "-q"]
        try git.run()
        git.waitUntilExit()
        #expect(git.terminationStatus == 0)
        try "*.ignore\n".write(toFile: root + "/.gitignore", atomically: true, encoding: .utf8)
        try "ignored\n".write(toFile: root + "/Sources/child.ignore", atomically: true, encoding: .utf8)
        try "keep\n".write(toFile: root + "/Sources/keep.swift", atomically: true, encoding: .utf8)
        let state = FileTreeState()
        defer { state.stopWatching() }
        state.load(worktreePath: root)
        try await waitUntil { !state.rootNodes.contains { $0.name == "root.ignore" } }
        state.expandNode(try #require(state.rootNodes.first { $0.name == "Sources" }))
        try await waitUntil {
            let children = state.rootNodes.first { $0.name == "Sources" }?.children ?? []
            return children.map(\.name) == ["keep.swift"]
        }
        // Add an allowed file so completion of each replacement is observable.
        for iteration in 0..<3 {
            try "new\n".write(toFile: root + "/poll-\(iteration).txt", atomically: true, encoding: .utf8)
            state.refresh()
            #expect(!state.rootNodes.contains { $0.name == "root.ignore" })
            #expect(
                !(state.rootNodes.first { $0.name == "Sources" }?.children ?? []).contains { $0.name == "child.ignore" }
            )
            try await waitUntil { state.rootNodes.contains { $0.name == "poll-\(iteration).txt" } }
            #expect(!state.rootNodes.contains { $0.name == "root.ignore" })
            #expect(
                !(state.rootNodes.first { $0.name == "Sources" }?.children ?? []).contains { $0.name == "child.ignore" }
            )
        }
        try "".write(toFile: root + "/.gitignore", atomically: true, encoding: .utf8)
        state.refresh()
        try await waitUntil {
            state.rootNodes.contains { $0.name == "root.ignore" }
                && (state.rootNodes.first { $0.name == "Sources" }?.children ?? []).contains {
                    $0.name == "child.ignore"
                }
        }
    }

}
