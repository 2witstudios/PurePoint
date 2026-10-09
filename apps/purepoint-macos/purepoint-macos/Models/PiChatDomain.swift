// Desktop adapter for the merged Pi bridge v1 contract.
import Foundation

struct PiDraftState: Equatable {
    var text: String = ""
    mutating func takeSubmission() -> String { let value = text; text = ""; return value }
    mutating func restore(_ value: String) {
        guard !value.isEmpty else { return }; text = text.isEmpty ? value : text + "\n\n" + value
    }
}
struct PiComposerAttachment: Codable, Identifiable, Sendable {
    let id: String
    let name: String
    let mimeType: String
    let data: Data
    var isImage: Bool { mimeType.hasPrefix("image/") }
}
struct PiComposerPayload {
    let message: String
    let images: [[String: String]]
    static func make(text: String, attachments: [PiComposerAttachment]) throws -> Self {
        guard attachments.count <= 4, attachments.reduce(0, { $0 + $1.data.count }) <= 512 * 1024 else {
            throw PiComposerError("Attach up to four files, totaling 512 KiB after preparation.")
        }
        var message = text
        var images: [[String: String]] = []
        for file in attachments {
            if file.isImage {
                images.append(["type": "image", "mimeType": file.mimeType, "data": file.data.base64EncodedString()])
            } else {
                guard let contents = String(data: file.data, encoding: .utf8) else {
                    throw PiComposerError("This file is not UTF-8 text.")
                }
                let name = String(
                    file.name.replacingOccurrences(of: "\n", with: " ").replacingOccurrences(of: "\r", with: " ")
                        .prefix(120))
                message += "\n\n--- Attachment: \(name) ---\n\(contents)\n--- End attachment ---"
            }
        }
        if message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !images.isEmpty {
            message = "Please review the attached images."
        }
        guard !message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, message.utf8.count <= 65536 else {
            throw PiComposerError("Message and attached text must fit within 64 KiB.")
        }
        return Self(message: message, images: images)
    }
}
struct PiComposerError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}
enum PiLocalRecoveryStore {
    private static func url(_ name: String) throws -> URL {
        let base = try FileManager.default.url(
            for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true
        ).appendingPathComponent("PurePoint.PointGuard", isDirectory: true)
        try FileManager.default.createDirectory(
            at: base, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        return base.appendingPathComponent(name + ".json")
    }
    static func load<T: Decodable>(_ name: String, as type: T.Type) -> T? {
        guard let file = try? url(name), let size = try? file.resourceValues(forKeys: [.fileSizeKey]).fileSize,
            size <= 40 * 1024 * 1024, let data = try? Data(contentsOf: file), data.count <= 40 * 1024 * 1024
        else { return nil }
        return try? JSONDecoder().decode(type, from: data)
    }
    static func save<T: Encodable>(_ value: T, name: String) throws {
        let data = try JSONEncoder().encode(value)
        let file = try url(name)
        try data.write(to: file, options: .atomic)
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: file.path)
    }
}
struct PiSnapshotCursor {
    private var epoch = ""
    private var revision = -1
    mutating func accept(epoch: String, revision: Int) -> Bool {
        guard self.epoch != epoch || revision > self.revision else { return false }
        self.epoch = epoch; self.revision = revision; return true
    }
    mutating func reset() { epoch = ""; revision = -1 }
}
enum PiMarkdownBlock: Equatable, Identifiable {
    case prose(String)
    case code(language: String, text: String)
    var id: String {
        switch self {
        case .prose(let text): return "p:" + text;
        case .code(let language, let text): return "c:" + language + text
        }
    }
}
enum PiRenderedMarkdownBlock: Sendable {
    case prose(AttributedString)
    case code(language: String, text: String)
}
enum PiMarkdownBlocks {
    static func render(_ text: String) -> [PiRenderedMarkdownBlock] {
        split(text).map { block in
            switch block {
            case .prose(let prose):
                return .prose(
                    (try? AttributedString(
                        markdown: prose, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
                        ?? AttributedString(prose))
            case .code(let language, let code): return .code(language: language, text: code)
            }
        }
    }
    static func split(_ text: String) -> [PiMarkdownBlock] {
        var result: [PiMarkdownBlock] = [], buffer: [String] = [], language: String? = nil
        func flush() {
            let text = buffer.joined(separator: "\n");
            if !text.isEmpty {
                if let language {
                    result.append(.code(language: language, text: text))
                } else {
                    result.append(.prose(text))
                }
            }; buffer = []
        }
        for line in text.components(separatedBy: "\n") {
            if line.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                flush()
                if language != nil {
                    language = nil
                } else {
                    language = String(line.trimmingCharacters(in: .whitespaces).dropFirst(3))
                }
            } else {
                buffer.append(line)
            }
        }
        flush(); return result
    }
}
enum PiConnectionAddress {
    static func url(_ input: String) -> URL? {
        guard let url = URL(string: input.trimmingCharacters(in: .whitespacesAndNewlines)),
            ["ws", "wss"].contains(url.scheme?.lowercased() ?? ""), let host = url.host?.lowercased(),
            url.user == nil, url.password == nil, url.query == nil, url.fragment == nil, url.path == "/v1"
        else { return nil }
        let labels = host.split(separator: ".", omittingEmptySubsequences: false)
        let numericIPv4 =
            labels.count == 4
            && labels.allSatisfy {
                !$0.isEmpty && $0.count <= 3 && $0.utf8.allSatisfy { (48...57).contains($0) }
                    && ($0 == "0" || !$0.hasPrefix("0"))
            }
        let parts = numericIPv4 ? labels.compactMap { Int($0) } : []
        let tailnetIPv4 =
            parts.count == 4 && parts[0] == 100 && (64...127).contains(parts[1])
            && parts.allSatisfy { (0...255).contains($0) }
        guard
            tailnetIPv4 || host == "127.0.0.1" || host == "localhost" || host == "[::1]" || host == "::1"
                || host.hasPrefix("fd7a:115c:a1e0:") || host.hasPrefix("[fd7a:115c:a1e0:") || host.hasSuffix(".ts.net")
        else { return nil }
        return url
    }
}
struct PiPairingCode: Decodable {
    let type: String
    let version: Int
    let endpoint: String
    let secret: String
    static func parse(_ text: String) -> PiPairingCode? {
        guard text.utf8.count <= 8192,
            let code = try? JSONDecoder().decode(Self.self, from: Data(text.utf8)),
            code.type == "pi-mobile-pairing", code.version == 1,
            PiConnectionAddress.url(code.endpoint) != nil,
            code.secret.count >= 32, code.secret.utf8.count <= 1024,
            !code.secret.contains("\n"), !code.secret.contains("\r")
        else { return nil }
        return code
    }
}
struct PiChatMessage: Codable, Identifiable, Equatable, Sendable {
    let id: String; let role: String; let text: String; var activity: String?; var error: String?
}
struct PiToolActivity: Codable, Identifiable, Equatable, Sendable {
    let id: String; let name: String; let state: String; let text: String
}
enum PiTranscriptRow: Identifiable, Equatable, Sendable {
    case message(PiChatMessage)
    case activity([PiToolActivity])
    var id: String {
        switch self {
        case .message(let message): return "message:" + message.id
        case .activity(let tools): return "activity:" + (tools.first?.id ?? "empty")
        }
    }
}
enum PiTranscriptRows {
    static func make(messages: [PiChatMessage], tools: [PiToolActivity]) -> [PiTranscriptRow] {
        var rows: [PiTranscriptRow] = []
        var represented = Set<String>()
        func appendTool(_ tool: PiToolActivity) {
            if let last = rows.last, case .activity(var group) = last {
                group.append(tool); rows[rows.count - 1] = .activity(group)
            } else {
                rows.append(.activity([tool]))
            }
        }
        for message in messages {
            if message.role == "toolResult" {
                // The pinned bridge keys native rows as role-timestamp-toolCallId.
                // Match the complete tool ID suffix, preserving IDs that contain hyphens.
                let live = tools.first { message.id.hasSuffix("-" + $0.id) }
                let id = live?.id ?? message.id
                guard represented.insert(id).inserted else { continue }
                let text = message.text + (message.error.map { "\n\n" + $0 } ?? "")
                appendTool(
                    PiToolActivity(
                        id: id, name: message.activity ?? live?.name ?? "Tool",
                        state: message.error == nil ? (live?.state ?? "finished") : "failed", text: text))
            } else if message.role != "assistant"
                || !message.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || message.error != nil
            {
                rows.append(.message(message))
            }
        }
        for tool in tools where !represented.contains(tool.id) { appendTool(tool) }
        return rows
    }
}
struct PiQueuedText: Codable, Sendable { let id: String; let clientId: String?; let mode: String; let text: String }
struct PiExtensionDialog: Codable, Identifiable, Sendable {
    let id: String; let method: String; var title: String?; var message: String?; var prefill: String?;
    var placeholder: String?; var options: [String]?; var optionIds: [String]? = nil
}
struct PiEditorText: Codable, Sendable { let id: String; let text: String }
struct PiCanceledText: Codable, Identifiable, Sendable {
    let clientId: String?; let id: String; let text: String; let sessionId: String
}
struct PiSnapshot: Codable, Sendable {
    let version: Int; let epoch: String; let revision: Int; let busy: Bool; var runId: String?; let sessionId: String;
    let title: String
    let messages: [PiChatMessage]; let tools: [PiToolActivity]; let queue: [PiQueuedText];
    let dialogs: [PiExtensionDialog]; let notices: [String]; var error: String?; var editor: PiEditorText? = nil;
    var canceled: [PiCanceledText]? = nil; var capabilities: [String]? = nil
}
struct PiConversation: Codable, Identifiable, Sendable { let id: String; let title: String; var date: String? }
struct PiHistory: Codable, Sendable { let sessionId: String; let title: String; let messages: [PiChatMessage] }
struct PiSubmission: Codable, Identifiable, Sendable {
    let id: String; let text: String; let sessionId: String; var status: String; var recoverable: Bool;
    var attachments: [PiComposerAttachment]? = nil
}
// Receipts are deliberately independent of transcript rows: acceptance does not imply completion.
enum PiJSONValue: Codable, Sendable {
    case object([String: PiJSONValue]), array([PiJSONValue]), string(String), number(Double), bool(Bool), null
    init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if c.decodeNil() {
            self = .null
        } else if let value = try? c.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? c.decode(String.self) {
            self = .string(value)
        } else if let value = try? c.decode(Double.self) {
            self = .number(value)
        } else if let value = try? c.decode([String: PiJSONValue].self) {
            self = .object(value)
        } else {
            self = .array(try c.decode([PiJSONValue].self))
        }
    }
    func encode(to encoder: Encoder) throws {
        var c = encoder.singleValueContainer()
        switch self {
        case .null: try c.encodeNil();
        case .object(let x): try c.encode(x);
        case .array(let x): try c.encode(x);
        case .string(let x): try c.encode(x);
        case .number(let x): try c.encode(x);
        case .bool(let x): try c.encode(x)
        }
    }
    subscript(_ key: String) -> PiJSONValue { if case .object(let x) = self { return x[key] ?? .null }; return .null }
    var text: String? { if case .string(let x) = self { return x }; return nil }
    var strings: [String] { if case .array(let x) = self { return x.compactMap(\.text) }; return [] }
    func decode<T: Decodable>(_ type: T.Type) throws -> T {
        try JSONDecoder().decode(type, from: JSONEncoder().encode(self))
    }
}
struct PiWireEnvelope: Decodable, Sendable {
    let type: String; var id: String?; var ok: Bool?; var data: PiJSONValue?; var error: String?; var text: String?
}

