import Foundation
import Testing
@testable import PurePoint

struct TranscriptParserTests {

    @Test func givenTranscriptWithUserAndAssistantShouldParseMessages() throws {
        let tempDir = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let path = tempDir.appendingPathComponent("session.jsonl")
        try Self.writeTranscript(
            to: path,
            lines: [
                .user("Hello, how are you?"),
                .assistant("I'm doing well! How can I help?"),
                .user("Tell me a joke"),
                .assistant("Why did the programmer quit? Because they didn't get arrays."),
            ])

        let messages = try TranscriptParser.parse(transcriptPath: path.path)

        #expect(messages.count == 4)
        #expect(messages[0].role == .user)
        #expect(messages[1].role == .assistant)
        #expect(messages[2].role == .user)
        #expect(messages[3].role == .assistant)

        // Check user messages have text content
        guard case .text(_, let text) = messages[0].contentBlocks.first else {
            Issue.record("Expected text block"); return
        }
        #expect(text == "Hello, how are you?")
    }

    @Test func givenTranscriptWithToolUseShouldCreateToolBlocks() throws {
        let tempDir = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let path = tempDir.appendingPathComponent("session.jsonl")
        try Self.writeTranscript(
            to: path,
            lines: [
                .user("Read my config file"),
                .assistantWithToolUse(
                    toolId: "toolu_abc", toolName: "Read", toolInput: "{\"file_path\":\"/etc/hosts\"}"),
            ])

        let messages = try TranscriptParser.parse(transcriptPath: path.path)

        #expect(messages.count == 2)
        guard case .toolUse(_, let name, _, _) = messages[1].contentBlocks.first else {
            Issue.record("Expected tool_use block"); return
        }
        #expect(name == "Read")
    }

    @Test func givenTranscriptWithToolResultShouldLinkToToolUse() throws {
        let tempDir = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let path = tempDir.appendingPathComponent("session.jsonl")
        try Self.writeTranscript(
            to: path,
            lines: [
                .user("Read the file"),
                .assistantWithToolUse(toolId: "toolu_xyz", toolName: "Read", toolInput: "{}"),
                .toolResult(toolUseId: "toolu_xyz", content: "file contents here", isError: false),
                .assistant("Here is the file content."),
            ])

        let messages = try TranscriptParser.parse(transcriptPath: path.path)

        // Tool result folded into assistant — no separate user message
        #expect(messages.count == 2)
        #expect(messages[0].role == .user)
        #expect(messages[1].role == .assistant)

        // Tool result block is on the assistant message
        let assistantBlocks = messages[1].contentBlocks
        let hasToolResult = assistantBlocks.contains { block in
            if case .toolResult(_, let tid, _, _) = block, tid == "toolu_xyz" { return true }
            return false
        }
        #expect(hasToolResult)

        // Tool use status updated to .completed
        let toolUseCompleted = assistantBlocks.contains { block in
            if case .toolUse(let id, _, _, let status) = block,
                id == "toolu_xyz",
                case .completed = status
            {
                return true
            }
            return false
        }
        #expect(toolUseCompleted)
    }

    @Test func givenToolResultOnlyUserRecordShouldFoldIntoAssistant() throws {
        let tempDir = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let path = tempDir.appendingPathComponent("session.jsonl")
        try Self.writeTranscript(
            to: path,
            lines: [
                .user("Do something"),
                .assistantWithToolUse(toolId: "toolu_001", toolName: "Bash", toolInput: "{}"),
                .toolResult(toolUseId: "toolu_001", content: "output", isError: false),
            ])

        let messages = try TranscriptParser.parse(transcriptPath: path.path)

        // Only 2 messages: original user + assistant (tool result folded in)
        #expect(messages.count == 2)
        #expect(messages[0].role == .user)
        #expect(messages[1].role == .assistant)

        // No user message for the tool result
        let userMessages = messages.filter { $0.role == .user }
        #expect(userMessages.count == 1)

        // Tool result is on the assistant message
        let assistantResults = messages[1].contentBlocks.filter { block in
            if case .toolResult = block { return true }
            return false
        }
        #expect(assistantResults.count == 1)
    }

    @Test func givenMixedUserContentShouldKeepUserMessage() throws {
        let tempDir = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let path = tempDir.appendingPathComponent("session.jsonl")

        // Manually build transcript with mixed user record (text + tool_result)
        var payloads: [String] = []
        payloads.append(
            """
            {"type":"progress","cwd":"/tmp/test","sessionId":"test-session","gitBranch":"main","timestamp":"2026-03-01T09:00:00.000Z"}
            """)
        payloads.append(Self.userLine("Setup"))
        payloads.append(
            """
            {"type":"assistant","sessionId":"test-session","timestamp":"2026-03-01T09:01:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_mix","name":"Read","input":{}}]}}
            """)
        payloads.append(
            """
            {"type":"user","sessionId":"test-session","timestamp":"2026-03-01T09:02:00.000Z","message":{"role":"user","content":[{"type":"text","text":"Here is context"},{"type":"tool_result","tool_use_id":"toolu_mix","content":"result data","is_error":false}]}}
            """)
        try Data(payloads.joined(separator: "\n").utf8).write(to: path)

        let messages = try TranscriptParser.parse(transcriptPath: path.path)

        // Mixed content keeps the user message (not folded)
        let userMessages = messages.filter { $0.role == .user }
        #expect(userMessages.count == 2)
    }

