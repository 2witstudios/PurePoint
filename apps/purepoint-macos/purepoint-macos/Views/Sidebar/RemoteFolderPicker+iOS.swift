#if os(iOS)
import SwiftUI

/// The iPad's Open Project panel, in place of the Mac's `NSOpenPanel`: choose a machine, browse
/// its folders through its daemon, and open one as a project on that machine. Laid out like the
/// Mac panel — navigation and path menu on top, the folder list, the path bar, Cancel/Open — with
/// the same keys: arrows and type-select move the selection, ⌘↓/→ go into a folder, ⌘↑/← go
/// to the enclosing one, ⌘[ / ⌘] go back and forward, ⇧⌘. shows hidden folders, Return opens
/// and Escape cancels. There is no New Folder: the daemon creates no directories on request.
struct RemoteFolderPicker: View {
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss
    @State private var browser: RemoteFolderBrowser
    @State private var machineId: UUID?
    @State private var isOpening = false
    @State private var openError: String?
    @FocusState private var listFocused: Bool

    static let message = "Choose a project directory"
    private static let rowHeight = PurePointTheme.sidebarRowHeight

    /// `browser` defaults to the machine daemons' folder listings.
    @MainActor init(browser: RemoteFolderBrowser? = nil) {
        _browser = State(initialValue: browser ?? .daemon())
    }

    var body: some View {
        VStack(spacing: 0) {
            Text(Self.message)
                .font(.system(size: 13, weight: .semibold))
                .frame(maxWidth: .infinity)
                .padding(.top, 14)
                .padding(.bottom, 8)
            toolbar
            Divider()
            folderList
            Divider()
            pathBar
            Divider()
            footer
        }
        .frame(minWidth: 560, idealWidth: 640, minHeight: 420, idealHeight: 480)
        .background(Color(nsColor: .windowBackgroundColor))
        .background(shortcuts)
        .task { chooseInitialMachine() }
        .onChange(of: appState.machines.map(\.state)) { _, _ in chooseInitialMachine() }
        .onChange(of: machineId) { _, id in
            guard let id else { return }
            openError = nil
            Task { await browser.choose(machineId: id) }
        }
        .onChange(of: browser.selection) { _, _ in openError = nil }
        .onChange(of: browser.path) { _, _ in openError = nil }
    }

    // MARK: - Machine

    private var machine: MachineState? { appState.machines.first { $0.machine.id == machineId } }

    /// The machine owning the current project, else the first one whose daemon answers.
    private func chooseInitialMachine() {
        if let machine, machine.state.isDaemonRunning { return }
        let activeMachine = appState.activeProjectRoot.flatMap { ProjectKey(id: $0).machineId }
        let running = appState.machines.filter(\.state.isDaemonRunning)
        let chosen = running.first { $0.machine.id == activeMachine } ?? running.first
        if let chosen { machineId = chosen.machine.id } else if machineId == nil {
            machineId = appState.machines.first?.machine.id
        }
    }

