import SwiftUI

@main
struct purepoint_macosApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                .preferredColorScheme(.dark)
                .frame(
                    minWidth: PurePointTheme.windowMinWidth,
                    minHeight: PurePointTheme.windowMinHeight
                )
        }
        .defaultSize(
            width: PurePointTheme.windowDefaultWidth,
            height: PurePointTheme.windowDefaultHeight
        )
        .windowToolbarStyle(.unified(showsTitle: true))
    }
}
