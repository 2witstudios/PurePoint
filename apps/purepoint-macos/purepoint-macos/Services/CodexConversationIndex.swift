import Foundation

nonisolated enum CodexConversationIndex {

    // MARK: - Public API

    static var defaultBaseURL: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".codex", isDirectory: true)
    }

    /// Phase 1: Parse ~/.codex/session_index.jsonl (one JSON object per line).
    /// Each line: {id, thread_name, updated_at}
    static func loadIndexedSessions(baseURL: URL = defaultBaseURL) throws -> [Conversation] {
        let indexURL = baseURL.appendingPathComponent("session_index.jsonl")
        guard FileManager.default.fileExists(atPath: indexURL.path) else { return [] }

        let data = try Data(contentsOf: indexURL)
        let text = String(decoding: data, as: UTF8.self)

        var conversations: [Conversation] = []
        var seenIds = Set<String>()

        for line in text.split(separator: "\n") {
            guard let record = parseJSONLine(String(line)) else { continue }
            guard let id = record["id"] as? String,
                seenIds.insert(id).inserted
            else { continue }

            let threadName = record["thread_name"] as? String
            let updatedAt = ClaudeConversationIndex.parseTimestamp(record["updated_at"] as? String) ?? Date.distantPast

            conversations.append(
                Conversation(
                    sessionId: id,
                    agentSource: .codex,
                    title: threadName ?? "Codex session",
                    previewSnippets: [],
                    projectPath: nil,
                    purePointProjectRoot: nil,
                    gitBranch: nil,
                    transcriptPath: nil,
                    createdAt: nil,
                    modifiedAt: updatedAt,
                    messageCount: nil
                ))
        }

        return conversations
    }

    /// Phase 2: For the top N sessions, locate the session JSONL file and read the first line
    /// (session_meta) to extract cwd and git info.
    static func enrichWithMetadata(
        _ sessions: [Conversation],
        baseURL: URL = defaultBaseURL,
        limit: Int = 50
    ) throws -> [Conversation] {
        let filePaths = try sessionFilePaths(baseURL: baseURL)
        var projectRootCache: [String: String?] = [:]
        var result: [Conversation] = []

        for session in sessions.prefix(limit) {
            guard let fileURL = filePaths[session.sessionId],
                let head = try? readPrefix(from: fileURL, byteCount: 8 * 1024)
            else {
                result.append(session)
                continue
            }

            // Parse first line for session metadata
            let firstLine: String
            if let newlineIdx = head.firstIndex(of: "\n") {
                firstLine = String(head[head.startIndex..<newlineIdx])
            } else {
                firstLine = head
            }

            guard let record = parseJSONLine(firstLine) else {
                result.append(session)
                continue
            }

            let payload = record["payload"] as? [String: Any]
            let cwd = payload?["cwd"] as? String ?? record["cwd"] as? String
            let gitInfo = payload?["git"] as? [String: Any]
            let branch = gitInfo?["branch"] as? String

            let purePointRoot: String? =
                if let cwd {
                    ClaudeConversationIndex.locatePurePointProjectRoot(
                        startingAt: cwd, cache: &projectRootCache
                    )
                } else {
                    nil
                }

            result.append(
                Conversation(
                    sessionId: session.sessionId,
                    agentSource: .codex,
                    title: session.title,
                    previewSnippets: session.previewSnippets,
                    projectPath: cwd,
                    purePointProjectRoot: purePointRoot,
                    gitBranch: branch,
                    transcriptPath: fileURL.path,
                    createdAt: session.createdAt,
                    modifiedAt: session.modifiedAt,
                    messageCount: session.messageCount
                ))
        }

        result.append(contentsOf: sessions.dropFirst(limit))
        return result
    }

    // MARK: - Private

    /// Build a UUID -> file URL map by listing session directories.
    /// Session file path: ~/.codex/sessions/{YYYY}/{MM}/{DD}/rollout-*-{uuid}.jsonl
    private static func sessionFilePaths(baseURL: URL) throws -> [String: URL] {
        let sessionsDir = baseURL.appendingPathComponent("sessions", isDirectory: true)
        let fileManager = FileManager.default
        guard fileManager.fileExists(atPath: sessionsDir.path) else { return [:] }

        var map: [String: URL] = [:]

        // Walk year/month/day directories
        guard
            let enumerator = fileManager.enumerator(
                at: sessionsDir,
                includingPropertiesForKeys: nil,
                options: [.skipsHiddenFiles]
            )
        else { return [:] }

        for case let fileURL as URL in enumerator where fileURL.pathExtension == "jsonl" {
            // Extract UUID from filename: rollout-*-{uuid}.jsonl
            let filename = fileURL.deletingPathExtension().lastPathComponent
            if let lastDash = filename.lastIndex(of: "-") {
                let uuid = String(filename[filename.index(after: lastDash)...])
                if uuid.count >= 8 {  // sanity check
                    map[uuid] = fileURL
                }
            }
        }

        return map
    }

    private static func readPrefix(from url: URL, byteCount: Int) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        let data = try handle.read(upToCount: byteCount) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }
}
