import SwiftUI
import UIKit

// MARK: - Shared workspace styling

extension ControlledAgent {
    var displayName: String { name.isEmpty ? id : name }
    var phaseLabel: String? { phase?.replacingOccurrences(of: "_", with: " ") }
    var shortIdleLabel: String? { idleSeconds == nil ? nil : idleLabel }
    func tone(available: Bool) -> Color {
        guard available else { return Color(uiColor: .tertiaryLabel) }
        if state == "broken" { return .red }
        if phase == "busy" || phase == "has_draft" || phase == "dialog" { return .blue }
        if phase == "input_ready" || phase == "ready" || phase == "idle" || phase == "waiting" { return .green }
        return Color(uiColor: .secondaryLabel)
    }
}

struct StatusDot: View {
    let color: Color
    var size: CGFloat = 8
    var body: some View { Circle().fill(color).frame(width: size, height: size).accessibilityHidden(true) }
}

/// One-line notice used instead of free-floating paragraphs of status text.
struct NoticeBanner: View {
    let text: String
    var systemImage = "exclamationmark.triangle.fill"
    var tint: Color = .orange
    var actionTitle: String? = nil
    var action: (() -> Void)? = nil
    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: systemImage).foregroundStyle(tint).accessibilityHidden(true)
            Text(text).font(.footnote).foregroundStyle(.primary).textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
            if let actionTitle, let action {
                Button(actionTitle, action: action).font(.footnote.weight(.semibold)).buttonStyle(.borderless)
            }
        }
        .padding(.horizontal, 12).padding(.vertical, 8)
        .background(tint.opacity(0.12), in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .accessibilityElement(children: .combine)
    }
}

