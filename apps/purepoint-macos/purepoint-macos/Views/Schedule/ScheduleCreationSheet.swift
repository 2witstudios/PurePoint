import SwiftUI

struct ScheduleCreationSheet: View {
    @Bindable var state: ScheduleState
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var type: ScheduleType = .swarm
    @State private var date = Date()
    @State private var recurrence: RecurrenceRule = .none
    @State private var projectName = "purepoint"
    @State private var target = ""

    private let mockProjects = ["purepoint", "acme-app", "data-pipeline", "analytics", "docs-site"]

    private static let previewFormatter: DateFormatter = {
        let f = DateFormatter()
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
        .frame(width: 400, height: 480)
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

            Picker("Type", selection: $type) {
                ForEach(ScheduleType.allCases) { t in
                    Label(t.label, systemImage: t.icon).tag(t)
                }
            }
            .pickerStyle(.segmented)

            DatePicker("Time", selection: $date)

            Picker("Repeats", selection: $recurrence) {
                ForEach(RecurrenceRule.allCases) { rule in
                    Text(rule.rawValue).tag(rule)
                }
            }

            Picker("Project", selection: $projectName) {
                ForEach(mockProjects, id: \.self) { project in
                    Text(project).tag(project)
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

    private func timeString(_ date: Date) -> String {
        let f = DateFormatter()
        f.dateFormat = "h:mm a"
        return f.string(from: date)
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
                let event = ScheduleEvent(
                    name: name.isEmpty ? "Untitled Schedule" : name,
                    type: type,
                    date: date,
                    recurrence: recurrence,
                    projectName: projectName,
                    target: target
                )
                state.addEvent(event)
                dismiss()
            }
            .keyboardShortcut(.defaultAction)
            .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
    }
}
