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
    var streamError: String?

    var canSend: Bool {
        !isStreaming && !inputText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    @ObservationIgnored private let processProvider: any ClaudeProcessProvider
    @ObservationIgnored private let accumulator = StreamAccumulator()

    /// Optional callback fired after a conversation completes (e.g. to refresh sidebar).
    @ObservationIgnored var onConversationCompleted: (() async -> Void)?

    init(processProvider: any ClaudeProcessProvider) {
        self.processProvider = processProvider
        accumulator.flushHandler = { [weak self] messageId in
            guard let self,
                let idx = self.messages.lastIndex(where: { $0.id == messageId })
            else { return }
            self.accumulator.flushPendingDeltas(for: idx, messages: &self.messages)
        }
    }

    func newConversation() {
        accumulator.reset()
        messages = []
        currentSessionId = nil
        inputText = ""
        streamError = nil
    }

    func send(_ text: String, cwd: String) async {
        let prompt = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty, !isStreaming else { return }

        messages.append(
            ChatMessage(
                role: .user,
                contentBlocks: [.text(id: UUID().uuidString, text: prompt)]
            ))

        let assistantId = UUID().uuidString
        messages.append(
            ChatMessage(
                id: assistantId,
                role: .assistant,
                isStreaming: true
            ))

        isStreaming = true
        streamError = nil
        accumulator.reset()

        do {
            let stream: AsyncStream<StreamEvent>
            if let sessionId = currentSessionId {
                stream = try await processProvider.resume(sessionId: sessionId, prompt: prompt, cwd: cwd)
            } else {
                stream = try await processProvider.start(prompt: prompt, cwd: cwd, sessionId: nil)
            }

            for await event in stream {
                let result = accumulator.handle(event, assistantMessageId: assistantId, messages: &messages)
                switch result {
                case .none:
                    break
                case .sessionId(let id):
                    currentSessionId = id
                case .error(let message):
                    streamError = message
                }
            }

            accumulator.finalize(assistantMessageId: assistantId, messages: &messages)

            if let callback = onConversationCompleted {
                Task { await callback() }
            }

            if let index = messages.lastIndex(where: { $0.id == assistantId }),
                messages[index].contentBlocks.isEmpty,
                streamError == nil
            {
                streamError = "No response received. Check that Claude CLI is working correctly."
            }
        } catch {
            streamError = error.localizedDescription
        }

        if let index = messages.lastIndex(where: { $0.id == assistantId }) {
            messages[index].isStreaming = false
        }
        isStreaming = false
    }

    func stopStreaming() async {
        await processProvider.cancel()
        accumulator.reset()
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
}
