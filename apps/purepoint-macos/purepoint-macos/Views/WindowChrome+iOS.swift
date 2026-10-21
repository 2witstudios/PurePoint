#if os(iOS)
import SwiftUI

/// iOS twin of the Mac window-chrome hook. UIKit scenes have no title bar to extend content
/// under, so there is nothing to configure and it draws nothing. It lives for as long as its
/// window, whatever the sidebar shows, so it hosts the window's Open Project picker.
struct WindowChromeConfigurator: View {
    var body: some View {
        Color.clear
            .remoteProjectPicker()
    }
}
#endif
