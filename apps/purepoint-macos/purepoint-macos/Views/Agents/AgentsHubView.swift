import SwiftUI

struct AgentsHubView: View {
    @State private var activeTab: AgentsHubTab = .agents
    @State private var selectedPromptID = AgentPromptRecord.samples[0].id
    @State private var selectedAgentID = AgentLibraryItem.samples[0].id
    @State private var selectedSwarmID = SwarmBlueprint.samples[0].id
    @State private var promptScope: PromptScopeChoice = AgentPromptRecord.samples[0].scope
    @State private var promptDraft = AgentPromptRecord.samples[0].body
    @State private var newAgentName = "Migration shepherd"
    @State private var agentPromptMode: AgentPromptSourceMode = .library
    @State private var selectedPromptTemplateID = AgentPromptRecord.samples[1].id
    @State private var inlineAgentPrompt = """
    # Migration shepherd

    Coordinate the rollout plan, collect risks, and keep the status summary tight.

    ## Output
    - milestones
    - blockers
    - owner map
    """
    @State private var agentTags = "migration, release"
    @State private var agentScope: PromptScopeChoice = .project
    @State private var availableInCommandDialog = true
    @State private var swarmName = "Security review"
    @State private var worktreeCount = 4
    @State private var worktreeTemplate = "review/{index}"
    @State private var reviewerCount = 1
    @State private var fixerCount = 1
    @State private var reporterCount = 1
    @State private var includeTerminal = true

    private var selectedPrompt: AgentPromptRecord {
        AgentPromptRecord.samples.first(where: { $0.id == selectedPromptID }) ?? AgentPromptRecord.samples[0]
    }

    private var selectedAgent: AgentLibraryItem {
        AgentLibraryItem.samples.first(where: { $0.id == selectedAgentID }) ?? AgentLibraryItem.samples[0]
    }

    private var selectedSwarm: SwarmBlueprint {
        SwarmBlueprint.samples.first(where: { $0.id == selectedSwarmID }) ?? SwarmBlueprint.samples[0]
    }

    private var draftSwarmAgentCount: Int {
        worktreeCount * (reviewerCount + fixerCount + reporterCount)
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
        .onAppear {
            syncPromptEditor()
            availableInCommandDialog = selectedAgent.availableInCommandDialog
        }
        .onChange(of: selectedPromptID) { _, _ in
            syncPromptEditor()
        }
        .onChange(of: selectedAgentID) { _, _ in
            availableInCommandDialog = selectedAgent.availableInCommandDialog
        }
    }

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

    @ViewBuilder
    private var content: some View {
        switch activeTab {
        case .prompts:
            promptsContent
        case .agents:
            agentsContent
        case .swarms:
            swarmsContent
        }
    }

    private var promptsContent: some View {
        HStack(spacing: 0) {
            promptListPanel
                .frame(width: 300)

            Divider()

            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    CommandHintBar(
                        icon: "text.alignleft",
                        text: "Prompts can be stored globally or inside a project, then assigned to agents and swarms."
                    )

                    MockSurfaceCard(
                        title: "Prompt editor",
                        subtitle: "Mock rich Markdown editing for reusable templates.",
                        actionTitle: "Preview"
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

                                HStack(spacing: 8) {
                                    Button("Create") {}
                                    Button("Edit") {}
                                    Button("Duplicate") {}
                                    Button("Delete") {}
                                        .foregroundStyle(.red)
                                }
                                .buttonStyle(.bordered)
                                .controlSize(.small)
                            }

                            PromptEditorToolbar()

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
                                        ForEach(selectedPrompt.variables, id: \.self) { variable in
                                            MockBadge(text: variable, tint: .orange)
                                        }
                                    }
                                }

                                Spacer()

