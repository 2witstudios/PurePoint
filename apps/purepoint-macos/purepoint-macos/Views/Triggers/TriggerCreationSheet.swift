import SwiftUI

struct TriggerCreationSheet: View {
    @Bindable var state: TriggersState
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var description = ""
    @State private var event: TriggerEvent = .agentIdle
    @State private var scope = "global"
    @State private var steps: [StepDraft] = [StepDraft()]
    @State private var selectedProjectRoot = ""

    var body: some View {
        VStack(spacing: 0) {
            sheetHeader
            Divider()
            formContent
            Divider()
            previewBar
            Divider()
            sheetFooter
        }
        .frame(width: 420)
        .frame(minHeight: 480, maxHeight: 700)
        .onAppear {
            selectedProjectRoot = appState.activeProjectRoot ?? appState.projects.first?.projectRoot ?? ""
        }
    }

    // MARK: - Header

    private var sheetHeader: some View {
        HStack {
            Text("New Trigger")
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

            TextField("Description (optional)", text: $description)
                .textFieldStyle(.roundedBorder)

            Picker("Event", selection: $event) {
                ForEach(TriggerEvent.allCases) { e in
                    Label(e.label, systemImage: e.icon).tag(e)
                }
            }
            .pickerStyle(.segmented)

            Picker("Scope", selection: $scope) {
                Text("Global").tag("global")
                Text("Local").tag("local")
            }

            if scope == "local" {
                Picker("Project", selection: $selectedProjectRoot) {
                    ForEach(appState.projects) { project in
                        Text(project.projectName).tag(project.projectRoot)
                    }
                }
            }

            Section("Sequence") {
                ForEach(Array(steps.enumerated()), id: \.offset) { index, _ in
                    stepEditor(index: index)
                }

                Button {
                    steps.append(StepDraft())
                } label: {
                    Label("Add Step", systemImage: "plus.circle")
                        .font(.system(size: 12))
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
    }

    private func stepEditor(index: Int) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack {
                Text("Step \(index + 1)")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(.secondary)
                Spacer()
                if steps.count > 1 {
                    Button {
                        steps.remove(at: index)
                    } label: {
                        Image(systemName: "minus.circle")
                            .font(.system(size: 11))
                            .foregroundStyle(.red)
                    }
                    .buttonStyle(.plain)
                }
            }

            TextField("Inject text", text: $steps[index].inject)
                .textFieldStyle(.roundedBorder)
                .font(.system(size: 12))

            HStack(spacing: 8) {
                TextField("Gate command (optional)", text: $steps[index].gateRun)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: 12))

                TextField("Exit", text: $steps[index].gateExitCode)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: 12))
                    .frame(width: 40)

                TextField("Retries", text: $steps[index].maxRetries)
                    .textFieldStyle(.roundedBorder)
                    .font(.system(size: 12))
                    .frame(width: 50)
            }
        }
    }

    // MARK: - Preview

    private var previewBar: some View {
        HStack(spacing: 6) {
            Image(systemName: event.icon)
                .font(.system(size: 11))
                .foregroundStyle(event.color)
            Text(previewText)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
            Spacer()
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 10)
    }

    private var previewText: String {
        let stepCount = steps.count
        let stepLabel = stepCount == 1 ? "1 step" : "\(stepCount) steps"
        return "On \(event.label) → \(stepLabel) (\(scope))"
    }

    // MARK: - Footer

    private var sheetFooter: some View {
        HStack {
            Button("Cancel") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)

            Spacer()

            Button("Create") {
                let saveRoot = scope == "local" ? selectedProjectRoot : (appState.projects.first?.projectRoot ?? "")
                Task {
                    await state.saveTrigger(
                        projectRoot: saveRoot,
                        projectRoots: appState.projects.map(\.projectRoot),
                        name: name.isEmpty ? "Untitled Trigger" : name,
                        description: description.isEmpty ? nil : description,
                        on: event.rawValue,
                        sequence: buildSequence(),
                        variables: [:],
                        scope: scope
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

    // MARK: - Helpers

    private func buildSequence() -> [TriggerActionPayload] {
        steps.map { step in
            let gate: GatePayload? =
                step.gateRun.isEmpty
                ? nil
                : GatePayload(
                    run: step.gateRun,
                    expectExit: Int32(step.gateExitCode)
                )
            return TriggerActionPayload(
                inject: step.inject.isEmpty ? nil : step.inject,
                gate: gate,
                maxRetries: UInt32(step.maxRetries)
            )
        }
    }
}

// MARK: - Step Draft

private struct StepDraft {
    var inject = ""
    var gateRun = ""
    var gateExitCode = ""
    var maxRetries = ""
}
