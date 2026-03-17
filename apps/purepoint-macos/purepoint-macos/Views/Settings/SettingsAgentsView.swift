import SwiftUI

// MARK: - Enums

enum ClaudePermissionMode: String, CaseIterable {
    case `default`, acceptEdits, plan, bypassPermissions, dontAsk, auto

    var displayName: String {
        switch self {
        case .default: "Default"
        case .acceptEdits: "Accept Edits"
        case .plan: "Plan"
        case .bypassPermissions: "Bypass Permissions"
        case .dontAsk: "Don't Ask"
        case .auto: "Auto"
        }
    }

    var description: String {
        switch self {
        case .default: "Ask before each action"
        case .acceptEdits: "Auto-approve file edits, ask for other actions"
        case .plan: "Read-only mode, no file changes allowed"
        case .bypassPermissions: "Auto-approve everything (fastest, no interruptions)"
        case .dontAsk: "Auto-approve everything (alternative mode)"
        case .auto: "Let Claude decide when to ask"
        }
    }

    var cliValue: String { rawValue }
}

enum ClaudeEffort: String, CaseIterable {
    case low, medium, high

    var displayName: String { rawValue.capitalized }
    var cliValue: String { rawValue }
}

enum ClaudeModel: String, CaseIterable {
    case sonnet, opus, haiku

    var displayName: String { rawValue.capitalized }
    var cliValue: String { rawValue }
}

enum CodexApprovalPolicy: String, CaseIterable {
    case untrusted, onRequest, never

    var displayName: String {
        switch self {
        case .untrusted: "Untrusted"
        case .onRequest: "On Request"
        case .never: "Never"
        }
    }

    var cliValue: String {
        switch self {
        case .untrusted: "untrusted"
        case .onRequest: "on-request"
        case .never: "never"
        }
    }
}

enum CodexSandboxMode: String, CaseIterable {
    case readOnly, workspaceWrite, fullAccess

    var displayName: String {
        switch self {
        case .readOnly: "Read Only"
        case .workspaceWrite: "Workspace Write"
        case .fullAccess: "Full Access"
        }
    }

    var cliValue: String {
        switch self {
        case .readOnly: "read-only"
        case .workspaceWrite: "workspace-write"
        case .fullAccess: "danger-full-access"
        }
    }
}

// MARK: - Parse/Compose

struct ClaudeConfig {
    var permissionMode: ClaudePermissionMode = .bypassPermissions
    var model: ClaudeModel? = nil
    var customModel: String = ""
    var effort: ClaudeEffort? = nil
    var systemPrompt: String = ""
    var verbose: Bool = false
    var remoteControl: Bool = false
}

struct CodexConfig {
    var approvalMode: CodexApprovalPolicy = .onRequest
    var sandboxMode: CodexSandboxMode = .workspaceWrite
    var model: String = ""
    var search: Bool = false
}

struct OpenCodeConfig {
    var model: String = ""
    var variant: String = ""
    var agent: String = ""
}

func parseClaudeLaunchArgs(_ args: [String]) -> ClaudeConfig {
    var config = ClaudeConfig()
    var i = 0
    while i < args.count {
        switch args[i] {
        case "--dangerously-skip-permissions":
            config.permissionMode = .bypassPermissions
        case "--permission-mode":
            if i + 1 < args.count {
                i += 1
                config.permissionMode = ClaudePermissionMode(rawValue: args[i]) ?? .default
            }
        case "--model":
            if i + 1 < args.count {
                i += 1
                if let known = ClaudeModel(rawValue: args[i]) {
                    config.model = known
                } else {
                    config.customModel = args[i]
                }
            }
        case "--effort":
            if i + 1 < args.count {
                i += 1
                config.effort = ClaudeEffort(rawValue: args[i])
            }
        case "--append-system-prompt":
            if i + 1 < args.count {
                i += 1
                config.systemPrompt = args[i]
            }
        case "--verbose":
            config.verbose = true
        case "--remote-control", "--rc":
            config.remoteControl = true
        default:
            break
        }
        i += 1
    }
    return config
}

