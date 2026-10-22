import SwiftUI
import UIKit

// MARK: - Theme

/// Dark workspace palette shared with the desktop Point Guard surfaces.
enum Theme {
    static let ground = Color(hex: 0x18181B)
    static let chrome = Color(hex: 0x121214)
    static let surface = Color(hex: 0x222226)
    static let raised = Color(hex: 0x2A2A2F)
    static let bubble = Color(hex: 0x26262B)
    static let ink = Color(hex: 0xECECEE)
    static let muted = Color(hex: 0xA1A1AA)
    static let faint = Color(hex: 0x7C7C85)
    static let line = Color.white.opacity(0.06)
    static let accent = Color(hex: 0xFF7A1A)
    static let accentText = Color(hex: 0xFF8A3D)
    static let online = Color(hex: 0x30D158)
    static let offline = Color(hex: 0xFF453A)
    static let warning = Color(hex: 0xFFB340)
    static let danger = Color(hex: 0xFF7A70)
    static let mono = Font.system(.caption, design: .monospaced)
    static let terminalBackground = UIColor(red: 0x18 / 255, green: 0x18 / 255, blue: 0x1B / 255, alpha: 1)
}

extension Color {
    init(hex: UInt32) {
        self.init(.sRGB, red: Double((hex >> 16) & 0xFF) / 255, green: Double((hex >> 8) & 0xFF) / 255, blue: Double(hex & 0xFF) / 255, opacity: 1)
    }
}

/// Colors for one agent signal: the row dot and the pill beside the name.
struct SignalStyle {
    let dot: Color
    let pillFill: Color
    let pillText: Color
    static func of(_ label: String, available: Bool = true) -> SignalStyle {
        guard available else { return SignalStyle(dot: Color(hex: 0x6A6A72), pillFill: Color.white.opacity(0.07), pillText: Theme.muted) }
        switch label {
        case "Permission", "Question": return SignalStyle(dot: Color(hex: 0xFF9F0A), pillFill: Color(hex: 0xFF9F0A).opacity(0.16), pillText: Theme.warning)
        case "Working": return SignalStyle(dot: Color(hex: 0x0A84FF), pillFill: Color(hex: 0x0A84FF).opacity(0.18), pillText: Color(hex: 0x7AB6FF))
        case "Finished": return SignalStyle(dot: Theme.online, pillFill: Theme.online.opacity(0.14), pillText: Color(hex: 0x5DD879))
        case "Error": return SignalStyle(dot: Theme.offline, pillFill: Theme.offline.opacity(0.16), pillText: Theme.danger)
        default: return SignalStyle(dot: Color(hex: 0x6A6A72), pillFill: Color.white.opacity(0.07), pillText: Color(hex: 0xA8A8B0))
        }
    }
}

extension ControlledAgent {
    var displayName: String { name.isEmpty ? id : name }
    var phaseLabel: String? { phase?.replacingOccurrences(of: "_", with: " ") }
    var shortIdleLabel: String? { idleSeconds == nil ? nil : idleLabel }
    func tone(available: Bool) -> Color {
        guard available else { return Color(uiColor: .tertiaryLabel) }
        if state == "broken" { return Theme.offline }
        let label = signalPresentation(available: available).label
        if label != AgentSignalPresentation.unknown.label { return SignalStyle.of(label).dot }
        if phase == "busy" || phase == "has_draft" { return Color(hex: 0x0A84FF) }
        if phase == "dialog" { return Color(hex: 0xFF9F0A) }
        return Color(hex: 0x6A6A72)
    }
}

struct StatusDot: View {
    let color: Color
    var size: CGFloat = 7
    var body: some View { Circle().fill(color).frame(width: size, height: size).accessibilityHidden(true) }
}

/// One-line notice used instead of free-floating paragraphs of status text.
struct NoticeBanner: View {
    let text: String
    var systemImage = "exclamationmark.triangle.fill"
    var tint: Color = Theme.warning
    var actionTitle: String? = nil
    var action: (() -> Void)? = nil
    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: systemImage).font(.footnote).foregroundStyle(tint).accessibilityHidden(true)
            Text(text).font(.footnote).foregroundStyle(Theme.ink).textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let actionTitle, let action {
                Button(actionTitle, action: action).font(.footnote.weight(.semibold)).foregroundStyle(Theme.accentText).buttonStyle(.borderless)
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 9)
        .background(tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .accessibilityElement(children: .combine)
    }
}

/// The ringed PurePoint mark used on the Point Guard and Connections heroes.
struct LogoRing: View {
    var size: CGFloat = 64
    var body: some View {
        Image("PurePointLogo").resizable().scaledToFit().frame(width: size * 0.56, height: size * 0.56)
            .frame(width: size, height: size)
            .background(Color(hex: 0x1F1F23), in: Circle())
            .overlay(Circle().stroke(Color.white.opacity(0.09), lineWidth: 1))
            .background(Circle().fill(Theme.ground.opacity(0.9)).padding(-size * 0.08))
            .accessibilityHidden(true)
    }
}

struct AgentStatePill: View {
    let agent: ControlledAgent
    let available: Bool
    var body: some View {
        TimelineView(.periodic(from: .now, by: 1)) { context in
            let state = agent.signalPresentation(available: available, now: context.date)
            if state.label != AgentSignalPresentation.unknown.label {
                let style = SignalStyle.of(state.label, available: available)
                Text(state.label)
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(style.pillText)
                    .padding(.horizontal, 5).padding(.vertical, 1)
                    .background(style.pillFill, in: RoundedRectangle(cornerRadius: 4))
                    .fixedSize(horizontal: true, vertical: false)
                    .accessibilityLabel("Agent state: \(state.accessibilityDescription)")
            }
        }
    }
}

// MARK: - Workspace actions

/// Where a New, Rename or Stop applies. The project root has no worktree.
struct WorkspaceTarget: Identifiable, Equatable {
    let host: String
    let machine: String
    let machineName: String
    let project: String
    let projectName: String
    var worktree: String? = nil
    var branch: String? = nil
    var id: String { WorkspaceLocation.key([host, machine, project, worktree ?? ""]) }
    var path: String { [machineName, projectName, branch ?? "root"].joined(separator: " · ") }
}