    @Test func givenErrorToolResultShouldSetFailedStatus() throws {
        let tempDir = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let path = tempDir.appendingPathComponent("session.jsonl")
        try Self.writeTranscript(
            to: path,
            lines: [
                .user("Run this"),
                .assistantWithToolUse(toolId: "toolu_err", toolName: "Bash", toolInput: "{}"),
                .toolResult(toolUseId: "toolu_err", content: "command failed", isError: true),
            ])

        let messages = try TranscriptParser.parse(transcriptPath: path.path)

        #expect(messages.count == 2)
        let assistantBlocks = messages[1].contentBlocks

        // Tool use status should be .failed when isError is true
        let toolUseFailed = assistantBlocks.contains { block in
            if case .toolUse(let id, _, _, let status) = block,
                id == "toolu_err",
                case .failed = status
            {
                return true
            }
            return false
        }
        #expect(toolUseFailed)

        // Tool result should be marked as error
        let toolResultIsError = assistantBlocks.contains { block in
            if case .toolResult(_, let tid, _, let isError) = block,
                tid == "toolu_err", isError
            {
                return true
            }
            return false
        }
        #expect(toolResultIsError)
    }

    @Test func givenAssistantWithCodeBlocksShouldSplitContent() throws {
        let tempDir = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let path = tempDir.appendingPathComponent("session.jsonl")
        let codeResponse = "Here is the code:\\n\\n```swift\\nlet x = 42\\n```\\n\\nThat should work."
        try Self.writeTranscript(
            to: path,
            lines: [
                .user("Write some code"),
                .assistantRaw(codeResponse),
            ])

        let messages = try TranscriptParser.parse(transcriptPath: path.path)

        #expect(messages.count == 2)
        let assistantBlocks = messages[1].contentBlocks
        // Should have text + code + text
        let hasCode = assistantBlocks.contains { block in
            if case .codeBlock = block { return true }
            return false
        }
        #expect(hasCode)
    }

    @Test func givenEmptyTranscriptShouldReturnEmptyArray() throws {
        let tempDir = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let path = tempDir.appendingPathComponent("empty.jsonl")
        try Data().write(to: path)

        let messages = try TranscriptParser.parse(transcriptPath: path.path)
        #expect(messages.isEmpty)
    }

    @Test func givenCorruptLineShouldSkipAndContinue() throws {
        let tempDir = try Self.makeTemporaryDirectory()
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let path = tempDir.appendingPathComponent("session.jsonl")
        var lines: [String] = []
        lines.append(Self.userLine("Before corrupt"))
        lines.append("{{not valid json}}")
        lines.append(Self.assistantLine("After corrupt"))
        try Data(lines.joined(separator: "\n").utf8).write(to: path)

        let messages = try TranscriptParser.parse(transcriptPath: path.path)

        #expect(messages.count == 2)
        #expect(messages[0].role == .user)
        #expect(messages[1].role == .assistant)
    }

    // MARK: - Helpers

    private static func makeTemporaryDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        return url
    }

    private enum TranscriptLine {
        case user(String)
        case assistant(String)
        case assistantRaw(String)  // pre-escaped text
        case assistantWithToolUse(toolId: String, toolName: String, toolInput: String)
        case toolResult(toolUseId: String, content: String, isError: Bool)
    }

    private static func writeTranscript(to url: URL, lines: [TranscriptLine]) throws {
        var payloads: [String] = []

        payloads.append(
            """
            {"type":"progress","cwd":"/tmp/test","sessionId":"test-session","gitBranch":"main","timestamp":"2026-03-01T09:00:00.000Z"}
            """)

        for (index, line) in lines.enumerated() {
            let timestamp = String(format: "2026-03-01T09:%02d:00.000Z", index + 1)
            switch line {
            case .user(let content):
                payloads.append(userLine(content, timestamp: timestamp))
            case .assistant(let content):
                payloads.append(assistantLine(content, timestamp: timestamp))
            case .assistantRaw(let content):
                payloads.append(
                    """
                    {"type":"assistant","sessionId":"test-session","timestamp":"\(timestamp)","message":{"role":"assistant","content":[{"type":"text","text":"\(content)"}]}}
                    """)
            case .assistantWithToolUse(let toolId, let toolName, let toolInput):
                payloads.append(
                    """
                    {"type":"assistant","sessionId":"test-session","timestamp":"\(timestamp)","message":{"role":"assistant","content":[{"type":"tool_use","id":"\(toolId)","name":"\(toolName)","input":\(toolInput)}]}}
                    """)
            case .toolResult(let toolUseId, let content, let isError):
                payloads.append(
                    """
                    {"type":"user","sessionId":"test-session","timestamp":"\(timestamp)","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"\(toolUseId)","content":"\(escaped(content))","is_error":\(isError)}]}}
                    """)
            }
        }

        try Data(payloads.joined(separator: "\n").utf8).write(to: url)
    }

    private static func userLine(_ content: String, timestamp: String = "2026-03-01T09:00:00.000Z") -> String {
        """
        {"type":"user","sessionId":"test-session","timestamp":"\(timestamp)","message":{"role":"user","content":"\(escaped(content))"}}
        """
    }

    private static func assistantLine(_ content: String, timestamp: String = "2026-03-01T09:00:00.000Z") -> String {
        """
        {"type":"assistant","sessionId":"test-session","timestamp":"\(timestamp)","message":{"role":"assistant","content":[{"type":"text","text":"\(escaped(content))"}]}}
        """
    }

    private static func escaped(_ string: String) -> String {
        string
            .replacingOccurrences(of: "\\", with: "\\\\")
            .replacingOccurrences(of: "\"", with: "\\\"")
            .replacingOccurrences(of: "\n", with: "\\n")
    }
}