func composeClaudeLaunchArgs(_ config: ClaudeConfig) -> [String] {
    var args: [String] = []
    if config.permissionMode == .bypassPermissions {
        args.append("--dangerously-skip-permissions")
    } else {
        args += ["--permission-mode", config.permissionMode.cliValue]
    }
    if let model = config.model {
        args += ["--model", model.cliValue]
    } else if !config.customModel.isEmpty {
        args += ["--model", config.customModel]
    }
    if let effort = config.effort {
        args += ["--effort", effort.cliValue]
    }
    if !config.systemPrompt.isEmpty {
        args += ["--append-system-prompt", config.systemPrompt]
    }
    if config.verbose {
        args.append("--verbose")
    }
    if config.remoteControl {
        args.append("--remote-control")
    }
    return args
}

func parseCodexLaunchArgs(_ args: [String]) -> CodexConfig {
    var config = CodexConfig()
    var i = 0
    // Check for --full-auto shortcut first
    if args.contains("--full-auto") {
        config.approvalMode = .onRequest
        config.sandboxMode = .workspaceWrite
    }
    while i < args.count {
        switch args[i] {
        case "--full-auto":
            break  // handled above
        case "-a", "--ask-for-approval":
            if i + 1 < args.count {
                i += 1
                config.approvalMode = CodexApprovalPolicy.allCases.first { $0.cliValue == args[i] } ?? .onRequest
            }
        case "-s", "--sandbox":
            if i + 1 < args.count {
                i += 1
                config.sandboxMode = CodexSandboxMode.allCases.first { $0.cliValue == args[i] } ?? .workspaceWrite
            }
        case "-m", "--model":
            if i + 1 < args.count {
                i += 1
                config.model = args[i]
            }
        case "--search":
            config.search = true
        default:
            break
        }
        i += 1
    }
    return config
}

func composeCodexLaunchArgs(_ config: CodexConfig) -> [String] {
    var args: [String] = []
    // Use --full-auto shortcut when matching the defaults
    if config.approvalMode == .onRequest && config.sandboxMode == .workspaceWrite {
        args.append("--full-auto")
    } else {
        args += ["-a", config.approvalMode.cliValue, "-s", config.sandboxMode.cliValue]
    }
    if !config.model.isEmpty {
        args += ["-m", config.model]
    }
    if config.search {
        args.append("--search")
    }
    return args
}

func parseOpenCodeLaunchArgs(_ args: [String]) -> OpenCodeConfig {
    var config = OpenCodeConfig()
    var i = 0
    while i < args.count {
        switch args[i] {
        case "-m", "--model":
            if i + 1 < args.count { i += 1; config.model = args[i] }
        case "--variant":
            if i + 1 < args.count { i += 1; config.variant = args[i] }
        case "--agent":
            if i + 1 < args.count { i += 1; config.agent = args[i] }
        default:
            break
        }
        i += 1
    }
    return config
}

func composeOpenCodeLaunchArgs(_ config: OpenCodeConfig) -> [String] {
    var args: [String] = []
    if !config.model.isEmpty { args += ["-m", config.model] }
    if !config.variant.isEmpty { args += ["--variant", config.variant] }
    if !config.agent.isEmpty { args += ["--agent", config.agent] }
    return args
}

// MARK: - Main View

