import Foundation
import Observation

protocol ClaudeProcessProvider: Sendable {
    func start(prompt: String, cwd: String, sessionId: String?) async throws -> AsyncStream<StreamEvent>
    func resume(sessionId: String, prompt: String, cwd: String) async throws -> AsyncStream<StreamEvent>
    func cancel() async
}

@Observable
@MainActor
final class ChatState {
    var messages: [ChatMessage] = []
    var inputText = ""
    var isStreaming = false
    var currentSessionId: String?
    var searchQuery = ""
    var sessions: [Conversation] = []
    var streamError: String?
    var isLoadingSessions = false

    var canSend: Bool {
        !isStreaming && !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var filteredSessions: [Conversation] {
        let query = searchQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return sessions }

        return sessions.filter { session in
            let haystacks =
                [
                    session.title,
                    session.projectName,
                    session.workspaceName,
                    session.projectPath ?? "",
                    session.gitBranch ?? "",
                    session.agentSource.rawValue,
                ] + session.previewSnippets

            return haystacks.contains { $0.localizedCaseInsensitiveContains(query) }
        }
    }

    var groupedSessions: [ConversationSection] {
        let calendar = Calendar.autoupdatingCurrent
        let now = Date()

        var today: [Conversation] = []
        var yesterday: [Conversation] = []
        var thisWeek: [Conversation] = []
        var thisMonth: [Conversation] = []
        var older: [Conversation] = []

        var seen = Set<String>()
        for session in filteredSessions where seen.insert(session.id).inserted {
            let date = session.modifiedAt
            if calendar.isDateInToday(date) {
                today.append(session)
            } else if calendar.isDateInYesterday(date) {
                yesterday.append(session)
            } else {
                let daysAgo = calendar.dateComponents([.day], from: date, to: now).day ?? Int.max
                if daysAgo < 7 {
                    thisWeek.append(session)
                } else if daysAgo < 30 {
                    thisMonth.append(session)
                } else {
                    older.append(session)
                }
            }
        }

        var sections: [ConversationSection] = []
        if !today.isEmpty { sections.append(ConversationSection(id: "today", title: "Today", sessions: today)) }
        if !yesterday.isEmpty {
            sections.append(ConversationSection(id: "yesterday", title: "Yesterday", sessions: yesterday))
        }
        if !thisWeek.isEmpty {
            sections.append(ConversationSection(id: "this-week", title: "This Week", sessions: thisWeek))
        }
        if !thisMonth.isEmpty {
            sections.append(ConversationSection(id: "this-month", title: "This Month", sessions: thisMonth))
        }
        if !older.isEmpty { sections.append(ConversationSection(id: "older", title: "Older", sessions: older)) }
        return sections
    }

    @ObservationIgnored private var streamingText = ""
    @ObservationIgnored private var pendingFlush: Task<Void, Never>?
    @ObservationIgnored private var pendingAssistantBlocks: [StreamContentBlock]?
    @ObservationIgnored private let processProvider: any ClaudeProcessProvider

    init(processProvider: any ClaudeProcessProvider) {
        self.processProvider = processProvider
    }

    func newConversation() {
        resetStreamingState()
        messages = []
        currentSessionId = nil
        inputText = ""
        streamError = nil
    }

    func send(_ text: String, cwd: String) async {
        let prompt = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty, !isStreaming else { return }

        // Append user message
        messages.append(
            ChatMessage(
                role: .user,
                contentBlocks: [.text(id: UUID().uuidString, text: prompt)]
            ))

        // Create placeholder assistant message
        let assistantId = UUID().uuidString
        messages.append(
            ChatMessage(
                id: assistantId,
                role: .assistant,
                isStreaming: true
            ))

        isStreaming = true
        streamError = nil
        streamingText = ""

        do {
            let stream: AsyncStream<StreamEvent>
            if let sessionId = currentSessionId {
                stream = try await processProvider.resume(sessionId: sessionId, prompt: prompt, cwd: cwd)
            } else {
                stream = try await processProvider.start(prompt: prompt, cwd: cwd, sessionId: nil)
            }

            for await event in stream {
                handleStreamEvent(event, assistantMessageId: assistantId)
            }

            // Apply final authoritative assistant snapshot
            finalizeAssistant(assistantMessageId: assistantId)

            // Refresh sidebar sessions after conversation completes
            Task { await refreshSessions() }

            // Surface error if stream produced no content
            if let index = messages.lastIndex(where: { $0.id == assistantId }),
                messages[index].contentBlocks.isEmpty,
                streamError == nil
            {
                streamError = "No response received. Check that Claude CLI is working correctly."
            }
        } catch {
            streamError = error.localizedDescription
        }

        // Mark assistant message as done streaming
        if let index = messages.lastIndex(where: { $0.id == assistantId }) {
            messages[index].isStreaming = false
        }
        isStreaming = false
    }

