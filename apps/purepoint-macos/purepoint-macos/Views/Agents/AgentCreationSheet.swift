import SwiftUI

struct AgentCreationSheet: View {
    @Bindable var hubState: AgentsHubState
    let projectRoot: String
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var agentType = "claude"
    @State private var promptMode: AgentPromptSourceMode = .library
    @State private var templateName = ""
    @State private var inlinePrompt = ""
    @State private var tags = ""
    @State private var scope: PromptScopeChoice = .project
    @State private var availableInCommandDialog = true
    @State private var command = ""

    private let agentTypes = AgentTypes.all

    var body: some View {
        VStack(spacing: 0) {
            sheetHeader
            Divider()
            formContent
            Divider()
            sheetFooter
        }
        .frame(width: 420, height: 520)
    }

    // MARK: - Header

    private var sheetHeader: some View {
        HStack {
            Text("New Agent")
                .font(.system(size: 14, weight: .semibold))
            Spacer()
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
    }

    // MARK: - Form

    private var formContent: some View {
        Form {
            TextField("Name", text: $name)
                .textFieldStyle(.roundedBorder)

            Picker("Agent type", selection: $agentType) {
                ForEach(agentTypes, id: \.self) { t in
                    Text(t).tag(t)
                }
            }

            if agentType == "terminal" {
                TextField("Command (e.g. npm run dev)", text: $command)
                    .textFieldStyle(.roundedBorder)
            }

            if agentType != "terminal" {
                Picker("Prompt source", selection: $promptMode) {
                    ForEach(AgentPromptSourceMode.allCases) { mode in
                        Text(mode.title).tag(mode)
                    }
                }
                .pickerStyle(.segmented)

                if promptMode == .library {
                    TextField("Template name", text: $templateName)
                        .textFieldStyle(.roundedBorder)
                } else {
                    TextEditor(text: $inlinePrompt)
                        .font(.system(size: 13, design: .monospaced))
                        .frame(minHeight: 100)
                        .padding(4)
                        .background(Color.primary.opacity(0.035))
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                }
            }

            TextField("Tags (comma-separated)", text: $tags)
                .textFieldStyle(.roundedBorder)

            Picker("Scope", selection: $scope) {
                ForEach(PromptScopeChoice.allCases) { s in
                    Text(s.title).tag(s)
                }
            }
            .pickerStyle(.segmented)

            Toggle("Available in command dialog", isOn: $availableInCommandDialog)
                .toggleStyle(.switch)
        }
        .formStyle(.grouped)
        .scrollDisabled(true)
    }

    // MARK: - Footer

    private var sheetFooter: some View {
        HStack {
            Button("Cancel") { dismiss() }
                .keyboardShortcut(.cancelAction)
            Spacer()
            Button("Create") {
                let parsedTags =
                    tags
                    .split(separator: ",")
                    .map { $0.trimmingCharacters(in: .whitespaces) }
                    .filter { !$0.isEmpty }

                let trimmedCommand = command.trimmingCharacters(in: .whitespaces)
                let def = AgentDefinition(
                    name: name.trimmingCharacters(in: .whitespaces),
                    agentType: agentType,
                    template: agentType != "terminal" && promptMode == .library ? templateName : nil,
                    inlinePrompt: agentType != "terminal" && promptMode == .inline ? inlinePrompt : nil,
                    tags: parsedTags,
                    scope: scope.wireValue,
                    availableInCommandDialog: availableInCommandDialog,
                    command: agentType == "terminal" && !trimmedCommand.isEmpty ? trimmedCommand : nil
                )
                Task {
                    await hubState.saveAgentDef(projectRoot: projectRoot, def: def)
                    dismiss()
                }
            }
            .keyboardShortcut(.defaultAction)
            .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
    }
}
