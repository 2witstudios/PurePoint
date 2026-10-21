#if os(iOS)
import SwiftUI

/// iOS twin of the Mac window-chrome hook. UIKit scenes have no title bar to extend content
/// under, so there is nothing to configure: it renders nothing.
struct WindowChromeConfigurator: View {
    var body: some View { EmptyView() }
}
#endif
