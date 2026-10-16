import SwiftUI
import UIKit

private struct AgentListRow: View {
    let agent: ControlledAgent
    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            Image(systemName: agent.phase == "dialog" ? "questionmark.bubble.fill" : "terminal")
                .foregroundStyle(agent.phase == "dialog" ? .orange : .secondary)
            VStack(alignment: .leading, spacing: 4) {
                Text(agent.name.isEmpty ? agent.id : agent.name).font(.body.weight(.medium))
                Text(
                    [agent.agentType, agent.state, agent.phase?.replacingOccurrences(of: "_", with: " ")].compactMap {
                        $0
                    }.joined(separator: " · ")
                )
                .font(.caption).foregroundStyle(.secondary)
                Text(agent.idleLabel).font(.caption2).foregroundStyle(.secondary)
                if agent.phase == "dialog" {
                    Text("Answer needed").font(.caption.weight(.semibold)).foregroundStyle(.orange)
                }
            }
            Spacer(minLength: 4)
            Image(systemName: "chevron.right").font(.caption).foregroundStyle(.secondary)
        }.frame(minHeight: 44).contentShape(Rectangle()).accessibilityElement(children: .combine)
    }
}

struct AgentDestinationPrincipal: View {
    @ObservedObject var model: AgentControlModel
    let bridgeName: String
    var body: some View {
        VStack(spacing: 2) {
            if let context = model.destinationContext {
                Text(context.agentName).font(.headline).lineLimit(1)
                Text(context.machineName + " · " + context.status).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
            } else {
                Text("Projects").font(.headline)
                Text("Via " + bridgeName).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
            }
        }.accessibilityElement(children: .combine)
    }
}

struct AgentControlView: View {
    @ObservedObject var model: AgentControlModel
    let screen: Int
    var body: some View {
        VStack(spacing: 0) {
            if let context = model.destinationContext {
                VStack(alignment: .leading, spacing: 4) {
                    Text(context.agentName).font(.headline)
                    Text(context.machineName + " · " + context.projectLabel).font(.callout)
                    Text(context.worktreeLabel + " · " + context.status).font(.caption).foregroundStyle(.secondary)
                }.frame(maxWidth: .infinity, alignment: .leading).padding(12).accessibilityElement(children: .combine)
            }
            if let error = model.error {
                Text(error).font(.callout).foregroundStyle(.orange).textSelection(.enabled).padding(12)
            }
            HStack {
                Text(model.hasSize ? "\(model.cols) × \(model.rows)" : "Agent size unavailable")
                Spacer()
                Text("Pinch to zoom · Scroll sideways")
            }.font(.caption2).foregroundStyle(.secondary).padding(.horizontal, 12).padding(.vertical, 8)
            if model.screenMayBeInexact || (!model.hasSize && model.streamId != nil) {
                Text(
                    model.screenMayBeInexact
                        ? "Legacy daemon: replay may be incomplete and the screen size may be inexact."
                        : "Waiting for agent size. Display may be inexact until it arrives."
                )
                .font(.caption).foregroundStyle(.orange).padding(.horizontal, 12).padding(.bottom, 8)
            }
            AgentTerminalView(model: model).frame(maxWidth: .infinity, maxHeight: .infinity)
            AgentQuickKeys(model: model)
            AgentPromptComposer(model: model)
        }
        .navigationTitle(model.agent?.name ?? "Agent")
        .navigationBarTitleDisplayMode(.inline)
        .onDisappear { model.closeScreen(screen: screen) }
    }
}

private struct AgentQuickKeys: View {
    @ObservedObject var model: AgentControlModel
    var body: some View {
        let token = model.screenToken
        ScrollView(.horizontal) {
            HStack(spacing: 8) {
                ForEach(AgentQuickKey.allCases) { key in
                    Button {
                        model.sendKey(key, screen: token)
                    } label: {
                        Text(key.label).font(.system(.callout, design: .monospaced)).frame(minWidth: 44, minHeight: 44)
                    }.buttonStyle(.bordered).accessibilityLabel(key.rawValue)
                }
            }.padding(.horizontal, 12).padding(.vertical, 8)
        }.disabled(!model.controlEnabled)
    }
}

