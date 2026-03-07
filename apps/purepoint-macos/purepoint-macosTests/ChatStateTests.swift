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

        let session1 = ClaudeConversation(
            sessionId: "s1", title: "Dashboard work",
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/s1.jsonl",
            createdAt: nil, modifiedAt: Date(), messageCount: nil
        )
        let session2 = ClaudeConversation(
            sessionId: "s2", title: "Terminal bugs",
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

    @Test func givenAssistantEventAfterDeltasShouldReplaceContent() async {
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
        guard case .text(_, let text) = assistant?.contentBlocks.first else {
            Issue.record("Expected text block"); return
        }
        #expect(text == "final authoritative text")
    }

    @Test func givenGroupedSessionsShouldPartitionByTimePeriod() {
        let state = ChatState(processProvider: MockClaudeProcess())
        let calendar = Calendar.autoupdatingCurrent
        let now = Date()

        let todaySession = ClaudeConversation(
            sessionId: "s-today", title: "Today work",
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/s-today.jsonl",
            createdAt: nil, modifiedAt: now, messageCount: nil
        )
        let yesterdaySession = ClaudeConversation(
            sessionId: "s-yesterday", title: "Yesterday work",
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/s-yesterday.jsonl",
            createdAt: nil,
            modifiedAt: calendar.date(byAdding: .day, value: -1, to: now)!,
            messageCount: nil
        )
        let weekSession = ClaudeConversation(
            sessionId: "s-week", title: "This week work",
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/s-week.jsonl",
            createdAt: nil,
            modifiedAt: calendar.date(byAdding: .day, value: -4, to: now)!,
            messageCount: nil
        )
        let oldSession = ClaudeConversation(
            sessionId: "s-old", title: "Old work",
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

        let session = ClaudeConversation(
            sessionId: "sess-load", title: "Test Session",
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
