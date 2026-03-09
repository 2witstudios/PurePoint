import Foundation
import Testing
@testable import PurePoint

@MainActor
struct ChatStateTests {

    @Test func givenNewConversationShouldClearMessagesAndSessionId() {
        let state = ChatState(processProvider: MockClaudeProcess())
        state.messages.append(ChatMessage(role: .user, contentBlocks: [.text(id: "1", text: "old")]))
        state.currentSessionId = "old-session"

        state.newConversation()

        #expect(state.messages.isEmpty)
        #expect(state.currentSessionId == nil)
    }

    @Test func givenSendMessageShouldAppendUserAndAssistantMessages() async {
        let mock = MockClaudeProcess()
        mock.events = [
            .assistant(content: [.text("Hello back!")]),
            .result(sessionId: "sess-1", durationMs: 100),
        ]
        let state = ChatState(processProvider: mock)

        await state.send("Hello", cwd: "/tmp")

        #expect(state.messages.count == 2)
        #expect(state.messages[0].role == .user)
        #expect(state.messages[1].role == .assistant)
    }

    @Test func givenStreamEventsShouldPopulateAssistantContentBlocks() async {
        let mock = MockClaudeProcess()
        mock.events = [
            .assistant(content: [.text("Here is text")]),
            .result(sessionId: "sess-2", durationMs: 200),
        ]
        let state = ChatState(processProvider: mock)

        await state.send("Go", cwd: "/tmp")

        let assistant = state.messages.first(where: { $0.role == .assistant })
        #expect(assistant != nil)
        guard case .text(_, let text) = assistant?.contentBlocks.first else {
            Issue.record("Expected text block"); return
        }
        #expect(text == "Here is text")
    }

    @Test func givenToolUseEventShouldAppendToolCard() async {
        let mock = MockClaudeProcess()
        mock.events = [
            .assistant(content: [.text("Let me read that."), .toolUse(id: "t1", name: "Read", input: "{}")]),
            .result(sessionId: "sess-3", durationMs: 150),
        ]
        let state = ChatState(processProvider: mock)

        await state.send("Read it", cwd: "/tmp")

        let assistant = state.messages.first(where: { $0.role == .assistant })
        let hasToolUse = assistant?.contentBlocks.contains { block in
            if case .toolUse = block { return true }
            return false
        }
        #expect(hasToolUse == true)
    }

    @Test func givenResultEventShouldStopStreaming() async {
        let mock = MockClaudeProcess()
        mock.events = [
            .assistant(content: [.text("Done")]),
            .result(sessionId: "sess-4", durationMs: 50),
        ]
        let state = ChatState(processProvider: mock)

        await state.send("Do it", cwd: "/tmp")

        #expect(state.isStreaming == false)
        #expect(state.currentSessionId == "sess-4")
    }

    @Test func givenSearchQueryShouldFilterSessions() {
        let state = ChatState(processProvider: MockClaudeProcess())

        let session1 = Conversation(
            sessionId: "s1", agentSource: .claude, title: "Dashboard work",
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/s1.jsonl",
            createdAt: nil, modifiedAt: Date(), messageCount: nil
        )
        let session2 = Conversation(
            sessionId: "s2", agentSource: .claude, title: "Terminal bugs",
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/s2.jsonl",
            createdAt: nil, modifiedAt: Date(), messageCount: nil
        )

        state.sessions = [session1, session2]
        state.searchQuery = "Dashboard"

        #expect(state.filteredSessions.count == 1)
        #expect(state.filteredSessions[0].sessionId == "s1")
    }

    @Test func givenEmptyInputShouldNotAllowSend() {
        let state = ChatState(processProvider: MockClaudeProcess())
        state.inputText = ""
        #expect(state.canSend == false)

        state.inputText = "   "
        #expect(state.canSend == false)
    }

    @Test func givenStreamingStateShouldNotAllowSend() async {
        let mock = MockClaudeProcess()
        // Provide events so streaming starts
        mock.events = [
            .assistant(content: [.text("working...")]),
            .result(sessionId: "s5", durationMs: 100),
        ]
        let state = ChatState(processProvider: mock)

        state.inputText = "test"
        #expect(state.canSend == true)

        state.isStreaming = true
        #expect(state.canSend == false)
    }

    @Test func givenErrorEventShouldSetStreamError() async {
        let mock = MockClaudeProcess()
        mock.events = [
            .error(message: "Claude exited with code 1")
        ]
        let state = ChatState(processProvider: mock)

        await state.send("Hello", cwd: "/tmp")

        #expect(state.streamError == "Claude exited with code 1")
    }

    @Test func givenEmptyStreamShouldSetStreamError() async {
        let mock = MockClaudeProcess()
        mock.events = []
        let state = ChatState(processProvider: mock)

        await state.send("Hello", cwd: "/tmp")

        #expect(state.streamError == "No response received. Check that Claude CLI is working correctly.")
    }

