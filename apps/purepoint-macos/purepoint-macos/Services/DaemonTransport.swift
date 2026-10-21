import Foundation

// MARK: - Byte streams

/// One duplex connection to a daemon. `DaemonClient` frames newline-delimited JSON over it;
/// the stream itself only moves bytes. On macOS it is a Unix socket; elsewhere it can be any
/// relay that carries the same bytes (an SSH exec channel running `pu daemon proxy`).
nonisolated protocol DaemonByteStream: AnyObject, Sendable {
    /// Write all of `data`, completing once the transport has accepted it.
    func send(_ data: Data) async throws
    /// The next non-empty chunk of bytes. Throws `DaemonClientError.eof` when the peer closes
    /// the stream, and the transport's own error when the stream fails or is closed locally.
    func receive() async throws -> Data
    /// Close the stream. Safe to call more than once and from any thread; a pending `receive`
    /// or `send` completes with an error.
    func close()
}

/// A connection for synchronous callers that already run off the main thread
/// (`DaemonClient.sendBlocking`). Every call blocks for at most the timeout it was opened with.
nonisolated protocol DaemonBlockingByteStream: AnyObject {
    /// Write all of `data`. Throws `DaemonClientError.requestTimeout` when the timeout elapses
    /// and `DaemonClientError.eof` when the stream is gone.
    func write(_ data: Data) throws
    /// The next non-empty chunk. Throws `DaemonClientError.eof` when the peer closes the
    /// stream and `DaemonClientError.requestTimeout` when the timeout elapses.
    func read() throws -> Data
    func close()
}

// MARK: - Transport

/// How `DaemonClient` reaches a daemon. Each `openStream` is one daemon connection, which is
/// what the daemon's protocol expects: one per request, plus one per long-lived stream.
nonisolated protocol DaemonTransport: Sendable {
    /// Open a connected stream.
    ///
    /// When `cancellable` is true, cancelling the calling task aborts a pending connect with
    /// `DaemonClientError.cancelled`. When it is false the connect runs until it succeeds, fails
    /// or times out regardless of task cancellation, which is what the long-lived streams'
    /// reconnect loops rely on.
    func openStream(cancellable: Bool) async throws -> any DaemonByteStream

    /// Open a stream for `DaemonClient.sendBlocking`. Throws `DaemonClientError.notRunning`
    /// when no daemon can be reached.
    func openBlockingStream(timeout: TimeInterval) throws -> any DaemonBlockingByteStream
}

extension DaemonTransport {
    /// A stream for long-lived subscriptions: the connect ignores task cancellation.
    nonisolated func openStream() async throws -> any DaemonByteStream {
        try await openStream(cancellable: false)
    }

    /// Transports without a native blocking path bridge the async stream with semaphores.
    /// Callers are already off the main thread, so blocking one of them is acceptable.
    nonisolated func openBlockingStream(timeout: TimeInterval) throws -> any DaemonBlockingByteStream {
        let stream = try BlockingBridge.wait(timeout: timeout, onTimeout: DaemonClientError.connectTimeout) {
            try await self.openStream(cancellable: true)
        } discard: { $0.close() }
        return BridgedBlockingByteStream(stream: stream, timeout: timeout)
    }
}

// MARK: - Blocking bridge

/// A blocking view of an async `DaemonByteStream`. A call that outlives the timeout closes the
/// stream, so the abandoned async operation completes with an error instead of lingering.
nonisolated final class BridgedBlockingByteStream: DaemonBlockingByteStream {
    private let stream: any DaemonByteStream
    private let timeout: TimeInterval

    init(stream: any DaemonByteStream, timeout: TimeInterval) {
        self.stream = stream
        self.timeout = timeout
    }

    func write(_ data: Data) throws {
        let stream = self.stream
        do {
            try BlockingBridge.wait(timeout: timeout, onTimeout: DaemonClientError.requestTimeout) {
                try await stream.send(data)
            } discard: { _ in }
        } catch DaemonClientError.requestTimeout {
            stream.close()
            throw DaemonClientError.requestTimeout
        }
    }

    func read() throws -> Data {
        let stream = self.stream
        do {
            return try BlockingBridge.wait(timeout: timeout, onTimeout: DaemonClientError.requestTimeout) {
                try await stream.receive()
            } discard: { _ in }
        } catch DaemonClientError.requestTimeout {
            stream.close()
            throw DaemonClientError.requestTimeout
        }
    }

    func close() { stream.close() }
}

nonisolated enum BlockingBridge {
    /// Run `operation` on the cooperative pool and block the calling thread for its result.
    /// After `timeout` the operation's task is cancelled and `onTimeout` is thrown; a value it
    /// still produces later is handed to `discard`.
    static func wait<T: Sendable>(
        timeout: TimeInterval,
        onTimeout: Error,
        operation: @escaping @Sendable () async throws -> T,
        discard: @escaping @Sendable (T) -> Void
    ) throws -> T {
        let box = ResultBox<T>()
        let semaphore = DispatchSemaphore(value: 0)
        let task = Task {
            let result: Result<T, Error>
            do { result = .success(try await operation()) } catch { result = .failure(error) }
            if !box.deliver(result), case .success(let value) = result { discard(value) }
            semaphore.signal()
        }
        let seconds = max(1, timeout.rounded(.up))
        if semaphore.wait(timeout: .now() + seconds) == .timedOut {
            if box.abandon() {
                task.cancel()
                throw onTimeout
            }
            // The result landed between the timeout and abandon(); use it.
        }
        return try box.take()
    }

    nonisolated private final class ResultBox<T>: @unchecked Sendable {
        private let lock = NSLock()
        private var result: Result<T, Error>?
        private var abandoned = false

        /// False when the waiter already gave up.
        func deliver(_ value: Result<T, Error>) -> Bool {
            lock.withLock {
                guard !abandoned else { return false }
                result = value
                return true
            }
        }

        /// False when a result already arrived.
        func abandon() -> Bool {
            lock.withLock {
                guard result == nil else { return false }
                abandoned = true
                return true
            }
        }

        func take() throws -> T {
            try lock.withLock { result! }.get()
        }
    }
}
