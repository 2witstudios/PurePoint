import Foundation
import Testing
@testable import PurePoint

struct WorkingTreeDiffTests {
    private func git(_ arguments: [String], at root: String) throws {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = ["-C", root] + arguments
        process.standardOutput = FileHandle.nullDevice
        process.standardError = FileHandle.nullDevice
        try process.run()
        process.waitUntilExit()
        #expect(process.terminationStatus == 0)
    }

    private func repository() throws -> String {
        let root = NSTemporaryDirectory() + "WorkingTreeDiffTests-\(UUID().uuidString)"
        try FileManager.default.createDirectory(atPath: root, withIntermediateDirectories: true)
        try git(["init", "-q"], at: root)
        try git(["config", "user.email", "test@example.com"], at: root)
        try git(["config", "user.name", "Test"], at: root)
        return root
    }

    @Test func givenStagedAndUnstagedEditsShouldReviewTheWholeWorkingTree() async throws {
        let root = try repository()
        defer { try? FileManager.default.removeItem(atPath: root) }
        let path = root + "/source.swift"
        try "let first = 1\nlet second = 2\n".write(toFile: path, atomically: true, encoding: .utf8)
        try git(["add", "."], at: root)
        try git(["commit", "-qm", "initial"], at: root)
        try "let first = 3\nlet second = 2\n".write(toFile: path, atomically: true, encoding: .utf8)
        try git(["add", "."], at: root)
        try "let first = 3\nlet second = 4\n".write(toFile: path, atomically: true, encoding: .utf8)

        let files = try await GitService.shared.fetchWorkingTreeChanges(worktreePath: root)
        #expect(files.map(\.filename) == ["source.swift"])
        let diff = try await GitService.shared.fetchWorkingTreeFileDiff(worktreePath: root, file: files[0])
        let added = diff.hunks.flatMap(\.lines).filter { $0.type == .addition }.map(\.content)
        #expect(added == ["let first = 3", "let second = 4"])
        #expect(diff.added == 2)
        #expect(diff.removed == 2)
    }

    @Test func givenUntrackedFileWithSpecialCharactersShouldExpandItsActualContents() async throws {
        let root = try repository()
        defer { try? FileManager.default.removeItem(atPath: root) }
        let name = "new file\t\"雪\".swift"
        try "let value = 1\n".write(toFile: root + "/" + name, atomically: true, encoding: .utf8)
        let files = try await GitService.shared.fetchWorkingTreeChanges(worktreePath: root)
        #expect(files.map(\.filename) == [name])
        let diff = try await GitService.shared.fetchWorkingTreeFileDiff(worktreePath: root, file: files[0])
        #expect(diff.hunks.flatMap(\.lines).map(\.content) == ["let value = 1"])
        #expect(diff.added == 1)
    }

    @Test func givenStagedFileInANewRepositoryShouldShowAnAddition() async throws {
        let root = try repository()
        defer { try? FileManager.default.removeItem(atPath: root) }
        try "hello\n".write(toFile: root + "/first.txt", atomically: true, encoding: .utf8)
        try git(["add", "."], at: root)
        let files = try await GitService.shared.fetchWorkingTreeChanges(worktreePath: root)
        let diff = try await GitService.shared.fetchWorkingTreeFileDiff(worktreePath: root, file: files[0])
        #expect(diff.statusCode == "A")
        #expect(diff.hunks.flatMap(\.lines).first?.type == .addition)
    }

    @Test func givenTrackedFilenameWithSpacesAndNewlineShouldKeepThePathAndDeletedLines() async throws {
        let root = try repository()
        defer { try? FileManager.default.removeItem(atPath: root) }
        let name = "file with\na newline.txt"
        try "before\n".write(toFile: root + "/" + name, atomically: true, encoding: .utf8)
        try git(["add", "."], at: root)
        try git(["commit", "-qm", "initial"], at: root)
        try FileManager.default.removeItem(atPath: root + "/" + name)
        let files = try await GitService.shared.fetchWorkingTreeChanges(worktreePath: root)
        #expect(files.map(\.filename) == [name])
        let diff = try await GitService.shared.fetchWorkingTreeFileDiff(worktreePath: root, file: files[0])
        #expect(diff.statusCode == "D")
        #expect(diff.hunks.flatMap(\.lines).first?.content == "before")
        #expect(diff.removed == 1)
    }

    @Test func givenUntrackedBinaryFileShouldNotRenderBinaryAsText() async throws {
        let root = try repository()
        defer { try? FileManager.default.removeItem(atPath: root) }
        try Data([0, 1, 2, 3]).write(to: URL(fileURLWithPath: root + "/image.bin"))
        let files = try await GitService.shared.fetchWorkingTreeChanges(worktreePath: root)
        let diff = try await GitService.shared.fetchWorkingTreeFileDiff(worktreePath: root, file: files[0])
        #expect(diff.hunks.isEmpty)
    }

