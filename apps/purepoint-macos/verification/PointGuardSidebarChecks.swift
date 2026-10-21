import AppKit
import SwiftUI

enum PaneSplitNode { enum Axis { case vertical, horizontal } }
// The daemon/workspace environment and setup sheets are stubbed. The production
// chat model, PointGuardView, conversation layout and pane splitter remain real.
// The script also compiles the production AppState root-safety method.
@MainActor @Observable final class AppState {
    var showSettings = false
    var pointGuardRoot: String? = "/fixture"
    var activeProjectRoot: String? = "/fixture"
    var projects = [ProjectFixture(projectRoot: "/fixture")]
    let pointGuardChat: PiChatModel
    let pointGuardService = ServiceFixture()
    init(chat: PiChatModel) { pointGuardChat = chat }
    func activatePointGuardWorkspace() {}
    func projectState(forRoot root: String) -> ProjectFixture? { projects.first { $0.projectRoot == root } }
    func pointGuardPrompts() async -> [PointGuardPrompt]? { [] }
    func pointGuardPrompt(named name: String) async -> PointGuardPrompt? { nil }
}
struct ProjectFixture { let projectRoot: String }
@MainActor final class ServiceFixture {
    func start(chat: PiChatModel) {}
    func connectPhone() async {}
}
enum SidebarSelection { case fixture }
enum SurfaceContent { case pointGuard, agent }
struct Surface { let content: SurfaceContent }
struct Pane { let activeTab: Surface? }
struct Workspace {
    let id = "fixture"
    let isPointGuard = true
    var panes: [Int: Pane]
}
@MainActor @Observable final class WorkspaceRegistry {
    let probes: Probes
    let focusLeft: Bool
    let showRightChat: Bool
    let showLeftChat: Bool
    var activeWorkspaceId: String?
    var layoutErrorsByProject: [String: String] = [:]
    var workspacesByProject: [String: [Workspace]] {
        ["/fixture": [Workspace(panes: [
            0: Pane(activeTab: Surface(content: showLeftChat ? .pointGuard : .agent)),
            1: Pane(activeTab: Surface(content: showRightChat ? .pointGuard : .agent))
        ])]]
    }
    init(probes: Probes, focusLeft: Bool, showRightChat: Bool, showLeftChat: Bool) {
        self.probes = probes
        self.focusLeft = focusLeft
        self.showRightChat = showRightChat
        self.showLeftChat = showLeftChat
    }
    func ensurePointGuardWorkspace(projectRoot: String) throws -> String { "fixture" }
    func activate(workspaceId: String) { activeWorkspaceId = workspaceId }
}
struct PointGuardSetupView: View {
    let service: ServiceFixture
    let chat: PiChatModel
    var body: some View { Color.clear }
}
struct PointGuardPhoneView: View {
    let service: ServiceFixture
    @Binding var isPresented: Bool
    var body: some View { Color.clear }
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
                        Button {} label: { Label("New Chat", systemImage: "square.and.pencil") }
                            .background(Probe(name: "newChat", probes: probes))
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
struct CompactFixture: View {
    @ObservedObject var state: LayoutState
    let probes: Probes
    var body: some View {
        PointGuardConversationLayout(isExpanded: $state.expanded, compact: true) {
            Button {} label: { Label("New Chat", systemImage: "square.and.pencil") }
                .background(Probe(name: "newChat", probes: probes))
        } sidebar: {
            Probe(name: "history", probes: probes)
        } content: {
            Probe(name: "transcript", probes: probes)
        }
    }
}
struct ChatFixture: View {
    let probes: Probes
    let focusLeft: Bool
    var showRightChat = true
    var showLeftChat = true
    var body: some View {
        NavigationSplitView {
            Probe(name: "navigation", probes: probes)
                .navigationSplitViewColumnWidth(min: 200, ideal: 200, max: 200)
        } detail: {
            PointGuardView(selection: .constant(nil))
                .environment(WorkspaceRegistry(
                    probes: probes, focusLeft: focusLeft, showRightChat: showRightChat, showLeftChat: showLeftChat))
        }
    }
}
struct PaneGridView: View {
    let workspaceId: String
    @Environment(WorkspaceRegistry.self) private var registry
    var body: some View {
        DraggableSplit(axis: .vertical, ratio: 0.5, onRatioChanged: { _ in }) {
            VStack(spacing: 0) {
                Color.gray.frame(height: 30)
                if registry.showLeftChat {
                    PointGuardChatSurfaceView(isFocused: registry.focusLeft)
                } else {
                    Color.clear
                }
            }
        } second: {
            VStack(spacing: 0) {
                Color.gray.frame(height: 30)
                if registry.showRightChat {
                    PointGuardChatSurfaceView(isFocused: !registry.focusLeft)
                } else {
                    Color.clear // Represents a terminal/file sibling with workspace focus.
                }
            }
        }
        .background(Probe(name: "grid", probes: registry.probes))
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
                    if transcript.width >= 160 {
                        precondition(frame("newChat").minX >= transcript.minX, "New chat button escapes transcript")
                        precondition(frame("newChat").maxX <= transcript.maxX, "New chat button escapes pane")
                    }
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
                    if collapsed.width >= 160 {
                        precondition(frame("newChat").width > 0, "New chat button disappeared with sidebar closed")
                        precondition(frame("newChat").minX >= collapsed.minX, "Collapsed new chat button escapes transcript")
                        precondition(frame("newChat").maxX <= collapsed.maxX, "Collapsed new chat button escapes pane")
                    }
                    clickToggle(atX: grid.minX + 18)
                    precondition(state.expanded, "Collapsed sidebar toggle did not reopen history")
                }
            }
        }
        // The inspector gives either history or chat its entire width. Check
        // binding-driven presentation changes at every supported sidebar size.
        for width: CGFloat in [240, 360, 640] {
            window.setContentSize(NSSize(width: width, height: 600))
            host.rootView = AnyView(CompactFixture(state: state, probes: probes))
            state.expanded = false
            settle()
            precondition(abs(frame("transcript").width - width) < 1, "Compact chat lost composer width")
            precondition(frame("newChat").minX >= 0 && frame("newChat").maxX <= width,
                         "Compact new-chat action escapes sidebar")
            state.expanded = true
            settle()
            precondition(abs(frame("history").width - width) < 1, "Compact history lost sidebar width")
            state.expanded = false
            settle()
            precondition(abs(frame("transcript").width - width) < 1, "Returning to chat reduced composer width")
        }
        // Exercise the actual chat and its AppStorage/notification wrapper with two
        // visible chat splits, using disconnected Pi and isolated preferences/storage.
        let suite = "PointGuardSidebarChecks-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let preference = "PP_pointGuardShowConversations"
        defaults.set(false, forKey: preference)
        let appState = AppState(chat: PiChatModel(defaults: defaults))
        precondition(appState.isTooBroadProjectRoot("/"), "Production root guard must reject filesystem root")
        precondition(!appState.isTooBroadProjectRoot("/fixture"), "Fixture root must remain usable")
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
        host.rootView = AnyView(
            ChatFixture(probes: probes, focusLeft: false, showRightChat: false)
                .environment(appState).defaultAppStorage(defaults))
        settle()
        NotificationCenter.default.post(name: .toggleChatSidebar, object: nil)
        settle()
        precondition(defaults.bool(forKey: preference), "Focused terminal/file sibling prevented history from opening")
        NotificationCenter.default.post(name: .toggleChatSidebar, object: nil)
        settle()
        precondition(!defaults.bool(forKey: preference), "Focused terminal/file sibling prevented history from closing")
        host.rootView = AnyView(
            ChatFixture(probes: probes, focusLeft: false, showRightChat: false, showLeftChat: false)
                .environment(appState).defaultAppStorage(defaults))
        settle()
        NotificationCenter.default.post(name: .toggleChatSidebar, object: nil)
        settle()
        precondition(!defaults.bool(forKey: preference), "Hidden chat tabs handled the history shortcut")
        appState.activeProjectRoot = nil
        appState.projects = []
        host.rootView = AnyView(
            ChatFixture(probes: probes, focusLeft: true).environment(appState).defaultAppStorage(defaults))
        settle()
        NotificationCenter.default.post(name: .toggleChatSidebar, object: nil)
        settle()
        precondition(defaults.bool(forKey: preference), "Folder-backed chat without a selected project did not handle history shortcut")
        defaults.set(false, forKey: preference)
        appState.pointGuardRoot = nil
        settle()
        NotificationCenter.default.post(name: .toggleChatSidebar, object: nil)
        settle()
        precondition(defaults.bool(forKey: preference), "No-project chat fallback did not handle history shortcut")
        print("Point Guard sidebar layout checks passed (real toggle clicks, repeated open/close, narrow panes and sibling splits)")
        print("Production chat integration checks passed (local controls, two chats, focused terminal/file, hidden chat, no-project fallback)")
        print("Compact sidebar checks passed (240/360/640pt, full-width history/chat and repeated presentation changes)")
        window.contentView = nil
    }
}
