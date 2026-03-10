import Foundation
import Testing
@testable import PurePoint

@MainActor
struct SessionListStateTests {

    private func makeSession(
        id: String, title: String, modifiedAt: Date = Date()
    ) -> Conversation {
        Conversation(
            sessionId: id, agentSource: .claude, title: title,
            previewSnippets: [], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/\(id).jsonl",
            createdAt: nil, modifiedAt: modifiedAt, messageCount: nil
        )
    }

    // MARK: - Filtering

    @Test func givenSearchQueryShouldFilterSessions() {
        let state = SessionListState()
        state.sessions = [
            makeSession(id: "s1", title: "Dashboard work"),
            makeSession(id: "s2", title: "Terminal bugs"),
        ]
        state.searchQuery = "Dashboard"

        #expect(state.filteredSessions.count == 1)
        #expect(state.filteredSessions[0].sessionId == "s1")
    }

    @Test func givenEmptySearchQueryShouldReturnAllSessions() {
        let state = SessionListState()
        state.sessions = [
            makeSession(id: "s1", title: "A"),
            makeSession(id: "s2", title: "B"),
        ]
        state.searchQuery = ""

        #expect(state.filteredSessions.count == 2)
    }

    @Test func givenSearchQueryMatchingSnippetShouldIncludeSession() {
        let state = SessionListState()
        let session = Conversation(
            sessionId: "s1", agentSource: .claude, title: "Some task",
            previewSnippets: ["fix the login bug"], projectPath: "/tmp",
            purePointProjectRoot: nil, gitBranch: nil,
            transcriptPath: "/tmp/s1.jsonl",
            createdAt: nil, modifiedAt: Date(), messageCount: nil
        )
        state.sessions = [session]
        state.searchQuery = "login"

        #expect(state.filteredSessions.count == 1)
    }

    // MARK: - Time-period grouping

    @Test func givenGroupedSessionsShouldPartitionByTimePeriod() {
        let state = SessionListState()
        let calendar = Calendar.autoupdatingCurrent
        let now = Date()

        state.sessions = [
            makeSession(id: "s-today", title: "Today work", modifiedAt: now),
            makeSession(
                id: "s-yesterday", title: "Yesterday work",
                modifiedAt: calendar.date(byAdding: .day, value: -1, to: now)!
            ),
            makeSession(
                id: "s-week", title: "This week work",
                modifiedAt: calendar.date(byAdding: .day, value: -4, to: now)!
            ),
            makeSession(
                id: "s-old", title: "Old work",
                modifiedAt: calendar.date(byAdding: .day, value: -60, to: now)!
            ),
        ]

        let sections = state.groupedSessions
        #expect(sections.count == 4)
        #expect(sections[0].id == "today")
        #expect(sections[0].sessions.count == 1)
        #expect(sections[1].id == "yesterday")
        #expect(sections[1].sessions.count == 1)
        #expect(sections[2].id == "this-week")
        #expect(sections[2].sessions.count == 1)
        #expect(sections[3].id == "older")
        #expect(sections[3].sessions.count == 1)
    }

    // MARK: - ConversationSection.grouped (pure function)

    @Test func givenConversationSectionGroupedShouldDeduplicateById() {
        let now = Date()
        let session = makeSession(id: "s1", title: "Dup", modifiedAt: now)
        let sections = ConversationSection.grouped(from: [session, session])
        let totalCount = sections.reduce(0) { $0 + $1.sessions.count }
        #expect(totalCount == 1)
    }

    @Test func givenEmptyInputGroupedShouldReturnEmptySections() {
        let sections = ConversationSection.grouped(from: [])
        #expect(sections.isEmpty)
    }
}
