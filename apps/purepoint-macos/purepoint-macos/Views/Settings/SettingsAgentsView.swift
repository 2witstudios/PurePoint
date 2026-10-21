import SwiftUI

// MARK: - Main View

struct SettingsAgentsView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        let state = appState.agentConfigState
        VStack(alignment: .leading, spacing: 16) {
            GroupBox {
                VStack(alignment: .leading, spacing: 8) {
                    Toggle("YOLO for all projects", isOn: Binding(
                        get: { state.codexYolo },
                        set: { enabled in
                            Task { await state.updateGlobalSettings(codexYolo: enabled, projectRoot: appState.activeLocalProjectRoot) }
                        }
                    ))
                    .toggleStyle(.switch)
                    .disabled(!state.globalSettingsLoaded || state.isSavingGlobalSettings)
                    Text("Codex gets full access with no command approvals in every project on this Mac. Applies to new and resumed sessions; restart running agents to apply it.")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                    Toggle("Agent status tracking", isOn: Binding(
                        get: { state.agentStatusTracking },
                        set: { enabled in
                            Task { await state.updateAgentStatusTracking(enabled) }
                        }
                    ))
                    .toggleStyle(.switch)
                    .disabled(!state.globalSettingsLoaded || state.isSavingGlobalSettings)
                    Text("Show whether Claude Code, Codex, OpenCode and Pi agents are working, idle or waiting on you. Works with any version of those tools. Applies to new and resumed sessions.")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                    Divider()
                    Text("Launch args for all projects")
                        .font(.system(size: 12, weight: .medium))
                    Text("Appended to the command in every project on this Mac that doesn't set its own. Agents launch with the bare command when this is empty.")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                    ForEach(launchArgsAgents, id: \.self) { agent in
                        LaunchArgsRow(
                            command: state.agentConfig(named: agent)?.command ?? agent,
                            stored: state.machineLaunchArgs[agent] ?? "",
                            placeholder: launchArgsPlaceholder
                        ) { text in
                            Task {
                                await state.updateMachineLaunchArgs(
                                    agentName: agent, launchArgs: text, projectRoot: appState.activeLocalProjectRoot)
                            }
                        }
                        .disabled(!state.globalSettingsLoaded || state.isSavingGlobalSettings)
                    }
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
            if let projectRoot = appState.activeLocalProjectRoot {
                SettingsAgentsContentView(projectRoot: projectRoot)
            } else {
                ContentUnavailableView(
                    "No Project Open",
                    systemImage: "folder.badge.questionmark",
                    description: Text("Open a project to configure agent launch settings.")
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
                    VStack(alignment: .leading, spacing: 10) {
                        Text("Appended to the command for this project. Leave empty to use the launch args for all projects. To launch the bare command here regardless, set launchArgs: \"\" for the agent in .pu/config.yaml.")
                            .font(.system(size: 11))
                            .foregroundStyle(.secondary)
                        ForEach(launchArgsAgents, id: \.self) { agent in
                            if let payload = state.agentConfig(named: agent) {
                                if agent == "codex" && state.codexYolo {
                                    VStack(alignment: .leading, spacing: 2) {
                                        Text("Codex: YOLO is enabled for all projects. Turn it off above to use this project's launch args.")
                                            .font(.system(size: 11))
                                            .foregroundStyle(.secondary)
                                        resolvedCommandView(payload)
                                    }
                                } else {
                                    VStack(alignment: .leading, spacing: 2) {
                                        LaunchArgsRow(
                                            command: payload.command,
                                            stored: payload.projectLaunchArgs
                                                ?? payload.launchArgs?.joined(separator: " ") ?? "",
                                            placeholder: payload.projectLaunchArgs == ""
                                                ? "" : payload.machineLaunchArgs ?? launchArgsPlaceholder
                                        ) { text in
                                            Task {
                                                await state.updateLaunchArgs(
                                                    projectRoot: projectRoot, agentName: agent,
                                                    launchArgs: text.trimmingCharacters(in: .whitespaces).isEmpty ? nil : text)
                                            }
                                        }
                                        if payload.projectLaunchArgs == "" {
                                            Text("Set to empty in .pu/config.yaml: launches the bare command, ignoring the launch args for all projects.")
                                                .font(.system(size: 11))
                                                .foregroundStyle(.tertiary)
                                        }
                                        resolvedCommandView(payload)
                                    }
                                }
                            }
                        }
                    }
                } label: {
                    Label("Launch Args for This Project", systemImage: "terminal")
                }
                .groupBoxStyle(SettingsGroupBoxStyle())
            }
        }
        .task(id: projectRoot) {
            await state.load(projectRoot: projectRoot)
        }
    }
}

// MARK: - Launch Args

/// Agent types whose launch args can be configured. PurePoint knows only their
/// base commands; extra args are a free-form string the user owns.
private let launchArgsAgents = ["claude", "codex", "opencode"]

private let launchArgsPlaceholder = "--model opus"

/// One agent's launch args: the read-only base command and a free-form text field.
struct LaunchArgsRow: View {
    let command: String
    let stored: String
    let placeholder: String
    let onSave: (String) -> Void

    @State private var text = ""

    var body: some View {
        LabeledContent {
            HStack(spacing: 6) {
                TextField(placeholder, text: $text)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: 12, design: .monospaced))
                    .onSubmit { save() }
                Button("Save") { save() }
                    .controlSize(.small)
                    .disabled(text == stored)
            }
        } label: {
            Text(command)
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(.secondary)
                .frame(minWidth: 70, alignment: .leading)
        }
        .onAppear { text = stored }
        .onChange(of: stored) { text = stored }
    }

    private func save() {
        guard text != stored else { return }
        onSave(text)
    }
}

// MARK: - Shared

private func resolvedCommandView(_ payload: AgentConfigPayload) -> some View {
    Text(payload.resolvedCommand ?? ([payload.command] + payload.resolvedLaunchArgs).joined(separator: " "))
        .font(.system(size: 11, design: .monospaced))
        .foregroundStyle(.tertiary)
        .textSelection(.enabled)
        .lineLimit(1)
        .truncationMode(.middle)
        .frame(maxWidth: .infinity, alignment: .leading)
}
