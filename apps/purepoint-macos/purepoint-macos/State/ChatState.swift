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
    var sessions: [ClaudeConversation] = []
    var streamError: String?
    var isLoadingSessions = false

    var canSend: Bool {
        !isStreaming && !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var filteredSessions: [ClaudeConversation] {
        let query = searchQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return sessions }

        return sessions.filter { session in
            let haystacks = [
                session.title,
                session.projectName,
                session.workspaceName,
                session.projectPath,
                session.gitBranch ?? ""
            ] + session.previewSnippets

            return haystacks.contains { $0.localizedCaseInsensitiveContains(query) }
        }
    }

    @ObservationIgnored private let processProvider: any ClaudeProcessProvider

    init(processProvider: any ClaudeProcessProvider) {
        self.processProvider = processProvider
    }

    func newConversation() {
        messages = []
        currentSessionId = nil
        inputText = ""
        streamError = nil
    }

    func send(_ text: String, cwd: String) async {
        let prompt = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty, !isStreaming else { return }

        // Append user message
        messages.append(ChatMessage(
            role: .user,
            contentBlocks: [.text(id: UUID().uuidString, text: prompt)]
        ))

        // Create placeholder assistant message
        let assistantId = UUID().uuidString
        messages.append(ChatMessage(
            id: assistantId,
            role: .assistant,
            isStreaming: true
        ))

        isStreaming = true
        streamError = nil

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
        isStreaming = false
    }

    func selectConversation(_ session: ClaudeConversation) async {
        if isStreaming {
            await stopStreaming()
        }
        currentSessionId = session.sessionId
        streamError = nil

        do {
            let parsed = try TranscriptParser.parse(transcriptPath: session.transcriptPath)
            messages = parsed
        } catch {
            messages = []
            streamError = "Failed to load conversation: \(error.localizedDescription)"
        }
    }

    func refreshSessions() async {
        guard !isLoadingSessions else { return }
        isLoadingSessions = true

        do {
            let loaded = try await Task.detached(priority: .utility) {
                try ClaudeConversationIndex.loadSessions()
            }.value
            sessions = loaded
        } catch {
            // Silently ignore — sessions sidebar just stays empty
        }

        isLoadingSessions = false
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
            for block in blocks {
                switch block {
                case .text(let text):
                    let split = ContentBlockSplitter.split(text)
                    messages[index].contentBlocks.append(contentsOf: split)
                case .toolUse(let id, let name, let input):
                    messages[index].contentBlocks.append(.toolUse(
                        id: id, name: name, input: input, status: .running
                    ))
                }
            }

        case .toolResult(let toolUseId, let content, let isError):
            // Update the matching tool_use status
            for i in messages[index].contentBlocks.indices {
                if case .toolUse(let id, let name, let input, _) = messages[index].contentBlocks[i],
                   id == toolUseId {
                    messages[index].contentBlocks[i] = .toolUse(
                        id: id, name: name, input: input,
                        status: isError ? .failed : .completed
                    )
                }
            }
            // Append tool result block
            messages[index].contentBlocks.append(.toolResult(
                id: UUID().uuidString,
                toolUseId: toolUseId,
                output: content,
                isError: isError
            ))

        case .result(let sessionId, _):
            currentSessionId = sessionId

        case .unknown:
            break
        }
    }
}