struct SettingsAgentsView: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        if let projectRoot = appState.activeProjectRoot {
            SettingsAgentsContentView(projectRoot: projectRoot)
        } else {
            ContentUnavailableView(
                "No Project Open",
                systemImage: "folder.badge.questionmark",
                description: Text("Open a project to configure agent launch settings.")
            )
        }
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
                if let claude = state.agentConfig(named: "claude") {
                    ClaudeAgentGroupBox(
                        projectRoot: projectRoot,
                        payload: claude,
                        state: state
                    )
                }

                if let codex = state.agentConfig(named: "codex") {
                    CodexAgentGroupBox(
                        projectRoot: projectRoot,
                        payload: codex,
                        state: state
                    )
                }

                if let opencode = state.agentConfig(named: "opencode") {
                    OpenCodeAgentGroupBox(
                        projectRoot: projectRoot,
                        payload: opencode,
                        state: state
                    )
                }
            }
        }
        .task(id: projectRoot) {
            await state.load(projectRoot: projectRoot)
        }
    }
}

// MARK: - Claude GroupBox

struct ClaudeAgentGroupBox: View {
    let projectRoot: String
    let payload: AgentConfigPayload
    let state: AgentConfigState

    @State private var useDefaults: Bool = true
    @State private var config = ClaudeConfig()

    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Toggle(
                    "Customize Launch Settings",
                    isOn: Binding(
                        get: { !useDefaults },
                        set: { useDefaults = !$0 }
                    )
                )
                .toggleStyle(.switch)
                .controlSize(.small)
                .onChange(of: useDefaults) {
                    if useDefaults {
                        Task {
                            await state.updateLaunchArgs(
                                projectRoot: projectRoot, agentName: "claude", launchArgs: nil)
                        }
                    } else {
                        commitChanges()
                    }
                }

                if useDefaults {
                    Text("Using recommended settings: auto-approve everything, no interruptions")
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                }

                VStack(alignment: .leading, spacing: 2) {
                    Toggle("Remote Control", isOn: $config.remoteControl)
                        .toggleStyle(.switch)
                        .controlSize(.small)
                        .onChange(of: config.remoteControl) {
                            if config.remoteControl && useDefaults {
                                useDefaults = false
                            }
                            commitChanges()
                        }
                    Text("Continue this session from your phone or browser via claude.ai/code")
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                }

                if useDefaults {
                    resolvedArgsView(payload.resolvedLaunchArgs)
                } else {
                    VStack(alignment: .leading, spacing: 2) {
                        LabeledContent("Permission Mode") {
                            Picker("", selection: $config.permissionMode) {
                                ForEach(ClaudePermissionMode.allCases, id: \.self) { mode in
                                    Text(mode.displayName).tag(mode)
                                }
                            }
                            .labelsHidden()
                            .frame(width: 180)
                            .onChange(of: config.permissionMode) { commitChanges() }
                        }
                        Text(config.permissionMode.description)
                            .font(.system(size: 11))
                            .foregroundStyle(.tertiary)
                    }

                    VStack(alignment: .leading, spacing: 2) {
                        LabeledContent("Model") {
                            HStack(spacing: 8) {
                                Picker(
                                    "",
                                    selection: Binding(
                                        get: { config.model ?? .sonnet },
                                        set: {
                                            config.model = $0; config.customModel = ""
                                        }
                                    )
                                ) {
                                    ForEach(ClaudeModel.allCases, id: \.self) { m in
                                        Text(m.displayName).tag(m)
                                    }
                                }
                                .labelsHidden()
                                .frame(width: 100)
                                .onChange(of: config.model) { commitChanges() }

                                TextField("Custom", text: $config.customModel)
                                    .textFieldStyle(.roundedBorder)
                                    .frame(width: 120)
                                    .onSubmit {
                                        if !config.customModel.isEmpty { config.model = nil }
                                        commitChanges()
                                    }
                            }
                        }
                        Text("Which AI model to use")
                            .font(.system(size: 11))
                            .foregroundStyle(.tertiary)
                    }

                    VStack(alignment: .leading, spacing: 2) {
                        LabeledContent("Effort") {
                            Picker(
                                "",
                                selection: Binding(
                                    get: { config.effort ?? .high },
                                    set: { config.effort = $0 }
                                )
                            ) {
                                ForEach(ClaudeEffort.allCases, id: \.self) { e in
                                    Text(e.displayName).tag(e)
                                }
                            }
                            .pickerStyle(.segmented)
                            .frame(width: 200)
                            .onChange(of: config.effort) { commitChanges() }
                        }
                        Text("How much compute the model uses per response")
                            .font(.system(size: 11))
                            .foregroundStyle(.tertiary)
                    }

                    VStack(alignment: .leading, spacing: 2) {
                        LabeledContent("System Prompt") {
                            TextField("Optional", text: $config.systemPrompt)
                                .textFieldStyle(.roundedBorder)
                                .onSubmit { commitChanges() }
                        }
                        Text("Additional instructions appended to the agent's system prompt")
                            .font(.system(size: 11))
                            .foregroundStyle(.tertiary)
                    }

                    VStack(alignment: .leading, spacing: 2) {
                        Toggle("Verbose", isOn: $config.verbose)
                            .toggleStyle(.switch)
                            .controlSize(.small)
                            .onChange(of: config.verbose) { commitChanges() }
                        Text("Show detailed logging output in the terminal")
                            .font(.system(size: 11))
                            .foregroundStyle(.tertiary)
                    }

                    resolvedArgsView(composeClaudeLaunchArgs(config))
                }
            }
        } label: {
            Label("Claude", systemImage: "brain")
        }
        .groupBoxStyle(SettingsGroupBoxStyle())
        .onAppear { syncFromPayload() }
        .onChange(of: payload.launchArgs) { syncFromPayload() }
    }

    private func syncFromPayload() {
        useDefaults = payload.launchArgs == nil
        if let args = payload.launchArgs {
            config = parseClaudeLaunchArgs(args)
        } else {
            config = parseClaudeLaunchArgs(payload.resolvedLaunchArgs)
        }
    }

    private func commitChanges() {
        let args = composeClaudeLaunchArgs(config)
        Task { await state.updateLaunchArgs(projectRoot: projectRoot, agentName: "claude", launchArgs: args) }
    }
}

