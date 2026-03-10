import Foundation

enum WorktreeNameNormalizer {
    /// Normalizes a user-provided name into a valid worktree branch slug.
    /// Lowercases, keeps ASCII alphanumerics, converts whitespace/underscores to hyphens,
    /// collapses consecutive hyphens, and trims leading/trailing hyphens.
    static func normalize(_ input: String) -> String {
        let lowered = input.lowercased()
        var result = ""
        for ch in lowered {
            if ch.isASCII && (ch.isLetter || ch.isNumber) {
                result.append(ch)
            } else if ch.isWhitespace || ch == "_" {
                result.append("-")
            }
        }
        // Collapse consecutive hyphens
        while result.contains("--") {
            result = result.replacingOccurrences(of: "--", with: "-")
        }
        // Trim leading/trailing hyphens
        return result.trimmingCharacters(in: CharacterSet(charactersIn: "-"))
    }
}
