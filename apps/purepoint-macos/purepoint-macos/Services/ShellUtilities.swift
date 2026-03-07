import Foundation

/// Shell-escape a string by wrapping in single quotes and escaping embedded single quotes.
nonisolated func shellEscape(_ s: String) -> String {
    "'" + s.replacingOccurrences(of: "'", with: "'\\''") + "'"
}

/// Parse a single JSON line into a dictionary, returning nil on failure.
nonisolated func parseJSONLine(_ line: String) -> [String: Any]? {
    guard let data = line.data(using: .utf8),
        let object = try? JSONSerialization.jsonObject(with: data),
        let dict = object as? [String: Any]
    else { return nil }
    return dict
}
