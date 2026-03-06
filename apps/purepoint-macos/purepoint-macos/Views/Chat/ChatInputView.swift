import SwiftUI

struct ChatInputView: View {
    @Bindable var chatState: ChatState
    @Environment(AppState.self) private var appState
    @FocusState private var isFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: 0) {
                TextField("Message Point Guard...", text: $chatState.inputText, axis: .vertical)
                    .textFieldStyle(.plain)
                    .font(.system(size: 14))
                    .lineLimit(1...10)
                    .fixedSize(horizontal: false, vertical: true)
                    .focused($isFocused)
                    .onKeyPress(.return) {
                        if NSEvent.modifierFlags.contains(.shift) {
                            return .ignored // Let shift+enter create newline
                        }
                        if chatState.canSend {
                            sendMessage()
                        }
                        return .handled
                    }

                actionButton
                    .padding(.leading, 4)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 8)
            .background(
                RoundedRectangle(cornerRadius: 16)
                    .fill(Color(NSColor.controlBackgroundColor))
                    .overlay(
                        RoundedRectangle(cornerRadius: 16)
                            .strokeBorder(Color.primary.opacity(0.08), lineWidth: 1)
                    )
            )
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
        }
        .onAppear { isFocused = true }
    }

    @ViewBuilder
    private var actionButton: some View {
        if chatState.isStreaming {
            Button {
                Task { await chatState.stopStreaming() }
            } label: {
                Image(systemName: "stop.circle.fill")
                    .font(.system(size: 24))
                    .foregroundStyle(.red)
            }
            .buttonStyle(.plain)
            .help("Stop streaming")
        } else {
            Button {
                sendMessage()
            } label: {
                Image(systemName: "arrow.up.circle.fill")
                    .font(.system(size: 24))
                    .foregroundStyle(chatState.canSend ? Color.accentColor : Color.secondary.opacity(0.4))
            }
            .buttonStyle(.plain)
            .disabled(!chatState.canSend)
            .help("Send message")
        }
    }

    private func sendMessage() {
        let text = chatState.inputText
        chatState.inputText = ""

        // Use the resumed session's project path if available, otherwise fall back to active project
        let cwd: String
        if let sessionId = chatState.currentSessionId,
           let session = chatState.sessions.first(where: { $0.sessionId == sessionId }) {
            cwd = session.projectPath
        } else {
            cwd = appState.activeProjectRoot ?? appState.projects.first?.projectRoot ?? FileManager.default.currentDirectoryPath
        }
        Task {
            await chatState.send(text, cwd: cwd)
        }
    }
}
