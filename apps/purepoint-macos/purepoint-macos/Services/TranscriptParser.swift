import Foundation

enum TranscriptParser {
    static func parse(transcriptPath: String) throws -> [ChatMessage] {
        let url = URL(fileURLWithPath: transcriptPath)
        let data = try Data(contentsOf: url)
        let text = String(decoding: data, as: UTF8.self)

        guard !text.isEmpty else { return [] }

        var messages: [ChatMessage] = []

        for line in text.split(separator: "\n") {
            guard let record = parseJSONLine(String(line)) else { continue }
            guard let type = record["type"] as? String else { continue }

            switch type {
            case "user":
                if let msg = parseUserRecord(record) {
                    messages.append(msg)
                }
            case "assistant":
                if let blocks = parseAssistantBlocks(record) {
                    // Merge consecutive assistant records into one message
                    if let lastIndex = messages.indices.last,
                        messages[lastIndex].role == .assistant
                    {
                        messages[lastIndex].contentBlocks.append(contentsOf: blocks)
                    } else {
                        let timestamp = parseTimestamp(record["timestamp"] as? String)
                        messages.append(
                            ChatMessage(
                                role: .assistant,
                                timestamp: timestamp,
                                contentBlocks: blocks
                            ))
                    }
                }
            default:
                break
            }
        }

        return messages
    }

    private static func parseUserRecord(_ record: [String: Any]) -> ChatMessage? {
        guard let message = record["message"] as? [String: Any],
            let role = message["role"] as? String,
            role == "user"
        else { return nil }

        let content = message["content"]
        let timestamp = parseTimestamp(record["timestamp"] as? String)

        // User content can be a string or array of blocks
        if let text = content as? String {
            return ChatMessage(
                role: .user,
                timestamp: timestamp,
                contentBlocks: [.text(id: UUID().uuidString, text: text)]
            )
        }

        if let blocks = content as? [[String: Any]] {
            var contentBlocks: [ContentBlock] = []
            var hasToolResult = false

            for block in blocks {
                guard let blockType = block["type"] as? String else { continue }

                switch blockType {
                case "text":
                    if let text = block["text"] as? String {
                        contentBlocks.append(.text(id: UUID().uuidString, text: text))
                    }
                case "tool_result":
                    hasToolResult = true
                    let toolUseId = block["tool_use_id"] as? String ?? ""
                    let isError = block["is_error"] as? Bool ?? false
                    let output: String
                    if let text = block["content"] as? String {
                        output = text
                    } else {
                        output = ""
                    }
                    contentBlocks.append(
                        .toolResult(
                            id: UUID().uuidString,
                            toolUseId: toolUseId,
                            output: output,
                            isError: isError
                        ))
                default:
                    break
                }
            }

            // Skip user messages that only contain tool_results (they're system messages)
            if hasToolResult
                && contentBlocks.allSatisfy({ block in
                    if case .toolResult = block { return true }
                    return false
                })
            {
                // Still include as a separate message so tool results can be displayed
                return ChatMessage(
                    role: .user,
                    timestamp: timestamp,
                    contentBlocks: contentBlocks
                )
            }

            guard !contentBlocks.isEmpty else { return nil }
            return ChatMessage(
                role: .user,
                timestamp: timestamp,
                contentBlocks: contentBlocks
            )
        }

        return nil
    }

    private static func parseAssistantBlocks(_ record: [String: Any]) -> [ContentBlock]? {
        guard let message = record["message"] as? [String: Any],
            let role = message["role"] as? String,
            role == "assistant",
            let contentArray = message["content"] as? [[String: Any]]
        else { return nil }

        var contentBlocks: [ContentBlock] = []
        var blockCount = 0

        for block in contentArray {
            guard let blockType = block["type"] as? String else { continue }

            switch blockType {
            case "text":
                if let text = block["text"] as? String {
                    let split = ContentBlockSplitter.split(text, startIndex: blockCount)
                    blockCount += split.count
                    contentBlocks.append(contentsOf: split)
                }
            case "tool_use":
                let id = block["id"] as? String ?? UUID().uuidString
                let name = block["name"] as? String ?? "unknown"
                let input: String
                if let inputObj = block["input"] {
                    if let inputData = try? JSONSerialization.data(withJSONObject: inputObj),
                        let inputStr = String(data: inputData, encoding: .utf8)
                    {
                        input = inputStr
                    } else {
                        input = "{}"
                    }
                } else {
                    input = "{}"
                }
                contentBlocks.append(
                    .toolUse(
                        id: id,
                        name: name,
                        input: input,
                        status: .completed
                    ))
            default:
                break  // Skip thinking blocks etc.
            }
        }

        guard !contentBlocks.isEmpty else { return nil }
        return contentBlocks
    }

    private static let isoFormatter: ISO8601DateFormatter = {
        let formatter = ISO8601DateFormatter()
        formatter.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return formatter
    }()

    private static func parseTimestamp(_ value: String?) -> Date {
        guard let value else { return Date() }
        return isoFormatter.date(from: value) ?? Date()
    }
}