// A serial utility queue keeps encoding/file IO away from keyboard and rendering.
// Coalescing replaces older pending values; serial execution prevents stale writes.
final class PiCoalescedWriter: @unchecked Sendable {
    private let queue: DispatchQueue
    private let delay: TimeInterval
    private var pending: [String: @Sendable () -> Void] = [:]
    init(
        queue: DispatchQueue = DispatchQueue(label: "purepoint.pointguard.persistence", qos: .utility),
        delay: TimeInterval = 0.25
    ) {
        self.queue = queue; self.delay = delay
    }
    func schedule(key: String, work: @escaping @Sendable () -> Void) {
        queue.async {
            let alreadyScheduled = self.pending[key] != nil
            self.pending[key] = work
            if !alreadyScheduled {
                self.queue.asyncAfter(deadline: .now() + self.delay) { self.pending.removeValue(forKey: key)?() }
            }
        }
    }
    func flush(completion: @escaping @Sendable () -> Void = {}) {
        queue.async {
            let work = self.pending; self.pending.removeAll()
            for action in work.values { action() }
            completion()
        }
    }
    func writeAndConfirm(_ work: @escaping @Sendable () throws -> Void) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            queue.async {
                // Drain older coalesced values before the confirmed write. Their
                // delayed callbacks then find no stale value to overwrite it.
                let older = self.pending; self.pending.removeAll()
                for action in older.values { action() }
                do { try work(); continuation.resume() } catch { continuation.resume(throwing: error) }
            }
        }
    }
}
enum PiIncomingRecord: Sendable {
    case snapshot(PiSnapshot, [PiTranscriptRow])
    case envelope(PiWireEnvelope)
    static func decode(_ data: Data) throws -> Self {
        struct Header: Decodable { let type: String }
        let decoder = JSONDecoder()
        if try decoder.decode(Header.self, from: data).type == "snapshot" {
            let state = try decoder.decode(PiSnapshot.self, from: data)
            return .snapshot(state, PiTranscriptRows.make(messages: state.messages, tools: state.tools))
        }
        return .envelope(try decoder.decode(PiWireEnvelope.self, from: data))
    }
}
