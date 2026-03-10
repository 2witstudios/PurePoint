import Foundation
import Testing
@testable import PurePoint

@MainActor
struct StreamAccumulatorTests {

    // MARK: - Delta accumulation

    @Test func givenTextDeltasShouldAccumulateAndFlush() {
        let accumulator = StreamAccumulator()
        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        _ = accumulator.handle(
            .contentBlockDelta(index: 0, delta: "Hello "),
            assistantMessageId: "a1", messages: &messages
        )
        _ = accumulator.handle(
            .contentBlockDelta(index: 0, delta: "world!"),
            assistantMessageId: "a1", messages: &messages
        )

        accumulator.finalize(assistantMessageId: "a1", messages: &messages)

        let texts = messages[0].contentBlocks.compactMap { block -> String? in
            if case .text(_, let text) = block { return text }
            return nil
        }
        #expect(texts.joined() == "Hello world!")
    }

    // MARK: - Tool use events

    @Test func givenToolBlockStartShouldFlushTextAndAppendTool() {
        let accumulator = StreamAccumulator()
        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        _ = accumulator.handle(
            .contentBlockDelta(index: 0, delta: "Before tool."),
            assistantMessageId: "a1", messages: &messages
        )
        _ = accumulator.handle(
            .contentBlockStart(index: 1, id: "t1", name: "Read"),
            assistantMessageId: "a1", messages: &messages
        )

        // Text should be flushed before the tool block
        let hasText = messages[0].contentBlocks.contains {
            if case .text = $0 { return true }
            return false
        }
        #expect(hasText)

        let hasTool = messages[0].contentBlocks.contains {
            if case .toolUse(let id, _, _, _) = $0 { return id == "t1" }
            return false
        }
        #expect(hasTool)
    }

    // MARK: - Tool result preservation

    @Test func givenToolResultShouldPreserveThroughFinalization() {
        let accumulator = StreamAccumulator()
        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        _ = accumulator.handle(
            .contentBlockStart(index: 0, id: "t1", name: "Bash"),
            assistantMessageId: "a1", messages: &messages
        )
        _ = accumulator.handle(
            .toolResult(toolUseId: "t1", content: "output here", isError: false),
            assistantMessageId: "a1", messages: &messages
        )
        _ = accumulator.handle(
            .assistant(content: [.toolUse(id: "t1", name: "Bash", input: "{\"cmd\":\"ls\"}")]),
            assistantMessageId: "a1", messages: &messages
        )

        accumulator.finalize(assistantMessageId: "a1", messages: &messages)

        let toolResults = messages[0].contentBlocks.filter {
            if case .toolResult = $0 { return true }
            return false
        }
        #expect(toolResults.count == 1)
        if case .toolResult(_, let toolUseId, let output, _) = toolResults[0] {
            #expect(toolUseId == "t1")
            #expect(output == "output here")
        }
    }

    // MARK: - Failed tool status preserved

    @Test func givenFailedToolResultShouldPreserveFailedStatus() {
        let accumulator = StreamAccumulator()
        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        _ = accumulator.handle(
            .contentBlockStart(index: 0, id: "t1", name: "Bash"),
            assistantMessageId: "a1", messages: &messages
        )
        _ = accumulator.handle(
            .toolResult(toolUseId: "t1", content: "error", isError: true),
            assistantMessageId: "a1", messages: &messages
        )
        _ = accumulator.handle(
            .assistant(content: [.toolUse(id: "t1", name: "Bash", input: "{}")]),
            assistantMessageId: "a1", messages: &messages
        )

        accumulator.finalize(assistantMessageId: "a1", messages: &messages)

        let statuses = messages[0].contentBlocks.compactMap { block -> ToolUseStatus? in
            if case .toolUse(_, _, _, let status) = block { return status }
            return nil
        }
        #expect(statuses == [.failed])
    }

    // MARK: - Finalize with no deltas uses snapshot

    @Test func givenNoDeltas_finalizeShouldUseAssistantSnapshot() {
        let accumulator = StreamAccumulator()
        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        _ = accumulator.handle(
            .assistant(content: [.text("Full text from snapshot")]),
            assistantMessageId: "a1", messages: &messages
        )

        accumulator.finalize(assistantMessageId: "a1", messages: &messages)

        let texts = messages[0].contentBlocks.compactMap { block -> String? in
            if case .text(_, let text) = block { return text }
            return nil
        }
        #expect(texts.joined() == "Full text from snapshot")
    }

