import Foundation

struct DraftState: Equatable {
    var text: String = ""
    mutating func takeSubmission() -> String { let value = text; text = ""; return value }
    mutating func restore(_ value: String) { guard !value.isEmpty else { return }; text = text.isEmpty ? value : text + "\n\n" + value }
}
struct SnapshotCursor {
    private var epoch = ""
    private var revision = -1
    mutating func accept(epoch: String, revision: Int) -> Bool {
        guard self.epoch != epoch || revision > self.revision else { return false }
        self.epoch = epoch; self.revision = revision; return true
    }
    mutating func reset() { epoch = ""; revision = -1 }
}
enum MarkdownBlock: Equatable, Identifiable {
    case prose(String)
    case code(language: String, text: String)
    var id: String { switch self { case .prose(let text): return "p:" + text; case .code(let language, let text): return "c:" + language + text } }
}
enum MarkdownBlocks {
    static func split(_ text: String) -> [MarkdownBlock] {
        var result: [MarkdownBlock] = [], buffer: [String] = [], language: String? = nil
        func flush() { let text = buffer.joined(separator: "\n"); if !text.isEmpty { if let language { result.append(.code(language: language, text: text)) } else { result.append(.prose(text)) } }; buffer = [] }
        for line in text.components(separatedBy: "\n") {
            if line.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                flush()
                if language != nil { language = nil } else { language = String(line.trimmingCharacters(in: .whitespaces).dropFirst(3)) }
            } else { buffer.append(line) }
        }
        flush(); return result
    }
}
enum ConnectionAddress {
    static func url(_ input: String) -> URL? {
        guard let url = URL(string: input.trimmingCharacters(in: .whitespacesAndNewlines)),
              ["ws", "wss"].contains(url.scheme?.lowercased() ?? ""), let host = url.host?.lowercased(),
              url.user == nil, url.password == nil, url.query == nil, url.fragment == nil, url.path == "/v1" else { return nil }
        let parts = host.split(separator: ".").compactMap { Int($0) }
        let tailnetIPv4 = parts.count == 4 && parts[0] == 100 && (64...127).contains(parts[1]) && parts.allSatisfy { (0...255).contains($0) }
        guard tailnetIPv4 || host == "127.0.0.1" || host == "localhost" || host == "[::1]" || host == "::1" || host.hasPrefix("fd7a:115c:a1e0:") || host.hasPrefix("[fd7a:115c:a1e0:") || host.hasSuffix(".ts.net") else { return nil }
        return url
    }
}
struct ChatMessage: Codable, Identifiable { let id: String; let role: String; let text: String; var activity: String?; var error: String? }
struct ToolActivity: Codable, Identifiable { let id: String; let name: String; let state: String; let text: String }
struct QueuedText: Codable { let mode: String; let text: String }
struct ExtensionDialog: Codable, Identifiable {
    let id: String; let method: String; var title: String?; var message: String?; var prefill: String?; var placeholder: String?; var options: [String]?
}
struct EditorText: Codable { let id: String; let text: String }
struct CanceledText: Codable, Identifiable { let id: String; let text: String; let sessionId: String }
struct Snapshot: Codable {
    let version: Int; let epoch: String; let revision: Int; let busy: Bool; var runId: String?; let sessionId: String; let title: String
    let messages: [ChatMessage]; let tools: [ToolActivity]; let queue: [QueuedText]; let dialogs: [ExtensionDialog]; let notices: [String]; var error: String?; var editor: EditorText? = nil; var canceled: [CanceledText]? = nil
}
struct Conversation: Codable, Identifiable { let id: String; let title: String; var date: String? }
struct History: Codable { let sessionId: String; let title: String; let messages: [ChatMessage] }
struct Submission: Codable, Identifiable {
    let id: String; let text: String; let sessionId: String; var status: String; var recoverable: Bool
}
// Receipts are deliberately independent of transcript rows: acceptance does not imply completion.
enum JSONValue: Codable {
    case object([String: JSONValue]), array([JSONValue]), string(String), number(Double), bool(Bool), null
    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() { self = .null }
        else if let value = try? c.decode(Bool.self) { self = .bool(value) }
        else if let value = try? c.decode(String.self) { self = .string(value) }
        else if let value = try? c.decode(Double.self) { self = .number(value) }
        else if let value = try? c.decode([String: JSONValue].self) { self = .object(value) }
        else { self = .array(try c.decode([JSONValue].self)) }
    }
    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self { case .null: try c.encodeNil(); case .object(let x): try c.encode(x); case .array(let x): try c.encode(x); case .string(let x): try c.encode(x); case .number(let x): try c.encode(x); case .bool(let x): try c.encode(x) }
    }
    subscript(_ key: String) -> JSONValue { if case .object(let x) = self { return x[key] ?? .null }; return .null }
    var text: String? { if case .string(let x) = self { return x }; return nil }
    var strings: [String] { if case .array(let x) = self { return x.compactMap(\.text) }; return [] }
    func decode<T: Decodable>(_ type: T.Type) throws -> T { try JSONDecoder().decode(type, from: JSONEncoder().encode(self)) }
}
struct WireEnvelope: Decodable { let type: String; var id: String?; var ok: Bool?; var data: JSONValue?; var error: String?; var text: String? }