/// Grouped card with a small caption header, matching iOS inset-grouped lists
/// while still supporting scroll-position memory in a LazyVStack.
struct CardSection<Header: View, Content: View>: View {
    private let header: Header
    private let content: Content
    init(@ViewBuilder header: () -> Header, @ViewBuilder content: () -> Content) {
        self.header = header(); self.content = content()
    }
    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            header.font(.footnote.weight(.semibold)).foregroundStyle(.secondary).padding(.horizontal, 6)
            VStack(alignment: .leading, spacing: 0) { content }
                .padding(.horizontal, 14).padding(.vertical, 4)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(Color(uiColor: .secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
    }
}

private struct IconTile: View {
    let systemName: String
    let tint: Color
    var body: some View {
        Image(systemName: systemName).font(.callout.weight(.medium)).foregroundStyle(tint)
            .frame(width: 32, height: 32)
            .background(tint.opacity(0.14), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
            .accessibilityHidden(true)
    }
}

private struct RowDivider: View {
    var inset: CGFloat = 44
    var body: some View { Divider().padding(.leading, inset) }
}

private struct AgentListRow: View {
    let agent: ControlledAgent
    var available = true
    var body: some View {
        let tone = agent.tone(available: available)
        HStack(spacing: 12) {
            StatusDot(color: tone).frame(width: 20)
            VStack(alignment: .leading, spacing: 2) {
                Text(agent.displayName).font(.body.weight(.medium)).foregroundStyle(.primary).lineLimit(1)
                Text([agent.agentType, available ? (agent.phaseLabel ?? agent.state) : "Unavailable", agent.shortIdleLabel].compactMap { $0 }.joined(separator: " · "))
                    .font(.caption).foregroundStyle(.secondary).lineLimit(1)
            }
            Spacer(minLength: 4)
            Image(systemName: "chevron.right").font(.caption.weight(.semibold)).foregroundStyle(.tertiary).accessibilityHidden(true)
        }
        .padding(.vertical, 8).frame(minHeight: 52).contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

private struct ToolbarIconButton: View {
    let systemName: String
    let label: String
    let action: () -> Void
    var body: some View {
        Button(action: action) { Image(systemName: systemName) }.accessibilityLabel(label)
    }
}

/// Two-line navigation title: what you are looking at, then where it lives.
private struct NavigationPrincipal: View {
    let title: String
    let subtitle: String
    var tone: Color? = nil
    var body: some View {
        VStack(spacing: 1) {
            Text(title).font(.headline).lineLimit(1)
            HStack(spacing: 5) {
                if let tone { StatusDot(color: tone, size: 6) }
                Text(subtitle).lineLimit(1).truncationMode(.middle)
            }.font(.caption2).foregroundStyle(.secondary)
        }.frame(maxWidth: 240).accessibilityElement(children: .combine)
    }
}

// MARK: - Agent terminal

struct AgentControlView: View {
    @ObservedObject var model: AgentControlModel
    let screen: Int
    var body: some View {
        VStack(spacing: 0) {
            if let notice {
                NoticeBanner(text: notice).padding(.horizontal, 8).padding(.vertical, 6)
                    .background(Color(uiColor: .systemBackground))
            }
            AgentTerminalView(model: model).frame(maxWidth: .infinity, maxHeight: .infinity)
                .overlay(alignment: .topTrailing) {
                    if model.hasSize {
                        Text("\(model.cols)×\(model.rows)").font(.system(.caption2, design: .monospaced))
                            .foregroundStyle(Color.white.opacity(0.7))
                            .padding(.horizontal, 8).padding(.vertical, 4)
                            .background(Color.white.opacity(0.1), in: Capsule())
                            .padding(8).allowsHitTesting(false)
                            .accessibilityLabel("Agent terminal \(model.cols) columns by \(model.rows) rows")
                    }
                }
        }
        .background(Color.black)
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
        VStack(alignment: .leading, spacing: 8) {
            // Saved recoveries stay reachable even when control is lost: copy, restore or dismiss.
            if !model.recoveries.isEmpty {
                Button { showRecoveries = true } label: {
                    Label("\(model.recoveries.count) saved prompt\(model.recoveries.count == 1 ? "" : "s")", systemImage: "tray.full")
                        .font(.caption.weight(.medium)).frame(minHeight: 44)
                }.buttonStyle(.borderless).padding(.horizontal, 16)
            }
            if model.permission == .control {
                AgentQuickKeys(model: model)
                if let status { Text(status.text).font(.caption).foregroundStyle(status.warning ? Color.orange : Color.secondary).lineLimit(2).textSelection(.enabled).padding(.horizontal, 16) }
                composerRow(token)
                if model.rawTyping && model.controlEnabled { rawRow(token) }
            } else {
                Label(model.sendExplanation ?? "Terminal access is view only. Enable control on your Mac.", systemImage: "eye")
                    .font(.footnote).foregroundStyle(.secondary).frame(maxWidth: .infinity, minHeight: 36, alignment: .leading).padding(.horizontal, 16)
            }
        }
        .padding(.vertical, 8)
        .background(.bar)
        .overlay(alignment: .top) { Divider() }
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
        HStack(alignment: .bottom, spacing: 8) {
            if focused {
                Button { field = nil } label: {
                    Image(systemName: "keyboard.chevron.compact.down").font(.body)
                        .foregroundStyle(.secondary).frame(width: 36, height: 44).contentShape(Rectangle())
                }.buttonStyle(.plain).accessibilityLabel("Hide keyboard")
            }
            TextField(
                "Message agent", text: Binding(get: { model.draft }, set: { model.editDraft($0, screen: token) }),
                axis: .vertical
            )
            .lineLimit(1...4).autocorrectionDisabled(false).focused($field, equals: .prompt)
            .padding(.horizontal, 14).padding(.vertical, 9)
            .background(Color(uiColor: .secondarySystemBackground), in: RoundedRectangle(cornerRadius: 20, style: .continuous))
            .accessibilityLabel("Agent prompt. Return inserts a new line.")
            Button { model.sendPrompt(screen: token) } label: {
                Image(systemName: "arrow.up").font(.body.weight(.semibold))
                    .foregroundStyle(Color(uiColor: .systemBackground))
                    .frame(width: 36, height: 36)
                    .background(model.canSend ? Color.accentColor : Color(uiColor: .tertiaryLabel), in: Circle())
                    .frame(width: 44, height: 44).contentShape(Rectangle())
            }.disabled(!model.canSend).accessibilityLabel("Send prompt")
        }.padding(.leading, focused ? 4 : 12).padding(.trailing, 6)
    }
    private func rawRow(_ token: Int) -> some View {
        HStack(spacing: 8) {
            TextField(
                "Raw text · typed without Enter",
                text: Binding(get: { model.rawText }, set: { model.editRawText($0, screen: token) }),
                axis: .vertical
            ).lineLimit(1...3).font(.system(.callout, design: .monospaced)).focused($field, equals: .raw)
                .autocorrectionDisabled().textInputAutocapitalization(.never)
                .padding(.horizontal, 12).padding(.vertical, 8)
                .background(Color(uiColor: .secondarySystemBackground), in: RoundedRectangle(cornerRadius: 12, style: .continuous))
                .accessibilityHint("Raw text goes directly to the terminal without submitting. Use the Enter key to submit.")
            Button("Type") { model.sendRaw(screen: token) }.buttonStyle(.bordered).frame(minHeight: 44)
                .disabled(model.rawText.isEmpty || model.rawText.utf8.count > 65536)
        }.padding(.horizontal, 12)
    }
}

private struct AgentQuickKeys: View {
    @ObservedObject var model: AgentControlModel
    var body: some View {
        let token = model.screenToken
        ScrollView(.horizontal, showsIndicators: false) {
            HStack(spacing: 6) {
                ForEach(AgentQuickKey.allCases) { key in
                    Button {
                        model.sendKey(key, screen: token)
                    } label: {
                        Text(key.label).font(.system(.footnote, design: .monospaced).weight(.medium))
                            .foregroundStyle(.primary)
                            .padding(.horizontal, 12).frame(minWidth: 44, minHeight: 32)
                            .background(Color(uiColor: .tertiarySystemFill), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                            // Compact key face, full 44-point hit area.
                            .frame(minHeight: 44).contentShape(Rectangle())
                    }.buttonStyle(.plain).accessibilityLabel(key.rawValue)
                }
            }.padding(.horizontal, 12)
        }.disabled(!model.controlEnabled).opacity(model.controlEnabled ? 1 : 0.4)
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
                    VStack(alignment: .leading, spacing: 8) {
                        HStack {
                            Text(item.status).font(.caption.weight(.semibold))
                            Spacer()
                        }
                        Text(item.text).font(.callout).lineLimit(4).textSelection(.enabled)
                        if let reason = item.reason { Text(reason).font(.caption).foregroundStyle(.secondary) }
                        HStack(spacing: 20) {
                            Button("Recover") { model.restoreRecovery(item.id, screen: screen); dismiss() }
                                .accessibilityHint("Append saved text below your current draft. Does not send.")
                            Button("Copy") {
                                guard model.screenToken == screen else { return }
                                UIPasteboard.general.string = item.text
                            }
                            Spacer()
                            Button("Dismiss", role: .destructive) { model.dismissRecovery(item.id, screen: screen) }
                                .disabled(model.composer.pending?.id == item.id)
                        }.font(.callout.weight(.medium)).buttonStyle(.borderless).frame(minHeight: 44)
                    }.padding(.vertical, 4)
                }
            }
            .overlay { if model.recoveries.isEmpty { ContentUnavailableView("No saved prompts", systemImage: "tray") } }
            .navigationTitle("Saved prompts").navigationBarTitleDisplayMode(.inline)
            .toolbar { ToolbarItem(placement: .topBarTrailing) { Button("Done") { dismiss() } } }
        }.presentationDetents([.medium, .large])
    }
}

// MARK: - Navigator

struct AgentNavigator: View {
    @ObservedObject var model: AgentControlModel
    let destination: String
    var hostName = "Mac"
    var hostStatus = ""
    let pointGuard: () -> Void
    let projects: () -> Void
    let open: (AgentShortcut) -> Void
    let connections: () -> Void
    let close: () -> Void
    @State private var removing: AgentShortcut?
    @State private var renaming: WorkspaceLaunchTarget?
    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Image("PurePointLogo").resizable().scaledToFit().frame(width: 28, height: 28).accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 1) {
                    Text("PurePoint").font(.headline)
                    HStack(spacing: 5) {
                        StatusDot(color: model.connected ? .green : Color(uiColor: .tertiaryLabel), size: 6)
                        Text(hostStatus.isEmpty ? hostName : hostName + " · " + hostStatus).lineLimit(1)
                    }.font(.caption).foregroundStyle(.secondary)
                }.accessibilityElement(children: .combine)
                Spacer()
                Button(action: close) { Image(systemName: "xmark").font(.body.weight(.medium)).frame(width: 44, height: 44) }.accessibilityLabel("Close navigator").keyboardShortcut(.cancelAction)
            }.padding(.leading, 20).padding(.trailing, 8).padding(.vertical, 8)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 2) {
                    destinationRow("Point Guard", selected: destination == "pointGuard", action: pointGuard) {
                        Image("PurePointLogo").resizable().scaledToFit().frame(width: 20, height: 20)
                    }
                    destinationRow("Projects", selected: destination != "pointGuard" && model.selected == nil, action: projects) {
                        Image(systemName: "folder").font(.body).foregroundStyle(Color.accentColor)
                    }
                    if model.permission == .off {
                        NoticeBanner(text: "Terminal access is off. Enable it in Connect Phone on your Mac.", systemImage: "lock.fill", tint: .secondary).padding(.top, 16)
                    }
                    if let error = model.error { NoticeBanner(text: error).padding(.top, 12) }
                    shortcutSection("Pinned", entries: model.pinnedShortcuts)
                    shortcutSection("Recent", entries: model.recentShortcuts)
                }.padding(.horizontal, 12).padding(.bottom, 20)
            }
            Divider()
            HStack {
                Button(action: connections) { Label("Connections", systemImage: "network").font(.callout.weight(.medium)).frame(minHeight: 44) }
                Spacer()
                Button { Task { await model.refresh() } } label: {
                    Group { if model.loading { ProgressView() } else { Image(systemName: "arrow.clockwise") } }.frame(width: 44, height: 44)
                }.disabled(!model.connected || model.permission == .off).accessibilityLabel("Refresh agents")
            }.padding(.horizontal, 20).padding(.vertical, 4)
        }.buttonStyle(.plain).foregroundStyle(.primary).background(Color(uiColor: .systemBackground))
        .sheet(item: $renaming) { target in WorkspaceLaunchForm(model: model, target: target) }
        .accessibilityAddTraits(.isModal)
        .accessibilityAction(.escape, close)
        .confirmationDialog("Remove from recents?", isPresented: Binding(get: { removing != nil }, set: { if !$0 { removing = nil } }), titleVisibility: .visible) {
            Button("Remove from recents", role: .destructive) { if let removing { model.hideShortcut(removing.scope) }; removing = nil }
        } message: { Text("Only this phone’s shortcut is removed and unpinned. The agent keeps running and remains in Projects. Files, native transcripts and saved drafts are kept.") }
    }
    private func destinationRow<Icon: View>(_ title: String, selected: Bool, action: @escaping () -> Void, @ViewBuilder icon: () -> Icon) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                icon().frame(width: 28, height: 28).accessibilityHidden(true)
                Text(title).font(.body.weight(.medium))
                Spacer()
            }.padding(.horizontal, 8).frame(minHeight: 46).contentShape(Rectangle())
        }
        .background(selected ? Color(uiColor: .secondarySystemBackground) : Color.clear, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .accessibilityAddTraits(selected ? .isSelected : [])
    }
    private func rename(_ entry: AgentShortcut) {
        guard let project = entry.agent.projectRoot else { return }
        renaming = WorkspaceLaunchTarget(host: entry.scope.host, machine: entry.machine.id, machineName: entry.machine.name, project: project, projectName: URL(fileURLWithPath: project).lastPathComponent, worktree: entry.agent.worktreeId, kind: "rename", agent: entry.agent.id)
    }
    private func available(_ entry: AgentShortcut) -> Bool { model.connected && entry.machine.online && entry.machine.metadataStatus != "error" }
    @ViewBuilder private func shortcutSection(_ title: String, entries: [AgentShortcut]) -> some View {
        if !entries.isEmpty {
            Text(title).font(.footnote.weight(.semibold)).foregroundStyle(.secondary).padding(.horizontal, 8).padding(.top, 20).padding(.bottom, 4)
            ForEach(entries) { entry in shortcutRow(entry) }
        }
    }
    private func shortcutRow(_ entry: AgentShortcut) -> some View {
        let isAvailable = available(entry)
        let project = entry.agent.projectRoot.map { URL(fileURLWithPath: $0).lastPathComponent } ?? "Project unavailable"
        return Button { if isAvailable { open(entry) } } label: {
            HStack(spacing: 12) {
                StatusDot(color: entry.agent.tone(available: isAvailable)).frame(width: 28)
                VStack(alignment: .leading, spacing: 2) {
                    Text(entry.agent.displayName).font(.callout.weight(.medium)).lineLimit(1)
                    Text("\(entry.machine.name) · \(project)").font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
                Spacer(minLength: 4)
                if !isAvailable { Text("Unavailable").font(.caption2).foregroundStyle(.tertiary) }
                if model.navigation.isPinned(entry.scope) { Image(systemName: "pin.fill").font(.caption2).foregroundStyle(.tertiary).accessibilityHidden(true) }
            }.padding(.horizontal, 8).frame(maxWidth: .infinity, minHeight: 52, alignment: .leading).contentShape(Rectangle())
        }
        .background(model.selected == entry.scope ? Color(uiColor: .secondarySystemBackground) : Color.clear, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        .accessibilityAddTraits(model.selected == entry.scope ? .isSelected : [])
        .accessibilityLabel([entry.agent.name, entry.machine.name, entry.agent.projectRoot, entry.agent.worktreeId, entry.agent.agentType, isAvailable ? entry.agent.state : "Unavailable", entry.agent.phase, entry.agent.idleLabel].compactMap { $0 }.joined(separator: ", "))
        .contextMenu {
            Button(model.navigation.isPinned(entry.scope) ? "Unpin" : "Pin", systemImage: model.navigation.isPinned(entry.scope) ? "pin.slash" : "pin") { model.togglePin(entry.scope) }
            Button("Rename…", systemImage: "pencil") { rename(entry) }.disabled(!model.canManage || entry.agent.projectRoot == nil)
            Button("Remove from recents", systemImage: "eye.slash", role: .destructive) { removing = entry }
        }
        .accessibilityAction(named: model.navigation.isPinned(entry.scope) ? "Unpin" : "Pin") { model.togglePin(entry.scope) }
        .accessibilityAction(named: "Rename") { if model.canManage { rename(entry) } }
        .accessibilityAction(named: "Remove from recents") { removing = entry }
    }
}

// MARK: - Projects, workspace and agent destinations

struct MobileProjectsView: View {
    @ObservedObject var model: AgentControlModel
    let hostName: String
    var active = true
    var openNavigator: () -> Void = {}
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
            } else if let machine, let project {
                workspace(machine, project)
            } else {
                projectList
            }
        }
        .toolbar { if active { toolbarContent } }
        .sheet(item: $launch) { target in WorkspaceLaunchSheet(model: model, target: target) }
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

    // MARK: Toolbar

    @ToolbarContentBuilder private var toolbarContent: some ToolbarContent {
        ToolbarItemGroup(placement: .topBarLeading) {
            ToolbarIconButton(systemName: "sidebar.left", label: "Open workspace navigator", action: openNavigator)
            if model.selected != nil {
                ToolbarIconButton(systemName: "chevron.backward", label: canLocate ? "Show in project" : "Back to Projects", action: back)
            } else if machine != nil && project != nil {
                Button(action: back) {
                    HStack(spacing: 3) { Image(systemName: "chevron.backward").font(.body.weight(.semibold)); Text("Projects") }
                }.accessibilityLabel("Back to Projects")
            }
        }
        // Lists and workspaces carry a large in-content title; only the terminal uses the bar.
        if model.selected != nil {
            ToolbarItem(placement: .principal) { agentPrincipal }
        }
        ToolbarItemGroup(placement: .topBarTrailing) {
            if model.selected != nil {
                agentMenu
            } else if let machine, let project {
                Menu {
                    Button("New agent", systemImage: "sparkles") { begin("spawn", machine, project) }
                    Button("New worktree", systemImage: "arrow.triangle.branch") { begin("createWorktree", machine, project) }
                    Button("New terminal", systemImage: "terminal") { begin("terminal", machine, project) }
                } label: {
                    HStack(spacing: 5) { Image(systemName: "plus").font(.footnote.weight(.bold)); Text("New").font(.subheadline.weight(.semibold)) }
                        .foregroundStyle(Color(uiColor: .systemBackground))
                        .padding(.horizontal, 12).padding(.vertical, 6)
                        .background(Color.accentColor, in: Capsule())
                }
                .disabled(!model.canManage || !machine.online || !project.available)
                .accessibilityLabel("New agent, worktree or terminal")
            } else {
                Button { Task { await model.refresh() } } label: {
                    if model.loading { ProgressView() } else { Image(systemName: "arrow.clockwise") }
                }.disabled(!model.connected || model.permission == .off).accessibilityLabel("Refresh projects")
            }
        }
    }
    @ViewBuilder private var agentPrincipal: some View {
        if let context = model.destinationContext {
            let location = context.worktreeLabel == "Project root" ? context.projectLabel + " · root" : context.projectLabel + " · " + context.worktreeLabel
            NavigationPrincipal(title: context.agentName, subtitle: context.machineName + " · " + location, tone: model.agent?.tone(available: model.connected && !context.status.hasPrefix("Unavailable")))
        }
    }
    private var agentMenu: some View {
        let token = model.screenToken
        return Menu {
            if let context = model.destinationContext {
                Section("\(context.projectLabel) · \(context.worktreeLabel)") {
                    Text(context.status)
                    Text(model.hasSize ? "Terminal \(model.cols) × \(model.rows)" : "Agent size unavailable")
                }
            }
            Button("Show in project", systemImage: "folder") { locateSelected() }.disabled(!canLocate)
            Button(canLocate ? "Refresh" : "Refresh metadata", systemImage: "arrow.clockwise") { Task { await model.refresh() } }
            if model.permission == .control {
                Toggle(isOn: Binding(get: { model.rawTyping }, set: { model.setRawTyping($0, screen: token) })) {
                    Label("Raw typing", systemImage: "keyboard")
                }.disabled(!model.controlEnabled)
            }
        } label: { Image(systemName: "ellipsis.circle") }
        .accessibilityLabel("Agent options")
    }
    private func back() {
        if model.selected != nil {
            if canLocate { locateSelected() } else { model.closeScreen() }
        } else { projectID = nil; machineID = nil }
    }
    private var selectedLocation: WorkspaceLocation? { model.selected.flatMap { WorkspaceLocation.locate($0, machines: model.machines) } }
    private var canLocate: Bool { selectedLocation != nil }
    private func locateSelected() {
        guard let location = selectedLocation, let scope = model.selected else { return }
        machineID = location.machine; projectID = location.project; highlighted = scope.agent
        memory.locate(location, agent: scope.agent)
        model.closeScreen()
    }

    // MARK: Project list

    private var projectList: some View {
        ScrollView { LazyVStack(alignment: .leading, spacing: 20) {
            VStack(alignment: .leading, spacing: 2) {
                Text("Projects").font(.largeTitle.weight(.bold))
                Text(listSummary).font(.footnote).foregroundStyle(.secondary)
            }.padding(.horizontal, 4).accessibilityAddTraits(.isHeader)
            listNotices
            ForEach(model.machines) { machine in machineSection(machine) }
            if model.machines.isEmpty && !model.loading && model.connected && model.permission != .off {
                ContentUnavailableView("No machines", systemImage: "desktopcomputer", description: Text("Pull to refresh, or register a project with pu on your Mac.")).padding(.top, 40)
            }
            if model.loading && model.machines.isEmpty { ProgressView("Loading projects").frame(maxWidth: .infinity).padding(.top, 40) }
        }.scrollTargetLayout().padding(.horizontal, 16).padding(.vertical, 12) }
        .background(Color(uiColor: .systemGroupedBackground))
        .refreshable { await model.refresh() }
        .scrollPosition(id: Binding(get: { memory.projectAnchors[model.bridgeIdentity] }, set: { memory.projectAnchors[model.bridgeIdentity] = $0 }))
    }
    private var listSummary: String {
        let machines = model.machines.count, agents = model.machines.reduce(0) { $0 + $1.agents.count }
        return "\(machines) machine\(machines == 1 ? "" : "s") · \(agents) agent\(agents == 1 ? "" : "s") · via \(hostName)"
    }
    @ViewBuilder private var listNotices: some View {
        if model.permission == .off { NoticeBanner(text: "Terminal access is off. Enable viewing or control in Connect Phone on your Mac.", systemImage: "lock.fill", tint: .secondary) }
        if !model.connected { NoticeBanner(text: "Bridge disconnected. Cached work is unavailable.", systemImage: "wifi.slash") }
        if let error = model.error { NoticeBanner(text: error) }
    }
    private func machineSection(_ machine: AgentMachine) -> some View {
        let projects = machine.registeredProjects ?? []
        let online = machine.online && model.connected
        return CardSection {
            HStack(spacing: 6) {
                Image(systemName: "desktopcomputer").accessibilityHidden(true)
                Text(machine.name).lineLimit(1)
                Spacer()
                StatusDot(color: online ? .green : Color(uiColor: .tertiaryLabel), size: 6)
                Text(online ? "Online" : "Offline").font(.caption)
            }.accessibilityElement(children: .combine)
        } content: {
            if !machine.online {
                Text(machine.error ?? "Machine offline").font(.footnote).foregroundStyle(.secondary).padding(.vertical, 10)
            }
            if machine.metadataStatus != "complete" {
                Label(machine.metadataError ?? "Project metadata incomplete or unsupported. Refresh to retry.", systemImage: "exclamationmark.triangle.fill")
                    .font(.footnote).foregroundStyle(.orange).padding(.vertical, 10)
            }
            ForEach(Array(projects.enumerated()), id: \.element.id) { index, project in
                if index > 0 { RowDivider() }
                projectRow(machine, project)
            }
            if machine.registeredProjects?.isEmpty == true {
                Text("No registered projects").font(.footnote).foregroundStyle(.secondary).padding(.vertical, 12)
            }
        }
    }
    private func projectRow(_ machine: AgentMachine, _ project: WorkspaceProject) -> some View {
        let agents = machine.agents.filter { $0.projectRoot == project.id }
        let worktrees = (machine.worktrees ?? []).filter { $0.projectId == project.id }.count
        let available = project.available && machine.online && model.connected
        let summary = available
            ? "\(agents.count) agent\(agents.count == 1 ? "" : "s")" + (worktrees > 0 ? " · \(worktrees) worktree\(worktrees == 1 ? "" : "s")" : "")
            : "Unavailable"
        return Button {
            machineID = machine.id; projectID = project.id
        } label: {
            HStack(spacing: 12) {
                IconTile(systemName: "folder.fill", tint: available ? .accentColor : Color(uiColor: .tertiaryLabel))
                VStack(alignment: .leading, spacing: 2) {
                    Text(project.name).font(.body.weight(.semibold)).foregroundStyle(.primary).lineLimit(1)
                    Text(summary).font(.caption).foregroundStyle(.secondary)
                    if let error = project.error { Text(error).font(.caption).foregroundStyle(.orange).lineLimit(2) }
                }
                Spacer(minLength: 4)
                Image(systemName: "chevron.right").font(.caption.weight(.semibold)).foregroundStyle(.tertiary).accessibilityHidden(true)
            }.padding(.vertical, 8).frame(minHeight: 56).contentShape(Rectangle())
        }.buttonStyle(.plain).accessibilityHint(project.id).id(WorkspaceLocation(host: model.bridgeIdentity, machine: machine.id, project: project.id, worktree: nil).storageKey)
    }

    // MARK: Workspace

    private func workspace(_ machine: AgentMachine, _ project: WorkspaceProject) -> some View {
        let workspaceKey = WorkspaceLocation(host: model.bridgeIdentity, machine: machine.id, project: project.id, worktree: nil).storageKey
        let rootAgents = machine.agents.filter { $0.projectRoot == project.id && $0.worktreeId == nil }
        let worktrees = (machine.worktrees ?? []).filter { $0.projectId == project.id }
        let missing = machine.agents.filter { agent in agent.projectRoot == project.id && agent.worktreeId != nil && !worktrees.contains(where: { $0.id == agent.worktreeId }) }
        return ScrollViewReader { proxy in
            ScrollView { LazyVStack(alignment: .leading, spacing: 20) {
                workspaceHeader(machine, project)
                CardSection {
                    Text("Project root").textCase(.uppercase)
                } content: {
                    ForEach(Array(rootAgents.enumerated()), id: \.element.id) { index, agent in
                        if index > 0 { RowDivider(inset: 32) }
                        workspaceAgent(machine, agent)
                    }
                    if rootAgents.isEmpty { emptyRow("No agents at the project root") }
                }
                ForEach(worktrees) { worktree in worktreeSection(machine, project, worktree) }
                if !missing.isEmpty {
                    CardSection {
                        Label("Worktree metadata unavailable", systemImage: "questionmark.folder")
                    } content: {
                        ForEach(Array(missing.enumerated()), id: \.element.id) { index, agent in
                            if index > 0 { RowDivider(inset: 32) }
                            workspaceAgent(machine, agent)
                        }
                    }
                }
            }.scrollTargetLayout().padding(.horizontal, 16).padding(.vertical, 12) }
            .background(Color(uiColor: .systemGroupedBackground))
            .refreshable { await model.refresh() }
            .scrollPosition(id: Binding(get: { memory.workspaceAnchors[workspaceKey] }, set: { memory.workspaceAnchors[workspaceKey] = $0 }))
            .onAppear { if let highlighted { proxy.scrollTo(highlighted, anchor: .center); focusedAgent = highlighted } }
        }
    }
    private func workspaceHeader(_ machine: AgentMachine, _ project: WorkspaceProject) -> some View {
        let available = machine.online && model.connected && project.available
        return VStack(alignment: .leading, spacing: 6) {
            Text(project.name).font(.largeTitle.weight(.bold)).lineLimit(1).accessibilityAddTraits(.isHeader)
            HStack(spacing: 6) {
                StatusDot(color: available ? .green : Color(uiColor: .tertiaryLabel), size: 6)
                Text(machine.name).lineLimit(1)
                Text("·").foregroundStyle(.tertiary)
                Text(project.id).font(.system(.caption, design: .monospaced)).lineLimit(1).truncationMode(.head).textSelection(.enabled)
            }.font(.footnote).foregroundStyle(.secondary)
            if !available { NoticeBanner(text: project.error ?? "Workspace unavailable").padding(.top, 6) }
            if machine.metadataStatus != "complete" { NoticeBanner(text: machine.metadataError ?? "Workspace metadata incomplete. Refresh to retry.").padding(.top, 6) }
            if !model.canManage {
                Label("Launching, renaming and stopping require workspace management and a separate grant in Connect Phone.", systemImage: "lock")
                    .font(.caption).foregroundStyle(.secondary).padding(.top, 4)
            }
        }.padding(.horizontal, 4)
    }
    private func worktreeSection(_ machine: AgentMachine, _ project: WorkspaceProject, _ worktree: WorkspaceWorktree) -> some View {
        let key = WorkspaceLocation(host: model.bridgeIdentity, machine: machine.id, project: project.id, worktree: worktree.id).storageKey
        let expanded = !memory.collapsed.contains(key)
        let agents = machine.agents.filter { $0.projectRoot == project.id && $0.worktreeId == worktree.id }
        let active = worktree.status.isEmpty || worktree.status.lowercased() == "active"
        return VStack(alignment: .leading, spacing: 0) {
            Button {
                withAnimation(.easeOut(duration: 0.18)) { if expanded { memory.collapsed.insert(key) } else { memory.collapsed.remove(key) } }
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: "arrow.triangle.branch").font(.footnote.weight(.semibold)).foregroundStyle(.secondary).accessibilityHidden(true)
                    Text(worktree.name).font(.subheadline.weight(.semibold)).foregroundStyle(active ? Color.primary : Color.secondary).lineLimit(1)
                    Spacer(minLength: 4)
                    Text(worktree.branch).font(.system(.caption2, design: .monospaced)).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                        .padding(.horizontal, 7).padding(.vertical, 2)
                        .background(Color(uiColor: .tertiarySystemFill), in: RoundedRectangle(cornerRadius: 6, style: .continuous))
                    if !active { Text(worktree.status.capitalized).font(.caption).foregroundStyle(.secondary) }
                    Image(systemName: "chevron.down").font(.caption2.weight(.bold)).foregroundStyle(.secondary)
                        .rotationEffect(.degrees(expanded ? 0 : -90)).accessibilityHidden(true)
                }
                .frame(minHeight: 44).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("\(worktree.name), \(worktree.branch), \(worktree.status)")
            .accessibilityValue(expanded ? "Expanded" : "Collapsed")
            if expanded {
                ForEach(agents) { agent in
                    RowDivider(inset: 32)
                    workspaceAgent(machine, agent)
                }
                if active {
                    RowDivider(inset: 32)
                    Button { begin("spawn", machine, project, worktree: worktree.id) } label: {
                        HStack(spacing: 12) {
                            Image(systemName: "plus").font(.footnote.weight(.bold)).frame(width: 20)
                            Text("Agent in this worktree").font(.callout.weight(.medium))
                        }.foregroundStyle(.secondary).frame(maxWidth: .infinity, minHeight: 44, alignment: .leading).contentShape(Rectangle())
                    }.buttonStyle(.plain).disabled(!model.canManage || !machine.online)
                }
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 2)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(uiColor: .secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
    private func emptyRow(_ text: String) -> some View {
        Text(text).font(.footnote).foregroundStyle(.secondary).frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
    }
    private func workspaceAgent(_ machine: AgentMachine, _ agent: ControlledAgent) -> some View {
        Button { model.open(machine: machine, agent: agent, context: model.contextToken) } label: { AgentListRow(agent: agent, available: model.connected && machine.online) }
            .buttonStyle(.plain).disabled(!model.connected || !machine.online).id(agent.id)
            .background(highlighted == agent.id ? Color.accentColor.opacity(0.15) : Color.clear, in: RoundedRectangle(cornerRadius: 8))
            .accessibilityAddTraits(highlighted == agent.id ? .isSelected : [])
            .accessibilityFocused($focusedAgent, equals: agent.id)
            .contextMenu {
                Button("Rename…", systemImage: "pencil") { begin("rename", machine, project ?? WorkspaceProject(id: agent.projectRoot ?? "", name: "Project", available: false, error: nil), agent: agent.id) }.disabled(!model.canManage)
                Button("Stop agent…", systemImage: "stop.circle", role: .destructive) { stop = model.shortcuts.first { $0.machine.id == machine.id && $0.agent.id == agent.id } }.disabled(!model.canManage)
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

/// New agent / worktree / terminal share one sheet with a tab per kind. Each kind keeps
/// its own saved form (and any in-flight operation) under its own target key.
struct WorkspaceLaunchSheet: View {
    @ObservedObject var model: AgentControlModel
    let target: WorkspaceLaunchTarget
    @State private var kind: String
    init(model: AgentControlModel, target: WorkspaceLaunchTarget) {
        self.model = model; self.target = target
        _kind = State(initialValue: target.kind)
    }
    private static let kinds = ["spawn", "createWorktree", "terminal"]
    var body: some View {
        if Self.kinds.contains(target.kind) {
            WorkspaceLaunchForm(
                model: model,
                target: WorkspaceLaunchTarget(host: target.host, machine: target.machine, machineName: target.machineName, project: target.project, projectName: target.projectName, worktree: target.worktree, kind: kind, agent: target.agent),
                kind: $kind
            ).id(kind)
        } else {
            WorkspaceLaunchForm(model: model, target: target)
        }
    }
}

struct WorkspaceLaunchForm: View {
    @ObservedObject var model: AgentControlModel
    let target: WorkspaceLaunchTarget
    var kind: Binding<String>? = nil
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
    private var hasInstructions: Bool { !draft.prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    // Mirrors the bridge: blank instructions add nothing to the preset.
    private func expandedPrompt(_ preset: WorkspaceCatalog.Preset) -> String { hasInstructions ? preset.prompt + "\n\n" + draft.prompt : preset.prompt }
    private var valid: Bool {
        guard model.canManage, model.bridgeIdentity == target.host, model.machines.contains(where: { $0.id == target.machine && $0.online && $0.metadataStatus != "error" && $0.registeredProjects?.contains(where: { $0.id == target.project && $0.available }) == true }), draft.operationId == nil, !submitting else { return false }
        if target.kind == "rename" || target.kind == "createWorktree" || (target.kind == "spawn" && draft.destination == "newWorktree") {
            guard !draft.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, draft.name.utf8.count <= 80, !draft.name.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else { return false }
        }
        if target.kind == "spawn" {
            guard catalog?.agents.contains(where: { $0.id == draft.agentType && $0.available }) == true,
                  draft.prompt.utf8.count <= 65536 else { return false }
            // Plan mode needs instructions or a preset; the daemon rejects an empty prompt in plan mode.
            if draft.planMode, !hasInstructions, draft.preset.isEmpty { return false }
        }
        if !draft.base.isEmpty && (target.kind == "createWorktree" || draft.destination == "newWorktree") {
            guard draft.base.utf8.count <= 128, draft.base.range(of: "^[A-Za-z0-9][A-Za-z0-9_./-]*$", options: .regularExpression) != nil, !draft.base.contains("..") else { return false }
        }
        if target.kind == "spawn", !draft.preset.isEmpty {
            guard let preset = catalog?.presets.first(where: { $0.id == draft.preset && $0.agentType == draft.agentType }), expandedPrompt(preset).utf8.count <= 65536 else { return false }
        }
        if target.kind == "terminal", catalog?.terminalAvailable != true { return false }
        if draft.destination == "worktree" && (target.kind == "spawn" || target.kind == "terminal") && !availableWorktrees.contains(where: { $0.id == draft.worktree }) { return false }
        return true
    }
    var body: some View {
        NavigationStack {
            Form {
                if let kind {
                    Section {
                        Picker("Create", selection: kind) {
                            Text("Agent").tag("spawn")
                            Text("Worktree").tag("createWorktree")
                            Text("Terminal").tag("terminal")
                        }.pickerStyle(.segmented).labelsHidden()
                    }.listRowBackground(Color.clear).listRowInsets(EdgeInsets()).disabled(draft.operationId != nil)
                }
                Section("Launch target") {
                    HStack(spacing: 12) {
                        IconTile(systemName: "folder.fill", tint: .accentColor)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(target.projectName).font(.headline)
                            Text(target.project).font(.system(.caption2, design: .monospaced)).foregroundStyle(.secondary)
                                .lineLimit(1).truncationMode(.head).textSelection(.enabled)
                        }
                    }
                    LabeledContent("Machine") { Text(target.machineName) }
                    if let agent = target.agent { LabeledContent("Agent") { Text(agent).font(.system(.callout, design: .monospaced)) } }
                    if target.kind == "spawn" || target.kind == "terminal" {
                        Picker("Where", selection: $draft.destination) {
                            Text("Root").tag("root")
                            if target.kind == "spawn" { Text("New worktree").tag("newWorktree") }
                            Text("Existing").tag("worktree")
                        }.pickerStyle(.segmented)
                        if draft.destination == "worktree" {
                            Picker("Worktree", selection: $draft.worktree) { Text("Select worktree").tag(""); ForEach(availableWorktrees) { Text("\($0.name) · \($0.branch)").tag($0.id) } }
                        }
                    }
                }.disabled(draft.operationId != nil)
                Section {
                    if target.kind == "spawn" {
                        Picker("Agent", selection: $draft.agentType) { Text("Select agent").tag(""); ForEach(catalog?.agents ?? []) { agent in if agent.available { Text(agent.name).tag(agent.id) } } }
                        Picker("Preset", selection: $draft.preset) { Text("None").tag(""); ForEach((catalog?.presets ?? []).filter { $0.agentType == draft.agentType }) { Text($0.name).tag($0.id) } }
                        TextField("Instructions (optional)", text: $draft.prompt, axis: .vertical).lineLimit(4...12)
                        Toggle("Plan mode", isOn: $draft.planMode).tint(.green).disabled(!hasInstructions && draft.preset.isEmpty) // An accent-tinted track hides the white knob in dark mode.
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
                        Text("\(target.machineName) / \(target.projectName) / \(draft.destination == "root" ? "project root" : draft.destination == "newWorktree" ? draft.name : draft.worktree)").font(.caption).foregroundStyle(.secondary)
                        Button(role: target.kind == "stop" ? .destructive : nil) { Task { await submit() } } label: {
                            Text(actionTitle).font(.body.weight(.semibold)).frame(maxWidth: .infinity, minHeight: 36)
                                // The accent is near-white in dark mode, so the default white label would vanish.
                                .foregroundStyle(target.kind == "stop" ? Color.white : Color(uiColor: .systemBackground))
                        }.buttonStyle(.borderedProminent).tint(target.kind == "stop" ? Color.red : Color.accentColor).disabled(!valid)
                    }
                }
            }
            .navigationTitle(kind != nil ? "New in \(target.projectName)" : target.kind == "rename" ? "Rename agent" : target.kind == "stop" ? "Stop agent" : "New \(target.kind == "createWorktree" ? "worktree" : target.kind == "spawn" ? "agent" : "terminal")")
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
    private var actionTitle: String {
        switch target.kind {
        case "spawn": return "Start agent"
        case "createWorktree": return "Create worktree"
        case "terminal": return "Open terminal"
        case "rename": return "Rename agent"
        case "stop": return "Confirm stop exact agent"
        default: return "Confirm \(target.kind)"
        }
    }
    private func save() { if let data = try? JSONEncoder().encode(draft) { UserDefaults.standard.set(data, forKey: key) } }
    private func loadCatalog() async {
        guard model.bridgeIdentity == target.host else { return }
        loading = true; defer { loading = false }
        do { catalog = try await model.catalog(machine: target.machine, project: target.project); error = nil } catch { self.error = error.localizedDescription }
    }
    private func submit() async {
        guard valid, let id = draft.beginOperation() else { return }
        save()
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
        do {
            let result = try await model.workspaceRequest("workspace." + target.kind, fields: fields)
            draft.state = result.state; draft.message = result.message
            // A launched agent opens immediately instead of leaving the sheet on a success message.
            if target.kind == "spawn", result.state == "succeeded", let agentId = result.detail?.agentId, openSpawned(agentId) { dismiss() }
        } catch {
            draft.markUncertain(error.localizedDescription)
            if error.localizedDescription.contains("workspace_duplicate"), let result = try? await model.operationStatus(id) { draft.state = result.state; draft.message = result.message }
        }
    }
    // Returns false when the refreshed inventory does not show the agent yet; the sheet then stays on its success message.
    private func openSpawned(_ agentId: String) -> Bool {
        guard let machine = model.machines.first(where: { $0.id == target.machine }),
              let agent = machine.agents.first(where: { $0.id == agentId }) else { return false }
        model.open(machine: machine, agent: agent, context: model.contextToken)
        return true
    }
    private func status(_ id: String) async {
        submitting = true; defer { submitting = false; save() }
        do { let result = try await model.operationStatus(id); draft.state = result.state; draft.message = result.message }
        catch { self.error = error.localizedDescription }
    }
}