    private var machineMenu: some View {
        Menu {
            ForEach(appState.machines) { candidate in
                Button {
                    machineId = candidate.machine.id
                } label: {
                    if candidate.machine.id == machineId {
                        Label("\(candidate.machine.name) — \(candidate.statusText)", systemImage: "checkmark")
                    } else {
                        Text("\(candidate.machine.name) — \(candidate.statusText)")
                    }
                }
                .disabled(!candidate.state.isDaemonRunning)
            }
        } label: {
            HStack(spacing: 5) {
                Circle()
                    .fill(Color(nsColor: Self.statusColor(machine?.state ?? .disconnected)))
                    .frame(width: PurePointTheme.statusDotSize, height: PurePointTheme.statusDotSize)
                Image(systemName: "desktopcomputer")
                Text(machine?.machine.name ?? "No Machine")
                    .font(PurePointTheme.treeFont)
                Image(systemName: "chevron.up.chevron.down")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(.tertiary)
            }
            .foregroundStyle(.primary)
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .fixedSize()
        .disabled(appState.machines.isEmpty)
        .accessibilityLabel("Machine")
    }

    /// The sidebar's machine status colors.
    static func statusColor(_ state: MachineConnectionState) -> NSColor {
        switch state {
        case .connected(.running): .systemGreen
        case .connected(.stopped): .systemOrange
        case .connected(.incompatible): .systemRed
        case .connecting: .systemYellow
        case .offline: .systemRed
        case .disconnected: .tertiaryLabelColor
        }
    }

    // MARK: - Toolbar

    private var toolbar: some View {
        HStack(spacing: 10) {
            HStack(spacing: 2) {
                iconButton("chevron.left", label: "Back", disabled: browser.backStack.isEmpty) {
                    Task { await browser.goBack() }
                }
                iconButton("chevron.right", label: "Forward", disabled: browser.forwardStack.isEmpty) {
                    Task { await browser.goForward() }
                }
            }
            pathMenu
            Spacer(minLength: 8)
            machineMenu
            iconButton(
                browser.showHidden ? "eye" : "eye.slash",
                label: browser.showHidden ? "Hide Hidden Folders" : "Show Hidden Folders",
                disabled: browser.path == nil
            ) {
                Task { await browser.setShowHidden(!browser.showHidden) }
            }
        }
        .padding(.horizontal, 12)
        .frame(height: 36)
    }

    /// The Mac panel's folder pop-up: the folder on screen, opening onto its ancestors.
    private var pathMenu: some View {
        Menu {
            ForEach(browser.pathComponents.reversed()) { component in
                Button {
                    Task { await browser.navigate(to: component.path) }
                } label: {
                    Label(componentName(component), systemImage: component.path == "/" ? "desktopcomputer" : "folder")
                }
            }
        } label: {
            HStack(spacing: 5) {
                Image(systemName: "folder.fill")
                    .foregroundStyle(Color.accentColor)
                Text(browser.pathComponents.last.map(componentName) ?? "—")
                    .font(PurePointTheme.navFont)
                    .lineLimit(1)
                Image(systemName: "chevron.up.chevron.down")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 8)
            .frame(height: 24)
            .background(RoundedRectangle(cornerRadius: 5).fill(Color.primary.opacity(0.06)))
            .foregroundStyle(.primary)
        }
        .menuStyle(.button)
        .buttonStyle(.plain)
        .fixedSize()
        .disabled(browser.path == nil)
        .accessibilityLabel("Folder")
    }

    private func iconButton(_ symbol: String, label: String, disabled: Bool, action: @escaping () -> Void)
        -> some View
    {
        Button(action: action) {
            Image(systemName: symbol)
                .font(.system(size: 13, weight: .medium))
                .frame(width: 26, height: 24)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(disabled ? .tertiary : .secondary)
        .disabled(disabled)
        .hoverEffect(.highlight)
        .help(label)
        .accessibilityLabel(label)
    }

    private func componentName(_ component: RemoteFolderBrowser.Folder) -> String {
        component.path == "/" ? (machine?.machine.name ?? "/") : component.name
    }

    // MARK: - Folder list

    @ViewBuilder
    private var folderList: some View {
        ZStack {
            if let message = unavailableMessage {
                placeholder(message, systemImage: "desktopcomputer.trianglebadge.exclamationmark")
            } else if let error = browser.error {
                VStack(spacing: 8) {
                    placeholder(error, systemImage: "exclamationmark.triangle")
                        .frame(maxHeight: nil)
                    Button("Try Again") { Task { await browser.reload() } }
                        .font(PurePointTheme.smallFont)
                }
            } else if browser.path == nil {
                ProgressView()
            } else if browser.folders.isEmpty {
                placeholder("No folders", systemImage: nil)
            } else {
                rows
            }
            if browser.isLoading, browser.path != nil {
                ProgressView()
                    .controlSize(.small)
                    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topTrailing)
                    .padding(8)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(Color(nsColor: .textBackgroundColor))
        .focusable()
        .focused($listFocused)
        .focusEffectDisabled()
        .onKeyPress(phases: [.down, .repeat], action: handleKey)
        .onAppear { listFocused = true }
    }

    private var unavailableMessage: String? {
        if appState.machines.isEmpty {
            return "No machines. Add a machine with the machine button in the sidebar footer, then open its folders."
        }
        guard let machine else { return nil }
        guard machine.state.isDaemonRunning else {
            return "\(machine.machine.name): \(machine.statusText). Its folders are shown once its daemon is running."
        }
        return nil
    }

    private func placeholder(_ text: String, systemImage: String?) -> some View {
        VStack(spacing: 6) {
            if let systemImage {
                Image(systemName: systemImage)
                    .font(.system(size: 22))
                    .foregroundStyle(.tertiary)
            }
            Text(text)
                .font(PurePointTheme.smallFont)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var rows: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 0) {
                    ForEach(browser.folders) { folder in
                        row(folder).id(folder.path)
                    }
                    if browser.truncated {
                        Text("Not every folder is shown.")
                            .font(PurePointTheme.smallFont)
                            .foregroundStyle(.tertiary)
                            .frame(height: Self.rowHeight)
                    }
                }
                .padding(.vertical, 4)
                .padding(.horizontal, 6)
            }
            .onChange(of: browser.selection) { _, selection in
                guard let selection else { return }
                proxy.scrollTo(selection)
            }
            .onChange(of: browser.path) { _, _ in
                if let selection = browser.selection { proxy.scrollTo(selection, anchor: .center) }
            }
        }
    }

    private func row(_ folder: RemoteFolderBrowser.Folder) -> some View {
        let isSelected = browser.selection == folder.path
        return HStack(spacing: 6) {
            Image(systemName: "folder.fill")
                .foregroundStyle(isSelected ? Color.white : Color.accentColor)
                .frame(width: 18)
            Text(folder.name)
                .font(PurePointTheme.treeFont)
                .lineLimit(1)
                .truncationMode(.middle)
            if folder.isSymlink {
                Image(systemName: "arrow.turn.up.right")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(isSelected ? AnyShapeStyle(Color.white.opacity(0.8)) : AnyShapeStyle(.tertiary))
                    .accessibilityLabel("Alias")
            }
            Spacer(minLength: 4)
            Image(systemName: "chevron.right")
                .font(.system(size: 9, weight: .semibold))
                .foregroundStyle(isSelected ? AnyShapeStyle(Color.white.opacity(0.8)) : AnyShapeStyle(.tertiary))
        }
        .foregroundStyle(isSelected ? Color.white : Color.primary)
        .padding(.horizontal, 8)
        .frame(height: Self.rowHeight)
        .background(
            RoundedRectangle(cornerRadius: 5)
                .fill(isSelected ? Color.accentColor : Color.clear)
        )
        .contentShape(Rectangle())
        .hoverEffect(.highlight)
        .onTapGesture(count: 2) { Task { await browser.enter(folder) } }
        .onTapGesture {
            browser.selection = folder.path
            listFocused = true
        }
        .contextMenu {
            Button("Open") { Task { await browser.enter(folder) } }
            Button("Open as Project") { open(folder.path) }
        }
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(isSelected ? [.isSelected, .isButton] : .isButton)
        .accessibilityAction(named: "Open") { Task { await browser.enter(folder) } }
    }

    // MARK: - Path bar

    /// The Mac panel's path bar: the folder on screen as clickable ancestors.
    private var pathBar: some View {
        ScrollViewReader { proxy in
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 3) {
                    ForEach(Array(browser.pathComponents.enumerated()), id: \.element.id) { index, component in
                        if index > 0 {
                            Image(systemName: "chevron.right")
                                .font(.system(size: 8, weight: .semibold))
                                .foregroundStyle(.tertiary)
                        }
                        Button {
                            Task { await browser.navigate(to: component.path) }
                        } label: {
                            HStack(spacing: 3) {
                                Image(systemName: component.path == "/" ? "desktopcomputer" : "folder.fill")
                                    .foregroundStyle(component.path == "/" ? Color.secondary : Color.accentColor)
                                Text(componentName(component))
                            }
                            .font(PurePointTheme.smallFont)
                            .foregroundStyle(index == browser.pathComponents.count - 1 ? .primary : .secondary)
                        }
                        .buttonStyle(.plain)
                        .hoverEffect(.highlight)
                        .id(component.path)
                    }
                }
                .padding(.horizontal, 12)
            }
            .onChange(of: browser.path) { _, path in
                if let path { proxy.scrollTo(path, anchor: .trailing) }
            }
        }
        .frame(height: 26)
    }

    // MARK: - Footer

    private var footer: some View {
        HStack(spacing: 10) {
            if let message = openError ?? openTarget.flatMap(browser.refusal(for:)) {
                Text(message)
                    .font(.system(size: 11))
                    .foregroundStyle(openError == nil ? Color.orange : Color.red)
                    .lineLimit(2)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer()
            Button("Cancel") { dismiss() }
                .keyboardShortcut(.cancelAction)
            Button(isOpening ? "Opening\u{2026}" : "Open") {
                if let openTarget { open(openTarget) }
            }
            .keyboardShortcut(.defaultAction)
            .buttonStyle(.borderedProminent)
            .disabled(!canOpen)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    private var openTarget: String? { machine?.state.isDaemonRunning == true ? browser.openTarget : nil }

    private var canOpen: Bool {
        guard let openTarget, !isOpening, !browser.isLoading else { return false }
        return browser.refusal(for: openTarget) == nil
    }

    private func open(_ root: String) {
        guard let machine, !isOpening else { return }
        if let refusal = browser.refusal(for: root) {
            openError = refusal
            return
        }
        isOpening = true
        openError = nil
        Task {
            do {
                try await machine.openProject(root: root)
                isOpening = false
                dismiss()
            } catch {
                isOpening = false
                openError = "Could not open \(root) on \(machine.machine.name): \(error.localizedDescription)"
            }
        }
    }

    // MARK: - Keyboard

    /// Hidden buttons carrying the panel's command shortcuts, so they reach the menu-less sheet.
    private var shortcuts: some View {
        ZStack {
            Button("Show Hidden Folders") { Task { await browser.setShowHidden(!browser.showHidden) } }
                .keyboardShortcut(".", modifiers: [.command, .shift])
            Button("Back") { Task { await browser.goBack() } }
                .keyboardShortcut("[", modifiers: .command)
            Button("Forward") { Task { await browser.goForward() } }
                .keyboardShortcut("]", modifiers: .command)
            Button("Enclosing Folder") { Task { await browser.goUp() } }
                .keyboardShortcut(.upArrow, modifiers: .command)
            Button("Open Folder") { Task { await browser.enterSelection() } }
                .keyboardShortcut(.downArrow, modifiers: .command)
        }
        .opacity(0)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }

    private func handleKey(_ press: KeyPress) -> KeyPress.Result {
        guard press.modifiers.isDisjoint(with: [.command, .control]) else { return .ignored }
        switch press.key {
        case .upArrow:
            browser.moveSelection(by: -1)
        case .downArrow:
            browser.moveSelection(by: 1)
        case .rightArrow:
            Task { await browser.enterSelection() }
        case .leftArrow:
            Task { await browser.goUp() }
        case .home:
            browser.selectFirst()
        case .end:
            browser.selectLast()
        default:
            let characters = press.characters
            guard !characters.isEmpty, characters.unicodeScalars.allSatisfy({
                !CharacterSet.controlCharacters.contains($0) && !CharacterSet.newlines.contains($0)
            }) else { return .ignored }
            browser.typeSelect(characters)
        }
        return .handled
    }
}

/// Presents the remote folder picker for Open Project. It registers `AppLifecycleHooks.openProject`
/// (called by the Open Project menu command and the sidebar footer's Open button) whenever its
/// scene appears or becomes active, so the command opens the picker in the window in front.
struct RemoteProjectPickerPresenter: ViewModifier {
    @Environment(\.scenePhase) private var scenePhase
    @State private var isPresented = false

    func body(content: Content) -> some View {
        content
            .onAppear(perform: register)
            .onChange(of: scenePhase) { _, phase in
                if phase == .active { register() }
            }
            .sheet(isPresented: $isPresented) {
                RemoteFolderPicker()
                    .presentedControlDefaults()
            }
    }

    private func register() {
        let presented = $isPresented
        AppLifecycleHooks.openProject = { presented.wrappedValue = true }
    }
}

extension View {
    /// Host the iPad's Open Project picker on this view's window.
    func remoteProjectPicker() -> some View {
        modifier(RemoteProjectPickerPresenter())
    }
}
#endif
