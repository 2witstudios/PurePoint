import SwiftUI

struct SettingsPointGuardView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        VStack(alignment: .leading, spacing: 24) {
            Text("Point Guard")
                .font(.system(size: 18, weight: .semibold))

            PointGuardSetupView(service: appState.pointGuardService, chat: appState.pointGuardChat)

        }
    }
}
