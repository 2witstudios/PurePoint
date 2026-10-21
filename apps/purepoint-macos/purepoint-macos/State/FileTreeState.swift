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
    private var watcher: (any HostDirectoryWatcher)?
    /// Where the tree's files live. Remote trees are listed through the machine's daemon.
    private(set) var host: any ProjectHost = LocalProjectHost.shared
    var isLocal: Bool { host.isLocal }
    private var refreshGeneration = 0

    private static let hiddenNames: Set<String> = [
        ".git", ".DS_Store", ".build", ".swiftpm", "xcuserdata",
        "DerivedData", "__pycache__", ".tsbuildinfo", "node_modules",
    ]

    func load(worktreePath: String, host: any ProjectHost = LocalProjectHost.shared) {
        self.worktreePath = worktreePath
        self.host = host
        expandedPaths.removeAll()
        // A remote listing is a daemon round trip; it arrives with the first refresh.
        rootNodes = host.isLocal ? scanDirectory(atPath: worktreePath, relativeTo: worktreePath) : []

        refresh()

        watcher?.stopAll()
        watcher = host.makeWatcher { [weak self] in
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
        if host.isLocal { node.children = scanDirectory(atPath: node.absolutePath, relativeTo: root) }

        refresh()

        watcher?.watchDirectory(path: node.absolutePath)
    }

    func collapseNode(_ node: FileTreeNode) {
        expandedPaths.remove(node.absolutePath)
        node.children = []
        refresh()
        watcher?.unwatchDirectory(path: node.absolutePath)
    }

    func refresh() {
        guard let root = worktreePath else { return }
        refreshGeneration += 1
        let generation = refreshGeneration
        let expanded = expandedPaths
        Task {
            // Build and filter a replacement before publishing it. Never expose
            // an unfiltered polling scan, even briefly, in the visible tree.
            let nodes =
                host.isLocal
                ? await filteredTree(directory: root, root: root, expanded: expanded)
                : await remoteTree(directory: root, root: root, expanded: expanded)
            guard generation == refreshGeneration, worktreePath == root else { return }
            rootNodes = nodes
        }
    }

    func stopWatching() {
        refreshGeneration += 1
        watcher?.stopAll()
        watcher = nil
    }

    // MARK: - Private

    private func filteredTree(directory: String, root: String, expanded: Set<String>) async -> [FileTreeNode] {
        let nodes = scanDirectory(atPath: directory, relativeTo: root)
        let ignored = await Self.computeGitIgnored(directory: directory, worktreeRoot: root)
        let visible = filterIgnored(nodes, ignored: ignored)
        for node in visible where node.isDirectory && expanded.contains(node.absolutePath) {
            node.children = await filteredTree(directory: node.absolutePath, root: root, expanded: expanded)
        }
        return visible
    }

    /// The daemon reports each entry's `git check-ignore` verdict, so one listing per
    /// directory replaces the local scan plus check-ignore process.
    private func remoteTree(directory: String, root: String, expanded: Set<String>) async -> [FileTreeNode] {
        let host = self.host
        let entries = await HostIO.value { (try? host.list(directory: directory)) ?? [] }
        let nodes = FileTreeNode.sorted(
            entries.compactMap { entry -> FileTreeNode? in
                guard !Self.hiddenNames.contains(entry.name) else { return nil }
                if entry.ignored == true && !entry.name.hasPrefix(".") { return nil }
                let relative =
                    entry.path.hasPrefix(root + "/") ? String(entry.path.dropFirst(root.count + 1)) : entry.name
                return FileTreeNode(
                    name: entry.name, absolutePath: entry.path, relativePath: relative, isDirectory: entry.isDirectory)
            })
        for node in nodes where node.isDirectory && expanded.contains(node.absolutePath) {
            node.children = await remoteTree(directory: node.absolutePath, root: root, expanded: expanded)
        }
        return nodes
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
        #if os(macOS)
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
        #else
        // No `/usr/bin/git` on iOS, and every iPad tree is remote: `remoteTree` takes each
        // entry's verdict from the daemon's own `git check-ignore` (`fs_list`), the same
        // hardened git `git_exec` runs, in the one round trip that lists the directory.
        // (`git_exec`'s allowlist has no `check-ignore`.) A local tree shows every file.
        Set<String>()
        #endif
    }
}
