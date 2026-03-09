import Foundation
import Observation

struct ConversationSection: Identifiable {
    let id: String
    let title: String
    let sessions: [ClaudeConversation]
}

@Observable
@MainActor
final class SessionListState {
    var sessions: [ClaudeConversation] = []
    var searchQuery = ""
    var isLoadingSessions = false

    var filteredSessions: [ClaudeConversation] {
        let query = searchQuery.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return sessions }

        return sessions.filter { session in
            let haystacks =
                [
                    session.title,
                    session.projectName,
                    session.workspaceName,
                    session.projectPath,
                    session.gitBranch ?? "",
                ] + session.previewSnippets

            return haystacks.contains { $0.localizedCaseInsensitiveContains(query) }
        }
    }

    var groupedSessions: [ConversationSection] {
        let calendar = Calendar.autoupdatingCurrent
        let now = Date()

        var today: [ClaudeConversation] = []
        var yesterday: [ClaudeConversation] = []
        var thisWeek: [ClaudeConversation] = []
        var thisMonth: [ClaudeConversation] = []
        var older: [ClaudeConversation] = []

        var seen = Set<String>()
        for session in filteredSessions where seen.insert(session.sessionId).inserted {
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

    func refreshSessions() async {
        guard !isLoadingSessions else { return }
        isLoadingSessions = true

        // Phase 1: indexed sessions (~30ms) -- immediate display
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
        let toEnrich = Array(sessions.prefix(limit).filter { $0.previewSnippets.isEmpty })
        guard !toEnrich.isEmpty else { return }

        let results = await withTaskGroup(of: (String, [String]).self) { group in
            for session in toEnrich {
                let sid = session.sessionId
                let url = URL(fileURLWithPath: session.transcriptPath)
                group.addTask { (sid, ClaudeConversationIndex.recentSnippets(from: url)) }
            }
            var dict: [String: [String]] = [:]
            for await (sid, snippets) in group where !snippets.isEmpty {
                dict[sid] = snippets
            }
            return dict
        }
        for (sid, snippets) in results {
            if let idx = sessions.firstIndex(where: { $0.sessionId == sid }) {
                sessions[idx] = sessions[idx].withSnippets(snippets)
            }
        }
    }
}
