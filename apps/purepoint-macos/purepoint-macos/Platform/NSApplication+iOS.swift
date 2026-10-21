#if os(iOS)
import UIKit

/// iOS twin of the `NSApplication` queries shared state makes (is the app frontmost, does it
/// have a key window), answered from `UIApplication` so the shared call sites stay unchanged.
final class NSApplication {
    static let shared = NSApplication()

    /// Foreground and active, as an active Mac app is.
    var isActive: Bool { UIApplication.shared.applicationState == .active }

    /// The window the user is working in. With several windows on screen every scene is
    /// foreground-active, but only the one last interacted with holds the app's key window.
    var keyWindow: UIWindow? {
        let scenes = UIApplication.shared.connectedScenes
            .filter { $0.activationState == .foregroundActive }
            .compactMap { $0 as? UIWindowScene }
        return scenes.flatMap(\.windows).first(where: \.isKeyWindow) ?? scenes.compactMap(\.keyWindow).first
    }

    /// iOS apps cannot bring themselves to the foreground; a notification tap already did.
    func activate(ignoringOtherApps: Bool) {}
}

/// AppKit's shared-application global, so `NSApp.keyWindow` call sites compile unchanged.
@MainActor var NSApp: NSApplication { .shared }

/// A Mac window is a UIKit window on iOS: shared views only pass it through (to anchor the
/// command palette) or hand it focus requests.
typealias NSWindow = UIWindow

extension UIWindow {
    /// AppKit's focus call. UIKit moves focus by asking the responder itself.
    @discardableResult
    func makeFirstResponder(_ responder: UIResponder?) -> Bool {
        responder?.becomeFirstResponder() ?? false
    }
}
#endif