/// One launch or management request. It is saved before dispatch and kept while its outcome is uncertain.
struct QuickLaunch: Codable, Identifiable, Equatable {
    let id: String
    let host: String
    let machine: String
    let machineName: String
    let project: String
    let projectName: String
    let worktree: String?
    let branch: String?
    let kind: String
    let label: String
    var agentType: String? = nil
    var presetId: String? = nil
    var name: String? = nil
    var agentId: String? = nil
    var state = "pending"
    var message: String? = nil
    /// Set once the person refreshed the authoritative workspace after an uncertain outcome.
    var reviewed = false
    var opensAgent: Bool { kind == "spawn" || kind == "terminal" }
    /// Launches that create work; an unconfirmed one must be reviewed before another runs in the same place.
    var createsWork: Bool { opensAgent || kind == "createWorktree" }
    /// Succeeded or failed are final; anything else may still have committed on the computer.
    var settled: Bool { state == "succeeded" || state == "failed" }
    var clearable: Bool { settled || reviewed }
    var where_: String { [machineName, projectName, branch ?? "root"].joined(separator: " / ") }
    var title: String {
        switch kind {
        case "spawn", "terminal": return "Starting \(label)"
        case "createWorktree": return "Creating \(name ?? "worktree")"
        case "rename": return "Renaming \(label)"
        case "stop": return "Stopping \(label)"
        case "removeProject": return "Removing \(label)"
        default: return label
        }
    }
    var fields: [String: Any] {
        var fields: [String: Any] = ["operationId": id, "machine": machine, "projectId": project]
        if opensAgent {
            fields["target"] = worktree == nil ? "root" : "worktree"
            if let worktree { fields["worktreeId"] = worktree }
        }
        if kind == "spawn" {
            fields["agentType"] = agentType ?? ""; fields["prompt"] = ""; fields["planMode"] = false
            if let presetId { fields["presetId"] = presetId }
        }
        if let name, kind == "createWorktree" || kind == "rename" { fields["name"] = name }
        if let agentId { fields["agentId"] = agentId }
        if kind == "stop" || kind == "removeProject" { fields["confirm"] = true }
        return fields
    }
}

/// Runs one-tap workspace operations. Every request is persisted before the first dispatch,
/// and an unconfirmed one stays visible until the person checks or dismisses it; nothing is resent.
@MainActor final class WorkspaceActions: ObservableObject {
    @Published private(set) var launches: [QuickLaunch] = []
    @Published var review: QuickLaunch?
    @Published var failure: String?
    private let defaults: UserDefaults
    private static let key = "pi.quickLaunch.v1"

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
        if let data = defaults.data(forKey: Self.key), let saved = try? JSONDecoder().decode([QuickLaunch].self, from: data) {
            // A launch still pending at relaunch was never confirmed.
            launches = saved.map { var launch = $0; if launch.state == "pending" { launch.state = "uncertain"; launch.message = "PurePoint closed before the computer confirmed." }; return launch }
        }
    }

    /// Opens the review sheet after any sheet or alert that triggered it has finished closing.
    func presentReview(_ launch: QuickLaunch) {
        Task { try? await Task.sleep(nanoseconds: 450_000_000); review = launches.first { $0.id == launch.id } }
    }
    var unresolved: [QuickLaunch] { launches.filter { $0.state != "pending" } }
    /// The in-flight or unreviewed request that blocks another launch at this place, if any.
    func blocking(_ target: WorkspaceTarget) -> QuickLaunch? {
        launches.first { $0.createsWork && !$0.clearable && $0.host == target.host && $0.machine == target.machine && $0.project == target.project && $0.worktree == target.worktree }
    }
    func starting(machine: String, project: String, worktree: String?) -> [QuickLaunch] {
        launches.filter { $0.state == "pending" && $0.opensAgent && $0.machine == machine && $0.project == project && $0.worktree == worktree }
    }

    private func save() -> Bool {
        guard let data = try? JSONEncoder().encode(launches) else { return false }
        defaults.set(data, forKey: Self.key)
        return defaults.data(forKey: Self.key) == data
    }
    private func update(_ launch: QuickLaunch) {
        if let index = launches.firstIndex(where: { $0.id == launch.id }) { launches[index] = launch }
        _ = save()
    }
    func dismiss(_ launch: QuickLaunch) {
        launches.removeAll { $0.id == launch.id }
        if review?.id == launch.id { review = nil }
        _ = save()
    }

    func run(_ launch: QuickLaunch, model: AgentControlModel, open: @escaping (AgentMachine, ControlledAgent) -> Void) {
        guard model.canManage, model.bridgeIdentity == launch.host else {
            failure = "Launching, renaming and stopping need Full access for this phone, set on the computer you paired with."; return
        }
        if launch.createsWork, let blocker = blocking(WorkspaceTarget(host: launch.host, machine: launch.machine, machineName: launch.machineName, project: launch.project, projectName: launch.projectName, worktree: launch.worktree)) {
            if blocker.state == "pending" { failure = "\(blocker.title) is still in progress here." } else { presentReview(blocker) }
            return
        }
        launches.append(launch)
        guard save() else {
            launches.removeAll { $0.id == launch.id }
            failure = "Could not save the request on this iPhone. Nothing was sent."; return
        }
        let context = model.contextToken
        Task {
            var result = launch
            do {
                let receipt = try await model.workspaceRequest("workspace." + launch.kind, fields: launch.fields)
                result.state = receipt.state; result.message = receipt.message
                if receipt.state == "succeeded" {
                    dismiss(result)
                    if launch.opensAgent, let agentId = receipt.detail?.agentId, model.contextToken == context,
                       let machine = model.machines.first(where: { $0.id == launch.machine }),
                       let agent = machine.agents.first(where: { $0.id == agentId }) { open(machine, agent) }
                    return
                }
                if receipt.state == "failed" {
                    dismiss(result); failure = receipt.message ?? "\(launch.title) failed."; return
                }
            } catch {
                result.state = "uncertain"; result.message = error.localizedDescription
                if error.localizedDescription.contains("workspace_duplicate"), let status = try? await model.operationStatus(launch.id) {
                    result.state = status.state; result.message = status.message
                }
            }
            update(result)
            review = result
        }
    }

    /// Refreshes the authoritative workspace; only a complete refresh lets the person clear the request.
    /// Status and inventory come from the connected bridge, so they only describe a request made through it.
    static let otherHostMessage = "This request was made through another paired computer. Select it in Connections to review it."

    func reviewWorkspace(_ launch: QuickLaunch, model: AgentControlModel) async {
        guard model.bridgeIdentity == launch.host else { failure = Self.otherHostMessage; return }
        guard await model.refreshWorkspaceForReview(machine: launch.machine, project: launch.project),
              model.bridgeIdentity == launch.host else {
            failure = "A complete, current workspace could not be loaded. Reconnect or retry before launching here again."; return
        }
        var updated = launch; updated.reviewed = true
        update(updated); if review?.id == launch.id { review = updated }
    }

    func checkStatus(_ launch: QuickLaunch, model: AgentControlModel) async {
        guard model.bridgeIdentity == launch.host else { failure = Self.otherHostMessage; return }
        do {
            let status = try await model.operationStatus(launch.id)
            guard model.bridgeIdentity == launch.host else { failure = Self.otherHostMessage; return }
            var updated = launch; updated.state = status.state; updated.message = status.message
            update(updated); if review?.id == launch.id { review = updated }
        } catch { failure = error.localizedDescription }
    }
}

/// Text-field alerts and the stop confirmation shared by the sidebar and the agent screen.
struct AgentManagement: ViewModifier {
    @ObservedObject var model: AgentControlModel
    @ObservedObject var actions: WorkspaceActions
    @Binding var renaming: AgentShortcut?
    @Binding var stopping: AgentShortcut?
    @Binding var creatingWorktree: WorkspaceTarget?
    let open: (AgentMachine, ControlledAgent) -> Void
    @State private var name = ""

