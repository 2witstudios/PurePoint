import Foundation

/// Metadata notifications supplement DiffState's foreground recursive-file polling.
nonisolated final class WorktreeWatcher: @unchecked Sendable {
    private let queue = DispatchQueue(label: "com.purepoint.worktree-watcher")
    private var sources: [DispatchSourceFileSystemObject] = []
    private var debounceWork: DispatchWorkItem?
    private var stopped = false
    private let root: String
    private let onChange: @Sendable () -> Void

    init(worktreePath: String, onChange: @escaping @Sendable () -> Void) {
        root = worktreePath; self.onChange = onChange
        queue.sync { install() }
    }

    /// Resolve linked worktree .git and commondir files without assuming .git is a directory.
    static func metadataPaths(worktreePath: String) -> [String] {
        let marker = URL(fileURLWithPath: worktreePath).appendingPathComponent(".git")
        var git = marker
        if let text = try? String(contentsOf: marker, encoding: .utf8), text.hasPrefix("gitdir:") {
            let value = text.dropFirst(7).trimmingCharacters(in: .whitespacesAndNewlines)
            git = URL(fileURLWithPath: value, relativeTo: marker.deletingLastPathComponent()).standardizedFileURL
        }
        var common = git
        if let text = try? String(contentsOf: git.appendingPathComponent("commondir"), encoding: .utf8) {
            common = URL(fileURLWithPath: text.trimmingCharacters(in: .whitespacesAndNewlines), relativeTo: git).standardizedFileURL
        }
        return Array(Set([marker.path, git.path, common.path, git.appendingPathComponent("HEAD").path,
                          git.appendingPathComponent("index").path, common.appendingPathComponent("packed-refs").path,
                          common.appendingPathComponent("refs").path, common.appendingPathComponent("refs/heads").path,
                          common.appendingPathComponent("refs/remotes").path]))
    }

    private func install() {
        sources.forEach { $0.cancel() }; sources = []
        guard !stopped else { return }
        for path in Self.metadataPaths(worktreePath: root) {
            let fd = open(path, O_EVTONLY)
            guard fd >= 0 else { continue }
            let source = DispatchSource.makeFileSystemObjectSource(fileDescriptor: fd, eventMask: [.write, .rename, .delete, .attrib], queue: queue)
            source.setEventHandler { [weak self] in self?.schedule() }
            source.setCancelHandler { close(fd) }
            sources.append(source); source.resume()
        }
    }

    private func schedule() {
        guard !stopped else { return }
        debounceWork?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self, !self.stopped else { return }
            self.install() // Reopen atomically replaced HEAD/index/refs.
            self.onChange()
        }
        debounceWork = work
        queue.asyncAfter(deadline: .now() + 0.5, execute: work)
    }

    func stop() {
        queue.sync {
            stopped = true; debounceWork?.cancel(); debounceWork = nil
            sources.forEach { $0.cancel() }; sources = []
        }
    }
    deinit { debounceWork?.cancel(); sources.forEach { $0.cancel() } }
}
