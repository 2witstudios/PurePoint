import Foundation

/// Watches `~/.pu/machines.json` for changes using GCD file system events.
///
/// The CLI (`pu machine add`/`remove`) and other app instances rewrite the registry with
/// an exclusive lock and atomic rename, so the watched file is replaced rather than
/// modified in place — this re-opens the new file and debounces the burst of events a
/// rename produces. All mutable state is accessed on a dedicated serial queue.
final class MachineRegistryWatcher: @unchecked Sendable {
    private let queue = DispatchQueue(label: "com.purepoint.machine-registry-watcher")
    private var source: DispatchSourceFileSystemObject?
    private var fileDescriptor: Int32 = -1
    private let path: String
    private let onChange: @MainActor @Sendable () -> Void
    private var debounceWork: DispatchWorkItem?
    private var retryTimer: DispatchSourceTimer?
    private static let debounceInterval: TimeInterval = 0.1
    private static let retryInterval: TimeInterval = 1.0

    var isWatching: Bool { queue.sync { source != nil } }

    init(
        path: String = MachinePaths.registryPath,
        onChange: @escaping @MainActor @Sendable () -> Void
    ) {
        self.path = path
        self.onChange = onChange
        startWatching()
        if source == nil { startRetryTimer() }
    }

    /// Re-attempt watching if the file didn't exist at creation time.
    func retry() {
        queue.async { [self] in
            guard source == nil else { return }
            startWatching()
            if source != nil { stopRetryTimer() }
        }
    }

    /// Poll for the file's appearance when it doesn't exist yet.
    private func startRetryTimer() {
        let timer = DispatchSource.makeTimerSource(queue: queue)
        timer.schedule(deadline: .now() + Self.retryInterval, repeating: Self.retryInterval)
        timer.setEventHandler { [weak self] in
            self?.retry()
        }
        timer.resume()
        retryTimer = timer
    }

    /// Must be called on `queue`.
    private func stopRetryTimer() {
        retryTimer?.cancel()
        retryTimer = nil
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
                self.reopenAfterReplace(attempt: 1)
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

    /// Re-open the file watcher after an atomic replace, retrying if the file hasn't appeared yet.
    /// Must be called on `queue`.
    private func reopenAfterReplace(attempt: Int) {
        let maxAttempts = 3
        queue.asyncAfter(deadline: .now() + 0.01) { [weak self] in
            guard let self else { return }
            self.startWatching()
            if self.source != nil {
                self.scheduleDebounce()
            } else if attempt < maxAttempts {
                self.reopenAfterReplace(attempt: attempt + 1)
            }
        }
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
            stopRetryTimer()
            stopSource()
        }
    }

    deinit {
        debounceWork?.cancel()
        retryTimer?.cancel()
        if source != nil {
            // Cancel handler will close the fd — do not also close it here.
            source?.cancel()
        } else if fileDescriptor >= 0 {
            close(fileDescriptor)
        }
    }
}
