import SwiftUI

// MARK: - Selectable Row Style

private struct SelectableRowStyle: ViewModifier {
    let isSelected: Bool

    func body(content: Content) -> some View {
        content
            .padding(12)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(isSelected ? Color.accentColor.opacity(0.12) : Color.primary.opacity(0.03))
            .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

extension View {
    fileprivate func selectableRow(isSelected: Bool) -> some View {
        modifier(SelectableRowStyle(isSelected: isSelected))
    }
}

// MARK: - AgentsHubView

struct AgentsHubView: View {
    @Environment(AppState.self) private var appState

    private var hubState: AgentsHubState {
        appState.agentsHubState
    }

    private var triggersState: TriggersState {
        appState.triggersState
    }

    @State private var activeTab: AgentsHubTab = .agents
    @State private var promptDraft = ""
    @State private var promptScope: PromptScopeChoice = .project
    @State private var promptAgent = ""
    @State private var promptCommand = ""

    private var projectRoots: [String] {
        appState.projects.map(\.projectRoot)
    }

    private var anyProjectRoot: String {
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
            await hubState.loadAll(projectRoots: projectRoots)
            await triggersState.loadTriggers(projectRoots: projectRoots)
        }
        .onChange(of: appState.projects.count) { _, _ in
            Task {
                await hubState.loadAll(projectRoots: projectRoots)
                await triggersState.loadTriggers(projectRoots: projectRoots)
            }
        }
        .onChange(of: hubState.selectedPromptId) { _, _ in
            syncPromptEditor()
        }
        .sheet(isPresented: Bindable(hubState).showingCreatePrompt) {
            PromptCreationSheet(hubState: hubState)
        }
        .sheet(isPresented: Bindable(hubState).showingCreateAgent) {
            AgentCreationSheet(hubState: hubState)
        }
        .sheet(isPresented: Bindable(hubState).showingCreateSwarm) {
            SwarmCreationSheet(hubState: hubState)
        }
        .sheet(isPresented: Bindable(triggersState).showingCreationSheet) {
            TriggerCreationSheet(state: triggersState)
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
                    InlineErrorBanner(message: error) {
                        hubState.error = nil
                    }
                }

                switch activeTab {
                case .prompts:
                    promptsContent
                case .agents:
                    agentsContent
                case .swarms:
                    swarmsContent
                case .triggers:
                    triggersContent
                }
            }
        }
    }

    // MARK: - Prompts

    private var promptsContent: some View {
        masterDetail(
            list: listPanel(
                title: "Prompt library",
                items: hubState.prompts,
                selectedId: hubState.selectedPromptId,
                onSelect: { hubState.selectedPromptId = $0 },
                onCreate: { hubState.showingCreatePrompt = true },
                emptyMessage: "No prompts yet"
            ) { prompt, isSelected in
                PromptListRow(prompt: prompt, isSelected: isSelected)
            },
            hasSelection: hubState.selectedPrompt != nil,
            emptyMessage: "Select a prompt or create one to get started."
        ) {
            if let prompt = hubState.selectedPrompt {
                promptDetailView(prompt)
            }
        }
    }

    private func promptDetailView(_ prompt: SavedPrompt) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                CommandHintBar(
                    icon: "text.alignleft",
                    text:
                        "Prompts can be stored globally or inside a project, then assigned to agents and swarms."
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
                                ForEach(AgentTypes.withAny, id: \.self) { t in
                                    Text(t.isEmpty ? "Any" : t).tag(t)
                                }
                            }
                            .frame(maxWidth: 140)

                            if promptAgent == "terminal" {
                                TextField("Command (e.g. npm run dev)", text: $promptCommand)
                                    .textFieldStyle(.roundedBorder)
                                    .frame(maxWidth: 200)
                            }

                            HStack(spacing: 8) {
                                Button("Save") {
                                    Task {
                                        let trimmed =
                                            promptCommand
                                            .trimmingCharacters(
                                                in: .whitespaces)
                                        let cmd =
                                            promptAgent == "terminal"
                                                && !trimmed.isEmpty
                                            ? trimmed : nil
                                        let saveRoot = prompt.projectRoot ?? anyProjectRoot
                                        await hubState.saveTemplate(
                                            projectRoot: saveRoot,
                                            projectRoots: projectRoots,
                                            name: prompt.name,
                                            description: prompt.description,
                                            agent: promptAgent,
                                            body: promptDraft,
                                            scope: promptScope.wireValue,
                                            command: cmd
                                        )
                                    }
                                }
                                Button("Delete") {
                                    Task {
                                        let deleteRoot = prompt.projectRoot ?? anyProjectRoot
                                        await hubState.deleteTemplate(
                                            projectRoot: deleteRoot,
                                            projectRoots: projectRoots,
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
                                sectionLabel("Detected variables")

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
    }

    // MARK: - Agents

    private var agentsContent: some View {
        masterDetail(
            list: listPanel(
                title: "Custom agents",
                items: hubState.agents,
                selectedId: hubState.selectedAgentId,
                onSelect: { hubState.selectedAgentId = $0 },
                onCreate: { hubState.showingCreateAgent = true },
                emptyMessage: "No agents yet"
            ) { agent, isSelected in
                AgentListRow(agent: agent, isSelected: isSelected)
            },
            hasSelection: hubState.selectedAgent != nil,
            emptyMessage: "Select an agent or create one to get started."
        ) {
            if let agent = hubState.selectedAgent {
                agentDetailView(agent)
            }
        }
    }

    private func agentDetailView(_ agent: AgentDefinition) -> some View {
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

                        codeSection(label: "Prompt template", content: agent.template)
                        codeSection(label: "Inline prompt", content: agent.inlinePrompt)
                        codeSection(label: "Command", content: agent.command)

                        HStack {
                            Spacer()
                            Button("Delete") {
                                Task {
                                    let deleteRoot = agent.projectRoot ?? anyProjectRoot
                                    await hubState.deleteAgentDef(
                                        projectRoot: deleteRoot,
                                        projectRoots: projectRoots,
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
    }

    // MARK: - Swarms

    private var swarmsContent: some View {
        masterDetail(
            list: listPanel(
                title: "Swarms",
                items: hubState.swarms,
                selectedId: hubState.selectedSwarmId,
                onSelect: { hubState.selectedSwarmId = $0 },
                onCreate: { hubState.showingCreateSwarm = true },
                emptyMessage: "No swarms yet"
            ) { swarm, isSelected in
                SwarmListRow(swarm: swarm, isSelected: isSelected)
            },
            hasSelection: hubState.selectedSwarm != nil,
            emptyMessage: "Select a swarm or create one to get started."
        ) {
            if let swarm = hubState.selectedSwarm {
                swarmDetailView(swarm)
            }
        }
    }

    private func swarmDetailView(_ swarm: SwarmDefinition) -> some View {
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
                                sectionLabel("Roster")

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
                            sectionLabel("Composition summary")

                            SwarmDiagramView(swarm: swarm)
                        }

                        HStack {
                            Spacer()
                            Button("Run") {
                                Task {
                                    let runRoot = swarm.projectRoot ?? anyProjectRoot
                                    await hubState.runSwarm(
                                        projectRoot: runRoot,
                                        name: swarm.name
                                    )
                                }
                            }
                            .buttonStyle(.borderedProminent)
                            .controlSize(.small)

                            Button("Delete") {
                                Task {
                                    let deleteRoot = swarm.projectRoot ?? anyProjectRoot
                                    await hubState.deleteSwarmDef(
                                        projectRoot: deleteRoot,
                                        projectRoots: projectRoots,
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
    }

    // MARK: - Triggers

    private var triggersContent: some View {
        masterDetail(
            list: listPanel(
                title: "Triggers",
                items: triggersState.triggers,
                selectedId: triggersState.selectedTriggerId,
                onSelect: { triggersState.selectedTriggerId = $0 },
                onCreate: { triggersState.showingCreationSheet = true },
                emptyMessage: "No triggers yet"
            ) { trigger, isSelected in
                TriggerListRow(trigger: trigger, isSelected: isSelected)
            },
            hasSelection: triggersState.selectedTrigger != nil,
            emptyMessage: "Select a trigger or create one to get started."
        ) {
            if let trigger = triggersState.selectedTrigger {
                TriggerDetailView(trigger: trigger, state: triggersState)
            }
        }
    }

    // MARK: - Reusable Layout Helpers

    private func masterDetail<List: View, Detail: View>(
        list: List,
        hasSelection: Bool,
        emptyMessage: String,
        @ViewBuilder detail: () -> Detail
    ) -> some View {
        HStack(spacing: 0) {
            list.frame(width: 300)
            Divider()
            if hasSelection {
                detail()
            } else {
                emptyDetailState(emptyMessage)
            }
        }
    }

    private func listPanel<Item: Identifiable, RowContent: View>(
        title: String,
        items: [Item],
        selectedId: Item.ID?,
        onSelect: @escaping (Item.ID) -> Void,
        onCreate: @escaping () -> Void,
        emptyMessage: String,
        @ViewBuilder row: @escaping (Item, Bool) -> RowContent
    ) -> some View {
        VStack(spacing: 0) {
            HStack {
                Text(title)
                    .font(.system(size: 13, weight: .semibold))
                Spacer()
                Button {
                    onCreate()
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)

            Divider()

            if items.isEmpty {
                emptyListState(emptyMessage, onCreate: onCreate)
            } else {
                ScrollView {
                    LazyVStack(spacing: 8) {
                        ForEach(items) { item in
                            Button {
                                onSelect(item.id)
                            } label: {
                                row(item, selectedId == item.id)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(12)
                }
            }
        }
    }

    @ViewBuilder
    private func codeSection(label: String, content: String?) -> some View {
        if let content, !content.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                sectionLabel(label)

                Text(content)
                    .font(.system(size: 12, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                    .background(Color.primary.opacity(0.035))
                    .clipShape(RoundedRectangle(cornerRadius: 10))
            }
        }
    }

    private func sectionLabel(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(.secondary)
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
        promptCommand = prompt.command ?? ""
        if !prompt.body.isEmpty {
            promptDraft = prompt.body
        }
        let capturedId = hubState.selectedPromptId
        let detailRoot = prompt.projectRoot ?? anyProjectRoot
        Task {
            await hubState.loadPromptDetail(projectRoot: detailRoot, name: prompt.name)
            guard hubState.selectedPromptId == capturedId else { return }
            if let updated = hubState.selectedPrompt {
                promptDraft = updated.body
                promptAgent = updated.agent
                promptCommand = updated.command ?? ""
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
    case triggers

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
                    text: prompt.source == "global" ? "Global" : (prompt.projectName ?? "Project"),
                    tint: prompt.source == "global" ? .blue : .green
                )
            }

            Text(prompt.description.isEmpty ? prompt.agent : prompt.description)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .selectableRow(isSelected: isSelected)
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
                MockBadge(
                    text: agent.scope == "global" ? "Global" : (agent.projectName ?? "Project"),
                    tint: agent.scope == "global" ? .blue : .green
                )
            }

            Text(agent.agentType)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
        }
        .selectableRow(isSelected: isSelected)
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
                MockBadge(
                    text: swarm.scope == "global" ? "Global" : (swarm.projectName ?? "Project"),
                    tint: swarm.scope == "global" ? .blue : .green
                )
            }

            Text("\(swarm.worktreeCount) worktrees \u{00B7} \(swarm.totalAgents) agents")
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
        }
        .selectableRow(isSelected: isSelected)
    }
}

private struct TriggerListRow: View {
    let trigger: TriggerItem
    let isSelected: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: trigger.event.icon)
                    .font(.system(size: 12))
                    .foregroundStyle(trigger.event.color)
                Text(trigger.name)
                    .font(.system(size: 13, weight: .medium))
                Spacer()
                MockBadge(
                    text: trigger.scope == "global" ? "Global" : (trigger.projectName ?? "Project"),
                    tint: trigger.scope == "global" ? .blue : .green
                )
            }

            HStack(spacing: 6) {
                Text(trigger.event.label)
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                Text("\u{00B7}")
                    .foregroundStyle(.tertiary)
                Text("\(trigger.sequence.count) step\(trigger.sequence.count == 1 ? "" : "s")")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
            }
        }
        .selectableRow(isSelected: isSelected)
    }
}

private struct SwarmDiagramView: View {
    let swarm: SwarmDefinition

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            ForEach(1...max(swarm.worktreeCount, 1), id: \.self) { index in
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
