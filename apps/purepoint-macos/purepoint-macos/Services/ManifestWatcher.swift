import Foundation

/// Watches a manifest.json file for changes using GCD file system events.
/// Handles atomic writes (rename/delete → re-open) and debounces rapid changes.
/// All mutable state is accessed on a dedicated serial queue to prevent data races.
final class ManifestWatcher: @unchecked Sendable {
    private let queue = DispatchQueue(label: "com.purepoint.manifest-watcher")
    private var source: DispatchSourceFileSystemObject?
    private var fileDescriptor: Int32 = -1
    private let path: String
    private let onChange: @MainActor @Sendable () -> Void
    private var debounceWork: DispatchWorkItem?
    private static let debounceInterval: TimeInterval = 0.3

    var isWatching: Bool { source != nil }

    init(path: String, onChange: @escaping @MainActor @Sendable () -> Void) {
        self.path = path
        self.onChange = onChange
        startWatching()
    }

    /// Re-attempt watching if the file didn't exist at creation time.
    func retry() {
        queue.async { [self] in
            guard source == nil else { return }
            startWatching()
        }
    }

    /// Must be called on `queue` (or from init, which is single-threaded).
    private func startWatching() {
        if source != nil { stopSource() }

        fileDescriptor = open(path, O_EVTONLY)
        guard fileDescriptor >= 0 else { return }

        let source = DispatchSource.makeFileSystemObjectSource(
            fileDescriptor: fileDescriptor,
            eventMask: [.write, .rename, .delete],
            queue: queue
        )

        source.setEventHandler { [weak self] in
            guard let self else { return }
            let flags = source.data
            if flags.contains(.delete) || flags.contains(.rename) {
                // File was replaced (atomic write) — re-open after brief delay
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

    /// Must be called on `queue`.
    private func scheduleDebounce() {
        debounceWork?.cancel()
        let callback = onChange
        let work = DispatchWorkItem {
            Task { @MainActor in
                callback()
            }
        }
        debounceWork = work
        queue.asyncAfter(deadline: .now() + Self.debounceInterval, execute: work)
    }

    /// Must be called on `queue`.
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