// MARK: - Codex GroupBox

struct CodexAgentGroupBox: View {
    let projectRoot: String
    let payload: AgentConfigPayload
    let state: AgentConfigState

    @State private var useDefaults: Bool = true
    @State private var config = CodexConfig()

    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Toggle(
                    "Customize Launch Settings",
                    isOn: Binding(
                        get: { !useDefaults },
                        set: { useDefaults = !$0 }
                    )
                )
                .toggleStyle(.switch)
                .controlSize(.small)
                .onChange(of: useDefaults) {
                    if useDefaults {
                        Task {
                            await state.updateLaunchArgs(
                                projectRoot: projectRoot, agentName: "codex", launchArgs: nil)
                        }
                    } else {
                        commitChanges()
                    }
                }

                if useDefaults {
                    Text("Using recommended settings: full-auto mode with workspace sandbox")
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                    resolvedArgsView(payload.resolvedLaunchArgs)
                } else {
                    LabeledContent("Approval Mode") {
                        Picker("", selection: $config.approvalMode) {
                            ForEach(CodexApprovalPolicy.allCases, id: \.self) { mode in
                                Text(mode.displayName).tag(mode)
                            }
                        }
                        .labelsHidden()
                        .frame(width: 180)
                        .onChange(of: config.approvalMode) { commitChanges() }
                    }

                    LabeledContent("Sandbox Mode") {
                        Picker("", selection: $config.sandboxMode) {
                            ForEach(CodexSandboxMode.allCases, id: \.self) { mode in
                                Text(mode.displayName).tag(mode)
                            }
                        }
                        .labelsHidden()
                        .frame(width: 180)
                        .onChange(of: config.sandboxMode) { commitChanges() }
                    }

                    LabeledContent("Model") {
                        TextField("Model name", text: $config.model)
                            .textFieldStyle(.roundedBorder)
                            .onSubmit { commitChanges() }
                    }

                    Toggle("Web Search", isOn: $config.search)
                        .toggleStyle(.switch)
                        .controlSize(.small)
                        .onChange(of: config.search) { commitChanges() }

                    resolvedArgsView(composeCodexLaunchArgs(config))
                }
            }
        } label: {
            Label("Codex", systemImage: "terminal")
        }
        .groupBoxStyle(SettingsGroupBoxStyle())
        .onAppear { syncFromPayload() }
        .onChange(of: payload.launchArgs) { syncFromPayload() }
    }

    private func syncFromPayload() {
        useDefaults = payload.launchArgs == nil
        if let args = payload.launchArgs {
            config = parseCodexLaunchArgs(args)
        } else {
            config = parseCodexLaunchArgs(payload.resolvedLaunchArgs)
        }
    }

    private func commitChanges() {
        let args = composeCodexLaunchArgs(config)
        Task { await state.updateLaunchArgs(projectRoot: projectRoot, agentName: "codex", launchArgs: args) }
    }
}

