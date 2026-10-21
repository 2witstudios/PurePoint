import Foundation

// MARK: - Line reader

nonisolated final class DaemonLineReader: @unchecked Sendable {
    private let stream: any DaemonByteStream
    private var buffer = Data()
    private var scanOffset = 0

    init(stream: any DaemonByteStream) {
        self.stream = stream
    }

    func readLine() async throws -> Data {
        while true {
            if let newlineIndex = buffer[scanOffset...].firstIndex(of: 0x0A) {
                let line = Data(buffer[scanOffset..<newlineIndex])
                scanOffset = newlineIndex + 1
                // Compact when consumed portion exceeds half the buffer
                if scanOffset > buffer.count / 2 {
                    buffer.removeSubrange(..<scanOffset)
                    scanOffset = 0
                }
                return line
            }
            let chunk = try await stream.receive()
            buffer.append(chunk)
        }
    }
}

// MARK: - Helpers

nonisolated private let hexDigits: [UInt8] = Array("0123456789abcdef".utf8)

nonisolated extension Data {
    var hexString: String {
        var chars = [UInt8]()
        chars.reserveCapacity(count * 2)
        for byte in self {
            chars.append(hexDigits[Int(byte >> 4)])
            chars.append(hexDigits[Int(byte & 0x0F)])
        }
        return String(bytes: chars, encoding: .ascii)!
    }

    init(hexString: String) {
        self.init()
        var index = hexString.startIndex
        while index < hexString.endIndex {
            let nextIndex = hexString.index(index, offsetBy: 2, limitedBy: hexString.endIndex) ?? hexString.endIndex
            if let byte = UInt8(hexString[index..<nextIndex], radix: 16) {
                append(byte)
            }
            index = nextIndex
        }
    }
}

// MARK: - Errors

nonisolated enum DaemonClientError: Error, LocalizedError {
    case eof
    case cancelled
    case notRunning
    case connectTimeout
    case busy
    case requestTimeout

    var errorDescription: String? {
        switch self {
        case .eof: "Connection to daemon closed"
        case .cancelled: "Connection cancelled"
        case .notRunning: "Daemon is not running"
        case .connectTimeout: "Timed out connecting to daemon"
        case .busy: "Daemon is at its connection limit"
        case .requestTimeout: "Timed out waiting for the daemon"
        }
    }
}
