import SwiftUI

struct SwarmCreationSheet: View {
    @Bindable var hubState: AgentsHubState
    let projectRoot: String
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var worktreeCount = 2
    @State private var worktreeTemplate = ""
    @State private var rosterItems: [SwarmRosterItem] = []
    @State private var includeTerminal = true
    @State private var scope: PromptScopeChoice = .project

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
            Text("New Swarm")
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

            Stepper("Worktrees: \(worktreeCount)", value: $worktreeCount, in: 1...8)

            TextField("Worktree template", text: $worktreeTemplate)
                .textFieldStyle(.roundedBorder)

            Section {
                ForEach($rosterItems) { $item in
                    HStack(spacing: 8) {
                        TextField("Agent def", text: $item.agentDef)
                            .textFieldStyle(.roundedBorder)
                            .frame(maxWidth: .infinity)
                        TextField("Role", text: $item.role)
                            .textFieldStyle(.roundedBorder)
                            .frame(maxWidth: .infinity)
                        Stepper("\(item.quantity)", value: $item.quantity, in: 1...8)
                            .frame(width: 80)
                        Button {
                            rosterItems.removeAll { $0.id == item.id }
                        } label: {
                            Image(systemName: "minus.circle.fill")
                                .foregroundStyle(.red)
                        }
                        .buttonStyle(.plain)
                    }
                }
                Button {
                    rosterItems.append(SwarmRosterItem(agentDef: "", role: "", quantity: 1))
                } label: {
                    Label("Add Agent", systemImage: "plus.circle")
                }
                .buttonStyle(.borderless)
            } header: {
                Text("Roster")
            }

            Toggle("Include terminal", isOn: $includeTerminal)
                .toggleStyle(.switch)

            Picker("Scope", selection: $scope) {
                ForEach(PromptScopeChoice.allCases) { s in
                    Text(s.title).tag(s)
                }
            }
            .pickerStyle(.segmented)
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
                let def = SwarmDefinition(
                    name: name.trimmingCharacters(in: .whitespaces),
                    worktreeCount: worktreeCount,
                    worktreeTemplate: worktreeTemplate,
                    roster: rosterItems,
                    includeTerminal: includeTerminal,
                    scope: scope.wireValue
                )
                Task {
                    await hubState.saveSwarmDef(projectRoot: projectRoot, def: def)
                    dismiss()
                }
            }
            .keyboardShortcut(.defaultAction)
            .disabled(
                name.trimmingCharacters(in: .whitespaces).isEmpty
                    || rosterItems.isEmpty
            )
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
    }
}
