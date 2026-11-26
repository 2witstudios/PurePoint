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

    @Test func givenIndexedSessionShouldFindItsRolloutByFullUUID() throws {
        let tempRoot = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempRoot) }

        let sessionID = "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b"
        let dayDirectory = tempRoot.appendingPathComponent("sessions/2026/11/26", isDirectory: true)
        try FileManager.default.createDirectory(at: dayDirectory, withIntermediateDirectories: true)
        let rolloutURL = dayDirectory.appendingPathComponent(
            "rollout-2026-11-26T10-00-00-\(sessionID).jsonl")
        try Data("{\"payload\":{\"cwd\":\"/srv/app\",\"git\":{\"branch\":\"main\"}}}".utf8)
            .write(to: rolloutURL)
        if !FileManager.default.fileExists(atPath: rolloutURL.path) {
            preconditionFailure("DIAG setup: rollout missing after write at \(rolloutURL.path)")
        }

        let sessions = [
            Conversation(
                sessionId: sessionID, agentSource: .codex, title: "Codex session",
                previewSnippets: [], projectPath: nil, purePointProjectRoot: nil, gitBranch: nil,
                transcriptPath: nil, createdAt: nil, modifiedAt: Date(timeIntervalSince1970: 0),
                messageCount: nil)
        ]

        let enriched = try CodexConversationIndex.enrichWithMetadata(sessions, baseURL: tempRoot)

        // TEMP DIAGNOSTIC (revert to #expect once the macOS-only failure is identified):
        // fatalError messages reach the CI log's stderr; Swift Testing issue details do not.
        if enriched.count != 1 {
            preconditionFailure("DIAG count=\(enriched.count) enriched=\(enriched)")
        }
        if enriched[0].transcriptPath != rolloutURL.path {
            preconditionFailure(
                "DIAG transcript=\(enriched[0].transcriptPath ?? "nil") expected=\(rolloutURL.path)")
        }
        if enriched[0].projectPath != "/srv/app" {
            preconditionFailure("DIAG project=\(enriched[0].projectPath ?? "nil")")
        }
        if enriched[0].gitBranch != "main" {
            preconditionFailure("DIAG branch=\(enriched[0].gitBranch ?? "nil")")
        }
    }

    private static func makeTemporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent("codex-index-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }
}
#endif
