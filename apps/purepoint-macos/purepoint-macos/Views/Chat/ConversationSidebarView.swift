import SwiftUI

struct ConversationSidebarView: View {
    @Bindable var chatState: ChatState
    @Binding var showSidebar: Bool
    @Environment(AppState.self) private var appState

    private static let relativeFormatter: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter
    }()

    private var agentNamesBySessionId: [String: String] {
        var dict: [String: String] = [:]
        for project in appState.projects {
            for agent in project.allAgents {
                if let sid = agent.sessionId {
                    dict[sid] = agent.displayName
                }
            }
        }
        return dict
    }

    var body: some View {
        let namesBySid = agentNamesBySessionId

        VStack(spacing: 0) {
            // Header: sidebar toggle + New Chat
            HStack {
                Button {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        showSidebar = false
                    }
                } label: {
                    Image(systemName: "sidebar.left")
                }
                .buttonStyle(.plain)
                .help("Hide conversations (⌘⇧S)")

                Spacer()

                Button {
                    chatState.newConversation()
                } label: {
                    Label("New Chat", systemImage: "plus.bubble")
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 12)
            .padding(.top, 8)
            .padding(.bottom, 4)

            // Search
            TextField("Search conversations...", text: $chatState.searchQuery)
                .textFieldStyle(.roundedBorder)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)

            Divider()

            // Session list
            if chatState.groupedSessions.isEmpty {
                ContentUnavailableView(
                    chatState.searchQuery.isEmpty ? "No conversations" : "No results",
                    systemImage: "text.bubble"
                )
                .frame(maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(spacing: 2, pinnedViews: .sectionHeaders) {
                        ForEach(chatState.groupedSessions) { section in
                            Section {
                                ForEach(section.sessions) { session in
                                    sessionRow(session, agentName: namesBySid[session.sessionId])
                                }
                            } header: {
                                Text(section.title)
                                    .font(.system(size: 10, weight: .medium))
                                    .foregroundStyle(.secondary)
                                    .textCase(.uppercase)
                                    .padding(.horizontal, 10)
                                    .padding(.top, 8)
                                    .padding(.bottom, 2)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .background(.background)
                            }
                        }
                    }
                    .padding(8)
                }
            }
        }
    }

    private func sessionRow(_ session: ClaudeConversation, agentName matchedAgentName: String?) -> some View {
        let isSelected = chatState.currentSessionId == session.sessionId

        return Button {
            Task { await chatState.selectConversation(session) }
        } label: {
            VStack(alignment: .leading, spacing: 3) {
                // Primary line: agent name (or title) + relative time
                HStack {
                    Text(matchedAgentName ?? session.title)
                        .font(.system(size: 13, weight: matchedAgentName != nil || isSelected ? .semibold : .regular))
                        .lineLimit(1)
                        .foregroundStyle(isSelected ? .primary : .secondary)

                    Spacer()

                    Text(Self.relativeFormatter.localizedString(
                        for: session.modifiedAt, relativeTo: Date()
                    ))
                    .font(.system(size: 10))
                    .foregroundStyle(.tertiary)
                }

                // Secondary: title as subtitle when agent name is primary
                if matchedAgentName != nil {
                    Text(session.title)
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                // Snippet preview (shows progressively as enrichment completes)
                if let snippet = session.previewSnippets.last, !snippet.isEmpty {
                    Text(snippet)
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                        .italic()
                }

                // Metadata: project, branch, message count
                HStack(spacing: 6) {
                    Text(session.projectName)
                        .font(.system(size: 10))
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)

                    if let branch = session.gitBranch {
                        HStack(spacing: 2) {
                            Image(systemName: "arrow.triangle.branch")
                                .font(.system(size: 8))
                            Text(branch)
                                .lineLimit(1)
                        }
                        .font(.system(size: 10))
                        .foregroundStyle(.tertiary)
                    }

                    if let count = session.messageCount {
                        HStack(spacing: 2) {
                            Image(systemName: "text.bubble")
                                .font(.system(size: 8))
                            Text("\(count)")
                        }
                        .font(.system(size: 10))
                        .foregroundStyle(.tertiary)
                    }

                    Spacer()
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 5)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: 8)
                    .fill(isSelected ? Color.accentColor.opacity(0.12) : Color.clear)
            )
        }
        .buttonStyle(.plain)
    }
}
