#if os(macOS)
import AppKit
#endif
import UserNotifications

/// Local delivery only. It never sends input, resolves requests or owns agent sessions.
@MainActor final class AgentSignalNotifications: NSObject, UNUserNotificationCenterDelegate {
    static let shared = AgentSignalNotifications()
    private var pendingTaps = AgentSignalPendingTaps()
    private var drainingTaps = false
    private let center = UNUserNotificationCenter.current()
    private let defaults: UserDefaults
    private var acknowledgedErrors: [String] = []
    private var policy = AgentSignalAlertPolicy()
    private var knownNotificationKeys = Set<String>()
    private var routes: [String: AgentSignalAlertPolicy.Agent] = [:]
    var open: ((AgentSignalAlertPolicy.Agent) async -> Bool)?

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        policy = AgentSignalAlertPolicy(deliveredKeys: defaults.stringArray(forKey: "PP_signalDeliveredEvents") ?? [])
        knownNotificationKeys = Set(policy.deliveredKeys)
        acknowledgedErrors = Array((defaults.stringArray(forKey: "PP_signalAcknowledgedErrors") ?? []).suffix(4096))
        super.init()
        center.delegate = self
    }

    func update(_ agents: [AgentSignalAlertPolicy.Agent], foregroundIdentity: String?) {
        // Empty until owner qualification records the tool/adapter combination.
        // This is a release capability gate, separate from notification preferences.
        let qualified = Set(defaults.stringArray(forKey: "PP_signalQualifiedMacAdapters") ?? [])
        for agent in agents where agent.identity == foregroundIdentity && agent.presentation.label == "Error" {
            if let key = agent.eventKey, !acknowledgedErrors.contains(key) {
                acknowledgedErrors.append(key)
                acknowledgedErrors = Array(acknowledgedErrors.suffix(4096))
                defaults.set(acknowledgedErrors, forKey: "PP_signalAcknowledgedErrors")
            }
        }
        let result = policy.project(agents, qualifiedAttention: { agent in (agent.macOSActionable || agent.presentation.label == "Error") && agent.qualificationKey.map(qualified.contains) == true }, acknowledgedErrors: Set(acknowledgedErrors))
        if defaults.stringArray(forKey: "PP_signalDeliveredEvents") != policy.deliveredKeys {
            defaults.set(policy.deliveredKeys, forKey: "PP_signalDeliveredEvents")
        }
        #if os(macOS)
        NSApp.dockTile.badgeLabel = result.badge == 0 ? nil : String(result.badge)
        #endif
        let current = Set(agents.compactMap(\.eventKey))
        let obsolete = knownNotificationKeys.filter { !current.contains($0) }
        if !obsolete.isEmpty {
            center.removeDeliveredNotifications(withIdentifiers: Array(obsolete))
            center.removePendingNotificationRequests(withIdentifiers: Array(obsolete))
            knownNotificationKeys.subtract(obsolete)
        }
        routes = agents.reduce(into: [:]) { result, agent in
            if let key = agent.eventKey { result[key] = agent }
        }
        drainPendingTaps()
        for agent in result.alerts {
            guard let eventKey = agent.eventKey else { continue }
            let kind = agent.presentation.label
            let key = kind == "Finished" ? "completion" : kind.lowercased()
            guard agent.destination != nil, agent.qualificationKey.map(qualified.contains) == true,
                defaults.bool(forKey: "PP_signal_\(key)Notifications"),
                agent.identity != foregroundIdentity else { continue }
            routes[eventKey] = agent
            knownNotificationKeys.insert(eventKey)
            Task {
                guard (try? await center.requestAuthorization(options: [.alert, .badge, .sound])) == true,
                    routes[eventKey]?.eventKey == eventKey else { return }
                let content = UNMutableNotificationContent()
                content.title = "PurePoint"
                content.body = kind == "Finished" ? "An agent finished its turn." : "An agent needs attention."
                if defaults.bool(forKey: "PP_signal_\(key)Sound") { content.sound = .default }
                // Opaque event key encodes the destination identity; no prompt/request content.
                content.userInfo = ["signalEvent": eventKey]
                try? await center.add(UNNotificationRequest(identifier: eventKey, content: content, trigger: nil))
                if routes[eventKey] == nil {
                    center.removeDeliveredNotifications(withIdentifiers: [eventKey])
                    center.removePendingNotificationRequests(withIdentifiers: [eventKey])
                }
            }
        }
    }

    private func drainPendingTaps() {
        guard !drainingTaps, let open else { return }
        drainingTaps = true
        Task {
            defer { drainingTaps = false }
            while let key = pendingTaps.next(availableKeys: Set(routes.keys)), let route = routes[key] {
                guard await open(route) else { return }
                pendingTaps.complete(key)
            }
        }
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let key = response.notification.request.identifier
        let shouldOpen = response.actionIdentifier == UNNotificationDefaultActionIdentifier
        Task { @MainActor [weak self] in
            if let self, shouldOpen {
                pendingTaps.enqueue(key)
                drainPendingTaps()
            }
            completionHandler()
        }
    }
}
