import SwiftUI

struct SettingsHotkeysView: View {
    @Environment(KeyBindingState.self) private var keyBindingState

    var body: some View {
        @Bindable var keyBindingState = keyBindingState

        VStack(alignment: .leading, spacing: 24) {
            HStack {
                Text("Hotkeys")
                    .font(.system(size: 18, weight: .semibold))
                Spacer()
                Button("Reset All") {
                    keyBindingState.resetAllBindings()
                }
                .buttonStyle(.plain)
                .font(.system(size: 12))
                .foregroundStyle(.secondary)
                .disabled(!HotkeyAction.allCases.contains(where: { keyBindingState.isCustomized($0) }))
            }

            ForEach(HotkeyCategory.allCases) { category in
                GroupBox(category.displayName) {
                    let actions = HotkeyAction.actions(for: category)
                    ForEach(Array(actions.enumerated()), id: \.element) { index, action in
                        if index > 0 { Divider() }
                        EditableHotkeyRow(action: action)
                    }
                }
                .groupBoxStyle(SettingsGroupBoxStyle())
            }
        }
    }
}

// MARK: - EditableHotkeyRow

private struct EditableHotkeyRow: View {
    let action: HotkeyAction
    @Environment(KeyBindingState.self) private var keyBindingState
    @State private var isRecording = false
    @State private var conflictAction: HotkeyAction?
    @State private var pendingBinding: KeyBinding?
    @State private var recordingMonitor: Any?

    var body: some View {
        HStack {
            Text(action.displayName)
                .font(.system(size: 13))
            Spacer()

            if isRecording {
                recordingView
            } else {
                keyCapsView
            }
        }
        .padding(.vertical, 6)
        .contentShape(Rectangle())
        .onTapGesture {
            startRecording()
        }
    }

    @ViewBuilder
    private var keyCapsView: some View {
        let binding = keyBindingState.binding(for: action)
        HStack(spacing: 4) {
            if keyBindingState.isCustomized(action) {
                Button {
                    keyBindingState.resetBinding(for: action)
                } label: {
                    Image(systemName: "arrow.counterclockwise")
                        .font(.system(size: 10))
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .help("Reset to default")
            }

            ForEach(binding.displayKeys, id: \.self) { key in
                Text(key)
                    .font(.system(size: 12, design: .monospaced))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.secondary.opacity(0.1))
                    .clipShape(RoundedRectangle(cornerRadius: 4))
            }
        }
    }

    @ViewBuilder
    private var recordingView: some View {
        HStack(spacing: 8) {
            if let conflict = conflictAction {
                VStack(alignment: .trailing, spacing: 2) {
                    Text("Already used by \(conflict.displayName)")
                        .font(.system(size: 11))
                        .foregroundStyle(.orange)
                    HStack(spacing: 8) {
                        Button("Override") {
                            if let binding = pendingBinding {
                                keyBindingState.resetBinding(for: conflict)
                                keyBindingState.setBinding(binding, for: action)
                            }
                            stopRecording()
                        }
                        .font(.system(size: 11, weight: .medium))

                        Button("Cancel") {
                            stopRecording()
                        }
                        .font(.system(size: 11))
                    }
                }
            } else {
                Text("Press shortcut\u{2026}")
                    .font(.system(size: 12))
                    .foregroundStyle(.secondary)
                    .opacity(pulsingOpacity)

                Button("Cancel") {
                    stopRecording()
                }
                .buttonStyle(.plain)
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
            }
        }
    }

    @State private var pulsePhase = false
    private var pulsingOpacity: Double {
        pulsePhase ? 1.0 : 0.4
    }

    private func startRecording() {
        guard !isRecording else { return }
        isRecording = true
        conflictAction = nil
        pendingBinding = nil
        pulsePhase = true
        keyBindingState.recordingAction = action

        withAnimation(.easeInOut(duration: 0.8).repeatForever(autoreverses: true)) {
            pulsePhase.toggle()
        }

        recordingMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [self] event in
            if event.keyCode == SpecialKey.escape.keyCode && event.modifierFlags.intersection([.command, .shift, .option, .control]).isEmpty {
                stopRecording()
                return nil
            }

            guard let binding = KeyBinding.from(event: event) else {
                return nil // Ignore bare keys without modifiers
            }

            if let conflict = keyBindingState.detectConflict(binding, excluding: action) {
                pendingBinding = binding
                conflictAction = conflict
            } else {
                keyBindingState.setBinding(binding, for: action)
                stopRecording()
            }
            return nil
        }
    }

    private func stopRecording() {
        isRecording = false
        conflictAction = nil
        pendingBinding = nil
        keyBindingState.recordingAction = nil

        if let monitor = recordingMonitor {
            NSEvent.removeMonitor(monitor)
            recordingMonitor = nil
        }
    }
}
