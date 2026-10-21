#if os(iOS)
import Observation
import UIKit

/// iOS twin of the Mac's key event monitor. The Mac catches the monitor-handled bindings in an
/// app-wide `NSEvent` monitor before the first responder (the terminal) sees them; here they are
/// `UIKeyCommand`s on every window's root view controller, which UIKit matches before it
/// delivers the press to the first responder. Like the Mac monitor they post `.hotkeyAction`,
/// handle only `isMonitorHandled` actions (the rest are the `AppCommands` menu items) and stand
/// down while Settings records a shortcut. Rebinding replaces the live commands.
@MainActor
final class HotkeyMonitor {
    var keyBindingState: KeyBindingState? {
        didSet { if isRunning { refresh() } }
    }

    private var isRunning = false
    private var observers: [NSObjectProtocol] = []
    /// The commands currently registered.
    private(set) var commands: [UIKeyCommand] = []
    /// Root view controllers holding commands, by identity, with the commands each holds, so a
    /// refresh can take them back.
    private var installed: [ObjectIdentifier: Installation] = [:]

    func start() {
        guard !isRunning else { return }
        isRunning = true
        // A new window (another scene, or SwiftUI replacing a root) needs the commands too.
        let names = [UIScene.didActivateNotification, UIWindow.didBecomeVisibleNotification, UIWindow.didBecomeKeyNotification]
        observers = names.map { name in
            NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.installInAllWindows() }
            }
        }
        refresh()
    }

    func stop() {
        guard isRunning else { return }
        isRunning = false
        for observer in observers { NotificationCenter.default.removeObserver(observer) }
        observers = []
        uninstallAll()
        commands = []
    }

    /// The key commands `state`'s bindings register: none while a shortcut is being recorded.
    static func keyCommands(for state: KeyBindingState) -> [UIKeyCommand] {
        // Don't intercept during recording
        guard state.recordingAction == nil else { return [] }
        return HotkeyAction.allCases.filter(\.isMonitorHandled).map { action in
            let command = state.binding(for: action).keyCommand(
                action: #selector(UIWindow.performHotkeyKeyCommand(_:)),
                propertyList: action.rawValue,
                title: action.displayName
            )
            // The menu bar already lists the ones that are menu items, as on the Mac.
            command.attributes = .hidden
            return command
        }
    }

    static func post(_ action: HotkeyAction) {
        NotificationCenter.default.post(
            name: .hotkeyAction,
            object: nil,
            userInfo: ["action": action]
        )
    }

    /// The action a fired command stands for.
    static func action(for command: UIKeyCommand) -> HotkeyAction? {
        (command.propertyList as? String).flatMap(HotkeyAction.init(rawValue:))
    }

    // MARK: - Registration

    /// Rebuild the commands from the bindings and re-register them; runs again whenever a
    /// binding or the recording state changes.
    private func refresh() {
        guard isRunning, let state = keyBindingState else { return }
        uninstallAll()
        commands = withObservationTracking {
            Self.keyCommands(for: state)
        } onChange: { [weak self] in
            Task { @MainActor [weak self] in self?.refresh() }
        }
        installInAllWindows()
        // The menu bar's shortcuts come from the same bindings.
        UIMenuSystem.main.setNeedsRebuild()
    }

    private func installInAllWindows() {
        guard isRunning else { return }
        let windows = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .flatMap(\.windows)
        for window in windows {
            guard let root = window.rootViewController else { continue }
            install(on: root)
        }
    }

    func install(on controller: UIViewController) {
        let id = ObjectIdentifier(controller)
        guard installed[id]?.controller !== controller else { return }
        commands.forEach(controller.addKeyCommand)
        installed[id] = Installation(controller: controller, commands: commands)
    }

    private func uninstallAll() {
        for entry in installed.values {
            guard let controller = entry.controller else { continue }
            entry.commands.forEach(controller.removeKeyCommand)
        }
        installed = [:]
    }

    private struct Installation {
        weak var controller: UIViewController?
        let commands: [UIKeyCommand]
    }
}

extension UIWindow {
    /// The target of the hotkey monitor's key commands. Every responder in a window passes
    /// unhandled actions up to its window, so the commands work from any first responder.
    @objc func performHotkeyKeyCommand(_ sender: UIKeyCommand) {
        guard let action = HotkeyMonitor.action(for: sender) else { return }
        // The sidebars' shown state is shared by every window, so it is toggled here once, as
        // the Mac monitor does, rather than by each window's handler.
        if SidebarVisibility.toggle(for: action) { return }
        HotkeyMonitor.post(action)
    }
}
#endif
