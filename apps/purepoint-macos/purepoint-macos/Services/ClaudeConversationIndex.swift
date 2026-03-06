import Foundation

enum ClaudeConversationIndex {
    static func loadSessions(baseURL: URL = defaultBaseURL, limit: Int? = nil) throws -> [ClaudeConversation] {
        let fileManager = FileManager.default
        guard fileManager.fileExists(atPath: baseURL.path) else { return [] }

        let directories = try fileManager.contentsOfDirectory(
            at: baseURL,
            includingPropertiesForKeys: [.isDirectoryKey],
            options: [.skipsHiddenFiles]
        )
        .filter { url in
            (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) == true
        }

        var conversations: [ClaudeConversation] = []
        var seenSessionIds = Set<String>()

        for directory in directories {
            let indexed = try loadIndexedSessions(in: directory)
            for session in indexed where seenSessionIds.insert(session.sessionId).inserted {
                conversations.append(session)
            }
        }

        for directory in directories {
            let transcripts = try transcriptFiles(in: directory)
            for transcript in transcripts {
                let sessionId = transcript.deletingPathExtension().lastPathComponent
                guard seenSessionIds.insert(sessionId).inserted else { continue }
                if let session = try loadLooseSession(from: transcript) {
                    conversations.append(session)
                }
            }
        }

        conversations.sort { $0.modifiedAt > $1.modifiedAt }
        if let limit {
            return Array(conversations.prefix(limit))
        }
        return conversations
    }

    static var defaultBaseURL: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".claude", isDirectory: true)
            .appendingPathComponent("projects", isDirectory: true)
    }

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

    private static func loadIndexedSessions(in directory: URL) throws -> [ClaudeConversation] {
        let indexURL = directory.appendingPathComponent("sessions-index.json")
        guard FileManager.default.fileExists(atPath: indexURL.path) else { return [] }

        let data = try Data(contentsOf: indexURL)
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
                fallback: URL(fileURLWithPath: projectPath).lastPathComponent
            )

            return ClaudeConversation(
                sessionId: entry.sessionId,
                title: title,
                previewSnippets: recentSnippets(from: transcriptURL),
                projectPath: projectPath,
                purePointProjectRoot: locatePurePointProjectRoot(startingAt: projectPath),
                gitBranch: entry.gitBranch?.trimmedNonEmpty,
                transcriptPath: transcriptURL.path,
                createdAt: parseTimestamp(entry.created),
                modifiedAt: modifiedAt,
                messageCount: entry.messageCount
            )
        }
    }

    private static func loadLooseSession(from transcriptURL: URL) throws -> ClaudeConversation? {
        let metadata = transcriptMetadata(from: transcriptURL)
        let sessionId = metadata.sessionId?.trimmedNonEmpty ?? transcriptURL.deletingPathExtension().lastPathComponent
        let projectPath = metadata.projectPath?.trimmedNonEmpty ?? transcriptURL.deletingLastPathComponent().path

        let modifiedAt =
            resourceDate(for: transcriptURL, key: .contentModificationDateKey)
            ?? Date.distantPast

        let title = sessionTitle(
            summary: nil,
            firstPrompt: metadata.firstPrompt,
            fallback: URL(fileURLWithPath: projectPath).lastPathComponent
        )

        return ClaudeConversation(
            sessionId: sessionId,
            title: title,
            previewSnippets: recentSnippets(from: transcriptURL),
            projectPath: projectPath,
            purePointProjectRoot: locatePurePointProjectRoot(startingAt: projectPath),
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

    private static func transcriptMetadata(from transcriptURL: URL) -> TranscriptMetadata {
        let head = (try? readPrefix(from: transcriptURL, byteCount: 24 * 1024)) ?? ""
        var metadata = TranscriptMetadata()

        for line in head.split(separator: "\n") {
            guard let record = parseJSONLine(String(line)) else { continue }
            updateMetadata(&metadata, with: record)
            if metadata.projectPath != nil,
               metadata.sessionId != nil,
               metadata.firstPrompt != nil,
               metadata.createdAt != nil {
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
           let parsed = parseTimestamp(timestamp) {
            metadata.createdAt = parsed
        }
        if metadata.firstPrompt == nil,
           let type = record["type"] as? String,
           type == "user",
           let snippet = messageSnippet(from: record, maxLength: 280) {
            metadata.firstPrompt = snippet
        }
    }

    private static func recentSnippets(from transcriptURL: URL, limit: Int = 3) -> [String] {
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
        let collapsed = text
            .components(separatedBy: .whitespacesAndNewlines)
            .filter { !$0.isEmpty }
            .joined(separator: " ")
            .trimmedNonEmpty

        guard let collapsed else { return nil }
        if collapsed.count <= maxLength { return collapsed }
        let index = collapsed.index(collapsed.startIndex, offsetBy: maxLength - 1)
        return String(collapsed[..<index]).trimmingCharacters(in: .whitespaces) + "..."
    }

    private static func sessionTitle(summary: String?, firstPrompt: String?, fallback: String) -> String {
        if let summary = summary?.trimmedNonEmpty, summary.lowercased() != "no prompt" {
            return summary
        }

        guard let prompt = firstPrompt?.trimmedNonEmpty else {
            return fallback
        }

        let strippedPrompt = prompt.replacingOccurrences(
            of: #"(?i)^implement the following plan:\s*"#,
            with: "",
            options: .regularExpression
        )

        let lines = strippedPrompt
            .split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }

        let candidateLine = lines.first ?? strippedPrompt
        let cleanedLine = candidateLine.replacingOccurrences(
            of: #"^#+\s*"#,
            with: "",
            options: .regularExpression
        )

        return compact(cleanedLine, maxLength: 72) ?? fallback
    }

    private static func locatePurePointProjectRoot(startingAt path: String) -> String? {
        var currentURL = URL(fileURLWithPath: path).standardizedFileURL
        let fileManager = FileManager.default

        while true {
            let manifestPath = currentURL
                .appendingPathComponent(".pu", isDirectory: true)
                .appendingPathComponent("manifest.json")
                .path

            if fileManager.fileExists(atPath: manifestPath) {
                return currentURL.path
            }

            let parent = currentURL.deletingLastPathComponent()
            if parent.path == currentURL.path {
                return nil
            }
            currentURL = parent
        }
    }

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