private struct AgentPromptComposer: View {
    @ObservedObject var model: AgentControlModel
    var body: some View {
        let token = model.screenToken
        VStack(alignment: .leading, spacing: 8) {
            if !model.recoveries.isEmpty { AgentPromptRecoveries(model: model, screen: token) }
            if let explanation = model.sendExplanation { Text(explanation).font(.caption).foregroundStyle(.secondary) }
            if let outcome = model.composer.outcome {
                Text((model.composer.deliveryUncertain ? "Delivery uncertain" : outcome.label) + (model.composer.reason.map { " · " + $0 } ?? ""))
                    .font(.caption).foregroundStyle(outcome == .failed || outcome == .blocked ? .orange : .secondary)
                    .textSelection(.enabled)
            } else if model.composer.pending != nil {
                Text("Sending…").font(.caption).foregroundStyle(.secondary)
            }
            TextField(
                "Message agent", text: Binding(get: { model.draft }, set: { model.editDraft($0, screen: token) }),
                axis: .vertical
            )
            .lineLimit(2...6).textFieldStyle(.roundedBorder).autocorrectionDisabled(false)
            .disabled(model.permission != .control)
            .accessibilityLabel("Agent prompt. Return inserts a new line.")
            HStack {
                Toggle(
                    "Raw typing",
                    isOn: Binding(get: { model.rawTyping }, set: { model.setRawTyping($0, screen: token) })
                ).font(.caption).fixedSize()
                    .disabled(!model.controlEnabled)
                Spacer()
                Button("Send prompt") { model.sendPrompt(screen: token) }.buttonStyle(.borderedProminent).disabled(
                    !model.canSend)
            }
            if model.rawTyping && model.controlEnabled {
                Text("Raw text goes directly to the terminal without submitting. Use Enter above to submit.")
                    .font(.caption2).foregroundStyle(.secondary)
                HStack {
                    TextField(
                        "Raw terminal text",
                        text: Binding(get: { model.rawText }, set: { model.editRawText($0, screen: token) }),
                        axis: .vertical
                    ).lineLimit(1...3)
                        .textFieldStyle(.roundedBorder).autocorrectionDisabled().textInputAutocapitalization(.never)
                    Button("Type") { model.sendRaw(screen: token) }.disabled(
                        model.rawText.isEmpty || model.rawText.utf8.count > 65536)
                }
            }
        }.padding(12).background(Color(uiColor: .secondarySystemBackground))
    }
}

private struct AgentPromptRecoveries: View {
    @ObservedObject var model: AgentControlModel
    let screen: Int
    var body: some View {
        DisclosureGroup("Saved prompts (\(model.recoveries.count))") {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    ForEach(model.recoveries) { item in
                        VStack(alignment: .leading, spacing: 6) {
                            Text(item.status).font(.caption.weight(.semibold))
                            Text(item.text).font(.caption).lineLimit(3).textSelection(.enabled)
                            if let reason = item.reason { Text(reason).font(.caption2).foregroundStyle(.secondary) }
                            HStack(spacing: 16) {
                                Button("Recover") { model.restoreRecovery(item.id, screen: screen) }
                                    .accessibilityHint("Append saved text below your current draft. Does not send.")
                                Button("Copy") {
                                    guard model.screenToken == screen else { return }
                                    UIPasteboard.general.string = item.text
                                }
                                Button("Dismiss", role: .destructive) { model.dismissRecovery(item.id, screen: screen) }
                                    .disabled(model.composer.pending?.id == item.id)
                            }.font(.caption).frame(minHeight: 44)
                        }
                    }
                }.frame(maxWidth: .infinity, alignment: .leading)
            }.frame(maxHeight: 180)
        }.font(.caption)
    }
}

