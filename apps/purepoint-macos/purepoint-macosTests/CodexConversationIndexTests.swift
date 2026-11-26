#if os(macOS)
import Foundation
import Testing

@testable import PurePoint

@Suite
struct CodexConversationIndexTests {

    /// Rollouts are named `rollout-2026-11-26T10-00-00-<uuid>` and a UUID has dashes of its
    /// own, so the session id is the trailing 36-character UUID — not the text after the
    /// last `-`, which is only the UUID's final group and never matches an indexed id.
    @Test func givenRolloutStemShouldParseTheTrailingUUID() {
        let stem = "rollout-2026-11-26T10-00-00-0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"

        #expect(
            CodexConversationIndex.sessionIdFromStem(stem)
                == "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b")
    }

    /// Stems without a trailing UUID keep the after-the-last-`-` rule.
    @Test func givenStemWithoutTrailingUUIDShouldKeepTheLegacyRule() {
        #expect(CodexConversationIndex.sessionIdFromStem("rollout-legacy-abcdefgh") == "abcdefgh")
        #expect(CodexConversationIndex.sessionIdFromStem("rollout-short") == nil)
    }

    // MARK: - Enrichment, one step per test so a CI failure names the broken step.

    /// The sessions walk indexes every rollout by its full session id.
    @Test func givenSessionsDirectoryShouldIndexRolloutsByFullSessionId() throws {
        let (tempRoot, rolloutURL, sessionID) = try Self.makeRollout()
        defer { try? FileManager.default.removeItem(at: tempRoot) }

        let paths = try CodexConversationIndex.sessionFilePaths(baseURL: tempRoot)

        #expect(paths[sessionID] != nil)
        #expect(paths[sessionID]?.lastPathComponent == rolloutURL.lastPathComponent)
    }

    /// Reading a rollout's session_meta line fills in project and branch.
    @Test func givenRolloutWithSessionMetaShouldEnrichProjectAndBranch() throws {
        let (tempRoot, _, sessionID) = try Self.makeRollout()
        defer { try? FileManager.default.removeItem(at: tempRoot) }
        let sessions = [Self.makeSession(sessionID)]

        let enriched = try CodexConversationIndex.enrichWithMetadata(sessions, baseURL: tempRoot)

        #expect(enriched.count == 1)
        #expect(enriched[0].projectPath == "/srv/app")
        #expect(enriched[0].gitBranch == "main")
    }

    /// The stored transcript path must be a readable file — not necessarily byte-identical
    /// to the URL the caller constructed: macOS may resolve /var to /private/var inside the
    /// directory walk, and either form reads the same file.
    @Test func givenEnrichedSessionShouldStoreAReadableTranscriptPath() throws {
        let (tempRoot, _, sessionID) = try Self.makeRollout()
        defer { try? FileManager.default.removeItem(at: tempRoot) }
        let sessions = [Self.makeSession(sessionID)]

        let enriched = try CodexConversationIndex.enrichWithMetadata(sessions, baseURL: tempRoot)

        let stored = enriched[0].transcriptPath
        #expect(stored != nil)
        #expect(stored.map { FileManager.default.fileExists(atPath: $0) } == true)
    }

    // MARK: - Fixtures

    private static func makeSession(_ sessionID: String) -> Conversation {
        Conversation(
            sessionId: sessionID, agentSource: .codex, title: "Codex session",
            previewSnippets: [], projectPath: nil, purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: nil, createdAt: nil, modifiedAt: Date(timeIntervalSince1970: 0),
            messageCount: nil)
    }

    private static func makeRollout() throws -> (tempRoot: URL, rolloutURL: URL, sessionID: String) {
        let sessionID = "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"
        let tempRoot = try Self.makeTemporaryDirectory()
        let dayDirectory = tempRoot.appendingPathComponent("sessions/2026/11/26", isDirectory: true)
        try FileManager.default.createDirectory(at: dayDirectory, withIntermediateDirectories: true)
        let rolloutURL = dayDirectory.appendingPathComponent(
            "rollout-2026-11-26T10-00-00-\(sessionID).jsonl")
        try Data("{\"payload\":{\"cwd\":\"/srv/app\",\"git\":{\"branch\":\"main\"}}}".utf8)
            .write(to: rolloutURL)
        return (tempRoot, rolloutURL, sessionID)
    }

    private static func makeTemporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("codex-index-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }
}
#endif
