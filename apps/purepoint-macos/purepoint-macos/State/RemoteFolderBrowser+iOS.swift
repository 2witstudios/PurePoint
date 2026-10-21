#if os(iOS)
import Foundation
import Observation

/// The folder browser behind the iPad's Open Project panel: one machine's directories, read
/// through the daemon's `fs_list` in folder-browsing mode (`DaemonRequest.fsBrowse`). It keeps
/// the Mac open panel's navigation: back/forward history, enclosing folder, a selection moved
/// with the arrow keys or by typing a name, and hidden folders shown on request.
@Observable
@MainActor
final class RemoteFolderBrowser {
    struct Folder: Identifiable, Equatable, Sendable {
        let name: String
        let path: String
        let isSymlink: Bool
        var id: String { path }
    }

    /// Lists the subdirectories of `path` on a machine.
    typealias Lister = @MainActor (_ machineId: UUID, _ path: String, _ includeHidden: Bool) async throws
        -> DaemonResponse

    private(set) var machineId: UUID?
    /// The canonical directory on screen, nil until the first listing answers.
    private(set) var path: String?
    /// The daemon user's home, resolved from the first listing of `~`.
    private(set) var home: String?
    private(set) var folders: [Folder] = []
    /// The selected folder's path.
    var selection: String?
    private(set) var showHidden = false
    private(set) var isLoading = false
    private(set) var error: String?
    /// The daemon capped the listing; more folders exist than are shown.
    private(set) var truncated = false
    private(set) var backStack: [String] = []
    private(set) var forwardStack: [String] = []

    @ObservationIgnored private let list: Lister
    @ObservationIgnored private var generation = 0
    @ObservationIgnored private var typeSelectPrefix = ""
    @ObservationIgnored private var typeSelectTime = Date.distantPast

    /// Typed characters within this interval extend the type-select prefix, as in a Mac list.
    static let typeSelectInterval: TimeInterval = 1

    init(list: @escaping Lister) {
        self.list = list
    }

    /// The production browser: each listing is one `fs_list` request to the machine's daemon.
    static func daemon() -> RemoteFolderBrowser {
        RemoteFolderBrowser { machineId, path, includeHidden in
            try await DaemonEndpoint.machine(machineId).makeClient()
                .send(.fsBrowse(path: path, includeHidden: includeHidden))
        }
    }

    // MARK: - Navigation

    /// Start browsing a machine at its user's home folder.
    func choose(machineId: UUID) async {
        guard machineId != self.machineId else { return }
        self.machineId = machineId
        path = nil
        home = nil
        folders = []
        selection = nil
        truncated = false
        backStack = []
        forwardStack = []
        await load("~", record: false)
    }

    func enter(_ folder: Folder) async {
        await load(folder.path, record: true)
    }

    /// Open the selected folder in the browser (not as a project).
    func enterSelection() async {
        guard let selected = selectedFolder else { return }
        await enter(selected)
    }

    /// Go to the enclosing folder and select the one just left, as the Mac panel does.
    func goUp() async {
        guard let path, path != "/" else { return }
        let parent = (path as NSString).deletingLastPathComponent
        await load(parent.isEmpty ? "/" : parent, record: true, select: path)
    }

    func goBack() async {
        guard let target = backStack.last, let current = path else { return }
        backStack.removeLast()
        forwardStack.append(current)
        await load(target, record: false, select: current)
    }

    func goForward() async {
        guard let target = forwardStack.last, let current = path else { return }
        forwardStack.removeLast()
        backStack.append(current)
        await load(target, record: false)
    }

    /// Jump to a folder from the path bar.
    func navigate(to target: String) async {
        guard target != path else { return }
        await load(target, record: true, select: childOnPath(of: target))
    }

    func setShowHidden(_ show: Bool) async {
        guard show != showHidden else { return }
        showHidden = show
        guard let path else { return }
        await load(path, record: false, select: selection)
    }

    /// List the folder on screen again.
    func reload() async {
        await load(path ?? "~", record: false, select: selection)
    }

    // MARK: - Selection

    var selectedFolder: Folder? {
        guard let selection else { return nil }
        return folders.first { $0.path == selection }
    }