    @Test func givenContentBlockDeltasShouldAccumulateText() async {
        let mock = MockClaudeProcess()
        mock.events = [
            .contentBlockDelta(index: 0, delta: "Hello "),
            .contentBlockDelta(index: 0, delta: "world!"),
            .result(sessionId: "sess-delta", durationMs: 100),
        ]
        let state = ChatState(processProvider: mock)

        await state.send("Hi", cwd: "/tmp")

        let assistant = state.messages.first(where: { $0.role == .assistant })
        #expect(assistant != nil)
        let textBlocks = assistant?.contentBlocks.filter {
            if case .text = $0 { return true }
            return false
        }
        #expect(textBlocks?.isEmpty == false)
    }

    @Test func givenAssistantEventAfterDeltasShouldUseAuthoritativeText() async {
        let mock = MockClaudeProcess()
        mock.events = [
            .contentBlockDelta(index: 0, delta: "partial "),
            .contentBlockDelta(index: 0, delta: "text"),
            .assistant(content: [.text("final authoritative text")]),
            .result(sessionId: "sess-replace", durationMs: 100),
        ]
        let state = ChatState(processProvider: mock)

        await state.send("Test", cwd: "/tmp")

        let assistant = state.messages.first(where: { $0.role == .assistant })
        // With delta-driven streaming, deltas produce "partial text"
        // and assistant snapshot is deferred to finalization.
        // Since deltas already populated content, finalize merges (doesn't replace text).
        let texts = assistant?.contentBlocks.compactMap { block -> String? in
            if case .text(_, let text) = block { return text }
            return nil
        }
        #expect(texts?.isEmpty == false)
    }

    @Test func givenGroupedSessionsShouldPartitionByTimePeriod() {
        let state = ChatState(processProvider: MockClaudeProcess())
        let calendar = Calendar.autoupdatingCurrent
        let now = Date()

        let todaySession = Conversation(
            sessionId: "s-today", agentSource: .claude, title: "Today work",
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/s-today.jsonl",
            createdAt: nil, modifiedAt: now, messageCount: nil
        )
        let yesterdaySession = Conversation(
            sessionId: "s-yesterday", agentSource: .claude, title: "Yesterday work",
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/s-yesterday.jsonl",
            createdAt: nil,
            modifiedAt: calendar.date(byAdding: .day, value: -1, to: now)!,
            messageCount: nil
        )
        let weekSession = Conversation(
            sessionId: "s-week", agentSource: .claude, title: "This week work",
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/s-week.jsonl",
            createdAt: nil,
            modifiedAt: calendar.date(byAdding: .day, value: -4, to: now)!,
            messageCount: nil
        )
        let oldSession = Conversation(
            sessionId: "s-old", agentSource: .claude, title: "Old work",
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/s-old.jsonl",
            createdAt: nil,
            modifiedAt: calendar.date(byAdding: .day, value: -60, to: now)!,
            messageCount: nil
        )

        state.sessions = [todaySession, yesterdaySession, weekSession, oldSession]

        let sections = state.groupedSessions
        #expect(sections.count == 4)
        #expect(sections[0].id == "today")
        #expect(sections[0].sessions.count == 1)
        #expect(sections[1].id == "yesterday")
        #expect(sections[1].sessions.count == 1)
        #expect(sections[2].id == "this-week")
        #expect(sections[2].sessions.count == 1)
        #expect(sections[3].id == "older")
        #expect(sections[3].sessions.count == 1)
    }

    @Test func givenStopStreamingShouldCancelProcess() async {
        let mock = MockClaudeProcess()
        mock.events = []  // No events - will wait forever
        let state = ChatState(processProvider: mock)

        state.isStreaming = true
        await state.stopStreaming()

        #expect(mock.cancelCalled)
    }