struct AgentNavigator: View {
    @ObservedObject var model: AgentControlModel
    let destination: String
    let pointGuard: () -> Void
    let projects: () -> Void
    let open: (AgentShortcut) -> Void
    let connections: () -> Void
    let close: () -> Void
    @State private var removing: AgentShortcut?
    @State private var renaming: WorkspaceLaunchTarget?
    var body: some View {
        VStack(spacing: 0) {
            HStack { Spacer(); Button(action: close) { Image(systemName: "xmark").frame(width: 44, height: 44) }.accessibilityLabel("Close navigator").keyboardShortcut(.cancelAction) }
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 4) {
                    Button(action: pointGuard) {
                        HStack(spacing: 12) {
                            Image("PurePointLogo").resizable().scaledToFit().frame(width: 24, height: 24)
                            Text("Point Guard").font(.body.weight(.medium))
                            Spacer()
                        }.frame(minHeight: 48)
                    }.background(destination == "pointGuard" ? Color(uiColor: .secondarySystemBackground) : Color.clear, in: RoundedRectangle(cornerRadius: 8)).accessibilityAddTraits(destination == "pointGuard" ? .isSelected : [])
                    Button(action: projects) {
                        HStack(spacing: 12) {
                            Image(systemName: "folder").frame(width: 24, height: 24)
                            Text("Projects").font(.body.weight(.medium))
                            Spacer()
                        }.frame(minHeight: 48)
                    }.background(destination != "pointGuard" && model.selected == nil ? Color(uiColor: .secondarySystemBackground) : Color.clear, in: RoundedRectangle(cornerRadius: 8)).accessibilityAddTraits(destination != "pointGuard" && model.selected == nil ? .isSelected : [])
                    shortcutSection("Pinned agents", entries: model.pinnedShortcuts)
                    shortcutSection("Recent agents", entries: model.recentShortcuts)
                    if model.permission == .off { Text("Terminal access is off. Enable it in Connect Phone on your Mac.").font(.callout).foregroundStyle(.secondary).padding(.vertical) }
                    if let error = model.error { Text(error).font(.caption).foregroundStyle(.orange) }
                    Button { Task { await model.refresh() } } label: { Label("Refresh agents", systemImage: "arrow.clockwise").frame(minHeight: 44) }.disabled(!model.connected || model.permission == .off)
                    Button(action: connections) { Label("Connections…", systemImage: "network").frame(minHeight: 44) }
                }.padding(.horizontal, 20).padding(.bottom, 20)
            }
        }.buttonStyle(.plain).foregroundStyle(.primary).background(Color(uiColor: .systemBackground))
        .sheet(item: $renaming) { target in WorkspaceLaunchForm(model: model, target: target) }
        .accessibilityAddTraits(.isModal)
        .accessibilityAction(.escape, close)
        .confirmationDialog("Remove from recents?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
            Button("Remove from recents", role: .destructive) { if let removing { model.hideShortcut(removing.scope) }; removing = nil }
        } message: { Text("Only this phone’s shortcut is removed and unpinned. The agent keeps running and remains in Projects. Files, native transcripts and saved drafts are kept.") }
    }
    private func rename(_ entry: AgentShortcut) {
        guard let project = entry.agent.projectRoot else { return }
        renaming = WorkspaceLaunchTarget(host: entry.scope.host, machine: entry.machine.id, machineName: entry.machine.name, project: project, projectName: URL(fileURLWithPath: project).lastPathComponent, worktree: entry.agent.worktreeId, kind: "rename", agent: entry.agent.id)
    }
    private func shortcutSection(_ title: String, entries: [AgentShortcut]) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.caption.weight(.semibold)).foregroundStyle(.secondary).padding(.top, 20)
            ForEach(entries) { entry in
                Button { if model.connected && entry.machine.online && entry.machine.metadataStatus != "error" { open(entry) } } label: {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(entry.agent.name.isEmpty ? entry.agent.id : entry.agent.name).font(.body.weight(.medium)).lineLimit(1)
                        Text("\(entry.machine.name) · \(entry.agent.projectRoot.map { URL(fileURLWithPath: $0).lastPathComponent } ?? "Project unavailable")").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                        Text([entry.agent.agentType, entry.machine.online && entry.machine.metadataStatus != "error" && model.connected ? entry.agent.state : "Unavailable", entry.agent.phase == "dialog" ? "Answer needed" : entry.agent.phase, entry.agent.idleLabel].compactMap { $0 }.joined(separator: " · ")).font(.caption).foregroundStyle(.secondary)
                    }.frame(maxWidth: .infinity, minHeight: 76, alignment: .leading).contentShape(Rectangle())
                }
                .background(model.selected == entry.scope ? Color(uiColor: .secondarySystemBackground) : Color.clear, in: RoundedRectangle(cornerRadius: 8))
                .accessibilityAddTraits(model.selected == entry.scope ? .isSelected : [])
                .accessibilityLabel([entry.agent.name, entry.machine.name, entry.agent.projectRoot, entry.agent.worktreeId, entry.agent.agentType, entry.agent.state, entry.agent.phase, entry.agent.idleLabel].compactMap { $0 }.joined(separator: ", "))
                .contextMenu {
                    Button(model.navigation.isPinned(entry.scope) ? "Unpin" : "Pin") { model.togglePin(entry.scope) }
                    Button("Rename…") { rename(entry) }.disabled(!model.canManage || entry.agent.projectRoot == nil)
                    Button("Remove from recents", role: .destructive) { removing = entry }
                }
                .accessibilityAction(named: model.navigation.isPinned(entry.scope) ? "Unpin" : "Pin") { model.togglePin(entry.scope) }
                .accessibilityAction(named: "Rename") { if model.canManage { rename(entry) } }
                .accessibilityAction(named: "Remove from recents") { removing = entry }
            }
        }
    }
}