    @Test func givenDeltasPlusSnapshot_finalizeShouldPreserveDeltaText() {
        let accumulator = StreamAccumulator()
        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        // Deltas drive the text content
        _ = accumulator.handle(
            .contentBlockDelta(index: 0, delta: "partial "),
            assistantMessageId: "a1", messages: &messages
        )
        _ = accumulator.handle(
            .contentBlockDelta(index: 0, delta: "text"),
            assistantMessageId: "a1", messages: &messages
        )
        // Snapshot arrives — stored but does NOT replace delta-driven text
        _ = accumulator.handle(
            .assistant(content: [.text("partial text")]),
            assistantMessageId: "a1", messages: &messages
        )

        accumulator.finalize(assistantMessageId: "a1", messages: &messages)

        let texts = messages[0].contentBlocks.compactMap { block -> String? in
            if case .text(_, let text) = block { return text }
            return nil
        }
        // Delta-driven text is authoritative when blocks are non-empty
        #expect(texts.joined() == "partial text")
    }

    // MARK: - Reset clears state

    @Test func givenResetShouldClearAllBufferingState() {
        let accumulator = StreamAccumulator()
        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        _ = accumulator.handle(
            .contentBlockDelta(index: 0, delta: "partial"),
            assistantMessageId: "a1", messages: &messages
        )

        accumulator.reset()

        // After reset, finalize should produce nothing (no pending blocks, no text)
        var freshMessages: [ChatMessage] = [
            ChatMessage(id: "a2", role: .assistant, isStreaming: true)
        ]
        accumulator.finalize(assistantMessageId: "a2", messages: &freshMessages)
        #expect(freshMessages[0].contentBlocks.isEmpty)
    }

    // MARK: - Session ID returned from result event

    @Test func givenResultEventShouldReturnSessionId() {
        let accumulator = StreamAccumulator()
        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        let result = accumulator.handle(
            .result(sessionId: "sess-42", durationMs: 100),
            assistantMessageId: "a1", messages: &messages
        )

        if case .sessionId(let id) = result {
            #expect(id == "sess-42")
        } else {
            Issue.record("Expected .sessionId result")
        }
    }

    // MARK: - Error returned from error event

    @Test func givenErrorEventShouldReturnError() {
        let accumulator = StreamAccumulator()
        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        let result = accumulator.handle(
            .error(message: "something broke"),
            assistantMessageId: "a1", messages: &messages
        )

        if case .error(let msg) = result {
            #expect(msg == "something broke")
        } else {
            Issue.record("Expected .error result")
        }
    }

    // MARK: - Timer-based flush callback

    @Test func givenDeltaWithFlushHandler_shouldCallHandlerAfterDelay() async throws {
        let accumulator = StreamAccumulator()
        var handlerCalledWith: String?
        accumulator.flushHandler = { messageId in
            handlerCalledWith = messageId
        }

        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        _ = accumulator.handle(
            .contentBlockDelta(index: 0, delta: "Hello"),
            assistantMessageId: "a1", messages: &messages
        )

        // Wait for the 50ms timer to fire
        try await Task.sleep(for: .milliseconds(100))

        #expect(handlerCalledWith == "a1")
    }

    @Test func givenSynchronousFlush_shouldNotCallHandler() async throws {
        let accumulator = StreamAccumulator()
        var handlerCalled = false
        accumulator.flushHandler = { _ in
            handlerCalled = true
        }

        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        _ = accumulator.handle(
            .contentBlockDelta(index: 0, delta: "Hello"),
            assistantMessageId: "a1", messages: &messages
        )
        // Synchronous flush via finalize cancels the timer
        accumulator.finalize(assistantMessageId: "a1", messages: &messages)

        try await Task.sleep(for: .milliseconds(100))

        #expect(handlerCalled == false)
    }

    // MARK: - Post-tool text accumulation

    @Test func givenTextAfterToolShouldAppendCorrectly() {
        let accumulator = StreamAccumulator()
        var messages: [ChatMessage] = [
            ChatMessage(id: "a1", role: .assistant, isStreaming: true)
        ]

        _ = accumulator.handle(
            .contentBlockDelta(index: 0, delta: "Before."),
            assistantMessageId: "a1", messages: &messages
        )
        _ = accumulator.handle(
            .contentBlockStart(index: 1, id: "t1", name: "Read"),
            assistantMessageId: "a1", messages: &messages
        )
        _ = accumulator.handle(
            .toolResult(toolUseId: "t1", content: "file data", isError: false),
            assistantMessageId: "a1", messages: &messages
        )
        _ = accumulator.handle(
            .contentBlockDelta(index: 2, delta: "After."),
            assistantMessageId: "a1", messages: &messages
        )
        _ = accumulator.handle(
            .assistant(content: [
                .text("Before."),
                .toolUse(id: "t1", name: "Read", input: "{}"),
                .text("After."),
            ]),
            assistantMessageId: "a1", messages: &messages
        )

        accumulator.finalize(assistantMessageId: "a1", messages: &messages)

        let texts = messages[0].contentBlocks.compactMap { block -> String? in
            if case .text(_, let text) = block { return text }
            return nil
        }
        #expect(texts.contains("Before."))
        #expect(texts.contains("After."))
    }
}
