import Foundation

enum ClaudeConversationIndex {

    // MARK: - Public API

    static var defaultBaseURL: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".claude", isDirectory: true)
            .appendingPathComponent("projects", isDirectory: true)
    }

    /// Convenience: loads all sessions (indexed + loose), without snippets.
    static func loadSessions(baseURL: URL = defaultBaseURL, limit: Int? = nil) throws -> [ClaudeConversation] {
        var conversations = try loadIndexedSessions(baseURL: baseURL)
        let excludedIds = Set(conversations.map(\.sessionId))
        let loose = try loadLooseSessions(baseURL: baseURL, excluding: excludedIds)
        conversations.append(contentsOf: loose)
        conversations.sort { $0.modifiedAt > $1.modifiedAt }
        if let limit {
            return Array(conversations.prefix(limit))
        }
        return conversations
    }

    /// Phase 1: fast — reads only sessions-index.json files (no JSONL I/O).
    static func loadIndexedSessions(baseURL: URL = defaultBaseURL) throws -> [ClaudeConversation] {
        let directories = try projectDirectories(baseURL: baseURL)
        var conversations: [ClaudeConversation] = []
        var seenSessionIds = Set<String>()
        var projectRootCache: [String: String?] = [:]

        for directory in directories {
            let indexed = try loadIndexedSessions(in: directory, projectRootCache: &projectRootCache)
            for session in indexed where seenSessionIds.insert(session.sessionId).inserted {
                conversations.append(session)
            }
        }

        return conversations
    }

    /// Phase 2: slower — scans .jsonl files, skipping already-indexed session IDs.
    static func loadLooseSessions(baseURL: URL = defaultBaseURL, excluding excludedIds: Set<String> = []) throws
        -> [ClaudeConversation]
    {
        let directories = try projectDirectories(baseURL: baseURL)
        var conversations: [ClaudeConversation] = []
        var seenSessionIds = excludedIds
        var projectRootCache: [String: String?] = [:]

        for directory in directories {
            let transcripts = try transcriptFiles(in: directory)
            for transcript in transcripts {
                let sessionId = transcript.deletingPathExtension().lastPathComponent
                guard seenSessionIds.insert(sessionId).inserted else { continue }
                if let session = try loadLooseSession(from: transcript, projectRootCache: &projectRootCache) {
                    conversations.append(session)
                }
            }
        }

        return conversations
    }

    /// Load recent snippets from a transcript file (for lazy background enrichment).
    static func recentSnippets(from transcriptURL: URL, limit: Int = 3) -> [String] {
        guard let tail = try? readSuffix(from: transcriptURL, byteCount: 64 * 1024) else { return [] }

        var snippets: [String] = []
        for line in tail.split(separator: "\n").reversed() {
            guard let record = parseJSONLine(String(line)),
                let snippet = messageSnippet(from: record, maxLength: 180),
                !snippets.contains(snippet)
            else {
                continue
            }

            snippets.append(snippet)
            if snippets.count == limit {
                break
            }
        }

        return snippets.reversed()
    }

    // MARK: - Private Types

    private struct SessionIndexFile: Decodable {
        let entries: [SessionIndexEntry]
    }

    private struct SessionIndexEntry: Decodable {
        let sessionId: String
        let fullPath: String
        let fileMtime: Int64?
        let firstPrompt: String?
        let summary: String?
        let messageCount: Int?
        let created: String?
        let modified: String?
        let gitBranch: String?
        let projectPath: String?
    }

    private struct TranscriptMetadata {
        var sessionId: String?
        var projectPath: String?
        var gitBranch: String?
        var firstPrompt: String?
        var createdAt: Date?
    }

    private static let isoFormatterWithFractionalSeconds: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()

    private static let isoFormatter: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime]
        return formatter
    }()

    // MARK: - Directory Discovery

    private static func projectDirectories(baseURL: URL) throws -> [URL] {
        let fileManager = FileManager.default
        guard fileManager.fileExists(atPath: baseURL.path) else { return [] }

        return try fileManager.contentsOfDirectory(
            at: baseURL,
            includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles]
        )
        .filter { url in
            (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true
        }
        .filter { url in
            let name = url.lastPathComponent
            return !name.contains("private-var-folders") && !name.contains("private-tmp")
        }
    }

    // MARK: - Indexed Sessions

    private static func loadIndexedSessions(
        in directory: URL,
        projectRootCache: inout [String: String?]
    ) throws -> [ClaudeConversation] {
        let indexURL = directory.appendingPathComponent("sessions-index.json")

        let data: Data
        do { data = try Data(contentsOf: indexURL) } catch { return [] }
        let index = try JSONDecoder().decode(SessionIndexFile.self, from: data)

        return index.entries.compactMap { entry in
            let transcriptURL = URL(fileURLWithPath: entry.fullPath)
            guard FileManager.default.fileExists(atPath: transcriptURL.path) else { return nil }

            let modifiedAt =
                parseTimestamp(entry.modified)
                ?? dateFromUnixMilliseconds(entry.fileMtime)
                ?? resourceDate(for: transcriptURL, key: .contentModificationDateKey)
                ?? Date.distantPast

            let projectPath = entry.projectPath?.trimmedNonEmpty ?? directory.path
            let title = sessionTitle(
                summary: entry.summary,
                firstPrompt: entry.firstPrompt,
                gitBranch: entry.gitBranch,
                fallback: URL(fileURLWithPath: projectPath).lastPathComponent
            )

            return ClaudeConversation(
                sessionId: entry.sessionId,
                title: title,
                previewSnippets: [],
                projectPath: projectPath,
                purePointProjectRoot: locatePurePointProjectRoot(
                    startingAt: projectPath, cache: &projectRootCache
                ),
                gitBranch: entry.gitBranch?.trimmedNonEmpty,
                transcriptPath: transcriptURL.path,
                createdAt: parseTimestamp(entry.created),
                modifiedAt: modifiedAt,
                messageCount: entry.messageCount
            )
        }
    }

    // MARK: - Loose Sessions

    private static func loadLooseSession(
        from transcriptURL: URL,
        projectRootCache: inout [String: String?]
    ) throws -> ClaudeConversation? {
        let metadata = transcriptMetadata(from: transcriptURL)
        let sessionId = metadata.sessionId?.trimmedNonEmpty ?? transcriptURL.deletingPathExtension().lastPathComponent
        let projectPath = metadata.projectPath?.trimmedNonEmpty ?? transcriptURL.deletingLastPathComponent().path

        let modifiedAt =
            resourceDate(for: transcriptURL, key: .contentModificationDateKey)
            ?? Date.distantPast

        let title = sessionTitle(
            summary: nil,
            firstPrompt: metadata.firstPrompt,
            gitBranch: metadata.gitBranch,
            fallback: URL(fileURLWithPath: projectPath).lastPathComponent
        )

        return ClaudeConversation(
            sessionId: sessionId,
            title: title,
            previewSnippets: [],
            projectPath: projectPath,
            purePointProjectRoot: locatePurePointProjectRoot(
                startingAt: projectPath, cache: &projectRootCache
            ),
            gitBranch: metadata.gitBranch?.trimmedNonEmpty,
            transcriptPath: transcriptURL.path,
            createdAt: metadata.createdAt,
            modifiedAt: modifiedAt,
            messageCount: nil
        )
    }

    private static func transcriptFiles(in directory: URL) throws -> [URL] {
        let fileManager = FileManager.default
        let files = try fileManager.contentsOfDirectory(
            at: directory,
            includingPropertiesForKeys: [.contentModificationDateKey],
            options: [.skipsHiddenFiles]
        )
        .filter { $0.pathExtension == "jsonl" }

        return files.sorted {
            let lhsDate = resourceDate(for: $0, key: .contentModificationDateKey) ?? .distantPast
            let rhsDate = resourceDate(for: $1, key: .contentModificationDateKey) ?? .distantPast
            return lhsDate > rhsDate
        }
    }

    // MARK: - Transcript Parsing

    private static func transcriptMetadata(from transcriptURL: URL) -> TranscriptMetadata {
        let head = (try? readPrefix(from: transcriptURL, byteCount: 24 * 1024)) ?? ""
        var metadata = TranscriptMetadata()

        for line in head.split(separator: "\n") {
            guard let record = parseJSONLine(String(line)) else { continue }
            updateMetadata(&metadata, with: record)
            if metadata.projectPath != nil,
                metadata.sessionId != nil,
                metadata.firstPrompt != nil,
                metadata.createdAt != nil
            {
                break
            }
        }

        if metadata.projectPath == nil || metadata.gitBranch == nil || metadata.sessionId == nil {
            let tail = (try? readSuffix(from: transcriptURL, byteCount: 32 * 1024)) ?? ""
            for line in tail.split(separator: "\n") {
                guard let record = parseJSONLine(String(line)) else { continue }
                updateMetadata(&metadata, with: record)
            }
        }

        return metadata
    }

    private static func updateMetadata(_ metadata: inout TranscriptMetadata, with record: [String: Any]) {
        if metadata.sessionId == nil, let sessionId = record["sessionId"] as? String {
            metadata.sessionId = sessionId
        }
        if metadata.projectPath == nil, let cwd = record["cwd"] as? String {
            metadata.projectPath = cwd
        }
        if metadata.gitBranch == nil, let branch = record["gitBranch"] as? String {
            metadata.gitBranch = branch
        }
        if metadata.createdAt == nil,
            let timestamp = record["timestamp"] as? String,
            let parsed = parseTimestamp(timestamp)
        {
            metadata.createdAt = parsed
        }
        if metadata.firstPrompt == nil,
            let type = record["type"] as? String,
            type == "user",
            let snippet = messageSnippet(from: record, maxLength: 280)
        {
            metadata.firstPrompt = snippet
        }
    }

    // MARK: - Text Extraction

    private static func messageSnippet(from record: [String: Any], maxLength: Int) -> String? {
        guard let type = record["type"] as? String, type == "user" || type == "assistant" else {
            return nil
        }
        guard let message = record["message"] as? [String: Any] else { return nil }
        return normalizedText(from: message["content"], maxLength: maxLength)
    }

    private static func normalizedText(from content: Any?, maxLength: Int) -> String? {
        guard let content else { return nil }

        if let text = content as? String {
            return compact(text, maxLength: maxLength)
        }

        if let parts = content as? [Any] {
            let extracted = parts.compactMap { part -> String? in
                guard let part = part as? [String: Any],
                    let type = part["type"] as? String
                else {
                    return nil
                }

                switch type {
                case "text":
                    return part["text"] as? String
                case "tool_result":
                    if let text = part["content"] as? String {
                        return text
                    }
                    return nil
                default:
                    return nil
                }
            }
            return compact(extracted.joined(separator: " "), maxLength: maxLength)
        }

        if let part = content as? [String: Any] {
            return compact(part["text"] as? String, maxLength: maxLength)
        }

        return nil
    }

    private static func compact(_ text: String?, maxLength: Int) -> String? {
        guard let text else { return nil }
        let collapsed =
            text
            .components(separatedBy: .whitespacesAndNewlines)
            .filter { !$0.isEmpty }
            .joined(separator: " ")
            .trimmedNonEmpty

        guard let collapsed else { return nil }
        if collapsed.count <= maxLength { return collapsed }
        let index = collapsed.index(collapsed.startIndex, offsetBy: maxLength - 1)
        return String(collapsed[..<index]).trimmingCharacters(in: .whitespaces) + "..."
    }

    // MARK: - Title Resolution

    private static func sessionTitle(
        summary: String?,
        firstPrompt: String?,
        gitBranch: String?,
        fallback: String
    ) -> String {
        // Use summary if available and not a known-bad pattern
        if let summary = summary?.trimmedNonEmpty,
            summary.lowercased() != "no prompt",
            !summary.lowercased().contains("invalid api key"),
            !summary.lowercased().hasPrefix("error:")
        {
            return summary
        }

        // Try first prompt with cleanup
        if let prompt = firstPrompt?.trimmedNonEmpty {
            let strippedPrompt = prompt.replacingOccurrences(
                of: #"(?i)^implement the following plan:\s*"#,
                with: "",
                options: .regularExpression
            )

            // Strip low-information filler prefixes
            let cleanedPrompt = strippedPrompt.replacingOccurrences(
                of: #"(?i)^(can you |could you |please |help me |i need to |i want to )"#,
                with: "",
                options: .regularExpression
            )

            let lines =
                cleanedPrompt
                .split(whereSeparator: \.isNewline)
                .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                .filter { !$0.isEmpty }

            let candidateLine = lines.first ?? cleanedPrompt
            let cleanedLine = candidateLine.replacingOccurrences(
                of: #"^#+\s*"#,
                with: "",
                options: .regularExpression
            )

            if let title = compact(cleanedLine, maxLength: 72) {
                return title
            }
        }

        // Use git branch as a descriptive fallback
        if let branch = gitBranch?.trimmedNonEmpty {
            let stripped = branch.replacingOccurrences(
                of: #"^(pu|feature|fix|bugfix|hotfix)/"#,
                with: "",
                options: .regularExpression
            )
            let words =
                stripped
                .replacingOccurrences(of: "-", with: " ")
                .replacingOccurrences(of: "_", with: " ")
                .split(separator: " ")
                .map { $0.prefix(1).uppercased() + $0.dropFirst().lowercased() }
                .joined(separator: " ")
            if !words.isEmpty {
                return words
            }
        }

        return fallback
    }

    // MARK: - Project Root Resolution (Cached)

    private static func locatePurePointProjectRoot(
        startingAt path: String,
        cache: inout [String: String?]
    ) -> String? {
        if let cached = cache[path] { return cached }

        var currentURL = URL(fileURLWithPath: path).standardizedFileURL
        let fileManager = FileManager.default

        while true {
            let manifestPath =
                currentURL
                .appendingPathComponent(".pu", isDirectory: true)
                .appendingPathComponent("manifest.json")
                .path

            if fileManager.fileExists(atPath: manifestPath) {
                let result = currentURL.path
                cache[path] = result
                return result
            }

            let parent = currentURL.deletingLastPathComponent()
            if parent.path == currentURL.path {
                cache[path] = nil
                return nil
            }
            currentURL = parent
        }
    }

    // MARK: - File I/O

    private static func readPrefix(from url: URL, byteCount: Int) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        let data = try handle.read(upToCount: byteCount) ?? Data()
        return String(decoding: data, as: UTF8.self)
    }

    private static func readSuffix(from url: URL, byteCount: Int) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }

        let fileSize = try handle.seekToEnd()
        let offset = fileSize > UInt64(byteCount) ? fileSize - UInt64(byteCount) : 0
        try handle.seek(toOffset: offset)
        let data = try handle.readToEnd() ?? Data()
        return String(decoding: data, as: UTF8.self)
    }

    private static func resourceDate(for url: URL, key: URLResourceKey) -> Date? {
        try? url.resourceValues(forKeys: [key]).allValues[key] as? Date
    }

    private static func parseTimestamp(_ value: String?) -> Date? {
        guard let value = value?.trimmedNonEmpty else { return nil }
        return isoFormatterWithFractionalSeconds.date(from: value)
            ?? isoFormatter.date(from: value)
    }

    private static func dateFromUnixMilliseconds(_ value: Int64?) -> Date? {
        guard let value else { return nil }
        return Date(timeIntervalSince1970: TimeInterval(value) / 1000)
    }
}

private extension String {
    var trimmedNonEmpty: String? {
        let trimmed = trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}
