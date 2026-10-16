import SwiftUI

struct AgentsView: View {
    @ObservedObject var model: AgentControlModel
    let hostName: String
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        let token = model.contextToken
        NavigationStack {
            Group {
                if model.permission == .off {
                    ContentUnavailableView(
                        "Terminal access is off", systemImage: "lock",
                        description: Text(
                            "Enable terminal viewing or control for this phone in Connect Phone on your Mac."))
                } else {
                    List {
                        if !model.connected {
                            Text("Mac disconnected. Reconnect from Pi chat.").foregroundStyle(.secondary)
                        }
                        if let error = model.error { Text(error).foregroundStyle(.orange).textSelection(.enabled) }
                        ForEach(model.machines) { machine in
                            Section {
                                if !machine.online {
                                    Label(machine.error ?? "Machine is offline", systemImage: "network.slash")
                                        .foregroundStyle(.secondary)
                                } else {
                                    if !machine.complete {
                                        Text("Agent inventory is incomplete. Refresh to try again.").foregroundStyle(
                                            .orange)
                                    }
                                    ForEach(machine.projects) { project in
                                        VStack(alignment: .leading, spacing: 8) {
                                            Text(project.name).font(.headline)
                                            if !project.id.isEmpty {
                                                Text(project.id).font(.caption2).foregroundStyle(.secondary).lineLimit(
                                                    2)
                                            }
                                            ForEach(project.agents) { agent in
                                                Button {
                                                    model.open(machine: machine, agent: agent, context: token)
                                                } label: {
                                                    AgentListRow(agent: agent)
                                                }.buttonStyle(.plain).disabled(!model.connected)
                                            }
                                        }.padding(.vertical, 4)
                                    }
                                    if machine.agents.isEmpty { Text("No running agents").foregroundStyle(.secondary) }
                                }
                            } header: {
                                HStack {
                                    Text(machine.name); Spacer(); Text(machine.online ? "Online" : "Offline")
                                }
                            }
                        }
                        if model.machines.isEmpty && !model.loading {
                            Text("No machines available. Refresh to try again.").foregroundStyle(.secondary)
                        }
                    }
                    .refreshable { await model.refresh() }
                    .overlay { if model.loading && model.machines.isEmpty { ProgressView() } }
                }
            }
            .navigationTitle("Agents · \(hostName)")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarLeading) { Button("Pi chat") { dismiss() } }
                ToolbarItem(placement: .topBarTrailing) {
                    Button {
                        Task { await model.refresh() }
                    } label: {
                        Image(systemName: "arrow.clockwise")
                    }
                    .disabled(!model.connected || model.permission == .off).accessibilityLabel("Refresh agents")
                }
            }
            .navigationDestination(
                isPresented: Binding(get: { model.selected != nil }, set: { if !$0 { model.closeScreen() } })
            ) {
                AgentControlView(model: model, screen: model.screenToken)
            }
            .task {
                await model.refresh()
                // Inventory is an explicit read; no background socket fleet or resend.
                while !Task.isCancelled {
                    try? await Task.sleep(nanoseconds: 5_000_000_000)
                    guard !Task.isCancelled else { return }
                    await model.refresh()
                }
            }
        }
    }
}

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

struct AgentControlView: View {
    @ObservedObject var model: AgentControlModel
    let screen: Int
    var body: some View {
        VStack(spacing: 0) {
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
            if let explanation = model.sendExplanation { Text(explanation).font(.caption).foregroundStyle(.secondary) }
            if let outcome = model.composer.outcome {
                Text(outcome.label + (model.composer.reason.map { " · " + $0 } ?? ""))
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