    func body(content: Content) -> some View {
        content
            .alert("Rename agent", isPresented: Binding(get: { renaming != nil }, set: { if !$0 { renaming = nil } })) {
                TextField("Agent name", text: $name)
                Button("Cancel", role: .cancel) { renaming = nil }
                Button("Rename") {
                    if let entry = renaming, let launch = Self.request("rename", entry, model: model, name: name) { actions.run(launch, model: model, open: open) }
                    renaming = nil
                }.disabled(!Self.validName(name))
            } message: { Text(renaming.map { "\($0.agent.displayName) on \($0.machine.name)" } ?? "") }
            .alert("New worktree", isPresented: Binding(get: { creatingWorktree != nil }, set: { if !$0 { creatingWorktree = nil } })) {
                TextField("Worktree name", text: $name)
                Button("Cancel", role: .cancel) { creatingWorktree = nil }
                Button("Create") {
                    if let target = creatingWorktree {
                        actions.run(QuickLaunch(id: UUID().uuidString, host: target.host, machine: target.machine, machineName: target.machineName, project: target.project, projectName: target.projectName, worktree: nil, branch: nil, kind: "createWorktree", label: "Worktree", name: name.trimmingCharacters(in: .whitespacesAndNewlines)), model: model, open: open)
                    }
                    creatingWorktree = nil
                }.disabled(!Self.validName(name))
            } message: { Text(creatingWorktree.map { "In \($0.projectName) on \($0.machineName)" } ?? "") }
            .confirmationDialog(stopping.map { "Stop \($0.agent.displayName)?" } ?? "", isPresented: Binding(get: { stopping != nil }, set: { if !$0 { stopping = nil } }), titleVisibility: .visible) {
                Button("Stop agent", role: .destructive) {
                    if let entry = stopping, let launch = Self.request("stop", entry, model: model) { actions.run(launch, model: model, open: open) }
                    stopping = nil
                }
            } message: {
                Text(stopping.map { "On \($0.machine.name) in \(Self.projectName($0)). Other agents keep running." } ?? "")
            }
            .onChange(of: renaming?.id) { _, _ in name = renaming?.agent.displayName ?? "" }
            .onChange(of: creatingWorktree?.id) { _, _ in name = "" }
    }

    static func validName(_ name: String) -> Bool {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        return !trimmed.isEmpty && trimmed.utf8.count <= 80 && !trimmed.unicodeScalars.contains { CharacterSet.controlCharacters.contains($0) }
    }
    static func projectName(_ entry: AgentShortcut) -> String {
        entry.agent.projectRoot.map { URL(fileURLWithPath: $0).lastPathComponent } ?? "an unknown project"
    }
    static func request(_ kind: String, _ entry: AgentShortcut, model: AgentControlModel, name: String? = nil) -> QuickLaunch? {
        guard let project = entry.agent.projectRoot else { return nil }
        let projectName = entry.machine.registeredProjects?.first { $0.id == project }?.name ?? URL(fileURLWithPath: project).lastPathComponent
        let branch = entry.machine.worktrees?.first { $0.id == entry.agent.worktreeId }?.branch
        return QuickLaunch(id: UUID().uuidString, host: entry.scope.host, machine: entry.machine.id, machineName: entry.machine.name, project: project, projectName: projectName, worktree: entry.agent.worktreeId, branch: branch, kind: kind, label: entry.agent.displayName, name: name?.trimmingCharacters(in: .whitespacesAndNewlines), agentId: entry.agent.id)
    }
}

// MARK: - New dialog

/// One tap launches: the place is already known from the row that opened it.
struct LaunchDialog: View {
    @ObservedObject var model: AgentControlModel
    @ObservedObject var actions: WorkspaceActions
    let target: WorkspaceTarget
    let open: (AgentMachine, ControlledAgent) -> Void
    var newWorktree: (() -> Void)? = nil
    @Environment(\.dismiss) private var dismiss
    @State private var catalog: WorkspaceCatalog?
    @State private var error: String?
    @State private var loading = false

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("New").font(.headline)
                    Text(target.path).font(Theme.mono).foregroundStyle(Theme.muted).lineLimit(1).truncationMode(.middle)
                }
                Spacer()
                Button { dismiss() } label: {
                    Image(systemName: "xmark").font(.footnote.weight(.bold)).foregroundStyle(Theme.muted)
                        .frame(width: 30, height: 30).background(Theme.raised, in: Circle()).frame(width: 44, height: 44)
                }.accessibilityLabel("Close")
            }.padding(.leading, 16).padding(.trailing, 6).padding(.top, 14)
            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    if let error { NoticeBanner(text: error, actionTitle: "Retry") { Task { await load() } }.padding(.horizontal, 12).padding(.top, 10) }
                    if let blocker = actions.blocking(target) {
                        NoticeBanner(text: blocker.state == "pending" ? "\(blocker.title) is still in progress here." : "\(blocker.title) here couldn't be confirmed. Review it before launching here again.", systemImage: "questionmark.circle.fill", actionTitle: blocker.state == "pending" ? nil : "Review") {
                            dismiss(); actions.presentReview(blocker)
                        }.padding(.horizontal, 12).padding(.top, 10)
                    }
                    if !model.canManage {
                        NoticeBanner(text: "Launching needs Full access for this phone, set on the computer you paired with.", systemImage: "lock.fill", tint: Theme.muted).padding(.horizontal, 12).padding(.top, 10)
                    }
                    if loading && catalog == nil { ProgressView().frame(maxWidth: .infinity).padding(.top, 30) }
                    if let catalog {
                        let agents = catalog.agents.filter(\.available)
                        if !agents.isEmpty {
                            section("AGENT") {
                                ForEach(Array(agents.enumerated()), id: \.element.id) { index, agent in
                                    option(agent.name, systemImage: "sparkle", highlighted: index == 0) { launch("spawn", label: agent.name, agentType: agent.id) }
                                }
                            }
                        }
                        let presets = catalog.presets.filter { preset in agents.contains { $0.id == preset.agentType } }
                        if !presets.isEmpty {
                            section("PRESETS") {
                                ForEach(presets) { preset in
                                    option(preset.name, subtitle: agents.first { $0.id == preset.agentType }?.name, systemImage: "checklist") {
                                        launch("spawn", label: preset.name, agentType: preset.agentType, preset: preset.id)
                                    }
                                }
                            }
                        }
                        if catalog.terminalAvailable || newWorktree != nil {
                            section("OTHER") {
                                if catalog.terminalAvailable { option("Terminal", systemImage: "terminal") { launch("terminal", label: "Terminal") } }
                                if let newWorktree { option("New worktree…", systemImage: "arrow.triangle.branch") { dismiss(); newWorktree() } }
                            }
                        }
                    }
                }.padding(.bottom, 24)
            }
        }
        .background(Theme.ground)
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
        .task { await load() }
    }

    private func load() async {
        loading = true; defer { loading = false }
        do { catalog = try await model.catalog(machine: target.machine, project: target.project); error = nil }
        catch { self.error = error.localizedDescription }
    }
    private func launch(_ kind: String, label: String, agentType: String? = nil, preset: String? = nil) {
        actions.run(QuickLaunch(id: UUID().uuidString, host: target.host, machine: target.machine, machineName: target.machineName, project: target.project, projectName: target.projectName, worktree: target.worktree, branch: target.branch, kind: kind, label: label, agentType: agentType, presetId: preset), model: model, open: open)
        dismiss()
    }
    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.system(size: 11, weight: .semibold)).tracking(0.4).foregroundStyle(Theme.muted).padding(.horizontal, 16)
            VStack(spacing: 0) { content() }
                .background(Theme.surface, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .padding(.horizontal, 12)
        }.padding(.top, 16)
    }
    private func option(_ title: String, subtitle: String? = nil, systemImage: String, highlighted: Bool = false, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: systemImage).font(.callout.weight(.medium))
                    .foregroundStyle(highlighted ? Theme.ground : Theme.ink)
                    .frame(width: 32, height: 32)
                    .background(highlighted ? Theme.accent : Theme.raised, in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                VStack(alignment: .leading, spacing: 2) {
                    Text(title).font(.body.weight(.semibold))
                    if let subtitle { Text(subtitle).font(.footnote).foregroundStyle(Theme.muted) }
                }
                Spacer()
            }.padding(.horizontal, 14).frame(minHeight: 54).contentShape(Rectangle())
        }
        .buttonStyle(.plain).disabled(!model.canManage || actions.blocking(target) != nil)
        .overlay(alignment: .top) { Rectangle().fill(Theme.line).frame(height: 1).padding(.leading, 58) }
    }
}

