import SwiftUI

struct ScheduleCreationSheet: View {
    @Bindable var state: ScheduleState
    let projectRoot: String
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var type: ScheduleType = .agent
    @State private var date = Date()
    @State private var recurrence: RecurrenceRule = .none
    @State private var target = ""

    // Trigger fields
    @State private var triggerName = ""
    @State private var triggerPrompt = ""
    @State private var triggerAgent = "claude"

    private static let previewFormatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = .autoupdatingCurrent
        f.dateFormat = "EEEE 'at' h:mm a"
        return f
    }()

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
        .frame(width: 400, height: 520)
        .onAppear {
            if let prefill = state.creationPrefillDate {
                date = prefill
            }
        }
    }

    // MARK: - Header

    private var sheetHeader: some View {
        HStack {
            Text("New Schedule")
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

            Picker("Trigger", selection: $type) {
                ForEach(ScheduleType.allCases) { t in
                    Label(t.label, systemImage: t.icon).tag(t)
                }
            }
            .pickerStyle(.segmented)

            // Trigger-specific fields
            switch type {
            case .agent:
                TextField("Agent Definition Name", text: $triggerName)
                    .textFieldStyle(.roundedBorder)
            case .swarm:
                TextField("Swarm Definition Name", text: $triggerName)
                    .textFieldStyle(.roundedBorder)
            case .inlinePrompt:
                TextField("Prompt", text: $triggerPrompt)
                    .textFieldStyle(.roundedBorder)
                TextField("Agent Type", text: $triggerAgent)
                    .textFieldStyle(.roundedBorder)
            }

            DatePicker("Time", selection: $date)

            Picker("Repeats", selection: $recurrence) {
                ForEach(RecurrenceRule.allCases) { rule in
                    Text(rule.rawValue).tag(rule)
                }
            }

            TextField("Target (path or scope)", text: $target)
                .textFieldStyle(.roundedBorder)
        }
        .formStyle(.grouped)
        .scrollDisabled(true)
    }

    // MARK: - Preview

    private var previewBar: some View {
        HStack(spacing: 6) {
            Image(systemName: "clock")
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
            Text(nextRunPreview)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
            Spacer()
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 10)
    }

    private var nextRunPreview: String {
        let calendar = Calendar.current
        if recurrence == .none {
            return "Runs once: \(Self.previewFormatter.string(from: date))"
        }
        if calendar.isDateInToday(date) {
            return "Next run: Today at \(timeString(date))"
        }
        if calendar.isDateInTomorrow(date) {
            return "Next run: Tomorrow at \(timeString(date))"
        }
        return "Next run: \(Self.previewFormatter.string(from: date))"
    }

    private static let timeFormatter: DateFormatter = {
        let f = DateFormatter()
        f.locale = .autoupdatingCurrent
        f.dateFormat = "h:mm a"
        return f
    }()

    private func timeString(_ date: Date) -> String {
        Self.timeFormatter.string(from: date)
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
                Task {
                    let trigger = buildTrigger()
                    await state.saveSchedule(
                        projectRoot: projectRoot,
                        name: name.isEmpty ? "Untitled Schedule" : name,
                        enabled: true,
                        recurrence: recurrence,
                        startAt: date,
                        trigger: trigger,
                        target: target,
                        scope: "local"
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

    private func buildTrigger() -> ScheduleTriggerPayload {
        switch type {
        case .agent:
            return .agentDef(name: triggerName)
        case .swarm:
            return .swarmDef(name: triggerName, vars: [:])
        case .inlinePrompt:
            return .inlinePrompt(prompt: triggerPrompt, agent: triggerAgent)
        }
    }
}