    func stopStreaming() async {
        await processProvider.cancel()
        resetStreamingState()
        isStreaming = false
    }

    func selectConversation(_ session: Conversation) async {
        if isStreaming {
            await stopStreaming()
        }
        currentSessionId = session.sessionId
        streamError = nil

        guard let transcriptPath = session.transcriptPath else {
            messages = []
            streamError = "No transcript available for this session."
            return
        }

        do {
            let parsed = try TranscriptParser.parse(transcriptPath: transcriptPath)
            messages = parsed
        } catch {
            messages = []
            streamError = "Failed to load conversation: \(error.localizedDescription)"
        }
    }

    func refreshSessions() async {
        guard !isLoadingSessions else { return }
        isLoadingSessions = true

        // Phase 1: indexed sessions (~30ms) — immediate display
        let indexed = try? await Task.detached(priority: .userInitiated) {
            try ClaudeConversationIndex.loadIndexedSessions()
        }.value
        if let indexed {
            sessions = indexed
        }

        // Phase 2: loose sessions (background, merge when ready)
        let existingIds = Set(sessions.map(\.sessionId))
        let loose = try? await Task.detached(priority: .utility) {
            try ClaudeConversationIndex.loadLooseSessions(excluding: existingIds)
        }.value
        if let loose, !loose.isEmpty {
            sessions.append(contentsOf: loose)
            sessions.sort { $0.modifiedAt > $1.modifiedAt }
        }

        isLoadingSessions = false

        // Phase 3: enrich top sessions with snippets (background)
        await enrichSnippets()
    }

    func enrichSnippets(limit: Int = 50) async {
        let toEnrich = Array(
            sessions.prefix(limit).filter {
                $0.previewSnippets.isEmpty && $0.transcriptPath != nil
            }
        )
        guard !toEnrich.isEmpty else { return }

        let results = await withTaskGroup(of: (String, [String]).self) { group in
            for session in toEnrich {
                let sid = session.id
                let url = URL(fileURLWithPath: session.transcriptPath!)
                group.addTask { (sid, ClaudeConversationIndex.recentSnippets(from: url)) }
            }
            var dict: [String: [String]] = [:]
            for await (sid, snippets) in group where !snippets.isEmpty {
                dict[sid] = snippets
            }
            return dict
        }
        for (sid, snippets) in results {
            if let idx = sessions.firstIndex(where: { $0.id == sid }) {
                sessions[idx] = sessions[idx].withSnippets(snippets)
            }
        }
    }

    // MARK: - Pulse

    func injectPulse(from appState: AppState) {
        let allAgents = appState.projects.flatMap(\.allAgents)
        let activeAgents = allAgents.filter { $0.status == .streaming || $0.status == .waiting }
        guard !activeAgents.isEmpty else { return }

        let events = activeAgents.prefix(5).map { agent in
            PulseEvent(agent: agent.name, event: agent.status == .streaming ? "streaming" : "waiting")
        }
        let summary = PulseSummary(activeAgents: activeAgents.count, recentEvents: events)
        let pulseMessage = ChatMessage(
            role: .assistant,
            contentBlocks: [.pulse(id: UUID().uuidString, summary: summary)]
        )
        messages.append(pulseMessage)
    }

    // MARK: - Private

