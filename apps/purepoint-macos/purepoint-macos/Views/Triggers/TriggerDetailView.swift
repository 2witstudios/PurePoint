import SwiftUI

struct TriggerDetailView: View {
    let trigger: TriggerItem
    @Bindable var state: TriggersState
    let projectRoot: String

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                header
                Divider()
                sequenceSection
                if !trigger.variables.isEmpty {
                    Divider()
                    variablesSection
                }
                Divider()
                deleteSection
            }
            .padding(20)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }

    // MARK: - Header

    private var header: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 8) {
                Image(systemName: trigger.event.icon)
                    .font(.system(size: 16))
                    .foregroundStyle(trigger.event.color)

                Text(trigger.name)
                    .font(.system(size: 16, weight: .semibold))

                Text(trigger.scope)
                    .font(.system(size: 10, weight: .semibold))
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(.secondary.opacity(0.15))
                    .clipShape(RoundedRectangle(cornerRadius: 4))
            }

            if let desc = trigger.description, !desc.isEmpty {
                Text(desc)
                    .font(.system(size: 13))
                    .foregroundStyle(.secondary)
            }

            HStack(spacing: 6) {
                Image(systemName: "bolt")
                    .font(.system(size: 11))
                    .foregroundStyle(.tertiary)
                Text("Fires on \(trigger.event.label)")
                    .font(.system(size: 12))
                    .foregroundStyle(.tertiary)
            }
        }
    }

    // MARK: - Sequence

    private var sequenceSection: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Sequence")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.secondary)

            ForEach(Array(trigger.sequence.enumerated()), id: \.offset) { index, action in
                stepRow(index: index, action: action)
            }
        }
    }

    private func stepRow(index: Int, action: TriggerActionPayload) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Text("\(index + 1)")
                .font(.system(size: 11, weight: .bold, design: .monospaced))
                .foregroundStyle(.secondary)
                .frame(width: 20, alignment: .trailing)

            VStack(alignment: .leading, spacing: 4) {
                if let inject = action.inject {
                    HStack(spacing: 4) {
                        Image(systemName: "text.cursor")
                            .font(.system(size: 10))
                            .foregroundStyle(.blue)
                        Text(inject)
                            .font(.system(size: 12, design: .monospaced))
                    }
                }

                if let gate = action.gate {
                    HStack(spacing: 4) {
                        Image(systemName: "shield.checkered")
                            .font(.system(size: 10))
                            .foregroundStyle(.orange)
                        Text(gate.run)
                            .font(.system(size: 12, design: .monospaced))
                        if let exitCode = gate.expectExit {
                            Text("exit=\(exitCode)")
                                .font(.system(size: 10))
                                .foregroundStyle(.tertiary)
                        }
                    }
                }

                if let retries = action.maxRetries {
                    Text("max retries: \(retries)")
                        .font(.system(size: 10))
                        .foregroundStyle(.tertiary)
                }
            }
        }
        .padding(.vertical, 4)
        .padding(.horizontal, 8)
        .background(Color.secondary.opacity(0.05))
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    // MARK: - Variables

    private var variablesSection: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Variables")
                .font(.system(size: 12, weight: .semibold))
                .foregroundStyle(.secondary)

            ForEach(Array(trigger.variables.sorted(by: { $0.key < $1.key })), id: \.key) { key, value in
                HStack(spacing: 8) {
                    Text(key)
                        .font(.system(size: 12, weight: .medium, design: .monospaced))
                    Text("=")
                        .foregroundStyle(.tertiary)
                    Text(value)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundStyle(.secondary)
                }
            }
        }
    }

    // MARK: - Delete

    private var deleteSection: some View {
        HStack {
            Spacer()
            Button("Delete Trigger", role: .destructive) {
                Task {
                    await state.deleteTrigger(projectRoot: projectRoot, name: trigger.name, scope: trigger.scope)
                }
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
        }
    }
}
