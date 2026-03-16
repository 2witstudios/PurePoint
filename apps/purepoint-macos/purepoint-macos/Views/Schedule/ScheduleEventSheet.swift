import SwiftUI

struct ScheduleEventSheet: View {
    @Bindable var state: ScheduleState
    let editingEvent: ScheduleEvent?
    @Environment(AppState.self) private var appState
    @Environment(\.dismiss) private var dismiss

    @State private var name = ""
    @State private var type: ScheduleType = .agent
    @State private var date = Date()
    @State private var recurrence: RecurrenceRule = .none
    @State private var target = ""
    @State private var selectedColorIndex: Int?
    @State private var showDeleteConfirm = false
    @State private var scope = "global"
    @State private var selectedProjectRoot = ""

    // Trigger fields
    @State private var triggerName = ""
    @State private var triggerPrompt = ""
    @State private var triggerAgent = "claude"

    private var isEditing: Bool { editingEvent != nil }

    private var projectRoots: [String] {
        appState.projects.map(\.projectRoot)
    }

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
        .frame(width: 400, height: 560)
        .onAppear { prefillFields() }
        .alert("Delete Schedule", isPresented: $showDeleteConfirm) {
            Button("Delete", role: .destructive) {
                guard let event = editingEvent else { return }
                let deleteRoot = event.projectRoot ?? appState.projects.first?.projectRoot ?? ""
                Task {
                    EventColor.remove(forEvent: event.name)
                    await state.deleteSchedule(
                        projectRoot: deleteRoot,
                        projectRoots: projectRoots,
                        name: event.name,
                        scope: event.scope
                    )
                    dismiss()
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This schedule will be permanently deleted.")
        }
    }

    // MARK: - Prefill

    private func prefillFields() {
        selectedProjectRoot = appState.activeProjectRoot ?? appState.projects.first?.projectRoot ?? ""

        if let event = editingEvent {
            name = event.name
            type = event.type
            date = event.date
            recurrence = event.recurrence
            target = event.target
            selectedColorIndex = EventColor.savedIndex(forEvent: event.name)
            scope = event.scope
            if let root = event.projectRoot {
                selectedProjectRoot = root
            }

            // Prefill trigger fields
            if let trigger = event.trigger {
                switch trigger {
                case .agentDef(let defName):
                    triggerName = defName
                case .swarmDef(let defName, _):
                    triggerName = defName
                case .inlinePrompt(let prompt, let agent):
                    triggerPrompt = prompt
                    triggerAgent = agent
                }
            }
        } else if let prefill = state.creationPrefillDate {
            date = prefill
        }
    }

    // MARK: - Header

    private var sheetHeader: some View {
        HStack {
            Text(isEditing ? "Edit Schedule" : "New Schedule")
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

            colorPicker
        }
        .formStyle(.grouped)
        .scrollDisabled(true)
    }

    // MARK: - Color Picker

    private var colorPicker: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Color")
                .font(.system(size: 12))
                .foregroundStyle(.secondary)

            HStack(spacing: 8) {
                ForEach(Array(EventColor.palette.enumerated()), id: \.offset) { index, color in
                    Circle()
                        .fill(color)
                        .frame(width: 20, height: 20)
                        .overlay(
                            Circle()
                                .strokeBorder(.white, lineWidth: 2)
                                .opacity(selectedColorIndex == index ? 1 : 0)
                        )
                        .overlay(
                            Circle()
                                .strokeBorder(color.opacity(0.5), lineWidth: 1)
                                .opacity(selectedColorIndex == index ? 0 : 1)
                        )
                        .onTapGesture {
                            selectedColorIndex = selectedColorIndex == index ? nil : index
                        }
                }
            }
        }
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
            if isEditing {
                Button("Delete") {
                    showDeleteConfirm = true
                }
                .foregroundStyle(.red)
            }

            Button("Cancel") {
                dismiss()
            }
            .keyboardShortcut(.cancelAction)

            Spacer()

            Button(isEditing ? "Save" : "Create") {
                Task { await saveAndDismiss() }
            }
            .keyboardShortcut(.defaultAction)
            .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
    }

    private func saveAndDismiss() async {
        let effectiveName = name.isEmpty ? "Untitled Schedule" : name
        let saveRoot = scope == "local" ? selectedProjectRoot : (appState.projects.first?.projectRoot ?? "")

        // If editing and name changed, delete old entry first (name is the backend key)
        if let original = editingEvent, original.name != effectiveName {
            let origRoot = original.projectRoot ?? saveRoot
            await state.deleteSchedule(
                projectRoot: origRoot,
                projectRoots: projectRoots,
                name: original.name,
                scope: original.scope
            )
            EventColor.remove(forEvent: original.name)
        }

        // Save color preference
        if let index = selectedColorIndex {
            EventColor.save(index: index, forEvent: effectiveName)
        } else if let original = editingEvent {
            // Clear color override if deselected
            EventColor.remove(forEvent: original.name)
            if original.name != effectiveName {
                EventColor.remove(forEvent: effectiveName)
            }
        }

        let trigger = buildTrigger()
        await state.saveSchedule(
            projectRoot: saveRoot,
            projectRoots: projectRoots,
            name: effectiveName,
            enabled: editingEvent?.enabled ?? true,
            recurrence: recurrence,
            startAt: date,
            trigger: trigger,
            target: target,
            scope: scope
        )
        dismiss()
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
