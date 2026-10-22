import SwiftUI

// MARK: - Main View

struct SettingsAgentsView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        let state = appState.agentConfigState
        VStack(alignment: .leading, spacing: 16) {
            GroupBox {
                VStack(alignment: .leading, spacing: 8) {
                    Toggle("Agent status tracking", isOn: Binding(
                        get: { state.agentStatusTracking },
                        set: { enabled in
                            Task { await state.updateAgentStatusTracking(enabled) }
                        }
                    ))
                    .toggleStyle(.switch)
                    .disabled(!state.globalSettingsLoaded || state.isSavingGlobalSettings)
                    Text("Show whether Claude Code, Codex, OpenCode and Point Guard agents are working, idle or waiting on you. Works with any version of those tools. Applies to new and resumed sessions.")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                    if let error = state.globalError {
                        Text(error).font(.system(size: 12)).foregroundStyle(.red)
                        Button("Retry") { Task { await state.loadGlobalSettings() } }
                    }
                }
            } label: {
                #if os(macOS)
                Label("Machine Settings", systemImage: "desktopcomputer")
                #else
                // Names the machine whose daemon these settings change (IPAD-012).
                Label("Machine Settings — \(appState.libraryMachine?.machine.name ?? "No machine")", systemImage: "desktopcomputer")
                #endif
            }
            .groupBoxStyle(SettingsGroupBoxStyle())

            GroupBox {
                AgentLaunchTable(
                    caption: "What runs in the terminal: launch command, then arguments.",
                    rows: state.machineAgentNames.map { name in
                        AgentLaunchRowModel(
                            agentType: name,
                            command: state.machineCommands[name] ?? "",
                            launchArgs: state.machineLaunchArgs[name] ?? "",
                            commandHint: name,
                            launchArgsHint: "",
                            removable: !AgentLaunchDefaults.builtIns.contains(name))
                    },
                    onSave: { row, command, args in
                        await state.updateMachineAgent(
                            agentName: row.agentType, command: command, launchArgs: args,
                            projectRoot: appState.activeLocalProjectRoot)
                    },
                    onAdd: { command, args in
                        await state.addMachineAgent(
                            command: command, launchArgs: args, projectRoot: appState.activeLocalProjectRoot)
                    },
                    onRemove: { row in
                        await state.removeMachineAgent(
                            agentName: row.agentType, projectRoot: appState.activeLocalProjectRoot)
                    }
                )
                .disabled(!state.globalSettingsLoaded || state.isSavingGlobalSettings)
            } label: {
                Label("Agents — All Projects", systemImage: "desktopcomputer")
            }
            .groupBoxStyle(SettingsGroupBoxStyle())

            if let projectRoot = appState.activeLocalProjectRoot {
                SettingsAgentsContentView(projectRoot: projectRoot)
            } else {
                ContentUnavailableView(
                    "No Project Open",
                    systemImage: "folder.badge.questionmark",
                    description: Text("Open a project to set its agents.")
                )
            }
        }
        #if os(macOS)
        .task { await state.loadGlobalSettings() }
        #else
        // Reloads when the library machine changes (IPAD-012).
        .task(id: appState.libraryScopeId) { await state.loadGlobalSettings() }
        #endif
    }
}

struct SettingsAgentsContentView: View {
    let projectRoot: String
    @Environment(AppState.self) private var appState

    var body: some View {
        let state = appState.agentConfigState
        VStack(alignment: .leading, spacing: 16) {
            if let error = state.error {
                HStack {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.yellow)
                    Text(error)
                        .font(.system(size: 12))
                        .foregroundStyle(.secondary)
                }
            }

            if state.isLoading && state.agents.isEmpty {
                ProgressView()
                    .frame(maxWidth: .infinity, alignment: .center)
            } else {
                GroupBox {
                    AgentLaunchTable(
                        caption: "Empty fields use the All Projects value, shown in grey.",
                        rows: state.agents.map(projectRow),
                        onSave: { row, command, args in
                            await state.updateProjectAgent(
                                projectRoot: projectRoot, agentName: row.agentType, command: command,
                                launchArgs: args)
                        },
                        onAdd: { command, args in
                            await state.addProjectAgent(projectRoot: projectRoot, command: command, launchArgs: args)
                        },
                        onRemove: { row in
                            await state.removeProjectAgent(projectRoot: projectRoot, agentName: row.agentType)
                        }
                    )
                } label: {
                    Label("Agents — This Project", systemImage: "folder")
                }
                .groupBoxStyle(SettingsGroupBoxStyle())
            }
        }
        .task(id: projectRoot) {
            await state.load(projectRoot: projectRoot)
        }
    }

    /// A project row's grey hints are exactly what runs when its field is left empty.
    private func projectRow(_ payload: AgentConfigPayload) -> AgentLaunchRowModel {
        // An explicit empty `launchArgs` in .pu/config.yaml launches the bare command.
        let launchArgsHint = payload.projectLaunchArgs == "" ? "" : payload.machineLaunchArgs ?? ""
        return AgentLaunchRowModel(
            agentType: payload.agentType,
            command: payload.projectCommand ?? "",
            launchArgs: payload.projectLaunchArgs ?? "",
            commandHint: payload.inheritedCommand ?? payload.command,
            launchArgsHint: launchArgsHint,
            removable: payload.origin == "project")
    }
}

