import Foundation

/// Flat, bounded listing of a directory's files for palette search.
/// Skips the same noise directories as the file tree so results match what the tree shows.
nonisolated enum FileIndex {
    static let defaultLimit = 5000

    private static let skippedNames: Set<String> = [
        ".git", ".DS_Store", ".build", ".swiftpm", "xcuserdata",
        "DerivedData", "__pycache__", ".tsbuildinfo", "node_modules", "target",
    ]

    static func list(root: String, limit: Int = defaultLimit) -> [PaletteFileEntry] {
        let rootURL = URL(fileURLWithPath: root, isDirectory: true)
        guard
            let enumerator = FileManager.default.enumerator(
                at: rootURL, includingPropertiesForKeys: [.isDirectoryKey],
                options: [.skipsPackageDescendants])
        else { return [] }

        let rootPath = rootURL.standardizedFileURL.path
        var entries: [PaletteFileEntry] = []
        for case let url as URL in enumerator {
            if skippedNames.contains(url.lastPathComponent) {
                enumerator.skipDescendants()
                continue
            }
            let isDirectory = (try? url.resourceValues(forKeys: [.isDirectoryKey]).isDirectory) ?? false
            if isDirectory { continue }
            let path = url.standardizedFileURL.path
            let relative = path.hasPrefix(rootPath + "/") ? String(path.dropFirst(rootPath.count + 1)) : path
            entries.append(PaletteFileEntry(relativePath: relative, absolutePath: path))
            if entries.count >= limit { break }
        }
        return entries
    }
}