    private func handleStreamEvent(_ event: StreamEvent, assistantMessageId: String) {
        guard let index = messages.lastIndex(where: { $0.id == assistantMessageId }) else { return }

        switch event {
        case .assistant(let blocks):
            // During streaming, assistant snapshots are redundant — deltas drive text.
            // Only apply as final authoritative state (handled in finalizeAssistant).
            // Store for finalization but don't reset mid-stream.
            pendingAssistantBlocks = blocks

        case .contentBlockStart(_, let id, let name):
            // Flush any pending text deltas before adding tool block
            flushPendingDeltas(for: index)
            messages[index].contentBlocks.append(
                .toolUse(id: id, name: name, input: "{}", status: .running)
            )
            // Reset streaming text — tool use starts a new text region
            streamingText = ""

        case .contentBlockDelta(_, let delta):
            streamingText += delta
            scheduleFlush(for: assistantMessageId)

        case .toolResult(let toolUseId, let content, let isError):
            // Flush any pending text first
            flushPendingDeltas(for: index)
            // Update the matching tool_use status
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
            // Append tool result block
            messages[index].contentBlocks.append(
                .toolResult(
                    id: UUID().uuidString,
                    toolUseId: toolUseId,
                    output: content,
                    isError: isError
                ))
            // Reset streaming text for post-tool text
            streamingText = ""

        case .result(let sessionId, _):
            flushPendingDeltas(for: index)
            currentSessionId = sessionId

        case .error(let message):
            flushPendingDeltas(for: index)
            streamError = message

        case .unknown:
            break
        }
    }

    /// Apply the last `assistant` snapshot as authoritative final state.
    /// Called after the stream ends to reconcile any missed deltas.
    private func finalizeAssistant(assistantMessageId: String) {
        pendingFlush?.cancel()
        pendingFlush = nil

        guard let index = messages.lastIndex(where: { $0.id == assistantMessageId }) else {
            pendingAssistantBlocks = nil
            return
        }

        // Always flush remaining deltas first
        flushPendingDeltas(for: index)

        guard let blocks = pendingAssistantBlocks else {
            pendingAssistantBlocks = nil
            return
        }

        // If deltas drove content, merge rather than replace:
        // - Update tool_use inputs from the authoritative snapshot (they have full input JSON)
        // - Preserve tool_result blocks and tool statuses set by toolResult events
        // - Don't touch text blocks — delta-driven text is already correct
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
            // Fallback: no deltas received — use assistant snapshot fully
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

    /// Schedule a throttled flush of accumulated text deltas (~60fps).
    private func scheduleFlush(for assistantMessageId: String) {
        guard pendingFlush == nil else { return }
        pendingFlush = Task { [weak self] in
            try? await Task.sleep(for: .milliseconds(50))
            guard !Task.isCancelled else { return }
            self?.flushPendingDeltasById(assistantMessageId)
            self?.pendingFlush = nil
        }
    }

    /// Resolve message ID to index, then flush.
    private func flushPendingDeltasById(_ assistantMessageId: String) {
        guard let index = messages.lastIndex(where: { $0.id == assistantMessageId }) else { return }
        flushPendingDeltas(for: index)
    }

    /// Apply accumulated streamingText to message content blocks immediately.
    private func flushPendingDeltas(for messageIndex: Int) {
        pendingFlush?.cancel()
        pendingFlush = nil
        guard messageIndex < messages.count, !streamingText.isEmpty else { return }

        // Find where text blocks start (after any non-text blocks at the beginning or after last tool block)
        let lastToolIndex = messages[messageIndex].contentBlocks.lastIndex(where: {
            if case .toolUse = $0 { return true }
            if case .toolResult = $0 { return true }
            return false
        })
        let textStartIndex = (lastToolIndex ?? -1) + 1

        // Remove existing text/codeBlock entries from textStartIndex onward
        // Keep any non-text blocks (pulse, etc.) by filtering
        if textStartIndex < messages[messageIndex].contentBlocks.count {
            var kept: [ContentBlock] = []
            for i in textStartIndex..<messages[messageIndex].contentBlocks.count {
                let block = messages[messageIndex].contentBlocks[i]
                switch block {
                case .text, .codeBlock:
                    continue  // remove these
                default:
                    kept.append(block)  // keep pulse, etc.
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

    private func resetStreamingState() {
        pendingFlush?.cancel()
        pendingFlush = nil
        pendingAssistantBlocks = nil
        streamingText = ""
    }
}
