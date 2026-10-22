import SwiftUI

/// What the command palette needs to open, independent of any window.
struct CommandPalettePresentation {
    let items: [CommandPaletteItem]
    let usePointGuardNames: Bool
    let onSelect: (CommandPaletteResult) -> Void
}

/// Hook points the shared App code calls where a platform supplies the behaviour. The App files
/// and `AppCommands` only call these; the features that implement them register a handler from
/// their own files, so no feature has to edit the App entry points.
///
/// - `openProject`: the Open Project command on iOS. The remote folder picker (IPAD-7.1)
///   registers it. On macOS the command runs `NSOpenPanel` directly and never calls it.
/// - `scenePhaseChanged`: the app's scene phase, forwarded by the iPad App. Background polling
///   and stream resumption (IPAD-7.4) register it.
/// - `presentPalette`: shows the command palette on iOS, where there is no key `NSWindow` to
///   anchor the Mac's palette panel to. The iOS palette registers it.
enum AppLifecycleHooks {
    static var openProject: (() -> Void)?
    static var scenePhaseChanged: ((ScenePhase) -> Void)?
    static var presentPalette: ((CommandPalettePresentation) -> Void)?

    /// Coalesces palette open requests while their data loads, as the Mac panel's coordinator does.
    static let paletteOpenCoordinator = CommandPaletteOpenCoordinator()
}
