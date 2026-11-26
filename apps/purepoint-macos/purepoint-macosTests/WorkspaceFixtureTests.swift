import Foundation
import Testing

@testable import PurePoint

/// The golden `.pu/workspaces.json` fixtures shared with the Windows app (WIN-6.3).
///
/// `docs/reference/fixtures/workspaces/` holds hand-written `JSONEncoder` output. The Windows
/// port (`apps/purepoint-windows/ui/src/model/workspacePersistence.test.ts`) decodes the same
/// files and re-encodes them byte for byte; these tests hold the real `WorkspacePersistence` to the
/// same bytes, so the two apps provably write the same file. When one fails, the fixture (and the
/// Windows serializer) is what needs fixing: the Swift output is the reference.
private let fixtureProjectRoot = "/Users/dev/project"

private func fixtureURL(_ name: String) -> URL {
    URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent()  // purepoint-macosTests
        .deletingLastPathComponent()  // purepoint-macos
        .deletingLastPathComponent()  // apps
        .deletingLastPathComponent()  // repository root
        .appendingPathComponent("docs/reference/fixtures/workspaces")
        .appendingPathComponent(name)
}

private func fixture(_ name: String) throws -> Data {
    try Data(contentsOf: fixtureURL(name))
}

/// Decode a document and encode it again the way `save` would.
private func reencode(_ data: Data) throws -> Data? {
    let document = try JSONDecoder().decode(PersistedWorkspaceDocument.self, from: data)
    let workspaces = WorkspacePersistence.workspaces(from: data, projectId: fixtureProjectRoot)
    return WorkspacePersistence.documentData(workspaces, activeWorkspaceId: document.activeWorkspaceId)
}

private func text(_ data: Data?) -> String {
    data.map { String(decoding: $0, as: UTF8.self) } ?? "<nil>"
}


@Suite
struct WorkspaceFixtureTests {

    @Test func givenVersionFourFixtureShouldReencodeByteIdentically() throws {
        let original = try fixture("v4.json")
        let encoded = try reencode(original)

        #expect(encoded == original, "\(text(encoded))")
    }

    @Test func givenVersionFourFixtureShouldKeepPointGuardTabs() throws {
        let data = try fixture("v4.json")
        let workspaces = WorkspacePersistence.workspaces(from: data, projectId: fixtureProjectRoot)

        #expect(workspaces.count == 2)
        #expect(workspaces.last?.id == Workspace.pointGuardId(projectRoot: fixtureProjectRoot))
        #expect(workspaces.last?.panes[0]?.tabs.map(\.content) == [.pointGuard, .agent("ag-c")])
        #expect(!WorkspacePersistence.isNewerThanSupported(data: data))
    }

    @Test func givenVersionThreeFixtureShouldSaveAsTheExpectedVersionFour() throws {
        let encoded = try reencode(try fixture("v3.json"))
        let expected = try fixture("v3.expected.json")

        #expect(encoded == expected, "\(text(encoded))")
    }

    @Test func givenVersionTwoFixtureShouldMigrateToTheExpectedVersionFour() throws {
        let encoded = try reencode(try fixture("v2.json"))
        let expected = try fixture("v2.expected.json")

        #expect(encoded == expected, "\(text(encoded))")
    }

    @Test func givenLegacyGridLayoutFixtureShouldMigrateToTheExpectedVersionFour() throws {
        let root = NSTemporaryDirectory() + "pp-workspace-fixture-tests-" + UUID().uuidString
        try FileManager.default.createDirectory(atPath: root + "/.pu", withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(atPath: root) }
        let legacy = try fixture("grid-layout.json")
        try legacy.write(to: URL(fileURLWithPath: root + "/.pu/grid-layout.json"))
        let expected = try fixture("grid-layout.expected.json")

        _ = WorkspacePersistence.load(projectRoot: root)
        let written = try? Data(contentsOf: URL(fileURLWithPath: WorkspacePersistence.filePath(projectRoot: root)))

        #expect(written == expected, "\(text(written))")
        #expect(!FileManager.default.fileExists(atPath: root + "/.pu/grid-layout.json"))
    }
}