/// Shown when a request could not be confirmed. Nothing is resent automatically.
struct LaunchStatusSheet: View {
    @ObservedObject var model: AgentControlModel
    @ObservedObject var actions: WorkspaceActions
    let launch: QuickLaunch
    @Environment(\.dismiss) private var dismiss
    @State private var busy = false
    var body: some View {
        NavigationStack {
            List {
                Section("Requested") {
                    LabeledContent("Where") { Text(launch.where_).font(Theme.mono) }
                    LabeledContent(launch.opensAgent ? "Agent" : "Target") { Text(launch.label) }
                    if let name = launch.name { LabeledContent("Name") { Text(name) } }
                }.listRowBackground(Theme.surface)
                Section("Status") {
                    if model.bridgeIdentity != launch.host {
                        Text(WorkspaceActions.otherHostMessage).font(.subheadline).foregroundStyle(Theme.warning)
                    }
                    VStack(alignment: .leading, spacing: 8) {
                        Label(launch.state.capitalized, systemImage: launch.state == "succeeded" ? "checkmark.circle" : "exclamationmark.circle")
                            .font(.subheadline.weight(.semibold)).foregroundStyle(launch.state == "succeeded" ? Theme.online : Theme.warning)
                        Text(launch.message ?? "The computer hasn't confirmed this yet. Check status and refresh the workspace before starting anything else.").font(.subheadline).textSelection(.enabled)
                        Text("Operation \(launch.id)").font(Theme.mono).foregroundStyle(Theme.muted).textSelection(.enabled)
                    }.padding(.vertical, 4)
                    Button("Check operation status") { busy = true; Task { await actions.checkStatus(launch, model: model); busy = false } }.disabled(busy || model.bridgeIdentity != launch.host)
                    Button(launch.reviewed ? "Workspace refreshed" : "Refresh workspace") { busy = true; Task { await actions.reviewWorkspace(launch, model: model); busy = false } }
                        .disabled(busy || launch.reviewed || model.bridgeIdentity != launch.host)
                    Button(launch.createsWork ? "Clear and allow new launches here" : "Clear", role: .destructive) { actions.dismiss(launch); dismiss() }
                        .disabled(!launch.clearable)
                }.listRowBackground(Theme.surface)
                Section { } footer: { Text("Nothing is resent after reconnecting. An unknown status doesn't mean it failed: check status or refresh the workspace before clearing it, so a launch that did commit isn't started twice.") }
            }
            .scrollContentBackground(.hidden).background(Theme.ground)
            .navigationTitle(launch.title).navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Close") { dismiss() } } }
        }
    }
}

// MARK: - Home (the desktop sidebar)

struct HomeView: View {
    @ObservedObject var model: AgentControlModel
    @ObservedObject var actions: WorkspaceActions
    let pointGuardStatus: String
    let pointGuardTone: Color
    let openPointGuard: () -> Void
    let openAgent: (AgentMachine, ControlledAgent) -> Void
    let openConnections: () -> Void
    @State private var collapsed: Set<String> = []
    @State private var launchTarget: WorkspaceTarget?
    @State private var creatingWorktree: WorkspaceTarget?
    @State private var renaming: AgentShortcut?
    @State private var stopping: AgentShortcut?
    @State private var removing: WorkspaceTarget?

