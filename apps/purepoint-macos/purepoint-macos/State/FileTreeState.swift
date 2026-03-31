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

    private static let hiddenNames: Set<String> = [
        ".git", ".DS_Store", ".build", ".swiftpm", "xcuserdata",
        "DerivedData", "__pycache__", ".tsbuildinfo", "node_modules",
    ]

    func load(worktreePath: String) {
        self.worktreePath = worktreePath
        expandedPaths.removeAll()
        rootNodes = scanDirectory(atPath: worktreePath, relativeTo: worktreePath)

        // Async gitignore filter — rescan after result
        Task {
            let ignored = await Self.computeGitIgnored(
                directory: worktreePath,
                worktreeRoot: worktreePath
            )
            if !ignored.isEmpty {
                self.rootNodes = self.filterIgnored(
                    self.rootNodes,
                    ignored: ignored
                )
            }
        }

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

        Task {
            let ignored = await Self.computeGitIgnored(
                directory: node.absolutePath,
                worktreeRoot: root
            )
            if !ignored.isEmpty {
                node.children = self.filterIgnored(node.children, ignored: ignored)
            }
        }

        watcher?.watchDirectory(path: node.absolutePath)
    }

    func collapseNode(_ node: FileTreeNode) {
        expandedPaths.remove(node.absolutePath)
        node.children = []
        watcher?.unwatchDirectory(path: node.absolutePath)
    }

    func refresh() {
        guard let root = worktreePath else { return }
        rootNodes = scanDirectory(atPath: root, relativeTo: root)
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

    private func scanDirectory(
        atPath path: String,
        relativeTo root: String
    ) -> [FileTreeNode] {
        let fm = FileManager.default
        guard let contents = try? fm.contentsOfDirectory(atPath: path) else { return [] }

        var nodes: [FileTreeNode] = []
        for name in contents {
            if Self.hiddenNames.contains(name) { continue }

            let absPath = (path as NSString).appendingPathComponent(name)
            let relPath = String(absPath.dropFirst(root.count + 1))

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

    private func filterIgnored(
        _ nodes: [FileTreeNode],
        ignored: Set<String>
    ) -> [FileTreeNode] {
        nodes.filter { !ignored.contains($0.relativePath) }
    }

    // MARK: - Git Ignore (off main actor)

    private nonisolated static func computeGitIgnored(
        directory: String,
        worktreeRoot: String
    ) async -> Set<String> {
        await Task.detached(priority: .utility) {
            let fm = FileManager.default
            guard let contents = try? fm.contentsOfDirectory(atPath: directory) else {
                return Set<String>()
            }

            let relativePaths = contents.compactMap { name -> String? in
                guard !name.hasPrefix(".") else { return nil }
                let absPath = (directory as NSString).appendingPathComponent(name)
                return String(absPath.dropFirst(worktreeRoot.count + 1))
            }

            guard !relativePaths.isEmpty else { return Set<String>() }

            let process = Process()
            process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
            process.arguments = ["-C", worktreeRoot, "check-ignore", "--stdin"]
            process.currentDirectoryURL = URL(fileURLWithPath: worktreeRoot)

            let inputPipe = Pipe()
            let outputPipe = Pipe()
            process.standardInput = inputPipe
            process.standardOutput = outputPipe
            process.standardError = FileHandle.nullDevice

            do {
                try process.run()
                let inputData =
                    relativePaths.joined(separator: "\n")
                    .data(using: .utf8) ?? Data()
                inputPipe.fileHandleForWriting.write(inputData)
                inputPipe.fileHandleForWriting.closeFile()
                process.waitUntilExit()

                let outputData = outputPipe.fileHandleForReading.readDataToEndOfFile()
                let output = String(data: outputData, encoding: .utf8) ?? ""
                return Set(output.split(separator: "\n").map { String($0) })
            } catch {
                return Set<String>()
            }
        }.value
    }
}