// MARK: - OpenCode GroupBox

struct OpenCodeAgentGroupBox: View {
    let projectRoot: String
    let payload: AgentConfigPayload
    let state: AgentConfigState

    @State private var useDefaults: Bool = true
    @State private var config = OpenCodeConfig()

    var body: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 10) {
                Toggle(
                    "Customize Launch Settings",
                    isOn: Binding(
                        get: { !useDefaults },
                        set: { useDefaults = !$0 }
                    )
                )
                .toggleStyle(.switch)
                .controlSize(.small)
                .onChange(of: useDefaults) {
                    if useDefaults {
                        Task {
                            await state.updateLaunchArgs(
                                projectRoot: projectRoot, agentName: "opencode", launchArgs: nil)
                        }
                    } else {
                        commitChanges()
                    }
                }

                if useDefaults {
                    Text("Using recommended settings: default configuration")
                        .font(.system(size: 11))
                        .foregroundStyle(.tertiary)
                    resolvedArgsView(payload.resolvedLaunchArgs)
                } else {
                    LabeledContent("Model") {
                        TextField("provider/model", text: $config.model)
                            .textFieldStyle(.roundedBorder)
                            .onSubmit { commitChanges() }
                    }

                    LabeledContent("Variant") {
                        TextField("e.g. high, max", text: $config.variant)
                            .textFieldStyle(.roundedBorder)
                            .onSubmit { commitChanges() }
                    }

                    LabeledContent("Agent") {
                        TextField("Agent name", text: $config.agent)
                            .textFieldStyle(.roundedBorder)
                            .onSubmit { commitChanges() }
                    }

                    resolvedArgsView(composeOpenCodeLaunchArgs(config))
                }
            }
        } label: {
            Label("OpenCode", systemImage: "chevron.left.forwardslash.chevron.right")
        }
        .groupBoxStyle(SettingsGroupBoxStyle())
        .onAppear { syncFromPayload() }
        .onChange(of: payload.launchArgs) { syncFromPayload() }
    }

    private func syncFromPayload() {
        useDefaults = payload.launchArgs == nil
        if let args = payload.launchArgs {
            config = parseOpenCodeLaunchArgs(args)
        } else {
            config = parseOpenCodeLaunchArgs(payload.resolvedLaunchArgs)
        }
    }

    private func commitChanges() {
        let args = composeOpenCodeLaunchArgs(config)
        Task { await state.updateLaunchArgs(projectRoot: projectRoot, agentName: "opencode", launchArgs: args) }
    }
}

// MARK: - Shared

private func resolvedArgsView(_ args: [String]) -> some View {
    DisclosureGroup("Resolved launch args") {
        Text(args.isEmpty ? "(none)" : args.joined(separator: " "))
            .font(.system(size: 11, design: .monospaced))
            .foregroundStyle(.secondary)
            .textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
    }
    .font(.system(size: 11))
    .foregroundStyle(.tertiary)
}
