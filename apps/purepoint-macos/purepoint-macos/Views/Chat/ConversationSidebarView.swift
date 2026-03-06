import SwiftUI

struct ConversationSidebarView: View {
    @Bindable var chatState: ChatState

    private static let relativeFormatter: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter
    }()

    var body: some View {
        VStack(spacing: 0) {
            // New Chat button
            Button {
                chatState.newConversation()
            } label: {
                Label("New Chat", systemImage: "plus.bubble")
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
            }
            .buttonStyle(.plain)
            .padding(.horizontal, 8)
            .padding(.top, 8)

            // Search
            TextField("Search conversations...", text: $chatState.searchQuery)
                .textFieldStyle(.roundedBorder)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)

            Divider()

            // Session list
            if chatState.filteredSessions.isEmpty {
                ContentUnavailableView(
                    chatState.searchQuery.isEmpty ? "No conversations" : "No results",
                    systemImage: "text.bubble"
                )
                .frame(maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 2) {
                        ForEach(chatState.filteredSessions) { session in
                            sessionRow(session)
                        }
                    }
                    .padding(8)
                }
            }
        }
        .background(Color(NSColor.controlBackgroundColor))
    }

    private func sessionRow(_ session: ClaudeConversation) -> some View {
        let isSelected = chatState.currentSessionId == session.sessionId

        return Button {
            Task { await chatState.selectConversation(session) }
        } label: {
            VStack(alignment: .leading, spacing: 4) {
                Text(session.title)
                    .font(.system(size: 13, weight: isSelected ? .semibold : .regular))
                    .lineLimit(2)
                    .foregroundStyle(isSelected ? .primary : .secondary)

                HStack(spacing: 6) {
                    Text(session.projectName)
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)

                    Spacer()

                    Text(Self.relativeFormatter.localizedString(
                        for: session.modifiedAt, relativeTo: Date()
                    ))
                    .font(.system(size: 10))
                    .foregroundStyle(.tertiary)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 8)
                    .fill(isSelected ? Color.accentColor.opacity(0.12) : Color.clear)
            )
        }
        .buttonStyle(.plain)
    }
}