    var body: some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 0) {
                pointGuardRow
                notices
                if model.permission != .off {
                    sectionHeader("MACHINES")
                    ForEach(model.machines) { machine in machineTree(machine) }
                    if model.machines.isEmpty && !model.loading && model.connected {
                        Text("No machines. Pull to refresh, or register a project with pu on your computer.").font(.footnote).foregroundStyle(Theme.muted).padding(.horizontal, 12).padding(.vertical, 10)
                    }
                    if model.loading && model.machines.isEmpty { ProgressView().frame(maxWidth: .infinity).padding(.top, 30) }
                }
                ForEach(actions.unresolved.filter { $0.host == model.bridgeIdentity }) { launch in
                    NoticeBanner(text: "\(launch.title) couldn't be confirmed.", systemImage: "questionmark.circle.fill", actionTitle: "Review") { actions.review = launch }.padding(.horizontal, 4).padding(.top, 8)
                }
            }.padding(.horizontal, 8).padding(.bottom, 16)
        }
        .background(Theme.ground)
        .refreshable { await model.refresh() }
        .safeAreaInset(edge: .bottom, spacing: 0) { footer }
        .toolbar(.hidden, for: .navigationBar)
        .sheet(item: $launchTarget) { target in
            LaunchDialog(model: model, actions: actions, target: target, open: openAgent,
                         newWorktree: target.worktree == nil ? { creatingWorktree = target } : nil)
        }
        .modifier(AgentManagement(model: model, actions: actions, renaming: $renaming, stopping: $stopping, creatingWorktree: $creatingWorktree, open: openAgent))
        .confirmationDialog(removing.map { "Remove \($0.projectName) from \($0.machineName)?" } ?? "", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
            Button("Remove from \(removing?.machineName ?? "machine")", role: .destructive) {
                if let target = removing {
                    actions.run(QuickLaunch(id: UUID().uuidString, host: target.host, machine: target.machine, machineName: target.machineName, project: target.project, projectName: target.projectName, worktree: nil, branch: nil, kind: "removeProject", label: target.projectName), model: model, open: openAgent)
                }
                removing = nil
            }
        } message: {
            Text(removing.map { "It disappears from PurePoint on every device connected to \($0.machineName). Running agents are suspended and resume when the project is added again. Files, including .pu/, are not changed." } ?? "")
        }
        .onChange(of: model.bridgeIdentity) { _, _ in collapsed = []; launchTarget = nil; renaming = nil; stopping = nil; removing = nil }
    }

    // MARK: Rows

    private var pointGuardRow: some View {
        Button(action: openPointGuard) {
            HStack(spacing: 10) {
                Image("PurePointLogo").resizable().scaledToFit().frame(width: 22, height: 22).accessibilityHidden(true)
                Text("Point Guard").font(.system(size: 16, weight: .semibold))
                Spacer()
                HStack(spacing: 5) { StatusDot(color: pointGuardTone, size: 6); Text(pointGuardStatus) }
                    .font(.caption).foregroundStyle(Theme.muted)
                Image(systemName: "chevron.right").font(.caption.weight(.semibold)).foregroundStyle(Theme.faint)
            }.padding(.horizontal, 10).frame(minHeight: 46).contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityHint("Open Point Guard chat")
    }
    @ViewBuilder private var notices: some View {
        if !model.connected {
            NoticeBanner(text: "Bridge disconnected. Agents below are cached and can't be opened.", systemImage: "wifi.slash").padding(.horizontal, 4).padding(.top, 6)
        }
        if model.permission == .off {
            VStack(spacing: 10) {
                Image(systemName: "lock").font(.title2).foregroundStyle(Theme.muted)
                    .frame(width: 56, height: 56).background(Color(hex: 0x1F1F23), in: Circle())
                Text("Terminal access is off. Enable it on the computer you paired with (Connect phone on a Mac, pu phone on Linux).")
                    .font(.subheadline).foregroundStyle(Theme.muted).multilineTextAlignment(.center)
            }.frame(maxWidth: .infinity).padding(.horizontal, 32).padding(.top, 48).accessibilityElement(children: .combine)
        }
        if let error = model.error { NoticeBanner(text: error).padding(.horizontal, 4).padding(.top, 6) }
        if let failure = actions.failure {
            NoticeBanner(text: failure, actionTitle: "OK") { actions.failure = nil }.padding(.horizontal, 4).padding(.top, 6)
        }
    }
    private func sectionHeader(_ title: String) -> some View {
        Text(title).font(.system(size: 11, weight: .semibold)).tracking(0.4).foregroundStyle(Theme.muted)
            .padding(.horizontal, 12).padding(.top, 16).padding(.bottom, 4).accessibilityAddTraits(.isHeader)
    }
    private func key(_ parts: String...) -> String { WorkspaceLocation.key([model.bridgeIdentity] + parts) }
    private func toggle(_ key: String) { withAnimation(.easeOut(duration: 0.15)) { if collapsed.contains(key) { collapsed.remove(key) } else { collapsed.insert(key) } } }
    private func disclosure(_ key: String, label: String) -> some View {
        Button { toggle(key) } label: {
            Image(systemName: "chevron.down").font(.system(size: 10, weight: .bold)).foregroundStyle(Theme.faint)
                .rotationEffect(.degrees(collapsed.contains(key) ? -90 : 0)).frame(width: 26, height: 40).contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityLabel(label).accessibilityValue(collapsed.contains(key) ? "Collapsed" : "Expanded")
    }
    private func plusButton(_ label: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: "plus").font(.footnote.weight(.semibold)).foregroundStyle(Theme.muted).frame(width: 40, height: 40).contentShape(Rectangle())
        }.buttonStyle(.plain).disabled(!model.canManage).accessibilityLabel(label)
    }

    @ViewBuilder private func machineTree(_ machine: AgentMachine) -> some View {
        let machineKey = key(machine.id)
        let online = machine.online && model.connected
        HStack(spacing: 8) {
            disclosure(machineKey, label: machine.name)
            StatusDot(color: !model.connected ? Color(hex: 0x6A6A72) : machine.online ? Theme.online : Theme.offline)
            Image(systemName: "desktopcomputer").font(.footnote).foregroundStyle(Theme.muted)
            Text(machine.name).font(.subheadline.weight(.semibold)).lineLimit(1)
            Spacer()
            Text(!model.connected ? "Unavailable" : machine.online ? "Online" : "Offline").font(.caption).foregroundStyle(Theme.muted)
        }.padding(.trailing, 12).frame(minHeight: 40).contentShape(Rectangle()).onTapGesture { toggle(machineKey) }
        .opacity(model.connected ? 1 : 0.5).accessibilityElement(children: .combine).accessibilityAddTraits(.isButton)
        if !collapsed.contains(machineKey) {
            if !machine.online, let error = machine.error {
                Text(error).font(.footnote).foregroundStyle(Theme.muted).padding(.leading, 34).padding(.trailing, 12).padding(.bottom, 6)
            }
            if machine.metadataStatus != nil && machine.metadataStatus != "complete" {
                NoticeBanner(text: machine.metadataError ?? "Project metadata incomplete. Refresh to retry.").padding(.leading, 28).padding(.bottom, 4)
            }
            ForEach(machine.registeredProjects ?? []) { project in projectTree(machine, project, online: online) }
            if machine.registeredProjects == nil {
                // Older bridges report agents without project metadata: group them by project root.
                ForEach(machine.projects) { group in
                    Text(group.name).font(.subheadline.weight(.semibold)).padding(.leading, 42).frame(minHeight: 40)
                    ForEach(group.agents) { agent in agentRow(machine, agent, indent: 58) }
                }
            }
        }
    }

    @ViewBuilder private func projectTree(_ machine: AgentMachine, _ project: WorkspaceProject, online: Bool) -> some View {
        let projectKey = key(machine.id, project.id)
        let available = online && project.available
        let target = WorkspaceTarget(host: model.bridgeIdentity, machine: machine.id, machineName: machine.name, project: project.id, projectName: project.name)
        HStack(spacing: 8) {
            disclosure(projectKey, label: project.name)
            Image(systemName: "folder").font(.footnote).foregroundStyle(Theme.muted)
            Text(project.name).font(.subheadline.weight(.semibold)).lineLimit(1)
            if !project.available { Text("Unavailable").font(.caption2).foregroundStyle(Theme.faint) }
            Spacer()
            plusButton("New in \(project.name)") { launchTarget = target }.disabled(!available || !model.canManage)
        }.padding(.leading, 16).frame(minHeight: 40).contentShape(Rectangle()).onTapGesture { toggle(projectKey) }
        .contextMenu {
            Button("New…", systemImage: "plus") { launchTarget = target }.disabled(!available || !model.canManage)
            Button("New worktree…", systemImage: "arrow.triangle.branch") { creatingWorktree = target }.disabled(!available || !model.canManage)
            Divider()
            // Removes it on the machine for every device; older daemons cannot.
            Button("Remove from \(machine.name)…", systemImage: "trash", role: .destructive) { removing = target }
                .disabled(!online || !model.canManage || (machine.protocolVersion ?? 0) < 13)
        }
        if let error = project.error { Text(error).font(.caption).foregroundStyle(Theme.warning).padding(.leading, 58).padding(.bottom, 4) }
        if !collapsed.contains(projectKey) {
            let worktrees = (machine.worktrees ?? []).filter { $0.projectId == project.id }
            ForEach(machine.agents.filter { $0.projectRoot == project.id && $0.worktreeId == nil }) { agent in agentRow(machine, agent, indent: 58) }
            startingRows(machine: machine.id, project: project.id, worktree: nil, indent: 58)
            ForEach(worktrees) { worktree in worktreeTree(machine, project, worktree, available: available) }
            let orphans = machine.agents.filter { agent in agent.projectRoot == project.id && agent.worktreeId != nil && !worktrees.contains { $0.id == agent.worktreeId } }
            if !orphans.isEmpty {
                Label("Worktree unavailable", systemImage: "questionmark.folder").font(.footnote).foregroundStyle(Theme.muted).padding(.leading, 58).frame(minHeight: 32)
                ForEach(orphans) { agent in agentRow(machine, agent, indent: 74) }
            }
        }
    }

    @ViewBuilder private func worktreeTree(_ machine: AgentMachine, _ project: WorkspaceProject, _ worktree: WorkspaceWorktree, available: Bool) -> some View {
        let worktreeKey = key(machine.id, project.id, worktree.id)
        let agents = machine.agents.filter { $0.projectRoot == project.id && $0.worktreeId == worktree.id }
        let active = worktree.status.isEmpty || worktree.status.lowercased() == "active"
        let target = WorkspaceTarget(host: model.bridgeIdentity, machine: machine.id, machineName: machine.name, project: project.id, projectName: project.name, worktree: worktree.id, branch: worktree.branch)
        HStack(spacing: 8) {
            disclosure(worktreeKey, label: worktree.branch)
            Image(systemName: "arrow.triangle.branch").font(.caption).foregroundStyle(Theme.muted)
            Text(worktree.branch.isEmpty ? worktree.name : worktree.branch).font(.subheadline).foregroundStyle(active ? Theme.ink : Theme.muted).lineLimit(1).truncationMode(.middle)
            if !active { Text(worktree.status.capitalized).font(.caption2).foregroundStyle(Theme.faint) }
            Spacer(minLength: 4)
            if !agents.isEmpty {
                Text("\(agents.count)").font(.system(size: 10.5, weight: .semibold)).foregroundStyle(Color(hex: 0xB4B4BC))
                    .frame(minWidth: 18, minHeight: 18).padding(.horizontal, 3).background(Color.white.opacity(0.08), in: RoundedRectangle(cornerRadius: 6))
                    .accessibilityLabel("\(agents.count) agents")
            }
            plusButton("New in \(worktree.branch)") { launchTarget = target }.disabled(!available || !active || !model.canManage)
        }.padding(.leading, 32).frame(minHeight: 40).contentShape(Rectangle()).onTapGesture { toggle(worktreeKey) }
        .contextMenu { Button("New…", systemImage: "plus") { launchTarget = target }.disabled(!available || !active || !model.canManage) }
        if !collapsed.contains(worktreeKey) {
            ForEach(agents) { agent in agentRow(machine, agent, indent: 74) }
            startingRows(machine: machine.id, project: project.id, worktree: worktree.id, indent: 74)
        }
    }

    private func startingRows(machine: String, project: String, worktree: String?, indent: CGFloat) -> some View {
        ForEach(actions.starting(machine: machine, project: project, worktree: worktree)) { launch in
            HStack(spacing: 9) {
                ProgressView().controlSize(.mini).tint(Theme.accentText)
                Text("Starting \(launch.label)…").font(.subheadline).foregroundStyle(Theme.muted)
                Spacer()
            }.padding(.leading, indent).frame(minHeight: 40).background(Color.white.opacity(0.05), in: RoundedRectangle(cornerRadius: 8))
        }
    }

    private func agentRow(_ machine: AgentMachine, _ agent: ControlledAgent, indent: CGFloat) -> some View {
        let available = model.connected && machine.online && machine.metadataStatus != "error"
        let entry = AgentShortcut(scope: AgentScope(host: model.bridgeIdentity, machine: machine.id, agent: agent.id), machine: machine, agent: agent)
        return Button { if available { openAgent(machine, agent) } } label: {
            HStack(spacing: 9) {
                StatusDot(color: agent.tone(available: available))
                Text(agent.displayName).font(.subheadline).foregroundStyle(available ? Theme.ink : Theme.muted).lineLimit(1)
                AgentStatePill(agent: agent, available: available)
                Spacer()
            }.padding(.leading, indent).padding(.trailing, 12).frame(minHeight: 40).contentShape(Rectangle())
        }
        .buttonStyle(.plain).disabled(!available)
        .accessibilityLabel([agent.displayName, machine.name, agent.agentType, agent.signalPresentation(available: available).accessibilityDescription, available ? agent.state : "Unavailable"].joined(separator: ", "))
        .contextMenu {
            Button("Rename…", systemImage: "pencil") { renaming = entry }.disabled(!model.canManage || agent.projectRoot == nil)
            Button("Stop agent…", systemImage: "stop.circle", role: .destructive) { stopping = entry }.disabled(!model.canManage || agent.projectRoot == nil)
        }
    }

    private var footer: some View {
        HStack {
            Button(action: openConnections) {
                Label("Connections", systemImage: "point.3.connected.trianglepath.dotted").font(.subheadline.weight(.medium)).foregroundStyle(Theme.ink).frame(minHeight: 44)
            }.buttonStyle(.plain)
            Spacer()
            Button { Task { await model.refresh() } } label: {
                Group { if model.loading { ProgressView() } else { Image(systemName: "arrow.clockwise") } }.foregroundStyle(Theme.muted).frame(width: 44, height: 44)
            }.buttonStyle(.plain).disabled(!model.connected || model.permission == .off).accessibilityLabel("Refresh")
        }
        .padding(.horizontal, 16).padding(.top, 2)
        // Fill down through the home-indicator area so rows don't scroll visibly beneath the footer.
        .background(Theme.ground.ignoresSafeArea(edges: .bottom).overlay(alignment: .top) { Rectangle().fill(Theme.line).frame(height: 1) })
    }
}

