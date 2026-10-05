import Foundation
import Network

/// Subscribes to status events from the daemon and delivers parsed models.
/// Modeled after DaemonGridSubscription — actor isolation serializes writes.
actor DaemonStatusSubscription {
    let projectRoot: String
    private var connection: NWConnection?
    private var stopped = false
    /// Set once a subscription is established. Losing an established stream
    /// means the daemon went away; even if the socket answers again it may be a
    /// replacement that has never initialized this project or resumed its agents.
    private var subscribed = false
    /// Consecutive failures (about 3.5 s of backoff) before a daemon that never
    /// answered is treated as gone rather than briefly busy or starting.
    private static let retriesBeforeDaemonLost = 4

    init(projectRoot: String) {
        self.projectRoot = projectRoot
    }

    /// Start the subscription loop with reconnection on failure.
    ///
    /// If an established stream drops, or the daemon stays unreachable for a few
    /// attempts (it died, or exited with the app instance that launched it),
    /// `onDaemonLost` is called once and the loop ends: retrying the socket alone
    /// would never start a daemon, nor init and resume on a replacement.
    func start(
        onEvent: @escaping @MainActor ([WorktreeModel], [AgentModel]) -> Void,
        onDaemonLost: (@MainActor () -> Void)? = nil
    ) async {
        guard !stopped else { return }

        var backoff: UInt64 = 500_000_000  // 0.5s
        let maxBackoff: UInt64 = 5_000_000_000  // 5s
        var retries = 0
        let maxRetries = 20

        while !stopped {
            do {
                try await runSubscriptionLoop(onEvent: onEvent)
                break
            } catch is CancellationError {
                break
            } catch is DaemonStatusError {
                break
            } catch {
                retries += 1
                guard !stopped, retries <= maxRetries else { break }
                if subscribed || retries == Self.retriesBeforeDaemonLost, let onDaemonLost {
                    await onDaemonLost()
                    break
                }
                do {
                    try await Task.sleep(nanoseconds: backoff)
                } catch {
                    break
                }
                backoff = min(backoff * 2, maxBackoff)
            }
        }
    }

    /// Stop the subscription.
    func stop() {
        stopped = true
        connection?.cancel()
        connection = nil
    }

    // MARK: - Private

    private func runSubscriptionLoop(
        onEvent: @escaping @MainActor ([WorktreeModel], [AgentModel]) -> Void
    ) async throws {
        let client = DaemonClient()
        let (conn, reader) = try await client.connect()
        self.connection?.cancel()
        self.connection = conn

        try await DaemonClient.write(.subscribeStatus(projectRoot: projectRoot), to: conn)

        let firstLine = try await reader.readLine()
        let firstResp = DaemonClient.parse(firstLine)
        guard case .statusSubscribed = firstResp else {
            // At its connection limit the daemon is alive: retry, don't give up.
            if case .error(let code, _) = firstResp, code == "BUSY" {
                throw DaemonClientError.busy
            }
            if case .error(_, let msg) = firstResp {
                throw DaemonStatusError.subscribeFailed(msg)
            }
            throw DaemonStatusError.unexpectedResponse
        }
        subscribed = true

        while !stopped {
            let line = try await reader.readLine()
            let resp = DaemonClient.parse(line)
            if case .statusEvent(let worktrees, let agents) = resp {
                let worktreeModels = DaemonWorkspaceService.parseWorktrees(worktrees)
                let agentModels = agents.map { report in
                    AgentModel(
                        id: report.id,
                        name: report.name,
                        agentType: report.agentType,
                        status: AgentStatus(rawValue: report.status) ?? .lost,
                        prompt: report.prompt ?? "",
                        startedAt: report.startedAt ?? "",
                        sessionId: report.sessionId,
                        suspended: report.suspended
                    )
                }
                await onEvent(worktreeModels, agentModels)
            }
        }
    }
}

enum DaemonStatusError: Error, LocalizedError {
    case subscribeFailed(String)
    case unexpectedResponse

    var errorDescription: String? {
        switch self {
        case .subscribeFailed(let msg): "Status subscribe failed: \(msg)"
        case .unexpectedResponse: "Unexpected response during status subscribe"
        }
    }
}