struct MobileProjectsView: View {
    @ObservedObject var model: AgentControlModel
    let hostName: String
    @State private var projectID: String?
    @State private var machineID: String?
    @State private var memory = WorkspaceNavigationMemory()
    @State private var highlighted: String?
    @AccessibilityFocusState private var focusedAgent: String?
    @State private var launch: WorkspaceLaunchTarget?
    @State private var stop: AgentShortcut?
    private var machine: AgentMachine? { model.machines.first { $0.id == machineID } }
    private var project: WorkspaceProject? { machine?.registeredProjects?.first { $0.id == projectID } }
    var body: some View {
        Group {
            if model.selected != nil {
                AgentControlView(model: model, screen: model.screenToken)
                    .safeAreaInset(edge: .top) {
                        VStack(alignment: .leading) {
                            Button("Open in project") { locateSelected() }.frame(minHeight: 44).disabled(!canLocate)
                            if !canLocate { Text("Project association unavailable. Refresh metadata to retry.").font(.caption); Button("Refresh metadata") { Task { await model.refresh() } } }
                        }.frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal)
                    }
            } else if let machine, let project {
                workspace(machine, project)
            } else {
                projectList
            }
        }
        .sheet(item: $launch) { target in WorkspaceLaunchForm(model: model, target: target) }
        .confirmationDialog("Stop this exact agent?", isPresented: Binding(get: { stop != nil }, set: { if !$0 { stop = nil } }), titleVisibility: .visible) {
            Button("Stop agent", role: .destructive) {
                guard let stop, let project = stop.agent.projectRoot else { return }
                launch = WorkspaceLaunchTarget(host: stop.scope.host, machine: stop.machine.id, machineName: stop.machine.name, project: project, projectName: URL(fileURLWithPath: project).lastPathComponent, worktree: nil, kind: "stop", agent: stop.agent.id)
                self.stop = nil
            }
        } message: { Text(stop.map { "Stop \($0.agent.name) on \($0.machine.name), \($0.agent.projectRoot ?? "unknown project")? Other agents keep running." } ?? "") }
        .task { await model.refresh() }
        .onChange(of: model.bridgeIdentity) { _, _ in projectID = nil; machineID = nil; highlighted = nil; launch = nil; stop = nil }
    }
    private var selectedLocation: WorkspaceLocation? { model.selected.flatMap { WorkspaceLocation.locate($0, machines: model.machines) } }
    private var canLocate: Bool { selectedLocation != nil }
    private func locateSelected() {
        guard let location = selectedLocation, let scope = model.selected else { return }
        machineID = location.machine; projectID = location.project; highlighted = scope.agent
        memory.locate(location, agent: scope.agent)
        model.closeScreen()
    }
    private var projectList: some View {
        ScrollView { LazyVStack(alignment: .leading, spacing: 16) {
            Section { Text("Bridge: \(hostName)").foregroundStyle(.secondary) }
            if model.permission == .off { Text("Terminal access is off. Enable viewing or control in Connect Phone on your Mac.") }
            if !model.connected { Text("Bridge disconnected. Cached work is unavailable.").foregroundStyle(.orange) }
            if let error = model.error { Text(error).foregroundStyle(.orange) }
            ForEach(model.machines) { machine in
                Section(machine.name) {
                    if !machine.online { Text(machine.error ?? "Machine offline").foregroundStyle(.secondary) }
                    if machine.metadataStatus != "complete" { Text(machine.metadataError ?? "Project metadata incomplete or unsupported. Refresh to retry.").foregroundStyle(.orange) }
                    ForEach(machine.registeredProjects ?? []) { project in
                        Button {
                            machineID = machine.id; projectID = project.id
                        } label: {
                            VStack(alignment: .leading, spacing: 6) {
                                Label(project.name, systemImage: "folder").font(.headline)
                                Text("\(machine.name) · \(machine.agents.filter { $0.projectRoot == project.id }.count) agents · \(project.available && machine.online && model.connected ? "Available" : "Unavailable")").font(.caption).foregroundStyle(.secondary)
                                if let error = project.error { Text(error).font(.caption).foregroundStyle(.orange) }
                            }.frame(minHeight: 60)
                        }.buttonStyle(.plain).accessibilityHint(project.id).id(WorkspaceLocation(host: model.bridgeIdentity, machine: machine.id, project: project.id, worktree: nil).storageKey)
                    }
                    if machine.registeredProjects?.isEmpty == true { Text("No registered projects") }
                }
            }
            if model.loading { ProgressView("Loading projects") }
            Button("Refresh projects") { Task { await model.refresh() } }.disabled(!model.connected || model.permission == .off)
        }.scrollTargetLayout().padding() }.navigationTitle("Projects")
        .scrollPosition(id: Binding(get: { memory.projectAnchors[model.bridgeIdentity] }, set: { memory.projectAnchors[model.bridgeIdentity] = $0 }))
    }
    private func workspace(_ machine: AgentMachine, _ project: WorkspaceProject) -> some View {
        ScrollViewReader { proxy in
            ScrollView { LazyVStack(alignment: .leading, spacing: 16) {
                Section {
                    Button("Back to Projects") { projectID = nil; machineID = nil }.frame(minHeight: 44)
                    Text("\(hostName) → \(machine.name) → \(project.name)").font(.caption)
                    Text(project.id).font(.caption2).textSelection(.enabled)
                    if !machine.online || !model.connected || !project.available { Text(project.error ?? "Workspace unavailable").foregroundStyle(.orange) }
                    if machine.metadataStatus != "complete" { Text(machine.metadataError ?? "Workspace metadata incomplete. Refresh to retry.").foregroundStyle(.orange) }
                    Menu("New") {
                        Button("New agent") { begin("spawn", machine, project) }
                        Button("New worktree") { begin("createWorktree", machine, project) }
                        Button("New terminal") { begin("terminal", machine, project) }
                    }.frame(minHeight: 44).disabled(!model.canManage || !machine.online || !project.available)
                    if !model.canManage { Text("Launching, renaming and stopping require supported workspace management and a separate grant in Connect Phone.").font(.caption).foregroundStyle(.secondary) }
                }
                Section("Project root") {
                    ForEach(machine.agents.filter { $0.projectRoot == project.id && $0.worktreeId == nil }) { agent in workspaceAgent(machine, agent) }
                    if !machine.agents.contains(where: { $0.projectRoot == project.id && $0.worktreeId == nil }) { Text("No root agents").foregroundStyle(.secondary) }
                }
                ForEach((machine.worktrees ?? []).filter { $0.projectId == project.id }) { worktree in
                    Section {
                        DisclosureGroup(isExpanded: Binding(get: { !memory.collapsed.contains(WorkspaceLocation(host: model.bridgeIdentity, machine: machine.id, project: project.id, worktree: worktree.id).storageKey) }, set: { if $0 { memory.collapsed.remove(WorkspaceLocation(host: model.bridgeIdentity, machine: machine.id, project: project.id, worktree: worktree.id).storageKey) } else { memory.collapsed.insert(WorkspaceLocation(host: model.bridgeIdentity, machine: machine.id, project: project.id, worktree: worktree.id).storageKey) } })) {
                            ForEach(machine.agents.filter { $0.projectRoot == project.id && $0.worktreeId == worktree.id }) { agent in workspaceAgent(machine, agent) }
                            Button("New agent") { begin("spawn", machine, project, worktree: worktree.id) }.frame(minHeight: 44).disabled(!model.canManage || !machine.online)
                        } label: {
                            VStack(alignment: .leading) { Text(worktree.name).font(.headline); Text("\(worktree.branch) · \(worktree.status)").font(.caption).foregroundStyle(.secondary) }
                        }
                    }
                }
                let missing = machine.agents.filter { agent in agent.projectRoot == project.id && agent.worktreeId != nil && !(machine.worktrees ?? []).contains(where: { $0.id == agent.worktreeId && $0.projectId == project.id }) }
                if !missing.isEmpty { Section("Worktree metadata unavailable") { ForEach(missing) { agent in workspaceAgent(machine, agent) } } }
                Button("Refresh workspace") { Task { await model.refresh() } }.frame(minHeight: 44)
            }.scrollTargetLayout().padding() }.navigationTitle(project.name)
            .scrollPosition(id: Binding(get: { memory.workspaceAnchors[WorkspaceLocation(host: model.bridgeIdentity, machine: machine.id, project: project.id, worktree: nil).storageKey] }, set: { memory.workspaceAnchors[WorkspaceLocation(host: model.bridgeIdentity, machine: machine.id, project: project.id, worktree: nil).storageKey] = $0 }))
            .onAppear { if let highlighted { proxy.scrollTo(highlighted, anchor: .center); focusedAgent = highlighted } }
        }
    }
    private func workspaceAgent(_ machine: AgentMachine, _ agent: ControlledAgent) -> some View {
        Button { model.open(machine: machine, agent: agent, context: model.contextToken) } label: { AgentListRow(agent: agent) }
            .buttonStyle(.plain).disabled(!model.connected || !machine.online).id(agent.id)
            .background(highlighted == agent.id ? Color.accentColor.opacity(0.15) : Color.clear, in: RoundedRectangle(cornerRadius: 8))
            .accessibilityAddTraits(highlighted == agent.id ? .isSelected : [])
            .accessibilityFocused($focusedAgent, equals: agent.id)
            .contextMenu {
                Button("Rename…") { begin("rename", machine, project ?? WorkspaceProject(id: agent.projectRoot ?? "", name: "Project", available: false, error: nil), agent: agent.id) }.disabled(!model.canManage)
                Button("Stop agent…", role: .destructive) { stop = model.shortcuts.first { $0.machine.id == machine.id && $0.agent.id == agent.id } }.disabled(!model.canManage)
            }
    }
    private func begin(_ kind: String, _ machine: AgentMachine, _ project: WorkspaceProject, worktree: String? = nil, agent: String? = nil) {
        launch = WorkspaceLaunchTarget(host: model.bridgeIdentity, machine: machine.id, machineName: machine.name, project: project.id, projectName: project.name, worktree: worktree, kind: kind, agent: agent)
    }
}

