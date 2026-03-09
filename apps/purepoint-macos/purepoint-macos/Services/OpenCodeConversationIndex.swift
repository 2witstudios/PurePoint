import Foundation

nonisolated enum OpenCodeConversationIndex {

    // MARK: - Public API

    static var defaultStoragePath: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".local/share/opencode/storage/session", isDirectory: true)
    }

    /// Read all session JSON files from ~/.local/share/opencode/storage/session/*/
    /// Format: {id, title, directory, time:{created,updated}, slug, projectID}
    static func loadSessions(baseURL: URL = defaultStoragePath) throws -> [Conversation] {
        let fileManager = FileManager.default
        guard fileManager.fileExists(atPath: baseURL.path) else { return [] }

        var conversations: [Conversation] = []
        var projectRootCache: [String: String?] = [:]

        // Walk subdirectories (project hashes), read each .json file
        guard
            let topLevel = try? fileManager.contentsOfDirectory(
                at: baseURL,
                includingPropertiesForKeys: [.isDirectoryKey],
                options: [.skipsHiddenFiles]
            )
        else { return [] }

        for dir in topLevel {
            let isDir = (try? dir.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) ?? false
            let targets: [URL]

            if isDir {
                targets =
                    (try? fileManager.contentsOfDirectory(
                        at: dir,
                        includingPropertiesForKeys: nil,
                        options: [.skipsHiddenFiles]
                    ).filter { $0.pathExtension == "json" }) ?? []
            } else if dir.pathExtension == "json" {
                targets = [dir]
            } else {
                continue
            }

            for jsonURL in targets {
                guard let session = try? parseSessionFile(jsonURL, projectRootCache: &projectRootCache) else {
                    continue
                }
                conversations.append(session)
            }
        }

        return conversations
    }

    // MARK: - Private

    private static func parseSessionFile(
        _ url: URL,
        projectRootCache: inout [String: String?]
    ) throws -> Conversation? {
        let data = try Data(contentsOf: url)
        guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { return nil }

        guard let id = json["id"] as? String else { return nil }

        let title = json["title"] as? String ?? "OpenCode session"
        let directory = json["directory"] as? String

        let timeDict = json["time"] as? [String: Any]
        let updatedStr = timeDict?["updated"] as? String
        let createdStr = timeDict?["created"] as? String

        // OpenCode timestamps can be ISO 8601 or Unix seconds
        let modifiedAt = parseFlexibleTimestamp(updatedStr) ?? Date.distantPast
        let createdAt = parseFlexibleTimestamp(createdStr)

        let purePointRoot: String? =
            if let directory {
                ClaudeConversationIndex.locatePurePointProjectRoot(
                    startingAt: directory, cache: &projectRootCache
                )
            } else {
                nil
            }

        return Conversation(
            sessionId: id,
            agentSource: .opencode,
            title: title,
            previewSnippets: [],
            projectPath: directory,
            purePointProjectRoot: purePointRoot,
            gitBranch: nil,
            transcriptPath: nil,
            createdAt: createdAt,
            modifiedAt: modifiedAt,
            messageCount: nil
        )
    }

    private static func parseFlexibleTimestamp(_ value: String?) -> Date? {
        guard let value, !value.isEmpty else { return nil }
        // Try ISO 8601 first
        if let date = ClaudeConversationIndex.parseTimestamp(value) { return date }
        // Try Unix seconds (integer or decimal)
        if let seconds = Double(value), seconds > 1_000_000_000 {
            return Date(timeIntervalSince1970: seconds)
        }
        return nil
    }
}
