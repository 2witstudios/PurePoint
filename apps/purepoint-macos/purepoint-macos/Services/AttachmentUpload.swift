#if os(macOS)
import AppKit
#endif
import Foundation

/// Sends a file to an agent's daemon so the agent can read it by path, even when
/// it runs on another machine. The daemon keeps it in `~/.pu/attachments` there.
nonisolated enum AttachmentUpload {
    /// Raw bytes per `attach_file` request: base64 keeps each request well under
    /// the daemon's 1 MB message limit.
    static let chunkSize = 512 * 1024
    /// Matches the daemon's own limit.
    static let maxBytes = 64 * 1024 * 1024

    enum UploadError: LocalizedError {
        case tooLarge(name: String)
        case notAFile(name: String)
        case daemon(code: String, message: String)
        case unexpectedResponse

        var errorDescription: String? {
            switch self {
            case .tooLarge(let name): "\(name) is larger than 64 MB"
            case .notAFile(let name): "\(name) is not a regular file"
            case .daemon(_, let message): message
            case .unexpectedResponse: "The agent's daemon did not accept the file"
            }
        }
    }

    /// The `attach_file` requests that carry `data`, in order.
    static func requests(for data: Data, name: String, uploadId: String) -> [DaemonRequest] {
        guard !data.isEmpty else {
            return [.attachFile(uploadId: uploadId, name: name, offset: 0, data: Data(), done: true)]
        }
        return stride(from: 0, to: data.count, by: chunkSize).map { offset in
            let end = min(offset + chunkSize, data.count)
            return .attachFile(
                uploadId: uploadId, name: name, offset: offset,
                data: data.subdata(in: data.startIndex + offset..<data.startIndex + end),
                done: end == data.count)
        }
    }

    /// Uploads `data` and returns its path on the daemon's machine.
    static func upload(_ data: Data, name: String, via client: DaemonClient) async throws -> String {
        guard data.count <= maxBytes else { throw UploadError.tooLarge(name: name) }
        let uploadId = UUID().uuidString.lowercased()
        for request in requests(for: data, name: name, uploadId: uploadId) {
            switch try await client.send(request) {
            case .attachFileResult(_, let path?):
                return path
            case .attachFileResult(_, nil):
                continue
            case .error(let code, let message):
                throw UploadError.daemon(code: code, message: message)
            default:
                throw UploadError.unexpectedResponse
            }
        }
        throw UploadError.unexpectedResponse
    }

    /// Reads a dropped file for upload. Checks it is a regular file within
    /// `maxBytes` before reading, so a folder or an oversized file is rejected
    /// without loading it. Blocking: call it off the main thread.
    static func readFile(at url: URL) throws -> Data {
        let name = url.lastPathComponent
        let values = try url.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
        guard values.isRegularFile == true else { throw UploadError.notAFile(name: name) }
        guard (values.fileSize ?? 0) <= maxBytes else { throw UploadError.tooLarge(name: name) }
        return try Data(contentsOf: url, options: .mappedIfSafe)
    }

    #if os(macOS)
    /// Whether a key event's modifiers are Command alone. Caps Lock, Fn and
    /// other non-shortcut flags are ignored.
    static func isCommandOnly(_ flags: NSEvent.ModifierFlags) -> Bool {
        flags.intersection([.command, .shift, .option, .control]) == .command
    }

    /// A pasted image as PNG, when the pasteboard holds an image and no text
    /// (a screenshot copied to the clipboard, or "Copy Image").
    static func pastedImage(from pasteboard: NSPasteboard) -> Data? {
        guard pasteboard.string(forType: .string) == nil else { return nil }
        if let png = pasteboard.data(forType: .png) { return png }
        guard let image = NSImage(pasteboard: pasteboard),
            let tiff = image.tiffRepresentation,
            let bitmap = NSBitmapImageRep(data: tiff)
        else { return nil }
        return bitmap.representation(using: .png, properties: [:])
    }
    #endif

    /// A readable, unique-enough name for a pasted image.
    static func pastedImageName(at date: Date = Date()) -> String {
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd 'at' HH.mm.ss"
        return "Pasted Image \(formatter.string(from: date)).png"
    }
}
