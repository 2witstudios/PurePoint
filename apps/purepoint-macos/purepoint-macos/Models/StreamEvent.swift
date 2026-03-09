import Foundation

nonisolated enum StreamContentBlock: Sendable {
    case text(String)
    case toolUse(id: String, name: String, input: String)
}

nonisolated enum StreamEvent: Sendable {
    case assistant(content: [StreamContentBlock])
    case contentBlockStart(index: Int, id: String, name: String)
    case contentBlockDelta(index: Int, delta: String)
    case toolResult(toolUseId: String, content: String, isError: Bool)
    case result(sessionId: String, durationMs: Int?)
    case error(message: String)
    case unknown

    static func parse(_ jsonLine: String) -> StreamEvent? {
        guard let data = jsonLine.data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data),
            let dict = object as? [String: Any],
            let type = dict["type"] as? String
        else {
            return nil
        }

        switch type {
        case "assistant":
            return parseAssistant(dict)
        case "content_block_delta":
            return parseContentBlockDelta(dict)
        case "stream_event":
            return parseStreamEvent(dict)
        case "tool_result":
            return parseToolResult(dict)
        case "result":
            return parseResult(dict)
        default:
            return .unknown
        }
    }

    private static func parseAssistant(_ dict: [String: Any]) -> StreamEvent {
        guard let message = dict["message"] as? [String: Any],
            let contentArray = message["content"] as? [[String: Any]]
        else {
            return .assistant(content: [])
        }

        let blocks: [StreamContentBlock] = contentArray.compactMap { block in
            guard let blockType = block["type"] as? String else { return nil }
            switch blockType {
            case "text":
                guard let text = block["text"] as? String else { return nil }
                return .text(text)
            case "tool_use":
                guard let id = block["id"] as? String,
                    let name = block["name"] as? String
                else { return nil }
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
                return .toolUse(id: id, name: name, input: input)
            default:
                return nil
            }
        }

        return .assistant(content: blocks)
    }

    private static func parseStreamEvent(_ dict: [String: Any]) -> StreamEvent {
        guard let event = dict["event"] as? [String: Any],
            let eventType = event["type"] as? String
        else { return .unknown }

        switch eventType {
        case "content_block_delta":
            return parseContentBlockDelta(event)
        case "content_block_start":
            guard let contentBlock = event["content_block"] as? [String: Any],
                let blockType = contentBlock["type"] as? String,
                blockType == "tool_use",
                let id = contentBlock["id"] as? String,
                let name = contentBlock["name"] as? String
            else { return .unknown }
            let index = event["index"] as? Int ?? 0
            return .contentBlockStart(index: index, id: id, name: name)
        default:
            return .unknown
        }
    }

    private static func parseContentBlockDelta(_ dict: [String: Any]) -> StreamEvent {
        let index = dict["index"] as? Int ?? 0
        guard let delta = dict["delta"] as? [String: Any],
            let text = delta["text"] as? String
        else { return .unknown }
        return .contentBlockDelta(index: index, delta: text)
    }

    private static func parseToolResult(_ dict: [String: Any]) -> StreamEvent {
        let toolUseId = dict["tool_use_id"] as? String ?? ""
        let content = dict["content"] as? String ?? ""
        let isError = dict["is_error"] as? Bool ?? false
        return .toolResult(toolUseId: toolUseId, content: content, isError: isError)
    }

    private static func parseResult(_ dict: [String: Any]) -> StreamEvent {
        let sessionId = dict["session_id"] as? String ?? ""
        let durationMs = dict["duration_ms"] as? Int
        return .result(sessionId: sessionId, durationMs: durationMs)
    }
}
