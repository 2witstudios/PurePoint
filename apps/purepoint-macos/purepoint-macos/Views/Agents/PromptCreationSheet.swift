import SwiftUI

struct PromptCreationSheet: View {
    @Bindable var hubState: AgentsHubState
    let projectRoot: String
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var description = ""
    @State private var scope: PromptScopeChoice = .project
    @State private var agentType = ""
    @State private var promptBody = ""

    private let agentTypes = ["", "claude", "codex", "opencode"]

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
            Text("New Prompt")
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

            TextField("Description", text: $description)
                .textFieldStyle(.roundedBorder)

            Picker("Scope", selection: $scope) {
                ForEach(PromptScopeChoice.allCases) { s in
                    Text(s.title).tag(s)
                }
            }
            .pickerStyle(.segmented)

            Picker("Agent type", selection: $agentType) {
                ForEach(agentTypes, id: \.self) { t in
                    Text(t.isEmpty ? "Any" : t).tag(t)
                }
            }

            TextEditor(text: $promptBody)
                .font(.system(size: 13, design: .monospaced))
                .frame(minHeight: 140)
                .padding(4)
                .background(Color.primary.opacity(0.035))
                .clipShape(RoundedRectangle(cornerRadius: 8))
        }
        .formStyle(.grouped)
    }

    // MARK: - Footer

    private var sheetFooter: some View {
        HStack {
            Button("Cancel") { dismiss() }
                .keyboardShortcut(.cancelAction)
            Spacer()
            Button("Create") {
                Task {
                    await hubState.saveTemplate(
                        projectRoot: projectRoot,
                        name: name.trimmingCharacters(in: .whitespaces),
                        description: description,
                        agent: agentType,
                        body: promptBody,
                        scope: scope.wireValue
                    )
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
