import Foundation
import Observation

@Observable
@MainActor
final class FileTreeState {
    var rootNodes: [FileTreeNode] = []
    var selectedFilePath: String?
    var searchQuery: String = ""

    private var worktreePath: String?
    private var expandedPaths: Set<String> = []
    private var watcher: FileTreeWatcher?
    private var gitIgnoredCache: Set<String> = []

    private static let hiddenNames: Set<String> = [
        ".git", ".DS_Store", ".build", ".swiftpm", "xcuserdata",
        "DerivedData", "__pycache__", ".tsbuildinfo", "node_modules",
    ]

    func load(worktreePath: String) {
        self.worktreePath = worktreePath
        expandedPaths.removeAll()
        loadGitIgnored(worktreePath: worktreePath)
        rootNodes = scanDirectory(atPath: worktreePath, relativeTo: worktreePath)

        watcher?.stopAll()
        watcher = FileTreeWatcher { [weak self] in
            let captured = self
            Task { @MainActor in
                captured?.refresh()
            }
        }
        watcher?.watchDirectory(path: worktreePath)
    }

    func expandNode(_ node: FileTreeNode) {
        guard node.isDirectory, let root = worktreePath else { return }
        expandedPaths.insert(node.absolutePath)
        node.children = scanDirectory(atPath: node.absolutePath, relativeTo: root)
        watcher?.watchDirectory(path: node.absolutePath)
    }

    func collapseNode(_ node: FileTreeNode) {
        expandedPaths.remove(node.absolutePath)
        node.children = []
        watcher?.unwatchDirectory(path: node.absolutePath)
    }

    func refresh() {
        guard let root = worktreePath else { return }
        loadGitIgnored(worktreePath: root)
        rootNodes = scanDirectory(atPath: root, relativeTo: root)

        // Refresh expanded directories
        refreshExpanded(nodes: rootNodes, root: root)
    }

    func stopWatching() {
        watcher?.stopAll()
        watcher = nil
    }

    // MARK: - Private

    private func refreshExpanded(nodes: [FileTreeNode], root: String) {
        for node in nodes where node.isDirectory && expandedPaths.contains(node.absolutePath) {
            node.children = scanDirectory(atPath: node.absolutePath, relativeTo: root)
            refreshExpanded(nodes: node.children, root: root)
        }
    }

    private func scanDirectory(atPath path: String, relativeTo root: String) -> [FileTreeNode] {
        let fm = FileManager.default
        guard let contents = try? fm.contentsOfDirectory(atPath: path) else { return [] }

        var nodes: [FileTreeNode] = []
        for name in contents {
            if Self.hiddenNames.contains(name) || name.hasPrefix(".") { continue }

            let absPath = (path as NSString).appendingPathComponent(name)
            let relPath = String(absPath.dropFirst(root.count + 1))

            if gitIgnoredCache.contains(relPath) { continue }

            var isDir: ObjCBool = false
            guard fm.fileExists(atPath: absPath, isDirectory: &isDir) else { continue }

            nodes.append(
                FileTreeNode(
                    name: name,
                    absolutePath: absPath,
                    relativePath: relPath,
                    isDirectory: isDir.boolValue
                )
            )
        }

        return FileTreeNode.sorted(nodes)
    }

    private func loadGitIgnored(worktreePath: String) {
        // Batch check using git check-ignore
        let fm = FileManager.default
        guard let contents = try? fm.contentsOfDirectory(atPath: worktreePath) else { return }

        let paths = contents.filter { !Self.hiddenNames.contains($0) && !$0.hasPrefix(".") }
        guard !paths.isEmpty else {
            gitIgnoredCache = []
            return
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = ["-C", worktreePath, "check-ignore", "--stdin"]
        process.currentDirectoryURL = URL(fileURLWithPath: worktreePath)

        let inputPipe = Pipe()
        let outputPipe = Pipe()
        process.standardInput = inputPipe
        process.standardOutput = outputPipe
        process.standardError = FileHandle.nullDevice

        do {
            try process.run()
            let inputData = paths.joined(separator: "\n").data(using: .utf8) ?? Data()
            inputPipe.fileHandleForWriting.write(inputData)
            inputPipe.fileHandleForWriting.closeFile()
            process.waitUntilExit()

            let outputData = outputPipe.fileHandleForReading.readDataToEndOfFile()
            let output = String(data: outputData, encoding: .utf8) ?? ""
            gitIgnoredCache = Set(
                output.split(separator: "\n").map { String($0) }
            )
        } catch {
            gitIgnoredCache = []
        }
    }
}
