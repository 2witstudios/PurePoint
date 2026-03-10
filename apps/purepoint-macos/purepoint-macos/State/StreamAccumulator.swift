import Foundation

/// Owns the delta-buffering state machine for streaming assistant responses.
/// Handles event dispatch, throttled flush, and final reconciliation with
/// the authoritative assistant snapshot.
@MainActor
final class StreamAccumulator {
    private(set) var streamingText = ""
    private var pendingFlush: Task<Void, Never>?
    private var pendingAssistantBlocks: [StreamContentBlock]?

    /// Called when the throttle timer fires, passing the assistant message ID.
    /// The owner (ChatState) uses this to flush accumulated text to the UI.
    var flushHandler: (@MainActor (String) -> Void)?

    /// Process a single stream event, mutating the messages array.
    func handle(_ event: StreamEvent, assistantMessageId: String, messages: inout [ChatMessage]) -> StreamEventResult {
        guard let index = messages.lastIndex(where: { $0.id == assistantMessageId }) else {
            return .none
        }

        switch event {
        case .assistant(let blocks):
            pendingAssistantBlocks = blocks
            return .none

        case .contentBlockStart(_, let id, let name):
            flushPendingDeltas(for: index, messages: &messages)
            messages[index].contentBlocks.append(
                .toolUse(id: id, name: name, input: "{}", status: .running)
            )
            streamingText = ""
            return .none

        case .contentBlockDelta(_, let delta):
            streamingText += delta
            scheduleFlush(for: assistantMessageId)
            return .none

        case .toolResult(let toolUseId, let content, let isError):
            flushPendingDeltas(for: index, messages: &messages)
            for i in messages[index].contentBlocks.indices {
                if case .toolUse(let id, let name, let input, _) = messages[index].contentBlocks[i],
                    id == toolUseId
                {
                    messages[index].contentBlocks[i] = .toolUse(
                        id: id, name: name, input: input,
                        status: isError ? .failed : .completed
                    )
                }
            }
            messages[index].contentBlocks.append(
                .toolResult(
                    id: UUID().uuidString,
                    toolUseId: toolUseId,
                    output: content,
                    isError: isError
                ))
            streamingText = ""
            return .none

        case .result(let sessionId, _):
            flushPendingDeltas(for: index, messages: &messages)
            return .sessionId(sessionId)

        case .error(let message):
            flushPendingDeltas(for: index, messages: &messages)
            return .error(message)

        case .unknown:
            return .none
        }
    }

    /// Apply the last assistant snapshot as authoritative final state.
    /// Called after the stream ends to reconcile any missed deltas.
    func finalize(assistantMessageId: String, messages: inout [ChatMessage]) {
        pendingFlush?.cancel()
        pendingFlush = nil

        guard let index = messages.lastIndex(where: { $0.id == assistantMessageId }) else {
            pendingAssistantBlocks = nil
            return
        }

        flushPendingDeltas(for: index, messages: &messages)

        guard let blocks = pendingAssistantBlocks else {
            pendingAssistantBlocks = nil
            return
        }

        if !messages[index].contentBlocks.isEmpty {
            for block in blocks {
                if case .toolUse(let id, _, let input) = block {
                    if let i = messages[index].contentBlocks.firstIndex(where: {
                        if case .toolUse(let existingId, _, _, _) = $0 { return existingId == id }
                        return false
                    }) {
                        if case .toolUse(_, let name, _, let status) = messages[index].contentBlocks[i] {
                            messages[index].contentBlocks[i] = .toolUse(
                                id: id, name: name, input: input,
                                status: status == .running ? .completed : status
                            )
                        }
                    }
                }
            }
        } else {
            var blockCount = 0
            for block in blocks {
                switch block {
                case .text(let text):
                    let split = ContentBlockSplitter.split(text, startIndex: blockCount)
                    blockCount += split.count
                    messages[index].contentBlocks.append(contentsOf: split)
                case .toolUse(let id, let name, let input):
                    messages[index].contentBlocks.append(
                        .toolUse(id: id, name: name, input: input, status: .completed)
                    )
                    blockCount += 1
                }
            }
        }

        pendingAssistantBlocks = nil
        streamingText = ""
    }

    /// Cancel pending flush and clear all buffering state.
    func reset() {
        pendingFlush?.cancel()
        pendingFlush = nil
        pendingAssistantBlocks = nil
        streamingText = ""
    }

    // MARK: - Private

    private func scheduleFlush(for assistantMessageId: String) {
        guard pendingFlush == nil else { return }
        pendingFlush = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(50))
            guard !Task.isCancelled else { return }
            self?.pendingFlush = nil
            self?.flushHandler?(assistantMessageId)
        }
    }

    /// Apply accumulated streamingText to message content blocks immediately.
    func flushPendingDeltas(for messageIndex: Int, messages: inout [ChatMessage]) {
        pendingFlush?.cancel()
        pendingFlush = nil
        guard messageIndex < messages.count, !streamingText.isEmpty else { return }

        let lastToolIndex = messages[messageIndex].contentBlocks.lastIndex(where: {
            if case .toolUse = $0 { return true }
            if case .toolResult = $0 { return true }
            return false
        })
        let textStartIndex = (lastToolIndex ?? -1) + 1

        if textStartIndex < messages[messageIndex].contentBlocks.count {
            var kept: [ContentBlock] = []
            for i in textStartIndex..<messages[messageIndex].contentBlocks.count {
                let block = messages[messageIndex].contentBlocks[i]
                switch block {
                case .text, .codeBlock:
                    continue
                default:
                    kept.append(block)
                }
            }
            messages[messageIndex].contentBlocks.removeSubrange(textStartIndex...)
            let split = ContentBlockSplitter.split(streamingText, startIndex: textStartIndex)
            messages[messageIndex].contentBlocks.append(contentsOf: split)
            messages[messageIndex].contentBlocks.append(contentsOf: kept)
        } else {
            let split = ContentBlockSplitter.split(streamingText, startIndex: textStartIndex)
            messages[messageIndex].contentBlocks.append(contentsOf: split)
        }
    }
}

/// Result from handling a stream event — lets ChatState act on side effects
/// without StreamAccumulator needing to know about ChatState's other properties.
enum StreamEventResult {
    case none
    case sessionId(String)
    case error(String)
}