    @Test func givenFilenameContainingPathspecCharactersShouldOnlyDiffThatFile() async throws {
        let root = try repository()
        defer { try? FileManager.default.removeItem(atPath: root) }
        let name = "[draft].txt"
        try "before\n".write(toFile: root + "/" + name, atomically: true, encoding: .utf8)
        try "unchanged\n".write(toFile: root + "/d.txt", atomically: true, encoding: .utf8)
        try git(["add", "."], at: root)
        try git(["commit", "-qm", "initial"], at: root)
        try "diff --git is part of this file\n".write(toFile: root + "/" + name, atomically: true, encoding: .utf8)
        let files = try await GitService.shared.fetchWorkingTreeChanges(worktreePath: root)
        let diff = try await GitService.shared.fetchWorkingTreeFileDiff(worktreePath: root, file: files[0])
        #expect(diff.filename == name)
        #expect(diff.hunks.flatMap(\.lines).last?.content == "diff --git is part of this file")
        #expect(diff.added == 1)
        #expect(diff.removed == 1)
    }

    @Test func givenOversizedFileShouldDeclineInlinePreview() async throws {
        let root = try repository()
        defer { try? FileManager.default.removeItem(atPath: root) }
        let path = root + "/large.txt"
        try Data(repeating: 65, count: 1_000_001).write(to: URL(fileURLWithPath: path))
        do {
            _ = try await FileIOService.readFile(at: path, limit: 1_000_000)
            Issue.record("Expected the bounded preview to reject a large file")
        } catch FilePreviewError.tooLarge {
            // Expected: do not allocate a huge inline text layout.
        }
    }
    @Test func givenLargeTrackedDiffShouldBoundCaptureAndRejectPreview() async throws {
        let root = try repository()
        defer { try? FileManager.default.removeItem(atPath: root) }
        let path = root + "/large.txt"
        try "before\n".write(toFile: path, atomically: true, encoding: .utf8)
        try git(["add", "."], at: root)
        try git(["commit", "-qm", "initial"], at: root)
        try String(repeating: "a line of changed text\n", count: 100_000)
            .write(toFile: path, atomically: true, encoding: .utf8)

        let result = await GitService.shared.runGit(
            ["diff", "HEAD", "--", "large.txt"], cwd: root, outputLimit: 1_000_000)
        #expect(result.outputExceededLimit)
        #expect(result.stdout.isEmpty)
        let files = try await GitService.shared.fetchWorkingTreeChanges(worktreePath: root)
        do {
            _ = try await GitService.shared.fetchWorkingTreeFileDiff(worktreePath: root, file: files[0])
            Issue.record("Expected the tracked diff to reject output beyond the capture limit")
        } catch FilePreviewError.tooLarge {}

        // A later smaller diff must still render after the oversized process exits.
        try "after\n".write(toFile: path, atomically: true, encoding: .utf8)
        let diff = try await GitService.shared.fetchWorkingTreeFileDiff(worktreePath: root, file: files[0])
        #expect(diff.hunks.flatMap(\.lines).last?.content == "after")
    }

    @Test func givenCachedRemovalWithDiskCopyShouldReturnOneDeletionPerPath() async throws {
        let root = try repository()
        defer { try? FileManager.default.removeItem(atPath: root) }
        let path = root + "/a.txt"
        try "before\n".write(toFile: path, atomically: true, encoding: .utf8)
        try git(["add", "."], at: root)
        try git(["commit", "-qm", "initial"], at: root)
        try git(["rm", "--cached", "a.txt"], at: root)
        try "untracked copy\n".write(toFile: path, atomically: true, encoding: .utf8)

        let files = try await GitService.shared.fetchWorkingTreeChanges(worktreePath: root)
        #expect(files.map(\.filename) == ["a.txt"])
        let file = try #require(files.first)
        #expect(file.statusCode == "D")
        #expect(file.added == 0)
        #expect(file.removed == 1)
        let diff = try await GitService.shared.fetchWorkingTreeFileDiff(worktreePath: root, file: file)
        #expect(diff.hunks.flatMap(\.lines).map(\.content) == ["before"])
        #expect(diff.hunks.flatMap(\.lines).allSatisfy { $0.type == .deletion })
        let preview = try await FileIOService.readFile(at: path, limit: 1_000_000)
        #expect(preview.content == "untracked copy\n")
    }

    @Test func givenStagedPathDeletedOrRenamedBeforeFirstCommitShouldOnlyShowExistingFiles() async throws {
        let root = try repository()
        defer { try? FileManager.default.removeItem(atPath: root) }
        try "deleted\n".write(toFile: root + "/deleted.txt", atomically: true, encoding: .utf8)
        try "renamed\n".write(toFile: root + "/old.txt", atomically: true, encoding: .utf8)
        try git(["add", "."], at: root)
        try FileManager.default.removeItem(atPath: root + "/deleted.txt")
        try FileManager.default.moveItem(atPath: root + "/old.txt", toPath: root + "/new.txt")
        let files = try await GitService.shared.fetchWorkingTreeChanges(worktreePath: root)
        #expect(files.map(\.filename) == ["new.txt"])
        let diff = try await GitService.shared.fetchWorkingTreeFileDiff(
            worktreePath: root, file: try #require(files.first))
        #expect(diff.hunks.flatMap(\.lines).map(\.content) == ["renamed"])
        #expect(diff.added == 1)
    }

}