// MARK: - Agent screen

struct AgentScreen: View {
    @ObservedObject var model: AgentControlModel
    @ObservedObject var actions: WorkspaceActions
    let open: (AgentMachine, ControlledAgent) -> Void
    @State private var launchTarget: WorkspaceTarget?
    @State private var renaming: AgentShortcut?
    @State private var stopping: AgentShortcut?
    @State private var creatingWorktree: WorkspaceTarget?
    @State private var visible = false
    @Environment(\.dismiss) private var dismiss

    private var machine: AgentMachine? { model.machines.first { $0.id == model.selected?.machine } }
    private var entry: AgentShortcut? {
        guard let selected = model.selected, let machine, let agent = model.agent else { return nil }
        return AgentShortcut(scope: selected, machine: machine, agent: agent)
    }
    private var siblings: [ControlledAgent] {
        guard let machine, let agent = model.agent else { return [] }
        return machine.agents.filter { $0.projectRoot == agent.projectRoot && $0.worktreeId == agent.worktreeId }
    }
    private var target: WorkspaceTarget? {
        guard let machine, let agent = model.agent, let project = agent.projectRoot else { return nil }
        let worktree = machine.worktrees?.first { $0.id == agent.worktreeId }
        let projectName = machine.registeredProjects?.first { $0.id == project }?.name ?? URL(fileURLWithPath: project).lastPathComponent
        return WorkspaceTarget(host: model.bridgeIdentity, machine: machine.id, machineName: machine.name, project: project, projectName: projectName, worktree: worktree?.id, branch: worktree?.branch)
    }