                                VStack(alignment: .trailing, spacing: 6) {
                                    Text("Last edited \(selectedPrompt.lastEdited)")
                                        .font(.system(size: 12))
                                        .foregroundStyle(.secondary)
                                    Text(selectedPrompt.storageHint)
                                        .font(.system(size: 11))
                                        .foregroundStyle(.tertiary)
                                }
                            }
                        }
                    }

                    MockSurfaceCard(
                        title: "Prompt usage",
                        subtitle: "Selected prompt is already attached to reusable workflows."
                    ) {
                        VStack(alignment: .leading, spacing: 10) {
                            usageRow(
                                title: "Attached agents",
                                value: selectedPrompt.attachedAgents,
                                icon: "cpu"
                            )
                            usageRow(
                                title: "Attached swarms",
                                value: selectedPrompt.attachedSwarms,
                                icon: "square.3.layers.3d.top.filled"
                            )
                            usageRow(
                                title: "Preferred scope",
                                value: promptScope.title,
                                icon: "scope"
                            )
                        }
                    }
                }
                .padding(20)
            }
        }
    }

    private var agentsContent: some View {
        HStack(spacing: 0) {
            agentListPanel
                .frame(width: 300)

            Divider()

            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    CommandHintBar(
                        icon: "command",
                        text: "cmd+n -> @agentname -> spawn agent session"
                    )

                    MockSurfaceCard(
                        title: "Create agent",
                        subtitle: "Assign a saved prompt or keep the prompt inline for one-off behavior."
                    ) {
                        VStack(alignment: .leading, spacing: 12) {
                            TextField("Agent name", text: $newAgentName)
                                .textFieldStyle(.roundedBorder)

                            Picker("Prompt source", selection: $agentPromptMode) {
                                ForEach(AgentPromptSourceMode.allCases) { mode in
                                    Text(mode.title)
                                        .tag(mode)
                                }
                            }
                            .pickerStyle(.segmented)

                            if agentPromptMode == .library {
                                Picker("Prompt", selection: $selectedPromptTemplateID) {
                                    ForEach(AgentPromptRecord.samples) { prompt in
                                        Text("\(prompt.name) (\(prompt.scope.title))")
                                            .tag(prompt.id)
                                    }
                                }
                                .pickerStyle(.menu)
                            } else {
                                TextEditor(text: $inlineAgentPrompt)
                                    .font(.system(size: 13, design: .monospaced))
                                    .frame(minHeight: 160)
                                    .padding(8)
                                    .background(Color.primary.opacity(0.035))
                                    .clipShape(RoundedRectangle(cornerRadius: 10))
                            }

                            HStack(spacing: 12) {
                                TextField("Tags", text: $agentTags)
                                    .textFieldStyle(.roundedBorder)

                                Picker("Scope", selection: $agentScope) {
                                    ForEach(PromptScopeChoice.allCases) { scope in
                                        Text(scope.title)
                                            .tag(scope)
                                    }
                                }
                                .pickerStyle(.segmented)
                                .frame(maxWidth: 220)
                            }

                            HStack {
                                MockBadge(text: "Appears as @\(newAgentName.replacingOccurrences(of: " ", with: "-").lowercased())", tint: .blue)
                                Spacer()
                                Button("Create Agent") {}
                                    .buttonStyle(.borderedProminent)
                                Button("Save Draft") {}
                                    .buttonStyle(.bordered)
                            }
                        }
                    }

                    MockSurfaceCard(
                        title: selectedAgent.name,
                        subtitle: "\(selectedAgent.typeName) agent. \(selectedAgent.promptSource). Last used \(selectedAgent.lastUsed).",
                        actionTitle: "Edit prompt"
                    ) {
                        VStack(alignment: .leading, spacing: 14) {
                            HStack(spacing: 8) {
                                Image(systemName: selectedAgent.icon)
                                    .foregroundStyle(.secondary)
                                ForEach(selectedAgent.tags, id: \.self) { tag in
                                    MockBadge(text: tag, tint: .purple)
                                }
                                MockBadge(text: selectedAgent.scope.title, tint: .green)
                                Spacer()
                                Toggle("Available in command dialog", isOn: $availableInCommandDialog)
                                    .toggleStyle(.switch)
                                    .controlSize(.small)
                            }

                            VStack(alignment: .leading, spacing: 8) {
                                Text("Prompt preview")
                                    .font(.system(size: 12, weight: .medium))
                                    .foregroundStyle(.secondary)

                                Text(selectedAgent.promptPreview)
                                    .font(.system(size: 12, design: .monospaced))
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(12)
                                    .background(Color.primary.opacity(0.035))
                                    .clipShape(RoundedRectangle(cornerRadius: 10))
                            }

                            VStack(alignment: .leading, spacing: 8) {
                                Text("Recent sessions")
                                    .font(.system(size: 12, weight: .medium))
                                    .foregroundStyle(.secondary)

                                ForEach(selectedAgent.sessions) { session in
                                    HStack(alignment: .top, spacing: 10) {
                                        Circle()
                                            .fill(session.tint)
                                            .frame(width: 8, height: 8)
                                            .padding(.top, 4)

                                        VStack(alignment: .leading, spacing: 4) {
                                            Text(session.title)
                                                .font(.system(size: 13, weight: .medium))
                                            Text("\(session.branch) · \(session.outcome)")
                                                .font(.system(size: 12))
                                                .foregroundStyle(.secondary)
                                        }

                                        Spacer()

                                        Text(session.startedAt)
                                            .font(.system(size: 12))
                                            .foregroundStyle(.secondary)
                                    }
                                    .padding(.vertical, 2)
                                }
                            }
                        }
                    }
                }
                .padding(20)
            }
        }
    }

    private var swarmsContent: some View {
        HStack(spacing: 0) {
            swarmListPanel
                .frame(width: 300)

            Divider()

            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    CommandHintBar(
                        icon: "command",
                        text: "cmd+n -> @swarmname -> execute composition"
                    )

                    MockSurfaceCard(
                        title: "Create swarm",
                        subtitle: "Reusable multi-worktree composition built from existing agents."
                    ) {
                        VStack(alignment: .leading, spacing: 12) {
                            TextField("Swarm name", text: $swarmName)
                                .textFieldStyle(.roundedBorder)

                            HStack(spacing: 12) {
                                Stepper("Worktrees: \(worktreeCount)", value: $worktreeCount, in: 1 ... 8)
                                TextField("Template", text: $worktreeTemplate)
                                    .textFieldStyle(.roundedBorder)
                            }

                            VStack(alignment: .leading, spacing: 10) {
                                Text("Agent roster per worktree")
                                    .font(.system(size: 12, weight: .medium))
                                    .foregroundStyle(.secondary)

                                SwarmRosterDraftRow(
                                    name: "Review auditor",
                                    role: "Lead reviewer",
                                    count: $reviewerCount,
                                    tint: .red
                                )
                                SwarmRosterDraftRow(
                                    name: "Patch runner",
                                    role: "Fix candidate owner",
                                    count: $fixerCount,
                                    tint: .blue
                                )
                                SwarmRosterDraftRow(
                                    name: "Report writer",
                                    role: "Roll-up summary",
                                    count: $reporterCount,
                                    tint: .green
                                )
                            }

                            Toggle("Include Terminal", isOn: $includeTerminal)
                                .toggleStyle(.switch)

                            HStack {
                                MockBadge(text: "\(draftSwarmAgentCount) total agents", tint: .orange)
                                Spacer()
                                Button("Create Swarm") {}
                                    .buttonStyle(.borderedProminent)
                                Button("Save as Template") {}
                                    .buttonStyle(.bordered)
                            }
                        }
                    }

                    MockSurfaceCard(
                        title: selectedSwarm.name,
                        subtitle: "\(selectedSwarm.worktrees) worktrees · \(selectedSwarm.totalAgents) agents total · last run \(selectedSwarm.lastRun)",
                        actionTitle: "Run"
                    ) {
                        VStack(alignment: .leading, spacing: 14) {
                            HStack(spacing: 10) {
                                MockBadge(text: selectedSwarm.worktreeTemplate, tint: .blue)
                                MockBadge(text: selectedSwarm.includeTerminal ? "Terminal attached" : "No terminal", tint: selectedSwarm.includeTerminal ? .green : .gray)
                                Spacer()
                            }

                            VStack(alignment: .leading, spacing: 8) {
                                Text("Composition summary")
                                    .font(.system(size: 12, weight: .medium))
                                    .foregroundStyle(.secondary)

                                SwarmDiagramView(swarm: selectedSwarm)
                            }

                            VStack(alignment: .leading, spacing: 8) {
                                Text("Recent runs")
                                    .font(.system(size: 12, weight: .medium))
                                    .foregroundStyle(.secondary)

                                ForEach(selectedSwarm.recentRuns) { run in
                                    HStack(alignment: .top, spacing: 10) {
                                        Circle()
                                            .fill(run.tint)
                                            .frame(width: 8, height: 8)
                                            .padding(.top, 4)

                                        VStack(alignment: .leading, spacing: 4) {
                                            Text(run.outcome)
                                                .font(.system(size: 13, weight: .medium))
                                            Text(run.note)
                                                .font(.system(size: 12))
                                                .foregroundStyle(.secondary)
                                        }

                                        Spacer()

                                        Text(run.startedAt)
                                            .font(.system(size: 12))
                                            .foregroundStyle(.secondary)
                                    }
                                    .padding(.vertical, 2)
                                }
                            }
                        }
                    }
                }
                .padding(20)
            }
        }
    }

    private var promptListPanel: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Prompt library")
                    .font(.system(size: 13, weight: .semibold))
                Spacer()
                Button {
                    selectedPromptID = AgentPromptRecord.samples[0].id
                    syncPromptEditor()
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)

            Divider()

            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(AgentPromptRecord.samples) { prompt in
                        Button {
                            selectedPromptID = prompt.id
                        } label: {
                            PromptListRow(prompt: prompt, isSelected: selectedPromptID == prompt.id)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(12)
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
                    activeTab = .agents
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)

            Divider()

            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(AgentLibraryItem.samples) { agent in
                        Button {
                            selectedAgentID = agent.id
                        } label: {
                            AgentListRow(agent: agent, isSelected: selectedAgentID == agent.id)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(12)
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
                    activeTab = .swarms
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(.borderless)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)

            Divider()

            ScrollView {
                LazyVStack(spacing: 8) {
                    ForEach(SwarmBlueprint.samples) { swarm in
                        Button {
                            selectedSwarmID = swarm.id
                        } label: {
                            SwarmListRow(swarm: swarm, isSelected: selectedSwarmID == swarm.id)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(12)
            }
        }
    }

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
        promptScope = selectedPrompt.scope
        promptDraft = selectedPrompt.body
    }
}

private enum AgentsHubTab: String, CaseIterable, Identifiable {
    case prompts
    case agents
    case swarms

    var id: String { rawValue }

    var title: String {
        rawValue.capitalized
    }
}

private struct AgentPromptRecord: Identifiable {
    let id: String
    let name: String
    let scope: PromptScopeChoice
    let lastEdited: String
    let body: String
    let variables: [String]
    let attachedAgents: String
    let attachedSwarms: String
    let storageHint: String

    static let samples: [AgentPromptRecord] = [
        AgentPromptRecord(
            id: "security-review",
            name: "Security review",
            scope: .global,
            lastEdited: "18m ago",
            body: """
            # Security review

            Inspect {{REPO}} for auth, secrets handling, and permission boundaries.

            ## Deliverable
            - findings by severity
            - concrete reproduction
            - patch ideas
            """,
            variables: ["{{REPO}}", "{{BRANCH}}"],
            attachedAgents: "3 agents",
            attachedSwarms: "1 swarm",
            storageHint: "Stored in ~/.purepoint/prompts/security-review.md"
        ),
        AgentPromptRecord(
            id: "release-notes",
            name: "Release notes draft",
            scope: .project,
            lastEdited: "2h ago",
            body: """
            # Release notes draft

            Summarize the diff since {{LAST_TAG}} and group changes by customer impact.

            ## Include
            - notable fixes
            - migration notes
            - follow-up docs
            """,
            variables: ["{{LAST_TAG}}", "{{PR_RANGE}}"],
            attachedAgents: "2 agents",
            attachedSwarms: "0 swarms",
            storageHint: "Stored in .pu/prompts/release-notes.md"
        ),
        AgentPromptRecord(
            id: "migration-plan",
            name: "Migration plan",
            scope: .project,
            lastEdited: "Yesterday",
            body: """
            # Migration plan

            Build a phased rollout plan for {{SYSTEM_NAME}} with owners and rollback points.

            ## Sections
            - prerequisites
            - milestones
            - verification
            """,
            variables: ["{{SYSTEM_NAME}}", "{{OWNER}}"],
            attachedAgents: "1 agent",
            attachedSwarms: "2 swarms",
            storageHint: "Stored in .pu/prompts/migration-plan.md"
        ),
    ]
}

private struct AgentLibraryItem: Identifiable {
    let id: String
    let name: String
    let icon: String
    let typeName: String
    let promptSource: String
    let lastUsed: String
    let scope: PromptScopeChoice
    let availableInCommandDialog: Bool
    let tags: [String]
    let promptPreview: String
    let sessions: [AgentSessionRecord]

    static let samples: [AgentLibraryItem] = [
        AgentLibraryItem(
            id: "review-auditor",
            name: "Review auditor",
            icon: "shield.lefthalf.filled",
            typeName: "Security",
            promptSource: "Security review (global)",
            lastUsed: "12m ago",
            scope: .global,
            availableInCommandDialog: true,
            tags: ["security", "review"],
            promptPreview: """
            # Security review

            Inspect the target diff for auth, input validation, and secret exposure.
            Return only actionable findings.
            """,
            sessions: [
                AgentSessionRecord(id: "ra-1", title: "Credential leak audit", branch: "codex/auth-hardening", outcome: "2 findings shipped", startedAt: "12m ago", tint: .green),
                AgentSessionRecord(id: "ra-2", title: "Webhook review", branch: "feature/webhook-retry", outcome: "1 medium severity issue", startedAt: "Yesterday", tint: .orange),
            ]
        ),
        AgentLibraryItem(
            id: "docs-synth",
            name: "Docs synth",
            icon: "doc.text.magnifyingglass",
            typeName: "Documentation",
            promptSource: "Release notes draft (project)",
            lastUsed: "3h ago",
            scope: .project,
            availableInCommandDialog: true,
            tags: ["docs", "release"],
            promptPreview: """
            # Release notes draft

            Summarize customer-facing changes and keep the output publication-ready.
            """,
            sessions: [
                AgentSessionRecord(id: "ds-1", title: "0.8.2 notes", branch: "release/0.8.2", outcome: "draft posted to docs", startedAt: "3h ago", tint: .green),
                AgentSessionRecord(id: "ds-2", title: "Migration guide stub", branch: "docs/migration-v2", outcome: "awaiting review", startedAt: "2d ago", tint: .blue),
            ]
        ),
        AgentLibraryItem(
            id: "refactor-scout",
            name: "Refactor scout",
            icon: "wand.and.stars",
            typeName: "Engineering",
            promptSource: "Inline prompt",
            lastUsed: "2d ago",
            scope: .project,
            availableInCommandDialog: false,
            tags: ["cleanup", "swift"],
            promptPreview: """
            # Refactor scout

            Look for repeated patterns, brittle coupling, and missing seams for future changes.
            """,
            sessions: [
                AgentSessionRecord(id: "rs-1", title: "Sidebar cleanup", branch: "codex/sidebar-nav", outcome: "3 simplifications proposed", startedAt: "2d ago", tint: .blue),
            ]
        ),
    ]
}

private struct AgentSessionRecord: Identifiable {
    let id: String
    let title: String
    let branch: String
    let outcome: String
    let startedAt: String
    let tint: Color
}

private struct SwarmBlueprint: Identifiable {
    let id: String
    let name: String
    let worktrees: Int
    let worktreeTemplate: String
    let includeTerminal: Bool
    let lastRun: String
    let roster: [SwarmRole]
    let recentRuns: [SwarmRun]

    var totalAgents: Int {
        worktrees * roster.reduce(0) { $0 + $1.quantity }
    }

    static let samples: [SwarmBlueprint] = [
        SwarmBlueprint(
            id: "security-review",
            name: "Security review",
            worktrees: 4,
            worktreeTemplate: "review/{index}",
            includeTerminal: true,
            lastRun: "45m ago",
            roster: [
                SwarmRole(id: "sec-lead", name: "Review auditor", icon: "shield.lefthalf.filled", role: "Lead reviewer", quantity: 1),
                SwarmRole(id: "sec-fix", name: "Patch runner", icon: "wrench.and.screwdriver", role: "Fix candidate owner", quantity: 1),
                SwarmRole(id: "sec-report", name: "Report writer", icon: "doc.plaintext", role: "Roll-up summary", quantity: 1),
            ],
            recentRuns: [
                SwarmRun(id: "sr-1", startedAt: "45m ago", outcome: "Completed", note: "12 agents across auth, billing, and webhook surfaces.", tint: .green),
                SwarmRun(id: "sr-2", startedAt: "Yesterday", outcome: "Needs follow-up", note: "2 worktrees hit flaky CI before aggregation finished.", tint: .orange),
            ]
        ),
        SwarmBlueprint(
            id: "release-train",
            name: "Release train",
            worktrees: 2,
            worktreeTemplate: "release/{index}",
            includeTerminal: false,
            lastRun: "Yesterday",
            roster: [
                SwarmRole(id: "rt-docs", name: "Docs synth", icon: "doc.text.magnifyingglass", role: "Customer notes", quantity: 1),
                SwarmRole(id: "rt-verify", name: "Refactor scout", icon: "wand.and.stars", role: "Regression scan", quantity: 1),
            ],
            recentRuns: [
                SwarmRun(id: "rt-1", startedAt: "Yesterday", outcome: "Completed", note: "Release notes and regression notes attached to milestone.", tint: .green),
            ]
        ),
        SwarmBlueprint(
            id: "prompt-cleanup",
            name: "Prompt cleanup",
            worktrees: 1,
            worktreeTemplate: "prompts/main",
            includeTerminal: false,
            lastRun: "3d ago",
            roster: [
                SwarmRole(id: "pc-1", name: "Docs synth", icon: "doc.text.magnifyingglass", role: "Template rewrite", quantity: 1),
                SwarmRole(id: "pc-2", name: "Refactor scout", icon: "wand.and.stars", role: "Variable audit", quantity: 1),
            ],
            recentRuns: [
                SwarmRun(id: "pc-run", startedAt: "3d ago", outcome: "Completed", note: "Normalized 7 project prompts and removed stale variables.", tint: .green),
            ]
        ),
    ]
}

private struct SwarmRole: Identifiable {
    let id: String
    let name: String
    let icon: String
    let role: String
    let quantity: Int
}

private struct SwarmRun: Identifiable {
    let id: String
    let startedAt: String
    let outcome: String
    let note: String
    let tint: Color
}

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

private struct PromptEditorToolbar: View {
    private let items = ["H1", "Bold", "Link", "List", "Code", "Quote"]

    var body: some View {
        HStack(spacing: 8) {
            ForEach(items, id: \.self) { item in
                Button(item) {}
                    .buttonStyle(.bordered)
                    .controlSize(.small)
            }
            Spacer()
            MockBadge(text: "Markdown", tint: .gray)
        }
    }
}

private struct PromptListRow: View {
    let prompt: AgentPromptRecord
    let isSelected: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(prompt.name)
                    .font(.system(size: 13, weight: .medium))
                Spacer()
                MockBadge(text: prompt.scope.title, tint: prompt.scope == .global ? .blue : .green)
            }

            Text(prompt.lastEdited)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(isSelected ? Color.accentColor.opacity(0.12) : Color.primary.opacity(0.03))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

private struct AgentListRow: View {
    let agent: AgentLibraryItem
    let isSelected: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: agent.icon)
                    .foregroundStyle(.secondary)
                Text(agent.name)
                    .font(.system(size: 13, weight: .medium))
                Spacer()
                Text(agent.lastUsed)
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
            }

            Text(agent.promptSource)
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
    let swarm: SwarmBlueprint
    let isSelected: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(swarm.name)
                    .font(.system(size: 13, weight: .medium))
                Spacer()
                Text(swarm.lastRun)
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
            }

            Text("\(swarm.worktrees) worktrees · \(swarm.totalAgents) agents")
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
        }
        .padding(12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(isSelected ? Color.accentColor.opacity(0.12) : Color.primary.opacity(0.03))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

private struct SwarmRosterDraftRow: View {
    let name: String
    let role: String
    @Binding var count: Int
    let tint: Color

    var body: some View {
        HStack(spacing: 12) {
            MockBadge(text: name, tint: tint)
            Text(role)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
            Spacer()
            Stepper("x\(count)", value: $count, in: 0 ... 4)
                .labelsHidden()
            Text("x\(count)")
                .font(.system(size: 12, weight: .medium))
                .frame(width: 28, alignment: .trailing)
        }
        .padding(10)
        .background(Color.primary.opacity(0.035))
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }
}

private struct SwarmDiagramView: View {
    let swarm: SwarmBlueprint

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            ForEach(1 ... swarm.worktrees, id: \.self) { index in
                VStack(alignment: .leading, spacing: 8) {
                    Text("Worktree \(index)")
                        .font(.system(size: 12, weight: .semibold))
                    Text(swarm.worktreeTemplate.replacingOccurrences(of: "{index}", with: "\(index)"))
                        .font(.system(size: 11, design: .monospaced))
                        .foregroundStyle(.secondary)

                    ForEach(swarm.roster) { role in
                        HStack(spacing: 6) {
                            Image(systemName: role.icon)
                                .font(.system(size: 11))
                                .foregroundStyle(.secondary)
                            Text("\(role.name) x\(role.quantity)")
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
