import AppKit

/// Quit and update wait for the exact owned runtime; busy work needs a deliberate choice.
@MainActor final class PointGuardApplicationDelegate: NSObject, NSApplicationDelegate {
    weak var pointGuardService: PointGuardServiceModel?
    private var terminationPending = false
    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        guard let service = pointGuardService else { return .terminateNow }
        guard !terminationPending else { return .terminateLater }
        terminationPending = true
        Task {
            var mayTerminate = false
            defer {
                terminationPending = false
                sender.reply(toApplicationShouldTerminate: mayTerminate)
            }
            do {
                try await service.stop()
                mayTerminate = true
            } catch {
                service.error = error.localizedDescription
                let alert = NSAlert()
                alert.alertStyle = .warning
                alert.messageText = "Point Guard is still running"
                alert.informativeText = "\(error.localizedDescription)\n\nStop Pi and quit ends the current Pi run. Your saved conversation and drafts remain available; uncertain prompts will not be resent. Quit and updates wait until the owned runtime exits."
                alert.addButton(withTitle: "Cancel")
                alert.addButton(withTitle: "Stop Pi and Quit")
                guard alert.runModal() == .alertSecondButtonReturn else { return }
                do {
                    try await service.stopOwnedForQuit()
                    mayTerminate = true
                } catch {
                    service.error = error.localizedDescription
                    let failure = NSAlert()
                    failure.alertStyle = .warning
                    failure.messageText = "Quit is paused"
                    failure.informativeText = error.localizedDescription
                    failure.addButton(withTitle: "Keep App Open")
                    failure.runModal()
                }
            }
        }
        return .terminateLater
    }
}