    var body: some View {
        VStack(spacing: 0) {
            if siblings.count > 1 || target != nil { tabStrip }
            AgentControlView(model: model, screen: model.screenToken)
        }
        .background(Theme.ground)
        .toolbarBackground(Theme.chrome, for: .navigationBar).toolbarBackground(.visible, for: .navigationBar)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .principal) { principal }
            ToolbarItem(placement: .topBarTrailing) { options }
        }
        .sheet(item: $launchTarget) { target in LaunchDialog(model: model, actions: actions, target: target, open: open) }
        .modifier(AgentManagement(model: model, actions: actions, renaming: $renaming, stopping: $stopping, creatingWorktree: $creatingWorktree, open: open))
        .onAppear { visible = true }
        .onDisappear { visible = false }
        // Losing the selection (permission off, bridge change, agent stopped) returns to the sidebar.
        .onChange(of: model.selected) { _, selected in if selected == nil && visible { dismiss() } }
    }

    @ViewBuilder private var principal: some View {
        if let context = model.destinationContext {
            VStack(spacing: 1) {
                HStack(spacing: 6) {
                    StatusDot(color: model.agent?.tone(available: model.connected) ?? Color(hex: 0x6A6A72))
                    Text(context.agentName).font(.headline).lineLimit(1)
                    if let agent = model.agent { AgentStatePill(agent: agent, available: model.connected) }
                }
                Text([context.machineName, context.projectLabel, context.worktreeLabel == "Project root" ? "root" : context.worktreeLabel].joined(separator: " · "))
                    .font(.system(size: 10.5, design: .monospaced)).foregroundStyle(Theme.muted).lineLimit(1).truncationMode(.middle)
            }.frame(maxWidth: 250).accessibilityElement(children: .combine)
        }
    }

    private var options: some View {
        let token = model.screenToken
        return Menu {
            if let context = model.destinationContext {
                Section("\(context.projectLabel) · \(context.worktreeLabel)") {
                    Text(context.status)
                    Text(model.hasSize ? "Terminal \(model.cols) × \(model.rows)" : "Agent size unavailable")
                }
            }
            if model.permission == .control {
                Toggle(isOn: Binding(get: { model.rawTyping }, set: { model.setRawTyping($0, screen: token) })) {
                    Label("Raw typing", systemImage: "keyboard")
                }.disabled(!model.controlEnabled)
            }
            Button("Rename…", systemImage: "pencil") { renaming = entry }.disabled(!model.canManage || entry?.agent.projectRoot == nil)
            Button("Refresh", systemImage: "arrow.clockwise") { Task { await model.refresh() } }
            Section {
                Button("Stop agent…", systemImage: "stop.circle", role: .destructive) { stopping = entry }.disabled(!model.canManage || entry?.agent.projectRoot == nil)
            }
        } label: { Image(systemName: "ellipsis").frame(width: 32, height: 32) }
        .accessibilityLabel("Agent options")
    }

    private var tabStrip: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 0) {
                ForEach(siblings) { agent in
                    let selected = agent.id == model.selected?.agent
                    Button {
                        if !selected, let machine { open(machine, agent) }
                    } label: {
                        HStack(spacing: 6) {
                            Image(systemName: agent.agentType == "terminal" ? "terminal" : "sparkle").font(.caption2)
                                .foregroundStyle(agent.tone(available: model.connected))
                            Text(agent.displayName).font(.caption.weight(selected ? .semibold : .regular)).lineLimit(1)
                        }
                        .foregroundStyle(selected ? Theme.ink : Color(hex: 0xA8A8B0))
                        .padding(.horizontal, 12).frame(height: 34)
                        .background(selected ? Theme.ground : Color.clear)
                        .overlay(alignment: .top) { if selected { Rectangle().fill(Theme.accent).frame(height: 2) } }
                        .contentShape(Rectangle())
                    }.buttonStyle(.plain).accessibilityAddTraits(selected ? .isSelected : [])
                }
                if let target {
                    Button { launchTarget = target } label: {
                        Image(systemName: "plus").font(.footnote.weight(.semibold)).foregroundStyle(Theme.muted).frame(width: 40, height: 34)
                    }.buttonStyle(.plain).disabled(!model.canManage).accessibilityLabel("New in this worktree")
                }
            }
        }
        .frame(height: 34).background(Theme.chrome)
        .overlay(alignment: .bottom) { Rectangle().fill(Theme.line).frame(height: 1) }
    }
}

struct AgentControlView: View {
    @ObservedObject var model: AgentControlModel
    let screen: Int
    var body: some View {
        VStack(spacing: 0) {
            if let notice {
                NoticeBanner(text: notice).padding(.horizontal, 8).padding(.vertical, 6).background(Theme.ground)
            }
            AgentTerminalView(model: model).frame(maxWidth: .infinity, maxHeight: .infinity)
                .overlay(alignment: .topTrailing) {
                    if model.hasSize {
                        Text("\(model.cols)×\(model.rows)").font(.system(.caption2, design: .monospaced))
                            .foregroundStyle(Color.white.opacity(0.6))
                            .padding(.horizontal, 7).padding(.vertical, 2)
                            .background(Color.white.opacity(0.08), in: Capsule())
                            .padding(8).allowsHitTesting(false)
                            .accessibilityLabel("Agent terminal \(model.cols) columns by \(model.rows) rows")
                    }
                }
        }
        .background(Theme.ground)
        .safeAreaInset(edge: .bottom, spacing: 0) { AgentControlBar(model: model) }
        .onDisappear { model.closeScreen(screen: screen) }
    }
    // Only one banner at a time: errors win over display-accuracy warnings.
    private var notice: String? {
        if let error = model.error { return error }
        if model.screenMayBeInexact { return "Legacy daemon: replay may be incomplete and the screen size may be inexact." }
        if !model.hasSize && model.streamId != nil { return "Waiting for agent size. Display may be inexact until it arrives." }
        return nil
    }
}