struct WorkspaceLaunchTarget: Identifiable {
    let host: String
    let machine: String
    let machineName: String
    let project: String
    let projectName: String
    let worktree: String?
    let kind: String
    var agent: String? = nil
    var id: String { WorkspaceLocation.key([host, machine, project, worktree ?? "root", kind, agent ?? ""]) }
    var storageKey: String { Data(id.utf8).base64EncodedString() }
}

struct WorkspaceLaunchForm: View {
    @ObservedObject var model: AgentControlModel
    let target: WorkspaceLaunchTarget
    @Environment(\.dismiss) private var dismiss
    @State private var draft = WorkspaceFormDraft()
    @State private var catalog: WorkspaceCatalog?
    @State private var loading = false
    @State private var submitting = false
    @State private var reviewedInventory = false
    @State private var confirmAnother = false
    @State private var error: String?
    private var key: String { "pi.workspaceForm.v1." + target.storageKey }
    private var availableWorktrees: [WorkspaceWorktree] { model.machines.first { $0.id == target.machine }?.worktrees?.filter { $0.projectId == target.project } ?? [] }
    private var needsCatalog: Bool { target.kind == "spawn" || target.kind == "terminal" }
    private var valid: Bool {
        guard model.canManage, model.bridgeIdentity == target.host, model.machines.contains(where: { $0.id == target.machine && $0.online && $0.metadataStatus != "error" && $0.registeredProjects?.contains(where: { $0.id == target.project && $0.available }) == true }), draft.operationId == nil, !submitting else { return false }
        if target.kind == "rename" || target.kind == "createWorktree" || (target.kind == "spawn" && draft.destination == "newWorktree") {
            guard !draft.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, draft.name.utf8.count <= 80, !draft.name.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else { return false }
        }
        if target.kind == "spawn" {
            guard catalog?.agents.contains(where: { $0.id == draft.agentType && $0.available }) == true,
                  !draft.prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, draft.prompt.utf8.count <= 65536 else { return false }
        }
        if !draft.base.isEmpty && (target.kind == "createWorktree" || draft.destination == "newWorktree") {
            guard draft.base.utf8.count <= 128, draft.base.range(of: "^[A-Za-z0-9][A-Za-z0-9_./-]*$", options: .regularExpression) != nil, !draft.base.contains("..") else { return false }
        }
        if target.kind == "spawn", !draft.preset.isEmpty {
            guard let preset = catalog?.presets.first(where: { $0.id == draft.preset && $0.agentType == draft.agentType }), (preset.prompt + "\n\n" + draft.prompt).utf8.count <= 65536 else { return false }
        }
        if target.kind == "terminal", catalog?.terminalAvailable != true { return false }
        if draft.destination == "worktree" && (target.kind == "spawn" || target.kind == "terminal") && !availableWorktrees.contains(where: { $0.id == draft.worktree }) { return false }
        return true
    }
    var body: some View {
        NavigationStack {
            Form {
                Section("Launch target") {
                    Text(target.machineName).font(.headline)
                    Text(target.projectName); Text(target.project).font(.caption).textSelection(.enabled)
                    if let agent = target.agent { Text("Agent: \(agent)").font(.caption) }
                    if target.kind == "spawn" || target.kind == "terminal" {
                        Picker("Location", selection: $draft.destination) {
                            Text("Project root").tag("root")
                            Text("Existing worktree").tag("worktree")
                            if target.kind == "spawn" { Text("New worktree").tag("newWorktree") }
                        }
                        if draft.destination == "worktree" {
                            Picker("Worktree", selection: $draft.worktree) { Text("Select worktree").tag(""); ForEach(availableWorktrees) { Text("\($0.name) · \($0.branch)").tag($0.id) } }
                        }
                    }
                }.disabled(draft.operationId != nil)
                Section {
                    if target.kind == "spawn" {
                        Picker("Agent", selection: $draft.agentType) { Text("Select agent").tag(""); ForEach(catalog?.agents ?? []) { agent in if agent.available { Text(agent.name).tag(agent.id) } } }
                        Picker("Preset", selection: $draft.preset) { Text("None").tag(""); ForEach((catalog?.presets ?? []).filter { $0.agentType == draft.agentType }) { Text($0.name).tag($0.id) } }
                        TextField("Instructions", text: $draft.prompt, axis: .vertical).lineLimit(4...12)
                        Toggle("Plan mode", isOn: $draft.planMode)
                    }
                    if target.kind == "rename" || target.kind == "createWorktree" || (target.kind == "spawn" && draft.destination == "newWorktree") {
                        TextField(target.kind == "rename" ? "Agent name" : "Worktree name", text: $draft.name)
                    }
                    if target.kind == "createWorktree" || (target.kind == "spawn" && draft.destination == "newWorktree") { TextField("Base branch (optional)", text: $draft.base).autocorrectionDisabled().textInputAutocapitalization(.never) }
                }.disabled(draft.operationId != nil)
                Section {
                    if loading { ProgressView("Loading authenticated options") }
                    if let error { Text(error).foregroundStyle(.orange).textSelection(.enabled) }
                    if needsCatalog && catalog == nil { Button("Reload launch options") { Task { await loadCatalog() } }.disabled(loading) }
                    if !model.canManage { Text("Control and workspace management permission required on this Mac.").foregroundStyle(.orange) }
                    if let id = draft.operationId {
                        Text("\(draft.state ?? "uncertain"): \(draft.message ?? "Check status and Projects before starting anything else.")").textSelection(.enabled)
                        Text("Operation: \(id)").font(.caption).textSelection(.enabled)
                        Button("Check operation status") { Task { await status(id) } }.disabled(submitting || model.bridgeIdentity != target.host)
                        Button("Refresh authoritative workspace") { Task {
                            reviewedInventory = false
                            guard model.bridgeIdentity == target.host else { return }
                            reviewedInventory = await model.refreshWorkspaceForReview(machine: target.machine, project: target.project)
                            if !reviewedInventory { error = "A complete current workspace could not be refreshed. Reconnect or retry before starting a separate operation." }
                        } }.disabled(model.bridgeIdentity != target.host)
                        if reviewedInventory && draft.state != "pending" { Button("Start a separate operation…") { confirmAnother = true } }
                        Text("No operation is replayed after reconnect. Unknown status is not proof that launch failed.").font(.caption)
                    } else {
                        Text("Confirm \(target.kind) on \(target.machineName) / \(target.projectName) / \(draft.destination == "root" ? "project root" : draft.destination == "newWorktree" ? draft.name : draft.worktree)").font(.callout)
                        Button(target.kind == "stop" ? "Confirm stop exact agent" : "Confirm \(target.kind)", role: target.kind == "stop" ? .destructive : nil) { Task { await submit() } }.disabled(!valid)
                    }
                }
            }
            .navigationTitle(target.kind == "rename" ? "Rename agent" : target.kind == "stop" ? "Stop agent" : "New \(target.kind == "createWorktree" ? "worktree" : target.kind == "spawn" ? "agent" : "terminal")")
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Close") { save(); dismiss() }.keyboardShortcut(.cancelAction) } }
            .task {
                if let data = UserDefaults.standard.data(forKey: key), let saved = try? JSONDecoder().decode(WorkspaceFormDraft.self, from: data) { draft = saved }
                else if let worktree = target.worktree { draft.destination = "worktree"; draft.worktree = worktree }
                if needsCatalog { await loadCatalog() }
            }
            .confirmationDialog("Create a separate operation?", isPresented: $confirmAnother, titleVisibility: .visible) {
                Button("Create new operation") { draft.operationId = nil; draft.state = nil; draft.message = nil; reviewedInventory = false; save() }
            } message: { Text("The previous operation may already have committed. Check Projects first. This creates a separate explicit launch and can duplicate work.") }
            .onChange(of: model.contextToken) { _, _ in reviewedInventory = false }
            .onChange(of: model.canManage) { _, value in if !value { reviewedInventory = false } }
            .onChange(of: draft.agentType) { _, _ in if catalog?.presets.contains(where: { $0.id == draft.preset && $0.agentType == draft.agentType }) != true { draft.preset = "" } }
            .onChange(of: draft) { _, _ in save() }
            .onDisappear { save() }
        }
    }
    private func save() { if let data = try? JSONEncoder().encode(draft) { UserDefaults.standard.set(data, forKey: key) } }
    private func loadCatalog() async {
        guard model.bridgeIdentity == target.host else { return }
        loading = true; defer { loading = false }
        do { catalog = try await model.catalog(machine: target.machine, project: target.project); error = nil } catch { self.error = error.localizedDescription }
    }
    private func submit() async {
        guard valid else { return }
        let id = UUID().uuidString
        draft.operationId = id; draft.state = "pending"; save()
        // Persist the immutable submitted form before the first network dispatch.
        guard let persisted = UserDefaults.standard.data(forKey: key), (try? JSONDecoder().decode(WorkspaceFormDraft.self, from: persisted)) == draft else { error = "Could not save operation. Nothing dispatched."; draft.operationId = nil; return }
        submitting = true; defer { submitting = false; save() }
        var fields: [String: Any] = ["operationId": id, "machine": target.machine, "projectId": target.project]
        if target.kind == "spawn" || target.kind == "terminal" {
            fields["target"] = draft.destination
            if draft.destination == "worktree" { fields["worktreeId"] = draft.worktree }
        }
        if target.kind == "spawn" {
            fields["agentType"] = draft.agentType; fields["prompt"] = draft.prompt; fields["planMode"] = draft.planMode
            if !draft.preset.isEmpty { fields["presetId"] = draft.preset }
        }
        if target.kind == "rename" || target.kind == "createWorktree" || (target.kind == "spawn" && draft.destination == "newWorktree") { fields["name"] = draft.name }
        if target.kind == "createWorktree" || (target.kind == "spawn" && draft.destination == "newWorktree"), !draft.base.isEmpty { fields["base"] = draft.base }
        if let agent = target.agent { fields["agentId"] = agent }
        if target.kind == "stop" { fields["confirm"] = true }
        do { let result = try await model.workspaceRequest("workspace." + target.kind, fields: fields); draft.state = result.state; draft.message = result.message }
        catch {
            draft.state = "uncertain"; draft.message = error.localizedDescription
            if error.localizedDescription.contains("workspace_duplicate"), let result = try? await model.operationStatus(id) { draft.state = result.state; draft.message = result.message }
        }
    }
    private func status(_ id: String) async {
        submitting = true; defer { submitting = false; save() }
        do { let result = try await model.operationStatus(id); draft.state = result.state; draft.message = result.message }
        catch { self.error = error.localizedDescription }
    }
}
