import AppKit
import SwiftUI

enum PaneSplitNode { enum Axis { case vertical, horizontal } }
// Only the app environment is stubbed. The chat model, surface wrapper,
// conversation layout and pane splitter compiled by the runner are production code.
@MainActor @Observable final class AppState {
    var showSettings = false
    let pointGuardChat: PiChatModel
    init(chat: PiChatModel) { pointGuardChat = chat }
}
extension Notification.Name {
    static let toggleChatSidebar = Notification.Name("PP_toggleChatSidebar")
}
@MainActor final class LayoutState: ObservableObject { @Published var expanded = true }
@MainActor final class Probes {
    var views: [String: NSView] = [:]
}
struct Probe: NSViewRepresentable {
    let name: String
    let probes: Probes
    func makeNSView(context: Context) -> NSView {
        let view = NSView()
        probes.views[name] = view
        return view
    }
    func updateNSView(_ view: NSView, context: Context) {}
}
struct Fixture: View {
    @ObservedObject var state: LayoutState
    let probes: Probes
    let split: Bool
    var ratio: CGFloat = 0.5
    var body: some View {
        NavigationSplitView {
            Probe(name: "navigation", probes: probes)
                .navigationSplitViewColumnWidth(min: 200, ideal: 200, max: 200)
        } detail: {
            DraggableSplit(axis: .vertical, ratio: split ? ratio : 1, onRatioChanged: { _ in }) {
                VStack(spacing: 0) {
                    Color.gray.frame(height: 30)
                    PointGuardConversationLayout(isExpanded: $state.expanded) {
                        Button {} label: { Image(systemName: "square.and.pencil") }
                    } sidebar: {
                        Probe(name: "history", probes: probes)
                    } content: {
                        Probe(name: "transcript", probes: probes)
                    }
                }
            } second: {
                Probe(name: "sibling", probes: probes)
            }
            .background(Probe(name: "grid", probes: probes))
        }
    }
}
struct ChatFixture: View {
    let probes: Probes
    let focusLeft: Bool
    var body: some View {
        NavigationSplitView {
            Probe(name: "navigation", probes: probes)
                .navigationSplitViewColumnWidth(min: 200, ideal: 200, max: 200)
        } detail: {
            DraggableSplit(axis: .vertical, ratio: 0.5, onRatioChanged: { _ in }) {
                VStack(spacing: 0) {
                    Color.gray.frame(height: 30)
                    PointGuardChatSurfaceView(isFocused: focusLeft)
                }
            } second: {
                VStack(spacing: 0) {
                    Color.gray.frame(height: 30)
                    PointGuardChatSurfaceView(isFocused: !focusLeft)
                }
            }
            .background(Probe(name: "grid", probes: probes))
        }
    }
}
@main struct Checks {
    @MainActor static func main() {
        let app = NSApplication.shared
        app.setActivationPolicy(.accessory)
        app.finishLaunching()
        let state = LayoutState()
        let probes = Probes()
        let host = NSHostingView(rootView: AnyView(Fixture(state: state, probes: probes, split: false)))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 1000, height: 600), styleMask: [.borderless], backing: .buffered, defer: false)
        window.contentView = host
        window.makeKeyAndOrderFront(nil)
        defer { window.orderOut(nil) }
        func settle() {
            host.layoutSubtreeIfNeeded()
            RunLoop.main.run(until: Date().addingTimeInterval(0.1))
            host.layoutSubtreeIfNeeded()
        }
        func frame(_ name: String) -> NSRect {
            let view = probes.views[name]!
            return view.convert(view.bounds, to: host)
        }
        func clickToggle(atX x: CGFloat) {
            // The pane strip is 30pt high; the sidebar header's center is 20pt below it.
            let grid = frame("grid")
            let y = host.isFlipped ? grid.minY + 50 : grid.maxY - 50
            let location = host.convert(NSPoint(x: x, y: y), to: nil)
            // Queue release first: AppKit's button tracking loop consumes it while
            // handling mouse-down. Sending release afterward would deadlock the test.
            for type in [NSEvent.EventType.leftMouseUp, .leftMouseDown] {
                let event = NSEvent.mouseEvent(
                    with: type, location: location, modifierFlags: [],
                    timestamp: ProcessInfo.processInfo.systemUptime, windowNumber: window.windowNumber,
                    context: nil, eventNumber: 0, clickCount: 1, pressure: type == .leftMouseDown ? 1 : 0)!
                if type == .leftMouseUp { app.postEvent(event, atStart: false) } else { window.sendEvent(event) }
            }
            settle()
        }
        for ratio: CGFloat in [1, 0.5, 0.1] {
            let split = ratio < 1
            host.rootView = AnyView(Fixture(state: state, probes: probes, split: split, ratio: ratio))
            for width: CGFloat in [1200, 800, 700, 600] {
                window.setContentSize(NSSize(width: width, height: 600))
                for _ in 0..<3 {
                    state.expanded = true
                    settle()
                    let history = frame("history")
                    let transcript = frame("transcript")
                    let grid = frame("grid")
                    let paneEnd = split ? grid.minX + grid.width * ratio - 2 : grid.maxX
                    precondition(frame("navigation").maxX <= grid.minX + 1, "Grid overlaps navigation sidebar")
                    precondition(history.minX >= grid.minX - 1, "History overlaps navigation: \(history)")
                    precondition(history.width > 0, "History is hidden")
                    precondition(transcript.minX >= history.maxX, "Transcript overlaps history")
                    precondition(abs(transcript.maxX - paneEnd) < 1, "Chat escapes pane: \(transcript), \(paneEnd)")
                    if split {
                        precondition(transcript.maxX <= frame("sibling").minX, "Chat overlaps sibling pane")
                    }
                    // Click the actual SwiftUI button at the sidebar's top-right edge.
                    clickToggle(atX: history.maxX - min(22, history.width / 2))
                    precondition(!state.expanded, "Top-right toggle did not close history")
                    let collapsed = frame("transcript")
                    precondition(abs(collapsed.minX - grid.minX - 37) < 1, "Collapsed rail moved: \(collapsed)")
                    precondition(abs(collapsed.maxX - paneEnd) < 1, "Collapsed chat escapes pane")
                    precondition(collapsed.width >= transcript.width, "Closing history reduced chat space")
                    clickToggle(atX: grid.minX + 18)
                    precondition(state.expanded, "Collapsed sidebar toggle did not reopen history")
                }
            }
        }
        // Exercise the actual chat and its AppStorage/notification wrapper with two
        // visible chat splits, using disconnected Pi and isolated preferences/storage.
        let suite = "PointGuardSidebarChecks-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let preference = "PP_pointGuardShowConversations"
        defaults.set(false, forKey: preference)
        let appState = AppState(chat: PiChatModel(defaults: defaults))
        window.setContentSize(NSSize(width: 1200, height: 600))
        host.rootView = AnyView(
            ChatFixture(probes: probes, focusLeft: true).environment(appState).defaultAppStorage(defaults))
        settle()
        let grid = frame("grid")
        clickToggle(atX: grid.minX + 18)
        precondition(defaults.bool(forKey: preference), "Actual chat's local toggle did not open history")
        let sidebarWidth = min(210, (grid.width * 0.5 - 2) * 0.45)
        clickToggle(atX: grid.minX + sidebarWidth - 22)
        precondition(!defaults.bool(forKey: preference), "Actual chat's top-right toggle did not close history")
        NotificationCenter.default.post(name: .toggleChatSidebar, object: nil)
        settle()
        precondition(defaults.bool(forKey: preference), "Multiple chat splits canceled the open shortcut")
        host.rootView = AnyView(
            ChatFixture(probes: probes, focusLeft: false).environment(appState).defaultAppStorage(defaults))
        settle()
        NotificationCenter.default.post(name: .toggleChatSidebar, object: nil)
        settle()
        precondition(!defaults.bool(forKey: preference), "Focused right chat did not close history exactly once")
        print("Point Guard sidebar layout checks passed (real toggle clicks, repeated open/close, narrow panes and sibling splits)")
        print("Production chat integration checks passed (local controls, shared preferences, focused split shortcuts)")
        window.contentView = nil
    }
}
