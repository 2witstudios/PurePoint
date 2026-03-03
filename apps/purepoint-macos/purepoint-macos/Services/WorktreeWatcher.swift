import Foundation

/// Watches a git working tree's .git directory for changes using GCD file system events.
/// Fires onChange callback (debounced) when files are modified, enabling auto-refresh of diffs.
nonisolated final class WorktreeWatcher: @unchecked Sendable {
    private let queue = DispatchQueue(label: "com.purepoint.worktree-watcher")
    private var source: DispatchSourceFileSystemObject?
    private var fileDescriptor: Int32 = -1
    private let path: String
    private let onChange: @Sendable () -> Void
    private var debounceWork: DispatchWorkItem?
    private static let debounceInterval: TimeInterval = 0.5

    init(worktreePath: String, onChange: @escaping @Sendable () -> Void) {
        // Watch the .git directory (or .git file for linked worktrees)
        let gitPath = (worktreePath as NSString).appendingPathComponent(".git")
        self.path = gitPath
        self.onChange = onChange
        startWatching()
    }

    private func startWatching() {
        if source != nil { stopSource() }

        fileDescriptor = open(path, O_EVTONLY)
        guard fileDescriptor >= 0 else { return }

        let source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: fileDescriptor,
            eventMask: [.write, .rename, .delete, .attrib],
            queue: queue
        )

        source.setEventHandler { [weak self] in
            guard let self else { return }
            let flags = source.data
            if flags.contains(.delete) || flags.contains(.rename) {
                self.queue.asyncAfter(deadline: .now() + 0.1) { [weak self] in
                    guard let self else { return }
                    self.startWatching()
                    self.scheduleDebounce()
                }
            } else {
                self.scheduleDebounce()
            }
        }

        source.setCancelHandler { [fd = fileDescriptor] in
            close(fd)
        }

        source.resume()
        self.source = source
    }

    private func scheduleDebounce() {
        debounceWork?.cancel()
        let callback = onChange
        let work = DispatchWorkItem {
            DispatchQueue.main.async {
                callback()
            }
        }
        debounceWork = work
        queue.asyncAfter(deadline: .now() + Self.debounceInterval, execute: work)
    }

    private func stopSource() {
        source?.cancel()
        source = nil
        fileDescriptor = -1
    }

    func stop() {
        queue.sync {
            debounceWork?.cancel()
            stopSource()
        }
    }

    deinit {
        debounceWork?.cancel()
        source?.cancel()
        if fileDescriptor >= 0 {
            close(fileDescriptor)
        }
    }
}
