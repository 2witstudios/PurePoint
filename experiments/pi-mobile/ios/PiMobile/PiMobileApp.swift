import SwiftUI
import OSLog

// Static markers only: no endpoint, credential, message, or device details.
enum LaunchLog {
    static let logger = Logger(subsystem: "PiMobile", category: "Launch")
}
@main struct PiMobileApp: App {
    init() { LaunchLog.logger.notice("SwiftUI App initialized") }
    var body: some Scene { WindowGroup { ChatView().tint(.accentColor) } }
}
