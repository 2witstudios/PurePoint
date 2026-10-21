#if os(iOS)
import UIKit

/// iOS twin of the `NSApplication` queries shared state makes (is the app frontmost, does it
/// have a key window), answered from `UIApplication` so the shared call sites stay unchanged.
final class NSApplication {
    static let shared = NSApplication()

    /// Foreground and active, as an active Mac app is.
    var isActive: Bool { UIApplication.shared.applicationState == .active }

    /// The key window of the foreground scene, if any.
    var keyWindow: UIWindow? {
        UIApplication.shared.connectedScenes
            .filter { $0.activationState == .foregroundActive }
            .compactMap { ($0 as? UIWindowScene)?.keyWindow }
            .first
    }

    /// iOS apps cannot bring themselves to the foreground; a notification tap already did.
    func activate(ignoringOtherApps: Bool) {}
}
#endif
