import Foundation

/// Watches multiple directories for file system changes using GCD DispatchSource.
/// Follows the WorktreeWatcher pattern with debounced callbacks.
nonisolated final class FileTreeWatcher: @unchecked Sendable {
    private let queue = DispatchQueue(label: "com.purepoint.file-tree-watcher")
    private var sources: [String: (source: DispatchSourceFileSystemObject, fd: Int32)] = [:]
    private var debounceWork: DispatchWorkItem?
    private let onChange: @Sendable () -> Void
    private static let debounceInterval: TimeInterval = 0.5

    init(onChange: @escaping @Sendable () -> Void) {
        self.onChange = onChange
    }

    func watchDirectory(path: String) {
        queue.async { [weak self] in
            guard let self, self.sources[path] == nil else { return }

            let fd = open(path, O_EVTONLY)
            guard fd >= 0 else { return }

            let source = DispatchSource.makeFileSystemObjectSource(
                fileDescriptor: fd,
                eventMask: [.write, .rename, .delete, .attrib],
                queue: self.queue
            )

            source.setEventHandler { [weak self] in
                guard let self else { return }
                let flags = source.data
                if flags.contains(.delete) || flags.contains(.rename) {
                    self.queue.asyncAfter(deadline: .now() + 0.1) { [weak self] in
                        guard let self else { return }
                        self.sources.removeValue(forKey: path)
                        close(fd)
                        self.watchDirectory(path: path)
                        self.scheduleDebounce()
                    }
                } else {
                    self.scheduleDebounce()
                }
            }

            source.setCancelHandler {
                close(fd)
            }

            source.resume()
            self.sources[path] = (source: source, fd: fd)
        }
    }

    func unwatchDirectory(path: String) {
        queue.async { [weak self] in
            guard let self, let entry = self.sources.removeValue(forKey: path) else { return }
            entry.source.cancel()
        }
    }

    func stopAll() {
        queue.sync { [weak self] in
            guard let self else { return }
            self.debounceWork?.cancel()
            for (_, entry) in self.sources {
                entry.source.cancel()
            }
            self.sources.removeAll()
        }
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

    deinit {
        debounceWork?.cancel()
        for (_, entry) in sources {
            entry.source.cancel()
        }
    }
}
