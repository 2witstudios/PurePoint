import SwiftUI

struct AgentsHubView: View {
    @Environment(AppState.self) private var appState

    private var hubState: AgentsHubState {
        appState.agentsHubState
    }

    @State private var activeTab: AgentsHubTab = .agents
    @State private var promptDraft = ""
    @State private var promptScope: PromptScopeChoice = .project
    @State private var promptAgent = ""

    private var projectRoot: String {
        appState.activeProjectRoot ?? appState.projects.first?.projectRoot ?? ""
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            tabBar
            Divider()
            content
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task {
            await hubState.loadAll(projectRoot: projectRoot)
        }
        .onChange(of: hubState.selectedPromptId) { _, _ in
            syncPromptEditor()
        }
        .sheet(isPresented: Bindable(hubState).showingCreatePrompt) {
            PromptCreationSheet(hubState: hubState, projectRoot: projectRoot)
        }
        .sheet(isPresented: Bindable(hubState).showingCreateAgent) {
            AgentCreationSheet(hubState: hubState, projectRoot: projectRoot)
        }
        .sheet(isPresented: Bindable(hubState).showingCreateSwarm) {
            SwarmCreationSheet(hubState: hubState, projectRoot: projectRoot)
        }
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 12) {
            Image(systemName: "cpu")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(.secondary)

            VStack(alignment: .leading, spacing: 3) {
                Text("Agents")
                    .font(.system(size: 15, weight: .semibold))

                Text("Reusable prompts, command-dialog agents, and multi-worktree swarms.")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
            }

            Spacer()

            MockBadge(text: "Hub", tint: .blue)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 12)
    }

    // MARK: - Tab Bar

    private var tabBar: some View {
        Picker("", selection: $activeTab) {
            ForEach(AgentsHubTab.allCases) { tab in
                Text(tab.title)
                    .tag(tab)
            }
        }
        .pickerStyle(.segmented)
        .padding(.horizontal, 18)
        .padding(.vertical, 10)
    }

    // MARK: - Content

    @ViewBuilder
    private var content: some View {
        if hubState.isLoading {
            VStack {
                Spacer()
                ProgressView("Loading...")
                Spacer()
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            VStack(spacing: 0) {
                if let error = hubState.error {
                    HStack(spacing: 8) {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(.yellow)
                        Text(error)
                            .font(.system(size: 12))
                            .lineLimit(2)
                        Spacer()
                        Button("Dismiss") {
                            hubState.error = nil
                        }
                        .buttonStyle(.borderless)
                        .font(.system(size: 11))
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 8)
                    .background(Color.red.opacity(0.1))
                }

                switch activeTab {
                case .prompts:
                    promptsContent
                case .agents:
                    agentsContent
                case .swarms:
                    swarmsContent
                }
            }
        }
    }

    // MARK: - Prompts

    private var promptsContent: some View {
        HStack(spacing: 0) {
            promptListPanel
                .frame(width: 300)

            Divider()

            if let prompt = hubState.selectedPrompt {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        CommandHintBar(
                            icon: "text.alignleft",
                            text: "Prompts can be stored globally or inside a project, then assigned to agents and swarms."
                        )

                        MockSurfaceCard(
                            title: "Prompt editor",
                            subtitle: "Edit reusable prompt template."
                        ) {
                            VStack(alignment: .leading, spacing: 12) {
                                HStack {
                                    Picker("Scope", selection: $promptScope) {
                                        ForEach(PromptScopeChoice.allCases) { scope in
                                            Text(scope.title)
                                                .tag(scope)
                                        }
                                    }
                                    .pickerStyle(.segmented)
                                    .frame(maxWidth: 220)

                                    Spacer()

                                    Picker("Agent", selection: $promptAgent) {
                                        ForEach(["", "claude", "codex", "opencode"], id: \.self) { t in
                                            Text(t.isEmpty ? "Any" : t).tag(t)
                                        }
                                    }
                                    .frame(maxWidth: 140)

                                    HStack(spacing: 8) {
                                        Button("Save") {
                                            Task {
                                                await hubState.saveTemplate(
                                                    projectRoot: projectRoot,
                                                    name: prompt.name,
                                                    description: prompt.description,
                                                    agent: promptAgent,
                                                    body: promptDraft,
                                                    scope: promptScope.wireValue
                                                )
                                            }
                                        }
                                        Button("Delete") {
                                            Task {
                                                await hubState.deleteTemplate(
                                                    projectRoot: projectRoot,
                                                    name: prompt.name,
                                                    scope: prompt.source
                                                )
                                            }
                                        }
                                        .foregroundStyle(.red)
                                    }
                                    .buttonStyle(.bordered)
                                    .controlSize(.small)
                                }

                                TextEditor(text: $promptDraft)
                                    .font(.system(size: 13, design: .monospaced))
                                    .frame(minHeight: 330)
                                    .padding(8)
                                    .background(Color.primary.opacity(0.035))
                                    .clipShape(RoundedRectangle(cornerRadius: 10))

                                HStack(alignment: .top) {
                                    VStack(alignment: .leading, spacing: 8) {
                                        Text("Detected variables")
                                            .font(.system(size: 12, weight: .medium))
                                            .foregroundStyle(.secondary)

                                        HStack(spacing: 8) {
                                            ForEach(prompt.variables, id: \.self) { variable in
                                                MockBadge(text: variable, tint: .orange)
                                            }
                                        }
                                    }

                                    Spacer()

                                    VStack(alignment: .trailing, spacing: 6) {
                                        Text("Source: \(prompt.source)")
                                            .font(.system(size: 12))
                                            .foregroundStyle(.secondary)
                                    }
                                }
                            }
                        }

                        MockSurfaceCard(
                            title: "Prompt usage",
                            subtitle: "Selected prompt details."
                        ) {
                            VStack(alignment: .leading, spacing: 10) {
                                usageRow(
                                    title: "Agent",
                                    value: prompt.agent.isEmpty ? "Any" : prompt.agent,
                                    icon: "cpu"
                                )
                                usageRow(
                                    title: "Description",
                                    value: prompt.description.isEmpty ? "None" : prompt.description,
                                    icon: "text.alignleft"
                                )
                                usageRow(
                                    title: "Source",
                                    value: prompt.source,
                                    icon: "globe"
                                )
                            }
                        }
                    }
                    .padding(20)
                }
            } else {
                emptyDetailState("Select a prompt or create one to get started.")
            }
        }
    }

    // MARK: - Agents

    private var agentsContent: some View {
        HStack(spacing: 0) {
            agentListPanel
                .frame(width: 300)

            Divider()

            if let agent = hubState.selectedAgent {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        CommandHintBar(
                            icon: "command",
                            text: "cmd+n -> @agentname -> spawn agent session"
                        )

                        MockSurfaceCard(
                            title: agent.name,
                            subtitle: "\(agent.agentType) agent. Scope: \(agent.scope)."
                        ) {
                            VStack(alignment: .leading, spacing: 14) {
                                HStack(spacing: 8) {
                                    Image(systemName: agent.icon ?? "cpu")
                                        .foregroundStyle(.secondary)
                                    ForEach(agent.tags, id: \.self) { tag in
                                        MockBadge(text: tag, tint: .purple)
                                    }
                                    MockBadge(text: agent.scope, tint: .green)
                                    Spacer()
                                    Text(agent.availableInCommandDialog ? "In command dialog" : "Not in command dialog")
                                        .font(.system(size: 12))
                                        .foregroundStyle(.secondary)
                                }

                                if let template = agent.template, !template.isEmpty {
                                    VStack(alignment: .leading, spacing: 8) {
                                        Text("Prompt template")
                                            .font(.system(size: 12, weight: .medium))
                                            .foregroundStyle(.secondary)

                                        Text(template)
                                            .font(.system(size: 12, design: .monospaced))
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                            .padding(12)
                                            .background(Color.primary.opacity(0.035))
                                            .clipShape(RoundedRectangle(cornerRadius: 10))
                                    }
                                }

                                if let inline = agent.inlinePrompt, !inline.isEmpty {
                                    VStack(alignment: .leading, spacing: 8) {
                                        Text("Inline prompt")
                                            .font(.system(size: 12, weight: .medium))
                                            .foregroundStyle(.secondary)

                                        Text(inline)
                                            .font(.system(size: 12, design: .monospaced))
                                            .frame(maxWidth: .infinity, alignment: .leading)
                                            .padding(12)
                                            .background(Color.primary.opacity(0.035))
                                            .clipShape(RoundedRectangle(cornerRadius: 10))
                                    }
                                }

                                HStack {
                                    Spacer()
                                    Button("Delete") {
                                        Task {
                                            await hubState.deleteAgentDef(
                                                projectRoot: projectRoot,
                                                name: agent.name,
                                                scope: agent.scope
                                            )
                                        }
                                    }
                                    .buttonStyle(.bordered)
                                    .foregroundStyle(.red)
                                    .controlSize(.small)
                                }
                            }
                        }
                    }
                    .padding(20)
                }
            } else {
                emptyDetailState("Select an agent or create one to get started.")
            }
        }
    }

    // MARK: - Swarms

    private var swarmsContent: some View {
        HStack(spacing: 0) {
            swarmListPanel
                .frame(width: 300)

            Divider()

            if let swarm = hubState.selectedSwarm {
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        CommandHintBar(
                            icon: "command",
                            text: "cmd+n -> @swarmname -> execute composition"
                        )

                        MockSurfaceCard(
                            title: swarm.name,
                            subtitle: "\(swarm.worktreeCount) worktrees \u{00B7} \(swarm.totalAgents) agents total"
                        ) {
                            VStack(alignment: .leading, spacing: 14) {
                                HStack(spacing: 10) {
                                    MockBadge(text: swarm.worktreeTemplate, tint: .blue)
                                    MockBadge(
                                        text: swarm.includeTerminal ? "Terminal attached" : "No terminal",
                                        tint: swarm.includeTerminal ? .green : .gray
                                    )
                                    Spacer()
                                }

                                if !swarm.roster.isEmpty {
                                    VStack(alignment: .leading, spacing: 8) {
                                        Text("Roster")
                                            .font(.system(size: 12, weight: .medium))
                                            .foregroundStyle(.secondary)

                                        ForEach(swarm.roster) { item in
                                            HStack(spacing: 10) {
                                                Text(item.agentDef)
                                                    .font(.system(size: 13, weight: .medium))
                                                Text(item.role)
                                                    .font(.system(size: 12))
                                                    .foregroundStyle(.secondary)
                                                Spacer()
                                                Text("x\(item.quantity)")
                                                    .font(.system(size: 12, weight: .medium))
                                                    .foregroundStyle(.secondary)
                                            }
                                            .padding(10)
                                            .background(Color.primary.opacity(0.035))
                                            .clipShape(RoundedRectangle(cornerRadius: 10))
                                        }
                                    }
                                }

                                VStack(alignment: .leading, spacing: 8) {
                                    Text("Composition summary")
                                        .font(.system(size: 12, weight: .medium))
                                        .foregroundStyle(.secondary)

                                    SwarmDiagramView(swarm: swarm)
                                }

                                HStack {
                                    Spacer()
                                    Button("Run") {
                                        Task {
                                            await hubState.runSwarm(
                                                projectRoot: projectRoot,
                                                name: swarm.name
                                            )
                                        }
                                    }
                                    .buttonStyle(.borderedProminent)
                                    .controlSize(.small)

                                    Button("Delete") {
                                        Task {
                                            await hubState.deleteSwarmDef(
                                                projectRoot: projectRoot,
                                                name: swarm.name,
                                                scope: swarm.scope
                                            )
                                        }
                                    }
                                    .buttonStyle(.bordered)
                                    .foregroundStyle(.red)
                                    .controlSize(.small)
                                }
                            }
                        }
                    }
                    .padding(20)
                }
            } else {
                emptyDetailState("Select a swarm or create one to get started.")
            }
        }
    }

    // MARK: - List Panels

    private var promptListPanel: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Prompt library")
                    .font(.system(size: 13, weight: .semibold))
                Spacer()
                Button {
                    hubState.showingCreatePrompt = true
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)

            Divider()

            if hubState.prompts.isEmpty {
                emptyListState("No prompts yet") {
                    hubState.showingCreatePrompt = true
                }
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(hubState.prompts) { prompt in
                            Button {
                                hubState.selectedPromptId = prompt.id
                            } label: {
                                PromptListRow(prompt: prompt, isSelected: hubState.selectedPromptId == prompt.id)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(12)
                }
            }
        }
    }

    private var agentListPanel: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Custom agents")
                    .font(.system(size: 13, weight: .semibold))
                Spacer()
                Button {
                    hubState.showingCreateAgent = true
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)

            Divider()

            if hubState.agents.isEmpty {
                emptyListState("No agents yet") {
                    hubState.showingCreateAgent = true
                }
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(hubState.agents) { agent in
                            Button {
                                hubState.selectedAgentId = agent.id
                            } label: {
                                AgentListRow(agent: agent, isSelected: hubState.selectedAgentId == agent.id)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(12)
                }
            }
        }
    }

    private var swarmListPanel: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Swarms")
                    .font(.system(size: 13, weight: .semibold))
                Spacer()
                Button {
                    hubState.showingCreateSwarm = true
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)

            Divider()

            if hubState.swarms.isEmpty {
                emptyListState("No swarms yet") {
                    hubState.showingCreateSwarm = true
                }
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(hubState.swarms) { swarm in
                            Button {
                                hubState.selectedSwarmId = swarm.id
                            } label: {
                                SwarmListRow(swarm: swarm, isSelected: hubState.selectedSwarmId == swarm.id)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(12)
                }
            }
        }
    }

    // MARK: - Helpers

    @ViewBuilder
    private func usageRow(title: String, value: String, icon: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: icon)
                .foregroundStyle(.secondary)
                .frame(width: 14)
            Text(title)
                .font(.system(size: 12))
            Spacer()
            Text(value)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(.secondary)
        }
    }

    private func syncPromptEditor() {
        guard let prompt = hubState.selectedPrompt else { return }
        promptScope = prompt.source == "global" ? .global : .project
        promptAgent = prompt.agent
        if !prompt.body.isEmpty {
            promptDraft = prompt.body
        }
        let capturedId = hubState.selectedPromptId
        Task {
            await hubState.loadPromptDetail(projectRoot: projectRoot, name: prompt.name)
            guard hubState.selectedPromptId == capturedId else { return }
            if let updated = hubState.selectedPrompt {
                promptDraft = updated.body
                promptAgent = updated.agent
            }
        }
    }

    private func emptyDetailState(_ message: String) -> some View {
        VStack(spacing: 12) {
            Text(message)
                .font(.system(size: 14))
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func emptyListState(_ message: String, onCreate: @escaping () -> Void) -> some View {
        VStack(spacing: 12) {
            Text(message)
                .font(.system(size: 13))
                .foregroundStyle(.secondary)
            Button("Create") {
                onCreate()
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

// MARK: - Tab Enum

enum AgentsHubTab: String, CaseIterable, Identifiable {
    case prompts
    case agents
    case swarms

    var id: String { rawValue }

    var title: String {
        rawValue.capitalized
    }
}

// MARK: - Helper Views

private struct CommandHintBar: View {
    let icon: String
    let text: String

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: icon)
                .foregroundStyle(.secondary)
            Text(text)
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(.secondary)
            Spacer()
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 10)
        .background(Color.primary.opacity(0.04))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

private struct PromptListRow: View {
    let prompt: SavedPrompt
    let isSelected: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(prompt.name)
                    .font(.system(size: 13, weight: .medium))
                Spacer()
                MockBadge(
                    text: prompt.source == "global" ? "Global" : "Project",
                    tint: prompt.source == "global" ? .blue : .green
                )
            }

            Text(prompt.description.isEmpty ? prompt.agent : prompt.description)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(isSelected ? Color.accentColor.opacity(0.12) : Color.primary.opacity(0.03))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

private struct AgentListRow: View {
    let agent: AgentDefinition
    let isSelected: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: agent.icon ?? "cpu")
                    .foregroundStyle(.secondary)
                Text(agent.name)
                    .font(.system(size: 13, weight: .medium))
                Spacer()
                MockBadge(text: agent.scope, tint: .green)
            }

            Text(agent.agentType)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(isSelected ? Color.accentColor.opacity(0.12) : Color.primary.opacity(0.03))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

private struct SwarmListRow: View {
    let swarm: SwarmDefinition
    let isSelected: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(swarm.name)
                    .font(.system(size: 13, weight: .medium))
                Spacer()
                MockBadge(text: swarm.scope, tint: .green)
            }

            Text("\(swarm.worktreeCount) worktrees \u{00B7} \(swarm.totalAgents) agents")
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(isSelected ? Color.accentColor.opacity(0.12) : Color.primary.opacity(0.03))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

private struct SwarmDiagramView: View {
    let swarm: SwarmDefinition

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            ForEach(1 ... max(swarm.worktreeCount, 1), id: \.self) { index in
                VStack(alignment: .leading, spacing: 8) {
                    Text("Worktree \(index)")
                        .font(.system(size: 12, weight: .semibold))
                    Text(swarm.worktreeTemplate.replacingOccurrences(of: "{index}", with: "\(index)"))
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundStyle(.secondary)

                    ForEach(swarm.roster) { item in
                        HStack(spacing: 6) {
                            Image(systemName: "cpu")
                                .font(.system(size: 11))
                                .foregroundStyle(.secondary)
                            Text("\(item.agentDef) x\(item.quantity)")
                                .font(.system(size: 11))
                            Spacer()
                        }
                    }

                    if swarm.includeTerminal {
                        Divider()
                        Text("Terminal panel")
                            .font(.system(size: 11, weight: .medium))
                            .foregroundStyle(.secondary)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(12)
                .background(Color.primary.opacity(0.035))
                .clipShape(RoundedRectangle(cornerRadius: 10))
            }
        }
    }
}
