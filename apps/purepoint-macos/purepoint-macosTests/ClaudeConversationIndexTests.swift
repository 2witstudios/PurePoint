import Foundation
import Testing
@testable import PurePoint

struct ClaudeConversationIndexTests {

    @Test func indexedSessionsUseSummaryAndResolvePurePointRoot() throws {
        let tempRoot = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempRoot) }

        let projectRoot = tempRoot.appendingPathComponent("purepoint", isDirectory: true)
        let worktreePath = projectRoot.appendingPathComponent(".pu/worktrees/wt-dashboard", isDirectory: true)
        let manifestPath = projectRoot.appendingPathComponent(".pu/manifest.json")

        try Self.createDirectory(projectRoot)
        try Self.createDirectory(worktreePath)
        try Data("{}".utf8).write(to: manifestPath)

        let claudeBase = tempRoot.appendingPathComponent("claude/projects", isDirectory: true)
        let transcriptDirectory = claudeBase.appendingPathComponent("project-hash", isDirectory: true)
        try Self.createDirectory(transcriptDirectory)

        let transcriptURL = transcriptDirectory.appendingPathComponent("sid-indexed.jsonl")
        try Self.writeTranscript(
            to: transcriptURL,
            cwd: worktreePath.path,
            sessionId: "sid-indexed",
            branch: "feature/dashboard",
            lines: [
                .user("Build a dashboard for session browsing"),
                .assistant("I'll index the local transcript files and wire a dashboard."),
                .user("Show a pulse card too"),
                .assistant("The pulse card is mocked, but the conversation browser is real."),
            ]
        )

        let indexURL = transcriptDirectory.appendingPathComponent("sessions-index.json")
        let indexJSON = """
            {
              "version": 1,
              "entries": [
                {
                  "sessionId": "sid-indexed",
                  "fullPath": "\(transcriptURL.path)",
                  "fileMtime": 1770046959218,
                  "firstPrompt": "Build a dashboard for session browsing",
                  "summary": "Useful dashboard",
                  "messageCount": 4,
                  "created": "2026-03-01T09:00:00.000Z",
                  "modified": "2026-03-01T09:10:05.000Z",
                  "gitBranch": "feature/dashboard",
                  "projectPath": "\(worktreePath.path)"
                }
              ]
            }
            """
        try Data(indexJSON.utf8).write(to: indexURL)

        let sessions = try ClaudeConversationIndex.loadSessions(baseURL: claudeBase)

        #expect(sessions.count == 1)
        #expect(sessions[0].title == "Useful dashboard")
        #expect(sessions[0].gitBranch == "feature/dashboard")
        #expect(sessions[0].purePointProjectRoot == projectRoot.path)
        #expect(sessions[0].messageCount == 4)
        // Snippets are deferred — loadSessions returns [] for lazy enrichment
        #expect(sessions[0].previewSnippets == [])
    }

    @Test func looseSessionsFallbackToTranscriptMetadata() throws {
        let tempRoot = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempRoot) }

        let projectRoot = tempRoot.appendingPathComponent("repo", isDirectory: true)
        let worktreePath = projectRoot.appendingPathComponent(".pu/worktrees/wt-raw", isDirectory: true)
        let manifestPath = projectRoot.appendingPathComponent(".pu/manifest.json")

        try Self.createDirectory(projectRoot)
        try Self.createDirectory(worktreePath)
        try Data("{}".utf8).write(to: manifestPath)

        let claudeBase = tempRoot.appendingPathComponent("claude/projects", isDirectory: true)
        let transcriptDirectory = claudeBase.appendingPathComponent("project-hash-raw", isDirectory: true)
        try Self.createDirectory(transcriptDirectory)

        let transcriptURL = transcriptDirectory.appendingPathComponent("sid-raw.jsonl")
        try Self.writeTranscript(
            to: transcriptURL,
            cwd: worktreePath.path,
            sessionId: "sid-raw",
            branch: "feature/raw-browser",
            lines: [
                .user("Implement the following plan:\n\n# Resume old Claude session"),
                .assistant("I'm reading the transcript directly because there is no sessions index here."),
                .user("Keep the conversation browser real"),
                .assistant("Done. The dashboard now falls back to raw JSONL files."),
            ]
        )

        let sessions = try ClaudeConversationIndex.loadSessions(baseURL: claudeBase)

        #expect(sessions.count == 1)
        #expect(sessions[0].title == "Resume old Claude session")
        #expect(sessions[0].gitBranch == "feature/raw-browser")
        #expect(sessions[0].purePointProjectRoot == projectRoot.path)
        // Snippets are deferred — loadSessions returns [] for lazy enrichment
        #expect(sessions[0].previewSnippets == [])
    }

    @Test func recentSnippetsLoadsFromTranscriptTail() throws {
        let tempRoot = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempRoot) }

        let transcriptURL = tempRoot.appendingPathComponent("sid-snippets.jsonl")
        try Self.writeTranscript(
            to: transcriptURL,
            cwd: "/tmp",
            sessionId: "sid-snippets",
            branch: "main",
            lines: [
                .user("First user message"),
                .assistant("First assistant reply"),
                .user("Second user message"),
                .assistant("Second assistant reply"),
            ]
        )

        let snippets = ClaudeConversationIndex.recentSnippets(from: transcriptURL)

        #expect(
            snippets == [
                "First assistant reply",
                "Second user message",
                "Second assistant reply",
            ])
    }

    @Test func tempDirectoriesAreSkipped() throws {
        let tempRoot = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempRoot) }

        let claudeBase = tempRoot.appendingPathComponent("claude/projects", isDirectory: true)

        // Create a temp-dir project (should be skipped)
        let tempDir = claudeBase.appendingPathComponent("private-var-folders-xx-xxxxxx", isDirectory: true)
        try Self.createDirectory(tempDir)
        let transcript = tempDir.appendingPathComponent("sid-temp.jsonl")
        try Self.writeTranscript(
            to: transcript,
            cwd: "/private/var/folders/xx/xxxxxx",
            sessionId: "sid-temp",
            branch: "main",
            lines: [.user("temp session")]
        )

        // Create a normal project (should be included)
        let normalDir = claudeBase.appendingPathComponent("normal-project", isDirectory: true)
        try Self.createDirectory(normalDir)
        let normalTranscript = normalDir.appendingPathComponent("sid-normal.jsonl")
        try Self.writeTranscript(
            to: normalTranscript,
            cwd: "/tmp/normal",
            sessionId: "sid-normal",
            branch: "main",
            lines: [.user("normal session")]
        )

        let sessions = try ClaudeConversationIndex.loadSessions(baseURL: claudeBase)

        #expect(sessions.count == 1)
        #expect(sessions[0].sessionId == "sid-normal")
    }

    @Test func badSummariesFallbackToPromptOrBranch() throws {
        let tempRoot = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempRoot) }

        let claudeBase = tempRoot.appendingPathComponent("claude/projects", isDirectory: true)
        let dir = claudeBase.appendingPathComponent("project-bad", isDirectory: true)
        try Self.createDirectory(dir)

        let transcript = dir.appendingPathComponent("sid-bad.jsonl")
        try Self.writeTranscript(
            to: transcript,
            cwd: "/tmp",
            sessionId: "sid-bad",
            branch: "pu/simplify-engine",
            lines: [.user("  ")]
        )

        let indexURL = dir.appendingPathComponent("sessions-index.json")
        let indexJSON = """
            {
              "version": 1,
              "entries": [
                {
                  "sessionId": "sid-bad",
                  "fullPath": "\(transcript.path)",
                  "summary": "error: invalid api key",
                  "gitBranch": "pu/simplify-engine",
                  "projectPath": "/tmp"
                }
              ]
            }
            """
        try Data(indexJSON.utf8).write(to: indexURL)

        let sessions = try ClaudeConversationIndex.loadSessions(baseURL: claudeBase)
        #expect(sessions.count == 1)
        // Bad summary rejected, empty prompt, falls back to branch name
        #expect(sessions[0].title == "Simplify Engine")
    }

    @Test func fillerPrefixesAreStrippedFromPrompt() throws {
        let tempRoot = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempRoot) }

        let claudeBase = tempRoot.appendingPathComponent("claude/projects", isDirectory: true)
        let dir = claudeBase.appendingPathComponent("project-filler", isDirectory: true)
        try Self.createDirectory(dir)

        let transcript = dir.appendingPathComponent("sid-filler.jsonl")
        try Self.writeTranscript(
            to: transcript,
            cwd: "/tmp",
            sessionId: "sid-filler",
            branch: "main",
            lines: [.user("Can you fix the sidebar layout bug")]
        )

        let sessions = try ClaudeConversationIndex.loadSessions(baseURL: claudeBase)
        #expect(sessions.count == 1)
        #expect(sessions[0].title == "fix the sidebar layout bug")
    }

    private static func makeTemporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString, isDirectory: true)
        try createDirectory(url)
        return url
    }

    private static func createDirectory(_ url: URL) throws {
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    }

    private static func writeTranscript(
        to url: URL,
        cwd: String,
        sessionId: String,
        branch: String,
        lines: [TranscriptLine]
    ) throws {
        var payloads: [String] = [
            """
            {"type":"progress","cwd":"\(cwd)","sessionId":"\(sessionId)","gitBranch":"\(branch)","timestamp":"2026-03-01T09:00:00.000Z"}
            """
        ]

        for (index, line) in lines.enumerated() {
            let timestamp = String(format: "2026-03-01T09:%02d:00.000Z", index + 1)
            switch line {
            case .user(let content):
                payloads.append(
                    """
                    {"type":"user","cwd":"\(cwd)","sessionId":"\(sessionId)","gitBranch":"\(branch)","timestamp":"\(timestamp)","message":{"role":"user","content":"\(escaped(content))"}}
                    """)
            case .assistant(let content):
                payloads.append(
                    """
                    {"type":"assistant","cwd":"\(cwd)","sessionId":"\(sessionId)","gitBranch":"\(branch)","timestamp":"\(timestamp)","message":{"role":"assistant","content":[{"type":"text","text":"\(escaped(content))"}]}}
                    """)
            }
        }

        try Data(payloads.joined(separator: "\n").utf8).write(to: url)
    }

    private static func escaped(_ string: String) -> String {
        string
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
            .replacingOccurrences(of: "\n", with: "\\n")
    }

    private enum TranscriptLine {
        case user(String)
        case assistant(String)
    }
}
