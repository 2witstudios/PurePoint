import Foundation

enum FileIOService {
    static func readFile(at path: String) async throws -> (content: String, isBinary: Bool) {
        try await Task.detached {
            let url = URL(fileURLWithPath: path)
            let data = try Data(contentsOf: url)

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
