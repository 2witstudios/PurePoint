#if os(iOS)
import SwiftUI

/// Placeholder twin of the Mac Point Guard setup. The iPad never hosts Point Guard, so the
/// Mac's hosting, provider, working-folder and phone controls have no iOS form; QR pairing with
/// a Mac replaces them (IPAD-8.2).
struct PointGuardSetupView: View {
    @ObservedObject var chat: PiChatModel

    var body: some View {
        NotYetOnIPadLabel(name: "PointGuardSetupView")
            .frame(minHeight: 120)
    }
}
#endif
