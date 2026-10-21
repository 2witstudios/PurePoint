import SwiftUI

struct SettingsPointGuardView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            Text("Point Guard")
                .font(.system(size: 18, weight: .semibold))

            #if os(macOS)
            PointGuardSetupView(service: appState.pointGuardService, chat: appState.pointGuardChat)
            #else
            // The iPad pairs with a Mac's Point Guard instead of hosting one (IPAD-011).
            PointGuardSetupView(chat: appState.pointGuardChat)
            #endif

        }
    }
}
