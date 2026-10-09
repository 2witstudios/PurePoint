import Foundation

nonisolated enum FilePreviewError: LocalizedError {
    case tooLarge, unsupported

    var errorDescription: String? {
        switch self {
        case .tooLarge: "File is too large to preview (over 1 MB)."
        case .unsupported: "This file cannot be previewed."
        }
    }
}

enum FileIOService {
    static func readFile(at path: String, limit: Int? = nil) async throws -> (content: String, isBinary: Bool) {
        try await Task.detached {
            let url = URL(fileURLWithPath: path)
            let data: Data
            if let limit {
                let attributes = try FileManager.default.attributesOfItem(atPath: path)
                guard attributes[.type] as? FileAttributeType == .typeRegular else {
                    throw FilePreviewError.unsupported
                }
                let handle = try FileHandle(forReadingFrom: url)
                defer { try? handle.close() }
                data = try handle.read(upToCount: limit + 1) ?? Data()
                guard data.count <= limit else { throw FilePreviewError.tooLarge }
            } else {
                data = try Data(contentsOf: url)
            }

            // Binary detection: scan first 8KB for null bytes
            let scanLength = min(data.count, 8192)
            let prefix = data.prefix(scanLength)
            if prefix.contains(0x00) {
                return (content: "", isBinary: true)
            }

            guard let content = String(data: data, encoding: .utf8) else {
                return (content: "", isBinary: true)
            }
            return (content: content, isBinary: false)
        }.value
    }

    static func writeFile(content: String, to path: String) async throws {
        try await Task.detached {
            let url = URL(fileURLWithPath: path)
            try content.write(to: url, atomically: true, encoding: .utf8)
        }.value
    }

    static func fileModificationDate(at path: String) -> Date? {
        try? FileManager.default.attributesOfItem(atPath: path)[.modificationDate] as? Date
    }
}
