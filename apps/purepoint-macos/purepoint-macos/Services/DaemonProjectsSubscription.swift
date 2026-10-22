import Foundation

/// Follows a daemon's registered projects (protocol 13). The daemon sends the full list of
/// roots when the stream opens and again after every add or remove, so a project added or
/// removed from any device, or with `pu projects`, shows up here without polling.
/// Modeled after DaemonStatusSubscription; unlike it, it never gives up while running: a
/// restarted daemon is simply subscribed to again.
actor DaemonProjectsSubscription {
    private var connection: (any DaemonByteStream)?
    private var stopped = false
    private let client: DaemonClient

    init(client: DaemonClient) {
        self.client = client
    }

    /// Delivers each list of roots until `stop()`. Reconnects with backoff after a failure.
    func start(onChange: @escaping @MainActor ([String]) -> Void) async {
        var backoff: UInt64 = 500_000_000  // 0.5s
        let maxBackoff: UInt64 = 10_000_000_000  // 10s
        while !stopped {
            do {
                try await run(onChange: onChange)
                backoff = 500_000_000
            } catch is CancellationError {
                return
            } catch {
                #if os(iOS)
                if await AppActivity.shared.isInBackground {
                    await AppActivity.shared.untilForeground()
                    backoff = 500_000_000
                    continue
                }
                #endif
            }
            guard !stopped else { return }
            do { try await Task.sleep(nanoseconds: backoff) } catch { return }
            backoff = min(backoff * 2, maxBackoff)
        }
    }

    func stop() {
        stopped = true
        connection?.close()
        connection = nil
    }

    private func run(onChange: @escaping @MainActor ([String]) -> Void) async throws {
        let (conn, reader) = try await client.connect()
        // Stopped while connecting: nothing else would ever close this connection.
        guard !stopped else {
            conn.close()
            return
        }
        connection?.close()
        connection = conn
        try await DaemonClient.write(.subscribeProjects, to: conn)
        guard case .projectsSubscribed = DaemonClient.parse(try await reader.readLine()) else {
            throw DaemonStatusError.unexpectedResponse
        }
        while !stopped {
            if case .projectsChanged(let projects) = DaemonClient.parse(try await reader.readLine()) {
                await onChange(projects)
            }
        }
    }
}