    /// Move the selection by `offset` rows. With nothing selected, Down selects the first
    /// folder and Up the last.
    func moveSelection(by offset: Int) {
        guard !folders.isEmpty else { return }
        guard let index = folders.firstIndex(where: { $0.path == selection }) else {
            selection = (offset >= 0 ? folders.first : folders.last)?.path
            return
        }
        selection = folders[min(max(index + offset, 0), folders.count - 1)].path
    }

    func selectFirst() { selection = folders.first?.path }
    func selectLast() { selection = folders.last?.path }

    /// Select the first folder whose name starts with the typed characters. Characters typed
    /// within `typeSelectInterval` of each other extend the prefix.
    func typeSelect(_ characters: String, at time: Date = Date()) {
        if time.timeIntervalSince(typeSelectTime) > Self.typeSelectInterval { typeSelectPrefix = "" }
        typeSelectTime = time
        typeSelectPrefix += characters
        let prefix = typeSelectPrefix
        if let match = folders.first(where: {
            $0.name.range(of: prefix, options: [.caseInsensitive, .anchored, .diacriticInsensitive]) != nil
        }) {
            selection = match.path
        }
    }

    // MARK: - Opening

    /// What Open registers as the project: the selected folder, else the folder on screen.
    var openTarget: String? { selectedFolder?.path ?? path }

    /// Why `target` cannot be a project root, mirroring the daemon's rule: `/`, the home
    /// folder and its ancestors are too broad.
    func refusal(for target: String) -> String? {
        if target == "/" { return "/ is too broad to open as a project. Choose a project folder." }
        guard let home else { return nil }
        if home == target || home.hasPrefix(target + "/") {
            return "\(target) contains your home folder. Choose a project folder inside it."
        }
        return nil
    }

    // MARK: - Path bar

    /// Every folder from `/` down to the folder on screen, for the path bar and the path menu.
    var pathComponents: [Folder] {
        guard let path else { return [] }
        var components = [Folder(name: "/", path: "/", isSymlink: false)]
        var current = ""
        for name in path.split(separator: "/") {
            current += "/" + name
            components.append(Folder(name: String(name), path: current, isSymlink: false))
        }
        return components
    }

    // MARK: - Listing

    private func childOnPath(of ancestor: String) -> String? {
        guard let path, path.hasPrefix(ancestor == "/" ? "/" : ancestor + "/") else { return nil }
        let rest = path.dropFirst(ancestor == "/" ? 1 : ancestor.count + 1)
        guard let name = rest.split(separator: "/").first else { return nil }
        return (ancestor as NSString).appendingPathComponent(String(name))
    }

    private func load(_ target: String, record: Bool, select: String? = nil) async {
        guard let machineId else { return }
        generation += 1
        let current = generation
        isLoading = true
        error = nil
        let response: DaemonResponse
        do {
            response = try await list(machineId, target, showHidden)
        } catch {
            guard current == generation else { return }
            isLoading = false
            self.error = error.localizedDescription
            return
        }
        // A newer navigation or another machine superseded this listing.
        guard current == generation, machineId == self.machineId else { return }
        isLoading = false
        switch response {
        case .fsListResult(let canonical, let entries, let truncated):
            if target == "~", home == nil { home = canonical }
            if record, let previous = path, previous != canonical {
                backStack.append(previous)
                forwardStack.removeAll()
            }
            let previousSelection = selection
            path = canonical
            folders = entries.filter(\.isDir)
                .map { Folder(name: $0.name, path: $0.path, isSymlink: $0.isSymlink) }
                .sorted { $0.name.localizedStandardCompare($1.name) == .orderedAscending }
            self.truncated = truncated
            let wanted = select ?? (record ? nil : previousSelection)
            selection = folders.contains { $0.path == wanted } ? wanted : nil
        // A daemon from before folder browsing ignores `directories_only` and refuses `~`
        // as a relative path outside every project.
        case .error("path_not_allowed", _) where target == "~":
            error = "This machine's PurePoint cannot browse folders. Update PurePoint on the machine."
        case .error(_, let message):
            error = message
        default:
            error = "The machine's daemon sent an unexpected reply to a folder listing."
        }
    }
}
#endif