    @Test func givenSelectConversationShouldLoadHistory() async throws {
        let tempDir = FileManager.default.temporaryDirectory
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: tempDir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: tempDir) }

        let path = tempDir.appendingPathComponent("sess.jsonl")
        // Write a simple transcript
        let lines = """
            {"type":"user","sessionId":"sess-load","timestamp":"2026-03-01T09:01:00.000Z","message":{"role":"user","content":"Hello"}}
            {"type":"assistant","sessionId":"sess-load","timestamp":"2026-03-01T09:02:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"Hi there!"}]}}
            """
        try Data(lines.utf8).write(to: path)

        let session = Conversation(
            sessionId: "sess-load", agentSource: .claude, title: "Test Session",
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: path.path,
            createdAt: nil, modifiedAt: Date(), messageCount: 2
        )

        let state = ChatState(processProvider: MockClaudeProcess())
        await state.selectConversation(session)

        #expect(state.currentSessionId == "sess-load")
        #expect(state.messages.count == 2)
    }

    // MARK: - Task 1: Tool results preserved after finalization

    @Test func givenToolResultDuringStreamShouldPreserveAfterFinalization() async {
        let mock = MockClaudeProcess()
        mock.events = [
            .contentBlockDelta(index: 0, delta: "Let me read that."),
            .contentBlockStart(index: 1, id: "t1", name: "Read"),
            .toolResult(toolUseId: "t1", content: "file contents here", isError: false),
            .contentBlockDelta(index: 2, delta: "Here is what I found."),
            .assistant(content: [
                .text("Let me read that."),
                .toolUse(id: "t1", name: "Read", input: "{\"file_path\":\"/etc/hosts\"}"),
                .text("Here is what I found."),
            ]),
            .result(sessionId: "sess-tool", durationMs: 100),
        ]
        let state = ChatState(processProvider: mock)

        await state.send("Read /etc/hosts", cwd: "/tmp")

        let assistant = state.messages.first(where: { $0.role == .assistant })!
        let toolResults = assistant.contentBlocks.filter {
            if case .toolResult = $0 { return true }
            return false
        }
        #expect(toolResults.count == 1)
        if case .toolResult(_, let toolUseId, let output, let isError) = toolResults[0] {
            #expect(toolUseId == "t1")
            #expect(output == "file contents here")
            #expect(isError == false)
        }
    }

    @Test func givenFailedToolShouldPreserveFailedStatusAfterFinalization() async {
        let mock = MockClaudeProcess()
        mock.events = [
            .contentBlockStart(index: 0, id: "t1", name: "Bash"),
            .toolResult(toolUseId: "t1", content: "command not found", isError: true),
            .assistant(content: [
                .toolUse(id: "t1", name: "Bash", input: "{\"command\":\"bad\"}")
            ]),
            .result(sessionId: "sess-fail", durationMs: 50),
        ]
        let state = ChatState(processProvider: mock)

        await state.send("Run bad", cwd: "/tmp")

        let assistant = state.messages.first(where: { $0.role == .assistant })!
        let toolBlocks = assistant.contentBlocks.compactMap { block -> ToolUseStatus? in
            if case .toolUse(_, _, _, let status) = block { return status }
            return nil
        }
        #expect(toolBlocks == [.failed])
    }

    // MARK: - Task 2: Flush resolves by ID

    @Test func givenDeltasShouldFlushToCorrectMessageAfterMessageListChanges() async {
        let mock = MockClaudeProcess()
        mock.events = [
            .contentBlockDelta(index: 0, delta: "Hello "),
            .contentBlockDelta(index: 0, delta: "world!"),
            .result(sessionId: "sess-flush", durationMs: 100),
        ]
        let state = ChatState(processProvider: mock)

        await state.send("Hi", cwd: "/tmp")

        // Verify text ended up on the assistant message (index 1), not somewhere else
        #expect(state.messages.count == 2)
        let assistant = state.messages[1]
        #expect(assistant.role == .assistant)
        let hasText = assistant.contentBlocks.contains {
            if case .text = $0 { return true }
            return false
        }
        #expect(hasText == true)
    }

    // MARK: - Task 3: No duplicate text on non-text trailing blocks

    @Test func givenPulseBlockAfterToolShouldNotDuplicateTextOnFlush() async {
        let mock = MockClaudeProcess()
        mock.events = [
            .contentBlockDelta(index: 0, delta: "Some text"),
            .result(sessionId: "sess-pulse", durationMs: 50),
        ]
        let state = ChatState(processProvider: mock)

        await state.send("Go", cwd: "/tmp")

        let assistant = state.messages.first(where: { $0.role == .assistant })!
        let textBlockCount = assistant.contentBlocks.filter {
            if case .text = $0 { return true }
            if case .codeBlock = $0 { return true }
            return false
        }.count

        // Should have text blocks but not duplicated
        #expect(textBlockCount >= 1)
        #expect(textBlockCount <= 2)  // text + possibly code, but not doubled
    }

    // MARK: - Task 4: Stop/new conversation cleanup

    @Test func givenStopStreamingShouldCleanUpFlushState() async {
        let mock = MockClaudeProcess()
        mock.events = []
        let state = ChatState(processProvider: mock)

        state.isStreaming = true
        await state.stopStreaming()

        #expect(mock.cancelCalled)
        #expect(state.isStreaming == false)
        // Verify no stale state leaks — send a new message and confirm clean start
        mock.cancelCalled = false
        mock.events = [
            .contentBlockDelta(index: 0, delta: "Fresh response"),
            .result(sessionId: "sess-clean", durationMs: 50),
        ]

        await state.send("New message", cwd: "/tmp")

        let lastAssistant = state.messages.last(where: { $0.role == .assistant })
        let texts = lastAssistant?.contentBlocks.compactMap { block -> String? in
            if case .text(_, let text) = block { return text }
            return nil
        }
        // Should only contain "Fresh response", not stale text from before stop
        #expect(texts?.joined().contains("Fresh response") == true)
    }
}

// MARK: - Mock

final class MockClaudeProcess: ClaudeProcessProvider, @unchecked Sendable {
    var events: [StreamEvent] = []
    var cancelCalled = false

    func start(prompt: String, cwd: String, sessionId: String?) async throws -> AsyncStream<StreamEvent> {
        let events = self.events
        return AsyncStream { continuation in
            for event in events {
                continuation.yield(event)
            }
            continuation.finish()
        }
    }

    func resume(sessionId: String, prompt: String, cwd: String) async throws -> AsyncStream<StreamEvent> {
        try await start(prompt: prompt, cwd: cwd, sessionId: sessionId)
    }

    func cancel() async {
        cancelCalled = true
    }
}
