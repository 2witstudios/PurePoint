import AppKit

@MainActor
final class HotkeyMonitor {
    var keyBindingState: KeyBindingState?
    private var monitor: Any?

    func start() {
        guard monitor == nil else { return }
        monitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self else { return event }
            return self.handleKeyDown(event)
        }
    }

    func stop() {
        if let monitor {
            NSEvent.removeMonitor(monitor)
            self.monitor = nil
        }
    }

    private func handleKeyDown(_ event: NSEvent) -> NSEvent? {
        guard let state = keyBindingState else { return event }

        // Don't intercept during recording
        if state.recordingAction != nil { return event }

        guard let action = state.action(for: event) else { return event }

        // Only consume events for monitor-handled actions
        // Menu-visible actions (newAgent, openProject, settings, split/close/focus panes)
        // are handled by SwiftUI .commands so they show in menus
        guard action.isMonitorHandled else { return event }

        NotificationCenter.default.post(
            name: .hotkeyAction,
            object: nil,
            userInfo: ["action": action]
        )
        return nil  // consume event
    }
}