/// Everything below the terminal: quick keys, delivery status and the prompt.
/// Kept to two compact rows so the terminal keeps most of the screen, even with the keyboard up.
private struct AgentControlBar: View {
    @ObservedObject var model: AgentControlModel
    @State private var showRecoveries = false
    private enum Field: Hashable { case prompt, raw }
    @FocusState private var field: Field?
    private var focused: Bool { field != nil }
    var body: some View {
        let token = model.screenToken
        VStack(alignment: .leading, spacing: 6) {
            // Saved recoveries stay reachable even when control is lost: copy, restore or dismiss.
            if !model.recoveries.isEmpty {
                Button { showRecoveries = true } label: {
                    Label("\(model.recoveries.count) saved prompt\(model.recoveries.count == 1 ? "" : "s")", systemImage: "tray.full")
                        .font(.caption.weight(.semibold)).foregroundStyle(Theme.accentText).frame(minHeight: 32)
                }.buttonStyle(.borderless).padding(.horizontal, 8)
            }
            if model.permission == .control {
                AgentQuickKeys(model: model)
                if let status { Text(status.text).font(.caption).foregroundStyle(status.warning ? Theme.warning : Theme.muted).lineLimit(2).textSelection(.enabled).padding(.horizontal, 8) }
                composerRow(token)
                if model.rawTyping && model.controlEnabled { rawRow(token) }
            } else {
                Label(model.sendExplanation ?? "Terminal access is view only. Enable control on the computer you paired with.", systemImage: "eye")
                    .font(.footnote).foregroundStyle(Theme.muted).frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                    .padding(.horizontal, 12).background(Theme.surface, in: RoundedRectangle(cornerRadius: 12))
            }
        }
        .padding(.horizontal, 8).padding(.top, 6).padding(.bottom, 8)
        .background(Theme.chrome)
        .overlay(alignment: .top) { Rectangle().fill(Theme.line).frame(height: 1) }
        .sheet(isPresented: $showRecoveries) { AgentPromptRecoveries(model: model, screen: token) }
    }
    private var status: (text: String, warning: Bool)? {
        if let outcome = model.composer.outcome {
            let text = (model.composer.deliveryUncertain ? "Delivery uncertain" : outcome.label) + (model.composer.reason.map { " · " + $0 } ?? "")
            return (text, outcome == .failed || outcome == .blocked || model.composer.deliveryUncertain)
        }
        if model.composer.pending != nil { return ("Sending…", false) }
        // Routine agent readiness (busy, dialog) is explained once there is a draft to send;
        // storage, connection and permission problems are always shown.
        if let explanation = model.sendExplanation, !model.draft.isEmpty || explanation != model.agent?.promptBlockReason {
            return (explanation, false)
        }
        return nil
    }
    private func composerRow(_ token: Int) -> some View {
        HStack(alignment: .bottom, spacing: 6) {
            if focused {
                Button { field = nil } label: {
                    Image(systemName: "keyboard.chevron.compact.down").font(.body)
                        .foregroundStyle(Theme.muted).frame(width: 36, height: 40).contentShape(Rectangle())
                }.buttonStyle(.plain).accessibilityLabel("Hide keyboard")
            }
            TextField(
                "Message \(model.agent?.displayName ?? "agent")", text: Binding(get: { model.draft }, set: { model.editDraft($0, screen: token) }),
                axis: .vertical
            )
            .lineLimit(1...4).autocorrectionDisabled(false).focused($field, equals: .prompt)
            .padding(.horizontal, 14).padding(.vertical, 9)
            .background(Theme.surface, in: RoundedRectangle(cornerRadius: 20, style: .continuous))
            .overlay(RoundedRectangle(cornerRadius: 20, style: .continuous).stroke(Color.white.opacity(0.07)))
            .accessibilityLabel("Agent prompt. Return inserts a new line.")
            Button { model.sendPrompt(screen: token) } label: {
                Image(systemName: "arrow.up").font(.body.weight(.semibold))
                    .foregroundStyle(model.canSend ? Theme.ground : Color(hex: 0x85858D))
                    .frame(width: 38, height: 38)
                    .background(model.canSend ? Theme.accent : Color(hex: 0x303034), in: Circle())
                    .frame(width: 44, height: 44).contentShape(Rectangle())
            }.disabled(!model.canSend).accessibilityLabel("Send prompt")
        }
    }
    private func rawRow(_ token: Int) -> some View {
        HStack(spacing: 6) {
            TextField(
                "Raw text · typed without Enter",
                text: Binding(get: { model.rawText }, set: { model.editRawText($0, screen: token) }),
                axis: .vertical
            ).lineLimit(1...3).font(.system(.callout, design: .monospaced)).focused($field, equals: .raw)
                .autocorrectionDisabled().textInputAutocapitalization(.never)
                .padding(.horizontal, 12).padding(.vertical, 8)
                .background(Theme.surface, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .accessibilityHint("Raw text goes directly to the terminal without submitting. Use the Enter key to submit.")
            Button("Type") { model.sendRaw(screen: token) }.font(.subheadline.weight(.semibold)).foregroundStyle(Theme.ink)
                .padding(.horizontal, 14).frame(minHeight: 40).background(Theme.raised, in: RoundedRectangle(cornerRadius: 10))
                .disabled(model.rawText.isEmpty || model.rawText.utf8.count > 65536)
        }
    }
}

/// Answering keys come first: prompts in the terminal are answered with Enter, arrows, y and n.
private struct AgentQuickKeys: View {
    @ObservedObject var model: AgentControlModel
    private static let order: [AgentQuickKey] = [.enter, .up, .down, .yes, .no, .escape, .interrupt, .tab, .left, .right]
    var body: some View {
        let token = model.screenToken
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 5) {
                ForEach(Self.order) { key in
                    Button {
                        model.sendKey(key, screen: token)
                    } label: {
                        Text(Self.face(key)).font(.system(.footnote, design: .monospaced).weight(.medium))
                            .foregroundStyle(Theme.ink)
                            .frame(minWidth: 38, minHeight: 34).padding(.horizontal, 2)
                            .background(Color(hex: 0x26262A), in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                            // Compact key face, full 44-point hit area.
                            .frame(minHeight: 44).contentShape(Rectangle())
                    }.buttonStyle(.plain).accessibilityLabel(key.rawValue)
                }
            }
        }.disabled(!model.controlEnabled).opacity(model.controlEnabled ? 1 : 0.4)
    }
    private static func face(_ key: AgentQuickKey) -> String {
        switch key {
        case .enter: return "⏎"
        case .escape: return "esc"
        case .interrupt: return "^C"
        case .tab: return "tab"
        default: return key.label
        }
    }
}

private struct AgentPromptRecoveries: View {
    @ObservedObject var model: AgentControlModel
    let screen: Int
    @Environment(\.dismiss) private var dismiss
    var body: some View {
        NavigationStack {
            List {
                ForEach(model.recoveries) { item in
                    VStack(alignment: .leading, spacing: 6) {
                        Text(item.status).font(.caption.weight(.semibold)).foregroundStyle(item.status.contains("uncertain") ? Theme.warning : Theme.muted)
                        Text(item.text).font(.callout).lineLimit(4).textSelection(.enabled)
                        if let reason = item.reason { Text(reason).font(.caption).foregroundStyle(Theme.muted) }
                        HStack(spacing: 20) {
                            Button("Recover") { model.restoreRecovery(item.id, screen: screen); dismiss() }
                                .foregroundStyle(Theme.accentText)
                                .accessibilityHint("Append saved text below your current draft. Does not send.")
                            Button("Copy") {
                                guard model.screenToken == screen else { return }
                                UIPasteboard.general.string = item.text
                            }.foregroundStyle(Theme.ink)
                            Spacer()
                            Button("Dismiss", role: .destructive) { model.dismissRecovery(item.id, screen: screen) }
                                .foregroundStyle(Theme.danger)
                                .disabled(model.composer.pending?.id == item.id)
                        }.font(.callout.weight(.semibold)).buttonStyle(.borderless).frame(minHeight: 44)
                    }.padding(.vertical, 4).listRowBackground(Theme.surface)
                }
                Section { } footer: { Text("Recover adds the text below your current draft. Nothing is sent.") }
            }
            .scrollContentBackground(.hidden).background(Theme.ground)
            .overlay { if model.recoveries.isEmpty { ContentUnavailableView("No saved prompts", systemImage: "tray") } }
            .navigationTitle("Saved prompts").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Done") { dismiss() } } }
        }.presentationDetents([.medium, .large])
    }
}