// MARK: - Launch table

/// One agent in a launch table: what is stored at this level, and what runs when it is empty.
struct AgentLaunchRowModel: Identifiable, Equatable {
    let agentType: String
    let command: String
    let launchArgs: String
    let commandHint: String
    let launchArgsHint: String
    let removable: Bool

    var id: String { agentType }
}

/// Launch command and arguments, one row per agent, plus rows for new agents.
struct AgentLaunchTable: View {
    let caption: String
    let rows: [AgentLaunchRowModel]
    let onSave: (AgentLaunchRowModel, String, String) async -> Void
    let onAdd: (String, String) async -> Void
    let onRemove: (AgentLaunchRowModel) async -> Void

    /// New rows not saved yet: an agent exists once its command is entered.
    @State private var drafts: [UUID] = []

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(caption)
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Text("Launch command").frame(width: AgentLaunchRowView.commandWidth, alignment: .leading)
                    Text("Arguments").frame(maxWidth: .infinity, alignment: .leading)
                    Color.clear.frame(width: AgentLaunchRowView.removeWidth, height: 1)
                }
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(.secondary)
                ForEach(rows) { row in
                    AgentLaunchRowView(
                        command: row.command, launchArgs: row.launchArgs,
                        commandHint: row.commandHint, launchArgsHint: row.launchArgsHint,
                        onSave: { command, args in await onSave(row, command, args) },
                        onRemove: row.removable ? { await onRemove(row) } : nil)
                }
                ForEach(drafts, id: \.self) { draft in
                    AgentLaunchRowView(
                        command: "", launchArgs: "", commandHint: "command", launchArgsHint: "",
                        onSave: { command, args in
                            guard !command.trimmingCharacters(in: .whitespaces).isEmpty else { return }
                            drafts.removeAll { $0 == draft }
                            await onAdd(command, args)
                        },
                        onRemove: { drafts.removeAll { $0 == draft } })
                }
            }
            Button {
                drafts.append(UUID())
            } label: {
                Label("Add agent", systemImage: "plus")
            }
            .controlSize(.small)
        }
    }
}

/// Two text fields, saved together when either is submitted or loses focus.
private struct AgentLaunchRowView: View {
    let command: String
    let launchArgs: String
    let commandHint: String
    let launchArgsHint: String
    let onSave: (String, String) async -> Void
    /// Nil for agents that can't be removed at this level.
    let onRemove: (() async -> Void)?

    static let commandWidth: CGFloat = 200
    static let removeWidth: CGFloat = 20

    private enum Field { case command, launchArgs }

    @State private var commandText = ""
    @State private var argsText = ""
    @FocusState private var focus: Field?

    var body: some View {
        HStack(spacing: 8) {
            field(.command, hint: commandHint, text: $commandText, label: "Launch command")
                .frame(width: Self.commandWidth)
            field(.launchArgs, hint: launchArgsHint, text: $argsText, label: "Arguments")
                .frame(maxWidth: .infinity)
            if let onRemove {
                Button {
                    Task { await onRemove() }
                } label: {
                    Image(systemName: "xmark")
                }
                .buttonStyle(.borderless)
                .accessibilityLabel("Remove agent")
                .frame(width: Self.removeWidth)
            } else {
                Color.clear.frame(width: Self.removeWidth, height: 1)
            }
        }
        .onAppear {
            commandText = command
            argsText = launchArgs
        }
        .onChange(of: command) { commandText = command }
        .onChange(of: launchArgs) { argsText = launchArgs }
        // Moving between the row's two fields keeps focus in the row; leaving it saves.
        .onChange(of: focus) { if focus == nil { save() } }
    }

    private func field(_ which: Field, hint: String, text: Binding<String>, label: String) -> some View {
        TextField(label, text: text, prompt: Text(hint))
            .labelsHidden()
            .textFieldStyle(.roundedBorder)
            .font(.system(size: 12, design: .monospaced))
            .autocorrectionDisabled()
            #if os(iOS)
            .textInputAutocapitalization(.never)
            #endif
            .focused($focus, equals: which)
            .onSubmit { save() }
    }

    private func save() {
        guard commandText != command || argsText != launchArgs else { return }
        let (command, args) = (commandText, argsText)
        Task { await onSave(command, args) }
    }
}
