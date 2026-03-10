import Foundation
import Observation

struct ConversationSection: Identifiable {
    let id: String
    let title: String
    let sessions: [Conversation]

    /// Group conversations by time period (today, yesterday, this week, this month, older).
    static func grouped(from conversations: [Conversation]) -> [ConversationSection] {
        let calendar = Calendar.autoupdatingCurrent
        let now = Date()

        var today: [Conversation] = []
        var yesterday: [Conversation] = []
        var thisWeek: [Conversation] = []
        var thisMonth: [Conversation] = []
        var older: [Conversation] = []

        var seen = Set<String>()
        for session in conversations where seen.insert(session.id).inserted {
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
}

@Observable
@MainActor
final class SessionListState {
    var sessions: [Conversation] = []
    var searchQuery = ""
    var isLoadingSessions = false

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
        ConversationSection.grouped(from: filteredSessions)
    }

    func refreshSessions() async {
        guard !isLoadingSessions else { return }
        isLoadingSessions = true

        // Phase 1: Load Claude indexed + Codex indexed + OpenCode (all 3 in parallel)
        async let claudeIndexed = Task.detached(priority: .userInitiated) {
            try ClaudeConversationIndex.loadIndexedSessions()
        }.value
        async let codexIndexed = Task.detached(priority: .userInitiated) {
            try CodexConversationIndex.loadIndexedSessions()
        }.value
        async let opencodeSessions = Task.detached(priority: .userInitiated) {
            try OpenCodeConversationIndex.loadSessions()
        }.value

        var all: [Conversation] = []
        if let claude = try? await claudeIndexed { all.append(contentsOf: claude) }
        if let codex = try? await codexIndexed { all.append(contentsOf: codex) }
        if let opencode = try? await opencodeSessions { all.append(contentsOf: opencode) }
        all.sort { $0.modifiedAt > $1.modifiedAt }
        sessions = all

        // Phase 2: Claude loose sessions + Codex metadata enrichment (background)
        let existingClaudeIds = Set(all.filter { $0.agentSource == .claude }.map(\.sessionId))
        let codexToEnrich = all.filter { $0.agentSource == .codex }

        async let loose = Task.detached(priority: .utility) {
            try ClaudeConversationIndex.loadLooseSessions(excluding: existingClaudeIds)
        }.value
        async let enrichedCodex = Task.detached(priority: .utility) {
            try CodexConversationIndex.enrichWithMetadata(codexToEnrich, limit: 50)
        }.value

        if let looseResults = try? await loose, !looseResults.isEmpty {
            sessions.append(contentsOf: looseResults)
            sessions.sort { $0.modifiedAt > $1.modifiedAt }
        }
        if let enriched = try? await enrichedCodex {
            for enrichedSession in enriched {
                if let idx = sessions.firstIndex(where: { $0.id == enrichedSession.id }) {
                    sessions[idx] = enrichedSession
                }
            }
        }

        isLoadingSessions = false

        // Phase 3: enrich Claude sessions with snippets
        await enrichSnippets()
    }

    func enrichSnippets(limit: Int = 50) async {
        let toEnrich = Array(
            sessions.prefix(limit).filter {
                $0.previewSnippets.isEmpty && $0.agentSource == .claude && $0.transcriptPath != nil
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
}
