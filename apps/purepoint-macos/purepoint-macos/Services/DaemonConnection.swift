import Foundation
import Network

// MARK: - Line reader

nonisolated final class DaemonLineReader: @unchecked Sendable {
    private let connection: NWConnection
    private var buffer = Data()
    private var scanOffset = 0

    init(connection: NWConnection) {
        self.connection = connection
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
            let chunk = try await readChunk()
            buffer.append(chunk)
        }
    }

    private func readChunk() async throws -> Data {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Data, Error>) in
            connection.receive(minimumIncompleteLength: 1, maximumLength: 65536) { data, _, _, error in
                if let error {
                    cont.resume(throwing: error)
                } else if let data, !data.isEmpty {
                    cont.resume(returning: data)
                } else {
                    cont.resume(throwing: DaemonClientError.eof)
                }
            }
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
